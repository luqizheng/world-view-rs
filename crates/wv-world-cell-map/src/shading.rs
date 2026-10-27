//! CellMap 真实地形晕渲的**公式唯一真源**（ADR-0077 D6）。
//!
//! 双光源美术光照 + Horn 米制梯度 + 三尺度概括权重的全部常量与纯函数
//! 都收在本模块——后续 CPU 块烘焙、GPU `FieldParams` uniform、WGSL
//! 场着色器三处**只准消费这里的签名与常量，禁散落字面量**（ADR-0077 D2/D6）。
//!
//! ## 为什么是「美术化光照」而非物理太阳模拟
//!
//! 用户明示：晕渲的目标是**让山体可读**（山脊受光 / 背光分明、宽缓地形
//! 有体量），不是渲染真实日照。因此三光系数（[`AMBIENT`] /
//! [`DIFFUSE_MAIN`] / [`DIFFUSE_AUX`]）是**美术调参**，和恒为 1：
//! `I = 0.4 + 0.48·max(0, N·L₁) + 0.12·max(0, N·L₂)`，入参 = 法线与
//! 光向量的点积。[`LIGHT_MAIN`] / [`LIGHT_AUX`] 自 2026-10-28 起按
//! spec/ADR-0077 D1 的字面存**真 45° 高度角单位向量**——`dot(n, LIGHT_*)`
//! 即合法入参，无需归一化（此前沿用的 `(−1/√2, 1/√2, 1/√2)` 模长 √(3/2)、
//! 实际高度角只有 atan(1/√2) ≈ 35.26°，与 spec 矛盾，已修正）。单位光向
//! 下 `I ∈ [0.4, 1.0]`（三系数和为 1，`max(0,·)` 非负）**界内性免费获得，
//! 明暗钳制（RELIEF_LO/HI）整体退役**，少一个调参自由度（ADR-0077 D1；
//! 若拿非单位光向量直接点积则可越界，见 [`lighting_intensity`] 文档的
//! 历史注记）。辅光 L₂ 治「山脉走向恰与主光平行时受光/背光面同时消失」
//! 的盲区。
//!
//! ## 量纲纪律
//!
//! [`horn_gradient`] 返回**米/格**（i16 高度直接作 f32 差分，不乘
//! H_RANGE）；[`slope_gain_of`] 把它无量纲化（`s = Z_EXAG / 米每格`）。
//! 米制化后「同物理坡 → 同法线」跨图幅成立（不论一格是多少米），
//! Z_EXAG 是全图统一的「纵向夸张旋钮」——换图的格距不再改变观感。
//!
//! ## 三尺度概括
//!
//! `I_final = W_MACRO·I_macro + W_MEDIUM·I_medium + W_DETAIL·I_detail`
//! （三权重和 = 1，混合后仍在 `[0.4, 1.0]`）。远观时细纹理成噪点，
//! detail 权重随 px_per_cell 平滑衰减、衰减量按宏观：中观比例回流
//! （[`scale_weights`]）——**取代** T33「远观浮雕整体关灯」（有意推翻，
//! ADR-0077 D3）。
//!
//! [ADR-0077]: ../docs/adr/ADR-0077-cellmap-relief-shading.md

use std::f32::consts::FRAC_1_SQRT_2;

use wv_core::terrain::TerrainData;

// ---------------------------------------------------------------------------
// 常量（ADR-0077 D1/D2/D3；值锁死于设计稿，验收期唯一校准点 = Z_EXAG）
// ---------------------------------------------------------------------------

/// 环境光系数：背光面保底亮度。与两漫反射系数和恒为 1（见模块文档）。
pub const AMBIENT: f32 = 0.4;
/// 主光漫反射系数。
pub const DIFFUSE_MAIN: f32 = 0.48;
/// 辅光漫反射系数（弱辅助，约为主光的 1/4）。
pub const DIFFUSE_AUX: f32 = 0.12;

/// 垂直夸张系数：真实地貌按真实比例打光几乎平得看不见（2000 m 山在
/// 100 m/格图上坡度只有 0.02），专业晕渲惯用夸张量让坡面可读。
/// 初始 2.0（用户给定；退役旧 `shape.z` 的 0.55），**验收期唯一校准点**
/// （ADR-0077 §4——校准结论按流程追记回 ADR，不散改）。
pub const Z_EXAG: f32 = 2.0;

/// 宏观档权重（远观山脉骨架）。
/// 三尺度权重 [`W_MACRO`] + [`W_MEDIUM`] + [`W_DETAIL`] 和 = 1，
/// 混合结果不需要再钳制——与光照系数同一「和=1 免钳制」纪律。
pub const W_MACRO: f32 = 0.45;
/// 中观档权重（支脉 / 山体）。
pub const W_MEDIUM: f32 = 0.40;
/// 细节档权重（局部坡面），随缩放衰减（[`detail_weight_fade`]）。
pub const W_DETAIL: f32 = 0.15;

/// 中观盒平均足迹（格）：中观档 ≈ 以此为半径的盒式低通后的地形打光。
/// GPU 侧映射到 mip 级偏移 / 跨步采样（mip 即盒式低通；高度为仿射编码
/// ⇒ 编码域滤波 ≡ 米域滤波，ADR-0077 D3）。
pub const MEDIUM_RADIUS_CELLS: f32 = 3.0;
/// 宏观盒平均足迹（格）：宏观档的盒式低通半径。
pub const MACRO_RADIUS_CELLS: f32 = 12.0;

/// 主光 L₁：方位 315°（西北）、**高度角 45°**（spec D1 字面；「左上受光」
/// 与全部地物符号/阴影同向——方位沿 `field/material.rs::LIGHT_DIR` 的既有
/// 惯例，高度角 D1 修正为真 45°）。**单位向量**：水平模 cos45° = 1/√2
/// 均分到西北两轴（各 0.5），z = sin45° = 1/√2 表示「光从观察者方向偏上
/// 打过来」——纯 2D hillshade 的 z=0 会让所有法线与光垂直、整幅图恒定
/// 亮度。2026-10-28 前沿用的 `(−1/√2, 1/√2, 1/√2)` 模长 √(3/2)、实际
/// 高度角仅 35.26°，与 spec 矛盾，随归一化契约取消一并修正——**消费侧
/// （T4 CPU / T6 WGSL）直接 dot 即可，无需再 normalize**（多 normalize
/// 也无害，单位向量不动）。
pub const LIGHT_MAIN: [f32; 3] = [-0.5, 0.5, FRAC_1_SQRT_2];
/// 辅光 L₂：方位 45°（东北）、高度角 45°。与主光**只差 x 符号**——
/// 正交的受光盲区互补：任一走向的山脉，两面至少被一盏灯看见。
pub const LIGHT_AUX: [f32; 3] = [0.5, 0.5, FRAC_1_SQRT_2];

/// detail 权重衰减曲线起点（px/格）：低于此分辨率细纹理全是噪点，
/// detail 权重归零。
pub const DETAIL_FADE_MIN_PX_PER_CELL: f32 = 1.0;
/// detail 权重衰减曲线终点（px/格）：高于此分辨率细部可信，detail 全额。
pub const DETAIL_FADE_MAX_PX_PER_CELL: f32 = 3.0;

// ---------------------------------------------------------------------------
// 纯函数（消费方：CPU 块烘焙 / GPU uniform / WGSL 三处只认这里）
// ---------------------------------------------------------------------------

/// 地图格距的米制换算——**唯一入口**，禁第二份硬编码（ADR-0077 D2）。
///
/// - `Some(k)`（`unit_per_km`：1 k 个世界单位 = 1 km）→
///   `cell_size_wu × 1000 / k` 米/格；
/// - `None` 或 `k <= 0` → 按「**1 wu = 1 m**」回落（`cell_size_wu` 原样）。
///
/// TODO(阶段: 晕渲): 无比例尺虚构图的「1 wu = 1 m」回落是**假设不是真值**，
/// 待决条目见 README §12-53「无比例尺地图的米制假设」（虚构图是否引入
/// 显式比例尺字段、回落是否长期保留）；收敛后此处同步改。
pub fn meters_per_cell_of(cell_size_wu: f32, unit_per_km: Option<f32>) -> f32 {
    match unit_per_km {
        Some(k) if k > 0.0 => cell_size_wu * 1000.0 / k,
        // None / k <= 0：回落假设，见上方 TODO 与 README §12-53。
        _ => cell_size_wu,
    }
}

/// 梯度无量纲化增益：`s = Z_EXAG / meters_per_cell`。
///
/// 梯度是米/格，s 把「每格米差」缩回「每格无纲坡度」再进法线公式；
/// 米制化后同物理坡跨图幅同法线（Z_EXAG 全图统一，见模块文档）。
pub fn slope_gain_of(meters_per_cell: f32) -> f32 {
    Z_EXAG / meters_per_cell
}

/// CPU 块烘焙旧签名（`terrain_pixels` / `patch_terrain_image`）委托
/// `_with_scale` 变体时传入的**过渡缺省格距**（米/格）。
///
/// **纯过渡缺省**：wv-app 按 `GridConfig.cell_size + MapNode.unit_per_km`
/// 经 [`meters_per_cell_of`] 接线 `_with_scale` 后即消失；在那之前，
/// 未接线的消费方统一按 100 m/格打光（`slope_gain = 2.0/100 = 0.02`，
/// 对生成图邻格 10–200 m 高差的浮雕观感与旧路径量级相近）。
///
/// TODO(阶段: 晕渲): 「100 m/格」是假设不是真值，待决条目见
/// README §12-53「无比例尺地图的米制假设」。注意它与 [`meters_per_cell_of`]
/// 的「1 wu = 1 m」回落是**两处不同的假设**——那边是换算入口的回落假设，
/// 这里是 CPU 烘焙未接线期的占位假设；收敛后两处同步改。
pub const DEFAULT_METERS_PER_CELL: f32 = 100.0;

/// 光照明度乘子：`AMBIENT + DIFFUSE_MAIN·max(0, N·L₁) + DIFFUSE_AUX·max(0, N·L₂)`。
///
/// ## 接口契约：入参 = `dot(n, LIGHT_*)`
///
/// [`LIGHT_MAIN`] / [`LIGHT_AUX`] 自 2026-10-28 起是**单位向量**（spec D1：
/// 方位 315°/45°、高度角 45°），`n_dot_l1 / n_dot_l2` 直接取
/// `dot(n, LIGHT_MAIN)` / `dot(n, LIGHT_AUX)` 即可，无需归一化
///（2026-10-28 前常量存模长 √(3/2) 方向、归一化是调用方义务——契约随
/// 常量单位化取消；本函数始终按 ADR-0077 D1 锁死为纯公式，不做隐式
/// 归一化）。
///
/// 对单位光向量点积入参，`I ∈ [0.4, 1.0]`（三系数和为 1，`max(0,·)` 非负）
/// ——**界内性免费获得，调用方不许再钳制**。上界不紧：全定义域实际
/// max = `0.4 + |0.48·L₁ + 0.12·L₂|`，其中 `L₁·L₂ = 0.5`，故
/// `|0.48L₁ + 0.12L₂| = √(0.48² + 0.12² + 2·0.48·0.12·0.5) = √0.3024
/// ≈ 0.5499`，即 max ≈ 0.9499（取得于 `n ∝ 0.48L₁ + 0.12L₂`）；下界 0.4
/// 可达（n 与两灯点积同时 ≤ 0，如 n = (0, −1, 0)）。
///
/// ## ⚠️ 历史注记：非单位光向量入参可越界
///
/// 若调用方拿**非单位**光向量直接点积（2026-10-28 前的常量口径），
/// `I` 可超 1（旧常量下全定义域上界 ≈ 1.0518，在 n ∝ (−0.36, 0.6, 0.6)
/// 处取得）——越界即亮部溢出，事后钳制救不回层次。现常量已单位化，
/// 此坑只在未来改光向时需要警惕：**改常量必须保持模长 1**（锚点测试
/// `lighting_constants_are_unit_vectors_at_45_degree_elevation`）。
pub fn lighting_intensity(n_dot_l1: f32, n_dot_l2: f32) -> f32 {
    AMBIENT + DIFFUSE_MAIN * n_dot_l1.max(0.0) + DIFFUSE_AUX * n_dot_l2.max(0.0)
}

/// Horn 3×3 梯度：`[1,2,1]ᵀ·[1,2,1]/8` 加权隔格差分，返回 `(hx, hy)` **米/格**。
///
/// 为什么 Horn 而不是中心差分：手绘/生成数据有台阶毛刺，中心权重 4、
/// 直邻 2、对角 1 的加权平滑能抑制单格噪声（ADR-0077 D2）——Horn 起是
/// 晕渲法线的**唯一梯度来源**（GPU 双三次解析梯度与 CPU 中心差分退役；
/// 高度**取值**仍走双三次插值，两者分工不变）。
///
/// 坐标约定：`x` 列向右、`y` 行号**向下**增大（南）；`hx > 0` = 高度向右
/// 增大、`hy > 0` = 高度**向南**增大。邻域 `clamp` 在图内
///（`0..width-1 / 0..height-1`），边界格不 panic、结果有限——边缘处
/// 梯度退化为中心半格差，晕渲可接受（外推假想坡比切断渲染更不可取）。
/// `y` 分量翻转的世界坐标含义在 [`normal_world`]（CPU/GPU 对齐锚点）。
pub fn horn_gradient(data: &TerrainData, x: i64, y: i64) -> (f32, f32) {
    let w = data.width as i64;
    let h = data.height as i64;
    if w == 0 || h == 0 || data.cells.is_empty() {
        // 空图没有坡可言；防止下方 clamp(0, -1) panic。
        return (0.0, 0.0);
    }
    let at = |dx: i64, dy: i64| -> f32 {
        let xi = (x + dx).clamp(0, w - 1) as usize;
        let yi = (y + dy).clamp(0, h - 1) as usize;
        data.cells[yi * w as usize + xi].height as f32
    };
    // 可分离核：列向 = (右列 [1,2,1] 和 − 左列 [1,2,1] 和)/8，行向同理。
    let left = at(-1, -1) + 2.0 * at(-1, 0) + at(-1, 1);
    let right = at(1, -1) + 2.0 * at(1, 0) + at(1, 1);
    let top = at(-1, -1) + 2.0 * at(0, -1) + at(1, -1);
    let bottom = at(-1, 1) + 2.0 * at(0, 1) + at(1, 1);
    ((right - left) / 8.0, (bottom - top) / 8.0)
}

/// 梯度 → 世界坐标法线：`normalize(−s·hx, +s·hy, 1)`。
///
/// ## ⚠️ `+s·hy`（不是 −s）——CPU/GPU 对齐锚点，两侧不许各改各的
///
/// 数据行号向下增大（南），世界坐标 **+y = 北**——法线的 y 分量在
/// 此**翻转一次**。这样「高度向南增大」的坡（`hy > 0`）法线朝北
///（`normal[1] > 0`），被东北—西北方向的主/辅光（`L.y = +0.5`）照亮：
/// **山影倒向南方、坡面朝左上受光**。本公式的受光方向锚点 =
/// `field/material.rs` 的 `LIGHT_DIR` + 纹理空间法线（GPU 路径，制图学
/// 西北受光惯例）。注意：`cells.rs::relief_mul` 的 `lit = −(dx + dy)`
/// 与本公式**逐轴相反**——其最亮分支（dx < 0 且 dy < 0）恰是本公式的
/// 背光分支（本公式最亮条件 hx > 0 且 hy > 0），属该旧 CPU 路径的既有
/// 缺陷；Task 4 切换后 CPU/GPU 方向统一到本公式（= material.rs）口径，
/// relief_mul 随路径退役。此翻转是全局约定：换光源方位必须两侧同步，
/// 不允许单侧改。
///
/// WGSL 侧对应约定（实施 WGSL 的任务必须把本段抄进 shader 注释，与本
/// 函数互指）：**纹理空间 v 与数据行同向、与世界 y 反向**，梯度在纹理
/// 空间取（`+v` 向南），法线写 `normalize(vec3(-zx, -zy, 1.0))` **不再翻**
/// ——本函数的一次翻转已隐含在「纹理空间 → 世界坐标」的变换里。
/// 两侧若同时翻或不翻，南北光照镜像，视觉验收立刻可察。
///
/// 健壮性契约：任何输入（含 `slope_gain = inf` 这类退化）都返回**有限的
/// 单位法线**——退化时回落平地法线 `[0, 0, 1]`。这是给 CPU/GPU 双侧
/// 消费的公式真源，不允许一侧 nan 一侧正常的分叉。
pub fn normal_world(hx: f32, hy: f32, slope_gain: f32) -> [f32; 3] {
    let nx = -slope_gain * hx;
    // + 号即上文翻转锚点：数据行向下（南）→ 世界 +y（北）。
    let ny = slope_gain * hy;
    let nz = 1.0;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if !len.is_finite() || len == 0.0 {
        return [0.0, 0.0, 1.0];
    }
    [nx / len, ny / len, nz / len]
}

/// detail 档权重衰减：`smoothstep(1.0, 3.0, px_per_cell)`。
///
/// 远观（< 1 px/格）细纹理在屏幕上是噪点 → detail 归零；近观
///（≥ 3 px/格）细部可信 → 全额。曲线必须随 px_per_cell **单调**
///（ADR-0077 D3 的验收口径），smoothstep 保证之。
pub fn detail_weight_fade(px_per_cell: f32) -> f32 {
    let t = ((px_per_cell - DETAIL_FADE_MIN_PX_PER_CELL)
        / (DETAIL_FADE_MAX_PX_PER_CELL - DETAIL_FADE_MIN_PX_PER_CELL))
        .clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 三尺度权重 `[macro, medium, detail]`，**和恒为 1**（不再钳制混合结果）。
///
/// detail 份额 = [`W_DETAIL`] × [`detail_weight_fade`]；衰减掉的部分按
/// 宏观：中观的既有比例（0.45 : 0.40）回流两档——衰减是「把噪声预算
/// 换给宏观结构」，不是整体减光（ADR-0077 D3 取代 T33 关灯的理由）。
pub fn scale_weights(px_per_cell: f32) -> [f32; 3] {
    let detail = W_DETAIL * detail_weight_fade(px_per_cell);
    let back = W_DETAIL - detail; // 衰减量，按既有比例回流
    let base = W_MACRO + W_MEDIUM;
    let macro_w = W_MACRO + back * (W_MACRO / base);
    let medium_w = W_MEDIUM + back * (W_MEDIUM / base);
    [macro_w, medium_w, detail]
}

// ---------------------------------------------------------------------------
// 测试（期望值全部手算锚点——「跑出来的值就是对的」不接受，AGENTS §7.2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::TerrainCellData;

    const EPS: f32 = 1e-6;

    /// 3×3 定高 fixture：行优先 `heights`，surface/biome 填 0。
    fn grid3(width: u32, height: u32, heights: &[i16]) -> TerrainData {
        assert_eq!(heights.len(), (width * height) as usize);
        TerrainData {
            width,
            height,
            cells: heights
                .iter()
                .map(|&h| TerrainCellData {
                    height: h,
                    surface: 0,
                    biome: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn lighting_intensity_matches_hand_values_and_stays_in_bounds() {
        // 全受光（两盏灯正对）：0.4 + 0.48 + 0.12 = 1.0
        let full = lighting_intensity(1.0, 1.0);
        assert!((full - 1.0).abs() < EPS, "got {}", full);
        // 纯背光：只吃环境光 0.4
        let dark = lighting_intensity(0.0, 0.0);
        assert!((dark - 0.4).abs() < EPS, "got {}", dark);
        // 单灯半受光：0.4 + 0.48·0.5 = 0.64
        let half = lighting_intensity(0.5, 0.0);
        assert!((half - 0.64).abs() < EPS, "got {}", half);
        // 负点积截断：0.4 + 0 + 0.12·0.5 = 0.46（max(0,·) 的手算证据）
        let clamped = lighting_intensity(-1.0, 0.5);
        assert!((clamped - 0.46).abs() < EPS, "got {}", clamped);
        // 平地（法线 z=1，两灯各得 1/√2）：0.4 + 0.6·(1/√2) = 0.82426407
        let flat = lighting_intensity(FRAC_1_SQRT_2, FRAC_1_SQRT_2);
        assert!(
            (flat - (0.4 + 0.6 * FRAC_1_SQRT_2)).abs() < EPS,
            "got {}",
            flat
        );
        // 界内性对抽样点成立（公式性质：[0.4, 1.0]，见函数文档）
        for &(n1, n2) in &[
            (0.0, 0.0),
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
            (0.5, 0.5),
            (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
            (1e-6, 1e-6),
        ] {
            let i = lighting_intensity(n1, n2);
            assert!(
                (0.4 - EPS..=1.0 + EPS).contains(&i),
                "N·L=({n1},{n2}) -> I={} 越界",
                i
            );
        }
    }

    #[test]
    fn lighting_coefficients_sum_to_one() {
        // 和 = 1 是「免钳制」的前提（模块文档）；漂移即破坏界内性。
        let sum = AMBIENT + DIFFUSE_MAIN + DIFFUSE_AUX;
        assert!((sum - 1.0).abs() < EPS, "got {}", sum);
    }

    #[test]
    fn horn_gradient_hand_fixture() {
        // 左列全 0、右列全 10（中列 0）：
        // hx = (10 + 2·10 + 10 − 0)/8 = 5.0；行向对称 → hy = 0.0
        let cols = grid3(3, 3, &[0, 0, 10, 0, 0, 10, 0, 0, 10]);
        let (hx, hy) = horn_gradient(&cols, 1, 1);
        assert!((hx - 5.0).abs() < EPS, "got {}", hx);
        assert!((hy - 0.0).abs() < EPS, "got {}", hy);
        // 行向坡：顶行全 0、底行全 10 → hy = 40/8 = 5.0，hx = 0.0
        let rows = grid3(3, 3, &[0, 0, 0, 0, 0, 0, 10, 10, 10]);
        let (hx, hy) = horn_gradient(&rows, 1, 1);
        assert!((hx - 0.0).abs() < EPS, "got {}", hx);
        assert!((hy - 5.0).abs() < EPS, "got {}", hy);
    }

    #[test]
    fn lighting_constants_are_unit_vectors_at_45_degree_elevation() {
        // spec D1：方位 315°/45°、高度角 45° ⇒ 单位向量 (±0.5, 0.5, 1/√2)，
        // 模长 1（0.25 + 0.25 + 0.5 = 1）——`lighting_intensity` 的「直接
        // dot」契约以此为前提，模长漂移即界内性失效（见该函数历史注记）。
        for l in [LIGHT_MAIN, LIGHT_AUX] {
            let len2 = l[0] * l[0] + l[1] * l[1] + l[2] * l[2];
            assert!((len2 - 1.0).abs() < EPS, "模长² = {len2} 应为 1");
            assert!(
                (l[2] - FRAC_1_SQRT_2).abs() < EPS,
                "z = {:?} 应为 sin45°",
                l[2]
            );
            assert!((l[1] - 0.5).abs() < EPS, "y = {:?} 应为 0.5", l[1]);
        }
        assert!((LIGHT_MAIN[0] + 0.5).abs() < EPS, "主光从西北（x = −0.5）");
        assert!((LIGHT_AUX[0] - 0.5).abs() < EPS, "辅光从东北（x = +0.5）");
        // 两灯只差 x 符号（盲区互补的结构前提）。
        assert_eq!(LIGHT_MAIN[1], LIGHT_AUX[1]);
        assert_eq!(LIGHT_MAIN[2], LIGHT_AUX[2]);
    }

    #[test]
    fn normal_world_north_facing_slope_is_lit_by_main_light() {
        // 高度向南增大（hy > 0）→ 坡面朝北 → normal[1] > 0（y 翻转锚点）。
        // 45° 坡：normalize(0, 1, 1) = (0, 1/√2, 1/√2)。
        let n = normal_world(0.0, 1.0, 1.0);
        assert!(n[1] > 0.0, "got {}", n[1]);
        assert!((n[1] - FRAC_1_SQRT_2).abs() < EPS, "got {}", n[1]);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < EPS, "got {}", len);
        // 单位光向量的原始点积即合法入参：
        // 0·(−0.5) + (1/√2)·0.5 + (1/√2)·(1/√2) = 0.5/√2 + 0.5 = 0.8535533；
        // 北向坡正对两灯的公共 y/z 锥，n·LIGHT_AUX 同值。
        let dot = n[0] * LIGHT_MAIN[0] + n[1] * LIGHT_MAIN[1] + n[2] * LIGHT_MAIN[2];
        assert!(dot > 0.0, "got {}", dot);
        assert!(
            (dot - (0.5 + FRAC_1_SQRT_2 / 2.0)).abs() < EPS,
            "got {}",
            dot
        );
        let i = lighting_intensity(
            dot,
            n[0] * LIGHT_AUX[0] + n[1] * LIGHT_AUX[1] + n[2] * LIGHT_AUX[2],
        );
        assert!(
            (i - (0.4 + 0.6 * (0.5 + FRAC_1_SQRT_2 / 2.0))).abs() < EPS,
            "got {}",
            i
        );
    }

    #[test]
    fn same_physical_slope_yields_same_normal_across_maps() {
        // 同一物理坡（垂直 2 : 水平 100，即 2% 坡）：
        // 甲图 100 m/格 → hx=100, s = 2/100 = 0.02 → s·hx = 2.0
        // 乙图  50 m/格 → hx=50,  s = 2/50  = 0.04 → s·hx = 2.0
        // 米制化的全部意义：法线与格距无关。
        let a = normal_world(100.0, 0.0, slope_gain_of(100.0));
        let b = normal_world(50.0, 0.0, slope_gain_of(50.0));
        for k in 0..3 {
            assert!(
                (a[k] - b[k]).abs() < EPS,
                "分量 {} 不等: a={} b={}",
                k,
                a[k],
                b[k]
            );
        }
        // 手算锚点：normalize(−2, 0, 1) = (−2/√5, 0, 1/√5)
        let inv_sqrt5 = 1.0 / 5.0f32.sqrt();
        assert!((a[0] - (-2.0 * inv_sqrt5)).abs() < EPS, "got {}", a[0]);
        assert!((a[2] - inv_sqrt5).abs() < EPS, "got {}", a[2]);
    }

    #[test]
    fn extreme_cliff_stays_finite_and_in_bounds() {
        // 邻格 ±9000 m 断崖：hx = (9000·4 − (−9000)·4)/8 = 9000 米/格，
        // 1 m/格图（s = 2）→ s·hx = 18000，远超 f32 顾虑线但仍有限。
        let cliff = grid3(3, 3, &[-9000, 0, 9000, -9000, 0, 9000, -9000, 0, 9000]);
        let (hx, hy) = horn_gradient(&cliff, 1, 1);
        assert!((hx - 9000.0).abs() < 1.0, "got {}", hx);
        assert!(hy.is_finite(), "got {}", hy);
        let n = normal_world(hx, hy, slope_gain_of(meters_per_cell_of(1.0, None)));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(len.is_finite(), "got {}", len);
        assert!((len - 1.0).abs() < 1e-4, "got {}", len);
        let i = lighting_intensity(
            n[0] * LIGHT_MAIN[0] + n[1] * LIGHT_MAIN[1] + n[2] * LIGHT_MAIN[2],
            n[0] * LIGHT_AUX[0] + n[1] * LIGHT_AUX[1] + n[2] * LIGHT_AUX[2],
        );
        assert!(
            (0.4 - 1e-4..=1.0 + 1e-4).contains(&i),
            "断崖明度越界: got {}",
            i
        );
    }

    #[test]
    fn horn_gradient_clamps_at_grid_edges() {
        // 1×1：九邻域全 clamp 到同一格 → 梯度必为 0，不 panic。
        let one = grid3(1, 1, &[42]);
        let (hx, hy) = horn_gradient(&one, 0, 0);
        assert_eq!((hx, hy), (0.0, 0.0));
        // 2×2（全格都是边界格），角落 (0,0) 邻域 clamp 手算：
        // 右下格 30、右上 10、左下 20 → hx = (10+20+30 − 0−0−20)/8 = 5.0
        // hy = (20+40+30 − 0−0−10)/8 = 10.0
        let two = grid3(2, 2, &[0, 10, 20, 30]);
        let (hx, hy) = horn_gradient(&two, 0, 0);
        assert!((hx - 5.0).abs() < EPS, "got {}", hx);
        assert!((hy - 10.0).abs() < EPS, "got {}", hy);
        // 越界坐标也要 clamp 而非 panic（调用方是逐格烘焙，防御在函数内）。
        let (hx, hy) = horn_gradient(&two, -5, 99);
        assert!(hx.is_finite() && hy.is_finite(), "got ({}, {})", hx, hy);
        // 全水 fixture：surface=0 不参与梯度，平高场 → 零梯度。
        let water = grid3(3, 3, &[120; 9]);
        let (hx, hy) = horn_gradient(&water, 1, 1);
        assert_eq!((hx, hy), (0.0, 0.0));
    }

    #[test]
    fn meters_per_cell_without_scale_falls_back_to_one_wu_one_m() {
        // None → 1 wu = 1 m 回落：原样返回格距（README §12-53 待决）。
        let none = meters_per_cell_of(4.0, None);
        assert!((none - 4.0).abs() < EPS, "got {}", none);
        // Some(8)：1 km = 8 wu → 1 wu = 125 m → 4 wu = 500 m（= 4×125）。
        let scaled = meters_per_cell_of(4.0, Some(8.0));
        assert!((scaled - 500.0).abs() < EPS, "got {}", scaled);
        // k <= 0 与 None 同待遇（除零 / 负比例尺没有意义）。
        let zero = meters_per_cell_of(4.0, Some(0.0));
        let neg = meters_per_cell_of(4.0, Some(-2.0));
        assert!((zero - 4.0).abs() < EPS, "got {}", zero);
        assert!((neg - 4.0).abs() < EPS, "got {}", neg);
    }

    #[test]
    fn scale_weights_sum_to_one_and_detail_fades_monotonically() {
        // 手算 smoothstep(1, 3, ·)：t = (ppc−1)/2，s = t²(3−2t)。
        // ppc=2.0 → t=0.5 → fade=0.5 → detail = 0.15·0.5 = 0.075，
        // 回流 0.075 按 45:40 分：macro = 0.45+0.075·(45/85) = 0.4897059，
        // medium = 0.40+0.075·(40/85) = 0.4352941。
        let mid = scale_weights(2.0);
        assert!((mid[2] - 0.075).abs() < EPS, "got {}", mid[2]);
        assert!((mid[0] - 0.4897059).abs() < EPS, "got {}", mid[0]);
        assert!((mid[1] - 0.4352941).abs() < EPS, "got {}", mid[1]);
        // 远观 0.5 px/格：fade=0 → detail=0，全额回流。
        let far = scale_weights(0.5);
        assert!((far[2] - 0.0).abs() < EPS, "got {}", far[2]);
        // 近观 ≥ 3 px/格：detail 全额 0.15，回到基线 (0.45, 0.40, 0.15)。
        for ppc in [3.0, 4.0] {
            let near = scale_weights(ppc);
            assert!(
                (near[2] - W_DETAIL).abs() < EPS,
                "ppc={} got {}",
                ppc,
                near[2]
            );
            assert!(
                (near[0] - W_MACRO).abs() < EPS,
                "ppc={} got {}",
                ppc,
                near[0]
            );
            assert!(
                (near[1] - W_MEDIUM).abs() < EPS,
                "ppc={} got {}",
                ppc,
                near[1]
            );
        }
        // 每个采样点三权和 = 1 ± 1e-6，detail 单调不减（ADR-0077 D3 口径）。
        let mut prev_detail = -1.0;
        let ppc_samples = [0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0];
        for &ppc in &ppc_samples {
            let w = scale_weights(ppc);
            let sum = w[0] + w[1] + w[2];
            assert!((sum - 1.0).abs() < EPS, "ppc={} sum={}", ppc, sum);
            assert!(
                w[2] >= prev_detail,
                "ppc={} detail={} < prev={}",
                ppc,
                w[2],
                prev_detail
            );
            prev_detail = w[2];
        }
    }
}
