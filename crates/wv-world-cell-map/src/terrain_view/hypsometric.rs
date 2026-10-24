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
}
