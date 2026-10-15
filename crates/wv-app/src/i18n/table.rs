//! zh/en 文案查表。
//!
//! ## 纪律
//! **每个 `I18nKey` 变体必须**在 `lookup_zh` 与 `lookup_en` 中都有匹配 arm；
//! 漏写一处编译器立即报错（`non-exhaustive patterns`）——这是
//! 「无半翻译」契约的**唯一**强制点（ADR-0018 §2.1）。
//!
//! ## 不做什么
//! - **不插值**：模板**不含** `{}` / `{:.2}` 等占位符，所有拼接在调用点
//!   用 `format!` 完成（避免引入 Fluent MessageFormat，AGENTS §9）。
//!   这意味着含运行时值的文案（如「已选：foo」「缩放 1.23×」「人物 5 个」）
//!   拆为前缀 Key + 字面量连接，详见 §2.2 ADR 讨论。
//! - **不引入 FTL 文件**：表就在 Rust 代码里；diff 友好。

use super::keys::I18nKey;

/// 简体中文文案（仅本表查源）。
/// 短句优先；超过 30 汉字的文案重新切成更小的 Key，或保留一句一 Key。
pub(crate) fn lookup_zh(key: I18nKey) -> &'static str {
    match key {
        // ── 公共动作 ──
        I18nKey::CommonDelete => "删除",
        I18nKey::CommonCancel => "取消",
        I18nKey::CommonOk => "确定",
        I18nKey::CommonConfirm => "确认",
        I18nKey::CommonSave => "保存",
        I18nKey::CommonOpen => "打开",
        I18nKey::CommonClose => "关闭",
        I18nKey::CommonApply => "应用",
        I18nKey::CommonBack => "返回",
        I18nKey::CommonNew => "新建",
        I18nKey::CommonUnknown => "（未知）",
        I18nKey::CommonDeleteEntire => "删除整个",

        // ── 顶栏 ──
        I18nKey::TopBarUnsaved => "未保存",
        I18nKey::TopBarSaved => "已保存",
        I18nKey::TopBarUnsavedDot => "●",
        I18nKey::TopBarUnsavedSubtitle => "有未保存的改动",
        I18nKey::TopBarForceRedrawTooltip => "强制重画地形（F5）",
        I18nKey::TopBarNoProjectLoaded => "工程未加载",

        // ── 顶栏右侧计数（拆为纯片段） ──
        I18nKey::TopBarCountUnits => "Unit",
        I18nKey::TopBarCountCharacters => "人物",
        I18nKey::TopBarCountSituations => "事态",
        I18nKey::TopBarCountEvents => "事件",
        I18nKey::TopBarSelectedMapPrefix => "已选：",
        I18nKey::TopBarZoomPrefix => "缩放 ",

        // ── 欢迎页 ──
        I18nKey::WelcomeHeading => "World View · 地图编辑器",
        I18nKey::WelcomeTagline => "整理关键人物、地理、事件、关系的单机世界设定工具",
        I18nKey::WelcomeHelpText => "还没有打开的工程——新建或打开一个之后才能开始绘制",
        I18nKey::WelcomeOpenProjectBtn => "打开工程…",
        I18nKey::WelcomeNewProjectBtn => "新建工程…",
        I18nKey::WelcomeRecentHeading => "最近工程",
        I18nKey::WelcomeRecentEmpty => "（尚无最近工程）",

        // ── 新建工程对话框 ──
        I18nKey::NewProjectDialogTitle => "新建工程",
        I18nKey::NewProjectDialogDirLabel => "工程目录",
        I18nKey::NewProjectDialogNameLabel => "项目名称",
        I18nKey::NewProjectDialogNameHover => "可中文；将与位置拼成项目目录",
        I18nKey::NewProjectDialogNameHint => "例如：美洲大陆",
        I18nKey::NewProjectDialogLocationLabel => "位置",
        I18nKey::NewProjectDialogLocationHint => "项目将创建在这个目录下",
        I18nKey::NewProjectDialogBrowseDir => "浏览…",
        I18nKey::NewProjectDialogCreateBtn => "创建",
        I18nKey::NewProjectDialogPreviewForPrefix => "项目将在 ",
        I18nKey::NewProjectDialogPreviewForSuffix => " 创建目录",
        I18nKey::NewProjectDialogPreviewEmpty => "项目将在 位置\\项目名称 创建目录（先填名称、选位置）",
        I18nKey::NewProjectDialogDefaultName => "新工程",
        I18nKey::NewProjectDialogDefaultFirstMapName => "世界",

        // ── 新建地图对话框 ──
        I18nKey::NewMapDialogTitle => "新建地图",
        I18nKey::NewMapDialogIdLabel => "地图 ID",
        I18nKey::NewMapDialogNameLabel => "名称",
        I18nKey::NewMapDialogNameHover => "显示名，可中文；地图 ID（YAML/Git 里用的）会按 map_1、map_2… 自动生成",
        I18nKey::NewMapDialogNameHint => "新地图",
        I18nKey::NewMapDialogLevelLabel => "层级",
        I18nKey::NewMapDialogExtentLabel => "幅面",
        I18nKey::NewMapDialogPrefillLabel => "预填充",
        I18nKey::NewMapDialogDefaultName => "新地图",
        I18nKey::NewMapDialogFirstMapDefaultName => "世界",
        I18nKey::NewMapDialogFirstMapInfo => "将成为世界地图（钻取树的根）",
        I18nKey::NewMapDialogNoParentInfo => "（当前没有选中地图，将作为独立根图创建）",
        I18nKey::NewMapDialogGridInfoPrefix => "栅格 ",
        I18nKey::NewMapDialogGridInfoMid => "×",
        I18nKey::NewMapDialogGridInfoSuffix => "（每格 8 wu）",
        I18nKey::NewMapDialogParentUnderPrefix => "挂在「",
        I18nKey::NewMapDialogParentUnderSuffix => "」之下",
        I18nKey::NewMapDialogPrefillOcean => "海洋",
        I18nKey::NewMapDialogPrefillLand => "陆地",
        I18nKey::NewMapDialogLevelFirstMapHover => "空世界的第一张地图固定为世界图",
        I18nKey::NewMapDialogPrefillHover => "整图先铺满海洋或陆地，再用地形笔刷修形——矢量地形视图下「陆」由高度决定，预填充同时写高度与类型",
        I18nKey::NewMapDialogCreatedPrefix => "已创建地图「",
        I18nKey::NewMapDialogCreatedSuffix => "」",
        I18nKey::NewMapDialogErrCreateFailPrefix => "创建失败：",
        I18nKey::NewMapDialogCreateBtn => "创建",

        // ── 笔刷面板 ──
        I18nKey::BrushModeToggle => "笔刷模式（关闭时左键拖动地图）",
        I18nKey::BrushShowGrid => "显示网格",
        I18nKey::BrushTerrainHeader => "地形",
        I18nKey::BrushRadiusPrefix => "半径 ",
        I18nKey::BrushRadiusUnitWu => " wu",
        I18nKey::BrushSoftnessPrefix => "衰减软度 ",
        I18nKey::BrushStreetsHeader => "街道",
        I18nKey::BrushStreetsToggle => "画街道模式（左键逐点连线，右键结束）",
        I18nKey::BrushClassLabel => "类别",
        I18nKey::BrushStreetNameLabel => "街道名",
        I18nKey::BrushStreetsDrawnPrefix => "已画 ",
        I18nKey::BrushStreetsDrawnUnit => " 段",
        I18nKey::BrushFinishLineBtn => "结束连线",
        I18nKey::BrushUndoLastSegmentBtn => "撤销上一段",
        I18nKey::BrushHintRadiusShortcut => "Ctrl+滚轮改半径",

        // ── 视图模式 ──
        I18nKey::ViewModeVectorFlat => "矢量地形",
        I18nKey::ViewModeGoogleMap => "地图",
        I18nKey::ViewModeRealTerrain => "真实地形",
        I18nKey::ViewModeSchematic => "色块地形",
        I18nKey::ViewModeFaction => "势力图",
        I18nKey::ViewModeEvent => "事件图",
        I18nKey::ViewModeCoastline => "轮廓图",

        // ── 图层面板标题 ──
        I18nKey::PanelMapTree => "地图树",
        I18nKey::PanelLayers => "图层管理",
        I18nKey::PanelBrush => "地形设计器",
        I18nKey::PanelInspector => "属性检查器",
        I18nKey::PanelTimeline => "时间轴",
        I18nKey::PanelFaction => "势力编辑",
        I18nKey::PanelRegion => "区域编辑",
        I18nKey::PanelSituation => "事态登记",
        I18nKey::PanelValidation => "校验面板",
        I18nKey::PanelMinimap => "小地图",
        I18nKey::PanelCompass => "指南针",
        I18nKey::PanelCharacter => "人物登记",
        I18nKey::PanelEvent => "事件登记",

        // ── 地图层级 ──
        I18nKey::MapLevelWorld => "世界",
        I18nKey::MapLevelRegion => "区域",
        I18nKey::MapLevelCity => "城市",
        I18nKey::MapLevelInterior => "建筑内",

        // ── 命令面板分组 ──
        I18nKey::CommandGroupProject => "项目",
        I18nKey::CommandGroupView => "视图",
        I18nKey::CommandGroupEdit => "编辑",
        I18nKey::CommandGroupPanel => "面板",
        I18nKey::CommandGroupTime => "时间",
        I18nKey::CommandGroupInterface => "界面",
        I18nKey::CommandGroupObject => "对象",
        I18nKey::CommandGroupNavigation => "导航",

        // ── 时间轴层 ──
        I18nKey::TimelineLayerDay => "日",
        I18nKey::TimelineLayerSituation => "事态",
        I18nKey::TimelineLayerCharacter => "人物",
        I18nKey::TimelineLayerCentury => "世纪",
    }
}

/// English copy. Same shape as `lookup_zh`; missing arm = compile error.
pub(crate) fn lookup_en(key: I18nKey) -> &'static str {
    match key {
        // ── Common actions ──
        I18nKey::CommonDelete => "Delete",
        I18nKey::CommonCancel => "Cancel",
        I18nKey::CommonOk => "OK",
        I18nKey::CommonConfirm => "Confirm",
        I18nKey::CommonSave => "Save",
        I18nKey::CommonOpen => "Open",
        I18nKey::CommonClose => "Close",
        I18nKey::CommonApply => "Apply",
        I18nKey::CommonBack => "Back",
        I18nKey::CommonNew => "New",
        I18nKey::CommonUnknown => "(unknown)",
        I18nKey::CommonDeleteEntire => "Delete entire",

        // ── Top bar ──
        I18nKey::TopBarUnsaved => "Unsaved",
        I18nKey::TopBarSaved => "Saved",
        I18nKey::TopBarUnsavedDot => "●",
        I18nKey::TopBarUnsavedSubtitle => "There are unsaved changes",
        I18nKey::TopBarForceRedrawTooltip => "Force redraw terrain (F5)",
        I18nKey::TopBarNoProjectLoaded => "No project loaded",

        // ── Top bar status counts (split into plain fragments) ──
        I18nKey::TopBarCountUnits => "Unit",
        I18nKey::TopBarCountCharacters => "Person",
        I18nKey::TopBarCountSituations => "Situation",
        I18nKey::TopBarCountEvents => "Event",
        I18nKey::TopBarSelectedMapPrefix => "Selected: ",
        I18nKey::TopBarZoomPrefix => "Zoom ",

        // ── Welcome screen ──
        I18nKey::WelcomeHeading => "World View · Map editor",
        I18nKey::WelcomeTagline => "A single-user world-setting tool for characters, places, events, and ties",
        I18nKey::WelcomeHelpText => "No project loaded yet — create or open one before you start drawing",
        I18nKey::WelcomeOpenProjectBtn => "Open project…",
        I18nKey::WelcomeNewProjectBtn => "New project…",
        I18nKey::WelcomeRecentHeading => "Recently opened",
        I18nKey::WelcomeRecentEmpty => "(no recent projects)",

        // ── New project dialog ──
        I18nKey::NewProjectDialogTitle => "New project",
        I18nKey::NewProjectDialogDirLabel => "Project directory",
        I18nKey::NewProjectDialogNameLabel => "Project name",
        I18nKey::NewProjectDialogNameHover => "May contain CJK; will be joined with the location to form the directory",
        I18nKey::NewProjectDialogNameHint => "e.g. \"My Continent\"",
        I18nKey::NewProjectDialogLocationLabel => "Location",
        I18nKey::NewProjectDialogLocationHint => "Project will be created under this directory",
        I18nKey::NewProjectDialogBrowseDir => "Browse…",
        I18nKey::NewProjectDialogCreateBtn => "Create",
        I18nKey::NewProjectDialogPreviewForPrefix => "Project will be created at ",
        I18nKey::NewProjectDialogPreviewForSuffix => "",
        I18nKey::NewProjectDialogPreviewEmpty => "Project will be created at <location>/<name> (fill both to preview)",
        I18nKey::NewProjectDialogDefaultName => "Untitled project",
        I18nKey::NewProjectDialogDefaultFirstMapName => "World",

        // ── New map dialog ──
        I18nKey::NewMapDialogTitle => "New map",
        I18nKey::NewMapDialogIdLabel => "Map ID",
        I18nKey::NewMapDialogNameLabel => "Name",
        I18nKey::NewMapDialogNameHover => "Display name; map ID (used in YAML/Git) is generated as map_1, map_2, …",
        I18nKey::NewMapDialogNameHint => "New map",
        I18nKey::NewMapDialogLevelLabel => "Level",
        I18nKey::NewMapDialogExtentLabel => "Extent",
        I18nKey::NewMapDialogPrefillLabel => "Prefill",
        I18nKey::NewMapDialogDefaultName => "New map",
        I18nKey::NewMapDialogFirstMapDefaultName => "World",
        I18nKey::NewMapDialogFirstMapInfo => "This will become the world map (root of the drilldown tree)",
        I18nKey::NewMapDialogNoParentInfo => "(No map is currently selected; will be created as an independent root)",
        I18nKey::NewMapDialogGridInfoPrefix => "Grid ",
        I18nKey::NewMapDialogGridInfoMid => "×",
        I18nKey::NewMapDialogGridInfoSuffix => " (8 wu per cell)",
        I18nKey::NewMapDialogParentUnderPrefix => "under \"",
        I18nKey::NewMapDialogParentUnderSuffix => "\"",
        I18nKey::NewMapDialogPrefillOcean => "Ocean",
        I18nKey::NewMapDialogPrefillLand => "Land",
        I18nKey::NewMapDialogLevelFirstMapHover => "First map of an empty world is fixed to world level",
        I18nKey::NewMapDialogPrefillHover => "Fill the whole map with ocean or land first, then refine with brushes. Under VectorFlat, 'land' is determined by height; prefill writes both height and kind.",
        I18nKey::NewMapDialogCreatedPrefix => "Created map \"",
        I18nKey::NewMapDialogCreatedSuffix => "\"",
        I18nKey::NewMapDialogErrCreateFailPrefix => "Failed to create: ",
        I18nKey::NewMapDialogCreateBtn => "Create",

        // ── Brush panel ──
        I18nKey::BrushModeToggle => "Brush mode (drag the map with left-click when off)",
        I18nKey::BrushShowGrid => "Show grid",
        I18nKey::BrushTerrainHeader => "Terrain",
        I18nKey::BrushRadiusPrefix => "Radius ",
        I18nKey::BrushRadiusUnitWu => " wu",
        I18nKey::BrushSoftnessPrefix => "Softness ",
        I18nKey::BrushStreetsHeader => "Streets",
        I18nKey::BrushStreetsToggle => "Draw streets (left-click to add points, right-click to finish)",
        I18nKey::BrushClassLabel => "Class",
        I18nKey::BrushStreetNameLabel => "Street name",
        I18nKey::BrushStreetsDrawnPrefix => "Drawn ",
        I18nKey::BrushStreetsDrawnUnit => " segment(s)",
        I18nKey::BrushFinishLineBtn => "Finish line",
        I18nKey::BrushUndoLastSegmentBtn => "Undo last segment",
        I18nKey::BrushHintRadiusShortcut => "Ctrl + wheel to change radius",

        // ── View modes ──
        I18nKey::ViewModeVectorFlat => "Vector terrain",
        I18nKey::ViewModeGoogleMap => "Map",
        I18nKey::ViewModeRealTerrain => "Real terrain",
        I18nKey::ViewModeSchematic => "Flat-shaded terrain",
        I18nKey::ViewModeFaction => "Faction map",
        I18nKey::ViewModeEvent => "Event map",
        I18nKey::ViewModeCoastline => "Coastline",

        // ── Panel titles ──
        I18nKey::PanelMapTree => "Map tree",
        I18nKey::PanelLayers => "Layers",
        I18nKey::PanelBrush => "Terrain designer",
        I18nKey::PanelInspector => "Inspector",
        I18nKey::PanelTimeline => "Timeline",
        I18nKey::PanelFaction => "Faction editor",
        I18nKey::PanelRegion => "Region editor",
        I18nKey::PanelSituation => "Situation log",
        I18nKey::PanelValidation => "Validation",
        I18nKey::PanelMinimap => "Minimap",
        I18nKey::PanelCompass => "Compass",
        I18nKey::PanelCharacter => "Character log",
        I18nKey::PanelEvent => "Event log",

        // ── Map levels ──
        I18nKey::MapLevelWorld => "World",
        I18nKey::MapLevelRegion => "Region",
        I18nKey::MapLevelCity => "City",
        I18nKey::MapLevelInterior => "Interior",

        // ── Command palette groups ──
        I18nKey::CommandGroupProject => "Project",
        I18nKey::CommandGroupView => "View",
        I18nKey::CommandGroupEdit => "Edit",
        I18nKey::CommandGroupPanel => "Panels",
        I18nKey::CommandGroupTime => "Time",
        I18nKey::CommandGroupInterface => "Interface",
        I18nKey::CommandGroupObject => "Object",
        I18nKey::CommandGroupNavigation => "Navigation",

        // ── Timeline layers ──
        I18nKey::TimelineLayerDay => "Day",
        I18nKey::TimelineLayerSituation => "Situation",
        I18nKey::TimelineLayerCharacter => "Character",
        I18nKey::TimelineLayerCentury => "Century",
    }
}
