//! 分层设色（hypsometric tint，ADR-0055 / ADR-0056）：CellMap 用**颜色**读出海拔。
//!
//! ## 为什么需要它
//!
//! CellMap 底图 = 群系固定色块（ADR-0051 §5），**高度不参与颜色**；现有
//! hillshade 只表达「坡」——两片同样平的高原与平原，颜色与明暗完全一致。
//! 本模块按「8 阶海拔色带」给**陆格**叠一层海拔色，与 hillshade 正交：
//! **色带读「高」、晕渲读「坡」**。
//!
//! ## 阈值口径（ADR-0056 修正 ADR-0055 的「百分位」）
//!
//! 1. **绝对地形学阶梯** [`DOMAIN_CUTS`]：`200 / 500 / 1000 / 1500 / 2500 / 4000 / 6000` 米。
//!    锚点与生成侧的地形语义同源（山地 1150–1500 / 雪线 2250–2750，ADR-0042），
//!    读图者可直接把颜色换算成高度；**笔刷抬一格山头不会改变别处的颜色**
//!    （阈值是常量，不随数据漂移）。
//! 2. **窄域兜底**（[`NARROW_SPAN_M`]）：陆地高程跨度 < 300 m 的图（沿海小岛 /
//!    滩涂）若照绝对阶梯，8 阶里只用得到 1–2 阶、整幅同色——此时改用**该图自身
//!    的 [min, max]（按 [`RANGE_QUANT_M`] 量化）等分 8 阶**，保住起伏可读。
//!
//! 为什么不沿用百分位：它保证「每阶人口相等」，但**阈值随数据漂移**——
//! 抬一格山头整图重洗；且在 CPU 增量补丁路径上，脏区用新阶、区外用旧阶，
//! 边界会出接缝（ADR-0056 §1）。
//!
//! ## 为什么色带固定、不进 ThemePack
//!
//! ADR-0051 §5 / ADR-0054 D2：CellMap 是数据视图，换主题不许影响它。
//! 故本表是 crate 级常量，不接收任何 palette 参数（签名保证）。

use wv_core::terrain::TerrainCellData;

/// 阶数（ADR-0055：8 阶，用户裁定）。
pub const BANDS: usize = 8;

/// 内部切分阈值个数（`BANDS` 阶 = `BANDS − 1` 条内边界）。
pub const CUTS: usize = BANDS - 1;

/// 8 阶固定色带（sRGB，低 → 高）。
///
/// **配色原则**：与 [`super::biome_raw`] 同族——低饱和地学配色，明度单调上升
/// （低 = 深绿，高 = 近纸白）。不设绝对雪线色：最高阶的色相是「高岩 + 雪」的
/// 混合倾向，是否真的是雪由绝对阶梯（`>= 2500 m` 起进入两高阶）决定。
///
/// **改这里的值 = 改 CellMap 的数据视图语义**，不要为「好看」随手调。
pub const RAMP_SRGB: [[u8; 3]; BANDS] = [
    [0x5E, 0x7F, 0x55], // 0 最低：深草绿
    [0x6F, 0x91, 0x60], // 1
    [0x82, 0xA2, 0x6B], // 2
    [0x97, 0xAE, 0x79], // 3
    [0xAC, 0xB8, 0x8A], // 4 中位：黄绿
    [0xC0, 0xBE, 0x9C], // 5
    [0xD0, 0xC8, 0xAE], // 6 高地：暖灰褐
    [0xDF, 0xDC, 0xD2], // 7 最高：近纸白
];

/// **绝对地形学阶梯**（米，升序）：7 条内边界 = 8 阶（ADR-0056 决策 1）。
///
/// 与生成侧已有的地形语义同源（山地 1150–1500 / 雪线 2250–2750，ADR-0042），
/// 故颜色的含义与世界生成规则一致：
///
/// | 阶 | 高度区间（米） | 读作 |
/// |---|---|---|
/// | 0 | < 200 | 低地 / 平原 |
/// | 1 | 200–500 | 缓丘 |
/// | 2 | 500–1000 | 丘陵 |
/// | 3 | 1000–1500 | 山地 |
/// | 4 | 1500–2500 | 高山 |
/// | 5 | 2500–4000 | 雪线以上 |
/// | 6 | 4000–6000 | 极高山 |
/// | 7 | ≥ 6000 | 上限（pack 编码钳位） |
///
/// **不落盘**：纯渲染常量（P3）；改它 = 改数据视图语义，需 ADR。
pub const DOMAIN_CUTS: Cuts = [200, 500, 1000, 1500, 2500, 4000, 6000];

/// 窄域兜底阈值（米）：陆地高程跨度小于它 → 改用该图自身的等分（[`cuts_for_cells`]）。
///
/// 300 m 的依据：绝对阶梯的头两档合计 500 m；跨度不足 300 m 的图在绝对阶梯下
/// 只会落在 1–2 档内，起伏读不出来，而 300 m 已足够显示「滩涂 vs 小丘」。
pub const NARROW_SPAN_M: i32 = 300;

/// 窄域兜底的**范围量化步长**（米）。
///
/// 把 `[min, max]` 向外取整到 100 m 的整数倍：小幅笔刷（±10 m）不会改变
/// 兜底阶梯，避免「拖一笔整图重洗」——ADR-0056 §4 记的已知残余。
pub const RANGE_QUANT_M: i32 = 100;

/// 内边界（米，升序）。7 条。
pub type Cuts = [i16; CUTS];

/// 该格是否按**水**处理（与湿判定同口径：作者标的水 ‖ 海平面以下）。
///
/// 与 `wv_terrain::edit::is_water_cell` 同语义；在此处复写是为了让
/// 「哪些格进高程区间 / 哪些格不叠色」只有一个入口。
pub fn is_water_cell(cell: &TerrainCellData) -> bool {
    cell.surface == wv_terrain::SURFACE_WATER || cell.height < 0
}

/// 求 7 条内边界（ADR-0056 决策 1/2）：**绝对阶梯优先，窄域兜底**。
///
/// - 陆地高程跨度 `>= NARROW_SPAN_M`（或全水 / 空图）→ [`DOMAIN_CUTS`]；
/// - 跨度 `< NARROW_SPAN_M` → 把该图的 `[min, max]`（按 [`RANGE_QUANT_M`]
///   向外量化后）等分 8 阶，让起伏仍然可分。
///
/// 复杂度 `O(N)`（只求 min/max，**不排序**）——这是相对百分位方案的直接收益：
/// 整图重打 / 每次补丁都能廉价重算。
pub fn cuts_for_cells(cells: &[TerrainCellData]) -> Cuts {
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    for cell in cells {
        if is_water_cell(cell) {
            continue;
        }
        lo = lo.min(cell.height as i32);
        hi = hi.max(cell.height as i32);
    }
    if lo > hi {
        // 全水 / 空图：没有陆格可着色，返回绝对阶梯（结果与着色侧无关）。
        return DOMAIN_CUTS;
    }
    let q = RANGE_QUANT_M;
    let lo_q = lo.div_euclid(q) * q;
    let hi_q = (hi + q - 1).div_euclid(q) * q;
    let span = hi_q - lo_q;
    if span <= 0 || span >= NARROW_SPAN_M {
        return DOMAIN_CUTS;
    }
    // 窄域：把 [lo_q, hi_q] 等分 8 阶（i/8 处取整，保证严格递增）。
    let mut cuts: Cuts = [0; CUTS];
    for (i, cut) in cuts.iter_mut().enumerate() {
        let step = (hi_q - lo_q) as f32 * (i + 1) as f32 / BANDS as f32;
        *cut = (lo_q + step.round() as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    }
    cuts
}

/// 高度落在第几阶（0..BANDS）。边界语义：`height >= cut` 即进下一阶。
pub fn band_of(height: i16, cuts: &Cuts) -> usize {
    let mut band = 0;
    for cut in cuts {
        if height >= *cut {
            band += 1;
        }
    }
    band.min(BANDS - 1)
}

/// 某一阶的色带色（`[f32; 4]`，0–1，与 [`super::biome_raw::pick`] 同口径）。
pub fn ramp_rgba(band: usize) -> [f32; 4] {
    let rgb = RAMP_SRGB[band.min(BANDS - 1)];
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    ]
}

/// 把色带色按 `strength`（0 = 关闭，1 = 完全替代群系色）叠到基色上。
///
/// 在 **sRGB 空间**混合：基色（`biome_raw::pick`）与 `RAMP_SRGB` 都是取色器
/// 口径的 sRGB 值，在别处混会得到偏亮的观感（本项目已踩过色彩空间坑）。
pub fn tint_rgba(base: [f32; 4], band: usize, strength: f32) -> [f32; 4] {
    let t = strength.clamp(0.0, 1.0);
    if t == 0.0 {
        return base;
    }
    let ramp = ramp_rgba(band);
    [
        base[0] + (ramp[0] - base[0]) * t,
        base[1] + (ramp[1] - base[1]) * t,
        base[2] + (ramp[2] - base[2]) * t,
        base[3],
    ]
}

// ── 段内连续插值（对 ADR-0055「8 阶离散」的 CellMap 数据视图修正）──

/// 把高度换算成 `(段号 b, 段内分数 t)`，供 [`tint_continuous`] 做段内渐变。
///
/// **对 ADR-0055 的修正**：ADR-0055 裁定的是「8 阶离散色带（用户裁定）」，
/// 实机暴露出「低地 0–200 m 一刀同色」——200 m 以内的缓丘与海岸平原在
/// 离散口径下不可分。CellMap 数据视图路径据此修正为「**8 锚点连续渐变**」：
/// 段号沿用 [`band_of`]（边界语义不变：`height >= cut` 进下一段），段内再给
/// 一个连续位置 `t ∈ [0, 1)`，颜色在 `ramp[b]` 与 `ramp[b+1]` 间线性过渡。
/// **`ElevationBand` / `BAND_WEIGHT` 的阶梯语义不动**——两者定义并接线于
/// **本 crate** `terrain_view/cells.rs`（wv-render 是下游消费方，不持语义）；
/// 那条路径刻意让低地零混入，见 [`BAND_WEIGHT`]。
///
/// 段内分数 `t` 的口径：
///
/// - `b = 0`（低地段）：下界按 `cuts[0]` 的**等宽外推**，`t = height / cuts[0]`。
///   正常调用里 `height < 0` 已被 [`is_water_cell`] 水域门挡住，负值在此仅
///   防御性钳回 0；`cuts[0] <= 0` 的退化输入无法外推，同样记 0。
/// - `1 <= b < BANDS − 1`：`t = (height − cuts[b−1]) / (cuts[b] − cuts[b−1])`。
/// - `b = BANDS − 1`（最高段）：无上界，`t` 恒 0（即纯 `ramp[BANDS − 1]`）。
pub fn segment_of(height: i16, cuts: &Cuts) -> (usize, f32) {
    let b = band_of(height, cuts);
    let t = if b == 0 {
        let c0 = cuts[0] as f32;
        if c0 > 0.0 {
            height as f32 / c0
        } else {
            0.0
        }
    } else if b < BANDS - 1 {
        let lo = cuts[b - 1] as f32;
        let hi = cuts[b] as f32;
        if hi > lo {
            (height as f32 - lo) / (hi - lo)
        } else {
            0.0
        }
    } else {
        // 最高段没有上界：t 恒 0（纯 ramp[BANDS-1]）。
        0.0
    };
    // t 钳 [0, 1)：负值（水上溯防御）钳 0；上界按构造即 < 1（越过 cuts[b]
    // 的高度会先被 band_of 升段），钳位只防御手造的退化 cuts。
    (b, t.clamp(0.0, 1.0 - f32::EPSILON))
}

/// 段内连续版的高程叠色：**8 锚点连续渐变**（sRGB 直混，与 [`tint_rgba`]
/// 同口径）。
///
/// `ramp[b]` 与 `ramp[b+1]` 按段内分数 `t` lerp（`b = BANDS − 1` 时 `t` 恒 0，
/// 即纯 `ramp[BANDS − 1]`），再按 `strength` 混入 `base`。混合仍在 **sRGB
/// 空间**——理由见 [`tint_rgba`]：取色器口径的 sRGB 值在别的空间混会偏亮
/// （本项目踩过这坑）。
///
/// 对 ADR-0055 的修正关系见 [`segment_of`]：锚点（恰在 `cuts` 各条边界上）
/// 处 `t = 0`，本函数逐通道退化为 [`tint_rgba`]——连续渐变是离散色带的
/// 超集，锚点处两者相等。
pub fn tint_continuous(base: [f32; 4], height: i16, cuts: &Cuts, strength: f32) -> [f32; 4] {
    let s = strength.clamp(0.0, 1.0);
    if s == 0.0 {
        return base;
    }
    let (b, t) = segment_of(height, cuts);
    let lo = ramp_rgba(b);
    let hi = ramp_rgba((b + 1).min(BANDS - 1));
    let lerped = [
        lo[0] + (hi[0] - lo[0]) * t,
        lo[1] + (hi[1] - lo[1]) * t,
        lo[2] + (hi[2] - lo[2]) * t,
        1.0,
    ];
    [
        base[0] + (lerped[0] - base[0]) * s,
        base[1] + (lerped[1] - base[1]) * s,
        base[2] + (lerped[2] - base[2]) * s,
        base[3],
    ]
}

// ── 专业制图的海拔色带（ADR-0067 D3）──

/// **风格自带的**海拔色带（与本模块的固定 8 阶同结构，不同数据源）。
///
/// 本模块的 [`RAMP_SRGB`] 是 **crate 级固定色**（ADR-0054 D4：数据视图不随
/// 主题变）；本结构是**风格层给的**色带——专业制图的色带要跟风格走
/// （ADR-0060 §4：「配色跟主题，不复用 `hypsometric` 固定 8 阶」）。
///
/// 切点仍复用 [`cuts_for_cells`]（绝对阶梯 + 窄域兜底）：阈值是**地形事实**，
/// 与配色无关，故两条路径同源。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationBand {
    /// 8 阶色带（sRGB 三元组，低 → 高）。
    pub ramp: [[u8; 3]; BANDS],
    /// 7 条内边界（米，升序）。
    pub cuts: Cuts,
    /// 混入强度（0..=1）。
    pub strength: f32,
}

/// 各阶的**混入权重**（`strength` 的倍率，低 → 高）。
///
/// 第 0 阶必须是 `0.0`：色带首色与专业制图的近白底图同色，若照样按
/// `strength` 混入，会把**低地上的群系色**（公园绿 `#C9E7C7` / 湿绿）整体
/// 洗向近白——2026-10-26 实机截图里绿地退成 `#E6F2E6` 正是这个原因
/// （0.62 的近白混入 = 绿地只剩 38% 本色）。色带只该在「真山」上出现，
/// 低地保持群系本色。
///
/// 中段略高于线性（约 `band/7` 的 0.8 次幂），让丘陵带仍有可读的起伏；
/// 写成常量表是为了避开逐格 `powf`（烘焙是百万格级的热循环）。
const BAND_WEIGHT: [f32; BANDS] = [0.0, 0.26, 0.41, 0.55, 0.67, 0.79, 0.90, 1.0];

impl ElevationBand {
    /// 某一阶的色（sRGB 0–1）。
    fn ramp_rgb(&self, band: usize) -> [f32; 3] {
        let rgb = self.ramp[band.min(BANDS - 1)];
        [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
        ]
    }

    /// 把色带色按 `strength × 阶权重` 叠到基色上（sRGB 空间混合，同
    /// [`tint_rgba`] 口径）。
    ///
    /// 权重见 [`BAND_WEIGHT`]：低地权重为 0（原色不动），越高越接近
    /// `strength`——「色带读高」与「低地读群系」互不干扰。
    pub fn tint(&self, base: [f32; 4], height: i16) -> [f32; 4] {
        let strength = self.strength.clamp(0.0, 1.0);
        if strength == 0.0 {
            return base;
        }
        let band = band_of(height, &self.cuts);
        let t = strength * BAND_WEIGHT[band];
        if t == 0.0 {
            return base;
        }
        let ramp = self.ramp_rgb(band);
        [
            base[0] + (ramp[0] - base[0]) * t,
            base[1] + (ramp[1] - base[1]) * t,
            base[2] + (ramp[2] - base[2]) * t,
            base[3],
        ]
    }
}

/// 把米制内边界换算成 `R16Unorm` 高度纹理的归一化域（GPU 侧 uniform 用）。
///
/// `norm = (h − h_min) / h_range`，与 `field::pack::H_MIN` / `H_MAX` 同口径；
/// 越界钳到 `[0, 1]`——pack 编码本身也钳位（`encode_unorm16`），两侧一致。
pub fn normalized_cuts(cuts: &Cuts, h_min: f32, h_range: f32) -> [f32; CUTS] {
    let mut out = [0.0f32; CUTS];
    for (dst, cut) in out.iter_mut().zip(cuts.iter()) {
        *dst = ((*cut as f32 - h_min) / h_range).clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(height: i16, surface: u8) -> TerrainCellData {
        TerrainCellData {
            height,
            surface,
            biome: 0,
        }
    }

    /// 绝对阶梯就是那张表：升序、7 条，且首条 = 200 m。
    #[test]
    fn domain_cuts_are_the_absolute_terrain_ladder() {
        assert_eq!(DOMAIN_CUTS, [200, 500, 1000, 1500, 2500, 4000, 6000]);
        for window in DOMAIN_CUTS.windows(2) {
            assert!(window[1] > window[0], "阶梯必须严格升序：{window:?}");
        }
    }

    /// 跨度足够的图（0..5000 m）→ 用绝对阶梯，与图的内容无关。
    #[test]
    fn wide_maps_use_domain_cuts() {
        let cells: Vec<TerrainCellData> = (0..=50).map(|h| cell(h * 100, 1)).collect();
        assert_eq!(cuts_for_cells(&cells), DOMAIN_CUTS);
    }

    /// 窄域图（0..100 m）→ 兜底：按自身区间等分，8 阶全用上。
    #[test]
    fn narrow_maps_split_their_own_range() {
        let cells: Vec<TerrainCellData> = (0..=100).map(|h| cell(h, 1)).collect();
        let cuts = cuts_for_cells(&cells);
        assert_eq!(cuts, [13, 25, 38, 50, 63, 75, 88], "100 m 区间 1/8..7/8");

        let mut used = [false; BANDS];
        for c in &cells {
            used[band_of(c.height, &cuts)] = true;
        }
        assert!(used.iter().all(|u| *u), "窄域兜底必须用到全部 8 阶");
    }

    /// 兜底阶梯按 100 m 量化：小幅笔刷不改阶梯（避免整图重洗）。
    #[test]
    fn narrow_cuts_are_quantized_against_small_edits() {
        let base: Vec<TerrainCellData> = (0..=90).map(|h| cell(h, 1)).collect();
        // 95 m 仍落在 [0, 100) 的量化桶内 → 阶梯不变。
        let nudged: Vec<TerrainCellData> =
            (0..=90).map(|h| cell(h, 1)).chain([cell(95, 1)]).collect();
        assert_eq!(
            cuts_for_cells(&base),
            cuts_for_cells(&nudged),
            "95 m 应与 90 m 同桶，阶梯不应变化"
        );

        // 跨桶（150 m → 上界抬到 200 m）才允许变——这是已知残余，ADR-0056 §4 记档。
        let crossed: Vec<TerrainCellData> =
            (0..=90).map(|h| cell(h, 1)).chain([cell(150, 1)]).collect();
        assert_ne!(cuts_for_cells(&base), cuts_for_cells(&crossed));
    }

    /// 深海不参与高程区间：把水格高度拉到 -3500 不影响阶梯。
    #[test]
    fn cuts_ignore_water_cells() {
        let mut cells: Vec<TerrainCellData> = (0..=100).map(|h| cell(h, 1)).collect();
        let dry = cuts_for_cells(&cells);
        for _ in 0..500 {
            cells.push(cell(-3500, wv_terrain::SURFACE_WATER));
        }
        assert_eq!(dry, cuts_for_cells(&cells), "水格必须被排除在高程区间外");
    }

    /// 全水 / 空图不得 panic，回落到绝对阶梯。
    #[test]
    fn cuts_fall_back_to_domain_when_no_land() {
        assert_eq!(cuts_for_cells(&[]), DOMAIN_CUTS);
        let water: Vec<TerrainCellData> = (0..10)
            .map(|_| cell(-100, wv_terrain::SURFACE_WATER))
            .collect();
        assert_eq!(cuts_for_cells(&water), DOMAIN_CUTS);
    }

    /// 完全平的图（同一高度）不得 panic，且 band 落在合法区间。
    #[test]
    fn flat_terrain_is_clamped_and_stable() {
        let flat: Vec<TerrainCellData> = (0..64).map(|_| cell(50, 1)).collect();
        let cuts = cuts_for_cells(&flat);
        let band = band_of(50, &cuts);
        assert!(band < BANDS, "band {band} 越界");
        // 常量输入两次调用同值（无随机、无排序依赖）
        assert_eq!(cuts, cuts_for_cells(&flat));
    }

    /// 边界语义：正好等于阈值即进下一阶；band 单调不减且落在 0..BANDS。
    #[test]
    fn band_of_is_monotonic_and_inclusive_at_cuts() {
        let cuts: Cuts = [10, 20, 30, 40, 50, 60, 70];
        assert_eq!(band_of(9, &cuts), 0);
        assert_eq!(band_of(10, &cuts), 1, "等于阈值应进下一阶");
        assert_eq!(band_of(70, &cuts), 7);
        assert_eq!(band_of(9999, &cuts), 7, "上限钳在 BANDS-1");
        let mut last = 0;
        for h in -50..100 {
            let b = band_of(h, &cuts);
            assert!(b >= last, "band 必须单调不减：h={h} → {b} < {last}");
            assert!(b < BANDS);
            last = b;
        }
    }

    /// 绝对阶梯下的读数与文档表一致（1000 m = 山地阶 3，3000 m = 雪线阶 5）。
    #[test]
    fn absolute_ladder_maps_heights_to_documented_bands() {
        assert_eq!(band_of(0, &DOMAIN_CUTS), 0, "海平面属低地");
        assert_eq!(band_of(199, &DOMAIN_CUTS), 0);
        assert_eq!(band_of(1000, &DOMAIN_CUTS), 3, "1000 m 起是山地");
        assert_eq!(band_of(3000, &DOMAIN_CUTS), 5, "3000 m 在雪线以上");
        assert_eq!(band_of(6000, &DOMAIN_CUTS), 7, "6000 m 是最高阶");
    }

    /// 强度 0 = 原色不动；强度 1 = 完全等于色带色。
    #[test]
    fn tint_strength_interpolates_from_base_to_ramp() {
        let base = [0.2, 0.4, 0.6, 1.0];
        assert_eq!(tint_rgba(base, 3, 0.0), base);
        let full = tint_rgba(base, 3, 1.0);
        let want = ramp_rgba(3);
        for i in 0..3 {
            assert!(
                (full[i] - want[i]).abs() < 1e-6,
                "channel {i}: got {} want {}",
                full[i],
                want[i]
            );
        }
        let mid = tint_rgba(base, 3, 0.5);
        for i in 0..3 {
            let expect = base[i] + (want[i] - base[i]) * 0.5;
            assert!((mid[i] - expect).abs() < 1e-6);
        }
    }

    /// 风格色带的低地（第 0 阶）**原色不动**：色带首色与底图同色，
    /// 若按 `strength` 直接混入，公园绿会被洗成 `#E6F2E6` 的灰绿
    /// （2026-10-26 实机发现的色彩缺陷）。
    #[test]
    fn elevation_band_leaves_lowland_biome_color_untouched() {
        let band = ElevationBand {
            ramp: [
                [0xF8, 0xF9, 0xFA],
                [0xF2, 0xEE, 0xE5],
                [0xE9, 0xE1, 0xD2],
                [0xDD, 0xD1, 0xBA],
                [0xCD, 0xBB, 0x9F],
                [0xB9, 0xA3, 0x84],
                [0xA1, 0x89, 0x6A],
                [0x87, 0x70, 0x53],
            ],
            cuts: DOMAIN_CUTS,
            strength: 0.62,
        };
        let park = [
            0xC9 as f32 / 255.0,
            0xE7 as f32 / 255.0,
            0xC7 as f32 / 255.0,
            1.0,
        ];
        assert_eq!(
            band.tint(park, 50),
            park,
            "50 m 低地属第 0 阶，色带不得改动群系本色"
        );
    }

    /// 混入量随阶单调增大：越高越接近色带色，且第 0 阶严格不混。
    #[test]
    fn elevation_band_tint_grows_with_band() {
        let ramp = [
            [0xF8, 0xF9, 0xFA],
            [0xF2, 0xEE, 0xE5],
            [0xE9, 0xE1, 0xD2],
            [0xDD, 0xD1, 0xBA],
            [0xCD, 0xBB, 0x9F],
            [0xB9, 0xA3, 0x84],
            [0xA1, 0x89, 0x6A],
            [0x87, 0x70, 0x53],
        ];
        let band = ElevationBand {
            ramp,
            cuts: DOMAIN_CUTS,
            strength: 0.62,
        };
        let base = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(band.tint(base, 50), base, "第 0 阶必须完全不混");
        let mut last = 0.0;
        for (i, h) in [400, 800, 1200, 2000, 3000, 5000, 7000]
            .into_iter()
            .enumerate()
        {
            let out = band.tint(base, h);
            let drift = (1.0 - out[0]) + (1.0 - out[1]) + (1.0 - out[2]);
            assert!(
                drift > last,
                "第 {} 阶（{h} m）混入量应大于上一阶：{drift:.4} <= {last:.4}",
                i + 1
            );
            last = drift;
        }
    }

    /// 色带从低到高明度必须单调上升（否则「越高越黑」会被读成阴影）。
    #[test]
    fn ramp_luminance_increases_with_altitude() {
        let lum = |c: [u8; 3]| 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
        for i in 1..BANDS {
            let prev = lum(RAMP_SRGB[i - 1]);
            let cur = lum(RAMP_SRGB[i]);
            assert!(cur > prev, "第 {i} 阶 {cur} 不亮于第 {} 阶 {prev}", i - 1);
        }
    }

    /// 归一化：米 → `R16Unorm` 域，与 pack 的 `H_MIN/H_MAX` 口径一致且钳位。
    #[test]
    fn normalized_cuts_match_pack_encoding_domain() {
        let cuts: Cuts = [-4000, -1000, 0, 1000, 2000, 4000, 6000];
        let norm = normalized_cuts(&cuts, -4000.0, 10_000.0);
        let want = [0.0, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0];
        for (i, (got, expect)) in norm.iter().zip(want.iter()).enumerate() {
            assert!(
                (got - expect).abs() < 1e-6,
                "cut {i}: got {got} want {expect}"
            );
        }
        // 越界钳位（pack 侧同样是钳位，不是 panic）
        let out = normalized_cuts(&[-9999, 1, 2, 3, 4, 5, 9999], -4000.0, 10_000.0);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[CUTS - 1], 1.0);
    }

    /// 段锚点处连续插值必须退化为离散色带：恰在 `cuts` 各条边界上的高度
    /// 段内分数为 0，结果与 `tint_rgba(base, b+1, …)` 逐通道相等；
    /// 高度 0 是段 0 的锚点，与 `tint_rgba(base, 0, …)` 相等。
    #[test]
    fn continuous_tint_matches_discrete_at_band_anchors() {
        let base = [0.2, 0.4, 0.6, 1.0];
        let strength = 0.7;

        // 高度 0：低地段锚点（t = 0/200 = 0）。
        let got0 = tint_continuous(base, 0, &DOMAIN_CUTS, strength);
        let want0 = tint_rgba(base, 0, strength);
        for c in 0..3 {
            assert!(
                (got0[c] - want0[c]).abs() < 1e-6,
                "height 0 ch {c}: got {} want {}",
                got0[c],
                want0[c]
            );
        }

        // 7 条内边界：恰在 cut 上 → 升段且 t = 0。
        for (i, cut) in DOMAIN_CUTS.iter().enumerate() {
            let (b, t) = segment_of(*cut, &DOMAIN_CUTS);
            assert_eq!(b, i + 1, "cut {i}（{cut} m）应落在第 {} 段起点", i + 1);
            assert!(t.abs() < 1e-6, "cut {i} 段内分数应为 0：got {t}");
            let got = tint_continuous(base, *cut, &DOMAIN_CUTS, strength);
            let want = tint_rgba(base, i + 1, strength);
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-6,
                    "cut {i}（{cut} m）ch {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    }

    /// 段内插值：350 m 落在缓丘段（200–500），手算 t = (350−200)/(500−200) = 0.5，
    /// strength = 1 时结果 = `ramp[1]` 与 `ramp[2]` 的中点色
    /// （[120.5, 153.5, 101.5]/255）。
    #[test]
    fn continuous_tint_interpolates_within_band() {
        let base = [1.0, 0.0, 0.5, 1.0];
        let got = tint_continuous(base, 350, &DOMAIN_CUTS, 1.0);

        let (b, t) = segment_of(350, &DOMAIN_CUTS);
        assert_eq!(b, 1, "350 m 应在缓丘段");
        assert!((t - 0.5).abs() < 1e-6, "got {t} want 0.5");

        let lo = ramp_rgba(1);
        let hi = ramp_rgba(2);
        for c in 0..3 {
            let want = lo[c] + (hi[c] - lo[c]) * 0.5;
            assert!(
                (got[c] - want).abs() < 1e-6,
                "ch {c}: got {} want {}",
                got[c],
                want
            );
        }
    }

    /// 低地绿不被洗白：公园绿 + 50 m（段 0，t = 0.25，向 ramp[1] 过渡
    /// 仍是绿系）+ strength = 1 → 结果 g 通道仍为最大分量。
    #[test]
    fn continuous_tint_keeps_lowland_green_not_washed() {
        let park = [
            0xC9 as f32 / 255.0,
            0xE7 as f32 / 255.0,
            0xC7 as f32 / 255.0,
            1.0,
        ];
        let got = tint_continuous(park, 50, &DOMAIN_CUTS, 1.0);
        assert!(
            got[1] > got[0] && got[1] > got[2],
            "50 m 低地绿：g 通道必须仍是最大分量，got {:?}",
            got
        );
    }

    /// 强度 0 = 关闭：任何高度都原色不动（沿用 `tint_rgba` 的口径）。
    #[test]
    fn continuous_tint_strength_zero_returns_base() {
        let base = [0.2, 0.4, 0.6, 1.0];
        assert_eq!(
            tint_continuous(base, 3500, &DOMAIN_CUTS, 0.0),
            base,
            "strength 0 必须原样返回 base"
        );
    }

    /// 段内分数在段内严格递增：低地段（0–200 m）取 0/50/100/150/199，
    /// t = h/200 = 0 / 0.25 / 0.5 / 0.75 / 0.995，且全部落在 [0, 1)。
    #[test]
    fn segment_fraction_is_monotonic_within_band() {
        let mut last = -1.0_f32;
        for h in [0, 50, 100, 150, 199] {
            let (b, t) = segment_of(h, &DOMAIN_CUTS);
            assert_eq!(b, 0, "{h} m 应仍在低地段");
            assert!(
                t > last,
                "{h} m 的段内分数应严格递增：got {t} 上一值 {last}"
            );
            assert!(
                (0.0..1.0).contains(&t),
                "段内分数必须落在 [0, 1)：{h} m → {t}"
            );
            last = t;
        }
    }
}
