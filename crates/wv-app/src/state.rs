//! 应用层状态：本层持有的是**投影与视图状态**，不是权威世界数据。
//!
//! `AGENTS.md` §3：YAML 是 source of truth，Bevy ECS 只是投影。
//! 本模块的资源分三类：
//!
//! 1. [`WorldProjection`]：`World` 的**可变工作副本**（由 `wv-io` 加载，`wv-edit` 修改）。
//!    注意：`wv-app` 持有它不违反 `AGENTS.md` §2.1——真正的 source of truth 是磁盘
//!    YAML，这里只是编辑器的工作区；约束的是**改动路径**，一律经 `wv-edit` 的
//!    `EditCommand` / `History`（§3.3），不得由 System 直接改。
//! 2. [`PlaybackState`]：`current_day` 等**视图状态**（不是世界数据，基线 §2.14）。
//! 3. [`ViewState`]：相机缩放、当前地图、视图模式、图层可见性。

use bevy::prelude::*;
use wv_core::World;
use wv_io::diagnostics::Diagnostics;

/// 世界的**可变工作副本**。渲染层**只读**它，且不做任何时间比较（`AGENTS.md` §3.2）。
///
/// 所有「某时刻的世界状态」都必须经 [`wv_core::time::TimelineQuery::at`] 求出，
/// 结果写入 [`super::projection`] 下的渲染资源，渲染 System 只读那些资源。
///
/// **写入路径**：只有编辑链（[`super::editing`]）可以改它，且必须把改动包成
/// `wv-edit` 的 `EditCommand` 经 `History` 提交（§3.3），System 不得直接改。
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

/// 当前工程的校验结果缓存（来自 `wv_io::diagnostics::derive_hints`）。
///
/// 为什么单独存一个资源而不是每次 UI 帧都算：
/// `derive_hints` 会跑 N 个事件 × 距离 A*，**只读**世界即可，无副作用。
/// 缓存层是为了让 UI 渲染零成本读，触发点收敛到「加载完成 / 编辑提交 / 撤销重做」。
///
/// **V1 守约**（AGENTS §7.5）：`Diagnostics` 把错误与提示在类型上分开，
/// 校验面板据此只列、不阻止保存。错误仅在「数据坏了」时出现——本工程
/// 加载期已硬失败（见 `wv_io::Project::load_world`），故面板里的错误主要
/// 来自加载后的二次校验（如悬空引用）；提示包括相遇提示、事件位移可行性等。
#[derive(Resource, Default)]
pub struct ProjectDiagnostics {
    /// 派生诊断项。空 = 还没跑过诊断（启动期）。
    pub diagnostics: Diagnostics,
    /// 最近一次诊断的项数。仅用于诊断自身的可见性展示（"X 项"计数）。
    pub last_count: usize,
    /// `true` = 需要重跑 [`wv_io::diagnostics::derive_hints`]。
    ///
    /// 由「加载完成 / 新建 / 编辑提交 / 撤销重做」四个点 set true，
    /// 由 `refresh_project_diagnostics` system 在重算后清回 false。
    /// UI 面板每帧**只读** `diagnostics`，不感知此标志。
    pub needs_diagnose: bool,
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
    /// 帧间小数进位。每帧增量 `speed*dt` 在 60fps 下不足 1 天，直接 `as i64`
    /// 会截断成 0（播放永远推不动）；不足整天的零头攒在这里，攒满 1 天再入账。
    pub carry: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            current_day: 1,
            speed: 5.0,
            playing: false,
            range_from: 1,
            range_to: 180001,
            carry: 0.0,
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
    /// E. 轮廓图：黑底 + 海岸线 + 城市路线（2026-11 用户需求：
    /// 地形纹理完全退场，只留水陆轮廓，要素对比度最大化）。
    Coastline,
}

impl ViewMode {
    pub const ALL: [ViewMode; 5] = [
        ViewMode::RealTerrain,
        ViewMode::Schematic,
        ViewMode::Faction,
        ViewMode::Event,
        ViewMode::Coastline,
    ];

    /// 顶栏显示名。
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::RealTerrain => "真实地形",
            ViewMode::Schematic => "色块地形",
            ViewMode::Faction => "势力图",
            ViewMode::Event => "事件图",
            ViewMode::Coastline => "轮廓图",
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
    /// **底图淡化**（2026-11 用户需求：路线/地标被高饱和地形纹理淹没的解法）。
    ///
    /// 开启后地形瓦片/纯色向亮灰去饱和混合（比例见 `palette::SOFTEN_KEEP`），
    /// 所有视图模式生效——Google 地图式「底图永远比要素淡一档」。
    /// 默认开：这正是本轮要修的「路线看不见」问题的默认答案。
    pub terrain_soften: bool,
    /// 事件点图层是否可见。
    pub events_visible: bool,
    /// 轨迹图层是否可见。
    pub tracks_visible: bool,
    /// 道路图层是否可见。
    pub roads_visible: bool,
    /// 地点（POI）图层是否可见：锚定地点的点标（矢量图层底子）。
    pub places_visible: bool,
    /// 势力多边形叠加层是否可见（S12）。
    pub factions_visible: bool,
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
            terrain_soften: true,
            events_visible: true,
            tracks_visible: true,
            roads_visible: true,
            places_visible: true,
            factions_visible: true,
            cursor_world: None,
            current_map: "world_main".into(),
        }
    }
}

/// 视野层级（LOD，v0.3 优化项 A）。
///
/// `ViewState.scale` 的语义是**世界单位/屏幕像素**（与 `OrthographicProjection.scale`
/// 一致）：值越大 = 拉得越远。三档阈值按「一张 4096² 世界图在 1600px 窗口里
/// 全图视野 ≈ scale 5.1」标定：
///
/// | 档 | scale 范围 | 视野 | 渲染策略（Google 地图式） |
/// |---|---|---|---|
/// | [`LodTier::Edit`] | < 1.0 | 编辑级（局部放大） | 全细节 |
/// | [`LodTier::Region`] | 1.0 – 3.0 | 省级 | 全细节 |
/// | [`LodTier::Nation`] | ≥ 3.0 | 国家/世界级 | 只画城市点标（有范围的地点），其余要素降权 |
///
/// Google 地图对应物：每个缩放级别只画「该级别该出现的要素」。当前第一版
/// 只做 POI 筛选（城市 vs 点标），道路按等级筛选等后续数据模型补 `rank` 再做。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodTier {
    /// 编辑级：放大看局部。
    Edit,
    /// 省级：中距离。
    Region,
    /// 国家级：拉到最远。
    Nation,
}

/// 国家档阈值：scale ≥ 此值视为「世界级视野」。
///
/// 依据：4096² 世界 / 800px 窗口高 = 5.12（全图），3.0 ≈ 可见世界宽度的 60%。
pub const LOD_NATION_SCALE: f32 = 3.0;
/// 省级下限：scale < 此值视为「编辑级视野」。
pub const LOD_REGION_SCALE: f32 = 1.0;

/// 由视图缩放求当前视野层级。纯函数，渲染各系统共用同一口径。
pub fn lod_tier(view_scale: f32) -> LodTier {
    if view_scale >= LOD_NATION_SCALE {
        LodTier::Nation
    } else if view_scale >= LOD_REGION_SCALE {
        LodTier::Region
    } else {
        LodTier::Edit
    }
}

#[cfg(test)]
mod lod_tests {
    use super::*;

    #[test]
    fn lod_tier_splits_at_exact_thresholds() {
        // 边界值：阈值本身归更高档（含闭区间语义）。
        assert_eq!(lod_tier(LOD_NATION_SCALE), LodTier::Nation);
        assert_eq!(lod_tier(LOD_REGION_SCALE), LodTier::Region);
        assert_eq!(lod_tier(LOD_REGION_SCALE - 0.001), LodTier::Edit);
        // 常规值。
        assert_eq!(lod_tier(0.1), LodTier::Edit);
        assert_eq!(lod_tier(1.5), LodTier::Region);
        assert_eq!(lod_tier(5.12), LodTier::Nation);
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
    ///
    /// **互不重叠**是硬要求（设计规格 §3.3，验证项 §9.3-1 由
    /// [`crate::ui_layout::tests`] 自动把关）。布局规则：
    ///
    /// - 四列，列起点 `x = 12 / 332 / 652 / 972`，列宽由该列最宽面板决定
    ///   （最右边界 1272 ≤ 1280，1280 宽窗口正好放下）；
    /// - **底部 880 起是时间轴专用带**（§3.3 P-5「底部通栏」）——它是通栏，
    ///   x 与所有面板相交，因此**其余面板必须全部止于 y = 880**，
    ///   否则时间轴一开就压住它们。
    ///
    /// 用「预留带」而不是「让时间轴绕开面板」：通栏是规格语义，绕开就不是通栏了。
    pub fn default_pos(self) -> [f32; 2] {
        match self {
            Panel::MapTree => [12.0, 56.0],
            Panel::Layers => [12.0, 398.0],
            Panel::Brush => [12.0, 578.0],
            Panel::Minimap => [332.0, 56.0],
            Panel::Faction => [332.0, 368.0],
            Panel::Event => [652.0, 56.0],
            Panel::Situation => [652.0, 380.0],
            Panel::Validation => [972.0, 56.0],
            Panel::Inspector => [972.0, 308.0],
            Panel::Character => [972.0, 560.0],
            // 底部通栏（§3.3 P-5）。
            Panel::Timeline => [12.0, 880.0],
        }
    }

    /// 默认尺寸 `[宽, 高]`（设计规格 §3.3「默认宽」列）。
    pub fn default_size(self) -> [f32; 2] {
        match self {
            Panel::MapTree => [216.0, 330.0],
            Panel::Layers => [216.0, 168.0],
            Panel::Brush => [264.0, 300.0],
            Panel::Minimap => [216.0, 300.0],
            // 通栏：1280 − 12×2 = 1256。
            Panel::Timeline => [1256.0, 180.0],
            Panel::Event => [300.0, 312.0],
            Panel::Situation => [300.0, 308.0],
            Panel::Character => [280.0, 260.0],
            Panel::Faction => [280.0, 300.0],
            Panel::Inspector => [264.0, 240.0],
            Panel::Validation => [300.0, 240.0],
        }
    }

    /// 面板 id（`ui-layout.yaml` 的键）。
    pub fn key(self) -> &'static str {
        match self {
            Panel::MapTree => "tree",
            Panel::Layers => "layers",
            Panel::Brush => "brush",
            Panel::Inspector => "inspector",
            Panel::Timeline => "timeline",
            Panel::Faction => "faction",
            Panel::Situation => "situation",
            Panel::Validation => "validation",
            Panel::Minimap => "minimap",
            Panel::Character => "character",
            Panel::Event => "event",
        }
    }

    /// 面板头类型图标（§4.1：类型图标着 `ui.accent` 色）。
    ///
    /// 用几何字符而非 emoji：emoji 跨系统字形差异大（§4.5 的教训），
    /// 且 `fonts.rs` 不打包字体，Linux 上可能缺字。
    pub fn icon(self) -> &'static str {
        match self {
            Panel::MapTree => "◈",
            Panel::Layers => "▤",
            Panel::Brush => "◧",
            Panel::Inspector => "◉",
            Panel::Timeline => "═",
            Panel::Faction => "◆",
            Panel::Situation => "▣",
            Panel::Validation => "△",
            Panel::Minimap => "□",
            Panel::Character => "◎",
            Panel::Event => "○",
        }
    }

    /// 是否「数据面板」（§9.2 `data_opt()`：小窗可隐藏，`<900px` 窗口默认收起）。
    pub fn data_opt(self) -> bool {
        matches!(
            self,
            Panel::Brush | Panel::Minimap | Panel::Layers | Panel::Validation
        )
    }

    /// 首次启动的默认可见性（地图为主，D1：默认开的面板尽量少）。
    pub fn default_visible(self) -> bool {
        matches!(self, Panel::Brush | Panel::Minimap | Panel::Timeline)
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

// ---------------------------------------------------------------------------
// 地图三级钻取（v0.2 §B）
// ---------------------------------------------------------------------------

/// 图层级别的显示名。
///
/// 放在 UI 层而不是给 `wv_core::map::MapLevel` 加 `label()`：数据层不该出现
/// 面向作者的中文文案（AGENTS §2 依赖方向，文案是外壳关注点）。
pub fn map_level_label(level: wv_core::map::MapLevel) -> &'static str {
    use wv_core::map::MapLevel;
    match level {
        MapLevel::World => "世界",
        MapLevel::Region => "区域",
        MapLevel::City => "城市",
        MapLevel::Interior => "建筑内",
    }
}

/// 地图钻取状态（v0.2 §B「世界 → 城市 → 建筑内」三级）。
///
/// ## 为什么只存 `Id`
///
/// 钻取就是「沿地图层级往下走」，而层级关系在数据层**已经**是
/// [`wv_core::map::MapNode::parent`] / [`wv_core::map::MapNode::children`] 的
/// **ID 引用**（AGENTS 原则四：只存引用，不重复存储）。所以本结构只保留
/// 一条 id 链，名字 / 范围 / 图层每帧现查——子图内容永不复制进 UI 状态，
/// 否则父图里那份副本迟早与子图不一致。
///
/// ## 与 [`ViewState::current_map`] 的关系
///
/// `current_map` 是「当前图 id」的**单值**，表达不了「从哪来」，而面包屑需要完整路径。
/// 故本结构升为**唯一真源**，`ViewState::current_map` 降级为派生镜像
/// （[`DrillState::sync_view`]）。旧字段暂留是因为渲染/编辑链还在读它（3 处），
/// 一次性改签名会把本次改动扩散到相机与笔刷系统。
#[derive(Resource, Debug, Clone, Default)]
pub struct DrillState {
    /// 从根图到当前图的 id 链（含首尾）。空 = 世界尚未加载。
    trail: Vec<wv_core::id::Id>,
    /// 访问过的地图 id（**钻出时**写入）。用于钻入标记的「已访问」灰显。
    visited: std::collections::BTreeSet<wv_core::id::Id>,
    /// 上一帧的深度。深度一变就说明发生了钻取，相机需要做 zoom-to-bounds 过渡。
    last_depth: usize,
    /// 上一次钻取的起点（世界坐标 → 缩放），供过渡插值；`None` 表示没有待播的过渡。
    pub transition_from: Option<(Vec2, f32)>,
}

impl DrillState {
    /// 当前深度：根图为 `0`，每钻一层 +1。
    pub fn depth(&self) -> usize {
        self.trail.len().saturating_sub(1)
    }

    /// 当前地图 id。
    pub fn current(&self) -> Option<&wv_core::id::Id> {
        self.trail.last()
    }

    /// 完整 id 链（根 → 当前）。面包屑据此逐段取名。
    ///
    /// 只读暴露：路径的写入一律走 [`DrillState::enter`] / [`DrillState::up`] /
    /// [`DrillState::jump_to`]，否则「只允许沿 `children` 走」这条不变式会失守。
    pub fn trail(&self) -> &[wv_core::id::Id] {
        &self.trail
    }

    /// 当前地图节点。
    pub fn current_node<'w>(&self, world: &'w World) -> Option<&'w wv_core::map::MapNode> {
        let id = self.current()?;
        world.maps.iter().find(|m| &m.id == id)
    }

    /// 按 id 找节点。
    pub fn node<'w>(
        &self,
        world: &'w World,
        id: &wv_core::id::Id,
    ) -> Option<&'w wv_core::map::MapNode> {
        world.maps.iter().find(|m| &m.id == id)
    }

    /// 当前图**已解析**的子图节点（子图 id 悬空时静默跳过——D4/§7.5：
    /// 悬空引用由校验面板列出，UI 不因此崩掉，也不能阻止作者保存）。
    pub fn child_nodes<'w>(&self, world: &'w World) -> Vec<&'w wv_core::map::MapNode> {
        let Some(node) = self.current_node(world) else {
            return Vec::new();
        };
        node.children
            .iter()
            .filter_map(|cid| world.maps.iter().find(|m| &m.id == cid))
            .collect()
    }

    /// 若路径为空（或根图变了）则落到 `world.meta.root_map`。
    ///
    /// 每帧调用是安全的：路径非空且仍在世界里时不动它。
    pub fn ensure_root(&mut self, world: &World) {
        // 根图优先取 `world.yaml` 的 `root_map`；工程没写 meta 时退而求其次，
        // 找第一个 `level: world` 的图——总比把整个 UI 卡在「未加载地图」好。
        let root = match &world.meta {
            Some(m) => Some(m.root_map.clone()),
            None => world
                .maps
                .iter()
                .find(|m| m.level == wv_core::map::MapLevel::World)
                .map(|m| m.id.clone()),
        };
        let Some(root) = root else {
            return;
        };
        let still_valid = self
            .current()
            .map(|id| world.maps.iter().any(|m| &m.id == id))
            .unwrap_or(false);
        if self.trail.is_empty() || !still_valid {
            self.trail.clear();
            self.trail.push(root);
            self.last_depth = 0;
        }
    }

    /// 换工程 / 打开新工程时重置：回到世界、清访问痕迹（v0.2 §A）。
    pub fn reset_to_world(&mut self) {
        self.trail.clear();
        self.visited.clear();
        self.last_depth = 0;
        self.transition_from = None;
    }

    /// 钻入某个子图。只有**当前图的直接子图**才能进入（拒绝越级，避免路径与
    /// `children` 引用不一致）。返回是否发生了钻取。
    pub fn enter(&mut self, world: &World, child: &wv_core::id::Id) -> bool {
        let Some(node) = self.current_node(world) else {
            return false;
        };
        if !node.children.iter().any(|c| c == child) {
            return false;
        }
        self.trail.push(child.clone());
        true
    }

    /// 跨层级跳到任意地图（阶段八 §A：校验面板「点提示跳定位」）。
    ///
    /// 与 [`Self::enter`] 的区别：后者只允许当前图的直接子图；本方法允许
    /// 跳到地图树里**任意**节点——从根 `world.meta.root_map` 出发，按
    /// `children` 链做 BFS 找到目标，重写 `trail`。
    ///
    /// **为什么不**只调 `enter` 一连串：`enter` 拒绝越级，且每次 enter
    /// 都要保证「child 是当前图的真子图」。跨层级跳转的语义是「我不在意
    /// 现在在哪儿，给我算一条到目标图的最短路径」。
    ///
    /// 返回 `true` 当且仅当找到了目标（且 `trail` 被改写）；目标不在树里
    /// 或树自身存在悬空引用时返回 `false`，trail 保持不变。
    ///
    /// **悬空引用处理**：BFS 沿途遇到 `children` 里的 id 在 `world.maps`
    /// 里找不到时，**静默跳过**（V1 守约，与 `child_nodes` 同源）——作者
    /// 已经在校验面板看到这条悬空了。
    pub fn drill_to(&mut self, world: &World, target: &wv_core::id::Id) -> bool {
        // 起点：根图。优先 meta.root_map，否则取第一个 level=World 的图。
        let root_id = match &world.meta {
            Some(m) => Some(m.root_map.clone()),
            None => world
                .maps
                .iter()
                .find(|m| m.level == wv_core::map::MapLevel::World)
                .map(|m| m.id.clone()),
        };
        let Some(root) = root_id else {
            return false;
        };
        if root == *target {
            // 根图必须实际存在于 maps 里——否则即便 meta 指着它，也跳不过去。
            if world.maps.iter().any(|m| m.id == *target) {
                self.trail = vec![root];
                return true;
            } else {
                return false;
            }
        }
        // 目标必须实际存在于 maps 里——「被引用为子图 id」不等于「节点存在」。
        // 悬空子图 id 已在 `derive_hints` 里被 V1 守约提示，本方法对此说「不跳」。
        if !world.maps.iter().any(|m| m.id == *target) {
            return false;
        }
        // BFS：维护「节点 → 父节点」反查表，结束后从 target 回溯得到路径。
        let mut parent: std::collections::HashMap<wv_core::id::Id, wv_core::id::Id> =
            std::collections::HashMap::new();
        parent.insert(root.clone(), root.clone());
        let mut frontier: Vec<wv_core::id::Id> = vec![root.clone()];
        let mut found = false;
        'bfs: while !frontier.is_empty() {
            let mut next_frontier: Vec<wv_core::id::Id> = Vec::new();
            for cur in &frontier {
                let Some(node) = world.maps.iter().find(|m| &m.id == cur) else {
                    continue; // 悬空引用：跳过（V1 守约）
                };
                for child in &node.children {
                    if parent.contains_key(child) {
                        continue; // 已访问
                    }
                    parent.insert(child.clone(), cur.clone());
                    if child == target {
                        found = true;
                        break 'bfs;
                    }
                    next_frontier.push(child.clone());
                }
            }
            frontier = next_frontier;
        }
        if !found {
            return false;
        }
        // 回溯：从 target 一直走到根，reverse 得正向路径。
        let mut path: Vec<wv_core::id::Id> = vec![target.clone()];
        let mut cursor = target.clone();
        while cursor != root {
            let p = parent.get(&cursor).cloned().expect("BFS 已建反查表");
            path.push(p.clone());
            cursor = p;
        }
        path.reverse();
        self.trail = path;
        true
    }

    /// 钻回上一级（`Backspace` / 面包屑上级）。返回是否发生了钻回。
    ///
    /// **钻出即计入访问痕迹**（v0.2 §B.2）：访问痕迹表示「进去过」，
    /// 所以是在离开时写入，而不是进入时——进入时写入会让刚钻进去的
    /// 那一格立刻变灰，看起来像「已经看过了」。
    pub fn up(&mut self) -> bool {
        if self.trail.len() <= 1 {
            return false;
        }
        if let Some(left) = self.trail.pop() {
            self.visited.insert(left);
        }
        true
    }

    /// 跳到路径中的第 `index` 段（面包屑点击）。成功返回 `true`。
    pub fn jump_to(&mut self, index: usize) -> bool {
        if index + 1 >= self.trail.len() {
            return false;
        }
        // 被跳过的中间层级全部计入访问痕迹。
        for left in self.trail.drain(index + 1..) {
            self.visited.insert(left);
        }
        true
    }

    /// 是否访问过（渲染「已访问」灰显标记）。
    pub fn is_visited(&self, id: &wv_core::id::Id) -> bool {
        self.visited.contains(id)
    }

    /// 深度是否刚刚变化（下一帧要播一次相机过渡）。
    pub fn take_depth_change(&mut self) -> bool {
        let d = self.depth();
        if d == self.last_depth {
            return false;
        }
        self.last_depth = d;
        true
    }

    /// 把当前图 id 写回 [`ViewState::current_map`] 镜像。
    pub fn sync_view(&self, view: &mut ViewState) {
        if let Some(id) = self.current() {
            view.current_map = id.as_str().to_owned();
        }
    }
}

// ---------------------------------------------------------------------------
// 最近工程（v0.2 §A 项目管理菜单）
// ---------------------------------------------------------------------------

/// 一个「最近打开」的工程条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    /// 工程根目录（`world.yaml` 所在目录）。
    pub dir: String,
    /// 显示名（`world.yaml` 的 `meta.name`）。
    pub name: String,
}

/// 最近打开的工程列表。
///
/// ## 为什么是**会话内**列表
///
/// 跨会话记住「最近工程」需要一处**工程之外**的配置存放点，而 AGENTS §1 P2
/// （目录即工程）明确禁止把状态写进注册表 / AppData / 全局配置。这里不擅自
/// 开这个口子，只维持会话内的列表；当前打开的工程由
/// [`WorldProjection::project_dir`] 提供，所以菜单永远至少有它一条。
///
/// 待决：是否落盘、落在哪里（`view.yaml` 的 `search_paths`？还是干脆不做），
/// 见 `README.md` §12 待决清单的问题记录。
#[derive(Resource, Debug, Default)]
pub struct RecentProjects {
    entries: Vec<RecentProject>,
}

/// 会话内最多记住几个最近工程（原型给的是 3 条，这里留一点余量）。
pub const RECENT_PROJECTS_MAX: usize = 8;

impl RecentProjects {
    /// 已记录的工程（按最近使用排序）。
    pub fn entries(&self) -> &[RecentProject] {
        &self.entries
    }

    /// 置顶一个工程（按目录去重，超上限截断）。
    pub fn remember(&mut self, dir: &str, name: &str) {
        if dir.is_empty() {
            return;
        }
        self.entries.retain(|e| e.dir != dir);
        self.entries.insert(
            0,
            RecentProject {
                dir: dir.to_owned(),
                name: name.to_owned(),
            },
        );
        self.entries.truncate(RECENT_PROJECTS_MAX);
    }

    /// 菜单 / 命令面板实际要显示的列表：会话记录 **∪** 当前工程。
    ///
    /// 当前工程放在最前——用户最可能想确认的就是「我现在开的是哪个」。
    pub fn visible(&self, projection: &WorldProjection) -> Vec<RecentProject> {
        let mut out: Vec<RecentProject> = Vec::new();
        if let Some(dir) = projection.project_dir.as_deref() {
            let name = projection
                .world
                .meta
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| short_dir(dir));
            out.push(RecentProject {
                dir: dir.to_owned(),
                name,
            });
        }
        for e in &self.entries {
            if !out.iter().any(|o| o.dir == e.dir) {
                out.push(e.clone());
            }
        }
        out
    }
}

/// 目录末段（用于没有 `meta.name` 时兜底显示）。
fn short_dir(dir: &str) -> String {
    std::path::Path::new(dir)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::id::Id;
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::time::TimeSpan;
    use wv_core::WorldMeta;

    fn node(
        id: &str,
        name: &str,
        level: MapLevel,
        parent: Option<&str>,
        children: &[&str],
    ) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: name.to_owned(),
            level,
            parent: parent.map(Id::new_unchecked),
            children: children.iter().map(|c| Id::new_unchecked(*c)).collect(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: None,
            raster: None,
        }
    }

    /// world_main ─┬─ city_zunyi ─── bld_yamen
    ///             └─ city_chishui
    /// 另有 `city_ghost` 被 `world_main` 引用但**没有对应节点**（悬空引用）。
    fn demo_world() -> World {
        let mut w = World {
            meta: Some(WorldMeta {
                name: "四渡赤水".into(),
                version: "0.1".into(),
                schema_version: 2,
                root_map: Id::new_unchecked("world_main"),
                timeline_range: TimeSpan::new(1, 180_001).expect("测试区间合法"),
            }),
            ..World::default()
        };
        w.maps = vec![
            node(
                "world_main",
                "主世界",
                MapLevel::World,
                None,
                &["city_zunyi", "city_chishui", "city_ghost"],
            ),
            node(
                "city_zunyi",
                "遵义城",
                MapLevel::City,
                Some("world_main"),
                &["bld_yamen"],
            ),
            node(
                "bld_yamen",
                "府衙",
                MapLevel::Interior,
                Some("city_zunyi"),
                &[],
            ),
            node(
                "city_chishui",
                "赤水城",
                MapLevel::City,
                Some("world_main"),
                &[],
            ),
        ];
        w
    }

    #[test]
    fn ensure_root_lands_on_the_declared_root_map() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        assert_eq!(d.current().map(Id::as_str), Some("world_main"));
        assert_eq!(d.depth(), 0, "根图深度应为 0");
    }

    #[test]
    fn ensure_root_is_idempotent_and_keeps_the_current_map() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        // 每帧都会调 ensure_root，它不能把已经钻进去的路径打回根图。
        d.ensure_root(&w);
        d.ensure_root(&w);
        assert_eq!(d.current().map(Id::as_str), Some("city_zunyi"));
        assert_eq!(d.depth(), 1);
    }

    #[test]
    fn drill_path_only_accepts_direct_children() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        // 越级：世界图不能直接跳进「府衙」（它是 city_zunyi 的子图）。
        assert!(
            !d.enter(&w, &Id::new_unchecked("bld_yamen")),
            "越级钻入被拒绝"
        );
        assert_eq!(d.depth(), 0);
        // 不存在的地图同样拒绝。
        assert!(!d.enter(&w, &Id::new_unchecked("city_nowhere")));
        // 直接子图可以。
        assert!(d.enter(&w, &Id::new_unchecked("city_zunyi")));
        assert_eq!(d.depth(), 1);
    }

    #[test]
    fn up_returns_a_level_and_stops_at_the_root() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        d.enter(&w, &Id::new_unchecked("bld_yamen"));
        assert_eq!(d.depth(), 2);
        assert!(d.up());
        assert_eq!(d.current().map(Id::as_str), Some("city_zunyi"));
        assert!(d.up());
        assert_eq!(d.current().map(Id::as_str), Some("world_main"));
        assert!(!d.up(), "根图之上没有更上级");
        assert_eq!(d.depth(), 0);
    }

    #[test]
    fn drilling_out_is_what_marks_a_map_visited() {
        let w = demo_world();
        let yamen = Id::new_unchecked("bld_yamen");
        let mut d = DrillState::default();
        d.ensure_root(&w);
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        d.enter(&w, &yamen);
        assert!(!d.is_visited(&yamen), "刚钻进去不该立刻算已访问");
        d.up();
        assert!(d.is_visited(&yamen), "钻出时应写入访问痕迹");
    }

    #[test]
    fn breadcrumb_jump_marks_every_skipped_level_visited() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        d.enter(&w, &Id::new_unchecked("bld_yamen"));
        // 点面包屑上的「世界」（index 0），跨过 city_zunyi 与 bld_yamen。
        assert!(d.jump_to(0));
        assert_eq!(d.current().map(Id::as_str), Some("world_main"));
        assert!(d.is_visited(&Id::new_unchecked("city_zunyi")));
        assert!(d.is_visited(&Id::new_unchecked("bld_yamen")));
        // 点当前级（最后一段）是空操作。
        assert!(!d.jump_to(0), "已在目标级，不应算成功");
    }

    #[test]
    fn dangling_child_references_are_skipped_not_fatal() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        let kids = d.child_nodes(&w);
        // world_main 声明了 3 个子图，其中 city_ghost 没有节点。
        assert_eq!(kids.len(), 2, "悬空引用应被跳过");
        assert_eq!(kids[0].name, "遵义城");
        assert_eq!(kids[1].name, "赤水城");
        // §7.5：悬空引用不阻止任何操作——现在仍然可以正常钻取。
        assert!(d.enter(&w, &Id::new_unchecked("city_zunyi")));
    }

    #[test]
    fn reset_to_world_clears_trail_and_visit_marks() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        d.enter(&w, &Id::new_unchecked("bld_yamen"));
        d.up();
        assert_eq!(d.depth(), 1);
        d.reset_to_world();
        assert_eq!(d.depth(), 0, "换工程后应回到世界（v0.2 §A）");
        assert!(
            !d.is_visited(&Id::new_unchecked("bld_yamen")),
            "访问痕迹应清空"
        );
        assert!(d.trail.is_empty(), "重置后需要 ensure_root 重新落根");
    }

    #[test]
    fn depth_change_is_reported_exactly_once() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        // 落根：0 → 0，没有变化。
        assert!(!d.take_depth_change(), "落根不算钻取");
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        assert!(d.take_depth_change(), "钻入应报一次变化");
        assert!(!d.take_depth_change(), "同一深度不该重复报");
    }

    #[test]
    fn sync_view_mirrors_the_current_map_id() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.ensure_root(&w);
        // 故意塞一个陈旧值：`sync_view` 的职责就是把它拉回真实层级。
        let mut view = ViewState {
            current_map: "stale".into(),
            ..ViewState::default()
        };
        d.sync_view(&mut view);
        assert_eq!(view.current_map, "world_main");
        d.enter(&w, &Id::new_unchecked("city_zunyi"));
        d.sync_view(&mut view);
        assert_eq!(view.current_map, "city_zunyi");
    }

    #[test]
    fn level_labels_are_chinese_and_cover_all_four_levels() {
        use wv_core::map::MapLevel;
        for l in [
            MapLevel::World,
            MapLevel::Region,
            MapLevel::City,
            MapLevel::Interior,
        ] {
            assert!(!map_level_label(l).is_empty());
        }
        assert_eq!(map_level_label(MapLevel::Interior), "建筑内");
    }

    #[test]
    fn recent_projects_deduplicates_by_dir_and_keeps_most_recent_first() {
        let mut r = RecentProjects::default();
        r.remember("/w/a", "甲");
        r.remember("/w/b", "乙");
        r.remember("/w/a", "甲改");
        let names: Vec<&str> = r.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["甲改", "乙"], "重复目录应置顶而不是追加");
    }

    #[test]
    fn recent_projects_caps_the_list_and_ignores_empty_dirs() {
        let mut r = RecentProjects::default();
        for i in 0..(RECENT_PROJECTS_MAX + 5) {
            r.remember(&format!("/w/{i}"), "x");
        }
        assert_eq!(r.entries().len(), RECENT_PROJECTS_MAX);
        r.remember("", "无目录");
        assert_eq!(r.entries().len(), RECENT_PROJECTS_MAX, "空目录不入表");
    }

    #[test]
    fn visible_recent_projects_always_leads_with_the_open_project() {
        let mut r = RecentProjects::default();
        r.remember("/w/other", "别的工程");
        let mut projection = WorldProjection::default();
        // 还没打开任何工程：只有会话记录。
        assert_eq!(r.visible(&projection).len(), 1);

        projection.project_dir = Some("/w/current".into());
        projection.world = demo_world();
        let shown = r.visible(&projection);
        assert_eq!(shown[0].dir, "/w/current");
        assert_eq!(shown[0].name, "四渡赤水", "名字取 world.yaml 的 meta.name");
        assert_eq!(shown[1].dir, "/w/other");
    }

    #[test]
    fn visible_recent_projects_does_not_repeat_the_open_project() {
        let r = {
            let mut r = RecentProjects::default();
            r.remember("/w/current", "四渡赤水");
            r
        };
        let projection = WorldProjection {
            project_dir: Some("/w/current".into()),
            ..WorldProjection::default()
        };
        assert_eq!(r.visible(&projection).len(), 1, "当前工程不应出现两次");
    }

    #[test]
    fn project_without_meta_name_falls_back_to_the_directory_leaf() {
        let projection = WorldProjection {
            project_dir: Some("/worlds/minimal-world".into()),
            ..WorldProjection::default()
        };
        let shown = RecentProjects::default().visible(&projection);
        assert_eq!(shown[0].name, "minimal-world");
    }

    // -----------------------------------------------------------------------
    // 阶段八 §A：DrillState::drill_to 跨层级跳转
    // -----------------------------------------------------------------------

    /// 直接跳到根图 = 单独把 trail 设为 `[root]`。
    #[test]
    fn drill_to_root_simplifies_trail() {
        let w = demo_world();
        let mut d = DrillState::default();
        // 先把 trail 弄复杂（演示「重置」语义）。
        d.drill_to(&w, &Id::new_unchecked("bld_yamen"));
        assert_eq!(
            d.depth(),
            2,
            "先到 bld_yamen 看 trail 是不是 [world, zunyi, yamen]"
        );
        // 再跳回根图。
        assert!(d.drill_to(&w, &Id::new_unchecked("world_main")));
        assert_eq!(d.depth(), 0);
        assert_eq!(d.current().map(Id::as_str), Some("world_main"));
    }

    /// 跨层级跳到二级子图：trail 必须是 [root, mid, target]。
    #[test]
    fn drill_to_deep_target_builds_full_trail() {
        let w = demo_world();
        let mut d = DrillState::default();
        assert!(d.drill_to(&w, &Id::new_unchecked("bld_yamen")));
        let trail: Vec<&str> = d.trail().iter().map(Id::as_str).collect();
        assert_eq!(trail, vec!["world_main", "city_zunyi", "bld_yamen"]);
    }

    /// 跳到兄弟子图：trail 重写为新分支。
    #[test]
    fn drill_to_sibling_switches_branch() {
        let w = demo_world();
        let mut d = DrillState::default();
        // 先到 zunyi。
        d.drill_to(&w, &Id::new_unchecked("city_zunyi"));
        // 再到兄弟 chishui。
        assert!(d.drill_to(&w, &Id::new_unchecked("city_chishui")));
        let trail: Vec<&str> = d.trail().iter().map(Id::as_str).collect();
        assert_eq!(trail, vec!["world_main", "city_chishui"]);
    }

    /// 目标不在树里 → 返回 false，trail 保持不变（V1 守约镜像）。
    #[test]
    fn drill_to_unknown_target_keeps_trail() {
        let w = demo_world();
        let mut d = DrillState::default();
        d.drill_to(&w, &Id::new_unchecked("city_zunyi"));
        let before: Vec<String> = d.trail().iter().map(|i| i.as_str().to_owned()).collect();
        // 一个根本不在 maps 里的 id。
        assert!(!d.drill_to(&w, &Id::new_unchecked("map_never_existed")));
        let after: Vec<String> = d.trail().iter().map(|i| i.as_str().to_owned()).collect();
        assert_eq!(before, after, "失败时 trail 保持原状");
    }

    /// 悬空引用（demo_world 里的 `city_ghost`）→ BFS 静默跳过，不影响其他分支的可达性。
    #[test]
    fn drill_to_with_dangling_child_in_path_still_works() {
        let w = demo_world();
        let mut d = DrillState::default();
        // `world_main` 引用了 `city_ghost`（悬空），但 `city_zunyi` 是兄弟可达的。
        assert!(
            d.drill_to(&w, &Id::new_unchecked("city_zunyi")),
            "悬空兄弟节点不应阻塞其他分支"
        );
        // 跳到 `city_ghost` 自身则失败（不在 maps 里）。
        assert!(!d.drill_to(&w, &Id::new_unchecked("city_ghost")));
    }
}
