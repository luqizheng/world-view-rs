//! 海的画法配方（SeaRecipe）——决定「海长什么样」的数值参数（ADR-0048）。
//!
//! ## 为什么在这里
//!
//! 海的表现此前散落在 `wv-render/terrain_view/cells.rs` 的硬编码常量里
//! （深度量程 3500、浅滩带宽 6 格、峰值 0.85…）。这些是**美术配方**不是
//! 渲染机制：水彩主题要浅滩带 + 纸纹，水墨主题可能要皴线而不要色带。
//! 若留在渲染层，切换风格只能往渲染代码里塞 `if theme` 分支——违反
//! ADR-0043「加新风格 = 加新 crate、零改渲染层源码」。
//!
//! 与 [`crate::overlays::FeatureRule`] 同构：**数值配方住主题层，执行器在
//! 渲染层**。
//!
//! ## 分工
//!
//! - `wv-terrain::shore` —— 离岸距离场（域事实，与主题无关）
//! - 本模块 —— 怎么把这个事实画成海（颜色值、带宽、曲线、墨线、颗粒）
//! - `wv-render` —— 通用执行器（读配方逐格/逐像素上色）
//!
//! ## 纯数据
//!
//! 全部字段 `Copy`，无运行时状态、无文件访问（AGENTS §2.1 对 wv-theme 的
//! 约束）。渲染热路径只读，无分配。

/// 一圈离岸波纹（墨线）。
///
/// 语义照 civ-atlas `RIPPLE_D` / `RIPPLE_A`（`src/render/fantasy.ts:2722`）：
/// 在以「离岸距离」为纵轴的剖面上放一条高斯带，**墨线平行于海岸**——
/// 这是手绘海图「水彩感」的主要来源（不是色阶渐变）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RippleSpec {
    /// 该圈波纹到岸的距离（**格**）。
    pub dist_cells: f32,
    /// 高斯带宽（格）：越小线越锐。0.55 约等于「一条细墨线」。
    pub sigma_cells: f32,
    /// 墨量（0..=1）：往 [`SeaRecipe::ink`] 混合的最大比例。
    pub alpha: f32,
}

/// 近岸排线（hatching）规格——手绘海图在浅海里画的平行短线。
///
/// 照 civ-atlas `fantasy.ts` 的 `HATCH_D` / `HATCH_A`（`src/render/fantasy.ts:2726`）：
/// 离岸一定距离内、每隔若干行画一道横线，墨量很淡。
///
/// **这是水彩感的关键缺失件**：civ-atlas 的近岸三件套是「岸线墨线 + 近岸排线
/// + 离岸波纹墨线」，我们此前只有后两件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatchSpec {
    /// 排线的**径向长度**（格）：横线段从岸线向外延伸多远。
    ///
    /// 语义照 civ-atlas：线段的一端**停在海岸线上**、另一端向海外伸出——
    /// 故长度由岸线形状决定，不是固定长度也不是整行实心。
    pub dist_cells: f32,
    /// 行距（**纹理像素**）：每隔多少像素画一行。
    ///
    /// 纹理空间而非屏幕空间——烘焙与缩放解耦，实现零新架构；代价是缩放时
    /// 行距的**屏幕观感**会变（缩小时变密、放大时变疏），近似手绘的疏密变化。
    pub row_spacing_px: u32,
    /// 墨量（0..=1）：往 [`SeaRecipe::ink`] 混合的最大比例。
    pub alpha: f32,
}

/// 海的画法配方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeaRecipe {
    /// 深度量程（米）：该深度及更深处抵达「深海」色端。
    ///
    /// 手绘海图常用「近岸浅滩 → 深海」两档，量程决定两者过渡的快慢；
    /// 3500 m 覆盖地球大洋极值，浅海盆地（如地中海）会挤在低端。
    pub depth_scale_m: f32,
    /// 深度曲线指数：`t = (depth / scale)^exponent`。
    ///
    /// `1.0` = 线性；`0.5` = 平方根（把浅水段展开，近岸层次可读）；
    /// `> 1` = 压浅水、只让极深处变深。
    pub depth_exponent: f32,
    /// 近岸浅滩带宽度（**格**）：离岸 ≤ 该值的水格向 `water_shore` 偏移。
    ///
    /// **制图参数**而非 DEM 副产品——近岸剖面陡的图照样有浅滩带。
    /// `0` = 关（与旗标 `shore_shallow` 双保险，旗标管主题级开关）。
    pub shore_band_cells: f32,
    /// 浅滩带着色峰值（0..=1）：水线处向 `water_shore` 的最大混合比。
    /// 留一点底色，深度渐变在水线附近仍可读。
    pub shore_peak: f32,
    /// 水面纸纹颗粒振幅（0..=1，相对明度）。
    ///
    /// **海也要吃纸纹**（civ-atlas `grain = 1 + 0.035*(fiber-0.5)`）：水
    /// 若不参与纸纤维，就会与有颗粒的陆地形成「两种画法」的观感断层。
    /// 颗粒是**逐像素世界空间噪声**（不是逐格抖动——水格抖会成马赛克），
    /// 后接水彩晕开不会把它抹掉（颗粒在晕开之后施加）。
    pub water_grain: f32,
    /// 波纹的离岸抖动幅度（格）：把同心圆打散成手绘的不规则线。
    /// 0 = 严格同心（机械感），1~2 格接近手绘。
    pub ripple_warp_cells: f32,
    /// 离岸墨线波纹（由近及远）。空 = 不画。
    pub ripples: &'static [RippleSpec],
    /// 波纹墨色（sRGB 字节）。不随色板走——墨是纸上的固定媒介色。
    pub ink: [u8; 3],
    /// 近岸白色泡沫带（浪花）：贴岸的破碎白带。
    ///
    /// 与 [`Self::ripples`] 同一套机制（离岸距离的高斯带），但**色不同**、
    /// **位不同**：泡沫贴岸（离岸 1~3 格）、墨线在外圈（4/9/15 格）——
    /// 三层叠起来才是手绘海图的水岸层次：岸线墨 → 白浪花 → 离岸波纹。
    /// 空 = 不画。
    pub foam: &'static [RippleSpec],
    /// 泡沫色（sRGB 字节）：近白偏冷（浪花反光），与海色区分。
    pub foam_ink: [u8; 3],
    /// 泡沫破碎度（0..=1）：噪声对泡沫强度的调制幅度。
    /// 0 = 连续的整条白带（机械）；1 = 强烈断续（真实的碎浪）。
    pub foam_break: f32,
    /// 泡沫带的边缘参差幅度（格）：噪声对**位置**的扰动。
    /// 与 [`Self::ripple_warp_cells`] 同理，但独立取值——泡沫的参差
    /// 应比墨线更强（碎浪的不规则）。
    pub foam_warp_cells: f32,
    /// 近岸排线规格。`None` = 不画排线。
    ///
    /// 排线是**离岸带**（贴岸那一圈）的手绘质感，与离岸波纹墨线（更外圈、
    /// 平行于岸线的等距线）不同族：排线是屏幕横向的平行短线，波纹是随岸线
    /// 起伏的等距线。
    pub shore_hatch: Option<HatchSpec>,
    /// 浅滩带**外缘的积墨**（水彩边缘效应）强度（0..=1）：0 = 不画。
    ///
    /// 水彩颜料在湿边堆积，色块交界处会形成一圈略深的轮廓。近岸浅滩带的
    /// 外缘正是这样的交界——加一道淡墨环，浅滩就不再是「发光的渐变」而是
    /// 「有边缘的水彩色块」。
    pub shore_edge_strength: f32,
    /// 浅滩带外缘积墨环的宽度（格）。
    pub shore_edge_width_cells: f32,
    /// 浅滩带外缘积墨的颜色（sRGB 字节）。`None` = 复用 [`Self::ink`]。
    pub shore_edge_ink: Option<[u8; 3]>,
    /// 泡沫破碎噪声的频率（每格）：碎块的细密程度。
    ///
    /// 1.0 上下 = 每格约一个碎块；数值越大碎块越细碎（高频）。
    /// 0 = 不破碎（连续白带）。
    pub foam_break_freq: f32,
}

impl SeaRecipe {
    /// 深度 → 归一化系数 [0, 1]，0 = 海平面，1 = 深海端。
    ///
    /// 与渲染层此前硬编码的 `clamp(depth, 0, 3500)/3500` + `sqrt` **逐位
    /// 等价**（`depth_scale_m = 3500`、`depth_exponent = 0.5` 时）。
    pub fn depth_t(&self, height: i16) -> f32 {
        let scale = self.depth_scale_m.max(1.0);
        let depth = (-height as f32).clamp(0.0, scale);
        (depth / scale).powf(self.depth_exponent)
    }

    /// 离岸距离 → 浅滩混合比 [0, 1]：smoothstep 衰减（水线最浓、带缘归零）。
    ///
    /// `band <= 0` 或缺 `peak` 时恒 0——带外格的输出与「关」逐位一致。
    pub fn shore_band_strength(&self, dist_cells: u16) -> f32 {
        if self.shore_band_cells <= 0.0 || self.shore_peak <= 0.0 {
            return 0.0;
        }
        let t = (1.0 - dist_cells as f32 / self.shore_band_cells).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t) * self.shore_peak
    }

    /// 距离场需要的最大支撑半径（格）：浅滩带与波纹都要看得见。
    ///
    /// 渲染层用它推 `shore_distance_field` 的截断值——**配方改了带宽，场
    /// 自动跟着扩**，不会出现「带比场宽、远端截断成硬边」。
    pub fn field_radius(&self) -> u16 {
        let mut r = self.shore_band_cells.max(0.0);
        for rp in self.ripples {
            r = r.max(rp.dist_cells + rp.sigma_cells * 3.0);
        }
        // 抖动要在场外也成立，留够余量；下限沿用 S1 的 12 格（浅滩带够用）。
        (r.ceil() as u16 + self.ripple_warp_cells.ceil() as u16).max(12)
    }
}

/// 水彩配方（日间）——2026-10-21 起为默认主题 [`crate::theme_pack::builtin::DEFAULT`]。
///
/// 参数依据：civ-atlas `src/render/fantasy.ts` 的 `RIPPLE_D = [4, 9, 15]`、
/// `RIPPLE_A = [0.42, 0.30, 0.18]`、`RIPPLE_SIG = 0.55`、`HATCH_D = 9`、
/// `HATCH_A = 0.16`，以及 `grain = 1 + 0.035*(fiber-0.5)`。
///
/// 与旧硬编码的关系：`depth_scale_m = 3500` + `depth_exponent = 0.5` +
/// `shore_band_cells = 6` 完全保留 S0 已验收的深度观感（抽配方那一步
/// **逐字节不变**，以烘焙 PNG 的 SHA256 对拍验证）。
///
/// 配方调优（2026-10-21 P1，视觉评审后）：浅滩峰值 0.85 → **0.55**（原值
/// 太像「发光」而不是水色；水彩的边缘是积墨不是提亮），并补上评审指出的
/// 两处缺项——**水面纸纹**（`water_grain`，海也要吃纸纤维，否则与有颗粒的
/// 陆地成两种画法）与**离岸墨线**（`ripples`，手绘海图水彩感的主要来源，
/// 不是色阶渐变）。
pub const WATERCOLOR: SeaRecipe = SeaRecipe {
    depth_scale_m: 3500.0,
    depth_exponent: 0.5,
    // 浅滩带 6 → 4 格（2026-10-21 P1）：实机评审「发光感太强、太宽」——
    // 4 格 ≈ 6 屏幕像素（26% 缩放下），是「薄水覆沙」而非「一滩光」。
    shore_band_cells: 4.0,
    shore_peak: 0.55,
    water_grain: 0.035,
    ripple_warp_cells: 1.2,
    // 三圈（离岸 4 / 9 / 15 格）：墨量递减、线宽略增——近岸密、远海疏。
    ripples: &[
        RippleSpec {
            dist_cells: 4.0,
            sigma_cells: 0.55,
            alpha: 0.42,
        },
        RippleSpec {
            dist_cells: 9.0,
            sigma_cells: 0.55,
            alpha: 0.30,
        },
        RippleSpec {
            dist_cells: 15.0,
            sigma_cells: 0.60,
            alpha: 0.18,
        },
    ],
    ink: [0x5A, 0x4E, 0x42],
    // 近岸碎浪：两道贴岸白带（离岸 1 / 2.6 格），破碎度 0.55、边缘参差 1.0 格。
    foam: &[
        RippleSpec {
            dist_cells: 1.0,
            sigma_cells: 0.45,
            alpha: 0.60,
        },
        RippleSpec {
            dist_cells: 2.6,
            sigma_cells: 0.50,
            alpha: 0.26,
        },
    ],
    foam_ink: [0xF2, 0xF6, 0xF2],
    foam_break: 0.78,
    foam_warp_cells: 1.4,
    foam_break_freq: 1.15,
    // 近岸排线（civ-atlas HATCH 语义）：离岸 9 格内、每 7 纹理像素一道横线，
    // 墨量 0.16（照搬参考值）。
    shore_hatch: Some(HatchSpec {
        dist_cells: 4.5,
        row_spacing_px: 16,
        alpha: 0.16,
    }),
    // 浅滩带外缘的积墨环：宽度 1.2 格、强度 0.30 —— 让浅滩从「发光渐变」
    // 变成「有边缘的水彩色块」（水彩颜料在湿边堆积）。
    shore_edge_strength: 0.30,
    shore_edge_width_cells: 1.2,
    shore_edge_ink: None,
};

/// 水彩配方（暮色）——暗纸上的浅滩与墨线都要更含蓄。
pub const WATERCOLOR_DUSK: SeaRecipe = SeaRecipe {
    depth_scale_m: 3500.0,
    depth_exponent: 0.5,
    shore_band_cells: 5.0,
    shore_peak: 0.40,
    water_grain: 0.030,
    ripple_warp_cells: 1.4,
    ripples: &[
        RippleSpec {
            dist_cells: 5.0,
            sigma_cells: 0.60,
            alpha: 0.26,
        },
        RippleSpec {
            dist_cells: 11.0,
            sigma_cells: 0.65,
            alpha: 0.14,
        },
    ],
    ink: [0xAC, 0x9E, 0x88],
    // 暮色：泡沫更弱更暗（暗纸上白浪刺眼），且只留贴岸一道。
    foam: &[RippleSpec {
        dist_cells: 1.1,
        sigma_cells: 0.60,
        alpha: 0.30,
    }],
    foam_ink: [0xC8, 0xD0, 0xC8],
    foam_break: 0.45,
    foam_warp_cells: 1.1,
    foam_break_freq: 0.8,
    // 暮色：排线更疏更淡（暗纸上重墨会脏）。
    shore_hatch: Some(HatchSpec {
        dist_cells: 3.5,
        row_spacing_px: 20,
        alpha: 0.10,
    }),
    shore_edge_strength: 0.20,
    shore_edge_width_cells: 1.0,
    shore_edge_ink: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// 深度曲线与旧硬编码逐位一致（S0/S2 已验收观感的等价性锚点）。
    #[test]
    fn depth_t_matches_the_former_hardcoded_curve() {
        let r = WATERCOLOR;
        assert_eq!(r.depth_t(0), 0.0, "海平面应为 0");
        assert_eq!(r.depth_t(5000), 0.0, "陆上高度应钳到 0");
        assert!((r.depth_t(-3500) - 1.0).abs() < 1e-6, "-3500 应为 1");
        assert_eq!(r.depth_t(-9000), 1.0, "更深应钳到 1");
        // 平方根曲线：-875 → sqrt(0.25) = 0.5
        assert!(
            (r.depth_t(-875) - 0.5).abs() < 1e-6,
            "-875 应为 0.5，实得 {}",
            r.depth_t(-875)
        );
    }

    /// 指数可调：1.0 = 线性，2.0 = 压浅水。
    #[test]
    fn depth_exponent_reshapes_the_curve() {
        let lin = SeaRecipe {
            depth_exponent: 1.0,
            ..WATERCOLOR
        };
        assert!(
            (lin.depth_t(-875) - 0.25).abs() < 1e-6,
            "线性下 -875 应为 0.25"
        );
        let sq = SeaRecipe {
            depth_exponent: 2.0,
            ..WATERCOLOR
        };
        assert!(
            (sq.depth_t(-875) - 0.0625).abs() < 1e-6,
            "平方下应为 0.0625"
        );
    }

    /// 浅滩带：水线最浓、带缘归零、带外恒 0。
    #[test]
    fn shore_band_ramps_to_zero_at_the_band_edge() {
        let r = WATERCOLOR;
        assert!(
            (r.shore_band_strength(0) - r.shore_peak).abs() < 1e-6,
            "水线应等于配方峰值 {}，实得 {}",
            r.shore_peak,
            r.shore_band_strength(0)
        );
        assert_eq!(r.shore_band_strength(6), 0.0, "带缘（d=6）应归零");
        assert_eq!(r.shore_band_strength(30), 0.0, "带外恒 0");
        // 单调递减
        let s: Vec<f32> = (0..=6).map(|d| r.shore_band_strength(d)).collect();
        assert!(
            s.windows(2).all(|w| w[0] >= w[1]),
            "强度应单调递减，实得 {s:?}"
        );
    }

    /// 带宽 / 峰值为 0 时整带关闭（与「旗标关」等价）。
    #[test]
    fn zero_band_or_peak_disables_the_tint() {
        let no_band = SeaRecipe {
            shore_band_cells: 0.0,
            ..WATERCOLOR
        };
        assert_eq!(no_band.shore_band_strength(0), 0.0);
        let no_peak = SeaRecipe {
            shore_peak: 0.0,
            ..WATERCOLOR
        };
        assert_eq!(no_peak.shore_band_strength(0), 0.0);
    }

    /// 场半径随配方扩张：波纹最远圈要能被场覆盖（否则远端截断成硬边）。
    #[test]
    fn field_radius_covers_ripples_and_band() {
        let no_ripples = SeaRecipe {
            ripples: &[],
            ripple_warp_cells: 0.0,
            ..WATERCOLOR
        };
        assert_eq!(no_ripples.field_radius(), 12, "无波纹时应保底 12 格");
        assert!(
            WATERCOLOR.field_radius() >= 17,
            "水彩配方最远波纹 15 格应被覆盖，实得 {}",
            WATERCOLOR.field_radius()
        );
        let with_ripples = SeaRecipe {
            ripples: &[RippleSpec {
                dist_cells: 15.0,
                sigma_cells: 0.55,
                alpha: 0.18,
            }],
            ..WATERCOLOR
        };
        assert!(
            with_ripples.field_radius() >= 17,
            "最远波纹 15 + 3σ 应被覆盖，实得 {}",
            with_ripples.field_radius()
        );
    }

    /// 两套配方的浅滩表现有可见差异（防「改了日间忘配暮色」）。
    #[test]
    fn dusk_recipe_differs_visibly_from_daylight() {
        assert_ne!(
            WATERCOLOR.shore_band_cells,
            WATERCOLOR_DUSK.shore_band_cells
        );
        assert_ne!(WATERCOLOR.shore_peak, WATERCOLOR_DUSK.shore_peak);
        assert_ne!(WATERCOLOR.ink, WATERCOLOR_DUSK.ink, "墨色应不同");
    }
}
