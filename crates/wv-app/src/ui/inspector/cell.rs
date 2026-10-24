//! Cell 表单（统一 Inspector 的「属性」页签 · 地形格分支）。
//!
//! 从旧 `cell_editor_panel` 整块搬入（2026-10-22 统一 Inspector 设计，
//! Figma `99-293` 形态）。旧的是**独立浮窗**，现在是 Inspector 的一个分支。
//!
//! ## 写入路径：直提，不是 pending 链
//!
//! 旧代码里存在**两条**写 cell 的路径：
//!
//! 1. `EditCellState.pending` → `CommandId::CommitEditCell` → `EditCellCommand`
//!    ——**死代码**：`pending` 全仓无人写入，那条命令也没进命令面板表。
//! 2. `push_cell_edit` / `apply_batch` ——**活路径**：直接把命令提交进
//!    `History`。
//!
//! 本模块沿用**活路径**（并删掉死路径），并**补上地形纹理脏标记**：旧代码
//! 直提时**不**调 `TerrainTexture::mark_dirty`——命令落进撤销栈了，但地形
//! 纹理不知道要重绘，地图可能不刷新。dispatcher 那条路径反而是标了脏的。
//! 本模块统一为「提交成功 → 标脏 → 地图重绘」。
//!
//! ## 单格 / 批量两态
//!
//! 有多边形选区时可批量改 N 格。**两态互斥**——杜绝「以为在改整片、
//! 实际只改了一格」的歧义（[ADR-0040] 补 B）。
//!
//! 关联：[ADR-0040](../../../../docs/adr/ADR-0040-cell-inspector-tool.md)（决策 6
//! 被本设计修正——Cell 表单并入统一 Inspector）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::i18n::{t, I18nKey};
use crate::state::{SelectionState, WorldProjection};
use crate::theme::ui_tokens::font_size;

use crate::ui::shell::UiShell;
use crate::ui::widgets::{
    divider, empty_state, icon_image, value_box, ButtonContent, ButtonStyle, ICON_FOCUS,
    ICON_LOCK_KEYHOLE,
};
use wv_render::state::EditCellState;
use wv_ui::widgets::controls::{numeric_input_salted, radio};

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

/// 「圈完自动落批量」的边沿检测（2026-10-24）。
///
/// 多边形选区**从无到有**的那一帧把 [`CellPanelMode`] 切到 `Batch`——
/// CellMap 工作区的意图菜单已撤出，批量区是圈选确认后的唯一应用入口，
/// 作者不该再手点一次「批量」才看到读数。只做**边沿触发**：作者随后手动
/// 切回单格不会被每帧强按回去；下次重新圈选（region 从空变非空）才再触发。
#[derive(Debug, Default, bevy::prelude::Resource)]
pub struct BatchModeAutoLand {
    /// 上一帧是否有多边形选区。
    pub had_region: bool,
}
use wv_ui::widgets::segmented_control::segmented;

/// Cell 表单（Inspector「属性」页签 · 地形格分支）。
///
/// **何时出现**：`EditCell` 工具激活时（设计稿 §11 Q4——工具即模式）；
/// 2026-10-24 起CellMap 工作区里圈选确认后（V 工具激活）也直出——
/// 见 `mod.rs` 的 `cell_form_active` 谓词。
///
/// **单格 / 批量两态**：有多边形选区时可批量改 N 格；两态互斥，杜绝
/// 「以为在改整片、实际只改了一格」的歧义（[ADR-0040] 补 B）。
#[allow(clippy::too_many_arguments)]
pub fn cell_form(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    edit_cell: &mut EditCellState,
    edit: &mut crate::edit::editing::EditHistory,
    selection: &mut SelectionState,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    loc: crate::i18n::Locale,
    mode: &mut CellPanelMode,
    session: &mut wv_world_cell_map::cell_paint::CellPaintSession,
    flood: &mut wv_world_cell_map::state::FloodSelectState,
    derived: &mut crate::edit::derived_op::DerivedOpRequest,
    playback_playing: bool,
) {
    // ── 读数条（D8）：面板顶部固定双段 ──
    // 左段永远读 hover、右段永远读 selected；任一段无数据显 `—`，
    // **绝不回退错位**（把 selected 的值填到 hover 段会误导作者改错格）。
    {
        let world = &projection.world;
        let map = edit_cell.last_map_id.clone();
        let read_cell = |gx: i32, gy: i32| -> Option<(i16, u8, u8)> {
            let map = map.as_ref()?;
            let terrain = world.terrain_of(map)?;
            let w = terrain.width as i32;
            let h = terrain.height as i32;
            if gx < 0 || gy < 0 || gx >= w || gy >= h {
                return None;
            }
            let idx = (gy as usize) * (w as usize) + (gx as usize);
            let c = terrain.cells.get(idx)?;
            Some((c.height, c.surface, c.biome))
        };
        super::cell_paint_panel::render_readout_bar(ui, shell, edit_cell, read_cell);
    }

    // ── 泛洪判据区（D6）：仅 FloodSelect 工具激活时显示 ──
    // 判据是**工具参数**（改变泛洪行为），不是落笔值——故独立成段。
    if flood.tool_active {
        super::cell_paint_panel::render_flood_criterion_section(ui, shell, flood, loc);
        divider(ui, shell);
    }

    // ── 落笔值区（D4）：拖绘工作流的「颜料盒」──
    // 紧贴读数条之下——拖绘前必须能看见落笔值（§5 M2 验收 ④）。
    super::cell_paint_panel::render_paint_value_section(ui, shell, session, loc);
    divider(ui, shell);

    // ── 派生运算区（D6 · M3）：平滑 / 量化，作用范围 = 选中的 region_cells ──
    // 播放中按钮保持可见但禁用（§9.4 Gate-1：只挡写入，不挡只读查看）。
    super::cell_paint_panel::render_derived_section(
        ui,
        shell,
        derived,
        edit_cell.region_cells.len(),
        playback_playing,
    );
    divider(ui, shell);

    // ── 模式切换：单格 / 批量 ──
    // 有多边形选区时自动落在「批量」（用户刚圈完，意图明确）；
    // 用户可手动切回「单格」——两态互斥。
    let has_region = !edit_cell.region_cells.is_empty();
    let eff_mode = match *mode {
        CellPanelMode::Single => CellPanelMode::Single,
        CellPanelMode::Batch => {
            if has_region {
                CellPanelMode::Batch
            } else {
                CellPanelMode::Single
            }
        }
    };
    let mode_labels = [
        t(loc, I18nKey::CellEditorModeSingle),
        t(loc, I18nKey::CellEditorModeBatch),
    ];
    segmented(
        ui,
        shell,
        "cell_editor_mode",
        64.0,
        &mode_labels,
        match eff_mode {
            CellPanelMode::Single => 0,
            CellPanelMode::Batch => 1,
        },
        font_size::SMALL,
        |i| {
            *mode = if i == 0 {
                CellPanelMode::Single
            } else {
                CellPanelMode::Batch
            };
        },
    );
    ui.add_space(6.0);

    match eff_mode {
        CellPanelMode::Batch => {
            render_batch_section(ui, shell, projection, edit_cell, edit, selection, tex, loc);
        }
        CellPanelMode::Single => {}
    }

    // ── 单格：选中态 / 空态 ──
    if eff_mode == CellPanelMode::Single {
        match edit_cell.selected {
            Some((gx, gy)) => {
                let Some(before) = read_cell(projection, edit_cell, gx, gy) else {
                    return;
                };
                render_identity(ui, shell, gx, gy);
                divider(ui, shell);
                render_fields(
                    ui,
                    shell,
                    projection,
                    edit_cell,
                    edit,
                    (gx, gy),
                    before,
                    tex,
                    loc,
                );
                divider(ui, shell);
                // 尾注（Figma "Selection scope"）：focus 图标 + 提示。
                ui.horizontal(|ui| {
                    ui.add(icon_image(ICON_FOCUS, shell.t3(), 12.0));
                    ui.label(
                        egui::RichText::new(t(loc, I18nKey::CellEditorScopeHint))
                            .size(font_size::MICRO)
                            .color(shell.t3()),
                    );
                });
            }
            None => {
                empty_state(
                    ui,
                    shell,
                    "▣",
                    t(loc, I18nKey::CellEditorEmptyTitle),
                    t(loc, I18nKey::CellEditorEmptyHint),
                    "",
                );
            }
        }
    }
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
    tex: &mut wv_render::terrain_view::TerrainTexture,
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
        // salted id（2026-10-24 修复「点击后不停抖动」）：旧实现在控件内部
        // 用**栈地址**当盐——`height_f` 是每帧新建的局部，与批量区的 `delta`
        // 复用同一栈槽时两框同 id，TextEdit 互抢焦点表现为不停抖动。
        // salted 变体由调用方给语义名，跨帧稳定且互不冲突；外层 push_id
        // 已多余（控件内部已 push），一并移除。
        ui.allocate_ui(egui::vec2(168.0, 28.0), |ui| {
            numeric_input_salted(
                ui,
                shell,
                "cell_height",
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
    divider(ui, shell);
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
        push_cell_edit(edit, projection, edit_cell, (gx, gy), before, next, tex);
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
                ui.add(crate::ui::widgets::icon_image(
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
    tex: &mut wv_render::terrain_view::TerrainTexture,
    loc: crate::i18n::Locale,
) {
    let n = edit_cell.region_cells.len();
    // ── 头：取消按钮（格数在读数区显示，用 stats.count 真实值）──
    ui.horizontal(|ui| {
        if crate::ui::widgets::button(
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
    // 头部计数改用 stats.count（越界格跳过的真实格数，与读数一致）。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{} {}",
                stats.count,
                t(loc, I18nKey::BatchCellsCount)
            ))
            .size(font_size::SMALL)
            .color(shell.accent()),
        );
    });
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
    divider(ui, shell);
    ui.add_space(6.0);

    // ── 海拔：**只做增减**（相对量）──
    // 用户 2026-10-20 裁定：批量编辑只提供「整体加减多少米」，
    // **不提供「统一设为某值」**——绝对设值会让「这一片本来多高」的信息
    // 被覆盖掉，且与「读数区已显示平均海拔」重复。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorHeight))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        let mut delta = 0.0f32;
        // salted id（同上，2026-10-24 抖动修复）。
        ui.allocate_ui(egui::vec2(120.0, 28.0), |ui| {
            numeric_input_salted(
                ui,
                shell,
                "batch_height_delta",
                &mut delta,
                "m",
                10.0,
                -32767.0,
                32767.0,
            );
        });
        if crate::ui::widgets::button(
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
            apply_batch(projection, edit, &map, edit_cell, tex, |c| {
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
        if crate::ui::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::CellEditorWater)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            apply_batch(projection, edit, &map, edit_cell, tex, |c| {
                wv_core::terrain::TerrainCellData {
                    surface: wv_terrain::SURFACE_WATER,
                    ..c
                }
            });
        }
        if crate::ui::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::CellEditorLand)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            apply_batch(projection, edit, &map, edit_cell, tex, |c| {
                wv_core::terrain::TerrainCellData {
                    surface: wv_terrain::SURFACE_LAND,
                    ..c
                }
            });
        }
    });
    ui.add_space(8.0);
    divider(ui, shell);
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
    // **选区群系统一时亮对应 radio**（用户 2026-10-20 实机反馈：
    // 「统一改为草原,草原的 radio 并没有 checked」）。
    // 混合选区（全不亮）→ 点谁整片设为谁；统一后 → 该项自动亮起。
    let uniform_biome = if stats.biome_hist.len() == 1 {
        Some(stats.biome_hist[0].0)
    } else {
        None
    };
    for group in biome_groups() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (num, name) in group.iter() {
                if radio(ui, shell, uniform_biome == Some(*num), name).clicked() {
                    let biome = *num;
                    apply_batch(projection, edit, &map, edit_cell, tex, |c| {
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
    tex: &mut wv_render::terrain_view::TerrainTexture,
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
    let cmd = BatchEditCellsCommand::new(map.clone(), changes.clone());
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // 标脏：改了格，地形纹理要重绘。旧代码直提时**漏了这一步**
            // ——命令进了撤销栈，但地图可能不刷新（见模块文档）。
            mark_dirty_cells(tex, &changes);
        }
        Err(e) => bevy::log::warn!("批量改 cell 失败: {e}"),
    }
}

/// 把改动过的格子标脏（地形纹理按脏矩形增量 patch）。
fn mark_dirty_cells(
    tex: &mut wv_render::terrain_view::TerrainTexture,
    changes: &[wv_edit::command::CellChange],
) {
    if changes.is_empty() {
        return;
    }
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for c in changes {
        let gx = c.gx.max(0) as u32;
        let gy = c.gy.max(0) as u32;
        min_x = min_x.min(gx);
        min_y = min_y.min(gy);
        max_x = max_x.max(gx + 1);
        max_y = max_y.max(gy + 1);
    }
    tex.mark_dirty([min_x, min_y, max_x, max_y]);
}

/// 单格提交（stale 快照校验 + History 落盘；每格 1 command）。
///
/// 收 `&mut WorldProjection` 而非裸 `&mut World`：提交成功后要把
/// `projection.dirty` 置 true——顶栏的保存指示读它；漏了就是
/// 「改了格、指示仍显示已保存、关工程即丢」（final review I1）。
fn push_cell_edit(
    edit: &mut crate::edit::editing::EditHistory,
    projection: &mut WorldProjection,
    edit_cell: &EditCellState,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    after: wv_core::terrain::TerrainCellData,
    tex: &mut wv_render::terrain_view::TerrainTexture,
) {
    let world = &mut projection.world;
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
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // 标脏：改了格，地形纹理要重绘。旧代码直提时**漏了这一步**
            // ——命令进了撤销栈，但地图可能不刷新（见模块文档）。
            tex.mark_dirty([
                gx.max(0) as u32,
                gy.max(0) as u32,
                gx as u32 + 1,
                gy as u32 + 1,
            ]);
        }
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

    /// 构造一张干净的 4×4 地形纹理（测试用）。
    fn blank_texture() -> wv_render::terrain_view::TerrainTexture {
        wv_render::terrain_view::TerrainTexture {
            handle: bevy::prelude::Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            field_dirty: None,
            last_mode: None,
            last_soften: false,
            last_theme_id: "",
            last_map: None,
        }
    }

    /// **改了格必须标脏**——否则命令进了撤销栈，但地形纹理不知道要重绘，
    /// 地图不刷新（2026-10-22 统一 Inspector 补的洞；旧直提路径漏了这步）。
    #[test]
    fn single_cell_edit_marks_terrain_texture_dirty() {
        let (mut projection, map) = terrain_of(&[(1, 2, 100, 1)]);
        let mut edit = crate::edit::editing::EditHistory::default();
        let edit_cell = EditCellState {
            last_map_id: Some(map.clone()),
            ..Default::default()
        };
        let mut tex = blank_texture();
        let before = wv_core::terrain::TerrainCellData {
            height: 100,
            surface: 1,
            biome: 1,
        };
        let after = wv_core::terrain::TerrainCellData {
            height: 250,
            surface: 1,
            biome: 1,
        };

        assert!(!projection.dirty, "前置：初始未脏");
        push_cell_edit(
            &mut edit,
            &mut projection,
            &edit_cell,
            (1, 2),
            before,
            after,
            &mut tex,
        );

        assert_eq!(
            tex.dirty,
            Some([1, 2, 2, 3]),
            "改了格 (1,2) 必须标脏矩形 [1,2,2,3]——否则地图不重绘"
        );
        assert!(
            projection.dirty,
            "改了格必须置 projection.dirty——否则顶栏保存指示仍显示已保存，关工程即丢"
        );
    }

    /// 批量改格：标脏矩形覆盖**所有**改动格（取包围盒）。
    #[test]
    fn batch_cell_edit_marks_terrain_texture_dirty() {
        let (mut projection, map) = terrain_of(&[(0, 0, 10, 1), (2, 1, 20, 1)]);
        let mut edit = crate::edit::editing::EditHistory::default();
        let edit_cell = EditCellState {
            last_map_id: Some(map.clone()),
            region_cells: vec![(0, 0), (2, 1)],
            ..Default::default()
        };
        let mut tex = blank_texture();

        apply_batch(
            &mut projection,
            &mut edit,
            &map,
            &edit_cell,
            &mut tex,
            |c| wv_core::terrain::TerrainCellData {
                height: c.height + 5,
                ..c
            },
        );

        assert_eq!(
            tex.dirty,
            Some([0, 0, 3, 2]),
            "批量改了 (0,0) 与 (2,1) → 脏矩形应为包围盒 [0,0,3,2]"
        );
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
