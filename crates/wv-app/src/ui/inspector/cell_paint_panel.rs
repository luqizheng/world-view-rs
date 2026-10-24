//! CellMap 的**落笔值区** + **读数条**（[ADR-0052] D4 / D8 · §5 M2）。
//!
//! ## 为什么独立成一个模块
//!
//! D4 与 D8 是 CellMap 工作区的**第一屏内容**——拖绘工作流的核心：
//!
//! - **读数条**（D8）：面板顶部固定双段，**左段永远读 hover**、
//!   **右段永远读 selected**，任一段无数据显 `—`，**绝不回退错位**。
//! - **落笔值区**（D4）：拖绘时写进每一格的值。**地表 / 群系是绝对值，
//!   海拔是相对增量**——与单格段的「此格海拔」**分区标注**，不得无标签并排。
//!
//! ## D4 的两控件边界
//!
//! 单格段的「此格海拔」（绝对）改的是**这一格**；落笔值区的「落笔增减」
//! （相对）改的是**拖到的每一格**。二者语义不同，标题旁各自标注
//! 「（绝对 / 点选设定）」与「（相对 / 拖绘写入）」。

use bevy_egui::egui;

use crate::i18n::{t, I18nKey};
use crate::theme::ui_theme::proportional;
use crate::theme::ui_tokens::font_size;
use crate::ui::shell::UiShell;
use crate::ui::widgets::divider;

use wv_render::state::EditCellState;
use wv_world_cell_map::cell_paint::CellPaintSession;
use wv_world_cell_map::terrain_view::biome_raw::{BIOME_NAMES_ZH, BIOME_RAW_COLORS};

/// **读数条**（D8）。
///
/// 固定双段（hover | selected），等宽字体，微号字。
///
/// **绝不回退**：hover 无数据显 `—`，**不**拿 selected 的值填上去——
/// 作者看到「光标在这格」与「选中的是那格」是两件事，混淆会误改地形。
pub fn render_readout_bar(
    ui: &mut egui::Ui,
    shell: &UiShell,
    edit_cell: &EditCellState,
    read_cell: impl Fn(i32, i32) -> Option<(i16, u8, u8)>,
) {
    let hover_text = match edit_cell.hover {
        Some((gx, gy)) => match read_cell(gx, gy) {
            Some((h, s, b)) => format!(
                "{}  ·  {}  ·  {} m  ·  {}  ·  {}",
                gx,
                gy,
                h,
                biome_name(b),
                surface_name(s)
            ),
            None => "—".to_string(),
        },
        None => "—".to_string(),
    };
    let selected_text = match edit_cell.selected {
        Some((gx, gy)) => match read_cell(gx, gy) {
            Some((h, s, b)) => format!(
                "{}  ·  {}  ·  {} m  ·  {}  ·  {}",
                gx,
                gy,
                h,
                biome_name(b),
                surface_name(s)
            ),
            None => "—".to_string(),
        },
        None => "—".to_string(),
    };

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            egui::RichText::new("光标")
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        ui.label(
            egui::RichText::new(hover_text)
                .monospace()
                .size(font_size::MICRO)
                .color(shell.t1()),
        );
        // 竖分隔线（1×16）。
        //
        // egui 0.36 的 `Separator` 只有水平方向（无 `orientation` 方法），
        // 故自绘：分配 1×16 的矩形后用 `painter` 填色。
        let (sep, _) = ui.allocate_exact_size(egui::vec2(1.0, 16.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(sep, 0.0, shell.border_subtle());
        ui.label(
            egui::RichText::new("选中")
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        ui.label(
            egui::RichText::new(selected_text)
                .monospace()
                .size(font_size::MICRO)
                .color(shell.accent()),
        );
    });
    ui.add_space(6.0);
    divider(ui, shell);
}

/// **落笔值区**（D4）。
///
/// 三个控件：
///
/// | 控件 | 语义 | 类型 |
/// |---|---|---|
/// | 落笔地表 | **绝对**（水 / 陆） | segmented |
/// | 落笔群系 | **绝对** | swatch 网格 |
/// | 落笔增减 | **相对**（米，`0` = 不动） | 数值框（D1 scrub） |
///
/// 群系用 **swatch 网格**而非 radio：拖绘时要能**快速切落笔群系**，
/// swatch 直接显示色块（与底图同色 = 所见即所涂）。
pub fn render_paint_value_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    session: &mut CellPaintSession,
    loc: crate::i18n::Locale,
) {
    ui.label(
        egui::RichText::new("落笔值")
            .size(font_size::SMALL)
            .color(shell.t2()),
    );
    ui.label(
        egui::RichText::new("拖绘时写进每一格的值")
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(4.0);

    // ── 落笔地表（绝对）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("地表")
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        crate::ui::widgets::segmented_control::segmented(
            ui,
            shell,
            "cell_paint_surface",
            56.0,
            &[
                t(loc, I18nKey::CellEditorWater),
                t(loc, I18nKey::CellEditorLand),
            ],
            if session.paint.surface == wv_terrain::SURFACE_WATER {
                0
            } else {
                1
            },
            font_size::MICRO,
            |i| {
                session.paint.surface = if i == 0 {
                    wv_terrain::SURFACE_WATER
                } else {
                    wv_terrain::SURFACE_LAND
                };
            },
        );
    });
    ui.add_space(4.0);

    // ── 落笔群系（绝对）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("群系")
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        render_biome_swatch(ui, shell, &mut session.paint.biome);
    });
    ui.add_space(4.0);

    // ── 落笔增减（相对）──
    // **D4 的关键标注**：标题旁写「（相对）」——与单格段的「此格海拔（绝对）」
    // 语义不同，不得无标签并排。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("海拔增减（相对）")
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let mut delta = session.paint.height_delta as f32;
        ui.push_id("cell_paint_delta", |ui| {
            ui.allocate_ui(egui::vec2(120.0, 26.0), |ui| {
                wv_ui::widgets::controls::numeric_input_bounded(
                    ui,
                    shell,
                    &mut delta,
                    "m",
                    10.0,
                    -32767.0,
                    32767.0,
                );
            });
        });
        session.paint.height_delta = delta.clamp(-32767.0, 32767.0) as i16;
    });
    ui.add_space(4.0);
}

/// **泛洪判据**（D6）。
///
/// 判据属于「工具参数」而非「落笔值」——它改变的是**泛洪行为**（手势），
/// 不是笔触写进格子的值。故独立成段，仅 FloodSelect 工具激活时显示。
pub fn render_flood_criterion_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    flood: &mut wv_world_cell_map::state::FloodSelectState,
    loc: crate::i18n::Locale,
) {
    use wv_world_cell_map::state::FloodCriterion;

    ui.label(
        egui::RichText::new("泛洪判据")
            .size(font_size::SMALL)
            .color(shell.t2()),
    );
    ui.label(
        egui::RichText::new("「同类」怎么算")
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(4.0);

    // 三档判据：按地表 / 按群系 / 按海拔容差。
    let current = match flood.criterion {
        FloodCriterion::Surface => 0,
        FloodCriterion::Biome => 1,
        FloodCriterion::HeightTolerance(_) => 2,
    };
    crate::ui::widgets::segmented_control::segmented(
        ui,
        shell,
        "cell_flood_criterion",
        60.0,
        &[
            t(loc, I18nKey::CellFloodBySurface),
            t(loc, I18nKey::CellFloodByBiome),
            t(loc, I18nKey::CellFloodByHeight),
        ],
        current,
        font_size::MICRO,
        |i| {
            flood.criterion = match i {
                0 => FloodCriterion::Surface,
                1 => FloodCriterion::Biome,
                _ => {
                    // 切到「按海拔容差」时，保留旧容差（默认 10 m）。
                    let tol = match flood.criterion {
                        FloodCriterion::HeightTolerance(t) => t,
                        _ => 10,
                    };
                    FloodCriterion::HeightTolerance(tol)
                }
            };
        },
    );
    // 容差输入：仅「按海拔容差」选中时出现（沿用 D1 的 scrub 数值框）。
    if let FloodCriterion::HeightTolerance(tol) = flood.criterion {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("容差 ±")
                    .size(font_size::MICRO)
                    .color(shell.t3()),
            );
            let mut tol_f = tol as f32;
            ui.push_id("cell_flood_tolerance", |ui| {
                ui.allocate_ui(egui::vec2(120.0, 26.0), |ui| {
                    wv_ui::widgets::controls::numeric_input_bounded(
                        ui,
                        shell,
                        &mut tol_f,
                        "m",
                        5.0,
                        0.0,
                        32767.0,
                    );
                });
            });
            flood.criterion = FloodCriterion::HeightTolerance(tol_f.clamp(0.0, 32767.0) as i16);
        });
    }
    ui.add_space(4.0);
}

/// **群系 swatch 网格**（落笔群系选择器）。
///
/// 5 列 × N 行，每格 = 一个色块 + 群系名。色源 = `BIOME_RAW_COLORS`
/// （与 CellMap 底图**同一张表**——所见即所涂）。
///
/// 选中态：`gold` 描边 + `gold_soft` 底（与工具瓷砖 active 态同款视觉语言）。
fn render_biome_swatch(ui: &mut egui::Ui, shell: &UiShell, current: &mut u8) {
    egui::Grid::new("cell_paint_biome_swatch")
        .num_columns(5)
        .spacing(egui::vec2(4.0, 4.0))
        .show(ui, |ui| {
            for biome in 0u8..21 {
                let selected = *current == biome;
                let color = egui::Color32::from_rgb(
                    BIOME_RAW_COLORS[biome as usize][0],
                    BIOME_RAW_COLORS[biome as usize][1],
                    BIOME_RAW_COLORS[biome as usize][2],
                );
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(42.0, 26.0), egui::Sense::click());
                let fill = if selected {
                    shell.gold_soft()
                } else {
                    egui::Color32::TRANSPARENT
                };
                let stroke = if selected {
                    egui::Stroke::new(1.0, shell.gold())
                } else {
                    egui::Stroke::NONE
                };
                ui.painter()
                    .rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);
                // 色块（缩进一点，避免压描边）。
                let swatch = egui::Rect::from_min_size(
                    rect.min + egui::vec2(4.0, 4.0),
                    egui::vec2(12.0, 12.0),
                );
                ui.painter().rect_filled(swatch, 3.0, color);
                // 群系名（微号，截断到 4 字避免挤爆）。
                let name = biome_name(biome);
                let short: String = name.chars().take(3).collect();
                ui.painter().text(
                    egui::pos2(swatch.max.x + 3.0, swatch.center().y),
                    egui::Align2::LEFT_CENTER,
                    short,
                    proportional(font_size::MICRO),
                    if selected { shell.t1() } else { shell.t3() },
                );
                if resp.clicked() {
                    *current = biome;
                }
                resp.on_hover_text(name);
                // 5 列一行 → 每 5 个换行（用 `end_row` 手动断）。
                if (biome as usize + 1) % 5 == 0 {
                    ui.end_row();
                }
            }
        });
}

/// 群系编号 → 中文名（越界回落 `#N`）。
fn biome_name(biome: u8) -> String {
    BIOME_NAMES_ZH
        .get(biome as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("#{biome}"))
}

/// 地表编号 → 显示名。
fn surface_name(surface: u8) -> &'static str {
    if surface == wv_terrain::SURFACE_WATER {
        "水"
    } else {
        "陆"
    }
}
