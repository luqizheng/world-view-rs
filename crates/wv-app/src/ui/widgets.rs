//! 共享 widget helper 汇总入口。
//!
//! 历史背景：原文件 [`widgets.rs`](self) 长达 2193 行，覆盖 8 类相互独立的
//! 职责。读起来要在 `ui_theme` / `egui` / `LayoutFile` 之间来回切换，严重
//! 影响 grep 效率，也不利于单测就近放置。现拆为 5 个职责内聚的子模块：
//!
//! - [`primitives`] —— 徽标 / 空状态 / 列表项 / 描边 helper / 图标 tint
//!   中央化 / 最近工程行 + `time_ago` 格式化 / 笔刷面板用地形网格。
//! - [`floater`] —— 浮动面板外壳：公共 [`FloatWindow`]（Figma `42:2` 提取，
//!   解耦 `Panel`）+ `Panel` 特化薄包装 `float_window` + 自绘 panel-head。
//! - [`button`] —— 统一按钮工厂（多 style × 多 content 形态）+
//!   `ToolButton` 工具栏按钮封装 + 全部 `ICON_*` 常量。
//! - [`controls`] —— 表单 / 浮层控件：`FieldInput` / `FieldSelect` /
//!   `field_checkbox` / `Switch` / `slider_row` / `PropertyRow`+`value_box`+
//!   `check_row`（检视器组件族）/ `DialogWindow`。
//! - [`svg_loader`] —— egui SVG 解码器的运行时安装（Bevy 资源 + image-loader
//!   marker）。
//!
//! ## 职责（与原 `widgets.rs` 顶部列项保持一致）
//! - `BadgeKind` + `badge` —— 药丸型徽标（中性 / 通过 / 警告 / 错误 / 事件）。
//! - `empty_state` —— "图标 + 一句人话 + 一个可点的下一步"占位卡。
//! - `list_item` —— 类型色块 + 主标题 + 等宽元信息。
//! - `terrain_grid` —— 笔刷面板用的 2×5 地形色块网格（被 [`crate::ui::brush_panel`] 调用）。
//! - `float_window` / `panel_head` —— 浮动面板外壳 + 自绘标题栏三段。
//! - `ToolButton` —— **标准按钮**（标准化控件第 1 项）：图标 / 文字 / 图标+文字
//!   三形态 + tooltip + 选中/禁用态，默认样式由 `UiShell` 令牌解析。
//!
//! ## 边界（不做什么）
//! - **不**装配具体业务面板内容 —— 各 `*_panel.rs` 调用 `float_window` 装自己。
//! - **不**改主题/令牌 —— 调 `shell::UiShell` getter 拿颜色。
//!
//! ## 主要导出
//! 全部原 `widgets.rs` 的 `pub` 符号通过下方 `pub use` 重新导出，
//! `use super::widgets::{…}` / `crate::ui::widgets::{…}` 路径**零改动**。
//!
//! ## 路径兼容
//! 所有 `pub` 符号在下方 `pub use` 重新导出，外部对
//! `crate::ui::BadgeKind` / `crate::ui::FieldInput` / `crate::ui::ToolButton` 等的引用零改动。

#![cfg(feature = "app")]

pub mod button;
pub mod controls;
pub mod floater;
pub mod primitives;
pub mod segmented_control;
pub mod svg_loader;

// ── 路径兼容 re-export（保持 `crate::ui::widgets::X` 不变） ───────────────
pub use button::{button, ButtonContent, ButtonStyle, ToolButton, ToolButtonContent, ToolIcon};
pub use controls::{
    check_row, field_checkbox, slider, slider_row, value_box, DialogWindow, FieldInput,
    FieldSelect, PropertyRow, Switch,
};
pub use floater::{float_window, FloatWindow, HeadIcon};
pub use primitives::{
    badge,
    divider,
    empty_state,
    format_time_ago,
    icon_image,
    identity_header,
    list_item,
    recent_row,
    rect_stroke,
    terrain_grid,
    BadgeKind, // 也由本模块直接重导出到 `crate::ui`
    ICON_ALERT,
    ICON_BRUSH,
    ICON_CHECK,
    ICON_CHEVRON_DOWN,
    ICON_CHEVRON_RIGHT,
    ICON_COMMAND,
    ICON_ERASER,
    ICON_FOLDER_OPEN,
    ICON_GLOBE,
    ICON_GRID,
    ICON_HILLS,
    ICON_LAKE,
    ICON_LAYERS,
    ICON_LINE,
    ICON_MAP,
    ICON_MAP_PIN,
    ICON_MAXIMIZE,
    ICON_MOUNTAIN_RANGE,
    ICON_PEN,
    ICON_PENCIL,
    ICON_PLATEAU,
    ICON_PLAY,
    ICON_PLUS,
    ICON_REFRESH,
    ICON_RIVER,
    ICON_RULER,
    ICON_SEARCH,
    ICON_SQUARE,
    ICON_VOLCANO,
    ICON_ZOOM_IN,
    ICON_ZOOM_OUT,
    RECENT_ROW_H,
    RECENT_ROW_ICON,
    RECENT_ROW_PAD_X,
};
pub use svg_loader::{install_svg_decoder, setup_egui_image_loaders, EguiImageLoadersInstalled};
