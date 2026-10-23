//! 地物图集（P3 · T36–T43）。
//!
//! ## 这一层解决什么
//!
//! 地形场给的是「地面」，地图要读得出「地上有什么」——山、树、聚落、地标。
//! 这些是**符号**，不是地形色：真实的地图符号是画上去的图元，有统一画布、
//! 统一锚点、统一投影阴影。
//!
//! ## 与本仓既有装饰的区别（别混）
//!
//! | | `terrain_decor::DecorKind` | 本模块 `SymbolFamily` |
//! |---|---|---|
//! | 语义 | **地面纹理**（森林/丘陵/雪原…按地形类别铺） | **地点符号**（一座山、一片林子、一个城） |
//! | 生成 | 程序化图元逐格铺 | 图集 Sprite，稀疏落点 |
//! | 状态 | 现行默认（`_default`，T43 要求保留） | 位图 pack 能力（P3 新增） |
//!
//! 两者不互斥：地面纹理照样铺，符号叠在上面。T43 的「双轨」说的就是
//! 「程序化符号保留、图集是**追加**能力」，不是替换。
//!
//! ## 规格出处
//!
//! `docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md` T36–T43。本模块落地其中的
//! **可测部分**（画布规格 / 锚点 / 阴影 / 变体确定性 / 符号清单 / 实例化口径）；
//! 美术资产（21 张符号的设计稿）不在代码范围内——这里用**占位图集**把管线
//! 跑通，出图后按 T41 验收「12 个符号缩到 32px 涂黑仍可辨」再换真图。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

// ── T36：画布规格 ────────────────────────────────────────────────────────

/// T36：图集边长（`2048²`，`RGBA8` 透明底）。
pub const ATLAS_SIZE: u32 = 2048;
/// T36：单元边长（`128×128`，规则网格、**无 padding**）。
pub const CELL_PX: u32 = 128;
/// 网格每边单元数（`2048 / 128 = 16`）。
pub const GRID_DIM: u32 = ATLAS_SIZE / CELL_PX;

/// T37：安全边比例。
///
/// 口径说明：T36/T37 写「128px 画布留 25% 安全边」，与 T37 的
/// 「3× 档 = 96px 显示」对照可知——**内容占画布 75%**（每边 12.5%，
/// 即 16px），96/128 = 0.75。若 25% 理解为「每边各 25%」，内容只剩 50%
/// （64px），就与 3× 档的 96px 对不上了。
pub const SAFE_MARGIN_PER_SIDE: f32 = 0.125;
/// 内容区边长（px）= `128 × 0.75 = 96`。
pub const CONTENT_PX: u32 = (CELL_PX as f32 * (1.0 - 2.0 * SAFE_MARGIN_PER_SIDE)) as u32;

// ── T41：符号清单 ────────────────────────────────────────────────────────

/// T41 的四族符号。数量 4 / 5 / 7 / 5 = **21**（Q2 裁定：以清单为准，不凑 24）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolFamily {
    /// 山：孤峰 / 双峰 / 三峰 / 雪山（含受光 + 背光两面）。
    Mountain,
    /// 树：阔叶 / 针叶 / 灌丛 / 枯树 / 椰林。
    Tree,
    /// 聚落：从独屋到城。
    Settlement,
    /// 地标：塔 / 神殿 / 港 / 遗迹 / 关隘。
    Landmark,
}

impl SymbolFamily {
    /// T41 的族内变体数。
    pub const fn count(self) -> u8 {
        match self {
            SymbolFamily::Mountain => 4,
            SymbolFamily::Tree => 5,
            SymbolFamily::Settlement => 7,
            SymbolFamily::Landmark => 5,
        }
    }

    /// 全部族（按图集槽位顺序）。
    pub const ALL: [SymbolFamily; 4] = [
        SymbolFamily::Mountain,
        SymbolFamily::Tree,
        SymbolFamily::Settlement,
        SymbolFamily::Landmark,
    ];

    /// 该族在槽位表里的起始下标。
    pub const fn first_slot(self) -> usize {
        match self {
            SymbolFamily::Mountain => 0,
            SymbolFamily::Tree => 4,
            SymbolFamily::Settlement => 9,
            SymbolFamily::Landmark => 16,
        }
    }

    /// T37 的默认尺寸档。
    ///
    /// 这是**提案值**，需美术验收：山是地标级体量（3×），聚落与地标是可见
    /// 节点（2×），树是纹理级细节（1×）。
    pub const fn size_class(self) -> SizeClass {
        match self {
            SymbolFamily::Mountain => SizeClass::X3,
            SymbolFamily::Tree => SizeClass::X1,
            SymbolFamily::Settlement => SizeClass::X2,
            SymbolFamily::Landmark => SizeClass::X2,
        }
    }
}

/// T37：尺寸档（1× / 2× / 3× 格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    X1,
    X2,
    X3,
}

impl SizeClass {
    /// 该档在屏幕上的显示边长（px）：32 / 64 / 96。
    ///
    /// 3× 档 96px 恰好等于 `CONTENT_PX`——即「3× 档是 1:1 显示，图集不缩放」，
    /// 这也是 128px 画布配 96px 内容区的依据。
    pub const fn display_px(self) -> f32 {
        match self {
            SizeClass::X1 => 32.0,
            SizeClass::X2 => 64.0,
            SizeClass::X3 => 96.0,
        }
    }
}

/// 图集槽位：某族第 `index` 个变体落在网格的 `(col, row)`。
///
/// 槽位沿行主序排布：0..21 依次填 `(0,0)`…`(5,0)` 等，其余 235 格留空
/// （后续扩符号不必重排）。
#[derive(Debug, Clone, Copy)]
pub struct SymbolSlot {
    pub family: SymbolFamily,
    pub index: u8,
    pub col: u32,
    pub row: u32,
}

/// T41：21 个符号的槽位表（编译期生成，长度即清单合计）。
pub const SYMBOLS: [SymbolSlot; 21] = build_slots();

/// 编译期构造槽位表。用 `const fn` 而非手写 21 行：手写容易漏号或重复，
/// 而「清单合计 21」是 Q2 的裁定依据，必须由结构保证。
const fn build_slots() -> [SymbolSlot; 21] {
    let mut out = [SymbolSlot {
        family: SymbolFamily::Mountain,
        index: 0,
        col: 0,
        row: 0,
    }; 21];
    let mut fam = 0;
    while fam < 4 {
        let family = SymbolFamily::ALL[fam];
        let n = family.count();
        let base = family.first_slot();
        let mut i = 0;
        while i < n {
            let slot = base + i as usize;
            out[slot] = SymbolSlot {
                family,
                index: i,
                col: (slot as u32) % GRID_DIM,
                row: (slot as u32) / GRID_DIM,
            };
            i += 1;
        }
        fam += 1;
    }
    out
}

// ── T40：变体选择的确定性 ────────────────────────────────────────────────

/// 变体哈希的盐（T40：`hash01(cx, cy, 0x5EED)`）。
pub const SALT_VARIANT: u64 = 0x5EED;
/// 翻转哈希的盐（T40：`hash01(cx, cy, 0x1)`）。
pub const SALT_FLIP: u64 = 0x1;
/// 尺寸抖动的盐。T40 只写了 `0.9 + hash×0.25` 没指定盐；取一个与上面
/// 两者都不同的常量，避免「尺寸与变体同步变化」导致同变体永远同尺寸。
pub const SALT_SIZE: u64 = 0x517E;

/// 确定性哈希 → `[0,1)`。
///
/// **不用随机数**（T40 / SPEC §3.8.6 守护项 8）：同一 `(cx, cy, salt)` 两次
/// 运行必须逐位一致，否则重开地图符号会跳。
pub fn hash01(cx: i64, cy: i64, salt: u64) -> f32 {
    let mut h = (cx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (cy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB2F)
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9);
    // splitmix64 收尾：雪崩充分，相邻格不相关。
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    // 取高 53 位映射到 [0,1)（f64 尾数精度，再降为 f32）。
    ((h >> 11) as f64 / (1u64 << 53) as f64) as f32
}

/// 一个格位上符号的确定性变体属性。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymbolVariant {
    /// 族内变体下标（`0..family.count()`）。
    pub index: u8,
    /// 是否水平翻转（T40：`hash01(cx,cy,1) > 0.5`）。
    pub flip: bool,
    /// 尺寸抖动倍数（T40：`0.9 + hash×0.25`，即 `[0.9, 1.15)`）。
    pub size_mul: f32,
}

/// 按格坐标算符号变体（T40）。
pub fn variant_for(family: SymbolFamily, cx: i64, cy: i64) -> SymbolVariant {
    let n = family.count();
    let index = ((hash01(cx, cy, SALT_VARIANT) * n as f32) as u8).min(n - 1);
    SymbolVariant {
        index,
        flip: hash01(cx, cy, SALT_FLIP) > 0.5,
        size_mul: 0.9 + hash01(cx, cy, SALT_SIZE) * 0.25,
    }
}

/// 槽位下标（图集里的 `TextureAtlas.index`）。
pub fn slot_index(family: SymbolFamily, variant: u8) -> usize {
    family.first_slot() + (variant % family.count()) as usize
}

// ── T38 锚点 / T39 阴影 ─────────────────────────────────────────────────

/// T38：锚点下移量（世界单位，`cell_px` 为每格屏幕像素时的格尺寸）。
///
/// 口径：锚点是 **bottom_center**（底部中点），它落在**格心偏下 1/8 格**处。
/// 这样符号「站」在格上而不是浮在格心——底边压住落点，视觉重心在格内。
///
/// 返回的世界 Y 偏移是**负值**（Bevy 的 +Y 向上）。
pub fn anchor_offset(cell_px: f32) -> Vec2 {
    Vec2::new(0.0, -cell_px / 8.0)
}

/// T39：阴影不透明度。
pub const SHADOW_ALPHA: f32 = 0.18;
/// T39：阴影位移相对符号高度（含宽高两个方向，统一右下）。
pub const SHADOW_OFFSET_K: f32 = 0.35;

/// T39：阴影位移（世界单位）。统一**右下**：`+X`（右）、`−Y`（屏幕向下）。
///
/// `content_h` 是符号当前显示高度（世界单位）。
pub fn shadow_offset(content_h: f32) -> Vec2 {
    Vec2::new(SHADOW_OFFSET_K * content_h, -SHADOW_OFFSET_K * content_h)
}

/// T39：阴影色（`#000` × α0.18，软阴影非黑块）。
pub fn shadow_color() -> Color {
    Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA)
}

// ── T37b：缩放门控（追记八锁值）─────────────────────────────────────────

/// T37b-2：全局退场阈值——`px_per_cell` 低于此值时全部符号退场。
///
/// 依据：T33 的远观档从 1 px/格 起判，符号与地形色域在同一档位交接。
pub const SYMBOL_EXIT_PX_PER_CELL: f32 = 1.0;
/// T37b-3：淡出带下缘——`0.8 ≤ px_per_cell < 1.0` 线性淡出。
///
/// 带宽 0.2 是「退场阈值 × 20%」：足够覆盖两帧以上的连续缩放（消除
/// 成批消失的跳感），又不至于让符号在半透明状态滞留过久（半透明图标
/// 压在同类图标上会出现深度串色）。
pub const SYMBOL_FADE_START_PX_PER_CELL: f32 = 0.8;

/// T37b：符号缩放门控 alpha（追记八锁值）。
///
/// 返回 `[0, 1]`，语义是「该缩放下符号的不透明度系数」：
///
/// | `px_per_cell` | 返回 | 语义 |
/// |---|---|---|
/// | `< 0.8` | `0.0` | 远观：全退场（T33 色域档） |
/// | `0.8 … 1.0` | 线性 `0 → 1` | 淡出带：消除成批消失的跳感 |
/// | `≥ 1.0` | `1.0` | 正常显示 |
///
/// 分档退场（山 1.0 / 地标 1.0 / 聚落 1.5 / 树 2.5）是**调用方**的口径——
/// 本函数只给全局系数，调用方把它与「族阈值是否已过」相乘。
#[must_use]
pub fn symbol_exit_alpha(px_per_cell: f32) -> f32 {
    let ppc = px_per_cell.max(0.0);
    if ppc >= SYMBOL_EXIT_PX_PER_CELL {
        return 1.0;
    }
    if ppc <= SYMBOL_FADE_START_PX_PER_CELL {
        return 0.0;
    }
    // 线性淡出：α = (ppc − 0.8) / (1.0 − 0.8)
    (ppc - SYMBOL_FADE_START_PX_PER_CELL)
        / (SYMBOL_EXIT_PX_PER_CELL - SYMBOL_FADE_START_PX_PER_CELL)
}

// ── T37c：屏幕密度抽稀（追记十）─────────────────────────────────────────

/// 符号密度基线（px/格）：此缩放以上不再抽稀，全部符号照常显示。
///
/// 取 8：这是「一格约一个符号画布」的档位（T37 的 1× 档 32 px ↔ 4 格，
/// 8 px/格 时一格 8 px、符号 32 px 覆盖 4 格——已是可接受的上限密度）。
pub const SYMBOL_DENSITY_REF_PX: f32 = 8.0;

/// T37c：按缩放算「符号保留比例」，让**屏幕空间**的符号密度保持恒定。
///
/// ## 为什么需要这个（追记十的实测问题）
///
/// T37 规定符号尺寸以**屏幕像素**恒定（32/64/96）。于是：
/// - 符号个数 ∝ 可见格数 ∝ `(屏幕尺寸 / px_per_cell)²` ∝ `1/px_per_cell²`
/// - 每个符号的屏幕面积恒定 `S²`
/// - 所以**屏幕覆盖率 ∝ `1/px_per_cell²`** —— 一缩就爆炸
///
/// 实测症状：512 格图在 2.5 px/格（大陆刚好铺满一屏）时，466 个符号把
/// 大陆盖得只剩零星地形；T37b 的门控只在 `<1 px/格` 才全退，这段是盲区。
///
/// ## 修法：保留比例 ∝ `px_per_cell²`
///
/// `keep = clamp((ppc / REF)², 0, 1)` —— 格数变少多少，就抽掉多少符号，
/// 屏幕上单位面积的符号数因此恒定。`ppc ≥ REF` 时 `keep = 1`（不抽稀）。
///
/// ## 为什么用哈希阈值而不是「每 N 个取一个」
///
/// 调用方拿 `hash01(cx, cy, salt)` 与本函数返回值比较。因为阈值随缩放
/// **单调**变化，保留集是**嵌套**的：放大时符号逐个回来、缩小时逐个退场，
/// **不会重排**（同一个格不会「这个缩放有、下个缩放换成别的格」），
/// 视觉上不闪。按索引取模则会在缩放时整片重排。
#[must_use]
pub fn symbol_density_keep(px_per_cell: f32) -> f32 {
    let ratio = px_per_cell.max(0.0) / SYMBOL_DENSITY_REF_PX;
    (ratio * ratio).clamp(0.0, 1.0)
}

/// T37c 的哈希盐——与变体 / 翻转 / 尺寸抖动的盐互不相同，
/// 否则「被抽掉的符号」会与「某个变体」相关，出现整族消失。
pub const SALT_DENSITY: u64 = 0xD3A5_17E0;

// ── 占位图集 ────────────────────────────────────────────────────────────

/// 一张可写的 8 位 RGBA 画布（仅本模块内部使用）。
struct Canvas {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    /// 写一个像素（**覆盖**，不做混合——占位图只求形状可辨）。
    fn set(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.px[i..i + 4].copy_from_slice(&c);
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, c);
            }
        }
    }

    fn circle(&mut self, cx: i32, cy: i32, r: i32, c: [u8; 4]) {
        for y in (cy - r)..=(cy + r) {
            for x in (cx - r)..=(cx + r) {
                let dx = x - cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// 实心三角形（重心坐标判内）。
    fn tri(&mut self, p: [i32; 2], q: [i32; 2], r: [i32; 2], c: [u8; 4]) {
        let minx = p[0].min(q[0]).min(r[0]);
        let maxx = p[0].max(q[0]).max(r[0]);
        let miny = p[1].min(q[1]).min(r[1]);
        let maxy = p[1].max(q[1]).max(r[1]);
        let area = (q[0] - p[0]) * (r[1] - p[1]) - (r[0] - p[0]) * (q[1] - p[1]);
        if area == 0 {
            return;
        }
        for y in miny..=maxy {
            for x in minx..=maxx {
                let w0 = (q[0] - p[0]) * (y - p[1]) - (x - p[0]) * (q[1] - p[1]);
                let w1 = (r[0] - q[0]) * (y - q[1]) - (x - q[0]) * (r[1] - q[1]);
                let w2 = (p[0] - r[0]) * (y - r[1]) - (x - r[0]) * (p[1] - r[1]);
                let inside = (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0);
                if inside {
                    self.set(x, y, c);
                }
            }
        }
    }
}

const INK: [u8; 4] = [58, 52, 44, 255];
const ROCK: [u8; 4] = [116, 104, 92, 255];
const SNOW: [u8; 4] = [244, 242, 236, 255];
const LEAF: [u8; 4] = [92, 128, 78, 255];
const TRUNK: [u8; 4] = [96, 74, 52, 255];
const ROOF: [u8; 4] = [150, 86, 66, 255];
const WALL: [u8; 4] = [216, 204, 182, 255];
const STONE: [u8; 4] = [168, 160, 148, 255];

/// 在 `100×100` 的内容区里画一个符号（原点 = 单元左上角 + 安全边）。
fn draw_symbol(canvas: &mut Canvas, family: SymbolFamily, index: u8, ox: i32, oy: i32) {
    let m = (CELL_PX as f32 * SAFE_MARGIN_PER_SIDE) as i32; // 16
    let x0 = ox + m;
    let y0 = oy + m;
    let cx = x0 + CONTENT_PX as i32 / 2;
    // 底边取 `CONTENT_PX − 1`：安全区是**半开区间** `[margin, CELL−margin)`，
    // 而 `bot = y0 + CONTENT_PX` 会落在 112 那一行（越界一格）。
    let bot = y0 + CONTENT_PX as i32 - 1;

    match family {
        SymbolFamily::Mountain => {
            // 峰数 = 1 + index%3，第 4 个变体带雪帽。
            let peaks = 1 + (index as i32 % 3);
            let snow = index == 3;
            let spread = CONTENT_PX as i32 / (peaks + 1);
            for p in 0..peaks {
                let pcx = x0 + spread / 2 + p * spread;
                let hgt = CONTENT_PX as i32 - 20 - (p % 2) * 14;
                canvas.tri(
                    [pcx - spread / 2 + 4, bot],
                    [pcx + spread / 2 - 4, bot],
                    [pcx, bot - hgt],
                    ROCK,
                );
                if snow {
                    let cap = hgt / 4;
                    canvas.tri(
                        [pcx - cap / 2, bot - hgt + cap],
                        [pcx + cap / 2, bot - hgt + cap],
                        [pcx, bot - hgt],
                        SNOW,
                    );
                }
            }
        }
        SymbolFamily::Tree => {
            match index {
                // 枯树：只有主干与两根枝。
                3 => {
                    canvas.rect(cx - 3, y0 + 30, cx + 3, bot, TRUNK);
                    canvas.rect(cx - 18, y0 + 44, cx, y0 + 50, TRUNK);
                    canvas.rect(cx, y0 + 34, cx + 16, y0 + 40, TRUNK);
                }
                // 灌丛：低矮多球。
                2 => {
                    canvas.circle(cx - 14, bot - 16, 16, LEAF);
                    canvas.circle(cx + 12, bot - 14, 14, LEAF);
                    canvas.circle(cx, bot - 24, 16, LEAF);
                }
                // 椰林：细高干 + 放射叶。
                4 => {
                    canvas.rect(cx - 3, y0 + 34, cx + 3, bot, TRUNK);
                    for k in 0..4 {
                        let ang = k as f32 * std::f32::consts::FRAC_PI_2;
                        let dx = (ang.cos() * 26.0) as i32;
                        let dy = (ang.sin() * 16.0) as i32;
                        canvas.tri(
                            [cx, y0 + 36],
                            [cx + dx, y0 + 36 - dy],
                            [cx + dx / 3, y0 + 36],
                            LEAF,
                        );
                    }
                }
                // 阔叶 / 针叶：冠形不同。
                _ => {
                    canvas.rect(cx - 4, bot - 26, cx + 4, bot, TRUNK);
                    if index == 0 {
                        canvas.circle(cx, bot - 48, 26, LEAF);
                    } else {
                        canvas.tri(
                            [cx - 24, bot - 26],
                            [cx + 24, bot - 26],
                            [cx, bot - 78],
                            LEAF,
                        );
                    }
                }
            }
        }
        SymbolFamily::Settlement => {
            // 房屋数 = 1 + index/2（1..=4），越大的聚落越多屋。
            //
            // 布局必须**按可用宽度反算屋宽**：写死屋宽会让 3 栋以上的聚落
            // 横向溢出安全区（实测踩过——`Settlement #4` 画到 x=112 越界）。
            // 这里先算总宽再居中，屋宽随栋数收缩。
            let houses = 1 + (index as i32 / 2).min(3);
            let gap = 5;
            let usable = CONTENT_PX as i32 - 16; // 左右各留 8
            let w = ((usable - gap * (houses - 1)) / houses).max(12);
            let total = w * houses + gap * (houses - 1);
            let mut x = cx - total / 2;
            for h in 0..houses {
                let hh = (26 + (h % 2) * 6).min(usable);
                let top = bot - hh;
                canvas.rect(x, top, x + w, bot, WALL);
                // 屋檐压在墙体宽度内（不向外挑），避免横向越界。
                canvas.tri([x, top], [x + w, top], [x + w / 2, top - 14], ROOF);
                x += w + gap;
            }
        }
        SymbolFamily::Landmark => {
            match index {
                // 神殿：柱廊。
                1 => {
                    canvas.rect(cx - 30, bot - 56, cx + 30, bot - 48, STONE);
                    for c in 0..4 {
                        let px = cx - 26 + c * 18;
                        canvas.rect(px, bot - 48, px + 8, bot - 8, WALL);
                    }
                    canvas.rect(cx - 34, bot - 8, cx + 34, bot, STONE);
                }
                // 港：栈桥。
                2 => {
                    canvas.rect(cx - 34, bot - 12, cx + 34, bot - 4, TRUNK);
                    canvas.rect(cx - 4, bot - 40, cx + 4, bot - 12, TRUNK);
                    canvas.tri(
                        [cx + 4, bot - 40],
                        [cx + 4, bot - 18],
                        [cx + 26, bot - 29],
                        WALL,
                    );
                }
                // 遗迹：断柱。
                3 => {
                    canvas.rect(cx - 28, bot - 20, cx + 28, bot, STONE);
                    canvas.rect(cx - 22, bot - 52, cx - 12, bot - 20, WALL);
                    canvas.rect(cx + 6, bot - 38, cx + 16, bot - 20, WALL);
                }
                // 关隘：两塔一梁。
                4 => {
                    canvas.rect(cx - 30, bot - 46, cx - 14, bot, WALL);
                    canvas.rect(cx + 14, bot - 46, cx + 30, bot, WALL);
                    canvas.rect(cx - 30, bot - 46, cx + 30, bot - 36, STONE);
                }
                // 塔：尖顶高塔。
                _ => {
                    canvas.rect(cx - 12, bot - 62, cx + 12, bot, WALL);
                    canvas.tri(
                        [cx - 16, bot - 62],
                        [cx + 16, bot - 62],
                        [cx, bot - 86],
                        ROOF,
                    );
                    canvas.rect(cx - 4, bot - 40, cx + 4, bot - 28, INK);
                }
            }
        }
    }
    let _ = INK;
}

/// 生成**占位图集**：`2048²` RGBA8，21 个符号画在各自的 128 单元里，
/// 内容严格落在 96px 安全区内。
///
/// 这不是美术资产——它的作用是让「图集 → 图集布局 → Sprite 实例化 → 锚点 /
/// 阴影」这条链路能跑起来、能被截图验收。真图接入时只需替换这个函数产出的
/// `Image`，下游代码不动。
pub fn placeholder_atlas() -> Image {
    let mut canvas = Canvas::new(ATLAS_SIZE, ATLAS_SIZE);
    for slot in SYMBOLS.iter() {
        let ox = (slot.col * CELL_PX) as i32;
        let oy = (slot.row * CELL_PX) as i32;
        draw_symbol(&mut canvas, slot.family, slot.index, ox, oy);
    }
    Image::new(
        Extent3d {
            width: canvas.w,
            height: canvas.h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.px,
        // sRGB：符号色是取色器口径的人眼值（与 FieldPalette 的约定一致）。
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T37b-1/2/3（追记八）：门控 alpha 的三段行为。
    #[test]
    fn symbol_exit_alpha_follows_lock_spec() {
        // 远观全退场
        assert_eq!(symbol_exit_alpha(0.4), 0.0);
        assert_eq!(symbol_exit_alpha(0.0), 0.0);
        assert_eq!(symbol_exit_alpha(SYMBOL_FADE_START_PX_PER_CELL), 0.0);
        // 淡出带：线性 0 → 1
        assert!((symbol_exit_alpha(0.9) - 0.5).abs() < 1e-6);
        assert!((symbol_exit_alpha(0.85) - 0.25).abs() < 1e-6);
        assert!((symbol_exit_alpha(0.95) - 0.75).abs() < 1e-6);
        // 正常显示
        assert_eq!(symbol_exit_alpha(1.0), 1.0);
        assert_eq!(symbol_exit_alpha(6.0), 1.0);
    }

    /// T37b-3：淡出带端点必须**恰好**钉在锁值上（0.8 出 0，1.0 出 1），
    /// 且带内单调不减——否则缩放时会出现「消失又浮现」的闪烁。
    #[test]
    fn symbol_exit_alpha_is_monotonic_with_exact_endpoints() {
        let mut prev = -1.0_f32;
        for k in 0..=100 {
            let ppc = 0.7 + k as f32 * 0.006; // 0.7 → 1.3
            let a = symbol_exit_alpha(ppc);
            assert!(a >= prev - 1e-6, "ppc={ppc} 处 alpha 回退：{a} < {prev}");
            prev = a;
        }
        assert_eq!(symbol_exit_alpha(0.8), 0.0);
        assert_eq!(symbol_exit_alpha(1.0), 1.0);
    }

    /// 负值 / NaN 防御：门控在异常输入下必须退场而不是 panic 或出 1。
    #[test]
    fn symbol_exit_alpha_handles_degenerate_input() {
        assert_eq!(symbol_exit_alpha(-0.5), 0.0);
        assert_eq!(symbol_exit_alpha(f32::NEG_INFINITY), 0.0);
        // NaN 走 max(0.0) 后是 0.0 → 落进 ppc <= 0.8 分支
        assert_eq!(symbol_exit_alpha(f32::NAN), 0.0);
    }

    /// T37c：密度保留比例的三个端点 + 单调性。
    #[test]
    fn symbol_density_keep_follows_inverse_square() {
        // 基线以上不抽稀
        assert_eq!(symbol_density_keep(SYMBOL_DENSITY_REF_PX), 1.0);
        assert_eq!(symbol_density_keep(32.0), 1.0);
        // 半缩放 → 1/4
        assert!((symbol_density_keep(4.0) - 0.25).abs() < 1e-6);
        // 大陆全景档（实测出问题的 2.5 px/格）→ 约 1/10
        assert!((symbol_density_keep(2.5) - 0.0977).abs() < 1e-3);
        // 退化输入
        assert_eq!(symbol_density_keep(0.0), 0.0);
        assert_eq!(symbol_density_keep(-1.0), 0.0);
        assert_eq!(symbol_density_keep(f32::NAN), 0.0);
    }

    /// T37c：保留集必须**嵌套**——放大时只增不减、缩小时只减不增。
    ///
    /// 调用方的判据是 `hash01(cx,cy,SALT_DENSITY) < keep(ppc)`；
    /// keep 单调不减于 ppc，故「小 ppc 保留的符号 ⊂ 大 ppc 保留的符号」。
    /// 这条成立才不会出现「缩放时整片符号重排」的闪烁。
    #[test]
    fn density_threshold_is_nested_across_zoom() {
        let cells: Vec<(i64, i64)> = (0..40)
            .flat_map(|y| (0..40).map(move |x| (x * 8, y * 8)))
            .collect();
        let zoom_levels = [1.0_f32, 2.0, 2.5, 4.0, 6.0, 8.0, 16.0];
        let mut prev_kept = std::collections::HashSet::new();
        let mut prev_keep = -1.0;
        for &ppc in &zoom_levels {
            let keep = symbol_density_keep(ppc);
            assert!(keep >= prev_keep - 1e-6, "keep 必须随 ppc 单调不减");
            let kept: std::collections::HashSet<_> = cells
                .iter()
                .filter(|(cx, cy)| hash01(*cx, *cy, SALT_DENSITY) < keep)
                .collect();
            for c in &prev_kept {
                assert!(kept.contains(c), "ppc={ppc} 时丢了上一档保留的符号");
            }
            prev_kept = kept;
            prev_keep = keep;
        }
        // 基线档必须全保留
        assert_eq!(prev_kept.len(), cells.len());
    }

    /// T41 / Q2：清单合计必须是 21，且各族数量是 4/5/7/5。
    #[test]
    fn catalog_has_21_symbols_with_expected_family_counts() {
        assert_eq!(SYMBOLS.len(), 21);
        for fam in SymbolFamily::ALL {
            let n = SYMBOLS.iter().filter(|s| s.family == fam).count();
            assert_eq!(n as u8, fam.count(), "{fam:?} 数量不符");
        }
        let sum: u8 = SymbolFamily::ALL.iter().map(|f| f.count()).sum();
        assert_eq!(sum, 21, "清单合计应为 21（Q2 裁定）");
    }

    /// 槽位沿行主序、互不重复——重复会让两个符号共用一个格。
    #[test]
    fn slots_are_unique_and_row_major() {
        for (i, s) in SYMBOLS.iter().enumerate() {
            assert_eq!(s.col, (i as u32) % GRID_DIM, "槽 {i} 的列不符");
            assert_eq!(s.row, (i as u32) / GRID_DIM, "槽 {i} 的行不符");
        }
        let mut seen = std::collections::HashSet::new();
        for s in SYMBOLS.iter() {
            assert!(seen.insert((s.col, s.row)), "槽位重复");
        }
    }

    /// T37：3× 档的显示尺寸必须等于内容区宽度——这是「128 画布配 96 内容」
    /// 的依据，也是 Q1 那类「两个数打架」的检查点。
    #[test]
    fn three_x_display_matches_content_px() {
        assert_eq!(CONTENT_PX, 96);
        assert_eq!(SizeClass::X3.display_px(), CONTENT_PX as f32);
        assert_eq!(SizeClass::X1.display_px(), 32.0);
        assert_eq!(SizeClass::X2.display_px(), 64.0);
    }

    /// T40：变体属性必须确定——同一格两次调用逐位相同。
    #[test]
    fn variants_are_deterministic() {
        for (cx, cy) in [(0i64, 0i64), (17, -9), (512, 512), (-3, 88)] {
            let a = variant_for(SymbolFamily::Mountain, cx, cy);
            let b = variant_for(SymbolFamily::Mountain, cx, cy);
            assert_eq!(a, b, "({cx},{cy}) 变体不确定");
        }
    }

    /// T40：变体下标必须落在该族的变体数内（否则取到别的族的格子）。
    #[test]
    fn variant_index_stays_within_family() {
        for fam in SymbolFamily::ALL {
            for cx in -40i64..40 {
                for cy in -3i64..3 {
                    let v = variant_for(fam, cx, cy);
                    assert!(
                        v.index < fam.count(),
                        "{fam:?} 变体下标 {} 越界（上限 {}）",
                        v.index,
                        fam.count()
                    );
                    let slot = slot_index(fam, v.index);
                    assert_eq!(SYMBOLS[slot].family, fam, "槽位落到别的族了");
                }
            }
        }
    }

    /// T40：尺寸抖动必须在 `[0.9, 1.15)`；翻转要大致一半一半（不能恒定）。
    #[test]
    fn size_jitter_range_and_flip_is_mixed() {
        let mut flips = 0;
        let mut total = 0;
        for cx in 0i64..60 {
            for cy in 0i64..60 {
                let v = variant_for(SymbolFamily::Tree, cx, cy);
                assert!(
                    (0.9..1.15).contains(&v.size_mul),
                    "尺寸抖动 {} 越界",
                    v.size_mul
                );
                if v.flip {
                    flips += 1;
                }
                total += 1;
            }
        }
        let ratio = flips as f32 / total as f32;
        assert!(
            (0.4..0.6).contains(&ratio),
            "翻转比例 {ratio} 太偏，哈希可能退化"
        );
    }

    /// 相邻格的变体不应大量相同（哈希雪崩不足会让整片区域同一符号）。
    #[test]
    fn adjacent_cells_rarely_share_variant() {
        let mut same = 0;
        let mut total = 0;
        for cx in 0i64..60 {
            for cy in 0i64..60 {
                let a = variant_for(SymbolFamily::Settlement, cx, cy).index;
                let b = variant_for(SymbolFamily::Settlement, cx + 1, cy).index;
                if a == b {
                    same += 1;
                }
                total += 1;
            }
        }
        // 7 个变体下随机期望 ≈ 1/7 ≈ 14%；留一倍余量。
        let ratio = same as f32 / (total as f32);
        assert!(ratio < 0.3, "相邻格同变体比例过高：{ratio}");
    }

    /// T38：锚点落在格心**偏下 1/8 格**。
    #[test]
    fn anchor_is_eighth_cell_below_center() {
        let off = anchor_offset(64.0);
        assert_eq!(off.x, 0.0);
        assert!(
            (off.y + 8.0).abs() < 1e-6,
            "应为 −64/8 = −8，实得 {}",
            off.y
        );
    }

    /// T39：阴影统一右下、α 0.18。
    #[test]
    fn shadow_goes_down_right_with_fixed_alpha() {
        let off = shadow_offset(40.0);
        assert!(off.x > 0.0, "阴影应向右");
        assert!(off.y < 0.0, "阴影应向下（Bevy +Y 朝上，故为负）");
        assert!((off.x - 14.0).abs() < 1e-6 && (off.y + 14.0).abs() < 1e-6);
        assert!((SHADOW_ALPHA - 0.18).abs() < 1e-6);
    }

    /// 占位图集：尺寸对不对，以及**每个符号的不透明像素都落在自己的安全区内**
    /// ——越界会压到邻格符号（无 padding 的规则网格没有缓冲带）。
    #[test]
    fn placeholder_atlas_keeps_content_inside_safe_area() {
        let img = placeholder_atlas();
        assert_eq!(img.width(), ATLAS_SIZE);
        assert_eq!(img.height(), ATLAS_SIZE);
        let data = img.data.as_ref().expect("占位图集应有像素数据");
        assert_eq!(data.len(), (ATLAS_SIZE * ATLAS_SIZE * 4) as usize);

        let margin = (CELL_PX as f32 * SAFE_MARGIN_PER_SIDE) as u32;
        for (i, slot) in SYMBOLS.iter().enumerate() {
            let ox = slot.col * CELL_PX;
            let oy = slot.row * CELL_PX;
            let mut n = 0usize;
            for y in 0..CELL_PX {
                for x in 0..CELL_PX {
                    let gi = (((oy + y) * ATLAS_SIZE + (ox + x)) * 4 + 3) as usize;
                    if data[gi] > 0 {
                        n += 1;
                        let inside = x >= margin
                            && x < CELL_PX - margin
                            && y >= margin
                            && y < CELL_PX - margin;
                        assert!(
                            inside,
                            "槽 {i}（{:?} #{}）在单元内 ({x},{y}) 越出安全区",
                            slot.family, slot.index
                        );
                    }
                }
            }
            assert!(n > 100, "槽 {i}（{:?}）几乎没画出东西", slot.family);
        }
    }

    /// 占位图集必须**确定**：两次生成逐字节一致（T40 / 守护项 8）。
    #[test]
    fn placeholder_atlas_is_byte_identical_across_runs() {
        let a = placeholder_atlas();
        let b = placeholder_atlas();
        assert_eq!(a.data, b.data);
    }
}
