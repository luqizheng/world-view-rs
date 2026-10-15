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

    // ── 顶栏右侧计数 ────────────────────────────────────────────
    /// 顶栏右侧「Unit {} · 人物 {} · 事态 {} · 事件 {}」
    TopBarCounts,
    /// 「已选：{}」当前地图标签
    TopBarSelectedMap,
    /// 「缩放 {:.2}×」
    TopBarZoomScale,

    // ── 欢迎页 ──────────────────────────────────────────────────
    /// 欢迎页标题（无工程）
    WelcomeScreenTitle,
    /// 欢迎页「打开工程」主按钮
    WelcomeOpenProjectBtn,
    /// 欢迎页「新建工程」次按钮
    WelcomeNewProjectBtn,
    /// 欢迎页最近工程为空时提示
    WelcomeRecentEmpty,
    /// 欢迎页最近工程段标题
    WelcomeRecentHeading,

    // ── 新建工程对话框 ──────────────────────────────────────────
    /// 对话框标题
    NewProjectDialogTitle,
    /// 「工程目录」字段标签
    NewProjectDialogDirLabel,
    /// 「工程名称」字段标签
    NewProjectDialogNameLabel,
    /// 「作者」字段标签
    NewProjectDialogAuthorLabel,
    /// 「描述」字段标签
    NewProjectDialogDescriptionLabel,
    /// 选择目录按钮
    NewProjectDialogBrowseDir,
    /// 「创建」按钮
    NewProjectDialogCreateBtn,
    /// 「取消」按钮（与 CommonCancel 重名复用）
    /// 留作未来区分占位：目前复用 CommonCancel

    // ── 新建地图对话框 ──────────────────────────────────────────
    /// 对话框标题
    NewMapDialogTitle,
    /// 「地图 ID」字段标签
    NewMapDialogIdLabel,
    /// 「地图名称」字段标签
    NewMapDialogNameLabel,
    /// 「父图 ID（留空 = 根图）」字段标签
    NewMapDialogParentLabel,
    /// 「原点 X」字段标签
    NewMapDialogOriginXLabel,
    /// 「原点 Y」字段标签
    NewMapDialogOriginYLabel,
    /// 「边长（wu）」字段标签
    NewMapDialogExtentLabel,
    /// 「创建」按钮
    NewMapDialogCreateBtn,

    // ── 通用：删除/确认 ──────────────────────────────────────────
    /// 「删除整个 {}」按钮红字
    CommonDeleteWhole,
}

impl I18nKey {
    /// 所有键的稳定数组，供测试遍历。
    pub const ALL: [I18nKey; 41] = [
        // 公共动作
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
        // 顶栏
        I18nKey::TopBarUnsaved,
        I18nKey::TopBarSaved,
        I18nKey::TopBarUnsavedDot,
        I18nKey::TopBarUnsavedSubtitle,
        I18nKey::TopBarForceRedrawTooltip,
        I18nKey::TopBarNoProjectLoaded,
        // 顶栏右侧计数
        I18nKey::TopBarCounts,
        I18nKey::TopBarSelectedMap,
        I18nKey::TopBarZoomScale,
        // 欢迎页
        I18nKey::WelcomeScreenTitle,
        I18nKey::WelcomeOpenProjectBtn,
        I18nKey::WelcomeNewProjectBtn,
        I18nKey::WelcomeRecentEmpty,
        I18nKey::WelcomeRecentHeading,
        // 新建工程对话框
        I18nKey::NewProjectDialogTitle,
        I18nKey::NewProjectDialogDirLabel,
        I18nKey::NewProjectDialogNameLabel,
        I18nKey::NewProjectDialogAuthorLabel,
        I18nKey::NewProjectDialogDescriptionLabel,
        I18nKey::NewProjectDialogBrowseDir,
        I18nKey::NewProjectDialogCreateBtn,
        // 新建地图对话框
        I18nKey::NewMapDialogTitle,
        I18nKey::NewMapDialogIdLabel,
        I18nKey::NewMapDialogNameLabel,
        I18nKey::NewMapDialogParentLabel,
        I18nKey::NewMapDialogOriginXLabel,
        I18nKey::NewMapDialogOriginYLabel,
        I18nKey::NewMapDialogExtentLabel,
        I18nKey::NewMapDialogCreateBtn,
        // 通用：删除/确认
        I18nKey::CommonDeleteWhole,
    ];
}
