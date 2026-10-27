//! UI 组件画廊 —— `wv gallery` 子命令（**验收工具**，不是业务面板）。
//!
//! 按 Figma《公共组件目录》两版（Dark `60:7813` / Parchment `60:8390`）
//! 铺开 `wv-ui` 的公共控件：
//!
//! - **01 按钮与工具**：Primary / Secondary / Ghost / Destructive / Disabled /
//!   IconButton / ToolbarTool（默认 + 选中）。
//! - **02 选择与切换**：Checkbox 四态 / Radio / Switch 三态 / Segmented / Tabs。
//! - **03 输入与表单**：Input 五态 / SearchInput / NumericInput / Dropdown /
//!   OptionMenu / TextArea。
//! - **04 数值与颜色**：Slider 三变体 / ValueUnit / Progress。
//! - **05 覆盖与反馈**：TreeRow / LayerRow / MenuItem 四态 / Breadcrumb /
//!   ListItem / StatusPill / Keycap / InlineAlert / EmptyState / Loading。
//! - **06 面板与外壳**：PanelHeader / FloatingPanel / Divider / ZoomControl /
//!   Playback / EventRow / Theme & Density Selector。
//!
//! ## 主题
//!
//! 顶栏分段控件切换 Dark / Parchment；初始主题也可用 `WV_THEME=light`
//! （或 `parchment`）指定，便于分别截图。
//!
//! ## 截图
//!
//! `WV_SHOT=<png 路径>` 时第 120 帧落一张主窗口截图，不自动退出。
//!
//! ## 边界
//!
//! - 只读 `wv-ui` 与主题令牌；不碰世界数据 / 编辑入口。

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

use crate::theme::ui_theme::UiShellConfig;
use wv_ui::shell::UiShell;
use wv_ui::ui_tokens::{font_size, space, UiDensity, UiTheme};
use wv_ui::widgets::{
    badge, breadcrumb, button, checkbox_state, divider, dropdown_box, empty_state, icon_button,
    inline_alert, keycap, layer_panel, layer_row, list_item, loading, menu_item,
    numeric_input_salted, option_menu, progress, radio, search_input, segmented_control::segmented,
    slider_row, slider_row_brush, slider_row_disabled, tabs, text_area, toolbar_tool, tree_row,
    value_box, value_unit, AlertKind, BadgeKind, ButtonContent, ButtonStyle, CheckState,
    ColorPicker, DialogWindow, FieldInput, FieldSelect, FloatWindow, HeadIcon, LayerEntry,
    LayerPanelAction, LayerPanelState, MenuState, PropertyRow, Switch, ICON_BRUSH, ICON_CHECK,
    ICON_GRID, ICON_IMAGE, ICON_LANDMARK, ICON_LAYERS, ICON_MAP_PIN, ICON_MAXIMIZE, ICON_MOUNTAIN,
    ICON_MOUSE_POINTER, ICON_PLAY, ICON_PLUS, ICON_ROUTE, ICON_RULER, ICON_TRASH, ICON_TYPE,
    ICON_WAVES, ICON_ZOOM_IN as ICON_TOOL_ZOOM_IN, ICON_ZOOM_OUT as ICON_TOOL_ZOOM_OUT,
};

/// 首次运行时若给了 `WV_THEME`，用它决定初始主题（便于双主题截图）。
fn initial_theme() -> UiTheme {
    match std::env::var("WV_THEME") {
        Ok(v) if v.eq_ignore_ascii_case("light") || v.eq_ignore_ascii_case("parchment") => {
            UiTheme::Light
        }
        _ => UiTheme::Dark,
    }
}

/// 交互态（画廊自持，纯展示用）。
struct GalleryState {
    theme: UiTheme,
    density: UiDensity,
    name: String,
    mono: String,
    search: String,
    area: String,
    select: usize,
    seg: usize,
    tab: usize,
    radio: usize,
    switch_on: bool,
    switch_off: bool,
    linear: f32,
    log: f32,
    steps: u32,
    brush: f32,
    numeric: f32,
    dialog_open: bool,
    /// 图层样张：选中行 / 不透明度 / 可见性 / 锁定（纯展示）。
    layer_selected: usize,
    layer_opacity: f32,
    layer_visible: [bool; 6],
    layer_locked: [bool; 6],
    /// 取色器样张（04 分区）：带 alpha / 不带 alpha 各一枚（纯展示）。
    pick_color: egui::Rgba,
    pick_opaque: egui::Rgba,
}

impl Default for GalleryState {
    fn default() -> Self {
        Self {
            theme: initial_theme(),
            density: UiDensity::Cozy,
            name: "Government Office".to_owned(),
            mono: "1142.00 / 496.00".to_owned(),
            search: String::new(),
            area: "Regional government office.\nOpen daily from 14:00.\nCouncil meets at 14:40."
                .to_owned(),
            select: 0,
            seg: 1,
            tab: 0,
            radio: 1,
            switch_on: true,
            switch_off: false,
            linear: 0.70,
            log: 48.0,
            steps: 8,
            brush: 0.60,
            numeric: 842.0,
            dialog_open: false,
            layer_selected: 1,
            layer_opacity: 1.0,
            layer_visible: [true, true, true, true, true, false],
            layer_locked: [false, false, false, false, true, false],
            // 取色器样张初值：势力色系的一红一蓝（展示 hex 回写）。
            pick_color: egui::Rgba::from_srgba_unmultiplied(0xC0, 0x39, 0x2B, 0xE6),
            pick_opaque: egui::Rgba::from_srgba_unmultiplied(0x39, 0x7E, 0xB4, 255),
        }
    }
}

const SELECT_OPTIONS: [usize; 3] = [0, 1, 2];
const SELECT_LABELS: [&str; 3] = ["Government", "Market town", "Temple"];

/// 启动画廊窗口。
pub fn run() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "World View · wv-ui Catalogue".into(),
                    resolution: WindowResolution::new(1180, 940),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .set(bevy::log::LogPlugin {
                filter: "naga=warn,wgpu=error,icu_provider=off".into(),
                level: bevy::log::Level::INFO,
                ..Default::default()
            }),
    )
    .add_plugins(EguiPlugin::default())
    .insert_resource(ClearColor({
        let bg = wv_ui::ui_tokens::ShellPalette::DARK.bg_app;
        Color::srgb(bg.r, bg.g, bg.b)
    }))
    .init_resource::<UiShellConfig>()
    .add_systems(Startup, setup_camera)
    .add_systems(
        PreUpdate,
        (
            crate::theme::fonts::setup_egui_fonts,
            crate::ui::setup_egui_image_loaders,
        )
            .after(bevy_egui::EguiPreUpdateSet::InitContexts),
    )
    .add_systems(bevy_egui::EguiPrimaryContextPass, draw_gallery)
    .add_systems(Update, screenshot_probe);
    app.run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn section(ui: &mut egui::Ui, shell: &UiShell, title: &str, note: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(shell.scale.base)
            .color(shell.t1())
            .strong(),
    );
    if !note.is_empty() {
        ui.label(
            egui::RichText::new(note)
                .size(shell.scale.micro)
                .color(shell.t3()),
        );
    }
    ui.add_space(space::XS);
}

fn draw_gallery(mut contexts: EguiContexts, cfg: Res<UiShellConfig>, mut st: Local<GalleryState>) {
    let shell = UiShell::new(&UiShellConfig {
        theme: st.theme,
        density: st.density,
    });
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    wv_ui::ui_theme::apply_ui_theme(
        ctx,
        &UiShellConfig {
            theme: st.theme,
            density: st.density,
        },
    );

    let frame = egui::Frame::default()
        .fill(shell.palette.bg_app.to_color32())
        .inner_margin(wv_ui::ui_theme::margin_all(24.0));
    let screen = ctx.content_rect();
    egui::Window::new("wv_ui_catalogue_root")
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .collapsible(false)
        .fixed_pos(screen.min)
        .default_size(screen.size())
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size() - egui::vec2(70.0, 70.0));
            ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::XS);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("WORLD-VIEW / SHARED UI")
                            .size(font_size::MICRO)
                            .color(shell.accent()),
                    );
                    ui.label(
                        egui::RichText::new("公共组件目录")
                            .size(24.0)
                            .color(shell.t1())
                            .strong(),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 主题切换（目录两版）。
                    let labels = ["Dark", "Parchment"];
                    let active = if st.theme == UiTheme::Dark { 0 } else { 1 };
                    segmented(
                        ui,
                        &shell,
                        "gallery_theme",
                        84.0,
                        &labels,
                        active,
                        shell.scale.small,
                        |i| st.theme = if i == 0 { UiTheme::Dark } else { UiTheme::Light },
                    );
                });
            });
            ui.add_space(space::MD);
            divider(ui, &shell);
            ui.add_space(space::MD);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.columns(2, |cols| {
                    // ── 左列 01–03 ─────────────────────────────────────
                    let ui = &mut cols[0];
                    section(ui, &shell, "01 / 按钮与工具 Buttons", "Primary / Secondary / Ghost / Destructive / Disabled / Icon / Toolbar");
                    ui.horizontal(|ui| {
                        let _ = button(ui, &shell, ButtonContent::Text("Apply"), ButtonStyle::Accent, None, true);
                        let _ = button(ui, &shell, ButtonContent::Text("Edit details"), ButtonStyle::Raised, None, true);
                        let _ = button(ui, &shell, ButtonContent::Text("Cancel"), ButtonStyle::Ghost, None, true);
                    });
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        let _ = button(ui, &shell, ButtonContent::ImageText(ICON_TRASH, "Delete"), ButtonStyle::Destructive, None, true);
                        let _ = button(ui, &shell, ButtonContent::Text("Apply"), ButtonStyle::Accent, None, false);
                        let _ = icon_button(ui, &shell, ICON_PLUS, true);
                    });
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        let _ = toolbar_tool(ui, &shell, ICON_MOUSE_POINTER, "Select", false);
                        let _ = toolbar_tool(ui, &shell, ICON_BRUSH, "Raise", true);
                        let _ = toolbar_tool(ui, &shell, ICON_RULER, "Measure", false);
                    });
                    ui.add_space(space::LG);

                    section(ui, &shell, "02 / 选择与切换 Selection", "Checkbox 四态 / Radio / Switch / Segmented / Tabs");
                    checkbox_state(ui, &shell, CheckState::Unchecked, "Visible");
                    checkbox_state(ui, &shell, CheckState::Checked, "Visible");
                    checkbox_state(ui, &shell, CheckState::Indeterminate, "Mixed");
                    checkbox_state(ui, &shell, CheckState::Disabled, "Disabled");
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        if radio(ui, &shell, st.radio == 0, "World").clicked() {
                            st.radio = 0;
                        }
                        if radio(ui, &shell, st.radio == 1, "Local").clicked() {
                            st.radio = 1;
                        }
                    });
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        let _ = Switch::new(&mut st.switch_on).show(ui, &shell);
                        let _ = Switch::new(&mut st.switch_off).show(ui, &shell);
                        let mut disabled = false;
                        let _ = Switch::new(&mut disabled).enabled(false).show(ui, &shell);
                    });
                    ui.add_space(space::XS);
                    segmented(
                        ui, &shell, "gallery_seg", 64.0, &["Browse", "Edit"], st.seg,
                        shell.scale.small, |i| st.seg = i,
                    );
                    ui.add_space(space::XS);
                    if let Some(i) =
                        tabs(ui, &shell, "gallery_tabs", &["Terrain", "Objects", "Roads"], st.tab, &[2])
                    {
                        st.tab = i;
                    }
                    ui.add_space(space::LG);

                    section(ui, &shell, "03 / 输入与表单 Inputs", "Input 五态 / Search / Numeric / Dropdown / OptionMenu / TextArea");
                    let mut empty = String::new();
                    FieldInput::new(&mut empty, "Enter a place name…").fill_width(true).show(ui, &shell);
                    ui.add_space(space::XS);
                    FieldInput::new(&mut st.name, "Filled").fill_width(true).show(ui, &shell);
                    ui.add_space(space::XS);
                    FieldInput::new(&mut st.mono, "x / y")
                        .font_mono(true)
                        .fill_width(true)
                        .show(ui, &shell);
                    ui.add_space(space::XS);
                    let mut disabled = String::from("poi_government_001");
                    FieldInput::new(&mut disabled, "").enabled(false).fill_width(true).show(ui, &shell);
                    ui.add_space(space::XS);
                    let mut err = String::new();
                    FieldInput::new(&mut err, "Required").error(true).fill_width(true).show(ui, &shell);
                    ui.label(egui::RichText::new("名称不能为空，请输入地点名称。").size(font_size::MICRO).color(shell.danger()));
                    ui.add_space(space::XS);
                    let _ = search_input(ui, &shell, &mut st.search, "Find a place…", "Ctrl+K");
                    ui.add_space(space::XS);
                    let _ = numeric_input_salted(
                        ui,
                        &shell,
                        "gallery_numeric",
                        &mut st.numeric,
                        "m",
                        1.0,
                        f32::MIN,
                        f32::MAX,
                    );
                    ui.add_space(space::XS);
                    let _ = dropdown_box(ui, &shell, "Government");
                    ui.add_space(space::XS);
                    FieldSelect::new(&mut st.select, &SELECT_OPTIONS, "gallery_select")
                        .show(ui, &shell, |i| SELECT_LABELS[*i].to_owned());
                    ui.add_space(space::XS);
                    if let Some(i) = option_menu(
                        ui, &shell, "gallery_menu",
                        &["Government", "Settlement", "Custom type"], st.select, &[2],
                    ) {
                        st.select = i;
                    }
                    ui.add_space(space::XS);
                    let _ = text_area(ui, &shell, &mut st.area, 3);
                    ui.add_space(space::LG);

                    // ── 右列 04–06 ─────────────────────────────────────
                    let ui = &mut cols[1];
                    section(ui, &shell, "04 / 数值与颜色 Values", "Slider 三变体 / ValueUnit / Progress / ColorPicker");
                    slider_row(ui, &shell, "Opacity", &mut st.linear, 0.0..=1.0, |n| format!("{:.0}%", n * 100.0), false);
                    slider_row_brush(ui, &shell, "Radius", &mut st.log, 16.0..=600.0, |n| format!("{n:.0} px"), true);
                    slider_row_disabled(ui, &shell, "Softness", &mut st.brush, 0.0..=1.0, |n| format!("{:.0}%", n * 100.0), false);
                    slider_row(ui, &shell, "Shelf cells", &mut st.steps, 0..=32, |n| format!("{n:.0}"), false);
                    ui.add_space(space::SM);
                    value_unit(ui, &shell, "842", "m · Elevation");
                    ui.add_space(space::SM);
                    progress(ui, &shell, "Exporting terrain", 0.64);
                    progress(ui, &shell, "Terrain exported", 1.0);
                    ui.add_space(space::SM);
                    // ColorPicker：带 alpha / 不带 alpha 两枚触发器（浮层点开可验）。
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("ColorPicker")
                                .size(font_size::SMALL)
                                .color(shell.t2()),
                        );
                        let _ = ColorPicker::new("gallery_pick_alpha", &mut st.pick_color)
                            .show(ui, &shell);
                        let _ = ColorPicker::new("gallery_pick_opaque", &mut st.pick_opaque)
                            .with_alpha(false)
                            .show(ui, &shell);
                        ui.label(
                            egui::RichText::new(wv_ui::widgets::rgba_to_hex(st.pick_color))
                                .size(font_size::MICRO)
                                .color(shell.t3()),
                        );
                    });
                    ui.add_space(space::LG);

                    section(ui, &shell, "05 / 覆盖与反馈 Overlays", "TreeRow / LayerRow / MenuItem / Breadcrumb / ListItem / StatusPill / Keycap / InlineAlert / EmptyState / Loading");
                    tree_row(ui, &shell, 0, "Zunyi Prefecture", true, false);
                    tree_row(ui, &shell, 1, "Government Office", false, true);
                    ui.add_space(space::XS);
                    layer_row(ui, &shell, "Terrain", true, false);
                    layer_row(ui, &shell, "Roads", false, true);
                    ui.add_space(space::XS);
                    menu_item(ui, &shell, Some(ICON_MAP_PIN), "Default", MenuState::Default);
                    menu_item(ui, &shell, Some(ICON_MAP_PIN), "Selected", MenuState::Selected);
                    menu_item(ui, &shell, Some(ICON_MAP_PIN), "Checked", MenuState::Checked);
                    menu_item(ui, &shell, Some(ICON_MAP_PIN), "Disabled", MenuState::Disabled);
                    ui.add_space(space::XS);
                    breadcrumb(ui, &shell, &["World", "Guizhou", "Zunyi"]);
                    ui.add_space(space::XS);
                    let _ = list_item(ui, &shell, shell.accent(), "Zunyi", "Y214 · 12 events", true);
                    let _ = list_item(ui, &shell, shell.gold(), "Hailongtun", "Y214 · 3 events", false);
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        badge(ui, &shell, BadgeKind::Ok, "Active");
                        badge(ui, &shell, BadgeKind::Warn, "Paused");
                        badge(ui, &shell, BadgeKind::Err, "Error");
                        badge(ui, &shell, BadgeKind::Neutral, "12");
                        badge(ui, &shell, BadgeKind::Event, "2 active");
                    });
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        keycap(ui, &shell, "Space");
                        keycap(ui, &shell, "Shift");
                        keycap(ui, &shell, "→");
                    });
                    ui.add_space(space::XS);
                    inline_alert(ui, &shell, AlertKind::Success, "Saved locally");
                    ui.add_space(space::XS);
                    inline_alert(ui, &shell, AlertKind::Error, "Dangling reference: city_zunyi");
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        loading(ui, &shell);
                        let _ = empty_state(ui, &shell, "◎", "No results", "Try another search.", "Clear");
                    });
                    ui.add_space(space::LG);

                    section(ui, &shell, "06 / 面板与外壳 Chrome", "PanelHeader / FloatingPanel / LayerPanel / Divider / ZoomControl / Playback / EventRow / Selectors");
                    PropertyRow::new("Type", "Government").dropdown(true).show(ui, &shell);
                    ui.add_space(space::XS);
                    PropertyRow::new("Elevation", "842").suffix("m").show(ui, &shell);
                    ui.add_space(space::XS);
                    value_box(ui, &shell, "1847.50 / 623.75", Some("wu"), false);
                    ui.add_space(space::XS);
                    divider(ui, &shell);
                    ui.add_space(space::XS);
                    ui.horizontal(|ui| {
                        // ZoomControl：28×28 图标钮竖排。
                        ui.vertical(|ui| {
                            let _ = icon_button(ui, &shell, ICON_TOOL_ZOOM_IN, true);
                            let _ = icon_button(ui, &shell, ICON_TOOL_ZOOM_OUT, true);
                            let _ = icon_button(ui, &shell, ICON_MAXIMIZE, true);
                        });
                        ui.add_space(space::SM);
                        ui.vertical(|ui| {
                            // Playback：播放 + 键帽 + 时间。
                            ui.horizontal(|ui| {
                                let _ = icon_button(ui, &shell, ICON_PLAY, true);
                                keycap(ui, &shell, "Space");
                                ui.label(
                                    egui::RichText::new("Y214 D82 · 5 days/s")
                                        .size(font_size::MICRO)
                                        .color(shell.t2()),
                                );
                            });
                            ui.add_space(space::XS);
                            // EventRow：2px 色条 + 名称 + 跨度。
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(2.0, 18.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 0.0, shell.accent());
                                ui.label(egui::RichText::new("Railway crisis").size(font_size::MICRO).color(shell.t1()));
                                ui.label(egui::RichText::new("Y214 D40 — Y215 D12").size(font_size::MICRO).color(shell.t3()));
                            });
                            ui.add_space(space::XS);
                            // Selectors：主题 / 密度。
                            segmented(
                                ui, &shell, "gallery_density", 64.0, &["Cozy", "Compact"],
                                if st.density == UiDensity::Cozy { 0 } else { 1 },
                                shell.scale.micro,
                                |i| st.density = if i == 0 { UiDensity::Cozy } else { UiDensity::Compact },
                            );
                        });
                    });
                    ui.add_space(space::SM);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Open dialog")
                                    .color(shell.t1())
                                    .size(shell.scale.small),
                            )
                            .fill(shell.raised_bg())
                            .stroke(egui::Stroke::new(1.0, shell.border_subtle()))
                            .corner_radius(wv_ui::ui_tokens::radius::FIELD),
                        )
                        .clicked()
                    {
                        st.dialog_open = true;
                    }
                });
            });
        });

    // 浮窗样张：目录 `FloatingPanel`（260 宽 / r10 / border_strong）+ `PanelHeader`。
    FloatWindow::new("gallery_float", "Inspector")
        .icon(HeadIcon::Svg(ICON_GRID))
        .tag(Some((BadgeKind::Ok, "ACTIVE".to_owned())))
        .default_pos(egui::pos2(840.0, 560.0))
        .default_size(egui::vec2(288.0, 300.0))
        .show(ctx, &shell, |ui| {
            egui::Frame::default()
                .inner_margin(wv_ui::ui_theme::margin_all(12.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::SM;
                    tree_row(ui, &shell, 0, "Zunyi", true, false);
                    layer_row(ui, &shell, "Terrain", true, false);
                    menu_item(ui, &shell, Some(ICON_CHECK), "Verified", MenuState::Checked);
                    keycap(ui, &shell, "Ctrl+K");
                });
        });

    // 图层样张：Figma `3:20892` Layers 的逐项对位（2026-10-18）——
    // 动作行 `DRAW ORDER` + 6 行列表 + 分隔线 + 不透明度行。
    FloatWindow::new("gallery_layers", "Layers")
        .icon(HeadIcon::Svg(ICON_LAYERS))
        .default_pos(egui::pos2(500.0, 560.0))
        .default_size(egui::vec2(268.0, 324.0))
        .show(ctx, &shell, |ui| {
            let entries = vec![
                LayerEntry {
                    label: "Labels",
                    count: Some(128),
                    visible: st.layer_visible[0],
                    locked: st.layer_locked[0],
                    icon: ICON_TYPE,
                },
                LayerEntry {
                    label: "Settlements",
                    count: Some(42),
                    visible: st.layer_visible[1],
                    locked: st.layer_locked[1],
                    icon: ICON_LANDMARK,
                },
                LayerEntry {
                    label: "Roads & paths",
                    count: Some(86),
                    visible: st.layer_visible[2],
                    locked: st.layer_locked[2],
                    icon: ICON_ROUTE,
                },
                LayerEntry {
                    label: "Rivers & water",
                    count: Some(24),
                    visible: st.layer_visible[3],
                    locked: st.layer_locked[3],
                    icon: ICON_WAVES,
                },
                LayerEntry {
                    label: "Terrain",
                    count: Some(1),
                    visible: st.layer_visible[4],
                    locked: st.layer_locked[4],
                    icon: ICON_MOUNTAIN,
                },
                LayerEntry {
                    label: "Reference sketch",
                    count: Some(1),
                    visible: st.layer_visible[5],
                    locked: st.layer_locked[5],
                    icon: ICON_IMAGE,
                },
            ];
            let mut panel = LayerPanelState {
                actions_label: "DRAW ORDER · TOP TO BOTTOM",
                // gallery 展示完整设计稿形态（含新建钮）；接真实数据的
                // panel_layers 置 false。
                show_add: true,
                entries,
                selected: st.layer_selected,
                opacity: st.layer_opacity,
                opacity_label: "Opacity",
            };
            if let Some(action) = layer_panel(ui, &shell, &mut panel) {
                match action {
                    LayerPanelAction::Select(i) => st.layer_selected = i,
                    LayerPanelAction::ToggleVisible(i) => {
                        st.layer_visible[i] = !st.layer_visible[i]
                    }
                    LayerPanelAction::ToggleLocked(i) => st.layer_locked[i] = !st.layer_locked[i],
                    LayerPanelAction::NewGroup | LayerPanelAction::NewLayer => {}
                }
            }
            st.layer_opacity = panel.opacity;
        });

    if st.dialog_open {
        DialogWindow::new("Showcase dialog", &mut st.dialog_open)
            .width(420.0)
            .show(ctx, &shell, |ui| {
                ui.label(
                    egui::RichText::new("DialogWindow：Foreground 层 + 原生标题栏 ×。")
                        .size(shell.scale.small)
                        .color(shell.t2()),
                );
                ui.add_space(space::MD);
                ui.horizontal(|ui| {
                    let _ = button(
                        ui,
                        &shell,
                        ButtonContent::Text("Cancel"),
                        ButtonStyle::Ghost,
                        None,
                        true,
                    );
                    let _ = button(
                        ui,
                        &shell,
                        ButtonContent::Text("Confirm"),
                        ButtonStyle::Accent,
                        None,
                        true,
                    );
                });
            });
    }
    let _ = &cfg;
}

/// 截图探针（`WV_SHOT=<路径>` 时第 120 帧落盘一张，不自动退出）。
fn screenshot_probe(mut frames: Local<u32>, mut sent: Local<bool>, mut commands: Commands) {
    let Some(path) = std::env::var_os("WV_SHOT") else {
        return;
    };
    if *sent {
        return;
    }
    *frames += 1;
    if *frames >= 120 {
        *sent = true;
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(path));
        info!("[wv_catalogue] screenshot requested at frame {}", *frames);
    }
}
