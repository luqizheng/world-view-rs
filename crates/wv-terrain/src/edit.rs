//! 笔刷编辑：圆形软边涂抹、海拔升降、高斯衰减（基线 §1.13 / §1.14 / §2.4）。
//!
//! ## 本层的职责边界（基线 §2.2）
//!
//! - **不感知 Undo/Redo**：撤销由 `wv-edit` 记录 before/after 影响块完成；
//!   本层是**纯函数 + 可变网格**，被调用时就地改网格。
//! - **不感知笔刷光标**：光标是屏幕空间的视觉叠加层（特效规格 §3.1），不进数据。
//!
//! ## falloff 必须只有一份实现
//!
//! 特效规格 §3.1 硬契约：**笔刷光标的径向渐变曲线必须与本层的实际衰减函数一致**，
//! 否则「可视与实际不符 = 工具失信」。因此衰减函数 [`falloff_weight`] 是本 crate
//! 的公开 API，`wv-app` 画光标时**调用同一个函数**，禁止另写一份。

use crate::{Biome, Height, Surface, TerrainCell, TerrainGrid, SURFACE_WATER};

/// **湿判定（唯一入口）**：surface 通道为水，**或**海拔低于海平面。
///
/// 两层口径（设计稿 D11）：surface 是 authoring 真值——湖在山上（海拔 +10 m）
/// 仍是水、被塑笔刷压到海下的陆格仍是陆；但渲染海岸线与笔刷高度域对齐
/// 需要「这格现在是不是湿的」，海拔低于海平面一律按湿处理（与旧
/// 「kind 水系 ‖ 高度<0」混合判定逐点等价）。
///
/// **为什么放在 wv-terrain**：水陆二分是地形数据的领域规则，`wv-render` 的
/// 轮廓图渲染（`cells.rs`）与笔刷的高度域对齐（[`apply_brush`]）都要用它；
/// 放在渲染层会让数据层反向引用 UI，放在两处会造成口径漂移。
pub fn is_water_cell(c: TerrainCell) -> bool {
    c.surface == SURFACE_WATER || c.height < 0
}

/// 涂出海时的保底深度：把陆格改成水面时，高度一并压到海平面以下。
///
/// 不用极深值：与周边海格的高度差太大，VectorFlat 的双线性填充会在笔刷
/// 边缘拉出陡峭的等值线带。
pub const PAINT_SEA_DEPTH: Height = -40;
/// 涂上陆地时的保底高度：把水格改成陆面时，高度一并抬到海平面以上。
pub const PAINT_LAND_HEIGHT: Height = 40;

/// 一次涂抹要写入的地表目标：surface + biome 二元组（设计稿 D12/D19）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaintTarget {
    /// 写入的地表二值（水/陆）。
    pub surface: Surface,
    /// 写入的生物群系编号（水目标常配 [`crate::BIOME_NONE`]，语义湖/河由标记段承载）。
    pub biome: Biome,
}

/// 笔刷形状与参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brush {
    /// 笔刷中心（图内局部坐标，世界单位）。
    pub center: (f32, f32),
    /// 影响半径 R（世界单位）。半径外权重为 0。
    pub radius: f32,
    /// 衰减软度。`0` = 硬边（半径内权重恒为 1）；`1` = 完全软边（高斯）。
    pub softness: f32,
    /// 本次涂抹要写入的地表目标。`None` 表示只改高度、不动地表（如海拔笔刷）。
    pub target: Option<PaintTarget>,
    /// 本次要施加的高度增量（正=抬升，负=降低）。`0` 表示不动高度。
    pub height_delta: f32,
    /// 整格直写（地形橡皮刷用，ADR-0043）。
    ///
    /// `Some(cell)` 时**三通道整格覆盖**（height / surface / biome），完全
    /// 绕过 `target` 那条「跨域必须把高度推过海平面」的自动规则。
    ///
    /// **为什么需要它**：[`target`] 只有 surface + biome，高度由
    /// [`apply_brush`] 按 `cell.height >= 0` 就地替换成 [`PAINT_SEA_DEPTH`]（-40）
    /// 或 [`PAINT_LAND_HEIGHT`]（+40）。那是为「涂抹」设计的软化高度——作者涂
    /// 一片陆时不该凭空多出一道 40 m 的坎。但橡皮要的是**底图态**：`MapBase::Ocean`
    /// 是 -3500 m 整片深水、`MapBase::Land` 是 +80 m 整片草原（`wv_core` 的
    /// `MapBase::cell()`，整图同值正是为了让 VectorFlat 双线性采样保持常数）。
    /// 靠 `target` 永远够不着这两个值——所以整格直写另开一条路。
    ///
    /// 优先级：`exact` 高于 `target`，两者同时给时只认 `exact`。
    pub exact: Option<TerrainCell>,
}

impl Brush {
    /// 一个只改地表的圆形软边笔刷。
    pub fn paint(center: (f32, f32), radius: f32, target: PaintTarget) -> Self {
        Self {
            center,
            radius,
            softness: 0.5,
            target: Some(target),
            height_delta: 0.0,
            exact: None,
        }
    }

    /// 一个只改高度的圈选笔刷（基线 §1.14 的羽化抬升）。
    pub fn raise_lower(center: (f32, f32), radius: f32, amount: f32) -> Self {
        Self {
            center,
            radius,
            softness: 0.7,
            target: None,
            height_delta: amount,
            exact: None,
        }
    }

    /// 整格直写笔刷（地形橡皮刷）：把半径内权重 ≥50% 的格子**原样覆盖**为
    /// `cell`，不走「地表目标 + 跨域推高度」的自动规则。
    ///
    /// `softness` 默认给 `0.7`（与 [`Self::raise_lower`] 一致）——橡皮要的是
    /// 一圈边界清楚的洞，不要拖尾。边缘仍是硬边：整格覆盖在 `w >= 0.5` 处
    /// 一步到位，没有半格渐变。
    pub fn exact_fill(center: (f32, f32), radius: f32, softness: f32, cell: TerrainCell) -> Self {
        Self {
            center,
            radius,
            softness,
            target: None,
            height_delta: 0.0,
            exact: Some(cell),
        }
    }
}

/// 一次笔触：一串连续的笔刷采样点。
///
/// 拖拽时鼠标未必每帧都落在整数格上，因此一个「笔触」是多个 [Brush] 的序列。
/// 全部采样点都作用在同一个网格上，**后一个覆盖前一个的重叠区**。
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub brushes: Vec<Brush>,
}

impl Stroke {
    pub fn new() -> Self {
        Self {
            brushes: Vec::new(),
        }
    }

    pub fn push(&mut self, brush: Brush) {
        self.brushes.push(brush);
    }

    pub fn is_empty(&self) -> bool {
        self.brushes.is_empty()
    }
}

impl Default for Stroke {
    fn default() -> Self {
        Self::new()
    }
}

/// 衰减权重：`[0, 1]`，距中心越远越小，半径外为 0。
///
/// **这是全项目唯一的 falloff 实现**（特效规格 §3.1 硬契约）。
///
/// - `softness == 0` → 硬边：半径内 1、半径外 0；
/// - `softness > 0` → 平滑过渡：`u = d / R`，`w = clamp((1 - u) / softness, 0, 1)`
///   再套一次平滑曲线（ease）使边缘更自然。
pub fn falloff_weight(distance: f32, radius: f32, softness: f32) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    let u = distance / radius;
    if u >= 1.0 {
        return 0.0;
    }
    let s = softness.clamp(0.0, 1.0);
    if s <= f32::EPSILON {
        return 1.0;
    }
    // 内圈（50% 强度半径 R * (1 - softness) 以内）恒为 1，外圈线性衰减到 0。
    let inner = 1.0 - s;
    if u <= inner {
        return 1.0;
    }
    let t = (1.0 - u) / s; // 1 → 0
                           // 平滑（smoothstep）让边缘更像自然羽化。
    t * t * (3.0 - 2.0 * t)
}

/// 在网格上施加一次笔刷（就地修改）。返回受影响的格数。
///
/// 本函数是 `wv-edit::PaintTerrainCommand::apply` 的落点（基线 §2.2 调用链）。
pub fn paint(grid: &mut TerrainGrid, brush: &Brush, cell_size: f32) -> usize {
    apply_brush(grid, brush, cell_size)
}

/// 在网格上施加一整个笔触。返回受影响的格数。
pub fn apply_stroke(grid: &mut TerrainGrid, stroke: &Stroke, cell_size: f32) -> usize {
    let mut touched = 0;
    for brush in &stroke.brushes {
        touched += apply_brush(grid, brush, cell_size);
    }
    touched
}

/// 单次笔刷的核心实现。
fn apply_brush(grid: &mut TerrainGrid, brush: &Brush, cell_size: f32) -> usize {
    if cell_size <= 0.0 || brush.radius <= 0.0 {
        return 0;
    }
    // 笔刷覆盖的格范围（世界半径换算成格数，向上取整 + 1 格余量）。
    let cell_radius = (brush.radius / cell_size).ceil() as i32 + 1;
    let center_x = brush.center.0 / cell_size;
    let center_y = brush.center.1 / cell_size;
    let gx0 = center_x.floor() as i32 - cell_radius;
    let gy0 = center_y.floor() as i32 - cell_radius;

    let mut touched = 0;
    for gy in gy0..=center_y.floor() as i32 + cell_radius {
        for gx in gx0..=center_x.floor() as i32 + cell_radius {
            // 格心世界坐标。
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let dx = wx - brush.center.0;
            let dy = wy - brush.center.1;
            let d = (dx * dx + dy * dy).sqrt();
            let w = falloff_weight(d, brush.radius, brush.softness);
            if w <= 0.0 {
                continue;
            }
            let Some(mut cell) = grid.get(gx, gy) else {
                continue; // 越界格跳过（笔刷扫过边缘时的合法情形）
            };
            // 整格直写（橡皮）：优先于 `target`，一步覆盖三通道。
            if let Some(exact) = brush.exact {
                if w >= 0.5 {
                    cell = exact;
                }
            } else if let Some(target) = brush.target {
                // 地表目标：超过 50% 权重才写入——避免软边把边缘涂成半吊子色块。
                // 高度域随目标对齐（2026-10-14 用户反馈「笔刷涂大地图没变化」）：
                // VectorFlat 填充的湿判定含「海拔 < 0」（[`is_water_cell`]），只写
                // surface 不写高度时，「把海涂成陆地 / 把陆涂成海」在等值线视图下
                // 完全不可见，笔刷等于失效。此处把跨域格的高度一并推过海平面，
                // 同域格不动（不抹平作者已有的起伏）。
                if w >= 0.5 {
                    cell.surface = target.surface;
                    cell.biome = target.biome;
                    // 场景分区（biome 落在 21..=32 保留带）**只改分区码，
                    // 不动高度**——分区是同一张基底图上的涂色，作者标
                    // 「这块是贫民区」不该凭空多出一座 40 m 的山包。
                    //
                    // 高度域规则是为「涂抹自然地表」设计的：海涂成陆要推过
                    // 海平面，否则等值线视图下完全看不见笔刷生效。分区不
                    // 涉及水陆切换（永远写陆面），故整段跳过。
                    if !crate::biome::is_district(target.biome) {
                        if target.surface == SURFACE_WATER {
                            if cell.height >= 0 {
                                cell.height = PAINT_SEA_DEPTH;
                            }
                        } else if cell.height < 0 {
                            cell.height = PAINT_LAND_HEIGHT;
                        }
                    }
                }
            }
            // 高度：按权重线性叠加（羽化抬升，基线 §1.14 / §2.4 高斯衰减的近亲）。
            if brush.height_delta != 0.0 {
                let delta = (brush.height_delta * w).round() as i32;
                if delta != 0 {
                    let new_h =
                        (cell.height as i32 + delta).clamp(i16::MIN as i32, i16::MAX as i32);
                    cell.height = new_h as Height;
                }
            }
            grid.set(gx, gy, cell);
            touched += 1;
        }
    }
    touched
}

/// 选区升降（基线 §1.14 / §2.4）：高斯衰减 `h' = h + amount * exp(-d²/2σ²)`。
///
/// `amount` 为最大抬升量、`sigma` 为衰减带宽（世界单位）。
/// 默认「羽化抬升」即本函数；`sigma → 0` 退化为整体平抬。
pub fn raise_region(
    grid: &mut TerrainGrid,
    center: (f32, f32),
    radius: f32,
    amount: f32,
    sigma: f32,
    cell_size: f32,
) -> usize {
    if cell_size <= 0.0 || radius <= 0.0 || amount == 0.0 {
        return 0;
    }
    let sigma = sigma.max(1e-3);
    let cell_radius = (radius / cell_size).ceil() as i32 + 1;
    let cx = center.0 / cell_size;
    let cy = center.1 / cell_size;
    let mut touched = 0;
    for gy in (cy.floor() as i32 - cell_radius)..=(cy.floor() as i32 + cell_radius) {
        for gx in (cx.floor() as i32 - cell_radius)..=(cx.floor() as i32 + cell_radius) {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let dx = wx - center.0;
            let dy = wy - center.1;
            let d2 = dx * dx + dy * dy;
            let d = d2.sqrt();
            if d > radius {
                continue;
            }
            let w = (-d2 / (2.0 * sigma * sigma)).exp();
            if w < 0.5 {
                continue; // 权重不足半格高度，不足以改变一格
            }
            let Some(mut cell) = grid.get(gx, gy) else {
                continue;
            };
            let delta = (amount * w).round() as i32;
            if delta != 0 {
                let new_h = (cell.height as i32 + delta).clamp(i16::MIN as i32, i16::MAX as i32);
                cell.height = new_h as Height;
                grid.set(gx, gy, cell);
                touched += 1;
            }
        }
    }
    touched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BIOME_DESERT, BIOME_FOREST, BIOME_GRASSLAND, BIOME_NONE, BIOME_OCEAN, SURFACE_LAND,
        SURFACE_WATER,
    };

    #[test]
    fn falloff_is_one_at_center_and_zero_outside() {
        assert_eq!(falloff_weight(0.0, 10.0, 0.5), 1.0);
        assert_eq!(falloff_weight(10.0, 10.0, 0.5), 0.0);
        assert_eq!(falloff_weight(999.0, 10.0, 0.5), 0.0);
    }

    #[test]
    fn falloff_decreases_monotonically_outward() {
        let radius = 20.0;
        let mut prev = f32::INFINITY;
        for i in 0..=20 {
            let d = i as f32;
            let w = falloff_weight(d, radius, 0.6);
            assert!(
                w <= prev + 1e-6,
                "权重应随距离单调不增，d={d} w={w} prev={prev}"
            );
            prev = w;
        }
    }

    #[test]
    fn hard_brush_edge_is_inner_plateau() {
        // softness=0 → 半径内恒为 1。
        assert_eq!(falloff_weight(5.0, 10.0, 0.0), 1.0);
        assert_eq!(falloff_weight(9.9, 10.0, 0.0), 1.0);
        assert_eq!(falloff_weight(10.0, 10.0, 0.0), 0.0);
    }

    #[test]
    fn paint_writes_target_at_center() {
        let mut g = TerrainGrid::new(20, 20);
        // cell_size 1.0，中心 (10.5,10.5) → 格 (10,10)
        let b = Brush::paint(
            (10.5, 10.5),
            5.0,
            PaintTarget {
                surface: SURFACE_LAND,
                biome: BIOME_GRASSLAND,
            },
        );
        let touched = paint(&mut g, &b, 1.0);
        assert!(touched > 0, "应有格被涂抹");
        let center = g.get(10, 10).unwrap();
        assert_eq!(center.biome, BIOME_GRASSLAND, "中心格群系应被写入");
        assert_eq!(center.surface, SURFACE_LAND, "中心格地表应被写入");
        let far = g.get(0, 0).unwrap();
        assert_eq!(far.surface, SURFACE_WATER, "远角不应被涂（保持默认水面）");
        assert_eq!(far.biome, BIOME_NONE, "远角不应被涂（保持无群系）");
    }

    /// 2026-10-14 用户反馈「笔刷涂大地图没变化」的核心修复：在 VectorFlat
    /// 填充口径下（湿判定含「海拔 < 0」），把海格涂成陆地时高度必须
    /// 一并抬过海平面，否则画面纹丝不动。
    #[test]
    fn painting_land_target_over_sea_raises_height_above_sea_level() {
        let mut g = TerrainGrid::new(20, 20);
        // 造一片海：高度压到 -200。
        for gy in 0..20 {
            for gx in 0..20 {
                let mut c = g.get(gx, gy).unwrap();
                c.height = -200;
                g.set(gx, gy, c);
            }
        }
        let b = Brush::paint(
            (10.5, 10.5),
            5.0,
            PaintTarget {
                surface: SURFACE_LAND,
                biome: BIOME_GRASSLAND,
            },
        );
        paint(&mut g, &b, 1.0);
        let cell = g.get(10, 10).unwrap();
        assert_eq!(cell.biome, BIOME_GRASSLAND, "中心格群系应写入");
        assert!(
            cell.height >= 0,
            "海格涂陆后高度应 ≥ 0（海平面），实得 {}",
            cell.height
        );
    }

    /// 场景分区笔刷**绝不改高度**——分区是同一张基底图上的涂色，
    /// 作者只想标「这块是贫民区」，不该凭空多出一座 40 m 的山包。
    #[test]
    fn painting_a_district_never_changes_height() {
        let mut g = TerrainGrid::new(20, 20);
        for gy in 0..20 {
            for gx in 0..20 {
                g.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: -3500,
                        surface: SURFACE_WATER,
                        biome: BIOME_OCEAN,
                    },
                );
            }
        }
        let b = Brush::paint(
            (10.5, 10.5),
            5.0,
            PaintTarget {
                surface: SURFACE_LAND,
                biome: crate::biome::DISTRICT_FIRST + 1,
            },
        );
        paint(&mut g, &b, 1.0);
        let cell = g.get(10, 10).unwrap();
        assert_eq!(
            cell.height, -3500,
            "刷分区不得改高度（实得 {}）",
            cell.height
        );
        assert_eq!(cell.biome, crate::biome::DISTRICT_FIRST + 1, "分区码应写入");
        assert_eq!(cell.surface, SURFACE_LAND, "分区笔刷写陆面");
    }

    /// 回归护栏：非分区笔刷的「跨域推高度」旧行为必须逐字不变。
    #[test]
    fn painting_a_normal_biome_still_pushes_height_across_sea_level() {
        let mut g = TerrainGrid::new(20, 20);
        for gy in 0..20 {
            for gx in 0..20 {
                g.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: -3500,
                        surface: SURFACE_WATER,
                        biome: BIOME_OCEAN,
                    },
                );
            }
        }
        let b = Brush::paint(
            (10.5, 10.5),
            5.0,
            PaintTarget {
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        paint(&mut g, &b, 1.0);
        assert!(
            g.get(10, 10).unwrap().height >= 0,
            "非分区笔刷仍应推过海平面"
        );
    }

    #[test]
    fn painting_water_target_over_land_lowers_height_below_sea_level() {
        let mut g = TerrainGrid::new(20, 20);
        // 造一片高原陆地：高度抬到 +300。
        for gy in 0..20 {
            for gx in 0..20 {
                g.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: 300,
                        surface: SURFACE_LAND,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        let b = Brush::paint(
            (10.5, 10.5),
            5.0,
            PaintTarget {
                surface: SURFACE_WATER,
                biome: BIOME_NONE,
            },
        );
        paint(&mut g, &b, 1.0);
        let cell = g.get(10, 10).unwrap();
        assert_eq!(cell.surface, SURFACE_WATER, "中心格地表应写入水面");
        assert!(
            cell.height < 0,
            "陆格涂海后高度应 < 0（海平面），实得 {}",
            cell.height
        );
    }

    /// 同域涂抹不动高度：陆上换群系（草原→森林）不得抹平作者已有的起伏。
    #[test]
    fn painting_target_within_the_same_domain_keeps_height() {
        let mut g = TerrainGrid::new(20, 20);
        for gy in 0..20 {
            for gx in 0..20 {
                g.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: 77,
                        surface: SURFACE_LAND,
                        biome: BIOME_GRASSLAND,
                    },
                );
            }
        }
        let b = Brush::paint(
            (10.5, 10.5),
            5.0,
            PaintTarget {
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        paint(&mut g, &b, 1.0);
        assert_eq!(g.get(10, 10).unwrap().height, 77, "陆→陆换群系不应改高度");
    }

    #[test]
    fn paint_outside_grid_is_safe() {
        let mut g = TerrainGrid::new(10, 10);
        let b = Brush::paint(
            (-50.0, -50.0),
            5.0,
            PaintTarget {
                surface: SURFACE_WATER,
                biome: BIOME_NONE,
            },
        );
        // 不 panic，且不产生改动。
        paint(&mut g, &b, 1.0);
        assert_eq!(g.get(0, 0).unwrap().surface, SURFACE_WATER);
    }

    #[test]
    fn raise_lower_adds_height_at_center() {
        let mut g = TerrainGrid::new(20, 20);
        let b = Brush::raise_lower((10.5, 10.5), 5.0, 10.0);
        paint(&mut g, &b, 1.0);
        let h_center = g.get(10, 10).unwrap().height;
        assert_eq!(h_center, 10, "中心应抬升满额");
        let h_edge = g.get(14, 10).unwrap().height;
        assert!(h_edge < h_center, "边缘抬升应小于中心, edge={h_edge}");
    }

    #[test]
    fn height_clamps_at_i16_bounds() {
        let mut g = TerrainGrid::new(10, 10);
        let b = Brush {
            center: (5.5, 5.5),
            radius: 3.0,
            softness: 0.0,
            target: None,
            height_delta: 100_000.0,
            exact: None,
        };
        paint(&mut g, &b, 1.0);
        assert_eq!(
            g.get(5, 5).unwrap().height,
            i16::MAX,
            "高度应钳制到 i16 上限"
        );
    }

    #[test]
    fn raise_region_uses_gaussian_falloff() {
        let mut g = TerrainGrid::new(40, 40);
        // sigma = 5, amount = 100, radius = 20
        raise_region(&mut g, (20.0, 20.0), 20.0, 100.0, 5.0, 1.0);
        let center = g.get(20, 20).unwrap().height;
        let mid = g.get(25, 20).unwrap().height; // d=5 → w=exp(-0.5)≈0.607
        assert!(center > mid && mid > 0, "中心={center} 中距={mid}");
        // 格心在 (20.5, 20.5) → d²=0.5 → 100*exp(-0.5/50)=99.0 → 99
        assert_eq!(center, 99, "中心格按高斯权重应为 99，实际 {center}");
        // 格心在 (25.5, 20.5) → d²=30.5 → 100*exp(-30.5/50)=100*0.5434=54.3 → 54
        assert!((53..=56).contains(&mid), "高斯权重手算约 54，实际 {mid}");
    }

    #[test]
    fn apply_stroke_accumulates_multiple_brushes() {
        let mut g = TerrainGrid::new(30, 30);
        let mut s = Stroke::new();
        s.push(Brush::paint(
            (5.5, 5.5),
            3.0,
            PaintTarget {
                surface: SURFACE_WATER,
                biome: BIOME_NONE,
            },
        ));
        s.push(Brush::paint(
            (25.5, 25.5),
            3.0,
            PaintTarget {
                surface: SURFACE_LAND,
                biome: BIOME_DESERT,
            },
        ));
        apply_stroke(&mut g, &s, 1.0);
        assert_eq!(g.get(5, 5).unwrap().surface, SURFACE_WATER);
        assert_eq!(g.get(25, 25).unwrap().biome, BIOME_DESERT);
    }

    /// 整格直写（`exact`）必须三通道一起覆盖，且**不受**「跨域推高度」规则
    /// 影响——橡皮要的就是底图那一个整值（-3500 / +80），不是 `PAINT_SEA_DEPTH`。
    ///
    /// 用一个「陆地 + 高山」的格子作被擦对象：若误走 `target` 分支，高度会被
    /// 压到 `PAINT_SEA_DEPTH`（-40）或压根不动，绝不是 -3500。
    #[test]
    fn exact_fill_overwrites_all_three_channels_verbatim() {
        let mut g = TerrainGrid::new(20, 20);
        // 先把整图铺成「高山陆地」，再拿橡皮擦中心那一块。
        for y in 0..20i32 {
            for x in 0..20i32 {
                g.set(
                    x,
                    y,
                    TerrainCell {
                        height: 2000,
                        surface: SURFACE_LAND,
                        biome: crate::BIOME_MOUNTAINS,
                    },
                );
            }
        }
        let base_ocean = TerrainCell {
            height: -3500,
            surface: SURFACE_WATER,
            biome: crate::BIOME_NONE,
        };
        let b = Brush::exact_fill((10.0, 10.0), 4.0, 0.7, base_ocean);
        let touched = paint(&mut g, &b, 1.0);

        assert!(touched > 0, "橡皮应触及若干格，实得 {touched}");
        assert_eq!(
            g.get(9, 9).unwrap(),
            base_ocean,
            "中心格应被整格覆盖为底图海洋"
        );
        assert_eq!(g.get(0, 0).unwrap().height, 2000, "半径外的高山不应被动到");
    }

    /// `exact` 优先级高于 `target`：两者同时给时只认 `exact`。这条优先级是
    /// 显式约定，不靠「谁写在 `if` 前面」的巧合。
    #[test]
    fn exact_takes_precedence_over_target() {
        let mut g = TerrainGrid::new(20, 20);
        let b = Brush {
            center: (10.0, 10.0),
            radius: 4.0,
            softness: 0.0,
            // 若 `exact` 缺席，这条 target 会把格子推成 +40 m 陆地。
            target: Some(PaintTarget {
                surface: SURFACE_LAND,
                biome: crate::BIOME_DESERT,
            }),
            height_delta: 0.0,
            exact: Some(TerrainCell {
                height: 80,
                surface: SURFACE_LAND,
                biome: crate::BIOME_GRASSLAND,
            }),
        };
        paint(&mut g, &b, 1.0);
        assert_eq!(
            g.get(10, 10).unwrap().biome,
            crate::BIOME_GRASSLAND,
            "`exact` 应胜出，不该被 target 改成沙漠"
        );
        assert_eq!(
            g.get(10, 10).unwrap().height,
            80,
            "`exact` 给了 80 就该是 80，不该被跨域规则推到 +40"
        );
    }

    /// 塑笔刷把陆格压到海平面以下：surface 通道保持 LAND（authoring 真值
    /// 不因海拔翻案），但湿判定按「海拔 < 0」读出湿——设计稿 D11 两层口径。
    #[test]
    fn lowered_land_below_sea_level_stays_land_channel_but_reads_wet() {
        let mut grid = TerrainGrid::new(64, 64);
        for gy in 0..64 {
            for gx in 0..64 {
                grid.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: 100,
                        surface: SURFACE_LAND,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        paint(
            &mut grid,
            &Brush::raise_lower((32.0, 32.0), 8.0, -500.0),
            1.0,
        );
        let c = grid.get(32, 32).unwrap();
        assert_eq!(c.surface, SURFACE_LAND, "got surface {}", c.surface);
        assert!(is_water_cell(c), "height {} should read wet", c.height);
    }
}
