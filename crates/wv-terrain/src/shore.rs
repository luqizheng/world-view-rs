//! 离岸距离场（shore distance field）——每个格到最近陆格的距离（格数）。
//!
//! ## 为什么存在
//!
//! 近岸浅滩带、浪花带需要「离岸多远」这一**几何事实**。水深（高度 < 0 的
//! 深度）与岸线位置没有几何关系——DEM 的近岸剖面陡缓不一，深度浅不等于
//! 离岸近。2026-10-20 海洋表现方案 S1（会话裁定）：先在本层造距离场，
//! 浅滩着色 / 浪花几何（渲染侧 S2/S3）消费它；水墨等外部主题同样可用。
//!
//! ## 契约
//!
//! - **纯函数、有界支撑**：任意格的值只依赖半径 `max_dist` 邻域内的水陆
//!   分布。渲染侧的增量 patch 只需把脏区按 `max_dist` 膨胀，即可保持
//!   「局部重画 = 全图重画」逐字节一致（`wv-render` 烘焙契约）。
//! - **距离度量 = 切比雪夫格距**（8 连通 BFS，斜向代价 1）：直线岸边的
//!   等距带与岸线平行，视觉上比曼哈顿距的菱形带更接近等宽浪花带。
//! - 陆格恒 `0`；水格 = 到最近陆格的切比雪夫距离，**截断在 `max_dist`**；
//!   全图无陆时所有格 = `max_dist`。
//!
//! ## 边界（不做什么）
//!
//! - 不做亚格插值存储——消费方（wv-render）用 [`sample_bilinear`] 自行
//!   采样出平滑距离，浪花边才能贴合海岸的插值边而非格边。
//! - 不缓存、不感知渲染：本模块只出 `Vec<u16>`，调用方决定生命周期
//!   （wv-render 现有派生缓存先例：isoline 的 `terrain_data_hash` 触发全图重算）。

use std::collections::VecDeque;

/// 缺省截断半径（格）。
///
/// 浪花带 + 浅滩带 + 一圈过渡的可用宽度：12 格在 work-view-test（4 世界
/// 单位/格）上约 48 wu，全局档观感下足够包住岸线；半径越大 BFS 越慢
/// （整场 O(N)，与半径无关；截断只影响队列长度），取小值保增量 patch
/// 的膨胀成本可控。
pub const DEFAULT_MAX_DIST: u16 = 12;

/// 多源 BFS 求离岸距离场。
///
/// - `w` × `h`：网格尺寸（行优先，与 [`crate::TerrainGrid`] 同序）；
/// - `is_land(x, y)`：水陆谓词（**只对界内坐标调用**）；陆格为种子；
/// - `max_dist`：截断半径（≥ 1；更远的水格一律钳到该值）。
///
/// 返回行优先 `Vec<u16>`，与网格同长度。
pub fn shore_distance_field(
    w: u32,
    h: u32,
    is_land: impl Fn(u32, u32) -> bool,
    max_dist: u16,
) -> Vec<u16> {
    let n = (w as usize) * (h as usize);
    let mut dist = vec![u16::MAX; n];
    if n == 0 || max_dist == 0 {
        return vec![0; n];
    }
    let wi = w as i64;
    let hi = h as i64;
    let mut queue = VecDeque::with_capacity(n);
    for y in 0..hi {
        for x in 0..wi {
            if is_land(x as u32, y as u32) {
                dist[(y * wi + x) as usize] = 0;
                queue.push_back((x, y));
            }
        }
    }
    // 8 连通均匀代价 BFS = 多源切比雪夫距离（截断在 max_dist）。
    while let Some((x, y)) = queue.pop_front() {
        let d = dist[(y * wi + x) as usize];
        if d >= max_dist {
            continue; // 截断：不再向外扩张
        }
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
                    continue;
                }
                let ni = (ny * wi + nx) as usize;
                if dist[ni] > d + 1 {
                    dist[ni] = d + 1;
                    queue.push_back((nx, ny));
                }
            }
        }
    }
    // 截断半径之外（或全图无陆）的水格统一钳到 max_dist。
    dist.iter_mut().for_each(|d| {
        if *d == u16::MAX {
            *d = max_dist;
        }
    });
    dist
}

/// [`wv_core` 无关的便捷版]：直接对 `TerrainCell` 切片求场。
///
/// 湿判定与渲染/笔刷同口径（[`crate::edit::is_water_cell`]：surface 通道
/// ‖ 高度 < 0——「山上的湖」也是水）。
pub fn shore_distance_for_cells(
    cells: &[crate::TerrainCell],
    w: u32,
    h: u32,
    max_dist: u16,
) -> Vec<u16> {
    assert_eq!(
        cells.len(),
        (w as usize) * (h as usize),
        "cells 长度须与 w×h 一致"
    );
    shore_distance_field(
        w,
        h,
        |x, y| !crate::edit::is_water_cell(cells[y as usize * w as usize + x as usize]),
        max_dist,
    )
}

/// 双线性采样距离场（格坐标可为小数），供渲染侧取亚格平滑距离。
///
/// 越界坐标钳到边界（岸线外的浪花带不会因采样出界而断掉）。
pub fn sample_bilinear(field: &[u16], w: u32, h: u32, fx: f32, fy: f32) -> f32 {
    if w == 0 || h == 0 || field.is_empty() {
        return 0.0;
    }
    let wf = (w - 1) as f32;
    let hf = (h - 1) as f32;
    let x = fx.clamp(0.0, wf);
    let y = fy.clamp(0.0, hf);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let at = |px: u32, py: u32| field[py as usize * w as usize + px as usize] as f32;
    let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
    let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
    top * (1.0 - ty) + bottom * ty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TerrainCell, SURFACE_LAND, SURFACE_WATER};

    /// 5×5 网格工具：`rows` 每字符一格，`#` = 陆，`.` = 水。
    fn grid(rows: [&str; 5]) -> Vec<TerrainCell> {
        rows.iter()
            .flat_map(|r| {
                r.chars().map(|c| TerrainCell {
                    height: if c == '#' { 100 } else { -100 },
                    surface: if c == '#' {
                        SURFACE_LAND
                    } else {
                        SURFACE_WATER
                    },
                    biome: 0,
                })
            })
            .collect()
    }

    /// 竖直直岸：陆在 x=0 列，向右距离递增（切比雪夫 = x 距离）。
    #[test]
    fn straight_vertical_coast_distances_increase_offshore() {
        let cells = grid(["#....", "#....", "#....", "#....", "#...."]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        let expected = [0u16, 1, 2, 3, 4];
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(
                    field[y * 5 + x],
                    expected[x],
                    "({x},{y}) 应为 {}，实得 {}",
                    expected[x],
                    field[y * 5 + x]
                );
            }
        }
    }

    /// 陆格恒 0（含被水包围的岛）。
    #[test]
    fn land_cells_are_zero_even_on_islands() {
        let cells = grid(["..#..", "..#..", "#####", "..#..", "..#.."]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        for y in 0..5 {
            for x in 0..5 {
                let is_land = cells[y * 5 + x].surface == SURFACE_LAND;
                assert_eq!(
                    field[y * 5 + x] == 0,
                    is_land,
                    "({x},{y}) 陆格应为 0、水格应非 0，实得 {}",
                    field[y * 5 + x]
                );
            }
        }
    }

    /// 十字岛的角隅水格：切比雪夫距离 = max(dx, dy)（斜向同价）。
    #[test]
    fn diagonal_offset_uses_chebyshev_metric() {
        // 陆只有中心 (2,2)。角 (0,0) 的切比雪夫距 = 2（曼哈顿是 4）。
        let cells = grid([".....", ".....", "..#..", ".....", "....."]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        assert_eq!(field[0], 2, "角 (0,0) 切比雪夫距应为 2");
        assert_eq!(field[4], 2, "(4,0) 应为 2");
        assert_eq!(field[6], 1, "(1,1) 应为 1");
        assert_eq!(field[2], 2, "(2,0) 正上方应为 2（dy=2, dx=0）");
    }

    /// 截断：max_dist=1 时所有水格（无论多远）钳到 1。
    #[test]
    fn field_caps_at_max_dist() {
        let cells = grid(["#....", "#....", "#....", "#....", "#...."]);
        let field = shore_distance_for_cells(&cells, 5, 5, 1);
        for y in 0..5 {
            for x in 1..5 {
                assert_eq!(field[y * 5 + x], 1, "截断后 ({x},{y}) 应为 1");
            }
        }
    }

    /// 全水：无陆种子，全部钳到 max_dist。
    #[test]
    fn all_water_field_caps_everywhere() {
        let cells = grid([".....", ".....", ".....", ".....", "....."]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        assert!(field.iter().all(|d| *d == DEFAULT_MAX_DIST));
    }

    /// 全陆：全部 0。
    #[test]
    fn all_land_field_is_zero() {
        let cells = grid(["#####", "#####", "#####", "#####", "#####"]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        assert!(field.iter().all(|d| *d == 0));
    }

    /// 陆内湖泊：湖心到岸的距离小（湖是水，按同口径参与）。
    #[test]
    fn lake_inside_land_has_small_distances() {
        let cells = grid(["#####", "##.##", "#...#", "##.##", "#####"]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        assert_eq!(field[2 * 5 + 2], 1, "湖心 (2,2) 到岸应为 1（对角即陆）");
        assert_eq!(field[5 + 2], 1, "湖格 (2,1) 应为 1");
    }

    /// 支持有界性：max_dist 半径外的陆地不影响近岸值（增量 patch 膨胀
    /// 脏区的正确性依据）；半径内的会变——附正对照。
    #[test]
    fn field_beyond_support_radius_does_not_change_near_shore() {
        let near = grid(["#....", "#....", "#....", "#....", "#...."]);
        // 第二个网格在 x=4 列加陆地：x≤1 的格距新陆 ≥ 3 > max_dist=2，
        // 值不应变；(3,y) 距新陆 1 格，在半径内，值应从 2（截断）变 1。
        let mut far = near.clone();
        for y in 0..5 {
            far[y * 5 + 4] = TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome: 0,
            };
        }
        let a = shore_distance_for_cells(&near, 5, 5, 2);
        let b = shore_distance_for_cells(&far, 5, 5, 2);
        for y in 0..5 {
            for x in 0..=1 {
                assert_eq!(
                    a[y * 5 + x],
                    b[y * 5 + x],
                    "max_dist=2 时 ({x},{y}) 不应受 3 格外新陆影响"
                );
            }
            assert_eq!(b[y * 5 + 3], 1, "支撑半径内 (3,{y}) 应被新陆拉近为 1");
        }
    }

    /// 双线性采样：整数格点 = 场值；半格点 = 两邻均值。
    #[test]
    fn bilinear_sample_interpolates_between_cells() {
        let cells = grid(["#....", "#....", "#....", "#....", "#...."]);
        let field = shore_distance_for_cells(&cells, 5, 5, DEFAULT_MAX_DIST);
        // 整点：x=2 → 2。
        assert_eq!(sample_bilinear(&field, 5, 5, 2.0, 2.0), 2.0);
        // 半点：x=1.5 → (1+2)/2 = 1.5。
        let mid = sample_bilinear(&field, 5, 5, 1.5, 2.0);
        assert!((mid - 1.5).abs() < 1e-6, "x=1.5 应为 1.5，实得 {mid}");
        // 越界钳制：x=-3 → 与 x=0 同值。
        assert_eq!(sample_bilinear(&field, 5, 5, -3.0, 2.0), 0.0);
    }

    /// 空 / 零尺寸输入不 panic。
    #[test]
    fn degenerate_inputs_return_empty_or_zero() {
        assert!(shore_distance_field(0, 0, |_, _| true, 12).is_empty());
        assert_eq!(shore_distance_field(2, 1, |_, _| false, 0), vec![0, 0]);
        assert_eq!(sample_bilinear(&[], 0, 0, 0.0, 0.0), 0.0);
    }
}
