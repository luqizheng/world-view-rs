//! 地形装饰（terrain decor，ADR-0031）——按栅格群系采样的程序化符号覆盖层。
//!
//! ## 职责
//! - 从**活动地形栅格**（`active_terrain`）按视口采样格子，把六类群系
//!   （森林 / 平原 / 丘陵 / 山地 / 雪原 / 沼泽）演绎成程序化符号
//!   （树冠团、草点、半圆丘、三角山 + 雪帽、白丘、芦苇）；
//! - 三档 LOD：Nation 档不画（色块承带）、Region 档粗步距、Edit 档细步距；
//! - 确定性：落点抖动用 splitmix64(格坐标 + kind)——同工程重开树不跳；
//! - 渲染走**六桶合批**（每类一个 mesh + 一个实体），可见集 / 尺寸 / Z
//!   变化才经 `Assets::get_mut` 原地改写顶点（ADR-0030 D4 同纪律）。
//!
//! ## 与 `feature_overlays`（主题地界覆盖物）的边界
//!
//! 那边是 **Place.biome 标记地界驱动**（作者声明的标记边界长树，ADR-0032 后唯一词汇）；这边是
//! **地形格驱动**（栅格群系本身长装饰）——不依赖任何 `Place`，纯地形投影。
//!
//! ## 边界（不做什么）
//! - **不写时间比较 / 不写世界**（AGENTS §3）；
//! - **不引入位图瓦片**（JRPG 图集已弃用；全部程序化三角化）；
//! - 符号颜色是 v1 常量（每类一枚「墨色」），主题化随 ThemePack 演绎规则后继
//!   （README §12-30 同款延期）。

#![cfg(feature = "app")]

use crate::fp::splitmix64;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{lod_tier, GridConfig, ViewState, WorldProjection};
use crate::terrain_view::terrain_resolve::active_terrain;
use crate::theme::decor::{DecorRecipe, SymbolStyle};
use wv_terrain::biome::{
    BIOME_DESERT, BIOME_FOREST, BIOME_GOBI, BIOME_GRASSLAND, BIOME_HILLS, BIOME_JUNGLE,
    BIOME_MOUNTAINS, BIOME_OASIS, BIOME_PLAINS, BIOME_SAVANNA, BIOME_SNOWFIELD, BIOME_SWAMP,
    BIOME_TUNDRA, BIOME_WETLAND,
};

/// 装饰 Z：地形族之上（等值线 0.04 / 雾霭 0.06 / 地界符号 0.07）、道路（0.09）之下。
const DECOR_Z: f32 = 0.075;
/// 桶间 Z 微差（六桶按序号错开，防同深度闪烁）。
const BUCKET_Z_STEP: f32 = 0.0004;
// 采样步距 / 尺寸系数已迁入主题配方（`wv_default_theme::decor::DecorRecipe`，
// ADR-0048 追记二）——渲染层不再持有美术常量。
/// 单类单帧符号组数上限（步距本身已把数量压到屏幕量级，此为兜底）。
const MAX_SITES_PER_KIND: usize = 2200;
/// 视口外扩余量（屏幕像素）：拖动相机时边缘符号不闪断。
const VIEWPORT_MARGIN_PX: f32 = 32.0;
/// `SymbolStyle::Tile` 的一次性告警标记（ADR-0059 D3 / 追记一）：贴图装载通道
/// 待 v2，warn 每进程**每类符号**只发一次——`site_primitives` 是逐站点调用的，
/// 每站点刷屏会把日志埋了。
///
/// **按类别各持一个 `Once`**（不共用）：共用会让后出现的类别被前一类的告警
/// 吞掉，调试「谁真的用了 Tile」时看不到真实使用者。
static TILE_STYLE_WARNED_MOUNTAIN: std::sync::Once = std::sync::Once::new();
static TILE_STYLE_WARNED_FOREST: std::sync::Once = std::sync::Once::new();

/// `Tile` 键在 v1 的统一兜底：一次性 warn 后由调用方走 `Default` 逻辑。
fn warn_tile_style_once(flag: &std::sync::Once, kind: &str, key: &str) {
    flag.call_once(|| {
        warn!(
            "SymbolStyle::Tile({key:?})：{kind}贴图装载通道待 v2（ADR-0023 M3），回退内置默认符号"
        );
    });
}

/// 装饰类别（八类）。下标即渲染桶序号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorKind {
    Forest = 0,
    Plains = 1,
    Hills = 2,
    Mountain = 3,
    Snow = 4,
    Marsh = 5,
    Desert = 6,
    /// 浅海白波（ADR-0039 D2）：水格专属，高度带 [-220, 0]。
    OceanWaves = 7,
}

impl DecorKind {
    /// 全部类别（按桶序号）。
    pub const ALL: [DecorKind; 8] = [
        DecorKind::Forest,
        DecorKind::Plains,
        DecorKind::Hills,
        DecorKind::Mountain,
        DecorKind::Snow,
        DecorKind::Marsh,
        DecorKind::Desert,
        DecorKind::OceanWaves,
    ];

    /// 类别的符号墨色（sRGB 字节 + alpha）。v1 常量；主题化后继。
    fn ink(self, recipe: &DecorRecipe) -> ([u8; 3], f32) {
        let i = match self {
            DecorKind::Forest => recipe.forest_ink,
            DecorKind::Plains => recipe.plains_ink,
            DecorKind::Hills => recipe.hills_ink,
            DecorKind::Mountain => recipe.mountain_ink,
            DecorKind::Snow => recipe.snow_ink,
            DecorKind::Marsh => recipe.marsh_ink,
            DecorKind::Desert => recipe.desert_ink,
            DecorKind::OceanWaves => recipe.wave_ink,
        };
        (i.color, i.alpha)
    }
}

/// 栅格群系 → 装饰类别。水系 / 荒漠 / 裸地不演绎（`None`）。
///
/// 投影唯一入口；与 `wv-terrain::biome::display_kind` 的词汇表同源。
pub fn decor_kind_for_biome(biome: u8) -> Option<DecorKind> {
    match biome {
        BIOME_FOREST | BIOME_JUNGLE | 11 => Some(DecorKind::Forest), // 11 = TROPICAL_RAINFOREST（wv-terrain 未导出常量，词表锚定）
        BIOME_PLAINS | BIOME_GRASSLAND | BIOME_SAVANNA | BIOME_OASIS => Some(DecorKind::Plains),
        BIOME_HILLS => Some(DecorKind::Hills),
        BIOME_MOUNTAINS => Some(DecorKind::Mountain),
        BIOME_SNOWFIELD | BIOME_TUNDRA => Some(DecorKind::Snow),
        BIOME_SWAMP | BIOME_WETLAND => Some(DecorKind::Marsh),
        BIOME_DESERT | BIOME_GOBI => Some(DecorKind::Desert),
        // 水系（14/15/16）与盐滩（19）不画装饰：海面有水彩带，盐滩保持纯色平面。
        _ => None,
    }
}

/// 覆盖物**变体**（ADR-0045 D2/D9）：同一符号族的不同长相。
///
/// **键由主题拥有**（`ThemePack::region_overlays`），渲染层只负责把键映射到
/// 程序化形状；未登记的键 → `None` → 沿用 cell 派生的默认形状（事实不消失）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorVariant {
    /// 针叶（细三角成排）。
    Conifer,
    /// 阔叶（圆团）。
    Broadleaf,
    /// 稀树（单颗小团）。
    Sparse,
    /// 乱石滩涂（几块小石 + 碎砾）——与族无关的专用形状。
    Boulder,
}

/// 覆盖物键 → 变体（渲染侧的映射表）。
///
/// **开放词表**：主题可以给任意键；这里只登记**渲染层能画**的那些。
/// 未登记的键 → `None` → 用 cell 派生的默认形状 + 主题给的颜色。
pub fn decor_variant_for_key(key: &str) -> Option<DecorVariant> {
    match key {
        "conifer" => Some(DecorVariant::Conifer),
        "broadleaf" => Some(DecorVariant::Broadleaf),
        "sparse" => Some(DecorVariant::Sparse),
        "boulder" => Some(DecorVariant::Boulder),
        _ => None,
    }
}

/// 装饰 LOD 档：Nation 粗步距（大地貌团）/ Region 粗 / Edit 细。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorBand {
    Nation,
    Region,
    Edit,
}

/// 缩放 → 装饰档。三档都画：Nation 档是新默认观感的全局地貌层
/// （ADR-0036 取代 ADR-0031 D2「Nation 不画」的旧口径）。
pub fn decor_band(view_scale: f32) -> Option<DecorBand> {
    match lod_tier(view_scale) {
        crate::state::LodTier::Nation => Some(DecorBand::Nation),
        crate::state::LodTier::Region => Some(DecorBand::Region),
        crate::state::LodTier::Edit => Some(DecorBand::Edit),
    }
}

impl DecorBand {
    /// 采样步距（屏幕像素）——由主题配方给定（ADR-0048 追记二）。
    fn spacing_px(self, recipe: &DecorRecipe) -> f32 {
        match self {
            DecorBand::Nation => recipe.spacing_px_nation,
            DecorBand::Region => recipe.spacing_px_region,
            DecorBand::Edit => recipe.spacing_px_edit,
        }
    }

    /// 符号尺寸系数（全局档放大，抵消粗步距带来的稀疏感）——主题配方给定。
    fn size_mul(self, recipe: &DecorRecipe) -> f32 {
        match self {
            DecorBand::Nation => recipe.size_mul_nation,
            DecorBand::Region => recipe.size_mul_region,
            DecorBand::Edit => recipe.size_mul_edit,
        }
    }
}

fn hash01(x: i64, y: i64, seed: u64) -> f32 {
    let h = splitmix64(
        (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ seed,
    );
    (h >> 11) as f32 / (1u64 << 53) as f32
}

/// 一个站点的一次性输入（重构自原 8 参 `site_primitives`）：把位置参数收进
/// 具名字段，消除 `too_many_arguments`，并让各符号族各成一法、可单独演进。
///
/// 字段语义：
/// - `kind`：该格**该画哪一族**的符号（域事实 → 画法）。
/// - `band`：缩放档，决定尺寸倍率 `size_mul`。
/// - `(cx, cy)`：格坐标——**同时也是哈希种子的一部分**，换格即换抖动。
/// - `height_m`：该格海拔（米），雪帽线 / 露岩线由此判定。
/// - `fy`：归一化 y（0 = 北），森林气候带切点由此判定。
/// - `variant`：region 覆盖物**强制**的变体（作者声明优先于气候派生）。
/// - `recipe`：主题配方（墨色 / 阈值 / 步距 / 尺寸倍率 / 画法）。
struct SiteCtx<'a> {
    kind: DecorKind,
    band: DecorBand,
    cx: i64,
    cy: i64,
    height_m: i16,
    fy: f32,
    variant: Option<DecorVariant>,
    recipe: &'a DecorRecipe,
}

impl SiteCtx<'_> {
    /// 产出该站点的全部符号图元：`(|dx|, |dy|, r|半宽, 半高|, 桶)` 列表。
    ///
    /// 坐标是**相对格中心的屏幕像素**（调用方乘 `view.scale` 转世界单位）。
    /// 输出四元组由 [`build_bucket_mesh`] 统一三角化——这里只产出**符号图元
    /// 参数**（v1 用「相对偏移 + 半径」的圆 / 半圆 / 三角参数化，避免每个采样点
    /// 手写顶点循环：改形状只动这里一处）。
    ///
    /// 分派：乱石滩涂（Boulder）是**与族无关**的专用形状，先行特判；其余按
    /// `kind` 分派到各符号族方法。
    fn primitives(self) -> Vec<SitePrim> {
        let mut out = Vec::new();
        let m = self.band.size_mul(self.recipe);
        if self.variant == Some(DecorVariant::Boulder) {
            self.boulder(&mut out, m);
            return out;
        }
        // 三个独立哈希盐：落点抖动 / 形状变体 / 尺寸变体。
        let r = [
            hash01(self.cx, self.cy, 1),
            hash01(self.cx, self.cy, 2),
            hash01(self.cx, self.cy, 3),
        ];
        match self.kind {
            DecorKind::Forest => self.forest(&mut out, m, r),
            DecorKind::Plains => self.plains(&mut out, m, r),
            DecorKind::Hills => self.hills(&mut out, m, r),
            DecorKind::Mountain => self.mountain(&mut out, m, r),
            DecorKind::Snow => self.snow(&mut out, m, r),
            DecorKind::Desert => self.desert(&mut out, m, r),
            DecorKind::OceanWaves => self.ocean_waves(&mut out, m, r),
            DecorKind::Marsh => self.marsh(&mut out, m, r),
        }
        out
    }

    /// 乱石滩涂：与族无关的专用形状（region 覆盖物键 `boulder_field`）。
    fn boulder(&self, out: &mut Vec<SitePrim>, m: f32) {
        for k in 0..4u64 {
            let jx = (hash01(self.cx, self.cy, 30 + k) - 0.5) * 9.0;
            let jy = (hash01(self.cx, self.cy, 40 + k) - 0.5) * 8.0;
            out.push(SitePrim::colored(
                self.kind,
                jx * m,
                jy * m,
                1.7 * m,
                2.0 * m,
                PrimShape::Triangle,
                [0.54, 0.52, 0.46, 0.85],
            ));
        }
    }

    /// 森林：画法分派（ADR-0059 追记一）+ 气候变体（ADR-0039 D3）+ region 强制变体。
    ///
    /// 画法分派：「画不画 / 换画法」由主题裁定——
    /// None：群系格不铺林团，林身份交给地形色（core[4]）+ 具名地物的
    ///   覆盖物树图集（`FeatureRule::symbol_texture`），本站点不产图元；
    /// Tile：贴图装载通道待 v2（ADR-0023 M3），一次性 warn 后走 Default
    ///   保底——森林的变体维度（寒带 / 温带 / 干旱）在贴图方案下由
    ///   「图集等分 + `(种子, 站点序号) % variants`」承载，故单键契约
    ///   无需扩容（见 `DecorRecipe::forest_style` 文档）；
    /// Default：内置三变体程序化形状（下方逻辑不变）。
    fn forest(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let (r1, r2) = (r[0], r[1]);
        match self.recipe.forest_style {
            SymbolStyle::None => return,
            SymbolStyle::Tile(key) => {
                warn_tile_style_once(&TILE_STYLE_WARNED_FOREST, "森林", key);
            }
            SymbolStyle::Default => {}
        }
        // 气候变体（ADR-0039 D3）：沿 y 轴（0=北）三档。
        // 颜色 = 顶点色（桶材质为白）；形状区分针叶排 / 阔叶团 / 绿洲稀树。
        // 变体色由主题配方给定（ADR-0048 追记二）。
        let boreal = self.recipe.conifer_ink.to_rgba();
        let temperate = self.recipe.temperate_ink.to_rgba();
        let oasis = self.recipe.oasis_ink.to_rgba();
        // **region 覆盖物可强制变体**（ADR-0045 D2/D9）：作者声明「这片用
        // 针叶画」→ 变体由声明决定，**优先于** y 轴气候带派生。
        let variant = match self.variant {
            Some(v @ (DecorVariant::Conifer | DecorVariant::Broadleaf | DecorVariant::Sparse)) => v,
            _ => {
                if self.fy < self.recipe.climate_cuts[0] {
                    DecorVariant::Conifer
                } else if self.fy < self.recipe.climate_cuts[1] {
                    DecorVariant::Broadleaf
                } else {
                    DecorVariant::Sparse
                }
            }
        };
        if variant == DecorVariant::Conifer {
            // 针叶：3 棵细三角（宽窄高挑），成排。
            for s in 0..3u64 {
                let jx = (hash01(self.cx, self.cy, 10 + s) - 0.5) * 9.0;
                let jy = (hash01(self.cx, self.cy, 20 + s) - 0.5) * 8.0;
                out.push(SitePrim::colored(
                    self.kind,
                    jx * m,
                    jy * m,
                    1.7 * m,
                    3.4 * m,
                    PrimShape::Triangle,
                    boreal,
                ));
            }
        } else if variant == DecorVariant::Broadleaf {
            // 阔叶：3 颗小圆（径 2.6px），确定性散布在 ±5px 内。
            for s in 0..3u64 {
                let jx = (hash01(self.cx, self.cy, 10 + s) - 0.5) * 10.0;
                let jy = (hash01(self.cx, self.cy, 20 + s) - 0.5) * 8.0;
                out.push(SitePrim::colored(
                    self.kind,
                    jx * m,
                    jy * m,
                    2.6 * m,
                    2.6 * m,
                    PrimShape::Circle,
                    temperate,
                ));
            }
        } else {
            // 绿洲稀树：1 颗暖绿小团（密度减半由 keep_prob 处理）。
            out.push(SitePrim::colored(
                self.kind,
                (r1 - 0.5) * 6.0 * m,
                (r2 - 0.5) * 6.0 * m,
                1.8 * m,
                1.8 * m,
                PrimShape::Circle,
                oasis,
            ));
        }
    }

    /// 平原：草点（单颗 1.2px 微圆，稀疏感靠步距）。
    fn plains(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let (r1, r2) = (r[0], r[1]);
        out.push(SitePrim::circle(
            self.kind,
            (r1 - 0.5) * 6.0 * m,
            (r2 - 0.5) * 6.0 * m,
            1.2 * m,
        ));
    }

    /// 丘陵：半圆丘（宽 9px 高 5px，底边贴格中心下方）。
    fn hills(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let r1 = r[0];
        out.push(SitePrim::half_disc(
            self.kind,
            (r1 - 0.5) * 4.0 * m,
            1.0 * m,
            4.5 * m,
            5.0 * m,
        ));
    }

    /// 山：画法分派（ADR-0059 D3）+ 三角山 + 背光面 + 簇生次峰 + 雪帽 + 露岩。
    fn mountain(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let (r1, r2, r3) = (r[0], r[1], r[2]);
        // 画法分派（ADR-0059 D3）：「画不画 / 换画法」由主题裁定——
        // None：山体表达收敛到等值线 / 皴法 / 海配方 / 地形色，
        //   本站点不产任何图元（水墨包的画法，方案 §7 留白裁定的彻底化）；
        // Tile：贴图装载通道待 v2（ADR-0023 M3），一次性 warn 后
        //   走 Default 保底（「缺资产不崩」）；
        // Default：内置默认程序化形状（下方逻辑不变）。
        match self.recipe.mountain_style {
            SymbolStyle::None => return,
            SymbolStyle::Tile(key) => {
                warn_tile_style_once(&TILE_STYLE_WARNED_MOUNTAIN, "山体", key);
            }
            SymbolStyle::Default => {}
        }
        // 三角山（ADR-0042 手绘化）：主三角右侧叠一道深色背光面——
        // 参考图的山是「受光面 + 背光面」两面体，单色三角只是剪影。
        // 宽 13px 高 11px；海拔 ≥ 1800m 顶部叠雪帽（小三角，雪桶）。
        //
        // D13 变体化：同一种三角山在全图重复会像「栅栏」。按格哈希给
        // 每座山 ±35% 尺寸 + 两成概率簇生第二峰——山链有大小起伏，
        // 且不再是一个模子刻出来的。
        let size_k = 0.68 + 0.64 * r3;
        let dx = (r1 - 0.5) * 5.0 * m;
        let w = 6.5 * m * size_k;
        let h = 11.0 * m * size_k;
        out.push(SitePrim::triangle(self.kind, dx, 2.0 * m, w, h));
        out.push(SitePrim::colored(
            self.kind,
            dx + w * 0.37,
            2.0 * m,
            w * 0.65,
            h * 0.76,
            PrimShape::Triangle,
            self.recipe.mountain_shade_ink.to_rgba(), // 背光面（压暗右侧）
        ));
        // 簇生次峰：约 30% 的山旁再起一座小的（低 35%、偏移随机）。
        if r2 > 0.70 {
            let ox = (r2 - 0.5) * 14.0 * m;
            out.push(SitePrim::triangle(
                self.kind,
                dx + ox,
                2.0 * m,
                w * 0.55,
                h * 0.62,
            ));
        }
        if self.height_m >= self.recipe.snow_cap_h {
            out.push(SitePrim::triangle(
                DecorKind::Snow,
                0.0,
                4.5 * m * size_k,
                2.6 * m * size_k,
                3.5 * m * size_k,
            ));
        }
        // 极高山露岩（ADR-0039 D4）：>=3400m 雪帽内缘叠露岩三角
        // （进 Snow 桶第二层图元，顶点色区分）。色走主题 rock_ink
        // （ADR-0059 D6：此前硬编码 #C9B98A，绕过主题——水墨包的
        // 岩墨灰自此生效）。
        if self.height_m >= self.recipe.rock_h {
            out.push(SitePrim::colored(
                DecorKind::Snow,
                0.0,
                3.2 * m * size_k,
                1.5 * m * size_k,
                2.2 * m * size_k,
                PrimShape::Triangle,
                self.recipe.rock_ink.to_rgba(),
            ));
        }
    }

    /// 雪原：白丘（宽 10px 高 4px 的扁半圆）。
    fn snow(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let r1 = r[0];
        out.push(SitePrim::half_disc(
            self.kind,
            (r1 - 0.5) * 5.0 * m,
            1.0 * m,
            5.0 * m,
            4.0 * m,
        ));
    }

    /// 沙漠：两道扁半圆脊错位叠出风向（宽 9/7px、高 3.2/2.6px）。
    fn desert(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let (r1, r2) = (r[0], r[1]);
        out.push(SitePrim::half_disc(
            self.kind,
            (r1 - 0.5) * 3.0 * m,
            1.0 * m,
            4.5 * m,
            3.2 * m,
        ));
        out.push(SitePrim::half_disc(
            self.kind,
            (r2 - 0.5) * 3.0 * m + 2.6 * m,
            -1.2 * m,
            3.5 * m,
            2.6 * m,
        ));
    }

    /// 浅海白波（ADR-0039 D2，ADR-0042 提密提亮）：1 条细弧。
    ///
    /// 旧值 2.4×1.4 @ alpha 0.5 在 Nation 档几乎不可见（实机截图确认）；
    /// 放大到 3.4×2.0 @ alpha 0.62 后远处读得出「海在动」。密度衰减由
    /// 调用方 `keep_prob` 处理（深度越深越稀）。
    fn ocean_waves(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let (r1, r2) = (r[0], r[1]);
        out.push(SitePrim::colored(
            self.kind,
            (r1 - 0.5) * 6.0 * m,
            (r2 - 0.5) * 4.0 * m,
            3.4 * m,
            2.0 * m,
            PrimShape::Arc,
            [0.99, 0.99, 1.0, 0.62], // 白波
        ));
    }

    /// 沼泽：芦苇（2 根细三角，高 6px）。
    fn marsh(&self, out: &mut Vec<SitePrim>, m: f32, r: [f32; 3]) {
        let r1 = r[0];
        let ox = (r1 - 0.5) * 6.0 * m;
        out.push(SitePrim::triangle(
            self.kind,
            ox - 2.0 * m,
            0.0,
            0.6 * m,
            6.0 * m,
        ));
        out.push(SitePrim::triangle(
            self.kind,
            ox + 2.0 * m,
            0.0,
            0.6 * m,
            5.0 * m,
        ));
    }
}

/// 一个符号图元：形状 + 相对格中心的屏幕像素偏移 / 半径（调用方统一换算）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SitePrim {
    pub kind: PrimShape,
    pub dx: f32,
    pub dy: f32,
    pub half_w: f32,
    pub half_h: f32,
    /// 专属桶（山雪帽跨桶进 Snow）。
    pub bucket: DecorKind,
    /// 图元颜色（sRGB 0..1 + alpha）。ADR-0039：变体配色走顶点色，
    /// 同桶可多色（雪帽白 + 帽缘露岩、针叶深绿 + 阔叶绿团）。
    pub color: [f32; 4],
}

impl SitePrim {
    fn circle(bucket: DecorKind, dx: f32, dy: f32, r: f32) -> Self {
        Self::colored(bucket, dx, dy, r, r, PrimShape::Circle, [0.0; 4])
    }

    fn colored(
        bucket: DecorKind,
        dx: f32,
        dy: f32,
        half_w: f32,
        half_h: f32,
        kind: PrimShape,
        color: [f32; 4],
    ) -> Self {
        Self {
            kind,
            dx,
            dy,
            half_w,
            half_h,
            bucket,
            color,
        }
    }

    fn half_disc(bucket: DecorKind, dx: f32, dy: f32, half_w: f32, half_h: f32) -> Self {
        Self::colored(
            bucket,
            dx,
            dy,
            half_w,
            half_h,
            PrimShape::HalfDisc,
            [0.0; 4],
        )
    }

    fn triangle(bucket: DecorKind, dx: f32, dy: f32, half_w: f32, half_h: f32) -> Self {
        Self::colored(
            bucket,
            dx,
            dy,
            half_w,
            half_h,
            PrimShape::Triangle,
            [0.0; 4],
        )
    }
}

/// 符号形状（三角化口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimShape {
    Circle,
    HalfDisc,
    Triangle,
    /// 细弧（上半圆环）：OceanWaves 白波。
    Arc,
}

/// 生成一个格采样点的三角形顶点 / 索引，写入对应桶（世界坐标）。
pub fn push_site_tris(
    site: &SitePrim,
    cx: f32,
    cy: f32,
    px_to_wu: f32,
    verts: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
) {
    // 顶点基址。
    let base = verts.len() as u32;
    let x = cx + site.dx * px_to_wu;
    // 顶点色：0 的占位色（构造器默认）由桶默认 ink 补齐——调用方保证
    // color 非占位。此处只透传。
    let color = site.color;
    let y = cy + site.dy * px_to_wu;
    let hw = site.half_w * px_to_wu;
    let hh = site.half_h * px_to_wu;
    match site.kind {
        PrimShape::Circle => {
            verts.push([x, y, 0.0]);
            colors.push(color);
            for s in 0..POI_LIKE_SEGMENTS {
                let a = s as f32 / POI_LIKE_SEGMENTS as f32 * std::f32::consts::TAU;
                verts.push([x + hw * a.cos(), y + hh * a.sin(), 0.0]);
                colors.push(color);
            }
            for s in 1..=POI_LIKE_SEGMENTS {
                let a = base + s as u32;
                let b = base + (s % POI_LIKE_SEGMENTS) as u32 + 1;
                indices.extend([base, a, b]);
            }
        }
        PrimShape::HalfDisc => {
            // 半圆：直径边贴 (x, y-hh)（底边在下，丘顶朝上）。
            verts.push([x - hw, y - hh, 0.0]);
            colors.push(color);
            verts.push([x + hw, y - hh, 0.0]);
            colors.push(color);
            for s in 0..=POI_LIKE_SEGMENTS {
                let a = std::f32::consts::PI * (s as f32 / POI_LIKE_SEGMENTS as f32);
                verts.push([x - hw * a.cos(), y - hh + hh * a.sin(), 0.0]);
                colors.push(color);
            }
            for s in 0..POI_LIKE_SEGMENTS {
                let a = base;
                let b = base + 2 + s as u32;
                let c = base + 3 + s as u32;
                indices.extend([a, b, c]);
            }
        }
        PrimShape::Triangle => {
            verts.push([x - hw, y - hh, 0.0]);
            colors.push(color);
            verts.push([x + hw, y - hh, 0.0]);
            colors.push(color);
            verts.push([x, y + hh, 0.0]);
            colors.push(color);
            indices.extend([base, base + 1, base + 2]);
        }
        PrimShape::Arc => {
            // 细弧：上半圆环（外半径 hw，内半径 hw*0.55），8 段。
            const SEG: usize = 8;
            let inner = hw * 0.55;
            for s in 0..=SEG {
                let a = std::f32::consts::PI * (s as f32 / SEG as f32);
                let (ca, sa) = (a.cos(), a.sin());
                verts.push([x - hw * ca, y + hh * sa, 0.0]);
                colors.push(color);
                verts.push([x - inner * ca, y + inner * sa, 0.0]);
                colors.push(color);
            }
            for s in 0..SEG {
                let a = base + (s * 2) as u32;
                let b = a + 1;
                let c = a + 2;
                let d = a + 3;
                indices.extend([a, b, c]);
                indices.extend([b, d, c]);
            }
        }
    }
}

/// 圆 / 扇区的离散段数（装饰符号 ≤14px，12 段与 POI 同观感）。
const POI_LIKE_SEGMENTS: usize = 12;

// ---------------------------------------------------------------------------
// 渲染侧：六桶合批池
// ---------------------------------------------------------------------------

/// 桶合批的重建判据：可见集指纹 + 符号尺寸 + Z。
#[derive(Debug, Clone, Copy, PartialEq)]
struct DecorBatchKey {
    fingerprint: u64,
    size: f32,
    z: f32,
}

/// 一个桶的实体与 Mesh 句柄。
struct DecorBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: DecorBatchKey,
}

/// 一个桶的池。
#[derive(Default)]
struct DecorBatchPool {
    batch: Option<DecorBatch>,
}

impl DecorBatchPool {
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

/// 七桶池 + 每桶材质句柄。
#[derive(Resource, Default)]
pub struct TerrainDecorPools {
    pools: [DecorBatchPool; 8],
    material: Option<Handle<ColorMaterial>>,
}

impl TerrainDecorPools {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for p in &mut self.pools {
            p.despawn(commands);
        }
    }
}

/// 保证装饰材质存在（ADR-0039：顶点色承载全部颜色 → 单一白材质，
/// 惰性创建一次；`DecorKind::ink` 保留作桶默认色/调试兜底）。
fn ensure_material(
    pools: &mut TerrainDecorPools,
    materials: &mut Assets<ColorMaterial>,
) -> Handle<ColorMaterial> {
    pools
        .material
        .get_or_insert_with(|| materials.add(ColorMaterial::from_color(Color::WHITE)))
        .clone()
}

/// 森林密度场的缓存（方案 1）——`sync_terrain_decor` 用它调制森林符号的保留率。
///
/// **为什么缓存**：森林密度是**全局量**（一片林的疏密取决于整张图的聚集噪声），
/// 不能像逐格属性那样采样即得；每帧重算 1024² × 2 层 Perlin 噪声是纯浪费
/// ——地形没变时场不会变。**键 = `terrain_data_hash`**（与离岸场缓存同源）：
/// hash 不变直接复用，变了整场重算。
///
/// **与离岸场缓存同构、不同源**：离岸场用 BFS 距离，森林场用
/// [`wv_terrain::forest::generate_forest_density_from_biome`]（群系 + 林线 +
/// 聚集噪声）。两者生命周期相同（地形变则重算），故各自独立缓存。
#[derive(Resource, Default)]
pub struct ForestDensityCache {
    hash: u64,
    field: std::sync::Arc<Vec<f32>>,
    w: u32,
    h: u32,
}

/// 森林密度场的固定种子。
///
/// 密度场是**视觉派生量**（森林疏密），不是世界事实，故用常量种子而非世界
/// 种子：同一张地形在任何机器、任何次重开都得到同一片林。
/// 接入世界种子的收敛点：把本常量换成 `projection.world` 的种子派生值。
const FOREST_DENSITY_SEED: u64 = 0x4653_5441; // "FSTA"

/// 森林桶的保留率 = 密度场 × 主题增益（裁剪到 `0..=1`）。
///
/// **为什么抽成纯函数**：疏密是「地形事实 × 主题参数」的乘积，两侧都可独立
/// 断言——密度场由 `wv_terrain::forest` 测，主题增益在这里测。
pub(crate) fn forest_keep_prob(density: f32, gain: f32) -> f32 {
    (density * gain).clamp(0.0, 1.0)
}

impl ForestDensityCache {
    /// 取当前地形的森林密度场（hash 不匹配则重算）。
    fn get_or_compute(&mut self, data: &wv_core::terrain::TerrainData) -> std::sync::Arc<Vec<f32>> {
        let hash = crate::terrain_view::isoline_overlay::terrain_data_hash(data);
        let (w, h) = (data.width.max(1), data.height.max(1));
        if self.hash != hash || self.w != w || self.h != h {
            let n = (w as usize) * (h as usize);
            let mut biomes = vec![0u8; n];
            let mut heights = vec![0i16; n];
            for (i, c) in data.cells.iter().take(n).enumerate() {
                biomes[i] = c.biome;
                heights[i] = c.height;
            }
            let field = wv_terrain::forest::generate_forest_density_from_biome(
                w as usize,
                h as usize,
                &biomes,
                &heights,
                FOREST_DENSITY_SEED,
            );
            *self = Self {
                hash,
                field: std::sync::Arc::new(field),
                w,
                h,
            };
        }
        self.field.clone()
    }
}

/// 离岸距离场的缓存（ADR-0048 追记四）——`sync_terrain_decor` 按视口采样
/// 格子判 OceanWaves 用。
///
/// **为什么缓存**：离岸距离是全局量（某格的值取决于全场水陆分布），不能像
/// 逐格属性那样采样即得；每帧重算 O(N) BFS（1024² ≈ 数 ms）虽不致命但纯浪费
/// ——地形没变时场不会变。**键 = `terrain_data_hash`**（与 isoline 缓存同源）：
/// hash 不变直接复用，变了整场重算。
///
/// 与烘焙路径（`cells.rs` 的 `shore_field_for`）**不共享**：烘焙是纯函数管线
/// （每次自算，行带并行契约），装饰是增量同步管线（缓存复用）——两条管线
/// 的生命周期不同，强行共用会把纯函数签名撕开。
#[derive(Resource, Default)]
pub struct ShoreFieldCache {
    hash: u64,
    field: std::sync::Arc<Vec<u16>>,
    w: u32,
    h: u32,
}

impl ShoreFieldCache {
    /// 取当前地形的离岸距离场（hash 不匹配则重算）。
    fn get_or_compute(
        &mut self,
        data: &wv_core::terrain::TerrainData,
        radius: u16,
    ) -> std::sync::Arc<Vec<u16>> {
        let hash = crate::terrain_view::isoline_overlay::terrain_data_hash(data);
        let (w, h) = (data.width.max(1), data.height.max(1));
        if self.hash != hash || self.w != w || self.h != h {
            let field = wv_terrain::shore::shore_distance_field(
                w,
                h,
                |x, y| {
                    let c = &data.cells[(y as usize) * w as usize + x as usize];
                    !(c.surface == wv_terrain::SURFACE_WATER || c.height < 0)
                },
                radius,
            );
            *self = Self {
                hash,
                field: std::sync::Arc::new(field),
                w,
                h,
            };
        }
        self.field.clone()
    }
}

/// 地形装饰主 system。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_decor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    overlay_index: Res<crate::region_overlay::RegionOverlayIndex>,
    mut pools: ResMut<TerrainDecorPools>,
    mut shore_cache: ResMut<ShoreFieldCache>,
    mut forest_cache: ResMut<ForestDensityCache>,
) {
    let enabled = recipe.enabled(PassId::TerrainDecor) && layers.terrain.visible;
    if !enabled {
        pools.despawn_all(&mut commands);
        return;
    }
    // Nation 档画粗步距大地貌团（ADR-0036 取代 ADR-0031 D2/D3）。
    let Some(band) = decor_band(view.scale) else {
        pools.despawn_all(&mut commands);
        return;
    };
    let Some((_, data)) = active_terrain(&projection.world, drill.current()) else {
        pools.despawn_all(&mut commands);
        return;
    };
    if grid.cell_size <= 0.0 || data.width == 0 || data.height == 0 {
        return;
    }

    // 主题的装饰配方（ADR-0048 追记二）：高度阈值 / 气候带 / 墨色 / 步距
    // 全部由主题给定——渲染层只做执行器。
    let recipe = &view.active_theme.decor;

    // 离岸距离场（ADR-0048 追记四）：OceanWaves 的离岸带判定用。缓存 Resource
    // 按 terrain hash 键控，地形没变就复用（O(N) BFS 只在编辑后算一次）。
    let shore_field = shore_cache.get_or_compute(data, recipe.wave_band_max_cells);

    // 森林密度场（方案 1）：「哪里有林 / 多密」由地形事实（群系 + 林线 +
    // 聚集噪声）给出，主题再用 `forest_density_gain` 调制疏密。
    let forest_density = forest_cache.get_or_compute(data);

    let material = ensure_material(&mut pools, &mut materials);

    // 视口矩形（视图空间）+ 外扩余量。
    let half = view.window_size * view.scale * 0.5 + Vec2::splat(VIEWPORT_MARGIN_PX * view.scale);
    let rect_min = view.camera_center - half;
    let rect_max = view.camera_center + half;

    // 栅格 → 视图：raster_origin + 格中心 × cell_size（与 terrain_chunks 同链）。
    let origin = grid.raster_origin;
    let cell = grid.cell_size;
    let px_to_wu = view.scale;

    // 步距（格）：由屏幕步距换算，至少 1。
    let stride = ((band.spacing_px(recipe) * view.scale) / cell)
        .ceil()
        .max(1.0) as i64;
    // 覆盖视口的格范围（钳到栅格）。
    let cx0 = (((rect_min.x - origin.x) / cell).floor() as i64 / stride) * stride;
    let cy0 = (((rect_min.y - origin.y) / cell).floor() as i64 / stride) * stride;
    let cx1 = (((rect_max.x - origin.x) / cell).ceil() as i64 + stride) / stride * stride;
    let cy1 = (((rect_max.y - origin.y) / cell).ceil() as i64 + stride) / stride * stride;

    // 八桶缓冲（位置 / 顶点色 / 索引）。
    let mut buckets: Vec<BucketBuf> = (0..8)
        .map(|_| (Vec::new(), Vec::new(), Vec::new()))
        .collect();
    let mut site_counts = [0usize; 8];

    let mut cy = cy0;
    while cy <= cy1 {
        let gy = cy;
        let mut cx = cx0;
        while cx <= cx1 {
            if cx < 0 || gy < 0 || cx >= data.width as i64 || gy >= data.height as i64 {
                cx += stride;
                continue;
            }
            let idx = (gy as u32) * data.width + cx as u32;
            // 缺失覆盖物标记（ADR-0045 追记一）：白底红斜线**接管**该区域 →
            // 装饰符号跳过（标记画在上面）。
            if overlay_index.is_missing_cell(idx) {
                cx += stride;
                continue;
            }
            // region 覆盖物规则（ADR-0045 D9：最深优先——索引已按深度降序归属）。
            let overlay_rule = overlay_index.rule_at(idx);
            let variant = overlay_rule.and_then(|r| decor_variant_for_key(r.variant));
            let size_scale = overlay_rule.map(|r| r.size_scale);
            // 脊点吸附（ADR-0044 D3）：Some((kind, 峰格x, 峰格y, 峰高))——
            // 山符号改放到块内峰格上，位置/高度用峰，哈希种子仍用网格点。
            let mut kind_override: Option<(DecorKind, i64, i64, i16)> = None;
            let Some(c) = data.cells.get(idx as usize) else {
                cx += stride;
                continue;
            };
            // 阈值抖动：部分格跳过，密度再降一档（森林 100%、平原 35%）。
            // 格 → 装饰类。水格（surface == 0，OCEAN/LAKE/RIVER）单独走
            // OceanWaves：浅海带（-220..0m）白弧，密度随深度衰减。
            //
            // 水陆判定与场管线底图**同源**（ADR-0072 T4 对齐）：权威边界是
            // `surface` 翻转处（coast_sdf T3 注记「authoring 真值，不由高度
            // 推断」）。曾经 `is_water_cell`（surface==0 ∨ height<0）——
            // 海面以下的陆上洼地被判成水，海浪白弧落在场管线画成陆地的
            // 格子上（2026-10-26 用户实机「陆地上多了很多白点点」）。
            let kind = if c.surface == 0 {
                // 离岸驱动（ADR-0048 追记四）：白弧只落在「泡沫带之外、墨线圈
                // 之内」的空隙带——按绝对深度判定会与新浅滩带 / 泡沫带在同一
                // 位置叠两套白色元素。深度不再参与（近岸浅滩的白浪花已由烘焙
                // 泡沫带承担）。
                let d = shore_field[gy as usize * data.width as usize + cx as usize];
                if d < recipe.wave_band_min_cells || d > recipe.wave_band_max_cells {
                    cx += stride;
                    continue;
                }
                DecorKind::OceanWaves
            } else {
                // ADR-0044 D3（civ-atlas 语法）：**山 = 符号，不是颜色**。
                // 生成端（wv-gen-demo）已不再把高地分类成 MOUNTAINS/SNOWFIELD
                // 色带，山形与雪帽改按**高度场**出图标——山链走向跟随
                // 脊线（含 warp 扭曲），笔直色带从根上不存在。
                // biome 显式声明仍优先（作者用笔刷手涂山地/丘陵时意图 wins，
                // 与高度规则取或——两条路互不干扰）。
                //
                // **脊点吸附**（D13 后高度带只有 2–13 格宽，步距网格点大多
                // 落在山外——直接按网格点采样会得到稀疏的孤峰）：在每个
                // 步距块内扫最高格，≥1500 就把符号**放到峰上**。效果 = 山链
                // 沿脊线连续成串（每步距一座），密度与脊线长度成正比。
                let by_biome = decor_kind_for_biome(c.biome);
                let mut peak = (cx, gy, c.height);
                if c.height < recipe.ridge_snap_min_h {
                    let r = (stride / 2).max(1);
                    let step = (stride / 8).max(1);
                    let mut bx = cx - r;
                    while bx <= cx + r {
                        let mut by = gy - r;
                        while by <= gy + r {
                            if bx >= 0
                                && by >= 0
                                && bx < data.width as i64
                                && by < data.height as i64
                            {
                                let o = (by as u32 * data.width + bx as u32) as usize;
                                let cell = &data.cells[o];
                                if !wv_terrain::edit::is_water_cell(wv_terrain::TerrainCell {
                                    height: cell.height,
                                    surface: cell.surface,
                                    biome: cell.biome,
                                }) && cell.height > peak.2
                                {
                                    peak = (bx, by, cell.height);
                                }
                            }
                            by += step;
                        }
                        bx += step;
                    }
                }
                let (site_cx, site_cy, site_h) = peak;
                if site_h >= recipe.mountain_min_h || by_biome == Some(DecorKind::Mountain) {
                    // 覆盖本块的符号位置/高度：峰格（哈希种子仍用网格点，确定性不变）。
                    kind_override = Some((DecorKind::Mountain, site_cx, site_cy, site_h));
                    DecorKind::Mountain
                } else if c.height >= recipe.hill_min_h || by_biome == Some(DecorKind::Hills) {
                    DecorKind::Hills
                } else {
                    match by_biome {
                        Some(k) => k,
                        None => {
                            cx += stride;
                            continue;
                        }
                    }
                }
            };
            let keep_prob = match kind {
                // 森林桶（方案 1）：保留率 = 密度场 × 主题增益。
                // 密度场已经含群系基础密度、林线衰减与聚集噪声，故此处
                // 不再是「有森林群系就画满」——林间空地自然出现。
                DecorKind::Forest => {
                    let d = forest_density[gy as usize * data.width as usize + cx as usize];
                    forest_keep_prob(d, recipe.forest_density_gain)
                }
                DecorKind::Mountain | DecorKind::Snow | DecorKind::Marsh => 1.0,
                DecorKind::Desert => 0.7,
                DecorKind::OceanWaves => {
                    // 密度随离岸距离线性衰减：带内缘全画，外缘稀至 0.15。
                    let d = shore_field[gy as usize * data.width as usize + cx as usize];
                    let span = recipe.wave_density_span_cells.max(1) as f32;
                    let t = (d - recipe.wave_band_min_cells) as f32 / span;
                    (1.0 - t).clamp(0.15, 1.0)
                }
                DecorKind::Plains | DecorKind::Hills => 0.4,
            };
            // 覆盖物规则的间距倍率（> 1 = 更疏；固定步距下"更密"不可达 → 夹到 1）。
            let keep_prob = match overlay_rule {
                Some(r) if r.spacing_scale > 1.0 => keep_prob / r.spacing_scale,
                _ => keep_prob,
            };
            // 脊点吸附命中：山符号的位置/高度切换到峰格。
            let (px, py, ph) = if let Some((_, sx, sy, sh)) = kind_override {
                (sx, sy, sh)
            } else {
                (cx, gy, c.height)
            };
            if hash01(cx, gy, 77) > keep_prob {
                cx += stride;
                continue;
            }
            let bucket_idx = kind as usize;
            if site_counts[bucket_idx] >= MAX_SITES_PER_KIND {
                cx += stride;
                continue;
            }
            site_counts[bucket_idx] += 1;
            // 站点级大抖动（ADR-0042 追记 D13）：旧实现 `wx/wy` 恒等于步距
            // 网格的格心，符号排成一张完美的棋盘——zoom out 一眼看出「整齐」
            //（用户实机反馈）。按 ±0.45 步距抖动，把规则网格揉成自然散布。
            let step_world = stride as f32 * cell;
            // 吸附命中时抖动幅度收半（峰位本身就是定位，大幅抖动会掉下脊线）。
            let jmul = if kind_override.is_some() { 0.25 } else { 0.9 };
            let jx = (hash01(cx, gy, 0x51) - 0.5) * step_world * jmul;
            let jy = (hash01(cx, gy, 0x52) - 0.5) * step_world * jmul;
            let wx = origin.x + (px as f32 + 0.5) * cell + jx;
            let wy = origin.y + (py as f32 + 0.5) * cell + jy;
            // 气候带（ADR-0039 D3）：归一化 y（0=北）。
            let fy = gy as f32 / data.height as f32;
            // 图元占位色 → 桶默认 ink（构造器 [0;4] 时回填；变体色原样透传）。
            let (ink_rgb, ink_a) = kind.ink(recipe);
            let ink = [
                ink_rgb[0] as f32 / 255.0,
                ink_rgb[1] as f32 / 255.0,
                ink_rgb[2] as f32 / 255.0,
                ink_a,
            ];
            let ctx = SiteCtx {
                kind,
                band,
                cx,
                cy: gy,
                height_m: ph,
                fy,
                variant,
                recipe,
            };
            for mut prim in ctx.primitives() {
                if prim.color == [0.0, 0.0, 0.0, 0.0] {
                    prim.color = ink;
                }
                // 覆盖物规则的颜色（主题拥有）覆盖变体色 / 桶默认墨色。
                if let Some(rule) = overlay_rule {
                    prim.color = [
                        rule.color[0] as f32 / 255.0,
                        rule.color[1] as f32 / 255.0,
                        rule.color[2] as f32 / 255.0,
                        rule.alpha.unwrap_or(prim.color[3]),
                    ];
                }
                // 尺寸倍率（主题规则）。
                if let Some(k) = size_scale {
                    prim.half_w *= k;
                    prim.half_h *= k;
                }
                let b = prim.bucket as usize;
                let (bv, bc, bi) = &mut buckets[b];
                push_site_tris(&prim, wx, wy, px_to_wu, bv, bc, bi);
            }
            cx += stride;
        }
        cy += stride;
    }
    // 同步八桶。
    let size_q = band.spacing_px(recipe) * view.scale * band.size_mul(recipe);
    for (i, (verts, colors, indices)) in buckets.iter_mut().enumerate() {
        let geom = DecorGeom {
            verts: core::mem::take(verts),
            colors: core::mem::take(colors),
            indices: core::mem::take(indices),
            size: size_q,
            z: DECOR_Z + i as f32 * BUCKET_Z_STEP,
        };
        sync_decor_bucket(
            &mut commands,
            &mut meshes,
            geom,
            &material,
            &mut pools.pools[i],
        );
    }
}

/// 桶缓冲：位置 + 顶点色 + 索引。
type BucketBuf = (Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>);

/// 一帧的桶几何（顶点 / 索引 + 重建判据的尺寸 / Z 分量）。
struct DecorGeom {
    verts: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    size: f32,
    z: f32,
}

/// 单桶合批同步：指纹 / 尺寸 / Z 变化才原地改写顶点。
fn sync_decor_bucket(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    geom: DecorGeom,
    material: &Handle<ColorMaterial>,
    pool: &mut DecorBatchPool,
) {
    let DecorGeom {
        verts,
        colors,
        indices,
        size,
        z,
    } = geom;
    let fingerprint = points_fingerprint(&verts);
    let key = DecorBatchKey {
        fingerprint,
        size,
        z,
    };
    match pool.batch.take() {
        None => {
            if verts.is_empty() {
                return;
            }
            let mesh = meshes.add(build_mesh(verts, colors, indices));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0.0, 0.0, key.z),
                ))
                .id();
            pool.batch = Some(DecorBatch { entity, mesh, key });
        }
        Some(mut batch) => {
            if verts.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            if batch.key != key {
                if let Some(mut m) = meshes.get_mut(&batch.mesh) {
                    *m = build_mesh(verts, colors, indices);
                }
                if batch.key.z != key.z {
                    commands
                        .entity(batch.entity)
                        .insert(Transform::from_xyz(0.0, 0.0, key.z));
                }
                batch.key = key;
            }
            commands
                .entity(batch.entity)
                .insert(MeshMaterial2d(material.clone()));
            pool.batch = Some(batch);
        }
    }
}

/// 顶点指纹（FNV-1a over f32 bits）——可见集或符号尺寸变化即换 key。
fn points_fingerprint(verts: &[[f32; 3]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for v in verts {
        for c in v {
            hash ^= u64::from(c.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn build_mesh(verts: Vec<[f32; 3]>, colors: Vec<[f32; 4]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        verts,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::biome::{BIOME_DESERT, BIOME_GOBI, BIOME_OCEAN, BIOME_SALT_FLAT};

    #[test]
    fn decor_kind_maps_forest_and_rejects_water() {
        assert_eq!(decor_kind_for_biome(BIOME_FOREST), Some(DecorKind::Forest));
        assert_eq!(decor_kind_for_biome(BIOME_HILLS), Some(DecorKind::Hills));
        assert_eq!(
            decor_kind_for_biome(BIOME_MOUNTAINS),
            Some(DecorKind::Mountain)
        );
        assert_eq!(decor_kind_for_biome(BIOME_SNOWFIELD), Some(DecorKind::Snow));
        assert_eq!(decor_kind_for_biome(BIOME_SWAMP), Some(DecorKind::Marsh));
        assert_eq!(
            decor_kind_for_biome(BIOME_GRASSLAND),
            Some(DecorKind::Plains)
        );
        assert_eq!(decor_kind_for_biome(BIOME_OCEAN), None, "海面不画装饰");
        assert_eq!(
            decor_kind_for_biome(BIOME_DESERT),
            Some(DecorKind::Desert),
            "沙漠画沙丘脊"
        );
        assert_eq!(decor_kind_for_biome(BIOME_GOBI), Some(DecorKind::Desert));
        assert_eq!(decor_kind_for_biome(BIOME_SALT_FLAT), None, "盐滩保持纯色");
    }

    /// 测试用配方 = 日间水彩（ADR-0048 追记二：抽取前的渲染层硬编码值）。
    fn recipe() -> DecorRecipe {
        crate::theme::decor::WATERCOLOR
    }

    #[test]
    fn all_three_bands_draw_and_nation_is_coarsest() {
        assert_eq!(decor_band(5.0), Some(DecorBand::Nation));
        assert_eq!(decor_band(2.0), Some(DecorBand::Region));
        assert_eq!(decor_band(0.5), Some(DecorBand::Edit));
        assert!(
            DecorBand::Nation.spacing_px(&recipe()) > DecorBand::Region.spacing_px(&recipe()),
            "Nation 档步距必须最粗"
        );
        assert!(
            DecorBand::Nation.size_mul(&recipe()) > DecorBand::Edit.size_mul(&recipe()),
            "Nation 档符号必须更大"
        );
    }

    #[test]
    fn region_band_spacing_is_coarser_than_edit() {
        assert!(DecorBand::Region.spacing_px(&recipe()) > DecorBand::Edit.spacing_px(&recipe()));
        assert!(DecorBand::Region.size_mul(&recipe()) < DecorBand::Edit.size_mul(&recipe()));
    }

    /// 测试便捷构造：把 8 个位置参数收成一个入口，最小化调用点噪声。
    /// 生产路径走 [`SiteCtx`] 结构体字面量（无 `too_many_arguments`）；
    /// 此处保留位置参数仅为测试可读性，属**测试专用的可辩护例外**。
    #[allow(clippy::too_many_arguments)]
    fn prims_at(
        kind: DecorKind,
        band: DecorBand,
        cx: i64,
        cy: i64,
        height_m: i16,
        fy: f32,
        variant: Option<DecorVariant>,
        recipe: &DecorRecipe,
    ) -> Vec<SitePrim> {
        SiteCtx {
            kind,
            band,
            cx,
            cy,
            height_m,
            fy,
            variant,
            recipe,
        }
        .primitives()
    }

    #[test]
    fn site_primitives_are_deterministic() {
        let a = prims_at(
            DecorKind::Forest,
            DecorBand::Edit,
            100,
            200,
            500,
            0.5,
            None,
            &recipe(),
        );
        let b = prims_at(
            DecorKind::Forest,
            DecorBand::Edit,
            100,
            200,
            500,
            0.5,
            None,
            &recipe(),
        );
        assert_eq!(a, b, "同输入必须同符号（确定性）");
        let c = prims_at(
            DecorKind::Forest,
            DecorBand::Edit,
            101,
            200,
            500,
            0.5,
            None,
            &recipe(),
        );
        assert_ne!(a, c, "不同格应有不同抖动");
    }

    #[test]
    fn mountains_above_snow_line_get_a_snow_cap_prim() {
        let high = prims_at(
            DecorKind::Mountain,
            DecorBand::Edit,
            5,
            5,
            2200,
            0.5,
            None,
            &recipe(),
        );
        assert!(
            high.iter().any(|p| p.bucket == DecorKind::Snow),
            "2200m 山应有雪帽图元"
        );
        let low = prims_at(
            DecorKind::Mountain,
            DecorBand::Edit,
            5,
            5,
            800,
            0.5,
            None,
            &recipe(),
        );
        assert!(
            low.iter().all(|p| p.bucket != DecorKind::Snow),
            "800m 山不应有雪帽"
        );
    }

    #[test]
    fn ocean_waves_spawn_in_shallow_band_only() {
        // ADR-0039 D2：-10m（浅海）有波浪图元；陆地/深海没有。
        let shallow = prims_at(
            DecorKind::OceanWaves,
            DecorBand::Edit,
            10,
            10,
            -10,
            0.5,
            None,
            &recipe(),
        );
        assert!(
            shallow.iter().all(|p| p.bucket == DecorKind::OceanWaves),
            "浅海波浪应落在 OceanWaves 桶"
        );
        assert!(!shallow.is_empty(), "浅海应有白波图元");
        // 波浪色 = 白（顶点色）
        assert!(
            shallow.iter().all(|p| p.color[0] > 0.9 && p.color[1] > 0.9),
            "白波应为白色系，得 {:?}",
            shallow[0].color
        );
    }

    #[test]
    fn forest_climate_variants_differ_by_fy() {
        // ADR-0039 D3：针叶（北）= 细三角；阔叶（中）= 圆团；绿洲（南）= 孤团。
        let boreal = prims_at(
            DecorKind::Forest,
            DecorBand::Edit,
            10,
            10,
            300,
            0.15,
            None,
            &recipe(),
        );
        let temperate = prims_at(
            DecorKind::Forest,
            DecorBand::Edit,
            10,
            10,
            300,
            0.5,
            None,
            &recipe(),
        );
        let oasis = prims_at(
            DecorKind::Forest,
            DecorBand::Edit,
            10,
            10,
            300,
            0.85,
            None,
            &recipe(),
        );
        assert!(
            boreal.iter().all(|p| p.kind == PrimShape::Triangle),
            "针叶应为细三角"
        );
        assert!(
            boreal.iter().all(|p| p.color[0] < 0.25 && p.color[1] > 0.3),
            "针叶应深绿"
        );
        assert!(
            temperate.iter().all(|p| p.kind == PrimShape::Circle),
            "阔叶应为圆团"
        );
        assert_eq!(oasis.len(), 1, "绿洲稀树应只有 1 颗");
        assert!(
            oasis.iter().all(|p| p.color[0] > 0.25),
            "绿洲应暖绿（R 分量更高）"
        );
    }

    #[test]
    fn alpine_peaks_get_rock_accent() {
        // ADR-0039 D4：>=3400m 雪帽 + 帽缘露岩（Snow 桶第二图元，黄褐）。
        let peak = prims_at(
            DecorKind::Mountain,
            DecorBand::Edit,
            5,
            5,
            3500,
            0.5,
            None,
            &recipe(),
        );
        let rocks: Vec<_> = peak
            .iter()
            .filter(|p| p.bucket == DecorKind::Snow && p.color[0] > 0.7 && p.color[2] < 0.7)
            .collect();
        assert_eq!(rocks.len(), 1, "3500m 峰应有 1 块露岩，得 {}", rocks.len());
        let low = prims_at(
            DecorKind::Mountain,
            DecorBand::Edit,
            5,
            5,
            2000,
            0.5,
            None,
            &recipe(),
        );
        assert!(
            low.iter()
                .all(|p| !(p.bucket == DecorKind::Snow && p.color[0] > 0.7 && p.color[2] < 0.7),),
            "2000m 山不应有露岩"
        );
    }

    /// ADR-0059 D3：`mountain_style: None` 时山站点不产任何图元——
    /// 水墨包的画法：山体表达收敛到等值线重墨 + 海岸顿笔，「不画」是
    /// 正常画法而非异常态（连雪帽 / 露岩一并让位）。
    #[test]
    fn mountain_style_none_yields_no_mountain_primitives() {
        let mut r = recipe();
        r.mountain_style = SymbolStyle::None;
        let prims = prims_at(
            DecorKind::Mountain,
            DecorBand::Edit,
            5,
            5,
            3500,
            0.5,
            None,
            &r,
        );
        assert!(
            prims.is_empty(),
            "None 画法应不产任何图元（含雪帽/露岩），得 {} 个",
            prims.len()
        );
    }

    /// ADR-0059 D3：`Tile` 画法 v1 一次性 warn 后回退 `Default`——
    /// 装载通道待 v2（ADR-0023 M3），分派本身不 panic 且保底画默认山符号。
    #[test]
    fn mountain_style_tile_falls_back_to_default_without_panic() {
        let mut r = recipe();
        r.mountain_style = SymbolStyle::Tile("terrain/mountain_tile.png");
        let prims = prims_at(
            DecorKind::Mountain,
            DecorBand::Edit,
            5,
            5,
            2200,
            0.5,
            None,
            &r,
        );
        assert!(!prims.is_empty(), "Tile 回退后应仍画默认山符号");
        assert!(
            prims.iter().any(|p| p.bucket == DecorKind::Snow),
            "2200m 回退路径应仍叠雪帽（走 Default 逻辑）"
        );
    }

    #[test]
    fn push_site_tris_circle_yields_fan_topology() {
        let prim = SitePrim::circle(DecorKind::Forest, 0.0, 0.0, 3.0);
        let mut verts = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        push_site_tris(&prim, 0.0, 0.0, 1.0, &mut verts, &mut colors, &mut indices);
        assert_eq!(verts.len(), POI_LIKE_SEGMENTS + 1, "圆心 + 12 边");
        assert_eq!(indices.len(), POI_LIKE_SEGMENTS * 3, "12 扇区");
    }

    #[test]
    fn push_site_tris_half_disc_and_triangle_shapes() {
        let half = SitePrim::half_disc(DecorKind::Hills, 0.0, 0.0, 4.0, 5.0);
        let mut verts = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        push_site_tris(&half, 0.0, 0.0, 1.0, &mut verts, &mut colors, &mut indices);
        assert_eq!(verts.len(), 2 + POI_LIKE_SEGMENTS + 1);

        let tri = SitePrim::triangle(DecorKind::Mountain, 0.0, 0.0, 6.0, 11.0);
        let mut v2 = Vec::new();
        let mut c2 = Vec::new();
        let mut i2 = Vec::new();
        push_site_tris(&tri, 0.0, 0.0, 1.0, &mut v2, &mut c2, &mut i2);
        assert_eq!(v2.len(), 3);
        assert_eq!(i2.len(), 3);
    }

    #[test]
    fn points_fingerprint_changes_with_content() {
        let a = vec![[0.0, 0.0, 0.0]];
        let b = vec![[1.0, 0.0, 0.0]];
        assert_ne!(points_fingerprint(&a), points_fingerprint(&b));
        assert_eq!(points_fingerprint(&a), points_fingerprint(&a));
    }

    /// 系统级集成：Edit 档生成装饰桶 mesh（森林 64² 全林网格）、二帧资产稳定、
    /// 拉到 Nation 档保留粗步距地貌团（ADR-0036 取代 ADR-0031 D2）、
    /// 关掉 pass 才整组下线。
    #[test]
    fn decor_system_spawns_at_edit_keeps_nation_band_and_clears_when_disabled() {
        use crate::layer_frame::LayerFrame;
        use crate::recipe::{PassId, PassPlan, RenderRecipe};
        use crate::state::DrillState;
        use bevy::ecs::schedule::Schedule;
        use wv_core::terrain::{TerrainCellData, TerrainData};

        let mut world = wv_core::World::default();
        let map_id = wv_core::Id::new_unchecked("map_t");
        world.maps.push(wv_core::map::MapNode {
            id: map_id.clone(),
            ..Default::default()
        });
        // 64² 全林格（height 500 → 无雪帽）。
        let n = 64u32;
        let cells = vec![
            TerrainCellData {
                height: 500,
                surface: 1,
                biome: BIOME_FOREST,
            };
            (n * n) as usize
        ];
        world.terrains.insert(
            map_id.clone(),
            TerrainData {
                width: n,
                height: n,
                cells,
            },
        );

        let mut app = World::default();
        app.insert_resource(ViewState {
            scale: 0.5, // Edit 档
            camera_center: Vec2::splat(n as f32 * 4.0 / 2.0),
            window_size: Vec2::new(1280.0, 800.0),
            ..Default::default()
        });
        app.insert_resource(GridConfig {
            cell_size: 4.0,
            ..Default::default()
        });
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        app.insert_resource(drill);
        app.insert_resource(WorldProjection {
            world,
            ..Default::default()
        });
        app.insert_resource(LayerFrame::default());
        let mut recipe = RenderRecipe::default();
        recipe.passes[PassId::TerrainDecor as usize] = PassPlan::enabled();
        app.insert_resource(recipe);
        app.init_resource::<TerrainDecorPools>();
        app.init_resource::<ShoreFieldCache>();
        // 森林密度场缓存（方案 1）：森林桶的保留率由它调制，漏初始化会 panic。
        app.init_resource::<ForestDensityCache>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();
        // 覆盖物规则索引（ADR-0045 D9）：sync_terrain_decor 的必选依赖，
        // 测试装配漏初始化会让系统参数校验 panic（b43b24e 接线后的缺口）。
        app.init_resource::<crate::region_overlay::RegionOverlayIndex>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_terrain_decor);

        schedule.run(&mut app);
        let meshes = app.resource::<Assets<Mesh>>();
        let first = meshes.iter().count();
        // **ADR-0066**：专业制图风格不画林团（`DecorRecipe::PROFESSIONAL` 的
        // `forest_style = None`）——绿地由群系色块表达，满地树图标是手绘语汇。
        // 故全林地形**不产任何 mesh**，这是新契约而非回归。
        assert_eq!(
            first, 0,
            "专业制图不画林团（forest_style = None），全林应产 0 个 mesh，got {first}"
        );

        // 二帧：无变化 → 零新增资产（原地改写纪律）。
        schedule.run(&mut app);
        let second = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(second, first, "第二帧不应新增 mesh");

        // Nation 档：同样不画（风格恒定，与档位无关）。
        app.resource_mut::<ViewState>().scale = 5.0;
        schedule.run(&mut app);
        let nation = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(nation, 0, "Nation 档同样不画林团，got {nation}");

        // 关掉 TerrainDecor pass：整组下线。
        let mut recipe = app.resource_mut::<RenderRecipe>();
        recipe.passes[PassId::TerrainDecor as usize] = PassPlan::disabled();
        schedule.run(&mut app);
        let off = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(off, 0, "pass 关闭后装饰应全部清除，got {off}");
    }

    /// 森林密度场：森林群系得正密度，荒漠得零（方案 1 的数据源断言）。
    #[test]
    fn forest_density_cache_marks_forest_and_rejects_desert() {
        use wv_core::terrain::{TerrainCellData, TerrainData};

        // 2×2：左上 = 森林（100 m），其余 = 荒漠。
        let cells = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: BIOME_FOREST,
            },
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: BIOME_DESERT,
            },
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: BIOME_DESERT,
            },
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: BIOME_DESERT,
            },
        ];
        let data = TerrainData {
            width: 2,
            height: 2,
            cells,
        };
        let mut cache = ForestDensityCache::default();
        let field = cache.get_or_compute(&data);
        assert_eq!(field.len(), 4, "密度场长度应为 4");
        assert!(field[0] > 0.0, "森林格应有正密度，got {}", field[0]);
        assert_eq!(field[1], 0.0, "荒漠格应无密度，got {}", field[1]);
        assert_eq!(field[2], 0.0, "荒漠格应无密度，got {}", field[2]);
        assert_eq!(field[3], 0.0, "荒漠格应无密度，got {}", field[3]);
    }

    /// 主题增益：0 = 不画森林符号，1 = 照密度场，2 = 提到满格。
    #[test]
    fn forest_keep_prob_applies_theme_gain() {
        assert_eq!(forest_keep_prob(0.8, 0.0), 0.0, "增益 0 应无符号");
        assert!(
            (forest_keep_prob(0.8, 1.0) - 0.8).abs() < 1e-6,
            "增益 1 照密度场"
        );
        assert_eq!(forest_keep_prob(0.8, 2.0), 1.0, "增益 2 应夹到满格");
        assert_eq!(forest_keep_prob(0.8, -1.0), 0.0, "负增益应夹到 0");
    }
}
