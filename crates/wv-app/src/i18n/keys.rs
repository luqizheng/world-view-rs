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
    /// 分段控件上的「笔刷」短标（点按切换笔刷总开/关）
    BrushModeShort,
    /// 工具行首组标签「涂」（分配式涂抹，2026-10-16 三行行内布局）
    BrushGroupPaint,
    /// 「塑」（拖拽骨架、程序生成）
    BrushGroupShape,
    /// 「放」（单击种子、扩散成形）
    BrushGroupPlace,
    /// 工具名「涂抹」（瓷砖 tooltip + 命令面板共用）
    BrushToolNamePaint,
    /// 「山脉」
    BrushToolNameRange,
    /// 「高原」
    BrushToolNamePlateau,
    /// 「丘陵」
    BrushToolNameHills,
    /// 「火山」
    BrushToolNameVolcano,
    /// 「湖泊」
    BrushToolNameLake,
    /// 「河流」
    BrushToolNameRiver,
    /// 涂抹工具交互提示（瓷砖 tooltip 第二行 + 面板提示行）
    BrushHintPaint,
    /// 山脉工具交互提示
    BrushHintRange,
    /// 高原工具交互提示
    BrushHintPlateau,
    /// 丘陵工具交互提示
    BrushHintHills,
    /// 火山工具交互提示
    BrushHintVolcano,
    /// 湖泊工具交互提示
    BrushHintLake,
    /// 河流工具交互提示
    BrushHintRiver,
    /// 「地形材料」段标题（旁附当前选中材质名）
    BrushMaterialHeader,
    /// 「半径」滑条标签
    BrushRadiusLabel,
    /// 「软度」滑条标签
    BrushSoftnessLabel,
    /// 「强度」滑条标签
    BrushStrengthLabel,
    /// 「画路」折叠段标题
    BrushRoadDrawingHeader,

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

    // ── 事实地界（theme overlay v1 / 选择工具 M4） ──────────────────
    /// 意图菜单「标记…」按钮（ADR-0029：原「森林…」升级为全群系标记）
    IntentMenuMarker,
    /// 标记对话框「生物群系」标签
    MarkerBiomeLabel,
    /// 群系名：森林
    MarkerBiomeForest,
    /// 群系名：平原
    MarkerBiomePlains,
    /// 群系名：草原
    MarkerBiomeGrassland,
    /// 群系名：戈壁
    MarkerBiomeGobi,
    /// 群系名：沙漠
    MarkerBiomeDesert,
    /// 群系名：沼泽
    MarkerBiomeSwamp,
    /// 群系名：湿地
    MarkerBiomeWetland,
    /// 群系名：苔原
    MarkerBiomeTundra,
    /// 群系名：雪原
    MarkerBiomeSnowfield,
    /// 群系名：丛林
    MarkerBiomeJungle,
    /// 群系名：热带雨林
    MarkerBiomeTropicalRainforest,
    /// 群系名：丘陵
    MarkerBiomeHills,
    /// 群系名：山地
    MarkerBiomeMountains,
    /// 群系名：绿洲
    MarkerBiomeOasis,
    /// 群系名：稀树草原
    MarkerBiomeSavanna,
    /// 群系名：盐碱地
    MarkerBiomeSaltFlat,
    /// 群系名：湖泊
    MarkerBiomeLake,
    /// 「新建地理事实」命名对话框标题
    FeaturePlaceDialogTitle,
    /// 命名对话框的「名称」标签
    FeaturePlaceNameLabel,
    /// 命名对话框的「创建」按钮
    FeaturePlaceCreate,

    // ── 势力划分（ADR-0028 / 势力交互 M1） ─────────────────────────
    /// 选区意图菜单「划分势力」按钮
    IntentMenuDivideFaction,
    /// 「划分势力」对话框标题
    FactionDivisionDialogTitle,
    /// 对话框「名称」标签
    DivisionNameLabel,
    /// 对话框「宿主」标签
    DivisionHostLabel,
    /// 无宿主（空势力陆地）
    DivisionHostNone,
    /// 对话框「边界」标签
    DivisionModeLabel,
    /// 边界模式：贴地（沿河流/山脊/湖岸）
    DivisionModeSnap,
    /// 边界模式：越地（无视地貌）
    DivisionModeStraight,
    /// 对话框「创建」按钮
    DivisionCreate,
    /// 成功回执（有宿主）：已从 {host} 划分出「{name}」
    DivisionNoticeCreatedFromHost,
    /// 成功回执（空势力）：已创建势力「{name}」
    DivisionNoticeCreated,
    /// 贴地退化提示：窗口内没找到地物，边界按直线生成
    DivisionNoticeSnapDegraded,
    /// 重叠提示（空势力路径）：该范围与既有势力重叠
    DivisionNoticeOverlap,

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
    /// 「生物群系标记」小节标题（区域面板）
    RegionPanelMarkerSection,
    /// 当前地图暂无标记的空态提示
    RegionPanelMarkerEmpty,
    /// 「重生成边界」按钮（标记行）
    RegionPanelMarkerRegenBtn,

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
    /// 「天/秒」速度单位后缀（单数档：format!("{s:.0}") + suffix 拼接）
    TimelineSpeedPillSuffix,
    /// 「天/秒」速度单位后缀（复数档，en 用 `days`；zh 与单数同值）
    TimelineSpeedPillSuffixPlural,
    /// 「+ Register event」按钮（加号由 `ICON_PLUS` 图标提供，文案不含加号）
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

    // ── W3：ui_shell 通知 toast 文本 ────────────────────────────────
    /// 「已保存到 」前缀（后接 dir）
    NoticeSavedToPrefix,
    /// 「（」前缀（已保存全量 / 部分）
    NoticeSavedScopePrefix,
    /// 「）」后缀
    NoticeSavedScopeSuffix,
    /// 「全量」作用域文案
    NoticeSavedScopeFull,
    /// 「{} 个目录」作用域文案（数字 + 字段）—— 拆为前缀 key + 单位 key
    NoticeSavedScopePartialCountPrefix,
    /// 「 个目录」单位
    NoticeSavedScopePartialCountSuffix,
    /// 「保存失败：」前缀（后接错误体）
    NoticeSaveFailedPrefix,
    /// 「打开工程失败：」前缀
    NoticeOpenProjectFailedPrefix,
    /// 「打开工程目录失败：」前缀
    NoticeOpenProjectDirFailedPrefix,
    /// 「已加载工程「」前缀
    NoticeProjectLoadedPrefix,
    /// 「」」后缀
    NoticeProjectLoadedSuffix,
    /// 「未加载工程：在欢迎页新建或打开工程后即可开始绘制」整段
    NoticeNoProjectNeed,
    /// 「最近工程列表已变化，请重开菜单」整段
    NoticeRecentsChanged,
    /// 「已关闭工程」整段
    NoticeProjectClosed,
    /// 「导出尚未实现：需要先定导出格式（PNG / SVG），见 v0.2 §G」整段
    NoticeExportPending,
    /// 「项目设置面板待 v0.3」整段
    NoticeSettingsPendingV03,
    /// 「已在世界地图，无法再钻出」整段
    NoticeAlreadyWorldMap,
    /// 「该层级就是当前层级」整段
    NoticeAlreadyAtLevel,
    /// 「请先新建或打开工程，再执行该操作」整段
    NoticeNeedProjectFirst,
    /// 「已强制重画地形」整段
    NoticeForceRedrawn,
    /// 「当前没有地图，无法重置地形」整段（2026-10-16）
    NoticeResetTerrainNoMap,

    // ── W3：ui_time 日期格式（中文 zh / 英文 en 不同） ────────────────
    /// 时间无日历 zh prefix：「第 」
    TimeNoCalZhPrefix,
    /// 时间无日历 zh suffix：「 天」
    TimeNoCalZhSuffix,
    /// 时间无日历 en prefix：「Day 」
    TimeNoCalEnPrefix,
    /// 时间无日历 en suffix：空
    TimeNoCalEnSuffix,

    // ── W4：命令面板 41 个静态命令的 label（ADR-0018 §6.4） ──────────
    /// 「新建工程…」
    CmdNewProject,
    /// 「新建地图…」
    CmdNewMap,
    /// 「打开工程…」
    CmdOpenProject,
    /// 「另存为…」
    CmdSaveProjectAs,
    /// 「关闭工程」
    CmdCloseProject,
    /// 「导出…」
    CmdExportProject,
    /// 「项目设置」
    CmdProjectSettings,
    /// 「切换到矢量地形视图」（命令 = 动词，与 W1 ViewMode 标签区分）
    CmdSwitchVectorFlatView,
    /// 「切换到色块地形视图」
    CmdSwitchSchematicView,
    /// 「切换到势力图」（ADR-0027 D6）
    CmdSwitchFactionView,
    /// 「全览地图（视野归位）」
    CmdFitMap,
    /// 「放大地图」
    CmdZoomIn,
    /// 「缩小地图」
    CmdZoomOut,
    /// 「沉浸模式（只留地图与时间轴）」
    CmdToggleImmersive,
    /// 「打开命令面板」
    CmdOpenCommandPalette,
    /// 「保存工程」
    CmdSave,
    /// 「撤销」
    CmdUndo,
    /// 「重做」
    CmdRedo,
    /// 「显隐世界树面板」
    CmdTogglePanelMapTree,
    /// 「显隐图层面板」
    CmdTogglePanelLayers,
    /// 「显隐地形笔刷面板」
    CmdTogglePanelBrush,
    /// 「显隐小地图面板」
    CmdTogglePanelMinimap,
    /// 「显隐指南针面板」
    CmdTogglePanelCompass,
    /// 「显隐时间轴」
    CmdTogglePanelTimeline,
    /// 「显隐事件登记面板」
    CmdTogglePanelEvent,
    /// 「显隐事态登记面板」
    CmdTogglePanelSituation,
    /// 「显隐人物登记面板」
    CmdTogglePanelCharacter,
    /// 「显隐势力层面板」
    CmdTogglePanelFaction,
    /// 「显隐区域面板」
    CmdTogglePanelRegion,
    /// 「显隐检视器面板」
    CmdTogglePanelInspector,
    /// 「显隐校验面板」
    CmdTogglePanelValidation,
    /// 「重置所有面板位置」
    CmdResetPanelLayout,
    /// 「播放 / 暂停时间轴」
    CmdTogglePlay,
    /// 「跳到时间轴起点」
    CmdJumpToStart,
    /// 「跳到时间轴终点」
    CmdJumpToEnd,
    /// 「切换明暗主题」
    CmdToggleTheme,
    /// 「切换紧凑 / 标准密度」
    CmdToggleDensity,
    /// 「切换地图主题（日间古舆图 / 暮色旧纸）」
    CmdToggleMapPalette,
    /// 「强制重画地形（怀疑没跟上时点一下）」
    CmdForceRebuildTexture,
    /// 「重置当前图地形到底图」（2026-10-16）
    CmdResetTerrainToBase,
    /// 「增大笔刷半径」
    CmdBrushRadiusUp,
    /// 「减小笔刷半径」
    CmdBrushRadiusDown,
    /// 「笔刷：涂抹」——选中即激活笔刷
    CmdSelectBrushToolPaint,
    /// 「笔刷：山脉」
    CmdSelectBrushToolRange,
    /// 「笔刷：高原」
    CmdSelectBrushToolPlateau,
    /// 「笔刷：丘陵」
    CmdSelectBrushToolHills,
    /// 「笔刷：火山」
    CmdSelectBrushToolVolcano,
    /// 「笔刷：湖泊」
    CmdSelectBrushToolLake,
    /// 「笔刷：河流」
    CmdSelectBrushToolRiver,
    /// 「显示参照网格」（地图工具条瓷砖同源）
    CmdToggleGrid,
    /// 「钻回世界地图」
    CmdDrillToRoot,
    /// 「钻回上一级」
    CmdDrillUp,

    // ── W6：CLI 用户可见文案（main.rs + overture_cli.rs） ────────────────
    /// 「用法: wv load <工程目录>」
    CliUsageLoad,
    /// 「用法: wv validate <工程目录>」
    CliUsageValidate,
    /// 「错误: 」前缀（stderr 输出错误体）
    CliErrorPrefix,
    /// 「GUI 未编译。请使用 \`cargo run -p wv-app --features app -- gui\`」
    CliGuiNotCompiled,
    /// 「未知命令: 」前缀
    CliUnknownCommandPrefix,
    /// 「wv —— World View 世界编辑器」（help banner 头）
    CliHelpBanner,
    /// help banner「用法:」段头
    CliHelpUsageHeader,
    /// help banner「wv load <工程目录>」+ 描述「加载工程并打印摘要」
    CliHelpLoad,
    /// help banner「wv validate <工程目录>」+ 描述
    CliHelpValidate,
    /// help banner「wv raster-slice ...」+ 描述
    CliHelpRasterSlice,
    /// help banner「wv overture fetch ...」+ 描述
    CliHelpOvertureFetch,
    /// help banner「wv overture apply ...」+ 描述
    CliHelpOvertureApply,
    /// help banner「wv gui ...」+ 描述
    CliHelpGui,
    /// help banner「wv gallery」+ 描述
    CliHelpGallery,
    /// help banner「wv --help」+ 描述
    CliHelpHelp,
    /// 「用法: wv raster-slice <工程目录> <map_id> [size]」
    CliUsageRasterSlice,
    /// 「缺少 <map_id>。用法: wv raster-slice <工程目录> <map_id> [size]」
    CliUsageRasterSliceMapId,
    /// 「size {s} 不是正整数」
    CliRasterSliceBadSize,
    /// 「非法 map_id {map_id}: {e}」
    CliRasterSliceBadMapId,
    /// 「已切片: maps/{map_id}/terrain.bin（{size}×{size}）」
    CliRasterSliceDone,
    /// 「重新打开工程后，该图即使用自己的底图（继承退场）。」
    CliRasterSliceInherit,
    /// 「已加载工程：」前缀（cli load 摘要前）
    CliProjectSummaryHeader,
    /// 「提示:」段头
    CliProjectSummaryHints,
    /// 「  - 」警告项缩进前缀
    CliProjectSummaryHintBullet,
    /// 「校验通过: 」前缀
    CliValidateOk,
    /// 「校验发现 {n} 项问题:」前缀
    CliValidateFailPrefix,
    /// 「-m 需要一个工程目录参数，例如 \`wv gui -m examples/minimal-world\`」
    CliGuiErrMDirNeeded,
    /// 「工程目录重复指定」
    CliGuiErrDirDup,
    /// 「\`--http-port\` 需要一个 1..=65535 的整数，例如 \`--http-port 9000\`」
    CliGuiErrHttpPortNeeded,
    /// 「\`--http-port\` 重复指定」
    CliGuiErrHttpPortDup,
    /// 「\`--http-port {s}\` 不是 1..=65535 的整数」
    CliGuiErrHttpPortParse,
    /// 「\`--http-port\` 必须 ≥ 1」
    CliGuiErrHttpPortZero,
    /// 「\`--http-port\` 需要 \`--features http\` 才会生效」
    CliGuiErrHttpPortDisabled,
    /// 「\`--http-bind\` 需要一个 IPv4 地址，例如 \`--http-bind 0.0.0.0\`」
    CliGuiErrHttpBindNeeded,
    /// 「\`--http-bind\` 重复指定」
    CliGuiErrHttpBindDup,
    /// 「\`--http-bind {s}\` 不是合法的 IPv4:port 形态」
    CliGuiErrHttpBindParse,
    /// 「\`--http-bind\` 需要 \`--features http\` 才会生效」
    CliGuiErrHttpBindDisabled,
    /// 「未知参数: 」前缀
    CliGuiErrUnknownArg,
    /// 「用法: wv gui [-m <工程目录>] [...]」
    CliGuiUsage,
    /// 「--language <zh|en>」标记帮助段
    CliLanguageFlagHelp,
    /// 「\`--language {s}\` 不是合法语言（可选 zh / en）」
    CliLanguageBadTag,
    /// 「\`--language\` 重复指定」
    CliLanguageDup,
    /// 「\`--language\` 缺值」
    CliLanguageMissing,

    // ── W7：HTTP 用户可见 stderr（ADR-0018 Q1 carve-out 可见部分） ────
    /// 「[wv-app http] 绑定 {addr} 失败: {e}。…」整段
    HttpBindFailed,
    /// 「[wv-app http] MCP server 监听 {addr}（POST /mcp）」
    HttpListening,
    /// 「绑定地址解析失败: 」ServerError 前缀
    HttpAddrParseFailed,
    /// 「TCP 绑定失败: 」ServerError 前缀
    HttpTcpBindFailed,
    /// 「HTTP 服务运行失败: 」ServerError 前缀
    HttpServeFailed,
    /// 「[wv-app http] 启 tokio 失败: 」http_integration 前缀
    HttpTokioStartFailed,
    /// 「[wv-app http] server 退出: 」http_integration 前缀
    HttpServerCrashed,

    // ── 大陆生成属性面板（2026-10-17 大陆生成 UI plan Task 2） ──────────
    /// 面板标题「生成大陆」
    ContinentDialogTitle,
    /// 参数「种子」
    ContinentParamSeed,
    /// 参数「海岸蜿蜒度」
    ContinentParamCoastWiggle,
    /// 参数「海岸细节」
    ContinentParamCoastDetail,
    /// 参数「起伏强度」
    ContinentParamRelief,
    /// 参数「森林覆盖」
    ContinentParamForest,
    /// 参数「荒漠覆盖」
    ContinentParamDesert,
    /// 参数「大陆架宽度」
    ContinentParamShelf,
    /// 参数「离岸小岛」
    ContinentParamIslands,
    /// 按钮「生成」
    ContinentGenerate,
    /// 按钮「取消」
    ContinentCancel,
    /// 工具名「圈画大陆」
    ContinentToolName,
    /// 提示「已生成大陆」
    ContinentNoticeGenerated,
    /// 错误「没有当前地图」
    ContinentNoticeNoMap,
    /// 错误「没有圈出大陆范围」
    ContinentNoticeNoEnvelopes,
    /// 错误前缀「生成失败」
    ContinentNoticeGenerateFailed,

    // ── 选区意图菜单（2026-10-17 选择工具 spec §4，M1 Task 6） ──────────
    /// 按钮「生成大陆…」
    IntentMenuGenerateContinent,
    /// 按钮「生成城市…」
    IntentMenuGenerateCity,
    /// 按钮「森林…」
    IntentMenuForest,
    /// 按钮「高山」
    IntentMenuMountains,
    /// 按钮「丘陵」
    IntentMenuHills,
    /// 按钮「平原」
    IntentMenuPlains,
    /// 按钮「湖泊」
    IntentMenuLake,

    // ── 地形/城市意图通知（终审 Important 2，spec §8） ─────────────────
    /// 通知「该意图在选区内没有产生任何变化」
    IntentNoticeNoOp,
    /// 通知「已应用地形意图（Ctrl+Z 可撤销）」
    IntentNoticeApplied,
    /// 错误前缀「地形意图未生效」
    IntentNoticeFailed,
    /// 通知「新建城市」
    CityNoticeCreated,
    /// 错误「已建图但自动进入失败」
    CityNoticeDrillFailed,
    /// 错误前缀「新建城市未生效」
    CityNoticeFailed,
    /// 弱化提示「Esc 取消选择」
    IntentMenuCancelHint,
    /// 占位「本图暂无可用意图」
    IntentMenuNoIntents,

    // ── 生成城市对话框（2026-10-17 选择工具 spec §5，M2 Task 3） ─────────
    /// 标题「新建城市」
    CityDialogTitle,
    /// 字段「ID」
    CityDialogFieldId,
    /// 字段「名字」
    CityDialogFieldName,
    /// 字段「面积 (wu²)」
    CityDialogFieldArea,
    /// 字段「底图」
    CityDialogFieldBase,
    /// 选项「海洋」
    CityDialogBaseOcean,
    /// 选项「陆地」
    CityDialogBaseLand,
    /// 按钮「生成并进入」
    CityDialogOk,
    /// 按钮「取消」
    CityDialogCancel,

    // ── 新建地图幅面比例（2026-10-17 ocean-default-16-9 Task 2） ────────
    /// 标签「宽高比」
    NewMapAspect,
    /// 比例档「16:9」
    Aspect16x9,
    /// 比例档「4:3」
    Aspect4x3,
    /// 比例档「1:1」
    Aspect1x1,

    // ── 世界图尺度只读提示（README §12-39） ─────────────────────────────
    /// 「图幅 {w}×{h} km」
    ScaleHintExtent,
    /// 「陆地约 {w}×{h} km」
    ScaleHintLand,
    /// 「横穿约 步行{a}天 · 骑马{b}天 · 帆船{c}天」
    ScaleHintCrossing,
}

impl I18nKey {
    /// 所有键的稳定数组，供测试遍历。
    pub const ALL: [I18nKey; 414] = [
        // 公共动作（12）
        I18nKey::CommonDelete,
        I18nKey::CommonCancel,
        I18nKey::IntentMenuMarker,
        I18nKey::MarkerBiomeLabel,
        I18nKey::MarkerBiomeForest,
        I18nKey::MarkerBiomePlains,
        I18nKey::MarkerBiomeGrassland,
        I18nKey::MarkerBiomeGobi,
        I18nKey::MarkerBiomeDesert,
        I18nKey::MarkerBiomeSwamp,
        I18nKey::MarkerBiomeWetland,
        I18nKey::MarkerBiomeTundra,
        I18nKey::MarkerBiomeSnowfield,
        I18nKey::MarkerBiomeJungle,
        I18nKey::MarkerBiomeTropicalRainforest,
        I18nKey::MarkerBiomeHills,
        I18nKey::MarkerBiomeMountains,
        I18nKey::MarkerBiomeOasis,
        I18nKey::MarkerBiomeSavanna,
        I18nKey::MarkerBiomeSaltFlat,
        I18nKey::MarkerBiomeLake,
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
        // 笔刷面板 2026-10-16 改版（23）：模式短标 + 组标签 3 + 工具名 7 +
        // 提示 7 + 滑条标签 4 + 材料段标 1（画路头 / 半径 / 软度 / 强度）
        I18nKey::BrushModeShort,
        I18nKey::BrushGroupPaint,
        I18nKey::BrushGroupShape,
        I18nKey::BrushGroupPlace,
        I18nKey::BrushToolNamePaint,
        I18nKey::BrushToolNameRange,
        I18nKey::BrushToolNamePlateau,
        I18nKey::BrushToolNameHills,
        I18nKey::BrushToolNameVolcano,
        I18nKey::BrushToolNameLake,
        I18nKey::BrushToolNameRiver,
        I18nKey::BrushHintPaint,
        I18nKey::BrushHintRange,
        I18nKey::BrushHintPlateau,
        I18nKey::BrushHintHills,
        I18nKey::BrushHintVolcano,
        I18nKey::BrushHintLake,
        I18nKey::BrushHintRiver,
        I18nKey::BrushMaterialHeader,
        I18nKey::BrushRadiusLabel,
        I18nKey::BrushSoftnessLabel,
        I18nKey::BrushStrengthLabel,
        I18nKey::BrushRoadDrawingHeader,
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
        // 事实地界（theme overlay v1 / 选择工具 M4，3）
        I18nKey::FeaturePlaceDialogTitle,
        I18nKey::FeaturePlaceNameLabel,
        I18nKey::FeaturePlaceCreate,
        I18nKey::IntentMenuForest,
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
        // 区域（14）
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
        I18nKey::RegionPanelMarkerSection,
        I18nKey::RegionPanelMarkerEmpty,
        I18nKey::RegionPanelMarkerRegenBtn,
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
        // 时间轴（3）
        I18nKey::TimelineSpeedPillSuffix,
        I18nKey::TimelineSpeedPillSuffixPlural,
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
        // ui_shell 通知（16）
        I18nKey::NoticeSavedToPrefix,
        I18nKey::NoticeSavedScopePrefix,
        I18nKey::NoticeSavedScopeSuffix,
        I18nKey::NoticeSavedScopeFull,
        I18nKey::NoticeSavedScopePartialCountPrefix,
        I18nKey::NoticeSavedScopePartialCountSuffix,
        I18nKey::NoticeSaveFailedPrefix,
        I18nKey::NoticeOpenProjectFailedPrefix,
        I18nKey::NoticeOpenProjectDirFailedPrefix,
        I18nKey::NoticeProjectLoadedPrefix,
        I18nKey::NoticeProjectLoadedSuffix,
        I18nKey::NoticeNoProjectNeed,
        I18nKey::NoticeRecentsChanged,
        I18nKey::NoticeProjectClosed,
        I18nKey::NoticeExportPending,
        I18nKey::NoticeSettingsPendingV03,
        I18nKey::NoticeAlreadyWorldMap,
        I18nKey::NoticeAlreadyAtLevel,
        I18nKey::NoticeNeedProjectFirst,
        I18nKey::NoticeForceRedrawn,
        I18nKey::NoticeResetTerrainNoMap,
        // ui_time（4）
        I18nKey::TimeNoCalZhPrefix,
        I18nKey::TimeNoCalZhSuffix,
        I18nKey::TimeNoCalEnPrefix,
        I18nKey::TimeNoCalEnSuffix,
        // 命令面板（41 + 1 共享前缀 = 42）
        I18nKey::CmdNewProject,
        I18nKey::CmdNewMap,
        I18nKey::CmdOpenProject,
        I18nKey::CmdSaveProjectAs,
        I18nKey::CmdCloseProject,
        I18nKey::CmdExportProject,
        I18nKey::CmdProjectSettings,
        I18nKey::CmdSwitchVectorFlatView,
        I18nKey::CmdSwitchSchematicView,
        I18nKey::CmdSwitchFactionView,
        I18nKey::CmdFitMap,
        I18nKey::CmdZoomIn,
        I18nKey::CmdZoomOut,
        I18nKey::CmdToggleImmersive,
        I18nKey::CmdOpenCommandPalette,
        I18nKey::CmdSave,
        I18nKey::CmdUndo,
        I18nKey::CmdRedo,
        I18nKey::CmdTogglePanelMapTree,
        I18nKey::CmdTogglePanelLayers,
        I18nKey::CmdTogglePanelBrush,
        I18nKey::CmdTogglePanelMinimap,
        I18nKey::CmdTogglePanelCompass,
        I18nKey::CmdTogglePanelTimeline,
        I18nKey::CmdTogglePanelEvent,
        I18nKey::CmdTogglePanelSituation,
        I18nKey::CmdTogglePanelCharacter,
        I18nKey::CmdTogglePanelFaction,
        I18nKey::CmdTogglePanelRegion,
        I18nKey::CmdTogglePanelInspector,
        I18nKey::CmdTogglePanelValidation,
        I18nKey::CmdResetPanelLayout,
        I18nKey::CmdTogglePlay,
        I18nKey::CmdJumpToStart,
        I18nKey::CmdJumpToEnd,
        I18nKey::CmdToggleTheme,
        I18nKey::CmdToggleDensity,
        I18nKey::CmdToggleMapPalette,
        I18nKey::CmdForceRebuildTexture,
        I18nKey::CmdResetTerrainToBase,
        I18nKey::CmdBrushRadiusUp,
        I18nKey::CmdBrushRadiusDown,
        // 笔刷工具 7 条 + 参照网格（2026-10-16）
        I18nKey::CmdSelectBrushToolPaint,
        I18nKey::CmdSelectBrushToolRange,
        I18nKey::CmdSelectBrushToolPlateau,
        I18nKey::CmdSelectBrushToolHills,
        I18nKey::CmdSelectBrushToolVolcano,
        I18nKey::CmdSelectBrushToolLake,
        I18nKey::CmdSelectBrushToolRiver,
        I18nKey::CmdToggleGrid,
        I18nKey::CmdDrillToRoot,
        I18nKey::CmdDrillUp,
        // CLI（38）
        I18nKey::CliUsageLoad,
        I18nKey::CliUsageValidate,
        I18nKey::CliErrorPrefix,
        I18nKey::CliGuiNotCompiled,
        I18nKey::CliUnknownCommandPrefix,
        I18nKey::CliHelpBanner,
        I18nKey::CliHelpUsageHeader,
        I18nKey::CliHelpLoad,
        I18nKey::CliHelpValidate,
        I18nKey::CliHelpRasterSlice,
        I18nKey::CliHelpOvertureFetch,
        I18nKey::CliHelpOvertureApply,
        I18nKey::CliHelpGui,
        I18nKey::CliHelpGallery,
        I18nKey::CliHelpHelp,
        I18nKey::CliUsageRasterSlice,
        I18nKey::CliUsageRasterSliceMapId,
        I18nKey::CliRasterSliceBadSize,
        I18nKey::CliRasterSliceBadMapId,
        I18nKey::CliRasterSliceDone,
        I18nKey::CliRasterSliceInherit,
        I18nKey::CliProjectSummaryHeader,
        I18nKey::CliProjectSummaryHints,
        I18nKey::CliProjectSummaryHintBullet,
        I18nKey::CliValidateOk,
        I18nKey::CliValidateFailPrefix,
        I18nKey::CliGuiErrMDirNeeded,
        I18nKey::CliGuiErrDirDup,
        I18nKey::CliGuiErrHttpPortNeeded,
        I18nKey::CliGuiErrHttpPortDup,
        I18nKey::CliGuiErrHttpPortParse,
        I18nKey::CliGuiErrHttpPortZero,
        I18nKey::CliGuiErrHttpPortDisabled,
        I18nKey::CliGuiErrHttpBindNeeded,
        I18nKey::CliGuiErrHttpBindDup,
        I18nKey::CliGuiErrHttpBindParse,
        I18nKey::CliGuiErrHttpBindDisabled,
        I18nKey::CliGuiErrUnknownArg,
        I18nKey::CliGuiUsage,
        I18nKey::CliLanguageFlagHelp,
        I18nKey::CliLanguageBadTag,
        I18nKey::CliLanguageDup,
        I18nKey::CliLanguageMissing,
        // HTTP（7）
        I18nKey::HttpBindFailed,
        I18nKey::HttpListening,
        I18nKey::HttpAddrParseFailed,
        I18nKey::HttpTcpBindFailed,
        I18nKey::HttpServeFailed,
        I18nKey::HttpTokioStartFailed,
        I18nKey::HttpServerCrashed,
        // 大陆生成属性面板（12）
        I18nKey::ContinentDialogTitle,
        I18nKey::ContinentParamSeed,
        I18nKey::ContinentParamCoastWiggle,
        I18nKey::ContinentParamCoastDetail,
        I18nKey::ContinentParamRelief,
        I18nKey::ContinentParamForest,
        I18nKey::ContinentParamDesert,
        I18nKey::ContinentParamShelf,
        I18nKey::ContinentParamIslands,
        I18nKey::ContinentGenerate,
        I18nKey::ContinentCancel,
        I18nKey::ContinentToolName,
        I18nKey::ContinentNoticeGenerated,
        I18nKey::ContinentNoticeNoMap,
        I18nKey::ContinentNoticeNoEnvelopes,
        I18nKey::ContinentNoticeGenerateFailed,
        I18nKey::NewMapAspect,
        I18nKey::Aspect16x9,
        I18nKey::Aspect4x3,
        I18nKey::Aspect1x1,
        I18nKey::ScaleHintExtent,
        I18nKey::ScaleHintLand,
        I18nKey::ScaleHintCrossing,
    ];
}
