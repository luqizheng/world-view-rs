//! 群系分界线（2026-10-22 内容矩阵 §2.7 / ADR-0051 · `ToolKind::BiomeBoundary`）。
//!
//! ## 语义（用户裁定 B「分界线」）
//!
//! 线段两侧各绑一个群系（`side_a` / `side_b`）。拖动 = 推移分界：**扫过区域**
//! （新旧线段四端点的凸包）里 `biome == 退让侧` 的格子改成 `推进侧`；
//! **第三种群系不动**（水/山不被森林线误染）。双向拖动对称。
//!
//! ## 线是会话工具，不落盘
//!
//! 落盘真相仍是 `terrain.bin` 的格子群系（P3/P7）。一次拖动（松手）= 1 条
//! `BatchEditCellsCommand`（app 层提交）；区域轮廓继续由连通性派生——线推到
//! 哪，轮廓自己长到哪。
//!
//! ## 坐标系
//!
//! 线段端点存**栅格局部 wu**（与 `rasterize_polygon_cells` 的多边形同空间），
//! 侧向判定用格子中心 `(gx + 0.5, gy + 0.5)`（同空间）。

use crate::cell_selection::rasterize_polygon_cells;
use wv_core::terrain::TerrainCellData;

/// 分界线：端点 `a → b`（栅格局部 wu）+ 两侧群系。
///
/// `side_a` 是 `a→b` 方向**左侧**的群系，`side_b` 是右侧（叉积符号约定，
/// 测试钉死）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundaryLine {
    pub a: (f32, f32),
    pub b: (f32, f32),
    pub side_a: u8,
    pub side_b: u8,
}

/// 点在 `a→b` 的哪一侧：`> 0` 左侧（side_a），`< 0` 右侧（side_b）。
fn side_sign(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// 计算拖动扫过区域的群系改写（纯函数，预览与提交**共用**——预览说什么、
/// 松手改什么，不得两套代码）。
///
/// 扫过区域 = 新旧线段四端点凸包（四边形 `old.a, old.b, new.b, new.a`），
/// 经 `rasterize_polygon_cells` 栅格化；逐格按旧线定「原本在哪侧」，
/// 侧群系变了且格子群系 == 退让侧 ⇒ 改成推进侧。
pub fn swept_changes(
    old: &BoundaryLine,
    new: &BoundaryLine,
    terrain: &wv_core::terrain::TerrainData,
    cell_size: f32,
) -> Vec<(i32, i32, TerrainCellData, TerrainCellData)> {
    // 线段端点是**栅格局部 wu**——栅格化必须用运行时真实格尺寸（默认 8 wu/格）。
    // 曾硬编码 1.0：扫过带被放大 cell_size 倍并错位（final review C1）。
    let quad = [old.a, old.b, new.b, new.a];
    let cells = rasterize_polygon_cells(
        &quad,
        bevy::math::Vec2::ZERO,
        &dummy_grid(terrain, cell_size),
        terrain.width,
        terrain.height,
    );
    let mut out = Vec::new();
    for (gx, gy) in cells {
        if gx < 0 || gy < 0 || gx >= terrain.width as i32 || gy >= terrain.height as i32 {
            continue;
        }
        let idx = (gy as usize) * terrain.width as usize + gx as usize;
        let before = terrain.cells[idx];
        // 原本在哪侧（按**旧**线）→ 退让侧群系；新线同侧 = 该侧没被动过。
        let old_left = side_sign(old.a, old.b, cell_center(gx, gy, cell_size)) > 0.0;
        let new_left = side_sign(new.a, new.b, cell_center(gx, gy, cell_size)) > 0.0;
        if old_left == new_left {
            continue; // 格子没有换侧——分界没扫过它
        }
        // 退让侧 = 旧线该侧群系；推进侧 = 新线该侧群系。第三种群系不动。
        let old_biome = if old_left { old.side_a } else { old.side_b };
        let new_biome = if new_left { new.side_a } else { new.side_b };
        if before.biome != old_biome {
            continue;
        }
        let after = TerrainCellData {
            height: before.height,
            surface: before.surface,
            biome: new_biome,
        };
        out.push((gx, gy, before, after));
    }
    out
}

fn cell_center(gx: i32, gy: i32, cell_size: f32) -> (f32, f32) {
    ((gx as f32 + 0.5) * cell_size, (gy as f32 + 0.5) * cell_size)
}

/// 栅格化所需的 GridConfig 骨架（只用到 cell_size 与原点 = 0）。
fn dummy_grid(terrain: &wv_core::terrain::TerrainData, cell_size: f32) -> crate::state::GridConfig {
    crate::state::GridConfig {
        cell_size,
        raster_origin: bevy::math::Vec2::ZERO,
        map_origin: bevy::math::Vec2::ZERO,
        map_size: bevy::math::Vec2::new(terrain.width as f32, terrain.height as f32),
    }
}

/// 多边形抽稀：保留距上一个保留点 ≥`min_spacing`（wu）的顶点。
///
/// 区域边界锚点的 LOD 抽稀（用户裁定：锚点数量随 zoom 自适应）——间距取
/// `view.scale × 屏幕像素数` 的世界长度，zoom out 稀疏 / zoom in 密集。
pub fn decimate_poly(poly: &[(f32, f32)], min_spacing: f32) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut last: Option<(f32, f32)> = None;
    for &p in poly {
        let far = last
            .map(|l| ((p.0 - l.0).powi(2) + (p.1 - l.1).powi(2)).sqrt() >= min_spacing)
            .unwrap_or(true);
        if far {
            out.push(p);
            last = Some(p);
        }
    }
    out
}

/// 当前缩放下的锚点间距（wu）= `px` 屏幕像素的世界长度，下限 2 格。
pub fn anchor_spacing(view_scale: f32, cs: f32, px: f32) -> f32 {
    (view_scale * px).max(cs * 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    /// 4×4 栅格，全格默认 (h=0, surface=陆, biome=1 森林)；可覆写个别格。
    fn fixture(overrides: &[(i32, i32, u8)]) -> TerrainData {
        let mut cells = vec![
            TerrainCellData {
                height: 0,
                surface: 1,
                biome: 1,
            };
            16
        ];
        for (gx, gy, b) in overrides {
            cells[(*gy as usize) * 4 + *gx as usize] = TerrainCellData {
                height: 0,
                surface: 1,
                biome: *b,
            };
        }
        TerrainData {
            width: 4,
            height: 4,
            cells,
        }
    }

    /// 竖直线 x=2（a=(2,0) b=(2,4)）：左（x<2）森林=1、右（x>2）草原=3。
    fn line(side_a: u8, side_b: u8, x: f32) -> BoundaryLine {
        BoundaryLine {
            a: (x, 0.0),
            b: (x, 4.0),
            side_a,
            side_b,
        }
    }

    #[test]
    fn swept_changes_converts_only_receding_side_biome() {
        // 左=森林(1)、右=草原(3)。右移一格（x 2→3）：扫过 x∈[2,3] 的格子，
        // 退让侧 = 右侧草原(3) → 森林(1)。第三方（水=14）不动。
        let terrain = fixture(&[(2, 0, 3), (3, 0, 3), (3, 1, 14)]);
        let old = line(1, 3, 2.0);
        let new = line(1, 3, 3.0);
        let changes = swept_changes(&old, &new, &terrain, 1.0);
        let touched: Vec<(i32, i32)> = changes.iter().map(|(gx, gy, ..)| (*gx, *gy)).collect();
        assert!(touched.contains(&(2, 0)), "扫过格 (2,0) 草原→森林");
        for (gx, gy, before, after) in &changes {
            assert_eq!(before.biome, 3, "只有退让侧（草原）被改");
            assert_eq!(after.biome, 1);
            let _ = (gx, gy);
        }
        // (3,1) 是水（14）= 第三方，不在改写列表。
        assert!(
            !touched.contains(&(3, 1)),
            "第三方群系（水）绝不被分界线误染"
        );
    }

    #[test]
    fn swept_changes_symmetric_both_drag_directions() {
        // 反向拖（x 3→2）：扫过 x∈[2,3]，退让侧 = 左侧森林(1) → 草原(3)。
        let terrain = fixture(&[(2, 0, 1)]);
        let old = line(1, 3, 3.0);
        let new = line(1, 3, 2.0);
        let changes = swept_changes(&old, &new, &terrain, 1.0);
        assert!(
            changes
                .iter()
                .any(|(gx, gy, _, after)| (*gx, *gy) == (2, 0) && after.biome == 3),
            "反向拖动：森林→草原"
        );
    }

    #[test]
    fn swept_changes_empty_when_line_returns_to_start() {
        let terrain = fixture(&[]);
        let l = line(1, 3, 2.0);
        let changes = swept_changes(&l, &l, &terrain, 1.0);
        assert!(changes.is_empty(), "线拖回原位 = 零改写");
    }

    /// 端点是**栅格局部 wu**——cell_size=8（运行时默认）时 wu 框架必须正确：
    /// x∈[16,24] wu = 格 2..3。曾硬编码 cs=1 导致扫过带放大错位（review C1）。
    #[test]
    fn swept_changes_rasterizes_in_world_units_with_runtime_cell_size() {
        let terrain = fixture(&[(2, 0, 3), (3, 0, 3), (3, 1, 14)]);
        let old = BoundaryLine {
            a: (16.0, 0.0),
            b: (16.0, 32.0),
            side_a: 1,
            side_b: 3,
        };
        let new = BoundaryLine {
            a: (24.0, 0.0),
            b: (24.0, 32.0),
            side_a: 1,
            side_b: 3,
        };
        let changes = swept_changes(&old, &new, &terrain, 8.0);
        let touched: Vec<(i32, i32)> = changes.iter().map(|(gx, gy, ..)| (*gx, *gy)).collect();
        eprintln!("cs=8 touched: {touched:?}");
        assert!(touched.contains(&(2, 0)), "wu 16..24 = 格 2：草原→森林");
        assert!(!touched.contains(&(3, 1)), "水格（第三方）不动");
        for (_, _, before, after) in &changes {
            assert_eq!(before.biome, 3);
            assert_eq!(after.biome, 1);
        }
    }

    #[test]
    fn swept_changes_side_convention_left_is_side_a() {
        // 侧向约定：a→b 方向左侧 = side_a。竖线 a=(2,0)→b=(2,4)（向上），
        // 左 = x<2。右移（扫 x∈[2,3]，原右侧）：退让侧应是 side_b。
        let terrain = fixture(&[(2, 2, 3)]);
        let old = line(1, 3, 2.0);
        let new = line(1, 3, 3.0);
        let changes = swept_changes(&old, &new, &terrain, 1.0);
        assert!(
            changes
                .iter()
                .any(|(gx, gy, before, _)| (*gx, *gy) == (2, 2) && before.biome == 3),
            "右侧格（草原=side_b）是退让侧"
        );
    }
}
