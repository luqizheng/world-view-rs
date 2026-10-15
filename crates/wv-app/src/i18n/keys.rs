//! UI 文案键枚举。
//!
//! ## 纪律（AGENTS §4.4 精神）
//! 键名一概使用 `PascalCase` 段分组 + `PascalCase` 短名（与 Rust 类型名同惯例）；
//! **禁止**自增整数（diff 不可读）。每个键在 `table.rs` 的 `lookup_zh` /
//! `lookup_en` 中**必须**有且仅有一条翻译——编译器强制完整性。
//!
//! ## 命名约定
//! - `Common*` —— 各面板共用的通用动作按钮 / 状态词（Delete / Cancel / OK / ...）。
//! - `TopBar*` —— 顶栏三区（ADR-0016）；新增一律先放本段。
//! - `Welcome*` —— 无工程欢迎页（`ui/welcome_screen.rs`，2026-10-15 eb8151c 后）。
//! - `NewProjectDialog*` / `NewMapDialog*` —— 两个对话框。
//! - `Panel<Name>*` —— 各面板专用；`<Name>` 与文件同名。
//! - `Render*` —— 渲染层（drill / terrain_view / streets / ...）叠加文字。
//! - `Cli*` —— `main.rs` / `overture_cli.rs` 的用户面向输出。
//!
//! 新增键一律就近归入合适段；若当下无合适段，建临时 `Common*` 或 `Placeholder*`
//! 段，后续重构时归并。

/// UI 文案的稳定键。
///
/// **重要**：`lookup_zh` / `lookup_en`（`table.rs`）必须为每个变体出匹配 arm，
/// 漏写一处即编译失败。这是 i18n 模块「无半翻译」契约的唯一强制点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum I18nKey {
    // ── 公共动作 ────────────────────────────────────────────────
    /// 删除（在按钮文案 / 菜单项复用）
    CommonDelete,
    /// 取消
    CommonCancel,
    /// 确定
    CommonOk,
    /// 确认
    CommonConfirm,
    /// 保存
    CommonSave,
    /// 打开
    CommonOpen,
    /// 关闭
    CommonClose,
    /// 应用（确认按钮变体）
    CommonApply,
    /// 返回
    CommonBack,
    /// 新建
    CommonNew,
    /// 「未知」通用占位（在 ID 解析失败 / 空 id 时显示）
    CommonUnknown,
    /// 「删除整个」前缀（「删除整个 {}」→「删除整个张献忠」之类）
    CommonDeleteEntire,

    // ── 顶栏（ADR-0016 三区结构） ────────────────────────────────
    /// 「未保存」
    TopBarUnsaved,
    /// 「已保存」
    TopBarSaved,
    /// 「●」脏标点
    TopBarUnsavedDot,
    /// 「未保存」状态条的副标题（若顶栏三区日后扩展）
    TopBarUnsavedSubtitle,
    /// 「强制重画地形（F5）」图标按钮 tooltip
    TopBarForceRedrawTooltip,
    /// 「工程未加载」红色 badge
    TopBarNoProjectLoaded,

    // ── 顶栏右侧计数（拆为不带占位符的纯片段，按 ADR-0018 用户裁定
    //    「调用点负责 format!」）────────────────────────────────
    /// 顶栏右侧计数「Unit」标签（中英都沿用英文术语「Unit」）
    TopBarCountUnits,
    /// 顶栏右侧计数「人物」 / 「Person」 标签
    TopBarCountCharacters,
    /// 顶栏右侧计数「事态」 / 「Situation」 标签
    TopBarCountSituations,
    /// 顶栏右侧计数「事件」 / 「Event」 标签
    TopBarCountEvents,
    /// 「已选：」当前地图标签前缀
    TopBarSelectedMapPrefix,
    /// 「缩放 」缩放标签前缀
    TopBarZoomPrefix,

    // ── 欢迎页 ──────────────────────────────────────────────────
    /// 欢迎页大标题（无工程锁）
    WelcomeHeading,
    /// 欢迎页副标题（一句话描述）
    WelcomeTagline,
    /// 欢迎页提示（`还没有打开的工程…`）
    WelcomeHelpText,
    /// 欢迎页「打开工程」按钮
    WelcomeOpenProjectBtn,
    /// 欢迎页「新建工程」按钮
    WelcomeNewProjectBtn,
    /// 欢迎页最近工程段标题
    WelcomeRecentHeading,
    /// 欢迎页最近工程为空时提示
    WelcomeRecentEmpty,

    // ── 新建工程对话框 ──────────────────────────────────────────
    /// 对话框标题
    NewProjectDialogTitle,
    /// 「工程目录」字段标签
    NewProjectDialogDirLabel,
    /// 「工程名称」字段标签
    NewProjectDialogNameLabel,
    /// 工程名称输入 hover 提示
    NewProjectDialogNameHover,
    /// 工程名称输入 placeholder
    NewProjectDialogNameHint,
    /// 位置字段标签
    NewProjectDialogLocationLabel,
    /// 位置输入 placeholder
    NewProjectDialogLocationHint,
    /// 选择目录按钮
    NewProjectDialogBrowseDir,
    /// 「创建」按钮
    NewProjectDialogCreateBtn,
    /// 路径预览前缀「项目将在 」
    NewProjectDialogPreviewForPrefix,
    /// 路径预览后缀「 创建目录」
    NewProjectDialogPreviewForSuffix,
    /// 路径预览空态（先填名称、选位置）
    NewProjectDialogPreviewEmpty,
    /// 默认工程名称（新建对话框打开时的初值）
    NewProjectDialogDefaultName,
    /// 首图默认显示名
    NewProjectDialogDefaultFirstMapName,
    /// 「取消」按钮（与 CommonCancel 重名复用）
    /// 留作未来区分占位：目前复用 CommonCancel

    // ── 新建地图对话框 ──────────────────────────────────────────
    /// 对话框标题
    NewMapDialogTitle,
    /// 「地图 ID」字段标签
    NewMapDialogIdLabel,
    /// 「地图名称」字段标签
    NewMapDialogNameLabel,
    /// 名称输入 hover 提示
    NewMapDialogNameHover,
    /// 名称输入 placeholder
    NewMapDialogNameHint,
    /// 「层级」字段标签
    NewMapDialogLevelLabel,
    /// 「幅面」字段标签
    NewMapDialogExtentLabel,
    /// 「预填充」字段标签
    NewMapDialogPrefillLabel,
    /// 默认地图名（对话框打开时的初值）
    NewMapDialogDefaultName,
    /// 默认首图地图名（空世界时）
    NewMapDialogFirstMapDefaultName,
    /// 首图会作为世界地图（钻取树的根）
    NewMapDialogFirstMapInfo,
    /// 当前没有选中地图时提示
    NewMapDialogNoParentInfo,
    /// 栅格信息前缀「栅格 」
    NewMapDialogGridInfoPrefix,
    /// 栅格信息中「×」
    NewMapDialogGridInfoMid,
    /// 栅格信息后缀「（每格 X wu）」
    NewMapDialogGridInfoSuffix,
    /// 「挂在「」之下」前缀
    NewMapDialogParentUnderPrefix,
    /// 「挂在「X」之下」后缀
    NewMapDialogParentUnderSuffix,
    /// 预填充：海洋
    NewMapDialogPrefillOcean,
    /// 预填充：陆地
    NewMapDialogPrefillLand,
    /// 首图级别 hover
    NewMapDialogLevelFirstMapHover,
    /// 预填充 hover
    NewMapDialogPrefillHover,
    /// 「已创建地图「」前缀
    NewMapDialogCreatedPrefix,
    /// 「已创建地图「X」」后缀
    NewMapDialogCreatedSuffix,
    /// 「创建失败：」前缀
    NewMapDialogErrCreateFailPrefix,
    /// 「创建」按钮
    NewMapDialogCreateBtn,

    // ── 笔刷面板（ADR-0010 / panel_brush） ────────────────────────
    /// 「笔刷模式（关闭时左键拖动地图）」
    BrushModeToggle,
    /// 「显示网格」开关
    BrushShowGrid,
    /// 「地形」分块小标题
    BrushTerrainHeader,
    /// 半径滑条格式前缀「半径 」
    BrushRadiusPrefix,
    /// 半径单位「wu」（中英同字，留作 Key 备用）
    BrushRadiusUnitWu,
    /// 衰减滑条格式前缀「衰减软度 」
    BrushSoftnessPrefix,
    /// 街道分块折叠头
    BrushStreetsHeader,
    /// 「画街道模式（左键逐点连线，右键结束）」
    BrushStreetsToggle,
    /// 「类别」下拉标签
    BrushClassLabel,
    /// 「街道名」字段标签
    BrushStreetNameLabel,
    /// 「已画 {} 段」前缀
    BrushStreetsDrawnPrefix,
    /// 「段」单位
    BrushStreetsDrawnUnit,
    /// 「结束连线」按钮
    BrushFinishLineBtn,
    /// 「撤销上一段」按钮
    BrushUndoLastSegmentBtn,
    /// 「Ctrl+滚轮改半径」底部提示
    BrushHintRadiusShortcut,

    // ── 视图模式（state::ViewMode） ──────────────────────────────────
    /// 「矢量地形」顶栏视图段（与 BROWSING 一致）
    ViewModeVectorFlat,
    /// 「地图」GoogleMap
    ViewModeGoogleMap,
    /// 「真实地形」RealTerrain
    ViewModeRealTerrain,
    /// 「色块地形」Schematic
    ViewModeSchematic,
    /// 「势力图」Faction
    ViewModeFaction,
    /// 「事件图」Event
    ViewModeEvent,
    /// 「轮廓图」Coastline
    ViewModeCoastline,

    // ── 图层面板（state::Panel::title） ─────────────────────────────
    /// 「地图树」面板标题
    PanelMapTree,
    /// 「图层管理」面板标题
    PanelLayers,
    /// 「地形设计器」面板标题
    PanelBrush,
    /// 「属性检查器」面板标题
    PanelInspector,
    /// 「时间轴」面板标题
    PanelTimeline,
    /// 「势力编辑」面板标题
    PanelFaction,
    /// 「区域编辑」面板标题
    PanelRegion,
    /// 「事态登记」面板标题
    PanelSituation,
    /// 「校验面板」面板标题
    PanelValidation,
    /// 「小地图」面板标题
    PanelMinimap,
    /// 「指南针」面板标题
    PanelCompass,
    /// 「人物登记」面板标题
    PanelCharacter,
    /// 「事件登记」面板标题
    PanelEvent,

    // ── 地图层级（state::map_level_label） ──────────────────────────
    /// 「世界」MapLevel
    MapLevelWorld,
    /// 「区域」MapLevel
    MapLevelRegion,
    /// 「城市」MapLevel
    MapLevelCity,
    /// 「建筑内」MapLevel
    MapLevelInterior,

    // ── 命令面板分组（ui_command_palette::CommandGroup） ─────────────
    /// 「项目」命令分组
    CommandGroupProject,
    /// 「视图」命令分组
    CommandGroupView,
    /// 「编辑」命令分组
    CommandGroupEdit,
    /// 「面板」命令分组
    CommandGroupPanel,
    /// 「时间」命令分组
    CommandGroupTime,
    /// 「界面」命令分组
    CommandGroupInterface,
    /// 「对象」命令分组
    CommandGroupObject,
    /// 「导航」命令分组
    CommandGroupNavigation,

    // ── 时间轴层（ui::timeline::TimelineLayer） ─────────────────────
    /// 「日」TimelineLayer
    TimelineLayerDay,
    /// 「事态」TimelineLayer
    TimelineLayerSituation,
    /// 「人物」TimelineLayer
    TimelineLayerCharacter,
    /// 「世纪」TimelineLayer
    TimelineLayerCentury,

    // ── 图层可见性（W2：layers_panel / minimap_panel 共享） ───────────
    /// 「地形」图层开关
    LayerTerrain,
    /// 「底图淡化」图层开关
    LayerBaseFading,
    /// 「地点」图层开关（layers_panel/minimap 共用；minimap 用 "Place"，layers_panel 用 "Place" 一致）
    LayerPlace,
    /// 「事件点」图层开关
    LayerEventPoint,
    /// 「轨迹」图层开关
    LayerTrail,
    /// 「道路」图层开关
    LayerRoad,
    /// 「建筑」图层开关
    LayerBuilding,
    /// 「势力」图层开关
    LayerFaction,
    /// 图层面板标题「图层」
    LayerPanelHeader,
    /// 世界地图（layers_panel 行项）
    LayerWorldMapLabel,

    // ── 地形种类（TERRAIN_NAMES：实际 10 种，写在 `crate::ui::shell::TERRAIN_NAMES`） ──
    /// 「深水」地形
    TerrainKindDeepWater,
    /// 「浅水」地形
    TerrainKindShallowWater,
    /// 「沙滩」地形
    TerrainKindBeach,
    /// 「草原」地形
    TerrainKindGrassland,
    /// 「林地」地形
    TerrainKindForest,
    /// 「丘陵」地形
    TerrainKindHill,
    /// 「山地」地形
    TerrainKindMountain,
    /// 「雪原」地形
    TerrainKindSnow,
    /// 「沼泽」地形
    TerrainKindSwamp,
    /// 「道路」地形
    TerrainKindRoad,

    // ── 项目菜单（project_menu） ─────────────────────────────────────
    /// 「（未命名工程）」
    ProjectMenuUnnamedProject,
    /// 「最近工程」分组
    ProjectMenuRecentSection,
    /// 「打开最近工程 · 」前缀（与 name 拼接）
    ProjectMenuRecentOpenPrefix,
    /// 「新建工程…」按钮
    ProjectMenuNewProject,
    /// 「新建地图…」按钮
    ProjectMenuNewMap,
    /// 「打开工程…」按钮
    ProjectMenuOpenProject,
    /// 「另存为…」按钮
    ProjectMenuSaveProjectAs,
    /// 「导出…」按钮
    ProjectMenuExportProject,
    /// 「关闭工程」按钮
    ProjectMenuCloseProject,
    /// 「项目设置」按钮
    ProjectMenuProjectSettings,
    /// 「工程」分组
    ProjectMenuProjectSection,
    /// 「面板」分组
    ProjectMenuPanelsSection,
    /// 「重置所有面板位置」按钮
    ProjectMenuResetPanels,

    // ── 时段 / 区间（公共：第 N 天、跨越时间段） ──────────────────────
    /// 「第 」时段前缀
    SpanDayPrefix,
    /// 「 天」时段后缀
    SpanDaySuffix,
    /// 「–」起止连接符
    SpanDayRangeMid,
    /// 「 天 –」开放区间后缀（结束时间未填）
    SpanDayOpenSuffix,

    // ── 事态面板 ─────────────────────────────────────────────────────
    /// 暂无事态空态标题
    SituationPanelEmptyTitle,
    /// 暂无事态空态提示
    SituationPanelEmptyHint,
    /// 暂无事态空态动作
    SituationPanelEmptyAction,

    // ── 区域面板 ─────────────────────────────────────────────────────
    /// 「+ 新建区域」按钮
    RegionPanelAddBtn,
    /// 默认新建区域名
    RegionPanelDefaultName,
    /// 「当前地图：」前缀
    RegionPanelCurrentMapPrefix,
    /// 尚无区域空态标题
    RegionPanelEmptyTitle,
    /// 尚无区域空态提示
    RegionPanelEmptyHint,
    /// 尚无区域空态动作
    RegionPanelEmptyAction,
    /// 「名称：」字段标签
    RegionPanelNameLabel,
    /// 「轮廓：」前缀
    RegionPanelOutlinePrefix,
    /// 「 个顶点」后缀
    RegionPanelOutlineSuffix,
    /// 「（未绘制）」占位
    RegionPanelOutlinePlaceholder,
    /// 「删除区域」按钮
    RegionPanelDeleteBtn,

    // ── 势力面板 ─────────────────────────────────────────────────────
    /// 「+ 新建势力」按钮
    FactionPanelAddBtn,
    /// 「+ 新建疆界」按钮
    FactionPanelAddBoundaryBtn,
    /// 默认新建势力名
    FactionPanelDefaultName,
    /// 尚无势力空态标题
    FactionPanelEmptyTitle,
    /// 尚无势力空态提示
    FactionPanelEmptyHint,
    /// 尚无势力空态动作
    FactionPanelEmptyAction,
    /// 「当前地图：」前缀
    FactionPanelCurrentMapPrefix,
    /// 「当前地图几何：」前缀
    FactionPanelCurrentMapGeomPrefix,
    /// 「名称：」字段标签
    FactionPanelFieldNameLabel,
    /// 「颜色：」字段标签
    FactionPanelFieldColorLabel,
    /// 「起 day：」字段标签
    FactionPanelFieldStartDayLabel,
    /// 「止（留空=至今）：」字段标签
    FactionPanelFieldEndDayLabel,
    /// 「删除」单段按钮
    FactionPanelDeleteBoundaryBtn,
    /// 「删除整个势力」按钮
    FactionPanelDeleteEntireBtn,
    /// 「 顶点」后缀（无前缀）
    FactionPanelPointCountSuffix,
    /// 「（无几何）」占位
    FactionPanelNoGeometry,

    // ── 检视器面板 ───────────────────────────────────────────────────
    /// 未选中对象空态标题
    InspectorEmptyTitle,
    /// 未选中对象空态提示
    InspectorEmptyHint,
    /// 未选中对象空态动作
    InspectorEmptyAction,

    // ── 时间轴面板（剩余字面量） ─────────────────────────────────────
    /// 「天/秒」速度单位后缀（format!("{s:.0}") + suffix 拼接）
    TimelineSpeedPillSuffix,
    /// 「＋ 登记事件」按钮
    TimelineRegisterEventBtn,

    // ── 校验面板 ─────────────────────────────────────────────────────
    /// 「错误 N」徽标前缀（数字前缀拼接）
    ValidationBadgeErrCountPrefix,
    /// 「提示 N」徽标前缀
    ValidationBadgeWarnCountPrefix,
    /// 「无错误」徽标
    ValidationBadgeErrNone,
    /// 「四维校验：时间 / 引用 / 几何 / 一致性」副标题
    ValidationSubtitle,
    /// 「校验只提示，不阻止保存（AGENTS §7.5）」脚注
    ValidationFootnote,
    /// 「错误」段标题
    ValidationSectionErrors,
    /// 「提示」段标题
    ValidationSectionHints,
    /// 「一切正常」空态
    ValidationOkAll,

    // ── 人物面板 ─────────────────────────────────────────────────────
    /// 无人物空态标题
    CharacterPanelEmptyTitle,
    /// 无人物空态提示
    CharacterPanelEmptyHint,
    /// 无人物空态动作
    CharacterPanelEmptyAction,

    // ── 事件面板 ─────────────────────────────────────────────────────
    /// 无事件空态标题
    EventPanelEmptyTitle,
    /// 无事件空态提示
    EventPanelEmptyHint,
    /// 无事件空态动作
    EventPanelEmptyAction,

    // ── 地图控制面板 ─────────────────────────────────────────────────
    /// 工具按钮 hover：切换工具（当前：笔刷 / 拖动）
    MapControlsToolSwitchHint,
    /// 测量：折线
    MapControlsMeasurePolylineHint,
    /// 测量：矩形
    MapControlsMeasureRectHint,
    /// 测量：橡皮
    MapControlsMeasureEraserHint,
    /// 「放大」
    MapControlsZoomIn,
    /// 「缩小」
    MapControlsZoomOut,
    /// 「缩放到全图」
    MapControlsFitMap,
}

impl I18nKey {
    /// 所有键的稳定数组，供测试遍历。
    pub const ALL: [I18nKey; 214] = [
        // 公共动作（12）
        I18nKey::CommonDelete,
        I18nKey::CommonCancel,
        I18nKey::CommonOk,
        I18nKey::CommonConfirm,
        I18nKey::CommonSave,
        I18nKey::CommonOpen,
        I18nKey::CommonClose,
        I18nKey::CommonApply,
        I18nKey::CommonBack,
        I18nKey::CommonNew,
        I18nKey::CommonUnknown,
        I18nKey::CommonDeleteEntire,
        // 顶栏（6）
        I18nKey::TopBarUnsaved,
        I18nKey::TopBarSaved,
        I18nKey::TopBarUnsavedDot,
        I18nKey::TopBarUnsavedSubtitle,
        I18nKey::TopBarForceRedrawTooltip,
        I18nKey::TopBarNoProjectLoaded,
        // 顶栏右侧计数（拆为不带占位符的纯片段，6）
        I18nKey::TopBarCountUnits,
        I18nKey::TopBarCountCharacters,
        I18nKey::TopBarCountSituations,
        I18nKey::TopBarCountEvents,
        I18nKey::TopBarSelectedMapPrefix,
        I18nKey::TopBarZoomPrefix,
        // 欢迎页（7）
        I18nKey::WelcomeHeading,
        I18nKey::WelcomeTagline,
        I18nKey::WelcomeHelpText,
        I18nKey::WelcomeOpenProjectBtn,
        I18nKey::WelcomeNewProjectBtn,
        I18nKey::WelcomeRecentHeading,
        I18nKey::WelcomeRecentEmpty,
        // 新建工程对话框（14）
        I18nKey::NewProjectDialogTitle,
        I18nKey::NewProjectDialogDirLabel,
        I18nKey::NewProjectDialogNameLabel,
        I18nKey::NewProjectDialogNameHover,
        I18nKey::NewProjectDialogNameHint,
        I18nKey::NewProjectDialogLocationLabel,
        I18nKey::NewProjectDialogLocationHint,
        I18nKey::NewProjectDialogBrowseDir,
        I18nKey::NewProjectDialogCreateBtn,
        I18nKey::NewProjectDialogPreviewForPrefix,
        I18nKey::NewProjectDialogPreviewForSuffix,
        I18nKey::NewProjectDialogPreviewEmpty,
        I18nKey::NewProjectDialogDefaultName,
        I18nKey::NewProjectDialogDefaultFirstMapName,
        // 新建地图对话框（23）
        I18nKey::NewMapDialogTitle,
        I18nKey::NewMapDialogIdLabel,
        I18nKey::NewMapDialogNameLabel,
        I18nKey::NewMapDialogNameHover,
        I18nKey::NewMapDialogNameHint,
        I18nKey::NewMapDialogLevelLabel,
        I18nKey::NewMapDialogExtentLabel,
        I18nKey::NewMapDialogPrefillLabel,
        I18nKey::NewMapDialogDefaultName,
        I18nKey::NewMapDialogFirstMapDefaultName,
        I18nKey::NewMapDialogFirstMapInfo,
        I18nKey::NewMapDialogNoParentInfo,
        I18nKey::NewMapDialogGridInfoPrefix,
        I18nKey::NewMapDialogGridInfoMid,
        I18nKey::NewMapDialogGridInfoSuffix,
        I18nKey::NewMapDialogParentUnderPrefix,
        I18nKey::NewMapDialogParentUnderSuffix,
        I18nKey::NewMapDialogPrefillOcean,
        I18nKey::NewMapDialogPrefillLand,
        I18nKey::NewMapDialogLevelFirstMapHover,
        I18nKey::NewMapDialogPrefillHover,
        I18nKey::NewMapDialogCreatedPrefix,
        I18nKey::NewMapDialogCreatedSuffix,
        I18nKey::NewMapDialogErrCreateFailPrefix,
        I18nKey::NewMapDialogCreateBtn,
        // 笔刷面板（14）
        I18nKey::BrushModeToggle,
        I18nKey::BrushShowGrid,
        I18nKey::BrushTerrainHeader,
        I18nKey::BrushRadiusPrefix,
        I18nKey::BrushRadiusUnitWu,
        I18nKey::BrushSoftnessPrefix,
        I18nKey::BrushStreetsHeader,
        I18nKey::BrushStreetsToggle,
        I18nKey::BrushClassLabel,
        I18nKey::BrushStreetNameLabel,
        I18nKey::BrushStreetsDrawnPrefix,
        I18nKey::BrushStreetsDrawnUnit,
        I18nKey::BrushFinishLineBtn,
        I18nKey::BrushUndoLastSegmentBtn,
        I18nKey::BrushHintRadiusShortcut,
        // 视图模式（7）
        I18nKey::ViewModeVectorFlat,
        I18nKey::ViewModeGoogleMap,
        I18nKey::ViewModeRealTerrain,
        I18nKey::ViewModeSchematic,
        I18nKey::ViewModeFaction,
        I18nKey::ViewModeEvent,
        I18nKey::ViewModeCoastline,
        // 图层面板（13）
        I18nKey::PanelMapTree,
        I18nKey::PanelLayers,
        I18nKey::PanelBrush,
        I18nKey::PanelInspector,
        I18nKey::PanelTimeline,
        I18nKey::PanelFaction,
        I18nKey::PanelRegion,
        I18nKey::PanelSituation,
        I18nKey::PanelValidation,
        I18nKey::PanelMinimap,
        I18nKey::PanelCompass,
        I18nKey::PanelCharacter,
        I18nKey::PanelEvent,
        // 地图层级（4）
        I18nKey::MapLevelWorld,
        I18nKey::MapLevelRegion,
        I18nKey::MapLevelCity,
        I18nKey::MapLevelInterior,
        // 命令面板分组（8）
        I18nKey::CommandGroupProject,
        I18nKey::CommandGroupView,
        I18nKey::CommandGroupEdit,
        I18nKey::CommandGroupPanel,
        I18nKey::CommandGroupTime,
        I18nKey::CommandGroupInterface,
        I18nKey::CommandGroupObject,
        I18nKey::CommandGroupNavigation,
        // 时间轴层（4）
        I18nKey::TimelineLayerDay,
        I18nKey::TimelineLayerSituation,
        I18nKey::TimelineLayerCharacter,
        I18nKey::TimelineLayerCentury,
        // 图层可见性（10）
        I18nKey::LayerTerrain,
        I18nKey::LayerBaseFading,
        I18nKey::LayerPlace,
        I18nKey::LayerEventPoint,
        I18nKey::LayerTrail,
        I18nKey::LayerRoad,
        I18nKey::LayerBuilding,
        I18nKey::LayerFaction,
        I18nKey::LayerPanelHeader,
        I18nKey::LayerWorldMapLabel,
        // 地形种类（10）
        I18nKey::TerrainKindDeepWater,
        I18nKey::TerrainKindShallowWater,
        I18nKey::TerrainKindBeach,
        I18nKey::TerrainKindGrassland,
        I18nKey::TerrainKindForest,
        I18nKey::TerrainKindHill,
        I18nKey::TerrainKindMountain,
        I18nKey::TerrainKindSnow,
        I18nKey::TerrainKindSwamp,
        I18nKey::TerrainKindRoad,
        // 项目菜单（13）
        I18nKey::ProjectMenuUnnamedProject,
        I18nKey::ProjectMenuRecentSection,
        I18nKey::ProjectMenuRecentOpenPrefix,
        I18nKey::ProjectMenuNewProject,
        I18nKey::ProjectMenuNewMap,
        I18nKey::ProjectMenuOpenProject,
        I18nKey::ProjectMenuSaveProjectAs,
        I18nKey::ProjectMenuExportProject,
        I18nKey::ProjectMenuCloseProject,
        I18nKey::ProjectMenuProjectSettings,
        I18nKey::ProjectMenuProjectSection,
        I18nKey::ProjectMenuPanelsSection,
        I18nKey::ProjectMenuResetPanels,
        // 区间（4）
        I18nKey::SpanDayPrefix,
        I18nKey::SpanDaySuffix,
        I18nKey::SpanDayRangeMid,
        I18nKey::SpanDayOpenSuffix,
        // 事态（3）
        I18nKey::SituationPanelEmptyTitle,
        I18nKey::SituationPanelEmptyHint,
        I18nKey::SituationPanelEmptyAction,
        // 区域（11）
        I18nKey::RegionPanelAddBtn,
        I18nKey::RegionPanelDefaultName,
        I18nKey::RegionPanelCurrentMapPrefix,
        I18nKey::RegionPanelEmptyTitle,
        I18nKey::RegionPanelEmptyHint,
        I18nKey::RegionPanelEmptyAction,
        I18nKey::RegionPanelNameLabel,
        I18nKey::RegionPanelOutlinePrefix,
        I18nKey::RegionPanelOutlineSuffix,
        I18nKey::RegionPanelOutlinePlaceholder,
        I18nKey::RegionPanelDeleteBtn,
        // 势力（15）
        I18nKey::FactionPanelAddBtn,
        I18nKey::FactionPanelAddBoundaryBtn,
        I18nKey::FactionPanelDefaultName,
        I18nKey::FactionPanelEmptyTitle,
        I18nKey::FactionPanelEmptyHint,
        I18nKey::FactionPanelEmptyAction,
        I18nKey::FactionPanelCurrentMapPrefix,
        I18nKey::FactionPanelCurrentMapGeomPrefix,
        I18nKey::FactionPanelFieldNameLabel,
        I18nKey::FactionPanelFieldColorLabel,
        I18nKey::FactionPanelFieldStartDayLabel,
        I18nKey::FactionPanelFieldEndDayLabel,
        I18nKey::FactionPanelDeleteBoundaryBtn,
        I18nKey::FactionPanelDeleteEntireBtn,
        I18nKey::FactionPanelPointCountSuffix,
        I18nKey::FactionPanelNoGeometry,
        // 检视器（3）
        I18nKey::InspectorEmptyTitle,
        I18nKey::InspectorEmptyHint,
        I18nKey::InspectorEmptyAction,
        // 时间轴（2）
        I18nKey::TimelineSpeedPillSuffix,
        I18nKey::TimelineRegisterEventBtn,
        // 校验（8）
        I18nKey::ValidationBadgeErrCountPrefix,
        I18nKey::ValidationBadgeWarnCountPrefix,
        I18nKey::ValidationBadgeErrNone,
        I18nKey::ValidationSubtitle,
        I18nKey::ValidationFootnote,
        I18nKey::ValidationSectionErrors,
        I18nKey::ValidationSectionHints,
        I18nKey::ValidationOkAll,
        // 人物（3）
        I18nKey::CharacterPanelEmptyTitle,
        I18nKey::CharacterPanelEmptyHint,
        I18nKey::CharacterPanelEmptyAction,
        // 事件（3）
        I18nKey::EventPanelEmptyTitle,
        I18nKey::EventPanelEmptyHint,
        I18nKey::EventPanelEmptyAction,
        // 地图控制（7）
        I18nKey::MapControlsToolSwitchHint,
        I18nKey::MapControlsMeasurePolylineHint,
        I18nKey::MapControlsMeasureRectHint,
        I18nKey::MapControlsMeasureEraserHint,
        I18nKey::MapControlsZoomIn,
        I18nKey::MapControlsZoomOut,
        I18nKey::MapControlsFitMap,
    ];
}
