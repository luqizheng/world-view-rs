//! 笔触提交：松手时把 `TerrainStroke` 转成 `wv-edit` 命令并提交。

use super::*;

/// 这次笔触要不要**先铺场景基底网格**？
///
/// ## 场景编辑器 M1（2026-10-29）
///
/// 一张新建的城市图没有自己的地形栅格，而笔刷要求「这张图有栅格」才生效
/// （`PaintTerrainCommand` 查不到就报 `TerrainMismatch`）。作者的心智是
/// 「进图就画」——故首次刷色时自动补一张基底网格（全「未分区」分区码），
/// 让第一笔直接落在可画的东西上。
///
/// ## 三条判定
///
/// - **世界图不引导**：世界图的地形由大陆生成器负责，铺满是毁数据；
/// - **已有栅格不引导**：作者画过的地形不能被覆盖；
/// - **非场景图不引导**：场景图 = City / Interior 级（判定按「图级 ≠ World」，
///   与渲染层的格级判定口径一致）。
pub(crate) fn needs_scene_base_grid(level: wv_core::map::MapLevel, has_terrain: bool) -> bool {
    level != wv_core::map::MapLevel::World && !has_terrain
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
    // 应用屏状态（2026-10-22 C 档）：加载期间禁写——世界即将被替换，
    // 此刻的笔迹会被整份覆盖丢弃（旧实现加载是同步的，不存在这个窗口）。
    screen: Res<crate::app_screen::AppScreen>,
) {
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    if screen.shows_loading_overlay() {
        active.points.clear();
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

    // 按工具分派：意图工具 → IntentTerrainCommand；涂 / 擦 → PaintTerrainCommand。
    let (cmd, dirty_rect) = if brush.tool.is_intent() {
        let Some(intent) = intent_from_stroke(brush.tool, &pts_local, brush.radius, brush.strength)
        else {
            return;
        };
        let dirty_rect = intent_bbox_in_cells(&pts_local, brush.radius, &grid);
        (
            Box::new(IntentTerrainCommand::new(
                target_map.clone(),
                intent,
                grid.cell_size,
            )) as Box<dyn EditCommand>,
            dirty_rect,
        )
    } else {
        let mut stroke = TerrainStroke::new();
        // 地形橡皮刷（ADR-0043）：整格回填本图底图态，走 `exact` 直写通道；
        // 涂抹则按色板写 `PaintTarget`，高度交给跨域规则。
        let base_cell = (brush.tool == BrushTool::Eraser)
            .then(|| crate::ui::shell::eraser_base_cell(&projection.world, &target_map));
        for (x, y) in &pts_local {
            stroke.push(Brush {
                center: (*x, *y),
                radius: brush.radius,
                softness: brush.softness,
                target: base_cell
                    .is_none()
                    .then(|| crate::ui::shell::brush_target(brush.kind)),
                height_delta: 0.0,
                exact: base_cell,
            });
        }
        // 笔触世界包围盒（在构造 `cmd` 之前算——`stroke` move 给 cmd 后不可用）。
        let dirty_rect = stroke_bbox_in_cells(&stroke, &grid);
        (
            Box::new(PaintTerrainCommand::new(
                target_map.clone(),
                stroke,
                grid.cell_size,
            )) as Box<dyn EditCommand>,
            dirty_rect,
        )
    };

    // 提交到 History（一次笔触 = 一个事务 = 一次可撤销单位）。
    let mut tx = Transaction::new();
    // 场景编辑器 M1：首次在场景图上刷色时，先铺基底网格。
    //
    // **两条命令同事务**——作者一步 Ctrl+Z 就该把「格子和笔迹」一起退掉，
    // 而不是退完格子还剩一笔、或退完笔还剩一张空网格。事务失败时整体
    // 回滚（`Transaction::commit` 的补偿逻辑），不会留半张图。
    let map_level = projection
        .world
        .maps
        .iter()
        .find(|m| m.id == target_map)
        .map(|m| m.level);
    let has_terrain = projection.world.terrain_of(&target_map).is_some();
    if let Some(level) = map_level {
        if needs_scene_base_grid(level, has_terrain) {
            tx.push(Box::new(CreateSceneBaseGridCommand::new(
                target_map.clone(),
                grid.cell_size,
            )));
        }
    }
    tx.push(cmd);
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
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
pub(super) fn stroke_bbox_in_cells(stroke: &TerrainStroke, grid: &GridConfig) -> [u32; 4] {
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
pub(super) fn intent_from_stroke(
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
        BrushTool::PaintKind | BrushTool::Eraser => return None,
    })
}

/// 意图影响包围盒（逻辑格，半开）。统一按 `2.6 × radius` 外扩——取全部意图的
/// 最大 reach（火山 2.6r、山脉 2.0r + 域扭曲 0.45r），脏矩形宁可大不能小：
/// 漏标的格会在纹理上残留旧像素。`radius` 无关的工具（河）传 0 即可只包点列。
pub(super) fn intent_bbox_in_cells(pts: &[(f32, f32)], radius: f32, grid: &GridConfig) -> [u32; 4] {
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
