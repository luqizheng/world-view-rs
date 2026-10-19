//! 地图工具条（对图 Figma `3:21848` · Map tool dock，2026-10-19）。
//!
//! ## 职责
//! - `map_controls` —— 地图工具条。**横 / 纵两种朝向**，首个按钮切换；
//!   朝向与吸附开关随工程落盘到 `ui-layout.yaml`（[`MapDockState`]）。
//! - `MapDockState` / `MapDockOrientation` —— 工具条偏好（朝向 + 吸附），
//!   由 `ui_shell` 的布局读写系统负责持久化。
//! - `tool_tile` / `icon_tile` / `icon_button` / `separator` —— 本模块私有的
//!   自绘控件，严格按 Figma 规格（40×38 工具瓷砖、32×32 图标钮、1×24 分隔条）。
//!
//! ## 工具映射（Figma → 本项目）
//!
//! | Figma | 键 | 本项目语义 |
//! |---|---|---|
//! | mouse-pointer-2 | V | 选择工具（圈选意图包络） |
//! | hand | H | 抓手：左键拖拽平移地图 |
//! | paintbrush | B | 笔刷（开关地形涂抹） |
//! | eraser | E | 测量橡皮（删除测量） |
//! | spline | L | 折线测量 |
//! | pentagon | P | 生成大陆（多边形包络 → 八参数面板） |
//! | ruler | M | 矩形测量 |
//! | undo-2 / redo-2 | — | 撤销 / 重做（同一分发入口 `CommandId::Undo/Redo`） |
//! | magnet | — | 吸附：势力划分默认「贴地」（沿河流 / 山脊 / 湖岸） |
//! | — | — | 网格 / 放大 / 缩小 / 全图（保留既有视图组） |
//!
//! Figma 原稿的 `T`（文字）在项目中还没有文字落点工具，故不设入口——宁可
//! 少一个假按钮，也不放一个点了没反应的瓷砖。
//!
//! ## 交互与架构边界
//! - 工具切换**不直接改状态**：点击瓷砖只把 `CommandId::SelectMapTool(_)`
//!   排进 [`CommandPalette::pending`]，由 `ui_shell::apply_pending_commands`
//!   统一做互斥（开一个工具 = 关掉其它工具）。快捷键 V/H/B/E/L/M/P 同源，
//!   避免两套逻辑漂移。
//! - 视图组（网格 / 缩放 / 全图）与「切换朝向」直接写 [`ViewState`] /
//!   [`MapDockState`]——它们不改世界数据，也不涉及工具互斥。
//! - 不在无工程时显示（与 `welcome_screen` 共用 [`StartupLoadRequest`] 信号）。
//!
//! ## 视觉规格（Figma `3:21848`，经 shell 令牌投影）
//! - 大框：`float_fill`（暗色 `#1B232A @ .96`）+ `border_strong`（`#46525B`）
//!   + 圆角 10 + 投影 `0 8 24 rgba(0,0,0,.2)`。
//! - 工具瓷砖 40×38：图标 16 + 间隙 2 + 8px 字母；活动态 `gold_soft` 底 +
//!   `gold` 描边与前景（Figma 的 `#4B422E` / `#D6B77B`）。
//! - 图标钮 32×32：活动态 `accent_soft` 底（Figma `#2C465F`）+ accent 图标。
//! - 分隔条 1×24，色 `border_subtle`（Figma `#303B43`）。
//! - 间距 6px；横向贴底居中，纵向贴右居中，均距窗体边缘 18px。
//!
//! ## 主要导出
//! - [`map_controls`] —— Bevy 系统。
//! - [`MapDockState`] / [`MapDockOrientation`] —— 持久化偏好。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use serde::{Deserialize, Serialize};

use crate::edit::editing::EditHistory;
use crate::state::{
    DrillState, GridConfig, MeasureMode, MeasureState, Panel, PanelState, ViewState,
    WorldProjection,
};
use crate::theme::ui_theme::{margin_xy, mono, UiShellConfig};
use crate::ui_command_palette::{CommandId, CommandPalette, MapToolId};
use crate::ui_shell::StartupLoadRequest;
use wv_render::state::{BrushState, SelectionState};

use super::shell::UiShell;
use super::widgets::icon_image;
use super::{
    ICON_BRUSH, ICON_ERASER, ICON_GRID, ICON_HAND, ICON_MAGNET, ICON_MOUNTAIN_RANGE,
    ICON_MOUSE_POINTER, ICON_MOVE_HORIZONTAL, ICON_MOVE_VERTICAL, ICON_REDO, ICON_ROUTE,
    ICON_SQUARE, ICON_UNDO,
};

/// 贴边距离（原型 `right: 18px`）。
const EDGE_OFFSET: f32 = 18.0;
/// 纵向工具条的顶距：顶栏 38 + 8，避开顶栏（2026-10-19 实测：13 项纵排
/// 居中会压到时间轴，改为贴顶排布后两者互不遮挡）。
const DOCK_TOP_CLEARANCE: f32 = 46.0;
/// 大框圆角（Figma 10）。
const DOCK_RADIUS: f32 = 10.0;
/// 瓷砖圆角（Figma 5）。
const TILE_RADIUS: f32 = 5.0;

/// 两种朝向的尺寸表。
///
/// - **横向**严格按 Figma `3:21848`：瓷砖 40×38（图标 16 + 字母 8）、图标钮
///   32×32、间距 6、大框内边距 10/7。
/// - **纵向**是紧凑变体：瓷砖 34×34（仅图标 18，不重复字母）、图标钮 28×28、
///   间距 4、内边距 6/10。原因见 [`DOCK_TOP_CLEARANCE`] 与实测截图：13 项
///   按 40×38 纵排会侵占时间轴；字母标签在纵向也读不清，保留在横向即可。
#[derive(Clone, Copy)]
struct DockMetrics {
    tile: egui::Vec2,
    tile_icon: f32,
    /// `Some(size)` = 画快捷键字母（仅横向）；`None` = 纯图标。
    label: Option<f32>,
    icon_btn: f32,
    icon_btn_icon: f32,
    gap: f32,
    sep_len: f32,
    pad_x: f32,
    pad_y: f32,
}

impl DockMetrics {
    fn for_orientation(vertical: bool) -> Self {
        if vertical {
            Self {
                tile: egui::vec2(34.0, 34.0),
                tile_icon: 18.0,
                label: None,
                icon_btn: 28.0,
                icon_btn_icon: 16.0,
                gap: 4.0,
                sep_len: 20.0,
                pad_x: 6.0,
                pad_y: 10.0,
            }
        } else {
            Self {
                tile: egui::vec2(40.0, 38.0),
                tile_icon: 16.0,
                label: Some(8.0),
                icon_btn: 32.0,
                icon_btn_icon: 16.0,
                gap: 6.0,
                sep_len: 24.0,
                pad_x: 10.0,
                pad_y: 7.0,
            }
        }
    }
}

/// 工具条朝向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapDockOrientation {
    /// 纵向：贴窗体右缘、垂直居中（历史默认）。
    #[default]
    Vertical,
    /// 横向：贴窗体底缘、水平居中（Figma `3:21848` 的形态）。
    Horizontal,
}

impl MapDockOrientation {
    /// `ui-layout.yaml` 的持久化键。
    pub fn key(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }

    /// 从持久化键解析；不可识别返回 `None`（V1：静默回退默认）。
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "vertical" => Some(Self::Vertical),
            "horizontal" => Some(Self::Horizontal),
            _ => None,
        }
    }

    /// 切换朝向（工具条首按钮）。
    pub fn toggled(self) -> Self {
        match self {
            Self::Vertical => Self::Horizontal,
            Self::Horizontal => Self::Vertical,
        }
    }
}

/// 工具条相对锚点的偏移（屏幕像素）。
///
/// 纵向模式：x = 离右缘的负偏移，y = 离顶栏下沿的偏移。
/// 横向模式：x = 离水平中心的偏移，y = 离窗体下沿的负偏移。
///
/// 持久化键 = `ui-layout.yaml::map_dock_pos`，自由 `[x, y]`。
/// `None` = 用默认锚点（项目历史位置）。
///
/// **设计要点**：用户能用 grip-vertical 把工具条拖到屏幕任意位置。拖到的
/// 位置存进本字段，下次启动自动恢复。这与其它面板的窗口记忆（egui `Memory`
/// 的 `area_rect`）平行——但工具条走自己的 `MapDockState`，不进 `LayoutFile`
/// 的 `panels` 表，因为它不是浮动面板。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Resource, Default)]
pub struct MapDockPos {
    /// x 偏移（像素）。
    pub x: f32,
    /// y 偏移（像素）。
    pub y: f32,
}

/// 地图工具条偏好（随工程落盘）。
#[derive(Debug, Clone, PartialEq, Resource)]
pub struct MapDockState {
    /// 朝向。
    pub orientation: MapDockOrientation,
    /// 吸附开关（magnet）。当前消费点：势力划分对话框的默认边界模式——
    /// 开启 = 默认「贴地」（沿河流 / 山脊 / 湖岸），关闭 = 默认直线。
    pub snapping: bool,
    /// 用户拖动 grip 后的位置偏移（绝对像素，与 `orientation` 联动解释）。
    /// `None` = 用默认锚点。
    pub pos: Option<MapDockPos>,
    /// 缩放控件条（grip + − / ＋ / 适配）的独立位置偏移。
    /// `None` = 默认贴在地图工具条**对侧**：
    ///   纵向 dock 时贴左缘中央；横向 dock 时贴顶中央。
    pub zoom_strip_pos: Option<MapDockPos>,
}

impl Default for MapDockState {
    fn default() -> Self {
        Self {
            // 启动朝向：纵向（项目历史与时间轴 / 顶栏布局都按纵向标定；横向
            // 供作者按需切换）。截图验收时可用 `WV_DOCK=horizontal` 强制横向首发。
            orientation: match std::env::var("WV_DOCK").as_deref() {
                Ok("horizontal") => MapDockOrientation::Horizontal,
                _ => MapDockOrientation::Vertical,
            },
            // Figma 原稿的 magnet 是启用态（`Snapping enabled`）；与「划分
            // 势力默认贴地」的历史默认（`DivisionMode::Snap`）一致。
            snapping: true,
            pos: None,
            zoom_strip_pos: None,
        }
    }
}

/// 地图工具条（横向 / 纵向两形态）。
///
/// 顶部早返守卫与 `welcome_screen` 同源 —— 无工程时整条不渲染。
#[allow(clippy::too_many_arguments)]
pub fn map_controls(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut view: ResMut<ViewState>,
    measure: Res<MeasureState>,
    selection: Res<SelectionState>,
    brush: Res<BrushState>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<wv_render::terrain_view::TerrainTexture>,
    projection: Res<WorldProjection>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    startup_req: Res<StartupLoadRequest>,
    mut dock: ResMut<MapDockState>,
    mut palette: ResMut<CommandPalette>,
    edit: Res<EditHistory>,
    panels: Res<PanelState>,
) {
    // 无工程锁（图片规格 §首页不可见）：与 `welcome_screen` 共用同一信号。
    if !projection.has_open_project() || startup_req.0.is_some() {
        return;
    }

    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // 当前级的缩放区间与「全图」尺寸。
    let screen = ctx.content_rect().size();
    let window_size = Vec2::new(screen.x, screen.y);
    let raster_size = Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size;
    let level_map_size =
        wv_render::zoom_level::map_size_of(&projection.world, drill.current(), raster_size);
    let zoom = wv_render::zoom_level::LevelZoom::for_map(level_map_size, window_size);

    // 工具活动态（全部从资源读；工具切换本身走命令分发）。
    let select_active = selection.enabled;
    let pan_active = view.pan_tool;
    let brush_active = brush.enabled;
    let eraser_active = measure.mode == MeasureMode::Eraser;
    let line_active = measure.mode == MeasureMode::Line;
    let rect_active = measure.mode == MeasureMode::Rect;
    let grid_active = view.grid_lines;
    let snap_active = dock.snapping;
    let can_undo = edit.history.can_undo();
    let can_redo = edit.history.can_redo();

    let vertical = dock.orientation == MapDockOrientation::Vertical;
    let metrics = DockMetrics::for_orientation(vertical);
    // 默认锚点（无拖动历史时）：纵向贴顶、横向贴底让时间轴。
    let (anchor, default_offset) = if vertical {
        (
            egui::Align2::RIGHT_TOP,
            egui::vec2(-EDGE_OFFSET, DOCK_TOP_CLEARANCE),
        )
    } else {
        let content = ctx.content_rect();
        let bottom_lift = if panels.is_visible(Panel::Timeline) {
            ctx.memory(|m| m.area_rect(egui::Id::new(("wv_panel", Panel::Timeline))))
                .filter(|r| r.top() > content.center().y)
                .map(|r| (content.bottom() - r.top()).max(0.0) + EDGE_OFFSET)
                .unwrap_or(EDGE_OFFSET)
        } else {
            EDGE_OFFSET
        };
        (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -bottom_lift))
    };
    // 用户拖动 grip 后的位置偏移：仍是「相对锚点的屏幕偏移」，但不再
    // 是「贴边 -18」，而是任意 (x, y)。把它折成同样的 `vec2(x, y)`：
    //   纵向：`RIGHT_TOP` + (offset_x, offset_y) —— x 越负越往左，y 越正越往下；
    //   横向：`CENTER_BOTTOM` + (offset_x, offset_y) —— 允许工具条在底部
    //   中心附近水平滑动，y 越负越往上。
    let offset = if let Some(p) = dock.pos {
        egui::vec2(p.x, p.y)
    } else {
        default_offset
    };
    let margin = margin_xy(metrics.pad_x, metrics.pad_y);

    egui::Area::new(egui::Id::new("wv_map_controls_v06"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // 圆角大框：float_fill 底 + border_strong 描边 + 投影（Figma `3:21848`）。
            egui::Frame::NONE
                .fill(shell.float_fill())
                .stroke(egui::Stroke::new(1.0, shell.border_strong()))
                .corner_radius(egui::CornerRadius::from(DOCK_RADIUS))
                .inner_margin(margin)
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(51),
                })
                .show(ui, |ui| {
                    let mut slots = |ui: &mut egui::Ui| {
                        // ── 朝向切换 + 拖动手柄（Figma grip-vertical） ─────
                        // grip-vertical 是 Figma 原稿的首图标，对应我们的两条
                        // 职责：① 拖动 = 整条工具条跟随；② 单击 = 切换朝向。
                        // 视觉上独占一个 icon_btn 槽（纵向 28px、横向 32px），
                        // hover 显 `bg_hover`。
                        let (swap_icon, swap_hint) = if vertical {
                            (ICON_MOVE_HORIZONTAL, "拖动移动 / 单击切换为横向工具条")
                        } else {
                            (ICON_MOVE_VERTICAL, "拖动移动 / 单击切换为纵向工具条")
                        };
                        let grip_resp = grip_handle(ui, &shell, metrics, swap_icon, swap_hint);
                        if grip_resp.dragged() {
                            // 把 grip 的拖动量累计到 `MapDockState::pos`：
                            //   纵向模式：anchor=RIGHT_TOP，pos.x 越负越靠左；
                            //   横向模式：anchor=CENTER_BOTTOM，pos.x 决定水平偏移。
                            // 两种模式的「屏幕 x 方向」一致（正方向 = 右），
                            // 不用换算；屏幕 y 方向在两个 anchor 下都一致（正 = 下）。
                            // 「(x, y) 相对锚点」与「(x, y) 相对 grip 起手」同义，
                            // 直接用 drag_delta 累加；松手后 dock.pos 落到新位置。
                            let pos = dock.pos.get_or_insert(MapDockPos {
                                x: default_offset.x,
                                y: default_offset.y,
                            });
                            pos.x += grip_resp.drag_delta().x;
                            pos.y += grip_resp.drag_delta().y;
                        }
                        if grip_resp.clicked() {
                            dock.orientation = dock.orientation.toggled();
                            // 朝向切换后旧 pos 不再合身：清掉，回到默认锚点，
                            // 下次重新拖动时再记录新坐标。
                            dock.pos = None;
                        }

                        // ── 工具组（V / H / B / E / L / P / M） ─────────────
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_MOUSE_POINTER,
                            "V",
                            "选择：圈选区域（左键加点；双击/回车/点首锚闭合；Esc 丢弃）",
                            select_active,
                            &mut palette,
                            MapToolId::Select,
                        );
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_HAND,
                            "H",
                            "抓手：左键拖拽平移地图（右键/中键始终可平移）",
                            pan_active,
                            &mut palette,
                            MapToolId::Pan,
                        );
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_BRUSH,
                            "B",
                            "笔刷：涂抹地形（再次点击关闭；半径/强度见笔刷面板）",
                            brush_active,
                            &mut palette,
                            MapToolId::Brush,
                        );
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_ERASER,
                            "E",
                            "测量橡皮：点击删除测量",
                            eraser_active,
                            &mut palette,
                            MapToolId::MeasureEraser,
                        );
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_ROUTE,
                            "L",
                            "折线测量：多段测量；右键/Esc 收笔",
                            line_active,
                            &mut palette,
                            MapToolId::MeasureLine,
                        );
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_MOUNTAIN_RANGE,
                            "P",
                            "生成大陆…（八参数面板；用「选择」工具圈出意图包络）",
                            false,
                            &mut palette,
                            MapToolId::Continent,
                        );
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_SQUARE,
                            "M",
                            "矩形测量：两点确定长×宽",
                            rect_active,
                            &mut palette,
                            MapToolId::MeasureRect,
                        );

                        separator(ui, &shell, metrics, vertical);

                        // ── 撤销 / 重做 ───────────────────────────────────
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_UNDO,
                            false,
                            can_undo,
                            "撤销 (Ctrl+Z)",
                        )
                        .clicked()
                        {
                            palette.pending = Some(CommandId::Undo);
                        }
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_REDO,
                            false,
                            can_redo,
                            "重做 (Ctrl+Shift+Z)",
                        )
                        .clicked()
                        {
                            palette.pending = Some(CommandId::Redo);
                        }

                        separator(ui, &shell, metrics, vertical);

                        // ── 吸附（magnet） ────────────────────────────────
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_MAGNET,
                            snap_active,
                            true,
                            "吸附：开启后势力划分默认「贴地」（沿河流 / 山脊 / 湖岸）",
                        )
                        .clicked()
                        {
                            dock.snapping = !dock.snapping;
                        }

                        separator(ui, &shell, metrics, vertical);

                        // ── 视图组 ──────────────────────────────────────
                        // 网格：参照网格开关（`ViewState.grid_lines`）。
                        if icon_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_GRID,
                            grid_active,
                            "显示参照网格（独立于笔刷）",
                        )
                        .clicked()
                        {
                            view.grid_lines = !view.grid_lines;
                        }
                    };

                    // ── 缩放控件条（`wv-ui::zoom_controls` · Figma 3:21882） ──
                    //
                    // 替代原先的 `icon_tile(zoom_in/zoom_out/maximize)` 三块：
                    // 把 − / ＋ / 百分比 / 适配 / grip 收成一个公共组件，
                    // 既缩小组件列表也复用到小地图。回调里只改 `view.scale`，
                    // 相机由 `set_map_scale` 同步（避免双真相）。
                    let _ = line_active;

                    ui.spacing_mut().item_spacing = egui::vec2(metrics.gap, metrics.gap);
                    if vertical {
                        ui.vertical(|ui| slots(ui));
                    } else {
                        ui.horizontal(|ui| slots(ui));
                    }
                });
        });

    // ── 缩放控件条（独立 Area，grip 可拖） ──────────────────────────
    //
    // 与 dock 是兄弟关系：dock 是「工具集合」，zoom_strip 是「相机操作」。
    // 二者共享 [`MapDockState`]（同一份持久化），但位置偏移独立记
    // `zoom_strip_pos` —— dock 横向 / 纵向切换时，strip 默认贴到对侧。
    //
    // Figma `3:21882` 的 strip 只能横排（设计上 32 高一行），dock 任何朝向
    // 下 strip 都是横排。默认位置：纵向 dock → 贴左缘**底**（让开顶栏
    // 顶区域，避开左下方的笔刷/小地图面板）；横向 dock → 贴顶栏下沿
    // **右**（让开顶栏的"主世界/日夜"标题，让开 dock 占的底部位置）。
    let (zoom_anchor, zoom_default) = if vertical {
        (
            egui::Align2::LEFT_BOTTOM,
            egui::vec2(EDGE_OFFSET, -EDGE_OFFSET),
        )
    } else {
        // 贴顶栏**下沿**（DOCK_TOP_CLEARANCE = 顶栏 38 + 8）；只偏移 18 会
        // 落在 38px 顶栏内部，压住 Rebuild / settings 按钮（2026-10-19 实测）。
        (
            egui::Align2::RIGHT_TOP,
            egui::vec2(-EDGE_OFFSET, DOCK_TOP_CLEARANCE),
        )
    };
    let zoom_offset = dock
        .zoom_strip_pos
        .map(|p| egui::vec2(p.x, p.y))
        .unwrap_or(zoom_default);
    let mut zoom_state = super::widgets::ZoomControlsState {
        pos: dock.zoom_strip_pos.map(|p| [p.x, p.y]),
        show_scale_text: true,
    };
    let can_zoom_in = !zoom.at_inner_limit(view.scale);
    let can_zoom_out = !zoom.at_outer_limit(view.scale);
    // 三个回调都需要 `&mut view` + `&mut cameras`；闭包同时持三份会撞借用。
    // 折中：闭包内只产生**意图**（写三个互斥 bool），闭包外统一执行。
    // 同时存目标的 `target_scale`，让 `set_map_scale` 在外边单写。
    let mut zoom_in_clicked = false;
    let mut zoom_out_clicked = false;
    let mut zoom_fit_clicked = false;
    egui::Area::new(egui::Id::new("wv_zoom_controls_v01"))
        .anchor(zoom_anchor, zoom_offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let current_scale = view.scale;
            super::widgets::zoom_controls(
                ui,
                &shell,
                &mut zoom_state,
                current_scale,
                can_zoom_in,
                can_zoom_out,
                || zoom_in_clicked = true,
                || zoom_out_clicked = true,
                || zoom_fit_clicked = true,
            );
        });
    if zoom_in_clicked {
        let target = view.scale / 1.6;
        set_map_scale(&mut view, &mut cameras, target, zoom);
    } else if zoom_out_clicked {
        let target = view.scale * 1.6;
        set_map_scale(&mut view, &mut cameras, target, zoom);
    } else if zoom_fit_clicked {
        let frame_origin = drill
            .current()
            .and_then(|id| wv_core::resolve_frame_in(&projection.world.maps, id))
            .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
            .unwrap_or(grid.map_origin);
        view.camera_center = frame_origin + level_map_size * 0.5;
        set_map_scale(&mut view, &mut cameras, zoom.fit, zoom);
    }
    // strip 的拖动位置回灌到 dock（落盘由 caller 负责）。
    dock.zoom_strip_pos = zoom_state.pos.map(|[x, y]| MapDockPos { x, y });

    let _ = line_active;
}

/// 工具瓷砖：图标 +（横向时）快捷键字母。
///
/// 点击只排队 [`CommandId::SelectMapTool`]——互斥与状态写入统一在分发器，
/// 本函数不碰任何工具资源（见模块头「交互与架构边界」）。
#[allow(clippy::too_many_arguments)]
fn tool_tile(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    letter: &str,
    hint: &str,
    active: bool,
    palette: &mut CommandPalette,
    tool: MapToolId,
) {
    let (rect, resp) = ui.allocate_exact_size(metrics.tile, egui::Sense::click());
    let hovered = resp.hovered();
    let fill = if active {
        shell.gold_soft()
    } else if hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if active {
        egui::Stroke::new(1.0, shell.gold())
    } else {
        egui::Stroke::NONE
    };
    ui.painter()
        .rect(rect, TILE_RADIUS, fill, stroke, egui::StrokeKind::Inside);

    let tint = if active {
        shell.gold()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    match metrics.label {
        // 横向（Figma）：图标上、字母下。
        Some(label_size) => {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.top() + 2.0 + metrics.tile_icon * 0.5),
                egui::Vec2::splat(metrics.tile_icon),
            );
            icon_image(icon, tint, metrics.tile_icon).paint_at(ui, icon_rect);
            ui.painter().text(
                egui::pos2(rect.center().x, rect.top() + 28.0),
                egui::Align2::CENTER_CENTER,
                letter,
                mono(label_size),
                tint,
            );
        }
        // 纵向紧凑：纯图标居中（字母只保留在横向，纵向 34px 内读不清）。
        None => {
            icon_image(icon, tint, metrics.tile_icon).paint_at(
                ui,
                egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.tile_icon)),
            );
        }
    }

    if resp.clicked() {
        palette.pending = Some(CommandId::SelectMapTool(tool));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint);
}

/// 图标瓷砖：仅图标、无字母（视图 / 导航组）。
fn icon_tile(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    active: bool,
    hint: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(metrics.tile, egui::Sense::click());
    let hovered = resp.hovered();
    let fill = if active {
        shell.gold_soft()
    } else if hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if active {
        egui::Stroke::new(1.0, shell.gold())
    } else {
        egui::Stroke::NONE
    };
    ui.painter()
        .rect(rect, TILE_RADIUS, fill, stroke, egui::StrokeKind::Inside);
    let tint = if active {
        shell.gold()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.tile_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.tile_icon)),
    );

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// 图标钮（Figma `Undo` / `Redo` / `Snapping` 32×32）。
///
/// `active` 走 `accent_soft` 底 + accent 图标（Figma magnet 的启用态）；
/// `enabled=false` 用禁用色且不可点（撤销栈空 / 重做栈空）。
fn icon_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    active: bool,
    enabled: bool,
    hint: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::splat(metrics.icon_btn),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hovered = enabled && resp.hovered();
    let fill = if active {
        shell.accent_soft()
    } else if hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, TILE_RADIUS, fill);

    let tint = if !enabled {
        shell.text_disabled()
    } else if active {
        shell.accent()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.icon_btn_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.icon_btn_icon)),
    );

    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(hint)
    } else {
        resp
    }
}

/// 分隔条：横向布局画 1×24 竖线，纵向布局画 24×1 横线（Figma `Tool separator`）。
fn separator(ui: &mut egui::Ui, shell: &UiShell, metrics: DockMetrics, vertical: bool) {
    let size = if vertical {
        egui::vec2(metrics.sep_len, 1.0)
    } else {
        egui::vec2(1.0, metrics.sep_len)
    };
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let stroke = egui::Stroke::new(1.0, shell.border_subtle());
    let [a, b] = if vertical {
        [rect.left_center(), rect.right_center()]
    } else {
        [rect.center_top(), rect.center_bottom()]
    };
    ui.painter().line_segment([a, b], stroke);
}

/// 拖动手柄 + 朝向切换按钮（grip-vertical，对应 Figma `3:21848` 的 grip）。
///
/// **双重语义**：
/// - 拖动（`Sense::drag`）→ 整条工具条跟随，调用方累计 `drag_delta` 到
///   `MapDockState::pos`。
/// - 单击（`Sense::click`）→ 切换朝向（与拖动不冲突：拖动距离足够大时
///   `clicked()` 不会触发——egui 0.36 内部阈值）。
///
/// 视觉上与 `icon_button` 风格一致：t1 文字色，hover 走 `hover_bg`，
/// 鼠标在把柄上时**显 `Grab` 光标**提示可拖。
fn grip_handle(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    hint: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::splat(metrics.icon_btn),
        egui::Sense::click_and_drag(),
    );
    let hovered = resp.hovered();
    let fill = if hovered || resp.dragged() {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, TILE_RADIUS, fill);
    let tint = if hovered || resp.dragged() {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.icon_btn_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.icon_btn_icon)),
    );
    if resp.dragged() {
        resp.on_hover_cursor(egui::CursorIcon::Grabbing)
    } else {
        resp.on_hover_cursor(egui::CursorIcon::Grab)
    }
    .on_hover_text(hint)
}

/// 写入缩放并同步相机，避免 ViewState 与 Camera 变成两个真相。
fn set_map_scale(
    view: &mut ViewState,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    scale: f32,
    zoom: wv_render::zoom_level::LevelZoom,
) {
    view.scale = zoom.clamp(scale);
    wv_render::camera::apply_view_to_camera(view.camera_center, view.scale, cameras);
}

#[cfg(test)]
mod map_control_tests {
    use super::*;
    use bevy::prelude::Vec2;

    /// `＋/－` 每次 1.6×（`view.scale / 1.6`、`* 1.6`），经 `set_map_scale` 夹在
    /// **当前级**区间内：连按到底应停在内界/外界，而不是穿过去。
    #[test]
    fn zoom_button_steps_stop_at_the_levels_band() {
        let zoom = wv_render::zoom_level::LevelZoom::for_map(
            Vec2::splat(1024.0),
            Vec2::new(1280.0, 800.0),
        );
        let mut s = zoom.fit;
        for _ in 0..40 {
            s = zoom.clamp(s / 1.6);
        }
        assert!((s - zoom.min).abs() < 1e-6, "got {s}, min {}", zoom.min);
        let mut s = zoom.fit;
        for _ in 0..40 {
            s = zoom.clamp(s * 1.6);
        }
        assert!((s - zoom.max).abs() < 1e-6, "got {s}, max {}", zoom.max);
    }

    /// 朝向持久化键 round-trip；不可识别值静默回退（V1）。
    #[test]
    fn dock_orientation_round_trips_through_key() {
        for o in [MapDockOrientation::Vertical, MapDockOrientation::Horizontal] {
            assert_eq!(MapDockOrientation::from_key(o.key()), Some(o));
        }
        assert_eq!(MapDockOrientation::from_key("diagonal"), None);
        assert_eq!(
            MapDockOrientation::Vertical.toggled(),
            MapDockOrientation::Horizontal
        );
        assert_eq!(
            MapDockOrientation::Horizontal.toggled(),
            MapDockOrientation::Vertical
        );
    }

    /// 横向内容宽度（15 个元素 + 3 分隔 + 17 间距 + 左右内边距）必须在
    /// 1280 窗口内留出两侧空白——否则贴底居中的工具条会被窗口裁掉。
    #[test]
    fn horizontal_dock_fits_a_1280_window() {
        let m = DockMetrics::for_orientation(false);
        let content = 11.0 * m.tile.x + 4.0 * m.icon_btn + 3.0 * 1.0 + 17.0 * m.gap;
        let framed = content + 2.0 * m.pad_x;
        assert!(framed < 1280.0, "横向工具条 {framed}px 超出 1280 窗口宽度");
    }

    /// 纵向紧凑形态必须在时间轴之上收口：800 高窗口 − 顶栏 38 − 时间轴
    /// 约 152 − 边距，可用高度约 600；13 项按横向尺寸纵排会超（实测截图
    /// 压到时间轴），紧凑尺寸必须留出余量。
    #[test]
    fn vertical_dock_fits_above_the_timeline() {
        let m = DockMetrics::for_orientation(true);
        let height = 11.0 * m.tile.y + 4.0 * m.icon_btn + 3.0 * 1.0 + 17.0 * m.gap + 2.0 * m.pad_y;
        let available = 800.0 - 152.0;
        assert!(
            DOCK_TOP_CLEARANCE + height < available,
            "纵向工具条到 {:.0}px，与时间轴（顶约 {:.0}px）重叠",
            DOCK_TOP_CLEARANCE + height,
            available
        );
    }
}
