//! 格集 → 大陆包络环提取——CellMap 统一选区（格集）与大陆配方（多边形）
//! 之间的桥（2026-10-26）。
//!
//! 大陆配方 [`crate::continent::generate_continents`] 吃「多边形包络」
//! （图内世界单位，与格遍历 `(gx+0.5)×cell_size` 同口径），而 CellMap
//! 工作台的统一选区（ADR-0057）是**格集**。本模块把格集边界追踪成外轮廓环：
//!
//! - **坐标契约**：顶点 = 格角坐标 × `cell_size` = **栅格局部 wu**——
//!   与 `terrain.bin` 的格 (0,0) 为原点，不沾 map_origin / raster_origin
//!   两套视图原点（配方格遍历就是这个口径）。
//! - **多连通域各出一环**：配方天然支持多包络（每块用自己的内半径做
//!   高度剖面），圈两块就长两块大陆。
//! - **洞填掉**（鞋带面积 ≤ 0 的环丢弃）：大陆语义下「圈了个环形」就是
//!   实心大陆——洞不是海岸线，作者要内海应该用湖意图。
//! - **Douglas–Peucker 抽稀**（`tol = 0.25 × cell_size`，复用
//!   [`crate::isoline::simplify_dp`] 闭环双锚版）：只删**共线节点**（相邻格
//!   共享边造成的中间角点，偏离弦为 0），保留一切真角。容差压在 0.25 格是
//!   算过账的：1 格凹角的弦偏离 ≈ 0.45 格、45° 楼梯齿 ≈ 0.71 格——任何
//!   ≥ 0.45 的容差都会开始吞真凹角，而楼梯齿要 0.71 以上才压得平，单一
//!   容差分不清两者，故只做无损的共线坍缩；贴格楼梯留给配方的 fBm 蜿蜒
//!   去盖（幅度 0.10×diag，远大于 1 格齿距）。距离场 O(格数×边数) 的真正
//!   大头（lasso/泛洪边界上的共享边节点）由此已消。
//! - **对角触（棋盘格）**：边界边带方向（内域在行进方向左侧），走环时按
//!   「最左转」配对——对角相触的两个格域各出各的环，不粘连成一个
//!   自交八边形。
//!
//! 纯函数、零第三方依赖（与 crate 同规）；不感知 Bevy / Undo / 选区状态。

use crate::isoline::{simplify_dp, Pt};
use crate::TerrainGrid;
use std::collections::{BTreeMap, HashSet};

/// 格集 → 包络环（栅格局部 wu）。
///
/// 输入格坐标任意顺序、可有重复；输出环的顶点数 ≥ 3，环按其最小角点排序
/// （确定性——同输入逐位同输出，与配方 D6 同规）。`cell_size` 非正或
/// 输入为空返回空 `Vec`（V1 精神：退化输入零改动，不 panic）。
pub fn cell_set_outline(cells: &[(i32, i32)], cell_size: f32) -> Vec<Vec<(f32, f32)>> {
    if cells.is_empty() || !(cell_size > 0.0 && cell_size.is_finite()) {
        return Vec::new();
    }
    let set: HashSet<(i32, i32)> = cells.iter().copied().collect();

    // 1) 收集边界有向边：邻位缺失 = 边界。方向约定「内域在行进方向左侧」，
    //    因此外环鞋带面积为正、洞为负（第 3 步据此滤洞）。
    let mut edges: BTreeMap<(i32, i32), Vec<(i32, i32)>> = BTreeMap::new();
    for &(x, y) in &set {
        if !set.contains(&(x, y - 1)) {
            edges.entry((x, y)).or_default().push((x + 1, y));
        }
        if !set.contains(&(x + 1, y)) {
            edges.entry((x + 1, y)).or_default().push((x + 1, y + 1));
        }
        if !set.contains(&(x, y + 1)) {
            edges.entry((x + 1, y + 1)).or_default().push((x, y + 1));
        }
        if !set.contains(&(x - 1, y)) {
            edges.entry((x, y + 1)).or_default().push((x, y));
        }
    }

    // 2) 走环：每个角点入度 = 出度（边界边一头一尾必各记一次），沿有向边
    //    必能闭合。对角触点（1 入 2 出）按「最左转」配对，贴着本格域拐走。
    //    `BTreeMap` 键序 + 环按最小角点排序 ⇒ 输出确定。
    let mut loops_int: Vec<Vec<(i32, i32)>> = Vec::new();
    let starts: Vec<(i32, i32)> = edges.keys().copied().collect();
    for start in starts {
        while edges.get(&start).is_some_and(|v| !v.is_empty()) {
            let mut loop_pts = vec![start];
            let mut cur = start;
            let mut dir = (0i32, 0i32);
            // 内层沿边走；`while let` 吃掉编译器建议（clippy while_let_loop）。
            while let Some(ends) = edges.get_mut(&cur) {
                let idx = if ends.len() == 1 {
                    0
                } else {
                    pick_leftmost(ends, dir)
                };
                let next = ends.remove(idx);
                if next == start {
                    break; // 环闭合（start 已在环首）
                }
                dir = (next.0 - cur.0, next.1 - cur.1);
                cur = next;
                loop_pts.push(next);
            }
            if loop_pts.len() >= 3 {
                loops_int.push(loop_pts);
            }
        }
    }

    // 3) 滤洞 + 抽稀 + 缩放到 wu。面积在**整数角点**上算（精确无浮点误差）；
    //    抽稀容差 0.25 格：只坍缩共线节点，真角一律保留（见模块文档的账）。
    /// 待排序的环：`(顶点环, 排序键 = 环的最小角点)`。
    type KeyedRing = (Vec<(f32, f32)>, (i32, i32));
    let mut loops: Vec<KeyedRing> = Vec::new();
    for pts in loops_int {
        if shoelace2(&pts) <= 0 {
            continue; // 洞（或退化）——填掉
        }
        let wu: Vec<Pt> = pts
            .iter()
            .map(|&(x, y)| Pt {
                x: x as f32 * cell_size,
                y: y as f32 * cell_size,
            })
            .collect();
        let simplified = simplify_dp(&wu, true, 0.25 * cell_size);
        let ring: Vec<(f32, f32)> = if simplified.len() >= 3 {
            simplified.iter().map(|p| (p.x, p.y)).collect()
        } else {
            wu.iter().map(|p| (p.x, p.y)).collect()
        };
        let min_corner = *pts.iter().min().unwrap();
        loops.push((ring, min_corner));
    }
    loops.sort_by_key(|(_, min)| *min);
    loops.into_iter().map(|(ring, _)| ring).collect()
}

/// 在 `ends`（≥ 2 条出边，对角触点）里挑「相对来向 `dir` 最左」的那条。
///
/// 优先级：左转 > 直行 > 右转（反向不存在——那条边刚被消费）。首步 `dir`
/// 为零向量时按固定顺序取（确定性；首步选择只影响先画哪个环）。
fn pick_leftmost(ends: &[(i32, i32)], dir: (i32, i32)) -> usize {
    let prefs: [(i32, i32); 3] = if dir == (0, 0) {
        [(1, 0), (0, 1), (-1, 0)]
    } else {
        [(-dir.1, dir.0), (dir.0, dir.1), (dir.1, -dir.0)]
    };
    for p in prefs {
        if let Some(i) = ends.iter().position(|&e| e == p) {
            return i;
        }
    }
    0 // 不可达：出边必为四向之一且非反向
}

/// 整数鞋带面积的两倍（避免除法留整数域；符号即绕向）。
fn shoelace2(pts: &[(i32, i32)]) -> i64 {
    let n = pts.len();
    let mut s = 0i64;
    for i in 0..n {
        let (x1, y1) = pts[i];
        let (x2, y2) = pts[(i + 1) % n];
        s += (x1 as i64) * (y2 as i64) - (x2 as i64) * (y1 as i64);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单元方格环的四角（集合相等，与起点/绕向无关——配方对绕向不敏感）。
    fn assert_corners(ring: &[(f32, f32)], expected: &[(f32, f32)], cell: f32) {
        assert_eq!(
            ring.len(),
            expected.len(),
            "顶点数不符: got {ring:?} want {expected:?}"
        );
        for e in expected {
            assert!(
                ring.iter()
                    .any(|&(x, y)| (x - e.0).abs() < 1e-4 && (y - e.1).abs() < 1e-4),
                "缺角点 {e:?}（cell={cell}）: got {ring:?}"
            );
        }
    }

    #[test]
    fn empty_input_yields_no_loops() {
        assert!(cell_set_outline(&[], 8.0).is_empty());
    }

    #[test]
    fn non_positive_cell_size_yields_no_loops() {
        assert!(cell_set_outline(&[(0, 0), (1, 0)], 0.0).is_empty());
        assert!(cell_set_outline(&[(0, 0)], -1.0).is_empty());
    }

    #[test]
    fn single_cell_yields_unit_square() {
        let out = cell_set_outline(&[(3, 2)], 8.0);
        assert_eq!(out.len(), 1, "单格一环: got {out:?}");
        assert_corners(
            &out[0],
            &[(24.0, 16.0), (32.0, 16.0), (32.0, 24.0), (24.0, 24.0)],
            8.0,
        );
    }

    #[test]
    fn domino_collapses_to_rectangle() {
        let out = cell_set_outline(&[(0, 0), (1, 0)], 1.0);
        assert_eq!(out.len(), 1, "相邻两格一环: got {out:?}");
        assert_corners(
            &out[0],
            &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)],
            1.0,
        );
    }

    #[test]
    fn l_shape_keeps_six_corners() {
        let out = cell_set_outline(&[(0, 0), (1, 0), (0, 1)], 1.0);
        assert_eq!(out.len(), 1, "L 三连一环: got {out:?}");
        assert_corners(
            &out[0],
            &[
                (0.0, 0.0),
                (2.0, 0.0),
                (2.0, 1.0),
                (1.0, 1.0),
                (1.0, 2.0),
                (0.0, 2.0),
            ],
            1.0,
        );
    }

    #[test]
    fn block_collapses_collinear_runs() {
        let mut cells = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                cells.push((x, y));
            }
        }
        let out = cell_set_outline(&cells, 2.0);
        assert_eq!(out.len(), 1, "5×5 实心一环: got {:?}", out.len());
        assert_corners(
            &out[0],
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            2.0,
        );
    }

    #[test]
    fn diagonal_touch_yields_two_loops() {
        // 棋盘对角触：两个格域「最左转」配对下各出各的环，不粘连。
        // 环按最小角点排序：先 (0,0) 域后 (1,1) 域。
        let out = cell_set_outline(&[(0, 0), (1, 1)], 1.0);
        assert_eq!(out.len(), 2, "对角触两环: got {out:?}");
        assert_corners(
            &out[0],
            &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            1.0,
        );
        assert_corners(
            &out[1],
            &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
            1.0,
        );
    }

    #[test]
    fn two_components_yield_two_loops_sorted() {
        let out = cell_set_outline(&[(5, 5), (0, 0)], 1.0);
        assert_eq!(out.len(), 2, "两连通域两环: got {out:?}");
        // 排序按各环最小角点：先 (0,0) 后 (5,5)。
        assert!(
            out[0].iter().all(|&(x, y)| x < 5.0 && y < 5.0),
            "第一环应属 (0,0) 域: got {:?}",
            out[0]
        );
        assert!(
            out[1].iter().all(|&(x, y)| x >= 5.0 && y >= 5.0),
            "第二环应属 (5,5) 域: got {:?}",
            out[1]
        );
    }

    #[test]
    fn donut_hole_is_filled() {
        // 3×3 挖中心：外环保留（正面积），内环（负面积）丢弃 = 洞填掉。
        let mut cells = Vec::new();
        for y in 0..3 {
            for x in 0..3 {
                if !(x == 1 && y == 1) {
                    cells.push((x, y));
                }
            }
        }
        let out = cell_set_outline(&cells, 1.0);
        assert_eq!(out.len(), 1, "洞应被填掉只剩外环: got {out:?}");
        assert_corners(
            &out[0],
            &[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)],
            1.0,
        );
    }

    #[test]
    fn duplicate_inputs_are_deduplicated() {
        let out = cell_set_outline(&[(0, 0), (0, 0), (1, 0), (1, 0)], 1.0);
        assert_eq!(out.len(), 1, "重复格去重后一环: got {out:?}");
        assert_corners(
            &out[0],
            &[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0)],
            1.0,
        );
    }

    #[test]
    fn thick_staircase_survives_as_concave_ring() {
        // 台阶形（两阶 + 末尾探出格）：抽稀不得改变形状——0.25 格容差下
        // 每个真角（最小偏离 ≈0.45 格）都保留。
        let cells = vec![(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (3, 2)];
        let out = cell_set_outline(&cells, 1.0);
        assert_eq!(out.len(), 1, "got {out:?}");
        assert_corners(
            &out[0],
            &[
                (0.0, 0.0),
                (3.0, 0.0),
                (3.0, 2.0),
                (4.0, 2.0),
                (4.0, 3.0),
                (2.0, 3.0),
                (2.0, 1.0),
                (0.0, 1.0),
            ],
            1.0,
        );
    }
}

#[cfg(test)]
mod land_outline_tests {
    //! 海陆掩膜 → 陆地外环（ADR-0074 T6 / `land_outlines`）。
    //!
    //! 坐标 = **栅格局部 wu**（`cell_set_outline` 同口径，见上文坐标契约）。
    //! 湿判定 = `crate::edit::is_water_cell`（surface==水 ∨ height<0）。

    use super::*;
    use crate::{TerrainCell, SURFACE_LAND, SURFACE_WATER};

    fn grid_from(rows: &[(i32, i32)]) -> TerrainGrid {
        let mut g = TerrainGrid::new(8, 8);
        for &(x, y) in rows {
            g.set(
                x,
                y,
                TerrainCell {
                    height: 100,
                    surface: SURFACE_LAND,
                    biome: 0,
                },
            );
        }
        g
    }

    fn all_sea() -> TerrainGrid {
        let mut g = TerrainGrid::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: -100,
                        surface: SURFACE_WATER,
                        biome: 0,
                    },
                );
            }
        }
        g
    }

    #[test]
    fn single_land_cell_produces_unit_square_ring() {
        // 1 格陆地 @ (3,2)，cell=10 → 外环 = 该格四角（10,20)..(40,50)。
        let g = grid_from(&[(3, 2)]);
        let outlines = crate::outline::land_outlines(&g, 10.0);
        assert_eq!(outlines.len(), 1, "1 格陆地应出 1 环: {:?}", outlines);
        let ring = &outlines[0].outer;
        assert_eq!(ring.len(), 4, "单格环 4 角: {:?}", ring);
        for (x, y) in [(30.0, 20.0), (40.0, 20.0), (40.0, 30.0), (30.0, 30.0)] {
            assert!(
                ring.iter()
                    .any(|&(rx, ry)| (rx - x).abs() < 1e-4 && (ry - y).abs() < 1e-4),
                "缺角 ({x},{y}): {ring:?}"
            );
        }
    }

    #[test]
    fn island_with_lake_emits_outer_ring_and_hole() {
        // 3×3 陆块中心 (4,4) 是湖（水）→ 外环 1 + 洞 1。
        let mut cells = Vec::new();
        for y in 3..6 {
            for x in 3..6 {
                cells.push((x, y));
            }
        }
        let mut g = grid_from(&cells);
        g.set(
            4,
            4,
            TerrainCell {
                height: -10,
                surface: SURFACE_WATER,
                biome: 0,
            },
        );
        let outlines = crate::outline::land_outlines(&g, 10.0);
        assert_eq!(outlines.len(), 1, "一块陆地应出 1 组环: {:?}", outlines);
        let group = &outlines[0];
        // 外环 = 3×3 块的四角（抽稀后 4 点）；洞 = 中心格四角。
        assert_eq!(group.outer.len(), 4, "外环应抽稀到 4 角: {:?}", group.outer);
        assert_eq!(group.holes.len(), 1, "湖应出一个洞: {:?}", group.holes);
        assert_eq!(group.holes[0].len(), 4, "单格洞 4 角: {:?}", group.holes[0]);
        // 洞角坐标 = (4,4) 格四角（40,40)-(50,50)。
        for (x, y) in [(40.0, 40.0), (50.0, 40.0), (50.0, 50.0), (40.0, 50.0)] {
            assert!(
                group.holes[0]
                    .iter()
                    .any(|&(rx, ry)| (rx - x).abs() < 1e-4 && (ry - y).abs() < 1e-4),
                "洞缺角 ({x},{y}): {:?}",
                group.holes[0]
            );
        }
    }

    #[test]
    fn all_sea_yields_empty() {
        let g = all_sea();
        assert!(
            crate::outline::land_outlines(&g, 10.0).is_empty(),
            "全海无陆地环"
        );
    }

    #[test]
    fn height_below_zero_counts_as_water_even_with_land_surface() {
        // surface=陆 但 height<0 → 湿判定为水（is_water_cell 口径）。
        let mut g = grid_from(&[(3, 3)]);
        g.set(
            3,
            3,
            TerrainCell {
                height: -5,
                surface: SURFACE_LAND,
                biome: 0,
            },
        );
        assert!(
            crate::outline::land_outlines(&g, 10.0).is_empty(),
            "height<0 的格按湿判定是水，不出环"
        );
    }

    #[test]
    fn two_disjoint_islands_emit_two_rings() {
        let g = grid_from(&[(1, 1), (6, 6)]);
        let outlines = crate::outline::land_outlines(&g, 10.0);
        assert_eq!(outlines.len(), 2, "两块不相连陆地各出 1 组: {outlines:?}");
    }
}

/// 陆地外环组（ADR-0074 T6）：一块连通陆地的外环 + 它的洞（湖）。
#[derive(Debug, Clone, PartialEq)]
pub struct LandOutline {
    /// 外环：栅格局部 wu 坐标，首尾不重复。
    pub outer: Vec<(f32, f32)>,
    /// 洞环列表（湖面），坐标口径同外环。
    pub holes: Vec<Vec<(f32, f32)>>,
}

/// 海陆掩膜 → 陆地外环（含洞）——势力图切蛋糕的「整块地」（ADR-0074 D1）。
///
/// 湿判定走 [`crate::edit::is_water_cell`]（surface==水 ∨ height<0，唯一
/// 入口，**不**在此重复公式）。坐标 = **栅格局部 wu**（`cell_set_outline`
/// 同口径——terrain.bin 的格 (0,0) 为原点；wv-core 的 `BeginPartitionSpan`
/// 会在存进分区数据前做栅格局部 → map-local 的平移）。
///
/// 与 [`cell_set_outline`] 的两点差异：
/// - **洞保留**：分区语义下湖是「不可分配的地」，必须成为脸的洞环——
///   大陆配方的「洞填掉」不适用；
/// - 输入是整张栅格而非格集（掩膜 = 该格不湿）。
///
/// 输出按外环最小角点排序（确定性，同输入逐位同输出）。
pub fn land_outlines(grid: &TerrainGrid, cell_size: f32) -> Vec<LandOutline> {
    if !(cell_size > 0.0 && cell_size.is_finite()) {
        return Vec::new();
    }
    // 收集陆格集合（湿判定唯一入口）。
    let land: Vec<(i32, i32)> = (0..grid.height())
        .flat_map(|y| (0..grid.width()).map(move |x| (x as i32, y as i32)))
        .filter(|&(x, y)| {
            grid.get(x, y)
                .map(|c| !crate::edit::is_water_cell(c))
                .unwrap_or(false)
        })
        .collect();
    if land.is_empty() {
        return Vec::new();
    }
    let set: HashSet<(i32, i32)> = land.into_iter().collect();

    // 边界有向边：与 cell_set_outline 同一约定「内域在行进方向左侧」——
    // 外环面积为正、洞为负。
    let mut edges: BTreeMap<(i32, i32), Vec<(i32, i32)>> = BTreeMap::new();
    for &(x, y) in &set {
        if !set.contains(&(x, y - 1)) {
            edges.entry((x, y)).or_default().push((x + 1, y));
        }
        if !set.contains(&(x + 1, y)) {
            edges.entry((x + 1, y)).or_default().push((x + 1, y + 1));
        }
        if !set.contains(&(x, y + 1)) {
            edges.entry((x + 1, y + 1)).or_default().push((x, y + 1));
        }
        if !set.contains(&(x - 1, y)) {
            edges.entry((x, y + 1)).or_default().push((x, y));
        }
    }

    // 走环（最左转配对，与 cell_set_outline 同法）。
    let mut loops_int: Vec<Vec<(i32, i32)>> = Vec::new();
    let starts: Vec<(i32, i32)> = edges.keys().copied().collect();
    for start in starts {
        while edges.get(&start).is_some_and(|v| !v.is_empty()) {
            let mut loop_pts = vec![start];
            let mut cur = start;
            let mut dir = (0i32, 0i32);
            while let Some(ends) = edges.get_mut(&cur) {
                let idx = if ends.len() == 1 {
                    0
                } else {
                    pick_leftmost(ends, dir)
                };
                let next = ends.remove(idx);
                if next == start {
                    break;
                }
                dir = (next.0 - cur.0, next.1 - cur.1);
                cur = next;
                loop_pts.push(next);
            }
            if loop_pts.len() >= 3 {
                loops_int.push(loop_pts);
            }
        }
    }

    // 面积分拣：正 = 外环（新组），负 = 洞（并入**包含它的最小外环**组——
    // 「湖属于最小的那块陆」：岛中湖不该归到大陆头上）。整数域算面积（精确）。
    // 先收全外环再分派洞：环序任意，外环可能晚于洞出现。
    let mut outers: Vec<(Vec<(i32, i32)>, i64)> = Vec::new();
    let mut holes: Vec<Vec<(i32, i32)>> = Vec::new();
    for pts in loops_int {
        let area2 = shoelace2(&pts);
        if area2 > 0 {
            outers.push((pts, area2));
        } else if area2 < 0 {
            holes.push(pts);
        }
    }
    outers.sort_by_key(|(outer, _)| *outer.iter().min().unwrap());
    /// 组 = (外环, 面积×2, 洞列表)。
    type HoleGroup = (Vec<(i32, i32)>, i64, Vec<Vec<(i32, i32)>>);
    let mut groups: Vec<HoleGroup> = outers
        .iter()
        .map(|(o, a)| (o.clone(), *a, Vec::new()))
        .collect();
    for hole in holes {
        let probe = hole[0];
        // 包含探针点的外环里取面积最小者（岛中湖归岛不归大陆）。
        let best = groups
            .iter()
            .enumerate()
            .filter(|(_, (outer, _, _))| point_in_ring(probe, outer))
            .min_by_key(|(_, (_, a2, _))| *a2);
        if let Some((gi, ..)) = best {
            groups[gi].2.push(hole);
        }
        // 洞不属于任何外环（不可能——环必由边界面围出）→ 丢弃。
    }

    // 抽稀 + wu。洞与外环同容差（0.25 格，只坍缩共线）。外环序已在分拣时定。
    groups
        .into_iter()
        .map(|(outer, _, holes)| {
            let to_ring = |pts: &[(i32, i32)]| -> Vec<(f32, f32)> {
                let wu: Vec<Pt> = pts
                    .iter()
                    .map(|&(x, y)| Pt {
                        x: x as f32 * cell_size,
                        y: y as f32 * cell_size,
                    })
                    .collect();
                let simplified = simplify_dp(&wu, true, 0.25 * cell_size);
                if simplified.len() >= 3 {
                    simplified.iter().map(|p| (p.x, p.y)).collect()
                } else {
                    wu.iter().map(|p| (p.x, p.y)).collect()
                }
            };
            LandOutline {
                outer: to_ring(&outer),
                holes: holes.iter().map(|h| to_ring(h)).collect(),
            }
        })
        .collect()
}

/// 射线法：整数点是否在整数环内（环顶点 ≥3；边界情形取「在」——洞探针
/// 是洞环自己的顶点，它恰落在外环边界内侧一格，不触边）。
fn point_in_ring(p: (i32, i32), ring: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if ((yi > p.1) != (yj > p.1)) && (p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}
