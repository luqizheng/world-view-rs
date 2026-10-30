//! 命令面板（`Ctrl+K`）与命令注册表（设计规格 §6.2/§6.3）。
#![cfg(feature = "app")]

//! ## 职责
//!
//! - **零隐藏功能**（D5）：每个功能要么常驻，要么在命令面板里能被搜到。
//!   [`CommandRegistry::all`] 收录**全部**功能，不是只收常用项（§6.2）。
//! - **快捷键的可发现性文档**（§6.3）：每条命令显示自己的快捷键。
//! - **模糊匹配**：按命令文本 + 分组标题匹配，中文子串即命中。
//!
//! ## 为什么用「命令 id」而不是闭包
//!
//! 命令面板要能被搜到、被列出、被测试；闭包无法枚举。命令以
//! [`CommandId`] 注册，执行时由 [`crate::ui`] 的分发器 switch 到具体动作
//! ——注册表是**数据**，执行是**行为**，分开才可测（§9.3 验证项 8）。

use bevy::prelude::*;

use crate::i18n::I18nKey;

use crate::state::{Panel, RecentProject};
use wv_render::state::SelectionShape;

/// 命令分组（§6.3，v0.2 §C 新增「项目」「导航」两组）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup {
    /// 工程级：新建 / 打开 / 另存为 / 关闭 / 设置 / 最近工程（v0.2 §A）。
    Project,
    /// 视图模式、缩放、沉浸。
    View,
    /// 撤销 / 重做 / 保存。
    Edit,
    /// 面板显隐。
    Panel,
    /// 时间轴播放与跳转。
    Time,
    /// 明暗、密度。
    Interface,
    /// 对象选择、查找。
    Object,
    /// 地图层级钻取（v0.2 §B）。
    Navigation,
}

impl CommandGroup {
    /// 分组标题（ADR-0018：locale-dependent 文案查表）。
    pub fn title(self, loc: crate::i18n::Locale) -> &'static str {
        use crate::i18n::{t, I18nKey};
        match self {
            CommandGroup::Project => t(loc, I18nKey::CommandGroupProject),
            CommandGroup::View => t(loc, I18nKey::CommandGroupView),
            CommandGroup::Edit => t(loc, I18nKey::CommandGroupEdit),
            CommandGroup::Panel => t(loc, I18nKey::CommandGroupPanel),
            CommandGroup::Time => t(loc, I18nKey::CommandGroupTime),
            CommandGroup::Interface => t(loc, I18nKey::CommandGroupInterface),
            CommandGroup::Object => t(loc, I18nKey::CommandGroupObject),
            CommandGroup::Navigation => t(loc, I18nKey::CommandGroupNavigation),
        }
    }

    /// 全部分组（面板按此顺序分节）。
    ///
    /// 「项目」放最前：它是**最外层的操作域**（决定后面所有操作作用在哪个工程上），
    /// 放在末尾会让人以为它是收尾动作。「导航」放最后：它是当前视图内的移动。
    pub const ALL: [CommandGroup; 8] = [
        CommandGroup::Project,
        CommandGroup::View,
        CommandGroup::Edit,
        CommandGroup::Panel,
        CommandGroup::Time,
        CommandGroup::Interface,
        CommandGroup::Object,
        CommandGroup::Navigation,
    ];
}

/// 命令标识。执行分发以它为键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    /// 切换视图模式（0..4 对应 [`crate::state::ViewMode`]）。
    SetViewMode(u8),
    /// 命令面板本身。
    OpenCommandPalette,
    /// 保存工程。
    Save,
    /// 撤销。
    Undo,
    /// 重做。
    Redo,
    /// 清除当前图的**全部**测量（2026-10-23 用户需求：工具条测量组右侧的
    /// 清除钮 / 命令面板双入口）。
    ///
    /// 与「测量橡皮」「Del」的区别：后两者删**一条**，本条清**一图**。
    /// 走 `ClearMeasurementsCommand`，一次撤销可整体还原。
    ClearMeasurements,
    /// 切换明暗主题。
    ToggleTheme,
    /// 切换密度档。
    ToggleDensity,
    /// 切换地图主题（注册表循环：JRPG 水彩 ⇄ 中国水墨）。
    /// 播放 / 暂停。
    TogglePlay,
    /// 沉浸模式。
    ToggleImmersive,
    /// 全览地图（视野归位）。
    FitMap,
    /// 缩放（+1 放大 / -1 缩小）。
    Zoom(i8),
    /// 跳到时间轴首 / 尾。
    JumpToStart,
    /// 跳到时间轴尾。
    JumpToEnd,
    /// 笔刷半径增减（像素步长）。
    BrushRadius(i8),
    /// 选中笔刷工具（索引对应 [`crate::state::BrushTool::ALL`]）。
    /// 选中即激活笔刷——「选工具 = 准备画」，与面板瓷砖点击同语义。
    SelectBrushTool(u8),
    /// 显示 / 隐藏参照网格（`ViewState.grid_lines`，地图工具条瓷砖同源）。
    ToggleGrid,
    /// 切换**地形场渲染**（`ViewState.field_render`，ADR-0053 D1 的回退开关）。
    ///
    /// 一条命令两条路径：开 = GPU 逐像素求值（分辨率无上限），关 = 现行 CPU
    /// 整图烘焙 + 分块。运行时可翻、当帧生效（不是重建 App 的启动项）。
    ToggleFieldRender,
    /// 显隐某个面板。
    TogglePanel(Panel),
    /// 重置所有面板位置。
    ResetPanelLayout,
    /// 打开最近工程列表里的第 n 条（索引对应 `RecentProjects::visible`）。
    OpenRecentProject(u8),
    /// 新建工程…（`Ctrl+Shift+N`）：弹「名称 + 位置 + 路径预览」对话框，
    /// 确认后在 `位置\名称` 落盘（含首张世界地图）并打开（2026-10-14 改版）。
    NewProject,
    /// 新建地图…（弹出参数对话框：名称 / 层级 / 幅面 / 海陆预填充）。
    NewMap,
    /// 打开工程…（`Ctrl+O`）。
    OpenProject,
    /// 另存为…（`Ctrl+Shift+S`）。
    SaveProjectAs,
    /// 关闭工程（`Ctrl+W`）。
    CloseProject,
    /// 导出（`Ctrl+E`）。§G 决议：v0.2 只加这一条工程级命令。
    ExportProject,
    /// 项目设置。
    ProjectSettings,
    /// 钻回上一级（`Backspace`）。
    DrillUp,
    /// 钻回世界地图（面包屑第一级）。
    DrillToRoot,
    /// 钻到面包屑的第 n 段（0 = 世界）。
    DrillToSegment(u8),
    /// 强制重建地形纹理（2026-10-14 用户需求：怀疑数据已改但画面没跟随时可用）。
    /// 把 `TerrainTexture.last_mode/last_map/soften` 一起重置为「未知」，
    /// 下一帧 `rebuild_texture_on_mode_change` 必然走全图重建路径。
    ForceRebuildTexture,
    /// 重新生成世界地形（ADR-0037 D4）：按新种子重跑模板生成。
    RegenWorld,
    /// 地图属性…（2026-10-27）：对当前图弹「名称 / 尺度声明」属性对话框，
    /// 工程菜单与命令面板双入口同源。
    MapProperties,
    /// 重置当前图的地形到底图（2026-10-16 用户需求）。
    /// 「底图」= `MapNode.base`，旧地图无 `base` 时按 `MapBase::Ocean` 兜底。
    /// 由 [`crate::edit::editing::apply_reset_terrain_to_base`] 执行。
    ResetTerrainToBase,
    /// 打开「生成大陆」属性面板（2026-10-17 大陆生成 UI plan Task 5）。
    /// 面板本身只收参数；生成走 `GenerateContinentCommand`（撤销恢复圈见 stash）。
    GenerateContinent,
    /// 地图工具条单选工具（2026-10-19 Figma `3:21848`）。
    ///
    /// **唯一入口**：工具条瓷砖与 V/H/B/E/L/M/P 快捷键都排这一条,由分发器
    /// 统一做互斥（开一个工具 = 关掉其它工具），避免两套逻辑漂移。
    SelectMapTool(MapToolId),
    /// 工具复位到浏览默认（ADR-0089 D5）：全关 + 开点选，**无翻转语义**。
    ///
    /// 与 [`CommandId::SelectMapTool`] 的差别在「已开着」分支——瓷砖点击的
    /// 翻转语义（再点一次 = 关）对程序化复位是错的：退出 CellMap 时点选正
    /// 开着，翻转落地反而不带工具。浏览性进入（切视图 / 退工作区 / 开工程）
    /// 一律排本命令，落地即「点选唯一激活」。
    ResetToolsToDefault,
}

/// 地图工具条的工具标识（[`CommandId::SelectMapTool`] 的载荷）。
///
/// **形态说明（ADR-0043）**：工具条上只剩「选择组」「测量组」两枚分裂按钮与
/// 一枚橡皮瓷砖；`Brush` / `Continent` / `MeasureEraser` 三个变体不再有瓷砖，
/// 但**必须保留**——快捷键（B / P / E）与命令面板仍指向它们，删变体会让
/// 那两个入口静默失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapToolId {
    /// 选择（圈选意图包络）。载荷 = 选区形状（多边形 / 矩形 / 椭圆）。
    Select(SelectionShape),
    /// 抓手：左键拖拽平移。
    Pan,
    /// 笔刷：开关地形涂抹（按 `BrushState.tool` 当前值走；工具条无瓷砖，
    /// B 快捷键 + 笔刷面板入口）。
    Brush,
    /// 地形橡皮刷：开关橡皮模式（`BrushTool::Eraser`）。
    BrushErase,
    /// 折线测量。
    MeasureLine,
    /// 矩形测量。
    MeasureRect,
    /// 测量橡皮。
    MeasureEraser,
    /// 打开「生成大陆」面板（不是互斥工具，只开门）。
    Continent,
    /// 单格编辑（[ADR-0040]）：点选一格改 surface/height/biome 三通道。
    ///
    /// 与 `Brush` 是**两类工具**：`Brush` 是批改（沿 stroke 改多格）,
    /// `EditCell` 是最 detail 的精细编辑器（一次一格,每格 1 command）——
    /// 故**不入 `BrushTool` 枚举,与现有工具平级进 `MapToolId`**。
    /// 由 `ui::cell_pick` 做命中测试,由 `inspector_panel` 渲染字段编辑 UI。
    EditCell,
    /// **点选工具**（ADR-0057 补记 6，原 `CellSelect` 改名）：点一下把该格
    /// 写入统一选区 `CellSelection`——无修饰 = 替换、Shift = 并、Alt = 差。
    ///
    /// 与 [`Self::EditCell`] 是**两件工具两个状态**：PointSelect 只选不画，
    /// 持有独立的 `PointSelectState`（不再与 EditCell 共用 `tool_active` /
    /// `allow_drag`）；EditCell 只画不选（ADR-0057 D2「先名词后动词」）。
    ///
    /// 为什么保留这枚工具：CellMap 的高频动作是「看/选这几格」，给一个
    /// 不会误落笔的入口，用户敢在地图上随手点。
    PointSelect,
    /// 群系分界线（2026-10-22 内容矩阵 §2.7 / ADR-0051）：可拖线段两侧各绑
    /// 一个群系，拖动推移分界——扫过格「退让侧→推进侧」互转（第三方不动）。
    /// 仅 Topographic 可用；线是会话工具不落盘。
    BiomeBoundary,
    /// **线段 POI**（ADR-0070 D4）：在地图上画河流 / 道路——一条线就是一个
    /// 地点实体（`Place::line` + `line_kind`）。载荷 = 线的种类（河 / 路）。
    ///
    /// 与 `Select(shape)` 同款「一类工具多个笔尖」建模：河与路不是两件工具，
    /// 是同一绘制工具的两个类，工具条分裂按钮换类时**不关工具**（分发器
    /// already_on 判据 = `tool_active` ∧ 同类）。
    LinePoi(wv_core::distance::LineKind),
    /// **地标放置**（ADR-0071）：激活后单击地图创建点状 `Place`（POI）——
    /// Edit 浏览档落点击点，CellMap 工作区吸附格中心。默认类别 / 级别
    /// 在地标面板选（先名词后动词，与笔刷同心智）。
    ///
    /// **无工具条瓷砖、无快捷键**：唯一入口是地标面板的「放置」toggle
    /// （排 `SelectMapTool(Landmark)`）。保留 `MapToolId` 身份是为了白拿
    /// dispatch 的互斥机制——激活时自动关闭选择 / 点选 / EditCell 等
    /// 一切左键消费者，反之进 CellMap 时也会被 EditCell 关掉。
    Landmark,
}

impl MapToolId {
    /// 工具的**工作区白名单**（2026-10-28 用户裁定·方案 A：声明式单表）。
    ///
    /// 三工作区功能分区的唯一过滤源——工具条瓷砖灰化、快捷键拦截、命令
    /// 面板隐藏、分发器终审四处置共用本表，不各自重拼：
    ///
    /// - **Browse / Map**（叙事与政治图档）：选择、抓手、点选（万能选择器）、
    ///   测量（Map 是距离主场）；
    /// - **CellMap**（Topographic 地形工作台）：地形编辑全家（笔刷/橡皮/
    ///   单格/群系分界/大陆生成）+ 地标放置（POI 编辑主场）；
    /// - **LinePoi**（河/路绘制）：Map 档——CellMap 只**看**线不画线
    ///   （2026-10-28 裁定②：CellMap 道路/河流可见属渲染层，编辑入口在 Map）。
    pub fn available_in(self, mode: crate::state::ViewMode) -> bool {
        use crate::state::ViewMode;
        match self {
            // 公共：导航与选择（点选 = 万能选择器，Browse 选图钉也靠它，
            // ADR-0074 追记一；「POI 浏览处处有」的载体）。
            Self::Select(_) | Self::Pan | Self::PointSelect => true,
            Self::MeasureLine | Self::MeasureRect | Self::MeasureEraser => {
                matches!(mode, ViewMode::VectorFlat | ViewMode::GoogleMap)
            }
            Self::LinePoi(_) => mode == ViewMode::GoogleMap,
            Self::Brush
            | Self::BrushErase
            | Self::EditCell
            | Self::BiomeBoundary
            | Self::Continent
            | Self::Landmark => mode == ViewMode::Topographic,
        }
    }
}

/// 进入 CellMap 工作区（顶栏段「浏览性进入」与「编辑城市」按钮的**编辑性进入**
/// 共用编排，ADR-0089 D3）：快照全部面板可见性 → 只开笔刷/地标浮窗 → 排默认工具。
///
/// 工具排队按 `editing` 分流：`false`（浏览性进入）排
/// [`CommandId::ResetToolsToDefault`]——落地即点选唯一激活；不能排
/// `SelectMapTool(PointSelect)`，它的「已开着 = 关」翻转语义会把浏览态带过来
/// 的点选关掉。`true`（编辑性进入）排 `SelectMapTool(Brush)`——场景分区刷色
/// 的载体，M1「进去即刷」；编辑性进入只能从浏览态发起，笔刷必关，翻转语义
/// 不会误触。工具语义全在分发器的互斥分支，这里只编排面板与排队。
pub fn enter_cellmap_workspace(
    panels: &mut crate::state::PanelState,
    palette: &mut CommandPalette,
    editing: bool,
) {
    let backup = Panel::ALL
        .iter()
        .map(|p| (*p, panels.is_visible(*p)))
        .collect();
    panels.cell_workspace_backup = Some(backup);
    for p in Panel::ALL {
        panels.set_visible(p, false);
    }
    // 2026-10-24 用户裁定：CellMap 的编辑控件（读数条 / 落笔值 / 泛洪判据 /
    // 派生运算 / 单格与批量表单）迁入**笔刷浮动窗**。故工作区开的是 Brush
    // 而不是 Inspector。
    panels.set_visible(Panel::Brush, true);
    // 地标面板（ADR-0071）随行开启：CellMap 工作区是地标放置的两用场景之一
    // （吸附格中心），没有它工作区内无放置入口。退出时随快照还原。
    panels.set_visible(Panel::Landmark, true);
    palette.pending = Some(if editing {
        CommandId::SelectMapTool(MapToolId::Brush)
    } else {
        CommandId::ResetToolsToDefault
    });
}

/// 退出 CellMap 工作区（2026-10-28 修：旁路泄漏根修；ADR-0089 D5 改排点选）。
///
/// 进 CellMap 时顶栏会把全部面板可见性快照进
/// [`crate::state::PanelState::cell_workspace_backup`] 再只开笔刷/地标窗；
/// 退出必须还原快照并复位工具。此前**只有顶栏三段按钮**走这套编排
/// ——快捷键 1/2、命令面板、HTTP `set_view_mode` 直接写 `view.mode` 绕过
/// 退出，快照泄漏：Inspector 保持隐藏、工作区旗标立着（势力层/事态页签
/// 持续禁用）、EditCell 工具不关。本助手是退出的**单一入口**，各路径共用。
///
/// 排队命令 = [`CommandId::ResetToolsToDefault`]（ADR-0089 D5：退出落地即
/// 浏览态默认点选，全关其余工具）。旧值 `EditCell` 充当关动作依赖「进入必开
/// EditCell」的前提——D5 后进入默认点选，EditCell 可能根本没开，排它会在
/// 浏览档撞白名单误报；排 `SelectMapTool(PointSelect)` 又有「已开着 = 关」
/// 翻转语义（退出时点选正开着，落地反而不带工具）。复位命令两坑都避开。
///
/// `palette: Option` —— HTTP 路径拿不到面板队列时传 `None`（工具关闭留待
/// 用户下一次切工具；面板还原是主要修复面）。
pub fn exit_cellmap_workspace(
    panels: &mut crate::state::PanelState,
    palette: Option<&mut CommandPalette>,
) {
    if let Some(backup) = panels.cell_workspace_backup.take() {
        for (p, v) in backup {
            panels.set_visible(p, v);
        }
        if let Some(palette) = palette {
            palette.pending = Some(CommandId::ResetToolsToDefault);
        }
    }
}

/// 面板显隐条目的标签键——命令面板「面板」组与工程菜单「面板」分组共用
/// （ADR-0087），不再各写一份 match 漂移。
pub fn panel_toggle_label_key(p: Panel) -> I18nKey {
    match p {
        Panel::MapTree => I18nKey::CmdTogglePanelMapTree,
        Panel::Layers => I18nKey::CmdTogglePanelLayers,
        Panel::Brush => I18nKey::CmdTogglePanelBrush,
        Panel::Minimap => I18nKey::CmdTogglePanelMinimap,
        Panel::Timeline => I18nKey::CmdTogglePanelTimeline,
        Panel::Event => I18nKey::CmdTogglePanelEvent,
        Panel::Situation => I18nKey::CmdTogglePanelSituation,
        Panel::Character => I18nKey::CmdTogglePanelCharacter,
        Panel::Faction => I18nKey::CmdTogglePanelFaction,
        Panel::Region => I18nKey::CmdTogglePanelRegion,
        Panel::Landmark => I18nKey::CmdTogglePanelLandmark,
        Panel::Inspector => I18nKey::CmdTogglePanelInspector,
        Panel::CellEditor => I18nKey::CmdTogglePanelCellEditor,
        Panel::Validation => I18nKey::CmdTogglePanelValidation,
        // 线段 POI 面板（ADR-0070 D4）：工具Owned——随工具开关，但
        // 也允许作者从菜单/设置直接显隐，标签复用面板标题键（不再
        // 另立 CmdTogglePanelLinePoi 键）。
        Panel::LinePoi => I18nKey::LinePoiPanelTitle,
    }
}

/// 工具类命令条目在当前工作区是否列出（非工具命令恒 true）——
/// [`CommandPalette::results`] / [`CommandPalette::results_with_locale`] 共用。
fn tool_entry_available(id: &CommandId, mode: crate::state::ViewMode) -> bool {
    match id {
        CommandId::SelectMapTool(t) => t.available_in(mode),
        _ => true,
    }
}

/// 一条命令：注册表的条目。
#[derive(Debug, Clone)]
pub struct Command {
    /// 命令标识。
    pub id: CommandId,
    /// 分组。
    pub group: CommandGroup,
    /// 显示名（可搜索文本）。
    ///
    /// 是 `String` 而不是 `&'static str`：最近工程的显示名来自工程自身
    /// （`world.yaml` 的 `meta.name`），运行时才知道。
    pub label: String,
    /// 快捷键（显示用，无则 None）。
    pub keys: Option<&'static str>,
}

impl Command {
    /// 是否命中查询（子串匹配，忽略大小写）。
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        self.label.to_lowercase().contains(&q)
            || self
                .group
                .title(crate::i18n::Locale::default())
                .contains(&q)
            || self.keys.unwrap_or("").to_lowercase().contains(&q)
    }
}

/// 命令注册表：全部功能的唯一清单。
pub struct CommandRegistry;

impl CommandRegistry {
    /// 全部命令，含动态的「最近工程」条目（§6.2：命令面板收录**全部**功能）。
    ///
    /// `recent` 由调用方传入而不是从全局读：注册表保持**纯函数**，
    /// 才能被单元测试直接戳（§9.3 验证项 8 靠它把关）。
    pub fn all_with(recent: &[RecentProject]) -> Vec<Command> {
        Self::all_with_locale(crate::i18n::Locale::default(), recent)
    }

    /// 全部命令 + 最近工程，按指定 locale 查表。
    pub fn all_with_locale(locale: crate::i18n::Locale, recent: &[RecentProject]) -> Vec<Command> {
        let mut v = Self::all_static(locale);
        // 项目：最近工程（插在本组固定条目之前，按最近使用排序）。
        let recent_prefix =
            crate::i18n::t(locale, crate::i18n::I18nKey::ProjectMenuRecentOpenPrefix);
        let mut recent_cmds: Vec<Command> = recent
            .iter()
            .take(8)
            .enumerate()
            .map(|(i, p)| Command {
                id: CommandId::OpenRecentProject(i as u8),
                group: CommandGroup::Project,
                label: format!("{recent_prefix}{}", p.name),
                keys: None,
            })
            .collect();
        let project_at = v
            .iter()
            .position(|c| c.group == CommandGroup::Project)
            .unwrap_or(v.len());
        recent_cmds.append(&mut v.split_off(project_at));
        v.extend(recent_cmds);
        v
    }

    /// 全部**静态**命令（不含运行时才知道名字的最近工程）。
    pub fn all() -> Vec<Command> {
        Self::all_static(crate::i18n::Locale::default())
    }

    /// 全部**静态**命令（不含运行时才知道名字的最近工程），按 Locale 查表。
    pub fn all_static(locale: crate::i18n::Locale) -> Vec<Command> {
        let mut v: Vec<Command> = Vec::new();
        let mut push =
            |id: CommandId, group: CommandGroup, label_key: I18nKey, keys: Option<&'static str>| {
                v.push(Command {
                    id,
                    group,
                    label: crate::i18n::t(locale, label_key).to_owned(),
                    keys,
                })
            };

        // 项目（v0.2 §A）
        push(
            CommandId::NewProject,
            CommandGroup::Project,
            I18nKey::CmdNewProject,
            Some("Ctrl+Shift+N"),
        );
        push(
            CommandId::NewMap,
            CommandGroup::Project,
            I18nKey::CmdNewMap,
            None,
        );
        push(
            CommandId::OpenProject,
            CommandGroup::Project,
            I18nKey::CmdOpenProject,
            Some("Ctrl+O"),
        );
        push(
            CommandId::SaveProjectAs,
            CommandGroup::Project,
            I18nKey::CmdSaveProjectAs,
            Some("Ctrl+Shift+S"),
        );
        push(
            CommandId::CloseProject,
            CommandGroup::Project,
            I18nKey::CmdCloseProject,
            Some("Ctrl+W"),
        );
        push(
            CommandId::ExportProject,
            CommandGroup::Project,
            I18nKey::CmdExportProject,
            Some("Ctrl+E"),
        );
        push(
            CommandId::ProjectSettings,
            CommandGroup::Project,
            I18nKey::CmdProjectSettings,
            None,
        );
        push(
            CommandId::MapProperties,
            CommandGroup::Project,
            I18nKey::CmdMapProperties,
            None,
        );

        // 视图（BROWSING 三项：矢量地形 / 普通地图 / 势力图；ADR-0069 后与顶栏 Browse/Map 对齐）
        for (i, mode) in crate::state::ViewMode::BROWSING.iter().enumerate() {
            push(
                CommandId::SetViewMode(i as u8),
                CommandGroup::View,
                match mode {
                    crate::state::ViewMode::VectorFlat => I18nKey::CmdSwitchVectorFlatView,
                    crate::state::ViewMode::Faction => I18nKey::CmdSwitchFactionView,
                    crate::state::ViewMode::GoogleMap => I18nKey::CmdSwitchGoogleMapView,
                    // BROWSING 之外的模式不再注册（枚举变体仍在，见 state.rs）。
                    _ => continue,
                },
                Some(["1", "2", "3"][i]),
            );
        }
        push(
            CommandId::FitMap,
            CommandGroup::View,
            I18nKey::CmdFitMap,
            Some("F"),
        );
        push(
            CommandId::ToggleGrid,
            CommandGroup::View,
            I18nKey::CmdToggleGrid,
            None,
        );
        // ADR-0053 D1 回退开关（Phase 1 接线 2026-10-24）：紧挨参照网格——
        // 两者都是「底图怎么画」的会话级开关，语义同一族。
        push(
            CommandId::ToggleFieldRender,
            CommandGroup::View,
            I18nKey::CmdToggleFieldRender,
            None,
        );
        push(
            CommandId::Zoom(1),
            CommandGroup::View,
            I18nKey::CmdZoomIn,
            Some("+ / 滚轮"),
        );
        push(
            CommandId::Zoom(-1),
            CommandGroup::View,
            I18nKey::CmdZoomOut,
            Some("−"),
        );
        push(
            CommandId::ToggleImmersive,
            CommandGroup::View,
            I18nKey::CmdToggleImmersive,
            Some("Ctrl+Shift+I"),
        );

        // 编辑
        push(
            CommandId::OpenCommandPalette,
            CommandGroup::Edit,
            I18nKey::CmdOpenCommandPalette,
            Some("Ctrl+K"),
        );
        // 单格编辑工具（[ADR-0040]）：与瓷砖/快捷键同源，排 SelectMapTool
        // 由分发器做互斥。dock 瓷砖被 16 系统参数上限卡住（ADR-0040 §6），
        // 命令面板 + 快捷键 C 是现行入口。
        push(
            CommandId::SelectMapTool(MapToolId::EditCell),
            CommandGroup::Edit,
            I18nKey::CmdToolEditCell,
            Some("C"),
        );
        // 点选工具（ADR-0057 补记 6）：点选一格进统一选区，不落笔。
        // 快捷键 `S` = Select（`V` 已属框选组，不复用）。
        push(
            CommandId::SelectMapTool(MapToolId::PointSelect),
            CommandGroup::Edit,
            I18nKey::CmdToolPointSelect,
            Some("S"),
        );
        // 群系分界线（2026-10-22 内容矩阵 / ADR-0051）：与 EditCell 同款——
        // dock 瓷砖被系统参数上限卡住，命令面板是现行入口。
        push(
            CommandId::SelectMapTool(MapToolId::BiomeBoundary),
            CommandGroup::Edit,
            I18nKey::CmdToolBiomeBoundary,
            None,
        );
        // 线段 POI（ADR-0070 D4）：工具条分裂按钮（仅 Edit 档出现）与命令
        // 面板同源。面板只登记河（默认类）一条——路是同一命令的另一个载荷，
        // 入口在工具条弹层，不另立可搜索条目。
        push(
            CommandId::SelectMapTool(MapToolId::LinePoi(wv_core::distance::LineKind::River)),
            CommandGroup::Edit,
            I18nKey::LinePoiTool,
            None,
        );
        push(
            CommandId::Save,
            CommandGroup::Edit,
            I18nKey::CmdSave,
            Some("Ctrl+S"),
        );
        push(
            CommandId::Undo,
            CommandGroup::Edit,
            I18nKey::CmdUndo,
            Some("Ctrl+Z"),
        );
        push(
            CommandId::Redo,
            CommandGroup::Edit,
            I18nKey::CmdRedo,
            Some("Ctrl+Shift+Z"),
        );
        // 清除全部测量（2026-10-23）：工具条测量组右侧的清除钮同源；
        // 「Del 删一条」是渲染层的输入，不进命令面板（它不是无指针可执行的动作）。
        push(
            CommandId::ClearMeasurements,
            CommandGroup::Edit,
            I18nKey::CmdClearMeasurements,
            None,
        );

        // 面板
        for p in Panel::ALL {
            let label_key = panel_toggle_label_key(p);
            push(
                CommandId::TogglePanel(p),
                CommandGroup::Panel,
                label_key,
                None,
            );
        }
        push(
            CommandId::ResetPanelLayout,
            CommandGroup::Panel,
            I18nKey::CmdResetPanelLayout,
            None,
        );

        // 时间
        push(
            CommandId::TogglePlay,
            CommandGroup::Time,
            I18nKey::CmdTogglePlay,
            Some("Space"),
        );
        push(
            CommandId::JumpToStart,
            CommandGroup::Time,
            I18nKey::CmdJumpToStart,
            Some("Home"),
        );
        push(
            CommandId::JumpToEnd,
            CommandGroup::Time,
            I18nKey::CmdJumpToEnd,
            Some("End"),
        );

        // 界面
        push(
            CommandId::ToggleTheme,
            CommandGroup::Interface,
            I18nKey::CmdToggleTheme,
            Some("Ctrl+J"),
        );
        push(
            CommandId::ToggleDensity,
            CommandGroup::Interface,
            I18nKey::CmdToggleDensity,
            None,
        );
        // 2026-10-14 用户需求：怀疑数据已改、画面没跟随时手动重画底图。
        push(
            CommandId::ForceRebuildTexture,
            CommandGroup::Interface,
            I18nKey::CmdForceRebuildTexture,
            Some("F5"),
        );

        // ADR-0037 D4：完全重生成世界地形（模板 + 新种子）。
        push(
            CommandId::RegenWorld,
            CommandGroup::Object,
            I18nKey::CmdRegenWorld,
            None,
        );

        // 2026-10-16 用户需求：笔刷面板的「重置到底图」按钮同样入口；
        // 命令面板里能搜到，方便不愿点面板的人。
        push(
            CommandId::ResetTerrainToBase,
            CommandGroup::Interface,
            I18nKey::CmdResetTerrainToBase,
            None,
        );
        // 大陆生成：先圈意图包络（工具条「大陆」瓷砖），再从这里开属性面板。
        push(
            CommandId::GenerateContinent,
            CommandGroup::Object,
            I18nKey::ContinentDialogTitle,
            None,
        );

        // 对象
        push(
            CommandId::BrushRadius(1),
            CommandGroup::Object,
            I18nKey::CmdBrushRadiusUp,
            Some("]"),
        );
        push(
            CommandId::BrushRadius(-1),
            CommandGroup::Object,
            I18nKey::CmdBrushRadiusDown,
            Some("["),
        );
        // 笔刷工具 7 条（索引 = BrushTool::ALL 下标，命令面板与选项条瓷砖同一
        // 口径）。**顺序必须与 `BrushTool::ALL` 逐项对齐**——按下标索引，插队
        // 会让后面所有工具的命令面板条目指向错误的工具。
        // （原 8 条含河流；2026-10-27 河流退役改线 POI（ADR-0070），与 ALL
        // 同批同位删除。）
        for (i, key) in [
            I18nKey::CmdSelectBrushToolPaint,
            I18nKey::CmdSelectBrushToolRange,
            I18nKey::CmdSelectBrushToolPlateau,
            I18nKey::CmdSelectBrushToolHills,
            I18nKey::CmdSelectBrushToolVolcano,
            I18nKey::CmdSelectBrushToolLake,
            I18nKey::CmdSelectBrushToolErase,
        ]
        .into_iter()
        .enumerate()
        {
            push(
                CommandId::SelectBrushTool(i as u8),
                CommandGroup::Object,
                key,
                None,
            );
        }

        // 导航（v0.2 §B）
        push(
            CommandId::DrillToRoot,
            CommandGroup::Navigation,
            I18nKey::CmdDrillToRoot,
            None,
        );
        push(
            CommandId::DrillUp,
            CommandGroup::Navigation,
            I18nKey::CmdDrillUp,
            Some("Backspace"),
        );

        v
    }

    /// 按查询过滤（保持注册顺序）。
    pub fn search(query: &str, recent: &[RecentProject]) -> Vec<Command> {
        Self::all_with_locale(crate::i18n::Locale::default(), recent)
            .into_iter()
            .filter(|c| c.matches(query))
            .collect()
    }
}

/// 命令面板的瞬时 UI 状态。
#[derive(Resource, Debug, Default)]
pub struct CommandPalette {
    /// 是否打开。
    pub open: bool,
    /// 查询串。
    pub query: String,
    /// 高亮项索引。
    pub selected: usize,
    /// 待执行命令（由 UI 系统消费）。
    pub pending: Option<CommandId>,
}

impl CommandPalette {
    /// 打开并聚焦。
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    /// 关闭。
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }

    /// 当前查询的命中项。
    pub fn results(&self, recent: &[RecentProject], mode: crate::state::ViewMode) -> Vec<Command> {
        CommandRegistry::search(&self.query, recent)
            .into_iter()
            .filter(|c| tool_entry_available(&c.id, mode))
            .collect()
    }

    /// 当前查询的命中项，按 Locale 查表（ADR-0018）。
    ///
    /// `mode` 过滤工作区外工具（[`MapToolId::available_in`]，2026-10-28
    /// 裁定⑤）：命令面板「零隐藏功能」原则不变——条目没消失，只是**当前
    /// 工作区用不了的不列出**（切到对应工作区即出现）。
    pub fn results_with_locale(
        &self,
        recent: &[RecentProject],
        locale: crate::i18n::Locale,
        mode: crate::state::ViewMode,
    ) -> Vec<Command> {
        CommandRegistry::all_with_locale(locale, recent)
            .into_iter()
            .filter(|c| tool_entry_available(&c.id, mode))
            .filter(|c| {
                if self.query.is_empty() {
                    return true;
                }
                let q = self.query.to_lowercase();
                c.label.to_lowercase().contains(&q)
                    || c.group.title(locale).contains(&q)
                    || c.keys.unwrap_or("").to_lowercase().contains(&q)
            })
            .collect()
    }

    /// 执行高亮项。
    pub fn run_selected(&mut self, recent: &[RecentProject], mode: crate::state::ViewMode) {
        let results = self.results(recent, mode);
        if let Some(cmd) = results.get(self.selected) {
            self.pending = Some(cmd.id);
        }
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_every_panel_and_view_mode() {
        let all = CommandRegistry::all();
        for p in Panel::ALL {
            assert!(
                all.iter().any(|c| c.id == CommandId::TogglePanel(p)),
                "面板 {} 未注册命令",
                p.title(crate::i18n::Locale::default())
            );
        }
        for i in 0..crate::state::ViewMode::BROWSING.len() as u8 {
            assert!(
                all.iter().any(|c| c.id == CommandId::SetViewMode(i)),
                "浏览视图模式 {i} 未注册命令"
            );
        }
        // BROWSING 之外的模式不占命令面板（三项 → 只有 0/1/2；
        // 2026-10-26 ADR-0069 起索引 1 = 普通地图、2 = 势力图）。
        assert!(!all.iter().any(|c| c.id == CommandId::SetViewMode(3)));
    }

    #[test]
    fn search_finds_command_by_chinese_label_substring() {
        let hits = CommandRegistry::search("笔刷", &[]);
        assert!(hits.iter().any(|c| c.id == CommandId::BrushRadius(1)));
        assert!(hits
            .iter()
            .any(|c| c.id == CommandId::TogglePanel(Panel::Brush)));
    }

    #[test]
    fn search_finds_command_by_hotkey_text() {
        let hits = CommandRegistry::search("ctrl+k", &[]);
        assert!(
            hits.iter().any(|c| c.id == CommandId::OpenCommandPalette),
            "应能按快捷键文本搜到命令面板"
        );
    }

    /// 全部笔刷工具（`BrushTool::ALL` 逐项）注册且中文可搜（「零隐藏功能」D5：
    /// 工具瓷砖之外，Ctrl+K 是第二入口）；网格开关注册在视图组。
    ///
    /// 断言按 `ALL.len()` 循环，**不是**写死个数——往 `ALL` 加工具而忘了在
    /// 注册表补条目，这条会立刻失败（反之，写死个数则新增时静默通过）。
    #[test]
    fn every_brush_tool_and_grid_toggle_are_registered_and_searchable() {
        let all = CommandRegistry::all();
        for i in 0..crate::state::BrushTool::ALL.len() as u8 {
            assert!(
                all.iter()
                    .any(|c| matches!(c.id, CommandId::SelectBrushTool(x) if x == i)),
                "笔刷工具 {i} 未注册"
            );
        }
        assert!(all.iter().any(|c| c.id == CommandId::ToggleGrid));
        // 中文子串搜到具体工具（ALL[1] = 山系笔，ADR-0076 D3 改名后按新名可搜）。
        let hits = CommandRegistry::search("山系", &[]);
        assert!(
            hits.iter()
                .any(|c| matches!(c.id, CommandId::SelectBrushTool(1))),
            "搜「山系」应命中笔刷工具（山系笔）"
        );
    }

    #[test]
    fn every_command_has_a_label() {
        for c in CommandRegistry::all_with(&sample_recent()) {
            assert!(!c.label.is_empty(), "{:?} 缺少显示名", c.id);
        }
    }

    fn sample_recent() -> Vec<RecentProject> {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        vec![
            RecentProject {
                dir: "/w/a".into(),
                name: "四渡赤水 · 行军图".into(),
                last_opened_at: t0,
            },
            RecentProject {
                dir: "/w/b".into(),
                name: "封神演义 · 架空".into(),
                last_opened_at: t0 - Duration::from_secs(3600),
            },
        ]
    }

    #[test]
    fn every_group_is_populated() {
        // 「零隐藏功能」D5 的另一面：分组不能在面板里显示成空标题。
        let all = CommandRegistry::all_with(&sample_recent());
        for g in CommandGroup::ALL {
            assert!(
                all.iter().any(|c| c.group == g),
                "分组「{}」没有任何命令",
                g.title(crate::i18n::Locale::default())
            );
        }
    }

    #[test]
    fn project_group_covers_the_v0_2_menu_entries() {
        let ids: Vec<CommandId> = CommandRegistry::all().iter().map(|c| c.id).collect();
        for want in [
            CommandId::NewProject,
            CommandId::OpenProject,
            CommandId::SaveProjectAs,
            CommandId::CloseProject,
            CommandId::ExportProject,
            CommandId::ProjectSettings,
        ] {
            assert!(ids.contains(&want), "{want:?} 未注册");
        }
        // 「保存」与「撤销」**不进**项目菜单（v0.2 §A：它们已有独立按钮）。
        let project_labels: Vec<String> = CommandRegistry::all()
            .into_iter()
            .filter(|c| c.group == CommandGroup::Project)
            .map(|c| c.label)
            .collect();
        assert!(
            !project_labels.iter().any(|l| l.contains("保存工程")),
            "保存工程不应出现在项目分组"
        );
    }

    #[test]
    fn navigation_group_covers_drill_commands() {
        let ids: Vec<CommandId> = CommandRegistry::all().iter().map(|c| c.id).collect();
        assert!(ids.contains(&CommandId::DrillUp));
        assert!(ids.contains(&CommandId::DrillToRoot));
    }

    #[test]
    fn recent_projects_become_searchable_commands_in_input_order() {
        let all = CommandRegistry::all_with(&sample_recent());
        let recents: Vec<&Command> = all
            .iter()
            .filter(|c| matches!(c.id, CommandId::OpenRecentProject(_)))
            .collect();
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[0].id, CommandId::OpenRecentProject(0));
        assert!(
            recents[0].label.contains("四渡赤水"),
            "got {}",
            recents[0].label
        );
        assert_eq!(recents[1].id, CommandId::OpenRecentProject(1));
        // 分组归属正确（否则面板会把它分错节）。
        assert!(recents.iter().all(|c| c.group == CommandGroup::Project));
        // 最近工程应排在项目组固定条目之前（列表顺序 = 视觉顺序）。
        let first_project = all
            .iter()
            .position(|c| c.group == CommandGroup::Project)
            .expect("项目组非空");
        assert!(matches!(
            all[first_project].id,
            CommandId::OpenRecentProject(_)
        ));
    }

    #[test]
    fn search_matches_a_recent_project_by_name() {
        let hits = CommandRegistry::search("封神", &sample_recent());
        assert_eq!(hits.len(), 1, "只应命中那一条最近工程");
        assert_eq!(hits[0].id, CommandId::OpenRecentProject(1));
    }

    #[test]
    fn command_count_stays_above_the_v0_1_baseline() {
        // v0.1 是 29 条，v0.2 加项目 6 条 + 导航 2 条 = 37 条（不含动态最近工程）。
        let n = CommandRegistry::all().len();
        assert!(n >= 37, "命令数 {n} 少于 v0.2 基线 37——有功能被漏注册");
    }

    #[test]
    fn run_selected_uses_the_same_result_list_that_was_displayed() {
        let mut p = CommandPalette::default();
        p.open();
        p.query = "钻回世界".into();
        let results = p.results(&sample_recent(), crate::state::ViewMode::VectorFlat);
        assert!(!results.is_empty());
        p.selected = 0;
        p.run_selected(&sample_recent(), crate::state::ViewMode::VectorFlat);
        assert_eq!(p.pending, Some(CommandId::DrillToRoot));
        assert!(!p.open, "执行后应关闭面板");
    }

    #[test]
    fn workspace_whitelist_routes_tools_to_their_workspaces() {
        // 2026-10-28 裁定⑤守点：白名单单表——地形编辑族只在 CellMap
        // （Topographic），测量在 Browse/Map，LinePoi 只在 Map，选择/抓手/
        // 点选三档公共（点选 = 万能选择器，ADR-0074）。
        use crate::state::ViewMode;
        use wv_core::distance::LineKind;
        let cellmap = [
            MapToolId::Brush,
            MapToolId::BrushErase,
            MapToolId::EditCell,
            MapToolId::BiomeBoundary,
            MapToolId::Continent,
            MapToolId::Landmark,
        ];
        for t in cellmap {
            assert!(
                t.available_in(ViewMode::Topographic),
                "{t:?} 应在 CellMap 可用"
            );
            assert!(
                !t.available_in(ViewMode::VectorFlat) && !t.available_in(ViewMode::GoogleMap),
                "{t:?} 是 CellMap 工具，Browse/Map 不可用"
            );
        }
        for t in [
            MapToolId::MeasureLine,
            MapToolId::MeasureRect,
            MapToolId::MeasureEraser,
        ] {
            assert!(t.available_in(ViewMode::VectorFlat) && t.available_in(ViewMode::GoogleMap));
            assert!(
                !t.available_in(ViewMode::Topographic),
                "CellMap 不要测量叠加"
            );
        }
        assert!(MapToolId::LinePoi(LineKind::River).available_in(ViewMode::GoogleMap));
        assert!(!MapToolId::LinePoi(LineKind::River).available_in(ViewMode::VectorFlat));
        assert!(!MapToolId::LinePoi(LineKind::Road).available_in(ViewMode::Topographic));
        for m in [
            ViewMode::VectorFlat,
            ViewMode::GoogleMap,
            ViewMode::Topographic,
        ] {
            assert!(MapToolId::Pan.available_in(m), "抓手是导航，三档公共");
            assert!(
                MapToolId::PointSelect.available_in(m),
                "点选=万能选择器，三档公共"
            );
            assert!(MapToolId::Select(SelectionShape::Polygon).available_in(m));
        }
    }

    #[test]
    fn palette_hides_cellmap_only_tools_outside_cellmap() {
        // 命令面板过滤守点：Browse 档搜不到单格编辑；CellMap 档搜得到。
        let mut p = CommandPalette::default();
        p.open();
        p.query = "单格".into();
        let hits = p.results_with_locale(
            &[],
            crate::i18n::Locale::default(),
            crate::state::ViewMode::VectorFlat,
        );
        assert!(
            hits.iter()
                .all(|c| c.id != CommandId::SelectMapTool(MapToolId::EditCell)),
            "Browse 档不应列出 CellMap 专属工具"
        );
        let hits = p.results_with_locale(
            &[],
            crate::i18n::Locale::default(),
            crate::state::ViewMode::Topographic,
        );
        assert!(
            hits.iter()
                .any(|c| c.id == CommandId::SelectMapTool(MapToolId::EditCell)),
            "CellMap 档应能搜到单格编辑"
        );
    }

    #[test]
    fn exit_cellmap_workspace_restores_panel_snapshot() {
        // 2026-10-28 旁路泄漏根修守点：退出助手还原面板快照 + 清工作区旗标。
        // （排 EditCell 关闭动作需要 palette——由调用方持有，此处钉还原语义。）
        let mut panels = crate::state::PanelState::default();
        panels.set_visible(crate::state::Panel::Inspector, false);
        panels.set_visible(crate::state::Panel::Brush, false);
        let backup = vec![
            (crate::state::Panel::Inspector, false),
            (crate::state::Panel::Brush, true),
        ];
        panels.cell_workspace_backup = Some(backup);
        exit_cellmap_workspace(&mut panels, None);
        assert!(
            panels.cell_workspace_backup.is_none(),
            "退出后工作区旗标必须落下（否则 Inspector 页签持续禁用）"
        );
        assert!(!panels.is_visible(crate::state::Panel::Inspector));
        assert!(panels.is_visible(crate::state::Panel::Brush));
        // 幂等：非工作区态再退出是无操作。
        exit_cellmap_workspace(&mut panels, None);
        assert!(panels.is_visible(crate::state::Panel::Brush));
    }
}
