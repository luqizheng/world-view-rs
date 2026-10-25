//! CellMap 的**落笔值区** + **读数条**（[ADR-0052] D4 / D8 · §5 M2）。
//!
//! ## 为什么独立成一个模块
//!
//! D4 与 D8 是 CellMap 工作区的**第一屏内容**——拖绘工作流的核心：
//!
//! - **读数条**（D8）：面板顶部固定双行，**上行永远读 hover**、
//!   **下行永远读 selected**，任一行无数据显 `—`，**绝不回退错位**。
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

use wv_render::state::{CellSelection, EditCellState};
use wv_world_cell_map::cell_paint::{CellPaintSession, MAX_STROKE_RADIUS};
use wv_world_cell_map::terrain_view::biome_raw::{BIOME_NAMES_ZH, BIOME_RAW_COLORS};

/// **读数条**（D8）。
///
/// 固定双行（上 hover / 下 选区），等宽字体，微号字。
/// 2026-10-25 用户裁定：原双段挤一行，窄面板里互相挤压，拆为两行。
///
/// **绝不回退**：hover 无数据显 `—`，**不**拿选区的值填上去——
/// 作者看到「光标在这格」与「选中的是那格」是两件事，混淆会误改地形。
///
/// ADR-0057 D1：下行读统一选区——**1 格**显该格读数（单格语义），
/// **N 格**显「N 格」（批量语义），空选区显 `—`。
pub fn render_readout_bar(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: crate::i18n::Locale,
    edit_cell: &EditCellState,
    sel: &CellSelection,
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
                surface_name(loc, s)
            ),
            None => "—".to_string(),
        },
        None => "—".to_string(),
    };
    let selected_text = match sel.only_cell() {
        Some((gx, gy)) => match read_cell(gx, gy) {
            Some((h, s, b)) => format!(
                "{}  ·  {}  ·  {} m  ·  {}  ·  {}",
                gx,
                gy,
                h,
                biome_name(b),
                surface_name(loc, s)
            ),
            None => "—".to_string(),
        },
        None if !sel.is_empty() => {
            t(loc, I18nKey::CellPaintReadoutCellCount).replace("{n}", &sel.len().to_string())
        }
        None => "—".to_string(),
    };

    // 上行：光标（hover）。
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellPaintReadoutCursor))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        ui.label(
            egui::RichText::new(hover_text)
                .monospace()
                .size(font_size::MICRO)
                .color(shell.t1()),
        );
    });
    ui.add_space(2.0);
    // 下行：选中（统一选区，ADR-0057 D1）。
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellPaintReadoutSelected))
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

/// **落笔表单字段**（[ADR-0052] D4 + ADR-0057 D9；ADR-0058 D2/D3 起为
/// 属性段「落笔」作用域的表单形态，段标题由作用域头提供）。
///
/// 四个控件：
///
/// | 控件 | 语义 | 类型 |
/// |---|---|---|
/// | 落笔地表 | **绝对**（水 / 陆） | segmented |
/// | 落笔群系 | **绝对** | swatch 网格 |
/// | 落笔增减 | **相对**（米，`0` = 不动） | 数值框（D1 scrub） |
/// | 笔刷半径 | 圆盘半径（格，1–8） | 数值框（D9） |
pub fn render_paint_fields(
    ui: &mut egui::Ui,
    shell: &UiShell,
    session: &mut CellPaintSession,
    loc: crate::i18n::Locale,
) {
    // ── 落笔地表（绝对）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorSurface))
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
            egui::RichText::new(t(loc, I18nKey::CellInspectorBiome))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let biome = session.paint.biome;
        render_biome_swatch(ui, shell, biome, |b| session.paint.biome = b);
    });
    ui.add_space(4.0);

    // ── 落笔增减（相对）──
    // **D4 的关键标注**：标题旁写「（相对）」——与单格段的「此格海拔（绝对）」
    // 语义不同，不得无标签并排。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellPaintHeightDelta))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let mut delta = session.paint.height_delta as f32;
        // salted id（2026-10-24 抖动修复）：栈地址盐会让两个每帧局部
        // 复用同一栈槽的输入框拿到同一 id，TextEdit 互抢焦点。
        ui.allocate_ui(egui::vec2(120.0, 26.0), |ui| {
            wv_ui::widgets::controls::numeric_input_salted(
                ui,
                shell,
                "cell_paint_delta",
                &mut delta,
                "m",
                10.0,
                -32767.0,
                32767.0,
            );
        });
        session.paint.height_delta = delta.clamp(-32767.0, 32767.0) as i16;
    });
    ui.add_space(4.0);

    // ── 笔刷半径（ADR-0057 D9）──
    // 圆盘 footprint 的半径：R=1 = 单格（原状），R=2 = 含对角 9 格。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellPaintBrushRadius))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let mut r = session.radius as f32;
        ui.allocate_ui(egui::vec2(120.0, 26.0), |ui| {
            wv_ui::widgets::controls::numeric_input_salted(
                ui,
                shell,
                "cell_paint_radius",
                &mut r,
                t(loc, I18nKey::CellPaintRadiusUnit),
                1.0,
                1.0,
                MAX_STROKE_RADIUS as f32,
            );
        });
        session.radius = r.round().clamp(1.0, MAX_STROKE_RADIUS as f32) as u8;
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
        egui::RichText::new(t(loc, I18nKey::CellFloodSectionTitle))
            .size(font_size::SMALL)
            .color(shell.t2()),
    );
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellFloodSectionHint))
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
                egui::RichText::new(t(loc, I18nKey::CellFloodTolerance))
                    .size(font_size::MICRO)
                    .color(shell.t3()),
            );
            let mut tol_f = tol as f32;
            // salted id（同上）。
            ui.allocate_ui(egui::vec2(120.0, 26.0), |ui| {
                wv_ui::widgets::controls::numeric_input_salted(
                    ui,
                    shell,
                    "cell_flood_tolerance",
                    &mut tol_f,
                    "m",
                    5.0,
                    0.0,
                    32767.0,
                );
            });
            flood.criterion = FloodCriterion::HeightTolerance(tol_f.clamp(0.0, 32767.0) as i16);
        });
    }
    ui.add_space(4.0);
}

/// **群系 7 组排列序**（ADR-0058 D5）：swatch 网格的展示顺序，全部引用
/// `wv_terrain` 冻结编号。这是全面板群系的**唯一**排列定义——落笔 / 单格 /
/// 批量三处共用。注意含**山地**（旧单格 radio 漏了它，本次补上）。
pub const BIOME_GROUP_ORDER: &[&[u8]] = &[
    &[
        wv_terrain::BIOME_FOREST,
        wv_terrain::BIOME_JUNGLE,
        wv_terrain::BIOME_TROPICAL_RAINFOREST,
    ],
    &[
        wv_terrain::BIOME_HILLS,
        wv_terrain::BIOME_MOUNTAINS,
        wv_terrain::BIOME_SAVANNA,
        wv_terrain::BIOME_PLAINS,
        wv_terrain::BIOME_GRASSLAND,
    ],
    &[
        wv_terrain::BIOME_GOBI,
        wv_terrain::BIOME_DESERT,
        wv_terrain::BIOME_SALT_FLAT,
        wv_terrain::BIOME_OASIS,
    ],
    &[
        wv_terrain::BIOME_SWAMP,
        wv_terrain::BIOME_WETLAND,
        wv_terrain::BIOME_TUNDRA,
        wv_terrain::BIOME_SNOWFIELD,
    ],
    &[wv_terrain::BIOME_OCEAN, wv_terrain::BIOME_LAKE],
    &[wv_terrain::BIOME_ROAD, wv_terrain::BIOME_RIVER],
    &[wv_terrain::BIOME_NONE],
];

/// **群系 swatch 网格**（ADR-0058 D5：全面板**唯一**的群系选择控件，
/// 落笔 / 单格 / 批量三处共用；回调式——选中态由调用方传入，点选经
/// `on_pick` 回给调用方按各自作用域的提交语义落盘）。
///
/// 5 列 × N 行，每格 = 色块 + 名（截断 3 字，hover 全名）。色源 =
/// `BIOME_RAW_COLORS`、名源 = `BIOME_NAMES_ZH`、序源 =
/// [`BIOME_GROUP_ORDER`]——所见即所涂，且与底图同表。
///
/// 选中态：`gold` 描边 + `gold_soft` 底（与工具瓷砖 active 态同款视觉语言）。
pub fn render_biome_swatch(
    ui: &mut egui::Ui,
    shell: &UiShell,
    selected: u8,
    mut on_pick: impl FnMut(u8),
) {
    egui::Grid::new("cell_biome_swatch")
        .num_columns(5)
        .spacing(egui::vec2(4.0, 4.0))
        .show(ui, |ui| {
            for group in BIOME_GROUP_ORDER {
                for &biome in group.iter() {
                    let is_selected = selected == biome;
                    let color = egui::Color32::from_rgb(
                        BIOME_RAW_COLORS[biome as usize][0],
                        BIOME_RAW_COLORS[biome as usize][1],
                        BIOME_RAW_COLORS[biome as usize][2],
                    );
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(42.0, 26.0), egui::Sense::click());
                    let fill = if is_selected {
                        shell.gold_soft()
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let stroke = if is_selected {
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
                    // 群系名（微号，截断到 3 字避免挤爆）。
                    let name = biome_name(biome);
                    let short: String = name.chars().take(3).collect();
                    ui.painter().text(
                        egui::pos2(swatch.max.x + 3.0, swatch.center().y),
                        egui::Align2::LEFT_CENTER,
                        short,
                        proportional(font_size::MICRO),
                        if is_selected { shell.t1() } else { shell.t3() },
                    );
                    if resp.clicked() {
                        on_pick(biome);
                    }
                    resp.on_hover_text(name);
                }
                ui.end_row();
            }
        });
}

/// 群系编号 → 中文名（越界回落 `#N`）。
pub(crate) fn biome_name(biome: u8) -> String {
    BIOME_NAMES_ZH
        .get(biome as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("#{biome}"))
}

/// 地表编号 → 显示名（i18n：复用 CellEditorWater / CellEditorLand）。
fn surface_name(loc: crate::i18n::Locale, surface: u8) -> &'static str {
    if surface == wv_terrain::SURFACE_WATER {
        t(loc, I18nKey::CellEditorWater)
    } else {
        t(loc, I18nKey::CellEditorLand)
    }
}

/// **派生运算区**（[ADR-0052] D6 · §5 M3）。
///
/// 两个按钮 + 各自参数，作用范围 = 统一选区（选中片）。
///
/// | 运算 | 参数 | 纯函数 |
/// |---|---|---|
/// | 平滑 | 强度 `0..=1` | `wv_terrain::cellops::smooth_cells` |
/// | 量化 | 步长（米） | `wv_terrain::cellops::quantize_heights` |
///
/// ## 为什么作用范围必须是选区
///
/// 对「全图」做平滑/量化会把作者没碰过的地方也改掉——这是**越权修改**。
/// 故选区为空时按钮禁用（不是静默无效）。
///
/// ## 播放中的门控（§9.4 Gate-1）
///
/// 按钮**保持可见但禁用**——ADR 明确「派生运算保持可见（只读态）」：
/// 播放中看参数是合法的，按下才该被挡。
pub fn render_derived_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: crate::i18n::Locale,
    request: &mut crate::edit::derived_op::DerivedOpRequest,
    region_count: usize,
    playback_playing: bool,
) {
    use crate::edit::derived_op::DerivedOp;

    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellDerivedSectionTitle))
            .size(font_size::SMALL)
            .color(shell.t2()),
    );
    let scope = if region_count == 0 {
        t(loc, I18nKey::CellDerivedScopeNone).to_string()
    } else {
        t(loc, I18nKey::CellDerivedScopeCount).replace("{n}", &region_count.to_string())
    };
    ui.label(
        egui::RichText::new(scope)
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(4.0);

    // 可点条件：与系统内门控**同源**（`derived_op_gate`）——两处各写一遍
    // 「能不能跑」的判据迟早漂移。
    let can_run = crate::edit::derived_op::derived_op_gate(playback_playing, region_count).is_ok();

    // ── 平滑 ──
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                can_run,
                egui::Button::new(t(loc, I18nKey::CellDerivedSmooth)),
            )
            .on_hover_text(t(loc, I18nKey::CellDerivedSmoothHint))
            .clicked()
        {
            request.request(DerivedOp::Smooth);
        }
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellDerivedStrength))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let mut strength_pct = request.smooth_strength * 100.0;
        // salted id（同上）。
        ui.allocate_ui(egui::vec2(110.0, 26.0), |ui| {
            wv_ui::widgets::controls::numeric_input_salted(
                ui,
                shell,
                "derived_smooth_strength",
                &mut strength_pct,
                "%",
                5.0,
                0.0,
                100.0,
            );
        });
        request.smooth_strength = (strength_pct / 100.0).clamp(0.0, 1.0);
    });
    ui.add_space(4.0);

    // ── 量化 ──
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                can_run,
                egui::Button::new(t(loc, I18nKey::CellDerivedQuantize)),
            )
            .on_hover_text(t(loc, I18nKey::CellDerivedQuantizeHint))
            .clicked()
        {
            request.request(DerivedOp::Quantize);
        }
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellDerivedStep))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let mut step = request.quantize_step_m as f32;
        // salted id（同上）。
        ui.allocate_ui(egui::vec2(110.0, 26.0), |ui| {
            wv_ui::widgets::controls::numeric_input_salted(
                ui,
                shell,
                "derived_quantize_step",
                &mut step,
                "m",
                5.0,
                1.0,
                32767.0,
            );
        });
        request.quantize_step_m = step.clamp(1.0, 32767.0) as i16;
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **ADR-0058 验收 A5**：群系 7 组序覆盖全部 21 个冻结编号，不重不漏。
    /// （旧单格 radio 的 7 组漏了「山地」，本测试防回归。）
    #[test]
    fn biome_group_order_covers_all_frozen_biomes_exactly_once() {
        let mut seen = [false; 21];
        for group in BIOME_GROUP_ORDER {
            assert!(!group.is_empty(), "空组无意义");
            for &b in group.iter() {
                assert!((b as usize) < 21, "编号 {b} 越界（冻结表共 21 项）");
                assert!(!seen[b as usize], "编号 {b} 重复入组");
                seen[b as usize] = true;
            }
        }
        let missing: Vec<usize> = seen
            .iter()
            .enumerate()
            .filter(|(_, &s)| !s)
            .map(|(i, _)| i)
            .collect();
        assert!(missing.is_empty(), "有冻结编号未入 7 组序：{missing:?}");
    }

    /// 名称与色表同长（swatch 按 BIOME_GROUP_ORDER 下标双读，错位即越界）。
    #[test]
    fn biome_name_and_color_tables_share_length() {
        assert_eq!(BIOME_NAMES_ZH.len(), BIOME_RAW_COLORS.len());
        for group in BIOME_GROUP_ORDER {
            for &b in group.iter() {
                assert!(biome_name(b) != format!("#{b}"), "编号 {b} 缺中文名");
            }
        }
    }
}
