//! 编辑桥：笔刷笔触 → `EditCommand` → `wv-edit::History`（S5 / S9）。
//!
//! ## 职责
//! - 收集 [`wv_render::camera::ActiveStroke`] 的点列，在松手（左键释放）时把
//!   `wv_terrain::edit::Stroke` 包成 `wv_edit::PaintTerrainCommand` 提交给 `History`。
//! - 处理 Undo/Redo：`Ctrl+Z` / `Ctrl+Shift+Z` 由 `ui_shell::hotkeys` 收键，
//!   经 `CommandPalette` 派发到本模块的 `apply_history_command`。
//! - 播放中一律拒绝编辑（基线 §1.8 / V37）：时间游标「当前世界」是投影，不能原地改。
//!
//! ## 边界（不做什么）
//! - **不自己听键盘**——所有键一律经 `ui_shell::hotkeys` → `CommandPalette::pending`
//!   → 本模块执行，避免两个系统重复处理同一组键。
//! - **不直接修改 `WorldProjection.world`**——只能走 `wv-edit` 的 `History`（§3.3）。
//! - **不在播放中接受编辑**——门禁在 `apply_history_command` / `apply_brush`。
//!
//! ## 主要导出
//! - `apply_brush` / `apply_history_command` / `push_history` — 编辑命令入口。
//! - 资源 `History` / `EditMode` 由 `state.rs` 暴露；本模块只读 / 提交。
//! - 测试 `mod tests` 覆盖 painting / undo / 播放拒绝。

use bevy::prelude::*;

use crate::state::{
    BrushState, BrushTool, ContinentGenerationStash, ContinentGenerationState, DrillState,
    GridConfig, PlaybackState, SelectionState, ViewState, WorldProjection,
};
use wv_edit::command::{
    EditCommand, GenerateContinentCommand, IntentTerrainCommand, PaintTerrainCommand,
    ResetTerrainToBaseCommand,
};
use wv_edit::history::Transaction;
use wv_edit::History;
use wv_render::camera::ActiveStroke;
use wv_render::terrain_view::{image_from_terrain, TerrainTexture, TileAtlas};
use wv_terrain::continent::ContinentParams;
use wv_terrain::edit::{Brush, Stroke as TerrainStroke};
use wv_terrain::intent::Intent;

/// 编辑历史（`wv-edit::History` 的应用层持有者）。
///
/// `dirty_set` 累计自上次 `take_dirty` / `clear_dirty` 以来**所有**已提交命令
/// 返回的相对目录路径（如 `maps` / `distances/edges` / `geometry/faction`）。
/// 阶段七 v0.2：Save 命令用这个集合把 `save_world(..., &dirty)` 喂进去，
/// 避免每次保存都全目录重写。
///
/// 持久化模型：
/// - 编辑（commit/redo）→ `record_dirty` 累加
/// - 加载新工程（`load_project_into`）→ `clear_dirty` 重置
/// - 保存成功 → `take_dirty` 排空
/// - 撤销（undo）→ 当前版本暂**不**回滚 dirty 集合（接受一定过度写盘换取
///   实现简单性；保存仍是幂等的 round-trip）
#[derive(Resource, Default)]
pub struct EditHistory {
    pub history: History,
    pub dirty_set: std::collections::BTreeSet<String>,
}

impl EditHistory {
    /// 把 `paths` 全部并入 dirty 集合。
    pub fn record_dirty<I, S>(&mut self, paths: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for p in paths {
            self.dirty_set.insert(p.into());
        }
    }

    /// 取出 dirty 集合并清空（原子）。
    pub fn take_dirty(&mut self) -> Vec<String> {
        let v: Vec<String> = self.dirty_set.iter().cloned().collect();
        self.dirty_set.clear();
        v
    }

    /// 不取，只清空（用于「加载新工程」之类的整体重置）。
    pub fn clear_dirty(&mut self) {
        self.dirty_set.clear();
    }
}

/// 松手时把笔触转成命令并提交。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道。
#[allow(clippy::too_many_arguments)]
pub fn commit_stroke_on_release(
    buttons: Res<ButtonInput<MouseButton>>,
    mut active: ResMut<ActiveStroke>,
    grid: Res<GridConfig>,
    brush: Res<BrushState>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    drill: Res<DrillState>,
    mut projection: ResMut<WorldProjection>,
    mut tex: ResMut<TerrainTexture>,
    mut edit: ResMut<EditHistory>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
) {
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    if active.points.is_empty() {
        return;
    }
    // 播放中只读（基线 §1.8 / V37）。
    if playback.playing {
        active.points.clear();
        return;
    }
    // 极低缩放禁画地形（基线 §1.16 L2：0.05×/0.15× 级）。
    if view.scale < 0.15 {
        active.points.clear();
        return;
    }

    // 世界坐标点列 → 图内局部坐标（反转 GridConfig 的 origin 偏移）。
    // 两种工具共用同一份点列：Paint 拿它当 Brush 序列，意图工具拿它当折线 / 首点。
    let pts_local: Vec<(f32, f32)> = active
        .points
        .drain(..)
        .map(|p| (p.x - grid.raster_origin.x, p.y - grid.raster_origin.y))
        .collect();

    // 笔刷应落在 `drill.current()`（单一真源），而不是 `view.current_map` 镜像——
    // 镜像在 `NewProject`/`CloseProject`/`adopt` 之后会失同步一帧（2026-10-14
    // 用户反馈"新建工程后笔刷不生效"即此因）。`drill.current()=None` 时直接
    // 拒绝：世界上没图可画，作者应该先用「新建地图…」建一张。
    let Some(target_map) = drill.current().cloned() else {
        bevy::log::warn!("笔刷无目标图（drill.current=None）：请先新建一张地图");
        notice.error(
            "笔刷未生效：当前没有地图（顶栏工程菜单 → 新建地图…）",
            (time.elapsed_secs() * 60.0) as u64,
        );
        return;
    };

    // 按工具分派：意图工具 → IntentTerrainCommand；Paint → PaintTerrainCommand。
    let (cmd, dirty_rect) = if brush.tool.is_intent() {
        let Some(intent) = intent_from_stroke(brush.tool, &pts_local, brush.radius, brush.strength)
        else {
            return;
        };
        let dirty_rect = intent_bbox_in_cells(&pts_local, brush.radius, &grid);
        (
            Box::new(IntentTerrainCommand::new(
                target_map,
                intent,
                grid.cell_size,
            )) as Box<dyn EditCommand>,
            dirty_rect,
        )
    } else {
        let mut stroke = TerrainStroke::new();
        for (x, y) in &pts_local {
            stroke.push(Brush {
                center: (*x, *y),
                radius: brush.radius,
                softness: brush.softness,
                kind: Some(brush.kind),
                height_delta: 0.0,
            });
        }
        // 笔触世界包围盒（在构造 `cmd` 之前算——`stroke` move 给 cmd 后不可用）。
        let dirty_rect = stroke_bbox_in_cells(&stroke, &grid);
        (
            Box::new(PaintTerrainCommand::new(target_map, stroke, grid.cell_size))
                as Box<dyn EditCommand>,
            dirty_rect,
        )
    };

    // 提交到 History（一次笔触 = 一个事务 = 一次可撤销单位）。
    let mut tx = Transaction::new();
    tx.push(cmd);
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // 增量重传（优化项 3d）：只标笔触影响矩形为脏，
            // `flush_terrain_texture` 会走 [`wv_render::terrain_view::patch_terrain_image`] 增量 patch。
            tex.mark_dirty(dirty_rect);
            // 地形 cells 改了 → 派生诊断（事件位移 / 相遇）需要重算。
            diagnostics.needs_diagnose = true;
        }
        Err(e) => {
            // 失败必须给人话回执（§5.3）：最常见的是在**没有自己栅格的图**上
            // 落笔（子图继承父图底图时，`terrain_of` 返回 None）。只写日志的
            // 旧做法在用户眼里就是「笔刷坏了」——状态栏要说清楚原因与下一步。
            bevy::log::warn!("编辑命令失败: {e}");
            let frame = (time.elapsed_secs() * 60.0) as u64;
            notice.error(format!("笔刷未生效：{e}"), frame);
        }
    }
}

/// 把 `stroke` 的所有 `Brush.center + radius` 转成**逻辑格**坐标的 AABB
/// （半开 `[gx0,gy0,gx1,gy1]`）。钳到 `[0, ∞)`（具体上限由 `mark_dirty` 与 `size` 并集兜底）。
fn stroke_bbox_in_cells(stroke: &TerrainStroke, grid: &GridConfig) -> [u32; 4] {
    if stroke.brushes.is_empty() {
        return [0, 0, 0, 0];
    }
    let cs = grid.cell_size.max(f32::EPSILON);
    let mut gx0 = i32::MAX;
    let mut gy0 = i32::MAX;
    let mut gx1 = i32::MIN;
    let mut gy1 = i32::MIN;
    for b in &stroke.brushes {
        let cx = (b.center.0 / cs) as i32;
        let cy = (b.center.1 / cs) as i32;
        let pad = (b.radius / cs).ceil() as i32 + 1;
        gx0 = gx0.min(cx - pad);
        gy0 = gy0.min(cy - pad);
        gx1 = gx1.max(cx + pad);
        gy1 = gy1.max(cy + pad);
    }
    [
        gx0.max(0) as u32,
        gy0.max(0) as u32,
        gx1.max(0) as u32,
        gy1.max(0) as u32,
    ]
}

/// 把一次拖拽点列 + 笔刷参数组装成地形意图（意图笔刷 plan v0.1）。
///
/// 点状意图（火山 / 湖）只取首点——「点一下」就是一条单点 ActiveStroke。
/// `PaintKind` 返回 `None`（调用方不应走到这，防御性兜底）。
fn intent_from_stroke(
    tool: BrushTool,
    pts: &[(f32, f32)],
    radius: f32,
    strength: f32,
) -> Option<Intent> {
    let first = pts.first().copied()?;
    Some(match tool {
        BrushTool::MountainRange => Intent::MountainRange {
            pts: pts.to_vec(),
            radius,
            strength,
        },
        BrushTool::Plateau => Intent::Plateau {
            pts: pts.to_vec(),
            radius,
            strength,
        },
        BrushTool::Hills => Intent::Hills {
            pts: pts.to_vec(),
            radius,
            strength,
        },
        BrushTool::Volcano => Intent::Volcano {
            center: first,
            radius,
            strength,
        },
        BrushTool::Lake => Intent::Lake {
            center: first,
            radius,
            strength,
        },
        BrushTool::River => Intent::River {
            pts: pts.to_vec(),
            strength,
        },
        BrushTool::PaintKind => return None,
    })
}

/// 意图影响包围盒（逻辑格，半开）。统一按 `2.6 × radius` 外扩——取全部意图的
/// 最大 reach（火山 2.6r、山脉 2.0r + 域扭曲 0.45r），脏矩形宁可大不能小：
/// 漏标的格会在纹理上残留旧像素。`radius` 无关的工具（河）传 0 即可只包点列。
fn intent_bbox_in_cells(pts: &[(f32, f32)], radius: f32, grid: &GridConfig) -> [u32; 4] {
    if pts.is_empty() {
        return [0, 0, 0, 0];
    }
    let cs = grid.cell_size.max(f32::EPSILON);
    let pad = (radius * 2.6 / cs).ceil() as i32 + 2;
    let mut gx0 = i32::MAX;
    let mut gy0 = i32::MAX;
    let mut gx1 = i32::MIN;
    let mut gy1 = i32::MIN;
    for &(px, py) in pts {
        let cx = (px / cs) as i32;
        let cy = (py / cs) as i32;
        gx0 = gx0.min(cx - pad);
        gy0 = gy0.min(cy - pad);
        gx1 = gx1.max(cx + pad);
        gy1 = gy1.max(cy + pad);
    }
    [
        gx0.max(0) as u32,
        gy0.max(0) as u32,
        gx1.max(0) as u32,
        gy1.max(0) as u32,
    ]
}

/// 把脏标记变为实际纹理更新（增量重传的出口）。
///
/// **优化项 3d**：
/// - 模式切换（`rebuild_texture_on_mode_change`）→ `tex.dirty = Some([0, 0, size, size])`
///   仍走 [`image_from_terrain`] 全图重建；
/// - 笔刷编辑 → `tex.dirty = Some(笔触包围盒)` 走 [`wv_render::terrain_view::patch_terrain_image`]
///   原地 patch 像素（**关键性能提升**：512² 整图重算 ≈ 80 ms → 几十格 patch ≈ <1 ms）。
///
/// **为何不能直接靠修改 `image.data` 触发 GPU upload**：bevy 0.15 的 `Image` 资源
/// 检测的是「资产添加/移除」，不是 `data` 字段写入——必须走 `Assets::get_mut` 拿到
/// `&mut Image` 后 `patch_terrain_image` 原地改 `data`（已是 `&mut`），bevy 在下一帧
/// 检测到 asset 变更时会上传。
pub fn flush_terrain_texture(
    mut tex: ResMut<TerrainTexture>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(rect) = tex.dirty else { return };
    // Phase 2：取**纹理正在显示的那张图**的地形（当前图或它向上继承到的祖先图）。
    let Some(data) =
        wv_render::terrain_view::texture_terrain(&tex, &projection.world, drill.current())
    else {
        tex.dirty = None;
        return;
    };
    let w = data.width.max(1);
    let h = data.height.max(1);
    // dirty 矩形 = 整图 → 走全图重建（模式切换 / 全图标记脏的场景）。
    let is_full = rect == [0, 0, w, h];
    if is_full {
        let image = image_from_terrain(
            data,
            view.mode,
            atlas.as_deref(),
            view.terrain_soften,
            crate::theme::palette::colors(view.map_palette),
            wv_render::theme::theme_pack::builtin::for_palette(view.map_palette).treatment,
        );
        if let Some(mut img) = images.get_mut(&tex.handle) {
            *img = image;
        }
        tex.dirty = None;
        return;
    }
    // 增量 patch：只重算 dirty 矩形内的 cell，原地写进 `image.data`。
    let res = tex.res;
    if let Some(mut img) = images.get_mut(&tex.handle) {
        wv_render::terrain_view::patch_terrain_image(
            &mut img,
            data,
            view.mode,
            atlas.as_deref(),
            rect,
            res,
            view.terrain_soften,
            crate::theme::palette::colors(view.map_palette),
            wv_render::theme::theme_pack::builtin::for_palette(view.map_palette).treatment,
        );
    }
    tex.dirty = None;
}

/// 逐格比较撤销/重做前后的地形 cells，返回变化格的包围盒（逻辑格，半开）。
///
/// 调用方保证两段 slices 等长（尺寸不一致由调用方退化为 `mark_all_dirty`）。
/// 无变化时返回 `None`。
fn terrain_diff_bbox(
    before: &[wv_core::terrain::TerrainCellData],
    after: &[wv_core::terrain::TerrainCellData],
    w: u32,
) -> Option<[u32; 4]> {
    let mut x0 = u32::MAX;
    let mut y0 = u32::MAX;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    let mut any = false;
    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        if b == a {
            continue;
        }
        any = true;
        let cx = (i as u32) % w;
        let cy = (i as u32) / w;
        x0 = x0.min(cx);
        y0 = y0.min(cy);
        x1 = x1.max(cx + 1);
        y1 = y1.max(cy + 1);
    }
    any.then_some([x0, y0, x1, y1])
}

/// 重置当前图的地形到底图（2026-10-16 用户需求）。
///
/// 走和「撤销/重做」同一套纹理脏追踪（前后 cells diff → 矩形标脏；
/// 尺寸变了走 `mark_all_dirty`），让笔刷面板上的「重置到底图」按钮
/// 不需要绕过 dispatcher。
///
/// 播放中只读（基线 V37）：拒绝重置并说明原因，而不是静默无反应。
/// 无工程锁由 [`crate::ui_shell`] 提前把 `CommandId::ResetTerrainToBase`
/// 挡掉，本函数不再重复。
pub fn apply_reset_terrain_to_base(
    map_id: &wv_core::Id,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    playback: &PlaybackState,
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再重置".into());
    }

    // 与 apply_history_command 同款：前后 cells diff → 矩形标脏
    // （避免 mark_all_dirty 的整图重建）。
    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(map_id))
        .map(|t| t.cells.clone());

    let mut tx = Transaction::new();
    tx.push(Box::new(ResetTerrainToBaseCommand::new(map_id.clone())));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;

            let after =
                wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(map_id))
                    .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                _ => tex.mark_all_dirty(),
            }
            Ok("已重置到底图".into())
        }
        Err(e) => Err(format!("重置失败：{e}")),
    }
}

/// 提交一次大陆生成（宏命令），成功后「生成即消费」选区并写恢复圈 stash。
///
/// 调用方只克隆环、不预消费选区——失败时选区原样保留，作者画了半天的圈
/// 不能因为一次提交失败就消失（V1 精神：只提示，不替你决定）。
///
/// 纹理脏矩形与 [`apply_reset_terrain_to_base`] 同款：前后 cells diff 出
/// 包围盒，避免整图重建。
#[allow(clippy::too_many_arguments)]
pub fn commit_continent_generation(
    map: wv_core::Id,
    envelopes: Vec<Vec<(f32, f32)>>,
    params: ContinentParams,
    seed: u64,
    cell_size: f32,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    selection: &mut SelectionState,
    gen: &mut ContinentGenerationState,
) -> Result<(), String> {
    if envelopes.is_empty() {
        return Err("没有圈出大陆范围：先用「选择」工具圈一块闭合区域".into());
    }

    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(&map))
        .map(|t| t.cells.clone());
    let mut tx = Transaction::new();
    tx.push(Box::new(GenerateContinentCommand::new(
        map.clone(),
        envelopes.clone(),
        params,
        seed,
        cell_size,
    )));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            let after =
                wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(&map))
                    .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                _ => tex.mark_all_dirty(),
            }
            // 生成即消费选区 + 记录恢复圈（spec §4：撤销回驻留、单槽位让位）。
            selection.cancel();
            gen.stash = Some(ContinentGenerationStash {
                envelopes,
                params,
                seed,
                tx_seq: edit.history.undo_top_seq().unwrap_or(0),
            });
            Ok(())
        }
        Err(e) => {
            // 调用方在提交前只克隆环、不预消费选区——失败即无事发生。
            Err(format!("生成失败：{e}"))
        }
    }
}

/// 撤销 / 重做成功后的**选区**恢复圈判定（纯逻辑，便于单测；spec §4 裁定）。
///
/// 选区是单槽位，与旧多包络 `ContinentSketch` 不同：
///
/// - undo 且 redo 栈顶正是生成事务：槽位**空闲**（无闭合、无草稿）才回驻留；
///   作者已画新选区则新选区赢、stash 丢弃；
/// - redo 且 undo 栈顶正是生成事务：若当前选区正是被消费的那条 → 再消费；
///   作者的新选区不动。
pub fn sync_selection_after_history_step(
    redo: bool,
    history: &History,
    selection: &mut SelectionState,
    gen: &mut ContinentGenerationState,
) {
    let (stash_ring, tx_seq) = match gen.stash.as_ref() {
        Some(s) => (s.envelopes.first().cloned(), s.tx_seq),
        None => return,
    };
    if redo {
        if history.undo_top_seq() == Some(tx_seq) {
            // 重做生成：当前选区正是被消费的那条 → 再消费；新选区不动。
            if selection.closed.as_ref() == stash_ring.as_ref() {
                selection.cancel();
            }
        }
    } else if history.redo_top_seq() == Some(tx_seq) {
        // 撤销生成：槽位空闲才回驻留；作者已画新选区则新选区赢、stash 丢弃。
        if selection.closed.is_none() && !selection.is_drawing() {
            if let Some(ring) = stash_ring {
                selection.close_with(ring);
            }
        } else {
            gen.stash = None;
        }
    }
}

/// 执行一次撤销 / 重做，返回给用户看的结果说明。
///
/// **唯一入口**：快捷键、命令面板、菜单都经此函数，避免「两处各写一遍」导致
/// 同一次 `Ctrl+Z` 被应用两次（曾经 `handle_undo_redo_keys` 与界面快捷键各自
/// 直接改历史，就是这个隐患）。
///
/// 播放中只读（基线 V37）：拒绝撤销并说明原因，而不是静默无反应。
pub fn apply_history_command(
    redo: bool,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    playback: &PlaybackState,
    selection: &mut SelectionState,
    gen: &mut ContinentGenerationState,
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再撤销".into());
    }
    // 撤销 dirty 追踪（v0.2）：撤销/重做前快照地形 cells，事后逐格 diff 出
    // 变化包围盒，只把该矩形标脏——避免 `mark_all_dirty` 引发的整图重建。
    // 512² 地形 ≈ 786KB 克隆 + 一次 O(N) 比较，亚毫秒级；换来的收益是
    // 撤销一笔刷只重算笔刷包围盒而非整图（~80ms → <1ms）。
    //
    // Phase 2：快照对象必须是**纹理正在显示的那张图**的地形（当前图或它继承到的
    // 祖先图），否则 diff 出的矩形会落在另一张图上。cells 一并 clone 出来，
    // 免得 `after` 借着 `tex` 而后面又要 `tex.mark_dirty`。
    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, None)
        .map(|t| t.cells.clone());
    let result = if redo {
        edit.history.redo(&mut projection.world)
    } else {
        edit.history.undo(&mut projection.world)
    };
    match result {
        Ok((true, _dirty_paths)) => {
            projection.dirty = true;
            // 大陆生成恢复圈：撤掉生成命令 → 消费掉的意图圈回来；redo → 再消费。
            sync_selection_after_history_step(redo, &edit.history, selection, gen);
            let after = wv_render::terrain_view::texture_terrain(tex, &projection.world, None)
                .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    // 有变化区域 → 增量重传该矩形；无地形变化（撤销的可能是
                    // 非地形命令）→ 不动纹理。
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                // 地形存在性或尺寸变了 → 全量重建。
                _ => tex.mark_all_dirty(),
            }
            Ok(if redo {
                "已重做".into()
            } else {
                "已撤销".into()
            })
        }
        Ok((false, _)) => Ok(if redo {
            "没有可重做的操作".into()
        } else {
            "没有可撤销的操作".into()
        }),
        Err(e) => Err(format!("撤销/重做失败: {e}")),
    }
}

/// 阶段六收尾：刷新 `ProjectDiagnostics`。
///
/// **触发条件**：`ProjectDiagnostics::needs_diagnose` 为 true。
/// 三个 set 点：`load_project_into`（加载后立刻灌过，标志被清回 false，
/// 这里只在新工程/编辑/撤销时跑）、`commit_stroke_on_release`、
/// `cmd_dispatch`（撤销/重做成功时）。
///
/// **何时跑**：Update 阶段，**早于**校验面板所在的 UI 帧。
/// 这样面板当帧读到的就是新结果（避免半帧错位）。
///
/// **节流**：当前不做节流——`derive_hints` 在本工程规模（≤ 几百事件）单次
/// 亚毫秒，无须防抖。若规模上来再加重算预算。
pub fn refresh_project_diagnostics(
    projection: Res<WorldProjection>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
) {
    if !diagnostics.needs_diagnose {
        return;
    }
    // 错误 + 提示。复用 `wv-io` 的引用校验逻辑（与 `Project::diagnose` 共享底层），
    // 但走的是**内存里的** `World` 而不是磁盘——编辑器不应当每帧都敲盘。
    let d = crate::edit::diagnostics_bridge::derive_diagnostics(&projection.world);
    diagnostics.last_count = d.items.len();
    diagnostics.diagnostics = d;
    diagnostics.needs_diagnose = false;
}

#[cfg(test)]
mod tests {
    //! 笔触包围盒 → dirty 矩形（优化项 3d 的入口算法）。
    use super::*;
    use wv_terrain::edit::Brush;

    /// 单点笔触：包围盒 = 中心格 ± radius/cell_size + 1。
    #[test]
    fn single_point_bbox_includes_radius_padding() {
        let grid = GridConfig {
            cell_size: 10.0,
            ..Default::default()
        };
        let mut stroke = TerrainStroke::new();
        stroke.push(Brush {
            center: (50.0, 50.0), // 格 (5, 5)
            radius: 10.0,         // 1 格半径
            softness: 0.0,
            kind: None,
            height_delta: 0.0,
        });
        let bbox = stroke_bbox_in_cells(&stroke, &grid);
        // center = 5, pad = ceil(10/10) + 1 = 2
        // → gx0 = 3, gx1 = 7；gy0 = 3, gy1 = 7
        assert_eq!(bbox, [3, 3, 7, 7]);
    }

    /// 多点笔触：取并集包围盒。
    #[test]
    fn multi_point_bbox_is_union() {
        let grid = GridConfig {
            cell_size: 10.0,
            ..Default::default()
        };
        let mut stroke = TerrainStroke::new();
        stroke.push(Brush {
            center: (10.0, 10.0), // 格 (1, 1)
            radius: 5.0,
            softness: 0.0,
            kind: None,
            height_delta: 0.0,
        });
        stroke.push(Brush {
            center: (50.0, 50.0), // 格 (5, 5)
            radius: 5.0,
            softness: 0.0,
            kind: None,
            height_delta: 0.0,
        });
        let bbox = stroke_bbox_in_cells(&stroke, &grid);
        // pad = ceil(5/10) + 1 = 1 + 1 = 2
        // → gx0 = max(1-2, 5-2) = 0；gx1 = max(1+2, 5+2) = 7
        assert_eq!(bbox, [0, 0, 7, 7]);
    }

    /// 钳到 [0, ∞)：负中心被 clamp。
    #[test]
    fn bbox_clamps_to_zero() {
        let grid = GridConfig {
            cell_size: 10.0,
            ..Default::default()
        };
        let mut stroke = TerrainStroke::new();
        stroke.push(Brush {
            center: (5.0, 5.0), // 格 (0, 0) + radius 边距会到负
            radius: 100.0,      // 远大于 cell_size
            softness: 0.0,
            kind: None,
            height_delta: 0.0,
        });
        let bbox = stroke_bbox_in_cells(&stroke, &grid);
        // gx0/gx1 在负值会被 clamp 到 0
        assert_eq!(bbox[0], 0);
        assert_eq!(bbox[1], 0);
        // max 仍 = 0 + 10 + 1 = 11
        assert_eq!(bbox[2], 11);
        assert_eq!(bbox[3], 11);
    }

    /// 空 stroke → `[0, 0, 0, 0]`。
    #[test]
    fn empty_stroke_returns_zero_rect() {
        let grid = GridConfig::default();
        let stroke = TerrainStroke::new();
        let bbox = stroke_bbox_in_cells(&stroke, &grid);
        assert_eq!(bbox, [0, 0, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // 意图笔刷：点列 → Intent 的映射 + 影响包围盒（意图笔刷 plan v0.1）
    // -----------------------------------------------------------------------

    #[test]
    fn intent_from_stroke_maps_each_tool_correctly() {
        let pts = vec![(10.0, 20.0), (30.0, 40.0)];
        // 线状意图：整条折线 + 半径 + 强度。
        match intent_from_stroke(BrushTool::MountainRange, &pts, 8.0, 1.5) {
            Some(Intent::MountainRange {
                pts: p,
                radius,
                strength,
            }) => {
                assert_eq!(p, pts);
                assert_eq!(radius, 8.0);
                assert_eq!(strength, 1.5);
            }
            other => panic!("应为 MountainRange，实得 {other:?}"),
        }
        // 点状意图：只取首点。
        match intent_from_stroke(BrushTool::Volcano, &pts, 8.0, 1.0) {
            Some(Intent::Volcano { center, .. }) => assert_eq!(center, (10.0, 20.0)),
            other => panic!("应为 Volcano，实得 {other:?}"),
        }
        match intent_from_stroke(BrushTool::Lake, &pts, 8.0, 1.0) {
            Some(Intent::Lake { center, .. }) => assert_eq!(center, (10.0, 20.0)),
            other => panic!("应为 Lake，实得 {other:?}"),
        }
        // 河：不带半径（河宽固定一格）。
        match intent_from_stroke(BrushTool::River, &pts, 8.0, 1.2) {
            Some(Intent::River { pts: p, strength }) => {
                assert_eq!(p, pts);
                assert_eq!(strength, 1.2);
            }
            other => panic!("应为 River，实得 {other:?}"),
        }
        // Paint 工具不产生意图（防御兜底）。
        assert_eq!(
            intent_from_stroke(BrushTool::PaintKind, &pts, 8.0, 1.0),
            None
        );
        // 空点列：任何工具都无意图。
        assert_eq!(intent_from_stroke(BrushTool::Volcano, &[], 8.0, 1.0), None);
    }

    /// 意图包围盒按 2.6×radius + 2 格外扩：单点 (50,50)、radius 10、cell 10
    /// → pad = ceil(26/10)+2 = 5 → [0,0,10,10]。
    #[test]
    fn intent_bbox_expands_by_max_reach() {
        let grid = GridConfig {
            cell_size: 10.0,
            ..Default::default()
        };
        let bbox = intent_bbox_in_cells(&[(50.0, 50.0)], 10.0, &grid);
        // 中心格 (5,5)，pad = 2.6×10/10 → ceil(2.6)+2 = 5 → [0,0,10,10]
        assert_eq!(bbox, [0, 0, 10, 10]);
    }

    /// 意图包围盒取点列并集并钳到 0；空点列返回零矩形。
    #[test]
    fn intent_bbox_unions_and_clamps() {
        let grid = GridConfig {
            cell_size: 10.0,
            ..Default::default()
        };
        let bbox = intent_bbox_in_cells(&[(5.0, 5.0), (95.0, 95.0)], 10.0, &grid);
        // pad = 5；两点格 (0,0) 与 (9,9) → [0,0,14,14]
        assert_eq!(bbox, [0, 0, 14, 14]);
        assert_eq!(intent_bbox_in_cells(&[], 10.0, &grid), [0, 0, 0, 0]);
    }

    // -----------------------------------------------------------------------
    // v0.2：terrain_diff_bbox 单元测试
    // -----------------------------------------------------------------------

    use wv_core::terrain::TerrainCellData;

    fn cell(kind: u8) -> TerrainCellData {
        TerrainCellData { height: 0, kind }
    }

    /// 无变化 → None。
    #[test]
    fn diff_bbox_none_when_identical() {
        let a = vec![cell(3), cell(4), cell(3), cell(4)];
        let b = a.clone();
        assert_eq!(terrain_diff_bbox(&a, &b, 2), None);
    }

    /// 单格变化 → 恰好包住该格的 1×1 矩形。
    #[test]
    fn diff_bbox_single_changed_cell() {
        // 2×2：索引 3 = (1,1)。
        let before = vec![cell(1), cell(1), cell(1), cell(1)];
        let mut after = before.clone();
        after[3] = cell(7);
        assert_eq!(terrain_diff_bbox(&before, &after, 2), Some([1, 1, 2, 2]));
    }

    /// 多格分散变化 → 包围盒取并集。
    #[test]
    fn diff_bbox_scattered_changes_union() {
        // 4×4：改 (0,0) 与 (3,2) → 包围盒 [0,0]-[4,3]。
        let before = vec![cell(1); 16];
        let mut after = before.clone();
        after[0] = cell(2); // (0,0)
        after[2 * 4 + 3] = cell(2); // (3,2)
        assert_eq!(terrain_diff_bbox(&before, &after, 4), Some([0, 0, 4, 3]));
    }

    /// 高度变化也算变化（不只 kind）。
    #[test]
    fn diff_bbox_detects_height_change() {
        let before = vec![cell(1); 4];
        let mut after = before.clone();
        after[1].height = 500;
        assert_eq!(terrain_diff_bbox(&before, &after, 2), Some([1, 0, 2, 1]));
    }

    // -----------------------------------------------------------------------
    // 大陆生成接线（大陆生成 UI plan Task 4）
    // -----------------------------------------------------------------------

    fn continent_test_world(
        size: u32,
    ) -> (WorldProjection, EditHistory, TerrainTexture, wv_core::Id) {
        let map = wv_core::Id::new_unchecked("world_main");
        let mut projection = WorldProjection::default();
        let grid = wv_terrain::TerrainGrid::new(size, size);
        let cells: Vec<wv_core::terrain::TerrainCellData> = grid
            .into_cells()
            .into_iter()
            .map(|c| wv_core::terrain::TerrainCellData {
                height: c.height,
                kind: c.kind,
            })
            .collect();
        projection.world.set_terrain(
            map.clone(),
            wv_core::terrain::TerrainData {
                width: size,
                height: size,
                cells,
            },
        );
        let tex = TerrainTexture {
            handle: Handle::default(),
            size,
            height: size,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
            last_map: Some(map.clone()),
        };
        (projection, EditHistory::default(), tex, map)
    }

    fn square_envelope() -> Vec<(f32, f32)> {
        vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)]
    }

    fn generate_once(
        projection: &mut WorldProjection,
        edit: &mut EditHistory,
        tex: &mut TerrainTexture,
        selection: &mut SelectionState,
        gen: &mut ContinentGenerationState,
        map: &wv_core::Id,
    ) {
        selection.close_with(square_envelope());
        let ring = selection.consume_closed().expect("选区已闭合，消费应得环");
        commit_continent_generation(
            map.clone(),
            vec![ring],
            ContinentParams::default(),
            7,
            1.0,
            edit,
            projection,
            tex,
            selection,
            gen,
        )
        .expect("生成应成功");
    }

    #[test]
    fn undo_after_generate_restores_selection_and_grid() {
        let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
        let before = projection
            .world
            .terrain_of(&map)
            .cloned()
            .expect("地形存在");
        let mut selection = SelectionState::default();
        let mut gen = ContinentGenerationState::default();

        generate_once(
            &mut projection,
            &mut edit,
            &mut tex,
            &mut selection,
            &mut gen,
            &map,
        );
        assert!(!selection.is_closed(), "生成即消费选区");
        assert!(gen.stash.is_some(), "生成后应留恢复圈 stash");
        let after = projection.world.terrain_of(&map).cloned().unwrap();
        assert_ne!(before, after, "生成应改变地形");
        assert!(after.cells.iter().any(|c| c.height > 0), "应长出陆地");

        apply_history_command(
            false,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("撤销应成功");
        assert_eq!(
            projection.world.terrain_of(&map).cloned().unwrap(),
            before,
            "撤销后网格应逐格复原"
        );
        assert!(selection.is_closed(), "撤销生成 → 选区回驻留");
        assert_eq!(
            selection.closed,
            Some(square_envelope()),
            "回驻留的应是原选区"
        );
    }

    #[test]
    fn undo_of_other_command_does_not_restore_selection() {
        let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
        let mut selection = SelectionState::default();
        let mut gen = ContinentGenerationState::default();
        generate_once(
            &mut projection,
            &mut edit,
            &mut tex,
            &mut selection,
            &mut gen,
            &map,
        );

        // 再提交一条别的命令（意图山）——历史变长。
        let mut tx = Transaction::new();
        tx.push(Box::new(IntentTerrainCommand::new(
            map.clone(),
            Intent::Hills {
                pts: vec![(24.0, 24.0)],
                radius: 4.0,
                strength: 1.0,
            },
            1.0,
        )));
        edit.history
            .commit(&mut projection.world, tx)
            .expect("第二条命令应成功");

        apply_history_command(
            false,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("撤销应成功");
        assert!(!selection.is_closed(), "撤销的是另一条命令，选区不得回驻留");
    }

    #[test]
    fn redo_reconsumes_selection() {
        let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
        let mut selection = SelectionState::default();
        let mut gen = ContinentGenerationState::default();
        generate_once(
            &mut projection,
            &mut edit,
            &mut tex,
            &mut selection,
            &mut gen,
            &map,
        );
        apply_history_command(
            false,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("撤销");
        assert_eq!(selection.closed, Some(square_envelope()), "撤销后选区回来");

        apply_history_command(
            true,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("重做");
        assert!(!selection.is_closed(), "重做生成 → 选区再次被消费");
    }

    /// 撤销生成时作者已画了新选区 → 新选区赢、stash 丢弃（spec §4 裁定：
    /// 选区是单槽位，回驻留仅在空闲时发生；重做因此无从再消费）。
    #[test]
    fn undo_of_generation_yields_to_newly_drawn_selection() {
        let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
        let mut selection = SelectionState::default();
        let mut gen = ContinentGenerationState::default();
        generate_once(
            &mut projection,
            &mut edit,
            &mut tex,
            &mut selection,
            &mut gen,
            &map,
        );
        let outline_b = vec![(44.0, 44.0), (47.0, 44.0), (47.0, 47.0)];
        selection.close_with(outline_b.clone());

        apply_history_command(
            false,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("撤销");
        assert_eq!(
            selection.closed,
            Some(outline_b.clone()),
            "作者新画的 B 不得被旧圈 A 覆盖"
        );
        assert!(gen.stash.is_none(), "新选区赢 → 恢复圈 stash 丢弃");

        apply_history_command(
            true,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("重做");
        assert_eq!(
            selection.closed,
            Some(outline_b),
            "stash 已丢弃，B 原样保留"
        );
    }

    /// 别的命令的 redo 不得消费意图圈（旧的长度启发式会误判）。
    #[test]
    fn redo_of_other_command_does_not_consume_selection() {
        let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
        let mut selection = SelectionState::default();
        let mut gen = ContinentGenerationState::default();
        generate_once(
            &mut projection,
            &mut edit,
            &mut tex,
            &mut selection,
            &mut gen,
            &map,
        );
        apply_history_command(
            false,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("撤销生成");
        assert_eq!(selection.closed, Some(square_envelope()), "撤销后 A 回来");

        // 提交另一条命令、撤销它、再重做它——意图圈必须原样保留。
        let mut tx = Transaction::new();
        tx.push(Box::new(IntentTerrainCommand::new(
            map.clone(),
            Intent::Hills {
                pts: vec![(24.0, 24.0)],
                radius: 4.0,
                strength: 1.0,
            },
            1.0,
        )));
        edit.history
            .commit(&mut projection.world, tx)
            .expect("第二条命令");
        apply_history_command(
            false,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("撤销别的命令");
        apply_history_command(
            true,
            &mut edit,
            &mut projection,
            &mut tex,
            &PlaybackState::default(),
            &mut selection,
            &mut gen,
        )
        .expect("重做别的命令");
        assert!(selection.is_closed(), "无关命令的 redo 不得消费选区");
    }
}
