//! UI 组件库（按职责拆分的子模块汇总，详见各子文件头）。
//!
//! ## 职责
//! - 顶栏、状态栏、项目菜单、命令面板窗口、时间轴、笔刷、小地图、罗盘、
//!   检视器、势力、校验等浮窗面板。
//! - 共享 widget helper（`badge` / `empty_state` / `list_item` / `float_window` 等）。
//!
//! ## 边界（不做什么）
//! - **不分发命令**——单一入口 [`crate::ui_shell::apply_pending_commands`]；本模块只
//!   把"用户选的命令"通过 `CommandPalette::pending` 排队，由分发器统一处理。
//! - **不装配 Bevy App**——那是 [`crate::app_bootstrap`] 与 [`crate::ui_shell`]。
//!
//! ## 路径兼容
//! 所有 `pub` 符号在下方 `pub use` 重新导出，外部对
//! `crate::ui::UiShell` / `crate::ui::ProjectMenuState` /
//! `crate::ui::command_palette` / `crate::ui::BadgeKind` 等的引用零改动。

#![cfg(feature = "app")]

pub mod brush_panel;
pub mod cell_pick;
pub mod city_intent;
pub mod command_palette_window;
pub mod continent_dialog;
pub mod faction_divide_dialog;
pub mod faction_panel;
pub mod feature_place_dialog;
pub mod hover_tooltip;
pub mod inspector;
pub mod intent_menu;
pub mod landmark_panel;
pub mod layers_panel;
pub mod line_poi_panel;
pub mod list_panels_panel;
pub mod map_controls;
pub mod map_properties_dialog;
pub mod map_scale_bar;
pub mod minimap_panel;
pub mod new_map_dialog;
pub mod new_project_dialog;
pub mod partition_edit;
pub mod partition_pen;
pub mod project_menu;
pub mod region_panel;
pub mod settings_dialog;
pub mod shell;
pub mod situation_panel;
pub mod timeline;
pub mod top_bar;
pub mod top_option_bar;
pub mod validation_panel;
pub mod welcome_screen;
pub mod widget_preview;
pub mod widgets;

// ── 路径兼容 re-export（保持 `crate::ui::X` 不变） ───────────────────────
pub use brush_panel::panel_brush;
pub use command_palette_window::command_palette;
pub use continent_dialog::{continent_dialog, continent_dialog_system, ContinentDialog};
pub use faction_divide_dialog::{faction_divide_dialog, FactionDivisionDialog};
pub use faction_panel::panel_faction;
pub use feature_place_dialog::{feature_place_dialog, FeaturePlaceDialog};
pub use inspector::cell::cell_form;
pub use inspector::panel_inspector;
pub use intent_menu::intent_menu;
pub use landmark_panel::panel_landmark;
pub use layers_panel::{panel_layers, panel_map_tree};
pub use line_poi_panel::panel_line_poi;
pub use list_panels_panel::{panel_character, panel_event};
pub use map_controls::map_controls;
pub use map_properties_dialog::{map_properties_dialog, MapPropertiesDialog};
pub use minimap_panel::panel_minimap;
pub use new_map_dialog::{new_map_dialog, NewMapDialog};
pub use new_project_dialog::{
    new_project_apply_create, new_project_dialog, NewProjectDialog, NewProjectIntent,
};
pub use project_menu::ProjectMenuState;
pub use region_panel::panel_region;
pub use settings_dialog::{settings_dialog, SettingsDialog};
pub use shell::{terrain_name_at, terrain_swatch, UiShell};
pub use situation_panel::panel_situation;
pub use timeline::{
    panel_timeline, pause_playback_off_browse, undo_last_street_segment, TimelineLayer,
};
pub use top_bar::{status_bar, top_bar, ValidationCount};
pub use top_option_bar::top_option_bar;
pub use validation_panel::panel_validation;
pub use welcome_screen::welcome_screen;
pub use widgets::{
    badge,
    empty_state,
    float_window,
    list_item,
    setup_egui_image_loaders,
    slider,
    slider_row,
    BadgeKind,
    DialogWindow,
    EguiImageLoadersInstalled,
    FieldInput,
    FieldSelect,
    Switch,
    ToolButton,
    ToolButtonContent,
    ToolIcon,
    // v0.3 视觉重做新图标（Figma 5E8BCBCmtGAHqk3ORibBlU 配套）
    ICON_ALERT,
    ICON_BRUSH,
    ICON_CHECK,
    ICON_CHEVRON_DOWN,
    ICON_CHEVRON_RIGHT,
    ICON_COMMAND,
    ICON_COPY,
    ICON_ERASER,
    ICON_FOLDER_OPEN,
    ICON_GLOBE,
    ICON_GRID,
    ICON_HAND,
    ICON_HILLS,
    ICON_LAKE,
    ICON_LAYERS,
    ICON_LINE,
    ICON_MAGNET,
    ICON_MAP,
    ICON_MAP_PIN,
    ICON_MAXIMIZE,
    ICON_MINUS,
    ICON_MOON,
    ICON_MOUNTAIN_RANGE,
    ICON_MOUSE_POINTER,
    ICON_MOVE_HORIZONTAL,
    ICON_MOVE_VERTICAL,
    ICON_PAINT_BUCKET,
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
    ICON_SEARCH,
    ICON_SQUARE,
    ICON_SQUARE_MOUSE_POINTER,
    ICON_SUN,
    ICON_TREES,
    ICON_UNDO,
    ICON_VOLCANO,
    ICON_WAVES,
    ICON_X,
    ICON_ZOOM_IN,
    ICON_ZOOM_OUT,
};

// `terrain_grid` 原仅被 panel_brush 调用；2026-10-25 笔刷段移除后暂无调用方，
// 控件本体保留在 widgets（地形材料网格若回归可直接取用）。
pub use widgets::terrain_grid;
