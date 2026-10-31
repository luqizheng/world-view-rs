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
//! ## 选区即编辑对象（ADR-0057 D1 / D6）
//!
//! ADR-0040 补 B 的「单格 / 批量两态互斥」已被 ADR-0057 **废止**：单格不再
//! 是独立模式，只是**恰好 1 格的选区**。表单按统一选区 `CellSelection` 的
//! 格数分派——0 格 → 空态；1 格 → 单格表单（绝对设值）；N 格 → 批量表单
//! （相对增减）。`CellPanelMode` / `BatchModeAutoLand` 随之退役。
//!
//! 关联：[ADR-0040](../../../../docs/adr/ADR-0040-cell-inspector-tool.md)（决策 6
//! 被本设计修正——Cell 表单并入统一 Inspector；补 B 被 ADR-0057 废止）、
//! [ADR-0057](../../../../docs/adr/ADR-0057-cellmap-unified-selection.md)。
//!
//! ## 地表 / 群系提交时机 = 显式「应用」（UX-017，修正「即改即提交」对地表 / 群系的适用）
//!
//! 2026-10-30 round-2 审计 UX-017：地表 / 群系点击此前每击一命令，点 N 格
//! 要 N 次 Ctrl+Z。现点击只写草稿（`EditCellState::form_draft`），「应用」
//! 才提交——1 次表单会话 = 1 个撤销单位。
//!
//! ## 海拔提交时机 = 手势完成帧（2026-10-25 P0-1 方案 A，修正上文「即改即提交」对海拔的适用）
//!
//! scrub 数值框拖动中**每 4px 写一次值**（wv-ui D1），若照搬「即改即提交」
//! 会把 1 个拖拽手势拆成一串撤销命令。单格海拔现走「拖动暂存 → 松手 /
//! Enter 落盘」：暂存值挂 `EditCellState::pending_height`（带坐标标签，
//! 换格 / 换图即失效），手势完成帧与地表 / 群系改动汇入**同一条**命令
//! （1 手势 = 1 撤销单位）。radio / segmented 是离散点击，维持「点击即提交」。
//!
//! ## 统一属性表单（ADR-0058，2026-10-25）——上文「分派」的呈现层重构
//!
//! 面板骨架改为四段：读数条（常驻）→ 泛洪判据（点选工具激活，ADR-0086）→
//! **属性段**（全浮窗唯一一份地表 / 群系 / 海拔控件）→ 派生运算（仅选区
//! 非空）。作用域由**激活工具**决定（D2，无模式控件）：`EditCell` 激活 =
//! 落笔作用域（ADR-0086 修正：笔刷调参在 topOptionBar，不再进本表单）；
//! 其余按选区分派 0 / 1 / N。
//! 单格「位置」只读区退役（D6，坐标入作用域头）；群系 radio 退役
//! （D5，与落笔共用 swatch）；海拔提交时机三档不变（D4）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::i18n::{t, I18nKey};
use crate::state::WorldProjection;
use crate::theme::ui_tokens::font_size;

use crate::ui::shell::UiShell;
use crate::ui::widgets::numeric_texts;
use crate::ui::widgets::{divider, empty_state, ButtonContent, ButtonStyle};
use wv_render::state::{CellSelection, EditCellState};
use wv_ui::widgets::controls::{numeric_input_salted, numeric_input_salted_report};

use wv_ui::widgets::segmented_control::segmented;

// 宿主变迁追记（ADR-0097，2026-10-31）：`cell_form`（读数条 + 0/1/N 分派
// 宿主）随「地形编辑器不要了」退役——单格/批量子件由 `ui::cell_edit_dialog`
// 直接复用（`render_fields` / `render_batch_section`，已 pub(crate)）；落笔
// 表单与泛洪判据留在笔刷面板（工具参数，非编辑器）。

/// 设计稿单格四段（位置 / 地表 / 海拔 / 群系）——ADR-0058 D6 后为三段
/// （位置退役，坐标在作用域头）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_fields(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    map: Option<&wv_core::Id>,
    edit: &mut crate::edit::editing::EditHistory,
    edit_cell: &mut EditCellState,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    loc: crate::i18n::Locale,
) {
    // UX-017：地表 / 群系改动先进草稿，「应用」显式提交（海拔手势帧提交
    // 时若草稿在则并成同一条命令）。换格 / 换图 = 草稿作废（坐标标签）。
    let draft_valid = edit_cell
        .form_draft
        .as_ref()
        .is_some_and(|d| d.cell == (gx, gy) && map.is_some_and(|m| d.map == m.as_str()));
    if !draft_valid {
        edit_cell.form_draft = None;
    }
    let mut next = before;
    if let Some(d) = edit_cell.form_draft.as_ref() {
        next.surface = d.surface;
        next.biome = d.biome;
    }
    ui.add_space(6.0);

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
            if next.surface == wv_terrain::SURFACE_WATER {
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
    //
    // **提交时机 = 手势完成帧**（2026-10-25 P0-1 方案 A）：scrub 拖动中
    // 每 4px 写一次值，若逐帧落盘会把 1 个拖拽手势拆成一串撤销命令。
    // 拖动中值进 `edit_cell.pending_height` 暂存（面板照常显示），松手 /
    // Enter 的那一帧才落盘成 1 条 `EditCellCommand`。radio / segmented
    // 是离散点击，仍「点击即提交」。
    let staged_height = match edit_cell.pending_height {
        // 手势中途换格 / 换图：暂存值作废（坐标标签失效判定）。
        Some((px, py, _)) if (px, py) != (gx, gy) => {
            edit_cell.pending_height = None;
            None
        }
        Some((_, _, h)) => Some(h),
        None => None,
    };
    let mut height_f = staged_height.unwrap_or(before.height) as f32;
    let mut report = None;
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
            report = Some(numeric_input_salted_report(
                ui,
                shell,
                "cell_height",
                numeric_texts(loc),
                &mut height_f,
                "m",
                1.0,
                i16::MIN as f32,
                i16::MAX as f32,
            ));
        });
    });
    let (_, report) = report.expect("horizontal 闭包必然执行一次");
    let scrubbed = height_f.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    // 本帧是否要把海拔落盘：`None` = 值仍在手势中或未变。
    let mut height_commit: Option<i16> = None;
    if report.gesture_active {
        // 拖动中 / 编辑中：只暂存，不落盘（`next.height` 保持 before——
        // 暂存值不进合并提交，否则又是逐帧刷命令）。
        edit_cell.pending_height = Some((gx, gy, scrubbed));
    } else {
        height_commit = height_commit_decision(report.changed, staged_height, scrubbed);
        edit_cell.pending_height = None;
    }
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellEditorHeightRange))
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(8.0);
    divider(ui, shell);
    ui.add_space(6.0);

    // ── 群系：heading（互斥单选）+ swatch（ADR-0058 D5：与落笔同款唯一
    //    控件；点选 = 改 next.biome，汇入本帧末尾的合并提交）──
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
    super::cell_paint_panel::render_biome_swatch(ui, shell, next.biome, |b| next.biome = b);

    // ── 提交时机（UX-017 修正 ADR-0040 决策 8 的适用面）──
    // 海拔：手势完成帧提交不变（P0-1 方案 A）；草稿若在同帧在场则并成
    // 同一条命令（1 手势 = 1 撤销单位）。
    // 地表 / 群系：点击只写草稿，**「应用」显式提交**——此前每击一命令，
    // 点 N 格要 N 次 Ctrl+Z，与批量区单命令粒度不一致。
    if let Some(h) = height_commit {
        next.height = h;
        push_cell_edit(edit, projection, map, (gx, gy), before, next, tex);
        edit_cell.form_draft = None;
        return;
    }
    let form_changed = next != before;
    if form_changed {
        edit_cell.form_draft = Some(wv_world_cell_map::state::CellFormDraft {
            map: map.map(|m| m.as_str().to_owned()).unwrap_or_default(),
            cell: (gx, gy),
            surface: next.surface,
            biome: next.biome,
        });
    }
    // 「应用」：草稿在场才可点（下一次点击也 refresh 草稿值）。
    ui.add_space(8.0);
    let apply = ui.add_enabled(
        form_changed,
        egui::Button::new(egui::RichText::new(t(loc, I18nKey::CellEditorApply)).strong()),
    );
    if apply.clicked() {
        push_cell_edit(edit, projection, map, (gx, gy), before, next, tex);
        edit_cell.form_draft = None;
    }
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
    sel: &CellSelection,
) -> Option<RegionStats> {
    let terrain = projection.world.terrain_of(map)?;
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    let mut count = 0usize;
    let mut sum_h = 0i64;
    let mut max_h = i32::MIN;
    let mut min_h = i32::MAX;
    let mut hist: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for (gx, gy) in sel.iter_cells() {
        if gx < 0 || gy < 0 || gx >= w || gy >= h {
            continue;
        }
        let idx = (gy as usize) * (w as usize) + (gx as usize);
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
/// 与单格布局同款）；
/// **生成**（2026-10-26）：「生成大陆」按钮——只开八参面板
/// （[`crate::ui::ContinentDialog`]），确认时经 `continent_dialog_system`
/// 从统一选区提取包络环走 `GenerateContinentCommand` 宏命令（1 撤销单位）。
/// 2026-10-24 裁定「工作台不出意图菜单」后，这就是统一选区的生成式出口。
///
/// 每次应用 = 1 条 `BatchEditCellsCommand`（N 格 1 个撤销单位）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_batch_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    loc: crate::i18n::Locale,
    cont: &mut crate::ui::ContinentDialog,
    sel: &mut CellSelection,
    map: Option<&wv_core::Id>,
    playback_playing: bool,
) {
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
            // 取消 = 清空统一选区（ADR-0057 D5：选区是会话态，不进撤销栈）。
            sel.clear();
        }
    });
    ui.add_space(6.0);

    let Some(map) = map else {
        return;
    };
    // 无选区 → 批量态空态（提示先用 V 圈选）。
    if sel.is_empty() {
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
    let Some(stats) = region_stats(projection, map, sel) else {
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
            .map(|(b, c)| format!("{} ×{}", super::cell_paint_panel::biome_name(*b), c))
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
                numeric_texts(loc),
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
            apply_batch(projection, edit, map, sel, tex, |c| {
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
            apply_batch(projection, edit, map, sel, tex, |c| {
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
            apply_batch(projection, edit, map, sel, tex, |c| {
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
    // **选区群系统一时亮对应 swatch**（用户 2026-10-20 实机反馈：
    // 「统一改为草原,草原的 radio 并没有 checked」——radio 已退役为
    // swatch，ADR-0058 D5，选中态口径沿用）。
    // 混合选区（全不亮）→ 点谁整片设为谁；统一后 → 该项自动亮起。
    let uniform_biome = if stats.biome_hist.len() == 1 {
        Some(stats.biome_hist[0].0)
    } else {
        None
    };
    super::cell_paint_panel::render_biome_swatch(
        ui,
        shell,
        uniform_biome.unwrap_or(u8::MAX),
        |b| {
            apply_batch(projection, edit, map, sel, tex, |c| {
                wv_core::terrain::TerrainCellData { biome: b, ..c }
            });
        },
    );

    // ── 生成大陆（2026-10-26）──
    // 只开八参面板；确认时经 continent_dialog_system 从统一选区格集提取
    // 包络环（wv_terrain::outline），提交走 GenerateContinentCommand 宏命令
    // （1 撤销单位，选区保留——ADR-0033）。播放中禁用（§9.4 Gate-1：只挡
    // 写入；面板侧另有兜底提示）。
    ui.add_space(8.0);
    divider(ui, shell);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if crate::ui::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::BatchContinentGenerate)),
            ButtonStyle::Accent,
            None,
            !playback_playing,
        )
        .clicked()
        {
            cont.open = true;
        }
    });
}

/// 批量应用：从 world 读每格当前值 → 构造 changes → 1 条
/// `BatchEditCellsCommand`（N 格 1 个撤销单位）→ History 落盘。
fn apply_batch(
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    map: &wv_core::id::Id,
    sel: &CellSelection,
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
    for (gx, gy) in sel.iter_cells() {
        if gx < 0 || gy < 0 || gx >= w || gy >= h {
            continue;
        }
        let idx = (gy as usize) * (w as usize) + (gx as usize);
        let before = terrain.cells[idx];
        let after = transform(before);
        if after != before {
            changes.push(CellChange {
                gx,
                gy,
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
    map: Option<&wv_core::Id>,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    after: wv_core::terrain::TerrainCellData,
    tex: &mut wv_render::terrain_view::TerrainTexture,
) {
    let world = &mut projection.world;
    let Some(map) = map.cloned() else {
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

/// **海拔提交判定**（P0-1 方案 A 的核心决策，纯函数以便测试锚定）：
///
/// | 手势态 | 控件本帧写入 | 结论 |
/// |---|---|---|
/// | 进行中 | 任意 | 不提交（拖动 / 编辑中只暂存） |
/// | 已结束 | 有 | 提交控件写入的最终值（文本提交 / 失焦提交） |
/// | 已结束 | 无 | 提交暂存值（scrub 松手帧：1 手势 = 1 command） |
fn height_commit_decision(report_changed: bool, staged: Option<i16>, scrubbed: i16) -> Option<i16> {
    if report_changed {
        Some(scrubbed)
    } else {
        staged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **P0-1 方案 A · 提交判定矩阵**：拖动中不提交；控件写入帧提交新值；
    /// 松手帧提交暂存值；无手势无写入不提交。
    #[test]
    fn height_commit_decision_matrix() {
        // 拖动中（active）：控件可能已在写值，但一律不落盘。
        assert_eq!(
            height_commit_decision(true, Some(120), 130),
            Some(130),
            "changed 优先——但调用方保证 active 分支不进本函数，此行防回归"
        );
        // 注意：active 的判定在调用方（gesture_active 走暂存分支），
        // 本函数只在「手势已结束」时被调用。
        // 松手帧：控件无写入 → 提交暂存值。
        assert_eq!(height_commit_decision(false, Some(120), 120), Some(120));
        // 松手帧但暂存被清（换格失效）→ 不提交。
        assert_eq!(height_commit_decision(false, None, 120), None);
        // 文本提交 / 失焦提交帧：控件写入新值 → 提交新值。
        assert_eq!(height_commit_decision(true, Some(100), 150), Some(150));
    }

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

    /// 构造绑定到给定图的选区（测试用；4×4 图幅）。
    fn selection_of(map: &wv_core::Id, cells: &[(i32, i32)]) -> CellSelection {
        let mut sel = CellSelection::default();
        sel.bind_map(map, 4, 4);
        sel.replace(cells.iter().copied());
        sel
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
        let sel = selection_of(&map, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
        let s = region_stats(&projection, &map, &sel).expect("stats");
        assert_eq!(s.count, 4);
        assert_eq!(s.avg_height, 250, "(100+200+300+400)/4");
        assert_eq!(s.min_height, 100);
        assert_eq!(s.max_height, 400);
        // 群系分布：森林 2 / 草原 2 → 同数按编号升序（森林=1 在前）
        assert_eq!(s.biome_hist, vec![(1, 2), (3, 2)]);
    }

    /// 越界格被跳过，不影响统计（位图本身拒绝越界写入，此处验证统计口径）。
    #[test]
    fn region_stats_skips_out_of_bounds_cells() {
        let (projection, map) = terrain_of(&[(0, 0, 50, 1)]);
        // `replace` 对越界格静默忽略——选区里只会有 (0,0)。
        let sel = selection_of(&map, &[(0, 0), (99, 99)]);
        assert_eq!(sel.len(), 1, "越界格不入位图");
        let s = region_stats(&projection, &map, &sel).expect("stats");
        assert_eq!(s.count, 1, "越界格应被跳过");
        assert_eq!(s.avg_height, 50);
    }

    /// 空选区 → None（面板据此显示空态）。
    #[test]
    fn region_stats_returns_none_for_empty_selection() {
        let (projection, map) = terrain_of(&[]);
        assert!(region_stats(&projection, &map, &CellSelection::default()).is_none());
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
            last_hypsometric: 0.0,
            last_map: None,
        }
    }

    /// **改了格必须标脏**——否则命令进了撤销栈，但地形纹理不知道要重绘，
    /// 地图不刷新（2026-10-22 统一 Inspector 补的洞；旧直提路径漏了这步）。
    #[test]
    fn single_cell_edit_marks_terrain_texture_dirty() {
        let (mut projection, map) = terrain_of(&[(1, 2, 100, 1)]);
        let mut edit = crate::edit::editing::EditHistory::default();
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
            Some(&map),
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
        let sel = selection_of(&map, &[(0, 0), (2, 1)]);
        let mut tex = blank_texture();

        apply_batch(&mut projection, &mut edit, &map, &sel, &mut tex, |c| {
            wv_core::terrain::TerrainCellData {
                height: c.height + 5,
                ..c
            }
        });

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
        let sel = selection_of(&map, &cells);
        let s = region_stats(&projection, &map, &sel).expect("stats");
        assert_eq!(s.biome_hist, vec![(3, 2), (1, 1)], "次数多的在前");
    }
}
