//! Cell 编辑器面板（[ADR-0040] · 2026-10-20 Figma `99-293` 对齐版）。
//!
//! ## 设计对照（Figma `world view rs` node `99-293` "Cell 编辑器 · 选中属性"）
//!
//! 288px 暗色浮动面板，自上而下：
//! 1. **Selected cell 头**：`float_window`（图标/标题/把手/折叠 chevron 已由壳画）
//!    + identity 两行（`Cell · X, Y` 主标 + `网格单元 · 已选中` 金色副标）；
//! 2. **位置**：heading「位置」+ 右侧 `lock-keyhole 只读坐标`；X/Y 两列
//!    Mono 标签 + [`value_box`] 只读值；
//! 3. **地表**：heading + 右侧 [`segmented`]（水 / 陆地，段宽 64）；
//! 4. **海拔**：heading + [`numeric_input_bounded`]（i16 米全域钳制，stepper
//!    步长 1）+ 下方范围提示；
//! 5. **群系**：heading「群系 / 互斥单选」+ 设计稿 7 组 [`radio`]
//!    （森林系/干旱系/湿寒系/高地草原系/水系/道路河流/无群系），选中即改；
//! 6. 尾注：`focus` 图标 + 「仅编辑当前选中的 cell」。
//!
//! 批量编辑区（多边形选区）保留在面板底部（divider 之下），设计稿未画、
//! 为 2026-10-20 批量需求的延续。
//!
//! 控件全部出自 wv-ui（用户 2026-10-20 要求）；缺的
//! [`numeric_input_bounded`] 已新建为公共组件。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState, SelectionState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::font_size;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{
    divider, empty_state, float_window, icon_image, value_box, ButtonContent, ButtonStyle,
    ICON_FOCUS, ICON_LOCK_KEYHOLE,
};
use wv_render::state::EditCellState;
use wv_ui::widgets::controls::{numeric_input_bounded, option_menu, radio};

/// Cell 编辑器的两个工作模式（[ADR-0040] 补 B）。
///
/// **两态互斥**——杜绝「以为在改整片、实际只改了一格」的歧义：
/// - `Single` —— 编辑当前点选的 1 格（`EditCellState.selected`）；
/// - `Batch`  —— 编辑多边形贴合的 N 格（`EditCellState.region_cells`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, bevy::prelude::Resource)]
pub enum CellPanelMode {
    /// 单格（默认）。
    #[default]
    Single,
    /// 批量（多边形选区）。
    Batch,
}
use wv_ui::widgets::segmented_control::segmented;

/// Cell 编辑器面板。
#[allow(clippy::too_many_arguments)]
pub fn panel_cell_editor(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut projection: ResMut<WorldProjection>,
    edit_cell: Res<EditCellState>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    // Bevy 单系统参数上限 16：把两个会话态折成一层元组（每层元组算 1 参）。
    (mut selection, mut cell_panel_mode): (ResMut<SelectionState>, ResMut<CellPanelMode>),
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let mut mode_pick = *cell_panel_mode;
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::CellEditor,
        None,
        current_loc.0,
        |ui| {
            // ── 模式切换：单格 / 批量（[ADR-0040] 补 B）──
            // 有多边形选区时自动落在「批量」（用户刚圈完，意图明确）；
            // 用户可手动切回「单格」——两态互斥，杜绝「以为在改整片、
            // 实际只改了一格」的歧义。
            let has_region = !edit_cell.region_cells.is_empty();
            let eff_mode = match mode_pick {
                CellPanelMode::Single => CellPanelMode::Single,
                CellPanelMode::Batch => {
                    if has_region {
                        CellPanelMode::Batch
                    } else {
                        CellPanelMode::Single
                    }
                }
            };
            // 「有选区但当前在单格态」→ 提示可切批量（不强制）。
            let mode_labels = [
                t(current_loc.0, I18nKey::CellEditorModeSingle),
                t(current_loc.0, I18nKey::CellEditorModeBatch),
            ];
            segmented(
                ui,
                &shell,
                "cell_editor_mode",
                64.0,
                &mode_labels,
                match eff_mode {
                    CellPanelMode::Single => 0,
                    CellPanelMode::Batch => 1,
                },
                font_size::SMALL,
                |i| {
                    mode_pick = if i == 0 {
                        CellPanelMode::Single
                    } else {
                        CellPanelMode::Batch
                    };
                },
            );
            ui.add_space(6.0);

            match eff_mode {
                CellPanelMode::Batch => {
                    render_batch_section(
                        ui,
                        &shell,
                        &mut projection,
                        &edit_cell,
                        &mut edit,
                        &mut selection,
                        current_loc.0,
                    );
                }
                CellPanelMode::Single => {}
            }

            // ── 单格：选中态 / 空态 ──
            if eff_mode == CellPanelMode::Single {
                match edit_cell.selected {
                    Some((gx, gy)) => {
                        let Some(before) = read_cell(&projection, &edit_cell, gx, gy) else {
                            return;
                        };
                        render_identity(ui, &shell, gx, gy);
                        divider(ui, &shell);
                        render_fields(
                            ui,
                            &shell,
                            &mut projection,
                            &edit_cell,
                            &mut edit,
                            (gx, gy),
                            before,
                            current_loc.0,
                        );
                        divider(ui, &shell);
                        // 尾注（Figma "Selection scope"）：focus 图标 + 提示。
                        // 禁止 cursor 手算 + ui.put（见 section_heading 注释）。
                        ui.horizontal(|ui| {
                            ui.add(icon_image(ICON_FOCUS, shell.t3(), 12.0));
                            ui.label(
                                egui::RichText::new(t(current_loc.0, I18nKey::CellEditorScopeHint))
                                    .size(font_size::MICRO)
                                    .color(shell.t3()),
                            );
                        });
                    }
                    None => {
                        empty_state(
                            ui,
                            &shell,
                            "▣",
                            t(current_loc.0, I18nKey::CellEditorEmptyTitle),
                            t(current_loc.0, I18nKey::CellEditorEmptyHint),
                            "",
                        );
                    }
                }
            }
        },
    );
    // 模式选择回写（面板内改的）。
    *cell_panel_mode = mode_pick;
}

/// identity 两行（Figma "Object identity"）：主标 `Cell · X, Y` + 金副标。
fn render_identity(ui: &mut egui::Ui, shell: &UiShell, gx: i32, gy: i32) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(format!("Cell · {gx}, {gy}"))
            .size(13.0)
            .color(shell.t1()),
    );
    ui.label(
        egui::RichText::new(t(crate::i18n::Locale::Zh, I18nKey::CellEditorSubtitle))
            .size(font_size::MICRO)
            .color(shell.accent()),
    );
    ui.add_space(4.0);
}

/// 设计稿单格四段（位置 / 地表 / 海拔 / 群系）。
#[allow(clippy::too_many_arguments)]
fn render_fields(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    edit_cell: &EditCellState,
    edit: &mut crate::edit::editing::EditHistory,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    loc: crate::i18n::Locale,
) {
    let mut next = before;
    ui.add_space(6.0);

    // ── 位置（只读）：heading + 🔒 只读坐标；X / Y 两列 ──
    section_heading(ui, shell, t(loc, I18nKey::CellInspectorPosition), true);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for (label, v) in [("X", gx), ("Y", gy)] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    egui::RichText::new(label)
                        .monospace()
                        .size(11.0)
                        .color(shell.t3()),
                );
                // value_box 吃 available_width:不限定宽会把 Y 列挤出面板。
                ui.allocate_ui(egui::vec2(96.0, 28.0), |ui| {
                    value_box(ui, shell, &v.to_string(), None, false);
                });
            });
        }
    });
    ui.add_space(8.0);

    // ── 地表：heading + 分段（水 / 陆地）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorSurface))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        segmented(
            ui,
            shell,
            "cell_editor_surface",
            64.0,
            &[
                t(loc, I18nKey::CellEditorWater),
                t(loc, I18nKey::CellEditorLand),
            ],
            if before.surface == wv_terrain::SURFACE_WATER {
                0
            } else {
                1
            },
            font_size::SMALL,
            |i| {
                next.surface = if i == 0 {
                    wv_terrain::SURFACE_WATER
                } else {
                    wv_terrain::SURFACE_LAND
                };
            },
        );
    });
    ui.add_space(8.0);

    // ── 海拔：heading + stepper 数值（i16 米全域钳制）+ 范围提示 ──
    let mut height_f = before.height as f32;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorHeight))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.allocate_ui(egui::vec2(168.0, 28.0), |ui| {
            numeric_input_bounded(
                ui,
                shell,
                &mut height_f,
                "m",
                1.0,
                i16::MIN as f32,
                i16::MAX as f32,
            );
        });
    });
    next.height = height_f.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellEditorHeightRange))
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(8.0);
    divider(ui, &shell);
    ui.add_space(6.0);

    // ── 群系：heading（互斥单选）+ 设计稿 7 组 radio ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorBiome))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellEditorBiomeExclusive))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
    });
    ui.add_space(4.0);
    for group in biome_groups() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (num, name) in group.iter() {
                if radio(ui, shell, next.biome == *num, name).clicked() {
                    next.biome = *num;
                }
            }
        });
        ui.add_space(4.0);
    }

    // ── 即改即提交（每格 1 command，[ADR-0040] §决策 8）──
    if next != before {
        push_cell_edit(
            edit,
            &mut projection.world,
            edit_cell,
            (gx, gy),
            before,
            next,
        );
    }
}

/// 段落 heading 行：左标题 + 右侧可选「🔒 只读」徽标（Figma Position heading）。
fn section_heading(ui: &mut egui::Ui, shell: &UiShell, title: &str, readonly_badge: bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        if readonly_badge {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let text = t(crate::i18n::Locale::Zh, I18nKey::CellEditorReadonly);
                ui.label(
                    egui::RichText::new(text)
                        .size(font_size::MICRO)
                        .color(shell.t3()),
                );
                // RTL 布局里第一个 add 的在最右 → label 先、icon 后 = 锁在文字左。
                // 禁止 ui.cursor() 手算 + ui.put:空 RTL 的 cursor.left 是未定义
                // (-inf/NaN),egui 0.36 断言 frame_rect is NaN 直接崩
                // (2026-10-20 实机「CellMap 一点击即退出」根因)。
                ui.add(super::widgets::icon_image(
                    ICON_LOCK_KEYHOLE,
                    shell.t3(),
                    10.0,
                ));
            });
        }
    });
}

/// 设计稿的群系 7 组（顺序、分组与 Figma `99-356` "Biome options" 一致）。
/// 每项 = (冻结编号, 显示名)；编号全部引用 `wv_terrain` 冻结常量，防手滑。
fn biome_groups() -> &'static [&'static [(u8, &'static str)]] {
    use wv_terrain::*;
    &[
        &[
            (BIOME_FOREST, "森林"),
            (BIOME_JUNGLE, "丛林"),
            (BIOME_TROPICAL_RAINFOREST, "热带雨林"),
        ],
        &[
            (BIOME_GOBI, "戈壁"),
            (BIOME_DESERT, "沙漠"),
            (BIOME_SALT_FLAT, "盐碱地"),
            (BIOME_OASIS, "绿洲"),
        ],
        &[
            (BIOME_SWAMP, "沼泽"),
            (BIOME_WETLAND, "湿地"),
            (BIOME_TUNDRA, "苔原"),
            (BIOME_SNOWFIELD, "雪原"),
        ],
        &[
            (BIOME_HILLS, "丘陵"),
            (BIOME_SAVANNA, "稀树草原"),
            (BIOME_PLAINS, "平原"),
            (BIOME_GRASSLAND, "草原"),
        ],
        &[(BIOME_OCEAN, "海洋"), (BIOME_LAKE, "湖泊")],
        &[(BIOME_ROAD, "道路"), (BIOME_RIVER, "河流")],
        &[(BIOME_NONE, "无群系")],
    ]
}

/// 批量区读数：N 格 / 平均海拔 / 群系分布（[ADR-0040] 补 A）。
///
/// 「这一片现在是什么」必须可读——否则用户只能盲改。
struct RegionStats {
    /// 选区格数。
    pub count: usize,
    /// 平均海拔（米，四舍五入）。
    pub avg_height: i32,
    /// 最高 / 最低海拔（米）。
    pub max_height: i32,
    pub min_height: i32,
    /// 群系分布：(编号, 出现次数)，按次数降序、编号升序。
    pub biome_hist: Vec<(u8, usize)>,
}

/// 统计选区当前值（读 world，不改）。
fn region_stats(
    projection: &WorldProjection,
    map: &wv_core::id::Id,
    cells: &[(i32, i32)],
) -> Option<RegionStats> {
    let terrain = projection.world.terrain_of(map)?;
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    let mut count = 0usize;
    let mut sum_h = 0i64;
    let mut max_h = i32::MIN;
    let mut min_h = i32::MAX;
    let mut hist: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for (gx, gy) in cells {
        if *gx < 0 || *gy < 0 || *gx >= w || *gy >= h {
            continue;
        }
        let idx = (*gy as usize) * (w as usize) + (*gx as usize);
        let Some(cell) = terrain.cells.get(idx) else {
            continue;
        };
        count += 1;
        sum_h += cell.height as i64;
        max_h = max_h.max(cell.height as i32);
        min_h = min_h.min(cell.height as i32);
        *hist.entry(cell.biome).or_insert(0) += 1;
    }
    if count == 0 {
        return None;
    }
    let mut biome_hist: Vec<(u8, usize)> = hist.into_iter().collect();
    biome_hist.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Some(RegionStats {
        count,
        avg_height: (sum_h / count as i64) as i32,
        max_height: max_h,
        min_height: min_h,
        biome_hist,
    })
}

/// 批量编辑区（多边形选区贴合 N 格）：
///
/// **读数**（补 A）：N 格 / 平均海拔 / 最高最低 / 群系分布；
/// **写入**：海拔整体设值、海拔增减（delta）、设水陆、设群系（7 组 radio，
/// 与单格布局同款）。
///
/// 每次应用 = 1 条 `BatchEditCellsCommand`（N 格 1 个撤销单位）。
#[allow(clippy::too_many_arguments)]
fn render_batch_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    edit_cell: &EditCellState,
    edit: &mut crate::edit::editing::EditHistory,
    selection: &mut SelectionState,
    loc: crate::i18n::Locale,
) {
    let n = edit_cell.region_cells.len();
    // ── 头：选区规模 + 取消按钮 ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} {}", n, t(loc, I18nKey::BatchCellsCount)))
                .size(font_size::SMALL)
                .color(shell.accent()),
        );
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::BatchCellsClear)),
            ButtonStyle::Ghost,
            None,
            true,
        )
        .clicked()
        {
            selection.cancel();
        }
    });
    ui.add_space(6.0);

    let Some(map) = edit_cell.last_map_id.clone() else {
        return;
    };
    // 无选区 → 批量态空态（提示先用 V 圈选）。
    if n == 0 {
        empty_state(
            ui,
            shell,
            "▦",
            t(loc, I18nKey::BatchCellsEmptyTitle),
            t(loc, I18nKey::BatchCellsEmptyHint),
            "",
        );
        return;
    }

    // ── 读数区（补 A）──
    let Some(stats) = region_stats(projection, &map, &edit_cell.region_cells) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::BatchStatsHeight))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.label(
            egui::RichText::new(format!(
                "{} m  ({} ~ {})",
                stats.avg_height, stats.min_height, stats.max_height
            ))
            .monospace()
            .size(font_size::MICRO)
            .color(shell.t1()),
        );
    });
    ui.add_space(4.0);
    // 群系分布读数：「森林 ×12 · 平原 ×4 · …」（超过 3 种显示「+N」）。
    let biome_text = {
        let mut parts: Vec<String> = stats
            .biome_hist
            .iter()
            .take(3)
            .map(|(b, c)| format!("{} ×{}", biome_label(*b), c))
            .collect();
        if stats.biome_hist.len() > 3 {
            parts.push(format!("+{}", stats.biome_hist.len() - 3));
        }
        parts.join(" · ")
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::BatchStatsBiome))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.label(
            egui::RichText::new(biome_text)
                .size(font_size::MICRO)
                .color(shell.t1()),
        );
    });
    ui.add_space(8.0);
    divider(ui, &shell);
    ui.add_space(6.0);

    // ── 海拔：整体设值（绝对）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorHeight))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        let mut abs = stats.avg_height as f32;
        ui.allocate_ui(egui::vec2(120.0, 28.0), |ui| {
            numeric_input_bounded(
                ui,
                shell,
                &mut abs,
                "m",
                10.0,
                i16::MIN as f32,
                i16::MAX as f32,
            );
        });
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::BatchCellsSet)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            let target = abs.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            apply_batch(projection, edit, &map, edit_cell, |c| {
                wv_core::terrain::TerrainCellData {
                    height: target,
                    ..c
                }
            });
        }
    });
    ui.add_space(4.0);
    // ── 海拔：增减（相对）──
    ui.horizontal(|ui| {
        let mut delta = 0.0f32;
        ui.allocate_ui(egui::vec2(120.0, 28.0), |ui| {
            numeric_input_bounded(ui, shell, &mut delta, "m", 10.0, -32767.0, 32767.0);
        });
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::BatchCellsApply)),
            ButtonStyle::Accent,
            None,
            true,
        )
        .clicked()
        {
            let d = delta as i32;
            apply_batch(projection, edit, &map, edit_cell, |c| {
                wv_core::terrain::TerrainCellData {
                    height: (c.height as i32 + d).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    ..c
                }
            });
        }
    });
    ui.add_space(8.0);

    // ── 地表：整体设水 / 陆 ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorSurface))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::CellEditorWater)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            apply_batch(projection, edit, &map, edit_cell, |c| {
                wv_core::terrain::TerrainCellData {
                    surface: wv_terrain::SURFACE_WATER,
                    ..c
                }
            });
        }
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::CellEditorLand)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            apply_batch(projection, edit, &map, edit_cell, |c| {
                wv_core::terrain::TerrainCellData {
                    surface: wv_terrain::SURFACE_LAND,
                    ..c
                }
            });
        }
    });
    ui.add_space(8.0);
    divider(ui, &shell);
    ui.add_space(6.0);

    // ── 群系：7 组 radio（与单格布局同款）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorBiome))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellEditorBiomeExclusive))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
    });
    ui.add_space(4.0);
    // 混合选区不显示选中态（radios 全不亮）——点谁就整片设为谁。
    for group in biome_groups() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (num, name) in group.iter() {
                if radio(ui, shell, false, name).clicked() {
                    let biome = *num;
                    apply_batch(projection, edit, &map, edit_cell, |c| {
                        wv_core::terrain::TerrainCellData { biome, ..c }
                    });
                }
            }
        });
        ui.add_space(4.0);
    }
}

/// 批量应用：从 world 读每格当前值 → 构造 changes → 1 条
/// `BatchEditCellsCommand`（N 格 1 个撤销单位）→ History 落盘。
fn apply_batch(
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    map: &wv_core::id::Id,
    edit_cell: &EditCellState,
    transform: impl Fn(wv_core::terrain::TerrainCellData) -> wv_core::terrain::TerrainCellData,
) {
    use wv_edit::command::{BatchEditCellsCommand, CellChange};
    let Some(terrain) = projection.world.terrain_of(map) else {
        return;
    };
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    let mut changes = Vec::new();
    for (gx, gy) in &edit_cell.region_cells {
        if *gx < 0 || *gy < 0 || *gx >= w || *gy >= h {
            continue;
        }
        let idx = (*gy as usize) * (w as usize) + (*gx as usize);
        let before = terrain.cells[idx];
        let after = transform(before);
        if after != before {
            changes.push(CellChange {
                gx: *gx,
                gy: *gy,
                before,
                after,
            });
        }
    }
    if changes.is_empty() {
        return;
    }
    let cmd = BatchEditCellsCommand::new(map.clone(), changes);
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => edit.record_dirty(dirty),
        Err(e) => bevy::log::warn!("批量改 cell 失败: {e}"),
    }
}

/// 单格提交（stale 快照校验 + History 落盘；每格 1 command）。
fn push_cell_edit(
    edit: &mut crate::edit::editing::EditHistory,
    world: &mut wv_core::World,
    edit_cell: &EditCellState,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    after: wv_core::terrain::TerrainCellData,
) {
    let Some(map) = edit_cell.last_map_id.clone() else {
        return;
    };
    let current = world.terrain_of(&map).and_then(|t| {
        let w = t.width as i32;
        let h = t.height as i32;
        if gx < 0 || gy < 0 || gx >= w || gy >= h {
            return None;
        }
        t.cells
            .get((gy as usize) * (w as usize) + (gx as usize))
            .copied()
    });
    if current != Some(before) {
        bevy::log::warn!("EditCell commit 拒收: cell ({gx}, {gy}) 当前值已不是面板上看到的值");
        return;
    }
    let cmd = wv_edit::command::EditCellCommand::new(map, gx, gy, before, after);
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(world, tx) {
        Ok(dirty) => edit.record_dirty(dirty),
        Err(e) => bevy::log::warn!("EditCell commit 失败: {e}"),
    }
}

/// 读选中格当前值（`EditCellState.last_map_id` 定图）。
fn read_cell(
    projection: &WorldProjection,
    edit_cell: &EditCellState,
    gx: i32,
    gy: i32,
) -> Option<wv_core::terrain::TerrainCellData> {
    let terrain = projection
        .world
        .terrain_of(edit_cell.last_map_id.as_ref()?)?;
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    if gx < 0 || gy < 0 || gx >= w || gy >= h {
        return None;
    }
    terrain
        .cells
        .get((gy as usize) * (w as usize) + (gx as usize))
        .copied()
}

/// 群系编号 → 显示名（读数用；未登记编号回落 `#N`）。
fn biome_label(num: u8) -> String {
    for group in biome_groups() {
        for (n, name) in group.iter() {
            if *n == num {
                return (*name).to_string();
            }
        }
    }
    format!("#{num}")
}

/// 批量下拉项标签（NONE + 冻结表 20 编号；下标即编号）。
fn biome_menu_labels() -> Vec<&'static str> {
    const NAMES: [&str; 21] = [
        "NONE",
        "森林",
        "平原",
        "草原",
        "戈壁",
        "沙漠",
        "沼泽",
        "湿地",
        "苔原",
        "雪原",
        "丛林",
        "热带雨林",
        "丘陵",
        "山地",
        "海洋",
        "湖泊",
        "河流",
        "绿洲",
        "稀树草原",
        "盐碱地",
        "道路",
    ];
    NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| if i == 0 { "NONE" } else { *n })
        .collect()
}

/// 下拉下标 → biome 编号。
fn menu_index_to_biome(index: usize) -> u8 {
    index as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造 4×4 地形：给定 (gx, gy) → (height, biome)。
    fn terrain_of(cells: &[(i32, i32, i16, u8)]) -> (WorldProjection, wv_core::id::Id) {
        let map = wv_core::id::Id::new_unchecked("world_main");
        let mut all = vec![
            wv_core::terrain::TerrainCellData {
                height: 0,
                surface: 1,
                biome: 0
            };
            16
        ];
        for (gx, gy, h, b) in cells {
            all[(*gy as usize) * 4 + *gx as usize] = wv_core::terrain::TerrainCellData {
                height: *h,
                surface: 1,
                biome: *b,
            };
        }
        let mut projection = WorldProjection::default();
        projection.world.set_terrain(
            map.clone(),
            wv_core::terrain::TerrainData {
                width: 4,
                height: 4,
                cells: all,
            },
        );
        (projection, map)
    }

    /// 批量读数：N 格 / 平均海拔 / 最高最低 / 群系分布（次数降序）。
    #[test]
    fn region_stats_summarizes_selection() {
        let (projection, map) = terrain_of(&[
            (0, 0, 100, 1), // 森林
            (1, 0, 200, 1), // 森林
            (2, 0, 300, 3), // 草原
            (3, 0, 400, 3), // 草原
        ]);
        let cells = vec![(0, 0), (1, 0), (2, 0), (3, 0)];
        let s = region_stats(&projection, &map, &cells).expect("stats");
        assert_eq!(s.count, 4);
        assert_eq!(s.avg_height, 250, "(100+200+300+400)/4");
        assert_eq!(s.min_height, 100);
        assert_eq!(s.max_height, 400);
        // 群系分布：森林 2 / 草原 2 → 同数按编号升序（森林=1 在前）
        assert_eq!(s.biome_hist, vec![(1, 2), (3, 2)]);
    }

    /// 越界格被跳过，不影响统计。
    #[test]
    fn region_stats_skips_out_of_bounds_cells() {
        let (projection, map) = terrain_of(&[(0, 0, 50, 1)]);
        let cells = vec![(0, 0), (99, 99)];
        let s = region_stats(&projection, &map, &cells).expect("stats");
        assert_eq!(s.count, 1, "越界格应被跳过");
        assert_eq!(s.avg_height, 50);
    }

    /// 空选区 → None（面板据此显示空态）。
    #[test]
    fn region_stats_returns_none_for_empty_selection() {
        let (projection, map) = terrain_of(&[]);
        assert!(region_stats(&projection, &map, &[]).is_none());
    }

    /// 群系分布按次数降序（多数在前）。
    #[test]
    fn region_stats_sorts_biome_hist_by_count_desc() {
        let (projection, map) = terrain_of(&[
            (0, 0, 0, 1), // 森林 ×1
            (1, 0, 0, 3), // 草原 ×2
            (2, 0, 0, 3),
        ]);
        let cells = vec![(0, 0), (1, 0), (2, 0)];
        let s = region_stats(&projection, &map, &cells).expect("stats");
        assert_eq!(s.biome_hist, vec![(3, 2), (1, 1)], "次数多的在前");
    }
}
