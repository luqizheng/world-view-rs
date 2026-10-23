//! Felzenszwalb 精确欧氏距离变换（EDT）——海岸距离场生成器（ADR-0051 D1/D2，T5/T6）。
//!
//! ## 为什么是 EDT 而不是现成 `wv_terrain::shore::shore_distance_field`
//!
//! 既有 BFS 场是**切比雪夫距离**（斜向代价 1），且只算「水到陆」单方向、
//! 截断在 `max_dist`。field 管线需要的是：
//! 1. **精确欧氏距离**（斜向 √2）——岸线墨线（T11/T12）与三段色带（T7–T10）
//!    以它为连续距离，切比雪夫距离的等距线是方形环、欧氏的是圆环，
//!    后者在斜向岸线上才不会出现「台阶感的色带」；
//! 2. **双通道**（水→陆 / 陆→水 各一遍，按格自身 `surface` 取用）——
//!    spec 锁 T5：不用带符号 SDF，符号由 `surface` 通道给出，两遍无符号
//!    距离语义最干净；
//! 3. **亚格连续**——shader 侧对 R16Unorm 做 Linear 采样（T3），f32 的精确
//!    值在插值后仍保持距离场的连续性（整数截断场插值会出现平顶带）。
//!
//! ## 算法
//!
//! Felzenszwalb & Huttenlocher *Distance Transforms of Sampled Functions*
//! （2012）——两趟一维下包络 + 一趟二维组合，O(W·H)。种子（异类格）距离 0，
//! 边界外按无限远处理（钳在场内）。
//!
//! ## P1 契约（demo 只调 [`distance_to_coast]，全量重算）
//!
//! 脏矩形增量（T6，半径 ≥2 格）在 P2 笔刷路径接入——本模块的
//! [`update_dirty_region`] 已实现「局部重算 = 全量重算」的一致性
//! （有单测钉死），P1 只需全量。

use wv_core::terrain::TerrainData;

/// 每格到最近**异类格**（水↔陆）的欧氏距离（格单位，f32）。
///
/// `surface == 0`（水）的格取「到最近陆格」距离；反之取「到最近水格」。
/// 全图同质（全水或全陆）时返回 [`FAR`]（饱和值，shader 侧编码后即 32 格）。
///
/// 权威边界是 `surface` 翻转处（T3：authoring 真值），不用海平面等深线。
pub fn distance_to_coast(data: &TerrainData) -> Vec<f32> {
    let w = data.width.max(1) as usize;
    let h = data.height.max(1) as usize;
    distance_field(w, h, |x, y| data.cells[y * w + x].surface != 0)
}

/// 全图同质时的饱和距离（格）——`coast_dist` 编码上限 32 格的近似值。
pub const FAR: f32 = 1.0e6;

/// `FAR` 的平方（`1e12`）——[`edt_2d`] 内部全程用「距离平方」语义，
/// 故「无种子」在该域里的表示是这个值而非 `FAR`。
///
/// f32 能精确表示 1e12（尾数 24 位内），故这不是精度妥协。
const FAR_SQ: f64 = 1.0e12;

/// 双通道距离场：`is_land` 谓词 → 每格到最近**异类**格的欧氏距离。
///
/// 独立于 `TerrainData` 的纯函数签名——单测可用手摆小网格对表，
/// demo / 未来笔刷路径直接喂谓词闭包。
pub fn distance_field(w: usize, h: usize, is_land: impl Fn(usize, usize) -> bool) -> Vec<f32> {
    // 通道一：陆格 = 0、水格 = FAR（算「水到最近陆」）。
    // 通道二取反重跑一遍，最后按每格自身类别各取对应通道。
    let n = w * h;
    if n == 0 {
        return Vec::new();
    }
    // 种子 = 0（距离平方域），非种子 = FAR_SQ（见 `edt_2d` 的域约定）。
    let mut land_dist = vec![FAR_SQ as f32; n];
    for y in 0..h {
        for x in 0..w {
            if is_land(x, y) {
                land_dist[y * w + x] = 0.0;
            }
        }
    }
    // 就地变换——⚠️ 不可写成 `let mut d1 = land_dist.clone(); edt_2d(&mut d1, ..)`
    // 再继续用 `land_dist`：那样读到的是**未经 EDT 的种子掩码**，
    // 非种子格仍是 FAR（= 1e6），表现为「水格距离全是 FAR」。
    // 这个 bug 的隐蔽之处在于 `edt_2d` 本身完全正确，只是结果没被接上。
    edt_2d(&mut land_dist, w, h);

    // 通道二：水格为种子（取反谓词）。
    let mut water_dist = vec![FAR_SQ as f32; n];
    for y in 0..h {
        for x in 0..w {
            if !is_land(x, y) {
                water_dist[y * w + x] = 0.0;
            }
        }
    }
    edt_2d(&mut water_dist, w, h);

    // 按格自身类别取通道：水格用 land_dist（到最近陆），陆格用 water_dist。
    let mut out = vec![0.0f32; n];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            out[i] = if is_land(x, y) {
                water_dist[i]
            } else {
                land_dist[i]
            };
        }
    }
    out
}

/// 脏矩形增量重算（T6）：把 `x0..x1, y0..y1`（含端点）内的距离值重写为
/// 与全量重算逐字节一致。
///
/// EDT 的传播半径无界（理论上全场），但**值有意义的半径**有限：超出
/// `FAR` 编码上限（32 格）的格子改成什么都一样（shader 编码 `d/32` 后
/// 饱和）。因此取 `margin`（≥2 格，T6 红线）把重算窗口外扩，窗口外
/// 若仍有格子「真实距离 < 32 格且被本次翻转改变」，会在下次全量重算
/// 时纠正——P2 笔刷接入时以「半径 2 格内与全量一致」的验收口径
/// 约束（spec 锁 T6 单测）。
///
/// P1 的 demo 不触发编辑，本函数只由单测消费（对表一致性）。
pub fn update_dirty_region(
    data: &TerrainData,
    field: &mut [f32],
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    margin: usize,
) {
    let w = data.width as usize;
    let h = data.height as usize;
    let mx0 = x0.saturating_sub(margin);
    let my0 = y0.saturating_sub(margin);
    let mx1 = (x1 + margin).min(w - 1);
    let my1 = (y1 + margin).min(h - 1);
    // 窗口内全量重算（demo 场景 512²，窗口级重算代价可忽略；窗口外的
    // 传播误差由 margin 覆盖——见函数 doc 的口径说明）。
    for y in my0..=my1 {
        for x in mx0..=mx1 {
            let i = y * w + x;
            field[i] = exact_distance(data, x, y);
        }
    }
}

/// 暴力参考实现：单格到最近异类格的精确距离（O(N)，只供单测对表与
/// 脏区重算的小窗口使用）。
fn exact_distance(data: &TerrainData, x: usize, y: usize) -> f32 {
    let w = data.width as usize;
    let h = data.height as usize;
    let self_land = data.cells[y * w + x].surface != 0;
    // 32 格半径内找不到即饱和（编码上限）；+1 保证半径边界包含在内。
    let r = 33usize;
    let mut best = FAR;
    for dy in -(r as i64)..=(r as i64) {
        let ny = y as i64 + dy;
        if ny < 0 || ny >= h as i64 {
            continue;
        }
        for dx in -(r as i64)..=(r as i64) {
            let nx = x as i64 + dx;
            if nx < 0 || nx >= w as i64 {
                continue;
            }
            let other_land = data.cells[(ny as usize) * w + nx as usize].surface != 0;
            if other_land != self_land {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if d < best {
                    best = d;
                }
            }
        }
    }
    best
}

/// 二维 EDT：先逐列做一维变换（f(x,y) 视为 y 的函数），再逐行做一维变换。
///
/// 输入 `d` 是种子掩码的初始值（种子 0、其余 FAR），原地变换为精确
/// 欧氏距离。标准 Felzenszwalb 二段式：列变换把「每列的一维距离」算出
/// （对每列独立跑 1D EDT），行变换把「列距离平方 + 行距离平方」的最小
/// 包络算出。
fn edt_2d(d: &mut [f32], w: usize, h: usize) {
    // **全流程统一「距离平方」语义**，只在最后一步开方。
    //
    // 入口：`d[i]` = 到最近种子的距离平方（种子 0，其余 ≥ FAR²）。
    // 出口：`d[i]` = 精确欧氏距离。
    //
    // 这个统一是正确性的关键。若让一维变换「入口平方、出口开方」，则两趟
    // 组合时必须手工在每趟前后做平方/开方转换——漏一处就静默算错（平方被
    // 吃掉或做两次）。实测踩过：第二趟喂「距离平方」却被入口再平方一次
    // 变四次方，`√((5−2)² + 1)` 算成 `√10` 而非 `√2`。
    //
    // `h == 1` 时列变换是恒等（每列 1 格，无行内传播），跳过即省一趟。
    if h > 1 {
        let mut col = vec![0.0f32; h];
        for x in 0..w {
            for y in 0..h {
                col[y] = d[y * w + x];
            }
            edt_1d(&mut col[..h]);
            for y in 0..h {
                d[y * w + x] = col[y];
            }
        }
    }
    // 行变换：距离平方 in → 距离平方 out（完成 `dx² + dy²` 的组合）。
    let mut row = vec![0.0f32; w];
    for y in 0..h {
        for x in 0..w {
            row[x] = d[y * w + x];
        }
        edt_1d(&mut row[..w]);
        for x in 0..w {
            d[y * w + x] = row[x];
        }
    }
    // 出口开方：`√(distance²)` = 距离。FAR² = 1e12 开方回 1e6 = FAR，
    // 恰好饱和，无需额外钳制。
    for v in d.iter_mut() {
        *v = v.sqrt();
    }
}

/// 一维精确距离变换（Felzenszwalb 下包络法），原地。
///
/// **全程「距离平方」语义**（这是与 `edt_2d` 组合正确的前提）：
/// - 入口：`f[i]` = 点 i 到最近种子的**距离平方**（种子 0，其余大值）。
/// - 出口：`f[i]` = min over seeds j of `(i−j)² + f[j]`（仍是距离平方）。
///
/// 二维 EDT 靠这个语义把两趟直接串联：行变换把「列变换算出的垂直距离平方」
/// 当作抛物线高度，与横向偏移平方相加——开方推迟到 `edt_2d` 的最后一步。
/// 若在这里开方，两趟之间就得手工平方回去，漏一处即静默算错。
fn edt_1d(f: &mut [f32]) {
    let n = f.len();
    if n == 0 || n == 1 {
        return;
    }
    // **快速路径：整维无种子**（所有元素都 ≥ FAR）。
    //
    // 二维 EDT 的行变换必然遇到这种情况：某行正上方/正下方一个种子都没有
    // （大片内陆或大片深海的行）。答案是「这一维全是 FAR」。
    // 整维无种子：入口是距离平方，FAR² = 1e12 即「无种子」的表示。
    if f.iter().all(|&x| (x as f64) >= FAR_SQ) {
        f.fill(FAR_SQ as f32);
        return;
    }

    // v：下包络抛物线的根位置；z：包络区间边界（交点横坐标）；
    // d：抛物线顶点的**距离平方**。
    //
    // `z` / `d` 一律 `f64`：下包络的交点公式含 `d[q] + q²` 的减法，
    // f32 在大坐标下会灾难性抵消（种子索引上千即失效）。`f` 保持 f32——
    // 它是最终距离输出，精度需求低一个量级。
    //
    // **关键：用大有限值而非 `INFINITY` 表示「无种子」**（`FAR` 的平方 =
    // 1e12，f64 装得下）。若用 `INFINITY`：
    // - 下包络的交点公式 `(d[q]+q² − d[v[k]]−v[k]²) / 2(q−v[k])` 会得到
    //   `NaN` 或 `±inf`，污染整个包络；
    // - 更致命的是 `v[0] = 0` 无条件把下标 0 当首个包络顶点——当 `d[0]`
    //   是无穷高时 `s` 恒为 `-inf`，而 `z[0] = -inf`，于是 `s <= z[0]`
    //   恒真 → `k` 反复回退到 0 并重算出同一个 `-inf` → **死循环**。
    // 用有限大值后，`s` 恒有限，算法的前���（首个包络顶点高度有限）自动成立。
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut d = vec![0.0f64; n];
    for (i, di) in d.iter_mut().enumerate() {
        // 抛物线顶点高度 = 输入的距离平方（**不再平方**——本函数全程平方域）。
        *di = f[i] as f64;
    }
    let inf = f64::INFINITY;
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -inf;
    z[1] = inf;
    for q in 1..n {
        // 分母 `2(q − v[k])` 恒 > 0（`v[k] < q`：v 只装已扫过的下标），
        // 故 `s` 有限；`z[0] = -inf` 使 `s <= z[k]` 在 k=0 处必为假，
        // 循环不会下溢。
        let mut s = ((d[q] + q as f64 * q as f64) - (d[v[k]] + v[k] as f64 * v[k] as f64))
            / (2.0 * q as f64 - 2.0 * v[k] as f64);
        while s <= z[k] {
            k -= 1;
            s = ((d[q] + q as f64 * q as f64) - (d[v[k]] + v[k] as f64 * v[k] as f64))
                / (2.0 * q as f64 - 2.0 * v[k] as f64);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = inf;
    }
    k = 0;
    for (q, fq) in f.iter_mut().enumerate() {
        let qf = q as f64;
        while z[k + 1] < qf {
            k += 1;
        }
        let dist2 = (qf - v[k] as f64).powi(2) + d[v[k]];
        // 出口保持**距离平方**（不开方——开方统一在 edt_2d 的最后一步）。
        *fq = dist2 as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    fn grid(w: u32, h: u32, land: &[(u32, u32)]) -> TerrainData {
        let mut cells = vec![
            TerrainCellData {
                height: -100,
                surface: 0,
                biome: 0
            };
            (w * h) as usize
        ];
        for &(x, y) in land {
            cells[(y * w + x) as usize].surface = 1;
            cells[(y * w + x) as usize].height = 100;
        }
        TerrainData {
            width: w,
            height: h,
            cells,
        }
    }

    /// 手工对表（T5 验收）：单行水陆分界，距离 = 到分界格的欧氏距离。
    #[test]
    fn single_row_boundary_matches_hand_computed_distances() {
        // 7 格一行：3 水 + 4 陆，分界在 x=2/3 之间。
        let land: Vec<_> = (3..7).map(|x| (x, 0u32)).collect();
        let data = grid(7, 1, &land);
        let d = distance_to_coast(&data);
        // 手算：水格 0,1,2 → 最近陆格是 3，距离 |x-3|；陆格 3..6 → 最近水格是 2。
        let want = [3.0, 2.0, 1.0, 1.0, 2.0, 3.0, 4.0];
        for (i, w) in want.iter().enumerate() {
            assert!((d[i] - w).abs() < 1e-4, "格 {i}：got {} want {w}", d[i]);
        }
    }

    /// 斜向距离是 √2（欧氏），不是 1（切比雪夫）——与 BFS 场的关键差异。
    #[test]
    fn diagonal_neighbor_distance_is_euclidean() {
        // 对角水格到陆格：距离 √2。
        let data = grid(3, 3, &[(0, 0), (2, 2)]);
        let d = distance_to_coast(&data);
        // 中心格 (1,1) 是水，最近陆是 (0,0) 或 (2,2)，距离 √2。
        assert!(
            (d[3 + 1] - std::f32::consts::SQRT_2).abs() < 1e-4,
            "中心格应 √2，got {}",
            d[4]
        );
        // 陆格 (0,0) 到最近水格：邻格 (1,0) 距离 1。
        assert!((d[0] - 1.0).abs() < 1e-4, "角格应 1，got {}", d[0]);
    }

    /// 双通道取用：水格拿「到陆」、陆格拿「到水」——海岸线两侧距离各自
    /// 从 0（对面的异类格）起算，即边界格值 = 1（相邻异类格）而非 0。
    #[test]
    fn both_sides_measure_to_the_opposite_class() {
        let data = grid(4, 1, &[(0, 0), (1, 0)]); // 左 2 陆右 2 水
        let d = distance_to_coast(&data);
        // 陆格 0：到最近水（格 2）= 2；陆格 1：到格 2 = 1；
        // 水格 2：到格 1 = 1；水格 3：到格 1 = 2。
        let want = [2.0, 1.0, 1.0, 2.0];
        for (i, w) in want.iter().enumerate() {
            assert!((d[i] - w).abs() < 1e-4, "格 {i}: got {} want {w}", d[i]);
        }
    }

    /// 全图同质 → 全场饱和 FAR（shader 编码后即 32 格上限）。
    #[test]
    fn uniform_map_saturates_to_far() {
        let data = grid(5, 5, &[]);
        let d = distance_to_coast(&data);
        assert!(d.iter().all(|&v| v >= FAR));
    }

    /// 与暴力参考逐格对表（8×8 随机-ish 水陆）——EDT 正确性的强断言。
    #[test]
    fn edt_matches_bruteforce_on_patterned_grid() {
        let w = 8u32;
        let h = 8u32;
        // 棋盘-ish 但非规则：手工摆 10 个陆格。
        let land = vec![
            (1, 1),
            (2, 1),
            (5, 2),
            (0, 4),
            (3, 3),
            (3, 4),
            (6, 5),
            (7, 5),
            (2, 6),
            (5, 7),
        ];
        let data = grid(w, h, &land);
        let d = distance_to_coast(&data);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let want = exact_distance(&data, x, y);
                let got = d[y * w as usize + x];
                // FAR 的格两边都可能饱和；否则 1e-3 内一致。
                if want < FAR {
                    assert!(
                        (got - want).abs() < 1e-3,
                        "({x},{y}): edt {got} vs bruteforce {want}"
                    );
                }
            }
        }
    }

    /// 脏区重算与全量重算一致（T6）：刷 1 格水陆翻转，半径 2 格内一致。
    #[test]
    fn dirty_region_update_matches_full_recompute() {
        let mut data = grid(12, 12, &[(3, 3), (8, 8), (4, 3)]);
        let mut field = distance_to_coast(&data);
        // 翻转一格：格 (6,6) 水→陆。
        data.cells[6 * 12 + 6].surface = 1;
        update_dirty_region(&data, &mut field, 6, 6, 6, 6, 2);
        let full = distance_to_coast(&data);
        // 半径 2 格窗口内逐格一致（T6 验收口径）。
        for dy in -2i64..=2 {
            for dx in -2i64..=2 {
                let (x, y) = ((6 + dx) as usize, (6 + dy) as usize);
                let i = y * 12 + x;
                if full[i] < FAR {
                    assert!(
                        (field[i] - full[i]).abs() < 1e-3,
                        "({x},{y}): dirty {} vs full {}",
                        field[i],
                        full[i]
                    );
                }
            }
        }
    }

    /// 512² 全量计时（T5 验收：<8ms）——release only，debug 跳过。
    #[test]
    fn edt_512_squared_release_under_8ms() {
        if cfg!(debug_assertions) {
            return; // debug 构建的计时无意义（约慢 10–30×）
        }
        let n = 512u32;
        // 程序化水陆：确定性 hash 场（结构与 demo 无关，只求有海岸）。
        let mut cells = Vec::with_capacity((n * n) as usize);
        for y in 0..n {
            for x in 0..n {
                let h = (x as u64 * 31 + y as u64 * 17) % 97;
                let land = h < 48;
                cells.push(TerrainCellData {
                    height: if land { 100 } else { -100 },
                    surface: u8::from(land),
                    biome: 0,
                });
            }
        }
        let data = TerrainData {
            width: n,
            height: n,
            cells,
        };
        let t0 = std::time::Instant::now();
        let field = distance_to_coast(&data);
        let dt = t0.elapsed();
        assert_eq!(field.len(), (n * n) as usize);
        assert!(
            dt.as_millis() < 8,
            "512² EDT 用时 {}ms（应 <8ms，T5）",
            dt.as_millis()
        );
    }
}
