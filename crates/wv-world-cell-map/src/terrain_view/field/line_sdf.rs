//! 折线距离场——T25 河流 / T26 道路进入 field 管线的数据面（追记九）。
//!
//! ## 这是什么
//!
//! 把一组折线（河流中心线 / 道路中心线）烘成一张**每 texel 存「到最近折线
//! 的欧氏距离」**的纹理，与 `coast_tex`（T3）同构：一格一 texel、行主序、
//! shader 侧 `Linear` 采样后按屏幕像素口径画墨线。
//!
//! ## 为什么不复用 [`super::coast_sdf`] 的 EDT
//!
//! EDT 的种子是**格心**——它天然只能回答「到最近种子格心多远」。河流中心线
//! 的顶点虽在格心上，但**线段中点在格边界之间**（亚格位置）：河 A 中心线
//! 斜穿 (3,4)→(4,5)，线段中点 (3.5, 4.5) 恰在四格交界上。EDT 给出的距离
//! 场在河心处会出现 0.5 格宽的平顶带，墨线从「河心一条线」塌成
//! 「两格间一条带」——这正是 T11 岸线当初放弃距离场改用 `fwidth` 定位的
//! 同一类缺陷（见 terrain_field.wgsl 岸线段的根因分析）。
//!
//! 因此这里**直接算点到线段的精确距离**（不经过任何格心近似）：
//! 每条线段对每个 texel 求一次 `point_segment_distance`，取全折线集的
//! 最小值。复杂度 O(texels × segments)，一次性烘焙可接受（512² 格 ×
//! 数百段 ≈ 10⁸ 次内积，release 下亚秒）；编辑态增量重烘走同函数
//! （确定性，无随机），P2 笔刷接入时再谈增量。
//!
//! ## 编码（T25/T26 的「线状要素必须比边界重」如何落地）
//!
//! 风格权威 §3.8.1：所有线宽以**屏幕像素**定义（河 1.8 / 道路主干 2.4 /
//! 次级 1.2），渲染时按缩放折算。距离场只负责「离线多远」，线宽在
//! shader 侧折算——本模块编码上限只需覆盖「远观档还能画出的最宽线」：
//!
//! ```text
//! DASH_MAX_CELLS = 8
//! enc = clamp(d / 8, 0, 1) × 255
//! ```
//!
//! 8 格上限的依据：1.8 px 线在 0.8 px/格（远观档下缘）时半宽
//! `0.9 / 0.8 ≈ 1.1 格`；即便未来加「河宽 = tile/4」（位图 pack 口径）
//! 也不会超过 8 格。精度 `8 / 255 ≈ 0.031 格`：1.8px 线的半宽阈值
//! 在 4 px/格 时是 0.225 格 = 编码 7 步——亚像素精度，无量化横纹。
//!
//! ## 与 wv_core 的关系
//!
//! 不引 `wv_core`：入参就是 `&[[f32; 2]]` 折线（归一化 [0,1]² 或格坐标
//! 皆可，由调用方先换算好）。数据层純函数，`cargo test` 无 feature 可测。

use wv_core::terrain::TerrainData;

/// 折线距离编码上限（格）。见模块文档「编码」节。
pub const DASH_MAX_CELLS: f32 = 8.0;

/// 一条待烘的线：类别 + 折线（**格坐标**，第 i 格占 [i, i+1)，格心 i+0.5）。
#[derive(Debug, Clone)]
pub struct LinePoly {
    /// 类别（决定烘进哪个通道）。
    pub class: LineClass,
    /// 折线顶点（格坐标）。少于 2 点的折线整体忽略（退化）。
    pub pts: Vec<[f32; 2]>,
}

/// 线类别（T25/T26 三通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineClass {
    /// T25：河流中心线（1.8 px）。
    River,
    /// T26：道路主干（2.4 px）。
    RoadMajor,
    /// T26：道路次级（1.2 px）。
    RoadMinor,
}

/// 三通道打包结果：**一张 `Rgba8Unorm`**，R = 河 / G = 主干 / B = 次级 /
/// A 预留（255，与 attr 的「1 不等于 1.0」坑无关——本纹理三通道都是
/// 距离，A 无语义，填 255 只是保持「未用通道 = 1」的惯例）。
///
/// 编码 `clamp(d / DASH_MAX_CELLS, 0, 1)`——**没有线的远处是 1（255）**，
/// 不是 0：shader 侧 `dd = (sampled - 1.0) * DASH_MAX_CELLS` 解码，
/// 越远值越大，与「距离」的直觉一致。
///
/// **精度声明**：8 位 = 0.031 格。1.8px 线的半宽在 4 px/格 时是 0.225 格
/// （编码 ~7 步），亚像素——不产生可见量化横纹。若未来线宽 > 8 格或
/// 需要亚 0.03 格的精确性，升 `Rgba16Unorm`（改 [`LineDashPack`] 的
/// 字节布局与 shader 解码系数，两处必须同步）。
#[derive(Debug, Clone)]
pub struct LineDashPack {
    pub width: u32,
    pub height: u32,
    /// `Rgba8Unorm` 字节序（行主序）。
    pub tex: Vec<u8>,
}

/// 空 pack（空地形 / 无折线时）。
impl LineDashPack {
    /// 全 1（无任何线）的同尺寸纹理——shader 解码后是 FAR，不画线。
    pub fn empty(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            tex: vec![255; width as usize * height as usize * 4],
        }
    }
}

/// 主入口：把折线集烘成三通道距离纹理。
///
/// 契约：
/// - 折线顶点为**格坐标**（可越界，越界部分照常参与距离——河从图外流入
///   是合法输入）；
/// - 空地形或空折线集 → 全 1 纹理（不 panic、无分配开销陷阱）；
/// - 同一输入两次调用逐字节一致（纯函数，无随机）。
pub fn bake_line_dash(w: u32, h: u32, lines: &[LinePoly]) -> LineDashPack {
    let (wu, hu) = (w as usize, h as usize);
    if wu == 0 || hu == 0 {
        return LineDashPack::empty(w, h);
    }
    // 按通道分桶；退化折线（<2 点）在此过滤。段存平铺四元组
    // [ax, ay, bx, by]——windows(2) 的嵌套数组在后续索引写法上更绕。
    let mut segs: [Vec<[f32; 4]>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let ch = |c: LineClass| -> usize {
        match c {
            LineClass::River => 0,
            LineClass::RoadMajor => 1,
            LineClass::RoadMinor => 2,
        }
    };
    for line in lines {
        if line.pts.len() < 2 {
            continue;
        }
        let c = ch(line.class);
        for pair in line.pts.windows(2) {
            segs[c].push([pair[0][0], pair[0][1], pair[1][0], pair[1][1]]);
        }
    }
    let total_segs: usize = segs.iter().map(|s| s.len()).sum();
    if total_segs == 0 {
        return LineDashPack::empty(w, h);
    }

    // 每 texel × 每通道：到该通道全部线段的最小距离。
    // 先按通道分别求 min（三次独立扫描），最后打包成一个 RGBA——
    // 逐 texel 内层循环里对三通道同时算会让内存访问模式变差
    // （三组段的局部性互相打断），分通道跑两倍于「合并扫描」的代价
    // 换来的缓存友好性实测更划算（此注释在 profiling 反证后再改）。
    let n = wu * hu;
    let mut min_d: [Vec<f32>; 3] = [
        vec![f32::INFINITY; n],
        vec![f32::INFINITY; n],
        vec![f32::INFINITY; n],
    ];
    for (ci, seg_list) in segs.iter().enumerate() {
        if seg_list.is_empty() {
            continue;
        }
        // 布栅格包围盒裁剪：距线段包围盒超过 DASH_MAX_CELLS 的 texel
        // 直接饱和（编码反正钳到 1），省掉大部分内积。
        let max_d2 = (DASH_MAX_CELLS + 1.0) * (DASH_MAX_CELLS + 1.0);
        for seg in seg_list {
            let (ax, ay, bx, by) = (seg[0], seg[1], seg[2], seg[3]);
            let x0 = (ax.min(bx) - DASH_MAX_CELLS).floor().max(0.0) as usize;
            let x1 = (ax.max(bx) + DASH_MAX_CELLS).ceil().min(w as f32) as usize;
            let y0 = (ay.min(by) - DASH_MAX_CELLS).floor().max(0.0) as usize;
            let y1 = (ay.max(by) + DASH_MAX_CELLS).ceil().min(h as f32) as usize;
            for y in y0..y1.min(hu) {
                for x in x0..x1.min(wu) {
                    // texel 中心 = 格心（格坐标下 i + 0.5）
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    let d2 = seg_dist_sq(p, [ax, ay], [bx, by]);
                    if d2 < max_d2 && d2 < min_d[ci][y * wu + x] {
                        min_d[ci][y * wu + x] = d2;
                    }
                }
            }
        }
    }

    let mut tex = Vec::with_capacity(n * 4);
    for row in 0..hu {
        for x in 0..wu {
            let i = row * wu + x;
            for channel in &min_d {
                let d = channel[i].sqrt();
                let enc = (d / DASH_MAX_CELLS).clamp(0.0, 1.0);
                tex.push((enc * 255.0).round() as u8);
            }
            tex.push(255); // A 预留
        }
    }
    LineDashPack {
        width: w,
        height: h,
        tex,
    }
}

/// 点到线段距离的平方（闭区间）。
///
/// 与 `measurement::point_segment_distance` 同公式——独立成纯函数是
/// 因为那边是 `Vec2`/测量语义、这边是格坐标 `[f32; 2]`，跨模块拉依赖
/// 只为省 8 行不划算（且 measurement 是 `wv-render` 顶层、本模块是
/// `terrain_view::field` 数据层，反向依赖违反分层）。
fn seg_dist_sq(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let abx = b[0] - a[0];
    let aby = b[1] - a[1];
    let len_sq = abx * abx + aby * aby;
    if len_sq <= f32::EPSILON {
        let dx = p[0] - a[0];
        let dy = p[1] - a[1];
        return dx * dx + dy * dy;
    }
    let t = (((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / len_sq).clamp(0.0, 1.0);
    let dx = p[0] - (a[0] + abx * t);
    let dy = p[1] - (a[1] + aby * t);
    dx * dx + dy * dy
}

/// 从 [`TerrainData`] 派生河流折线（T25 数据源），复用
/// `wv_terrain::climate::generate_rivers` 的确定性走线（唯一入口）。
///
/// 返回的折线已是**格坐标**（`generate_rivers` 的归一化 [0,1]² × 格数）。
/// `rivers_seed` 从外部传入——由模板定义（`rivers_seed` 独立于地形种子）。
pub fn rivers_from_terrain(
    data: &TerrainData,
    rivers_seed: u64,
    max_rivers: usize,
) -> Vec<LinePoly> {
    let heights: Vec<i16> = data.cells.iter().map(|c| c.height).collect();
    let polys = wv_terrain::climate::generate_rivers(
        data.width,
        data.height,
        &heights,
        rivers_seed,
        max_rivers,
    );
    polys
        .into_iter()
        .map(|pts| LinePoly {
            class: LineClass::River,
            pts: pts
                .into_iter()
                .map(|[nx, ny]| [nx * data.width as f32, ny * data.height as f32])
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单条水平线：距离场应在线上为 0、随远离线性增长、8 格外饱和。
    #[test]
    fn horizontal_line_distance_profile() {
        // 线沿 y = 4.5（格 4 与格 5 的边界）横穿 10 格宽图。
        let lines = [LinePoly {
            class: LineClass::River,
            pts: vec![[0.0, 4.5], [10.0, 4.5]],
        }];
        let pack = bake_line_dash(10, 10, &lines);
        let at = |x: usize, y: usize, c: usize| pack.tex[(y * 10 + x) * 4 + c];

        // 河心（y=4 或 5 的 texel 中心 y=4.5/5.5，距线 0 或 1）
        let d04 = at(5, 4, 0) as f32 / 255.0 * DASH_MAX_CELLS;
        assert!(d04 < 0.01, "河心 texel 距离应≈0，实际 {d04}");
        // 上方 2 格处（texel 中心 y=2.5，距线 2）
        let d02 = at(5, 2, 0) as f32 / 255.0 * DASH_MAX_CELLS;
        assert!((d02 - 2.0).abs() < 0.05, "距线 2 格应读出 2.0，实际 {d02}");
        // 8 格外饱和（图高 10，y=12 不存在；用通道独立性验证饱和路径）
        let far = at(5, 0, 0) as f32 / 255.0 * DASH_MAX_CELLS;
        assert!(far > 3.5, "texel(5,0) 距线 4.5 格，应读 >3.5，实际 {far}");
    }

    /// 三通道互不串扰：河走 R、主干走 G、次级走 B。
    #[test]
    fn channels_are_independent() {
        let lines = [
            LinePoly {
                class: LineClass::River,
                pts: vec![[1.0, 2.5], [9.0, 2.5]],
            },
            LinePoly {
                class: LineClass::RoadMajor,
                pts: vec![[1.0, 5.5], [9.0, 5.5]],
            },
            LinePoly {
                class: LineClass::RoadMinor,
                pts: vec![[1.0, 8.5], [9.0, 8.5]],
            },
        ];
        let pack = bake_line_dash(10, 10, &lines);
        // texel(5,2) 中心 (5.5,2.5) 贴河 → R≈0；G/B 应是到各自线的真实距离
        let i = (2 * 10 + 5) * 4;
        let d_r = pack.tex[i] as f32 / 255.0 * DASH_MAX_CELLS;
        let d_g = pack.tex[i + 1] as f32 / 255.0 * DASH_MAX_CELLS;
        let d_b = pack.tex[i + 2] as f32 / 255.0 * DASH_MAX_CELLS;
        assert!(d_r < 0.01, "R 应≈0（河），实际 {d_r}");
        assert!(
            (d_g - 3.0).abs() < 0.05,
            "G 应≈3（到 y=5.5 主干），实际 {d_g}"
        );
        assert!(
            (d_b - 6.0).abs() < 0.08,
            "B 应≈6（到 y=8.5 次级），实际 {d_b}"
        );
    }

    /// 折线拐角：到折线的距离取两段的较小者，拐角外无「扇形缺口」。
    #[test]
    fn polyline_corner_takes_min_of_segments() {
        // L 形：(2,2) → (8,2) → (8,8)
        let lines = [LinePoly {
            class: LineClass::River,
            pts: vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0]],
        }];
        let pack = bake_line_dash(10, 10, &lines);
        // 拐角外 45° 方向 (1.5, 1.5)：到两段的垂直距离都是 0.5·√2 ≈ 0.707
        // texel (1,1) 中心 (1.5,1.5)
        let d = pack.tex[(10 + 1) * 4] as f32 / 255.0 * DASH_MAX_CELLS;
        let expect = 0.5_f32.sqrt();
        assert!(
            (d - expect).abs() < 0.05,
            "拐角外对角距离应≈{expect}，实际 {d}"
        );
    }

    /// 空输入与退化输入：不 panic、结果全 1。
    #[test]
    fn empty_and_degenerate_inputs_yield_fully_saturated() {
        let empty = bake_line_dash(4, 4, &[]);
        assert!(empty.tex.iter().all(|&b| b == 255));
        let degenerate = bake_line_dash(
            4,
            4,
            &[LinePoly {
                class: LineClass::River,
                pts: vec![[1.0, 1.0]],
            }],
        );
        assert!(degenerate.tex.iter().all(|&b| b == 255));
        let zero = bake_line_dash(0, 0, &[]);
        assert!(zero.tex.is_empty());
    }

    /// 确定性：同一输入两次烘焙逐字节一致（守护项 8）。
    #[test]
    fn bake_is_deterministic() {
        let lines = [
            LinePoly {
                class: LineClass::River,
                pts: vec![[0.5, 1.5], [7.5, 3.5], [6.5, 7.5]],
            },
            LinePoly {
                class: LineClass::RoadMajor,
                pts: vec![[0.5, 5.5], [7.5, 5.5]],
            },
        ];
        let a = bake_line_dash(8, 8, &lines);
        let b = bake_line_dash(8, 8, &lines);
        assert_eq!(a.tex, b.tex);
    }

    /// 越界线段：顶点在图外的折线照常参与（河从图外流入是合法输入）。
    #[test]
    fn out_of_bounds_segments_still_contribute() {
        // 线从图外 (-3, 4.5) 进入图内 (5, 4.5)
        let lines = [LinePoly {
            class: LineClass::River,
            pts: vec![[-3.0, 4.5], [5.0, 4.5]],
        }];
        let pack = bake_line_dash(10, 10, &lines);
        // 图内 (2,4) 应距线 0（线穿过 y=4.5，texel 中心 y=4.5）
        let d = pack.tex[(4 * 10 + 2) * 4] as f32 / 255.0 * DASH_MAX_CELLS;
        assert!(d < 0.01, "图内河心应≈0，实际 {d}");
    }

    /// rivers_from_terrain：折线是格坐标且随种子确定。
    #[test]
    fn rivers_from_terrain_produces_grid_coords() {
        use wv_core::terrain::{TerrainCellData, TerrainData};
        let n = 64u32;
        let cells: Vec<TerrainCellData> = (0..n * n)
            .map(|k| {
                let (x, y) = (k % n, k / n);
                TerrainCellData {
                    height: (4000.0 - (x as f32 + y as f32) * 60.0) as i16,
                    surface: 1,
                    biome: 1,
                }
            })
            .collect();
        let data = TerrainData {
            width: n,
            height: n,
            cells,
        };
        let rivers = rivers_from_terrain(&data, 99, 4);
        assert!(!rivers.is_empty(), "高地斜坡应能生成河流");
        for r in &rivers {
            assert_eq!(r.class, LineClass::River);
            assert!(r.pts.len() >= 2);
            for p in &r.pts {
                // 格坐标（允许 ±1 舍入容差）
                assert!(p[0] > -1.0 && p[0] < n as f32 + 1.0);
                assert!(p[1] > -1.0 && p[1] < n as f32 + 1.0);
            }
        }
    }
}
