//! 地图工具条（2026-10-20 重构 · ADR-0043；原对图 Figma `3:21848` 平铺版）。
//!
//! ## 职责
//! - `map_controls` —— 地图工具条。**横 / 纵两种朝向**，首个按钮切换；
//!   朝向与吸附开关随工程落盘到 `ui-layout.yaml`（[`MapDockState`]）。
//! - `MapDockState` / `MapDockOrientation` —— 工具条偏好（朝向 + 吸附），
//!   由 `ui_shell` 的布局读写系统负责持久化。
//! - `tool_tile` / `icon_tile` / `icon_button` / `separator` / `grip_handle`
//!   —— 本模块私有的自绘控件，严格按 Figma 规格（40×38 工具瓷砖、32×32
//!   图标钮、1×24 分隔条）。
//!
//! ## 布局（ADR-0043 · 11 槽 + 1；2026-10-23 起 12 项）
//!
//! ```text
//! ⇅    ✋    ┊    ◆◢    ◻    ┊    📏◢   🗑    ┊    ↷   ↶   ▦
//! grip pan  sep 选择组 橡皮  sep  测量组 清除  sep  redo undo grid
//! ```
//!
//! 「选择组」与「测量组」是两枚**分裂按钮**（[`wv_ui::widgets::split_button`]）：
//! 主区点按 = 立即用当前子项，右下角小三角 = 弹出菜单换子项。主区图标**跟着
//! 当前子项走**（选了矩形，主区就显示方块）。
//!
//! | 槽 | 内容 | 快捷键 |
//! |---|---|---|
//! | 0 | grip：拖动 = 移动工具条，单击 = 切换横/纵 | — |
//! | 1 | 抓手 | `H` |
//! | 3 | 选择组：多边形（默认）/ 矩形 / 椭圆 | `V` |
//! | 4 | 地形橡皮刷（擦回本图底图） | `E` |
//! | 6 | 测量组：线段距离（默认）/ 面积 | `L` / `M` |
//! | 6.5 | 清除全部测量（当前图；可一次撤销） | — |
//! | 8/9 | 重做 / 撤销 | `Ctrl+Shift+Z` / `Ctrl+Z` |
//! | 10 | 网格开关（开启高亮） | `G` |
//!
//! ## 与 v0.6 的差异（ADR-0043 裁定）
//!
//! 画笔（B）、生成大陆（P）、磁吸三枚瓷砖**退出工具条**——快捷键与命令面板
//! 入口原样保留；`MapToolId::Brush / Continent / MeasureEraser` 变体不删，
//! 删了会让那两个入口静默失效。测量橡皮同样只剩命令面板入口（`E` 键让位给
//! 地形橡皮刷，两把刷子一个键位族）。
//!
//! ## 交互与架构边界
//! - 工具切换**不直接改状态**：点击只把 `CommandId::SelectMapTool(_)` 排进
//!   [`CommandPalette::pending`]，由 `ui_shell::apply_pending_commands`
//!   统一做互斥（开一个工具 = 关掉其它工具）。快捷键同源，避免两套逻辑漂移。
//! - 视图组（网格）与「切换朝向」直接写 [`ViewState`] / [`MapDockState`]——
//!   它们不改世界数据，也不涉及工具互斥。
//! - 不在无工程时显示（与 `welcome_screen` 共用 [`StartupLoadRequest`] 信号）。
//!
//! ## 视觉规格（Figma `3:21848`，经 shell 令牌投影）
//! - 大框：`float_fill`（暗色 `#1B232A @ .96`）+ `border_strong`（`#46525B`）
//!   + 圆角 10 + 投影 `0 8 24 rgba(0,0,0,.2)`。
//! - 工具瓷砖 40×38（纵排紧凑 34×34）：活动态 `gold_soft` 底 + `gold` 描边
//!   与前景；分裂按钮的小三角画在右下角 13px 命中区正中。
//! - 图标钮 32×32：活动态 `accent_soft` 底 + accent 图标。
//! - 分隔条 1×24，色 `border_subtle`。
//! - 间距 6px；横向贴底居中，纵向贴顶右缘，均距窗体边缘 18px。
//!
//! ## 主要导出
//! - [`map_controls`] —— Bevy 系统。
//! - [`MapDockState`] / [`MapDockOrientation`] / [`DockParams`] —— 持久化偏好
//!   与参数包。

#![cfg(feature = "app")]

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use serde::{Deserialize, Serialize};

use crate::edit::editing::EditHistory;
use crate::i18n::{t, CurrentLocale, I18nKey, Locale};
use crate::state::{
    DrillState, GridConfig, MeasureMode, MeasureState, Panel, PanelState, ViewMode, ViewState,
    WorldProjection,
};
use crate::theme::ui_theme::{margin_xy, UiShellConfig};
use crate::ui_command_palette::{CommandId, CommandPalette, MapToolId};
use wv_render::state::{BrushState, BrushTool, SelectionShape, SelectionState};
use wv_ui::widgets::{split_button, SplitButtonMetrics, SplitMenuItem};

use super::shell::UiShell;
use super::widgets::icon_image;
use super::widgets::{
    ICON_ELLIPSE, ICON_MOVE_HORIZONTAL, ICON_MOVE_VERTICAL, ICON_PAINT_BUCKET,
    ICON_RULER_DIMENSION_LINE, ICON_SQUARES_UNITE, ICON_SQUARE_MOUSE_POINTER, ICON_TRASH,
    ICON_VECTOR_POLYGON, ICON_VECTOR_SQUARE,
};
use super::{ICON_ERASER, ICON_GRID, ICON_HAND, ICON_REDO, ICON_TREES, ICON_UNDO, ICON_WAVES};

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
/// - **横向**按 Figma `3:21848` 的瓷砖规格：瓷砖 40×38、图标钮 32×32、
///   间距 6、大框内边距 10/7。
/// - **纵向**是紧凑变体：瓷砖 34×34（图标 18）、图标钮 28×28、间距 4、
///   内边距 6/10。原因见 [`DOCK_TOP_CLEARANCE`] 与实测截图：13 项按 40×38
///   纵排会侵占时间轴。
///
/// 两种朝向都**不画快捷键字母**（2026-10-20 用户裁定）：分裂按钮不占字母位
/// 之后，仅剩的几枚纯工具瓷砖带字母反而显得孤立，字母统一退到 tooltip。
#[derive(Clone, Copy)]
struct DockMetrics {
    tile: egui::Vec2,
    tile_icon: f32,
    icon_btn: f32,
    icon_btn_icon: f32,
    gap: f32,
    sep_len: f32,
    pad_x: f32,
    pad_y: f32,
    /// 分裂按钮右下角小三角的命中区边长。
    chevron_hit: f32,
}

impl DockMetrics {
    fn for_orientation(vertical: bool) -> Self {
        if vertical {
            Self {
                tile: egui::vec2(34.0, 34.0),
                tile_icon: 18.0,
                icon_btn: 28.0,
                icon_btn_icon: 16.0,
                gap: 4.0,
                sep_len: 20.0,
                pad_x: 6.0,
                pad_y: 10.0,
                chevron_hit: 12.0,
            }
        } else {
            Self {
                tile: egui::vec2(40.0, 38.0),
                tile_icon: 16.0,
                icon_btn: 32.0,
                icon_btn_icon: 16.0,
                gap: 6.0,
                sep_len: 24.0,
                pad_x: 10.0,
                pad_y: 7.0,
                chevron_hit: 13.0,
            }
        }
    }

    /// 投影成 [`SplitButtonMetrics`]——几何只在尺寸表里定义一次。
    ///
    /// 小三角方位随朝向（2026-10-20 用户裁定，两轮迭代终版）：横排 =
    /// 右缘垂直居中（右下角会把图标往上挤），纵排 = 下缘水平居中（图标正
    /// 下方）。两种方位三角都朝下。
    fn split_metrics(&self, vertical: bool) -> SplitButtonMetrics {
        SplitButtonMetrics {
            tile: self.tile,
            icon: self.tile_icon,
            chevron_hit: self.chevron_hit,
            radius: TILE_RADIUS,
            chevron_side: if vertical {
                wv_ui::widgets::ChevronSide::BottomCenter
            } else {
                wv_ui::widgets::ChevronSide::RightCenter
            },
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
    ///
    /// ADR-0043 后工具条上没有这枚瓷砖了；开关仍在（意图菜单 → 划分势力
    /// 读取），`ui-layout.yaml` 的读写两端都不动。
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
            // 与「划分势力默认贴地」的历史默认（`DivisionMode::Snap`）一致。
            snapping: true,
            pos: None,
            zoom_strip_pos: None,
        }
    }
}

/// 工具条要写的三份资源（打包成一个 SystemParam）。
///
/// **为什么打包**：`map_controls` 的参数顶在 Bevy 16 元组上限（v0.6 就已是
/// 16 个）；ADR-0043 要再加 `Res<CurrentLocale>`（tooltip 走 i18n），不打包
/// 就超限。三个资源在语义上也确实是一伙的——「工具条自身的输入输出」，
/// 与视图 / 世界资源不同伙。
#[derive(SystemParam)]
pub struct DockParams<'w> {
    pub dock: ResMut<'w, MapDockState>,
    pub palette: ResMut<'w, CommandPalette>,
    pub locale: Res<'w, CurrentLocale>,
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
    screen: Res<crate::app_screen::AppScreen>,
    mut params: DockParams,
    edit: Res<EditHistory>,
    panels: Res<PanelState>,
    // CellMap 工具活动态（[ADR-0052] D7-1 EditCell 第 12 槽 / D6 FloodSelect
    // 第 13 槽）——两块瓷砖都按 `tool_active` 画选中态。
    // 时间轴播放态（§9.4 Gate-1）：播放中 CellMap 编辑类瓷砖灰化。
    //
    // **折成一层元组**：Bevy 单系统参数上限 16，本函数原有 15 个，三个
    // `Res` 分开写会到 18（编译报「not a system set」）。元组算 1 个参数。
    (edit_cell, flood_select, playback): (
        Res<wv_render::state::EditCellState>,
        Res<crate::state::FloodSelectState>,
        Res<crate::state::PlaybackState>,
    ),
) {
    // 无工程闸门（图片规格 §首页不可见）：与 `welcome_screen` 共用同一信号。
    // 2026-10-22（B 档）：改读 `AppScreen`——「首页不可见」的口径由屏状态
    // 一处裁决，不再在本系统重拼 `has_open_project || 加载中`。
    if !screen.editor_visible() {
        return;
    }

    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = params.locale.0;

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
    let eraser_active = brush.enabled && brush.tool == BrushTool::Eraser;
    let measure_active = measure.mode.is_active();
    // 清除按钮的可用态：当前图真有测量才点得动（空图按下去只会弹「没有测量」）。
    let has_measurements = drill
        .current()
        .and_then(|id| projection.world.measurement_of(id))
        .is_some_and(|o| !o.measurements.is_empty());
    let grid_active = view.grid_lines;
    let can_undo = edit.history.can_undo();
    let can_redo = edit.history.can_redo();
    // EditCell 活动态（[ADR-0052] D7-1）：判据 = `tool_active`，
    // **不是** `selected.is_some()`——后者把「点过一格」误当成「工具开着」。
    let edit_cell_active = edit_cell.tool_active;
    // 泛洪活动态（[ADR-0052] D6）：判据 = `tool_active`。
    let flood_select_active = flood_select.tool_active;
    // **Gate-1**（[ADR-0052] §9.4）：时间轴播放中，CellMap 写入口工具灰化。
    // 禁工具而非禁面板——播放中看读数条是合法只读操作。
    let cell_tools_enabled = !playback.playing;
    let locked_hint = t(loc, I18nKey::MapControlsPlaybackLocked);

    let vertical = params.dock.orientation == MapDockOrientation::Vertical;
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
    // 是「贴边 -18」，而是任意 (x, y)。
    let offset = params
        .dock
        .pos
        .map(|p| egui::vec2(p.x, p.y))
        .unwrap_or(default_offset);
    let margin = margin_xy(metrics.pad_x, metrics.pad_y);

    egui::Area::new(egui::Id::new("wv_map_controls_v07"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // 圆角大框：float_fill 底 + border_strong 描边 + 投影。
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
                        // ── 槽 0：朝向切换 + 拖动手柄（grip-vertical） ─────
                        let (swap_icon, swap_hint) = if vertical {
                            (ICON_MOVE_HORIZONTAL, "拖动移动 / 单击切换为横向工具条")
                        } else {
                            (ICON_MOVE_VERTICAL, "拖动移动 / 单击切换为纵向工具条")
                        };
                        let grip_resp = grip_handle(ui, &shell, metrics, swap_icon, swap_hint);
                        if grip_resp.dragged() {
                            let pos = params.dock.pos.get_or_insert(MapDockPos {
                                x: default_offset.x,
                                y: default_offset.y,
                            });
                            pos.x += grip_resp.drag_delta().x;
                            pos.y += grip_resp.drag_delta().y;
                        }
                        if grip_resp.clicked() {
                            params.dock.orientation = params.dock.orientation.toggled();
                            // 朝向切换后旧 pos 不再合身：清掉，回到默认锚点。
                            params.dock.pos = None;
                        }

                        // ── 槽 1：抓手 ────────────────────────────────────
                        // 抓手是**导航**工具，不是写入口——Gate-1 不灰化它
                        // （播放中仍可拖动看图，属只读操作）。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_HAND,
                            t(loc, I18nKey::MapControlsPanHint),
                            pan_active,
                            true,
                            &mut params.palette,
                            MapToolId::Pan,
                        );

                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 3：选择组（分裂按钮） ──────────────────────
                        selection_split(
                            ui,
                            &shell,
                            metrics,
                            vertical,
                            loc,
                            selection.shape,
                            select_active,
                            &mut params.palette,
                        );

                        // ── 槽 4：地形橡皮刷 ─────────────────────────────
                        // Gate-1：播放中灰化（写入口）。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_ERASER,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsEraserHint)
                            } else {
                                locked_hint
                            },
                            eraser_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::BrushErase,
                        );
                        // EditCell 工具的 dock 瓷砖见下方「槽 12」
                        // （[ADR-0052] D7-1 已补齐入口）。

                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 6：测量组（分裂按钮） ──────────────────────
                        measure_split(
                            ui,
                            &shell,
                            metrics,
                            vertical,
                            loc,
                            measure.mode,
                            measure_active,
                            &mut params.palette,
                        );

                        // ── 槽 6.5：清除全部测量（2026-10-23 用户需求）────
                        // 与测量组相邻：紧贴「测量」右侧，一眼归到测量这一族。
                        // 语义 = 一键清空当前图的全部测量（可一次撤销）；删**一条**
                        // 仍走测量橡皮 / `Del`（渲染层输入，不经本工具条）。
                        // 编号取 6.5（不重排后续槽位）：沿用 ADR-0043 的 0–10 号
                        // 与 isolines「10.5」同款做法，历史注释不必整段改号。
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_TRASH,
                            false,
                            has_measurements,
                            t(loc, I18nKey::MapControlsClearMeasurements),
                        )
                        .clicked()
                        {
                            params.palette.pending = Some(CommandId::ClearMeasurements);
                        }

                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 8/9：重做 / 撤销 ───────────────────────────
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
                            params.palette.pending = Some(CommandId::Redo);
                        }
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
                            params.palette.pending = Some(CommandId::Undo);
                        }

                        // ── 槽 10：网格 ──────────────────────────────────
                        if icon_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_GRID,
                            grid_active,
                            "显示参照网格（G）",
                        )
                        .clicked()
                        {
                            view.grid_lines = !view.grid_lines;
                        }

                        // ── 槽 10.5：等值线（2026-10-23 验收裁定）────────
                        // 会话开关（`view.isolines`，默认关）：等值线默认是
                        // 「海面黑线修饰」，要时再开。仅在 pass 有产出的两
                        // 模式显示（VectorFlat 分析参照 / Topographic 数据视图）。
                        if matches!(view.mode, ViewMode::VectorFlat | ViewMode::Topographic)
                            && icon_tile(
                                ui,
                                &shell,
                                metrics,
                                ICON_WAVES,
                                view.isolines,
                                "显示等值线",
                            )
                            .clicked()
                        {
                            view.isolines = !view.isolines;
                        }

                        // ── 槽 11：自然区（2026-10-22 内容矩阵 §2.6）──────
                        // CellMap 里显示 Nature 类区域 + 未命名候选高亮；
                        // 仅 CellMap（Topographic 工作区）显示本按钮——
                        // 其他模式区域层不受开关控制（用户裁定：只在 cell map）。
                        if matches!(view.mode, ViewMode::Topographic)
                            && icon_tile(
                                ui,
                                &shell,
                                metrics,
                                ICON_TREES,
                                view.nature_overlay,
                                "显示自然区与候选（CellMap）",
                            )
                            .clicked()
                        {
                            view.nature_overlay = !view.nature_overlay;
                        }

                        // ── 分隔条：通用地图工具 ↔ CellMap 编辑工具 ──
                        // [ADR-0052] §4.3（UX 审视）：槽 0–11 是「通用地图工具」
                        // （导航 / 选择 / 测量 / 视图 / 历史），槽 12+ 是
                        // 「CellMap 专属编辑工具」——语义层级不同，必须视觉隔断。
                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 12：EditCell（单格编辑）────────────────
                        // [ADR-0052] D7-1。此前 EditCell 只有后端 plumbing
                        //（命令 + 状态 + 点选），**工具条没有可见入口**——
                        // 用户无法从 UI 打开单格编辑工作台。
                        // 二次点击 = 关闭（互斥由 `apply_pending_commands` 统一做）。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_SQUARE_MOUSE_POINTER,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsEditCellHint)
                            } else {
                                locked_hint
                            },
                            edit_cell_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::EditCell,
                        );

                        // ── 槽 13：FloodSelect（泛洪选择）────────────
                        // [ADR-0052] D6：点一下 = 按判据自动圈出同值连通区。
                        // 与槽 12 同属「CellMap 编辑」组，不另加分隔条。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_PAINT_BUCKET,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsFloodSelectHint)
                            } else {
                                locked_hint
                            },
                            flood_select_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::FloodSelect,
                        );
                    };

                    // **不用**交叉轴 `Align::Center`（2026-10-20 教训）：大框的
                    // 内容 Ui 铺满整屏，Center 会把小钮摆到屏幕中央，而
                    // `Frame` 按 `min_rect` 收缩——min_rect 从锚点一路扩张到
                    // 屏幕中央，整条工具条鼓成一块盖住半个屏幕的面板。
                    // 对齐改由**槽位统一**解决：小钮 / 分隔条与瓷砖占同样的
                    // `metrics.tile` 槽位，视觉内容在槽内居中（见 `icon_button`
                    // / `grip_handle` / `separator`），帧自然贴合。
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
    let zoom_offset = params
        .dock
        .zoom_strip_pos
        .map(|p| egui::vec2(p.x, p.y))
        .unwrap_or(zoom_default);
    let mut zoom_state = super::widgets::ZoomControlsState {
        pos: params.dock.zoom_strip_pos.map(|p| [p.x, p.y]),
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
    params.dock.zoom_strip_pos = zoom_state.pos.map(|[x, y]| MapDockPos { x, y });
}

/// 槽 3 —— 选择组的分裂按钮（ADR-0043）。
///
/// 主区图标**跟着当前形状走**：多边形显示 `vector-polygon`、矩形显示
/// `vector-square`、椭圆显示 `ellipse`。点主区 = 用当前形状开关选择工具；
/// 点小三角 = 弹层里换形状（换形状即重开选择工具——分发器语义：
/// 不同形状开着 = 换，不关）。
///
/// 两个待发命令用两个局部变量接（主区回调是 `FnOnce`、选中回调是 `FnMut`，
/// 两者不能同时可变借用同一份 `palette`），闭包外统一入队——与缩放条
/// 三 bool 模式同款。
#[allow(clippy::too_many_arguments)]
fn selection_split(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    vertical: bool,
    loc: Locale,
    shape: SelectionShape,
    active: bool,
    palette: &mut CommandPalette,
) {
    let items = [
        SplitMenuItem::new(
            ICON_VECTOR_POLYGON,
            t(loc, I18nKey::MapControlsSelectPolygon),
        )
        .with_hint("V"),
        SplitMenuItem::new(ICON_VECTOR_SQUARE, t(loc, I18nKey::MapControlsSelectRect)),
        SplitMenuItem::new(ICON_ELLIPSE, t(loc, I18nKey::MapControlsSelectEllipse)),
    ];
    let selected = match shape {
        SelectionShape::Polygon => 0,
        SelectionShape::Rect => 1,
        SelectionShape::Ellipse => 2,
    };
    let icon = match shape {
        SelectionShape::Polygon => ICON_VECTOR_POLYGON,
        SelectionShape::Rect => ICON_VECTOR_SQUARE,
        SelectionShape::Ellipse => ICON_ELLIPSE,
    };
    let hint = match shape {
        SelectionShape::Polygon => t(loc, I18nKey::MapControlsSelectGroupHint),
        SelectionShape::Rect => t(loc, I18nKey::MapControlsSelectRectHint),
        SelectionShape::Ellipse => t(loc, I18nKey::MapControlsSelectEllipseHint),
    };
    let mut main_cmd: Option<CommandId> = None;
    let mut pick_cmd: Option<CommandId> = None;
    split_button(
        ui,
        shell,
        "wv_split_select",
        metrics.split_metrics(vertical),
        icon,
        active,
        hint,
        &items,
        selected,
        || main_cmd = Some(CommandId::SelectMapTool(MapToolId::Select(shape))),
        |i| {
            let picked = [
                SelectionShape::Polygon,
                SelectionShape::Rect,
                SelectionShape::Ellipse,
            ][i];
            pick_cmd = Some(CommandId::SelectMapTool(MapToolId::Select(picked)));
        },
    );
    if let Some(c) = main_cmd.or(pick_cmd) {
        palette.pending = Some(c);
    }
}

/// 槽 6 —— 测量组的分裂按钮（ADR-0043）。
///
/// 两项语义（用户裁定：功能不变，只换图标）：
/// - **线段距离**（`ruler-dimension-line`，默认）= 现有折线测量
///   （多点累积，每段读数 + Σ 总长）；
/// - **面积**（`squares-unite`）= 现有矩形测量（两点定对角，长 × 宽）。
#[allow(clippy::too_many_arguments)]
fn measure_split(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    vertical: bool,
    loc: Locale,
    mode: MeasureMode,
    active: bool,
    palette: &mut CommandPalette,
) {
    let items = [
        SplitMenuItem::new(
            ICON_RULER_DIMENSION_LINE,
            t(loc, I18nKey::MapControlsMeasureDistance),
        )
        .with_hint("L"),
        SplitMenuItem::new(ICON_SQUARES_UNITE, t(loc, I18nKey::MapControlsMeasureArea))
            .with_hint("M"),
    ];
    // Off 时主区显示默认项（线段距离）——「默认工具」的含义就是没开过也长这样。
    let rect_mode = mode == MeasureMode::Rect;
    let icon = if rect_mode {
        ICON_SQUARES_UNITE
    } else {
        ICON_RULER_DIMENSION_LINE
    };
    let hint = if rect_mode {
        t(loc, I18nKey::MapControlsMeasureRectHint)
    } else {
        t(loc, I18nKey::MapControlsMeasurePolylineHint)
    };
    let mut main_cmd: Option<CommandId> = None;
    let mut pick_cmd: Option<CommandId> = None;
    split_button(
        ui,
        shell,
        "wv_split_measure",
        metrics.split_metrics(vertical),
        icon,
        active,
        hint,
        &items,
        usize::from(rect_mode),
        || {
            main_cmd = Some(CommandId::SelectMapTool(if rect_mode {
                MapToolId::MeasureRect
            } else {
                MapToolId::MeasureLine
            }))
        },
        |i| {
            pick_cmd = Some(CommandId::SelectMapTool(if i == 1 {
                MapToolId::MeasureRect
            } else {
                MapToolId::MeasureLine
            }));
        },
    );
    if let Some(c) = main_cmd.or(pick_cmd) {
        palette.pending = Some(c);
    }
}

/// 工具瓷砖：纯图标居中（2026-10-20 用户裁定：不画快捷键字母，字母退到
/// tooltip 与 i18n 提示里）。
///
/// 点击只排队 [`CommandId::SelectMapTool`]——互斥与状态写入统一在分发器，
/// 本函数不碰任何工具资源。
#[allow(clippy::too_many_arguments)]
fn tool_tile(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    hint: &str,
    active: bool,
    enabled: bool,
    palette: &mut CommandPalette,
    tool: MapToolId,
) {
    // `enabled = false`（[ADR-0052] §9.4 Gate-1/Gate-2）：灰化不可点。
    // `Sense::hover` 而非 `click`——不是「点了不响应」，而是**根本不成命中区**，
    // 连误触的机会都不给。
    let (rect, resp) = ui.allocate_exact_size(
        metrics.tile,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hovered = enabled && resp.hovered();
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

    let tint = if !enabled {
        shell.text_disabled()
    } else if active {
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

    if enabled && resp.clicked() {
        palette.pending = Some(CommandId::SelectMapTool(tool));
    }
    if !enabled {
        // 禁用态不给手型光标——避免「看起来能点」的误判（与 `icon_button` 同口径）。
        resp.on_hover_text(hint);
        return;
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
/// `active` 走 `accent_soft` 底 + accent 图标；
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
    // 槽位对齐（2026-10-20）：小钮与瓷砖占**同样大小**的槽（`metrics.tile`），
    // 视觉钮（28/32px）画在槽正中。这样纵向排里小钮不会贴左、横向排里不会
    // 贴顶，整条天然成列；命中区也比视觉钮大一圈，更好点。
    let (slot, resp) = ui.allocate_exact_size(
        metrics.tile,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let rect = egui::Rect::from_center_size(slot.center(), egui::Vec2::splat(metrics.icon_btn));
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
///
/// 占一个完整瓷砖槽（竖排 = tile 宽 ×1，横排 = 1×tile 高），线段在槽内居中
/// ——与 [`icon_button`] 的槽位对齐同一口径。
fn separator(ui: &mut egui::Ui, shell: &UiShell, metrics: DockMetrics, vertical: bool) {
    let size = if vertical {
        egui::vec2(metrics.tile.x, 1.0)
    } else {
        egui::vec2(1.0, metrics.tile.y)
    };
    let (slot, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let stroke = egui::Stroke::new(1.0, shell.border_subtle());
    let [a, b] = if vertical {
        let y = slot.center().y;
        let half = metrics.sep_len * 0.5;
        [
            egui::pos2(slot.center().x - half, y),
            egui::pos2(slot.center().x + half, y),
        ]
    } else {
        let x = slot.center().x;
        let half = metrics.sep_len * 0.5;
        [
            egui::pos2(x, slot.center().y - half),
            egui::pos2(x, slot.center().y + half),
        ]
    };
    ui.painter().line_segment([a, b], stroke);
}

/// 拖动手柄 + 朝向切换按钮（grip-vertical）。
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
    // 槽位对齐同 `icon_button`：占满一个瓷砖槽，视觉钮画在槽正中。
    let (slot, resp) = ui.allocate_exact_size(metrics.tile, egui::Sense::click_and_drag());
    let rect = egui::Rect::from_center_size(slot.center(), egui::Vec2::splat(metrics.icon_btn));
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

    /// 横向内容宽度：**12 个等宽槽**（5 瓷砖 + 4 图标钮 + 3 分隔，槽位统一
    /// 后全部 = `tile.x`）+ 11 间距 + 左右内边距。必须在 1280 窗口内留出两侧
    /// 空白——否则贴底居中的工具条会被窗口裁掉。**元素数与 `slots` 闭包的
    /// 排布一一对应**，改布局必须同步改这条。
    ///
    /// 2026-10-23：「清除全部测量」图标钮插在测量组右侧（槽 6.5），
    /// 图标钮 3 → 4、槽位 11 → 12。
    #[test]
    fn horizontal_dock_fits_a_1280_window() {
        let m = DockMetrics::for_orientation(false);
        let content = 12.0 * m.tile.x + 11.0 * m.gap;
        let framed = content + 2.0 * m.pad_x;
        assert!(framed < 1280.0, "横向工具条 {framed}px 超出 1280 窗口宽度");
    }

    /// 纵向紧凑形态必须在时间轴之上收口：800 高窗口 − 顶栏 38 − 时间轴
    /// 约 152 − 边距，可用高度约 648；12 个等高槽纵排必须留出余量。
    /// **元素数与 `slots` 闭包一一对应**，改布局必须同步改这条。
    #[test]
    fn vertical_dock_fits_above_the_timeline() {
        let m = DockMetrics::for_orientation(true);
        let height = 12.0 * m.tile.y + 11.0 * m.gap + 2.0 * m.pad_y;
        let available = 800.0 - 152.0;
        assert!(
            DOCK_TOP_CLEARANCE + height < available,
            "纵向工具条到 {:.0}px，与时间轴（顶约 {:.0}px）重叠",
            DOCK_TOP_CLEARANCE + height,
            available
        );
    }

    /// 分裂按钮的几何必须从 `DockMetrics` 单点投影——两处各写一遍尺寸表，
    /// 早晚漂移成「弹层锚点歪了」这类难查的视觉 bug。
    #[test]
    fn split_metrics_project_from_dock_metrics() {
        for vertical in [false, true] {
            let m = DockMetrics::for_orientation(vertical);
            let s = m.split_metrics(vertical);
            assert_eq!(s.tile, m.tile);
            assert_eq!(s.icon, m.tile_icon);
            assert_eq!(s.chevron_hit, m.chevron_hit);
            assert_eq!(s.radius, TILE_RADIUS);
            assert_eq!(
                s.chevron_side,
                if vertical {
                    wv_ui::widgets::ChevronSide::BottomCenter
                } else {
                    wv_ui::widgets::ChevronSide::RightCenter
                },
                "小三角方位应随朝向"
            );
            // 小三角命中区必须完全在瓷砖内（wv-ui 侧测试再验几何，这里验的是
            // 我们喂进去的尺寸本身合法）。
            assert!(m.chevron_hit < m.tile.x.min(m.tile.y));
        }
    }
}
