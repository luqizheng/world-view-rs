//! 像素合成：`CellRenderer` + `shade_*` + 边界扰动 + `image_from_terrain` / `patch_terrain_image`。
//!
//! ## 职责
//! - `CellRenderer` —— 单元渲染器（全图合成与增量 patch **必须共用**）。
//! - `shade_*` —— 视图模式 → 着色函数（`shade_real` / `shade_google` / `shade_flat` / `shade_faded` / `shade_event` / `shade_coast`），随主题色板（`PaletteColors`，ADR-0016）取值。
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

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use wv_core::terrain::TerrainData;

use crate::state::ViewMode;
use crate::theme::colors;
use crate::theme::colors_google;
use crate::theme::palette::PaletteColors;

use super::terrain_texture::terrain_res_for;
use super::tile_atlas::{
    blit_block_tinted, sample_tile_block, tile_tint_for_mode, TileAtlas, TileTint,
};

/// 把一个着色函数应用到一格，返回 sRGB 线性分量。
///
/// 第三参 = 当前主题色板（ADR-0016）：主题相关色一律经它取，禁止直引常量。
type Shader = fn(&TerrainData, u8, &PaletteColors) -> Color;

/// 依据当前视图模式选择纯色着色函数（基线 §1.10 四态；图集缺项/无图集时也走这里）。
fn shade_for_mode(mode: ViewMode) -> Shader {
    match mode {
        // 真实地形：按高度调制明度，制造立体感（法线光照的廉价近似）。
        ViewMode::RealTerrain => shade_real,
        // 色块地形：纯色平涂。
        ViewMode::Schematic => shade_flat,
        // 势力图：原型阶段底色淡化地形（势力几何属 S12，本轮不画）。
        ViewMode::Faction => shade_faded,
        // 事件图：极简灰白底图。
        ViewMode::Event => shade_event,
        // 轮廓图：水陆二分深色 + 海岸线（海岸线在边界格逐像素画，见
        // `write_coast_boundary_cell`；这里只管内部格的水/陆平涂）。
        ViewMode::Coastline => shade_coast,
        // Google Default Map：近白陆地 + 淡水 + 绿地（色板见 palette_google）。
        ViewMode::GoogleMap => shade_google,
        // 等值线矢量平涂（ADR-0012 / plan v0.1 M2）：内部格走本 shader 的
        // 类型色平涂；边界格的水陆由 `sample_vector_flat` 的双线性高度采样
        // 决定——填充边与 `isoline_overlay` 的等值线描边同源吻合。
        ViewMode::VectorFlat => shade_flat,
    }
}

fn shade_google(_data: &TerrainData, kind: u8, _pal: &PaletteColors) -> Color {
    // Google Default Map 做普通地图折叠（2026-10-13 用户裁定）：
    // 不渲染地形高低分带。Google 普通地图的陆地是单一近白平涂；若把
    // Perlin 高度分带（沙/草/丘/山/雪）逐档着色，整图会退化成近色马赛克。
    // 折叠规则：水两档保留层次、森林作公园绿斑、沼泽保留湿绿，
    // 其余陆上类一律折叠为陆地基色（`GOOGLE_CORE_TERRAIN[2]` 近白陆色）。
    match kind {
        0 | 1 | 4 | 8 => colors_google::google_core_terrain(kind),
        _ => colors_google::GOOGLE_CORE_TERRAIN[2],
    }
}

fn shade_flat(_data: &TerrainData, kind: u8, pal: &PaletteColors) -> Color {
    pal.core_terrain(kind)
}

fn shade_real(data: &TerrainData, kind: u8, pal: &PaletteColors) -> Color {
    // 用 kind 对应的基色，叠加高度明度调制。
    // 这里 data 已无单格高度上下文，故高度调制在 write_terrain_pixels 里做。
    let _ = data;
    pal.core_terrain(kind)
}

fn shade_faded(_data: &TerrainData, kind: u8, pal: &PaletteColors) -> Color {
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

fn shade_event(_data: &TerrainData, _kind: u8, pal: &PaletteColors) -> Color {
    pal.event_backdrop
}

/// 水系判定（轮廓图唯一关心的二分）：深水/浅水算水，其余全算陆。
///
/// 口径委托给 [`wv_terrain::edit::is_water_kind`]（笔刷高度域对齐共用同一判定，
/// 单一真源；见该函数文档）。
fn is_water_kind(kind: u8) -> bool {
    wv_terrain::edit::is_water_kind(kind)
}

fn shade_coast(_data: &TerrainData, kind: u8, _pal: &PaletteColors) -> Color {
    // 轮廓图三色是模式语义色，不随主题切换（ADR-0016 §2）。
    if is_water_kind(kind) {
        colors::COAST_WATER
    } else {
        colors::COAST_LAND
    }
}

// ---------------------------------------------------------------------------
// 水域按高度的渐变（2026-10-15，手绘地图呈现改进 #1）
// ---------------------------------------------------------------------------

/// 高度域 → 颜色 lerp 系数 [0, 1]：
/// 0 = 海平面（最浅），1 = 深海（最暗）。
///
/// 端口区间按 civ-atlas 的 `sea` 频谱：近岸 0 段 / 浅海 −200 / 大陆坡拐点
/// −1500 / 老洋底 −3000。钳到 [−3500, 0] 之外 → 0 或 1。
fn water_depth_t(height: i16) -> f32 {
    // 把 i16 高度转成「深度」（正值 = 越深）。
    let depth = (-height as f32).clamp(0.0, 3500.0);
    depth / 3500.0
}

/// 水域按高度的渐变色——lerp `water_shallow → water_deep`，并对深色带
/// 提一点蓝饱和（海沟观感）。所有视图模式下的水域**内部格**都走这条；
/// 边界格的双线性高度采样会通过 `sample_vector_flat` 拿到亚格高度，
/// 但单格渲染阶段我们只能用该格的 `cell.height`（一格一种颜色，与 civ-atlas
/// 同样离散），从远处看仍是渐变观感。
///
/// 海岸/轮廓模式不进这里（`shade_coast` 单独出口）。
///
/// **最深深水位（`t = 1.0`，约 `height ≤ -3500 m`）直接返回 [`PaletteColors::deep_ocean_blue`]**：
/// 不再走主题里的「深海」色 + 冷偏移。原因：旧路径过 `soften (35% keep)` +
/// 远缩放 `veil` 后会退成中性灰（约 `(210, 213, 212)`），肉眼辨不出「海」
///（2026-10-16 用户反馈）。新版用一个语义「深海」色，无论缩放 / 主题
/// 怎么叠都明确深蓝。`MapBase::Ocean.cell()` 的 `height = -3500` 命中
/// 这一分支——所以选了「海洋」的用户立刻看到深蓝。
fn water_shade_at(height: i16, pal: &PaletteColors) -> Color {
    let t = water_depth_t(height);
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

/// 整数格点确定性 hash → `[0,1)`。无依赖、跨平台一致（测试与运行共用）。
fn hash01(xi: i64, yi: i64, seed: u64) -> f32 {
    let mut h = (xi as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (yi as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h & 0xFFFF_FFFF) as f32 / u32::MAX as f32
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
    let over = (height as f32 - colors::SNOW_SHADOW_MIN_HEIGHT as f32).max(0.0);
    let t = over / 1000.0 * colors::SNOW_SHADOW_PER_KM;
    t.clamp(0.0, 1.0)
}

/// 雪格按高度叠冷蓝灰阴影（手绘地图 #3）。
/// 高度低于 `SNOW_SHADOW_MIN_HEIGHT` → 走 `snow_highlight`（比 core_terrain[7] 略亮的近白），
/// 高于阈值则按 `snow_shadow_t` 在 highlight ↔ shadow 之间 lerp。
fn snow_shade_at(height: i16, pal: &PaletteColors) -> Color {
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
    mode: ViewMode,
    /// 当前主题色板（ADR-0016）：着色与淡化的取值来源。
    palette: &'a PaletteColors,
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
    /// 装饰旗标（ADR-0023 D4：`ThemePack.treatment`）。HandDrawn 全 `true` 时
    /// 所有 gate 与历史恒开行为逐位一致。
    treatment: crate::theme::theme_pack::TreatmentFlags,
    /// `kind → res² 瓦片块`（按需填充；`None` = 尚未尝试）。
    blocks: [Option<Vec<u8>>; 10],
    /// `kind` 是否已尝试过建块（避免无映射 kind 被反复查）。
    tried: [bool; 10],
}

impl<'a> CellRenderer<'a> {
    fn new(
        data: &'a TerrainData,
        mode: ViewMode,
        atlas: Option<&'a TileAtlas>,
        soften: bool,
        palette: &'a PaletteColors,
        treatment: crate::theme::theme_pack::TreatmentFlags,
    ) -> Self {
        let (w, h) = (data.width.max(1), data.height.max(1));
        // 事件图与轮廓图都不铺瓦片：前者灰白底，后者水陆三色。
        let atlas = if matches!(mode, ViewMode::Event | ViewMode::Coastline) {
            None
        } else {
            atlas
        };
        // 事件图与轮廓图不铺瓦片；Google 模式无图集也使用亚像素分辨率。
        let res = terrain_res_for(data, mode, atlas);
        let (hmin, hmax) = if mode == ViewMode::RealTerrain {
            data.cells.iter().fold((i16::MAX, i16::MIN), |(lo, hi), c| {
                (lo.min(c.height), hi.max(c.height))
            })
        } else {
            (0, 0)
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
            soften: soften && !matches!(mode, ViewMode::Coastline | ViewMode::GoogleMap),
            // 等值线模式（VectorFlat）也关闭揉边：它的自然蜿蜒由
            // `isoline_overlay` 的等值线描边承担，逐像素噪声会和几何曲线打架。
            // 主题装饰旗标（ADR-0023 D4）是第三道闸：EMPTY 类主题整体关揉边。
            wobble: treatment.boundary_wobble
                && !matches!(mode, ViewMode::GoogleMap | ViewMode::VectorFlat),
            treatment,
            blocks: std::array::from_fn(|_| None),
            tried: [false; 10],
        }
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

    /// 高度明度系数（仅 `RealTerrain` 非 1）。
    fn height_mul(&self, height: i16) -> f32 {
        if self.mode != ViewMode::RealTerrain {
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
        let kind0 = self.data.cells[(cy * self.w + cx) as usize].kind;
        let differs = |a: u8, b: u8| {
            if self.mode == ViewMode::Coastline {
                is_water_kind(a) != is_water_kind(b)
            } else if self.mode == ViewMode::GoogleMap {
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
        if self.mode == ViewMode::VectorFlat {
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
                if self.mode == ViewMode::VectorFlat {
                    hmin = hmin.min(cell.height);
                    hmax = hmax.max(cell.height);
                }
                if differs(kind0, cell.kind) {
                    return true;
                }
            }
        }
        // 高度跨海平面（Q1：海平面 = 0，负值水下）。
        self.mode == ViewMode::VectorFlat && hmin < 0 && hmax >= 0
    }

    /// 在连续纹理像素坐标取一次颜色样本（写入 `out`，RGB，0..255）。
    ///
    /// 落点跨出地图时钳到边缘格——扰动把边缘格的采样点推出去是常态，不是错误。
    fn sample_into(&self, fx: f32, fy: f32, out: &mut [f32; 3]) {
        let resf = self.res as f32;
        // 等值线模式：水陆由双线性高度采样决定（与等值线描边同源，D2）。
        if self.mode == ViewMode::VectorFlat {
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
        let kind = self.data.cells[(cy * self.w + cx) as usize].kind;
        let land = height >= 0.0;
        // 类型 ↔ 高度矛盾时高度赢（D2）；回退色取「岸」语义的沙滩 / 深水。
        let kind = if land && kind <= 1 {
            2 // 沙
        } else if !land && kind > 1 {
            0 // 深水
        } else {
            kind
        };
        let Color::Srgba(c) = (self.shader)(self.data, kind, self.palette) else {
            return;
        };
        out[0] = c.red * 255.0;
        out[1] = c.green * 255.0;
        out[2] = c.blue * 255.0;
    }

    /// 通用模式采样：最近格着色 / 瓦片块，供 `sample_into` 分派。
    fn sample_kind(&self, gx: u32, gy: u32, fx: f32, fy: f32, out: &mut [f32; 3]) {
        let resf = self.res as f32;
        let cell = &self.data.cells[(gy * self.w + gx) as usize];
        let kind = cell.kind;
        let mul = self.height_mul(cell.height);
        if let Some(block) = self.blocks.get(kind as usize).and_then(|b| b.as_deref()) {
            let r = self.res as usize;
            let u = (fx / resf - gx as f32).clamp(0.0, 0.999);
            let v = (fy / resf - gy as f32).clamp(0.0, 0.999);
            let tx = ((u * r as f32) as usize).min(r - 1);
            let ty = ((v * r as f32) as usize).min(r - 1);
            let o = (ty * r + tx) * 4;
            out[0] = block[o] as f32 * mul;
            out[1] = block[o + 1] as f32 * mul;
            out[2] = block[o + 2] as f32 * mul;
        } else {
            if let Color::Srgba(c) = (self.shader)(self.data, kind, self.palette) {
                out[0] = c.red * 255.0 * mul;
                out[1] = c.green * 255.0 * mul;
                out[2] = c.blue * 255.0 * mul;
            }
        }
    }

    /// 连续纹理像素坐标处的水陆判定（轮廓图海岸线专用；坐标口径同 `sample_into`）。
    fn sample_is_water(&self, fx: f32, fy: f32) -> bool {
        let resf = self.res as f32;
        let gx = ((fx / resf).floor() as i32).clamp(0, self.w as i32 - 1) as u32;
        let gy = ((fy / resf).floor() as i32).clamp(0, self.h as i32 - 1) as u32;
        let kind = self.data.cells[(gy * self.w + gx) as usize].kind;
        is_water_kind(kind)
    }

    /// 轮廓图边界格：逐像素扰动 + 4 样本超采样的**水陆占比**判定。
    ///
    /// - 全水 → 水色；全陆 → 陆色；
    /// - 混合（即海岸线穿过该像素）→ 按「靠线程度」向 `COAST_LINE` 混合：
    ///   4 样本 2/2 分 = 完整线色，3/1 分 = 半强度（天然抗锯齿）。
    ///
    /// 与 `write_boundary_cell` 共用同一 `wobble_px` 扰动——同一条海岸线在
    /// 全图合成与增量 patch 中揉出的曲线逐字节一致。
    fn write_coast_boundary_cell(&self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        let to_rgb = |c: Color| -> [f32; 3] {
            let Color::Srgba(s) = c else { return [0.0; 3] };
            [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
        };
        let water = to_rgb(colors::COAST_WATER);
        let land = to_rgb(colors::COAST_LAND);
        let line = to_rgb(colors::COAST_LINE);
        let res = self.res;
        for ty in 0..res {
            for tx in 0..res {
                let ox = (cx * res + tx) as f32;
                let oy = (cy * res + ty) as f32;
                let (wx, wy) = wobble_px(ox, oy, res);
                let mut n_water = 0u32;
                let mut n_land = 0u32;
                for s in SUBSAMPLES {
                    if self.sample_is_water(ox + s[0] + wx, oy + s[1] + wy) {
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
                let o = ((cy * res + ty) * stride + cx * res + tx) as usize * 4;
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
    fn write_boundary_cell(&mut self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        // 轮廓图：海岸线有专用路径（水陆占比判定），不走颜色混合。
        if self.mode == ViewMode::Coastline {
            self.write_coast_boundary_cell(pixels, stride, cx, cy);
            return;
        }
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                    continue;
                }
                let kind = self.data.cells[(ny as u32 * self.w + nx as u32) as usize].kind;
                self.block_for(kind);
            }
        }
        let res = self.res;
        for ty in 0..res {
            for tx in 0..res {
                let ox = (cx * res + tx) as f32;
                let oy = (cy * res + ty) as f32;
                let (wx, wy) = if self.wobble {
                    wobble_px(ox, oy, res)
                } else {
                    (0.0, 0.0)
                };
                let (mut r, mut g, mut b) = (0.0f32, 0.0f32, 0.0f32);
                let mut c = [0.0f32; 3];
                for s in SUBSAMPLES {
                    self.sample_into(ox + s[0] + wx, oy + s[1] + wy, &mut c);
                    r += c[0];
                    g += c[1];
                    b += c[2];
                }
                let o = ((cy * res + ty) * stride + cx * res + tx) as usize * 4;
                pixels[o] = (r / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 1] = (g / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 2] = (b / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 3] = 255;
            }
        }
    }

    /// 写一格像素：内部格快路径 / 边界格扰动路径。
    fn write_cell(&mut self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        let idx = (cy * self.w + cx) as usize;
        let Some(cell) = self.data.cells.get(idx) else {
            return;
        };
        if !self.is_boundary(cx, cy) {
            // 内部格：与旧行为一致——整块直铺（快路径）。
            let kind = cell.kind;
            if self.block_for(kind).is_some() {
                let tint = if self.mode == ViewMode::RealTerrain {
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
                        cy * self.res,
                    );
                }
                return;
            }
            // 纯色回退（无图集 / 缺项）：整格平涂。
            let mut color = (self.shader)(self.data, kind, self.palette);
            // 水域按高度渐变（手绘地图 #1）：kind 0/1 的内部格从单色水变
            // height 调度的水彩带；轮廓图模式不进这里。
            if self.treatment.water_gradient
                && is_water_kind(kind)
                && self.mode != ViewMode::Coastline
            {
                color = water_shade_at(cell.height, self.palette);
            }
            // 雪格双调（手绘地图 #3）：kind=7 全部走 snow_shade_at（RealTerrain
            // 模式之外也给 highlight 视觉——海报感的"近白雪"）。轮廓图模式不动。
            if self.treatment.snow_shade && kind == 7 && self.mode != ViewMode::Coastline {
                color = snow_shade_at(cell.height, self.palette);
            }
            if self.soften {
                color = self.palette.soften_color(color);
            }
            if self.mode == ViewMode::RealTerrain {
                let mul = self.height_mul(cell.height);
                let Color::Srgba(c) = color else { return };
                color = Color::srgb(
                    (c.red * mul).clamp(0.0, 1.0),
                    (c.green * mul).clamp(0.0, 1.0),
                    (c.blue * mul).clamp(0.0, 1.0),
                );
            }
            let Color::Srgba(c) = color else { return };
            let res = self.res;
            for ty in 0..res {
                let o = ((cy * res + ty) * stride + cx * res) as usize * 4;
                for tx in 0..res {
                    let o = o + tx as usize * 4;
                    pixels[o] = (c.red * 255.0) as u8;
                    pixels[o + 1] = (c.green * 255.0) as u8;
                    pixels[o + 2] = (c.blue * 255.0) as u8;
                    pixels[o + 3] = 255;
                }
            }
            return;
        }
        self.write_boundary_cell(pixels, stride, cx, cy);
    }
}

/// 把整张 `TerrainData` 写成 RGBA8 像素（每逻辑格 `res` 像素见方）。
///
/// 有图集且该格 kind 有映射 → 瓦片直铺（内部格）/ 扰动超采样（边界格）；
/// 否则回退纯色 shader。返回 `(pixels, res)`。
fn write_terrain_pixels(
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    palette: &PaletteColors,
    treatment: crate::theme::theme_pack::TreatmentFlags,
) -> (Vec<u8>, u32) {
    let mut renderer = CellRenderer::new(data, mode, atlas, soften, palette, treatment);
    let (w, h, res) = (renderer.w, renderer.h, renderer.res);
    let stride = w * res;
    let mut pixels = vec![0u8; (stride * h * res * 4) as usize];
    for cy in 0..h {
        for cx in 0..w {
            renderer.write_cell(&mut pixels, stride, cx, cy);
        }
    }
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
pub fn image_from_terrain(
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
    palette: &PaletteColors,
    treatment: crate::theme::theme_pack::TreatmentFlags,
) -> Image {
    let (pixels, res) = write_terrain_pixels(data, mode, atlas, soften, palette, treatment);
    let (w, h) = (data.width.max(1), data.height.max(1));
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

/// 把 `data` 的 dirty 矩形 patch 进**已有 Image**（原地改 `image.data` 切片）。
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
#[allow(clippy::too_many_arguments)]
pub fn patch_terrain_image(
    image: &mut Image,
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
    dirty_cell_rect: [u32; 4],
    res: u32,
    soften: bool,
    palette: &PaletteColors,
    treatment: crate::theme::theme_pack::TreatmentFlags,
) {
    let (w, h) = (data.width.max(1), data.height.max(1));
    let stride = w * res; // 像素 stride
    let [gx0, gy0, gx1, gy1] = dirty_cell_rect;
    // 钳到 [0, w/h]（防御性）。
    let gx1 = gx1.min(w);
    let gy1 = gy1.min(h);

    // 与全图合成共用一个渲染器（边界扰动必须一致，否则笔刷区出现拼接裂缝）。
    let mut renderer = CellRenderer::new(data, mode, atlas, soften, palette, treatment);
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

    /// 像素颜色是否接近目标 sRGB 色（±2/255，吸收 u8 舍入）。
    fn pixel_near(img: &Image, x: u32, y: u32, target: bevy::color::Srgba) -> bool {
        let data = img.data.as_ref().expect("地形图必须有像素数据");
        let w = img.width();
        let o = ((y * w + x) * 4) as usize;
        (data[o] as f32 / 255.0 - target.red).abs() < 0.02
            && (data[o + 1] as f32 / 255.0 - target.green).abs() < 0.02
            && (data[o + 2] as f32 / 255.0 - target.blue).abs() < 0.02
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
            snow_shadow_t(crate::theme::colors::SNOW_SHADOW_MIN_HEIGHT),
            0.0,
            "恰阈值处无阴影"
        );
        let t = snow_shadow_t(crate::theme::colors::SNOW_SHADOW_MIN_HEIGHT + 1000);
        assert!(
            (t - crate::theme::colors::SNOW_SHADOW_PER_KM).abs() < 1e-6,
            "+1 km 应为 per_km，实得 {t}"
        );
        assert_eq!(snow_shadow_t(i16::MAX), 1.0, "高海拔应封顶 1.0");
    }

    /// 雪格 highlight：低海拔雪 = 近白雪（与 core_terrain[7] 提亮一档同族但更亮）。
    #[test]
    fn snow_shade_at_low_elevation_is_highlight() {
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
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
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
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
                    kind: 3,
                },
                wv_core::terrain::TerrainCellData {
                    height: -50,
                    kind: 3,
                },
            ],
        };
        let img = image_from_terrain(
            &data,
            ViewMode::VectorFlat,
            None,
            false,
            crate::theme::palette::colors(Default::default()),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );
        let Color::Srgba(grass) = colors::core_terrain(3) else {
            unreachable!()
        };
        let Color::Srgba(deep) = colors::core_terrain(0) else {
            unreachable!()
        };
        // 2 格 ≤1024 → res = clamp(4096/2, 4, 8) = 8：图像 16×8。
        // 等值线在格缘 gx=1.0 → 像素 x∈0..8 陆、x∈8..16 水。
        assert!(pixel_near(&img, 2, 2, grass), "左半应为草色");
        assert!(
            pixel_near(&img, 12, 2, deep),
            "右半应为深水色（最近格为草地、高度判水 → 回退深水）"
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
                    kind: 0,
                };
                9
            ],
        };
        let img = image_from_terrain(
            &data,
            ViewMode::VectorFlat,
            None,
            false,
            crate::theme::palette::colors(Default::default()),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );
        // 深水渐变（手绘地图 #1）启用后：深水中心像素 = water_shade_at(−3500)，
        // 不再是 core_terrain(0) 原色。统一从 water_shade_at 取色源，避免硬编码颜色。
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
        let Color::Srgba(deep) = water_shade_at(-3500, pal) else {
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
        let vector = terrain_res_for(&data, ViewMode::VectorFlat, None);
        let google = terrain_res_for(&data, ViewMode::GoogleMap, None);
        assert_eq!(vector, google, "VectorFlat 应与 GoogleMap 同口径");
        assert!(vector > 1, "512² 图应取 {vector}px/格（亚像素）");
    }

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
            ViewMode::VectorFlat,
            None,
            // 默认 soften=true 也要清晰蓝——这是用户最直观的视觉预期。
            true,
            crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
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
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
        let img = image_from_terrain(
            &data,
            ViewMode::VectorFlat,
            None,
            true,
            pal,
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
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
                    kind: 0,
                };
                16
            ],
        };
        let img = image_from_terrain(
            &data,
            ViewMode::Schematic,
            None,
            true,
            crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
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
        let palette = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
        let ocean_img = image_from_terrain(
            &ocean_data,
            ViewMode::VectorFlat,
            None,
            true,
            palette,
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );
        let land_img = image_from_terrain(
            &land_data,
            ViewMode::VectorFlat,
            None,
            true,
            palette,
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );

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
        let paper = palette.soften_toward;
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
        });
        let (map, data) = crate::terrain_view::active_terrain_or_placeholder(
            &mut world,
            Some(&Id::new_unchecked("world_main")),
            64,
        );

        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            &data,
            ViewMode::VectorFlat,
            None,
            false,
            crate::theme::palette::colors(Default::default()),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );
        let tex = crate::terrain_view::new_terrain_texture(
            &mut images,
            image,
            &data,
            map.clone(),
            ViewMode::VectorFlat,
            false,
            Default::default(),
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
                painted.cells[gy * 64 + gx].kind = 0;
            }
        }
        world.set_terrain(Id::new_unchecked("world_main"), painted);

        let after = world.terrain_of(&Id::new_unchecked("world_main")).unwrap();
        assert!(after.cells[4 * 64 + 4].height < 0, "海格高度应在海平面下");
        assert_eq!(after.cells[4 * 64 + 4].kind, 0);

        // 全图重建（同 `flush_terrain_texture` 的整图分支）。
        let new_image = image_from_terrain(
            after,
            ViewMode::VectorFlat,
            None,
            false,
            crate::theme::palette::colors(Default::default()),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
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
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
        let Color::Srgba(deep) = water_shade_at(-40, pal) else {
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
        });
        let (map, data) = crate::terrain_view::active_terrain_or_placeholder(
            &mut world_core,
            Some(&Id::new_unchecked("world_main")),
            64,
        );

        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            &data,
            ViewMode::VectorFlat,
            None,
            false,
            crate::theme::palette::colors(Default::default()),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );
        let mut tex = crate::terrain_view::new_terrain_texture(
            &mut images,
            image,
            &data,
            map.clone(),
            ViewMode::VectorFlat,
            false,
            Default::default(),
        );
        let expected_handle = tex.handle.clone();
        // 二次写入（NewMap / LoadProject 的真实路径）。
        let image2 = image_from_terrain(
            &data,
            ViewMode::VectorFlat,
            None,
            false,
            crate::theme::palette::colors(Default::default()),
            crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
        );
        crate::terrain_view::replace_terrain_texture(
            &mut tex,
            &mut images,
            image2,
            &data,
            map.clone(),
            ViewMode::VectorFlat,
            false,
            Default::default(),
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

    use super::water_depth_t;
    use super::water_shade_at;

    /// 深度系数端点 + 钳制：海平面 0、深海 −3500 → 1；越界 → 钳。
    #[test]
    fn water_depth_t_endpoints_and_clamps() {
        assert_eq!(water_depth_t(0), 0.0, "海平面应为 0");
        assert!(
            (water_depth_t(-3500) - 1.0).abs() < 1e-6,
            "−3500 m 应为 1.0（满深）"
        );
        assert_eq!(water_depth_t(-5000), 1.0, "更深应钳到 1.0");
        assert_eq!(water_depth_t(5000), 0.0, "陆上高度应钳到 0.0");
        // 中点：−1750 → 0.5。
        let mid = water_depth_t(-1750);
        assert!((mid - 0.5).abs() < 1e-6, "−1750 m 应为 0.5，实得 {mid}");
    }

    /// 渐变色：海平面 = water_shallow，−3500 = water_deep。中间深度居中。
    #[test]
    fn water_shade_at_lerps_between_shallow_and_deep() {
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
        let shallow = pal.core_terrain(1);
        let (Color::Srgba(s), Color::Srgba(_)) = (shallow, Color::srgb(0.0, 0.0, 0.0)) else {
            panic!("应为 Srgba")
        };
        let at_zero = water_shade_at(0, pal);
        let Color::Srgba(z) = at_zero else { panic!() };
        assert!(
            (z.red - s.red).abs() < 1e-4 && (z.green - s.green).abs() < 1e-4,
            "海平面应等于 water_shallow"
        );
        // 2026-10-16：最深深水位（t=1.0）不再 lerp 到 water_deep，直接返回
        // `pal.deep_ocean_blue`——一个明确的深蓝色，不再受主题 CORE_TERRAIN[0]
        // 影响（2026-10-16 用户裁定）。
        let at_max = water_shade_at(-3500, pal);
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
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
        let at_70pct = water_shade_at(-2450, pal); // t = 0.7
        let at_100pct = water_shade_at(-3500, pal); // t = 1.0
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
        use crate::theme::colors::ICE_LATITUDE_T;
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
        use crate::theme::colors::ICE_MAX_DEPTH_M;
        const { assert!(ICE_MAX_DEPTH_M == 200, "深度上限锚点应为 200 m") }
    }

    /// 海冰色源色：ice_paper 是淡白偏冷（civ-atlas `ICE_PAPER = #EEF1EC`，
    /// 极浅白略偏绿/冷调），与雪 highlight 区分。
    #[test]
    fn ice_paper_is_pale_white() {
        let pal = crate::theme::palette::colors(crate::theme::palette::MapPalette::Daylight);
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
