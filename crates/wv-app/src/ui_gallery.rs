//! UI 组件画廊 —— `wv gallery` 子命令（**验收工具**，不是业务面板）。
//!
//! 起一个独立的 Bevy 窗口，把 `wv-ui` 的全部公共控件按真实样式铺一遍：
//! 按钮四档 / 文本框 / 下拉 / 分段控件 / 复选框 / 复选行 / 开关 / 滑条 /
//! 属性行 / 徽标 / 列表项 / 身份块 / 空状态 / 图标 / 浮窗 / 模态壳。
//!
//! ## 为什么单独一个入口
//!
//! 控件没有业务调用点之前（以及之后），要回答「它长什么样、组合起来协不协调」
//! 只能靠真渲染。编辑器内的 `WV_WIDGET_PREVIEW` 只能摆一块 Inspector 样张，
//! 且压在真实工程之上；画廊是**全量、可控、可截图**的验收载体。
//!
//! ## 截图
//!
//! `WV_SHOT=<png 路径>` 时第 120 帧落一张主窗口截图（与 `widget_preview` 同款
//! 探针），不自动退出——由外部脚本杀进程。
//!
//! ## 边界
//!
//! - 只读 `wv-ui` 与主题令牌；不碰世界数据 / 编辑入口。
//! - 文案以英文为主（组件名称即英文术语）；夹一条中文验证打包字体。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy_egui::{egui, EguiContexts, EguiPlugin};

use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::ShellPalette;
use wv_ui::shell::UiShell;
use wv_ui::ui_tokens::{font_size, space};
use wv_ui::widgets::{
    badge, button, check_row, divider, empty_state, field_checkbox, icon_image, identity_header,
    list_item, segmented_control::segmented, slider_row, value_box, BadgeKind, ButtonContent,
    ButtonStyle, DialogWindow, FieldInput, FieldSelect, FloatWindow, HeadIcon, PropertyRow, Switch,
    ToolButton, ToolIcon, ICON_BRUSH, ICON_CHECK, ICON_CHEVRON_DOWN, ICON_FOLDER_OPEN, ICON_GLOBE,
    ICON_GRID, ICON_LAYERS, ICON_MAP, ICON_MAP_PIN, ICON_MAXIMIZE, ICON_PEN, ICON_PENCIL,
    ICON_PLUS, ICON_RULER, ICON_SEARCH,
};

/// 交互态（画廊自持，纯展示用；不落盘、不接世界）。
struct GalleryState {
    name: String,
    mono: String,
    select: usize,
    seg: usize,
    cb_a: bool,
    cb_b: bool,
    check_row: bool,
    switch: bool,
    linear: f32,
    log: f32,
    steps: u32,
    dialog_open: bool,
}

impl Default for GalleryState {
    fn default() -> Self {
        Self {
            name: "Zunyi Prefecture".to_owned(),
            mono: "1142.00 / 496.00".to_owned(),
            select: 0,
            seg: 1,
            cb_a: true,
            cb_b: false,
            check_row: true,
            switch: true,
            linear: 0.42,
            log: 64.0,
            steps: 8,
            dialog_open: false,
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
                    title: "World View · wv-ui Gallery".into(),
                    resolution: WindowResolution::new(1180, 900),
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
        let bg = ShellPalette::DARK.bg_app;
        Color::srgb(bg.r, bg.g, bg.b)
    }))
    .init_resource::<UiShellConfig>()
    .add_systems(Startup, setup_camera)
    // 字体与图标 loader 必须排在 bevy_egui 建上下文之后（与 gui 主程序同口径）。
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

/// 小节标题 + 说明。
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

/// 横向铺开一组控件。
fn row(ui: &mut egui::Ui, shell: &UiShell, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::SM;
        add(ui);
    });
    let _ = shell;
    ui.add_space(space::LG);
}

fn draw_gallery(mut contexts: EguiContexts, cfg: Res<UiShellConfig>, mut st: Local<GalleryState>) {
    let shell = UiShell::new(&cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // 让 egui 原生控件（TextEdit / ComboBox / checkbox）也吃项目令牌。
    ctx.set_visuals(wv_ui::ui_theme::build_visuals(shell.palette));

    let frame = egui::Frame::default()
        .fill(shell.palette.bg_app.to_color32())
        .inner_margin(wv_ui::ui_theme::margin_all(20.0));
    // egui 0.36：`CentralPanel::show` 收 `&mut Ui`；整屏用无标题栏 Window
    // 铺底（与项目既有 `egui::Window` 用法一致）。
    let screen = ctx.content_rect();
    egui::Window::new("wv_ui_gallery_root")
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .collapsible(false)
        .fixed_pos(screen.min)
        .default_size(screen.size())
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size() - egui::vec2(60.0, 60.0));
            ui.spacing_mut().item_spacing = egui::vec2(space::SM, space::XS);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(
                    egui::RichText::new("wv-ui · Component Gallery")
                        .size(font_size::DISPLAY)
                        .color(shell.t1())
                        .strong(),
                );
                ui.label(
                    egui::RichText::new(
                        "ADR-0026 · 控件层实际渲染样式（界面控件画廊，中文验证打包字体）",
                    )
                    .size(shell.scale.small)
                    .color(shell.t3()),
                );
                ui.add_space(space::MD);
                divider(ui, &shell);
                ui.add_space(space::MD);

                ui.columns(2, |cols| {
                    // ── 左列 ──────────────────────────────────────────────
                    let ui = &mut cols[0];
                    section(ui, &shell, "Buttons", "button() 四档 + 禁用 + ToolButton");
                    row(ui, &shell, |ui| {
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::ImageText(ICON_PLUS, "Create"),
                            ButtonStyle::Accent,
                            None,
                            true,
                        );
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::ImageText(ICON_FOLDER_OPEN, "Open"),
                            ButtonStyle::Raised,
                            None,
                            true,
                        );
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::Text("Cancel"),
                            ButtonStyle::Ghost,
                            None,
                            true,
                        );
                    });
                    row(ui, &shell, |ui| {
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::Text("Learn more"),
                            ButtonStyle::Subtle,
                            None,
                            true,
                        );
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::Text("Disabled"),
                            ButtonStyle::Accent,
                            None,
                            false,
                        );
                    });
                    row(ui, &shell, |ui| {
                        let _ = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "Pen").show(
                            ui,
                            &shell,
                            || {},
                        );
                        let _ = ToolButton::icon(ToolIcon::Svg(ICON_RULER), "Ruler")
                            .active(true)
                            .show(ui, &shell, || {});
                        let _ = ToolButton::icon_text(ToolIcon::Svg(ICON_BRUSH), "Brush", "").show(
                            ui,
                            &shell,
                            || {},
                        );
                        let _ = ToolButton::text("Text", "").show(ui, &shell, || {});
                    });

                    section(
                        ui,
                        &shell,
                        "Text inputs",
                        "FieldInput：比例 / 等宽 / 占位空态",
                    );
                    FieldInput::new(&mut st.name, "Place name")
                        .fill_width(true)
                        .show(ui, &shell);
                    ui.add_space(space::XS);
                    FieldInput::new(&mut st.mono, "x / y")
                        .font_mono(true)
                        .fill_width(true)
                        .show(ui, &shell);
                    ui.add_space(space::XS);
                    let mut empty = String::new();
                    FieldInput::new(&mut empty, "Empty with hint…")
                        .fill_width(true)
                        .show(ui, &shell);
                    ui.add_space(space::LG);

                    section(
                        ui,
                        &shell,
                        "Select",
                        "FieldSelect（ComboBox 封装，需唯一 id_salt）",
                    );
                    FieldSelect::new(&mut st.select, &SELECT_OPTIONS, "gallery_select").show(
                        ui,
                        &shell,
                        |i| SELECT_LABELS[*i].to_owned(),
                    );
                    ui.add_space(space::LG);

                    section(ui, &shell, "Segmented", "segmented()：分段控件");
                    segmented(
                        ui,
                        &shell,
                        "gallery_seg",
                        76.0,
                        &["Day", "Event", "Person", "Century"],
                        st.seg,
                        shell.scale.small,
                        |i| st.seg = i,
                    );
                    ui.add_space(space::LG);

                    section(
                        ui,
                        &shell,
                        "Checkboxes",
                        "field_checkbox（原生薄壳）/ check_row（自绘行）",
                    );
                    row(ui, &shell, |ui| {
                        let _ = field_checkbox(ui, &shell, &mut st.cb_a, "Checked");
                        let _ = field_checkbox(ui, &shell, &mut st.cb_b, "Unchecked");
                    });
                    check_row(
                        ui,
                        &shell,
                        &mut st.check_row,
                        "Visible on map",
                        Some(ICON_MAP_PIN),
                    );
                    ui.add_space(space::LG);

                    section(ui, &shell, "Switch", "Switch：25×14 胶囊滑钮");
                    row(ui, &shell, |ui| {
                        let _ = Switch::new(&mut st.switch).show(ui, &shell);
                        let mut off = false;
                        let _ = Switch::new(&mut off).show(ui, &shell);
                    });

                    // ── 右列 ──────────────────────────────────────────────
                    let ui = &mut cols[1];
                    section(ui, &shell, "Sliders", "slider_row：线性 / 对数 / 整数泛型");
                    slider_row(
                        ui,
                        &shell,
                        "Softness",
                        &mut st.linear,
                        0.0..=1.0,
                        |n| format!("{n:.2}"),
                        false,
                    );
                    slider_row(
                        ui,
                        &shell,
                        "Radius",
                        &mut st.log,
                        16.0..=600.0,
                        |n| format!("{n:.0} wu"),
                        true,
                    );
                    slider_row(
                        ui,
                        &shell,
                        "Shelf cells",
                        &mut st.steps,
                        0..=32,
                        |n| format!("{n:.0}"),
                        false,
                    );
                    ui.add_space(space::LG);

                    section(
                        ui,
                        &shell,
                        "Property rows",
                        "value_box / PropertyRow（Figma 42:2）",
                    );
                    PropertyRow::new("Type", "Government")
                        .dropdown(true)
                        .show(ui, &shell);
                    ui.add_space(space::XS);
                    PropertyRow::new("Position", "1142.00 / 496.00").show(ui, &shell);
                    ui.add_space(space::XS);
                    PropertyRow::new("Elevation", "842")
                        .suffix("m")
                        .show(ui, &shell);
                    ui.add_space(space::XS);
                    value_box(ui, &shell, "1847.50 / 623.75", Some("wu"), false);
                    ui.add_space(space::LG);

                    section(ui, &shell, "Badges", "badge()：五档语义色");
                    row(ui, &shell, |ui| {
                        badge(ui, &shell, BadgeKind::Neutral, "12");
                        badge(ui, &shell, BadgeKind::Ok, "Saved locally");
                        badge(ui, &shell, BadgeKind::Warn, "Unsaved");
                        badge(ui, &shell, BadgeKind::Err, "Dangling ref");
                        badge(ui, &shell, BadgeKind::Event, "2 active");
                    });

                    section(ui, &shell, "Identity & list", "identity_header / list_item");
                    let _ = identity_header(
                        ui,
                        &shell,
                        shell.gold().gamma_multiply(0.35),
                        ICON_GLOBE,
                        "Government Office",
                        "Point of interest · Civic",
                        Some(ICON_MAP_PIN),
                    );
                    ui.add_space(space::SM);
                    let _ = list_item(
                        ui,
                        &shell,
                        shell.accent(),
                        "Zunyi",
                        "Y214 · 12 events",
                        true,
                    );
                    let _ = list_item(
                        ui,
                        &shell,
                        shell.gold(),
                        "Hailongtun",
                        "Y214 · 3 events",
                        false,
                    );
                    ui.add_space(space::LG);

                    section(
                        ui,
                        &shell,
                        "Icons (Lucide)",
                        "ICON_*：include_image! 编译期嵌入",
                    );
                    row(ui, &shell, |ui| {
                        for icon in [
                            ICON_MAP,
                            ICON_LAYERS,
                            ICON_BRUSH,
                            ICON_PENCIL,
                            ICON_SEARCH,
                            ICON_CHECK,
                            ICON_CHEVRON_DOWN,
                            ICON_MAXIMIZE,
                        ] {
                            ui.add(icon_image(icon, shell.t2(), 18.0));
                        }
                    });

                    section(
                        ui,
                        &shell,
                        "Modal & float",
                        "DialogWindow / FloatWindow（右上浮窗）",
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Open dialog")
                                    .color(shell.t1())
                                    .size(shell.scale.small),
                            )
                            .fill(shell.raised_bg())
                            .corner_radius(wv_ui::ui_tokens::radius::CONTROL),
                        )
                        .clicked()
                    {
                        st.dialog_open = true;
                    }
                    ui.add_space(space::LG);

                    section(
                        ui,
                        &shell,
                        "Empty state",
                        "empty_state()：图标 + 人话 + 一个下一步",
                    );
                    let _ = empty_state(
                        ui,
                        &shell,
                        "◎",
                        "No object selected",
                        "Click a map plot, event, or character\nto inspect properties.",
                        "Learn more",
                    );
                });
            });
        });

    // 浮动窗样张：标题栏 icon|title|…|⠿|▼ + 右下缩放角标。
    FloatWindow::new("gallery_float", "Inspector")
        .icon(HeadIcon::Svg(ICON_GRID))
        .tag(Some((BadgeKind::Ok, "ACTIVE".to_owned())))
        .default_pos(egui::pos2(860.0, 90.0))
        .default_size(egui::vec2(288.0, 300.0))
        .show(ctx, &shell, |ui| {
            egui::Frame::default()
                .inner_margin(wv_ui::ui_theme::margin_all(12.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = space::MD;
                    PropertyRow::new("Type", "Government")
                        .dropdown(true)
                        .show(ui, &shell);
                    PropertyRow::new("Elevation", "842")
                        .suffix("m")
                        .show(ui, &shell);
                    check_row(ui, &shell, &mut st.check_row, "Visible on map", None);
                });
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
        info!("[wv_gallery] screenshot requested at frame {}", *frames);
    }
}

// 窗口实体与 `PrimaryWindow` 的关系仅用于截图（`Screenshot::primary_window`）；
// 保留导入说明，防后续改成多窗口时漏改。
#[allow(unused_imports)]
use PrimaryWindow as _;
