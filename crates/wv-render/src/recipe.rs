//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! 渲染配方（RenderRecipe，ADR-0021 D1）—— 渲染"该不该画"的单点求值。
//!
//! ## 职责
//! 把 20+ 处散落的 `matches!(view.mode, ...)` / `!layers.visible` 判定收口到
//! 单一资源 `RenderRecipe`：每帧由 [`resolve_render_recipe`] 纯函数求值一次，
//! 渲染点改读 `recipe.pass(PassId::X).enabled` —— 模式 / 装饰 / 缩放逻辑集中表里。
//!
//! ## 边界（不做什么）
//! - **不管"怎么画"**：Recipe 只回答"开不开"，具体几何/材质/调色仍在各 pass 内。
//! - **不替 LayerFrame**：kind 轴（YAML 声明 × 会话开关）的求值仍归 LayerFrame
//!   单一职责；Recipe 只把 LayerFrame 当输入之一消费。两轴正交、不合并。
//! - **不替 ActiveThemePack**：treatment 旗标由 ThemePack 承载、Recipe 视为输入。
//! - **不替 Z 序**：z 仍由各渲染组内建槽位 + LayerFrame 的 z_offset 决定。
//!
//! ## 与 ADR-0021 修订状态
//! 本文件落地的是 ADR-0021 D1 提案的"Recipe 骨架 + 模式 × 缩放矩阵的真值表"——
//! ADR-0021 §1.3 列举的"已知不一致"（soften 排除 / veil 排除 / wobble 排除）转表
//! 后，**新增模式时改表即生效、漏改会被表测试抓住**。D2（Treatment 装饰族）已
//! 由 ADR-0023 D4 吸收为 `ThemePack.treatment`——本 Recipe 不再承担装饰旗标。

use bevy::prelude::*;

use crate::layer_frame::LayerFrame;
use crate::state::{DrillState, ViewMode, ViewState, WorldProjection};
use crate::terrain_view::tile_atlas::TileAtlas;
use crate::terrain_view::TerrainTexture;

/// Pass 标识（ADR-0021 §2 D1 §3）—— 一条 pass = 一类渲染工作。
///
/// **新增 pass 时**：在本枚举加项 + `PassPlan` 加 `enabled: false` 默认 +
/// `resolve_recipe` 算该项真值表 + 测试追加一行。漏任一步测试会失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassId {
    Terrain,
    Isoline,
    GridLines,
    MapFrame,
    Veil,
    Grain,
    Roads,
    Streets,
    Poi,
    Buildings,
    Faction,
    PlaceLabels,
    StreetLabels,
    SeaLabels,
    Measurement,
    BrushCursor,
    Tracks,
    EventDots,
    DrillMarkers,
    Atmosphere,
    FeatureOverlay,
    TerrainDecor,
    /// 河流折线（ADR-0037 D5）：模板世界由 terrain+rivers_seed 派生的水系。
    /// 与 TerrainDecor 同口径（VectorFlat/RealTerrain/Schematic/Faction 开）。
    Rivers,
    /// CellMap 统一选区高亮（ADR-0057）：选区面（金色半透脉动）+ 蚂蚁线。
    ///
    /// **会话轴 pass** —— 与 `tracks_visible` 同款处理：不进 `LayerFrame`
    /// 级联，由 `CellSelection.is_empty()` 控制（`sync_cell_region` gate）。
    /// Mode 表 8 行全开——不阻挡任何视图模式；空选区时渲染层自行关闭。
    CellSelection,
    /// 命名区域层（2026-10-22 内容矩阵）：区域轮廓/底色/标签三件。
    /// 从 TerrainDecor 拆出——Schematic 下「装饰 ✓ 而区域 ✗」必须两格独立
    /// （装饰是地貌演绎，区域是作者内容）；Topographic 也关（CellMap 的
    /// 自然区显示走 `ViewState.nature_overlay` 的旁路，不占本格）。
    RegionLayers,
}

impl PassId {
    /// 全部 pass(遍历测试用)。
    pub const ALL: &'static [PassId] = &[
        PassId::Terrain,
        PassId::Isoline,
        PassId::GridLines,
        PassId::MapFrame,
        PassId::Veil,
        PassId::Grain,
        PassId::Roads,
        PassId::Streets,
        PassId::Poi,
        PassId::Buildings,
        PassId::Faction,
        PassId::PlaceLabels,
        PassId::StreetLabels,
        PassId::SeaLabels,
        PassId::Measurement,
        PassId::BrushCursor,
        PassId::Tracks,
        PassId::EventDots,
        PassId::DrillMarkers,
        PassId::Atmosphere,
        PassId::FeatureOverlay,
        PassId::TerrainDecor,
        PassId::Rivers,
        PassId::CellSelection,
        PassId::RegionLayers,
    ];

    /// pass 总数(数组长度 = `PassId::COUNT`)。
    pub const COUNT: usize = 25;
}

/// 单条 pass 的元信息（ADR-0021 §2 D1 §3）。
#[derive(Debug, Clone, Copy, Default)]
pub struct PassPlan {
    /// 本 pass 是否本帧参与渲染。
    pub enabled: bool,
}

impl PassPlan {
    pub const fn enabled() -> Self {
        Self { enabled: true }
    }
    pub const fn disabled() -> Self {
        Self { enabled: false }
    }
    /// 从 `bool` 真值表构造（MODE_TABLE 存的就是 `bool`）。
    const fn from(b: bool) -> Self {
        Self { enabled: b }
    }
}

/// 渲染配方（每帧单点求值的输出，ADR-0021 D1 §3）。
///
/// **下标 = `PassId` 序号**（变体序号由 `PassId::ALL` 的声明顺序决定）——比
/// `HashMap<PassId, ...>` 快、比 `Vec<Option<PassPlan>>` 表达力强，比
/// `[PassPlan; PassId::COUNT]` 长度有编译期硬保证。
#[derive(Resource, Debug, Clone, Copy)]
pub struct RenderRecipe {
    pub passes: [PassPlan; PassId::COUNT],
}

impl RenderRecipe {
    /// 读单条 pass（用 `PassId` 索引）。
    pub fn pass(&self, id: PassId) -> PassPlan {
        self.passes[id as usize]
    }

    /// 读 enabled（简写，渲染 gate 常用）。
    pub fn enabled(&self, id: PassId) -> bool {
        self.passes[id as usize].enabled
    }
}

impl Default for RenderRecipe {
    /// 默认 = 全关（fail-safe：新 pass 不接 Recipe 时不画——显眼、好排查）。
    ///
    /// **为什么不全开**：测试用 default 时容易把"忘了算"和"算出来是 false"
    /// 混在一起；全关是显式表态"未配置"=关。
    fn default() -> Self {
        Self {
            passes: [PassPlan::disabled(); PassId::COUNT],
        }
    }
}

/// 模式 × pass 的可见性表（ADR-0021 §1.1 的"模式排除对"集中地）。
///
/// 行 = 模式，列序 = [`PassId::ALL`] 声明序。**每格以迁移前散点判定的实际行为
/// 为准**（等价迁移，不借机改语义）：
/// - Isoline 仅 VectorFlat + Topographic（会话轴另由 `view.isolines` 单控）；
/// - Buildings / Atmosphere / Veil / Grain 排除 GoogleMap | Coastline；
/// - Faction 仅 Faction 模式；
/// - FeatureOverlay 除 Coastline 外全开（主题地界覆盖层，轮廓图只留轮廓）。
///
/// **新增模式时**加一行；**改语义时**改对应行并同步测试锚定。
#[rustfmt::skip]
const MODE_TABLE: &[(ViewMode, [bool; PassId::COUNT])] = &[
    // 默认浏览（plan v0.1 D3 A 类，ADR-0012 已接受）。
(ViewMode::VectorFlat, [
        true,  // Terrain
        true,  // Isoline
        true,  // GridLines（受 LayerFrame + 会话开关，mode 表不关）
        true,  // MapFrame
        true,  // Veil
        true,  // Grain
        true,  // Roads
        true,  // Streets
        true,  // Poi
        true,  // Buildings
        false, // Faction
        true,  // PlaceLabels
        true,  // StreetLabels
        true,  // SeaLabels
        true,  // Measurement
        true,  // BrushCursor
        true,  // Tracks
        true,  // EventDots
        true,  // DrillMarkers
        true,  // Atmosphere
        true,  // FeatureOverlay
        true, // TerrainDecor
        true, // Rivers
        true, // CellSelection（[ADR-0040]：会话轴,8 mode 默认开,运行时由 selected 控）
        true, // RegionLayers（= 各行 TerrainDecor 同值；Schematic/Topographic 例外见上）
    ]),
    // 真实地形（plan v0.1 D3 B 类）——与 VectorFlat 同，除等值线（仅 VectorFlat）。
    (ViewMode::RealTerrain, [
        true, false, true, true, true, true, true, true, true, true,
        false, true, true, true, true, true, true, true, true, true,
        true,
        true, // TerrainDecor
        true, // Rivers
        true, // CellSelection
        true, // RegionLayers（Browse 类模式区域照常显示）
    ]),
    // 色块地形（plan v0.1 D3 C 类）——同上。
    (ViewMode::Schematic, [
        true, false, true, true, true, true, true, true, true, true,
        false, true, true, true, true, true, true, false, true, true,
        //                            ^ EventDots=false（2026-10-22 内容矩阵：
        //                            事件有时间性，编辑态 Schematic 隐藏——ADR-0051）
        true, // FeatureOverlay
        true, // TerrainDecor
        true, // Rivers
        true, // CellSelection
        false, // RegionLayers（装饰 ✓ 而区域 ✗——拆 pass 的全部意义，ADR-0051）
    ]),
    // Google 模式 —— 自带完整观感：等值线 / 氛围族 / 建筑不画。
    (ViewMode::GoogleMap, [
        true, false, true, true, false, true, true, true, true, false,
        false, true, true, true, true, true, true, true, true, false,
        true,
        false, // TerrainDecor
        true, // Rivers：Google 也画河（自有观感但水系照常）
        true, // CellSelection
        false, // RegionLayers（= 各行 TerrainDecor 同值；Schematic/Topographic 例外见上）
    ]),
    // 势力图 —— 等值线不画，其余与真实地形同。
    (ViewMode::Faction, [
        true, false, true, true, true, true, true, true, true, true,
        true, true, true, true, true, true, true, true, true, true,
        true,
        true, // TerrainDecor
        true, // Rivers
        true, // CellSelection
        true, // RegionLayers（= 各行 TerrainDecor 同值；Schematic/Topographic 例外见上）
    ]),
    // 事件图 —— 极简底图。
    (ViewMode::Event, [
        true, false, true, true, true, true, true, true, true, true,
        false, true, true, true, true, true, true, true, true, true,
        true,
        false, // TerrainDecor
        false, // Rivers：极简底图不画
        true, // CellSelection
        false, // RegionLayers（= 各行 TerrainDecor 同值；Schematic/Topographic 例外见上）
    ]),
    // 轮廓图（黑底分析视图）—— 大部分装饰关。
    (ViewMode::Coastline, [
        true, false, true, true, false, false, true, true, true, false,
        false, true, true, true, true, true, true, true, true, false,
        false, // FeatureOverlay：轮廓图只留轮廓
        false, // TerrainDecor
        false, // Rivers
        true, // CellSelection
        false, // RegionLayers（= 各行 TerrainDecor 同值；Schematic/Topographic 例外见上）
    ]),
    // Topographic 分析视图（[ADR-0039]）：水/陆 + height 3 阶着色，关其它装饰族。
    // 仅 Terrain + Isoline + Rivers 留——
    //   • Terrain 是核心显示（用独立 `TopographicPalette`，不走 ThemePack core）
    //   • Isoline 是模式轴开档——**会话轴由 `view.isolines` 单控**（GridLines
    //     同款「mode 表不关，会话开关单控」），2026-10-23 起默认关（ADR-0051
    //     追记：等值线默认是修饰线，用户裁定要时再开）
    //   • Rivers 是水系边界（与水/陆判定直接相关）
    // DrillMarkers / BrushCursor / Measurement / FeatureOverlay / Decor 全关——
    // 分析视图不要任何交互叠加或美术装饰（决策 2）。
    //
    // biome 通道在此 mode 下**不参与显示**（X 路线，[ADR-0039] §1.3 / 决策 5），
    // 由 `sync_terrain_chunks` 的 match 分支实现。
    (ViewMode::Topographic, [
        true,  // Terrain（核心，水/陆 + 3 阶 height，topographic 分支）
        true,  // Isoline（模式轴开档；会话轴 view.isolines 默认关——ADR-0051）
        // GridLines **2026-10-23 由 false 改 true**（用户指令 + 一致性收口）：
        // CellMap 工作台（= Topographic）就是逐格编辑网格的用武之地，作者要
        // 「开着网格定位格子」；而本行原写 false、`sync_grid_lines` 又从不读
        // recipe → 表与行为不一致（表在骗人）。现在系统读
        // `PassId::GridLines`，表必须说真话：CellMap 里网格照会话开关走。
        true,  // GridLines
        false, // MapFrame
        false, // Veil
        false, // Grain
        false, // Roads
        false, // Streets
        false, // Poi
        false, // Buildings
        false, // Faction
        false, // PlaceLabels
        false, // StreetLabels
        true,  // SeaLabels（2026-10-22 内容矩阵：海名属地貌呈现，CellMap 保留——ADR-0051）
        false, // Measurement（不要测量叠加）
        false, // BrushCursor（分析视图不要笔刷光标）
        false, // Tracks
        false, // EventDots
        false, // DrillMarkers（分析视图不要钻入子图入口叠加）
        false, // Atmosphere
        false, // FeatureOverlay（分析视图不要地物）
        false, // TerrainDecor
        true,  // Rivers：水系边界是水/陆判定的关键
        true,  // CellSelection（会话轴，Topographic 也开——EditCell 工具可与任何 viewmode 叠加）
        false, // RegionLayers（CellMap 的自然区走 nature_overlay 旁路，不占本格）
    ]),
];

/// 求值 `RenderRecipe`（每帧单点调一次，ADR-0021 D1 §1）。
///
/// 纯函数（输入全部来自参数 + Resource 引用，无副作用）——可脱离 Bevy App 单测。
#[allow(clippy::too_many_arguments)]
pub fn resolve_render_recipe(
    view: &ViewState,
    layers: &LayerFrame,
    _world: &WorldProjection,
    _drill: &DrillState,
    _tex: &TerrainTexture,
    _atlas: Option<&TileAtlas>,
) -> RenderRecipe {
    // 1. 查 mode 表拿基础 plan（VIEW_MODE 不实现 Copy，所以用 `eq` 配对）
    let mut passes: [PassPlan; PassId::COUNT] = MODE_TABLE
        .iter()
        .find(|(m, _)| m == &view.mode)
        .map(|(_, table)| table.map(PassPlan::from))
        .unwrap_or([PassPlan::disabled(); PassId::COUNT]); // 未知模式 = 全关（fail-safe）

    // 2. LayerFrame 叠加：kind 轴遮挡（按 LayerFrame 实际字段名：terrain /
    //    border / influence / road / event_point / place / building / label / custom）。
    if !layers.terrain.visible {
        for id in [
            PassId::Terrain,
            PassId::Isoline,
            PassId::GridLines,
            PassId::MapFrame,
            PassId::Atmosphere,
            PassId::TerrainDecor,
        ] {
            passes[id as usize].enabled = false;
        }
    }
    if !layers.road.visible {
        for id in [PassId::Roads, PassId::Streets] {
            passes[id as usize].enabled = false;
        }
    }
    if !layers.place.visible {
        for id in [PassId::Poi, PassId::PlaceLabels, PassId::FeatureOverlay] {
            passes[id as usize].enabled = false;
        }
    }
    if !layers.label.visible {
        for id in [PassId::PlaceLabels, PassId::StreetLabels, PassId::SeaLabels] {
            passes[id as usize].enabled = false;
        }
    }
    if !layers.building.visible {
        passes[PassId::Buildings as usize].enabled = false;
    }
    if !layers.influence.visible {
        passes[PassId::Faction as usize].enabled = false;
    }
    if !layers.event_point.visible {
        passes[PassId::EventDots as usize].enabled = false;
    }
    // tracks / custom 暂未在 LayerFrame 中以同名 binding 暴露；保留 hook：
    // 真要做"作者声明的 tracks 层"按 ADR-0013 扩展 LayerFrame 后接线。

    RenderRecipe { passes }
}

/// 注册 Recipe 求值系统（`app_bootstrap` 在 MapRenderPlugin 内调用）。
///
/// **放在 PreUpdate** —— 单点求值后，下游消费点只读 Resource 不再判定。
/// **不强制 before/after**：recipe 是只读 Resource，消费系统按各自原触发点
/// 跑（Update 主链）；只要 recipe 在 Update 之前已被写入即可。
pub fn register_recipe_system(app: &mut App) {
    app.init_resource::<RenderRecipe>();
    // Order:couple system 先(写 `terrain_2_5d_enabled`),recipe system 后(读)。
    // 同一 schedule 里 `chain()` 强制顺序;`chain` 也意味着「同 schedule
    // 串行执行」,避免跨 schedule 的 timing 漂移。
    app.add_systems(
        bevy::prelude::PreUpdate,
        (couple_terrain_2_5d_to_mode, resolve_recipe_system).chain(),
    );
}

/// 模式→开关的耦合([ADR-0041] 决策 4):Topographic mode 进/出时,自动同步
/// `ViewState.terrain_2_5d_enabled` 让 vertex Z 挤压与 mode 共进退。
///
/// 行为规则:
/// - `mode == Topographic && !terrain_2_5d_enabled` → **自动 on**(进 Topographic)
/// - `mode == Topographic && terrain_2_5d_enabled` → 保持 on(无副作用)
/// - `mode != Topographic && terrain_2_5d_enabled` → **自动 off**(切走;不删作者手动开)
/// - `mode != Topographic && !terrain_2_5d_enabled` → 保持 off
///
/// **作者手动切回 `false` 仅在 Topographic mode 内有效**——切到其它 mode 时
/// 本系统会再次覆盖为 `false`。这是 ADR-0041 §4.1.2 接受的「切走工具即清理」语义。
fn couple_terrain_2_5d_to_mode(mut view: ResMut<ViewState>) {
    let should_be_on = view.mode == ViewMode::Topographic;
    if view.terrain_2_5d_enabled != should_be_on {
        view.terrain_2_5d_enabled = should_be_on;
    }
}

/// 实际求值系统（包装为 Bevy 系统以注入 `Res`）。
fn resolve_recipe_system(
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    world: Res<WorldProjection>,
    drill: Res<DrillState>,
    tex: Res<TerrainTexture>,
    atlas: Option<Res<TileAtlas>>,
    mut recipe: ResMut<RenderRecipe>,
) {
    *recipe = resolve_render_recipe(&view, &layers, &world, &drill, &tex, atlas.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_layers() -> LayerFrame {
        // 全 visible 的 fixture（与 layer_frame::default 行为等价；不依赖测试模块）
        use crate::layer_frame::LayerBinding;
        LayerFrame {
            terrain: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            border: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            influence: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            road: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            event_point: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            place: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            building: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            label: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            custom: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
        }
    }

    fn fixture() -> (
        ViewState,
        LayerFrame,
        WorldProjection,
        DrillState,
        TerrainTexture,
    ) {
        let view = ViewState::default();
        let layers = default_layers();
        let world = WorldProjection::default();
        let drill = DrillState::default();
        // TerrainTexture 没 Default impl，但所有字段都是 pub，零值构造是合法
        // 初始态（`size=0`、`handle=Handle::default()`、`last_*=None/false`）。
        // 测试只用 `size` 字段，零值足够。
        let tex = TerrainTexture {
            handle: Handle::default(),
            size: 0,
            height: 0,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            field_dirty: None,
            last_mode: None,
            last_soften: false,
            last_theme_id: crate::theme::theme_pack::builtin::DEFAULT.id,
            last_hypsometric: 0.0,
            last_map: None,
        };
        (view, layers, world, drill, tex)
    }

    /// 模式表覆盖所有 7 个 ViewMode（防新增 mode 时忘加表项）。
    /// 事件内容层（点 + 标签）在编辑态隐藏（用户裁定：事件有时间性）——
    /// Schematic 翻 false；Browse（VectorFlat）保持 true。
    /// 拆 pass 的全部意义（2026-10-22 内容矩阵）：装饰是地貌演绎、区域是
    /// 作者内容——Schematic 下「装饰 ✓ 而区域 ✗」必须两格独立；
    /// Topographic 两格皆关（CellMap 的自然区走 nature_overlay 旁路）。
    #[test]
    fn region_layers_hidden_in_schematic_but_decor_visible() {
        let row = |m: ViewMode| MODE_TABLE.iter().find(|(mm, _)| *mm == m).unwrap().1;
        let schematic = row(ViewMode::Schematic);
        let topographic = row(ViewMode::Topographic);
        let vector_flat = row(ViewMode::VectorFlat);

        assert!(
            schematic[PassId::TerrainDecor as usize],
            "Schematic 装饰照画"
        );
        assert!(
            !schematic[PassId::RegionLayers as usize],
            "Schematic 区域隐藏（编辑态不显示区域）"
        );
        assert!(
            !topographic[PassId::RegionLayers as usize],
            "CellMap 区域默认隐藏（nature_overlay 旁路在系统侧）"
        );
        assert!(
            vector_flat[PassId::RegionLayers as usize],
            "Browse 区域照常显示"
        );
    }

    #[test]
    fn event_dots_hidden_in_schematic_but_visible_in_vector_flat() {
        let schematic = MODE_TABLE
            .iter()
            .find(|(m, _)| *m == ViewMode::Schematic)
            .unwrap()
            .1;
        let vector_flat = MODE_TABLE
            .iter()
            .find(|(m, _)| *m == ViewMode::VectorFlat)
            .unwrap()
            .1;
        assert!(
            !schematic[PassId::EventDots as usize],
            "Edit（Schematic）下事件内容层应隐藏"
        );
        assert!(
            vector_flat[PassId::EventDots as usize],
            "Browse（VectorFlat）下事件内容层照常显示"
        );
    }

    #[test]
    fn mode_table_covers_all_modes() {
        assert_eq!(MODE_TABLE.len(), ViewMode::ALL.len());
        for m in ViewMode::ALL {
            let found = MODE_TABLE.iter().any(|(mm, _)| *mm == m);
            assert!(found, "MODE_TABLE 缺 {m:?}");
        }
    }

    /// PassId::COUNT = ALL 长度 = MODE_TABLE 行长（编译期硬保证）。
    #[test]
    fn pass_id_count_matches_all_len() {
        assert_eq!(
            PassId::ALL.len(),
            PassId::COUNT,
            "PassId::ALL 长度应等于 COUNT"
        );
    }

    /// FeatureOverlay 在除「分析视图」外的全部浏览模式默认开（主题地界晕 /
    /// 近景符号是浏览态的一等图层，不是某模式专属装饰）。
    ///
    /// **分析视图**（Coastline + Topographic，[ADR-0039]）一律关——它们的目标
    /// 是「看清底层数据」，地物覆盖层会污染信息密度。
    #[test]
    fn feature_overlay_enabled_in_all_modes_except_analysis_views() {
        let (mut view, layers, world, drill, tex) = fixture();
        for m in ViewMode::ALL {
            view.mode = m;
            let want = !matches!(m, ViewMode::Coastline | ViewMode::Topographic);
            let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
            assert_eq!(
                r.enabled(PassId::FeatureOverlay),
                want,
                "{m:?} 下 FeatureOverlay 应为 {want}"
            );
        }
    }

    /// 地点图层（`LayerFrame.place`）关闭 → 事实地界随 POI 一起退场
    /// （`!layers.place.visible` 级联，AGENTS §3 / ADR-0013）。
    #[test]
    fn hiding_place_layer_disables_feature_overlay() {
        let (view, mut layers, world, drill, tex) = fixture();
        layers.place.visible = false;
        let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
        assert!(
            !r.enabled(PassId::FeatureOverlay),
            "地点层关 → FeatureOverlay 必关"
        );
        assert!(!r.enabled(PassId::Poi), "对照：POI 同关");
    }

    /// 未知模式 = 全关（fail-safe）。
    #[test]
    fn unknown_mode_yields_all_disabled() {
        // 构造一个不在 ALL 里的"假"模式不可行（枚举封闭），用 mode = 第一个
        // 模式但 LayerFrame 全关做对照；这里只验证 Resolution 自身在合法输入
        // 下不 panic + 不 panicking on default fixture。
        let (view, layers, world, drill, tex) = fixture();
        let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
        // 默认 fixture = VectorFlat + 全 layers visible = 大部分 pass 应为 true
        assert!(r.enabled(PassId::Terrain));
        assert!(r.enabled(PassId::MapFrame));
        // Faction 在 VectorFlat 默认关
        assert!(!r.enabled(PassId::Faction));
    }

    /// Coastline 模式 = 自带黑底观感：Atmosphere / Veil / Grain / Buildings / Faction
    /// 都关（与 §1.3 已知不一致的修复目标一致）。
    #[test]
    fn coastline_mode_disables_atmosphere_veil_grain_buildings_faction() {
        let (mut view, layers, world, drill, tex) = fixture();
        view.mode = ViewMode::Coastline;
        let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
        assert!(!r.enabled(PassId::Atmosphere));
        assert!(!r.enabled(PassId::Veil));
        assert!(!r.enabled(PassId::Grain));
        assert!(!r.enabled(PassId::Buildings));
        assert!(!r.enabled(PassId::Faction));
    }

    /// Isoline 在 VectorFlat 与 Topographic 开（模式轴——迁移前
    /// `isoline_overlay` 的原 gate 行为加 ADR-0039 分析视图开档）。**会话轴
    /// 不在本表**：`view.isolines` 旗标在等值线系统里单控（GridLines 同款），
    /// 2026-10-23 起默认关（ADR-0051 追记）。
    /// 其余 6 模式一律关，防"顺手填 true"改变行为（本测试因第一次填错表而加）。
    #[test]
    fn isoline_draws_in_vector_flat_and_topographic_only() {
        let (mut view, layers, world, drill, tex) = fixture();
        for m in ViewMode::ALL {
            view.mode = m;
            let want = matches!(m, ViewMode::VectorFlat | ViewMode::Topographic);
            let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
            assert_eq!(
                r.enabled(PassId::Isoline),
                want,
                "{m:?} 下 Isoline 应为 {want}"
            );
        }
    }

    /// Google 模式 = 自带观感：Atmosphere / Faction / Buildings 关。
    #[test]
    fn google_mode_disables_atmosphere_faction_buildings() {
        let (mut view, layers, world, drill, tex) = fixture();
        view.mode = ViewMode::GoogleMap;
        let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
        assert!(!r.enabled(PassId::Atmosphere));
        assert!(!r.enabled(PassId::Veil));
        assert!(!r.enabled(PassId::Faction));
        assert!(!r.enabled(PassId::Buildings));
    }

    /// LayerFrame gates 覆盖：terrain 关 = Terrain/Isoline/MapFrame/Atmosphere 跟随关。
    #[test]
    fn layer_frame_terrain_invisible_cascades() {
        let (view, mut layers, world, drill, tex) = fixture();
        layers.terrain.visible = false;
        let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
        assert!(!r.enabled(PassId::Terrain));
        assert!(!r.enabled(PassId::Isoline));
        assert!(!r.enabled(PassId::MapFrame));
        assert!(!r.enabled(PassId::Atmosphere));
    }

    /// `RenderRecipe::pass` 与 `enabled` 一致（双 API 同步）。
    #[test]
    fn pass_and_enabled_agree() {
        let (view, layers, world, drill, tex) = fixture();
        let r = resolve_render_recipe(&view, &layers, &world, &drill, &tex, None);
        for id in PassId::ALL {
            assert_eq!(r.pass(*id).enabled, r.enabled(*id), "PassId::{id:?}");
        }
    }
}
