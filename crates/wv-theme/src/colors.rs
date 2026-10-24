//! 待批准：本文件在 `app` feature 下才编译。
//! 色板令牌 —— `ART_STYLE_SPEC_v0.1.md` §1.4 的 **28 色**骨架值的唯一代码载体。
//!
//! ## 纪律
//!
//! - **色彩唯一权威源是 `docs/ART_STYLE_SPEC_v0.1.md`**（基线 §3.1）。
//!   本文件是它的**投影**，不是第二权威——若两者冲突，改本文件。
//! - **2026-10-14 起（ADR-0016）本文件承载「日间古舆图」（Antique Map）定稿值**，
//!   是默认地图观感；暗调变体「暮色旧纸」见 [`super::colors_dusk`]（同槽位镜像），
//!   运行时二选一经 [`super::palette`] 分发。渲染代码画「随主题变化的要素」
//!   时**必须经 palette 分发**，不得直引本模块常量
//!   （accent / 语义色 / 派系 / 轮廓图三色等主题不变项除外）。
//! - **禁止**在渲染代码里写裸十六进制色值（`Color::srgb(0.2, 0.3, 0.4)`）：
//!   一律经此模块的令牌引用。这样换 Theme Pack 时只改一处。
//! - `ui_link.*` 三色是**语义色**（可达绿 / 迟到黄 / 不可达红），
//!   Theme Pack 只允许微调明度，色相不得替换（美术 §1.4.3 ①）。
//!
//! ## 结构（28 色）
//!
//! ```text
//! core     10 色  地形基色（水/沙/草/林/丘/山/雪/沼/路 + 深水/浅水）
//! faction   2 色  已定骨架色（neutral / player）；a..f 由 pack 提供
//! ui_link   3 色  语义色
//! line      3 色  描线色（浅底/中底/深底）
//! accent    4 色  强调色（选中/游标/事件/便签）
//! ```

use bevy::prelude::*;

/// 从 sRGB 十六进制构造 `Color`（`0xRRGGBB`）。
///
/// **公开**（2026-10-21）：外部主题 crate（如 `wv-theme-chinese-ink`）要自建
/// 自己的色板就得能造 `Color`——此前 `hex` 是私有的，外部包只能沿用内置值，
/// 谈不���「各主题各带色板」。
pub const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// 地形基色（`core.*`）。索引即 `TerrainKind` 编号（0..10）。
///
/// **「日间 JRPG」（2026-10-18 用户定稿，A 方案中高饱和）**：16-bit 晚期
/// JRPG 的鲜明地形色——草/林靠明度差区分（色弱冗余）、山地→雪山在雪线附近
/// 留过渡、阴影同色相压暗。顺序刻意与「色块地形」视图模式的默认图例一致：
/// 0 为最低（深水）→ 8 为最高（雪），9 为道路。作者改 `kind` 编号即可换色，
/// 无需改代码。（现行值见 ART_STYLE_SPEC §1.4.5 / ADR-0035；
/// 原「日间古舆图」低饱和版见 §1.4.4 历史记录。）
pub const CORE_TERRAIN: [Color; 10] = [
    hex(0x2C6389), // 0 core.water_deep   深水（远海，明确的蓝）
    hex(0x57A0C0), // 1 core.water_shallow 浅水 / 海岸带
    hex(0xE8D6A2), // 2 core.sand         沙 / 沙漠（暖沙）
    hex(0x8CB95F), // 3 core.grass        草原 / 平原（鲜明草绿）
    hex(0x3F7A46), // 4 core.forest       森林（深林绿，与草靠明度+色相差区分）
    hex(0xB5A765), // 5 core.hill         丘陵（干草黄）
    hex(0x9E9070), // 6 core.mountain     山地（暖岩灰；ADR-0042 由 #8A7A66 提暖收
    //                      近丘陵——山体身份交给装饰图标承担，
    //                      填充色只做「高地变灰」的水彩过渡）
    hex(0xF5F8F9), // 7 core.snow         雪线 / 冰原（冷白）
    hex(0x6F8564), // 8 core.marsh        沼泽（湿绿）
    hex(0xC8A272), // 9 core.road         道路 / 区域分界（土路棕）
];

/// 海岸线描边色（轮廓图模式专用三色之一；模式语义，不随 [`super::palette`] 切换）。
pub const COAST_LINE: Color = hex(0x4A90E2); // 浅水色，与近海一致
/// POI 图标底色（砖赭，2026-10-14 用户定稿）。
pub const PLACE_DOT: Color = hex(0xB88E6C);
/// POI 外环（= 纸面色，防止绿地 / 水面吞没圆点，2026-10-14 用户定稿）。
pub const POI_RING: Color = hex(0xF2EAD8);
/// 城市间连线（`frame.road_segments`）的描线：官道棕褐（2026-10-14 用户定稿）。
pub const ROAD_STROKE: Color = hex(0xA67C52);

// ---------------------------------------------------------------------------
// 古舆图矢量层令牌（ADR-0016 新增；`colors_dusk` 有同槽位镜像）
// ---------------------------------------------------------------------------

/// 主干道（城市主街 / 官道）填充：棕褐粗线（2026-10-14 用户定稿）。
pub const STREET_MAJOR: Color = hex(0xA67C52);
/// 次级道路 / 街巷填充：浅卡其细线（2026-10-14 用户定稿）。
pub const STREET_MINOR: Color = hex(0xD9C8B4);
/// 道路 casing（外缘描边）= 纸面色：把路从同类色地形上「托」出来。
pub const ROAD_CASING: Color = hex(0xF2EAD8);
/// 地名 / 街名文字填充：深褐墨色，替代纯黑 / 纯白（2026-10-14 用户定稿）。
pub const LABEL_FILL: Color = hex(0x4A3F35);
/// 文字晕边（halo）填充 = 纸面色：保证文字在绿地 / 水面上可读（M2 启用）。
pub const HALO_FILL: Color = hex(0xF2EAD8);
/// 等值线 / 海岸线墨色：褐墨（取代旧硬编码「制图蓝灰」，ADR-0016 令牌化）。
pub const ISOLINE_INK: Color = hex(0x6B5B49); // 待复核
/// 编辑网格线基色（alpha 由 `grid_lines` 叠加，token 只存本色）。
pub const GRID_LINE: Color = hex(0x4A3F35); // 待复核
/// 建筑顶面（浅陶土灰，2026-10-14 用户定稿；M3 建筑挤出消费）。
pub const BUILDING_TOP: Color = hex(0xD4C8BB);
/// 建筑侧面 / 投影基色（阴影 = 本色 α0.35，ART_STYLE_SPEC §1.5）。
pub const BUILDING_SIDE: Color = hex(0x9A8B7A);

/// 派系色（`faction.*`，已定的 2 个骨架值）。
pub const FACTION_NEUTRAL: Color = hex(0x8C8C94); // 无主 / 未归属
pub const FACTION_PLAYER: Color = hex(0xC4553F); // 玩家方（暖红）

/// 语义色（`ui_link.*`）—— 色相不得替换。
pub const UI_REACHABLE: Color = hex(0x3F9B4E); // 可达绿
pub const UI_LATE: Color = hex(0xC9971F); // 迟到黄
pub const UI_UNREACHABLE: Color = hex(0xB5453A); // 不可达红

/// 描线三档（`line.*`）：地图要素描边的墨色深浅档。
///
/// 命名是历史语义（「画在什么底上用什么档」），实际口径 = **同色系的三档深浅**：
/// 日间古舆图全档褐墨（纸上要深墨），暮色旧纸全档浅米（暗纸上要浅墨）——
/// 两套值见 [`super::colors_dusk`]。
pub const LINE_ON_LIGHT: Color = hex(0x4A3F35); // 墨（= label.fill 同源）
pub const LINE_ON_MID: Color = hex(0x443A30); // 略深一档 待复核
pub const LINE_ON_DARK: Color = hex(0x382F27); // 最深一档 待复核

// ---------------------------------------------------------------------------
// 标注三级字重（手绘地图呈现 #2，2026-10-15）
// ---------------------------------------------------------------------------

/// 标注三级字重 + 颜色的色源（手绘地图 #2，对应 civ-atlas 国/城/海三色梯度）。
///
/// - `region`（国/大区）—— 大字 + 最深墨（与 `line_on_dark` 同族）；
/// - `place`（城/POI）—— 中字 + `label_fill`（与旧 `LABEL_FILL` 一致，避免视觉回归）；
/// - `water`（海域名）—— 中字 + 青蓝（civ-atlas 海名色；worldview-rs 暂无独立
///   海名 entity，但预留令牌供后续水路 / 海洋 overlay 引用）。
///
/// **不是字号本身**——字号仍由 `view.scale × LABEL_PX` 决定；这里是颜色梯度，
/// 字重差由 `font_size × tier_factor` 体现（见 `drill.rs` 各 label 系统）。
/// 所有日间 / 暮色两套值见 [`super::colors_dusk`]。
pub const LABEL_REGION: Color = hex(0x382F27); // 国/大区：最深的褐墨
pub const LABEL_PLACE: Color = hex(0x4A3F35); // 城/POI：原 label.fill（不变）
/// 海域名：青蓝（civ-atlas `INK_CSS` 同族偏青）。暮色版另见 colors_dusk。
pub const LABEL_WATER: Color = hex(0x3F5B6A);

// ---------------------------------------------------------------------------
// POI 圆点色族（手绘地图呈现 #9，2026-10-15）
// ---------------------------------------------------------------------------

/// POI 悬停气泡圆点色——按 `EntityKind` 区分，统一调色板（之前 hover_tooltip
/// 里硬编码 RGB，theme 切换不联动）。色源与「标签三级字重」同调：
/// - Place 砖赭（与 `LABEL_PLACE` 同源——POI 即地点）
/// - Region 冷蓝灰（与 `LABEL_REGION` 同族偏亮）
/// - Faction 暖红（与 faction 势力色 `faction_color_a` 同族）
/// - Event 紫（与 `ACCENT_EVENT` 同源）
pub const DOT_PLACE: Color = hex(0xB48C5A);
pub const DOT_REGION: Color = hex(0x5A6E8C);
pub const DOT_FACTION: Color = hex(0xC4553F);
pub const DOT_EVENT: Color = hex(0xC08BD8);

// ---------------------------------------------------------------------------
// POI hover 文字色族（手绘地图呈现 #12，2026-10-15）
// ---------------------------------------------------------------------------

/// POI hover 文字色——比 `DOT_*` 略深一档（保证可读性）+ 同色族。
/// civ-atlas 风格：圆点 + 文字同源色族，hover 整体像「同色系徽章」。
pub const TEXT_PLACE: Color = hex(0x7A5A36); // 砖赭深
pub const TEXT_REGION: Color = hex(0x3C4A60); // 冷蓝灰深
pub const TEXT_FACTION: Color = hex(0x8C3525); // 暖红深
pub const TEXT_EVENT: Color = hex(0x8050A0); // 紫深

// ---------------------------------------------------------------------------
// 雪色双调（手绘地图呈现 #3，2026-10-15）
// ---------------------------------------------------------------------------

/// 雪格"亮面"——比 `core.snow` 更亮一档、近白（高光面、迎光坡）。
/// civ-atlas `SNOW_PAINT` 思路。手绘地图 #3：让雪原从一片纸面中跳出来。
/// JRPG 雪 `#F5F8F9` 已近满亮，highlight 提到冷白满档。
pub const SNOW_HIGHLIGHT: Color = hex(0xFCFEFF);
/// 雪格"冷阴影"——冷蓝灰，叠在高海拔/背光雪格上模拟阴影（civ-atlas
/// `SNOW_SHADOW #AEBBC3` 思路）。`cells.rs` 在 RealTerrain 模式下对
/// kind=7 的格按高度混合叠这个色。
pub const SNOW_SHADOW: Color = hex(0xAEBBC3);
/// 雪阴影高度阈值：海拔高于此（米）开始叠冷蓝灰阴影；阈值以下保持雪色基底。
/// 默认 2500 m——主线国家的高山带永久雪线附近。
pub const SNOW_SHADOW_MIN_HEIGHT: i16 = 2500;
/// 雪阴影强度：海拔每超过阈值 1000 m 多叠多少冷蓝灰（0..1 的 lerp 系数）。
/// 0.0 = 全不叠（雪原是平的），1.0 = 高海拔全冷蓝。
pub const SNOW_SHADOW_PER_KM: f32 = 0.35;

// ---------------------------------------------------------------------------
// 海冰色族（手绘地图呈现 #5，2026-10-15）
// ---------------------------------------------------------------------------

/// 海冰纸面——比 `core.water_shallow` 更亮、近白略偏冷（civ-atlas `ICE_PAPER`）。
/// 海域格在「足够冷」（按 y 纬度判定）时，叠这色。
pub const ICE_PAPER: Color = hex(0xEEF1EC);
/// 冰缘墨线：比海岸墨线偏冷、偏淡（civ-atlas `ICE_INK`）——一眼能与陆上
/// 岸线分开。当前世界观不画冰缘线（isoline 没专门画冰边界），但令牌
/// 留下供未来 M3 海冰描边消费。
pub const ICE_INK: Color = hex(0x44525A);
/// 海冰背光边的排线色：淡蓝灰（civ-atlas `ICE_SHADOW`）。
pub const ICE_SHADOW: Color = hex(0x8C9EA8);
/// 海冰纬度阈值：y < TERRAIN_H × `ICE_LATITUDE_T` 的海域格视为「足够冷」、
/// 叠海冰色。`0.20` = 顶部 20% 区域为极地海冰带。
pub const ICE_LATITUDE_T: f32 = 0.20;
/// 海冰深度上限：仅在浅于这个深度（米，海拔 ≥ -threshold）的水域叠冰——
/// 极深海底温度虽冷，但 civ-atlas 海冰只到大陆架。`200` = -200 m 以上才叠。
pub const ICE_MAX_DEPTH_M: i16 = 200;

/// 强调色（`accent.*`）。
pub const ACCENT_SELECT: Color = hex(0xF2C14E); // 选中高亮（金）
/// **当前时刻指示色**：时间轴游标 + 生长笔尖**共用**（动画规格 §7.1：禁止拆成两令牌）。
pub const ACCENT_CURSOR: Color = hex(0xE8623C);
pub const ACCENT_EVENT: Color = hex(0xC08BD8); // 事件标记（紫）
pub const ACCENT_NOTE: Color = hex(0x9AA5B1); // 「故事」便签（灰蓝）

/// **待落笔触色**（红橙）——[ADR-0052] §9.6 新增令牌。
///
/// CellMap 连续绘制（拖绘）期间，「已采样但尚未写入世界」的格子用本色
/// 半透填充（alpha 0.26，不脉动）标出。与 `ACCENT_SELECT`（金）的色相角
/// 差 ≥ 30°，确保「待落笔触」与「已提交选中」在同框时可辨（ADR-0052 §9.5
/// 四态视觉分工总表：region → stroke → selected → hover 的绘制序）。
pub const STROKE_PENDING: Color = hex(0xD9634E);

/// **撤销/重做瞬时闪标色**（纯白）——[ADR-0052] §9.6 新增令牌。
///
/// Undo / Redo 之后对受影响格做 250ms 白色脉冲：这是**纯视觉标记**，
/// 不承载任何状态语义（不复用 `ACCENT_SELECT` 的金，避免与「已提交
/// 选中」的常驻脉动混淆）。
pub const UNDO_FLASH: Color = hex(0xFFFFFF);

/// 视图模式 D（事件图）的极简底图色：= 纸面色（中性、不抢事件高亮）。
pub const EVENT_VIEW_BACKDROP: Color = hex(0xF2EAD8);

/// 视图模式 E（轮廓图）三色（2026-11 用户需求：黑底 + 海岸线）。
/// 水面近黑、陆地深灰、海岸线中亮灰——黑底上白圈 POI 与强调色全高对比。
/// **模式语义色，不随 MapPalette 切换**（ADR-0016 §2）。
pub const COAST_WATER: Color = hex(0x07090C); // 水面：近黑
pub const COAST_LAND: Color = hex(0x1A1E24); // 陆地：深灰（与水可辨但不抢要素）

/// 底图淡化的保留比例：`out = base*KEEP + SOFTEN_TOWARD*(1-KEEP)`。
///
/// 0.35 = 与势力图淡化同源（`shade_faded` 的 k=0.45 略再淡一档），
/// 目标是「仍可辨山川骨架，但饱和度让位给路线与地标」。
/// 底图淡化保留比例（旧古舆图默认：35% 本色 + 65% 纸面）。
///
/// 2026-10-18 起日间主题换 JRPG 色板后改用 [`JRPG_SOFTEN_KEEP`]；
/// 本值仍被暮色主题引用。
pub const SOFTEN_KEEP: f32 = 0.35;

/// JRPG 主题的底图淡化保留比例：92% 本色 + 8% 纸面。
///
/// 中高饱和色若仍按 0.35 洗，草绿 `#8CB95F` 会退成 `#CED9AE` 灰白
/// （soften 语义本身保留——笔迹/要素对比让位的机制不动，只调强度）。
///
/// 2026-10-20 S0 调参 0.85 → 0.92（洗白归因实测 work-view-test）：0.85 时
/// 15% 纸面混入与远景 veil（12%）叠加，海面主导色从烘焙基色 (68,134,168)
/// 被抬到实测 (121,160,180)，整幅发闷。0.92 把烘焙期洗白压到 8%，要素
/// 让位职能仍由 veil / mottle 覆盖层承担。
pub const JRPG_SOFTEN_KEEP: f32 = 0.92;
/// 底图淡化目标色：= 纸面色（2026-10-14 用户定稿）。
pub const SOFTEN_TOWARD: Color = hex(0xF2EAD8);

/// 「明确深蓝」色（2026-10-16 用户裁定）。
///
/// `water_shade_at` 在 `t = 1.0`（最深水位，约 ≤ -3500 m）直接返回本色——
/// 不再走主题里的 `CORE_TERRAIN[0]` 浅蓝 lerp + 冷偏移。理由：旧路径过
/// `soften (35% keep)` + 远缩放 `veil` 后会退成中性灰，肉眼辨不出「海」；
/// 新路径用一个语义「深海」色，无论缩放 / 主题怎么叠都明确深蓝。
///
/// 数值与古舆图「水墨 + 宣纸」基调不冲突：深蓝只在最深的海域出现（海
/// 沟、大洋中央），其余水深仍走原 lerp + cold shift。
///
/// 数值口径（2026-10-16 定稿）：R=12, G=50, B=200。比冷偏移的旧路径
///（蓝 179）还要深一档——蓝通道在 t=1.0 直接拿 200，过 `soften (35% keep)`
/// 后 ≈ (161, 170, 210)——蓝通道仍显著高于红 / 绿；再过 4.73× 缩放
/// `veil (alpha=0.261)` 后 ≈ (182, 184, 216)，仍辨为「海」。
pub const DEEP_OCEAN_BLUE: Color = hex(0x0C32C8); // ≈ (12, 50, 200) 明确深蓝

/// 近岸浅滩色（2026-10-21 S2，海洋表现方案）：离岸 ≤ `SHORE_BAND_CELLS`
/// 格的水格向本色偏移的「极浅滩」色——比 `core.water_shallow` 明显更亮、
/// 更淡（色相略抬向青），水线处读出「薄水覆沙」的观感。
///
/// 与 `water_shallow` 的分工：浅水是**深度带**色（DEM 深度 lerp 端点），
/// 浅滩是**离岸几何带**色（`wv_terrain::shore` 距离场驱动）——近岸剖面
/// 陡的图（水深 200 m 贴岸）照样有浅滩带。消费点：`cells::base_color_of`
/// （`TreatmentFlags::shore_shallow` 开时）。
pub const WATER_SHORE: Color = hex(0xA9D3D5); // ≈ (169, 211, 213) 淡青滩色

/// 底图淡化混合（所有视图模式共用的唯一实现；禁止散落重写）。
pub fn soften_color(base: Color) -> Color {
    let (Color::Srgba(b), Color::Srgba(t)) = (base, SOFTEN_TOWARD) else {
        return base;
    };
    let k = SOFTEN_KEEP;
    Color::srgb(
        b.red * k + t.red * (1.0 - k),
        b.green * k + t.green * (1.0 - k),
        b.blue * k + t.blue * (1.0 - k),
    )
}

/// 取地形基色；`kind` 越界时回退到沼泽色（可辨识，不是黑）。
pub fn core_terrain(kind: u8) -> Color {
    CORE_TERRAIN
        .get(kind as usize)
        .copied()
        .unwrap_or(CORE_TERRAIN[8])
}

/// 规格锚点：基准格边长（`TILE_BASE = 32px`，美术 §3.1）。
pub const TILE_BASE: f32 = 32.0;
/// 允许的格边长档位（美术 §3.1：必须五档之一）。
pub const TILE_SIZE_STEPS: [f32; 5] = [16.0, 24.0, 32.0, 48.0, 64.0];

/// 描线线宽 = `TILE_BASE / 16`（美术 §3.2：32px → 2px）。
pub const OUTLINE_WIDTH: f32 = TILE_BASE / 16.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_palette_has_ten_entries() {
        assert_eq!(CORE_TERRAIN.len(), 10);
    }

    #[test]
    fn out_of_range_kind_falls_back_without_panic() {
        // 越界 kind 不应 panic，回退到沼泽色。
        let c = core_terrain(200);
        assert_eq!(c, CORE_TERRAIN[8]);
    }

    #[test]
    fn tile_base_matches_style_anchor() {
        assert_eq!(TILE_BASE, 32.0);
        assert_eq!(OUTLINE_WIDTH, 2.0);
        assert!(TILE_SIZE_STEPS.contains(&TILE_BASE));
    }

    #[test]
    fn hex_decodes_to_expected_channels() {
        // 0xE8623C → r=232/255, g=98/255, b=60/255
        let Color::Srgba(c) = ACCENT_CURSOR else {
            panic!("应为 Srgba");
        };
        assert!((c.red - 232.0 / 255.0).abs() < 1e-6);
        assert!((c.green - 98.0 / 255.0).abs() < 1e-6);
        assert!((c.blue - 60.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn soften_color_keeps_keep_ratio_toward_paper_gray() {
        // 纯红 (255,0,0) × keep 0.35 + 纸面色 (237,232,220) × 0.65。
        let Color::Srgba(t) = SOFTEN_TOWARD else {
            panic!("应为 Srgba")
        };
        let out = soften_color(Color::srgb(1.0, 0.0, 0.0));
        let Color::Srgba(o) = out else {
            panic!("应为 Srgba")
        };
        let k = SOFTEN_KEEP;
        assert!(
            (o.red - (1.0 * k + t.red * (1.0 - k))).abs() < 1e-6,
            "got {o:?}"
        );
        assert!((o.green - t.green * (1.0 - k)).abs() < 1e-6, "got {o:?}");
        assert!((o.blue - t.blue * (1.0 - k)).abs() < 1e-6, "got {o:?}");
    }

    #[test]
    fn soften_color_of_paper_gray_is_idempotent() {
        // 目标色自身淡化后仍是目标色（混合的不动点）——保证连续两次淡化不漂白。
        // 用近似比较：f32 舍入会引入 ~1e-7 误差，不应用 ==。
        let out = soften_color(SOFTEN_TOWARD);
        let Color::Srgba(a) = out else {
            unreachable!("soften_color 应返回 Srgba")
        };
        let Color::Srgba(b) = SOFTEN_TOWARD else {
            unreachable!("SOFTEN_TOWARD 应为 Srgba")
        };
        assert!(
            (a.red - b.red).abs() < 1e-6,
            "R: got {:.8} want {:.8}",
            a.red,
            b.red
        );
        assert!(
            (a.green - b.green).abs() < 1e-6,
            "G: got {:.8} want {:.8}",
            a.green,
            b.green
        );
        assert!(
            (a.blue - b.blue).abs() < 1e-6,
            "B: got {:.8} want {:.8}",
            a.blue,
            b.blue
        );
    }

    #[test]
    fn coastline_triple_is_dark_background_with_visible_line() {
        // 轮廓图三色的约束：水最暗，陆地略亮，线最亮（黑底可辨）。
        let f = |c: Color| -> [f32; 3] {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            [s.red, s.green, s.blue]
        };
        let lum = |c: Color| -> f32 { f(c).iter().sum::<f32>() / 3.0 };
        assert!(lum(COAST_WATER) < lum(COAST_LAND));
        assert!(lum(COAST_LAND) < lum(COAST_LINE));
    }

    // -----------------------------------------------------------------------
    // 标注三级字重梯度（手绘地图 #2，2026-10-15）
    // -----------------------------------------------------------------------

    use super::hex;

    /// `LABEL_REGION` 比 `LABEL_PLACE` 深一档（国名/大区 比 城/POI 更醒目）。
    /// 三色梯度要求明度递减：region < place < water（海青蓝更冷，明度近似但色相不同）。
    #[test]
    fn label_tier_region_is_deeper_than_place() {
        let f = |c: Color| -> f32 {
            let Color::Srgba(s) = c else { panic!() };
            (s.red + s.green + s.blue) / 3.0
        };
        assert!(
            f(LABEL_REGION) < f(LABEL_PLACE),
            "region ({}) 应比 place ({}) 更深",
            f(LABEL_REGION),
            f(LABEL_PLACE)
        );
    }

    /// `LABEL_WATER` 是青蓝色族（蓝 > 红），与墨色 `LABEL_PLACE` 在色相上
    /// 明显区分——海名一眼能跟城/陆名分开。
    #[test]
    fn label_water_is_cyan_blue_hue() {
        let Color::Srgba(w) = LABEL_WATER else {
            panic!()
        };
        assert!(
            w.blue > w.red,
            "海名应偏青蓝（蓝 > 红），实得 r={} b={}",
            w.red,
            w.blue
        );
    }

    /// 海名青蓝锚点：日间 `#3F5B6A`——记死防手滑（与 civ-atlas 海名同族）。
    #[test]
    fn label_water_user_pinned_hex_is_stable() {
        assert_eq!(hex(0x3F5B6A), LABEL_WATER, "海名青蓝锚点应为 #3F5B6A");
    }

    // -----------------------------------------------------------------------
    // POI 圆点色族（手绘地图呈现 #9，2026-10-15）
    // -----------------------------------------------------------------------

    /// DOT_PLACE 锚点：与 `LABEL_PLACE` 同源——POI 即地点。
    #[test]
    fn dot_place_anchored_to_label_place_family() {
        // LABEL_PLACE = 0x4A3F35, DOT_PLACE = 0xB48C5A; 砖赭同族：
        // r 高、b 低、g 中。三个通道各自手算断言（不是 hex 等值——避免两个
        // 令牌意外被改成完全相同值而测试还过）。
        let (Color::Srgba(p), Color::Srgba(l)) = (DOT_PLACE, LABEL_PLACE) else {
            panic!()
        };
        assert!(
            p.red > p.blue,
            "DOT_PLACE 应偏暖（红 > 蓝），实得 r={} b={}",
            p.red,
            p.blue
        );
        assert!(
            l.red > l.blue,
            "LABEL_PLACE 应偏暖（红 > 蓝），实得 r={} b={}",
            l.red,
            l.blue
        );
    }

    /// TEXT_* 文字色与 DOT_* 圆点色**色族一致**（手绘地图 #12）：
    /// 每对 text_xx 与 dot_xx 的 r/g/b 比例相近——文字比圆点略深（同色相）。
    /// 防手滑改色时把"色族关联"破坏。
    #[test]
    fn text_colors_share_hue_with_dot_colors() {
        let (Color::Srgba(p_dot), Color::Srgba(p_text)) = (DOT_PLACE, TEXT_PLACE) else {
            panic!()
        };
        // 同一色相：r/g/b 的大小顺序一致。
        let dot_order = (p_dot.red > p_dot.blue, p_dot.red > p_dot.green);
        let text_order = (p_text.red > p_text.blue, p_text.red > p_text.green);
        assert_eq!(
            dot_order, text_order,
            "PLACE 文字与圆点同色相（红 > 蓝、> 绿）"
        );
    }

    /// 文字色明度 < 圆点色（hover 内文字更深，确保可读性 + 与圆点对比）。
    #[test]
    fn text_color_is_darker_than_dot_color() {
        let lum = |c: Color| -> f32 {
            let Color::Srgba(s) = c else { panic!() };
            (s.red + s.green + s.blue) / 3.0
        };
        assert!(lum(TEXT_PLACE) < lum(DOT_PLACE), "TEXT 文字应比 DOT 圆点深");
        assert!(
            lum(TEXT_REGION) < lum(DOT_REGION),
            "TEXT_REGION 应比 DOT_REGION 深"
        );
    }

    /// 四个 kind 圆点色互不相同：色源若被误改成同值，hover_tooltip 不可分辨 kind。
    #[test]
    fn dot_colors_per_kind_are_all_distinct() {
        let (Color::Srgba(p), Color::Srgba(r), Color::Srgba(f), Color::Srgba(e)) =
            (DOT_PLACE, DOT_REGION, DOT_FACTION, DOT_EVENT)
        else {
            panic!()
        };
        let colors = [
            (p.red, p.green, p.blue),
            (r.red, r.green, r.blue),
            (f.red, f.green, f.blue),
            (e.red, e.green, e.blue),
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "DOT_ kind {i} 与 kind {j} 同色");
            }
        }
    }
}
