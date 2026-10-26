//! 线段 POI 的路径规划：作者钉起终点 → 高度场上出折线（ADR-0070 D3）。
//!
//! ## 两种「性格」
//!
//! | 模式 | 语义 | 代价里的坡度项 |
//! |---|---|---|
//! | [`LinePathMode::Route`]（绕行，默认） | **河**：水不能自己爬坡，路径必须绕开山岭、贴谷而行 | 极重（`LAMBDA_UP` 每米 8.0） |
//! | [`LinePathMode::Carve`]（强制改变地形） | **路 / 河谷**：路径近乎直连，随后由 [`carve_corridor`] 把地形压出来 | 极轻（`CARVE_LAMBDA_UP` 每公里 0.2） |
//!
//! 两者**共用同一套 A\***，只差代价函数里的坡度权重——这是刻意的：同一张网格、
//! 同一条起终点，换个模式就得到「绕山的河」与「直穿山脊的路」，下游的
//! 「下切」动作（[`carve_corridor`]）对两者是同一套。
//!
//! ## 坐标口径
//!
//! - 输入输出都是**图内局部坐标（wu）**，网格原点在 `(0, 0)`（与
//!   [`crate::intent`] 的笔刷坐标同口径：格 `(i, j)` 覆盖
//!   `wu ∈ [i·cell_size, (i+1)·cell_size)`）；
//! - 输出折线的顶点是**途经格的中心**（最多偏起终点半格），随后经
//!   **Chaikin 平滑两轮**（spec §4.2）—— Chaikin 保留首尾点，故折线两端
//!   仍落在起终点所在格的中心上。
//!
//! ## 退化输入
//!
//! 空 `Vec` = **不可达 / 退化**（不 panic）：
//!
//! - 起终点任一落在网格外；
//! - 起终点落在**同一格**（无路径可言）；
//! - 网格被山脊彻底封死（A\* 找不到通路）。
//!
//! 调用方（wv-edit 命令层）负责把这些退化情形翻译成对作者的解释，
//! 本层只负责「要么给出一条路，要么给一条空路」。
//!
//! ## 确定性
//!
//! 同 `(网格, cell_size, 起终点, 模式, seed)` → 逐点相同的结果：
//! A\* 的堆序对 `f` 与格下标做**全序**比较（同 `f` 时按下标定序），
//! 蜿蜒噪声取自 [`crate::hash::cell_hash01`]（仓库唯一确定性格点哈希，
//! 不得另写第二套——ADR-0059）。
//!
//! ## 分层位置
//!
//! 纯数据层叶子 crate `wv-terrain` 内部模块：**零第三方依赖、零文件 IO、
//! 不感知 Undo/Redo**（撤销是 wv-edit 的事）。

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::hash::cell_hash01;
use crate::TerrainGrid;

/// 路径规划模式（ADR-0070 D3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinePathMode {
    /// **绕行**（默认）：上坡重罚，路径自然绕开山岭、贴谷而行。
    ///
    /// 这是**河**的默认性格——水不能自己爬坡，宁可绕远。
    #[default]
    Route,
    /// **强制改变地形**：坡度弱权重，路径近乎直连，随后的地形下切
    /// （[`carve_corridor`]）把通道压出来。
    ///
    /// 这是**路**的默认性格——路可以开山（等而下切由调用方负责）。
    Carve,
}

/// 绕行模式的爬坡权重（**每米**高差）。
///
/// 量级刻意极大：翻越一道 2000 m 的山脊要付 `8.0 × 1950 ≈ 1.5 万` 的代价，
/// 绕行几百米（代价个位数）永远更便宜——于是「河不爬山」成为**涌现行为**，
/// 而不是特判。
const LAMBDA_UP: f32 = 8.0;
/// 绕行模式的下坡权重（**每米**高差）。
///
/// 远小于爬坡权重：下坡是要付代价的（河床一路降坡），但不能贵到让路径
/// 宁可翻山也不肯下坡。
const LAMBDA_DOWN: f32 = 0.1;
/// 下切模式的爬坡权重（**每公里**高差）。
///
/// 与绕行模式的量级差三个数量级：这是刻意的**量纲差异**，不是笔误。
/// 下切模式的语义是「路径近乎直连，随后把地形压出来」——若坡度权重与
/// 绕行模式同量级，山脊会让路径绕远，与该模式语义相反；若按**米**计
/// 0.2（`0.2 × 1950 = 390`），同样会绕远。故此处的 Δh 以**公里**计，
/// 权重 0.2 / km（翻越 2 km 山脊 ≈ 0.4，与「多走几格」的代价同量级，
/// 直连仍然胜出）。
const CARVE_LAMBDA_UP: f32 = 0.2;
/// 绕行模式的蜿蜒噪声幅值（相对 `cell_size`）。
///
/// 作用：让**两条相邻的河**不走出完全重合的折线（视觉上每条河各有自己的
/// 脾气），同时保持确定性（噪声取自格点哈希，非随机数）。
const NOISE_AMP: f32 = 0.3;
/// Chaikin 平滑轮数（spec §4.2：两轮）。
const SMOOTH_ROUNDS: usize = 2;

/// 8 邻域的位移表（正交在前、对角在后）。
const NEIGHBORS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// A\* 堆项。`Ord` 反向（`BinaryHeap` 是大顶堆，此处要小顶堆）。
///
/// 同 `f` 时按格下标定序——**全序**是确定性的前提：否则等代价的两条
/// 路径谁先出堆取决于插入顺序，同 seed 也可能走出不同折线。
#[derive(Debug, Clone, Copy, PartialEq)]
struct HeapItem {
    f: f32,
    idx: u32,
}

impl Eq for HeapItem {}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 在高度场上规划一条从 `start` 到 `end` 的折线（ADR-0070 D3）。
///
/// 返回**图内局部坐标（wu）**的折线顶点；空 `Vec` = 不可达 / 退化输入
/// （见模块文档的「退化输入」）。
///
/// - `mode`：绕行（[`LinePathMode::Route`]）还是近直连下切
///   （[`LinePathMode::Carve`]）；
/// - `seed`：确定性种子（蜿蜒噪声用；同 seed → 同折线）。
///
/// 折线顶点是**途经格的中心**，经 Chaikin 平滑两轮；两端落在起终点所在
/// 格的中心（最多偏半格——调用方若要精确到作者钉的点，自行替换首尾即可）。
pub fn plan_polyline(
    grid: &TerrainGrid,
    cell_size: f32,
    start: (f32, f32),
    end: (f32, f32),
    mode: LinePathMode,
    seed: u64,
) -> Vec<(f32, f32)> {
    if cell_size <= 0.0 {
        return Vec::new();
    }
    let start_cell = cell_of(start, cell_size);
    let end_cell = cell_of(end, cell_size);
    if !grid.contains_xy(start_cell.0, start_cell.1) || !grid.contains_xy(end_cell.0, end_cell.1) {
        return Vec::new();
    }
    if start_cell == end_cell {
        // 同格 = 无路径可言（退化输入）。
        return Vec::new();
    }

    let cells = astar(grid, cell_size, start_cell, end_cell, mode, seed);
    if cells.is_empty() {
        return Vec::new();
    }
    let wu: Vec<(f32, f32)> = cells
        .iter()
        .map(|&(cx, cy)| ((cx as f32 + 0.5) * cell_size, (cy as f32 + 0.5) * cell_size))
        .collect();
    chaikin(&wu, SMOOTH_ROUNDS)
}

/// wu 坐标 → 格坐标（格 `(i, j)` 覆盖 `wu ∈ [i·cell_size, (i+1)·cell_size)`）。
fn cell_of(p: (f32, f32), cell_size: f32) -> (i32, i32) {
    (
        (p.0 / cell_size).floor() as i32,
        (p.1 / cell_size).floor() as i32,
    )
}

/// A\* 主循环。返回**格坐标**路径（起点 → 终点），空 = 不可达。
fn astar(
    grid: &TerrainGrid,
    cell_size: f32,
    start: (i32, i32),
    goal: (i32, i32),
    mode: LinePathMode,
    seed: u64,
) -> Vec<(i32, i32)> {
    let w = grid.width() as usize;
    let h = grid.height() as usize;
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let idx_of = |x: i32, y: i32| (y as usize) * w + (x as usize);

    let n = w * h;
    let mut g_score = vec![f32::INFINITY; n];
    let mut came_from = vec![usize::MAX; n];
    let mut closed = vec![false; n];

    let start_idx = idx_of(start.0, start.1);
    let goal_idx = idx_of(goal.0, goal.1);
    g_score[start_idx] = 0.0;

    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();
    heap.push(HeapItem {
        f: heuristic(start, goal, cell_size),
        idx: start_idx as u32,
    });

    while let Some(item) = heap.pop() {
        let cur = item.idx as usize;
        if closed[cur] {
            continue;
        }
        closed[cur] = true;
        if cur == goal_idx {
            break;
        }
        let cx = (cur % w) as i32;
        let cy = (cur / w) as i32;
        let h_cur = height_at(grid, cx, cy);
        for (dx, dy) in NEIGHBORS {
            let nx = cx + dx;
            let ny = cy + dy;
            if !grid.contains_xy(nx, ny) {
                continue;
            }
            let nidx = idx_of(nx, ny);
            if closed[nidx] {
                continue;
            }
            let dh = height_at(grid, nx, ny) - h_cur;
            let step = cell_size
                * if dx != 0 && dy != 0 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
            let cost = match mode {
                LinePathMode::Route => {
                    let factor = 1.0 + LAMBDA_UP * dh.max(0.0) + LAMBDA_DOWN * (-dh).max(0.0);
                    step * factor + NOISE_AMP * cell_size * cell_hash01(nx as i64, ny as i64, seed)
                }
                LinePathMode::Carve => {
                    let factor = 1.0 + CARVE_LAMBDA_UP * (dh.max(0.0) / 1000.0);
                    step * factor
                }
            };
            let tentative = g_score[cur] + cost;
            if tentative < g_score[nidx] {
                g_score[nidx] = tentative;
                came_from[nidx] = cur;
                heap.push(HeapItem {
                    f: tentative + heuristic((nx, ny), goal, cell_size),
                    idx: nidx as u32,
                });
            }
        }
    }

    if !closed[goal_idx] {
        // 目标未被取出 = 不可达（被山脊彻底封死等）。
        return Vec::new();
    }
    // 回溯
    let mut path = Vec::new();
    let mut cur = goal_idx;
    loop {
        path.push(((cur % w) as i32, (cur / w) as i32));
        if cur == start_idx {
            break;
        }
        let prev = came_from[cur];
        if prev == usize::MAX {
            return Vec::new();
        }
        cur = prev;
    }
    path.reverse();
    path
}

/// 一格的高度（米）。越界按 0 计（调用方已保证在界内）。
fn height_at(grid: &TerrainGrid, x: i32, y: i32) -> f32 {
    grid.get(x, y).map(|c| c.height as f32).unwrap_or(0.0)
}

/// 启发式：到终点的欧氏距离（wu）。
///
/// **可采纳**：任一步的真实代价 ≥ 该步的欧氏长度（代价 = 步长 × 因子，
/// 因子 ≥ 1），故「直线距离 × 1」是代价下界，A\* 给出最优路径。
fn heuristic(from: (i32, i32), goal: (i32, i32), cell_size: f32) -> f32 {
    let dx = (goal.0 - from.0) as f32;
    let dy = (goal.1 - from.1) as f32;
    (dx * dx + dy * dy).sqrt() * cell_size
}

/// Chaikin 角切平滑（spec §4.2：两轮）。
///
/// **保留首尾点**（标准 Chaikin 会丢掉端点）——折线的两端是作者钉的起点 /
/// 终点，不能被平滑吃掉。
fn chaikin(points: &[(f32, f32)], rounds: usize) -> Vec<(f32, f32)> {
    let mut pts = points.to_vec();
    for _ in 0..rounds {
        if pts.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            for t in [0.25f32, 0.75f32] {
                out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
        out.push(pts[pts.len() - 1]);
        pts = out;
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TerrainCell, BIOME_NONE, SURFACE_LAND};

    /// 10×10 夹具：第 5 列的**第 0–8 行**是 2000 m 山脊，其余是 50 m 平原。
    ///
    /// 山脊只留**最下一行（第 9 行）**一个缺口——于是绕行路径的走向被
    /// 几何**钉死**（唯一通路），不依赖噪声项在两条候选通路之间的偶然
    /// 选择；断言阈值因此是稳定的。
    fn ridge_grid() -> TerrainGrid {
        let mut g = TerrainGrid::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let is_ridge = x == 5 && y <= 8;
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: if is_ridge { 2000 } else { 50 },
                        surface: SURFACE_LAND,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        g
    }

    /// 折线长度（wu）。
    fn polyline_length(pts: &[(f32, f32)]) -> f32 {
        pts.windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum()
    }

    /// 起终点：格 `(0, 4)` 与格 `(9, 5)` 的中心（wu）。
    const START: (f32, f32) = (0.5, 4.5);
    const END: (f32, f32) = (9.5, 5.5);

    fn straight_distance() -> f32 {
        ((END.0 - START.0).powi(2) + (END.1 - START.1).powi(2)).sqrt()
    }

    /// 绕行模式：路径必须**避开山脊**（河不爬山），且明显绕远。
    #[test]
    fn routing_mode_detours_around_ridge() {
        let g = ridge_grid();
        let path = plan_polyline(&g, 1.0, START, END, LinePathMode::Route, 7);
        assert!(!path.is_empty(), "绕行模式应找得到路（山脊留有缺口）");
        for &(x, y) in &path {
            let cx = (x / 1.0).floor() as i32;
            let cy = (y / 1.0).floor() as i32;
            let h = g.get(cx, cy).map(|c| c.height).unwrap_or(0);
            assert!(
                h < 2000,
                "路径点 ({x}, {y}) 落在山脊格 ({cx}, {cy}) 上，高度 {h}——河不该爬山"
            );
        }
        let len = polyline_length(&path);
        let straight = straight_distance();
        assert!(
            len > straight * 1.3,
            "绕行路径应明显绕远：实得 {len}（直线 {straight}，门槛 {}）",
            straight * 1.3
        );
    }

    /// 下切模式：坡度弱权重 → 路径近直连（穿山脊，随后由下切压出通道）。
    #[test]
    fn carve_mode_takes_near_direct_path() {
        let g = ridge_grid();
        let path = plan_polyline(&g, 1.0, START, END, LinePathMode::Carve, 7);
        assert!(!path.is_empty(), "下切模式应找得到路");
        let len = polyline_length(&path);
        let straight = straight_distance();
        assert!(
            len < straight * 1.15,
            "下切模式应近直连：实得 {len}（直线 {straight}，门槛 {}）",
            straight * 1.15
        );
    }

    /// 起终点同点 = 退化输入 → 空路径（不 panic）。
    #[test]
    fn identical_endpoints_return_empty() {
        let g = ridge_grid();
        assert!(
            plan_polyline(&g, 1.0, START, START, LinePathMode::Route, 7).is_empty(),
            "起终点同点应返回空路径"
        );
    }

    /// 起终点落在网格外 = 退化输入 → 空路径（不 panic）。
    #[test]
    fn out_of_bounds_points_return_empty() {
        let g = ridge_grid();
        assert!(
            plan_polyline(&g, 1.0, (-5.0, 4.5), END, LinePathMode::Route, 7).is_empty(),
            "起点越界应返回空路径"
        );
        assert!(
            plan_polyline(&g, 1.0, START, (99.0, 99.0), LinePathMode::Route, 7).is_empty(),
            "终点越界应返回空路径"
        );
    }

    /// 确定性契约：同 seed 两次规划逐点相同。
    #[test]
    fn same_seed_same_path() {
        let g = ridge_grid();
        let a = plan_polyline(&g, 1.0, START, END, LinePathMode::Route, 42);
        let b = plan_polyline(&g, 1.0, START, END, LinePathMode::Route, 42);
        assert_eq!(a, b, "同 seed 应逐点一致");
    }
}
