//! 共享 widget helper 汇总入口。
//!
//! 历史背景：原文件 [`widgets.rs`](self) 长达 2193 行，覆盖 8 类相互独立的
//! 职责。读起来要在 `ui_theme` / `egui` / `LayoutFile` 之间来回切换，严重
//! 影响 grep 效率，也不利于单测就近放置。现拆为 6 个职责内聚的子模块：
//!
//! - [`primitives`] —— 徽标 / 空状态 / 列表项 / 描边 helper / 图标 tint
//!   中央化 / 最近工程行 + `time_ago` 格式化 。
//! - [`floater`] —— 浮动面板外壳：公共 [`FloatWindow`]（Figma `42:2` 提取，
//!   解耦 `Panel`）+ 自绘 panel-head。`Panel` 特化包装 `float_window` 留
//!   wv-app（ADR-0026 D3）。
//! - [`layer_panel`] —— 图层面板控件族（Figma `3:20892`）：`layer_panel` /
//!   `layer_list_row` / `opacity_row`，尺寸逐项对位设计稿。
//! - [`button`] —— 统一按钮工厂（多 style × 多 content 形态）+
//!   `ToolButton` 工具栏按钮封装 + 全部 `ICON_*` 常量。
//! - [`controls`] —— 表单 / 浮层控件：`FieldInput` / `FieldSelect` /
//!   `Switch` / `slider_row` / `PropertyRow`+`value_box`+`check_row`（检视器
//!   组件族）/ `DialogWindow`。
//! - [`svg_loader`] —— egui SVG 解码器的运行时安装（Bevy 资源 + image-loader
//!   marker）。
//!
//! ## 职责（与原 `widgets.rs` 顶部列项保持一致）
//! - `BadgeKind` + `badge` —— 药丸型徽标（中性 / 通过 / 警告 / 错误 / 事件）。
//! - `empty_state` —— "图标 + 一句人话 + 一个可点的下一步"占位卡。
//! - `list_item` —— 类型色块 + 主标题 + 等宽元信息。
//! - `FloatWindow` / `panel_head` —— 浮动面板外壳 + 自绘标题栏（`float_window`
//!   包装留 wv-app）。
//! - `ToolButton` —— **标准按钮**（标准化控件第 1 项）：图标 / 文字 / 图标+文字
//!   三形态 + tooltip + 选中/禁用态，默认样式由 `UiShell` 令牌解析。
//!
//! ## 边界（不做什么）
//! - **不**装配具体业务面板内容 —— 各 `*_panel.rs` 调用 `float_window` 装自己。
//! - **不**改主题/令牌 —— 调 `shell::UiShell` getter 拿颜色。
//!
//! ## 主要导出
//! 全部原 `widgets.rs` 的 `pub` 符号通过下方 `pub use` 重新导出，
//! `use super::widgets::{…}` / `crate::widgets::{…}` 路径**零改动**。
//!
//! ## 路径兼容
//! 所有 `pub` 符号在下方 `pub use` 重新导出，外部对
//! `crate::ui::BadgeKind` / `crate::ui::FieldInput` / `crate::ui::ToolButton` 等的引用零改动。

pub mod button;
pub mod controls;
pub mod floater;
pub mod layer_panel;
pub mod primitives;
pub mod segmented_control;
pub mod split_button;
pub mod svg_loader;
#[cfg(test)]
pub(crate) mod test_support;
pub mod zoom_controls;

// ── 路径兼容 re-export（保持 `crate::widgets::X` 不变） ───────────────
pub use button::{
    button, icon_button, toolbar_tool, ButtonContent, ButtonStyle, ToolButton, ToolButtonContent,
    ToolIcon,
};
pub use controls::{
    check_row, checkbox_state, dropdown_box, numeric_input, numeric_input_bounded, option_menu,
    progress, radio, search_input, slider, slider_row, slider_row_brush, slider_row_disabled, tabs,
    text_area, value_box, value_unit, CheckState, DialogWindow, FieldInput, FieldSelect,
    PropertyRow, SliderTone, Switch,
};
pub use floater::{FloatWindow, HeadIcon};
pub use layer_panel::{
    layer_actions, layer_list, layer_list_row, layer_panel, opacity_row, LayerEntry,
    LayerPanelAction, LayerPanelState, LAYER_BLOCK_GAP, LAYER_ROW_HEIGHT, LAYER_ROW_SPACING,
};
pub use primitives::{
    badge,
    breadcrumb,
    divider,
    empty_state,
    format_time_ago,
    icon_image,
    identity_header,
    inline_alert,
    keycap,
    layer_row,
    list_item,
    loading,
    menu_item,
    recent_row,
    rect_stroke,
    tree_row,
    AlertKind,
    BadgeKind, // 也由本模块直接重导出到 `crate::ui`
    MenuState,
    ICON_ALERT,
    ICON_BRUSH,
    ICON_CALENDAR,
    ICON_CHECK,
    ICON_CHEVRON_DOWN,
    ICON_CHEVRON_RIGHT,
    ICON_CLOCK,
    ICON_COMMAND,
    ICON_ELLIPSE,
    ICON_ERASER,
    ICON_EYE,
    ICON_EYE_OFF,
    ICON_FLAG,
    ICON_FOCUS,
    ICON_FOLDER_OPEN,
    ICON_FOLDER_PLUS,
    ICON_GLOBE,
    ICON_GRID,
    ICON_HAND,
    ICON_HILLS,
    ICON_IMAGE,
    ICON_LAKE,
    ICON_LANDMARK,
    ICON_LANGUAGES,
    ICON_LAYERS,
    ICON_LINE,
    ICON_LIST,
    ICON_LOCK,
    ICON_LOCK_KEYHOLE,
    ICON_MAGNET,
    ICON_MAP,
    ICON_MAP_PIN,
    ICON_MAXIMIZE,
    ICON_MOON,
    ICON_MOUNTAIN,
    ICON_MOUNTAIN_RANGE,
    ICON_MOUSE_POINTER,
    ICON_MOVE_HORIZONTAL,
    ICON_MOVE_VERTICAL,
    ICON_PEN,
    ICON_PENCIL,
    ICON_PLATEAU,
    ICON_PLAY,
    ICON_PLUS,
    ICON_REDO,
    ICON_REFRESH,
    ICON_RIVER,
    ICON_ROUTE,
    ICON_RULER,
    ICON_RULER_DIMENSION_LINE,
    ICON_SEARCH,
    ICON_SETTINGS,
    ICON_SLIDERS,
    ICON_SQUARE,
    ICON_SQUARES_UNITE,
    ICON_SQUARE_CHECK,
    ICON_SUN,
    ICON_TRASH,
    ICON_TREES,
    ICON_TYPE,
    ICON_UNDO,
    ICON_USER,
    ICON_VECTOR_POLYGON,
    ICON_VECTOR_SQUARE,
    ICON_VOLCANO,
    ICON_WAVES,
    ICON_ZOOM_IN,
    ICON_ZOOM_OUT,
    RECENT_ROW_H,
    RECENT_ROW_ICON,
    RECENT_ROW_PAD_X,
};
pub use split_button::{split_button, ChevronSide, SplitButtonMetrics, SplitMenuItem};
pub use svg_loader::{install_svg_decoder, setup_egui_image_loaders, EguiImageLoadersInstalled};
pub use zoom_controls::{scale_to_percent, zoom_controls, ZoomControlsState};
