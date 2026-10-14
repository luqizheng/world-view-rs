//! 等值线提取：marching squares + 类型场边界 + 链装配 + Chaikin + Douglas-Peucker。
//!
//! ## 设计依据（plan v0.1，`docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md`）
//!
//! - **D2**：水陆边界由高度场驱动（海岸 = 海平面等值线），地表覆盖边界由类型场驱动；
//! - **Q4**：**格心采样 + 对偶格插值**——每格存的值即样本，等值点落在相邻格心连线上
//!   （曲线穿行格子内部）；单格小岛 = 内切菱形；
//! - **Q6**：鞍点（对角同侧）用**角点仲裁**——取四格均值作为共享角点（对偶格中心，
//!   恰为四格的几何共享角）的样本值，均值 ≥ 等值面 → 对角「水面」互不连通；
//! - **Q8**：Chaikin 2 轮 + Douglas-Peucker 抽稀（容差由调用方按 LOD 分档）；
//! - **D6**：全部函数为 `(输入数据, 参数) → 输出` 的**纯函数**——同输入必同输出，
//!   派生几何不落盘（D9），渲染期重算永远安全。
//!
//! ## 坐标口径
//!
//! 格坐标（逻辑格为单位）：格 `(x, y)` 的**中心**在 `(x + 0.5, y + 0.5)`，
//! 四格共享的**角点**在整数值上。所有输出点都在该坐标系里；调用方按格径换算到世界坐标。
//!
//! ## 边界（不做什么）
//!
//! - **零第三方依赖**（与 crate 同规）：不引 lyon / geo；
//! - **不感知 Bevy / 渲染**：输出折线数据，mesh 化是 `wv-app` 的事；
//! - **不缓存**：缓存键与失效策略属渲染层（plan §3）。

/// 等值线折点（格坐标，见模块文档）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// 一条等值线段（两端点，方向无意义——装配时按无向处理）。
pub type Seg = [Pt; 2];

/// 装配后的折线链。
///
/// `closed = true` 时 `pts` 是**环**（首尾不重复）；否则是开链（在网格边界被裁断）。
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

// ---------------------------------------------------------------------------
// 高度场等值线（海岸线）
// ---------------------------------------------------------------------------

/// 高度场等值线：对每个**对偶格**（四个相邻格心构成的四边形）跑 marching squares，
/// 输出等值线段（plan Q4）。
///
/// `value_at(x, y)` 返回格 `(x, y)` 的高度（米，Q1）；`level` 为等值面
/// （海岸线传 `0.0`，即海平面）。大于等于 `level` 记为「内侧」。
///
/// 鞍点（案例 5 / 10，对角同侧）按 Q6 角点仲裁：以四格均值作为对偶格中心
/// （= 四格共享的几何角点）的样本，把对偶格拆成四个三角形分别提取——
/// 均值 ≥ `level` 时中心在内侧，对角「外侧」互不连通；反之对角「内侧」被隔断。
pub fn height_segments(w: u32, h: u32, value_at: impl Fn(u32, u32) -> f32, level: f32) -> Vec<Seg> {
    let mut out = Vec::new();
    if w == 0 || h == 0 {
        return out;
    }
    // 越界防御性钳制：正常调用不会越界，但闭包是外部给的，兜底不 panic。
    let v = |x: u32, y: u32| value_at(x.min(w - 1), y.min(h - 1));
    for dy in 0..h - 1 {
        for dx in 0..w - 1 {
            let (v0, v1, v2, v3) = (v(dx, dy), v(dx + 1, dy), v(dx + 1, dy + 1), v(dx, dy + 1));
            // 四格心 = 对偶格四角：p0 左上、p1 右上、p2 右下、p3 左下（y 向下）。
            let p0 = pt(dx as f32 + 0.5, dy as f32 + 0.5);
            let p1 = pt(dx as f32 + 1.5, dy as f32 + 0.5);
            let p2 = pt(dx as f32 + 1.5, dy as f32 + 1.5);
            let p3 = pt(dx as f32 + 0.5, dy as f32 + 1.5);
            let inside = |x: f32| x >= level;
            // 位序与角序一致：bit0..3 = c0..c3。
            let case = inside(v0) as u8
                | (inside(v1) as u8) << 1
                | (inside(v2) as u8) << 2
                | (inside(v3) as u8) << 3;
            // 边交点：0 = 顶(c0-c1)、1 = 右(c1-c2)、2 = 底(c3-c2)、3 = 左(c0-c3)。
            let edge = |e: u8| -> Pt {
                match e {
                    0 => lerp_pt(p0, v0, p1, v1, level),
                    1 => lerp_pt(p1, v1, p2, v2, level),
                    2 => lerp_pt(p3, v3, p2, v2, level),
                    _ => lerp_pt(p0, v0, p3, v3, level),
                }
            };
            match case {
                0 | 15 => {}
                // 鞍点（Q6）：角点仲裁后按四个三角形提取。
                5 | 10 => {
                    let m = (v0 + v1 + v2 + v3) * 0.25;
                    let c = sample(pt(dx as f32 + 1.0, dy as f32 + 1.0), m);
                    let (s0, s1, s2, s3) = (
                        sample(p0, v0),
                        sample(p1, v1),
                        sample(p2, v2),
                        sample(p3, v3),
                    );
                    tri_seg(s0, s1, c, level, &mut out);
                    tri_seg(s1, s2, c, level, &mut out);
                    tri_seg(s2, s3, c, level, &mut out);
                    tri_seg(s3, s0, c, level, &mut out);
                }
                // 常规 14 案例的边配对表（查表免分支漂移；方向不重要）。
                1 | 14 => out.push([edge(3), edge(0)]),
                2 | 13 => out.push([edge(0), edge(1)]),
                3 | 12 => out.push([edge(3), edge(1)]),
                4 | 11 => out.push([edge(1), edge(2)]),
                6 | 9 => out.push([edge(0), edge(2)]),
                7 | 8 => out.push([edge(3), edge(2)]),
                _ => unreachable!("case 由 4 bit 组成，必在 0..16"),
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 类型场边界（地表覆盖）
// ---------------------------------------------------------------------------

/// 类型场边界：每对**相邻异类**格在共享边上输出一段阶梯折线（4 邻域配对）。
///
/// 类型场无自然等值面（不是连续标量场），故不做插值：边界先取格缘阶梯，
/// 交由 [`chaikin`] 磨成自然曲线（plan §2.4 管线：提取 → Chaikin → DP）。
/// 每条异类边恰好输出一段——多型交汇不会双线。
///
/// 对角接触（仅角相触）无共享边、不输出段：几何上两类只交于一个点，
/// 该点由相邻段自然汇聚，不产生面积歧义。
pub fn kind_segments(w: u32, h: u32, kind_at: impl Fn(u32, u32) -> u8) -> Vec<Seg> {
    let mut out = Vec::new();
    if w == 0 || h == 0 {
        return out;
    }
    let k = |x: u32, y: u32| kind_at(x.min(w - 1), y.min(h - 1));
    for y in 0..h {
        for x in 0..w {
            let here = k(x, y);
            // 右邻异类：共享边 x = x+1，从 y 到 y+1。
            if x + 1 < w && k(x + 1, y) != here {
                let gx = x as f32 + 1.0;
                out.push([pt(gx, y as f32), pt(gx, y as f32 + 1.0)]);
            }
            // 下邻异类：共享边 y = y+1，从 x 到 x+1。
            if y + 1 < h && k(x, y + 1) != here {
                let gy = y as f32 + 1.0;
                out.push([pt(x as f32, gy), pt(x as f32 + 1.0, gy)]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 链装配
// ---------------------------------------------------------------------------

/// 把线段汤按端点装配成折线链（marching squares 的原始输出是离散段）。
///
/// 端点匹配用 `f32::to_bits` **精确位匹配**：相邻对偶格共享边上的交点由
/// 同一对格值、同一公式算出，位级一致——不需要浮点容差。手算坐标（如格缘
/// 整数点）同样精确。
///
/// 闭合链（首尾相接）的 `pts` 不含重复的收尾点；开链在网格边界自然截断。
pub fn assemble_chains(segs: &[Seg]) -> Vec<Chain> {
    let key = |p: &Pt| (p.x.to_bits() as u64, p.y.to_bits() as u64);
    // 端点 → 引用它的段下标（每段登记两次）。
    let mut at: std::collections::HashMap<(u64, u64), Vec<usize>> =
        std::collections::HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        // 零长度段（防御）：跳过，不参与装配。
        if s[0] == s[1] {
            continue;
        }
        at.entry(key(&s[0])).or_default().push(i);
        at.entry(key(&s[1])).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut chains = Vec::new();
    for i in 0..segs.len() {
        if used[i] || segs[i][0] == segs[i][1] {
            continue;
        }
        used[i] = true;
        let mut pts = vec![segs[i][0], segs[i][1]];
        // 向尾端延伸：从末点出发找未用段，接它的另一端。
        while let Some(next) = take_next(&at, &mut used, pts.last().expect("非空"), segs) {
            pts.push(next);
        }
        // 向首端延伸：同法反向接。
        let mut front = Vec::new();
        while let Some(next) = take_next(&at, &mut used, &pts[0], segs) {
            front.push(next);
        }
        if !front.is_empty() {
            front.reverse();
            front.extend(pts);
            pts = front;
        }
        let closed = pts.len() >= 3 && key(&pts[0]) == key(pts.last().expect("非空"));
        if closed {
            pts.pop();
        }
        chains.push(Chain { pts, closed });
    }
    chains
}

/// 从 `from` 端点出发找一条未用段，标记已用并返回它的另一端。
fn take_next(
    at: &std::collections::HashMap<(u64, u64), Vec<usize>>,
    used: &mut [bool],
    from: &Pt,
    segs: &[Seg],
) -> Option<Pt> {
    let k = (from.x.to_bits() as u64, from.y.to_bits() as u64);
    for &i in at.get(&k)? {
        if used[i] {
            continue;
        }
        used[i] = true;
        let s = &segs[i];
        return Some(if s[0] == *from { s[1] } else { s[0] });
    }
    None
}

// ---------------------------------------------------------------------------
// Chaikin 切角平滑（Q8）
// ---------------------------------------------------------------------------

/// Chaikin 切角：每轮把每个折点替换为相邻边上的 1/4、3/4 两点。
///
/// - 开链：保留首尾端点（边界不缩水）；
/// - 闭环：`pts` 为环（无重复收尾点），输出 2n 点。
///
/// `rounds = 2` 是 plan Q8 的缺省；每轮点数翻倍，调用方按 LOD 权衡。
pub fn chaikin(pts: &[Pt], closed: bool, rounds: u32) -> Vec<Pt> {
    let mut cur = pts.to_vec();
    for _ in 0..rounds {
        let n = cur.len();
        if (closed && n < 3) || (!closed && n < 2) {
            break;
        }
        let mut next = Vec::with_capacity(n * 2);
        if closed {
            for i in 0..n {
                let a = cur[i];
                let b = cur[(i + 1) % n];
                next.push(pt(a.x * 0.75 + b.x * 0.25, a.y * 0.75 + b.y * 0.25));
                next.push(pt(a.x * 0.25 + b.x * 0.75, a.y * 0.25 + b.y * 0.75));
            }
        } else {
            next.push(cur[0]);
            for i in 0..n - 1 {
                let a = cur[i];
                let b = cur[i + 1];
                next.push(pt(a.x * 0.75 + b.x * 0.25, a.y * 0.75 + b.y * 0.25));
                next.push(pt(a.x * 0.25 + b.x * 0.75, a.y * 0.25 + b.y * 0.75));
            }
            next.push(cur[n - 1]);
        }
        cur = next;
    }
    cur
}

// ---------------------------------------------------------------------------
// Douglas-Peucker 抽稀（Q8）
// ---------------------------------------------------------------------------

/// Douglas-Peucker 抽稀：删去到弦线距离不超过 `tol` 的中间点，保形降顶点。
///
/// 闭环先以「首点 + 距首点最远点」双锚切半，两半各自抽稀后拼接——
/// 保证环的两个锚点必被保留，且最远锚的选择是确定性的（并列取最先）。
pub fn simplify_dp(pts: &[Pt], closed: bool, tol: f32) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    if closed {
        // 最远锚（确定性：严格更远才替换）。
        let mut m = 1;
        let mut md = -1.0f32;
        for (i, p) in pts.iter().enumerate().skip(1) {
            let d = dist(*p, pts[0]);
            if d > md {
                md = d;
                m = i;
            }
        }
        let mut keep = vec![false; pts.len()];
        keep[0] = true;
        keep[m] = true;
        dp_mark(pts, 0, m, &mut keep, tol);
        // 后半：m..n 加上首点作为收尾锚。
        let mut tail: Vec<Pt> = pts[m..].to_vec();
        tail.push(pts[0]);
        let mut tail_keep = vec![false; tail.len()];
        tail_keep[0] = true;
        let last = tail.len() - 1;
        tail_keep[last] = true;
        dp_mark(&tail, 0, last, &mut tail_keep, tol);
        let mut out: Vec<Pt> = Vec::new();
        for (i, k) in keep.iter().enumerate() {
            if *k {
                out.push(pts[i]);
            }
        }
        for (i, k) in tail_keep.iter().enumerate().skip(1) {
            if *k {
                out.push(tail[i]);
            }
        }
        out.pop(); // 去掉收尾的首点副本——环不重复收尾。
        out
    } else {
        let mut keep = vec![false; pts.len()];
        keep[0] = true;
        keep[pts.len() - 1] = true;
        dp_mark(pts, 0, pts.len() - 1, &mut keep, tol);
        pts.iter()
            .zip(keep)
            .filter(|(_, k)| *k)
            .map(|(p, _)| *p)
            .collect()
    }
}

/// 经典 DP 的显式栈版（防长链爆栈）：`s..=e` 保留端点，中间超差点递归保留。
fn dp_mark(pts: &[Pt], s: usize, e: usize, keep: &mut [bool], tol: f32) {
    if e <= s + 1 {
        return;
    }
    let mut stack = vec![(s, e)];
    while let Some((s, e)) = stack.pop() {
        if e <= s + 1 {
            continue;
        }
        let (a, b) = (pts[s], pts[e]);
        let mut far = s;
        let mut fd = -1.0f32;
        for (i, p) in pts[s + 1..e].iter().enumerate() {
            let d = seg_dist(*p, a, b);
            if d > fd {
                fd = d;
                far = s + 1 + i;
            }
        }
        if fd > tol {
            keep[far] = true;
            stack.push((s, far));
            stack.push((far, e));
        }
    }
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// 位置 + 样本值（鞍点三角形提取的角点描述）。
#[derive(Debug, Clone, Copy)]
struct Sample {
    p: Pt,
    v: f32,
}

fn sample(p: Pt, v: f32) -> Sample {
    Sample { p, v }
}

/// 两样本间的等值交点：线性插值 + 钳制（防御，位不同则分母非零）。
fn lerp_pt(a: Pt, va: f32, b: Pt, vb: f32, level: f32) -> Pt {
    let t = ((level - va) / (vb - va)).clamp(0.0, 1.0);
    pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// 三角形（三格样本）的等值线段：恰有两侧内外不同时输出一段。
fn tri_seg(a: Sample, b: Sample, c: Sample, level: f32, out: &mut Vec<Seg>) {
    let (ia, ib, ic) = (a.v >= level, b.v >= level, c.v >= level);
    let mut xs: [Option<Pt>; 3] = [None; 3];
    if ia != ib {
        xs[0] = Some(lerp_pt(a.p, a.v, b.p, b.v, level));
    }
    if ib != ic {
        xs[1] = Some(lerp_pt(b.p, b.v, c.p, c.v, level));
    }
    if ic != ia {
        xs[2] = Some(lerp_pt(c.p, c.v, a.p, a.v, level));
    }
    let pts: Vec<Pt> = xs.into_iter().flatten().collect();
    if pts.len() == 2 {
        out.push([pts[0], pts[1]]);
    }
}

fn dist(a: Pt, b: Pt) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// 点到线段 `a-b` 的距离（垂距，端点外钳到端点距）。
fn seg_dist(p: Pt, a: Pt, b: Pt) -> f32 {
    let (abx, aby) = (b.x - a.x, b.y - a.y);
    let len2 = abx * abx + aby * aby;
    if len2 <= f32::EPSILON {
        return dist(p, a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / len2).clamp(0.0, 1.0);
    dist(p, pt(a.x + t * abx, a.y + t * aby))
}

// ---------------------------------------------------------------------------
// 测试（plan §5：手算期望；AGENTS §7.3：完整英文短句命名）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn field(w: u32, h: u32, f: impl Fn(u32, u32) -> f32) -> impl Fn(u32, u32) -> f32 {
        let cells: Vec<f32> = (0..w * h).map(|i| f(i % w, i / w)).collect();
        move |x, y| cells[(y * w + x) as usize]
    }

    fn kinds(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> impl Fn(u32, u32) -> u8 {
        let cells: Vec<u8> = (0..w * h).map(|i| f(i % w, i / w)).collect();
        move |x, y| cells[(y * w + x) as usize]
    }

    /// plan §5.3：同输入两次提取逐位相等（纯函数确定性）。
    #[test]
    fn same_input_yields_bitwise_identical_segments() {
        let f = field(8, 8, |x, y| ((x * 7 + y * 13) % 5) as f32 - 2.0);
        let a = height_segments(8, 8, &f, 0.0);
        let b = height_segments(8, 8, &f, 0.0);
        assert_eq!(a, b, "纯函数必须逐位可复现");
    }

    /// 全水面无等值线。
    #[test]
    fn flat_below_level_yields_no_segments() {
        let f = field(4, 4, |_, _| -100.0);
        assert!(height_segments(4, 4, &f, 0.0).is_empty());
    }

    /// 单格小岛 = 内切菱形（Q4 的招牌手算案例）：3×3 中央格 (1,1) 为 +100、四周 -100。
    /// 格心在 (1.5,1.5)，交点落在与四邻格心的中点：菱形顶点
    /// (1.5,1) (2,1.5) (1.5,2) (1,1.5)，装配成一条闭环。
    #[test]
    fn single_land_cell_yields_inscribed_diamond_chain() {
        let f = field(3, 3, |x, y| if x == 1 && y == 1 { 100.0 } else { -100.0 });
        let chains = assemble_chains(&height_segments(3, 3, &f, 0.0));
        assert_eq!(chains.len(), 1, "单岛应装配成一条链，got {:?}", chains);
        let c = &chains[0];
        assert!(c.closed, "单岛等值线应闭合");
        assert_eq!(c.pts.len(), 4, "菱形 4 顶点，got {:?}", c.pts);
        let mut got: Vec<[f32; 2]> = c.pts.iter().map(|p| [p.x, p.y]).collect();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let want: Vec<[f32; 2]> = vec![[1.0, 1.5], [1.5, 1.0], [1.5, 2.0], [2.0, 1.5]];
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g[0] - w[0]).abs() < 1e-6 && (g[1] - w[1]).abs() < 1e-6,
                "顶点 {g:?} ≠ 手算 {w:?}"
            );
        }
    }

    /// 纵向海岸：左列 +100、右侧 -100，交点精确落在 x = 1.0（插值 0.5 + 0.5）。
    #[test]
    fn vertical_coast_crosses_at_interpolated_column() {
        let f = field(3, 2, |x, _| if x == 0 { 100.0 } else { -100.0 });
        let segs = height_segments(3, 2, &f, 0.0);
        assert_eq!(segs.len(), 1, "3×2 只有一个对偶格有等值，got {:?}", segs);
        let s = segs[0];
        for p in &s {
            assert!((p.x - 1.0).abs() < 1e-6, "x 应精确为 1.0，got {}", p.x);
            assert!((0.5..=1.5).contains(&p.y), "y 在本行格心区间，got {}", p.y);
        }
    }

    /// 鞍点 · Q6 角点仲裁（手算）：2×2 = [+100, -40; -40, +100]，均值 +30 ≥ 0
    /// → 中心在内侧，被隔断的是两个「水角」（右上、左下）——链的中点（辐条交点）
    /// 必落在水角象限。
    #[test]
    fn saddle_with_nonnegative_mean_isolates_water_corners() {
        let f = field(2, 2, |x, y| match (x, y) {
            (0, 0) | (1, 1) => 100.0,
            _ => -40.0,
        });
        let chains = assemble_chains(&height_segments(2, 2, &f, 0.0));
        assert_eq!(chains.len(), 2, "两条水角弧，got {:?}", chains);
        for c in &chains {
            assert!(!c.closed);
            // 链中点 = 对偶格中心辐条上的交点，判定它在哪个象限。
            let mid = &c.pts[c.pts.len() / 2];
            let in_tr = mid.x > 1.0 && mid.y < 1.0;
            let in_bl = mid.x < 1.0 && mid.y > 1.0;
            assert!(in_tr || in_bl, "中点 {mid:?} 应落在水角（右上/左下）象限");
        }
    }

    /// 鞍点反面：均值 < 0 → 中心在外侧，被隔断的是两个「陆角」。
    #[test]
    fn saddle_with_negative_mean_isolates_land_corners() {
        let f = field(2, 2, |x, y| match (x, y) {
            (0, 0) | (1, 1) => 40.0,
            _ => -100.0,
        });
        let chains = assemble_chains(&height_segments(2, 2, &f, 0.0));
        assert_eq!(chains.len(), 2, "两条陆角弧，got {:?}", chains);
        for c in &chains {
            let mid = &c.pts[c.pts.len() / 2];
            let in_tl = mid.x < 1.0 && mid.y < 1.0;
            let in_br = mid.x > 1.0 && mid.y > 1.0;
            assert!(in_tl || in_br, "中点 {mid:?} 应落在陆角（左上/右下）象限");
        }
    }

    /// 类型场：2×1 两类相邻 → 共享边上一段竖直阶梯。
    #[test]
    fn kind_boundary_emits_segment_on_shared_edge() {
        let k = kinds(2, 1, |x, _| if x == 0 { 3 } else { 4 });
        let segs = kind_segments(2, 1, &k);
        assert_eq!(segs.len(), 1, "got {:?}", segs);
        assert_eq!(segs[0][0], pt(1.0, 0.0));
        assert_eq!(segs[0][1], pt(1.0, 1.0));
    }

    /// 类型场棋盘：2×2 全异类相邻 → 四段交于中心角点 (1,1)，成「+」字。
    #[test]
    fn checkerboard_kinds_emit_four_segments_meeting_at_corner() {
        let k = kinds(2, 2, |x, y| if (x + y) % 2 == 0 { 3 } else { 4 });
        let segs = kind_segments(2, 2, &k);
        assert_eq!(segs.len(), 4, "4 条异类边各一段，got {:?}", segs);
        let hits_corner = segs.iter().all(|s| {
            s.iter().any(|p| {
                (p.x == 0.0 || p.x == 1.0 || p.x == 2.0) && (p.y == 0.0 || p.y == 1.0 || p.y == 2.0)
            })
        });
        assert!(hits_corner, "所有段端点都在格缘整数位，got {:?}", segs);
    }

    /// 装配：菱形四段接成闭环且首尾不重复。
    #[test]
    fn assembly_joins_shared_endpoints_into_closed_ring() {
        let f = field(3, 3, |x, y| if x == 1 && y == 1 { 100.0 } else { -100.0 });
        let chains = assemble_chains(&height_segments(3, 3, &f, 0.0));
        let c = &chains[0];
        assert_ne!(c.pts.first(), c.pts.last(), "环不重复收尾点");
        assert!(c.closed);
    }

    /// Chaikin 开链一轮（手算）：[(0,0),(2,0),(2,2)] →
    /// [(0,0),(0.5,0),(1.5,0),(2,0.5),(2,1.5),(2,2)]——首尾保留，折点变 1/4、3/4 两点。
    #[test]
    fn chaikin_open_round_matches_hand_computed_points() {
        let pts = vec![pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 2.0)];
        let out = chaikin(&pts, false, 1);
        let want = [
            [0.0, 0.0],
            [0.5, 0.0],
            [1.5, 0.0],
            [2.0, 0.5],
            [2.0, 1.5],
            [2.0, 2.0],
        ];
        assert_eq!(out.len(), want.len(), "got {out:?}");
        for (g, w) in out.iter().zip(want) {
            assert!(
                (g.x - w[0]).abs() < 1e-6 && (g.y - w[1]).abs() < 1e-6,
                "{g:?} ≠ {w:?}"
            );
        }
    }

    /// Chaikin 闭环一轮：正方形 4 点 → 8 点，角被切掉。
    #[test]
    fn chaikin_closed_round_doubles_points_and_cuts_corners() {
        let sq = vec![pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 2.0), pt(0.0, 2.0)];
        let out = chaikin(&sq, true, 1);
        assert_eq!(out.len(), 8);
        // 首两点手算：0.75(0,0)+0.25(2,0) = (0.5,0)，0.25(0,0)+0.75(2,0) = (1.5,0)。
        assert!(
            (out[0].x - 0.5).abs() < 1e-6 && out[0].y.abs() < 1e-6,
            "got {:?}",
            out[0]
        );
        assert!(
            (out[1].x - 1.5).abs() < 1e-6 && out[1].y.abs() < 1e-6,
            "got {:?}",
            out[1]
        );
        assert!(
            out.iter().all(|p| p.x.abs() < 1e-6
                || (p.x - 2.0).abs() < 1e-6
                || p.y.abs() < 1e-6
                || (p.y - 2.0).abs() < 1e-6),
            "一轮后所有点仍在边界上（角被切但不出界），got {out:?}"
        );
    }

    /// DP：共线中间点在容差内被删，拐点保留（手算）。
    #[test]
    fn simplify_dp_removes_collinear_points_keeps_corners() {
        let pts = vec![pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, 0.0), pt(2.0, 2.0)];
        let out = simplify_dp(&pts, false, 0.1);
        assert_eq!(out.len(), 3, "got {out:?}");
        assert_eq!(out[0], pt(0.0, 0.0));
        assert_eq!(out[1], pt(2.0, 0.0));
        assert_eq!(out[2], pt(2.0, 2.0));
    }

    /// DP 大容差：闭环退化到两个锚点（首点 + 最远点）。
    #[test]
    fn simplify_dp_with_huge_tolerance_collapses_ring_to_anchors() {
        let sq = vec![pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 2.0), pt(0.0, 2.0)];
        let out = simplify_dp(&sq, true, 100.0);
        assert_eq!(out.len(), 2, "got {out:?}");
    }

    /// 端到端小管线：提取 → 装配 → Chaikin → DP 全链可用（烟囱测试）。
    #[test]
    fn full_pipeline_extract_assemble_smooth_simplify() {
        let f = field(16, 16, |x, y| {
            if (x as i32 - 8).pow(2) + (y as i32 - 8).pow(2) < 25 {
                100.0
            } else {
                -100.0
            }
        });
        let chains = assemble_chains(&height_segments(16, 16, &f, 0.0));
        assert_eq!(
            chains.len(),
            1,
            "单连通圆应成一条闭环，got {}",
            chains.len()
        );
        let c = &chains[0];
        assert!(c.closed);
        let smooth = chaikin(&c.pts, true, 2);
        assert_eq!(smooth.len(), c.pts.len() * 4, "闭环两轮翻两倍");
        let thin = simplify_dp(&smooth, true, 0.2);
        assert!(thin.len() >= 4, "抽稀后仍是多边形，got {}", thin.len());
        assert!(thin.len() <= smooth.len(), "抽稀只会变少");
    }
}
