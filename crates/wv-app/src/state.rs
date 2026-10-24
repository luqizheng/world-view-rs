//! 应用层状态容器（Bevy Resource）——ADR-0020 拆分后的应用会话态。
//!
//! ## 职责（ADR-0020 拆分后）
//! - **重导出**渲染消费的视图状态（`ViewState` / `WorldProjection` / `DrillState` /
//!   `BrushState` / `MeasureState` / `PlaybackState` / `GridConfig` / `LodTier` /
//!   `ViewMode` / `lod_tier` 等，本体在 `wv_render::state`）——
//!   应用侧既有 `use crate::state::X` 路径零改动（ADR-0020 §2.3/§2.6）。
//! - 保留 **UI / 工程会话态**：`Panel` / `PanelState`（egui 面板显隐）、
//!   `ProjectDiagnostics`（校验缓存，依赖 `wv_io`）、`RecentProjects`（最近工程）、
//!   `map_level_label` / `ViewModeLabelExt`（i18n 文案——外壳关注点，AGENTS §2）。
//!
//! ## 边界（不做什么）
//! - **不修改世界**——`WorldProjection.world` 只能由 `wv-edit` 的 `EditCommand` 改
//!   （AGENTS.md §3.3）。
//! - **不是 source of truth**——YAML 才是。
//! - **不持有渲染状态本体**——那在 `wv_render::state`（ADR-0020）。

use bevy::prelude::*;
use wv_io::diagnostics::Diagnostics;

pub use wv_render::state::{
    lod_tier, BrushState, BrushTool, BrushToolGroup, DrillState, EditCellState, GridConfig,
    LodTier, MeasureMode, MeasureState, PlaybackState, SelectionState, ViewMode, ViewState,
    WorldProjection, LOD_NATION_SCALE, LOD_REGION_SCALE,
};

/// 泛洪选择工具会话态（[ADR-0052] D6）。
///
/// 经 `wv_render::state` 的 re-export 链暴露（与 `EditCellState` 同款路径）。
pub use wv_world_cell_map::state::{FloodCriterion, FloodSelectState};

// 2026-10-18（ADR-0033）：`ContinentGenerationStash` / `ContinentGenerationState`
// 已删除。意图生效不再消费选区，撤销/重做无需把意图圈「还回来」——
// 选区自始至终留在 `SelectionState`。

/// [`ViewMode`](wv_render::state::ViewMode) 的顶栏显示名（ADR-0018：locale 查表）。
///
/// ADR-0020 把 `ViewMode` 本体迁入 `wv-render` 后，i18n 文案留在应用层
/// （文案是外壳关注点，AGENTS §2 依赖方向）。以扩展 trait 维持
/// `mode.label(loc)` 调用点语法不变；调用方需 `use crate::state::ViewModeLabelExt`。
pub trait ViewModeLabelExt {
    /// 顶栏显示名。
    fn label(self, loc: crate::i18n::Locale) -> &'static str;
}

impl ViewModeLabelExt for wv_render::state::ViewMode {
    fn label(self, loc: crate::i18n::Locale) -> &'static str {
        use crate::i18n::{t, I18nKey};
        match self {
            // 2026-10-14 用户裁定：浏览态主打模式定名「矢量地形」（此前叫
            // 「等值线」——等值线只是它的描边产物，名字该描述整体观感）。
            wv_render::state::ViewMode::VectorFlat => t(loc, I18nKey::ViewModeVectorFlat),
            wv_render::state::ViewMode::GoogleMap => t(loc, I18nKey::ViewModeGoogleMap),
            wv_render::state::ViewMode::RealTerrain => t(loc, I18nKey::ViewModeRealTerrain),
            wv_render::state::ViewMode::Schematic => t(loc, I18nKey::ViewModeSchematic),
            wv_render::state::ViewMode::Faction => t(loc, I18nKey::ViewModeFaction),
            wv_render::state::ViewMode::Event => t(loc, I18nKey::ViewModeEvent),
            wv_render::state::ViewMode::Coastline => t(loc, I18nKey::ViewModeCoastline),
            wv_render::state::ViewMode::Topographic => t(loc, I18nKey::ViewModeTopographic),
        }
    }
}

/// 当前工程的校验结果缓存（来自 `wv_io::diagnostics::derive_hints`）。
///
/// 为什么单独存一个资源而不是每次 UI 帧都算：
/// `derive_hints` 会跑 N 个事件 × 距离 A*，**只读**世界即可，无副作用。
/// 缓存层是为了让 UI 渲染零成本读，触发点收敛到「加载完成 / 编辑提交 / 撤销重做」。
///
/// **V1 守约**（AGENTS §7.5）：`Diagnostics` 把错误与提示在类型上分开，
/// 校验面板据此只列、不阻止保存。错误仅在「数据坏了」时出现——本工程
/// 加载期已硬失败（见 `wv_io::Project::load_world`），故面板里的错误主要
/// 来自加载后的二次校验（如悬空引用）；提示包括相遇提示、事件位移可行性等。
#[derive(Resource, Default)]
pub struct ProjectDiagnostics {
    /// 派生诊断项。空 = 还没跑过诊断（启动期）。
    pub diagnostics: Diagnostics,
    /// 最近一次诊断的项数。仅用于诊断自身的可见性展示（"X 项"计数）。
    pub last_count: usize,
    /// `true` = 需要重跑 [`wv_io::diagnostics::derive_hints`]。
    ///
    /// 由「加载完成 / 新建 / 编辑提交 / 撤销重做」四个点 set true，
    /// 由 `refresh_project_diagnostics` system 在重算后清回 false。
    /// UI 面板每帧**只读** `diagnostics`，不感知此标志。
    pub needs_diagnose: bool,
}

/// 自然区域面板的**会话态**（ADR-0045 D4「命名即物化」）。
///
/// 面板是无状态的渲染函数，跨帧的输入文本必须有处放——独立 Resource 而非
/// `PanelState`（浮窗闭包与 `PanelState` 互斥借用）。
#[derive(Resource, Debug, Default)]
pub struct NatureRegionUiState {
    /// 物化时用的名字（作者输入；物化成功后由面板清空）。
    pub name: String,
    /// CellMap 候选命名上下文（2026-10-22 内容矩阵 §2.6-4）：点击候选格 →
    /// 记下该格（栅格坐标）；Inspector「属性」页签据此出候选命名表单。
    /// `None` = 无候选上下文。丢弃/物化成功后清空。
    pub pending_candidate: Option<(i32, i32)>,
    /// 候选的**编辑中多边形**（图内局部 wu，扁平坐标）。`None` = 未拖动，
    /// 物化时按格重新派生；`Some` = 作者拖过锚点，物化直接用此形状
    /// （2026-10-22 验收：候选态也要能拉锚点调整形状）。
    pub candidate_polygon: Option<Vec<f32>>,
}

/// 可浮动的面板枚举（基线 §1.11 的 8 个面板 + 小地图）。
///
/// **关键**：面板是浮动的（`egui::Window`），**不推挤地图**（§1.11 R1/R2）。
/// 每个面板的开关与位置都记录在 [`PanelState`] 里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    /// P-1 地图树：World→Region→City→Interior，切换当前图。
    MapTree,
    /// P-2 图层管理：图层开关。
    Layers,
    /// P-3 笔刷工具：紧凑 Photoshop 式浮动窗（2 列 × 5 行色块，街道区折叠）。
    Brush,
    /// P-4 属性检查器。
    Inspector,
    /// P-5 时间轴 + 播放控件（底部浮动）。
    Timeline,
    /// P-6 势力编辑。
    Faction,
    /// P-6b 区域编辑（ADR-0017 §2.2）。
    Region,
    /// P-7 事态登记（含事件登记入口）。
    Situation,
    /// P-8 校验面板。
    Validation,
    /// 小地图（用户 1.2 要求，含图层开关）。
    Minimap,
    /// 人物登记（用户 1.4 要求）。
    Character,
    /// 事件登记（用户 1.4 要求，可从时间轴进入）。
    Event,
    /// Cell 编辑器（[ADR-0040]，2026-10-20 独立面板化）：单格水陆/海拔/群系编辑。
    CellEditor,
}

impl Panel {
    /// 全部面板（菜单按此顺序列出）。
    ///
    /// **2026-10-22 统一 Inspector**：`CellEditor` / `Faction` / `Situation`
    /// 三个面板已从中移除——CellEditor 的内容迁进了 Inspector 的「属性」页签；
    /// Faction 与 Situation 分别成为 Inspector 的「势力层」「事态」页签。
    /// 独立浮窗均不再渲染（`app_bootstrap` 不再注册对应系统）。本表驱动
    /// **用户可见的列表面**（命令面板 / 设置对话框 / 布局落盘），
    /// 故一个不渲染的面板留在本表里 = 菜单里出现「点了没反应」的死项。
    ///
    /// 枚举变体本身**暂留**——存量工程的 `ui-layout.yaml` 里有对应键，
    /// 删枚举会让旧文件的该项变成孤儿。
    pub const ALL: [Panel; 9] = [
        Panel::MapTree,
        Panel::Layers,
        Panel::Brush,
        Panel::Timeline,
        Panel::Event,
        Panel::Character,
        Panel::Region,
        Panel::Inspector,
        Panel::Validation,
    ];

    /// 菜单 / 标题显示名（ADR-0018：locale-dependent 文案查表）。
    pub fn title(self, loc: crate::i18n::Locale) -> &'static str {
        use crate::i18n::{t, I18nKey};
        match self {
            Panel::MapTree => t(loc, I18nKey::PanelMapTree),
            Panel::Layers => t(loc, I18nKey::PanelLayers),
            Panel::Brush => t(loc, I18nKey::PanelBrush),
            Panel::Inspector => t(loc, I18nKey::PanelInspector),
            Panel::Timeline => t(loc, I18nKey::PanelTimeline),
            Panel::Faction => t(loc, I18nKey::PanelFaction),
            Panel::Region => t(loc, I18nKey::PanelRegion),
            Panel::Situation => t(loc, I18nKey::PanelSituation),
            Panel::Validation => t(loc, I18nKey::PanelValidation),
            Panel::Minimap => t(loc, I18nKey::PanelMinimap),
            Panel::Character => t(loc, I18nKey::PanelCharacter),
            Panel::Event => t(loc, I18nKey::PanelEvent),
            Panel::CellEditor => t(loc, I18nKey::PanelCellEditor),
        }
    }

    /// 默认位置（首次打开 / 重置位置时用）。坐标单位是窗口逻辑像素。
    ///
    /// **互不重叠**是硬要求（设计规格 §3.3，验证项 §9.3-1 由
    /// [`crate::ui_layout::tests`] 自动把关）。布局规则：
    ///
    /// - 四列，列起点 `x = 12 / 332 / 652 / 972`，列宽由该列最宽面板决定
    ///   （最右边界 1272 ≤ 1280，1280 宽窗口正好放下）；
    /// - **底部 880 起是时间轴专用带**（§3.3 P-5「底部通栏」）——它是通栏，
    ///   x 与所有面板相交，因此**其余面板必须全部止于 y = 880**，
    ///   否则时间轴一开就压住它们。
    ///
    /// 用「预留带」而不是「让时间轴绕开面板」：通栏是规格语义，绕开就不是通栏了。
    pub fn default_pos(self) -> [f32; 2] {
        match self {
            // 2026-10-23 用户裁定：图层管理（接真实图层声明，Figma `3:20892`）
            // 顶到左上角，地图树（静态占位）让位到中列顶部。图层面板 268 宽
            // （56+480=536 底）与 Brush 顶 578 仍留 42 间隙，左列不重叠。
            Panel::Layers => [12.0, 56.0],
            Panel::MapTree => [332.0, 56.0],
            Panel::Brush => [12.0, 578.0],
            Panel::Minimap => [332.0, 56.0],
            Panel::Faction => [332.0, 368.0],
            Panel::Region => [332.0, 680.0],
            Panel::Event => [652.0, 56.0],
            Panel::Situation => [652.0, 380.0],
            // v0.3（Figma）：Inspector 改到顶栏正下方右侧（紧贴 Figma 风格的右上角），
            // 旧位 972,308 让给 Validation；Character / Event 错开避免矩形重叠。
            Panel::Validation => [12.0, 808.0],
            Panel::Inspector => [992.0, 56.0],
            // Cell 编辑器：右列 Inspector 下方（56+240=296 底，留 14 间隙），
            // 310+198=508 ≤ Character 顶 520，右列三面板互不重叠。
            Panel::CellEditor => [992.0, 310.0],
            Panel::Character => [992.0, 520.0],
            // 底部通栏（§3.3 P-5）。
            Panel::Timeline => [12.0, 880.0],
        }
    }

    /// 默认尺寸 `[宽, 高]`（设计规格 §3.3「默认宽」列）。
    pub fn default_size(self) -> [f32; 2] {
        match self {
            // v0.3-B：宽度对齐 web（.w-tree/.w-layers 204 / .w-brush 274）。
            Panel::MapTree => [204.0, 330.0],
            // Figma `3:20892` 面板宽 268；高 480 = 头 30 + 动作行/列表/滑条
            // （≈313）+ 会话开关段（≈120）+ 边距，列表超 6 行内部滚动。
            Panel::Layers => [268.0, 480.0],
            Panel::Brush => [274.0, 220.0],
            Panel::Minimap => [216.0, 300.0],
            // 通栏：1280 − 12×2 = 1256；高 164 对齐 web `.timeline`（50 顶排 + 轨道）。
            Panel::Timeline => [1256.0, 164.0],
            Panel::Event => [300.0, 312.0],
            Panel::Situation => [300.0, 308.0],
            Panel::Character => [280.0, 260.0],
            Panel::Faction => [280.0, 300.0],
            Panel::Region => [280.0, 200.0],
            // 2026-10-22 统一 Inspector：属性表单要放「标签 + 控件」两列，
            // 264px 太窄。Figma `42:2` 的 Inspector 是 292px，web 原型
            // `.w-inspector` 也是 292px——取 300 留边距。
            // 高 420 = 右列空隙（Inspector 底 56+420=476 < Character 顶 520）。
            Panel::Inspector => [300.0, 420.0],
            // Cell 编辑器：内容已迁入统一 Inspector 的「属性」页签
            // （2026-10-22 设计），**不再单独渲染**。枚举项与默认位暂留，
            // 兼容存量 `ui-layout.yaml` 的 `cell_editor` 键；退役见 M4。
            Panel::CellEditor => [288.0, 210.0],
            // v0.3：Validation 改成 240 高（默认隐藏，validation 触发时才出现）
            Panel::Validation => [300.0, 60.0],
        }
    }

    /// 面板 id（`ui-layout.yaml` 的键）。
    pub fn key(self) -> &'static str {
        match self {
            Panel::MapTree => "tree",
            Panel::Layers => "layers",
            Panel::Brush => "brush",
            Panel::Inspector => "inspector",
            Panel::Timeline => "timeline",
            Panel::Faction => "faction",
            Panel::Region => "region",
            Panel::Situation => "situation",
            Panel::Validation => "validation",
            Panel::Minimap => "minimap",
            Panel::Character => "character",
            Panel::Event => "event",
            Panel::CellEditor => "cell_editor",
        }
    }

    /// 面板头类型图标（Figma 头部图标一律 Lucide 12×12，见 `3:20893` / `42:4`）。
    ///
    /// 2026-10-18 由几何字符改 SVG：旧字符里 ◈ ◧ ⬡ 在打包字体（Inter +
    /// Noto Sans SC）无字形，egui 落成豆腐块——实机截图确认，非渲染问题。
    /// 图标资产在 `crates/wv-ui/assets/ui/tool_icons/`（AGENTS.md §8）。
    pub fn icon(self) -> wv_ui::widgets::HeadIcon {
        use wv_ui::widgets::{
            HeadIcon, ICON_ALERT, ICON_BRUSH, ICON_CALENDAR, ICON_CLOCK, ICON_FLAG, ICON_GLOBE,
            ICON_LANDMARK, ICON_LAYERS, ICON_LIST, ICON_MAP, ICON_MAP_PIN, ICON_SLIDERS, ICON_USER,
        };
        HeadIcon::Svg(match self {
            Panel::MapTree => ICON_GLOBE,
            Panel::Layers => ICON_LAYERS,
            Panel::Brush => ICON_BRUSH,
            Panel::Inspector => ICON_SLIDERS,
            Panel::Timeline => ICON_CLOCK,
            Panel::Faction => ICON_FLAG,
            Panel::Region => ICON_LANDMARK,
            Panel::Situation => ICON_LIST,
            Panel::Validation => ICON_ALERT,
            Panel::Minimap => ICON_MAP,
            Panel::Character => ICON_USER,
            Panel::Event => ICON_CALENDAR,
            Panel::CellEditor => ICON_MAP_PIN,
        })
    }

    /// 是否「数据面板」（§9.2 `data_opt()`：小窗可隐藏，`<900px` 窗口默认收起）。
    pub fn data_opt(self) -> bool {
        matches!(
            self,
            Panel::Brush | Panel::Layers | Panel::Region | Panel::Validation
        )
    }

    /// 首次启动的默认可见性（地图为主，D1：默认开的面板尽量少）。
    pub fn default_visible(self) -> bool {
        matches!(self, Panel::Brush | Panel::Timeline)
    }
}

/// 面板可见性与位置状态。
///
/// **为什么用资源而不是 egui 自己的记忆**：用户要求「顶部菜单可启动/关闭、
/// 可重置位置」，需要程序化控制，不能只靠 egui 的隐式 memory。
#[derive(Resource)]
pub struct PanelState {
    /// 每个面板是否可见。
    pub visible: std::collections::HashMap<Panel, bool>,
    /// 重置位置信号：置 true 后由 UI system 消费并清回 false。
    pub reset_requested: bool,
    /// 沉浸模式（`Tab`）：所有面板淡出但**仍可点击**（动画 §4 规范：alpha 0.10）。
    pub immersive: bool,
    /// CellMap 工作区（顶栏第三段，2026-10-20）进入时的面板可见性快照。
    ///
    /// 进 CellMap：快照当前全部面板 visible → 只开 Inspector（Cell 编辑唯一
    /// 相关面板）；退 CellMap（点 Browse/Edit 或再点 CellMap）：还原快照并清
    /// `None`。`None` = 不在工作区。
    pub cell_workspace_backup: Option<Vec<(Panel, bool)>>,
}

impl Default for PanelState {
    fn default() -> Self {
        use std::collections::HashMap;
        // 默认只开最核心的几个，保证「地图为主」——
        // 用户反馈「地图可视面积太少」，所以默认打开的面板要尽量少。
        let mut visible = HashMap::new();
        for p in Panel::ALL {
            visible.insert(p, false);
        }
        visible.insert(Panel::Brush, true);
        // visible.insert(Panel::Minimap, true); // TODO: 恢复小地图后取消注释
        visible.insert(Panel::Timeline, true);
        Self {
            visible,
            reset_requested: false,
            immersive: false,
            cell_workspace_backup: None,
        }
    }
}

impl PanelState {
    pub fn is_visible(&self, p: Panel) -> bool {
        self.visible.get(&p).copied().unwrap_or(false)
    }

    pub fn set_visible(&mut self, p: Panel, v: bool) {
        self.visible.insert(p, v);
    }

    pub fn toggle(&mut self, p: Panel) {
        let v = !self.is_visible(p);
        self.set_visible(p, v);
    }
}

// ---------------------------------------------------------------------------
// 地图三级钻取（v0.2 §B）
// ---------------------------------------------------------------------------

/// 图层级别的显示名（ADR-0018：locale-dependent 文案查表）。
///
/// 放在 UI 层而不是给 `wv_core::map::MapLevel` 加 `label()`：数据层不该出现
/// 面向作者的中文文案（AGENTS §2 依赖方向，文案是外壳关注点）。
pub fn map_level_label(level: wv_core::map::MapLevel, loc: crate::i18n::Locale) -> &'static str {
    use crate::i18n::{t, I18nKey};
    use wv_core::map::MapLevel;
    match level {
        MapLevel::World => t(loc, I18nKey::MapLevelWorld),
        MapLevel::Region => t(loc, I18nKey::MapLevelRegion),
        MapLevel::City => t(loc, I18nKey::MapLevelCity),
        MapLevel::Interior => t(loc, I18nKey::MapLevelInterior),
    }
}

// ---------------------------------------------------------------------------
// 最近工程（v0.2 §A 项目管理菜单）
// ---------------------------------------------------------------------------

/// 一个「最近打开」的工程条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    /// 工程根目录（`world.yaml` 所在目录）。
    pub dir: String,
    /// 显示名（`world.yaml` 的 `meta.name`）。
    pub name: String,
    /// 上次被 `remember` / 当前正在打开的时间戳。
    ///
    /// **会话内 monotonic 时钟**（`Instant` 不参与 Serialize，所以目前**不**进
    /// 落盘；跨会话持久化见 README §12 待决）。欢迎页右侧 `time_ago` 由此
    /// 算出（`format_time_ago`，见 [`crate::ui::widgets`]）。
    pub last_opened_at: std::time::Instant,
}

/// 最近打开的工程列表。
///
/// ## 为什么是**会话内**列表
///
/// 跨会话记住「最近工程」需要一处**工程之外**的配置存放点，而 AGENTS §1 P2
/// （目录即工程）明确禁止把状态写进注册表 / AppData / 全局配置。这里不擅自
/// 开这个口子，只维持会话内的列表；当前打开的工程由
/// [`WorldProjection::project_dir`] 提供，所以菜单永远至少有它一条。
///
/// 待决：是否落盘、落在哪里（`view.yaml` 的 `search_paths`？还是干脆不做），
/// 见 `README.md` §12 待决清单的问题记录。
#[derive(Resource, Debug, Default)]
pub struct RecentProjects {
    entries: Vec<RecentProject>,
}

/// 会话内最多记住几个最近工程（原型给的是 3 条，这里留一点余量）。
pub const RECENT_PROJECTS_MAX: usize = 8;

impl RecentProjects {
    /// 已记录的工程（按最近使用排序）。
    pub fn entries(&self) -> &[RecentProject] {
        &self.entries
    }

    /// 置顶一个工程（按目录去重，超上限截断）。时间戳由 `remember_at` 灌入
    /// 当前时刻——多数 caller 都想要「现在」语义，所以这个 wrapper 是默认入口。
    pub fn remember(&mut self, dir: &str, name: &str) {
        self.remember_at(dir, name, std::time::Instant::now());
    }

    /// 时间戳由 caller 注入的版本。测试用——给一个固定 `Instant` 让断言无 flaky。
    /// 业务路径都用 [`Self::remember`]。
    pub fn remember_at(&mut self, dir: &str, name: &str, at: std::time::Instant) {
        if dir.is_empty() {
            return;
        }
        self.entries.retain(|e| e.dir != dir);
        self.entries.insert(
            0,
            RecentProject {
                dir: dir.to_owned(),
                name: name.to_owned(),
                last_opened_at: at,
            },
        );
        self.entries.truncate(RECENT_PROJECTS_MAX);
    }

    /// 菜单 / 命令面板实际要显示的列表：会话记录 **∪** 当前工程。
    ///
    /// 当前工程放在最前——用户最可能想确认的就是「我现在开的是哪个」。
    /// 当前正在打开的工程其 `last_opened_at` 取 `Instant::now()`（"现在开着"
    /// 这一事实本身就是它最近一次被打开的标志）；如果之后某次工程**重新**
    /// 进入 `RecentProjects.entries`（例如关闭后再次打开），那个时间戳会
    /// 由 [`Self::remember`] 在打开时被刷新。
    pub fn visible(&self, projection: &WorldProjection) -> Vec<RecentProject> {
        let now = std::time::Instant::now();
        let mut out: Vec<RecentProject> = Vec::new();
        if let Some(dir) = projection.project_dir.as_deref() {
            let name = projection
                .world
                .meta
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| short_dir(dir));
            out.push(RecentProject {
                dir: dir.to_owned(),
                name,
                last_opened_at: now,
            });
        }
        for e in &self.entries {
            if !out.iter().any(|o| o.dir == e.dir) {
                out.push(e.clone());
            }
        }
        out
    }
}

/// 目录末段（用于没有 `meta.name` 时兜底显示）。
fn short_dir(dir: &str) -> String {
    std::path::Path::new(dir)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::id::Id;
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::time::TimeSpan;
    use wv_core::{World, WorldMeta};

    fn node(
        id: &str,
        name: &str,
        level: MapLevel,
        parent: Option<&str>,
        children: &[&str],
    ) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: name.to_owned(),
            level,
            parent: parent.map(Id::new_unchecked),
            children: children.iter().map(|c| Id::new_unchecked(*c)).collect(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        }
    }

    /// world_main ─┬─ city_zunyi ─── bld_yamen
    ///             └─ city_chishui
    /// （与 `wv_render::state` 测试里的同名 helper 同一套演示数据。）
    fn demo_world() -> World {
        let mut w = World {
            meta: Some(WorldMeta {
                name: "四渡赤水".into(),
                version: "0.1".into(),
                schema_version: 2,
                root_map: Id::new_unchecked("world_main"),
                timeline_range: TimeSpan::new(1, 180_001).expect("测试区间合法"),
                template: None,
            }),
            ..World::default()
        };
        w.maps = vec![
            node(
                "world_main",
                "主世界",
                MapLevel::World,
                None,
                &["city_zunyi", "city_chishui"],
            ),
            node(
                "city_zunyi",
                "遵义城",
                MapLevel::City,
                Some("world_main"),
                &["bld_yamen"],
            ),
            node(
                "city_chishui",
                "赤水城",
                MapLevel::City,
                Some("world_main"),
                &[],
            ),
        ];
        w
    }

    #[test]
    fn level_labels_are_chinese_and_cover_all_four_levels() {
        use wv_core::map::MapLevel;
        for l in [
            MapLevel::World,
            MapLevel::Region,
            MapLevel::City,
            MapLevel::Interior,
        ] {
            assert!(!map_level_label(l, crate::i18n::Locale::default()).is_empty());
        }
        assert_eq!(
            map_level_label(MapLevel::Interior, crate::i18n::Locale::default()),
            "建筑内"
        );
    }

    #[test]
    fn recent_projects_deduplicates_by_dir_and_keeps_most_recent_first() {
        let mut r = RecentProjects::default();
        r.remember("/w/a", "甲");
        r.remember("/w/b", "乙");
        r.remember("/w/a", "甲改");
        let names: Vec<&str> = r.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["甲改", "乙"], "重复目录应置顶而不是追加");
    }

    #[test]
    fn recent_projects_caps_the_list_and_ignores_empty_dirs() {
        let mut r = RecentProjects::default();
        for i in 0..(RECENT_PROJECTS_MAX + 5) {
            r.remember(&format!("/w/{i}"), "x");
        }
        assert_eq!(r.entries().len(), RECENT_PROJECTS_MAX);
        r.remember("", "无目录");
        assert_eq!(r.entries().len(), RECENT_PROJECTS_MAX, "空目录不入表");
    }

    /// `remember_at` 用显式 `Instant` 灌入，断言两次 remember 之间的时间戳
    /// 严格区分——会话内 monotonic 时钟下 `a < b` 意味着 `a + 1ns <= b` 成立。
    #[test]
    fn remember_at_records_explicit_instant_per_dir() {
        use std::time::{Duration, Instant};
        let mut r = RecentProjects::default();
        let t0 = Instant::now();
        r.remember_at("/w/a", "alpha", t0);
        r.remember_at("/w/b", "beta", t0 + Duration::from_secs(60));
        r.remember_at("/w/c", "gamma", t0 + Duration::from_secs(86400));

        let entries = r.entries();
        assert_eq!(entries.len(), 3);
        // newest-first（最近放在最前）；c 是最新
        assert_eq!(entries[0].dir, "/w/c");
        assert_eq!(entries[0].last_opened_at, t0 + Duration::from_secs(86400));
        assert_eq!(entries[1].dir, "/w/b");
        assert_eq!(entries[1].last_opened_at, t0 + Duration::from_secs(60));
        assert_eq!(entries[2].dir, "/w/a");
        assert_eq!(entries[2].last_opened_at, t0);
        // 重新 remember 同一个 dir 时，旧条目被替换、新时间戳生效
        r.remember_at("/w/a", "alpha v2", t0 + Duration::from_secs(30));
        let entries = r.entries();
        assert_eq!(entries[0].dir, "/w/a", "去重后置顶");
        assert_eq!(entries[0].name, "alpha v2", "name 也被替换");
        assert_eq!(entries[0].last_opened_at, t0 + Duration::from_secs(30));
        assert_eq!(entries.len(), 3, "去重不增加计数");
    }

    #[test]
    fn visible_recent_projects_always_leads_with_the_open_project() {
        let mut r = RecentProjects::default();
        r.remember("/w/other", "别的工程");
        let mut projection = WorldProjection::default();
        // 还没打开任何工程：只有会话记录。
        assert_eq!(r.visible(&projection).len(), 1);

        projection.project_dir = Some("/w/current".into());
        projection.world = demo_world();
        let shown = r.visible(&projection);
        assert_eq!(shown[0].dir, "/w/current");
        assert_eq!(shown[0].name, "四渡赤水", "名字取 world.yaml 的 meta.name");
        assert_eq!(shown[1].dir, "/w/other");
    }

    #[test]
    fn visible_recent_projects_does_not_repeat_the_open_project() {
        let r = {
            let mut r = RecentProjects::default();
            r.remember("/w/current", "四渡赤水");
            r
        };
        let projection = WorldProjection {
            project_dir: Some("/w/current".into()),
            ..WorldProjection::default()
        };
        assert_eq!(r.visible(&projection).len(), 1, "当前工程不应出现两次");
    }

    #[test]
    fn project_without_meta_name_falls_back_to_the_directory_leaf() {
        let projection = WorldProjection {
            project_dir: Some("/worlds/minimal-world".into()),
            ..WorldProjection::default()
        };
        let shown = RecentProjects::default().visible(&projection);
        assert_eq!(shown[0].name, "minimal-world");
    }
}
