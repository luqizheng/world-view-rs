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
use std::collections::{BinaryHeap, HashSet};

use crate::edit::is_water_cell;
use crate::hash::cell_hash01;
use crate::{TerrainCell, TerrainGrid, BIOME_NONE, SURFACE_LAND, SURFACE_WATER};

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
/// 路：走廊削到**岸高附近**的下切深度（米 × strength）。
///
/// 与河的 [`crate::intent::RIVER_CUT_M`] **刻意不同**：路不挖 12 m 深沟，
/// 只把走廊削低到岸高附近——依据 spec §5-9「压平走廊」与 ADR-0070 D3
/// （评审裁定 I1：路是贴地铺路，不是切河谷）。
const ROAD_CUT_M: f32 = 2.0;
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

/// 沿折线下切一条走廊（河谷 / 道路通道），返回**实际被改动的格数**
/// （ADR-0070 D3）。
///
/// `mark_water` 决定这条走廊的语义：
///
/// - `true` = **河**：走廊格写 [`SURFACE_WATER`]、群系清 [`BIOME_NONE`]；
///   下切深度与 legacy 挖河同源（[`crate::intent::RIVER_CUT_M`] /
///   [`crate::intent::RIVER_DROP_M`]），并复刻 legacy 的「不低于 1 m」——
///   走廊内负高度格（海面）下切后**抬回 ≥ 1 m**，宽河与一格宽涂格河的
///   入海口行为一致（评审裁定 I5）；
/// - `false` = **路**：走廊格削到**岸高附近**（[`ROAD_CUT_M`]，压平走廊、
///   不挖 12 m 深沟——评审裁定 I1），**不写水面**，群系保留；其中
///   `height < 0` 的格（海面）**整格跳过**——surface 与 height 都不动，
///   路不在海上铺（评审裁定 I2）。
///
/// `width_cells` 是走廊的**总宽格数**（评审裁定 I4）：走廊 = 中轴 ± `half`
/// 格，`half = (width_cells − 1) / 2`。下游把「宽度米 ÷ 格宽」换算成总宽
/// 格数传入——若按「半径」语义解释，实际走廊会比作者要的宽约 2 倍。
/// `width_cells = 1` ⇒ 单格链（= 既有涂格河），`width_cells = 3` ⇒
/// 中轴 + 两侧各 1 格 = 3 格宽。
///
/// 走廊内**只降不升**（路不把旁边的坑填成 mound）；唯一例外是河分支的
/// 负高度格——它们要按 legacy 口径**抬**回 ≥ 1 m（见上）。
///
/// ## 与既有涂格河的关系
///
/// `width_cells == 1 && mark_water` 时**直接委托**既有的
/// [`crate::intent::apply_river`]（一格宽的涂格河），保证与既有行为
/// **逐位相同**（测试 `width_one_river_matches_legacy_apply_river` 钉死）；
/// 更宽的河走本函数的通用路径（下切深度与 legacy 同源，见上），道路走
/// [`ROAD_CUT_M`] 的独立口径。
pub fn carve_corridor(
    grid: &mut TerrainGrid,
    pts: &[(f32, f32)],
    strength: f32,
    cell_size: f32,
    width_cells: u32,
    mark_water: bool,
) -> usize {
    if pts.is_empty() || cell_size <= 0.0 {
        return 0;
    }
    let width = width_cells.max(1);
    if width == 1 && mark_water {
        // 一格宽 + 写水面 = 既有「涂格河」语义：委托既有实现，行为逐位不变。
        return crate::intent::apply_river(grid, pts, strength, cell_size);
    }
    let s = crate::intent::clamp_strength(strength);

    let chain = dense_chain(grid, pts, cell_size);
    if chain.len() < 2 {
        return 0;
    }
    let chain = orient_chain(grid, chain, mark_water);
    if chain.len() < 2 {
        return 0;
    }

    let bands = corridor_cells(&chain, width);
    let corridor: HashSet<(i32, i32)> = bands.iter().flatten().copied().collect();

    let mut prev = f32::INFINITY;
    let mut touched = 0usize;
    for (i, cells_i) in bands.iter().enumerate() {
        let (cx, cy) = chain[i];
        // 两岸最低（走廊格之外的水格按海平面 0 计）。
        let mut low = f32::INFINITY;
        for &(bx, by) in cells_i {
            for (nx, ny) in [(bx + 1, by), (bx - 1, by), (bx, by + 1), (bx, by - 1)] {
                if corridor.contains(&(nx, ny)) {
                    continue;
                }
                if let Some(c) = grid.get(nx, ny) {
                    let v = if is_water_cell(c) {
                        0.0
                    } else {
                        c.height as f32
                    };
                    low = low.min(v);
                }
            }
        }
        // 河床 / 路床高度：
        // - 河：比两岸最低低 RIVER_CUT_M·s、比上游低 DROP（沿链向下游降）；
        // - 路：只削到岸高附近（ROAD_CUT_M·s，压平走廊不挖深沟——评审 I1），
        //   没有「向下游降」的河语义。
        let mut e = height_at(grid, cx, cy);
        if low.is_finite() {
            let cut = if mark_water {
                crate::intent::RIVER_CUT_M
            } else {
                ROAD_CUT_M
            };
            e = e.min(low - cut * s);
        }
        if mark_water {
            e = e.min(prev - crate::intent::RIVER_DROP_M);
        }
        // 不低于 1 m：河复刻 legacy `apply_river` 的 `.max(1.0)`——入海口处的
        // 负高度格下切后抬回 1 m，宽河窄河行为一致（评审 I5）。
        e = e.max(1.0);
        for &(bx, by) in cells_i {
            let Some(cell0) = grid.get(bx, by) else {
                continue;
            };
            // 路遇负高度格（海面）：整格跳过，surface 与 height 都不动
            // （评审 I2：路不在海上铺）。
            if !mark_water && cell0.height < 0 {
                continue;
            }
            // 河：与 legacy `apply_river` 同式 `to_height(e)`——负高度格被抬到
            // ≥ 1 m 后不得被「只降不升」钳回海里（评审 I5）；对 height ≥ 0 的
            // 格 e 本就 ≤ 原高，结果等价于只降不升。路：保留只降不升——路
            // 不把坑填成丘。
            let new_height = if mark_water {
                crate::intent::to_height(e)
            } else {
                crate::intent::to_height(e.min(cell0.height as f32))
            };
            let new_cell = TerrainCell {
                height: new_height,
                surface: if mark_water {
                    SURFACE_WATER
                } else {
                    SURFACE_LAND
                },
                biome: if mark_water { BIOME_NONE } else { cell0.biome },
            };
            if new_cell != cell0 {
                grid.set(bx, by, new_cell);
                touched += 1;
            }
        }
        // 传未取整的坡降给下游，保证长走廊累计下切。
        prev = e;
    }
    touched
}

/// 密采点 → 格链（相邻去重）。步长 < 格径，折线绝不跳格。
///
/// 与 [`crate::intent::apply_river`] 同款（`step = cell_size / 3`）——同一条
/// 折线在「涂格河」与「走廊下切」两条路径上必须落在**同一串格**上，否则
/// 同一处河谷会被切出两条错位的沟。
fn dense_chain(grid: &TerrainGrid, pts: &[(f32, f32)], cell_size: f32) -> Vec<(i32, i32)> {
    let step = (cell_size / 3.0).max(0.25);
    let mut chain: Vec<(i32, i32)> = Vec::new();
    for w in pts.windows(2) {
        let (ax, ay) = w[0];
        let (bx, by) = w[1];
        let len = ((bx - ax) * (bx - ax) + (by - ay) * (by - ay)).sqrt();
        let steps = (len / step).ceil().max(1.0) as u32;
        for k in 0..=steps {
            let t = k as f32 / steps as f32;
            let x = ax + (bx - ax) * t;
            let y = ay + (by - ay) * t;
            let gx = (x / cell_size).floor() as i32;
            let gy = (y / cell_size).floor() as i32;
            if !grid.contains_xy(gx, gy) {
                continue;
            }
            if chain.last() != Some(&(gx, gy)) {
                chain.push((gx, gy));
            }
        }
    }
    chain
}

/// 定向：把链摆成「源头 → 下游」。
///
/// `stop_at_water` = 河的语义（源头端的水段截掉、走到水即到河口）；道路
/// 没有水陆语义，只按「高的一端是源头」定向。
fn orient_chain(
    grid: &TerrainGrid,
    chain: Vec<(i32, i32)>,
    stop_at_water: bool,
) -> Vec<(i32, i32)> {
    let wet = |c: (i32, i32)| grid.get(c.0, c.1).map(is_water_cell).unwrap_or(true);
    let head = chain[0];
    let tail = *chain.last().expect("链非空");
    let h_head = grid.get(head.0, head.1).map(|c| c.height).unwrap_or(0);
    let h_tail = grid.get(tail.0, tail.1).map(|c| c.height).unwrap_or(0);
    let head_wet = wet(head);
    let tail_wet = wet(tail);
    let reverse = if stop_at_water {
        // 一头在水 → 另一头是源头；都在陆上 → 高的一头是源头。
        (head_wet && !tail_wet) || (!head_wet && !tail_wet && h_head < h_tail)
    } else {
        h_head < h_tail
    };
    let mut chain = chain;
    if reverse {
        chain.reverse();
    }
    if !stop_at_water {
        return chain;
    }
    // 截掉源头端的水段；走到水（河口）就停。
    let mut s0 = 0;
    while s0 < chain.len() && wet(chain[s0]) {
        s0 += 1;
    }
    let mut s1 = s0;
    while s1 < chain.len() && !wet(chain[s1]) {
        s1 += 1;
    }
    chain[s0..s1].to_vec()
}

/// 走廊加宽：**总宽语义**（评审裁定 I4）——`half = (width − 1) / 2`，链上
/// 每格 = 轴心格 + 法线**两侧各** `half` 格；`width = 1` ⇒ 只中轴（与既有
/// 涂格河一致）。下游把「宽度米 ÷ 格宽」换算成**总宽**格数传入，若按半径
/// 语义解释，实际走廊会比作者要的宽约 2 倍。
///
/// 法线取链的**前进方向**的垂直方向（8 邻域单位向量 → 整数偏移），故竖直
/// 走廊的侧格在**左右**、水平走廊的侧格在**上下**。
fn corridor_cells(chain: &[(i32, i32)], width: u32) -> Vec<Vec<(i32, i32)>> {
    let n = chain.len();
    let half = (width.saturating_sub(1) / 2) as i32;
    (0..n)
        .map(|i| {
            let (cx, cy) = chain[i];
            let (tx, ty) = if i + 1 < n {
                (chain[i + 1].0 - cx, chain[i + 1].1 - cy)
            } else if i > 0 {
                (cx - chain[i - 1].0, cy - chain[i - 1].1)
            } else {
                (1, 0)
            };
            let (tx, ty) = if tx == 0 && ty == 0 { (1, 0) } else { (tx, ty) };
            // 前进方向的垂直方向（单位整数向量）。
            let (px, py) = (-ty, tx);
            let mut cells = vec![(cx, cy)];
            for k in 1..=half {
                cells.push((cx + px * k, cy + py * k));
                cells.push((cx - px * k, cy - py * k));
            }
            cells
        })
        .collect()
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

    /// 确定性契约的另一面：**异 seed 应给出不同路径**（守点：蜿蜒噪声不得
    /// 退化成摆设）。
    ///
    /// 夹具用「双缺口山脊」——两条候选通路步数相同，噪声是唯一裁决者；
    /// 单缺口夹具下路径被几何钉死，异 seed 也逐点相同（噪声不可观测）。
    /// 先例：`intent.rs::same_seed_replays_identically_and_different_seed_differs`。
    #[test]
    fn different_seed_yields_different_path() {
        let g = twin_gap_ridge_grid();
        let a = plan_polyline(&g, 1.0, START, END, LinePathMode::Route, 42);
        let b = plan_polyline(&g, 1.0, START, END, LinePathMode::Route, 43);
        assert!(
            !a.is_empty() && !b.is_empty(),
            "两条候选通路都在，不应返回空路径（len a={} b={}）",
            a.len(),
            b.len()
        );
        assert_ne!(a, b, "异 seed 应给出不同路径（等长候选时噪声应改变选择）");
    }

    /// 10×10 夹具：第 5 列**整列**是 2000 m 山脊，只在第 2、8 行各留一个缺口。
    ///
    /// 与 [`ridge_grid`] 的单缺口不同：走 `(5,2)` 或 `(5,8)` 两个缺口的通路
    /// 步数**相同**（各 9 步）——存在等长候选通路时，蜿蜒噪声成为唯一的
    /// 裁决者。单缺口夹具下路径被几何钉死，噪声不可观测（实测 seed 42/43
    /// 路径逐点相同），异 seed 守点因此必须用本夹具。
    fn twin_gap_ridge_grid() -> TerrainGrid {
        let mut g = TerrainGrid::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let is_ridge = x == 5 && y != 2 && y != 8;
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

    // ---- 走廊下切（carve_corridor）----

    /// 10×10 夹具：全平地 100 m，第 (5, 4) 格是 160 m 的**中段鼓包**。
    fn bump_grid() -> TerrainGrid {
        let mut g = TerrainGrid::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let h = if (x, y) == (5, 4) { 160 } else { 100 };
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: h,
                        surface: SURFACE_LAND,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        g
    }

    /// 全平地 100 m（无鼓包）。
    fn flat_grid() -> TerrainGrid {
        let mut g = TerrainGrid::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: 100,
                        surface: SURFACE_LAND,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        g
    }

    /// 竖直河道折线（格中心，列 5、行 0..9）。
    fn vertical_pts() -> Vec<(f32, f32)> {
        (0..10).map(|y| (5.5, y as f32 + 0.5)).collect()
    }

    /// 河：沿链高度单调不增（向下游降）、末段不低于 1 m、中段鼓包被切开。
    ///
    /// 「单调不增」而非「逐格严格递减」：坡降是 `RIVER_DROP_M = 0.4` m/格，
    /// 而高度按**整米**取整（`to_height`），逐格严格递减在数学上不可能成立
    /// ——既有涂格河的测试（`intent.rs::river_chain_descends_and_cuts_below_banks`）
    /// 用的也是「非增」口径。这里额外断言**首尾确有下降**（源头比河口高），
    /// 以证明「河往低处流」这一语义确实成立。
    #[test]
    fn river_carve_descends_monotonically_to_mouth() {
        let mut g = bump_grid();
        let pts = vertical_pts();
        let touched = carve_corridor(&mut g, &pts, 1.0, 1.0, 1, true);
        assert!(touched > 0, "河应改动若干格，实得 {touched}");

        let chain: Vec<(i32, i32, i16)> = (0..10)
            .map(|y| (5, y, g.get(5, y).expect("链格在界内").height))
            .collect();
        for w in chain.windows(2) {
            assert!(
                w[1].2 <= w[0].2,
                "高度应沿链非增：{:?} → {:?}",
                (w[0].0, w[0].1, w[0].2),
                (w[1].0, w[1].1, w[1].2)
            );
        }
        let first = chain[0].2;
        let last = chain[chain.len() - 1].2;
        assert!(
            last < first,
            "河床应比源头低：源头 {first} m → 河口 {last} m"
        );
        assert!(last >= 1, "河床不低于 1 m，实得 {last}");

        // 中段鼓包被切开：河床比两侧平地（100 m）低 ≥ 11 m。
        let bump_h = g.get(5, 4).expect("鼓包格在界内").height;
        assert!(
            bump_h <= 100 - 11,
            "中段鼓包应被切开（≤ 89 m），实得 {bump_h} m"
        );
        assert_eq!(
            g.get(5, 4).expect("鼓包格在界内").surface,
            SURFACE_WATER,
            "河走廊应写水面"
        );
    }

    /// 路：走廊格被**削到岸高附近**（[`ROAD_CUT_M`]，压平走廊不挖深沟——
    /// 评审裁定 I1），且**不写水面**（山间开通道）。
    ///
    /// 断言钉两个边界：① 走廊格高度 ≥ 岸高 − ROAD_CUT_M·s − 1（整米取整
    /// 容差）——削到岸高附近；② 明显高于 12 m 沟（岸高 − `RIVER_CUT_M`）
    /// ——排除复用河常量的回归。
    #[test]
    fn road_carve_flattens_without_water_surface() {
        let mut g = flat_grid();
        let pts = vertical_pts();
        let touched = carve_corridor(&mut g, &pts, 1.0, 1.0, 1, false);
        assert!(touched > 0, "路走廊应改动若干格，实得 {touched}");
        let bank = 100i32; // 夹具岸高（走廊外平地）
        for y in 0..10 {
            let c = g.get(5, y).expect("走廊格在界内");
            let near_bank_floor = bank as f32 - ROAD_CUT_M - 1.0;
            assert!(
                c.height as f32 >= near_bank_floor,
                "走廊格 (5, {y}) 应削到岸高附近（≥ 岸高 − ROAD_CUT_M·s − 1 = {near_bank_floor}），实得 {} m",
                c.height
            );
            let deep_cut_ceiling = bank - crate::intent::RIVER_CUT_M as i32;
            assert!(
                c.height as i32 > deep_cut_ceiling,
                "走廊格 (5, {y}) 不得是 12 m 深沟（应 > 岸高 − RIVER_CUT_M = {deep_cut_ceiling}），实得 {} m",
                c.height
            );
            assert!(
                (c.height as i32) < bank,
                "走廊格 (5, {y}) 应被削低（< 岸高 {bank}），实得 {} m",
                c.height
            );
            assert_ne!(
                c.surface, SURFACE_WATER,
                "路走廊不得写水面（走廊格 (5, {y})）"
            );
            assert_eq!(c.surface, SURFACE_LAND, "路走廊应是陆地");
        }
        // 走廊之外一格不动。
        assert_eq!(
            g.get(4, 5).expect("走廊外格在界内").height,
            100,
            "走廊外的格子不应被改动"
        );
    }

    /// 路遇负高度格（海面）：**整格跳过**——surface 与 height 都不动
    /// （评审裁定 I2：路不在海上铺）。
    #[test]
    fn road_corridor_skips_water_cells() {
        // 前 5 行 100 m 陆地，后 5 行 −40 m 海面。
        let mut g = TerrainGrid::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let is_sea = y >= 5;
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: if is_sea { -40 } else { 100 },
                        surface: if is_sea { SURFACE_WATER } else { SURFACE_LAND },
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        let before: Vec<TerrainCell> = (0..10)
            .map(|y| g.get(5, y).expect("走廊格在界内"))
            .collect();
        let pts = vertical_pts();
        let touched = carve_corridor(&mut g, &pts, 1.0, 1.0, 1, false);
        assert_eq!(touched, 5, "只有陆上 5 格该被改动，实得 {touched}");
        for y in 0..5 {
            let c = g.get(5, y).expect("走廊格在界内");
            assert!(
                c.height < 100,
                "陆上走廊格 (5, {y}) 应被削低，实得 {} m",
                c.height
            );
            assert_eq!(c.surface, SURFACE_LAND, "陆上走廊格 (5, {y}) 应是陆地");
        }
        for y in 5..10 {
            let c = g.get(5, y).expect("走廊格在界内");
            assert_eq!(
                c, before[y as usize],
                "海面格 (5, {y}) 应整格跳过（逐字段相等）"
            );
        }
    }

    /// 加宽走廊（3 格）：中轴**左右各 1 格**都要被改到。
    ///
    /// 宽度语义（**总宽口径**，评审裁定 I4）：`width_cells` 是总宽格数，
    /// `half = (width_cells − 1) / 2`，走廊 = 中轴 ± half 格。
    /// `width_cells = 3` ⇒ 中轴 + 两侧各 1 格 = **3 格宽**。
    #[test]
    fn wide_corridor_touches_flanking_cells() {
        let mut g = flat_grid();
        let pts = vertical_pts();
        let touched = carve_corridor(&mut g, &pts, 1.0, 1.0, 3, true);
        assert!(touched > 0, "加宽走廊应改动若干格，实得 {touched}");
        for y in 0..10 {
            for x in [4, 6] {
                let c = g.get(x, y).expect("侧格在界内");
                assert!(
                    c.height < 100,
                    "加宽走廊应改到侧格 ({x}, {y})，实得 {} m",
                    c.height
                );
                assert_eq!(c.surface, SURFACE_WATER, "侧格 ({x}, {y}) 应写水面");
            }
        }
        // 走廊宽度之外（距中轴 3 格）不该被改到。
        for x in [2, 8] {
            assert_eq!(
                g.get(x, 5).expect("格外格在界内").height,
                100,
                "走廊宽度之外的格子 ({x}, 5) 不应被改动"
            );
        }
    }

    /// 宽度是**总宽**格数而非半径（评审裁定 I4）：`width_cells = 3` ⇒ 走廊
    /// 恰为 3 格宽（中轴 ± 1）。半径口径会把走廊切成 5 格宽——列 3、7 在
    /// 总宽口径下必须原封不动（本测试钉住该边界）。
    #[test]
    fn corridor_width_three_spans_three_cells_in_total() {
        let mut g = flat_grid();
        let pts = vertical_pts();
        let touched = carve_corridor(&mut g, &pts, 1.0, 1.0, 3, true);
        assert!(touched > 0, "走廊应改动若干格，实得 {touched}");
        for y in 0..10 {
            // 中轴 ± 1（列 4、5、6）都在走廊内。
            for x in [4, 5, 6] {
                let c = g.get(x, y).expect("走廊格在界内");
                assert!(
                    c.height < 100,
                    "走廊格 ({x}, {y}) 应被改动，实得 {} m",
                    c.height
                );
                assert_eq!(c.surface, SURFACE_WATER, "走廊格 ({x}, {y}) 应写水面");
            }
            // 半径口径（旧语义）会切到列 3、7——总宽口径下不得波及。
            for x in [3, 7] {
                let c = g.get(x, y).expect("格外格在界内");
                assert_eq!(
                    c.height, 100,
                    "总宽 3 的走廊不得波及格 ({x}, {y})，实得 {} m",
                    c.height
                );
            }
        }
    }

    /// 宽河（通用路径）走廊内的负高度格（海面）：下切后抬到 **≥ 1 m**，与
    /// legacy `apply_river` 的 `.max(1.0)` 口径一致（评审裁定 I5：宽河窄河
    /// 入海口行为一致）。
    #[test]
    fn wide_river_raises_negative_height_cells_like_legacy() {
        // 全图 100 m 陆地，第 6 列是 −40 m 的海（紧贴中轴列 5）。
        let mut g = TerrainGrid::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let is_sea = x == 6;
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: if is_sea { -40 } else { 100 },
                        surface: if is_sea { SURFACE_WATER } else { SURFACE_LAND },
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        let pts = vertical_pts();
        let touched = carve_corridor(&mut g, &pts, 1.0, 1.0, 3, true);
        assert!(touched > 0, "宽河应改动若干格，实得 {touched}");
        for y in 0..10 {
            let c = g.get(6, y).expect("海列格在界内");
            assert!(
                c.height >= 1,
                "宽河走廊内的海面格 (6, {y}) 应抬到 ≥ 1 m（legacy 口径），实得 {} m",
                c.height
            );
            assert_eq!(c.surface, SURFACE_WATER, "海面格 (6, {y}) 应仍是水面");
        }
    }

    /// 一格宽 + 写水面 = 既有涂格河：**逐格相同**（不回归）。
    #[test]
    fn width_one_river_matches_legacy_apply_river() {
        let pts = vertical_pts();
        let mut a = bump_grid();
        let mut b = bump_grid();
        let n1 = carve_corridor(&mut a, &pts, 1.0, 1.0, 1, true);
        let n2 = crate::intent::apply_intent(
            &mut b,
            &crate::intent::Intent::River {
                pts: pts.clone(),
                strength: 1.0,
            },
            1,
            1.0,
        );
        assert_eq!(n1, n2, "改动格数应与既有涂格河一致");
        assert_eq!(a, b, "逐格结果应与既有涂格河完全一致");
    }
}
