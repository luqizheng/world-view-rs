//! 待批准：本文件在 `app` feature 下才编译。
//! 主题包（Theme Pack）—— 配色 / 装饰 / 贴图 / 覆盖物规则的聚合（ADR-0023 D1）。
//!
//! ## 职责
//! - 承载一个**完整视觉风格**的静态外观数据：色板（`PaletteColors`）+ 装饰旗标
//!   （`TreatmentFlags`，待 ADR-0021 D2 落地后接线）+ 可选贴图（`Option<TileAtlasSpec>`
//!   = 现有 `TileAtlas` 的薄包装，**不**造第二套贴图抽象）+ 覆盖物演绎规则
//!   （`FeatureOverlaySet`，2026-10-17 主题覆盖物设计）；
//! - 提供 `BuiltinPacks::DEFAULT` 作为本编辑器默认观感（= 现有 `PaletteColors::DAYLIGHT`），
//!   保证 M1 零回归；
//! - `ActiveThemePack` Resource 是渲染系统统一取色入口（M1 暂**不**切换消费点，
//!   存量 `palette::colors(MapPalette)` 调用走默认兜底；M2 再做消费点替换）。
//!
//! ## 边界（不做什么）
//! - **不**做 YAML 加载（M3 推迟到 README §12-30 推进时单开 ADR）；
//! - **不**做热切换脏标传播（M2 沿用现有 `TerrainTexture.last_palette` 路径）；
//! - **不**做 Plugin 拆分（M2 验收 `app_bootstrap.rs` 后再改）；
//! - **不**消费 `Box<dyn ThemePack>`（D2 §2：保持 `&'static` 单态化，热路径无 vtable）。
//!
//! ## 与 ADR-0021 协同（D4）
//! ADR-0021 D2 提议的 `TreatmentFlags` 本 M1 留为 `Default::default()` 占位；
//! 待 ADR-0021 M2 落地时把 `Treatment::HandDrawn` 常量作为 `BuiltinPacks::DEFAULT.treatment` 装载。
//! **零回归是 M1 接受的硬前提**：内置古舆图包的所有 `PaletteColors` 字段值与
//! `PaletteColors::DAYLIGHT` **逐位一致**（见 `BuiltinPacks::DEFAULT` 构造点）。

use bevy::prelude::*;

use super::decor::{
    DecorRecipe, WATERCOLOR as DECOR_WATERCOLOR, WATERCOLOR_DUSK as DECOR_WATERCOLOR_DUSK,
};
use super::overlays::{FeatureOverlaySet, FeatureRule, RegionOverlayRule, RegionOverlaySet};
use super::palette::{MapPalette, PaletteColors};
use super::sea::{SeaRecipe, WATERCOLOR, WATERCOLOR_DUSK};

/// 主题包：色板 + 装饰 + 贴图 + 覆盖物规则的原子组合（ADR-0023 D1）。
///
/// **全部字段 `Copy`**——主题切换是原子的、整组失效；任何 `&self -> Self` 风格
/// 的修改都不应在这里出现，切换走"换 Resource 引用"。
#[derive(Debug, Clone, Copy)]
pub struct ThemePack {
    /// 稳定 id（持久化键 / `ThemeRegistry` 查找键）。约定 `^[a-z][a-z0-9_]*$`。
    pub id: &'static str,
    /// 顶栏 / 命令面板展示名——中文（2026-10-25 用户裁定：主题名不走 i18n，
    /// 中英文常量直接住在主题 crate 里）。
    pub display_name: &'static str,
    /// 顶栏 / 命令面板展示名——英文（与 [`Self::display_name`] 同源同寿命，
    /// 由 [`Self::display_name_for`] 按语言分发）。
    pub display_name_en: &'static str,
    /// 配色 36 槽位（现有 `PaletteColors` 直引）。
    pub palette: PaletteColors,
    /// 装饰旗标（ADR-0021 D2 载荷，经 ADR-0023 D4 修订归入本结构）。
    ///
    /// 内置两包装载 [`TreatmentFlags::HAND_DRAWN`]（手绘全开 = 历史恒开观感，
    /// 零回归基准）；「干净矢量」类未来主题装载 `TreatmentFlags::EMPTY`。
    /// 渲染 gate 点自 2026-10-16 起逐点接线（cells / isoline_overlay / atmosphere）。
    pub treatment: TreatmentFlags,
    /// 可选贴图（`None` = 纯色平涂，**当前默认行为**）。
    ///
    /// M1 阶段恒为 `None`：`TileAtlas` 资源路径在 wv-render 内部（`tile_atlas::atlas_base_dir()`）
    /// 是 `env!("CARGO_MANIFEST_DIR")/assets`，外部主题 crate 的贴图进不来——这是
    /// D2 待解决的差距。M2 接入 `wv-theme-xxx` 时这里改为 `Some(TileAtlasSpec)` 即可。
    pub atlas: Option<TileAtlasSpec>,
    /// 地形装饰的画法配方（ADR-0048 追记二）：高度阈值、气候带切点、变体与
    /// 族墨色、LOD 步距与尺寸、浅海白波带。**数值配方住主题层**（与
    /// [`Self::sea`] 同构）——ADR-0039 D1 挂账的「迁 ThemePack」在此补上，
    /// 换风格不再需要改渲染层源码（ADR-0043）。
    pub decor: DecorRecipe,
    /// 海的画法配方（ADR-0048）：深度量程 / 曲线、近岸浅滩带、水面纸纹、
    /// 离岸墨线波纹、墨色。**数值配方住主题层**（与 [`Self::overlays`] 同构）——
    /// 渲染层只做执行器，换风格不改渲染源码（ADR-0043）。
    pub sea: SeaRecipe,
    /// 覆盖物演绎规则：feature 词汇 → 远 / 近带规则（晕、符号集、密度、采样参数）。
    ///
    /// v1 为代码常量（内置两包各一套、外部主题 crate 自带一本），YAML 化随
    /// ADR-0023 M3 推迟（README §12-30）。换主题 = 整包换规则；主题没给某
    /// feature 规则时渲染侧回退「地界描边 + 名字」，事实不消失。
    pub overlays: FeatureOverlaySet,
    /// 区域覆盖物规则表（ADR-0045 D10）：「自然区域声明的覆盖物键」→ 长相。
    ///
    /// 键是**开放字符串**，词表由主题拥有（外部主题 crate 各自带一本）。
    /// 区域声明的键在当前主题查无此键时，**不回退**到 cell 派生默认（那等于
    /// 拿另一个变体冒充作者声明），而是画**缺失覆盖物标记**（白底红斜线，
    /// ADR-0045 追记一）——本表同时充当「键存在性查询」的实现。
    pub region_overlays: RegionOverlaySet,
}

/// 按语言取展示名（2026-10-25 用户裁定：主题名中英文常量住在 crate 里、
/// 不进 i18n 系统；wv-theme 自身不知道 app 的 Locale 枚举，用布尔分发）。
impl ThemePack {
    /// `english = false` 取中文名，`true` 取英文名。
    pub fn display_name_for(&self, english: bool) -> &'static str {
        if english {
            self.display_name_en
        } else {
            self.display_name
        }
    }
}

/// 贴图规格（= 现有 `TileAtlas` 的薄包装，新增 `format_version` 与可选 `path` 元数据）。
///
/// **不**复制 `TileAtlas` 字段（避免与 `terrain_view` 双源真相）——M2 接入时直接持
/// `TileAtlas` 引用或 `Arc<TileAtlas>` 共享（与地图实体共享同一份像素）。本 M1 留作
/// 占位，`BuiltinPacks::DEFAULT.atlas = None` 不触发任何路径。
#[derive(Debug, Clone, Copy)]
pub struct TileAtlasSpec {
    /// 格式版本（与 `TileAtlas` 同语义；保留供未来 manifest schema 对齐用）。
    pub format_version: u32,
}

impl TileAtlasSpec {
    /// 由已加载的图集的 `format_version` 推导规格（M2 接入时用，M1 不消费）。
    ///
    /// **签名刻意不收 `TileAtlas`**：那是 `wv-render` 的运行时类型，契约层
    /// 引用它就等于契约反向依赖渲染层（ADR-0043 D2 的红线）。调用方
    /// （`wv-render`）自己取字段传进来。
    pub fn from_format_version(format_version: u32) -> Self {
        Self { format_version }
    }
}

/// 装饰旗标集合（ADR-0021 D2 特性归轴表的代码载体，经 ADR-0023 D4 修订归入 ThemePack）。
///
/// **归入本结构的 9 项**：水彩晕（#4）、海色渐变（#1）、雪调（#3）、海冰（#5，渲染
/// 未接线、旗标预留）、纸纹（#6）、河中线（#8）、边界扰动（wobble）、墨线亚线性
///（#10）、纸面雾霭。
/// **追加归入**（原 9 项之后按落地时间增补）：陆格明度颗粒（ADR-0042 D6）、
/// 水彩晕开（ADR-0044 D1）、近岸浅滩带（2026-10-21 S2，`shore_shallow`）。**不归入**（归各自既有轴，ADR-0021 §2 D2 §4）：标注字重（#2）、
/// POI 行为与 hover（#7/#9/#12）、海名（#13/#14）、笔刷内环（#11，编辑态反馈）。
///
/// 两极常量：[`TreatmentFlags::HAND_DRAWN`]（手绘全开）/ [`TreatmentFlags::EMPTY`]
///（全关 = 干净矢量基准）。`bool::default` 非 const，故两极都用手写 const——
/// `ThemePack` 的 `static` 构造点要求编译期求值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreatmentFlags {
    /// 是否启用海岸线水彩晕（手绘地图呈现 #4，commit 45bd159）。
    pub coast_halo: bool,
    /// 是否启用海色按高度渐变（#1，commit 65b9373）。
    pub water_gradient: bool,
    /// 是否启用雪独立色 + 冷调阴影（#3，commit 386571d）。
    pub snow_shade: bool,
    /// 是否启用海冰层（#5，commit cd4aa40）。
    ///
    /// **注意**：海冰当前只有色源令牌（`ICE_PAPER` / `ICE_INK` / `ICE_SHADOW`）
    /// 与纬度/深度阈值常量，**渲染消费从未接线**（`cells.rs` 无任何读取点）。
    /// 本旗标为该未来的渲染接线预留；接线前恒 `true` 无副作用。
    pub sea_ice: bool,
    /// 是否启用纸纹 grain uv（#6，commit d32c590）。
    pub paper_grain: bool,
    /// 是否启用河中线（#8，commit 875bb2f）。
    pub river_centerline: bool,
    /// 是否启用边界扰动 wobble（`cells.rs` `CellRenderer::wobble`）。
    pub boundary_wobble: bool,
    /// 是否启用墨线亚线性缩放（#10，commit 93becde）。
    pub ink_sublinear: bool,
    /// 是否启用纸面雾霭 + 远缩放 veil（ADR-0016 M2，`atmosphere.rs`）。
    pub paper_fog: bool,
    /// 是否启用陆格明度颗粒（ADR-0042 D6：每格 ±4.5% 确定性抖动，
    /// 平涂色块出水彩颗粒）。Google 类「干净普通地图」主题关。
    pub land_grain: bool,
    /// 是否启用**水彩晕开**（ADR-0044 D1，学 civ-atlas `paintBase` 的
    /// `boxBlurWrap`：先把「每格基色」做 5×5 盒式模糊再上屏，群系边界
    /// 从硬切格边变成 2 格宽的柔和渐变）。实现走纯函数（任意格的模糊色
    /// 只依赖邻域），全图烘焙与增量 patch 天然逐字节一致。
    pub watercolor_bleed: bool,
    /// 是否启用**近岸浅滩带**（2026-10-21 S2，海洋表现方案）：离岸
    /// ≤ `SHORE_BAND_CELLS` 格的水格向主题 `water_shore` 色偏移，由
    /// `wv_terrain::shore` 距离场驱动（水深 ≠ 离岸远近；近岸剖面陡的图
    /// 照样有浅滩带）。仅平涂语法两模式（VectorFlat / Schematic）参与。
    pub shore_shallow: bool,
}

impl TreatmentFlags {
    /// 手绘全开（= 2026-10-15 前 14 项手绘特性恒开的现状观感）。
    ///
    /// 内置两包（古舆图日间/暮色）都装载本值——**零回归硬前提**：所有 gate
    /// 点在旗标 `true` 时走与恒开时期**逐位相同**的代码路径。
    pub const HAND_DRAWN: Self = Self {
        coast_halo: true,
        water_gradient: true,
        snow_shade: true,
        sea_ice: true,
        paper_grain: true,
        river_centerline: true,
        boundary_wobble: true,
        ink_sublinear: true,
        paper_fog: true,
        land_grain: true,
        watercolor_bleed: true,
        shore_shallow: true,
    };

    /// 全 `false`（「干净矢量」观感的基准，未来 Skyline 类主题的起点；
    /// 也是测试的对照组）。
    pub const EMPTY: Self = Self {
        coast_halo: false,
        water_gradient: false,
        snow_shade: false,
        sea_ice: false,
        paper_grain: false,
        river_centerline: false,
        boundary_wobble: false,
        ink_sublinear: false,
        paper_fog: false,
        land_grain: false,
        watercolor_bleed: false,
        shore_shallow: false,
    };
}

impl Default for TreatmentFlags {
    /// 缺省 = 手绘全开（安全缺省：新主题 crate 忘写 `treatment` 时得到与
    /// 编辑器一致的手绘观感，而非全关的素图）。
    fn default() -> Self {
        Self::HAND_DRAWN
    }
}

/// 主题包注册 trait（ADR-0023 D2）。
///
/// **实现者**是各主题 crate（`wv-theme-chinese-ink` 等），**不在** wv-render 内
/// 暴露的 trait 上做反射——`&'static [(&'static str, &'static ThemePack)]` 切片
/// 即可，不引入 dyn 调度（D2 §2 否决 `Box<dyn>`）。
///
/// M1 阶段 wv-render **不**消费此 trait（没有"加载多个包"的诉求）；
/// trait 定义仅作为契约锚点供 M2 主题 crate 实现使用。
pub trait ThemePackRegistry {
    /// 本注册表持有的主题包列表。
    fn packs() -> &'static [(&'static str, &'static ThemePack)];
}

/// 渲染系统消费入口（M1 暂不被任何系统读，但 Resource 已就位）。
///
/// M2 切换流程：用户选主题 → 写 `ViewState::theme_pack_id` → `app_bootstrap`
/// 解析为 `&'static ThemePack` → 塞本 Resource → 触发 `terrain_view::rebuild_texture_on_mode_change`。
///
/// **不派生 `Copy`**：`&'static ThemePack` 自身是 `Copy`（裸引用），但 `Resource`
/// 派生与外层结构 `Copy` 同时存在会撞 `E0119`（同一 trait 被不同方式实现）。
/// 走 `Default` blanket impl（`T: Default => T: FromWorld`）——`builtin::DEFAULT` 是
/// `static` 但 `&'static ThemePack` 让 `Default::default` 拿引用即可。
#[derive(Resource, Debug)]
pub struct ActiveThemePack(pub &'static ThemePack);

impl Default for ActiveThemePack {
    fn default() -> Self {
        Self(&builtin::DEFAULT)
    }
}

impl ActiveThemePack {
    /// 当前生效的色板（ADR-0023 M2：色板随包走，不再由 `MapPalette` 派生）。
    pub fn palette(&self) -> &PaletteColors {
        &self.0.palette
    }

    /// 当前生效的装饰旗标。
    pub fn treatment(&self) -> &TreatmentFlags {
        &self.0.treatment
    }

    /// 当前生效的海面画法配方（ADR-0048）。
    pub fn sea(&self) -> &super::sea::SeaRecipe {
        &self.0.sea
    }

    /// 当前生效的装饰画法配方（ADR-0048 追记二）。
    pub fn decor(&self) -> &super::decor::DecorRecipe {
        &self.0.decor
    }

    /// 当前生效的 Place 覆盖物规则。
    pub fn overlays(&self) -> &super::overlays::FeatureOverlaySet {
        &self.0.overlays
    }

    /// 当前生效的 Region 覆盖物规则。
    pub fn region_overlays(&self) -> &super::overlays::RegionOverlaySet {
        &self.0.region_overlays
    }
}

/// 内置主题包（M1 唯一，M2+ 接 `wv-theme-xxx` 时扩展）。
///
/// `DEFAULT = DAYLIGHT`：值源 = `PaletteColors::DAYLIGHT`，**逐位一致**
/// （构造时未做任何字段映射或转换）。这是 M1 零回归的硬保证——
/// `BuiltinPacks::DEFAULT.palette == PaletteColors::DAYLIGHT`（类型与值双重相等）。
///
/// **为何用 `static` 而非 `const`**：Rust 的 `const` item 引用在每次使用处内联展开，
/// `ThemePack` 作为聚合身份（id / registry 查找键）必须**地址稳定**——`static` 保证
/// 单例；**不影响 D2 §2 性能前提**（仍是 `&'static` 引用，热路径无 vtable）。
pub mod builtin {
    use super::*;

    /// 渲染系统找不到 `theme_pack_id` 时的兜底（M1 = `MapPalette::Daylight` 对应包）。
    ///
    /// 展示名（2026-10-25 用户裁定）：本 crate 的观感定位 = **JRPG 水彩**
    /// （ADR-0035 的 JRPG 色板方案），不再叫「日间古舆图」。
    pub static DEFAULT: ThemePack = ThemePack {
        id: "antique_default",
        display_name: "JRPG 水彩",
        display_name_en: "JRPG Watercolor",
        palette: PaletteColors::DAYLIGHT,
        treatment: TreatmentFlags::HAND_DRAWN,
        atlas: None,
        sea: WATERCOLOR,
        decor: DECOR_WATERCOLOR,
        // 设计与取值依据：`docs/superpowers/specs/2026-10-17-theme-overlay-geo-fact-design.md`
        // §6.4（D10：两内置包各一套，差异做在晕色 / 浓度 / 符号密度）。
        // 日间：偏灰橄榄绿晕 + 深墨绿树冠。
        //
        // `symbol_size_px` 是**屏幕像素**（spec §6.6：树冠符号 24–40 px）；
        // `symbol_size_px * view.scale`（world-units-per-pixel）换成世界边长。
        // `symbol_spacing_wu` 是**世界单位**采样间距——放大符号后同步放宽，
        // 否则近带树冠会重叠成一片糊。
        overlays: FeatureOverlaySet {
            forest: Some(FeatureRule {
                halo_color: [0x5F, 0x7A, 0x4A],
                halo_alpha: 0.30,
                symbol_color: [0x3F, 0x5A, 0x35],
                symbol_size_px: 28.0,
                symbol_spacing_wu: 40.0,
                // Task 9（D12）：日间走 3 变体树图集 + 森林盖层晕图章。
                symbol_texture: Some("overlays/forest/tree_atlas_day.png"),
                halo_texture: Some("overlays/forest/forest_fill.png"),
            }),
            // 其余族 v1 无内置演绎（ADR-0032 D2：字段一次定型，规则内容待美术）。
            grassland: None,
            desert: None,
            wetland: None,
            cold: None,
            highland: None,
            water: None,
            road: None,
        },
        // 区域覆盖物键（ADR-0045 D10）：日间古舆图的四档变体。
        // 变体色与 `terrain_decor` 现有气候带变体同源（针叶深绿 / 阔叶墨绿 /
        // 稀树暖绿），乱石滩涂是新增的灰褐档。
        region_overlays: RegionOverlaySet::new(&[
            (
                "conifer",
                RegionOverlayRule {
                    variant: "conifer",
                    color: [0x2E, 0x5C, 0x36],
                    alpha: None,
                    size_scale: 1.0,
                    spacing_scale: 1.0,
                },
            ),
            (
                "broadleaf",
                RegionOverlayRule {
                    variant: "broadleaf",
                    color: [0x2B, 0x57, 0x31],
                    alpha: None,
                    size_scale: 1.0,
                    spacing_scale: 1.0,
                },
            ),
            (
                "sparse",
                RegionOverlayRule {
                    variant: "sparse",
                    color: [0x4A, 0x6B, 0x35],
                    alpha: None,
                    size_scale: 1.0,
                    spacing_scale: 1.0,
                },
            ),
            (
                "boulder_field",
                RegionOverlayRule {
                    variant: "boulder",
                    color: [0x8A, 0x84, 0x74],
                    alpha: Some(0.80),
                    size_scale: 1.0,
                    spacing_scale: 1.2,
                },
            ),
        ]),
    };

    /// 暮色旧纸包（与 `PaletteColors::DUSK` 同源；M1 提供作内置第二档，M2 再接持久化）。
    ///
    /// 未注册进 [`ALL`]——循环切换不可达，展示名保持历史名。
    pub static DUSK: ThemePack = ThemePack {
        id: "antique_dusk",
        display_name: "暮色旧纸",
        display_name_en: "Dusk Old Paper",
        palette: PaletteColors::DUSK,
        treatment: TreatmentFlags::HAND_DRAWN,
        atlas: None,
        sea: WATERCOLOR_DUSK,
        decor: DECOR_WATERCOLOR_DUSK,
        // 暮色：压暗的橄榄绿晕 + 更深的树冠，晕更淡（0.20）、符号略小
        //（24 px，仍落在 spec §6.6 的 24–40 px）、采样比日间更密（32wu）
        // ——与 DEFAULT 在 halo_color / halo_alpha / symbol_size / spacing
        // 四项上都可见不同（D10 要求 ≥ 2 项）。
        overlays: FeatureOverlaySet {
            forest: Some(FeatureRule {
                halo_color: [0x3E, 0x4A, 0x34],
                halo_alpha: 0.20,
                symbol_color: [0x28, 0x32, 0x24],
                symbol_size_px: 24.0,
                symbol_spacing_wu: 32.0,
                // Task 9：暮色符号走浅墨图集；晕保持纯色（贴图校准留待后续）。
                symbol_texture: Some("overlays/forest/tree_atlas_dusk.png"),
                halo_texture: None,
            }),
            // 其余族 v1 无内置演绎（ADR-0032 D2）。
            grassland: None,
            desert: None,
            wetland: None,
            cold: None,
            highland: None,
            water: None,
            road: None,
        },
        // 区域覆盖物键（ADR-0045 D10）：暮色版压暗一档，与 DEFAULT 可见不同。
        region_overlays: RegionOverlaySet::new(&[
            (
                "conifer",
                RegionOverlayRule {
                    variant: "conifer",
                    color: [0x25, 0x46, 0x2C],
                    alpha: None,
                    size_scale: 0.9,
                    spacing_scale: 1.0,
                },
            ),
            (
                "broadleaf",
                RegionOverlayRule {
                    variant: "broadleaf",
                    color: [0x23, 0x43, 0x28],
                    alpha: None,
                    size_scale: 0.9,
                    spacing_scale: 1.0,
                },
            ),
            (
                "sparse",
                RegionOverlayRule {
                    variant: "sparse",
                    color: [0x3B, 0x55, 0x2A],
                    alpha: None,
                    size_scale: 0.9,
                    spacing_scale: 1.0,
                },
            ),
            (
                "boulder_field",
                RegionOverlayRule {
                    variant: "boulder",
                    color: [0x6E, 0x69, 0x5C],
                    alpha: Some(0.78),
                    size_scale: 0.9,
                    spacing_scale: 1.2,
                },
            ),
        ]),
    };

    /// 内置包列表（M1 = `[DEFAULT]`；M2 改 `[DEFAULT, DUSK]` 后 `view.theme_pack_id` 即可持久化切换）。
    ///
    /// **当前显式只暴露 `DEFAULT`**——DUSK 暂不开放为独立持久化键（M2 验收时
    /// 同步把 `MapPalette::Dusk -> antique_dusk` 的迁移做掉，详见 ADR-0023 §6.2 M2）。
    pub static ALL: &[(&str, &ThemePack)] = &[("antique_default", &DEFAULT)];

    /// 从 `MapPalette` 派生 `ThemePack`（M1 兼容层——存量 `view.map_palette` 仍为权威源，
    /// 直到 M2 把 `theme_pack_id` 提升为权威）。
    pub fn for_palette(p: MapPalette) -> &'static ThemePack {
        match p {
            MapPalette::Daylight => &DEFAULT,
            MapPalette::Dusk => &DUSK,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M1 零回归硬保证：内置默认包与 `PaletteColors::DAYLIGHT` **逐位一致**。
    /// 防手滑把 `DEFAULT.palette` 改成 DUSK 或注入"清理"逻辑。
    #[test]
    fn default_pack_palette_is_daylight() {
        assert_eq!(
            builtin::DEFAULT.palette.core,
            PaletteColors::DAYLIGHT.core,
            "DEFAULT.core 必与 DAYLIGHT.core 逐位一致"
        );
        assert_eq!(
            builtin::DEFAULT.palette.soften_toward,
            PaletteColors::DAYLIGHT.soften_toward,
            "DEFAULT.soften_toward 必与 DAYLIGHT 逐位一致"
        );
        assert_eq!(
            builtin::DEFAULT.palette.label_fill,
            PaletteColors::DAYLIGHT.label_fill,
            "DEFAULT.label_fill 必与 DAYLIGHT 逐位一致"
        );
    }

    /// 内置默认包 id 稳定（持久化键 / 状态栏告警依赖）。
    #[test]
    fn default_pack_id_is_stable() {
        assert_eq!(builtin::DEFAULT.id, "antique_default");
    }

    /// `for_palette` 派生口径：Daylight -> DEFAULT, Dusk -> DUSK（M1 兼容层）。M2 把
    /// `view.map_palette` 退役时此函数一并退役。
    #[test]
    fn for_palette_routes_to_matching_builtin() {
        assert_eq!(
            builtin::for_palette(MapPalette::Daylight).id,
            "antique_default"
        );
        assert_eq!(builtin::for_palette(MapPalette::Dusk).id, "antique_dusk");
    }

    /// `ThemePack` 全部字段 `Copy`（D2 §2 性能前提）。
    #[test]
    fn theme_pack_is_copy() {
        let p = builtin::DEFAULT;
        let _q = p; // 必须能 Copy 不 move
        let _r = p; // 两次仍可用 = Copy
    }

    /// `ActiveThemePack::default()` 走内置 DEFAULT（Bevy `init_resource` 兜底）。
    #[test]
    fn active_theme_pack_default_routes_to_builtin() {
        let a = ActiveThemePack::default();
        assert_eq!(a.0.id, "antique_default");
    }

    /// `TreatmentFlags::default()` = `HAND_DRAWN`（安全缺省：忘写 treatment 的新
    /// 主题 crate 得到与编辑器一致的手绘观感）。
    #[test]
    fn treatment_flags_default_is_hand_drawn() {
        assert_eq!(
            TreatmentFlags::default().coast_halo,
            TreatmentFlags::HAND_DRAWN.coast_halo
        );
        assert!(TreatmentFlags::default().coast_halo);
        assert!(TreatmentFlags::default().water_gradient);
        assert!(TreatmentFlags::default().snow_shade);
        assert!(TreatmentFlags::default().paper_grain);
        assert!(TreatmentFlags::default().river_centerline);
        assert!(TreatmentFlags::default().boundary_wobble);
        assert!(TreatmentFlags::default().ink_sublinear);
        assert!(TreatmentFlags::default().paper_fog);
    }

    /// `HAND_DRAWN` 与 `EMPTY` 互补：每个旗标一真一假（两极对照完整，防加字段时
    /// 漏更新某一个 const）。
    #[test]
    fn hand_drawn_and_empty_are_complementary() {
        let h = TreatmentFlags::HAND_DRAWN;
        let e = TreatmentFlags::EMPTY;
        assert!(h.coast_halo && !e.coast_halo);
        assert!(h.water_gradient && !e.water_gradient);
        assert!(h.snow_shade && !e.snow_shade);
        assert!(h.sea_ice && !e.sea_ice);
        assert!(h.paper_grain && !e.paper_grain);
        assert!(h.river_centerline && !e.river_centerline);
        assert!(h.boundary_wobble && !e.boundary_wobble);
        assert!(h.ink_sublinear && !e.ink_sublinear);
        assert!(h.paper_fog && !e.paper_fog);
        assert!(h.land_grain && !e.land_grain);
        assert!(h.watercolor_bleed && !e.watercolor_bleed);
        assert!(h.shore_shallow && !e.shore_shallow);
    }

    /// 内置两包都装载 `HAND_DRAWN`（零回归硬前提：渲染 gate 读到的旗标全 `true`
    /// = 与历史恒开行为逐位一致）。
    #[test]
    fn builtin_packs_carry_hand_drawn_treatment() {
        assert_eq!(builtin::DEFAULT.treatment, TreatmentFlags::HAND_DRAWN);
        assert_eq!(builtin::DUSK.treatment, TreatmentFlags::HAND_DRAWN);
    }

    /// 默认包必须给森林族配置覆盖规则（D10：内置两包各一套），且值锁定为设计稿
    /// 指定的日间古舆图参数——防手滑改成 DUSK 值或漏配 `overlays`。
    #[test]
    fn default_pack_has_forest_rule() {
        let rule = builtin::DEFAULT
            .overlays
            .rule_for(wv_terrain::biome::BiomeFamily::Forest)
            .expect("默认包必须给森林族配置规则");
        assert_eq!(rule.halo_color, [0x5F, 0x7A, 0x4A], "日间晕色");
        assert!(
            (rule.halo_alpha - 0.30).abs() < 1e-6,
            "日间晕浓度 alpha=0.30，got {}",
            rule.halo_alpha
        );
        assert_eq!(rule.symbol_color, [0x3F, 0x5A, 0x35], "日间符号色");
        assert!(
            (rule.symbol_size_px - 28.0).abs() < 1e-6,
            "日间符号尺寸 28px（spec §6.6：24–40px），got {}",
            rule.symbol_size_px
        );
        assert!(
            (rule.symbol_spacing_wu - 40.0).abs() < 1e-6,
            "日间采样间距 40wu，got {}",
            rule.symbol_spacing_wu
        );
        assert_eq!(
            rule.symbol_texture,
            Some("overlays/forest/tree_atlas_day.png"),
            "日间符号走 3 变体树图集"
        );
        assert_eq!(
            rule.halo_texture,
            Some("overlays/forest/forest_fill.png"),
            "日间晕走森林盖层图章"
        );
    }

    /// 暮色包：符号用浅墨图集，晕**不**接贴图（保持纯色暮色晕，Task 9 裁定）。
    #[test]
    fn dusk_pack_uses_dusk_atlas_and_flat_halo() {
        let rule = builtin::DUSK
            .overlays
            .rule_for(wv_terrain::biome::BiomeFamily::Forest)
            .expect("暮色包必须给森林族配置规则");
        assert_eq!(
            rule.symbol_texture,
            Some("overlays/forest/tree_atlas_dusk.png"),
            "暮色符号走浅墨图集"
        );
        assert!(
            rule.halo_texture.is_none(),
            "暮色晕保持纯色（halo_texture = None）"
        );
    }

    /// D10 验收口径：DUSK 与 DEFAULT 至少 2 项可见差异（晕色 / 晕浓度 / 符号
    /// 尺寸 / 采样间距）。
    #[test]
    fn dusk_differs_from_default_in_at_least_two_visual_fields() {
        let day = builtin::DEFAULT
            .overlays
            .rule_for(wv_terrain::biome::BiomeFamily::Forest)
            .expect("默认包必须有森林族规则");
        let dusk = builtin::DUSK
            .overlays
            .rule_for(wv_terrain::biome::BiomeFamily::Forest)
            .expect("暮色包必须有森林族规则");
        let diffs = [
            day.halo_color != dusk.halo_color,
            (day.halo_alpha - dusk.halo_alpha).abs() > 1e-6,
            (day.symbol_size_px - dusk.symbol_size_px).abs() > 1e-6,
            (day.symbol_spacing_wu - dusk.symbol_spacing_wu).abs() > 1e-6,
        ]
        .into_iter()
        .filter(|different| *different)
        .count();
        assert!(
            diffs >= 2,
            "D10 要求至少 2 项可见差异，实际 {diffs} 项；\
             halo_color {:?} vs {:?}, alpha {} vs {}, size {} vs {}, spacing {} vs {}",
            day.halo_color,
            dusk.halo_color,
            day.halo_alpha,
            dusk.halo_alpha,
            day.symbol_size_px,
            dusk.symbol_size_px,
            day.symbol_spacing_wu,
            dusk.symbol_spacing_wu
        );
        assert!(
            (dusk.symbol_size_px - 24.0).abs() < 1e-6,
            "暮色符号尺寸 24px（spec §6.6 下限），got {}",
            dusk.symbol_size_px
        );
        assert!(
            (dusk.symbol_spacing_wu - 32.0).abs() < 1e-6,
            "暮色采样间距 32wu（比日间 40wu 更密），got {}",
            dusk.symbol_spacing_wu
        );
    }

    /// 无规则族没有规则——渲染侧回退「地界晕（回退色）+ 名字」，不消失
    /// （ADR-0032 D2：v1 内置两包只演绎森林族）。
    #[test]
    fn ruleless_family_falls_back_to_none() {
        assert!(
            builtin::DEFAULT
                .overlays
                .rule_for(wv_terrain::biome::BiomeFamily::Highland)
                .is_none(),
            "高地族未演绎，默认包应回 None"
        );
        assert!(
            builtin::DUSK
                .overlays
                .rule_for(wv_terrain::biome::BiomeFamily::Highland)
                .is_none(),
            "高地族未演绎，暮色包应回 None"
        );
    }
}
