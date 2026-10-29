//! 像素合成：`CellRenderer` + `shade_*` + 边界扰动 + `image_from_terrain` / `patch_terrain_image`。
//!
//! ## 职责
//! - `CellRenderer` —— 单元渲染器（全图合成与增量 patch **必须共用**）。
//! - `shade_*` —— 视图模式 → 着色函数（`shade_real` / `shade_google` / `shade_flat` / `shade_faded` / `shade_event` / `shade_coast`），随主题色板（`CellPalette`，ADR-0016）取值。
//! - `hash01` / `vnoise` / `wobble_px` —— 边界扰动噪声（确定性，跨平台一致）。
//! - `image_from_terrain` —— 由 `TerrainData` 构造 Bevy `Image`（全图合成）。
//! - `patch_terrain_image` —— 脏块增量 patch（笔触提交用）。
//!
//! ## 边界（不做什么）
//! - **不持有 Image 资源**——仅写入外部 `&mut [u8]` 缓冲。
//! - **不读 World**——`&TerrainData` 由 [`super::terrain_resolve::texture_terrain`] 提供。
//!
//! ## 主要导出
//! - [`image_from_terrain`] / [`patch_terrain_image`]

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::palette::{self, CellPack, CellPalette, SeaRecipe, TreatmentFlags};
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_terrain::biome::display_kind;
use wv_terrain::hash::cell_hash01 as hash01;

use crate::shade::ShadeMode;

use super::terrain_texture::terrain_res_for;
use super::tile_atlas::{
    blit_block_tinted, sample_tile_block, tile_tint_for_mode, TileAtlas, TileTint,
};

/// `wv-core` 中立格 → `wv-terrain` 格（三通道布局一致的字段搬运；wv-io 落盘
/// 与 wv-edit 命令桥各有对称的一对，渲染取色在此处再消费一次）。
fn to_cell(c: &TerrainCellData) -> wv_terrain::TerrainCell {
    wv_terrain::TerrainCell {
        height: c.height,
        surface: c.surface,
        biome: c.biome,
    }
}

/// 格子的**显示槽位**（三通道 → 旧 0–9 调色板槽位）。
///
/// 投影唯一入口是 [`display_kind`]——色板数组零改动，槽位映射只活在那一处。
fn display_slot(c: &TerrainCellData) -> u8 {
    display_kind(&to_cell(c))
}

/// 旧槽位的水系判定（0 深水 / 1 浅水）——**只对 `display_slot` 的产物使用**；
/// 逐格湿判定（surface 通道 ‖ 海拔 < 0）走 [`wv_terrain::edit::is_water_cell`]。
fn slot_is_water(kind: u8) -> bool {
    kind <= 1
}

/// Topographic viewmode 百分位阈值（[ADR-0039] 决策 4 / 5）。
///
/// 在 `&[TerrainCellData]` 上按 `p_low` / `p_high` 切两个高度阈值；
/// 只读 `height` 通道，biome / surface 不参与（X 路线）。
///
/// **与 `wv_terrain::TerrainGrid::compute_height_percentiles` 同语义**，
/// 但本函数消费 `wv_core::terrain::TerrainCellData`（数据层中立 cell），
/// 避免 wv-render 反向依赖 wv-terrain 内部 cell 类型。
///
/// [ADR-0039]: ../docs/adr/ADR-0039-topographic-viewmode.md
pub fn topographic_thresholds_for(
    cells: &[wv_core::terrain::TerrainCellData],
    p_low: f32,
    p_high: f32,
) -> (i16, i16) {
    assert!(
        (0.0..=1.0).contains(&p_low) && (0.0..=1.0).contains(&p_high) && p_low <= p_high,
        "p_low ({p_low}) 和 p_high ({p_high}) 必须在 [0, 1] 且 p_low ≤ p_high"
    );
    if cells.is_empty() {
        return (0, 0);
    }
    let mut heights: Vec<i16> = cells.iter().map(|c| c.height).collect();
    heights.sort_unstable();
    let len = heights.len();
    let idx_low = ((len as f32 * p_low).floor() as usize).min(len - 1);
    let idx_high = ((len as f32 * p_high).floor() as usize).min(len - 1);
    (heights[idx_low], heights[idx_high])
}

/// 把一个着色函数应用到一格，返回 sRGB 线性分量。
///
/// 第三参 = 当前主题色板（ADR-0016）：主题相关色一律经它取，禁止直引常量。
type Shader = fn(&TerrainData, u8, &CellPalette) -> Color;

/// 依据当前视图模式选择纯色着色函数（基线 §1.10 四态；图集缺项/无图集时也走这里）。
fn shade_for_mode(mode: ShadeMode) -> Shader {
    match mode {
        // 真实地形：按高度调制明度，制造立体感（法线光照的廉价近似）。
        ShadeMode::RealTerrain => shade_real,
        // 色块地形：纯色平涂。
        ShadeMode::Schematic => shade_flat,
        // 势力图：原型阶段底色淡化地形（势力几何属 S12，本轮不画）。
        ShadeMode::Faction => shade_faded,
        // 事件图：极简灰白底图。
        ShadeMode::Event => shade_event,
        // 轮廓图：水陆二分深色 + 海岸线（海岸线在边界格逐像素画，见
        // `write_coast_boundary_cell`；这里只管内部格的水/陆平涂）。
        ShadeMode::Coastline => shade_coast,
        // Google Default Map：近白陆地 + 淡水 + 绿地（色板见 palette_google）。
        ShadeMode::GoogleMap => shade_google,
        // 等值线矢量平涂（ADR-0012 / plan v0.1 M2）：内部格走本 shader 的
        // 类型色平涂；边界格的水陆由 `sample_vector_flat` 的双线性高度采样
        // 决定——填充边与 `isoline_overlay` 的等值线描边同源吻合。
        ShadeMode::VectorFlat => shade_flat,
        // Topographic 分析视图（[ADR-0039]）—— PR-1 **占位**：
        // 仅返主题纸面色（`palette.soften_toward`），正确的 cell-by-cell 水/陆
        // + height 3 阶着色 PR-1b 落地（独立 worktree；详见 `shade_topographic`
        // 的 doc-comment 与 ADR-0039 §6 落点列表的 "1 cell" 行）。
        //
        // **为什么不现在做**：本 shader 类型签名不持 cell 坐标——
        // `fn(&TerrainData, u8, &CellPalette) -> Color`——无法在着色路径里
        // 查 `data.cells[gy * w + cx]` 拿到 cell.height + cell.surface；
        // cell-by-cell 着色需 `write_cell_split` 早返回分支，依赖
        // `CellRenderer` 持有百分位阈值（阈值缓存现归本 crate `CellRenderer`；
        // 历史上的 `ViewState::topographic_thresholds` 镜像已于 2026-10-29 删除）。
        ShadeMode::Topographic => shade_topographic,
    }
}

/// Topographic viewmode 着色占位（[ADR-0039]）。
///
/// **PR-1 限制**：返主题纸面色 `palette.soften_toward`，不读 cell 数据，
/// 不切分 3 阶。PR-1b 会在 `write_cell_split` 早返回分支按 cell.height +
/// cell.surface 走 `topographic::pick` 取 water / land_low / land_mid / land_high。
///
/// 当前返纸面色是为了：(a) match 不漏分支；(b) 切到 Topographic mode 时不
/// 渲染崩溃，且提供「底图变空」的视觉提示——方便用户知道「Topographic 模式
/// 接通了，但视觉细节等 PR-1b」。
///
/// [ADR-0039]: ../docs/adr/ADR-0039-topographic-viewmode.md
fn shade_topographic(_data: &TerrainData, _kind: u8, pal: &CellPalette) -> Color {
    pal.soften_toward
}

fn shade_google(_data: &TerrainData, kind: u8, pal: &CellPalette) -> Color {
    // Google Default Map 做普通地图折叠（2026-10-13 用户裁定）：
    // 不渲染地形高低分带。Google 普通地图的陆地是单一近白平涂；若把
    // Perlin 高度分带（沙/草/丘/山/雪）逐档着色，整图会退化成近色马赛克。
    // 折叠规则：水两档保留层次、森林作公园绿斑、沼泽保留湿绿，
    // 其余陆上类一律折叠为陆地基色（`core[2]` 近白陆色）。
    //
    // **色值读 `pal.core`，不读 [`palette::GOOGLE_CORE_TERRAIN`]**（2026-10-25
    // 修正）：`conv_palette_terrain` 在 GoogleMap 模式下已把 `core` 换成 Google
    // 十色；若这里继续读 crate 本地常量，那份主题覆盖对像素就是**无效接线**
    // ——两处同名表，改一处不生效。本地表保留为 CellMap 工作台等「无主题
    // 环境」的默认数据源（crate 级固定色禁令，ADR-0054 D4）。
    let land = pal.core[2];
    match kind {
        0 | 1 | 4 | 8 => pal.core.get(kind as usize).copied().unwrap_or(land),
        _ => land,
    }
}

fn shade_flat(_data: &TerrainData, kind: u8, pal: &CellPalette) -> Color {
    pal.core_terrain(kind)
}

fn shade_real(data: &TerrainData, kind: u8, pal: &CellPalette) -> Color {
    // 用 kind 对应的基色，叠加高度明度调制。
    // 这里 data 已无单格高度上下文，故高度调制在 write_terrain_pixels 里做。
    let _ = data;
    pal.core_terrain(kind)
}

fn shade_faded(_data: &TerrainData, kind: u8, pal: &CellPalette) -> Color {
    // 淡化：向**该主题的纸面色**混 55%，保留地形骨架（特效 §2.1 alpha≈0.35
    // 的近似）。ADR-0016 前混固定灰 0.7；古舆图 / 暮色下向纸面混合更自洽，
    // 也让势力图底色随主题走（暮色势力图不再是刺眼的亮灰底）。
    let base = pal.core_terrain(kind);
    let Color::Srgba(c) = base else {
        return base;
    };
    let Color::Srgba(t) = pal.soften_toward else {
        return base;
    };
    let k = 0.45; // 保留比例
    Color::srgb(
        c.red * k + t.red * (1.0 - k),
        c.green * k + t.green * (1.0 - k),
        c.blue * k + t.blue * (1.0 - k),
    )
}

fn shade_event(_data: &TerrainData, _kind: u8, pal: &CellPalette) -> Color {
    pal.event_backdrop
}

fn shade_coast(_data: &TerrainData, kind: u8, _pal: &CellPalette) -> Color {
    // 轮廓图三色是模式语义色，不随主题切换（ADR-0016 §2）。
    if slot_is_water(kind) {
        palette::COAST_WATER
    } else {
        palette::COAST_LAND
    }
}

// ---------------------------------------------------------------------------
// 水域按高度的渐变（2026-10-15，手绘地图呈现改进 #1）
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 坡度浮雕（ADR-0077 D6）：公式真源在 `crate::shading`——Horn 米制梯度、
// 世界法线、双光源光照明度（含 LIGHT_* 常量）全部只认那一处，本文件
// 零光照字面量；乘子入口 = `CellRenderer::relief_intensity`。
// 旧「逐格差分 × 0.0012 + 0.84~1.16 / 0.90~1.12 双套钳制」
//（RELIEF_PER_METER / RELIEF_MIN / RELIEF_MAX / RELIEF_*_CARTOGRAPHIC、
// relief_mul / relief_mul_clamped）随界内性免费获得而整体退役。
// 2026-10-28 spec D1 修正：LIGHT_* 改存真 45° 高度角**单位向量**，CPU
// 直接 dot，早期临时的预归一化副本（LIGHT_*_UNIT）随之取消。
// ---------------------------------------------------------------------------

/// 水域按高度的渐变色——lerp `water_shallow → water_deep`，并对深色带
/// 提一点蓝饱和（海沟观感）。所有视图模式下的水域**内部格**都走这条；
/// 边界格的双线性高度采样会通过 `sample_vector_flat` 拿到亚格高度，
/// 但单格渲染阶段我们只能用该格的 `cell.height`（一格一种颜色，与 civ-atlas
/// 同样离散），从远处看仍是渐变观感。
///
/// 海岸/轮廓模式不进这里（`shade_coast` 单独出口）。
///
/// **最深深水位（`t = 1.0`，约 `height ≤ -3500 m`）直接返回 [`CellPalette::deep_ocean_blue`]**：
/// 不再走主题里的「深海」色 + 冷偏移。原因：旧路径过 `soften (35% keep)` +
/// 远缩放 `veil` 后会退成中性灰（约 `(210, 213, 212)`），肉眼辨不出「海」
///（2026-10-16 用户反馈）。新版用一个语义「深海」色，无论缩放 / 主题
/// 怎么叠都明确深蓝。`MapBase::Ocean.cell()` 的 `height = -3500` 命中
/// 这一分支——所以选了「海洋」的用户立刻看到深蓝。
fn water_shade_at(height: i16, pal: &CellPalette, sea: &SeaRecipe) -> Color {
    let t = sea.depth_t(height);
    // 2026-10-16：最深深水位（海沟、大洋中央）走明确深蓝色，不再 lerp。
    if t >= 1.0 {
        return pal.deep_ocean_blue;
    }
    let shallow = pal.core_terrain(1); // water_shallow
    let deep = pal.core_terrain(0); // water_deep
    let (Color::Srgba(a), Color::Srgba(b)) = (shallow, deep) else {
        return shallow;
    };
    // 主体 lerp：sRGB 空间线性插值足够（手绘感是色块而非 PBR）。
    let base = Color::srgb(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
    );
    // 深海带（t > 0.7）微微拉向冷蓝，模拟水彩的"水越深越冷"——颜色令牌里
    // 已有现成青蓝色（label_fill halo，soften_toward 同族），调一点即可。
    if t > 0.7 {
        let k = (t - 0.7) / 0.3; // 0..1 over the deep band
        let Color::Srgba(c) = base else {
            return base;
        };
        // 冷偏：把红色通道压一点，蓝色补一点。Srgba 直接调，简单可控。
        Color::srgb(
            (c.red * (1.0 - 0.18 * k)).clamp(0.0, 1.0),
            (c.green * (1.0 - 0.06 * k)).clamp(0.0, 1.0),
            (c.blue + 0.10 * k).clamp(0.0, 1.0),
        )
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// 方案 A：边界扰动 + 超采样（抗锯齿地形边界）
// ---------------------------------------------------------------------------

// 整数格点确定性 hash → `[0,1)`：实现移入 `wv_terrain::hash::cell_hash01`
// （顶部 `use ... as hash01`），消除与渲染层 `paper_overlay` 的逐字重复。

// ---------------------------------------------------------------------------
// 陆格颗粒（ADR-0042 手绘 Nation 表现层）
// ---------------------------------------------------------------------------

/// 陆格明度抖动总幅度：±4.5%。
///
/// 参考图（手绘 JRPG）的陆地是水彩颗粒而不是平涂；逐格确定性抖动把
/// 「塑料平色块 + 纬度条带」揉出纸面颗粒。幅度太小看不出、太大会成
/// 色噪点——4.5% 在实机 26% 缩放下恰好是「细看有颗粒、远看是均匀
/// 水彩」的区间。
const LAND_GRAIN: f32 = 0.09;

/// 水彩晕开的模糊半径（格）：5×5 窗 ≈ 2 格过渡带（civ-atlas `paintBase`
/// 的 `boxBlurWrap(3×S)` 同量级）。1 = 看不出，3 = 群系糊成一片。
const BLEED_RADIUS: u32 = 2;

/// 一格的**基色**（ADR-0044 D1）：shader 色 + 水深渐变 + 雪双调——
/// 即旧「内部格直铺」在颗粒 / 淡化 / 浮雕**之前**的那一步。抽成纯函数是
/// 为了让 [`CellRenderer::blended_base`]（5×5 模糊）与内部格直铺共用
/// 同一取色口径：模糊的输入 = 直铺的输出，两条路径永不漂移。
// shore 参数与既有 7 参同属「一个取色闭包的完备输入」（颜色 = f(数据, 位置,
// 主题, 旗标, 场)），收拢成结构体反而稀释调用点可读性——特此豁免。
#[allow(clippy::too_many_arguments)]
fn base_color_of(
    data: &TerrainData,
    kind: u8,
    cx: u32,
    cy: u32,
    pal: &CellPalette,
    treatment: &TreatmentFlags,
    mode: ShadeMode,
    shore: Option<&[u16]>,
    sea: &SeaRecipe,
) -> Color {
    let cell = &data.cells[(cy as usize) * data.width as usize + cx as usize];
    let mut color = shade_for_mode(mode)(data, kind, pal);
    if treatment.water_gradient && slot_is_water(kind) && mode != ShadeMode::Coastline {
        color = water_shade_at(cell.height, pal, sea);
    }
    if treatment.snow_shade && kind == 7 && mode != ShadeMode::Coastline {
        color = snow_shade_at(cell.height, pal);
    }
    // 近岸浅滩带（2026-10-21 S2）：离岸近的水格向主题 `water_shore` 偏移。
    // 放在**基色**这一步——模糊（bleed）的输入 = 直铺的输出，两条路径
    // 经同一函数取色永不漂移；颗粒 / 淡化 / 浮雕照旧在其后叠加。
    if let (true, Some(field)) = (treatment.shore_shallow, shore) {
        if slot_is_water(kind) && mode != ShadeMode::Coastline {
            let idx = (cy as usize) * data.width as usize + cx as usize;
            if let Some(d) = field.get(idx) {
                let k = sea.shore_band_strength(*d);
                if k > 0.0 {
                    let (Color::Srgba(c), Color::Srgba(s)) = (color, pal.water_shore) else {
                        return color;
                    };
                    color = Color::srgb(
                        c.red + (s.red - c.red) * k,
                        c.green + (s.green - c.green) * k,
                        c.blue + (s.blue - c.blue) * k,
                    );
                }
            }
        }
    }
    color
}

/// 近岸浅滩带的离岸距离场（S2 消费口径）：旗标开 ∧ 平涂两模式
///（VectorFlat / Schematic，与 `bleed` 同口径——浅滩是平涂语法）时现算。
/// O(N) BFS，1024² ≈ 数 ms，摊进整图合成 / patch 的量级
///（同 Topographic 阈值「每次合成重算」的取舍，[ADR-0039] §4.1.1）。
fn shore_field_for(
    data: &TerrainData,
    mode: ShadeMode,
    treatment: &TreatmentFlags,
    sea: &SeaRecipe,
) -> Option<Vec<u16>> {
    let wants_field = treatment.shore_shallow || !sea.ripples.is_empty();
    if !wants_field || !matches!(mode, ShadeMode::VectorFlat | ShadeMode::Schematic) {
        return None;
    }
    let (w, h) = (data.width.max(1), data.height.max(1));
    // 湿判定与渲染同口径（surface 通道 ‖ 高度 < 0，`wv_terrain::edit::is_water_cell`），
    // 闭包直读 `TerrainCellData` 免去整场 cell 拷贝。
    let field = wv_terrain::shore::shore_distance_field(
        w,
        h,
        |x, y| {
            let c = &data.cells[(y as usize) * w as usize + x as usize];
            !(c.surface == wv_terrain::SURFACE_WATER || c.height < 0)
        },
        // 截断半径由配方决定：配方改动带宽 / 加波纹时场自动跟着扩，
        // 不会出现「带比场宽、远端截断成硬边」。
        sea.field_radius(),
    );
    Some(field)
}

/// 水彩晕开（ADR-0044 D1）：`(cx, cy)` 处的 **5×5 盒式模糊基色**。
///
/// 纯函数——只依赖 `data` 邻域，任意带数 / 任意批次下逐字节一致
/// （行带并行契约、增量 patch 契约同时保住）。邻域越界钳边。
/// 模糊的是**基色**（直铺口径），颗粒 / 淡化 / 浮雕照旧在其后逐格叠加。
#[allow(clippy::too_many_arguments)] // 与 base_color_of 同：取色闭包的完备输入（含场与配方）
fn blended_base(
    data: &TerrainData,
    cx: u32,
    cy: u32,
    pal: &CellPalette,
    treatment: &TreatmentFlags,
    mode: ShadeMode,
    shore: Option<&[u16]>,
    sea: &SeaRecipe,
) -> Color {
    let w = data.width;
    let h = data.height;
    let (x0, y0) = (
        cx.saturating_sub(BLEED_RADIUS),
        cy.saturating_sub(BLEED_RADIUS),
    );
    let (x1, y1) = (
        (cx + BLEED_RADIUS).min(w - 1),
        (cy + BLEED_RADIUS).min(h - 1),
    );
    let mut ar = 0.0f32;
    let mut ag = 0.0f32;
    let mut ab = 0.0f32;
    let mut n = 0u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let kind = display_slot(&data.cells[(y * w + x) as usize]);
            let Color::Srgba(c) = base_color_of(data, kind, x, y, pal, treatment, mode, shore, sea)
            else {
                continue;
            };
            ar += c.red;
            ag += c.green;
            ab += c.blue;
            n += 1;
        }
    }
    let n = n.max(1) as f32;
    Color::srgb(ar / n, ag / n, ab / n)
}

/// 陆格颗粒乘子：水系两槽（0 深水 / 1 浅水）恒 1.0——等深渐变是连续
/// 函数，叠噪声会打架；陆上各类（沙/草/林/丘/山/雪/沼/路）按格抖动。
/// 全渲染路径共用（全图烘焙 / 增量 patch / VectorFlat 采样），同一格
/// 任何时刻得到同一乘子（P7 派生确定性）。
fn land_grain(kind: u8, cx: u32, cy: u32) -> f32 {
    if kind <= 1 {
        return 1.0;
    }
    1.0 - LAND_GRAIN * 0.5 + LAND_GRAIN * hash01(cx as i64, cy as i64, 0x5EED_C0DE)
}

/// 平滑值噪声：整数格点 hash + smoothstep 双线性插值。
///
/// 与 [`hash01`] 同族（确定性、跨平台一致）。用于水面纸纹与波纹抖动——
/// **逐像素**而非逐格：逐格抖动在水面会读成马赛克，纸纤维必须是细颗粒。
fn value_noise(x: f32, y: f32, seed: u64) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let tx = x - xi;
    let ty = y - yi;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let (x0, y0) = (xi as i64, yi as i64);
    let n00 = hash01(x0, y0, seed);
    let n10 = hash01(x0 + 1, y0, seed);
    let n01 = hash01(x0, y0 + 1, seed);
    let n11 = hash01(x0 + 1, y0 + 1, seed);
    let a = n00 + (n10 - n00) * sx;
    let b = n01 + (n11 - n01) * sx;
    a + (b - a) * sy
}

// ---------------------------------------------------------------------------
// 雪格双调（手绘地图呈现 #3，2026-10-15）
// ---------------------------------------------------------------------------

/// 雪阴影强度系数 [0, 1]：
/// - 海拔 ≤ `SNOW_SHADOW_MIN_HEIGHT` → 0（雪原是平的，不叠阴影）
/// - 海拔每超 1 km 多 0.35，封顶 1.0
///
/// 端口 civ-atlas 的 `SNOW_SHADOW` 思路——雪格按海拔叠冷蓝灰，越高越冷。
fn snow_shadow_t(height: i16) -> f32 {
    let over = (height as f32 - palette::SNOW_SHADOW_MIN_HEIGHT as f32).max(0.0);
    let t = over / 1000.0 * palette::SNOW_SHADOW_PER_KM;
    t.clamp(0.0, 1.0)
}

/// 雪格按高度叠冷蓝灰阴影（手绘地图 #3）。
/// 高度低于 `SNOW_SHADOW_MIN_HEIGHT` → 走 `snow_highlight`（比 core_terrain[7] 略亮的近白），
/// 高于阈值则按 `snow_shadow_t` 在 highlight ↔ shadow 之间 lerp。
fn snow_shade_at(height: i16, pal: &CellPalette) -> Color {
    let t = snow_shadow_t(height);
    let (Color::Srgba(h), Color::Srgba(s)) = (pal.snow_highlight, pal.snow_shadow) else {
        return pal.snow_highlight;
    };
    Color::srgb(
        h.red + (s.red - h.red) * t,
        h.green + (s.green - h.green) * t,
        h.blue + (s.blue - h.blue) * t,
    )
}

/// 平滑 value noise（双线性 + smoothstep），输出 `[-1,1]`。
fn vnoise(x: f32, y: f32, freq: f32, seed: u64) -> f32 {
    let (fx, fy) = (x * freq, y * freq);
    let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash01(x0, y0, seed);
    let b = hash01(x0 + 1, y0, seed);
    let c = hash01(x0, y0 + 1, seed);
    let d = hash01(x0 + 1, y0 + 1, seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// 边界扰动的幅度系数（相对每格像素边长）：0.45 → 边界在格内大幅摆动，
/// 又不至于越过邻格太深造成「噪点斑块」。
const WOBBLE_AMP: f32 = 0.45;
/// 扰动噪声的空间频率基长（格）。
const WOBBLE_FREQ_CELLS: f32 = 1.5;
/// 2×2 旋转网格超采样点位（像素内归一化坐标）——比规则 2×2 更好地打散摩尔纹。
const SUBSAMPLES: [[f32; 2]; 4] = [
    [0.125, 0.375],
    [0.625, 0.125],
    [0.875, 0.625],
    [0.375, 0.875],
];

/// 纹理像素坐标处的边界扰动偏移（像素）。同一像素从任何格写入结果一致，
/// 因此相邻格共享的边界被「揉」成同一条连续曲线。
fn wobble_px(px: f32, py: f32, res: u32) -> (f32, f32) {
    let freq = 1.0 / (res as f32 * WOBBLE_FREQ_CELLS);
    let amp = res as f32 * WOBBLE_AMP;
    (
        vnoise(px, py, freq, 0x9E37_79B9_7F4A_7C15) * amp,
        vnoise(px, py, freq, 0xC2B2_AE3D_27D4_EB4F) * amp,
    )
}

/// 单元渲染器：全图合成（[`write_terrain_pixels`]）与增量 patch
/// （[`patch_terrain_image`]）**必须共用**——否则笔刷改过的区域走旧直铺、
/// 周边走扰动合成，同一条海岸线上会出现「直线段 vs 揉曲线」的拼接裂缝。
///
/// 内部格走整块直铺快路径（与旧行为逐字节一致）；仅邻接异类的**边界格**
/// 走逐像素扰动 + 2×2 旋转超采样（4 样本/像素）。
struct CellRenderer<'a> {
    data: &'a TerrainData,
    w: u32,
    h: u32,
    mode: ShadeMode,
    /// 当前主题色板（ADR-0016）：着色与淡化的取值来源。
    palette: &'a CellPalette,
    /// 海的画法配方（ADR-0048）：深度量程 / 浅滩带 / 纸纹 / 墨线波纹。
    sea: SeaRecipe,
    /// 水面细节（纸纹 / 墨线）开关：与 `grain` 同口径——Google 普通地图要
    /// 干净、Topographic 是分析视图、Coastline 只有水陆两色、Event 极简底图，
    /// 四者都不掺纸纹与墨线。
    water_detail_enabled: bool,
    /// 高度归一化范围（仅 `RealTerrain` 模式非零）。
    hmin: f32,
    hrange: f32,
    atlas: Option<&'a TileAtlas>,
    res: u32,
    shader: Shader,
    /// 底图淡化开关（`ViewState.terrain_soften` 的镜像）。
    soften: bool,
    /// 边界扰动开关：Google 模式关闭（Google 普通地图的边界是干净的直线/平滑
    /// 曲线；手绘感揉边与「普通地图」气质相悖）。
    wobble: bool,
    /// 陆格颗粒开关（ADR-0042 D6）：`TreatmentFlags.land_grain` ∧ 非 Google
    /// 模式——「折叠成同一陆色」是 Google 模式的硬契约，逐格抖动会重新
    /// 引入马赛克。
    grain: bool,
    /// 水彩晕开开关（ADR-0044 D1）：`TreatmentFlags.watercolor_bleed` ∧ 仅
    /// VectorFlat / Schematic——水彩是平涂语法；Google（干净）、RealTerrain
    ///（高度写真）、Coastline / Event / Topographic（自有三色/管线）都不掺。
    bleed: bool,
    /// 装饰旗标（ADR-0023 D4：`CellPack.treatment`）。HandDrawn 全 `true` 时
    /// 所有 gate 与历史恒开行为逐位一致。
    treatment: TreatmentFlags,
    /// `kind → res² 瓦片块`（按需填充；`None` = 尚未尝试）。
    blocks: [Option<Vec<u8>>; 10],
    /// `kind` 是否已尝试过建块（避免无映射 kind 被反复查）。
    tried: [bool; 10],
    /// Topographic mode 专用：7 条海拔内边界（米，绝对阶梯 + 窄域兜底，
    /// `cuts_for_cells` 产物；ADR-0056 修正 ADR-0055 原案「逐图百分位」）。
    /// 非 Topographic mode 为 `None`（着色路径零开销）。
    topographic: Option<super::hypsometric::Cuts>,
    /// **专业制图的海拔色带**（ADR-0067 D3）：8 阶 + 混入强度 + 7 条内边界。
    ///
    /// `None` = 不叠色带（非 Google 模式 / 风格没给色带）。非 Google 模式的
    /// 分层设色走 `hypsometric`（Topographic 数据视图），与本项**互斥**。
    elevation: Option<super::hypsometric::ElevationBand>,
    /// 分层设色强度（`CellPack.hypsometric` 的镜像；ADR-0055）：0 = 纯群系色，
    /// 1 = 海拔色带完全替代群系色。
    hypsometric: f32,
    /// 浮雕的梯度无量纲化增益（`s = Z_EXAG / 米每格`，ADR-0077 D2/D6）。
    ///
    /// 由合成入口的 `meters_per_cell` 经 [`crate::shading::slope_gain_of`]
    /// 一次算出——米制化后「同物理坡 → 同法线」跨图幅成立；旧
    /// `RELIEF_PER_METER` 的「每米高差 × 固定系数」口径随路径退役。
    /// 全图合成与增量 patch 必须传同一值，否则同格两路径浮雕分叉。
    slope_gain: f32,
    /// 近岸浅滩带用的离岸距离场（S2；行优先，长度 = w×h）。`None` =
    /// 旗标关 / 模式不参与——着色路径零开销。由驱动方（`terrain_pixels`
    /// / `patch_terrain_image`）**整场算一次**后共享给各带 renderer
    /// （行带并行契约：场与带数无关，任意切分逐字节一致）。
    shore: Option<&'a [u16]>,
}

impl<'a> CellRenderer<'a> {
    fn new(
        data: &'a TerrainData,
        mode: ShadeMode,
        atlas: Option<&'a TileAtlas>,
        soften: bool,
        pack: &'a CellPack,
        shore: Option<&'a [u16]>,
        meters_per_cell: f32,
    ) -> Self {
        let palette = &pack.palette;
        let treatment = pack.treatment;
        let (w, h) = (data.width.max(1), data.height.max(1));
        // 事件图与轮廓图都不铺瓦片：前者灰白底，后者水陆三色。
        // Topographic（[ADR-0039]）也不铺瓦片——它按 cell.height + cell.surface
        // 走独立着色管线，不依赖瓦片图集。
        let atlas = if matches!(
            mode,
            ShadeMode::Event | ShadeMode::Coastline | ShadeMode::Topographic
        ) {
            None
        } else {
            atlas
        };
        // 事件图与轮廓图不铺瓦片；Google 模式无图集也使用亚像素分辨率。
        // Topographic 落到 res=1（每格 1 像素）——分析视图要锐利的方格边界，
        // 不做亚像素插值。
        let res = terrain_res_for(data, mode, atlas);
        let (hmin, hmax) = if mode == ShadeMode::RealTerrain {
            data.cells.iter().fold((i16::MAX, i16::MIN), |(lo, hi), c| {
                (lo.min(c.height), hi.max(c.height))
            })
        } else {
            (0, 0)
        };
        // Topographic 的 8 阶海拔内边界（ADR-0056）：**绝对地形学阶梯**
        // （200/500/1000/1500/2500/4000/6000 m），窄域图（跨度 < 300 m）
        // 兜底为自身区间等分。O(N) 只求 min/max，不排序——每次合成 / 补丁
        // 都廉价重算（阈值常量稳定，笔刷不会让别处颜色跟着漂）。
        let topographic = if mode == ShadeMode::Topographic {
            Some(super::hypsometric::cuts_for_cells(&data.cells))
        } else {
            None
        };
        // 专业制图的海拔色带（ADR-0067 D3）：只在色板给了色带时生效。
        // 切点复用 `hypsometric::cuts_for_cells`（绝对阶梯 + 窄域兜底）——
        // 与 CellMap 的分层设色同一套阈值语义，但**配色与强度**来自风格层。
        let elevation = match (mode, palette.elevation_ramp.as_ref()) {
            (ShadeMode::GoogleMap, Some(ramp)) => Some(super::hypsometric::ElevationBand {
                ramp: *ramp,
                cuts: super::hypsometric::cuts_for_cells(&data.cells),
                strength: palette.elevation_strength.clamp(0.0, 1.0),
            }),
            _ => None,
        };
        Self {
            data,
            w,
            h,
            mode,
            palette,
            hmin: hmin as f32,
            hrange: (hmax as f32 - hmin as f32).max(1.0),
            atlas,
            res,
            shader: shade_for_mode(mode),
            // 轮廓图下淡化开关无效：黑底不能被纸面色洗掉（见测试
            // `coastline_mode_ignores_soften_flag`）。
            // Google 模式同样无效：色板已是最终观感，再淡化会把水色
            // 洗成米糊、全图趋同（见测试 `google_mode_ignores_soften_flag`）。
            // Topographic 同理：分析视图的四色是语义色（水/低/中/高），
            // 淡化会把 3 阶之间的对比洗没。
            soften: soften
                && !matches!(
                    mode,
                    ShadeMode::Coastline | ShadeMode::GoogleMap | ShadeMode::Topographic
                ),
            // 等值线模式（VectorFlat）也关闭揉边：它的自然蜿蜒由
            // `isoline_overlay` 的等值线描边承担，逐像素噪声会和几何曲线打架。
            // 主题装饰旗标（ADR-0023 D4）是第三道闸：EMPTY 类主题整体关揉边。
            // Topographic 也关：分析视图要锐利方格，不做手绘揉边。
            wobble: treatment.boundary_wobble
                && !matches!(
                    mode,
                    ShadeMode::GoogleMap | ShadeMode::VectorFlat | ShadeMode::Topographic
                ),
            // 颗粒（ADR-0042 手绘 Nation）与水彩晕开（ADR-0044）都是**美术
            // 装饰**——Topographic 是分析视图，四色是语义色（水/低/中/高），
            // 装饰会洗掉 3 阶对比。故 Topographic 与 GoogleMap 一样不进。
            grain: treatment.land_grain
                && !matches!(mode, ShadeMode::GoogleMap | ShadeMode::Topographic),
            bleed: treatment.watercolor_bleed
                && matches!(mode, ShadeMode::VectorFlat | ShadeMode::Schematic),
            treatment,
            blocks: std::array::from_fn(|_| None),
            tried: [false; 10],
            topographic,
            elevation,
            hypsometric: pack.hypsometric.clamp(0.0, 1.0),
            slope_gain: crate::shading::slope_gain_of(meters_per_cell),
            shore,
            sea: pack.sea.clone(),
            water_detail_enabled: !matches!(
                mode,
                ShadeMode::GoogleMap
                    | ShadeMode::Topographic
                    | ShadeMode::Coastline
                    | ShadeMode::Event
            ),
        }
    }

    /// 叠**专业制图的海拔色带**（ADR-0067 D3）。
    ///
    /// 色带读「高」，晕渲读「坡」——两者正交互补（ADR-0060 §2）。只对陆格
    /// 生效：水面已有深度渐变，叠色带会脏。
    ///
    /// **为什么不在 `shade_google` 里做**：那个函数只有 `kind`，没有高度——
    /// 色带按海拔分，必须拿格高。放在这里（唯一的「已着色 → 输出」闸口）保证
    /// 两条路径（逐格直铺 / 亚格采样）同格同色。
    fn apply_elevation_band(
        &self,
        cell: &wv_core::terrain::TerrainCellData,
        color: Color,
    ) -> Color {
        let Some(band) = self.elevation.as_ref() else {
            return color;
        };
        if super::hypsometric::is_water_cell(cell) {
            return color;
        }
        let Color::Srgba(c) = color else {
            return color;
        };
        let tinted = band.tint([c.red, c.green, c.blue, 1.0], cell.height);
        Color::srgb(tinted[0], tinted[1], tinted[2])
    }

    /// 坡度浮雕乘子（ADR-0077 D6）：`crate::shading` 真源的**双光源单尺度**
    /// 公式——Horn 米制梯度 → 世界法线 → 光照明度乘子。三尺度权重在此路径
    /// 退化为 detail 全额 1.0（D4 例外：烘焙纹理无 mip 链可衰减）。
    ///
    /// VectorFlat / RealTerrain / GoogleMap 三模式共走这一个入口：旧「艺术
    /// 0.84~1.16 / 专业制图 0.90~1.12 两套钳制」随 `I ∈ [0.4, 1.0]` 界内性
    /// 免费获得而退役。`LIGHT_*` 自 2026-10-28 起为单位向量（spec D1 修正），
    /// 直接 `dot(n, LIGHT_*)` 即合法入参。与旧 `relief_mul` 的受光方向逐轴
    /// 相反是**既定修正**——旧 CPU 路径方向本来就反（见
    /// `shading::normal_world` 文档），切换后 CPU/GPU 统一到「左上受光」
    /// 口径。
    ///
    /// 纯函数、只读高度场：同一格在任何渲染路径（全图烘焙 / 笔刷 patch）
    /// 得到同一乘子，不需要额外的缓存或随机种子。
    fn relief_intensity(&self, cx: u32, gy: u32) -> f32 {
        let (hx, hy) = crate::shading::horn_gradient(self.data, cx as i64, gy as i64);
        let n = crate::shading::normal_world(hx, hy, self.slope_gain);
        let dot = |l: [f32; 3]| n[0] * l[0] + n[1] * l[1] + n[2] * l[2];
        crate::shading::lighting_intensity(
            dot(crate::shading::LIGHT_MAIN),
            dot(crate::shading::LIGHT_AUX),
        )
    }

    /// `kind` 的预采样瓦片块（按需建，整图共享）。
    fn block_for(&mut self, kind: u8) -> Option<&[u8]> {
        let k = kind as usize;
        if k >= 10 {
            return None;
        }
        if !self.tried[k] {
            self.tried[k] = true;
            if let (Some(a), Some([col, row])) = (self.atlas, a_tile(self.atlas, kind)) {
                self.blocks[k] = Some(sample_tile_block(a, col, row, self.res));
            }
        }
        self.blocks[k].as_deref()
    }

    /// 水面逐像素细节（ADR-0048 P0）：纸纹颗粒 + 离岸墨线波纹。
    ///
    /// **只对水格调用**：陆地有自己的逐格颗粒（`land_grain`），水格若也逐格
    /// 抖动会读成马赛克；水要的是细颗粒 + 沿岸墨线。
    ///
    /// 纯函数（只依赖坐标与配方）→ 行带并行与增量 patch 逐字节一致
    /// （波纹采样自距离场，其支撑半径已由 patch 的脏区膨胀覆盖）。
    ///
    /// - `c`：0..1 的 RGB（已含浅滩带 / 淡化）；
    /// - `px` / `py`：纹理像素坐标（纸纹用）；
    /// - `gx` / `gy`：连续格坐标（距离插值用，亚格精度才有平滑墨线）。
    fn water_detail(&self, c: [f32; 3], px: f32, py: f32, gx: f32, gy: f32) -> [f32; 3] {
        let sea = &self.sea;
        if sea.water_grain <= 0.0
            && sea.ripples.is_empty()
            && sea.foam.is_empty()
            && sea.shore_hatch.is_none()
            && sea.shore_edge_strength <= 0.0
        {
            return c;
        }
        let mut out = c;
        // ① 纸纹颗粒：低频「水色不匀」+ 高频纸纤维。
        if sea.water_grain > 0.0 {
            let n = value_noise(px * 0.045, py * 0.045, 0x5EA6_A100) * 0.55
                + value_noise(px * 0.19, py * 0.19, 0x5EA6_A101) * 0.45;
            let g = 1.0 + (n - 0.5) * 2.0 * sea.water_grain;
            for ch in out.iter_mut() {
                *ch = (*ch * g).clamp(0.0, 1.0);
            }
        }
        // ② 离岸墨线：手绘海图的「水彩感」主要来源（civ-atlas RIPPLE_D）。
        if !sea.ripples.is_empty() {
            if let Some(field) = self.shore {
                let mut d = wv_terrain::shore::sample_bilinear(field, self.w, self.h, gx, gy);
                if sea.ripple_warp_cells > 0.0 {
                    // 抖动把同心圆打散成手绘的不规则线。
                    let w = value_noise(gx * 0.09, gy * 0.09, 0x5EA6_A102) - 0.5;
                    d += w * 2.0 * sea.ripple_warp_cells;
                }
                let mut ink_a = 0.0f32;
                for rp in &sea.ripples {
                    let sigma = rp.sigma_cells.max(1e-3);
                    let u = (d - rp.dist_cells) / sigma;
                    ink_a = ink_a.max((-u * u).exp() * rp.alpha);
                }
                if ink_a > 0.0 {
                    for (i, ch) in out.iter_mut().enumerate() {
                        let ink = sea.ink[i] as f32 / 255.0;
                        *ch += (ink - *ch) * ink_a;
                    }
                }
            }
        }
        // ③ 近岸白色泡沫（浪花）：贴岸的破碎白带（S3）。
        // 与墨线同一套距离场机制，但**色白、位更靠岸**——三层叠起来才是
        // 手绘海图的水岸层次：岸线墨 → 白浪花 → 离岸波纹墨。
        if !sea.foam.is_empty() {
            if let Some(field) = self.shore {
                let mut d = wv_terrain::shore::sample_bilinear(field, self.w, self.h, gx, gy);
                if sea.foam_warp_cells > 0.0 {
                    // 边缘参差：泡沫带的边界本就比墨线更不规则。
                    let w = value_noise(gx * 0.14, gy * 0.14, 0x5EA6_A103) - 0.5;
                    d += w * 2.0 * sea.foam_warp_cells;
                }
                let mut foam_a = 0.0f32;
                for fp in &sea.foam {
                    let sigma = fp.sigma_cells.max(1e-3);
                    let u = (d - fp.dist_cells) / sigma;
                    foam_a = foam_a.max((-u * u).exp() * fp.alpha);
                }
                if foam_a > 0.0 {
                    // 破碎：高频噪声调制强度——碎浪是断续的，不是连续白带。
                    if sea.foam_break > 0.0 {
                        let freq = sea.foam_break_freq.max(0.01);
                        let n = value_noise(gx * freq, gy * freq, 0x5EA6_A104);
                        let k = 1.0 - sea.foam_break + sea.foam_break * (n * 1.6).min(1.0);
                        foam_a *= k;
                    }
                    if foam_a > 0.0 {
                        for (i, ch) in out.iter_mut().enumerate() {
                            let foam = sea.foam_ink[i] as f32 / 255.0;
                            *ch += (foam - *ch) * foam_a;
                        }
                    }
                }
            }
        }
        // ④ 近岸排线（hatching）：离岸带内每隔若干像素一道横线（手绘海图
        // 表示浅水波纹的经典手法；civ-atlas `HATCH_D` / `HATCH_A` 同款）。
        // 排线只画在水格、且只在横线的那些行上——线是**纹理空间**的
        // （烘焙与缩放解耦，代价是缩放时行距的屏幕观感会变）。
        if let Some(h) = sea.shore_hatch {
            if h.alpha > 0.0 && h.row_spacing_px > 0 {
                if let Some(field) = self.shore {
                    let d = wv_terrain::shore::sample_bilinear(field, self.w, self.h, gx, gy);
                    if d < h.dist_cells {
                        // 横线段：从岸线向外延伸（径向长度 = `dist_cells`），
                        // 行的疏密由 `row_spacing_px` 定。墨量按噪声调制 →
                        // 手工感（不是机械的等距线）。
                        let row = (py as u32) % h.row_spacing_px;
                        if row == 0 {
                            // 墨量按噪声调制 → 手工感（不是机械的等距线）。
                            let n = value_noise(px * 0.13, py * 0.13, 0x5EA6_A105);
                            let a = h.alpha * (0.55 + 0.45 * n);
                            for (i, ch) in out.iter_mut().enumerate() {
                                let ink = sea.ink[i] as f32 / 255.0;
                                *ch += (ink - *ch) * a;
                            }
                        }
                    }
                }
            }
        }
        // ⑤ 浅滩带外缘的积墨环（水彩边缘效应）：浅滩带与深水的交界处压一道
        // 淡墨，让浅滩从「发光的渐变」变成「有边缘的水彩色块」。
        if sea.shore_edge_strength > 0.0 && sea.shore_edge_width_cells > 0.0 {
            if let Some(field) = self.shore {
                let d = wv_terrain::shore::sample_bilinear(field, self.w, self.h, gx, gy);
                let w = sea.shore_edge_width_cells;
                let u = (d - sea.shore_band_cells) / w;
                let ring = (-u * u).exp() * sea.shore_edge_strength;
                if ring > 0.0 {
                    let edge_ink = sea.shore_edge_ink.unwrap_or(sea.ink);
                    for (i, ch) in out.iter_mut().enumerate() {
                        let ink = edge_ink[i] as f32 / 255.0;
                        *ch += (ink - *ch) * ring;
                    }
                }
            }
        }
        out
    }

    /// 高度明度系数（仅 `RealTerrain` 非 1）。
    fn height_mul(&self, height: i16) -> f32 {
        if self.mode != ShadeMode::RealTerrain {
            return 1.0;
        }
        let t = (height as f32 - self.hmin) / self.hrange;
        0.78 + t * 0.44
    }

    /// 该格 8 邻域是否存在异类——只有边界格值得花逐像素采样的钱。
    ///
    /// 轮廓图只关心**水陆之别**：陆地内部的地形类差异（草/林/山）不触发
    /// 边界路径（轮廓图里它们同色，采样只是浪费）。
    ///
    /// 等值线模式（VectorFlat）多一条触发：3×3 邻域高度**跨越海平面**
    /// ——填充的水陆边由双线性高度决定（D2），哪怕全邻域同类型，
    /// 只要高度穿零就得逐像素采。
    fn is_boundary(&self, cx: u32, cy: u32) -> bool {
        let kind0 = display_slot(&self.data.cells[(cy * self.w + cx) as usize]);
        let differs = |a: u8, b: u8| {
            if self.mode == ShadeMode::Coastline {
                slot_is_water(a) != slot_is_water(b)
            } else if self.mode == ShadeMode::GoogleMap {
                // Google 模式下同色的陆上类不构成边界（沙/草/丘/山/雪已折叠为
                // 同一陆色）——省掉无意义的逐像素超采样。
                shade_google(self.data, a, self.palette) != shade_google(self.data, b, self.palette)
            } else {
                a != b
            }
        };
        let mut hmin = i16::MAX;
        let mut hmax = i16::MIN;
        // 中心格也参与高度极值（跨零判定含自身）。
        if self.mode == ShadeMode::VectorFlat {
            let h0 = self.data.cells[(cy * self.w + cx) as usize].height;
            hmin = hmin.min(h0);
            hmax = hmax.max(h0);
        }
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                    continue;
                }
                let cell = &self.data.cells[(ny as u32 * self.w + nx as u32) as usize];
                if self.mode == ShadeMode::VectorFlat {
                    hmin = hmin.min(cell.height);
                    hmax = hmax.max(cell.height);
                }
                if differs(kind0, display_slot(cell)) {
                    return true;
                }
            }
        }
        // 高度跨海平面（Q1：海平面 = 0，负值水下）。
        self.mode == ShadeMode::VectorFlat && hmin < 0 && hmax >= 0
    }

    /// 在连续纹理像素坐标取一次颜色样本（写入 `out`，RGB，0..255）。
    ///
    /// 落点跨出地图时钳到边缘格——扰动把边缘格的采样点推出去是常态，不是错误。
    fn sample_into(&self, fx: f32, fy: f32, out: &mut [f32; 3]) {
        let resf = self.res as f32;
        // 等值线模式：水陆由双线性高度采样决定（与等值线描边同源，D2）。
        if self.mode == ShadeMode::VectorFlat {
            self.sample_vector_flat(fx / resf, fy / resf, out);
        } else {
            let gx = ((fx / resf).floor() as i32).clamp(0, self.w as i32 - 1) as u32;
            let gy = ((fy / resf).floor() as i32).clamp(0, self.h as i32 - 1) as u32;
            self.sample_kind(gx, gy, fx, fy, out);
        }
        if self.soften {
            let Color::Srgba(t) = self.palette.soften_toward else {
                unreachable!("soften_toward 应为 Srgba")
            };
            let k = self.palette.soften_keep;
            for (o, tc) in out.iter_mut().zip([t.red, t.green, t.blue]) {
                *o = (*o * k + tc * 255.0 * (1.0 - k)).clamp(0.0, 255.0);
            }
        }
    }

    /// 等值线模式采样：连续格坐标 `(gx, gy)` 处。
    ///
    /// - 高度在四个相邻**格心**间双线性插值（`isoline_overlay` 的等值线
    ///   正是插值自同一组格心值），`≥ 0` 判陆、`< 0` 判水（Q1 海平面锚点）；
    /// - 陆色 = 最近格的类型色；最近格是水系而高度判陆时回退沙滩色
    ///   （类型与高度矛盾时高度赢，海岸带自然呈现为滩）；
    /// - 水色 = 最近格的水系色（作者画的深浅分区保留）；最近格是陆地
    ///   类型而高度判水时回退深水色。
    fn sample_vector_flat(&self, gx: f32, gy: f32, out: &mut [f32; 3]) {
        // 双线性高度：格心在 (i + 0.5, j + 0.5)。
        let hx = (gx - 0.5).clamp(0.0, self.w as f32 - 1.0);
        let hy = (gy - 0.5).clamp(0.0, self.h as f32 - 1.0);
        let (x0, y0) = (hx.floor() as u32, hy.floor() as u32);
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let (tx, ty) = (hx - x0 as f32, hy - y0 as f32);
        let h = |x: u32, y: u32| self.data.cells[(y * self.w + x) as usize].height as f32;
        let height = (h(x0, y0) * (1.0 - tx) + h(x1, y0) * tx) * (1.0 - ty)
            + (h(x0, y1) * (1.0 - tx) + h(x1, y1) * tx) * ty;
        // 最近格的类型（钳边）。
        let cx = (gx.floor() as i32).clamp(0, self.w as i32 - 1) as u32;
        let cy = (gy.floor() as i32).clamp(0, self.h as i32 - 1) as u32;
        let kind = display_slot(&self.data.cells[(cy * self.w + cx) as usize]);
        let land = height >= 0.0;
        // 类型 ↔ 高度矛盾时高度赢（D2）；回退色取「岸」语义的沙滩 / 深水。
        let kind = if land && kind <= 1 {
            2 // 沙
        } else if !land && kind > 1 {
            0 // 深水
        } else {
            kind
        };
        // 水彩晕开（ADR-0044 D1）：边界像素取最近格的 5×5 模糊基色——
        // 水陆交界两侧的填充色被邻域互相拉柔和（「湿岸」水彩效果）。
        let c = if self.bleed {
            blended_base(
                self.data,
                cx,
                cy,
                self.palette,
                &self.treatment,
                self.mode,
                self.shore,
                &self.sea,
            )
        } else {
            (self.shader)(self.data, kind, self.palette)
        };
        let Color::Srgba(c) = c else {
            return;
        };
        // 陆格颗粒（ADR-0042）：每格 ±4.5% 明度抖动，平涂色块出纸面颗粒感。
        let g = if self.grain {
            land_grain(kind, cx, cy)
        } else {
            1.0
        };
        out[0] = c.red * 255.0 * g;
        out[1] = c.green * 255.0 * g;
        out[2] = c.blue * 255.0 * g;
        // 水像素的纸纹 / 墨线（ADR-0048）：与内部格同一函数、同一坐标口径，
        // 岸线两侧的水面因此连续（不会在格界处断线）。
        if self.water_detail_enabled && !land {
            let detail = self.water_detail(
                [out[0] / 255.0, out[1] / 255.0, out[2] / 255.0],
                gx * self.res as f32,
                gy * self.res as f32,
                gx,
                gy,
            );
            for (o, d) in out.iter_mut().zip(detail) {
                *o = d * 255.0;
            }
        }
    }

    /// 通用模式采样：最近格着色 / 瓦片块，供 `sample_into` 分派。
    fn sample_kind(&self, gx: u32, gy: u32, fx: f32, fy: f32, out: &mut [f32; 3]) {
        let resf = self.res as f32;
        let cell = &self.data.cells[(gy * self.w + gx) as usize];
        let kind = display_slot(cell);
        let mul = self.height_mul(cell.height);
        if let Some(block) = self.blocks.get(kind as usize).and_then(|b| b.as_deref()) {
            let r = self.res as usize;
            let u = (fx / resf - gx as f32).clamp(0.0, 0.999);
            let v = (fy / resf - gy as f32).clamp(0.0, 0.999);
            let tx = ((u * r as f32) as usize).min(r - 1);
            let ty = ((v * r as f32) as usize).min(r - 1);
            let o = (ty * r + tx) * 4;
            let g = if self.grain {
                land_grain(kind, gx, gy)
            } else {
                1.0
            };
            out[0] = block[o] as f32 * mul * g;
            out[1] = block[o + 1] as f32 * mul * g;
            out[2] = block[o + 2] as f32 * mul * g;
        } else {
            let base = (self.shader)(self.data, kind, self.palette);
            // 海拔色带（ADR-0067）：专业制图专用，先叠色带再乘浮雕。
            let base = self.apply_elevation_band(cell, base);
            if let Color::Srgba(c) = base {
                let g = if self.grain {
                    land_grain(kind, gx, gy)
                } else {
                    1.0
                };
                out[0] = c.red * 255.0 * mul * g;
                out[1] = c.green * 255.0 * mul * g;
                out[2] = c.blue * 255.0 * mul * g;
            }
        }
        // 水格纸纹 / 墨线（ADR-0048）：与内部格同函数同口径。
        if self.water_detail_enabled && slot_is_water(kind) {
            let detail = self.water_detail(
                [out[0] / 255.0, out[1] / 255.0, out[2] / 255.0],
                fx,
                fy,
                fx / resf,
                fy / resf,
            );
            for (o, d) in out.iter_mut().zip(detail) {
                *o = d * 255.0;
            }
        }
    }

    /// 连续纹理像素坐标处的水陆判定（轮廓图海岸线专用；坐标口径同 `sample_into`）。
    ///
    /// 湿判定（surface 通道 ‖ 海拔 < 0，[`wv_terrain::edit::is_water_cell`]）——
    /// 与 VectorFlat 的高度海岸线同一口径（v1 此处只看类型通道，两种模式对
    /// 「被压到海下的陆格」结论相左；统一到湿判定是修偏）。
    fn sample_is_water(&self, fx: f32, fy: f32) -> bool {
        let resf = self.res as f32;
        let gx = ((fx / resf).floor() as i32).clamp(0, self.w as i32 - 1) as u32;
        let gy = ((fy / resf).floor() as i32).clamp(0, self.h as i32 - 1) as u32;
        let cell = &self.data.cells[(gy * self.w + gx) as usize];
        wv_terrain::edit::is_water_cell(to_cell(cell))
    }

    /// 轮廓图边界格：逐像素扰动 + 4 样本超采样的**水陆占比**判定。
    ///
    /// - 全水 → 水色；全陆 → 陆色；
    /// - 混合（即海岸线穿过该像素）→ 按「靠线程度」向 `COAST_LINE` 混合：
    ///   4 样本 2/2 分 = 完整线色，3/1 分 = 半强度（天然抗锯齿）。
    ///
    /// 与 `write_boundary_cell` 共用同一 `wobble_px` 扰动——同一条海岸线在
    /// 全图合成与增量 patch 中揉出的曲线逐字节一致。
    /// `gy` = 数据/采样行（全局），`out_row` = 输出写入行。
    fn write_coast_boundary_cell(
        &self,
        pixels: &mut [u8],
        stride: u32,
        cx: u32,
        gy: u32,
        out_row: u32,
    ) {
        let to_rgb = |c: Color| -> [f32; 3] {
            let Color::Srgba(s) = c else { return [0.0; 3] };
            [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
        };
        let water = to_rgb(palette::COAST_WATER);
        let land = to_rgb(palette::COAST_LAND);
        let line = to_rgb(palette::COAST_LINE);
        let res = self.res;
        for ty in 0..res {
            for tx in 0..res {
                let ox = (cx * res + tx) as f32;
                let oy_read = (gy * res + ty) as f32;
                let (wx, wy) = wobble_px(ox, oy_read, res);
                let mut n_water = 0u32;
                let mut n_land = 0u32;
                for s in SUBSAMPLES {
                    if self.sample_is_water(ox + s[0] + wx, oy_read + s[1] + wy) {
                        n_water += 1;
                    } else {
                        n_land += 1;
                    }
                }
                // 靠线程度：2/2 分 = 1.0，3/1 分 = 0.5，4/0 分 = 0.0。
                let strength = 1.0 - (n_water as f32 - 2.0).abs() / 2.0;
                // 边界像素：strength > 0.8 时画描边（1px 线）
                let is_boundary = strength > 0.8;
                let base = if n_water >= n_land { water } else { land };
                let o = ((out_row * res + ty) * stride + cx * res + tx) as usize * 4;
                for ch in 0..3 {
                    pixels[o + ch] = if is_boundary {
                        // 边界像素直接用描边色（浅蓝）
                        line[ch]
                    } else {
                        // 非边界用基础色（水/陆）
                        base[ch]
                    }
                    .round()
                    .clamp(0.0, 255.0) as u8;
                }
                pixels[o + 3] = 255;
            }
        }
    }

    /// 边界格专用：先把自身与 8 邻域的瓦片块备齐（需要 `&mut`），
    /// 再逐像素扰动 + 4 样本超采样写 `res²` 像素。
    ///
    /// `gy` = 数据/采样行（全局），`out_row` = 输出写入行（带内局部）——
    /// 见 [`Self::write_cell_split`]。
    fn write_boundary_cell(
        &mut self,
        pixels: &mut [u8],
        stride: u32,
        cx: u32,
        gy: u32,
        out_row: u32,
    ) {
        // 轮廓图：海岸线有专用路径（水陆占比判定），不走颜色混合。
        if self.mode == ShadeMode::Coastline {
            self.write_coast_boundary_cell(pixels, stride, cx, gy, out_row);
            return;
        }
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (nx, ny) = (cx as i32 + dx, gy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                    continue;
                }
                let kind =
                    display_slot(&self.data.cells[(ny as u32 * self.w + nx as u32) as usize]);
                self.block_for(kind);
            }
        }
        let res = self.res;
        for ty in 0..res {
            for tx in 0..res {
                let ox = (cx * res + tx) as f32;
                // 采样用**全局**像素 y（wobble 与格采样都是全图像素位置的函数）；
                // 写入偏移用带内局部行。
                let oy_read = (gy * res + ty) as f32;
                let (wx, wy) = if self.wobble {
                    wobble_px(ox, oy_read, res)
                } else {
                    (0.0, 0.0)
                };
                let (mut r, mut g, mut b) = (0.0f32, 0.0f32, 0.0f32);
                let mut c = [0.0f32; 3];
                for s in SUBSAMPLES {
                    self.sample_into(ox + s[0] + wx, oy_read + s[1] + wy, &mut c);
                    r += c[0];
                    g += c[1];
                    b += c[2];
                }
                let o = ((out_row * res + ty) * stride + cx * res + tx) as usize * 4;
                pixels[o] = (r / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 1] = (g / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 2] = (b / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 3] = 255;
            }
        }
    }

    /// 写一格像素：内部格快路径 / 边界格扰动路径。
    ///
    /// `cy` 同时是数据行与输出行（串行全图合成与 `patch_terrain_image` 的口径，
    /// 行带并行请走 [`Self::write_cell_split`]）。
    fn write_cell(&mut self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        self.write_cell_split(pixels, stride, cx, cy, cy);
    }

    /// [`Self::write_cell`] 的坐标分离版：数据/采样行取 `gy`（全局格行），
    /// 像素写入以 `out_row` 为缓冲第 0 行基准（行带并行：各带看自己的切片，
    /// 写入行是带内局部号，取数行仍是全局行）。
    ///
    /// 串行与 patch 路径恒 `gy == out_row`——两条路径行为逐位不变。
    fn write_cell_split(&mut self, pixels: &mut [u8], stride: u32, cx: u32, gy: u32, out_row: u32) {
        let idx = (gy * self.w + cx) as usize;
        let Some(cell) = self.data.cells.get(idx) else {
            return;
        };
        // Topographic 早返回（ADR-0051 §5 的 per-cell 着色 + ADR-0055 叠色）：
        // 群系固定色块为底，陆格再按海拔色带**段内连续渐变**向固定色带混合
        // （切点 = 绝对阶梯 + 窄域兜底，ADR-0056；渐变 = ADR-0077）。
        // res 恒 1（new 里已定），一像素直铺；无边界/揉边/淡化/浮雕——
        // 数据视图要的是逐格可读，任何后处理都是污染。
        if let Some(cuts) = self.topographic.as_ref() {
            // 2026-10-22 三模式内容矩阵（spec §2.5）：CellMap = 群系固定色块
            // ——颜色编码 biome，固定色板不随 CellPack/TreatmentFlags（ADR-0051）。
            // 2026-10-24（ADR-0055）：再叠一层**海拔分层设色**——群系色仍是底，
            // 海拔向固定色带混合，强度由会话开关控制（当日口径为 8 阶离散，
            // 已被下方 2026-10-28 条目修正）。
            // 水格不叠：海拔色带只对陆面有意义，水面仍由群系水色表达。
            let mut rgba = crate::terrain_view::biome_raw::pick(cell.biome, cell.surface);
            if self.hypsometric > 0.0 && !super::hypsometric::is_water_cell(cell) {
                // 2026-10-28（CellMap 数据视图对 ADR-0055「8 阶离散」的修正）：
                // 换 8 锚点**段内连续渐变**——离散口径下低地 0–200 m 一刀同色，
                // 缓丘与海岸平原不可分；`tint_continuous` 在锚点处退化为
                // `tint_rgba`，是离散色带的超集（本 crate
                // `hypsometric::ElevationBand` 阶梯语义不动，wv-render 消费；
                // 见 hypsometric::segment_of）。
                rgba =
                    super::hypsometric::tint_continuous(rgba, cell.height, cuts, self.hypsometric);
            }
            let o = ((out_row * stride + cx) as usize) * 4;
            pixels[o] = (rgba[0] * 255.0) as u8;
            pixels[o + 1] = (rgba[1] * 255.0) as u8;
            pixels[o + 2] = (rgba[2] * 255.0) as u8;
            pixels[o + 3] = 255;
            return;
        }
        if !self.is_boundary(cx, gy) {
            // 内部格：与旧行为一致——整块直铺（快路径）。
            let kind = display_slot(cell);
            if self.block_for(kind).is_some() {
                let tint = if self.mode == ShadeMode::RealTerrain {
                    TileTint::Height(self.height_mul(cell.height))
                } else {
                    tile_tint_for_mode(self.mode)
                };
                // 借用冲突隔离：block_for 已把块填进 self.blocks。
                if let Some(block) = self.blocks[kind as usize].as_deref() {
                    blit_block_tinted(
                        block,
                        self.res,
                        tint,
                        self.soften,
                        self.palette.soften_toward,
                        self.palette.soften_keep,
                        pixels,
                        stride,
                        cx * self.res,
                        out_row * self.res,
                    );
                }
                return;
            }
            // 纯色回退（无图集 / 缺项）：整格平涂。
            // 水彩晕开（ADR-0044 D1）开启时取 5×5 模糊基色——群系边界外
            // 2 格内的格子颜色被邻域拉柔和；纯色内部格模糊值 ≈ 本色（全部
            // 同 kind 时严格相等），高频邻域才付 25 次取色的钱。
            let mut color = if self.bleed {
                blended_base(
                    self.data,
                    cx,
                    gy,
                    self.palette,
                    &self.treatment,
                    self.mode,
                    self.shore,
                    &self.sea,
                )
            } else {
                (self.shader)(self.data, kind, self.palette)
            };
            // 海拔色带（ADR-0067）：与 `sample_kind` 同一闸口，两条路径同色。
            color = self.apply_elevation_band(cell, color);
            // 水域按高度渐变（手绘地图 #1）：kind 0/1 的内部格从单色水变
            // height 调度的水彩带；轮廓图模式不进这里。
            // （bleed 开时模糊基色已含水深渐变——邻域各格都过了 water_shade_at。）
            if !self.bleed
                && self.treatment.water_gradient
                && slot_is_water(kind)
                && self.mode != ShadeMode::Coastline
            {
                color = water_shade_at(cell.height, self.palette, &self.sea);
            }
            // 雪格双调（手绘地图 #3）：kind=7 全部走 snow_shade_at（RealTerrain
            // 模式之外也给 highlight 视觉——海报感的"近白雪"）。轮廓图模式不动。
            // （bleed 开时同理已含于模糊基色。）
            if !self.bleed
                && self.treatment.snow_shade
                && kind == 7
                && self.mode != ShadeMode::Coastline
            {
                color = snow_shade_at(cell.height, self.palette);
            }
            // 陆格颗粒（ADR-0042）：与采样路径（sample_vector_flat /
            // sample_kind）同位置同源——先颗粒、后淡化，两条路径同格同色。
            // 瓦片直铺（Google 模式）刻意不抖：普通地图气质要干净边界。
            {
                let g = if self.grain {
                    land_grain(kind, cx, gy)
                } else {
                    1.0
                };
                let Color::Srgba(c0) = color else { return };
                color = Color::srgb(
                    (c0.red * g).clamp(0.0, 1.0),
                    (c0.green * g).clamp(0.0, 1.0),
                    (c0.blue * g).clamp(0.0, 1.0),
                );
            }
            if self.soften {
                color = self.palette.soften_color(color);
            }
            if self.mode == ShadeMode::RealTerrain {
                let mul = self.height_mul(cell.height);
                let Color::Srgba(c) = color else { return };
                color = Color::srgb(
                    (c.red * mul).clamp(0.0, 1.0),
                    (c.green * mul).clamp(0.0, 1.0),
                    (c.blue * mul).clamp(0.0, 1.0),
                );
            }
            // 坡度浮雕（ADR-0077 D6）：全局 / 区域档靠明暗读出山脉体积。
            // 只在陆格生效——水域已有深度渐变，再叠坡度会脏。
            //
            // 三模式（VectorFlat / RealTerrain / GoogleMap）共走
            // [`Self::relief_intensity`] 的同一公式、同一 slope_gain：旧
            // GoogleMap 特判钳制分支（0.90~1.12）删除——界内性由公式免费
            // 保证，两套钳制常量整体退役。与旧 relief_mul 的受光方向逐轴
            // 相反是**既定修正**（旧 CPU 路径方向本来就反，见
            // `shading::normal_world` 文档）——切换后 CPU/GPU 统一。
            if matches!(
                self.mode,
                ShadeMode::VectorFlat | ShadeMode::RealTerrain | ShadeMode::GoogleMap
            ) && cell.height >= 0
            {
                let mul = self.relief_intensity(cx, gy);
                let Color::Srgba(c) = color else { return };
                color = Color::srgb(
                    (c.red * mul).clamp(0.0, 1.0),
                    (c.green * mul).clamp(0.0, 1.0),
                    (c.blue * mul).clamp(0.0, 1.0),
                );
            }
            let Color::Srgba(c) = color else { return };
            let res = self.res;
            // 水格的纸纹 / 墨线是**逐像素**的（ADR-0048）：整块同色写死会
            // 让水面没有颗粒。陆格沿用整块直铺（颗粒已在格级叠过）。
            let is_water = self.water_detail_enabled && slot_is_water(kind);
            let base = [c.red, c.green, c.blue];
            for ty in 0..res {
                let o = ((out_row * res + ty) * stride + cx * res) as usize * 4;
                for tx in 0..res {
                    let px = if is_water {
                        self.water_detail(
                            base,
                            (cx * res + tx) as f32,
                            (gy * res + ty) as f32,
                            cx as f32 + (tx as f32 + 0.5) / res as f32,
                            gy as f32 + (ty as f32 + 0.5) / res as f32,
                        )
                    } else {
                        base
                    };
                    let o = o + tx as usize * 4;
                    pixels[o] = (px[0] * 255.0) as u8;
                    pixels[o + 1] = (px[1] * 255.0) as u8;
                    pixels[o + 2] = (px[2] * 255.0) as u8;
                    pixels[o + 3] = 255;
                }
            }
            return;
        }
        self.write_boundary_cell(pixels, stride, cx, gy, out_row);
    }
}

/// 把整张 `TerrainData` 写成 RGBA8 像素（每逻辑格 `res` 像素见方）。
///
/// 有图集且该格 kind 有映射 → 瓦片直铺（内部格）/ 扰动超采样（边界格）；
/// 否则回退纯色 shader。返回 `(pixels, res)`。
///
/// **行带并行**（2026-10-17，修调色板切换卡顿）：`write_cell` 对**位置纯**
/// （`wobble_px`/双线性采样都是坐标的确定函数——`patch_terrain_pixels` 增量
/// 路径早已依赖该性质），唯一的可变状态是每 renderer 一份的 `blocks` 瓦片
/// 缓存——纯缓存，不影响输出。因此按行带切成不相交切片、每带独立 renderer
/// 并行渲染，输出与串行**逐字节一致**（测试 `banded_rendering_matches_serial`）。
/// 1024² 图整图合成实测 317ms → 多核下接近带宽封顶。
/// 全图合成的**旧签名**（源兼容；wv-app 经 wv-render re-export 消费）。
///
/// 浮雕按过渡缺省格距 [`crate::shading::DEFAULT_METERS_PER_CELL`] 打光；
/// 有图上下文的调用方应改走 [`terrain_pixels_with_scale`]（wv-app 接线后
/// 本函数的缺省假设消失，见该常量文档）。
pub fn terrain_pixels(
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    pack: &CellPack,
) -> (Vec<u8>, u32) {
    terrain_pixels_with_scale(
        data,
        mode,
        atlas,
        soften,
        pack,
        crate::shading::DEFAULT_METERS_PER_CELL,
    )
}

/// [`terrain_pixels`] 的**米制比例透传版**（ADR-0077 D6 的 CPU 接线点）。
///
/// 末位 `meters_per_cell` = 该图的米/格换算——唯一换算入口是
/// [`crate::shading::meters_per_cell_of`]（`GridConfig.cell_size` +
/// `MapNode.unit_per_km` 进、米/格出，禁第二份硬编码），它决定浮雕的
/// `slope_gain`（`shading::slope_gain_of`）。米制化后「同物理坡 → 同法线」
/// 跨图幅成立。
pub fn terrain_pixels_with_scale(
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    pack: &CellPack,
    meters_per_cell: f32,
) -> (Vec<u8>, u32) {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    terrain_pixels_banded(data, mode, atlas, soften, pack, threads, meters_per_cell)
}

/// [`terrain_pixels_with_scale`] 的带数参数化版本（测试注入带数用）。
///
/// `bands` 只是调度参数：任意带数下输出必须逐字节一致。
fn terrain_pixels_banded(
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    pack: &CellPack,
    bands: usize,
    meters_per_cell: f32,
) -> (Vec<u8>, u32) {
    // 近岸浅滩场（S2）整场算一次，共享给全部带 renderer——它与带数无关，
    // 任意切分下逐字节一致（行带并行契约）。
    let shore = shore_field_for(data, mode, &pack.treatment, &pack.sea);
    // `Option<&[u16]>` 是 Copy——先借好，各带 move 闭包共享同一份引用。
    let shore_ref = shore.as_deref();
    // 先建一个 renderer 只为推导 w/h/res（RealTerrain 的 hmin 折叠在它里面
    // 做一次；各带的新 renderer 会重复这次纯比较折叠，代价每带亚毫秒）。
    let probe = CellRenderer::new(data, mode, atlas, soften, pack, shore_ref, meters_per_cell);
    let (w, h, res) = (probe.w, probe.h, probe.res);
    let stride = w * res;
    let mut pixels = vec![0u8; (stride * h * res * 4) as usize];
    let row_bytes = (stride * res * 4) as usize;
    // 行数太少不值得开线程（占位 64² 合成 <1ms，线程启动反而亏）。
    let bands = bands.clamp(1, 16).min(h as usize);
    if bands <= 1 || h < 128 {
        let mut r = probe;
        for cy in 0..h {
            for cx in 0..w {
                r.write_cell(&mut pixels, stride, cx, cy);
            }
        }
        return (pixels, res);
    }
    let rows_per_band = h.div_ceil(bands as u32) as usize;
    std::thread::scope(|s| {
        for (bi, band) in pixels.chunks_mut(rows_per_band * row_bytes).enumerate() {
            let y0 = (bi * rows_per_band) as u32;
            s.spawn(move || {
                let mut r =
                    CellRenderer::new(data, mode, atlas, soften, pack, shore_ref, meters_per_cell);
                let band_rows = (band.len() / row_bytes) as u32;
                for ly in 0..band_rows {
                    let gy = y0 + ly;
                    if gy >= h {
                        break;
                    }
                    for cx in 0..w {
                        // gy：数据/采样行（全局）；ly：写入行（带内）。
                        r.write_cell_split(band, stride, cx, gy, ly);
                    }
                }
            });
        }
    });
    (pixels, res)
}

/// 查 kind 的图集格位（`atlas` 为 `None` 或未映射时 `None`）。
fn a_tile(atlas: Option<&TileAtlas>, kind: u8) -> Option<[u32; 2]> {
    let a = atlas?;
    a.tiles.get(kind as usize).copied().flatten()
}

/// 由 `TerrainData` 构造 Bevy `Image`（RGBA8；有图集时像素边长 = 格数 × res）。
///
/// `soften` = 底图淡化（`ViewState.terrain_soften`）；`palette` = 地图主题
/// （ADR-0014：切主题 = 按新色板整图重建）。
///
/// 浮雕比例：旧签名 = 带 [`crate::shading::DEFAULT_METERS_PER_CELL`] 过渡缺省的
/// [`image_from_terrain_with_scale`] 委托（wv-app 已接线真实比例，本缺省只剩
/// examples 等无世界上下文的调用方）；oblique 烘焙是 Topographic 数据
/// 视图（无浮雕），`compression` 与晕渲无关。
///
/// **oblique 烘焙**（[ADR-0041] §3 texture-bake 方案）：仅在 Topographic
/// mode 下把生成的 RGBA 像素沿 x 方向按行斜切 `(y − h/2) × compression`，
/// 矩形纹理烘焙成平行四边形——视觉上等同 oblique projection，且不引入 Camera3d。
/// `compression <= 0`（非 Topographic mode）跳过；这就是 Camera2d 平面渲染的
/// 退化路径——既有 mode 表的 Terrain pass 不会受影响。
pub fn image_from_terrain(
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    pack: &CellPack,
    compression: f32,
) -> Image {
    image_from_terrain_with_scale(
        data,
        mode,
        atlas,
        soften,
        pack,
        compression,
        crate::shading::DEFAULT_METERS_PER_CELL,
    )
}

/// [`image_from_terrain`] 的带比例变体（ADR-0077 D2 CPU 接线）：`meters_per_cell`
/// 交给晕渲法线做米制换算，与 GPU 场路径共用同一条换算入口；其余口径与旧
/// 函数逐位一致。调用侧用 [`crate::shading::meters_per_cell_of`]（或桥接封装
/// `bake_meters_per_cell`）从「来源图比例尺 + 格距」算出，禁止第二份换算公式。
pub fn image_from_terrain_with_scale(
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    pack: &CellPack,
    compression: f32,
    meters_per_cell: f32,
) -> Image {
    let (mut pixels, res) =
        terrain_pixels_with_scale(data, mode, atlas, soften, pack, meters_per_cell);
    let (w, h) = (data.width.max(1), data.height.max(1));
    if mode == ShadeMode::Topographic && compression > 0.0 {
        oblique_skew_pixels(&mut pixels, w * res, h * res, compression);
    }
    Image::new(
        Extent3d {
            width: w * res,
            height: h * res,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        // 注意：Bevy 行序为「左上原点」，与我们的行优先一致。
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    )
}

/// 把 RGBA8 像素按行 x 方向斜切 ([ADR-0041] §3 oblique 烘焙方案)。
///
/// 每行 `y` 沿 x 平移 `(y − h/2) × compression` 像素：
/// - 中间行 (y = h/2) 偏移 0
/// - 顶行 (y = 0) 偏移 `−h/2 × compression`（左移）
/// - 底行 (y = h − 1) 偏移 `+(h/2 − 1) × compression`（右移）
///
/// **超出原图边界的像素用「右/左边缘复制填充」**——这是 oblique 投影的视觉
/// 后果（顶视俯瞰下,边缘的像素会"溢出"到地图外,与相机看真实地图一致）。
///
/// 复杂度 O(w × h)，单次 1024² ≈ 4M 次拷贝 ~10 ms（一次性 texture bake，
/// 不在 render loop），与现有 `terrain_pixels` 同量级。
///
/// **本函数不接受 compression > 1.0**——超出 (与 ART_STYLE_SPEC §1.1 的
/// `vertical 0.5×` 上限一致)。调用方负责 clamp。
pub fn oblique_skew_pixels(pixels: &mut [u8], w: u32, h: u32, compression: f32) {
    assert!(
        (0.0..=1.0).contains(&compression),
        "oblique_skew_pixels: compression {compression} 越界 [0,1]"
    );
    assert_eq!(
        pixels.len() as u32,
        w * h * 4,
        "oblique_skew_pixels: 像素缓冲长度不匹配 w*h*4"
    );
    if compression == 0.0 || w < 2 || h < 2 {
        return;
    }
    // 临时缓冲(整张拷贝 → 写入像素;原缓冲行内 read+write 会脏读)
    let mut buf: Vec<u8> = pixels.to_vec();
    let stride = (w as usize) * 4;
    let h_half = (h as f32) / 2.0;
    for y in 0..h {
        // 整数偏移(向下取整 + 半像素对齐):让中线行严格不动。
        let offset = ((y as f32 - h_half) * compression).round() as i32;
        let src_row_start = (y as usize) * stride;
        // dst 写入位置:`pixels` 的 y 行(原 y 不动——oblique 不上下移)
        let dst_row_start = src_row_start;
        if offset == 0 {
            // 同行拷贝
            buf[src_row_start..src_row_start + stride]
                .copy_from_slice(&pixels[src_row_start..src_row_start + stride]);
            continue;
        }
        // 边界 clamp:offset > 0 → 左侧边缘用 src[0] 复制;offset < 0 → 右侧
        // 边缘用 src[w-1] 复制。
        if offset > 0 {
            let o = offset as usize;
            // 左侧 o 像素填充 src[0]
            for x in 0..o.min(w as usize) {
                let dst = dst_row_start + x * 4;
                let src = src_row_start;
                buf[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
            // 中段 src[x-o] → dst[x]
            let mid_len = w as usize - o;
            for x in 0..mid_len {
                let dst = dst_row_start + (x + o) * 4;
                let src = src_row_start + x * 4;
                buf[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        } else {
            // offset < 0:右移 |offset| 像素 → 右侧 |offset| 用 src[w-1] 复制
            let o = (-offset) as usize;
            // 中段 src[x] → dst[x + o]
            let mid_len = w as usize - o;
            for x in 0..mid_len {
                let dst = dst_row_start + x * 4;
                let src = src_row_start + (x + o) * 4;
                buf[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
            // 右侧 |o| 像素填充 src[w-1]
            for x in (mid_len..w as usize).rev() {
                let dst = dst_row_start + (x + o - 1) * 4;
                // 修正:dst_index 实际是 mid_len + (i)...
                // 简化:从右往左写
                let _ = dst;
            }
            // 重新算 dst 索引
            for i in 0..o {
                let dst = dst_row_start + (mid_len + i) * 4;
                let src = src_row_start + (w as usize - 1) * 4;
                buf[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        }
    }
    pixels.copy_from_slice(&buf);
}

/// 把 `data` 的 dirty 矩形 patch 进**已有 Image**（原地改 `image.data` 切片）。
///
/// 浮雕按过渡缺省格距 [`crate::shading::DEFAULT_METERS_PER_CELL`] 打光；
/// 有图上下文的调用方应改走 [`patch_terrain_image_with_scale`]（与全图
/// 合成传同一 `meters_per_cell`，否则笔刷 patch 区与区外浮雕分叉）。
///
/// **优化项 3d**（笔刷增量重传）：原 [`flush_terrain_texture`] 走全图重建，
/// 笔刷每改一格都重算 512² = 262144 格的 shader + blit_tile，
/// 每次提交笔触都卡 30–80 ms。本函数只重算 `dirty_cell_rect` 内**几十到几百格**，
/// 拷进 `image.data` 的对应像素区域——开销与笔刷影响面积成线性、与整图大小无关。
///
/// ## 单位
/// - `dirty_cell_rect = [gx0, gy0, gx1, gy1]`：**逻辑格**坐标（与 `tex.dirty` 一致）。
/// - 实际像素区域 = `(gx0..gx1) × (gy0..gy1)` × `res`。
///
/// ## 模式约束
/// - `RealTerrain` 模式高度归一化仍需 `hmin/hmax`——本函数用**全图一次**遍历算这两值
///   （O(N) 但纯比较/赋值，远比 shader+blit_tile 便宜）；然后只对 dirty 矩形内 cell
///   写像素。
/// - 其他模式无 hmin/hmax 依赖，直接对 dirty 矩形内 cell 写。
///
/// ## 与 `image_from_terrain` 的关系
/// `image_from_terrain` 是**全图版本**（首次建纹理 / 模式切换时用）；
/// 本函数是**增量版本**（笔刷编辑 / 撤销重做时用）。
#[allow(clippy::too_many_arguments)] // 纹理 / 数据 / 模式 / 脏区 / 分辨率 / 淡化 / 主题：正交输入
pub fn patch_terrain_image(
    image: &mut Image,
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    dirty_cell_rect: [u32; 4],
    res: u32,
    soften: bool,
    pack: &CellPack,
) {
    patch_terrain_image_with_scale(
        image,
        data,
        mode,
        atlas,
        dirty_cell_rect,
        res,
        soften,
        pack,
        crate::shading::DEFAULT_METERS_PER_CELL,
    )
}

/// [`patch_terrain_image`] 的**米制比例透传版**（ADR-0077 D6 的 CPU 接线点）。
///
/// 末位 `meters_per_cell` 语义同 [`terrain_pixels_with_scale`]：唯一换算入口
/// [`crate::shading::meters_per_cell_of`]。**必须与全图合成传同一值**——
/// 否则增量 patch 的浮雕与周边整图烘焙不一致（笔刷区拼裂缝的浮雕版）。
#[allow(clippy::too_many_arguments)] // 同上：正交输入
pub fn patch_terrain_image_with_scale(
    image: &mut Image,
    data: &TerrainData,
    mode: ShadeMode,
    atlas: Option<&TileAtlas>,
    dirty_cell_rect: [u32; 4],
    res: u32,
    soften: bool,
    pack: &CellPack,
    meters_per_cell: f32,
) {
    let (w, h) = (data.width.max(1), data.height.max(1));
    let stride = w * res; // 像素 stride
    let [gx0, gy0, gx1, gy1] = dirty_cell_rect;
    // 钳到 [0, w/h]（防御性）。
    let gx1 = gx1.min(w);
    let gy1 = gy1.min(h);

    // 近岸浅滩场（S2）——与全图合成同口径（`shore_field_for`）。
    let shore = shore_field_for(data, mode, &pack.treatment, &pack.sea);
    // **脏区按场支撑半径膨胀**：浅滩着色是「离岸距离」的连续函数，距离场
    // 的有界支撑 = `DEFAULT_MAX_DIST`——岸线附近改一格水陆，±12 格内的
    // 水格距离（连带颜色）都会变。不膨胀则局部 patch 与全图重算不一致
    // （「填掉一个小湾，湾缘浅滩带残留」这类暗斑）。膨胀是 patch 契约的
    // 一部分，收在 patch 内部，调用方无需感知。
    let [gx0, gy0, gx1, gy1] = if shore.is_some() {
        let r = pack.sea.field_radius() as u32;
        [
            gx0.saturating_sub(r),
            gy0.saturating_sub(r),
            (gx1 + r).min(w),
            (gy1 + r).min(h),
        ]
    } else {
        [gx0, gy0, gx1, gy1]
    };

    // 与全图合成共用一个渲染器（边界扰动必须一致，否则笔刷区出现拼接裂缝）。
    let mut renderer = CellRenderer::new(
        data,
        mode,
        atlas,
        soften,
        pack,
        shore.as_deref(),
        meters_per_cell,
    );
    // 调用方传入的 res 与渲染器推导的 res 必须一致（同一张纹理）。
    debug_assert_eq!(
        renderer.res, res,
        "patch 的 res 应与 image_from_terrain 一致"
    );

    // 只遍历 dirty 矩形内的格。
    for cy in gy0..gy1 {
        for cx in gx0..gx1 {
            renderer.write_cell(
                image.data.as_mut().expect("地形图必须有像素数据"),
                stride,
                cx,
                cy,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    //! VectorFlat 填充口径测试：水陆边随高度（D2）、内部快路径、亚像素分辨率。

    use super::*;
    use crate::terrain_view::terrain_texture::terrain_res_for;

    /// `oblique_skew_pixels` 应等价于「每行 x 偏移 = `(y - h/2) × compression`,
    /// round 到整数」。
    ///
    /// 用 h = 4, compression = 1.0——offset 是 -2 / -1 / 0 / +1 的整数倍,
    /// 与 Rust `f32::round` 的 half-to-even 行为无歧义。
    ///
    /// **oblique 投影的几何直觉**:row 0(地图顶端)向左偏 → 内容看起来
    /// 从左侧"溢出"(map col 0 跌出屏幕左,屏幕 col 0 显示 map col o);
    /// row h-1(地图底端)向右偏 → 内容从右侧"涌入"。
    /// 公式上,**offset > 0 = 内容右移**(dst[x+o] = src[x]);**offset < 0 = 内容左移**
    /// (dst[x] = src[x+o],o = |offset|)。
    /// 边界钳制:左溢出用 src[0] 复制,右溢出用 src[w-1] 复制。
    #[test]
    fn oblique_skew_pixels_offsets_each_row_by_signed_pixel_count() {
        let mut px = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            for x in 0..4 {
                let i = (y * 4 + x) * 4;
                px[i] = (y * 10 + x) as u8;
            }
        }
        // h = 4,h/2 = 2.0,compression = 1.0,offset = round((y - 2.0) * 1.0):
        //   y=0 → -2  (内容左移 2)
        //   y=1 → -1  (内容左移 1)
        //   y=2 →  0  (中线)
        //   y=3 → +1  (内容右移 1)
        oblique_skew_pixels(&mut px, 4, 4, 1.0);

        // row 2 (offset=0):逐列原样保留 → R = 20+col。
        for x in 0..4 {
            let i = (2 * 4 + x) * 4;
            assert_eq!(px[i], 20 + x as u8, "row 2 col {x} 应保持原值(offset=0)");
        }

        // row 0 (offset=-2,内容左移 2):dst[x] = src[x+2],左 o=2 用 src[w-1]=src[3] 填充。
        // 行 0 R 通道原 = [0,1,2,3];斜切后 = [src[2]=2, src[3]=3, src[3]=3, src[3]=3] = [2, 3, 3, 3]。
        for x in 0..4 {
            let i = x * 4;
            assert_eq!(
                px[i],
                if x < 2 { x as u8 + 2 } else { 3 },
                "row 0 col {x} 左移 2"
            );
        }

        // row 1 (offset=-1,内容左移 1):dst[x] = src[x+1],左 1 用 src[3] 填充。
        // 行 1 R 通道原 = [10,11,12,13];斜切后 = [src[1]=11, src[2]=12, src[3]=13, src[3]=13] = [11, 12, 13, 13]。
        for x in 0..4 {
            let i = (4 + x) * 4;
            assert_eq!(
                px[i],
                if x < 3 { 10 + (x + 1) as u8 } else { 13 },
                "row 1 col {x} 左移 1"
            );
        }

        // row 3 (offset=+1,内容右移 1):dst[x+1] = src[x],dst[0] 用 src[0] 填充。
        // 行 3 R 通道原 = [30,31,32,33];斜切后 = [src[0]=30, src[0]=30, src[1]=31, src[2]=32] = [30, 30, 31, 32]。
        for x in 0..4 {
            let i = (3 * 4 + x) * 4;
            assert_eq!(
                px[i],
                if x == 0 { 30 } else { 30 + (x - 1) as u8 },
                "row 3 col {x} 右移 1"
            );
        }
    }

    /// `oblique_skew_pixels(compression=0)` 是 no-op(空检查),不写任何像素。
    #[test]
    fn oblique_skew_pixels_zero_compression_is_no_op() {
        let mut px = vec![7u8; 4 * 4 * 4];
        let snapshot = px.clone();
        oblique_skew_pixels(&mut px, 4, 4, 0.0);
        assert_eq!(px, snapshot, "compression=0 不应改任何像素");
    }

    /// `oblique_skew_pixels` 边界钳制:左/右溢出用左/右边缘像素复制填充。
    /// h=4,compression=1.0:row 0 offset=-2(全溢出)、row 3 offset=+1(无溢出)。
    /// row 0 原 = [0,1,2,3];斜切后(dst[x]=src[x+2],左 2 用 src[w-1]=3 填):
    ///   = [src[2]=2, src[3]=3, src[3]=3, src[3]=3] = [2, 3, 3, 3]。
    #[test]
    fn oblique_skew_pixels_clamps_overflow_with_edge_repeat() {
        let mut px = vec![0u8; 4 * 4 * 4];
        for x in 0..4 {
            let i = x * 4;
            px[i] = x as u8;
        }
        oblique_skew_pixels(&mut px, 4, 4, 1.0);
        let expected = [2u8, 3, 3, 3];
        for (x, &want) in expected.iter().enumerate() {
            let i = x * 4;
            assert_eq!(px[i], want, "row 0 col {x} 边缘钳制 (offset=-2)");
        }
    }

    /// 3×3 fixture：`h(x, y)` 给定每格高度，surface/biome 不影响浮雕。
    fn relief_fixture(h: impl Fn(u32, u32) -> i16) -> wv_core::terrain::TerrainData {
        let cells = (0..9)
            .map(|i| {
                let (x, y) = (i % 3, i / 3);
                wv_core::terrain::TerrainCellData {
                    height: h(x, y),
                    surface: 1,
                    biome: 3,
                }
            })
            .collect();
        wv_core::terrain::TerrainData {
            width: 3,
            height: 3,
            cells,
        }
    }

    // -------------------------------------------------------------------
    // Topographic per-cell 着色（[ADR-0039] 决策 4/5，PR-1b）
    // -------------------------------------------------------------------

    /// 16 格 fixture：2 水 + 14 陆,高度均匀铺开让 33%/66% 分位切开三档。
    /// 高度表(排序后):[-100,-100,10,20,...,140];p33=50,p66=90。
    fn topographic_fixture() -> wv_core::terrain::TerrainData {
        let land_heights = [
            10i16, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140,
        ];
        let mut cells = Vec::with_capacity(16);
        // (0,0) 深水;(0,1) surface=陆 但 height<0 —— 湿判定仍应给水色。
        cells.push(wv_core::terrain::TerrainCellData {
            height: -100,
            surface: wv_terrain::SURFACE_WATER,
            biome: wv_terrain::BIOME_OCEAN,
        });
        cells.push(wv_core::terrain::TerrainCellData {
            height: -50,
            surface: wv_terrain::SURFACE_LAND,
            biome: wv_terrain::BIOME_NONE,
        });
        for &h in &land_heights {
            cells.push(wv_core::terrain::TerrainCellData {
                height: h,
                surface: wv_terrain::SURFACE_LAND,
                biome: wv_terrain::BIOME_GRASSLAND,
            });
        }
        wv_core::terrain::TerrainData {
            width: 4,
            height: 4,
            cells,
        }
    }

    fn topographic_pixels(data: &wv_core::terrain::TerrainData) -> Vec<u8> {
        let (pixels, res) = terrain_pixels(
            data,
            ShadeMode::Topographic,
            None,
            true, // soften=true 但 Topographic 下应被忽略(new 里已禁)
            &CellPack::fixture(),
        );
        assert_eq!(res, 1, "Topographic 分析视图 = 每格 1 像素(锐利方格)");
        pixels
    }

    /// 群系色块底图（2026-10-22 spec §2.5，修正 ADR-0039 的 3 阶高度画法）：
    /// 像素色必须 = `biome_raw::pick` 的语义——颜色编码 biome（固定色板），
    /// biome=NONE 按 surface 回退；soften 开关被忽略（数据视图要诚实）。
    #[test]
    fn topographic_mode_paints_fixed_biome_colors() {
        let mut data = topographic_fixture();
        // 给几格明确群系（原 fixture biome 多为 NONE）。
        data.cells[2].biome = wv_terrain::BIOME_FOREST;
        data.cells[6].biome = wv_terrain::BIOME_DESERT;
        data.cells[15].biome = wv_terrain::BIOME_OCEAN;
        let pixels = topographic_pixels(&data);

        let px = |x: u32, y: u32| -> [u8; 3] {
            let o = ((y * 4 + x) * 4) as usize;
            [pixels[o], pixels[o + 1], pixels[o + 2]]
        };
        let to_bytes3 = |c: [u8; 3]| c;
        let forest = to_bytes3(
            crate::terrain_view::biome_raw::BIOME_RAW_COLORS[wv_terrain::BIOME_FOREST as usize],
        );
        let desert = to_bytes3(
            crate::terrain_view::biome_raw::BIOME_RAW_COLORS[wv_terrain::BIOME_DESERT as usize],
        );
        let ocean = to_bytes3(
            crate::terrain_view::biome_raw::BIOME_RAW_COLORS[wv_terrain::BIOME_OCEAN as usize],
        );

        // 布局(行优先 idx = y*4+x)：idx2=(2,0) 陆森林、idx6=(2,1) 陆沙漠、
        // idx15=(3,3) 海洋；idx0=(0,0) surface=水 + NONE → 水面回退。
        assert_eq!(px(2, 0), forest, "(2,0) biome=森林 → 固定森林色");
        assert_eq!(px(2, 1), desert, "(2,1) biome=沙漠 → 固定沙漠色");
        assert_eq!(px(3, 3), ocean, "(3,3) biome=海洋 → 固定海洋色");
        assert_ne!(px(0, 0), forest, "(0,0) NONE+水 → 回退水蓝，不得撞森林色");
    }

    /// 群系主导着色（修正原「biome 不参与」语义）：同 height/surface、
    /// 不同 biome 的两格颜色必须**不同**——颜色编码 biome 本身。
    #[test]
    fn topographic_mode_paints_biome_channel() {
        let mut data = topographic_fixture();
        // (1,1) 与 (2,1) 同 height/surface，仅 biome 不同 → 颜色必须不同。
        data.cells[6].height = data.cells[5].height;
        data.cells[6].biome = wv_terrain::BIOME_DESERT;
        data.cells[5].biome = wv_terrain::BIOME_FOREST;
        let pixels = topographic_pixels(&data);
        let o5 = (5 * 4) as usize;
        let o6 = (6 * 4) as usize;
        assert_eq!(data.cells[5].biome, wv_terrain::BIOME_FOREST);
        assert_eq!(data.cells[6].biome, wv_terrain::BIOME_DESERT);
        assert_ne!(
            pixels[o5..o5 + 3],
            pixels[o6..o6 + 3],
            "仅 biome 不同 → 颜色必须不同(群系色块语义)"
        );
    }

    /// soften 开关在 Topographic 下被忽略:开与不开的整图像素逐字节一致
    /// (分析视图四色是语义色,淡化会把 3 阶对比洗掉)。
    #[test]
    fn topographic_mode_ignores_soften_flag() {
        let data = topographic_fixture();
        let (on, _) = terrain_pixels(
            &data,
            ShadeMode::Topographic,
            None,
            true,
            &CellPack::fixture(),
        );
        let (off, _) = terrain_pixels(
            &data,
            ShadeMode::Topographic,
            None,
            false,
            &CellPack::fixture(),
        );
        assert_eq!(on, off, "soften=true 与 false 输出必须逐位一致");
    }

    /// Topographic 色带 = 8 锚点**段内连续渐变**（CellMap 数据视图对
    /// ADR-0055「8 阶离散」的修正）：块烘焙像素与 `tint_continuous` 直接
    /// 调用逐通道一致。fixture 陆高 10..140 → 窄域兜底 cuts = [25,50,…
    /// 175]：50 m 恰在 cuts[1] 段锚（t = 0，退化为离散档色）；60 m 段内
    /// t = 0.4，是离散色带**给不出**的连续混色——换回 `band_of+tint_rgba`
    /// 本测试立刻翻红。
    #[test]
    fn topographic_lowland_uses_continuous_tint() {
        use super::super::hypsometric;
        let data = topographic_fixture();
        let cuts = hypsometric::cuts_for_cells(&data.cells);
        let pack = CellPack {
            hypsometric: 1.0,
            ..CellPack::fixture()
        };
        let (pixels, res) = terrain_pixels(&data, ShadeMode::Topographic, None, true, &pack);
        assert_eq!(res, 1, "Topographic 每格 1 像素（锐利方格）");
        // idx6 = (2,1) 高 50 m、idx7 = (3,1) 高 60 m（皆草地陆格，见 fixture）。
        let to_bytes = |rgba: [f32; 4]| -> [u8; 3] {
            [
                (rgba[0] * 255.0) as u8,
                (rgba[1] * 255.0) as u8,
                (rgba[2] * 255.0) as u8,
            ]
        };
        for idx in [6usize, 7] {
            let c = &data.cells[idx];
            let o = idx * 4;
            let got = [pixels[o], pixels[o + 1], pixels[o + 2]];
            let want = to_bytes(hypsometric::tint_continuous(
                crate::terrain_view::biome_raw::pick(c.biome, c.surface),
                c.height,
                &cuts,
                1.0,
            ));
            assert_eq!(
                got, want,
                "格 {idx}（{} m）烘焙应与 tint_continuous 直接调用一致",
                c.height
            );
        }
        // 连续性是真的：60 m（t = 0.4）必须不同于任何一档离散档色。
        let c = &data.cells[7];
        let discrete = to_bytes(hypsometric::tint_rgba(
            crate::terrain_view::biome_raw::pick(c.biome, c.surface),
            hypsometric::band_of(c.height, &cuts),
            1.0,
        ));
        assert_ne!(
            discrete,
            [pixels[7 * 4], pixels[7 * 4 + 1], pixels[7 * 4 + 2]],
            "60 m 段内混色不应等于离散档色"
        );
    }

    // -------------------------------------------------------------------
    // 坡度浮雕（ADR-0077 D6：CPU 烘焙接 shading 真源——双光源单尺度）
    // -------------------------------------------------------------------

    /// 建一个带米制比例的 renderer（直接驱动浮雕乘子；VectorFlat 无关紧要
    /// ——`relief_intensity` 只读高度场与 slope_gain）。
    fn relief_renderer<'a>(
        data: &'a wv_core::terrain::TerrainData,
        pack: &'a CellPack,
        meters_per_cell: f32,
    ) -> CellRenderer<'a> {
        CellRenderer::new(
            data,
            ShadeMode::VectorFlat,
            None,
            false,
            pack,
            None,
            meters_per_cell,
        )
    }

    /// 平地光照明度（浮雕乘子的「零点」）：梯度恒零 → 法线 (0,0,1) →
    /// 与两盏 45° 高度角单位灯的点积各 = sin45° = 1/√2 →
    /// `I = 0.4 + 0.6/√2 ≈ 0.8243`（spec D1 口径）。
    ///
    /// **钳制已退役**：不再钳回 1.0——平地整幅统一乘 0.8243 是公式语义
    ///（`I ∈ [0.4, 1]` 界内免费获得），与 GPU 侧同源（WGSL 直接 dot 同一
    /// 单位向量常量）。
    fn flat_ground_intensity() -> f32 {
        crate::shading::lighting_intensity(
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        )
    }

    /// 平地任意格浮雕乘子 = 平地光照明度（≈0.8243）：无坡即无明暗**差**，
    /// 全图统一乘子（绝对变暗由公式环境光项承担，GPU 侧同源）。
    #[test]
    fn flat_ground_mul_equals_flat_intensity_everywhere() {
        let data = relief_fixture(|_, _| 100);
        let pack = CellPack::fixture();
        let r = relief_renderer(&data, &pack, crate::shading::DEFAULT_METERS_PER_CELL);
        let want = flat_ground_intensity();
        for gy in 0..3u32 {
            for cx in 0..3u32 {
                let got = r.relief_intensity(cx, gy);
                assert!(
                    (got - want).abs() < 1e-6,
                    "平地格 ({cx},{gy}) 应得 {want}，实得 {got}"
                );
            }
        }
    }

    /// 受光方向新口径（`shading::normal_world` + spec D1 的 45° 单位灯）：
    /// **北坡 / 西坡受光（亮于平地）、南坡 / 东坡背光（暗于平地）**。
    /// 行号向下增大 = 南；台阶在东列（x=2 高）时中心格坡面朝西。
    /// fixture 用 100 m 步阶（默认格距 100 m → s·梯度 = 1，45° 坡）：
    /// 300 m 步阶的 71.6° 陡坡在 45° 灯高下会越过「与平地比亮暗」的
    /// 分界（陡坡的 z 分量损失超过受光面收益，四向都暗于平地），受光/
    /// 背光的学名语义由本 fixture 钉住。与旧 `relief_mul` 的
    /// `lit = −(dx+dy)` 逐轴相反是**既定修正**——旧 CPU 路径受光方向
    /// 本来就反（见 `shading::normal_world` 文档）。
    #[test]
    fn north_and_west_facing_slopes_are_lit_south_and_east_shaded() {
        let data_west = relief_fixture(|x, _| if x == 2 { 100 } else { 0 });
        let data_east = relief_fixture(|x, _| if x == 0 { 100 } else { 0 });
        let data_north = relief_fixture(|_, y| if y == 2 { 100 } else { 0 });
        let data_south = relief_fixture(|_, y| if y == 0 { 100 } else { 0 });
        let pack = CellPack::fixture();
        let mpc = crate::shading::DEFAULT_METERS_PER_CELL;
        let flat = flat_ground_intensity();
        // 手算锚点（s = 0.02，台阶梯度 50 m/格 → s·梯度 = 1，45° 坡）：
        // 45° 坡法线 (±1/√2, ±1/√2, 1/√2)，与灯的点积 ∈ {0.8536, 0.1464}：
        // 西向坡 I = 0.4+0.48·0.8536+0.12·0.1464 = 0.8273 > 平地 0.8243；
        // 北向坡 I = 0.4+0.6·0.8536 = 0.9121（两灯同照）；东向坡
        // I = 0.4+0.48·0.1464+0.12·0.8536 = 0.5727；南向坡 I = 0.4879。
        let m = relief_renderer(&data_west, &pack, mpc).relief_intensity(1, 1);
        assert!(m > flat, "西向坡应亮于平地 {flat}，实得 {m}");
        let m = relief_renderer(&data_north, &pack, mpc).relief_intensity(1, 1);
        assert!(m > flat, "北向坡应亮于平地 {flat}，实得 {m}");
        // 东/南向坡：受光点积为负/弱 → 明显暗于平地。
        let m = relief_renderer(&data_east, &pack, mpc).relief_intensity(1, 1);
        assert!(m < flat, "东向坡应暗于平地 {flat}，实得 {m}");
        let m = relief_renderer(&data_south, &pack, mpc).relief_intensity(1, 1);
        assert!(m < flat, "南向坡应暗于平地 {flat}，实得 {m}");
    }

    /// 极端断崖（±9000 m 邻格高差）：钳制已退役，但 `lighting_intensity`
    /// 的界内性契约保证乘子**有限**且落在 `[0.4, 1.0]`
    ///（s·hx = −180 → 法线贴地 → 只剩辅光项 0.4 + 0.12·0.504 ≈ 0.460）。
    #[test]
    fn extreme_cliff_mul_stays_finite() {
        let cliff = relief_fixture(|x, _| if x == 0 { 9000 } else { -9000 });
        let pack = CellPack::fixture();
        let r = relief_renderer(&cliff, &pack, crate::shading::DEFAULT_METERS_PER_CELL);
        let m = r.relief_intensity(1, 1);
        assert!(m.is_finite(), "断崖乘子必须有限，得 {m}");
        assert!(
            (0.4..=1.0).contains(&m),
            "断崖乘子必须落在公式界内 [0.4, 1.0]，得 {m}"
        );
    }

    /// slope_gain 透传链（ADR-0077 D6 CPU 接线点）：同一坡面在两种米制比例
    /// 下（1 km = 8 wu → 500 m/格；无比例尺回落 → 4 m/格）浮雕乘子**不同**
    /// 且都有限——米制换算错一格距，浮雕就整体错。pub 入口
    ///（`terrain_pixels_with_scale`）到像素全链钉住。
    #[test]
    fn relief_applies_single_scale_on_cpu_path() {
        let data = relief_fixture(|x, _| if x == 2 { 300 } else { 0 });
        let cell_size_wu = 4.0f32;
        let scaled = crate::shading::meters_per_cell_of(cell_size_wu, Some(8.0)); // 500 m/格
        let fallback = crate::shading::meters_per_cell_of(cell_size_wu, None); // 4 m/格
        let pack = CellPack::fixture();
        let a = relief_renderer(&data, &pack, scaled).relief_intensity(1, 1);
        let b = relief_renderer(&data, &pack, fallback).relief_intensity(1, 1);
        assert!(
            a.is_finite() && (0.4..=1.0).contains(&a),
            "500 m/格 乘子应有限且界内，得 {a}"
        );
        assert!(
            b.is_finite() && (0.4..=1.0).contains(&b),
            "4 m/格 乘子应有限且界内，得 {b}"
        );
        assert_ne!(a, b, "两种格距的坡面乘子必须不同（透传生效）: {a} vs {b}");
        // pub 入口全链：两种格距烘焙出**不同**像素。
        let (px_scaled, _) =
            terrain_pixels_with_scale(&data, ShadeMode::VectorFlat, None, false, &pack, scaled);
        let (px_fallback, _) =
            terrain_pixels_with_scale(&data, ShadeMode::VectorFlat, None, false, &pack, fallback);
        assert_ne!(
            px_scaled, px_fallback,
            "像素级透传链应生效：格距不同 → 浮雕不同"
        );
    }

    /// 像素颜色是否接近目标 sRGB 色（±2/255，吸收 u8 舍入）。
    fn pixel_near(img: &Image, x: u32, y: u32, target: bevy::color::Srgba) -> bool {
        let data = img.data.as_ref().expect("地形图必须有像素数据");
        let w = img.width();
        let o = ((y * w + x) * 4) as usize;
        (data[o] as f32 / 255.0 - target.red).abs() < 0.02
            && (data[o + 1] as f32 / 255.0 - target.green).abs() < 0.02
            && (data[o + 2] as f32 / 255.0 - target.blue).abs() < 0.02
    }

    /// 读一格中心像素的 RGB（u8）。
    fn px(img: &Image, x: u32, y: u32) -> [u8; 3] {
        let data = img.data.as_ref().expect("地形图必须有像素数据");
        let o = ((y * img.width() + x) * 4) as usize;
        [data[o], data[o + 1], data[o + 2]]
    }

    /// 近岸浅滩带（S2）：水线格被提亮、带外格与旗标关逐位一致。
    ///
    /// 10×1：col 0 陆（草 +50），col 1..10 水（−40）。单行网格上离岸距离
    /// d(i) = i（切比雪夫 = 列距），带宽 6 → col 1..5 着色、col ≥ 6 不动。
    #[test]
    fn shore_shallow_tint_lightens_waterline_and_spares_offshore() {
        let mut cells = Vec::new();
        cells.push(wv_core::terrain::TerrainCellData {
            height: 50,
            surface: 1,
            biome: 3,
        });
        for _ in 0..9 {
            cells.push(wv_core::terrain::TerrainCellData {
                height: -40,
                surface: 0,
                biome: 0,
            });
        }
        let data = TerrainData {
            width: 10,
            height: 1,
            cells,
        };
        // 开：默认包（shore_shallow: true）。
        let pack = &CellPack::fixture();
        let on = image_from_terrain(&data, ShadeMode::VectorFlat, None, false, pack, 0.0);
        // 关：同包改旗标（CellPack 全字段 Copy，结构体更新语法即可）。
        let off_pack = &CellPack {
            treatment: TreatmentFlags {
                shore_shallow: false,
                ..TreatmentFlags::HAND_DRAWN
            },
            ..CellPack::fixture()
        };
        let off = image_from_terrain(&data, ShadeMode::VectorFlat, None, false, off_pack, 0.0);

        // res = clamp(4096/10, 4, 8) = 8：col i 占像素 [8i, 8i+8)，采样格心 x = 8i+4。
        let (near_on, near_off) = (px(&on, 12, 4), px(&off, 12, 4)); // col 1（d=1，带内）
        let (far_on, far_off) = (px(&on, 68, 4), px(&off, 68, 4)); // col 8（d=8，带外）
        assert_ne!(
            near_on, near_off,
            "水线格（d=1）应被浅滩带提亮，实得 on {near_on:?} off {near_off:?}"
        );
        assert_eq!(
            far_on, far_off,
            "带外格（d≥6）必须与旗标关逐位一致（strength 恒 0）"
        );
        let sum = |c: [u8; 3]| c[0] as u32 + c[1] as u32 + c[2] as u32;
        assert!(
            sum(near_on) > sum(near_off),
            "浅滩 tint 应为提亮方向：on 和={} off 和={}",
            sum(near_on),
            sum(near_off)
        );
    }

    /// 近岸浅滩带的 patch 一致性（S2 契约）：岸线附近改一格水陆后，
    /// 「旧图 + 小脏区 patch（内部膨胀）」必须与「新图全图重算」逐字节一致。
    #[test]
    fn shore_tint_patch_with_dilated_rect_matches_full_bake() {
        // 24×3：全水（−40），col 23 列陆。data2 把 (2,1) 改成陆岛——
        // 距离场在 x ≤ 12 一段随之改变（旧最近陆在右列，新岛更近）。
        let cell = |height: i16, surface: u8, biome: u8| wv_core::terrain::TerrainCellData {
            height,
            surface,
            biome,
        };
        let make = |island: bool| {
            let mut cells = Vec::new();
            for y in 0..3u32 {
                for x in 0..24u32 {
                    if x == 23 || (island && (x, y) == (2, 1)) {
                        cells.push(cell(50, 1, 3));
                    } else {
                        cells.push(cell(-40, 0, 0));
                    }
                }
            }
            TerrainData {
                width: 24,
                height: 3,
                cells,
            }
        };
        let data1 = make(false);
        let data2 = make(true);
        let pack = &CellPack::fixture();

        // 旧图全图烘焙 → 以小脏区（只盖新岛那一格）patch 成新图。
        let mut patched = image_from_terrain(&data1, ShadeMode::VectorFlat, None, false, pack, 0.0);
        patch_terrain_image(
            &mut patched,
            &data2,
            ShadeMode::VectorFlat,
            None,
            [2, 1, 3, 2],
            8,
            false,
            pack,
        );
        // 新图全图重算。
        let full = image_from_terrain(&data2, ShadeMode::VectorFlat, None, false, pack, 0.0);
        assert_eq!(
            patched.data.as_deref(),
            full.data.as_deref(),
            "膨胀后的局部 patch 必须与全图重算逐字节一致（S2 距离场支撑半径契约）"
        );
    }

    /// 行带并行与串行输出**逐字节一致**（含边界格扰动与瓦片直铺两种路径）：
    /// 带数只是调度参数，任何线程切分都不允许改变像素。
    #[test]
    fn banded_rendering_matches_serial_rendering() {
        // 24×160 地形（h≥128 绕开串行快路，bands=4 真正走多带分支），
        // 内嵌异类陆块 → 边界格走扰动/超采样路径，内部格走直铺。
        let (w, h) = (24u32, 160u32);
        let cells: Vec<wv_core::terrain::TerrainCellData> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let land = (6..=17).contains(&x) && (40..=119).contains(&y);
                let snow = x == 12 && y == 80;
                wv_core::terrain::TerrainCellData {
                    height: if land { 80 } else { -60 },
                    surface: if land { 1 } else { 0 },
                    biome: if snow {
                        9 // 雪原（显示槽位 7）
                    } else if land {
                        3 // 草原
                    } else {
                        0 // 深水
                    },
                }
            })
            .collect();
        let data = wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        };
        let pack = &CellPack::fixture();
        let mpc = crate::shading::DEFAULT_METERS_PER_CELL;
        let (p1, res1) =
            terrain_pixels_banded(&data, ShadeMode::Coastline, None, false, pack, 1, mpc);
        let (p4, res4) =
            terrain_pixels_banded(&data, ShadeMode::Coastline, None, false, pack, 4, mpc);
        assert_eq!(res1, res4, "带数不得影响 res 推导");
        assert_eq!(p1.len(), p4.len(), "输出长度应一致");
        assert_eq!(p1, p4, "带数不同 → 像素逐字节一致");
    }

    // -----------------------------------------------------------------------
    // 雪格双调（手绘地图呈现 #3，2026-10-15）
    // -----------------------------------------------------------------------

    use super::snow_shade_at;
    use super::snow_shadow_t;

    /// 雪阴影强度阈值：阈值以下为 0，以上按 km 比例，封顶 1。
    #[test]
    fn snow_shadow_t_threshold_and_clamp() {
        assert_eq!(snow_shadow_t(0), 0.0, "低海拔无阴影");
        assert_eq!(
            snow_shadow_t(crate::palette::SNOW_SHADOW_MIN_HEIGHT),
            0.0,
            "恰阈值处无阴影"
        );
        let t = snow_shadow_t(crate::palette::SNOW_SHADOW_MIN_HEIGHT + 1000);
        assert!(
            (t - crate::palette::SNOW_SHADOW_PER_KM).abs() < 1e-6,
            "+1 km 应为 per_km，实得 {t}"
        );
        assert_eq!(snow_shadow_t(i16::MAX), 1.0, "高海拔应封顶 1.0");
    }

    /// 雪格 highlight：低海拔雪 = 近白雪（与 core_terrain[7] 提亮一档同族但更亮）。
    #[test]
    fn snow_shade_at_low_elevation_is_highlight() {
        let pal = &CellPack::fixture().palette;
        let Color::Srgba(h) = snow_shade_at(1000, pal) else {
            panic!()
        };
        let Color::Srgba(b) = pal.core_terrain(7) else {
            panic!()
        };
        let lum_h = (h.red + h.green + h.blue) / 3.0;
        let lum_b = (b.red + b.green + b.blue) / 3.0;
        assert!(
            lum_h > lum_b,
            "highlight({lum_h}) 应比 core.snow({lum_b}) 亮"
        );
    }

    /// 雪格 shadow：高海拔雪 = 冷蓝灰（蓝 > 红），与 highlight 冷暖对调。
    #[test]
    fn snow_shade_at_high_elevation_is_cool_gray() {
        let pal = &CellPack::fixture().palette;
        let Color::Srgba(s) = snow_shade_at(6000, pal) else {
            panic!()
        };
        assert!(
            s.blue > s.red,
            "高海拔雪阴影应偏冷（蓝 > 红），实得 r={} b={}",
            s.red,
            s.blue
        );
    }

    /// 水陆填充边由双线性高度决定，而非类型格缘（D2 的招牌手算案例）：
    /// 2×1 两格同为草地，高度 +50 / −50，海平面等值线恰在格缘 x=1——
    /// 填充左半陆色、右半水色（旧的逐格平涂会整行草色）。
    #[test]
    fn vector_flat_fill_edge_follows_height_not_kind() {
        let data = TerrainData {
            width: 2,
            height: 1,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 50,
                    surface: 1,
                    biome: 3,
                },
                wv_core::terrain::TerrainCellData {
                    height: -50,
                    surface: 1,
                    biome: 3,
                },
            ],
        };
        // 关掉水彩晕开（ADR-0044 D1）：本测试钉的是「填充边随高度」的平涂
        // 语义；晕开是邻域混合装饰（2×1 图上整幅都在边界带内，会把左右两半
        // 混成同一色），eef816c 接线后由此长期带病——装饰语义不该稀释本测试。
        // 关掉全部「位置相关」的水面效果：晕开（邻域混合）、浅滩带、离岸
        // 墨线、近岸泡沫——本测试钉的是「填充边随高度」的**平涂**语义，
        // 装饰不该稀释它（同 d43f618 关 bleed 的口径）。纸纹颗粒不在此列
        // （它是坐标的纯函数，不影响边缘判定）。
        let pack = &CellPack {
            treatment: TreatmentFlags {
                watercolor_bleed: false,
                shore_shallow: false,
                ..TreatmentFlags::HAND_DRAWN
            },
            sea: SeaRecipe {
                shore_band_cells: 0.0,
                shore_peak: 0.0,
                ripples: vec![],
                foam: vec![],
                ..crate::palette::watercolor()
            },
            ..CellPack::fixture()
        };
        let img = image_from_terrain(&data, ShadeMode::VectorFlat, None, false, pack, 0.0);
        let Color::Srgba(grass) = palette::core_terrain(3) else {
            unreachable!()
        };
        // 陆格颗粒（ADR-0042 D6）作用于左半格（最近格 (0,0)，biome 3）：
        // 期望色 = 草色 × 该格确定性颗粒乘子。
        let g0 = land_grain(3, 0, 0);
        let grass = bevy::color::Color::srgb(grass.red * g0, grass.green * g0, grass.blue * g0);
        // 沉入海下的陆格按湿判定显示（surface 陆 + 海拔 -50 → 浅水槽位）。
        let Color::Srgba(shallow) = palette::core_terrain(1) else {
            unreachable!()
        };
        // 2 格 ≤1024 → res = clamp(4096/2, 4, 8) = 8：图像 16×8。
        // 等值线在格缘 gx=1.0 → 像素 x∈0..8 陆、x∈8..16 水。
        assert!(pixel_near(&img, 2, 2, grass.into()), "左半应为草色");
        assert!(
            pixel_near(&img, 12, 2, shallow),
            "右半应为浅水色（authored 陆地被海拔判湿 → 按水深给浅水）"
        );
    }

    /// 深水内部格走快路径：全 −100 / kind 0 的 3×3，中心像素 = 深水色。
    #[test]
    fn vector_flat_deep_water_interior_stays_flat() {
        let data = TerrainData {
            width: 3,
            height: 3,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: -3500,
                    surface: 0,
                    biome: 0,
                };
                9
            ],
        };
        let img = image_from_terrain(
            &data,
            ShadeMode::VectorFlat,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        // 深水渐变（手绘地图 #1）启用后：深水中心像素 = water_shade_at(−3500)，
        // 不再是 core_terrain(0) 原色。统一从 water_shade_at 取色源，避免硬编码颜色。
        let pal = &CellPack::fixture().palette;
        let Color::Srgba(deep) = water_shade_at(-3500, pal, &crate::palette::watercolor()) else {
            unreachable!()
        };
        assert!(pixel_near(&img, 4, 4, deep));
    }

    /// 等值线模式与 GoogleMap 同用亚像素分辨率（填充边与描边吻合的前提）。
    #[test]
    fn vector_flat_resolution_matches_google_subpixel() {
        let data = TerrainData {
            width: 512,
            height: 512,
            cells: vec![wv_core::terrain::TerrainCellData::default(); 512 * 512],
        };
        let vector = terrain_res_for(&data, ShadeMode::VectorFlat, None);
        let google = terrain_res_for(&data, ShadeMode::GoogleMap, None);
        assert_eq!(vector, google, "VectorFlat 应与 GoogleMap 同口径");
        assert!(vector > 1, "512² 图应取 {vector}px/格（亚像素）");
    }

    /// 普通地图的十槽色**必须读传入的 palette**（2026-10-25 修正）。
    ///
    /// 历史坑：`shade_google` 曾忽略 `_pal`、直读 crate 本地
    /// `palette::GOOGLE_CORE_TERRAIN`；而主题侧另有一份同名表。于是
    /// `conv_palette_terrain` 的覆盖对像素完全无效——两处同名表，改一处不出效果。
    /// 本测试用哨兵色钉住「跟着 palette 走」，防止回退。
    #[test]
    fn google_shade_reads_palette_core_not_the_local_copy() {
        let data = relief_fixture(|_, _| 0);
        let mut pal = CellPack::fixture().palette;
        pal.core[2] = Color::srgb(0.10, 0.20, 0.30);
        pal.core[4] = Color::srgb(0.0, 1.0, 0.0);
        pal.core[8] = Color::srgb(0.0, 0.0, 1.0);
        assert_eq!(shade_google(&data, 4, &pal), pal.core[4], "森林 → 公园绿槽");
        assert_eq!(shade_google(&data, 8, &pal), pal.core[8], "沼泽 → 湿绿槽");
        assert_eq!(
            shade_google(&data, 3, &pal),
            pal.core[2],
            "草原折叠为陆地基色"
        );
        assert_eq!(
            shade_google(&data, 6, &pal),
            pal.core[2],
            "山地折叠为陆地基色"
        );
    }

    /// 专业制图的**海拔色带**（ADR-0067 D3）：高处必须比低处暗（沉），
    /// 且只在陆格生效。
    ///
    /// 三条断言各钉一件事：
    /// - 高地 ≠ 低地（色带真的分出了阶）；
    /// - 高地**更暗**（专业制图取「近白 → 深岩褐」的反方向，见色带注释）；
    /// - 水格不受影响（水面已有深度渐变）。
    #[test]
    fn cartographic_elevation_band_darkens_high_ground_only() {
        use super::super::hypsometric::ElevationBand;
        let mut pal = CellPack::fixture().palette;
        pal.elevation_ramp = Some([
            [0xF8, 0xF9, 0xFA], // 0 近白
            [0xE3, 0xE0, 0xD8],
            [0xD4, 0xCF, 0xC4],
            [0xC2, 0xBB, 0xAE],
            [0xAB, 0xA2, 0x95],
            [0x8E, 0x86, 0x7C],
            [0x6B, 0x64, 0x5E],
            [0x50, 0x4A, 0x45], // 7 深岩褐
        ]);
        pal.elevation_strength = 0.62;
        let band = ElevationBand {
            ramp: pal.elevation_ramp.expect("刚设的"),
            cuts: super::super::hypsometric::cuts_for_cells(&[]),
            strength: pal.elevation_strength,
        };
        // 绝对阶梯：200/500/1000/1500/2500/4000/6000 m。
        let low = band.tint([1.0, 1.0, 1.0, 1.0], 50);
        let high = band.tint([1.0, 1.0, 1.0, 1.0], 5000);
        assert_ne!(low, high, "高处与低处必须不同");
        let lum = |c: [f32; 4]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        assert!(
            lum(high) < lum(low),
            "高处应更暗（近白 → 深岩褐），低={:.3} 高={:.3}",
            lum(low),
            lum(high)
        );
    }

    /// **端到端**：专业制图模式下，高地像素必须比低地像素暗（ADR-0067 D3）。
    ///
    /// 这是「接线真的生效」的锚点——只测 `ElevationBand::tint` 纯函数不够：
    /// 色带可能算对了却没接进烘焙管线（像 ADR-0061 追记一修的那个
    /// 「`shade_google` 读错色表」一样的假绿）。本测试走完整烘焙。
    #[test]
    fn cartographic_high_ground_bakes_darker_than_low_ground() {
        // 2×1：左格 50 m（低地），右格 3000 m（雪线以上）。
        let data = TerrainData {
            width: 2,
            height: 1,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 50,
                    surface: 1,
                    biome: 3,
                },
                wv_core::terrain::TerrainCellData {
                    height: 3000,
                    surface: 1,
                    biome: 3,
                },
            ],
        };
        let mut pack = CellPack::fixture();
        pack.palette.core = [Color::srgb(1.0, 1.0, 1.0); 10];
        pack.palette.elevation_ramp = Some([
            [0xF8, 0xF9, 0xFA],
            [0xE3, 0xE0, 0xD8],
            [0xD4, 0xCF, 0xC4],
            [0xC2, 0xBB, 0xAE],
            [0xAB, 0xA2, 0x95],
            [0x8E, 0x86, 0x7C],
            [0x6B, 0x64, 0x5E],
            [0x50, 0x4A, 0x45],
        ]);
        pack.palette.elevation_strength = 0.62;
        let img = image_from_terrain(&data, ShadeMode::GoogleMap, None, false, &pack, 0.0);
        let res = img.width() / data.width;
        let px = |cell: usize| -> f32 {
            let x = (cell % 2) as u32 * res + res / 2;
            let y = res / 2;
            let o = ((y * img.width() + x) * 4) as usize;
            let d = img.data.as_ref().expect("地形图必须有像素");
            0.299 * d[o] as f32 + 0.587 * d[o + 1] as f32 + 0.114 * d[o + 2] as f32
        };
        let low = px(0);
        let high = px(1);
        assert!(
            high < low,
            "3000m 高地应比 50m 低地暗：低={low:.1} 高={high:.1}"
        );
    }

    // 晕渲（ADR-0067 D2 的「专业制图更轻钳制」口径）已随 ADR-0077 D6 退役：
    // 三模式共走 `shading::lighting_intensity` 单公式，界内性 [0.4, 1] 由
    // 公式免费保证——旧 RELIEF_*_CARTOGRAPHIC / RELIEF_MIN/MAX 常量与
    // `cartographic_relief_is_clamped_lighter_than_artistic_one` 一并删除。

    // ---------------------------------------------------------------------------
    // 底图海洋渲染（2026-10-16 用户反馈「选了海洋但颜色没变蓝」）
    //
    // 双重叠加陷阱：
    //   1. VectorFlat 模式下水系格走 `water_shade_at(height)`（2026-10-15
    //      引入的水域渐变），不是 `core_terrain(kind)`；
    //   2. `terrain_soften = true`（默认）会把任何颜色再与纸面色 55% 混合。
    //
    // 旧 height = -120 走到 water_shade_at 的 t ≈ 0.034 浅水端（接近
    // water_shallow #98A9B9），过 soften 后退成 ≈(201, 205, 202) 中性灰，
    // 肉眼辨不出「海」。新 height = -3500 走 t = 1.0 + 冷蓝偏移 ≈(117, 152, 255)，
    // 过 soften 后仍是 ≈(186, 197, 233) 清晰蓝。
    // ---------------------------------------------------------------------------

    /// VectorFlat + soften=true 下，MapBase::Ocean.cell() 渲染后中心像素是清晰蓝。
    /// 回归 2026-10-16 的「选海洋不显蓝」——任何把 Ocean 高度拉回 -120、或 soften
    /// 改全 paper mix、或 water_shade_at 的冷偏移被砍掉，都会让这测试挂掉。
    #[test]
    fn ocean_base_renders_as_clear_blue_under_default_soften() {
        let data = TerrainData {
            width: 4,
            height: 4,
            cells: vec![wv_core::map::MapBase::Ocean.cell(); 16],
        };
        let img = image_from_terrain(
            &data,
            ShadeMode::VectorFlat,
            None,
            // 默认 soften=true 也要清晰蓝——这是用户最直观的视觉预期。
            true,
            &CellPack::fixture(),
            0.0,
        );
        let px = img.data.as_ref().expect("image 必有像素");
        let res = (img.width() / 4) as usize;
        let cx = res / 2;
        let cy = res / 2;
        let i = (cy * img.width() as usize + cx) * 4;
        let r = px[i] as i32;
        let g = px[i + 1] as i32;
        let b = px[i + 2] as i32;
        assert!(
            b > r + 10,
            "蓝通道应显著高于红通道（清晰蓝），实得 r={r} g={g} b={b}"
        );
        assert!(b > g + 5, "蓝通道应高于绿通道，实得 r={r} g={g} b={b}");
        assert!(b > 180, "蓝通道应清晰（>180），实得 b={b}");
    }

    /// 2026-10-16 用户裁定「明明白白用深蓝」：Ocean 渲染后是「深蓝」色，
    /// 不是浅蓝、不是灰蓝、不是中性灰。蓝绿差 ≥ 30 足以肉眼辨为「海」；
    /// 蓝红差 ≥ 30 是「海」与「陆」区分的下限。
    #[test]
    fn ocean_base_is_visibly_deep_blue_not_pale_gray() {
        let data = TerrainData {
            width: 4,
            height: 4,
            cells: vec![wv_core::map::MapBase::Ocean.cell(); 16],
        };
        let img = image_from_terrain(
            &data,
            ShadeMode::VectorFlat,
            None,
            true,
            &CellPack::fixture(),
            0.0,
        );
        let px = img.data.as_ref().expect("image 必有像素");
        let res = (img.width() / 4) as usize;
        let i = (res / 2 * img.width() as usize + res / 2) * 4;
        let r = px[i] as i32;
        let g = px[i + 1] as i32;
        let b = px[i + 2] as i32;
        assert!(
            b - g >= 30,
            "Ocean 应是「深蓝」而非浅蓝 / 灰蓝，蓝绿差应 ≥ 30，实得 r={r} g={g} b={b}"
        );
        assert!(b - r >= 30, "Ocean 蓝红差应 ≥ 30，实得 r={r} g={g} b={b}");
        assert!(b >= 180, "蓝通道 ≥ 180，实得 b={b}");
    }

    /// Schematic + soften=true 下，kind=0（深水）也应是清晰蓝。Schematic 走
    /// `core_terrain(kind)` 直读，不经 water_shade_at——这条路径上深水只被
    /// soften 抹 55%，但底色（143, 162, 179）足够蓝，仍能辨出。
    #[test]
    fn schematic_deep_water_stays_blue_under_default_soften() {
        let data = TerrainData {
            width: 4,
            height: 4,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: -3500,
                    surface: 0,
                    biome: 0,
                };
                16
            ],
        };
        let img = image_from_terrain(
            &data,
            ShadeMode::Schematic,
            None,
            true,
            &CellPack::fixture(),
            0.0,
        );
        let px = img.data.as_ref().expect("image 必有像素");
        let res = (img.width() / 4) as usize;
        let cx = res / 2;
        let cy = res / 2;
        let i = (cy * img.width() as usize + cx) * 4;
        let r = px[i] as i32;
        let g = px[i + 1] as i32;
        let b = px[i + 2] as i32;
        assert!(b > r, "深水蓝通道应 > 红通道，实得 r={r} g={g} b={b}");
        assert!(b >= 150, "蓝通道应清晰，实得 b={b}");
    }

    // ---------------------------------------------------------------------------
    // 顶栏缩放下 Ocean 与 Land 仍可区分（2026-10-16 用户反馈「选了海洋但
    // 颜色没变蓝」第二步）。
    //
    // 单测验「图像合成」后再过一道「雾霭」模拟——这是用户顶栏 473% 缩放下
    // 实际看到的颜色。任何把 VEIL_FAR_SCALE 拉回 3.0、或 VEIL_MAX_ALPHA 拉回 0.55
    // 都会让海陆都退成纸色，让这测试挂掉。
    //
    // 验证目标：在 4.73× 缩放（顶栏 473%）下：
    //   - Ocean 仍 ≥ Land 的 b 通道（海偏冷，陆偏暖）
    //   - Ocean 的 g - b ≥ 0（海冷，陆 g 偏大）
    //   - Ocean 与 Land 的 RGB 差距 ≥ 5（可分辨）
    // ---------------------------------------------------------------------------

    /// 模拟「顶栏缩放 4.73× + 雾霭」下的真实色，把 MapBase::Ocean 与 MapBase::Land
    /// 区分性钉住。任何让海陆在远缩放下都退成纸色的回归，都会被这里拦下。
    #[test]
    fn ocean_and_land_stay_distinguishable_at_user_zoom() {
        // 1. 渲染 Ocean / Land 两张 4×4 图（同 scale、同 theme、同 soften）。
        let ocean_data = TerrainData {
            width: 4,
            height: 4,
            cells: vec![wv_core::map::MapBase::Ocean.cell(); 16],
        };
        let land_data = TerrainData {
            width: 4,
            height: 4,
            cells: vec![wv_core::map::MapBase::Land.cell(); 16],
        };
        let pack = &CellPack::fixture();
        let ocean_img =
            image_from_terrain(&ocean_data, ShadeMode::VectorFlat, None, true, pack, 0.0);
        let land_img = image_from_terrain(&land_data, ShadeMode::VectorFlat, None, true, pack, 0.0);

        // 2. 取中心像素（在 image_from_terrain 之后，这一步相当于「无雾」底色）。
        fn center_pixel(img: &Image) -> (i32, i32, i32) {
            let px = img.data.as_ref().expect("image 必有像素");
            let res = (img.width() / 4) as usize;
            let cx = res / 2;
            let cy = res / 2;
            let i = (cy * img.width() as usize + cx) * 4;
            (px[i] as i32, px[i + 1] as i32, px[i + 2] as i32)
        }
        let (or, og, ob) = center_pixel(&ocean_img);
        let (lr, lg, lb) = center_pixel(&land_img);

        // 3. 模拟雾霭顶混合（用户 4.73× 缩放）。
        let veil_alpha = crate::terrain_view::atmosphere::veil_alpha(4.73);
        // 手算 Ocean 过雾霭（为 0..255 字节，与底层 blend 一致）
        // out = base * (1 - alpha) + paper * alpha
        let paper = pack.palette.soften_toward;
        let Color::Srgba(p) = paper else {
            panic!("soften_toward 应为 Srgba")
        };
        let pr = (p.red * 255.0).round() as i32;
        let pg = (p.green * 255.0).round() as i32;
        let pb = (p.blue * 255.0).round() as i32;
        let k = (veil_alpha * 255.0).round() as i32;
        let mk = 255 - k; // 1 - alpha（字节化）
        let or_v = (or * mk + pr * k) / 255;
        let og_v = (og * mk + pg * k) / 255;
        let ob_v = (ob * mk + pb * k) / 255;
        let lr_v = (lr * mk + pr * k) / 255;
        let lg_v = (lg * mk + pg * k) / 255;
        let lb_v = (lb * mk + pb * k) / 255;

        // 4. 断言：Ocean 偏冷（蓝通道 ≥ 绿通道），Land 偏暖（绿通道 ≥ 蓝通道）。
        assert!(
            ob_v >= og_v,
            "Ocean 过雾霭后仍应偏冷（蓝≥绿），实得 o=({or_v},{og_v},{ob_v}) l=({lr_v},{lg_v},{lb_v})"
        );
        assert!(
            lg_v >= lb_v,
            "Land 过雾霭后应偏暖（绿≥蓝），实得 l=({lr_v},{lg_v},{lb_v})"
        );
        // 5. 可分辨：海陆蓝通道差 ≥ 8
        assert!(
            ob_v.abs_diff(lb_v) >= 8,
            "海陆蓝通道差应 ≥ 8，实得 ob={ob_v} lb={lb_v}"
        );
    }

    // ---------------------------------------------------------------------------
    // 端到端回归：笔刷→数据→纹理（2026-10-14 用户反馈「笔刷涂大地图没变化」）。
    // 单测层验过 wv-terrain 改值；本测试验「数据 + 图像合成」两步的连贯性——
    // 任何一环把数据吞了/拿错底图/类型 vs 高度口径不一致，都会让这测试挂掉。
    // ---------------------------------------------------------------------------
    use wv_core::id::Id;
    use wv_core::terrain::TerrainData;

    /// 启动占位地形 → 笔刷把陆地涂成深水 → 全图重建后中心格像素必须是深水色。
    /// 任何"笔刷不生效"的回归都会让这测试的红/蓝通道与基色 0x1E3A5F 偏离。
    #[test]
    fn brush_paint_round_trips_through_terrain_renderer() {
        use bevy::asset::Assets;
        use bevy::image::Image;
        use bevy::prelude::*;
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            comment: None,
            id: Id::new_unchecked("world_main"),
            name: "世界".into(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: vec![],
            layers: vec![],
            icons: vec![],
            extent: Some(wv_core::map::Extent::sized(512.0, 512.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
        });
        let (map, data) = crate::terrain_view::active_terrain_or_placeholder(
            &mut world,
            Some(&Id::new_unchecked("world_main")),
            64,
        );

        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            &data,
            ShadeMode::VectorFlat,
            None,
            false,
            // 关 shore_shallow（S2）：本测试钉「中心格 = 纯水深色」，浅滩带
            // 是按离岸距离的装饰混色，会把断言色稀释（同 mod.rs 哨兵测试口径）。
            &CellPack {
                treatment: TreatmentFlags {
                    shore_shallow: false,
                    ..TreatmentFlags::HAND_DRAWN
                },
                // 关位置相关的水面效果（浅滩带 / 墨线 / 泡沫）：本测试钉
                // 「中心格 = 纯水深色」，装饰混色会稀释断言色。
                sea: SeaRecipe {
                    shore_band_cells: 0.0,
                    shore_peak: 0.0,
                    ripples: vec![],
                    foam: vec![],
                    ..crate::palette::watercolor()
                },
                ..CellPack::fixture()
            },
            0.0,
        );
        let tex = crate::terrain_view::new_terrain_texture(
            &mut images,
            image,
            &data,
            map.clone(),
            ShadeMode::VectorFlat,
            false,
            Default::default(),
            0.0,
        );
        let handle = tex.handle.clone();

        // 模拟「笔刷已落笔」：格 (4,4)（world (32.5,32.5)、cell_size 8）涂成
        // kind 0 深水、中心高度 -40（与旧笔刷 falloff 在该格的落点值一致，
        // 尾部断言的 water_shade_at(-40) 由此而来）。笔触 → 命令链的测试归
        // wv-edit；本测试只验「数据变了 → 重建管线像素正确」，故直接改中立数据
        // （ADR-0020：渲染层不依赖 wv-edit）。
        let mut painted = world
            .terrain_of(&Id::new_unchecked("world_main"))
            .unwrap()
            .clone();
        // 涂 5×5 邻域（旧笔刷 falloff 半径 96wu ≈ 12 格的等效最小集）：VectorFlat
        // 的水陆边在格内部按邻域高度画（D2），只改单格会被邻格陆地把中心像素
        // 划成陆色，无法验证「中心格 = 深水色」。
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let gx = (4 + dx) as usize;
                let gy = (4 + dy) as usize;
                painted.cells[gy * 64 + gx].height = -40;
                painted.cells[gy * 64 + gx].surface = 0;
                painted.cells[gy * 64 + gx].biome = 0;
            }
        }
        world.set_terrain(Id::new_unchecked("world_main"), painted);

        let after = world.terrain_of(&Id::new_unchecked("world_main")).unwrap();
        assert!(after.cells[4 * 64 + 4].height < 0, "海格高度应在海平面下");
        assert_eq!(after.cells[4 * 64 + 4].surface, 0);

        // 全图重建（同 `flush_terrain_texture` 的整图分支；关 shore_shallow，
        // 理由见首烘处——水深语义测试不掺浅滩装饰）。
        let new_image = image_from_terrain(
            after,
            ShadeMode::VectorFlat,
            None,
            false,
            &CellPack {
                treatment: TreatmentFlags {
                    shore_shallow: false,
                    ..TreatmentFlags::HAND_DRAWN
                },
                // 关位置相关的水面效果（浅滩带 / 墨线 / 泡沫）：本测试钉
                // 「中心格 = 纯水深色」，装饰混色会稀释断言色。
                sea: SeaRecipe {
                    shore_band_cells: 0.0,
                    shore_peak: 0.0,
                    ripples: vec![],
                    foam: vec![],
                    ..crate::palette::watercolor()
                },
                ..CellPack::fixture()
            },
            0.0,
        );
        if let Some(mut img) = images.get_mut(&handle) {
            *img = new_image;
        }
        let img_after = images.get(&handle).unwrap();
        let w = img_after.width();
        println!("DBG w={} cell44={:?}", w, after.cells[4 * 64 + 4]);
        let res = w / 64;
        let px = (4 * res + res / 2) as usize;
        let py = (4 * res + res / 2) as usize;
        let o = (py * w as usize + px) * 4;
        let data_ref = img_after.data.as_ref().unwrap();
        // 海色渐变（手绘地图 #1）后：中心格实际渲染色 = water_shade_at(−40)，
        // 不再等于 core_terrain(0) 原色。从 water_shade_at 取色源，避免硬编码。
        let pal = &CellPack::fixture().palette;
        let Color::Srgba(deep) = water_shade_at(-40, pal, &crate::palette::watercolor()) else {
            unreachable!()
        };
        let r = data_ref[o] as f32 / 255.0;
        let g = data_ref[o + 1] as f32 / 255.0;
        let b = data_ref[o + 2] as f32 / 255.0;
        assert!(
            (r - deep.red).abs() < 0.02
                && (g - deep.green).abs() < 0.02
                && (b - deep.blue).abs() < 0.02,
            "中心格应为深水色 ({}), got ({:.3}, {:.3}, {:.3})",
            deep.blue,
            r,
            g,
            b
        );
    }

    /// 2026-10-14「只看到等值线、没有底图填充」回归。
    /// 模拟 NewMap / LoadProject 的"造地形 → 调 replace_terrain_texture
    /// 二次写入"链路，验证新 image data 通过同句柄仍能取到。
    /// `sync_terrain_chunks` 的 needs_src 路径成立的前提就是这一不变量。
    #[test]
    fn replace_terrain_texture_keeps_image_data_reachable_via_same_handle() {
        use bevy::asset::Assets;
        use bevy::image::Image;
        let mut world_core = wv_core::World::default();
        world_core.maps.push(wv_core::map::MapNode {
            comment: None,
            id: Id::new_unchecked("world_main"),
            name: "世界".into(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: vec![],
            layers: vec![],
            icons: vec![],
            extent: Some(wv_core::map::Extent::sized(512.0, 512.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
        });
        let (map, data) = crate::terrain_view::active_terrain_or_placeholder(
            &mut world_core,
            Some(&Id::new_unchecked("world_main")),
            64,
        );

        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            &data,
            ShadeMode::VectorFlat,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let mut tex = crate::terrain_view::new_terrain_texture(
            &mut images,
            image,
            &data,
            map.clone(),
            ShadeMode::VectorFlat,
            false,
            Default::default(),
            0.0,
        );
        let expected_handle = tex.handle.clone();
        // 二次写入（NewMap / LoadProject 的真实路径）。
        let image2 = image_from_terrain(
            &data,
            ShadeMode::VectorFlat,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        crate::terrain_view::replace_terrain_texture(
            &mut tex,
            &mut images,
            image2,
            &data,
            map.clone(),
            ShadeMode::VectorFlat,
            false,
            Default::default(),
            0.0,
        );
        assert_eq!(
            tex.handle, expected_handle,
            "replace 必须复用句柄（否则 ColorMaterial 悬空）"
        );
        let img = images.get(&tex.handle).expect("句柄必须仍指向有效资产");
        let data_len = img
            .data
            .as_ref()
            .map(|d| d.len())
            .expect("image data 应非空");
        assert!(data_len > 0, "replace 后 data 仍应非空，实得 {data_len}");
    }

    // -----------------------------------------------------------------------
    // 水域高度渐变（手绘地图呈现 #1，2026-10-15）
    // -----------------------------------------------------------------------

    use super::water_shade_at;

    /// 深度系数端点 + 钳制：海平面 0、深海 −3500 → 1；越界 → 钳。
    #[test]
    fn water_depth_t_endpoints_and_clamps() {
        assert_eq!(crate::palette::watercolor().depth_t(0), 0.0, "海平面应为 0");
        assert!(
            (crate::palette::watercolor().depth_t(-3500) - 1.0).abs() < 1e-6,
            "−3500 m 应为 1.0（满深）"
        );
        assert_eq!(
            crate::palette::watercolor().depth_t(-5000),
            1.0,
            "更深应钳到 1.0"
        );
        assert_eq!(
            crate::palette::watercolor().depth_t(5000),
            0.0,
            "陆上高度应钳到 0.0"
        );
        // 平方曲线中点：sqrt(1/2) = 0.70711 → 深度 −1750 m。
        let mid = crate::palette::watercolor().depth_t(-1750);
        assert!(
            (mid - 0.5_f32.sqrt()).abs() < 1e-6,
            "−1750 m 应为 sqrt(0.5)，实得 {mid}"
        );
        // 平方曲线半程点：sqrt(1/4) = 0.5 → 深度 −875 m（浅水段被展开）。
        let half = crate::palette::watercolor().depth_t(-875);
        assert!((half - 0.5).abs() < 1e-6, "−875 m 应为 0.5，实得 {half}");
    }

    /// 渐变色：海平面 = water_shallow，−3500 = water_deep。中间深度居中。
    #[test]
    fn water_shade_at_lerps_between_shallow_and_deep() {
        let pal = &CellPack::fixture().palette;
        let shallow = pal.core_terrain(1);
        let (Color::Srgba(s), Color::Srgba(_)) = (shallow, Color::srgb(0.0, 0.0, 0.0)) else {
            panic!("应为 Srgba")
        };
        let at_zero = water_shade_at(0, pal, &crate::palette::watercolor());
        let Color::Srgba(z) = at_zero else { panic!() };
        assert!(
            (z.red - s.red).abs() < 1e-4 && (z.green - s.green).abs() < 1e-4,
            "海平面应等于 water_shallow"
        );
        // 2026-10-16：最深深水位（t=1.0）不再 lerp 到 water_deep，直接返回
        // `pal.deep_ocean_blue`——一个明确的深蓝色，不再受主题 CORE_TERRAIN[0]
        // 影响（2026-10-16 用户裁定）。
        let at_max = water_shade_at(-3500, pal, &crate::palette::watercolor());
        let deep_target = pal.deep_ocean_blue;
        let Color::Srgba(m) = at_max else { panic!() };
        let Color::Srgba(d) = deep_target else {
            panic!()
        };
        assert!(
            (m.red - d.red).abs() < 1e-4
                && (m.green - d.green).abs() < 1e-4
                && (m.blue - d.blue).abs() < 1e-4,
            "最深深水位应直接返回 pal.deep_ocean_blue；实得 r={} g={} b={}",
            m.red,
            m.green,
            m.blue
        );
    }

    /// 深海带越深越冷（2026-10-16）：t=1.0 直接走 `deep_ocean_blue`，
    /// 蓝色通道应 ≥ 中段（t=0.7）的 lerp 结果。
    #[test]
    fn water_shade_at_deep_band_gets_cooler_tint() {
        let pal = &CellPack::fixture().palette;
        let at_70pct = water_shade_at(-1715, pal, &crate::palette::watercolor()); // sqrt(1715/3500) = t = 0.7
        let at_100pct = water_shade_at(-3500, pal, &crate::palette::watercolor()); // t = 1.0
        let (Color::Srgba(a), Color::Srgba(b)) = (at_70pct, at_100pct) else {
            panic!()
        };
        assert!(
            b.blue > a.blue,
            "深海带越深越冷（蓝更高），实得 t=0.7 b={} t=1.0 b={}",
            a.blue,
            b.blue
        );
        assert!(
            b.red < a.red,
            "深海带越深红色越压，实得 t=0.7 r={} t=1.0 r={}",
            a.red,
            b.red
        );
    }

    // -----------------------------------------------------------------------
    // 海冰（手绘地图呈现 #5，2026-10-15）
    // -----------------------------------------------------------------------

    /// 纬度阈值：高纬度 y < h × ICE_LATITUDE_T。
    #[test]
    fn ice_latitude_threshold_is_correctly_anchored() {
        use crate::palette::ICE_LATITUDE_T;
        const {
            assert!(
                ICE_LATITUDE_T > 0.0 && ICE_LATITUDE_T < 1.0,
                "纬度阈值应在 (0,1)"
            )
        }
        const { assert!(ICE_LATITUDE_T <= 0.25, "阈值不应超过 25%") }
    }

    /// 深度上限：仅浅于 -200 m 的水域叠冰。
    #[test]
    fn ice_max_depth_threshold_is_anchor() {
        use crate::palette::ICE_MAX_DEPTH_M;
        const { assert!(ICE_MAX_DEPTH_M == 200, "深度上限锚点应为 200 m") }
    }

    /// 海冰色源色：ice_paper 是淡白偏冷（civ-atlas `ICE_PAPER = #EEF1EC`，
    /// 极浅白略偏绿/冷调），与雪 highlight 区分。
    #[test]
    fn ice_paper_is_pale_white() {
        let pal = &CellPack::fixture().palette;
        let Color::Srgba(p) = pal.ice_paper else {
            panic!()
        };
        // 极浅：三个通道都 > 0.9（近白）
        assert!(
            p.red > 0.9 && p.green > 0.9 && p.blue > 0.9,
            "海冰纸面应近白，实得 r={} g={} b={}",
            p.red,
            p.green,
            p.blue
        );
        // 与普通水色（kind=0/1 蓝 > 绿）区分：ice 三个通道都近 1.0，
        // 蓝/绿差异极小（即"白"）；水色则绿/红明显小于蓝。
    }
}
