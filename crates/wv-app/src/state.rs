//! 应用层状态：本层持有的是**投影与视图状态**，不是权威世界数据。
//!
//! `AGENTS.md` §3：YAML 是 source of truth，Bevy ECS 只是投影。
//! 本模块的资源分三类：
//!
//! 1. [`WorldProjection`]：`World` 的只读快照（由 `wv-io` 加载，`wv-edit` 修改）。
//! 2. [`PlaybackState`]：`current_day` 等**视图状态**（不是世界数据，基线 §2.14）。
//! 3. [`ViewState`]：相机缩放、当前地图、视图模式、图层可见性。

use bevy::prelude::*;
use wv_core::World;

/// 世界数据的投影。**渲染层只读它，不做任何时间比较**（`AGENTS.md` §3.2）。
///
/// 所有「某时刻的世界状态」都必须经 [`wv_core::time::TimelineQuery::at`] 求出，
/// 结果写入 [`super::projection`] 下的渲染资源，渲染 System 只读那些资源。
#[derive(Resource, Default)]
pub struct WorldProjection {
    pub world: World,
    /// 工程根目录（用于保存）。
    pub project_dir: Option<String>,
    /// 是否「脏」（有未保存的编辑）。顶栏的 `●` 标记读它。
    pub dirty: bool,
    /// 工程加载失败时的错误信息。**必须显式显示给用户**——
    /// 否则启动目录不对时会静默退化成空白占位地形，看起来像「崩溃」。
    pub load_error: Option<String>,
}

/// 时间轴视图状态（基线 §2.14）。
///
/// **关键区分**：`current_day` 是**视图状态**（Bevy Resource），不是世界数据。
/// 世界数据里的时间一律是 `TimeSpan`。
#[derive(Resource)]
pub struct PlaybackState {
    /// 当前时刻（天数计数器，半开区间的求值入口）。
    pub current_day: i64,
    /// 播放速率（天/秒）。三档：1 / 5 / 30（基线 §1.8）。
    pub speed: f32,
    /// 是否正在播放。
    pub playing: bool,
    /// 播放范围（锁定在某个事态的 span 内；无选中事态时为时间轴全range）。
    pub range_from: i64,
    pub range_to: i64,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            current_day: 1,
            speed: 5.0,
            playing: false,
            range_from: 1,
            range_to: 180001,
        }
    }
}

/// 视图模式（基线 §1.10，D4 四态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// A. 真实地形：高度图 + 法线光照 + 贴图。
    #[default]
    RealTerrain,
    /// B. 色块地形：每种地形一种颜色。
    Schematic,
    /// C. 势力图：颜色 = 势力，底色淡化地形。
    Faction,
    /// D. 事件图：极简底图 + 事件高亮。
    Event,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [
        ViewMode::RealTerrain,
        ViewMode::Schematic,
        ViewMode::Faction,
        ViewMode::Event,
    ];

    /// 顶栏显示名。
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::RealTerrain => "真实地形",
            ViewMode::Schematic => "色块地形",
            ViewMode::Faction => "势力图",
            ViewMode::Event => "事件图",
        }
    }
}

/// 相机与视图状态（缩放范围见基线 §2.12：0.1×–8.0×）。
#[derive(Resource)]
pub struct ViewState {
    pub mode: ViewMode,
    /// 相机缩放（渲染层钳制范围）。
    pub scale: f32,
    /// 相机中心（世界坐标）。
    pub camera_center: Vec2,
    /// 地形纹理是否可见（图层开关之一）。
    pub terrain_visible: bool,
    /// 事件点图层是否可见。
    pub events_visible: bool,
    /// 轨迹图层是否可见。
    pub tracks_visible: bool,
    /// 道路图层是否可见（S12 后接入真实数据）。
    pub roads_visible: bool,
    /// 鼠标在地图上的世界坐标（顶栏坐标显示用）。
    pub cursor_world: Option<Vec2>,
    /// 当前地图 ID。
    pub current_map: String,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            mode: ViewMode::default(),
            scale: 1.0,
            camera_center: Vec2::ZERO,
            terrain_visible: true,
            events_visible: false,
            tracks_visible: true,
            roads_visible: true,
            cursor_world: None,
            current_map: "world_main".into(),
        }
    }
}

/// 笔刷工具状态（`AGENTS.md` §3.3：System 只生成命令，不直接改世界）。
#[derive(Resource)]
pub struct BrushState {
    /// 当前选中的地形类型编号。
    pub kind: u8,
    /// 笔刷半径（世界单位）。
    pub radius: f32,
    /// 衰减软度 0..=1（与 `wv_terrain::edit::falloff_weight` 同一参数）。
    pub softness: f32,
    /// 是否处于涂抹模式（按住左键拖拽中）。
    pub painting: bool,
    /// 笔刷是否启用。
    pub enabled: bool,
}

impl Default for BrushState {
    fn default() -> Self {
        Self {
            kind: 3, // 默认草原
            radius: 96.0,
            softness: 0.5,
            painting: false,
            enabled: true,
        }
    }
}

/// 网格几何参数（世界单位与格的换算，基线 §2.15 变换链的输入）。
#[derive(Resource)]
pub struct GridConfig {
    /// 每格世界边长。
    pub cell_size: f32,
    /// 网格左上角在世界坐标中的位置。
    pub origin: Vec2,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cell_size: 8.0,
            origin: Vec2::ZERO,
        }
    }
}

/// 可浮动的面板枚举（基线 §1.11 的 8 个面板 + 小地图）。
///
/// **关键**：面板是浮动的（`egui::Window`），**不推挤地图**（§1.11 R1/R2）。
/// 每个面板的开关与位置都记录在 [`PanelState`] 里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    /// P-1 地图树：World→Region→City→Interior，切换当前图。
    MapTree,
    /// P-2 图层管理：图层开关。
    Layers,
    /// P-3 笔刷工具：紧凑 Photoshop 式浮动窗（2 列 × 2 图标）。
    Brush,
    /// P-4 属性检查器。
    Inspector,
    /// P-5 时间轴 + 播放控件（底部浮动）。
    Timeline,
    /// P-6 势力编辑。
    Faction,
    /// P-7 事态登记（含事件登记入口）。
    Situation,
    /// P-8 校验面板。
    Validation,
    /// 小地图（用户 1.2 要求，含图层开关）。
    Minimap,
    /// 人物登记（用户 1.4 要求）。
    Character,
    /// 事件登记（用户 1.4 要求，可从时间轴进入）。
    Event,
}

impl Panel {
    /// 全部面板（菜单按此顺序列出）。
    pub const ALL: [Panel; 11] = [
        Panel::MapTree,
        Panel::Layers,
        Panel::Brush,
        Panel::Minimap,
        Panel::Timeline,
        Panel::Event,
        Panel::Situation,
        Panel::Character,
        Panel::Faction,
        Panel::Inspector,
        Panel::Validation,
    ];

    /// 菜单 / 标题显示名。
    pub fn title(self) -> &'static str {
        match self {
            Panel::MapTree => "地图树",
            Panel::Layers => "图层管理",
            Panel::Brush => "地形设计器",
            Panel::Inspector => "属性检查器",
            Panel::Timeline => "时间轴",
            Panel::Faction => "势力编辑",
            Panel::Situation => "事态登记",
            Panel::Validation => "校验面板",
            Panel::Minimap => "小地图",
            Panel::Character => "人物登记",
            Panel::Event => "事件登记",
        }
    }

    /// 默认位置（首次打开 / 重置位置时用）。坐标单位是窗口逻辑像素。
    pub fn default_pos(self) -> [f32; 2] {
        match self {
            Panel::MapTree => [12.0, 44.0],
            Panel::Layers => [12.0, 300.0],
            Panel::Brush => [12.0, 44.0], // 与 MapTree 同列，叠开时用户可拖
            Panel::Minimap => [1000.0, 44.0],
            Panel::Timeline => [12.0, 620.0],
            Panel::Event => [320.0, 120.0],
            Panel::Situation => [320.0, 300.0],
            Panel::Character => [320.0, 460.0],
            Panel::Faction => [700.0, 120.0],
            Panel::Inspector => [700.0, 300.0],
            Panel::Validation => [700.0, 460.0],
        }
    }
}

/// 面板可见性与位置状态。
///
/// **为什么用资源而不是 egui 自己的记忆**：用户要求「顶部菜单可启动/关闭、
/// 可重置位置」，需要程序化控制，不能只靠 egui 的隐式 memory。
#[derive(Resource)]
pub struct PanelState {
    /// 每个面板是否可见。
    pub visible: std::collections::HashMap<Panel, bool>,
    /// 重置位置信号：置 true 后由 UI system 消费并清回 false。
    pub reset_requested: bool,
    /// 沉浸模式（`Tab`）：所有面板淡出但**仍可点击**（动画 §4 规范：alpha 0.10）。
    pub immersive: bool,
}

impl Default for PanelState {
    fn default() -> Self {
        use std::collections::HashMap;
        // 默认只开最核心的几个，保证「地图为主」——
        // 用户反馈「地图可视面积太少」，所以默认打开的面板要尽量少。
        let mut visible = HashMap::new();
        for p in Panel::ALL {
            visible.insert(p, false);
        }
        visible.insert(Panel::Brush, true);
        visible.insert(Panel::Minimap, true);
        visible.insert(Panel::Timeline, true);
        Self {
            visible,
            reset_requested: false,
            immersive: false,
        }
    }
}

impl PanelState {
    pub fn is_visible(&self, p: Panel) -> bool {
        self.visible.get(&p).copied().unwrap_or(false)
    }

    pub fn set_visible(&mut self, p: Panel, v: bool) {
        self.visible.insert(p, v);
    }

    pub fn toggle(&mut self, p: Panel) {
        let v = !self.is_visible(p);
        self.set_visible(p, v);
    }
}
