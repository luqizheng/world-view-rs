//! zh/en 文案查表。
//!
//! ## 纪律
//! **每个 `I18nKey` 变体必须**在 `lookup_zh` 与 `lookup_en` 中都有匹配 arm；
//! 漏写一处编译器立即报错（`non-exhaustive patterns`）——这是
//! 「无半翻译」契约的**唯一**强制点（ADR-0018 §2.1）。
//!
//! ## 不做什么
//! - **不插值**：`t()` 直返 `&'static str`；调用点若要拼参数自行 `format!`
//!   （避免引入 Fluent MessageFormat，AGENTS §9）。
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
        I18nKey::CommonDeleteWhole => "删除整个 {}",

        // ── 顶栏 ──
        I18nKey::TopBarUnsaved => "未保存",
        I18nKey::TopBarSaved => "已保存",
        I18nKey::TopBarUnsavedDot => "●",
        I18nKey::TopBarUnsavedSubtitle => "有未保存的改动",
        I18nKey::TopBarForceRedrawTooltip => "强制重画地形（F5）",
        I18nKey::TopBarNoProjectLoaded => "工程未加载",

        // ── 顶栏右侧计数 ──
        I18nKey::TopBarCounts => "Unit {} · 人物 {} · 事态 {} · 事件 {}",
        I18nKey::TopBarSelectedMap => "已选：{}",
        I18nKey::TopBarZoomScale => "缩放 {:.2}×",

        // ── 欢迎页 ──
        I18nKey::WelcomeScreenTitle => "World View",
        I18nKey::WelcomeOpenProjectBtn => "打开工程",
        I18nKey::WelcomeNewProjectBtn => "新建工程",
        I18nKey::WelcomeRecentEmpty => "（尚无最近工程）",
        I18nKey::WelcomeRecentHeading => "最近打开",

        // ── 新建工程对话框 ──
        I18nKey::NewProjectDialogTitle => "新建工程",
        I18nKey::NewProjectDialogDirLabel => "工程目录",
        I18nKey::NewProjectDialogNameLabel => "工程名称",
        I18nKey::NewProjectDialogAuthorLabel => "作者",
        I18nKey::NewProjectDialogDescriptionLabel => "描述",
        I18nKey::NewProjectDialogBrowseDir => "选择…",
        I18nKey::NewProjectDialogCreateBtn => "创建",

        // ── 新建地图对话框 ──
        I18nKey::NewMapDialogTitle => "新建地图",
        I18nKey::NewMapDialogIdLabel => "地图 ID",
        I18nKey::NewMapDialogNameLabel => "显示名称",
        I18nKey::NewMapDialogParentLabel => "父图 ID（留空 = 根图）",
        I18nKey::NewMapDialogOriginXLabel => "原点 X",
        I18nKey::NewMapDialogOriginYLabel => "原点 Y",
        I18nKey::NewMapDialogExtentLabel => "边长（wu）",
        I18nKey::NewMapDialogCreateBtn => "创建",
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
        I18nKey::CommonDeleteWhole => "Delete entire {}",

        // ── Top bar ──
        I18nKey::TopBarUnsaved => "Unsaved",
        I18nKey::TopBarSaved => "Saved",
        I18nKey::TopBarUnsavedDot => "●",
        I18nKey::TopBarUnsavedSubtitle => "There are unsaved changes",
        I18nKey::TopBarForceRedrawTooltip => "Force redraw terrain (F5)",
        I18nKey::TopBarNoProjectLoaded => "No project loaded",

        // ── Top bar status counts ──
        I18nKey::TopBarCounts => "Unit {} · Person {} · Situation {} · Event {}",
        I18nKey::TopBarSelectedMap => "Selected: {}",
        I18nKey::TopBarZoomScale => "Zoom {:.2}×",

        // ── Welcome screen ──
        I18nKey::WelcomeScreenTitle => "World View",
        I18nKey::WelcomeOpenProjectBtn => "Open project…",
        I18nKey::WelcomeNewProjectBtn => "New project…",
        I18nKey::WelcomeRecentEmpty => "(no recent projects)",
        I18nKey::WelcomeRecentHeading => "Recently opened",

        // ── New project dialog ──
        I18nKey::NewProjectDialogTitle => "New project",
        I18nKey::NewProjectDialogDirLabel => "Project directory",
        I18nKey::NewProjectDialogNameLabel => "Project name",
        I18nKey::NewProjectDialogAuthorLabel => "Author",
        I18nKey::NewProjectDialogDescriptionLabel => "Description",
        I18nKey::NewProjectDialogBrowseDir => "Browse…",
        I18nKey::NewProjectDialogCreateBtn => "Create",

        // ── New map dialog ──
        I18nKey::NewMapDialogTitle => "New map",
        I18nKey::NewMapDialogIdLabel => "Map ID",
        I18nKey::NewMapDialogNameLabel => "Display name",
        I18nKey::NewMapDialogParentLabel => "Parent map ID (empty = root)",
        I18nKey::NewMapDialogOriginXLabel => "Origin X",
        I18nKey::NewMapDialogOriginYLabel => "Origin Y",
        I18nKey::NewMapDialogExtentLabel => "Extent (wu)",
        I18nKey::NewMapDialogCreateBtn => "Create",
    }
}
