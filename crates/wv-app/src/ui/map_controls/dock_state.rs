//! 工具条偏好：朝向 / 吸附 / 位置，以及 `ui-layout.yaml` 读写结构。

use super::*;

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
