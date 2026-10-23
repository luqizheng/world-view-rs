//! 独立验收测试 —— 网格可见性 / 双档网格（QA 严过关，2026-10-23）。
//!
//! ## 口径（不许违反）
//! 本文件**不引用被测常量**算期望值：`GRID_FADE_START_PX` / `GRID_FADE_END_PX` /
//! `GRID_LINE_PX` / `GridConfig::default()` 一律不参与期望值构造。
//! 期望值全部来自手算，算式写在每个断言旁边，可逐条复算。
//!
//! - `cell_size = 4.0`：手算自 `examples/lod-demo/maps/map_world/index.yaml`
//!   （`extent.width 4096.0 / raster.width 1024`），**不是**读 `GridConfig::default()`
//!   ——后者是 8.0，用它会把每个期望值算错 2 倍。
//! - `scale` 口径 = 世界单位/屏幕像素（大 = 远）。
//! - `fit = max(4096/1280, 4096/800) = 5.12`（`ViewState::default().window_size`
//!   = 1280×800）。
//!
//! ## 与实现的关系
//! 只消费 `wv_world_cell_map::terrain_view::grid_lines` 的现有公开纯函数
//! （`grid_line_fade` / `grid_chains`）。需要主程序尚未提供的接口的断言
//! （粗档 stride、桶化重建键）**不在此伪造**——见验收单「等接口」清单。

use wv_world_cell_map::terrain_view::grid_lines::{
    grid_cell_px, grid_chains, grid_coarse_px, grid_coarse_stride, grid_line_alpha,
    grid_line_alpha_with_opacity, grid_line_fade,
};

/// 手算常量：lod-demo 世界图 4096 世界单位 / 1024 栅格格 = 4.0 世界单位/格。
const CELL_SIZE: f32 = 4.0;

/// 世界级「全图适配」缩放：max(4096/1280, 4096/800) = 5.12。
const FIT_SCALE: f32 = 5.12;

/// 格子屏幕边长（px）——`cell_size / scale`，与实现的消费式同源但由本文件独立计算。
fn cell_px(scale: f32) -> f32 {
    CELL_SIZE / scale
}

/// A1（独立复算）：三个缩放点的手算期望。
///
/// | scale | cell_px = 4.0/scale | 期望 fade | 期望结论 |
/// |---|---|---|---|
/// | 5.12 | 0.78125 | 0.0 | 不可见（整层退场） |
/// | 2.0  | 2.0     | 0.0 | 不可见（恰在退场终点） |
/// | 1.0  | 4.0     | 1.0 | 全显 |
#[test]
fn a1_hand_computed_cell_px_and_fade_at_three_scales() {
    // scale = 5.12：4.0 / 5.12 = 0.78125（精确值，二进制可表示）
    let px_fit = cell_px(FIT_SCALE);
    assert!(
        (px_fit - 0.78125).abs() < 1e-9,
        "手算 cell_px = 4.0/5.12 = 0.78125，实得 {px_fit}"
    );
    assert_eq!(
        grid_line_fade(px_fit),
        0.0,
        "0.78125px 低于「<2px 整层退场」的要求 → 期望 fade = 0"
    );

    // scale = 2.0：4.0 / 2.0 = 2.0
    let px_mid = cell_px(2.0);
    assert!((px_mid - 2.0).abs() < 1e-9, "手算 2.0，实得 {px_mid}");
    assert_eq!(grid_line_fade(px_mid), 0.0, "恰在 2px 退场终点 → 期望 0");

    // scale = 1.0：4.0 / 1.0 = 4.0
    let px_in = cell_px(1.0);
    assert!((px_in - 4.0).abs() < 1e-9, "手算 4.0，实得 {px_in}");
    assert_eq!(grid_line_fade(px_in), 1.0, "≥4px 全显 → 期望 1");
}

/// A2（边界三点）：淡出两端与中点的独立复算。
///
/// 要求「4px 全显 / 2px 退场 / 其间线性」→ 3.0px 应为 0.5，2.01px 应略大于 0，
/// 0.5px 与 100px 应被钳在 0 与 1。
#[test]
fn a2_fade_boundaries_are_linear_between_two_and_four_px() {
    assert_eq!(grid_line_fade(3.0), 0.5, "中点手算 (3-2)/2 = 0.5");
    assert_eq!(grid_line_fade(4.0), 1.0, "起点 4px 全显");
    assert_eq!(grid_line_fade(2.0), 0.0, "终点 2px 退场");
    assert_eq!(grid_line_fade(100.0), 1.0, "超上限钳在 1");
    assert_eq!(grid_line_fade(0.5), 0.0, "超下限钳在 0");
    let just_above = grid_line_fade(2.01);
    assert!(
        (just_above - 0.005).abs() < 1e-4,
        "2.01px 手算 (2.01-2)/2 = 0.005，实得 {just_above}"
    );
}

/// A3（缺陷证据 · 现状）：世界级 fit 缩放落在不可见区，且可见上界是 scale = 2.0。
///
/// 独立复算：可见 ⇔ cell_px > 2 ⇔ 4.0/scale > 2 ⇔ scale < 2.0。
/// 世界级缩放带 = [0.16, 8.0]（`fit/32` … `fit×1.5625`），故
/// **scale ∈ [2.0, 8.0] 全部无网格**；按档数（几何步进）计约占
/// log(8/2)/log(8/0.16) = 1.386/3.912 ≈ 35% 的档位。
#[test]
fn a3_world_fit_scale_has_no_grid_and_visible_band_starts_below_scale_two() {
    assert_eq!(
        grid_line_fade(cell_px(FIT_SCALE)),
        0.0,
        "世界级 fit（5.12）下 fade 必须为 0 —— 这是「开了网格看不到」的直接证据"
    );
    // 边界内侧（scale = 1.99 → cell_px ≈ 2.0101）必须 > 0
    let inner = grid_line_fade(cell_px(1.99));
    assert!(
        inner > 0.0,
        "scale 1.99 时 cell_px ≈ 2.01px，应仍可见，实得 {inner}"
    );
    // 边界外侧（scale = 2.0 → cell_px = 2.0）必须为 0
    assert_eq!(grid_line_fade(cell_px(2.0)), 0.0, "scale 2.0 已到退场终点");
    // 世界带外端（scale = 8.0 → cell_px = 0.5）
    assert_eq!(grid_line_fade(cell_px(8.0)), 0.0, "最远处彻底不可见");
}

/// A4（可达性证明 · 双档方案）：粗档倍率 m 能否把整条世界带顶到 ≥4px。
///
/// 手算表（cell_size = 4.0）：
///
/// | scale | 细档 cell_px | m=8 粗档 px | 粗档可见 |
/// |---|---|---|---|
/// | 0.16 | 25.0 | 200.0 | 是 |
/// | 1.0  | 4.0  | 32.0  | 是 |
/// | 2.0  | 2.0  | 16.0  | 是 |
/// | 5.12 | 0.78125 | 6.25 | 是 |
/// | 8.0  | 0.5  | 4.0   | 是（恰在 4px 起点） |
///
/// 结论：1024² 栅格的世界级只需 m = 8 即可覆盖整带。4096² 栅格（cell_size = 1.0）
/// 在 scale 8.0 时细档 0.125px，需 m = 32 才到 4px —— 所以粗档倍率上限必须
/// **≥ 32**，且必须是 2 的幂（见验收单误伤评估 ③：保证粗线落在偶数列）。
#[test]
fn a4_coarse_tier_multiplier_reaches_four_px_across_the_world_band() {
    for scale in [0.16_f32, 1.0, 2.0, FIT_SCALE, 8.0] {
        let coarse = cell_px(scale) * 8.0;
        assert!(
            coarse >= 4.0,
            "scale {scale}：m=8 粗档 {coarse}px 应 ≥ 4px（全显）"
        );
        assert_eq!(
            grid_line_fade(coarse),
            1.0,
            "scale {scale}：粗档 {coarse}px 期望全显（fade = 1）"
        );
    }
    // 4096² 栅格（cell_size = 1.0）最远端：细档 0.125px，m=32 → 4.0px
    let px_4096: f32 = 1.0 / 8.0;
    assert!((px_4096 - 0.125).abs() < 1e-9, "手算 1/8 = 0.125");
    assert_eq!(
        grid_line_fade(px_4096 * 32.0),
        1.0,
        "4096² 栅格 m=32 在 scale 8.0 时恰达 4px → 粗档倍率上限至少 32"
    );
}

/// A5（成本口径 · 现行 API 可测）：1024² 栅格的细档链数 vs m=8 粗档链数。
///
/// 细档：`(1024+1) 竖 + (1024+1) 横 = 2050` 条链 → 4100 个四边形。
/// 粗档（每 8 格一条）：`(128+1)×2 = 258` 条链 → 516 个四边形（≈1/8）。
///
/// 后者用**现有** `grid_chains` 在降采样幅面（128²）上直接算出，不是估算值；
/// 真正接线时主程序需要 `grid_chains_strided(width, height, stride, half_w)`，
/// 见验收单「等接口」清单（本条只用现有 API 证明数量级，不碰未实现接口）。
#[test]
fn a5_chain_cost_fine_tier_versus_coarse_tier() {
    let fine = grid_chains(1024, 1024, 0.5);
    assert_eq!(fine.len(), 2 * (1024 + 1), "细档链数 = 2050");
    let coarse = grid_chains(128, 128, 0.5);
    // 手算：stride = 8 时每位是 8 格 → (1024/8 + 1) 竖 + 同数横 = 2 × 129 = 258
    assert_eq!(
        coarse.len(),
        2 * (1024 / 8 + 1),
        "m=8 粗档链数 = 258，实得 {}",
        coarse.len()
    );
    assert_eq!(coarse.len(), 258, "手算 258 条链（不是估算值）");
}

// ══════════════ 2 轮补充断言（2026-10-23 双档修好后的正向复签） ══════════════

/// A6（AC11 可断言部分）：粗档倍率必须是 **2 的幂**，且屏上定位格够大。
///
/// 2 的幂是「粗线落在格缘（`k·stride` 格）」的充分条件——否则粗线与细线错拍，
/// 作者按粗线定位会偏半格。
///
/// 关于定位格下限 **4px**（2026-10-23 第 3 轮修正，原为 8px）：
/// - 4px 是 v2 复签给的可达参数 `m = min{2^n : 2^n × cell_px ≥ 4}`，
///   见 `docs/reviews/2026-10-23-nature-overlay-acceptance.md` 与
///   `docs/PERF_GRID_AND_PROFILE_v0.1.md`（`GRID_COARSE_MIN_PX` 8.0 → 4.0 的二次定标）；
/// - **用户已在 4.0 的实机上目视验收过网格**（「正确」「粗细两档还能接受」），
///   所以 4.0 是有效规格，原来那条 8px 是旧实例留下的陈旧口径。
/// - 语义断言**未放宽**：`m` 仍必须是 2 的幂、仍必须满足 `m × cell_px ≥ 4px`
///   （下面的期望值按 4px 重新手算，不是把阈值改小就算过）。
#[test]
fn a6_coarse_stride_is_power_of_two_and_locator_cells_stay_readable() {
    for scale in [0.16_f32, 0.5, 1.0, 2.0, FIT_SCALE, 8.0] {
        let px = grid_cell_px(CELL_SIZE, scale);
        let stride = grid_coarse_stride(px);
        assert!(
            stride.is_power_of_two(),
            "scale {scale}：粗档倍率 {stride} 不是 2 的幂 → 粗线不落格缘"
        );
        assert!(
            stride >= 1,
            "scale {scale}：倍率不能为 0（0 会让 shader 除零）"
        );
        let coarse = grid_coarse_px(px);
        assert!(
            coarse >= 4.0 - 1e-4,
            "scale {scale}：定位格 {coarse}px < 4px（cell_px {px} × stride {stride}）"
        );
    }
    // fit 档手算（4px 下限）：4.0/5.12 = 0.78125px；×4 = 3.125 < 4 → 取 8 → 6.25px
    assert_eq!(grid_coarse_stride(0.78125), 8, "fit 档应落在 8×");
    assert!(
        (grid_coarse_px(0.78125) - 6.25).abs() < 1e-4,
        "手算 0.78125 × 8 = 6.25px"
    );
    // 4096² 栅格（cell_size = 1.0）最远端：0.125px × 32 = 4.0px（最小满足 4px 的 2 的幂）
    assert_eq!(grid_coarse_stride(0.125), 32, "0.125px 需 32× 才到 4px");
    assert!((grid_coarse_px(0.125) - 4.0).abs() < 1e-4);
}

/// A7（缺陷已修的**正向**断言）：细档退出后整体 alpha 仍严格 > 0。
///
/// 旧语义「fade = 0 ⇒ 整层退场」已被拆掉：`grid_line_fade` 保留原公式
/// （4px→1 / 3px→0.5 / 2px→0），但 `grid_line_alpha` 恒 > 0。
/// 这条断言不看基色 alpha 的具体取值（0.48 是主程序本轮改的设计值），
/// 只钉「开关开 = 一定看得见」这个**需求**。
#[test]
fn a7_alpha_never_drops_to_zero_once_the_switch_is_on() {
    let mut prev = grid_line_alpha(0.001);
    assert!(prev > 0.0, "极密格距下 alpha 仍须 > 0");
    for px in [0.01_f32, 0.125, 0.5, 0.78125, 1.0, 2.0, 3.0, 4.0, 8.0, 32.0] {
        let a = grid_line_alpha(px);
        assert!(
            a > 0.0,
            "cell_px {px}：alpha 必须 > 0（细档退出 ≠ 整层退场）"
        );
        assert!(
            a >= prev - 1e-6,
            "cell_px {px}：alpha {a} 低于更密档 {prev}——淡出必须单调不增"
        );
        prev = a;
    }
    // 细档在 fit 档已退出（这是事实，不是缺陷），但 alpha 不为 0
    let px_fit = grid_cell_px(CELL_SIZE, FIT_SCALE);
    assert_eq!(grid_line_fade(px_fit), 0.0, "细档在 0.78px 退出");
    assert!(grid_line_alpha(px_fit) > 0.0, "粗档必须接手");
    // 图层 opacity = 0 → 透明（负向：opacity 不能被无视）
    assert_eq!(
        grid_line_alpha_with_opacity(px_fit, 0.0),
        0.0,
        "opacity 0 时必须完全透明"
    );
    assert!(grid_line_alpha_with_opacity(px_fit, 1.0) > 0.0);
}
