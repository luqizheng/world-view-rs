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
//! - **不引入位图瓦片**——底图仍由 `terrain_view::cells` 逐格程序化合成
//!   （JRPG 瓦片图集已弃用）。**地物符号**可走主题贴图（ADR-0050）：
//!   森林族由 `DecorRecipe::forest_art` 给出手绘树图集，缺资产回退本文件的
//!   程序化图元（不空帧）。「底图瓦片」与「地物符号」是两回事，见 ADR-0050；
//! - 符号颜色 / 步距 / 尺寸由主题配方（[`DecorRecipe`]）给定，渲染层只做执行器。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};
use std::collections::{HashMap, HashSet};

use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{lod_tier, GridConfig, ViewState, WorldProjection};
use crate::terrain_view::terrain_resolve::active_terrain;
use crate::theme::decor::DecorRecipe;
use wv_terrain::biome::{
    BIOME_DESERT, BIOME_FOREST, BIOME_GOBI, BIOME_GRASSLAND, BIOME_HILLS, BIOME_JUNGLE,
    BIOME_MOUNTAINS, BIOME_OASIS, BIOME_PLAINS, BIOME_SAVANNA, BIOME_SNOWFIELD, BIOME_SWAMP,
    BIOME_TUNDRA, BIOME_WETLAND,
};

/// 装饰 Z：地形族之上（等值线 0.04 / 雾霭 0.06 / 地界符号 0.07）、道路（0.09）之下。
const DECOR_Z: f32 = 0.075;
/// 桶间 Z 微差（六桶按序号错开，防同深度闪烁）。
const BUCKET_Z_STEP: f32 = 0.0004;
// 采样步距 / 尺寸系数已迁入主题配方（`wv_theme::decor::DecorRecipe`，
// ADR-0048 追记二）——渲染层不再持有美术常量。
/// 单类单帧符号组数上限（步距本身已把数量压到屏幕量级，此为兜底）。
const MAX_SITES_PER_KIND: usize = 2200;
/// 视口外扩余量（屏幕像素）：拖动相机时边缘符号不闪断。
const VIEWPORT_MARGIN_PX: f32 = 32.0;
/// 贴图符号的屏幕边长（px）——**按镜头档给绝对值，不与采样步距成比例**。
///
/// 教训（2026-10-21 实机截图）：早期取 `0.9 × 采样步距`，Nation 档单棵树
/// 65 px，比地名（12 px）大 5 倍，整片森林像一排巨型盆栽。树是**地表纹理**
/// 不是地标，必须比地名小一档；放大档位时只允许缓慢增长，不许跟着步距翻倍。
const TEXTURE_SIZE_PX_EDIT: f32 = 14.0;
/// Region 档贴图边长（px）。
const TEXTURE_SIZE_PX_REGION: f32 = 20.0;
/// Nation 档贴图边长（px）。
const TEXTURE_SIZE_PX_NATION: f32 = 24.0;

/// 编译期嵌入的装饰贴图（ADR-0050）。
///
/// 与 `feature_overlays::embedded_texture` 同一手法（`include_bytes!`，不走
/// `AssetServer`）：路径是主题配方给的相对路径，未知键返回 `None`，渲染侧回退
/// 程序化图元——缺资产不 panic、不空帧。
const EMBEDDED_DECOR: &[(&str, &[u8])] = &[
    (
        "overlays/forest/decor_atlas_day.png",
        include_bytes!("../assets/overlays/forest/decor_atlas_day.png"),
    ),
    (
        "overlays/forest/decor_atlas_dusk.png",
        include_bytes!("../assets/overlays/forest/decor_atlas_dusk.png"),
    ),
];

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

/// 画一棵 **civ-atlas 式**树：描边层 + 填充层 + 树干（ADR-0050 追记三）。
///
/// `(dx, dy)` 是**树根**在站点局部坐标里的偏移；`s` 是像素缩放（已含镜头档
/// 尺寸系数与每棵抖动）。描边 = 同形状放大半个像素、色值 ×0.38；填充 =
/// 族墨色提亮（阔叶浅绿 / 针叶中绿）。**先描边后填充**，重叠处只剩外轮廓——
/// 这是「平涂 + 硬边描线」能像手绘地图而不是贴纸的关键。
#[allow(clippy::too_many_arguments)]
fn push_forest_glyph(
    out: &mut Vec<SitePrim>,
    kind: DecorKind,
    variant: DecorVariant,
    dx: f32,
    dy: f32,
    s: f32,
    boreal: [f32; 4],
    temperate: [f32; 4],
    oasis: [f32; 4],
    trunk: [f32; 4],
) {
    let outline = |c: [f32; 4]| [c[0] * 0.38, c[1] * 0.38, c[2] * 0.38, 1.0];
    let canopy = |c: [f32; 4]| {
        [
            (c[0] * 1.15 + 0.22).min(1.0),
            (c[1] * 1.15 + 0.22).min(1.0),
            (c[2] * 1.15 + 0.22).min(1.0),
            1.0,
        ]
    };
    let ground = |out: &mut Vec<SitePrim>, h: f32| {
        out.push(SitePrim::colored(
            kind,
            dx,
            dy + h * 0.5,
            0.55 * s,
            h * 0.5,
            PrimShape::Triangle,
            trunk,
        ));
    };

    match variant {
        DecorVariant::Conifer => {
            // 针叶：三层塔形（下宽上窄），中绿填充 + 更深描边。
            ground(out, 2.2 * s);
            let rim = outline(boreal);
            let fill = [
                (boreal[0] * 1.35 + 0.12).min(1.0),
                (boreal[1] * 1.35 + 0.12).min(1.0),
                (boreal[2] * 1.35 + 0.12).min(1.0),
                1.0,
            ];
            let tiers = [(3.6, 5.4, 2.0), (2.9, 4.4, 4.6), (2.2, 3.4, 7.0)];
            for (w, h, y0) in tiers {
                out.push(SitePrim::colored(
                    kind,
                    dx,
                    dy + (y0 + h * 0.5) * s,
                    (w + 0.6) * s,
                    (h * 0.5 + 0.35) * s,
                    PrimShape::Triangle,
                    rim,
                ));
            }
            for (w, h, y0) in tiers {
                out.push(SitePrim::colored(
                    kind,
                    dx,
                    dy + (y0 + h * 0.5) * s,
                    (w - 0.12) * s,
                    (h * 0.5 - 0.1) * s,
                    PrimShape::Triangle,
                    fill,
                ));
            }
        }
        DecorVariant::Sparse => {
            // 稀树：两团稀疏冠 + 细干（暖绿，树冠更小更透）。
            ground(out, 2.2 * s);
            let rim = outline(oasis);
            let fill = canopy(oasis);
            let r = 4.6 * s;
            let cy = dy + 2.2 * s + r * 1.0;
            let lobes = [
                (dx - 0.9 * r, cy, 0.82 * r),
                (dx + 0.75 * r, cy - 0.2 * r, 0.66 * r),
            ];
            for (x, y, rr) in lobes {
                out.push(SitePrim::colored(
                    kind,
                    x,
                    y,
                    rr + 0.55 * s,
                    rr + 0.55 * s,
                    PrimShape::Circle,
                    rim,
                ));
            }
            for (x, y, rr) in lobes {
                out.push(SitePrim::colored(
                    kind,
                    x,
                    y,
                    rr,
                    rr,
                    PrimShape::Circle,
                    fill,
                ));
            }
        }
        // 阔叶（含 boulder 兜底，后者在函数开头已分流）
        _ => {
            // 阔叶：四团扇贝冠（主团 + 左右 + 下缘）+ 短干，浅绿填充 + 深描边。
            ground(out, 2.4 * s);
            let rim = outline(temperate);
            let fill = canopy(temperate);
            let r = 5.0 * s;
            let cy = dy + 2.4 * s + r * 1.05;
            let lobes = [
                (0.0, 0.0, r),
                (-0.80 * r, -0.12 * r, 0.66 * r),
                (0.82 * r, -0.06 * r, 0.64 * r),
                (0.04 * r, 0.58 * r, 0.58 * r),
            ];
            for (ox, oy, rr) in lobes {
                out.push(SitePrim::colored(
                    kind,
                    dx + ox,
                    cy + oy,
                    rr + 0.55 * s,
                    rr + 0.55 * s,
                    PrimShape::Circle,
                    rim,
                ));
            }
            for (ox, oy, rr) in lobes {
                out.push(SitePrim::colored(
                    kind,
                    dx + ox,
                    cy + oy,
                    rr,
                    rr,
                    PrimShape::Circle,
                    fill,
                ));
            }
        }
    }
}

/// 森林族的变体判定（气候带 + region 覆盖物强制）——程序化与贴图两条路径共用。
///
/// 抽成公开纯函数是为了让两条路径**不可能**对同一格给出不同变体：
/// 一旦分叉，同一棵针叶会用阔叶格画，或反之。
pub fn forest_variant(fy: f32, forced: Option<DecorVariant>, recipe: &DecorRecipe) -> DecorVariant {
    match forced {
        Some(v @ (DecorVariant::Conifer | DecorVariant::Broadleaf | DecorVariant::Sparse)) => v,
        _ => {
            if fy < recipe.climate_cuts[0] {
                DecorVariant::Conifer
            } else if fy < recipe.climate_cuts[1] {
                DecorVariant::Broadleaf
            } else {
                DecorVariant::Sparse
            }
        }
    }
}

/// 变体 → 图集格号。**格序契约**：针叶 0 / 阔叶 1 / 稀树 2（`DecorArt` 文档）。
///
/// `boulder` 不是森林族的变体（乱石滩涂），落回阔叶格；`cells` 小于 3 时钳到
/// 最后一格——宁可复用一格，也不越界取到相邻格。
pub fn forest_art_cell(variant: DecorVariant, cells: u8) -> u32 {
    if cells == 0 {
        return 0;
    }
    let cell = match variant {
        DecorVariant::Conifer => 0,
        DecorVariant::Broadleaf => 1,
        DecorVariant::Sparse => 2,
        DecorVariant::Boulder => 1,
    };
    cell.min(cells as u32 - 1)
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

    /// 贴图符号的屏幕边长（px，尚未乘 `view.scale`）。
    ///
    /// 与 [`Self::spacing_px`] **解耦**：步距可随配方变大，树不能跟着一起长
    /// ——否则 Nation 档会出现「一棵树比五个地名还大」（2026-10-21 实机截图）。
    fn texture_size_px(self) -> f32 {
        match self {
            DecorBand::Nation => TEXTURE_SIZE_PX_NATION,
            DecorBand::Region => TEXTURE_SIZE_PX_REGION,
            DecorBand::Edit => TEXTURE_SIZE_PX_EDIT,
        }
    }
}

/// splitmix64（与 `theme::overlays` 同族）：落点抖动 / 变体选择的确定性来源。
fn splitmix64(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn hash01(x: i64, y: i64, seed: u64) -> f32 {
    let h = splitmix64(
        (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ seed,
    );
    (h >> 11) as f32 / (1u64 << 53) as f32
}

/// 一个采样点的装饰符号集合：`(|dx|, |dy|, r|半宽, 半高|, 桶)` 三角形列表。
///
/// 坐标是**相对格中心的屏幕像素**（调用方乘 `view.scale` 转世界单位）。
/// 输出 `(dx, dy, half_w, half_h)` 四元组——都是轴对齐缩放后的三角形顶点
/// 由 [`build_bucket_mesh`] 统一三角化；这里只产出**符号图元参数**。
///
/// v1 用「相对偏移 + 半径」的圆 / 半圆 / 三角参数化，避免在每个采样点手写
/// 顶点循环：三角化集中在 mesh 构建一处（改形状只动一处）。
#[allow(clippy::too_many_arguments)] // 符号参数（kind/位置/高度/气候带/变体）+ 主题配方
pub fn site_primitives(
    kind: DecorKind,
    band: DecorBand,
    cx: i64,
    cy: i64,
    height_m: i16,
    fy: f32,
    variant: Option<DecorVariant>,
    recipe: &DecorRecipe,
) -> Vec<SitePrim> {
    let mut out = Vec::new();
    let m = band.size_mul(recipe);
    // 乱石滩涂：与族无关的专用形状（region 覆盖物键 `boulder_field`）。
    if variant == Some(DecorVariant::Boulder) {
        for k in 0..4u64 {
            let jx = (hash01(cx, cy, 30 + k) - 0.5) * 9.0;
            let jy = (hash01(cx, cy, 40 + k) - 0.5) * 8.0;
            out.push(SitePrim::colored(
                kind,
                jx * m,
                jy * m,
                1.7 * m,
                2.0 * m,
                PrimShape::Triangle,
                [0.54, 0.52, 0.46, 0.85],
            ));
        }
        return out;
    }
    let r1 = hash01(cx, cy, 1);
    let r2 = hash01(cx, cy, 2);
    let r3 = hash01(cx, cy, 3);
    match kind {
        DecorKind::Forest => {
            // 气候变体（ADR-0039 D3）：沿 y 轴（0=北）三档。
            //
            // **画法照 civ-atlas（ADR-0050 追记三）**：符号 = 平涂色块 + 深色
            // 描边，不是写实贴图。每棵树 = 描边层（整组先画一遍压暗轮廓）+
            // 填充层（扇贝状树冠）+ 树干；颜色全部由主题配方的族墨色派生，
            // 因此与底图同一色族。远景下多棵叠成「暗色林斑」，拉近才看出单棵。
            let boreal = recipe.conifer_ink.to_rgba();
            let temperate = recipe.temperate_ink.to_rgba();
            let oasis = recipe.oasis_ink.to_rgba();
            // 树干取山墨（暖褐灰）压暗。
            let trunk = {
                let c = recipe.mountain_ink.color;
                [
                    c[0] as f32 / 255.0 * 0.62,
                    c[1] as f32 / 255.0 * 0.62,
                    c[2] as f32 / 255.0 * 0.62,
                    0.96,
                ]
            };
            // **region 覆盖物可强制变体**（ADR-0045 D2/D9）：作者声明「这片用
            // 针叶画」→ 变体由声明决定，**优先于** y 轴气候带派生。
            let variant = forest_variant(fy, variant, recipe);
            // 每棵树的尺寸抖动 ±10%（同格重开不变，跨格不同）。
            let k = 0.90 + 0.20 * r3;
            let s = m * k;
            // 第二棵（更小、偏左上）破重复感：约一半站点补一棵。
            if r1 > 0.45 {
                push_forest_glyph(
                    &mut out,
                    kind,
                    variant,
                    -3.6 * m,
                    1.4 * m,
                    s * 0.7,
                    boreal,
                    temperate,
                    oasis,
                    trunk,
                );
            }
            push_forest_glyph(
                &mut out, kind, variant, 0.0, 0.0, s, boreal, temperate, oasis, trunk,
            );
        }
        DecorKind::Plains => {
            // 草点：单颗 1.2px 微圆，稀疏感靠步距。
            out.push(SitePrim::circle(
                kind,
                (r1 - 0.5) * 6.0 * m,
                (r2 - 0.5) * 6.0 * m,
                1.2 * m,
            ));
        }
        DecorKind::Hills => {
            // 半圆丘：宽 9px 高 5px 的半圆（底边贴格中心下方）。
            out.push(SitePrim::half_disc(
                kind,
                (r1 - 0.5) * 4.0 * m,
                1.0 * m,
                4.5 * m,
                5.0 * m,
            ));
        }
        DecorKind::Mountain => {
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
            out.push(SitePrim::triangle(kind, dx, 2.0 * m, w, h));
            out.push(SitePrim::colored(
                kind,
                dx + w * 0.37,
                2.0 * m,
                w * 0.65,
                h * 0.76,
                PrimShape::Triangle,
                recipe.mountain_shade_ink.to_rgba(), // 背光面（压暗右侧）
            ));
            // 簇生次峰：约 30% 的山旁再起一座小的（低 35%、偏移随机）。
            if r2 > 0.70 {
                let ox = (r2 - 0.5) * 14.0 * m;
                out.push(SitePrim::triangle(
                    kind,
                    dx + ox,
                    2.0 * m,
                    w * 0.55,
                    h * 0.62,
                ));
            }
            if height_m >= recipe.snow_cap_h {
                out.push(SitePrim::triangle(
                    DecorKind::Snow,
                    0.0,
                    4.5 * m * size_k,
                    2.6 * m * size_k,
                    3.5 * m * size_k,
                ));
            }
            // 极高山露岩（ADR-0039 D4）：>=3400m 雪帽内缘叠黄褐岩三角
            // （进 Snow 桶第二层图元，顶点色区分）。
            if height_m >= recipe.rock_h {
                out.push(SitePrim::colored(
                    DecorKind::Snow,
                    0.0,
                    3.2 * m * size_k,
                    1.5 * m * size_k,
                    2.2 * m * size_k,
                    PrimShape::Triangle,
                    [0.79, 0.73, 0.54, 0.95], // #C9B98A 露岩
                ));
            }
        }
        DecorKind::Snow => {
            // 雪原白丘：宽 10px 高 4px 的扁半圆。
            out.push(SitePrim::half_disc(
                kind,
                (r1 - 0.5) * 5.0 * m,
                1.0 * m,
                5.0 * m,
                4.0 * m,
            ));
        }
        DecorKind::Desert => {
            // 沙丘：两道扁半圆脊错位叠出风向（宽 9/7px、高 3.2/2.6px）。
            out.push(SitePrim::half_disc(
                kind,
                (r1 - 0.5) * 3.0 * m,
                1.0 * m,
                4.5 * m,
                3.2 * m,
            ));
            out.push(SitePrim::half_disc(
                kind,
                (r2 - 0.5) * 3.0 * m + 2.6 * m,
                -1.2 * m,
                3.5 * m,
                2.6 * m,
            ));
            let _ = r3;
        }
        DecorKind::OceanWaves => {
            // 浅海白波（ADR-0039 D2，ADR-0042 提密提亮）：1 条细弧。
            // 旧值 2.4×1.4 @ alpha 0.5 在 Nation 档几乎不可见（实机截图
            // 确认）；放大到 3.4×2.0 @ alpha 0.62 后远处读得出「海在动」。
            // 密度衰减由调用方 keep_prob 处理（深度越深越稀）。
            let _ = (r2, r3);
            out.push(SitePrim::colored(
                kind,
                (r1 - 0.5) * 6.0 * m,
                (r2 - 0.5) * 4.0 * m,
                3.4 * m,
                2.0 * m,
                PrimShape::Arc,
                [0.99, 0.99, 1.0, 0.62], // 白波
            ));
        }
        DecorKind::Marsh => {
            // 芦苇：2 根细三角（高 6px）+ 底部水线意象由半圆替代。
            let ox = (r1 - 0.5) * 6.0 * m;
            out.push(SitePrim::triangle(
                kind,
                ox - 2.0 * m,
                0.0,
                0.6 * m,
                6.0 * m,
            ));
            out.push(SitePrim::triangle(
                kind,
                ox + 2.0 * m,
                0.0,
                0.6 * m,
                5.0 * m,
            ));
            let _ = (r2, r3);
        }
    }
    out
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

/// 桶合批的重建判据：可见集指纹 + 符号尺寸 + Z + 贴图键。
///
/// `tex` 进 key 是必要的：贴图符号的**顶点**在换主题时可能完全相同（同样的
/// 落位与尺寸），只有图集换了——不把它算进判据，换主题会继续用旧图集。
#[derive(Debug, Clone, Copy, PartialEq)]
struct DecorBatchKey {
    fingerprint: u64,
    size: f32,
    z: f32,
    tex: Option<&'static str>,
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

/// 八桶池 + 森林贴图桶 + 每桶材质句柄。
#[derive(Resource, Default)]
pub struct TerrainDecorPools {
    pools: [DecorBatchPool; 8],
    /// 森林贴图桶（ADR-0050）：与程序化八桶分开存，因为它的 mesh 带 UV、
    /// 材质带纹理，重建判据不同。
    textured_forest: DecorBatchPool,
    material: Option<Handle<ColorMaterial>>,
    /// 装饰贴图句柄缓存（图集键 → 句柄，跨帧复用；解码远贵于重建 mesh）。
    textures: HashMap<&'static str, Handle<Image>>,
    /// 解码失败 / 未知的贴图键（只告警一次，之后静默回退程序化）。
    failed_textures: HashSet<&'static str>,
    /// 贴图材质缓存（图集键 → 材质）。
    textured_materials: HashMap<&'static str, Handle<ColorMaterial>>,
}

impl TerrainDecorPools {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for p in &mut self.pools {
            p.despawn(commands);
        }
        self.textured_forest.despawn(commands);
    }

    /// 图集键 → 已解码句柄；未知键 / 解码失败返回 `None`（调用方回退程序化）。
    ///
    /// **首帧语义**：解码同步完成，第一帧要么拿到句柄要么确定失败——不存在
    /// 「句柄未就绪」的空白帧。失败按键记忆，避免每帧重复解码与重复告警。
    fn texture(&mut self, key: &'static str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(handle) = self.textures.get(key) {
            return Some(handle.clone());
        }
        if self.failed_textures.contains(key) {
            return None;
        }
        let Some(bytes) = embedded_decor_texture(key) else {
            warn!("装饰贴图键不在内嵌表：{key}；回退程序化符号");
            self.failed_textures.insert(key);
            return None;
        };
        match decode_decor_texture(bytes) {
            Some(image) => {
                let handle = images.add(image);
                self.textures.insert(key, handle.clone());
                Some(handle)
            }
            None => {
                warn!("装饰贴图解码失败：{key}；回退程序化符号");
                self.failed_textures.insert(key);
                None
            }
        }
    }

    /// 图集键 + 句柄 → 贴图材质（同键复用；`alpha_mode: Blend` 保留 PNG 软边）。
    fn textured_material(
        &mut self,
        key: &'static str,
        texture: Handle<Image>,
        materials: &mut Assets<ColorMaterial>,
    ) -> Handle<ColorMaterial> {
        if let Some(handle) = self.textured_materials.get(key) {
            return handle.clone();
        }
        let handle = materials.add(ColorMaterial {
            color: Color::WHITE,
            texture: Some(texture),
            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
            ..default()
        });
        self.textured_materials.insert(key, handle.clone());
        handle
    }
}

/// 贴图键 → 编译期嵌入的 PNG 字节（纯函数，零 ECS 访问；可单测）。
pub fn embedded_decor_texture(key: &str) -> Option<&'static [u8]> {
    EMBEDDED_DECOR
        .iter()
        .find_map(|(candidate, bytes)| (*candidate == key).then_some(*bytes))
}

/// 解码内嵌 PNG 为 Bevy `Image`；失败返回 `None`（调用方回退程序化）。
///
/// `is_srgb = true`：手绘稿按 sRGB 创作；`ImageSampler::Default` 走线性过滤，
/// 缩小树冠图集时不出硬边。
fn decode_decor_texture(bytes: &[u8]) -> Option<Image> {
    Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .ok()
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

/// 地形装饰主 system。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_decor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    overlay_index: Res<crate::region_overlay::RegionOverlayIndex>,
    mut pools: ResMut<TerrainDecorPools>,
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

    let material = ensure_material(&mut pools, &mut materials);

    // 森林贴图（ADR-0050）：配方给图集且解码成功才走贴图路径；否则森林族继续
    // 走下面的程序化图元（回退不空帧）。句柄跨帧缓存，不重复解码。
    let forest_art = recipe
        .forest_art
        .and_then(|art| pools.texture(art.atlas, &mut images).map(|tex| (art, tex)));

    // 视口矩形（视图空间）+ 外扩余量。
    let half = view.window_size * view.scale * 0.5 + Vec2::splat(VIEWPORT_MARGIN_PX * view.scale);
    let rect_min = view.camera_center - half;
    let rect_max = view.camera_center + half;

    // 栅格 → 视图：raster_origin + 格中心 × cell_size（与 terrain_chunks 同链）。
    let origin = grid.raster_origin;
    let cell = grid.cell_size;
    let px_to_wu = view.scale;

    // 本图图框（世界单位）。**贴图符号必须夹在框内**：边界行的落点抖动
    // （±0.45 步距）会把树推出图框，落在图外的纯底色上——2026-10-21 实机
    // 截图里图框下方那排树就是这么来的。程序化符号只有几像素，不在此列。
    let frame_min = grid.map_origin;
    let frame_max = grid.map_origin + grid.map_size;
    let tex_size = band.texture_size_px() * view.scale;

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
    // 森林贴图站点：世界坐标 + 图集格号（ADR-0050）。
    let mut textured_sites: Vec<(f32, f32, u32)> = Vec::new();

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
            // 格 → 装饰类。水格（OCEAN/LAKE/RIVER，surface==0）单独走
            // OceanWaves：浅海带（-220..0m）白弧，密度随深度衰减。
            let kind = if wv_terrain::edit::is_water_cell(wv_terrain::TerrainCell {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            }) {
                if c.height < -recipe.wave_max_depth_m || c.height >= 0 {
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
                DecorKind::Forest | DecorKind::Mountain | DecorKind::Snow | DecorKind::Marsh => 1.0,
                DecorKind::Desert => 0.7,
                DecorKind::OceanWaves => {
                    // 密度随深度线性衰减：-10m 全画，-220m 稀至 0.15。
                    ((recipe.wave_depth_scale_m + c.height as f32) / recipe.wave_depth_scale_m)
                        .clamp(0.15, 1.0)
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
            // 森林走贴图（ADR-0050）：只换「长相」，落位 / 密度 / 抖动复用同一条链。
            if kind == DecorKind::Forest {
                if let Some((art, _)) = forest_art.as_ref() {
                    let v = forest_variant(fy, variant, recipe);
                    if let Some((sx, sy)) =
                        clamp_texture_site(wx, wy, tex_size, frame_min, frame_max)
                    {
                        textured_sites.push((sx, sy, forest_art_cell(v, art.cells)));
                    }
                    cx += stride;
                    continue;
                }
            }
            // 图元占位色 → 桶默认 ink（构造器 [0;4] 时回填；变体色原样透传）。
            let (ink_rgb, ink_a) = kind.ink(recipe);
            let ink = [
                ink_rgb[0] as f32 / 255.0,
                ink_rgb[1] as f32 / 255.0,
                ink_rgb[2] as f32 / 255.0,
                ink_a,
            ];
            for mut prim in site_primitives(kind, band, cx, gy, ph, fy, variant, recipe) {
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
    // 森林贴图桶（ADR-0050）：与程序化八桶互斥——有贴图时 Forest 桶为空。
    // 缺资产 / 解码失败 → 清掉可能残留的贴图批，森林回到程序化图元。
    match forest_art {
        Some((art, texture)) => {
            let (verts, uvs, indices) = textured_quads(&textured_sites, tex_size, art.cells);
            let material = pools.textured_material(art.atlas, texture, &mut materials);
            let geom = DecorGeom {
                verts,
                colors: Vec::new(),
                indices,
                uvs: Some(uvs),
                tex: Some(art.atlas),
                size: tex_size,
                z: DECOR_Z,
            };
            sync_decor_bucket(
                &mut commands,
                &mut meshes,
                geom,
                &material,
                &mut pools.textured_forest,
            );
        }
        None => pools.textured_forest.despawn(&mut commands),
    }

    // 同步八桶。
    let size_q = band.spacing_px(recipe) * view.scale * band.size_mul(recipe);
    for (i, (verts, colors, indices)) in buckets.iter_mut().enumerate() {
        let geom = DecorGeom {
            verts: core::mem::take(verts),
            colors: core::mem::take(colors),
            indices: core::mem::take(indices),
            uvs: None,
            tex: None,
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

/// 一帧的桶几何（顶点 / 索引 + 重建判据的尺寸 / Z / 贴图键）。
///
/// `uvs = Some(..)` 时走贴图 mesh（顶点色置空、只带 UV）；`None` = 程序化
/// 顶点色 mesh。两条路径共用同一套桶同步逻辑，区别集中在 [`build_mesh`] /
/// [`build_textured_mesh`] 两个构造函数。
struct DecorGeom {
    verts: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    uvs: Option<Vec<[f32; 2]>>,
    tex: Option<&'static str>,
    size: f32,
    z: f32,
}

/// 单桶合批同步：指纹 / 尺寸 / Z / 贴图键变化才原地改写顶点。
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
        uvs,
        tex,
        size,
        z,
    } = geom;
    let fingerprint = points_fingerprint(&verts);
    let key = DecorBatchKey {
        fingerprint,
        size,
        z,
        tex,
    };
    let mesh = match uvs {
        Some(uvs) if !verts.is_empty() => Some(build_textured_mesh(verts, uvs, indices)),
        Some(_) => None,
        None if !verts.is_empty() => Some(build_mesh(verts, colors, indices)),
        None => None,
    };
    sync_batch(commands, meshes, mesh, material, key, pool);
}

/// 桶批本的增 / 改 / 删（mesh 为 `None` 即「本帧无符号」→ 拆除）。
fn sync_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mesh: Option<Mesh>,
    material: &Handle<ColorMaterial>,
    key: DecorBatchKey,
    pool: &mut DecorBatchPool,
) {
    match pool.batch.take() {
        None => {
            let Some(mesh) = mesh else {
                return;
            };
            let mesh = meshes.add(mesh);
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
            let Some(mesh) = mesh else {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            };
            if batch.key != key {
                if let Some(mut m) = meshes.get_mut(&batch.mesh) {
                    *m = mesh;
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

/// 把贴图站点夹进本图图框；图框比符号还小时返回 `None`（不画，不越界）。
///
/// 贴图符号**底边锚定**（占 `y .. y + size`），所以纵向夹取范围是
/// `[min.y, max.y - size]`，横向留半个身位。边界行的落点抖动（±0.45 步距）
/// 会把树推出图框，落在图外的纯底色上——2026-10-21 实机截图的「图框下方
/// 一排树」正是这个原因。
pub fn clamp_texture_site(
    x: f32,
    y: f32,
    size: f32,
    frame_min: Vec2,
    frame_max: Vec2,
) -> Option<(f32, f32)> {
    let half = size * 0.5;
    let (min_x, max_x) = (frame_min.x + half, frame_max.x - half);
    let (min_y, max_y) = (frame_min.y, frame_max.y - size);
    if min_x > max_x || min_y > max_y {
        return None;
    }
    Some((x.clamp(min_x, max_x), y.clamp(min_y, max_y)))
}

/// 森林贴图站点 → 每站一个轴对齐 quad 的顶点 / UV / 索引（ADR-0050）。
///
/// **锚点 = 底边中心**：树根坐落在采样点上（`y .. y + size`），而不是居中——
/// 居中会让树干穿过地面点、树冠下方留出一截空白。
///
/// **UV 口径**与 `feature_overlays::build_symbol_mesh_uv_seeded` 一致：
/// v=0 是图集顶行（树冠），v=1 是树根；世界 +Y 朝屏幕上方，故 +Y 顶点绑 v=0。
pub fn textured_quads(
    sites: &[(f32, f32, u32)],
    size: f32,
    cells: u8,
) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
    let half = size * 0.5;
    let cells = cells.max(1) as f32;
    let mut verts = Vec::with_capacity(sites.len() * 4);
    let mut uvs = Vec::with_capacity(sites.len() * 4);
    let mut indices = Vec::with_capacity(sites.len() * 6);
    for &(x, y, cell) in sites {
        let u0 = cell as f32 / cells;
        let u1 = (cell as f32 + 1.0) / cells;
        let base = verts.len() as u32;
        verts.push([x - half, y, 0.0]);
        verts.push([x + half, y, 0.0]);
        verts.push([x + half, y + size, 0.0]);
        verts.push([x - half, y + size, 0.0]);
        uvs.push([u0, 1.0]);
        uvs.push([u1, 1.0]);
        uvs.push([u1, 0.0]);
        uvs.push([u0, 0.0]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (verts, uvs, indices)
}

/// 带 UV 的桶 mesh（贴图路径）。不写顶点色：颜色由纹理决定，材质 `color` 为白。
fn build_textured_mesh(verts: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        verts,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
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

    /// 贴图树必须**明显小于采样步距**，且三档绝对值不随步距翻倍——
    /// 早期 `0.9 × 步距` 在 Nation 档出过 65 px 的巨型树（2026-10-21 实机截图）。
    #[test]
    fn texture_sprite_stays_much_smaller_than_the_sampling_stride() {
        let r = recipe();
        // 三档的「树 / 步距」占比上限：Edit 允许最密（树几乎贴着走），
        // Nation 最松（放大档位下树只能占步距的一小半，否则一片林变一排盆栽）。
        for (band, cap) in [
            (DecorBand::Edit, 0.6),
            (DecorBand::Region, 0.5),
            (DecorBand::Nation, 0.4),
        ] {
            let size = band.texture_size_px();
            let stride = band.spacing_px(&r);
            assert!(
                size <= stride * cap,
                "{band:?}: 树 {size} px 不得超过步距 {stride} 的 {:.0}%",
                cap * 100.0
            );
        }
        assert!(
            DecorBand::Nation.texture_size_px() <= 26.0,
            "Nation 档树高上限 26 px（地名 12 px 的两倍余量），got {}",
            DecorBand::Nation.texture_size_px()
        );
        assert!(
            DecorBand::Edit.texture_size_px() >= 12.0,
            "Edit 档不得小于 12 px（再小看不清树种），got {}",
            DecorBand::Edit.texture_size_px()
        );
    }

    #[test]
    fn site_primitives_are_deterministic() {
        let a = site_primitives(
            DecorKind::Forest,
            DecorBand::Edit,
            100,
            200,
            500,
            0.5,
            None,
            &recipe(),
        );
        let b = site_primitives(
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
        let c = site_primitives(
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
        let high = site_primitives(
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
        let low = site_primitives(
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
        let shallow = site_primitives(
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
        let boreal = site_primitives(
            DecorKind::Forest,
            DecorBand::Edit,
            10,
            10,
            300,
            0.15,
            None,
            &recipe(),
        );
        let temperate = site_primitives(
            DecorKind::Forest,
            DecorBand::Edit,
            10,
            10,
            300,
            0.5,
            None,
            &recipe(),
        );
        let oasis = site_primitives(
            DecorKind::Forest,
            DecorBand::Edit,
            10,
            10,
            300,
            0.85,
            None,
            &recipe(),
        );
        // 针叶：三层塔形 ×（描边 + 填充）= 6 片三角冠；站点补第二棵时翻倍。
        let conifer_crowns = boreal
            .iter()
            .filter(|p| p.kind == PrimShape::Triangle && p.color[1] > p.color[0])
            .count();
        assert!(
            conifer_crowns >= 6 && conifer_crowns % 6 == 0,
            "针叶应为 3 层 × 描边/填充 的三角冠（6 或 12 片），got {conifer_crowns}"
        );
        // 阔叶：四团扇贝冠 ×（描边 + 填充）= 8 个圆；描边暗、填充亮。
        let broadleaf_circles: Vec<_> = temperate
            .iter()
            .filter(|p| p.kind == PrimShape::Circle)
            .collect();
        assert!(
            broadleaf_circles.len() >= 8,
            "阔叶应为 4 团 × 描边/填充（≥8 个圆），got {}",
            broadleaf_circles.len()
        );
        assert!(
            broadleaf_circles.iter().any(|p| p.color[1] < 0.2),
            "阔叶必须有深色描边层"
        );
        assert!(
            broadleaf_circles.iter().any(|p| p.color[1] > 0.4),
            "阔叶必须有浅色填充层"
        );
        // 稀树：两团冠 ×（描边 + 填充）= 4 个圆。
        let sparse_crowns = oasis.iter().filter(|p| p.kind == PrimShape::Circle).count();
        assert!(
            sparse_crowns >= 4,
            "稀树应为 2 团 × 描边/填充（≥4 个圆），got {sparse_crowns}"
        );
        assert!(
            oasis.iter().any(|p| p.color[0] > 0.25),
            "绿洲应暖绿（R 分量更高）"
        );
    }

    #[test]
    fn alpine_peaks_get_rock_accent() {
        // ADR-0039 D4：>=3400m 雪帽 + 帽缘露岩（Snow 桶第二图元，黄褐）。
        let peak = site_primitives(
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
        let low = site_primitives(
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
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();
        // 森林贴图（ADR-0050）：系统会解码内嵌图集，测试必须提供 Image 资产库。
        app.init_resource::<Assets<Image>>();
        // 覆盖物规则索引（ADR-0045 D9）：sync_terrain_decor 的必选依赖，
        // 测试装配漏初始化会让系统参数校验 panic（b43b24e 接线后的缺口）。
        app.init_resource::<crate::region_overlay::RegionOverlayIndex>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_terrain_decor);

        schedule.run(&mut app);
        let meshes = app.resource::<Assets<Mesh>>();
        let first = meshes.iter().count();
        assert_eq!(
            first, 1,
            "Edit 档 64² 全林应生成 1 个森林桶 mesh，got {first}"
        );

        // 二帧：无变化 → 零新增资产（原地改写纪律）。
        schedule.run(&mut app);
        let second = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(second, first, "第二帧不应新增 mesh");

        // Nation 档：仍在画（粗步距大地貌团），原地改写而不是新增资产。
        app.resource_mut::<ViewState>().scale = 5.0;
        schedule.run(&mut app);
        let nation = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            nation, 1,
            "Nation 档应保留森林地貌团（ADR-0036），got {nation}"
        );

        // 关掉 TerrainDecor pass：整组下线。
        let mut recipe = app.resource_mut::<RenderRecipe>();
        recipe.passes[PassId::TerrainDecor as usize] = PassPlan::disabled();
        schedule.run(&mut app);
        let off = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(off, 0, "pass 关闭后装饰应全部清除，got {off}");
    }

    /// 变体判定：气候带切点来自配方，region 覆盖物的显式声明优先。
    #[test]
    fn forest_variant_follows_climate_cuts_and_region_override() {
        let r = recipe();
        assert_eq!(
            forest_variant(0.0, None, &r),
            DecorVariant::Conifer,
            "最北 = 针叶"
        );
        assert_eq!(
            forest_variant(0.5, None, &r),
            DecorVariant::Broadleaf,
            "温带 = 阔叶"
        );
        assert_eq!(
            forest_variant(0.95, None, &r),
            DecorVariant::Sparse,
            "最南 = 稀树"
        );
        assert_eq!(
            forest_variant(0.95, Some(DecorVariant::Conifer), &r),
            DecorVariant::Conifer,
            "region 声明优先于气候带"
        );
        assert_eq!(
            forest_variant(0.0, Some(DecorVariant::Boulder), &r),
            DecorVariant::Conifer,
            "boulder 不是森林变体，落回气候带派生"
        );
    }

    /// 图集格序契约：针叶 0 / 阔叶 1 / 稀树 2；格数不足时钳到最后一格。
    #[test]
    fn forest_art_cell_maps_variants_to_atlas_order() {
        assert_eq!(forest_art_cell(DecorVariant::Conifer, 3), 0);
        assert_eq!(forest_art_cell(DecorVariant::Broadleaf, 3), 1);
        assert_eq!(forest_art_cell(DecorVariant::Sparse, 3), 2);
        assert_eq!(forest_art_cell(DecorVariant::Sparse, 2), 1, "钳到最后一格");
        assert_eq!(forest_art_cell(DecorVariant::Conifer, 0), 0, "零格不 panic");
    }

    /// 贴图 quad：每站点 4 顶点 / 6 索引，UV 落各自格，底边锚定在采样点。
    #[test]
    fn textured_quads_put_one_quad_per_site_with_cell_uv() {
        let sites = [(0.0_f32, 0.0_f32, 0u32), (10.0, 20.0, 1), (0.0, 0.0, 2)];
        let (verts, uvs, indices) = textured_quads(&sites, 10.0, 3);
        assert_eq!(verts.len(), 12, "3 站点 × 4 顶点");
        assert_eq!(uvs.len(), 12);
        assert_eq!(indices.len(), 18, "3 站点 × 6 索引");

        // 第一站点：底边在 y=0，顶边在 y=size（底边锚定）。
        assert_eq!((verts[0][0], verts[0][1]), (-5.0, 0.0));
        assert_eq!(verts[2][1], 10.0, "顶边 = 采样点 + size");
        // v=0 绑顶边（树冠），v=1 绑底边（树根）。
        assert_eq!((uvs[0][1], uvs[2][1]), (1.0, 0.0));

        // 格号 → u 区间：0 → [0, 1/3]，1 → [1/3, 2/3]，2 → [2/3, 1]。
        for (site, (u0, u1)) in [
            (0usize, (0.0_f32, 1.0 / 3.0)),
            (1, (1.0 / 3.0, 2.0 / 3.0)),
            (2, (2.0 / 3.0, 1.0)),
        ] {
            assert!(
                (uvs[site * 4][0] - u0).abs() < 1e-6 && (uvs[site * 4 + 1][0] - u1).abs() < 1e-6,
                "站点 {site} 的 u 区间应为 {u0}..{u1}，got {}..{}",
                uvs[site * 4][0],
                uvs[site * 4 + 1][0]
            );
        }
    }

    /// 树的绘制顺序：树干 → 描边层 → 填充层。后端按索引顺序覆盖，
    /// 顺序错了描边会盖住填充（整棵变黑）。
    #[test]
    fn forest_glyph_draws_trunk_then_outline_then_fill() {
        let mut prims = Vec::new();
        push_forest_glyph(
            &mut prims,
            DecorKind::Forest,
            DecorVariant::Broadleaf,
            0.0,
            0.0,
            1.0,
            [0.18, 0.36, 0.21, 0.9],
            [0.17, 0.34, 0.19, 0.9],
            [0.29, 0.42, 0.21, 0.8],
            [0.30, 0.20, 0.15, 1.0],
        );
        assert_eq!(prims[0].kind, PrimShape::Triangle, "先画树干");
        assert!(
            prims[1..5].iter().all(|p| p.color[1] < 0.2),
            "第 1–4 个是描边层（暗）"
        );
        assert!(
            prims[5..9].iter().all(|p| p.color[1] > 0.4),
            "第 5–8 个是填充层（亮）"
        );
    }

    /// 美术迭代用的离线预览（**忽略态**，不参与门禁）：把森林字形按顶点
    /// 顺序光栅化到 PPM，人工看「像不像手绘地图」。
    ///
    /// `cargo test -p wv-render --features app dump_forest_preview -- --ignored --nocapture`
    /// 产物：`crates/wv-render/target/decor_preview/forest.ppm`。
    #[test]
    #[ignore = "art preview, writes target/decor_preview/forest.ppm"]
    fn dump_forest_preview() {
        const W: usize = 380;
        const H: usize = 240;
        let mut px = vec![[0x8c_u8, 0xb9, 0x5f]; W * H];
        let mut blend = |x: i32, y: i32, c: [f32; 4]| {
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                return;
            }
            let a = c[3].clamp(0.0, 1.0);
            let i = y as usize * W + x as usize;
            for k in 0..3 {
                px[i][k] = ((c[k] * 255.0) * a + px[i][k] as f32 * (1.0 - a)).round() as u8;
            }
        };
        let r = recipe();
        let mut draw = |site_x: f32, site_y: f32, prims: &[SitePrim]| {
            for p in prims {
                let (cx, cy) = (site_x + p.dx, site_y - p.dy);
                match p.kind {
                    PrimShape::Circle => {
                        for y in (cy - p.half_h).floor() as i32..=(cy + p.half_h).ceil() as i32 {
                            for x in (cx - p.half_w).floor() as i32..=(cx + p.half_w).ceil() as i32
                            {
                                let nx = (x as f32 - cx) / p.half_w.max(0.01);
                                let ny = (y as f32 - cy) / p.half_h.max(0.01);
                                if nx * nx + ny * ny <= 1.0 {
                                    blend(x, y, p.color);
                                }
                            }
                        }
                    }
                    PrimShape::Triangle => {
                        let v = [
                            (cx - p.half_w, cy + p.half_h),
                            (cx + p.half_w, cy + p.half_h),
                            (cx, cy - p.half_h),
                        ];
                        let area = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
                            (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)
                        };
                        let a0 = area(v[0], v[1], v[2]).signum();
                        for y in (cy - p.half_h).floor() as i32..=(cy + p.half_h).ceil() as i32 {
                            for x in (cx - p.half_w).floor() as i32..=(cx + p.half_w).ceil() as i32
                            {
                                let q = (x as f32, y as f32);
                                let w0 = area(v[0], v[1], q) * a0;
                                let w1 = area(v[1], v[2], q) * a0;
                                let w2 = area(v[2], v[0], q) * a0;
                                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                                    blend(x, y, p.color);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        };
        // 左半：阔叶簇；右上：稀树；右下：针叶簇。
        for gy in 0..7 {
            for gx in 0..8 {
                let prims = site_primitives(
                    DecorKind::Forest,
                    DecorBand::Edit,
                    gx,
                    gy,
                    300,
                    0.5,
                    None,
                    &r,
                );
                draw(30.0 + gx as f32 * 17.0, 170.0 - gy as f32 * 17.0, &prims);
            }
        }
        for gy in 0..5 {
            for gx in 0..6 {
                let prims = site_primitives(
                    DecorKind::Forest,
                    DecorBand::Edit,
                    gx,
                    gy + 40,
                    300,
                    0.15,
                    None,
                    &r,
                );
                draw(190.0 + gx as f32 * 18.0, 215.0 - gy as f32 * 18.0, &prims);
            }
        }
        for gx in 0..5 {
            let prims = site_primitives(
                DecorKind::Forest,
                DecorBand::Edit,
                gx,
                90,
                300,
                0.85,
                None,
                &r,
            );
            draw(210.0 + gx as f32 * 18.0, 50.0, &prims);
        }

        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/decor_preview"
        ));
        std::fs::create_dir_all(dir).unwrap();
        let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
        for p in &px {
            out.extend_from_slice(p);
        }
        std::fs::write(dir.join("forest.ppm"), out).unwrap();
    }

    /// 贴图站点必须夹在本图图框内：框内不动、框外夹回、框太小则丢弃。
    #[test]
    fn texture_sites_are_clamped_inside_the_map_frame() {
        let min = Vec2::ZERO;
        let max = Vec2::new(100.0, 100.0);

        let inside = clamp_texture_site(50.0, 50.0, 20.0, min, max).expect("框内应有值");
        assert_eq!(inside, (50.0, 50.0), "框内站点不得被移动");

        let (x, y) = clamp_texture_site(-10.0, -30.0, 20.0, min, max).expect("可夹取");
        assert_eq!(
            (x, y),
            (10.0, 0.0),
            "越左/越下夹回框内（底边锚定，y 下界是框底）"
        );

        let (x, y) = clamp_texture_site(500.0, 500.0, 20.0, min, max).expect("可夹取");
        assert_eq!((x, y), (90.0, 80.0), "越右/越上夹回框内");

        assert!(
            clamp_texture_site(50.0, 50.0, 200.0, min, max).is_none(),
            "图框比符号还小时应丢弃，而不是画一半在框外"
        );
    }

    /// 内嵌表登记日 / 暮两套图集；未知键返回 `None`（调用方回退程序化）。
    #[test]
    fn embedded_decor_textures_cover_both_palettes_and_reject_unknown_keys() {
        for key in [
            "overlays/forest/decor_atlas_day.png",
            "overlays/forest/decor_atlas_dusk.png",
        ] {
            let bytes = embedded_decor_texture(key).unwrap_or_else(|| panic!("{key} 应内嵌"));
            assert!(bytes.len() > 1024, "{key} 应是真实 PNG");
        }
        assert!(embedded_decor_texture("overlays/forest/does_not_exist.png").is_none());
    }
}
