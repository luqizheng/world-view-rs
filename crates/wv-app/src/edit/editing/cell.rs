//! 单格编辑：点选落值 + hover 追踪。

use super::*;

/// 单格编辑工具的鼠标点击处理器（[ADR-0040] §决策 3 + §决策 5）。
///
/// 只在 [`EditCellState::tool_active`] 为 true 时生效——
/// 由 `apply_pending_commands` 的 `SelectMapTool(EditCell)` 分支翻 on。
///
/// 行为：
/// 1. 收 `MouseButton::Left` **按下**而非松开（点击即选中,不需要拖拽）
/// 2. 拿 `view.cursor_world` 与 `drill.current()` 的图地形；
/// 3. 用 [`cell_pick::pick_at`] 反查 (gx, gy),写入 [`EditCellState::selected`]。
///
/// 与笔刷（commit_stroke_on_release）**有意分开**：笔刷是按 stroke 批改,
/// 这里是一次一格、点选就够；混进 commit_stroke 会让 `btn.just_released`、
/// `btn.just_pressed` 互相打架。
///
/// 播放中只读（基线 §1.8 / V37）：直接跳过,不写 selected。
/// PointerOverUi 暂不实现（已有 `app_bootstrap` 初始化但无人写——UI 顶栏/状态栏
/// 的 egui 优先级由 egui 自己 take_pointer 处理,不影响 canvas 直接点击）。
#[allow(clippy::too_many_arguments)]
pub fn commit_edit_cell_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut edit_cell: ResMut<EditCellState>,
    screen: Res<crate::app_screen::AppScreen>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // 加载期间不选格：选中的 id 来自旧世界，对不上新世界。
    if screen.shows_loading_overlay() {
        return;
    }
    if !edit_cell.tool_active {
        return;
    }
    // 指针压在 egui 面板/顶栏上：这是面板交互（拖数值、按按钮、拖窗口），
    // 不是「选格」——吃了它会把面板底下那格误设为选中，Inspector 字段
    // 全部跟着跳（2026-10-20 实机 bug：点面板即丢焦点）。与 drill /
    // brush_cursor 的 PointerOverUi gate 同口径。
    if ui.wants_pointer {
        return;
    }
    if playback.playing {
        // 时间游标在动 → 当前 world 是投影 → 不要写。
        return;
    }
    let Some(cursor) = view.cursor_world else {
        edit_cell.selected = None;
        return;
    };
    let Some(target_map) = drill.current() else {
        // 没图 = 没有地形；与切图同语义,清空。
        edit_cell.selected = None;
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        // 子图继承父图栅格时 `terrain_of` 会返 None,这里走清空分支。
        edit_cell.selected = None;
        return;
    };

    edit_cell.last_map_id = Some(target_map.clone());

    if let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) {
        edit_cell.selected = Some((pick.gx, pick.gy));
        // 每图一次 hit-test 顺路更新 last_map_id；on_map_change 的语义保留
        // 给切图等"用户切到另一张图"路径。
    } else {
        // 点在地图外 = 取消选中（与 BatchSelect tool 同样克制："点空白 = 取消当前选"）。
        edit_cell.selected = None;
    }
}

/// **每帧**追踪 hover 格（[ADR-0052] D2 写入侧）。
///
/// `commit_edit_cell_on_click` 只在**按下**那一刻写 `selected`；hover 是
/// **持续**态——光标在格上移动就得跟着变，故需一个每帧跑的系统。
///
/// 写入 [`EditCellState::hover`]，供渲染侧 `sync_cell_hover` 画双色描边
/// + 准星（D2）。
///
/// gate 与点选同口径：
/// - `tool_active` 为 false → 不写（工具没开就别画准星）；
/// - 指针压在 egui 面板上 → 不写（面板交互不是选格）；
/// - 时间轴播放中 → 不写（投影态不写世界）；
/// - 光标不在图上 / 无图 / 无地形 → 写 `None`（清掉旧准星）。
pub fn track_edit_cell_hover(
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    ui: Res<wv_render::camera::PointerOverUi>,
    playback: Res<PlaybackState>,
    mut edit_cell: ResMut<EditCellState>,
) {
    if !edit_cell.tool_active {
        if edit_cell.hover.is_some() {
            edit_cell.hover = None;
        }
        return;
    }
    if ui.wants_pointer || playback.playing {
        if edit_cell.hover.is_some() {
            edit_cell.hover = None;
        }
        return;
    }
    let Some(cursor) = view.cursor_world else {
        edit_cell.hover = None;
        return;
    };
    let Some(target_map) = drill.current() else {
        edit_cell.hover = None;
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        edit_cell.hover = None;
        return;
    };
    edit_cell.hover =
        cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain).map(|pick| (pick.gx, pick.gy));
}
