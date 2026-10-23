//! 折线加宽成三角网——T25 河流的几何路径（追记十一，裁定乙）。
//!
//! ## 为什么河流不走距离场（追记九/十的那条路）
//!
//! 距离场的分辨率是 **1 texel/格**。T25 的河宽是 **1.8 屏幕像素**：
//!
//! | 缩放 | 河宽折算成格 | 结果 |
//! |---|---|---|
//! | 8 px/格 | 0.225 格 | 勉强连续（靠 fwidth 拓宽） |
//! | 16 px/格 | 0.11 格 | **退化成虚线**——低于采样密度 |
//!
//! 这不是调参能救的：双线性重建的最低值受「屏幕采样点与 texel 中心的
//! 最大偏移」限制（约 0.35 格），永远够不到 0.11 格的门槛。**亚格细线
//! 超出 1 texel/格 数据的表达能力**。
//!
//! 几何没有这个问题：线的宽度是**世界尺寸**，按 `px / px_per_cell` 每帧
//! （实为每次缩放）重算，任何缩放下都是精确的 1.8 px。
//!
//! ## 与 `isoline_overlay::build_stroke_mesh` 的关系
//!
//! 同一套展开算法（段两端沿方向外扩半宽，让相邻段的扩出部分互相覆盖，
//! 转角无豁口、无需 miter 计算）。那边吃 `wv_terrain::isoline::Pt` + 网格
//! 配置，这边吃纯 `[f32; 2]` 世界坐标——不共用是因为跨模块拉依赖只为省
//! 20 行不划算，且那边的入参形态与本模块的数据源（`line_sdf::LinePoly`）
//! 不同。**两边算法若漂移，以「转角无豁口」的验收截图为准。**
//!
//! 数据层纯 Rust，无 Bevy 类型——`cargo test` 不带 feature 可测。

/// 一条待展开的折线（**世界坐标**，逐点）。
///
/// 世界坐标由调用方换算好：河流折线来自格坐标，换算见 demo 的
/// `grid_to_world`（与 shader `uv0 = in.uv` 的约定同源，见追记九）。
#[derive(Debug, Clone, PartialEq)]
pub struct StrokePath {
    /// 顶点序列；少于 2 点的路径整体忽略。
    pub pts: Vec<[f32; 2]>,
    /// 是否闭合（河流是开放折线，保留给未来的等值线复用）。
    pub closed: bool,
}

/// 展开结果：位置 + 三角索引（每段 2 个三角形）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrokeMesh {
    /// `[x, y, 0]`——z 恒 0，层序交给实体 Z。
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl StrokeMesh {
    /// 顶点数。
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }
    /// 三角形数。
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    /// 全空（无任何可展开的段）——调用方必须以**不建实体**响应。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// 把折线集展开成加宽三角网。
///
/// `half_w` 是**世界单位**的半宽：屏幕像素口径 `LINE_PX / 2 / px_per_cell`
/// 由调用方算好传入（本函数不知道缩放，只做几何）。
///
/// 退化输入（空路径、单点、零长段、非有限坐标）逐段跳过，不 panic。
#[must_use]
pub fn expand_strokes(paths: &[StrokePath], half_w: f32) -> StrokeMesh {
    let mut out = StrokeMesh::default();
    if !(half_w.is_finite() && half_w > 0.0) {
        return out;
    }
    for path in paths {
        let n = path.pts.len();
        if n < 2 {
            continue;
        }
        let last = if path.closed { n } else { n - 1 };
        for i in 0..last {
            let a = path.pts[i];
            let b = path.pts[(i + 1) % n];
            if !(a[0].is_finite() && a[1].is_finite() && b[0].is_finite() && b[1].is_finite()) {
                continue;
            }
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-9 {
                continue; // 零长段：方向未定义，跳过（否则会出一片 NaN 法线）
            }
            let (ux, uy) = (dx / len, dy / len);
            let (nx, ny) = (-uy, ux); // 左法线
                                      // 段两端沿切线各外扩 half_w：相邻段的扩出部分互相盖住转角缝隙。
            let ax = a[0] - ux * half_w;
            let ay = a[1] - uy * half_w;
            let bx = b[0] + ux * half_w;
            let by = b[1] + uy * half_w;
            let base = out.positions.len() as u32;
            out.positions
                .push([ax + nx * half_w, ay + ny * half_w, 0.0]);
            out.positions
                .push([ax - nx * half_w, ay - ny * half_w, 0.0]);
            out.positions
                .push([bx - nx * half_w, by - ny * half_w, 0.0]);
            out.positions
                .push([bx + nx * half_w, by + ny * half_w, 0.0]);
            out.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    out
}

/// Chaikin 角切平滑（一轮：每条线段取 1/4 与 3/4 两点替换原端点）。
///
/// ## 为什么需要
///
/// `generate_rivers` 的走线是 **8 邻域格步**（`dx,dy ∈ {-1,0,1}`），
/// 生成的折线只有 0° / 45° / 90° 三种方向。烘焙成距离场时被双线性糊掉，
/// 看不出来；改成几何后（追记十一）线宽精确到 1.8 px，**折角也精确了**——
/// 放大即见「程序员美术」的硬拐角。手绘地图的河是蜿蜒的，故加一道平滑。
///
/// ## 为什么是 Chaikin 而不是 Catmull-Rom 重采样
///
/// Chaikin 只做线性插值（不引入过冲），**不会把河甩出原始折线的凸包**——
/// 河流不能因为平滑而跑到分水岭另一侧或穿过山脊。Catmull-Rom 有过冲，
/// 在急转弯处会把控制点甩出去。
///
/// 端点保持不动（开折线的首尾是源头与入海口，位置有水文含义）。
///
/// 一轮把点数变为约 2 倍（每段出 2 点），两轮足以消掉 45° 棱角。
#[must_use]
pub fn smooth_polyline(pts: &[[f32; 2]], rounds: u8) -> Vec<[f32; 2]> {
    let mut cur = pts.to_vec();
    for _ in 0..rounds {
        if cur.len() < 3 {
            return cur;
        }
        let mut next = Vec::with_capacity(cur.len() * 2);
        next.push(cur[0]); // 端点不动
        for w in cur.windows(2) {
            let (a, b) = (w[0], w[1]);
            // 1/4 与 3/4 点
            next.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25]);
            next.push([a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75]);
        }
        next.push(cur[cur.len() - 1]);
        cur = next;
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(pts: Vec<[f32; 2]>) -> Vec<StrokePath> {
        vec![StrokePath { pts, closed: false }]
    }

    /// 单段：4 顶点 / 2 三角形，宽度恒为 2×half_w（沿法线量）。
    #[test]
    fn single_segment_has_expected_topology_and_width() {
        let m = expand_strokes(&one(vec![[0.0, 0.0], [10.0, 0.0]]), 0.5);
        assert_eq!(m.vertex_count(), 4);
        assert_eq!(m.triangle_count(), 2);
        // 水平段 → 法线是竖直方向，宽 = |y| 跨度 = 1.0
        let ys: Vec<f32> = m.positions.iter().map(|p| p[1]).collect();
        let (mn, mx) = (
            ys.iter().cloned().fold(f32::INFINITY, f32::min),
            ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
        );
        assert!((mx - mn - 1.0).abs() < 1e-5, "宽应为 1.0，实际 {}", mx - mn);
    }

    /// 多段斜线：每点处的**垂直**宽度恒为 2×half_w（不随方向变化）。
    #[test]
    fn width_is_constant_regardless_of_direction() {
        for &(dx, dy) in &[(1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (3.0, -4.0)] {
            let m = expand_strokes(&one(vec![[0.0, 0.0], [dx * 5.0, dy * 5.0]]), 0.4);
            assert_eq!(m.vertex_count(), 4);
            // 首段两点的距离 = 2×half_w
            let a = m.positions[0];
            let b = m.positions[1];
            let w = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
            assert!((w - 0.8).abs() < 1e-4, "方向 ({dx},{dy}) 宽度 {w} ≠ 0.8");
        }
    }

    /// 转角处无豁口：相邻两段的四边形在拐点处**重叠**（不是首尾相接）。
    #[test]
    fn corner_quads_overlap() {
        // L 形：(0,0)→(10,0)→(10,10)，half_w = 1
        let m = expand_strokes(&one(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]), 1.0);
        assert_eq!(m.vertex_count(), 8, "两段 → 8 顶点");
        // 第一段终点四边形覆盖 x ∈ [9, 11]（外扩 1）；第二段起点同样覆盖
        // 拐点 (10,0) 附近 —— 只要有顶点落在 x>10 与 y<0 说明外扩生效。
        assert!(
            m.positions.iter().any(|p| p[0] > 10.0 + 1e-6),
            "水平段右端应沿切线外扩"
        );
        assert!(
            m.positions.iter().any(|p| p[1] < -1e-6),
            "竖直段下端应沿切线外扩（盖住转角）"
        );
    }

    /// 闭合路径：n 点产生 n 段（含收尾闭合段）。
    #[test]
    fn closed_path_wraps_around() {
        let square = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let open = expand_strokes(
            &[StrokePath {
                pts: square.clone(),
                closed: false,
            }],
            0.5,
        );
        let closed = expand_strokes(
            &[StrokePath {
                pts: square,
                closed: true,
            }],
            0.5,
        );
        assert_eq!(open.vertex_count(), 4 * 3, "3 段");
        assert_eq!(closed.vertex_count(), 4 * 4, "4 段（含闭合段）");
    }

    /// Chaikin：点在原折线凸包内（不甩出去），端点不动，点数约翻倍。
    #[test]
    fn smooth_polyline_stays_within_hull_and_pins_endpoints() {
        let pts = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let sm = smooth_polyline(&pts, 2);
        assert_eq!(sm[0], pts[0], "起点必须不动");
        assert_eq!(*sm.last().unwrap(), pts[2], "终点必须不动");
        assert!(sm.len() > pts.len() * 2, "两轮后点数应显著增加");
        // 凸包 = [0,10]×[0,10]；Chaikin 是凸组合，点必在包内
        for p in &sm {
            assert!(p[0] >= -1e-6 && p[0] <= 10.0 + 1e-6);
            assert!(p[1] >= -1e-6 && p[1] <= 10.0 + 1e-6);
        }
    }

    /// 平滑后折角变缓：转角处相邻三段的方向变化必须比原始小。
    #[test]
    fn smooth_polyline_softens_the_corner() {
        let pts = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let sm = smooth_polyline(&pts, 2);
        // 找 y 方向开始上升的那一段，其与水平段的夹角应 < 45°
        let mut max_turn = 0.0_f32;
        for w in sm.windows(3) {
            let d1 = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
            let d2 = [w[2][0] - w[1][0], w[2][1] - w[1][1]];
            let a1 = d1[1].atan2(d1[0]);
            let a2 = d2[1].atan2(d2[0]);
            max_turn = max_turn.max((a2 - a1).abs());
        }
        assert!(
            max_turn < std::f32::consts::FRAC_PI_2,
            "平滑后单步转角应小于 90°，实际 {max_turn}"
        );
    }

    /// 不足 3 点的折线原样返回（没有可切的角）。
    #[test]
    fn smooth_polyline_passes_through_short_paths() {
        let two = vec![[0.0, 0.0], [1.0, 1.0]];
        assert_eq!(smooth_polyline(&two, 3), two);
        assert!(smooth_polyline(&[], 2).is_empty());
    }

    /// 退化输入：不 panic、产出空网格。
    #[test]
    fn degenerate_inputs_yield_empty_mesh() {
        assert!(expand_strokes(&[], 0.5).is_empty());
        assert!(expand_strokes(&one(vec![[1.0, 1.0]]), 0.5).is_empty());
        // 零长段
        assert!(expand_strokes(&one(vec![[1.0, 1.0], [1.0, 1.0]]), 0.5).is_empty());
        // 非法半宽
        assert!(expand_strokes(&one(vec![[0.0, 0.0], [1.0, 0.0]]), 0.0).is_empty());
        assert!(expand_strokes(&one(vec![[0.0, 0.0], [1.0, 0.0]]), -1.0).is_empty());
        assert!(expand_strokes(&one(vec![[0.0, 0.0], [1.0, 0.0]]), f32::NAN).is_empty());
        // 非有限坐标
        assert!(expand_strokes(&one(vec![[0.0, 0.0], [f32::NAN, 1.0]]), 0.5).is_empty());
    }

    /// 半宽随缩放变化时必须**线性**反映到几何上（这是「屏幕像素恒定」的前提）。
    #[test]
    fn half_width_scales_geometry_linearly() {
        let path = one(vec![[0.0, 0.0], [10.0, 0.0]]);
        let thin = expand_strokes(&path, 0.25);
        let thick = expand_strokes(&path, 1.0);
        let span = |m: &StrokeMesh| {
            let ys: Vec<f32> = m.positions.iter().map(|p| p[1]).collect();
            ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
                - ys.iter().cloned().fold(f32::INFINITY, f32::min)
        };
        assert!((span(&thick) / span(&thin) - 4.0).abs() < 1e-4);
    }
}
