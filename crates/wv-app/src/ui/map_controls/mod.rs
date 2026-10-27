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
//! | 10.5 | 等值线开关（VectorFlat / CellMap） | — |
//! | 10.75 | 海拔分层设色强度滑条（仅 CellMap，ADR-0055） | — |
//! | 14 | 线段 POI：河流（默认）/ 道路（**仅 Edit 档渲染**，ADR-0070 D4） | — |
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
//! ## 子模块（2026-10-24 由单文件 1260 行拆出）
//! - [`dock_state`]：朝向 / 吸附偏好与持久化结构。
//! - [`toolbar`]：工具条系统本体（槽位布局与命令排队）。
//! - [`widgets`]：自绘控件（分裂按钮、瓷砖、图标钮、分隔条、抓手）。
//! - `tests`：原 `mod tests` 整段平移。
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
    DrillState, MeasureMode, MeasureState, Panel, PanelState, ViewMode, ViewState, WorldProjection,
};
use crate::theme::ui_theme::{margin_xy, UiShellConfig};
use crate::ui_command_palette::{CommandId, CommandPalette, MapToolId};
use wv_render::state::{BrushState, BrushTool, SelectionShape, SelectionState};
use wv_ui::widgets::{split_button, SplitButtonMetrics, SplitMenuItem};

use super::shell::UiShell;
use super::widgets::icon_image;
use super::widgets::{
    ICON_ELLIPSE, ICON_MOUSE_POINTER, ICON_MOVE_HORIZONTAL, ICON_MOVE_VERTICAL, ICON_PAINT_BUCKET,
    ICON_RIVER, ICON_ROUTE, ICON_RULER_DIMENSION_LINE, ICON_SQUARES_UNITE,
    ICON_SQUARE_MOUSE_POINTER, ICON_TRASH, ICON_VECTOR_POLYGON, ICON_VECTOR_SQUARE,
};
use super::{
    ICON_BRUSH, ICON_ERASER, ICON_GRID, ICON_HAND, ICON_REDO, ICON_TREES, ICON_UNDO, ICON_WAVES,
};

mod dock_state;
#[cfg(test)]
mod tests;
mod toolbar;
mod widgets;

pub use dock_state::{DockParams, MapDockOrientation, MapDockPos, MapDockState};
pub use toolbar::map_controls;

/// 工具活动态的会话资源束（`map_controls` 的第 13 参）。
///
/// 折成一层元组是给 Bevy 的单系统 16 参数上限让路（元组算 1 个参数）；
/// 单独提出类型别名是给 clippy 的 `type_complexity` 让路——五元组内联写
/// 会超复杂度阈值（2026-10-26 线段 POI 入列时实锤）。
type ToolSessionStates<'w> = (
    Res<'w, wv_render::state::EditCellState>,
    Res<'w, crate::state::PointSelectState>,
    Res<'w, crate::state::FloodSelectState>,
    Res<'w, crate::state::PlaybackState>,
    Res<'w, crate::state::LinePoiState>,
);

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

    /// 槽 10.75（ADR-0055 海拔分层设色滑条）的槽位尺寸。
    ///
    /// **竖排必须就地转竖**（2026-10-24 用户反馈：纵向工具条过宽）：纵向
    /// 工具条的宽度由**最宽子项**决定，滑条原先按横排写死 `tile.x * 3`，
    /// 纵排里照样占 3 个瓷砖宽，把整条从 34px 撑到约 116px、右侧空出一大
    /// 片底。竖排改用 1 列宽 × 2 槽高的竖滑条；横排维持 3 槽横条（横排下
    /// 它只影响长度，不影响框高）。
    fn tint_slider_slot(&self, vertical: bool) -> egui::Vec2 {
        if vertical {
            egui::vec2(self.tile.x, self.tile.y * 2.0)
        } else {
            egui::vec2(self.tile.x * 3.0, self.tile.y)
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
