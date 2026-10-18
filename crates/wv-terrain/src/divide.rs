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
    /// 宿主余部的外环（v1 恰 1 个）。
    pub remainder_outer: Vec<Vec<(f32, f32)>>,
    /// 余部的洞（与 `remainder_outer` 按下标对应；无洞为空）。
    pub remainder_holes: Vec<Vec<Vec<(f32, f32)>>>,
}

/// 布尔栅格。行优先，`index = y * w + x`。
type Mask = Vec<bool>;

fn point_in_mask(mask: &Mask, w: u32, h: u32, x: i64, y: i64) -> bool {
    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
        return false;
    }
    mask[(y as u32 * w + x as u32) as usize]
}

/// 把多边形环栅格化：格心落在环内 → true（even-odd，边界算在内）。
pub fn rasterize_ring(w: u32, h: u32, ring: &[(f32, f32)]) -> Mask {
    let mut mask = vec![false; (w * h) as usize];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let c = (x as f32 + 0.5, y as f32 + 0.5);
            if crate::intent::point_in_polygon(c, ring) {
                mask[(y as u32 * w + x as u32) as usize] = true;
            }
        }
    }
    mask
}

/// 布尔掩膜的边界环提取：格边有向段（内部在左）跟踪成闭合链，
/// 再 chaikin 平滑 + Douglas-Peucker 抽稀。
///
/// 环的方向携带语义：外环与洞的 shoelace 符号相反——调用方据此分捡。
fn mask_boundary_rings(mask: &Mask, w: u32, h: u32) -> Vec<Vec<(f32, f32)>> {
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
            rings.push(smooth_ring(&ring));
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
type Ring = Vec<(f32, f32)>;

/// 外环列表 + 每个外环的洞列表（下标对应）。
type OutersWithHoles = (Vec<Ring>, Vec<Vec<Ring>>);

/// 分捡「外环 + 隶属洞」：面积符号相反者为洞，按洞代表点落入哪个外环归组。
fn split_outers_and_holes(rings: Vec<Ring>) -> OutersWithHoles {
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
    let inside_rings = mask_boundary_rings(&inside_mask, grid_w, grid_h);
    let remainder_rings = mask_boundary_rings(&remainder_mask, grid_w, grid_h);
    let (inside, _) = split_outers_and_holes(inside_rings);
    let (remainder_outer, remainder_holes) = split_outers_and_holes(remainder_rings);
    if inside.len() != 1 || remainder_outer.len() > 1 {
        return Err(DivideError::Fragmented {
            inside: inside.len(),
            remainder: remainder_outer.len(),
        });
    }
    Ok(DivisionGeometry {
        inside,
        remainder_outer,
        remainder_holes,
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
