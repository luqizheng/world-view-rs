//! 属性窗体与编辑性进入的跨系统请求（ADR-0089 D2 / D3）。
//!
//! 交互语法的两条「点击 → 弹窗」路径都跨系统：判定在输入系统（wv-render
//! `drill_pick` / wv-app `commit_point_select_on_click`），**拉起面板与编排
//! 工作区**是应用层关注点。本模块是两组请求的**唯一消费点**：
//!
//! - [`OpenInspectorRequest`]：双击命中（格子 / 无实体的命中）→ 拉起
//!   Inspector。带实体的双击走 [`wv_render::drill::PropertyWindowRequest`]
//!   （wv-render 侧置请求，本模块一并消费）。
//! - [`CityEditRequest`]：属性窗体「编辑城市」按钮 → **编辑性进入**：
//!   `drill.enter` + CellMap 工作区编排 + 默认激活该场景编辑工具（笔刷）。
//!   浏览性进入（顶栏段 / 切视图 / 开工程）默认点选，编排入口在
//!   [`crate::ui_command_palette::enter_cellmap_workspace`]。
//!
//! 消费系统一帧一请求（take 语义）；请求资源在 `register` 里 init。

use bevy::prelude::*;

use crate::state::{DrillState, Panel, PanelState, PlaybackState, ViewState, WorldProjection};
use crate::ui_command_palette::{enter_cellmap_workspace, CommandPalette};
use crate::ui_shell::ShellNotice;

/// 「拉起 Inspector」请求（一次性旗标）。
///
/// `pub` 是必须的：它出现在 pub 系统的参数里（同 `RightDragWatch` 教训）。
#[derive(Resource, Default)]
pub struct OpenInspectorRequest {
    open: bool,
}

impl OpenInspectorRequest {
    /// 请求拉起 Inspector 面板（重复请求合并）。
    pub fn request(&mut self) {
        self.open = true;
    }
}

/// 「编辑城市」请求（属性窗体按钮 → 编辑性进入，ADR-0089 D3）。
#[derive(Resource, Default)]
pub struct CityEditRequest {
    target: Option<wv_core::Id>,
}

impl CityEditRequest {
    /// 请求进入 `target`（必须是当前图的直接子图，由消费方 `drill.enter` 终审）。
    pub fn request(&mut self, target: wv_core::Id) {
        self.target = Some(target);
    }
}

/// 消费两组「拉起属性窗体」请求：双击一律属性窗体（ADR-0089 D2）。
///
/// 带实体的（wv-render 双击判定）与不带实体的（CellMap 格子双击）合并处理
/// ——两者都只做一件事：把 Inspector 面板拉起来，选中态由判定系统已写好。
pub fn open_property_window_on_request(
    mut property: ResMut<wv_render::drill::PropertyWindowRequest>,
    mut inspector: ResMut<OpenInspectorRequest>,
    mut panels: ResMut<PanelState>,
) {
    if property.take().is_some() || inspector.open {
        inspector.open = false;
        // 地点表单在 Inspector「属性」页签——面板不可见则拉起
        // （与 pin_drag 双击分支同款动作）。
        panels.visible.insert(Panel::Inspector, true);
    }
}

/// 消费「编辑城市」请求：**编辑性进入**（ADR-0089 D3——浏览进来看，按钮进去改）。
///
/// `drill.enter` 成功后：切 Topographic + CellMap 工作区编排（快照面板 /
/// 开笔刷与地标浮窗）+ 默认激活**该场景的编辑工具**（笔刷——场景分区刷色的
/// 载体，M1「进去即刷」由本路径保住）。失败只 notice 不静默（V1 精神）。
#[allow(clippy::too_many_arguments)]
pub fn apply_city_edit_request(
    mut request: ResMut<CityEditRequest>,
    projection: Res<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut view: ResMut<ViewState>,
    mut panels: ResMut<PanelState>,
    mut palette: ResMut<CommandPalette>,
    playback: Res<PlaybackState>,
    mut notice: ResMut<ShellNotice>,
    time: Res<Time>,
) {
    let Some(target) = request.target.take() else {
        return;
    };
    let frame = (time.elapsed_secs() * 60.0) as u64;
    if playback.playing {
        notice.error("播放中为只读：先暂停再进入编辑", frame);
        return;
    }
    if !drill.enter(&projection.world, &target) {
        notice.error("无法进入编辑：该地图不是当前图的直接子图", frame);
        return;
    }
    view.mode = crate::state::ViewMode::Topographic;
    enter_cellmap_workspace(&mut panels, &mut palette, true);
}

/// 把「工具复位到浏览默认」排队（ADR-0089 D5）。
///
/// 供 `dispatch::SetViewMode` / `top_bar` 段 / `project_load` 共用一句：
/// 排 [`crate::ui_command_palette::CommandId::ResetToolsToDefault`]（全关 +
/// 开点选，无翻转语义），落地动作全在分发器。
pub fn queue_reset_to_default_tool(palette: &mut CommandPalette) {
    palette.pending = Some(crate::ui_command_palette::CommandId::ResetToolsToDefault);
}
