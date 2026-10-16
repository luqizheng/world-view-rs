//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! 主题包（Theme Pack）—— 配色 / 装饰 / 贴图的聚合（ADR-0023 D1）。
//!
//! ## 职责
//! - 承载一个**完整视觉风格**的静态外观数据：色板（`PaletteColors`）+ 装饰旗标
//!   （`TreatmentFlags`，待 ADR-0021 D2 落地后接线）+ 可选贴图（`Option<TileAtlasSpec>`
//!   = 现有 `TileAtlas` 的薄包装，**不**造第二套贴图抽象）；
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

use super::palette::{MapPalette, PaletteColors};
use crate::terrain_view::tile_atlas::TileAtlas;

/// 主题包：色板 + 装饰 + 贴图的原子组合（ADR-0023 D1）。
///
/// **全部字段 `Copy`**——主题切换是原子的、整组失效；任何 `&self -> Self` 风格
/// 的修改都不应在这里出现，切换走"换 Resource 引用"。
#[derive(Debug, Clone, Copy)]
pub struct ThemePack {
    /// 稳定 id（持久化键 / `ThemeRegistry` 查找键）。约定 `^[a-z][a-z0-9_]*$`。
    pub id: &'static str,
    /// 顶栏 / 命令面板展示名（可含中文；本项目 i18n 不纳入主题名，按 ADR-0016 顶栏纪律）。
    pub display_name: &'static str,
    /// 配色 36 槽位（现有 `PaletteColors` 直引）。
    pub palette: PaletteColors,
    /// 装饰旗标（ADR-0021 D2 载荷）。M1 阶段 `Default::default()`（即所有旗标为
    /// `false` 的占位）；M2 落地时切到 `Treatment::HandDrawn` 默认值——届时
    /// 14 项手绘特性旗标全 `true`、与现状观感逐位一致。
    pub treatment: TreatmentFlags,
    /// 可选贴图（`None` = 纯色平涂，**当前默认行为**）。
    ///
    /// M1 阶段恒为 `None`：`TileAtlas` 资源路径在 wv-render 内部（`tile_atlas::atlas_base_dir()`）
    /// 是 `env!("CARGO_MANIFEST_DIR")/assets`，外部主题 crate 的贴图进不来——这是
    /// D2 待解决的差距。M2 接入 `wv-theme-xxx` 时这里改为 `Some(TileAtlasSpec)` 即可。
    pub atlas: Option<TileAtlasSpec>,
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
    /// 由已加载的 `TileAtlas` 推导规格（M2 接入时用，M1 不消费）。
    pub fn from_atlas(_atlas: &TileAtlas) -> Self {
        Self { format_version: 1 }
    }
}

/// 装饰旗标（M1 占位 = 全 `false`；M2 接 ADR-0021 D2 时切到 `Treatment::HandDrawn` 默认值）。
///
/// **当前所有旗标为 `false` 不影响观感**——因为 M1 阶段渲染层**完全**不读本结构
///（现有 `palette::colors(MapPalette)` 走 `PaletteColors::DAYLIGHT` 路径，与 `BuiltinPacks::DEFAULT.palette`
/// 值源同源）。M2 切读路径时再启用。
///
/// **为何用 const `EMPTY` 而非 `Default` derive**：`bool::default` 不是 const fn，
/// 无法在 `const` 上下文里调用——但 M1 阶段 `ThemePack` 字段必须用 const 构造
///（M1 选 const 而非 static 是为了让 `&'static` 引用与 palette::DAYLIGHT 同档；
/// static 的成本是首帧一次 lazy-init，const 是纯编译期）。**M1 在不改 ADR
/// 性能档的前提下**改用 const + `EMPTY` 显式零值，绕开 `bool::default` 限制。
#[derive(Debug, Clone, Copy)]
pub struct TreatmentFlags {
    /// 是否启用海岸线水彩晕（手绘地图呈现 #4，commit 45bd159）。
    pub coast_halo: bool,
    /// 是否启用海色按高度渐变（#1，commit 65b9373）。
    pub water_gradient: bool,
    /// 是否启用雪独立色 + 冷调阴影（#3，commit 386571d）。
    pub snow_shade: bool,
    /// 是否启用海冰层（#5，commit cd4aa40）。
    pub sea_ice: bool,
    /// 是否启用纸纹 grain uv（#6，commit d32c590）。
    pub paper_grain: bool,
    /// 是否启用河中线（#8，commit 875bb2f）。
    pub river_centerline: bool,
    /// 是否启用边界扰动 wobble（`cells.rs:343`）。
    pub boundary_wobble: bool,
    /// 是否启用墨线亚线性缩放（#10，commit 93becde）。
    pub ink_sublinear: bool,
}

impl TreatmentFlags {
    /// 全 `false` 常量（const fn 替代 `Default::default()`——`bool::default` 非 const）。
    pub const EMPTY: Self = Self {
        coast_halo: false,
        water_gradient: false,
        snow_shade: false,
        sea_ice: false,
        paper_grain: false,
        river_centerline: false,
        boundary_wobble: false,
        ink_sublinear: false,
    };
}

impl Default for TreatmentFlags {
    fn default() -> Self {
        Self::EMPTY
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

/// 内置主题包（M1 唯一，M2+ 接 `wv-theme-xxx` 时扩展）。
///
/// `DEFAULT = DAYLIGHT`：值源 = `PaletteColors::DAYLIGHT`，**逐位一致**
/// （构造时未做任何字段映射或转换）。这是 M1 零回归的硬保证——
/// `BuiltinPacks::DEFAULT.palette == PaletteColors::DAYLIGHT`（类型与值双重相等）。
///
/// **为何用 `static` 而非 `const`**：`TreatmentFlags` 没有 const 构造器（`bool::default`
/// 不是 const fn），const item 要求所有字段都能在编译期求值。M1 阶段 `DEFAULT` /
/// `DUSK` 是只读单例，语义上等价于 const；选 `static` 拿到 `&'static` 引用 + 单态
/// 初始化，**不影响 D2 §2 性能前提**（仍是 `&'static` 切片，热路径无 vtable）。
pub mod builtin {
    use super::*;

    /// 渲染系统找不到 `theme_pack_id` 时的兜底（M1 = `MapPalette::Daylight` 对应包）。
    pub static DEFAULT: ThemePack = ThemePack {
        id: "antique_default",
        display_name: "日间古舆图",
        palette: PaletteColors::DAYLIGHT,
        treatment: TreatmentFlags::EMPTY,
        atlas: None,
    };

    /// 暮色旧纸包（与 `PaletteColors::DUSK` 同源；M1 提供作内置第二档，M2 再接持久化）。
    pub static DUSK: ThemePack = ThemePack {
        id: "antique_dusk",
        display_name: "暮色旧纸",
        palette: PaletteColors::DUSK,
        treatment: TreatmentFlags::EMPTY,
        atlas: None,
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

    /// `TreatmentFlags::default()` 全 `false`（M1 占位语义——见字段文档）。
    #[test]
    fn treatment_flags_default_is_all_false() {
        let t = TreatmentFlags::default();
        assert!(!t.coast_halo);
        assert!(!t.water_gradient);
        assert!(!t.snow_shade);
        assert!(!t.sea_ice);
        assert!(!t.paper_grain);
        assert!(!t.river_centerline);
        assert!(!t.boundary_wobble);
        assert!(!t.ink_sublinear);
    }

    /// `TreatmentFlags::EMPTY` const 与 `Default::default()` 行为等价。
    #[test]
    fn treatment_flags_empty_matches_default() {
        assert_eq!(
            TreatmentFlags::EMPTY.coast_halo,
            TreatmentFlags::default().coast_halo
        );
        assert_eq!(
            TreatmentFlags::EMPTY.water_gradient,
            TreatmentFlags::default().water_gradient
        );
        assert_eq!(
            TreatmentFlags::EMPTY.ink_sublinear,
            TreatmentFlags::default().ink_sublinear
        );
    }
}
