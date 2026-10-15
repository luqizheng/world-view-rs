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
    // ── 通用：删除/确认 ──────────────────────────────────────────
}

impl I18nKey {
    /// 所有键的稳定数组，供测试遍历。
    pub const ALL: [I18nKey; 70] = [
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
    ];
}
