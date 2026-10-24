//! 界面外壳的输入与外观投影：快捷键、日夜主题、面板位置重置、窗口清屏色。

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::state::{PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_command_palette::{CommandId, CommandPalette};
use crate::ui_layout::LayoutFile;
use wv_render::state::SelectionShape;

/// 快捷键（§6.1，19 条；**禁止占用裸 `Tab`**）。
///
/// **输入框聚焦时全部快捷键失效**（除 `Esc` / `Ctrl+K`）——
/// 在打字时按 `1` 不该切视图（§6.1）。
pub(crate) fn hotkeys(
    mut contexts: EguiContexts,
    mut palette: ResMut<CommandPalette>,
    mut cfg: ResMut<UiShellConfig>,
    mut panels: ResMut<PanelState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let wants_text = ctx.egui_wants_keyboard_input();
    let input = ctx.input(|i| i.clone());

    // Esc：关闭命令面板（即使聚焦输入框也生效）。
    if input.key_pressed(bevy_egui::egui::Key::Escape) && palette.open {
        palette.close();
        return;
    }

    // Ctrl+K：命令面板（任意时刻可用）。
    if input.modifiers.command && input.key_pressed(bevy_egui::egui::Key::K) {
        palette.open();
        return;
    }

    // 输入框聚焦时，除 Esc / Ctrl+K 外全部失效（§6.1）。
    if wants_text {
        return;
    }

    let cmd = input.modifiers.command;
    let shift = input.modifiers.shift;

    // Ctrl+J：切换明暗。
    if cmd && input.key_pressed(bevy_egui::egui::Key::J) {
        cfg.theme = cfg.theme.toggled();
    }
    // Ctrl+Shift+I：沉浸模式（裸 Tab 保留给焦点移动，WCAG 2.1.1）。
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::I) {
        panels.immersive = !panels.immersive;
    }
    // 数字 1–3：切换浏览视图模式（BROWSING 现行三项：矢量地形 / 色块地形 /
    // 势力图，见 `ViewMode::BROWSING`；其余模式不占数字键）。
    for (i, key) in [
        bevy_egui::egui::Key::Num1,
        bevy_egui::egui::Key::Num2,
        bevy_egui::egui::Key::Num3,
    ]
    .iter()
    .enumerate()
    {
        if input.key_pressed(*key) {
            palette.pending = Some(CommandId::SetViewMode(i as u8));
        }
    }
    // Ctrl+S / Ctrl+Shift+S：保存 / 另存为（后者先判，否则 Shift 版会被当成普通保存）。
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::S) {
        palette.pending = Some(CommandId::SaveProjectAs);
    } else if cmd && input.key_pressed(bevy_egui::egui::Key::S) {
        palette.pending = Some(CommandId::Save);
    }
    // Ctrl+O / Ctrl+E / Ctrl+W：工程命令（v0.2 §A）。
    if cmd && input.key_pressed(bevy_egui::egui::Key::O) {
        palette.pending = Some(CommandId::OpenProject);
    }
    if cmd && input.key_pressed(bevy_egui::egui::Key::E) {
        palette.pending = Some(CommandId::ExportProject);
    }
    if cmd && input.key_pressed(bevy_egui::egui::Key::W) {
        palette.pending = Some(CommandId::CloseProject);
    }
    // Ctrl+Shift+N：新建工程（同样先判 Shift 版）。
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::N) {
        palette.pending = Some(CommandId::NewProject);
    }
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::Z) {
        palette.pending = Some(CommandId::Redo);
    } else if cmd && input.key_pressed(bevy_egui::egui::Key::Z) {
        palette.pending = Some(CommandId::Undo);
    }
    // Backspace：钻回上一级地图（v0.2 §B.2）。
    // 输入框聚焦时不会走到这里（上面已 `return`），故与「删除文字」不冲突。
    if input.key_pressed(bevy_egui::egui::Key::Backspace) {
        palette.pending = Some(CommandId::DrillUp);
    }
    // Space：播放 / 暂停。
    if input.key_pressed(bevy_egui::egui::Key::Space) {
        palette.pending = Some(CommandId::TogglePlay);
    }
    // Home / End：时间轴首 / 尾。
    if input.key_pressed(bevy_egui::egui::Key::Home) {
        palette.pending = Some(CommandId::JumpToStart);
    }
    if input.key_pressed(bevy_egui::egui::Key::End) {
        palette.pending = Some(CommandId::JumpToEnd);
    }
    // [ ]：笔刷半径。
    if input.key_pressed(bevy_egui::egui::Key::OpenBracket) {
        palette.pending = Some(CommandId::BrushRadius(-1));
    }
    if input.key_pressed(bevy_egui::egui::Key::CloseBracket) {
        palette.pending = Some(CommandId::BrushRadius(1));
    }
    // F：全览地图。
    if input.key_pressed(bevy_egui::egui::Key::F) {
        palette.pending = Some(CommandId::FitMap);
    }
    // F5：强制重画地形（2026-10-14 用户需求）。和 FitMap 的 F 不冲突。
    if input.key_pressed(bevy_egui::egui::Key::F5) {
        palette.pending = Some(CommandId::ForceRebuildTexture);
    }

    // ── 地图工具条字母键（2026-10-19 Figma `3:21848`） ──────────────────
    // 与瓷砖上的字母一一对应；**只在无修饰键时生效**——Ctrl+E 是导出，
    // 不能被测量橡皮抢走。互斥逻辑统一在 `CommandId::SelectMapTool`。
    if !cmd && !shift {
        use crate::ui_command_palette::MapToolId;
        let tool = [
            (
                bevy_egui::egui::Key::V,
                MapToolId::Select(SelectionShape::Polygon),
            ),
            (bevy_egui::egui::Key::H, MapToolId::Pan),
            (bevy_egui::egui::Key::B, MapToolId::Brush),
            // 橡皮贴着涂抹：两把刷子一个键位族。E 原属「测量橡皮」——那枚
            // 瓷砖已随 ADR-0043 退出工具条，E 顺位给地形橡皮刷（测量橡皮
            // 仍可从命令面板 Ctrl+K 进入）。
            (bevy_egui::egui::Key::E, MapToolId::BrushErase),
            (bevy_egui::egui::Key::L, MapToolId::MeasureLine),
            (bevy_egui::egui::Key::M, MapToolId::MeasureRect),
            (bevy_egui::egui::Key::P, MapToolId::Continent),
            // 单格编辑（[ADR-0040]）：C = Cell。与 Ctrl+C 复制不冲突
            // （本表只在无修饰键时生效，见上方 `!cmd && !shift` gate）。
            (bevy_egui::egui::Key::C, MapToolId::EditCell),
            // 单选模式（2026-10-24）：S = Select。`V` 已属框选组，不复用。
            (bevy_egui::egui::Key::S, MapToolId::CellSelect),
        ]
        .into_iter()
        .find(|(key, _)| input.key_pressed(*key));
        if let Some((_, tool)) = tool {
            palette.pending = Some(CommandId::SelectMapTool(tool));
        }
        // G：参照网格开关（ADR-0043 把网格瓷砖留在工具条末位，给它配同款
        // 字母键；此前该命令无快捷键）。
        if input.key_pressed(bevy_egui::egui::Key::G) {
            palette.pending = Some(CommandId::ToggleGrid);
        }
    }
}

/// 把 [`UiShellConfig`] 投影进 egui 上下文（日夜模式的**唯一**全局写入口）。
///
/// 只在配置变化时执行：`Res::is_changed()` 覆盖「资源首次加入」与「切换主题」
/// 两种情形。写入内容见 [`wv_ui::ui_theme::apply_ui_theme`]——`set_theme` 决定
/// egui 自带控件取哪套风格，`set_visuals_of` 把令牌投影上去。
///
/// 这里只改**界面外壳**；地图配色走 [`MapPalette`](crate::theme::palette::MapPalette)
/// （`ViewState::map_palette`），两条线互不影响（D2：世界色归世界、界面色归界面）。
pub(crate) fn apply_ui_theme(mut contexts: EguiContexts, cfg: Res<UiShellConfig>) {
    if !cfg.is_changed() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    wv_ui::ui_theme::apply_ui_theme(ctx, &cfg);
}

/// 消费「重置浮动窗口位置」信号（设置对话框 / 命令面板 `ResetPanelLayout`）。
///
/// 位置的真相有两份：egui 自己的窗口记忆（`Memory::area_rect`，拖动后再也不看
/// `default_pos`）和 `ui-layout.yaml` 的内存副本。只清一份不能复位——清磁盘
/// 不清 memory，窗口仍停在旧处；清 memory 不清磁盘，下一帧 `persist_layout_on_change`
/// 会把旧矩形写回来。因此这里两处同时抹掉：
/// - `reset_areas()`：所有 egui Area（含 12 个浮动面板）回到首次布局；
/// - `LayoutFile.panels.clear()`：`get` 回退 [`crate::ui_layout::default_layout`]。
///
/// 只清 `panels`，**不动** `theme` / `language` / `map_theme`——重置的是位置，
/// 不是用户偏好（用户裁定 2026-10-19：设置里的「重置浮动窗口位置」只对窗口几何）。
///
/// 放在 `PreUpdate`（`apply_ui_theme` 之后）：本帧面板绘制前生效，避免旧位置
/// 先闪一帧。
pub(crate) fn apply_panel_reset(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    mut layouts: ResMut<LayoutFile>,
    projection: Res<WorldProjection>,
) {
    if !panels.reset_requested {
        return;
    }
    panels.reset_requested = false;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    ctx.memory_mut(|m| m.reset_areas());
    layouts.panels.clear();
    // 磁盘也要复位：否则下次启动 `load_layout_on_project_open` 又把旧几何读回来。
    // 这里保留 `theme` / `density` / `map_theme` / `language`——重置的是窗口位置，
    // 不是用户偏好。
    if let Some(path) = crate::ui_layout::layout_path(projection.project_dir.as_deref()) {
        layouts.save(&path);
    }
}

/// 窗口清屏色跟随界面外壳底色（日夜模式切换时）。
///
/// 清屏色只在**地图外**可见（地图铺满时被完全盖住），属于应用外壳而非地图配色；
/// 亮色模式下若仍留着暗色底，窗口边缘会有一圈割裂的黑边。
pub(crate) fn sync_window_clear_color(cfg: Res<UiShellConfig>, mut clear: ResMut<ClearColor>) {
    if !cfg.is_changed() {
        return;
    }
    let bg = cfg.palette().bg_app;
    *clear = ClearColor(Color::srgb(bg.r, bg.g, bg.b));
}
