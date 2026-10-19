//! 单格命中测试（[ADR-0040] §决策 3）。
//!
//! 把鼠标在视图世界坐标按下，转换到栅格（地形）整数格坐标 `(gx, gy)`。
//! **仅在 `ToolKind::EditCell` 激活时调用**（其他工具不浪费采样）。
//!
//! 与 [`crate::ui::hover_tooltip`] 的 `label_pick::hit_test` 同款模板：
//! 输入光标视图坐标，输出命中格或 `None`。
//!
//! ## 单位换算
//!
//! ```text
//! grid_x = (cursor_world.x - grid.raster_origin.x) / grid.cell_size
//! gx      = grid_x.floor() as i32
//! gy      = (cursor_world.y - grid.raster_origin.y) / grid.cell_size   // 同法
//! ```
//!
//! 越界（gx < 0 / gx ≥ width / gy < 0 / gy ≥ height）→ `None`。
//! 浮点 NaN（坐标脏）→ `None`（不 panic）。
//!
//! [ADR-0040]: ../docs/adr/ADR-0040-cell-inspector-tool.md

use wv_core::terrain::TerrainData;
use wv_render::state::GridConfig;

/// 单格命中结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPickResult {
    /// 格点整数坐标（逻辑格空间,`0 ≤ gx < width`）。
    pub gx: i32,
    pub gy: i32,
}

/// 按给定光标视图坐标 + 当前栅格配置 + 当前图地形命中测试。
///
/// **`grid.raster_origin` 是地形栅格在世界里的原点**；光标转栅格公式取与
/// `wv_render::terrain_chunks::sync_terrain_chunks` 同一口径（用同一 grid）
/// ——保证「光标压在视觉上的格」与「pick 返回的格」**逐格一致**。
pub fn pick_at(
    cursor_world: (f32, f32),
    grid: &GridConfig,
    terrain: &TerrainData,
) -> Option<CellPickResult> {
    let (cx, cy) = cursor_world;
    if !cx.is_finite() || !cy.is_finite() || grid.cell_size <= 0.0 {
        return None;
    }
    // 栅格坐标 = (cursor_world - raster_origin) / cell_size。
    // 与 `wv_terrain::TerrainGrid::sample_world` 同口径。
    let grid_x = (cx - grid.raster_origin.x) / grid.cell_size;
    let grid_y = (cy - grid.raster_origin.y) / grid.cell_size;
    // `floor()` 后钳 [0, w-1] / [0, h-1]；floor 落在负坐标（在图外）→ 越界返 None。
    let gx_f = grid_x.floor();
    let gy_f = grid_y.floor();
    if !gx_f.is_finite() || !gy_f.is_finite() {
        return None;
    }
    let gx = gx_f as i32;
    let gy = gy_f as i32;
    if gx < 0 || gx >= terrain.width as i32 || gy < 0 || gy >= terrain.height as i32 {
        return None;
    }
    Some(CellPickResult { gx, gy })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    fn grid(cell_size: f32, ox: f32, oy: f32) -> GridConfig {
        GridConfig {
            cell_size,
            raster_origin: Vec2::new(ox, oy),
            map_origin: Vec2::ZERO,
            map_size: Vec2::new(1024.0, 512.0),
        }
    }

    fn terrain(w: u32, h: u32) -> TerrainData {
        TerrainData {
            width: w,
            height: h,
            cells: vec![],
        }
    }

    /// 命中栅格内部格 → Some。
    #[test]
    fn pick_at_hits_inside_grid() {
        // cell_size 8, origin (0, 0): 格 (3, 4) 世界坐标 ≈ (24.5, 32.7)。
        let g = grid(8.0, 0.0, 0.0);
        let t = terrain(100, 100);
        let r = pick_at((24.5, 32.7), &g, &t);
        assert_eq!(r, Some(CellPickResult { gx: 3, gy: 4 }));
    }

    /// origin 不为 0 时,正确减去 origin。
    #[test]
    fn pick_at_subtracts_raster_origin() {
        // origin (100, 200) + cell_size 10,格 (2, 3) 世界坐标 = (120, 230)。
        let g = grid(10.0, 100.0, 200.0);
        let t = terrain(10, 10);
        let r = pick_at((123.0, 234.0), &g, &t);
        assert_eq!(r, Some(CellPickResult { gx: 2, gy: 3 }));
    }

    /// 越界 → None(负坐标也返 None 而非 clamp)。
    #[test]
    fn pick_at_out_of_bounds_returns_none() {
        let g = grid(8.0, 0.0, 0.0);
        let t = terrain(10, 10); // 10×10 → gx ∈ [0,9],gy ∈ [0,9]
        // 上方越界
        assert_eq!(pick_at((5.0, -1.0), &g, &t), None);
        // 右方越界
        assert_eq!(pick_at((80.0, 5.0), &g, &t), None);
        // 左侧远
        assert_eq!(pick_at((-1.0, 5.0), &g, &t), None);
    }

    /// 退化输入(NaN / cell_size=0) 不 panic,返 None。
    #[test]
    fn pick_at_rejects_degenerate_input() {
        let g = grid(0.0, 0.0, 0.0);
        let t = terrain(10, 10);
        assert_eq!(pick_at((f32::NAN, 0.0), &g, &t), None);
        assert_eq!(pick_at((0.0, 0.0), &grid(0.0, 0.0, 0.0), &t), None);
    }

    /// 边界格（恰好压在右下角）也命中。
    #[test]
    fn pick_at_inclusive_at_right_edge() {
        let g = grid(8.0, 0.0, 0.0);
        let t = terrain(8, 8);
        // 格 (7, 7) 覆盖 x ∈ [56, 64), y ∈ [56, 64)
        let r = pick_at((63.999, 63.999), &g, &t);
        assert_eq!(r, Some(CellPickResult { gx: 7, gy: 7 }));
    }
}
