//! 划分内核（ADR-0028 D2/D5）：意图多边形 + 宿主环 → 新势力环 + 宿主余部（带洞）。
//!
//! ## 为什么走栅格而不是多边形布尔代数
//!
//! 零依赖红线（AGENTS §2.1）排除了布尔运算库；而划分天然发生在「有地形栅格的
//! 图上」——把意图与宿主栅格化成布尔掩膜后，区域边界就是掩膜的格边拓扑，
//! 环提取 + 平滑（[`crate::isoline::chaikin`]）即得作者可见的边境线。
//! **新势力环与宿主余部的洞沿来自同一张边界拓扑——共边由构造保证（D3）**。
//!
//! ## 口径
//!
//! - 全部坐标为**格坐标**（格 (x,y) 覆盖 [x,x+1)²，格心 (x+0.5,y+0.5)）；
//!   wu 换算是调用方（wv-edit 桥）的事。
//! - 布尔判定逐格心 even-odd（[`crate::intent::point_in_polygon`] 同源）。
//! - 退化输入（<3 点、意图不在宿主内、余部/新势力碎块化）→ `Err`，不 panic（V1）。

use crate::isoline::{chaikin, simplify_dp, Pt};
use crate::TerrainGrid;

/// 划分失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DivideError {
    /// 意图或宿主不足 3 点。
    TooFewPoints,
    /// 意图与宿主的交集为空（画歪了 / 宿主选错）。
    NotInsideHost,
    /// 碎块化：新势力或宿主余部被切成多块（v1 不支持，ADR-0028 D2）。
    Fragmented { inside: usize, remainder: usize },
}

/// 划分几何结果（格坐标）。
#[derive(Debug, Clone, PartialEq)]
pub struct DivisionGeometry {
    /// 新势力的边境环（v1 恰 1 个）。
    pub inside: Vec<Vec<(f32, f32)>>,
    /// 新势力的洞（与 `inside` 按下标对应；意图圈住既有国中国时非空）。
    pub inside_holes: Vec<Vec<Vec<(f32, f32)>>>,
    /// 宿主余部的外环（v1 恰 1 个）。
    pub remainder_outer: Vec<Vec<(f32, f32)>>,
    /// 余部的洞（与 `remainder_outer` 按下标对应；无洞为空）。
    pub remainder_holes: Vec<Vec<Vec<(f32, f32)>>>,
    /// 贴地是否真的吸附到了地物（Q2 退化回执用）：`Snap` 模式下为 false
    /// 表示窗口内没有地物、边界按原直线生成，调用方应给作者提示。
    pub snapped: bool,
}

/// 布尔栅格。行优先，`index = y * w + x`。
pub(crate) type Mask = Vec<bool>;

fn point_in_mask(mask: &[bool], w: u32, h: u32, x: i64, y: i64) -> bool {
    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
        return false;
    }
    mask[(y as u32 * w + x as u32) as usize]
}

/// 把多边形环栅格化：格心落在环内 → true（even-odd，边界算在内）。
pub fn rasterize_ring(w: u32, h: u32, ring: &[(f32, f32)]) -> Mask {
    let mut mask = vec![false; (w * h) as usize];
    if ring.len() < 3 || w == 0 || h == 0 {
        return mask;
    }
    // 只扫环的 bbox（+1 格余量）：4096² 地图上全图 × 每条边是平方级浪费。
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for &(x, y) in ring {
        if !x.is_finite() || !y.is_finite() {
            return mask;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let x0 = ((min_x.floor() as i64) - 1).clamp(0, w as i64 - 1) as u32;
    let y0 = ((min_y.floor() as i64) - 1).clamp(0, h as i64 - 1) as u32;
    let x1 = ((max_x.ceil() as i64) + 1).clamp(0, w as i64 - 1) as u32;
    let y1 = ((max_y.ceil() as i64) + 1).clamp(0, h as i64 - 1) as u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let c = (x as f32 + 0.5, y as f32 + 0.5);
            if crate::intent::point_in_polygon(c, ring) {
                mask[(y * w + x) as usize] = true;
            }
        }
    }
    mask
}

/// 布尔掩膜的边界环提取：格边有向段（内部在左）跟踪成闭合链，
/// 再 chaikin 平滑 + Douglas-Peucker 抽稀。
///
/// 环的方向携带语义：外环与洞的 shoelace 符号相反——调用方据此分捡。
pub(crate) fn mask_boundary_rings(mask: &[bool], w: u32, h: u32) -> Vec<Vec<(f32, f32)>> {
    raw_mask_boundary_rings(mask, w, h)
        .into_iter()
        .map(|r| smooth_ring(&r))
        .collect()
}

/// 掩膜边界的**未平滑**原始环（格边阶梯）。M2 切弧拼接用原始点列做
/// 「贴宿主边界 / 切口」分类，平滑只在切口弧上做一次。
fn raw_mask_boundary_rings(mask: &[bool], w: u32, h: u32) -> Vec<Vec<(f32, f32)>> {
    // 有向边：内部在左。key = 起点（格点，整数），value = 终点列表。
    use std::collections::BTreeMap;
    let mut edges: BTreeMap<(i64, i64), Vec<(i64, i64)>> = BTreeMap::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !point_in_mask(mask, w, h, x, y) {
                continue;
            }
            // 上邻空 → 边 (x,y)→(x+1,y)；右邻空 → (x+1,y)→(x+1,y+1)；
            // 下邻空 → (x+1,y+1)→(x,y+1)；左邻空 → (x,y+1)→(x,y)。
            if !point_in_mask(mask, w, h, x, y - 1) {
                edges.entry((x, y)).or_default().push((x + 1, y));
            }
            if !point_in_mask(mask, w, h, x + 1, y) {
                edges.entry((x + 1, y)).or_default().push((x + 1, y + 1));
            }
            if !point_in_mask(mask, w, h, x, y + 1) {
                edges.entry((x + 1, y + 1)).or_default().push((x, y + 1));
            }
            if !point_in_mask(mask, w, h, x - 1, y) {
                edges.entry((x, y + 1)).or_default().push((x, y));
            }
        }
    }
    let mut rings = Vec::new();
    // 消费边：每个起点按插入序取边跟踪（插入序由扫描序决定 → 确定性）。
    while let Some((&start, _)) = edges.first_key_value().map(|(k, _)| (k, ())) {
        let mut ring = Vec::new();
        let mut cur = start;
        while let Some(next) = edges.get(&cur).and_then(|t| t.first().copied()) {
            if let Some(targets) = edges.get_mut(&cur) {
                targets.remove(0);
                if targets.is_empty() {
                    edges.remove(&cur);
                }
            }
            ring.push((cur.0 as f32, cur.1 as f32));
            cur = next;
            if cur == start {
                break;
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    rings
}

/// 环平滑：chaikin 2 轮 + 抽稀（容差 0.5 格）。
fn smooth_ring(ring: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let pts: Vec<Pt> = ring.iter().map(|&(x, y)| Pt { x, y }).collect();
    let smoothed = chaikin(&pts, true, 2);
    let simplified = simplify_dp(&smoothed, true, 0.5);
    simplified.into_iter().map(|p| (p.x, p.y)).collect()
}

fn ring_signed_area(ring: &[(f32, f32)]) -> f32 {
    let n = ring.len();
    let mut a = 0.0f32;
    for i in 0..n {
        let (px, py) = ring[i];
        let (qx, qy) = ring[(i + 1) % n];
        a += px * qy - qx * py;
    }
    a / 2.0
}

/// 环 = 点列（格坐标）。
pub(crate) type Ring = Vec<(f32, f32)>;

/// 外环列表 + 每个外环的洞列表（下标对应）。
pub(crate) type OutersWithHoles = (Vec<Ring>, Vec<Vec<Ring>>);

/// 分捡「外环 + 隶属洞」：面积符号相反者为洞，按洞代表点落入哪个外环归组。
pub(crate) fn split_outers_and_holes(rings: Vec<Ring>) -> OutersWithHoles {
    let mut outers: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut holes: Vec<Vec<(f32, f32)>> = Vec::new();
    for r in rings {
        if ring_signed_area(&r) >= 0.0 {
            outers.push(r);
        } else {
            holes.push(r);
        }
    }
    let mut grouped = vec![Vec::new(); outers.len()];
    for hole in holes {
        let Some(&(x, y)) = hole.first() else {
            continue;
        };
        if let Some(idx) = outers
            .iter()
            .enumerate()
            .find(|(_, o)| crate::intent::point_in_polygon((x, y), o))
            .map(|(i, _)| i)
        {
            grouped[idx].push(hole);
        }
    }
    (outers, grouped)
}

/// 划分：意图 ∩ 宿主 = 新势力；宿主 ∖ 意图 = 余部（可带洞）。
///
/// 碎块化（新势力或余部被切成多块 / 交集为空）→ `Err`（ADR-0028 D2）。
pub fn divide_geometry(
    grid_w: u32,
    grid_h: u32,
    host_outer: &[(f32, f32)],
    host_holes: &[Vec<(f32, f32)>],
    intent: &[(f32, f32)],
) -> Result<DivisionGeometry, DivideError> {
    if intent.len() < 3 || host_outer.len() < 3 {
        return Err(DivideError::TooFewPoints);
    }
    let intent_mask = rasterize_ring(grid_w, grid_h, intent);
    let mut host_mask = rasterize_ring(grid_w, grid_h, host_outer);
    for hole in host_holes {
        let hm = rasterize_ring(grid_w, grid_h, hole);
        for i in 0..host_mask.len() {
            if hm[i] {
                host_mask[i] = false;
            }
        }
    }
    let mut inside_mask = vec![false; host_mask.len()];
    let mut remainder_mask = vec![false; host_mask.len()];
    for i in 0..host_mask.len() {
        inside_mask[i] = host_mask[i] && intent_mask[i];
        remainder_mask[i] = host_mask[i] && !intent_mask[i];
    }
    if !inside_mask.iter().any(|&b| b) {
        return Err(DivideError::NotInsideHost);
    }
    let core = division_core(
        host_outer,
        host_holes,
        &inside_mask,
        &remainder_mask,
        grid_w,
        grid_h,
    )?;
    let closed = core
        .spliced
        .as_ref()
        .map(|p| p.is_closed_cut())
        .unwrap_or(false);
    Ok(if closed {
        finish_division(&core, smooth_ring)
    } else {
        finish_division(&core, smooth_chain)
    })
}

// ---------------------------------------------------------------------------
// 贴地吸附（ADR-0028 D5）
// ---------------------------------------------------------------------------

/// 边界模式：贴地吸附地物线 / 越地（不做任何吸附）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeMode {
    /// 边框顶点吸附到最近的地物线（水陆界面 / 河流骨架 / 山脊峰值）。
    Snap,
    /// 无视地貌：保留栅格提取的原始边框（不吸附）。
    Straight,
}

/// 从地形栅格派生的地物线集合（格坐标）。
///
/// 三族（GLOSSARY「地物线」）：水陆界面（海岸 + 湖岸）、河流骨架点、
/// 山脊峰值带。全部纯派生、不落盘。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Features {
    /// 水陆界面的链（每条是点列）。
    pub water_rings: Vec<Vec<(f32, f32)>>,
    /// 河流骨架点：湿格且 4 邻湿数 ≤ 2（细水链的格心）。
    pub river_points: Vec<(f32, f32)>,
    /// 山脊峰值带：3×3 局部极大且海拔 ≥ 300 m 的格心。
    pub ridge_points: Vec<(f32, f32)>,
}

/// 山脊峰值带的最低海拔（米）——与大陆分带 `HILL_MIN_M` 同源。
const RIDGE_MIN_M: i16 = 300;

/// 从地形栅格派生贴地目标（确定性；只扫一遍）。
pub fn extract_features(grid: &TerrainGrid) -> Features {
    let (w, h) = (grid.width(), grid.height());
    // 水陆界面：湿判定二值场上的异类边（kind_segments）+ 链装配。
    let wet = |x: u32, y: u32| -> u8 {
        grid.get(x as i32, y as i32)
            .map(|c| u8::from(crate::edit::is_water_cell(c)))
            .unwrap_or(0)
    };
    let segs = crate::isoline::kind_segments(w, h, wet);
    let water_rings: Vec<Vec<(f32, f32)>> = crate::isoline::assemble_chains(&segs)
        .into_iter()
        .map(|c| c.pts.into_iter().map(|p| (p.x, p.y)).collect())
        .collect();

    let mut river_points = Vec::new();
    let mut ridge_points = Vec::new();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let Some(cell) = grid.get(x, y) else { continue };
            if crate::edit::is_water_cell(cell) {
                let wet_neighbors = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter(|(dx, dy)| {
                        grid.get(x + dx, y + dy)
                            .map(crate::edit::is_water_cell)
                            .unwrap_or(false)
                    })
                    .count();
                if wet_neighbors <= 2 {
                    river_points.push((x as f32 + 0.5, y as f32 + 0.5));
                }
            } else if cell.height >= RIDGE_MIN_M {
                // 「3×3 局部极大 **且至少一个邻居严格更低**」：高原/平台内部
                // 每格都同高，不加严格项会把整片高原当山脊（候选爆炸 +
                // 「山脊线」语义错成「高原轮廓」）。
                let mut is_local_max = true;
                let mut has_lower = false;
                'scan: for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        if let Some(n) = grid.get(x + dx, y + dy) {
                            if n.height > cell.height {
                                is_local_max = false;
                                break 'scan;
                            }
                            if n.height < cell.height {
                                has_lower = true;
                            }
                        }
                    }
                }
                if is_local_max && has_lower {
                    ridge_points.push((x as f32 + 0.5, y as f32 + 0.5));
                }
            }
        }
    }
    Features {
        water_rings,
        river_points,
        ridge_points,
    }
}

/// 按固定步长重采样环（加密）：逐段线性插值，步长 ≤ `step`。
///
/// **为什么必须加密再吸附**：环经 DP 抽稀后常退化成 4 个角点，逐点吸附
/// 只有角能碰到地物线，整条边永远贴不上岸/河/脊。加密到亚格步长后，
/// 边上的顶点各有机会吸附。
fn densify_ring(ring: &[(f32, f32)], step: f32) -> Vec<(f32, f32)> {
    let n = ring.len();
    if n < 2 || step <= 0.0 {
        return ring.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let subdiv = (len / step).ceil().max(1.0) as usize;
        for k in 0..subdiv {
            let t = k as f32 / subdiv as f32;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
}

/// 吸附：每个顶点移到 `search` 格范围内最近的地物点；范围外原样保留。
///
/// 无候选（纯平地）→ 逐点原样返回（Q2 退化 = 直线，由调用方给提示）。
pub fn snap_ring(ring: &[(f32, f32)], features: &Features, search: f32) -> Vec<(f32, f32)> {
    let mut candidates: Vec<(f32, f32)> = Vec::new();
    for ring_pts in &features.water_rings {
        candidates.extend(ring_pts.iter().copied());
    }
    candidates.extend(features.river_points.iter().copied());
    candidates.extend(features.ridge_points.iter().copied());
    if candidates.is_empty() {
        return ring.to_vec();
    }
    // 候选按格分桶（零依赖空间索引）：大地图上水陆界面动辄 10^5 点，
    // 逐顶点全扫是 O(V×F) 的点击卡顿源。桶键 = 候选点所在格。
    let mut buckets: std::collections::HashMap<(i32, i32), Vec<(f32, f32)>> =
        std::collections::HashMap::new();
    for &(cx, cy) in &candidates {
        buckets
            .entry((cx.floor() as i32, cy.floor() as i32))
            .or_default()
            .push((cx, cy));
    }
    let span = search.ceil() as i32;
    ring.iter()
        .map(|&(vx, vy)| {
            let (bx, by) = (vx.floor() as i32, vy.floor() as i32);
            let mut best = (vx, vy);
            let mut best_d2 = search * search;
            for dy in -span..=span {
                for dx in -span..=span {
                    let Some(list) = buckets.get(&(bx + dx, by + dy)) else {
                        continue;
                    };
                    for &(cx, cy) in list {
                        let d2 = (cx - vx).powi(2) + (cy - vy).powi(2);
                        if d2 <= best_d2 {
                            best_d2 = d2;
                            best = (cx, cy);
                        }
                    }
                }
            }
            best
        })
        .collect()
}

fn ring_area(ring: &[(f32, f32)]) -> f32 {
    ring_signed_area(ring).abs()
}

/// 划分 + 贴地（组合入口，ADR-0028 D5/D2）。
///
/// 先栅格几何（[`divide_geometry`]），再对**新切出的那条边**做吸附——
/// 只动新边，宿主的外沿与既有洞一律不碰（Q1 不级联的几何侧）。
/// 贴地时余部里与新势力对应的洞被替换为**反序的新势力环**，保证共边逐点
/// 相等（D3）；越地（[`EdgeMode::Straight`]）跳过吸附。
pub fn divide(
    grid: &TerrainGrid,
    host_outer: &[(f32, f32)],
    host_holes: &[Vec<(f32, f32)>],
    intent: &[(f32, f32)],
    mode: EdgeMode,
    search_cells: f32,
) -> Result<DivisionGeometry, DivideError> {
    let (w, h) = (grid.width(), grid.height());
    if intent.len() < 3 || host_outer.len() < 3 {
        return Err(DivideError::TooFewPoints);
    }
    let intent_mask = rasterize_ring(w, h, intent);
    let host_mask = rasterize_host(w, h, host_outer, host_holes);
    let (inside_mask, remainder_mask) = split_masks(&host_mask, &intent_mask);
    if !inside_mask.iter().any(|&b| b) {
        return Err(DivideError::NotInsideHost);
    }
    let core = division_core(host_outer, host_holes, &inside_mask, &remainder_mask, w, h)?;

    // M2：拼接路径只吸附**切口弧**（宿主外沿逐点不碰）；兜底路径沿用旧行为。
    if let Some(plan) = &core.spliced {
        let (cut, moved) =
            if mode == EdgeMode::Snap && search_cells.is_finite() && search_cells > 0.0 {
                let features = extract_features(grid);
                snap_cut_arc(&plan.cut, &features, search_cells, host_outer, host_holes)
            } else {
                (smooth_chain(&plan.cut), false)
            };
        if let Some(mut geo) = build_spliced(plan, &cut) {
            if area_is_sane(&geo, &core) {
                geo.snapped = moved;
                return Ok(geo);
            }
        }
    }

    // I3（终审）：兜底必须走**旧路径几何**，不能再让 finish_division 取一次拼接
    // 结果——否则吸附后的 inside 与未吸附的余部不共边，且 mirror 会误配既有洞。
    let mut geo = legacy_geometry(&core);
    let raw_inside_area = geo.inside.first().map(|r| ring_area(r)).unwrap_or(0.0);
    if mode == EdgeMode::Snap && search_cells.is_finite() && search_cells > 0.0 {
        let features = extract_features(grid);
        for ring in &mut geo.inside {
            let dense = densify_ring(ring, (search_cells * 0.5).max(0.25));
            let snapped = snap_ring(&dense, &features, search_cells);
            // 钳制（M3）：吸附点必须仍在宿主内——否则切边会被拉到宿主外，
            // 新势力越界压到邻国。越界的顶点退回未吸附位置。
            let clamped: Vec<(f32, f32)> = snapped
                .iter()
                .zip(dense.iter())
                .map(|(&(sx, sy), &raw)| {
                    let inside_host = crate::intent::point_in_polygon((sx, sy), host_outer)
                        && !host_holes
                            .iter()
                            .any(|h| crate::intent::point_in_polygon((sx, sy), h));
                    if inside_host {
                        (sx, sy)
                    } else {
                        raw
                    }
                })
                .collect();
            let deduped = dedup_ring(&clamped);
            geo.snapped |= deduped != dense;
            *ring = deduped;
        }
    }
    mirror_shared_edge(&mut geo, raw_inside_area, host_holes);
    Ok(geo)
}

/// 去掉相邻重复点（吸附可能把多个顶点吸到同一个地物点）。
fn dedup_ring(ring: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(ring.len());
    for &p in ring {
        if out.last().map(|q| *q != p).unwrap_or(true) {
            out.push(p);
        }
    }
    // 首尾重复也要去（闭合环）。
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// 共边硬约束（D3）：把余部中「与新势力同一条边」的洞换成**反序的新势力环**。
///
/// 两种模式都做——inside 与 remainder 来自两张不同掩膜的边界提取，平滑后
/// 只是「近似相同」；不镜像就不是严格共边。匹配按面积最近（两者本出自
/// 同一掩膜边界，面积差只来自平滑）；容差 25% 防止误配既有洞。
fn mirror_shared_edge(
    geo: &mut DivisionGeometry,
    raw_inside_area: f32,
    preserved_holes: &[Vec<(f32, f32)>],
) {
    let Some(holes) = geo.remainder_holes.first_mut() else {
        return;
    };
    // 终审 I3：不得把宿主**原有**的洞当成「本次切出的洞」替换掉——面积接近时
    // 会误配（作者手画的国中国凭空变成新势力形状）。按点列近似相等排除。
    let is_preserved = |h: &[(f32, f32)]| -> bool {
        preserved_holes.iter().any(|p| {
            p.len() == h.len()
                && p.iter()
                    .zip(h.iter())
                    .all(|(a, b)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3)
        })
    };
    let Some(idx) = holes
        .iter()
        .enumerate()
        .filter(|(_, h)| !is_preserved(h))
        .map(|(i, h)| (i, (ring_area(h) - raw_inside_area).abs()))
        .filter(|(_, d)| *d <= raw_inside_area * 0.25)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
    else {
        return;
    };
    let mut reversed = geo.inside[0].clone();
    reversed.reverse();
    holes[idx] = reversed;
}

// ---------------------------------------------------------------------------
// M2：切弧拼接（共边逐点保真，ADR-0028 追记）
// ---------------------------------------------------------------------------

/// 拼接容差（格）：原始掩膜边界顶点离宿主原环多近，才算「沿宿主边界走」。
///
/// 原始掩膜边界是格边阶梯；宿主环可能已经被上一轮平滑，两者天然差 ~0.5 格。
/// 1 格留余量；误判由 [`area_is_sane`] 兜底（差异过大 → 回退旧路径）。
const HOST_ARC_TOL: f32 = 1.0;

/// 宿主环上的位置：段下标 `seg`（`ring[seg] → ring[seg+1]`）+ 段内比例。
#[derive(Debug, Clone, Copy, PartialEq)]
struct RingPos {
    seg: usize,
    t: f32,
}

impl RingPos {
    fn point(&self, ring: &[(f32, f32)]) -> (f32, f32) {
        let a = ring[self.seg];
        let b = ring[(self.seg + 1) % ring.len()];
        (a.0 + (b.0 - a.0) * self.t, a.1 + (b.1 - a.1) * self.t)
    }

    /// 环上标量位置（段号 + 段内比例）。
    fn scalar(&self) -> f32 {
        self.seg as f32 + self.t
    }
}

/// 环序里的一段：宿主原环弧（逐点照抄）或切口弧（原始掩膜点列）。
#[derive(Debug, Clone, PartialEq)]
enum ArcSeg {
    /// 宿主原环上**正向**（沿宿主环推进方向）从 `from` 到 `to` 的一段。
    /// `reverse = true` 表示写进新势力环时按反序（宿主环与掩膜环反向时）。
    Host {
        from: RingPos,
        to: RingPos,
        reverse: bool,
    },
    /// 切口弧（原始掩膜阶梯，含两端宿主投影点）。
    Cut(Vec<(f32, f32)>),
}

/// 拼接方案：环序段 + 唯一切口弧 + 未被触碰的宿主洞。
#[derive(Debug, Clone)]
struct SplicedDivision {
    segs: Vec<ArcSeg>,
    /// 唯一切口弧的原始点列（含两端宿主投影点）。
    cut: Vec<(f32, f32)>,
    /// 宿主原有、未被本次意图触碰的洞（余部原样保留）。
    preserved_holes: Vec<Vec<(f32, f32)>>,
    /// 宿主原始外环（拼接时逐点拷贝的来源）。
    host_outer: Vec<(f32, f32)>,
}

impl SplicedDivision {
    /// 意图完全含于宿主（无宿主弧）：切口是一条**闭环**，平滑要按闭链做。
    fn is_closed_cut(&self) -> bool {
        !self.segs.iter().any(|s| matches!(s, ArcSeg::Host { .. }))
    }
}

/// 划分核心：拼接方案（可空）+ 旧路径所需的平滑几何。
struct DivisionCore {
    spliced: Option<SplicedDivision>,
    inside: Vec<Ring>,
    inside_holes: Vec<Vec<Ring>>,
    remainder_outer: Vec<Ring>,
    remainder_holes: Vec<Vec<Ring>>,
}

/// 宿主掩膜 = 外环 − 既有洞。
fn rasterize_host(
    w: u32,
    h: u32,
    host_outer: &[(f32, f32)],
    host_holes: &[Vec<(f32, f32)>],
) -> Mask {
    let mut host = rasterize_ring(w, h, host_outer);
    for hole in host_holes {
        let hm = rasterize_ring(w, h, hole);
        for i in 0..host.len() {
            if hm[i] {
                host[i] = false;
            }
        }
    }
    host
}

/// 宿主掩膜 × 意图掩膜 →（新势力掩膜, 余部掩膜）。
fn split_masks(host: &[bool], intent: &[bool]) -> (Mask, Mask) {
    let mut inside = vec![false; host.len()];
    let mut remainder = vec![false; host.len()];
    for i in 0..host.len() {
        inside[i] = host[i] && intent[i];
        remainder[i] = host[i] && !intent[i];
    }
    (inside, remainder)
}

/// 点在环（折线，隐式闭合）上的最近投影：`(距离, 位置)`。
fn project_on_ring(p: (f32, f32), ring: &[(f32, f32)]) -> Option<(f32, RingPos)> {
    let n = ring.len();
    if n < 3 {
        return None;
    }
    let mut best: Option<(f32, RingPos)> = None;
    for seg in 0..n {
        let a = ring[seg];
        let b = ring[(seg + 1) % n];
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= 1e-12 {
            0.0
        } else {
            (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
        };
        let q = (a.0 + dx * t, a.1 + dy * t);
        let d = ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
        if best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, RingPos { seg, t }));
        }
    }
    best
}

/// 宿主环上 `from → to` 的**正向**弧点列（含两端投影点与中间全部原顶点）。
fn host_arc_points(ring: &[(f32, f32)], from: RingPos, to: RingPos) -> Vec<(f32, f32)> {
    let n = ring.len();
    if n < 3 {
        return Vec::new();
    }
    let span = (to.scalar() - from.scalar()).rem_euclid(n as f32);
    let mut out = vec![from.point(ring)];
    if span <= 1e-6 {
        return out;
    }
    let end = from.scalar() + span;
    let mut k = 1usize;
    while k <= n + 1 {
        let abs = (from.seg + k) as f32;
        if abs >= end - 1e-6 {
            break;
        }
        out.push(ring[(from.seg + k) % n]);
        k += 1;
    }
    out.push(to.point(ring));
    out
}

/// 近段在宿主环上的推进方向（按投影点标量位置的多数票）。
fn run_is_forward(run: &[usize], proj: &[RingPos], n: usize) -> bool {
    if run.len() < 2 {
        return true;
    }
    let (mut f, mut b) = (0.0f32, 0.0f32);
    for w in run.windows(2) {
        let s0 = proj[w[0]].scalar();
        let s1 = proj[w[1]].scalar();
        let d = (s1 - s0).rem_euclid(n as f32);
        if d <= n as f32 / 2.0 {
            f += d;
        } else {
            b += n as f32 - d;
        }
    }
    f >= b
}

/// 原始掩膜环 → 环序段（宿主弧 / 切口弧）。分类不明确返回 `None`（走兜底）。
fn classify_against_host(raw: &[(f32, f32)], host: &[(f32, f32)]) -> Option<Vec<ArcSeg>> {
    let n = raw.len();
    if n < 3 || host.len() < 3 {
        return None;
    }
    let mut proj = Vec::with_capacity(n);
    let mut near = Vec::with_capacity(n);
    for &p in raw {
        let (d, pos) = project_on_ring(p, host)?;
        near.push(d < HOST_ARC_TOL);
        proj.push(pos);
    }
    // 整条环都贴着宿主 = 意图吃光宿主：交兜底（旧路径给空余部）。
    if near.iter().all(|&b| b) {
        return None;
    }
    // 整条环都在宿主内部 = 意图完全含于宿主（洞路径）：单段切口。
    if !near.iter().any(|&b| b) {
        return Some(vec![ArcSeg::Cut(raw.to_vec())]);
    }
    // 从「近/远」跃变处起扫，保证切口弧与宿主弧交替。
    let start = (0..n).find(|&i| near[i] != near[(i + n - 1) % n])?;
    let order: Vec<usize> = (0..n).map(|k| (start + k) % n).collect();

    // 用最长的近段定全局方向（短段投票不可靠）。
    let mut direction = true;
    let mut best_len = 0usize;
    let mut k = 0;
    while k < n {
        if near[order[k]] {
            let mut j = k;
            while j < n && near[order[j]] {
                j += 1;
            }
            if j - k > best_len {
                best_len = j - k;
                let idx: Vec<usize> = order[k..j].to_vec();
                direction = run_is_forward(&idx, &proj, host.len());
            }
            k = j;
        } else {
            k += 1;
        }
    }

    let mut segs = Vec::new();
    let mut k = 0;
    while k < n {
        if near[order[k]] {
            let mut j = k;
            while j < n && near[order[j]] {
                j += 1;
            }
            let (from, to) = (proj[order[k]], proj[order[j - 1]]);
            segs.push(if direction {
                ArcSeg::Host {
                    from,
                    to,
                    reverse: false,
                }
            } else {
                ArcSeg::Host {
                    from: to,
                    to: from,
                    reverse: true,
                }
            });
            k = j;
        } else {
            let mut j = k;
            while j < n && !near[order[j]] {
                j += 1;
            }
            // 切口弧 = 入口投影 + 原始阶梯点 + 出口投影（与宿主弧端点逐点重合）。
            let mut pts = Vec::new();
            pts.push(proj[order[(k + n - 1) % n]].point(host));
            for &idx in &order[k..j] {
                pts.push(raw[idx]);
            }
            pts.push(proj[order[j % n]].point(host));
            segs.push(ArcSeg::Cut(pts));
            k = j;
        }
    }
    Some(segs)
}

/// 切口弧平滑（开链：chaikin 2 轮 + DP 0.5 格，端点保留）。
fn smooth_chain(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let p: Vec<Pt> = pts.iter().map(|&(x, y)| Pt { x, y }).collect();
    simplify_dp(&chaikin(&p, false, 2), false, 0.5)
        .into_iter()
        .map(|p| (p.x, p.y))
        .collect()
}

/// 构建划分核心：先尝试拼接（M2），失败/不明确则走旧路径的平滑几何。
fn division_core(
    host_outer: &[(f32, f32)],
    host_holes: &[Vec<(f32, f32)>],
    inside_mask: &[bool],
    remainder_mask: &[bool],
    w: u32,
    h: u32,
) -> Result<DivisionCore, DivideError> {
    let inside_raw = raw_mask_boundary_rings(inside_mask, w, h);
    let remainder_raw = raw_mask_boundary_rings(remainder_mask, w, h);
    let (inside_raw_outers, inside_raw_holes) = split_outers_and_holes(inside_raw);
    let (rem_raw_outers, rem_raw_holes) = split_outers_and_holes(remainder_raw);
    if inside_raw_outers.len() != 1 || rem_raw_outers.len() > 1 {
        return Err(DivideError::Fragmented {
            inside: inside_raw_outers.len(),
            remainder: rem_raw_outers.len(),
        });
    }

    let spliced = try_spliced(
        host_outer,
        host_holes,
        &inside_raw_outers[0],
        &inside_raw_holes,
    );
    let smooth_all =
        |rings: &[Ring]| -> Vec<Ring> { rings.iter().map(|r| smooth_ring(r)).collect() };
    let smooth_groups =
        |groups: &[Vec<Ring>]| -> Vec<Vec<Ring>> { groups.iter().map(|g| smooth_all(g)).collect() };
    Ok(DivisionCore {
        spliced,
        inside: smooth_all(&inside_raw_outers),
        inside_holes: smooth_groups(&inside_raw_holes),
        remainder_outer: smooth_all(&rem_raw_outers),
        remainder_holes: smooth_groups(&rem_raw_holes),
    })
}

/// 尝试构造拼接方案；任何不确定都返回 `None`（旧路径兜底）。
fn try_spliced(
    host_outer: &[(f32, f32)],
    host_holes: &[Vec<(f32, f32)>],
    inside_raw: &[(f32, f32)],
    inside_raw_holes: &[Vec<Ring>],
) -> Option<SplicedDivision> {
    // 意图圈住/切到既有洞：洞的分捡超出 v1 拼接范围，交兜底。
    // 注意 `inside_raw_holes` 是**按外环分组**的结构（每组 = 该外环的洞）；
    // 无洞时是 `vec![vec![]]`，不能用 `is_empty()` 判。
    if inside_raw_holes.iter().any(|g| !g.is_empty()) {
        return None;
    }
    // 切口掠过既有洞 → 分类会与洞混淆，交兜底。
    if host_holes.iter().any(|hole| {
        inside_raw.iter().any(|&p| {
            project_on_ring(p, hole)
                .map(|(d, _)| d < HOST_ARC_TOL)
                .unwrap_or(false)
        })
    }) {
        return None;
    }
    let segs = classify_against_host(inside_raw, host_outer)?;
    let cuts: Vec<Vec<(f32, f32)>> = segs
        .iter()
        .filter_map(|s| match s {
            ArcSeg::Cut(p) => Some(p.clone()),
            _ => None,
        })
        .collect();
    // 余部必须保持单块：只支持单切口（多切口 = 多条弦，会把宿主切成多块）。
    if cuts.len() != 1 {
        return None;
    }
    // C1（终审）：宿主弧退化成单点（近段只有 1 个顶点）时「补弧」也是同一个点，
    // 余部会塌缩成新势力本身、宿主静默丢地。整条弧几乎覆盖宿主环时同理（补弧退化）。
    let n = host_outer.len() as f32;
    for seg in &segs {
        if let ArcSeg::Host { from, to, .. } = seg {
            let span = (to.scalar() - from.scalar()).rem_euclid(n);
            if span <= 1e-6 || span >= n - 1e-6 {
                return None;
            }
        }
    }
    let cut = cuts.into_iter().next()?;
    Some(SplicedDivision {
        segs,
        cut,
        preserved_holes: host_holes.to_vec(),
        host_outer: host_outer.to_vec(),
    })
}

/// 切口环序位置：返回（切口前宿主弧的发射终点, 切口后宿主弧的发射起点, 是否反序）。
///
/// 单切口时必然恰有一个 Host 段，prev 与 next 是同一段；`reverse` 指出该段
/// 相对宿主环正向是否反序发射（宿主环与掩膜环 winding 相反时为 true）。
fn cut_neighbors(segs: &[ArcSeg]) -> Option<(RingPos, RingPos, bool)> {
    let ci = segs.iter().position(|s| matches!(s, ArcSeg::Cut(_)))?;
    let prev = &segs[(ci + segs.len() - 1) % segs.len()];
    let next = &segs[(ci + 1) % segs.len()];
    let ArcSeg::Host {
        from: pf,
        to: pt,
        reverse: pr,
    } = prev
    else {
        return None;
    };
    let ArcSeg::Host {
        from: nf,
        to: nt,
        reverse: nr,
    } = next
    else {
        return None;
    };
    // prev 的发射终点：reverse 时发射 from，否则发射 to。
    // next 的发射起点：reverse 时发射 to，否则发射 from。
    Some((
        if *pr { *pf } else { *pt },
        if *nr { *nt } else { *nf },
        *pr && *nr,
    ))
}

/// 由拼接方案组装最终几何（切口弧已平滑/吸附）。
fn build_spliced(plan: &SplicedDivision, cut: &[(f32, f32)]) -> Option<DivisionGeometry> {
    let host = &plan.host_outer;
    let mut inside_ring: Vec<(f32, f32)> = Vec::new();
    for seg in &plan.segs {
        match seg {
            ArcSeg::Host { from, to, reverse } => {
                let mut arc = host_arc_points(host, *from, *to);
                if *reverse {
                    arc.reverse();
                }
                inside_ring.extend(arc);
            }
            ArcSeg::Cut(_) => inside_ring.extend(cut.iter().copied()),
        }
    }
    let inside_ring = dedup_ring(&inside_ring);
    if inside_ring.len() < 3 || ring_area(&inside_ring) <= 0.0 {
        return None;
    }

    let has_host_arc = plan.segs.iter().any(|s| matches!(s, ArcSeg::Host { .. }));
    let (remainder_outer, remainder_holes) = if !has_host_arc {
        // 意图完全含于宿主：宿主原环原样保留 + 新势力的洞（反序）。
        let mut rev = inside_ring.clone();
        rev.reverse();
        let mut holes = vec![rev];
        holes.extend(plan.preserved_holes.iter().cloned());
        (host.to_vec(), holes)
    } else {
        let (cut_start, cut_end, reverse) = cut_neighbors(&plan.segs)?;
        // 余部 = 宿主**补弧** + 反序切口。
        // 正向 winding：补弧 = forward(cut_start → cut_end)。
        // 反向 winding（C2，终审）：inside 用的是 forward(from_s → to_s)，补弧是
        // 另一条弧 = forward(cut_end → cut_start) **反序**发射，否则取回的是
        // inside 自己那条弧，余部塌缩。
        let mut rem: Vec<(f32, f32)> = if reverse {
            let mut arc = host_arc_points(host, cut_end, cut_start);
            arc.reverse();
            arc
        } else {
            host_arc_points(host, cut_start, cut_end)
        };
        rem.extend(cut.iter().rev().copied());
        let rem = dedup_ring(&rem);
        if rem.len() < 3 || ring_area(&rem) <= 0.0 {
            return None;
        }
        (rem, plan.preserved_holes.clone())
    };

    Some(DivisionGeometry {
        inside: vec![inside_ring],
        inside_holes: vec![Vec::new()],
        remainder_outer: vec![remainder_outer],
        remainder_holes: vec![remainder_holes],
        snapped: false,
    })
}

/// 面积校验：拼接环与旧路径环差异过大 = 分类误判（拼接环不可信）。
///
/// 终审 I1：**两端都查**（inside 与余部净面积）——只查 inside 会漏掉
/// 「inside 对、余部塌缩」的错误（C1/C2 就属于这一类）。
fn area_is_sane(geo: &DivisionGeometry, core: &DivisionCore) -> bool {
    let net = |outer: &[Ring], holes: &[Vec<Ring>]| -> Option<f32> {
        let outer_area = outer.first().map(|r| ring_area(r))?;
        let hole_area: f32 = holes
            .first()
            .map(|hs| hs.iter().map(|h| ring_area(h)).sum())
            .unwrap_or(0.0);
        Some(outer_area - hole_area)
    };
    let built_inside = net(&geo.inside, &geo.inside_holes);
    let legacy_inside = net(&core.inside, &core.inside_holes);
    let (Some(bi), Some(li)) = (built_inside, legacy_inside) else {
        return false;
    };
    if bi <= 0.0 || li <= 0.0 || (bi - li).abs() > li * 0.35 {
        return false;
    }
    // 余部：任一侧为空时要求另一侧也为空（吃光宿主）。
    match (
        net(&geo.remainder_outer, &geo.remainder_holes),
        net(&core.remainder_outer, &core.remainder_holes),
    ) {
        (None, None) => true,
        (Some(br), Some(lr)) => br > 0.0 && lr > 0.0 && (br - lr).abs() <= lr * 0.35,
        _ => false,
    }
}

/// 旧路径几何（拼接不可用时使用；不含任何吸附）。
fn legacy_geometry(core: &DivisionCore) -> DivisionGeometry {
    DivisionGeometry {
        inside: core.inside.clone(),
        inside_holes: core.inside_holes.clone(),
        remainder_outer: core.remainder_outer.clone(),
        remainder_holes: core.remainder_holes.clone(),
        snapped: false,
    }
}

/// 组装最终几何：拼接可用且面积合理 → 拼接；否则旧路径。
fn finish_division(
    core: &DivisionCore,
    map_cut: impl Fn(&[(f32, f32)]) -> Vec<(f32, f32)>,
) -> DivisionGeometry {
    if let Some(plan) = &core.spliced {
        let cut = map_cut(&plan.cut);
        if let Some(geo) = build_spliced(plan, &cut) {
            if area_is_sane(&geo, core) {
                return geo;
            }
        }
    }
    DivisionGeometry {
        inside: core.inside.clone(),
        inside_holes: core.inside_holes.clone(),
        remainder_outer: core.remainder_outer.clone(),
        remainder_holes: core.remainder_holes.clone(),
        snapped: false,
    }
}

/// 吸附切口弧：只动**内部**顶点（两端宿主投影点必须保持逐点重合）。
///
/// 返回（吸附后的弧, 是否真的吸附到地物）。
fn snap_cut_arc(
    cut: &[(f32, f32)],
    features: &Features,
    search: f32,
    host_outer: &[(f32, f32)],
    host_holes: &[Vec<(f32, f32)>],
) -> (Vec<(f32, f32)>, bool) {
    if cut.len() < 3 {
        return (cut.to_vec(), false);
    }
    // 开链加密（端点保留）：段内步长 ≤ search/2。
    let step = (search * 0.5).max(0.25);
    let mut dense: Vec<(f32, f32)> = Vec::new();
    for (i, &a) in cut.iter().enumerate() {
        dense.push(a);
        if let Some(&b) = cut.get(i + 1) {
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let subdiv = (len / step).ceil().max(1.0) as usize;
            for k in 1..subdiv {
                let t = k as f32 / subdiv as f32;
                dense.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
    }
    let snapped = snap_ring(&dense, features, search);
    let last = dense.len() - 1;
    let mut moved = false;
    let clamped: Vec<(f32, f32)> = snapped
        .iter()
        .zip(dense.iter())
        .enumerate()
        .map(|(i, (&(sx, sy), &raw))| {
            if i == 0 || i == last {
                return raw; // 端点 = 宿主投影，不许移动
            }
            let inside_host = crate::intent::point_in_polygon((sx, sy), host_outer)
                && !host_holes
                    .iter()
                    .any(|h| crate::intent::point_in_polygon((sx, sy), h));
            if inside_host {
                if (sx, sy) != raw {
                    moved = true;
                }
                (sx, sy)
            } else {
                raw
            }
        })
        .collect();
    // 开链去重（不闭合）。
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(clamped.len());
    for p in clamped {
        if out.last().map(|q| *q != p).unwrap_or(true) {
            out.push(p);
        }
    }
    (out, moved)
}

/// 空势力宿主划分（ADR-0028 D4/Q3）：整张图的**陆格**就是宿主。
///
/// 意图圈住的水域不进入新势力（湖/海归空势力），因此新势力可能带洞。
/// 余部是「未分配的陆」——没有实体承载，**不产出也不写回**（空势力 v1 是
/// 派生补集，不建实体）。
pub fn divide_land_host(
    grid: &TerrainGrid,
    intent: &[(f32, f32)],
    mode: EdgeMode,
    search_cells: f32,
) -> Result<DivisionGeometry, DivideError> {
    let (w, h) = (grid.width(), grid.height());
    if intent.len() < 3 {
        return Err(DivideError::TooFewPoints);
    }
    let intent_mask = rasterize_ring(w, h, intent);
    let mut inside_mask = vec![false; (w * h) as usize];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !point_in_mask(&intent_mask, w, h, x, y) {
                continue;
            }
            let Some(cell) = grid.get(x as i32, y as i32) else {
                continue;
            };
            if crate::edit::is_water_cell(cell) {
                continue;
            }
            inside_mask[(y as u32 * w + x as u32) as usize] = true;
        }
    }
    if !inside_mask.iter().any(|&b| b) {
        return Err(DivideError::NotInsideHost);
    }
    let rings = mask_boundary_rings(&inside_mask, w, h);
    let (mut inside, inside_holes) = split_outers_and_holes(rings);
    if inside.len() != 1 {
        return Err(DivideError::Fragmented {
            inside: inside.len(),
            remainder: 0,
        });
    }
    let mut snapped_any = false;
    if mode == EdgeMode::Snap && search_cells.is_finite() && search_cells > 0.0 {
        let features = extract_features(grid);
        for ring in &mut inside {
            let dense = densify_ring(ring, (search_cells * 0.5).max(0.25));
            let snapped = snap_ring(&dense, &features, search_cells);
            // 钳制到陆地：吸附点落在水域（如湖心）时退回未吸附位置。
            let clamped: Vec<(f32, f32)> = snapped
                .iter()
                .zip(dense.iter())
                .map(|(&(sx, sy), &raw)| {
                    let land = grid
                        .get(sx.floor() as i32, sy.floor() as i32)
                        .map(|c| !crate::edit::is_water_cell(c))
                        .unwrap_or(false);
                    if land {
                        (sx, sy)
                    } else {
                        raw
                    }
                })
                .collect();
            let deduped = dedup_ring(&clamped);
            snapped_any |= deduped != dense;
            *ring = deduped;
        }
    }
    Ok(DivisionGeometry {
        inside,
        inside_holes,
        remainder_outer: Vec::new(),
        remainder_holes: Vec::new(),
        snapped: snapped_any,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    fn perimeter(ring: &[(f32, f32)]) -> f32 {
        let n = ring.len();
        (0..n)
            .map(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
            })
            .sum()
    }

    #[test]
    fn square_intent_inside_square_host_yields_nested_ring_and_ring_with_hole() {
        let host = sq(0.0, 0.0, 20.0, 20.0);
        let intent = sq(6.0, 6.0, 14.0, 14.0);
        let g = divide_geometry(40, 40, &host, &[], &intent).expect("正常划分");
        assert_eq!(g.inside.len(), 1, "新势力恰 1 环");
        assert_eq!(g.remainder_outer.len(), 1, "余部恰 1 外环");
        assert_eq!(g.remainder_holes.len(), 1, "余部带 1 洞");
        assert_eq!(g.remainder_holes[0].len(), 1, "恰 1 个洞环");
        // 共边（D3 的构造性证据）：新势力环周长 ≈ 余部洞沿周长 ≈ 32（4×8），
        // 平滑与抽稀允许 ±4 容差。
        let pi = perimeter(&g.inside[0]);
        let ph = perimeter(&g.remainder_holes[0][0]);
        assert!(
            (pi - ph).abs() < 4.0,
            "共边应等长：inside {pi} vs hole {ph}"
        );
        assert!((pi - 32.0).abs() < 4.0, "8×8 方形周长应 ≈32，实得 {pi}");
    }

    #[test]
    fn intent_touching_host_border_cuts_without_hole() {
        let host = sq(0.0, 0.0, 20.0, 20.0);
        let intent = sq(0.0, 0.0, 10.0, 10.0);
        let g = divide_geometry(40, 40, &host, &[], &intent).expect("切角划分");
        assert_eq!(g.inside.len(), 1);
        assert_eq!(g.remainder_outer.len(), 1);
        assert!(
            g.remainder_holes[0].is_empty(),
            "贴边切不应产生洞，实得 {}",
            g.remainder_holes[0].len()
        );
    }

    #[test]
    fn fragmented_remainder_is_rejected() {
        // 哑铃宿主：两间房由 1 格走廊相连；意图横切走廊 → 余部碎块化。
        let host = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 4.0),
            (11.0, 4.0),
            (11.0, 0.0),
            (21.0, 0.0),
            (21.0, 10.0),
            (11.0, 10.0),
            (11.0, 6.0),
            (10.0, 6.0),
            (10.0, 10.0),
            (0.0, 10.0),
        ];
        let intent = sq(9.0, 0.0, 12.0, 10.0);
        let err = divide_geometry(40, 40, &host, &[], &intent).expect_err("应碎块化");
        assert!(
            matches!(err, DivideError::Fragmented { remainder: 2, .. }),
            "余部应恰 2 块，实得 {err:?}"
        );
    }

    #[test]
    fn intent_outside_host_is_not_inside_host_error() {
        let host = sq(0.0, 0.0, 10.0, 10.0);
        let intent = sq(15.0, 15.0, 20.0, 20.0);
        let err = divide_geometry(40, 40, &host, &[], &intent).expect_err("应不在宿主内");
        assert_eq!(err, DivideError::NotInsideHost);
    }

    #[test]
    fn degenerate_inputs_error_not_panic() {
        let host = sq(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            divide_geometry(40, 40, &host, &[], &[(0.0, 0.0), (1.0, 1.0)]),
            Err(DivideError::TooFewPoints)
        );
        assert_eq!(
            divide_geometry(40, 40, &[], &[], &sq(0.0, 0.0, 1.0, 1.0)),
            Err(DivideError::TooFewPoints)
        );
    }

    // ---- 贴地吸附（Task 4） ----

    fn flat_land(w: u32, h: u32, height: i16) -> crate::TerrainGrid {
        let mut g = crate::TerrainGrid::new(w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                g.set(
                    x,
                    y,
                    crate::TerrainCell {
                        height,
                        surface: crate::SURFACE_LAND,
                        biome: crate::BIOME_GRASSLAND,
                    },
                );
            }
        }
        g
    }

    fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    #[test]
    fn lake_shore_snaps_border_to_water_interface() {
        let mut grid = flat_land(64, 64, 100);
        // 居中湖（半径 8，碗形挖深）：水陆界面约在半径 8 处。
        crate::intent::apply_intent(
            &mut grid,
            &crate::intent::Intent::Lake {
                center: (32.0, 32.0),
                radius: 8.0,
                strength: 1.0,
            },
            13,
            1.0,
        );
        let features = extract_features(&grid);
        assert!(!features.water_rings.is_empty(), "湖岸应产出水陆界面链");
        let host = sq(4.0, 4.0, 60.0, 60.0);
        // 意图贴着湖的东岸切一刀：东岸 x 从实际界面推出（湖形有域扭曲，
        // 不能硬编码），留 1 格间隙确保在吸附窗口内。
        let shore_x = features
            .water_rings
            .iter()
            .flatten()
            .map(|p| p.0)
            .fold(f32::MIN, f32::max);
        let x0 = shore_x + 1.0;
        let intent = sq(x0, 16.0, x0 + 12.0, 48.0);

        let snapped =
            divide(&grid, &host, &[], &intent, EdgeMode::Snap, 2.0).expect("贴地划分应成功");
        let straight =
            divide(&grid, &host, &[], &intent, EdgeMode::Straight, 2.0).expect("直线划分应成功");
        assert_ne!(
            snapped.inside[0], straight.inside[0],
            "贴地环应与直线环不同（吸附确实发生）"
        );
        // 至少一个顶点落在某个水陆界面点 1 格内。
        let near_shore = snapped.inside[0].iter().any(|v| {
            features
                .water_rings
                .iter()
                .flatten()
                .any(|p| dist(*v, *p) <= 1.0)
        });
        assert!(near_shore, "贴地环至少一个顶点应吸到湖岸");
        // 共边硬约束在贴地模式下仍成立：余部洞沿 == 反序的新势力环。
        let hole = &snapped.remainder_holes[0][0];
        let mut rev = snapped.inside[0].clone();
        rev.reverse();
        assert_eq!(hole, &rev, "贴地后余部洞沿必须是同一条环（D3）");
    }

    #[test]
    fn snapping_never_pulls_vertices_outside_the_host() {
        // 湖在宿主**外**（西侧）：贴地窗口够得着，但钳制必须把越界的吸附
        // 退回原位——否则切边被拉出宿主、新势力压到邻国（终审 M3）。
        let mut grid = flat_land(64, 64, 100);
        crate::intent::apply_intent(
            &mut grid,
            &crate::intent::Intent::Lake {
                center: (8.0, 32.0),
                radius: 5.0,
                strength: 1.0,
            },
            13,
            1.0,
        );
        let host = sq(14.0, 12.0, 52.0, 52.0);
        let intent = sq(15.0, 16.0, 34.0, 48.0);
        let g = divide(&grid, &host, &[], &intent, EdgeMode::Snap, 6.0).expect("划分");
        for &(x, y) in &g.inside[0] {
            assert!(
                (14.0 - 0.01..=52.0 + 0.01).contains(&x)
                    && (12.0 - 0.01..=52.0 + 0.01).contains(&y),
                "吸附后顶点 ({x},{y}) 越出宿主 14..52 / 12..52"
            );
        }
    }

    #[test]
    fn no_feature_nearby_falls_back_to_straight_ring() {
        // 纯平地（无水、无 ≥300 m 山）：吸附无目标 → 环逐点不变，划分照常成功。
        let grid = flat_land(48, 48, 100);
        let features = extract_features(&grid);
        let ring = vec![(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 20.0)];
        let snapped = snap_ring(&ring, &features, 2.0);
        assert_eq!(snapped, ring, "无地物时环应原样返回（Q2 退化）");

        let host = sq(4.0, 4.0, 44.0, 44.0);
        let intent = sq(10.0, 10.0, 30.0, 30.0);
        assert!(
            divide(&grid, &host, &[], &intent, EdgeMode::Snap, 2.0).is_ok(),
            "无地物不得阻塞划分（V1）"
        );
    }

    #[test]
    fn ridge_peak_attracts_border_vertex() {
        let mut grid = flat_land(48, 48, 0);
        crate::intent::apply_intent(
            &mut grid,
            &crate::intent::Intent::MountainRange {
                pts: vec![(24.0, 24.0)],
                radius: 5.0,
                strength: 1.0,
            },
            7,
            1.0,
        );
        let features = extract_features(&grid);
        assert!(!features.ridge_points.is_empty(), "单峰应产出脊点");
        let peak = features
            .ridge_points
            .iter()
            .min_by(|a, b| {
                dist(**a, (24.5, 24.5))
                    .partial_cmp(&dist(**b, (24.5, 24.5)))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .expect("非空");
        assert!(
            dist(peak, (24.5, 24.5)) < 3.0,
            "最高峰应在山心附近，实得 {peak:?}"
        );
        // 顶点在峰周围 3.5 格内 → 吸附到峰点。
        let ring = vec![(22.0, 22.0), (27.0, 22.0), (27.0, 27.0), (22.0, 27.0)];
        let snapped = snap_ring(&ring, &features, 4.0);
        assert!(
            snapped.iter().any(|p| dist(*p, peak) < 1.0),
            "至少一个顶点应被吸到峰点，实得 {snapped:?}"
        );
    }

    // ---- M2 终审回归：winding / 退化宿主弧 / 吸附共边 / 既有洞 ----

    /// 顺时针 winding 的宿主（YAML 合法输入）：余部必须取**补弧**，不得塌缩。
    #[test]
    fn clockwise_host_winding_keeps_remainder_intact() {
        let host = vec![(0.0, 0.0), (0.0, 20.0), (20.0, 20.0), (20.0, 0.0)];
        let intent = sq(0.0, 0.0, 10.0, 10.0);
        let g = divide_geometry(40, 40, &host, &[], &intent).expect("顺时针宿主也应可划分");
        let rem = ring_area(&g.remainder_outer[0]).abs();
        assert!(
            (rem - 300.0).abs() < 20.0,
            "顺时针宿主的余部应 ≈ 400−100 = 300，实得 {rem}（塌缩说明补弧取错）"
        );
        assert!(
            g.remainder_holes[0].is_empty(),
            "贴边切不应产生洞，实得 {:?}",
            g.remainder_holes[0]
        );
    }

    /// 斜边宿主 + 很小的贴边意图：近段可能只有单个顶点（宿主弧退化）——
    /// 必须回退旧路径，余部不得塌缩成新势力。
    #[test]
    fn slanted_host_tiny_edge_cut_never_collapses_remainder() {
        let host = vec![(8.0, 0.0), (24.0, 4.0), (20.0, 20.0), (4.0, 16.0)];
        let intent = vec![(8.21, 9.29), (16.37, 9.29), (16.37, 16.16), (8.21, 16.16)];
        let host_area = ring_area(&host);
        let g = divide_geometry(40, 40, &host, &[], &intent).expect("斜边宿主应可划分");
        let inside = ring_area(&g.inside[0]);
        let rem = ring_area(&g.remainder_outer[0]).abs();
        assert!(
            rem > host_area * 0.5,
            "余部不得塌缩（宿主 {host_area}，inside {inside}，余部 {rem}）"
        );
        assert!(
            (inside + rem - host_area).abs() < host_area * 0.25,
            "面积应守恒：inside {inside} + 余部 {rem} ≈ 宿主 {host_area}"
        );
    }

    /// 贴地（Snap）模式的贴边切割：吸附只动切口，两侧切口弧仍逐点互为反序。
    #[test]
    fn snapped_edge_cut_keeps_shared_arc_pointwise() {
        let mut grid = flat_land(64, 64, 100);
        // 湖在切口附近，保证吸附真的发生。
        crate::intent::apply_intent(
            &mut grid,
            &crate::intent::Intent::Lake {
                center: (26.0, 26.0),
                radius: 6.0,
                strength: 1.0,
            },
            13,
            1.0,
        );
        let host = sq(4.0, 4.0, 60.0, 60.0);
        let intent = sq(4.0, 4.0, 30.0, 30.0); // 贴左上角
        let g = divide(&grid, &host, &[], &intent, EdgeMode::Snap, 3.0).expect("贴地划分");
        let rem = &g.remainder_outer[0];
        let cuts = groups_outside_host(&g.inside[0], &host);
        assert!(!cuts.is_empty(), "贴边切割应有切口弧");
        for cut in &cuts {
            assert!(
                contains_reversed_run(rem, cut),
                "吸附后切口弧 {cut:?} 的反序仍须逐点出现在余部外环"
            );
        }
    }

    /// 既有洞 + 贴边切割：作者的洞必须原样留在余部，不得被 mirror 误配。
    #[test]
    fn edge_cut_preserves_existing_host_hole() {
        let host = sq(0.0, 0.0, 40.0, 40.0);
        let existing_hole = sq(28.0, 28.0, 34.0, 36.0);
        let intent = sq(0.0, 0.0, 10.0, 10.0);
        let g = divide_geometry(48, 48, &host, std::slice::from_ref(&existing_hole), &intent)
            .expect("划分");
        let holes = &g.remainder_holes[0];
        assert_eq!(
            holes.len(),
            1,
            "余部应恰有 1 个洞（原有洞），实得 {holes:?}"
        );
        assert!(
            (ring_area(&holes[0]) - ring_area(&existing_hole)).abs() < 1e-3,
            "原有洞面积必须不变（{} vs {}），实得 {:?}",
            ring_area(&holes[0]),
            ring_area(&existing_hole),
            holes[0]
        );
    }

    // ---- M2：共边逐点保真（切弧拼接，ADR-0028 追记 M2） ----

    /// 环上「完全落在宿主边界上」的连续段（用严格相等判定：拼接路径下这些
    /// 顶点是宿主原顶点的字面拷贝）。
    fn groups_outside_host<'a>(
        ring: &'a [(f32, f32)],
        host: &[(f32, f32)],
    ) -> Vec<&'a [(f32, f32)]> {
        let on_host = |p: (f32, f32)| {
            let n = host.len();
            (0..n).any(|i| {
                let (a, b) = (host[i], host[(i + 1) % n]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len2 = dx * dx + dy * dy;
                let t = if len2 <= 1e-12 {
                    0.0
                } else {
                    (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
                };
                let proj = (a.0 + dx * t, a.1 + dy * t);
                dist(p, proj) <= 1e-4
            })
        };
        let n = ring.len();
        let mut out = Vec::new();
        let mut i = 0;
        while i < n {
            if on_host(ring[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < n && !on_host(ring[i]) {
                i += 1;
            }
            out.push(&ring[start..i]);
        }
        out
    }

    fn contains_reversed_run(hay: &[(f32, f32)], needle: &[(f32, f32)]) -> bool {
        let rev: Vec<(f32, f32)> = needle.iter().rev().copied().collect();
        hay.windows(rev.len()).any(|w| w == rev.as_slice())
            || hay.windows(needle.len()).any(|w| w == needle)
    }

    /// 贴边切割：新势力环必须**逐点**带出宿主外沿的折点（不再重平滑）。
    #[test]
    fn edge_touching_cut_splices_host_arc_verbatim() {
        // 宿主上缘在 (8,3) 有折点；意图从原点圈出的地块贴着这条上缘。
        let host = vec![
            (0.0, 0.0),
            (8.0, 3.0),
            (20.0, 3.0),
            (20.0, 20.0),
            (0.0, 20.0),
        ];
        let intent = sq(0.0, 0.0, 14.0, 12.0);
        let g = divide_geometry(40, 40, &host, &[], &intent).expect("贴边划分");
        let ring = &g.inside[0];
        assert!(
            ring.windows(2)
                .any(|w| w == [(0.0, 0.0), (8.0, 3.0)] || w == [(8.0, 3.0), (0.0, 0.0)]),
            "新势力环必须逐点保留宿主折点 (0,0)-(8,3)，实得 {ring:?}"
        );
    }

    /// 贴边切割（余部无洞）：两侧共用同一条切口弧，逐点相等（D3）。
    #[test]
    fn edge_touching_cut_mirrors_shared_arc_pointwise() {
        let host = sq(0.0, 0.0, 20.0, 20.0);
        let intent = sq(0.0, 0.0, 10.0, 10.0);
        let g = divide_geometry(40, 40, &host, &[], &intent).expect("切角划分");
        let rem = &g.remainder_outer[0];
        let cuts = groups_outside_host(&g.inside[0], &host);
        assert!(!cuts.is_empty(), "贴边切割应存在切口弧");
        for cut in &cuts {
            assert!(
                contains_reversed_run(rem, cut),
                "切口弧 {cut:?} 的反序必须逐点出现在余部外环 {rem:?} 中"
            );
        }
    }

    /// 连续两次划分：第二次没碰到的邻边（宿主外沿）必须逐点不变。
    #[test]
    fn second_division_preserves_untouched_neighbour_edges_pointwise() {
        let host = sq(0.0, 0.0, 20.0, 20.0);
        let first = divide_geometry(40, 40, &host, &[], &sq(0.0, 0.0, 10.0, 10.0)).expect("第一次");
        let rem1 = first.remainder_outer[0].clone();
        // 第二次切右上角；余部最终的左缘（x=0）与下缘（y=20）自始至终未被触碰。
        let second =
            divide_geometry(40, 40, &rem1, &[], &sq(10.0, 0.0, 20.0, 10.0)).expect("第二次");
        let rem2 = &second.remainder_outer[0];
        let untouched: Vec<(f32, f32)> = rem1
            .iter()
            .copied()
            .filter(|&(x, y)| (x - 0.0).abs() < 1e-4 || (y - 20.0).abs() < 1e-4)
            .collect();
        assert!(
            untouched.len() >= 3,
            "宿主未触碰边应至少 3 个顶点，实得 {untouched:?}"
        );
        let mut matched = 0;
        for pair in untouched.windows(2) {
            let fwd = rem2
                .windows(2)
                .any(|w| w == pair || (w[0] == pair[1] && w[1] == pair[0]));
            if fwd {
                matched += 1;
            }
        }
        assert_eq!(
            matched,
            untouched.len() - 1,
            "未触碰的邻边必须逐点不变：期望 {} 段全部命中，实得 {matched}；rem1={rem1:?} rem2={rem2:?}",
            untouched.len() - 1
        );
    }

    #[test]
    fn divide_into_existing_hole_nests_remainder_correctly() {
        // 宿主已带洞（国中国已存在），再在实部划一块 → 余部仍有原洞。
        let host = sq(0.0, 0.0, 30.0, 30.0);
        let existing_hole = sq(20.0, 20.0, 28.0, 28.0);
        let intent = sq(2.0, 2.0, 8.0, 8.0);
        let g = divide_geometry(40, 40, &host, &[existing_hole], &intent).expect("划分");
        assert_eq!(g.remainder_holes.len(), 1);
        // 余部有 2 个洞：新切出的（= 新势力环，D3 共边）+ 原有洞。
        assert_eq!(
            g.remainder_holes[0].len(),
            2,
            "原洞必须留在余部，且新增新势力的洞"
        );
    }
}
