//! 编辑桥：笔刷笔触 → `PaintTerrainCommand` → `wv-edit::History`（S5 / S9）。
//!
//! ## 铁律（`AGENTS.md` §3.3）
//!
//! **System 只能生成 `EditCommand`，不能自己改世界。** 本模块是唯一的桥：
//! 松手（左键释放）时，把 [`crate::camera::ActiveStroke`] 里的点列转成
//! `wv_terrain::edit::Stroke`，包成 `PaintTerrainCommand`，交给 `History` 提交。
//!
//! ## Undo/Redo（S9）
//!
//! `Ctrl+Z` / `Ctrl+Shift+Z` 由 `crate::ui_shell::hotkeys` 收键、经 `CommandPalette`
//! 派发回本模块的 [`apply_history_command`] 执行——**这里不再自己听键盘**。
//! 撤销/重做与编辑命令都服从同一条门禁：播放中一律拒绝（基线 §1.8 / V37），
//! 因为播放中「当前世界」是时间游标的投影，改它等于改了一个并不存在的现在。
//!
//! 注意这个布局的另一个好处：快捷键只在「输入框未聚焦」时生效这件事，
//! 只在一个地方处理一次（`ui_shell::hotkeys`），不会出现「两个系统都听同一组键」
//! 导致一次按键应用两次的经典 bug。

use bevy::prelude::*;

use crate::camera::ActiveStroke;
use crate::state::{BrushState, GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::terrain_view::{image_from_terrain, TerrainTexture, TileAtlas};
use wv_core::Id;
use wv_edit::command::PaintTerrainCommand;
use wv_edit::history::Transaction;
use wv_edit::History;
use wv_terrain::edit::{Brush, Stroke as TerrainStroke};

/// 编辑历史（`wv-edit::History` 的应用层持有者）。
#[derive(Resource, Default)]
pub struct EditHistory {
    pub history: History,
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
    mut projection: ResMut<WorldProjection>,
    mut tex: ResMut<TerrainTexture>,
    mut edit: ResMut<EditHistory>,
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

    // 世界坐标点列 → wv-terrain 的笔触（每点一个 Brush）。
    let mut stroke = TerrainStroke::new();
    for p in active.points.drain(..) {
        // 世界坐标 → 图内局部坐标（反转 GridConfig 的 origin 偏移）。
        let local = p - grid.origin;
        stroke.push(Brush {
            center: (local.x, local.y),
            radius: brush.radius,
            softness: brush.softness,
            kind: Some(brush.kind),
            height_delta: 0.0,
        });
    }

    // 笔触世界包围盒（在构造 `cmd` 之前算——`stroke` move 给 cmd 后不可用）。
    let dirty_rect = stroke_bbox_in_cells(&stroke, &grid);

    let cmd = PaintTerrainCommand::new(
        Id::new_unchecked(view.current_map.clone()),
        stroke,
        grid.cell_size,
    );

    // 提交到 History（一次笔触 = 一个事务 = 一次可撤销单位）。
    let mut tx = Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(_dirty) => {
            projection.dirty = true;
            // 增量重传（优化项 3d）：只标笔触影响矩形为脏，
            // `flush_terrain_texture` 会走 [`crate::terrain_view::patch_terrain_image`] 增量 patch。
            tex.mark_dirty(dirty_rect);
        }
        Err(e) => {
            bevy::log::warn!("编辑命令失败: {e}");
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

/// 把脏标记变为实际纹理更新（增量重传的出口）。
///
/// **优化项 3d**：
/// - 模式切换（`rebuild_texture_on_mode_change`）→ `tex.dirty = Some([0, 0, size, size])`
///   仍走 [`image_from_terrain`] 全图重建；
/// - 笔刷编辑 → `tex.dirty = Some(笔触包围盒)` 走 [`crate::terrain_view::patch_terrain_image`]
///   原地 patch 像素（**关键性能提升**：512² 整图重算 ≈ 80 ms → 几十格 patch ≈ <1 ms）。
///
/// **为何不能直接靠修改 `image.data` 触发 GPU upload**：bevy 0.15 的 `Image` 资源
/// 检测的是「资产添加/移除」，不是 `data` 字段写入——必须走 `Assets::get_mut` 拿到
/// `&mut Image` 后 `patch_terrain_image` 原地改 `data`（已是 `&mut`），bevy 在下一帧
/// 检测到 asset 变更时会上传。
pub fn flush_terrain_texture(
    mut tex: ResMut<TerrainTexture>,
    projection: Res<WorldProjection>,
    view: Res<ViewState>,
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(rect) = tex.dirty else { return };
    let Some(data) = projection.world.terrain.as_ref() else {
        tex.dirty = None;
        return;
    };
    let w = data.width.max(1);
    let h = data.height.max(1);
    // dirty 矩形 = 整图 → 走全图重建（模式切换 / 全图标记脏的场景）。
    let is_full = rect == [0, 0, w, h];
    if is_full {
        let image = image_from_terrain(data, view.mode, atlas.as_deref());
        if let Some(img) = images.get_mut(&tex.handle) {
            *img = image;
        }
        tex.dirty = None;
        return;
    }
    // 增量 patch：只重算 dirty 矩形内的 cell，原地写进 `image.data`。
    let res = tex.res;
    if let Some(img) = images.get_mut(&tex.handle) {
        crate::terrain_view::patch_terrain_image(img, data, view.mode, atlas.as_deref(), rect, res);
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
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再撤销".into());
    }
    // 撤销 dirty 追踪（v0.2）：撤销/重做前快照地形 cells，事后逐格 diff 出
    // 变化包围盒，只把该矩形标脏——避免 `mark_all_dirty` 引发的整图重建。
    // 512² 地形 ≈ 786KB 克隆 + 一次 O(N) 比较，亚毫秒级；换来的收益是
    // 撤销一笔刷只重算笔刷包围盒而非整图（~80ms → <1ms）。
    let before = projection.world.terrain.as_ref().map(|t| t.cells.clone());
    let result = if redo {
        edit.history.redo(&mut projection.world)
    } else {
        edit.history.undo(&mut projection.world)
    };
    match result {
        Ok(true) => {
            projection.dirty = true;
            let after = projection.world.terrain.as_ref();
            match (before, after) {
                (Some(b), Some(a)) if b.len() == a.cells.len() => {
                    // 有变化区域 → 增量重传该矩形；无地形变化（撤销的可能是
                    // 非地形命令）→ 不动纹理。
                    if let Some(rect) = terrain_diff_bbox(&b, &a.cells, a.width.max(1)) {
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
        Ok(false) => Ok(if redo {
            "没有可重做的操作".into()
        } else {
            "没有可撤销的操作".into()
        }),
        Err(e) => Err(format!("撤销/重做失败: {e}")),
    }
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
}
