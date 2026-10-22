//! 渲染层状态容器（Bevy Resource）——ADR-0020 自 `wv-app::state` 整体迁入。
//!
//! ## 职责
//! - 声明**渲染与视图**所需的全部 Bevy Resource：`WorldProjection`（世界的工作副本）、
//!   `ViewState`（相机/图层）、`PlaybackState`、`DrillState`（下钻层级）、`BrushState`、
//!   `GridConfig` / `LodTier` / `ViewMode`、`MeasureState` 等。
//! - 提供类型与默认实现；wv-app::state 对它们做 `pub use` 重导出，
//!   应用侧既有 `crate::state::X` 路径零改动（ADR-0020 §2.3/§2.6）。
//!
//! ## 边界（不做什么）
//! - **不修改世界**——`WorldProjection.world` 只能由 `wv-edit` 的 `EditCommand` 改，
//!   渲染 System 不得直接动手（AGENTS.md §3.3）；渲染层的编辑意图走
//!   [`crate::editing_bridge`]，由 wv-app 翻译提交。
//! - **不持有时间结论**——`current_day` 等只存「视图当前在哪一刻」；
//!   任何「某时刻的状态」必须经 [`wv_core::time::TimelineQuery::at`] 求值（AGENTS §3.2）。
//! - **不是 source of truth**——YAML 才是；这里只是投影与视图状态。
//! - **不持有 UI/工程会话态**——`Panel` / `PanelState` / `RecentProjects` / 诊断缓存
//!   留在 `wv-app::state`（它们不属渲染）。

use bevy::prelude::*;
use wv_core::World;

/// 世界的**可变工作副本**。渲染层**只读**它，且不做任何时间比较（`AGENTS.md` §3.2）。
///
/// 所有「某时刻的世界状态」都必须经 [`wv_core::time::TimelineQuery::at`] 求出，
/// 结果写入各渲染资源（如 `world_projection::ProjectionFrame`），渲染 System 只读那些资源。
///
/// **写入路径**：只有应用层编辑链（`wv-app::edit`）可以改它，且必须把改动包成
/// `wv-edit` 的 `EditCommand` 经 `History` 提交（§3.3），System 不得直接改；
/// 渲染层的编辑意图走 [`crate::editing_bridge`] 由应用层翻译。
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
    /// 当前选中的实体（ADR-0017 §2.7）。
    ///
    /// 会话状态——`Some(s)` = 标签点击 / 详情面板引用，UI 据此高亮并打开面板；
    /// `None` = 未选中。**不**落盘，**不**经 `EditCommand`，**不**进
    /// `History.apply`（撤销实体编辑不应一并撤销"选谁"——选谁不是世界数据）。
    pub selection: Option<wv_core::SelectedEntity>,
}

impl WorldProjection {
    /// 是否有工程在手——「无工程闸门」的判据（2026-10-14 用户需求：
    /// 程序刚打开不能画图，必须先新建或打开一个工程）。
    ///
    /// 「新建工程」改为对话框直接落盘后，工程在手 ⇔ 有磁盘目录：
    /// 启动未加载 / 加载失败 / 关闭工程都回到 `false`（此时欢迎页接管界面，
    /// 笔刷与街道系统按同一判据禁画）。
    pub fn has_open_project(&self) -> bool {
        self.project_dir.is_some()
    }
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

/// 视图模式（基线 §1.10，D4 四态 + plan v0.1 D3 三模式）。
///
/// 浏览态默认底图自 [ADR-0012](file:///<root>/docs/adr/ADR-0012-terrain-default-vector-flat-shading.md) 起改为
/// 等值线矢量平涂（plan v0.1 D3 A 类）；原 Google Default Map 视觉（亚像素纹理合成）
/// 保留为 [`ViewMode::GoogleMap`] 可切换模式，**不再**是默认入口。Google 色板
/// （`theme/colors_google.rs`）作为可切换主题之一继续生效，但浏览态默认走
/// [`ViewMode::VectorFlat`] 的矢量管径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// H. 等值线矢量平涂：高度场 marching squares 等值线 + 类型场邻域边界 +
    /// 多边形平涂（plan v0.1 D3 A 类 · 浏览态默认 · [ADR-0012]）。
    ///
    /// 为什么是默认：与 ADR-0008 §2④「亚像素纹理合成」**同目标异路径**——
    /// 同样给「平滑地形边界」，但走的是几何对象（marching squares 等值线）、
    /// 而不是埋在像素位图里。山脊 / 海岸 / 大类色块的边界都成了可挂动画、
    /// 可挂样式、可挂 LOD 的对象；这是 Google Maps 那种像素滤镜给不出的
    /// 结构化产物。
    #[default]
    VectorFlat,
    /// G. Google Default Map：近白陆地 + 淡水 + 公园绿 + 白路灰缘。
    ///
    /// ADR-0012 之后**降为可切换主题之一**，不再是默认入口；色板与交互
    /// 仍走 [`crate::theme::colors_google`]。
    GoogleMap,
    /// A. 真实地形：高度图 + 法线光照 + 贴图。
    RealTerrain,
    /// B. 色块地形：每种地形一种颜色。
    Schematic,
    /// D. 势力图：颜色 = 势力，底色淡化地形。
    Faction,
    /// E. 事件图：极简底图 + 事件高亮。
    Event,
    /// F. 轮廓图：黑底 + 海岸线 + 城市路线（2026-11 用户需求：
    /// 地形纹理完全退场，只留水陆轮廓，要素对比度最大化）。
    Coastline,
    /// T. Topographic（水/陆 + height 3 阶着色，[ADR-0039]）。
    ///
    /// **不**走 ThemePack core 10 色，独立调色（`theme::topographic::TopographicPalette`）。
    /// 关 Roads / Streets / POI / Buildings / Faction / Labels / Veil / Atmosphere /
    /// TerrainDecor / FeatureOverlay——只留 Terrain（+ Isoline 子开关默认开）。
    ///
    /// biome 字段**存在但不参与显示**（X 路线，[ADR-0039] §1.3 / 决策 5）。
    /// 适合「快速读懂陌生地形」的分析视图，不强调 JRPG / 古舆图美术。
    ///
    /// [ADR-0039]: ../docs/adr/ADR-0039-topographic-viewmode.md
    Topographic,
}

impl ViewMode {
    pub const ALL: [ViewMode; 8] = [
        ViewMode::VectorFlat,
        ViewMode::GoogleMap,
        ViewMode::RealTerrain,
        ViewMode::Schematic,
        ViewMode::Faction,
        ViewMode::Event,
        ViewMode::Coastline,
        ViewMode::Topographic,
    ];

    /// 当前对作者开放的浏览模式。
    ///
    /// - [`ViewMode::VectorFlat`]：**矢量地形**——Google 地图矢量瓦片式渲染
    ///   （等值线描边 + 类型平涂，水陆边由高度场决定，ADR-0012）；
    /// - [`ViewMode::Schematic`]：**色块地形**——一格一色，直接呈现每格的地形类型；
    /// - [`ViewMode::Faction`]：**势力图**——分层填色 + 边界 + 声称/实控
    ///   （ADR-0027 D6，2026-10-18 对「只开放两种」的单点例外）。
    ///
    /// 顶栏分段控件、数字快捷键与命令面板都只列这份；其余模式保留在
    /// [`Self::ALL`] 与枚举里（数据层纹理管线仍支持，随时可重新开放）。
    pub const BROWSING: [ViewMode; 3] =
        [ViewMode::VectorFlat, ViewMode::Schematic, ViewMode::Faction];

    // 顶栏显示名（`label(loc)`）留在 wv-app：i18n 文案查表是外壳关注点
    // （AGENTS §2 依赖方向），应用层以扩展 trait `ViewModeLabelExt` 提供
    // 同名方法，调用点语法不变（ADR-0020 §2.3 落地注记）。
}

/// 相机与视图状态（缩放范围见基线 §2.12：0.1×–8.0×）。
#[derive(Resource)]
pub struct ViewState {
    pub mode: ViewMode,
    /// **地图主题**（配色轴，ADR-0016）：日间古舆图 / 暮色旧纸，与 `mode`
    /// （技法轴）正交。切换会触发底图纹理重建（`TerrainTexture.last_palette`）
    /// 与矢量层材质刷新（缓存键含本字段）。
    pub map_palette: crate::theme::palette::MapPalette,
    /// 主题包 id（ADR-0023 D1/D3）—— M1 阶段权威源仍是 `map_palette`；
    /// M2 把权威迁到本字段、保留 `map_palette` 作为兼容 fallback。
    ///
    /// 持久化键沿用 `ui-layout.yaml::map_theme` 字符串（D3 零迁移）；内置主题
    /// id 与 `MapPalette` key 同名（`daylight` / `dusk`），存量 YAML 不动。
    pub theme_pack_id: Option<String>,
    /// **当前生效的主题包**（ADR-0023 M2：权威源 = `theme_pack_id`）。
    ///
    /// 为什么挂在 `ViewState` 上而不是独立的 `ActiveThemePack` Resource：
    /// Bevy 的 system 参数上限是 16，`wv-app` 的几个重载 system 已经贴着上限
    /// （`poll_open_project_dialog` 原本正好 16 个）——再加一个 Resource 参数
    /// 就超限。`ViewState` 是所有消费点**本来就持有**的载体，挂这里零参数成本。
    ///
    /// 值由 `wv_render::theme::switch::sync_active_theme_pack` 写入（PreUpdate
    /// 最前，**先于**所有消费点）。
    pub active_theme: &'static crate::theme::theme_pack::ThemePack,
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
    /// 标签层是否可见（ADR-0017 · 标签层会话开关）。
    ///
    /// 与 YAML 中 `Layer.visible: false` **合取**决定标签是否参与渲染
    /// （参见 `LayerFrame::resolve`）—— 面板布尔是本会话临时隐藏，
    /// YAML 是作者持久声明。
    pub labels_visible: bool,
    /// 地点（POI）图层是否可见：锚定地点的点标（矢量图层底子）。
    pub places_visible: bool,
    /// 建筑（2.5D 挤出）图层是否可见（ADR-0016 §6）。
    pub buildings_visible: bool,
    /// 势力多边形叠加层是否可见（S12）。
    pub factions_visible: bool,
    /// 参照网格（逐格编辑网格叠加层）是否可见。
    ///
    /// 2026-10-15 起**与笔刷解耦**（33b65b9：不再是「笔刷开着恒显」的或分支）；
    /// 2026-10-16 字段本身从 [`BrushState`] 搬入视图态——开关 UI 也随之上
    /// 地图工具条（`map_controls` 瓷砖），它本来就是「看图方式」而非「笔刷参数」，
    /// 与 `labels_visible` 等同族。
    pub grid_lines: bool,
    /// 抓手工具（手形 H）：开启后**左键拖拽**平移地图。
    ///
    /// 会话态、不写盘：与 [`MeasureMode`] 同口径（每次重启从关闭起）。
    /// 归属视图态而不是 `BrushState`——它改的是相机，不是笔刷参数；由
    /// `map_controls` 与 `CommandId::SelectMapTool` 互斥地写入（开抓手 =
    /// 关选区 / 测量 / 笔刷 / 街道），`camera::camera_pan` 只读它。
    pub pan_tool: bool,
    /// 鼠标在地图上的世界坐标（顶栏坐标显示用）。
    pub cursor_world: Option<Vec2>,
    /// 主窗口逻辑尺寸——分级缩放区间（`zoom_level`）算 `fit` 需要它。
    ///
    /// 由 `camera::track_cursor_world` 每帧顺路刷新（不另开系统）。
    pub window_size: Vec2,
    /// 当前地图 ID。
    pub current_map: String,
    /// Topographic viewmode 百分位阈值缓存（[ADR-0039] 决策 5）。
    ///
    /// Key = map id（与 `current_map` 同口径字符串）；Value = (`p33`, `p66`) 两个
    /// height 阈值（米）。**首次进入 Topographic mode 时按图填一次**，
    /// `data_hash` 变化时（编辑/地形重构）才重算。
    ///
    /// biome 通道不参与计算（[ADR-0039] X 路线）。
    /// 用 `String` key 而非 `wv_core::Id` 是为了与 `current_map` 同款（避免
    /// 跨 crate 的 `Id` 耦合；map id 在 wv-render 是字符串口径）。
    ///
    /// [ADR-0039]: ../docs/adr/ADR-0039-topographic-viewmode.md
    pub topographic_thresholds: std::collections::HashMap<String, (i16, i16)>,
    /// 2.5D vertex Z 挤压开关（[ADR-0039] 决策 6 / [ADR-0041]）。
    ///
    /// **本字段是预埋**——本 PR-1 仅声明不写行为；由 PR-3 ([ADR-0041]) 在
    /// `resolve_render_recipe` PreUpdate 阶段落地以下耦合：
    /// `view.mode == Topographic && !terrain_2_5d_enabled` → 自动 on。
    ///
    /// 当前默认 `false`；mode-coupled on 行为落地前，Topographic mode 下
    /// 高度立体感不启用（仅 3 阶着色）。
    ///
    /// [ADR-0041]: ../docs/adr/ADR-0041-topographic-2-5d-vertex-z.md
    pub terrain_2_5d_enabled: bool,
    /// 2.5D vertex Z 挤压系数（[ADR-0041] 决策 2）。
    ///
    /// mesh 顶点 z = `cell.height × compression × effective_scale`。
    ///
    /// - 默认 `0.3`（长途跨度地形专用，[ADR-0041] §3 推荐），
    ///   而非 [ART_STYLE_SPEC §1.1](docs/ART_STYLE_SPEC_v0.1.md) 默认的
    ///   `0.5×`——后者适合 32× 起步的城市子图，前者适合 guangdong 级（50km 跨度）
    ///   长途地形（避免山峰过度耸立遮挡背后内容）。
    /// - 范围 `[0.0, 1.0]`：>1.0 时画面像城市场景，与 Topographic 语义不符；
    ///   `0.0` 即纯 2D（无立体感）。
    /// - 切换即触发 chunk mesh rebuild（`data_hash` 联动 + compression cache key）。
    ///
    /// 后续 v2 留口:per-mode 自适应（子图用 0.5，长途用 0.3，DrillState.zoom_level 联动）。
    pub topographic_compression: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            mode: ViewMode::default(),
            scale: 1.0,
            map_palette: Default::default(),
            theme_pack_id: None,
            active_theme: &crate::theme::theme_pack::builtin::DEFAULT,
            camera_center: Vec2::ZERO,
            terrain_visible: true,
            terrain_soften: true,
            events_visible: true,
            tracks_visible: true,
            roads_visible: true,
            labels_visible: true,
            places_visible: true,
            buildings_visible: true,
            factions_visible: true,
            grid_lines: false,
            pan_tool: false,
            cursor_world: None,
            window_size: Vec2::new(1280.0, 800.0),
            current_map: "world_main".into(),
            topographic_thresholds: std::collections::HashMap::new(),
            terrain_2_5d_enabled: false,
            topographic_compression: 0.3,
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

/// 淡出带位置 0..1：`scale` **大 = 远**。近端 `0.75×threshold` 之前为 0，
/// 远端 `1.125×threshold` 之后为 1，中间 smoothstep `3t²−2t³`。
///
/// 语义是「离 Nation 档有多近」——**可见度 = `1 − lod_fade`**。把它做成纯函数
/// 是为了让渲染层所有硬 LOD 剔除点（建筑）共用同一条过渡曲线（spec §4.2 缺省）。
/// POI 已改走 [`poi_visible_at_rank`] 的档位硬门（ADR-0030 D2），不再吃淡出带。
pub fn lod_fade(scale: f32, threshold: f32) -> f32 {
    let lo = threshold * 0.75;
    let hi = threshold * 1.125;
    if scale <= lo {
        0.0
    } else if scale >= hi {
        1.0
    } else {
        let t = (scale - lo) / (hi - lo);
        t * t * (3.0 - 2.0 * t)
    }
}

/// POI 呈现门槛（ADR-0030 D2）：视野档位 × 最低级别，**硬门**。
///
/// Nation 档只画 rank ≥ 3（国家级），Region 档 ≥ 2（市镇级起），Edit 档全见。
/// 以硬门取代旧「非城市淡出带」：档位边界处点标直接出现/消失（缩放运动掩盖
/// pop-in，Google 同款）。`rank` 缺省的旧工程经 `Place::effective_rank` 折算
/// （w/h → 3，否则 1）后，与引入分级前的两档行为逐位一致。
///
/// ADR-0047 D2 起本函数只是 [`rank_visible_at_tier`] 的 POI 别名——门槛数值
/// 只有这一份，POI / 道路 / 街道 / 河流共用。
pub fn poi_visible_at_rank(rank: u8, tier: LodTier) -> bool {
    rank_visible_at_tier(rank, tier)
}

/// 统一呈现门槛（ADR-0047 D2）：视野档位 × 最低级别，**硬门**，全要素共用。
///
/// 词表即 GLOSSARY「呈现级别」：`3` 国家级、`2` 区域/市镇级、`1` 局部、
/// `0` 网络节点/微点。Nation 档放行 rank ≥ 3，Region 档 ≥ 2，Edit 档全见。
///
/// 消费方与各自的级别来源：
/// - POI：`Place::effective_rank`（w/h 折算）→ 本函数；
/// - 世界道路：`PlaceEdge::effective_rank`（距离折算）→ 本函数；
/// - 城市街道：`RoadClass::presentation_rank_from_lod`（lod 折算）→ 本函数；
/// - 河流：派生长度定级（`compute_projection` 内）→ 本函数。
///
/// 阈值是表现层参数，收口在这一个 match 里；改门槛 = 改这里 + 对应测试。
pub fn rank_visible_at_tier(rank: u8, tier: LodTier) -> bool {
    match tier {
        LodTier::Nation => rank >= 3,
        LodTier::Region => rank >= 2,
        LodTier::Edit => true,
    }
}

#[cfg(test)]
mod lod_tests {
    use super::*;

    /// 淡出带锚点（手算）：lo=2.25（0）、阈值 3.0（t=2/3 → 0.740）、hi=3.375（1）。
    #[test]
    fn lod_fade_anchors_at_threshold_band() {
        assert_eq!(lod_fade(2.25, 3.0), 0.0, "带下沿为 0");
        assert_eq!(lod_fade(3.375, 3.0), 1.0, "带上沿为 1");
        let mid = lod_fade(3.0, 3.0);
        assert!(
            (mid - 0.740).abs() < 1e-3,
            "阈值处 smoothstep(2/3)≈0.740，得 {mid}"
        );
    }

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

    /// 呈现门槛逐档手算（ADR-0030 D2）：Nation 只放行 3，Region 放行 2+，
    /// Edit 全放行。rank 0（网络节点）任何档都不被 Nation/Region 放行。
    #[test]
    fn poi_gate_admits_higher_ranks_at_closer_tiers() {
        for r in 0..=2u8 {
            assert!(
                !poi_visible_at_rank(r, LodTier::Nation),
                "Nation 档不放行 rank {r}"
            );
        }
        assert!(
            poi_visible_at_rank(3, LodTier::Nation),
            "Nation 档放行 rank 3"
        );
        assert!(
            !poi_visible_at_rank(1, LodTier::Region),
            "Region 档不放行 rank 1"
        );
        assert!(
            poi_visible_at_rank(2, LodTier::Region),
            "Region 档放行 rank 2"
        );
        for r in 0..=3u8 {
            assert!(
                poi_visible_at_rank(r, LodTier::Edit),
                "Edit 档放行 rank {r}"
            );
        }
    }

    /// 统一门槛（ADR-0047 D2）与 POI 别名同值（门槽数值只有一份的证据），
    /// 且 Nation/Region 两档的边界行为与手算一致。
    #[test]
    fn unified_rank_gate_matches_poi_alias_and_hand_bounds() {
        for r in 0..=4u8 {
            for tier in [LodTier::Edit, LodTier::Region, LodTier::Nation] {
                assert_eq!(
                    rank_visible_at_tier(r, tier),
                    poi_visible_at_rank(r, tier),
                    "rank {r} × {tier:?}：统一门槛必须与 POI 别名同值"
                );
            }
        }
        assert!(rank_visible_at_tier(3, LodTier::Nation));
        assert!(!rank_visible_at_tier(2, LodTier::Nation));
        assert!(rank_visible_at_tier(2, LodTier::Region));
        assert!(!rank_visible_at_tier(1, LodTier::Region));
        assert!(rank_visible_at_tier(0, LodTier::Edit));
    }
}

/// 笔刷工具（意图笔刷 plan v0.1）：直接涂类型、六种「给意图、程序生成」，
/// 或地形橡皮刷。
///
/// 会话态，不写盘；工具选择不跨会话保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrushTool {
    /// 直接涂抹地形类型（原有行为，默认）。
    #[default]
    PaintKind,
    /// 意图：沿拖拽线长一道山脉；单点 = 一座圆山；跨海自动成岛链。
    MountainRange,
    /// 意图：抬起一片顶面平坦的高原。
    Plateau,
    /// 意图：加一片低矮起伏（仅陆上）。
    Hills,
    /// 意图：点一下放一座带火山口的锥形山；海上自动成岛。
    Volcano,
    /// 意图：点一下挖一个碗形湖（碗底低于溢出口）。
    Lake,
    /// 意图：从源头拖到河口，程序拓扑挖出河床。
    River,
    /// 地形橡皮刷（ADR-0043）：把笔下格子**整格回填成本图的底图态**——
    /// 底图是海洋就擦成海洋，底图是陆地就擦成陆地。
    ///
    /// 与 [`Self::PaintKind`] 的分工：涂是「按色板写入某一种地形」，擦是
    /// 「撤销回该图的初始态」。两者共用 `PaintTerrainCommand` 的撤销 / 脏矩形
    /// 管线，差别只在写入什么格子（见 `wv_terrain::edit::Brush::exact_fill`）。
    ///
    /// **枚举里必须排末位**：`CommandId::SelectBrushTool(i)` 按下标索引
    /// [`Self::ALL`]，中间插入会静默改掉后面所有工具的命令面板绑定。
    Eraser,
}

impl BrushTool {
    /// 面板遍历用。顺序即 `CommandId::SelectBrushTool` 的下标，**只增不改**。
    pub const ALL: [BrushTool; 8] = [
        BrushTool::PaintKind,
        BrushTool::MountainRange,
        BrushTool::Plateau,
        BrushTool::Hills,
        BrushTool::Volcano,
        BrushTool::Lake,
        BrushTool::River,
        BrushTool::Eraser,
    ];

    /// 「塑」组（拖拽骨架、程序生成）四工具——面板第二行。
    pub const SHAPE: [BrushTool; 4] = [
        BrushTool::MountainRange,
        BrushTool::Plateau,
        BrushTool::Hills,
        BrushTool::River,
    ];

    /// 「放」组（单击种子、按半径扩散）两工具——面板第三行。
    pub const PLACE: [BrushTool; 2] = [BrushTool::Volcano, BrushTool::Lake];

    /// 所属分组（面板三行行内布局与命令面板注册共用同一口径）。
    pub fn group(self) -> BrushToolGroup {
        match self {
            Self::PaintKind | Self::Eraser => BrushToolGroup::Paint,
            Self::MountainRange | Self::Plateau | Self::Hills | Self::River => {
                BrushToolGroup::Shape
            }
            Self::Volcano | Self::Lake => BrushToolGroup::Place,
        }
    }

    /// 是否意图工具（决定提交 `IntentTerrainCommand` 而非 `PaintTerrainCommand`）。
    pub fn is_intent(self) -> bool {
        self.group() != BrushToolGroup::Paint
    }

    /// 「涂」组（[`BrushToolGroup::Paint`]）两工具——面板第一行。
    ///
    /// 涂与擦同组：两者都是「对既有地表做增量写入」，只是写入目标不同；
    /// 分成两组会让分段控件多出一格，而那一格承载不了实质区别。
    pub const PAINT: [BrushTool; 2] = [BrushTool::PaintKind, BrushTool::Eraser];
}

/// 笔刷工具的三个操作语义分组（面板行内布局的行首标签）。
///
/// 分组的差别是**作者的手在做什么**，不只是产出什么：
/// - [`BrushToolGroup::Paint`]：分配式——涂抹即所得，每个落点都是印记；
/// - [`BrushToolGroup::Shape`]：演绎式——拖的是几何骨架（脊线 / 台缘 /
///   覆盖域 / 河床线），形状由 `wv_terrain::intent` 程序派生；
/// - [`BrushToolGroup::Place`]：种子式——单击落种子，圆盘按半径扩散。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrushToolGroup {
    /// 涂：直接涂地形类型。
    Paint,
    /// 塑：拖拽骨架、程序生成。
    Shape,
    /// 放：单击种子、扩散成形。
    Place,
}

/// 笔刷工具状态（`AGENTS.md` §3.3：System 只生成命令，不直接改世界）。
#[derive(Resource)]
pub struct BrushState {
    /// 当前选中的地形类型编号（仅 Paint 工具用）。
    pub kind: u8,
    /// 当前选中的工具组（笔刷面板 3 段控件用：涂 / 塑 / 放）。
    ///
    /// 2026-10-16 增：段控件需要单独持有「当前组」状态，瓷砖行**按组过滤**渲染。
    /// `tool` 与 `tool_group` 在所有写入路径（面板段、面板瓷砖、命令面板
    /// `SelectBrushTool`）由 `sync_tool_group` 同步，**禁止**单独写其中一个。
    pub tool_group: BrushToolGroup,
    /// 当前工具（直接涂类型 / 意图生成）。
    pub tool: BrushTool,
    /// 笔刷半径（世界单位）。River 不用半径（河宽固定一格）。
    pub radius: f32,
    /// 衰减软度 0..=1（与 `wv_terrain::edit::falloff_weight` 同一参数；仅 Paint 工具用）。
    pub softness: f32,
    /// 意图强度（0.2..=2.0，wv_terrain::intent 内部夹取）。
    pub strength: f32,
    /// 是否处于涂抹模式（按住左键拖拽中）。
    pub painting: bool,
    /// 笔刷是否启用。
    pub enabled: bool,
}

/// 测量工具模式（2026-10-14）。
///
/// 枚举是**会话态**——不写盘。每次切图或重启应用都从 `Off` 起。
/// 草稿数据（当前画的折线 / 矩形首点）走 [`MeasureState::draft`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeasureMode {
    /// 不启用测量工具（默认值）。
    #[default]
    Off,
    /// 折线：多次左键累积点；右键 / Enter / 双击 / Esc 收笔。
    Line,
    /// 矩形：两次左键确定两对角；第二次点击即收。
    Rect,
    /// 橡皮：点击容差内的 measurement 即删除。
    Eraser,
}

impl MeasureMode {
    /// 是否处于「启用」状态（≠ Off）。给按钮高亮 / 互斥闸共用。
    pub fn is_active(self) -> bool {
        self != Self::Off
    }

    /// 该模式下鼠标左键是否被测量工具「吃掉」（不再驱动其它交互）。
    /// Off / Eraser 模式下左键仍属地图（橡皮点击本身是测量输入）。
    pub fn consumes_left_click(self) -> bool {
        matches!(self, Self::Line | Self::Rect)
    }
}

/// 测量会话状态（`AGENTS.md §3.3`：System 只生成命令，不直接改世界）。
///
/// 与 [`BrushState`] 平级——独立的工具资源。**与笔刷开启同一模式时**
/// 用 [`Self::mode`] = Off 把笔刷顶下去（街道 / 笔刷都走同样的「工具语义」）。
#[derive(Resource)]
pub struct MeasureState {
    /// 当前模式（默认 `Off`）。
    pub mode: MeasureMode,
    /// 当前草稿：**本图局部坐标**。
    ///
    /// - `Line` 模式：≥1 表示已起笔，渲染时画「末段 → 光标」的橡皮筋；
    /// - `Rect` 模式：长度 0/2（0 = 未起笔，2 = 已点击两点即可提交）。
    pub draft: Vec<Vec2>,
    /// 切图时强制清空——测量**不能跨图**。
    pub last_map_id: Option<wv_core::Id>,
    /// 当前草稿绑定的图 ID。`commit_measurement_click` 用它判断「草稿已
    /// 切换图」并丢弃旧草稿（不污染新图）。
    pub draft_map_id: Option<wv_core::Id>,
}

impl Default for MeasureState {
    fn default() -> Self {
        Self {
            mode: MeasureMode::Off,
            draft: Vec::new(),
            last_map_id: None,
            draft_map_id: None,
        }
    }
}

impl MeasureState {
    /// 清空草稿（提交 / 切模式 / 切图时调）。
    pub fn clear_draft(&mut self) {
        self.draft.clear();
        self.draft_map_id = None;
    }

    /// 切图时调用——`new_map` 与 `last_map_id` 不一致则丢弃草稿。
    pub fn on_map_change(&mut self, new_map: Option<&wv_core::Id>) {
        if self.last_map_id.as_ref() != new_map {
            self.clear_draft();
        }
        self.last_map_id = new_map.cloned();
    }
}

/// 选择器的形状（ADR-0043）。
///
/// **三种形状在数据层没有区别**——成形后都只是 `SelectionState::closed` 里的一
/// 条点环。下游（意图菜单 / 大陆生成 / 城镇 / 地物标记 / 势力划分 / CellMap 批量
/// 改）读的全都是那条环，因此新增矩形与椭圆对它们**零改动**。
///
/// 差别只发生在**交互期**：多边形逐点点击成环，矩形 / 椭圆按住拖拽一次成形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionShape {
    /// 多边形：逐点点击 + 橡皮筋，锚点可拖 / 双击删 / 点边插（Figma pen）。
    #[default]
    Polygon,
    /// 矩形：按住拖出对角框；成形后整体可拖、拖角缩放。
    Rect,
    /// 椭圆：按住拖出内接椭圆框；成形后整体可拖、拖角缩放。
    Ellipse,
}

impl SelectionShape {
    /// 是否逐点成环（只有多边形是）。false = 按住拖拽成形。
    pub fn is_polygon(self) -> bool {
        self == Self::Polygon
    }
}

/// 框选形状的拖拽会话（ADR-0043）。
///
/// 三种手势共用「按下点 → 当前点」这一对坐标，区别只在松手时怎么用：
/// - [`Self::Draw`]：还没成形，拖出一个新框；
/// - [`Self::Move`]：已成形的形状整体平移；
/// - [`Self::Resize`]：已成形的形状按包围盒重算。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeDrag {
    /// 拖出新框：`from` = 按下点（不动），`to` = 当前点。
    Draw { from: Vec2, to: Vec2 },
    /// 整体平移：`from` / `to` 是同一手势的按下点与当前点，位移 = `to - from`。
    Move { from: Vec2, to: Vec2 },
    /// 缩放：把原包围盒整体按 `to - from` 推移后重算边界。
    Resize { from: Vec2, to: Vec2 },
}

impl ShapeDrag {
    /// 按下点。
    pub fn from(&self) -> Vec2 {
        match *self {
            Self::Draw { from, .. } | Self::Move { from, .. } | Self::Resize { from, .. } => from,
        }
    }

    /// 当前点。
    pub fn to(&self) -> Vec2 {
        match *self {
            Self::Draw { to, .. } | Self::Move { to, .. } | Self::Resize { to, .. } => to,
        }
    }

    /// 更新当前点（按住拖动时逐帧调）。
    pub fn set_to(&mut self, to: Vec2) {
        match self {
            Self::Draw { to: t, .. } | Self::Move { to: t, .. } | Self::Resize { to: t, .. } => {
                *t = to;
            }
        }
    }
}

/// 选择工具会话态（2026-10-17 选择工具 spec §3；M1 Task 1；ADR-0043 增形状）。
///
/// pen 方法的数据底座：草稿锚点（Drawing）和闭合选区（Closed）二态互斥——
/// `draft` 非空即画中，`closed` 为 `Some` 即已闭合。选区是**会话态**（spec D11）：
/// 不落盘、不进 YAML；意图确认时由 wv-app **只读克隆**环后自行提交
/// （ADR-0033：不再经 [`Self::consume_closed`] 消费；该方法保留为通用原语）。
/// 坐标为**当前图局部 wu**（与 [`MeasureState::draft`] 同口径）。
///
/// 手感对齐 Figma pen：逐点 + 橡皮筋、双击 / 回车 / 点首锚闭合、
/// 锚点可拖 / 双击删 / 点边插——交互语义见 `crate::selection`。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct SelectionState {
    /// 选择工具是否启用（工具条瓷砖切换；与测量的互斥由 wv-app 瓷砖 toggle 家族处理）。
    pub enabled: bool,
    /// 当前形状（ADR-0043）。`Polygon` 走原有的逐点交互，另两种走拖拽成形。
    pub shape: SelectionShape,
    /// 框选的进行中手势。`Some` = 左键正按住。
    pub drag: Option<ShapeDrag>,
    /// 画中草稿锚点（未闭合）。
    pub draft: Vec<(f32, f32)>,
    /// 已闭合选区（≥3 点）。`Some` = Closed 态。
    pub closed: Option<Vec<(f32, f32)>>,
    /// Closed 态下被选中（待拖动）的锚点下标。
    pub selected_anchor: Option<usize>,
    /// 上一次左键落点与时刻（双击判定，`crate::selection::handle_click` 用）。
    pub last_click: Option<((f32, f32), f64)>,
    /// 草稿 / 选区绑定的图 ID——[`Self::on_map_change`] 变图即清（spec D9：切图 = 取消选择）。
    pub last_map_id: Option<wv_core::Id>,
}

/// Inspector Panel 写 → `apply_pending_commands` 读 → `EditCellCommand::apply`
/// 的草稿载荷([ADR-0040] §决策 5)。
///
/// 为什么存进 `EditCellState.pending` 而不是 `CommandId::EditCell(...)` 嵌套 5 字段:
/// - 与 `SelectionState.draft` 同款「draft → take」节奏,降低心智；
/// - 编辑面板多字段一次性封装成不可变快照,与其他 Edit* 命令一致；
/// - 枚举 variant 写 5 字段会让 pattern match 笨拙。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditCellPending {
    pub map: wv_core::Id,
    pub gx: i32,
    pub gy: i32,
    pub before: wv_core::terrain::TerrainCellData,
    pub after: wv_core::terrain::TerrainCellData,
}

/// 单格编辑工具状态（[ADR-0040] §决策 5）。
///
/// 与 [`SelectionState`] 平级：**会话态,本地图使用**——
/// `last_map_id` 变即清空（防「切换图后旧 hover 卡在画面上」）。
///
/// **三层状态**：
/// - `hover` = 当前鼠标压在哪一格（即时刷新,Inspector 不必读）；
/// - `selected` = 点选 committed 的那一格（Inspector 与 PassId::CellSelection
///   必读,ESC 清空）；
/// - `pending` = Inspector 字段改完之后,等待 `apply_pending_commands` 取走
///   的草稿（[EditCellPending]）。分发器读 + `take()` 后由 EditCellCommand
///   走 [wv_edit] 的 snapshot-diff-revert 路径落盘。
///
/// hover 与 selected 与 pending 共存不冲突——hover 来自光标,selected 来自点击,
/// pending 来自字段改。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct EditCellState {
    /// EditCell 工具是否激活（与 [`BrushState::enabled`] 同款会话布尔）。
    /// 由命令分发器在 `SelectMapTool(EditCell)` 翻 true / 切走其它工具翻 false。
    pub tool_active: bool,
    /// 当前鼠标压住的格 (gx, gy)；`None` 表示鼠标不在地图内。
    pub hover: Option<(i32, i32)>,
    /// 点击落定的格 (gx, gy)。ESC 清空 / 切图也清空。
    pub selected: Option<(i32, i32)>,
    /// Inspector 待分发的字段变化草稿。
    pub pending: Option<EditCellPending>,
    /// 多边形选区贴合的方格集合（2026-10-20 批量编辑）：V 工具闭合多边形后
    /// 由 `sync_region_cells` 栅格化写入（cell 中心点落在多边形内即选中）。
    /// 空 = 无区域选区。高亮渲染与批量应用（BatchEditCellsCommand）都消费它。
    pub region_cells: Vec<(i32, i32)>,
    /// 上次栅格化时**多边形**的指纹（2026-10-20 性能修）。
    ///
    /// `sync_region_cells` 每帧都会跑；多边形没变时**不必重算**格集合
    /// （栅格化是 O(bbox 格数 × 边数)，大选区每帧重算会明显吃帧）。
    /// 指纹变了（或为 0 = 从未算过）才重算。
    pub region_fingerprint: u64,
    /// 绑定的图 ID；变图即清空选中格(与 [`SelectionState::last_map_id`]
    /// 一致语义,统一通过 [`EditCellState::on_map_change`] 处理)。
    pub last_map_id: Option<wv_core::Id>,
}

impl EditCellState {
    /// 清空 tool_active + hover + selected + pending；
    /// **不会**清 `last_map_id`（那是别处碰）。
    pub fn clear(&mut self) {
        self.tool_active = false;
        self.hover = None;
        self.selected = None;
        self.pending = None;
        self.region_cells.clear();
        self.region_fingerprint = 0;
    }

    /// 切换图时调用——如果新图 ≠ 上次记录的图,清空所有格相关字段
    /// （与 [`SelectionState`] 的 `on_map_change` 同口径）。
    pub fn on_map_change(&mut self, new_map: &wv_core::Id) {
        if self.last_map_id.as_ref() != Some(new_map) {
            self.hover = None;
            self.selected = None;
        }
        self.last_map_id = Some(new_map.clone());
    }
}

impl SelectionState {
    /// 画中（草稿非空）。
    pub fn is_drawing(&self) -> bool {
        !self.draft.is_empty()
    }

    /// 已闭合。
    pub fn is_closed(&self) -> bool {
        self.closed.is_some()
    }

    /// 左键落点：追加到草稿（Closed 态的点击归 `crate::selection::handle_click`）。
    pub fn add_point(&mut self, p: (f32, f32)) {
        self.draft.push(p);
    }

    /// 收笔：≥3 点才成环（成功 `true`）；失败清空草稿——退化多边形不产生选区，
    /// 作者不会得到一个点了两次却没反应的谜（内核硬前提在交互层挡住，
    /// `ContinentSketch::try_close` 同语义）。
    pub fn try_close(&mut self) -> bool {
        match std::mem::take(&mut self.draft) {
            pts if pts.len() >= 3 => {
                self.closed = Some(pts);
                true
            }
            _ => false,
        }
    }

    /// Esc（画中）：只丢草稿，已闭合选区保留——作者辛苦圈的地不因手滑蒸发。
    pub fn discard_draft(&mut self) {
        self.draft.clear();
    }

    /// 取消选择：草稿与闭合一起清（点空白 / 切工具 / 切图）。不动 `enabled`——
    /// 工具开关归瓷砖，取消的是「这次选择」不是「这个工具」。
    pub fn cancel(&mut self) {
        self.draft.clear();
        self.closed = None;
        self.selected_anchor = None;
        self.last_click = None;
        self.drag = None;
    }

    /// 整体平移闭合选环（ADR-0043 框选的 Move 手势）。
    ///
    /// 走**逐点平移**而不是「只挪包围盒」——两者对矩形 / 椭圆等价，但逐点平移
    /// 让这段代码对任何环都正确，将来加第五种形状时不必再想一遍。
    pub fn translate_closed(&mut self, delta: Vec2) -> bool {
        match self.closed.as_mut() {
            Some(ring) => {
                for p in ring.iter_mut() {
                    p.0 += delta.x;
                    p.1 += delta.y;
                }
                true
            }
            None => false,
        }
    }

    /// 用新的两角点重算整个选环（ADR-0043 框选的 Resize 手势）。
    ///
    /// 形状由 [`Self::shape`] 决定：矩形出 4 角，椭圆出 48 段。**不是**在旧环上
    /// 插删点——那样椭圆会被拉成多边形。旧环的包围盒偏移量由调用方算好传进来。
    pub fn rebuild_closed(&mut self, a: Vec2, b: Vec2) -> bool {
        if a.x == b.x && a.y == b.y {
            return false;
        }
        let ring = match self.shape {
            SelectionShape::Rect => crate::selection::rect_ring(a, b),
            SelectionShape::Ellipse => crate::selection::ellipse_ring(a, b),
            // 多边形没有「重算边界」这回事——它的边界就是作者逐点敲出来的。
            SelectionShape::Polygon => return false,
        };
        self.closed = Some(ring);
        self.selected_anchor = None;
        true
    }

    /// stash 回驻留（大陆生成撤销恢复圈，spec §4）：直接落为闭合选区（替换现状）。
    pub fn close_with(&mut self, pts: Vec<(f32, f32)>) {
        self.draft = Vec::new();
        self.closed = Some(pts);
        self.selected_anchor = None;
        self.last_click = None;
    }

    /// 取走闭合选环并复位选中锚点（通用会话态原语；ADR-0033 后意图不再调用它）。
    pub fn consume_closed(&mut self) -> Option<Vec<(f32, f32)>> {
        self.selected_anchor = None;
        self.closed.take()
    }

    /// 拖动闭合选区的锚点；越界返回 `false`（防御——系统层按命中下标调用）。
    pub fn move_anchor(&mut self, i: usize, p: (f32, f32)) -> bool {
        match self.closed.as_mut() {
            Some(ring) if i < ring.len() => {
                ring[i] = p;
                true
            }
            _ => false,
        }
    }

    /// 删锚（双击锚点）；剩余 <3 点时闭合**降级回画中**——绝不留下悬空的
    /// 退化选区喂内核（spec Review Focus 3）。
    pub fn delete_anchor(&mut self, i: usize) {
        let Some(ring) = self.closed.as_mut() else {
            return;
        };
        if i >= ring.len() {
            return;
        }
        ring.remove(i);
        if ring.len() < 3 {
            self.draft = self.closed.take().unwrap_or_default();
        }
    }

    /// 点边插入：在顶点 `splice`（1..=len，i-1 与 i%len 之间）处插入新锚点。
    pub fn insert_anchor(&mut self, splice: usize, p: (f32, f32)) {
        if let Some(ring) = self.closed.as_mut() {
            let splice = splice.clamp(1, ring.len());
            ring.insert(splice, p);
        }
    }

    /// 变图清空（`MeasureState::on_map_change` 同款）——测量不能跨图，选区同理。
    pub fn on_map_change(&mut self, new_map: Option<&wv_core::Id>) {
        if self.last_map_id.as_ref() != new_map {
            self.cancel();
        }
        self.last_map_id = new_map.cloned();
    }
}

impl Default for BrushState {
    fn default() -> Self {
        Self {
            kind: 3, // 默认草原
            tool: BrushTool::PaintKind,
            tool_group: BrushToolGroup::Paint, // 与 tool.group() 一致
            radius: 96.0,
            softness: 0.5,
            strength: 1.0,
            painting: false,
            // Google 式默认手势是左键拖动平移；笔刷必须显式开启，避免一开图就误改。
            enabled: false,
        }
    }
}

impl BrushState {
    /// 段控件路径：调用方刚改了 `tool_group`——把 `tool` 落到该组第一个。
    ///
    /// 设计取舍：**`tool_group` 是 UI 主状态**（段控件高亮就代表选工具），
    /// 段切换比瓷砖切换粒度更粗。所以「段被切」= 强制重置 tool 到该组首项，
    /// 旧 tool 即便仍在该组内也归零（避免「塑」段亮着但 `tool=Hills` 这种
    /// 「随机停留」感）。
    pub fn on_group_changed(&mut self) {
        self.tool = match self.tool_group {
            BrushToolGroup::Paint => BrushTool::PaintKind,
            BrushToolGroup::Shape => BrushTool::SHAPE[0],
            BrushToolGroup::Place => BrushTool::PLACE[0],
        };
    }

    /// 瓷砖 / `SelectBrushTool` 路径：调用方刚改了 `tool`——把 `tool_group`
    /// 跟它走（不重置 tool）。该函数是面板与命令面板共用的轻量同步。
    pub fn on_tool_changed(&mut self) {
        self.tool_group = self.tool.group();
    }
}

/// 网格几何参数（世界单位与格的换算，基线 §2.15 变换链的输入）。
///
/// ## 两套原点——先问「这份坐标属于哪个帧」，再挑字段
///
/// 视图空间**就是**根图坐标系（相机、[`crate::state::ViewState::cursor_world`]、
/// `drill_target` 全在根帧，见 [`crate::drill`]）。坐标要落到视图空间时，
/// 按数据来源选字段，不要顺手拿一个：
///
/// | 字段 | 含义 | 谁该用 |
/// |---|---|---|
/// | [`Self::raster_origin`] | 当前显示的**栅格纹理**的原点（栅格空间 `(0,0)` 的视图位置） | **地形**：笔刷拾取、chunk 摆位、小地图——它们换算的是**纹理格** |
/// | [`Self::map_origin`] | **当前图**的图帧原点（本图局部 `(0,0)` 的视图位置） | **本图局部内容**：地点、街道与街道名、地图外框 |
///
/// 两者只在「当前图自带底图」时相等。子图**继承**父图底图时（ADR-0009 底图继承），
/// 栅格仍是父图那一张、`raster_origin` 停在父图原点，而 `map_origin` 随当前图走：
/// 1024² 的城市图用 `raster_origin` 换算自己的街道，整层会被画到 `(0..1024)`
/// ——世界图左上角、视野外，也就是「钻进城看不到武昌」的根因。
#[derive(Resource)]
pub struct GridConfig {
    /// 每格世界边长。
    pub cell_size: f32,
    /// 栅格空间原点的视图位置（上表：**地形**专用）。
    pub raster_origin: Vec2,
    /// 当前图图帧原点的视图位置（上表：**本图局部内容**专用）。
    pub map_origin: Vec2,
    /// 当前图幅面（`extent` 的宽 × 高，世界单位）：地图外框与 1/4 参考网格用。
    /// 16:9 缺省后不再是正方形——`x` / `y` 分开。
    pub map_size: Vec2,
}

impl GridConfig {
    /// 本图局部坐标 → 视图空间。
    ///
    /// **本图局部内容（地点 / 街道 / 街道名 / 外框）一律走这里**——绕过去手写
    /// `map_origin + local` 就等于把上面那张表丢在一边，而这条路已经错过一次。
    pub fn map_to_view(&self, local: Vec2) -> Vec2 {
        self.map_origin + local
    }
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cell_size: 8.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        }
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

    /// 把当前图设为**唯一起点**：trail 只剩 `[id]`，访问痕迹与待播过渡全清。
    ///
    /// 与 [`Self::enter`] 的分工：`enter` 是「沿树往下走一步」（保留来路面包屑）；
    /// 本方法是「导航上下文整体换新」——世界层级新图创建为**独立根图**后用。
    /// 若沿用 `enter`，新图会带着旧图的面包屑，zoom-out 撞外界就弹回旧图
    /// （2026-10-17 用户报告的「新建世界地图 zoom out 到最大加载前一张地图」即此）。
    ///
    /// 注意 `ensure_root` 不会搅局：它只在「trail 为空或当前图不在世界里」时
    /// 重置，`[id]` 非空且 id 真实存在时每帧调用都是恒等。
    pub fn settle_on(&mut self, id: wv_core::id::Id) {
        self.trail.clear();
        self.trail.push(id);
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
    ///
    /// **None 时清空镜像**（2026-10-14 回归）：旧实现只覆盖 `Some` 分支，
    /// `drill.current() = None`（新建 / 关闭工程后）时镜像保留上一帧的 id，
    /// 笔触按陈旧 id 提交 `PaintTerrainCommand` 走 `TerrainMismatch` 错误分支。
    /// 现在任何调用都把镜像对齐到当前真源——空工程时镜像也为空。
    pub fn sync_view(&self, view: &mut ViewState) {
        view.current_map = self
            .current()
            .map(|i| i.as_str().to_owned())
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::id::Id;
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::time::TimeSpan;
    use wv_core::WorldMeta;

    /// 浏览态默认从 [ADR-0012](file:///<root>/docs/adr/ADR-0012-terrain-default-vector-flat-shading.md) 起切到等值线
    /// 矢量平涂；Google Default Map 降为可切换主题之一。`ALL[0]` 与 `default()`
    /// 都应指 `VectorFlat`，以保证顶栏「默认 viewmode」与「主题列表第一位」
    /// 不漂移。
    #[test]
    fn vector_flat_is_the_default_browsing_view_after_adr_0012() {
        assert_eq!(ViewMode::default(), ViewMode::VectorFlat);
        assert_eq!(ViewMode::ALL[0], ViewMode::VectorFlat);
        // GoogleMap 仍可达（可切换主题之一），仅不再是默认入口。
        assert!(ViewMode::ALL.contains(&ViewMode::GoogleMap));
    }

    /// 2026-10-18（ADR-0027 D6）：浏览态开放第三项「势力图」。
    #[test]
    fn browsing_list_offers_vector_flat_schematic_and_faction() {
        assert_eq!(
            ViewMode::BROWSING,
            [ViewMode::VectorFlat, ViewMode::Schematic, ViewMode::Faction]
        );
        assert!(ViewMode::BROWSING.contains(&ViewMode::default()));
    }

    #[test]
    fn brush_starts_disabled_so_left_drag_pans_the_map() {
        assert!(!BrushState::default().enabled);
    }

    /// 三组（涂 / 塑 / 放）必须恰好划分全体工具（ADR-0043 后 8 个）：面板三行
    /// 与命令面板注册都按组枚举，任何工具漏组 = 面板上点不到它。
    #[test]
    fn tool_groups_partition_all_tools() {
        let mut grouped = BrushTool::PAINT.to_vec();
        grouped.extend(BrushTool::SHAPE);
        grouped.extend(BrushTool::PLACE);
        assert_eq!(
            grouped.len(),
            BrushTool::ALL.len(),
            "组并集大小 {} ≠ 全体 {}——有工具被分了两次或漏了",
            grouped.len(),
            BrushTool::ALL.len()
        );
        for t in BrushTool::ALL {
            assert!(grouped.contains(&t), "{t:?} 不在任何组里");
        }
    }

    /// 组员身份与操作语义一致：仅涂组非意图（分派闸 `is_intent` 的口径）。
    #[test]
    fn group_membership_matches_intent_dispatch_gate() {
        assert_eq!(BrushTool::PaintKind.group(), BrushToolGroup::Paint);
        for t in BrushTool::SHAPE {
            assert_eq!(t.group(), BrushToolGroup::Shape, "{t:?}");
            assert!(t.is_intent(), "{t:?} 应走 IntentTerrainCommand");
        }
        for t in BrushTool::PLACE {
            assert_eq!(t.group(), BrushToolGroup::Place, "{t:?}");
            assert!(t.is_intent(), "{t:?} 应走 IntentTerrainCommand");
        }
        // 涂组两个工具（涂抹 / 橡皮）都走 PaintTerrainCommand，不是意图。
        for t in BrushTool::PAINT {
            assert_eq!(t.group(), BrushToolGroup::Paint, "{t:?}");
            assert!(!t.is_intent(), "{t:?} 应走 PaintTerrainCommand");
        }
    }

    /// 段路径：`on_group_changed` 强制 `tool` 落到该组第一个，与旧 tool 无
    /// 关——避免「塑」段亮着但 `tool=Hills` 这种随机停留。
    #[test]
    fn on_group_changed_resets_tool_to_group_first() {
        let mut b = BrushState {
            tool: BrushTool::Hills,
            tool_group: BrushToolGroup::Shape,
            ..BrushState::default()
        };
        b.on_group_changed();
        assert_eq!(
            b.tool,
            BrushTool::MountainRange,
            "Shape 组首项=MountainRange"
        );

        let mut b = BrushState {
            tool: BrushTool::Lake,
            tool_group: BrushToolGroup::Place,
            ..BrushState::default()
        };
        b.on_group_changed();
        assert_eq!(b.tool, BrushTool::Volcano, "Place 组首项=Volcano");
    }

    /// 瓷砖 / 命令路径：`on_tool_changed` 把 group 跟 tool 走，tool 不动。
    #[test]
    fn on_tool_changed_pulls_group_to_tool() {
        let mut b = BrushState {
            tool: BrushTool::Volcano,
            tool_group: BrushToolGroup::Paint, // 错位
            ..BrushState::default()
        };
        b.on_tool_changed();
        assert_eq!(b.tool_group, BrushToolGroup::Place);
        assert_eq!(b.tool, BrushTool::Volcano, "tool 不应被改");
    }

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
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
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
                template: None,
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

    /// 独立根图落点（2026-10-17 新建世界图修复）：settle_on 后 trail 只含新图，
    /// 旧访问痕迹清空，zoom-out（`up`）必须原地不动——不能弹回旧世界。
    #[test]
    fn settle_on_replaces_trail_and_zoom_out_stays_put() {
        let w = demo_world();
        let zunyi = Id::new_unchecked("city_zunyi");
        let mut d = DrillState::default();
        d.ensure_root(&w);
        d.enter(&w, &zunyi);
        d.up();
        assert!(d.is_visited(&zunyi), "前置：钻出应留下访问痕迹");
        let fresh = Id::new_unchecked("world_second");
        d.settle_on(fresh.clone());
        assert_eq!(d.current(), Some(&fresh), "当前图应只剩新图");
        assert_eq!(d.depth(), 0, "独立根图深度为 0");
        assert!(!d.is_visited(&zunyi), "旧访问痕迹应被清空");
        assert!(!d.up(), "独立根图 zoom-out 不得换图（否则弹回旧世界）");
        assert_eq!(d.current(), Some(&fresh), "zoom-out 后仍在新图");
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

    // -----------------------------------------------------------------------
    // ADR-0017 §2.7：WorldProjection.selection（session 状态，不走 EditCommand）
    // -----------------------------------------------------------------------

    #[test]
    fn default_world_projection_has_no_selection() {
        let p = WorldProjection::default();
        assert!(
            p.selection.is_none(),
            "WorldProjection::default() ⇒ selection = None（默认未选中）"
        );
    }

    #[test]
    fn selection_round_trip_through_resource_replace() {
        use wv_core::{EntityKind, SelectedEntity};
        let mut p = WorldProjection::default();
        let sel = SelectedEntity::new(EntityKind::Place, wv_core::Id::new("city_qingyun").unwrap());
        // 模拟"标签点击"：直接设字段。
        p.selection = Some(sel.clone());
        assert_eq!(p.selection.as_ref(), Some(&sel));
        // 模拟"点击空白"：回到 None。
        p.selection = None;
        assert!(p.selection.is_none());
    }

    #[test]
    fn closing_a_project_resets_selection() {
        // 模拟"切工程 / 关工程"：selection 必须随 WorldProjection 一起被重置，
        // 否则旧工程的 id 会在新工程里被误读为"已选中"。
        use wv_core::{EntityKind, SelectedEntity};
        let mut p = WorldProjection {
            project_dir: Some("/w/old".into()),
            selection: Some(SelectedEntity::new(
                EntityKind::Place,
                wv_core::Id::new("city_x").unwrap(),
            )),
            ..WorldProjection::default()
        };
        // 关闭工程：project_dir 回到 None，selection 应被显式清掉
        // （不能用 ..default() 因为我们要保留其他可能状态——但 selection 必须 None）
        p.project_dir = None;
        p.selection = None; // 显式重置
        assert!(p.selection.is_none(), "关闭工程后 selection 必须为 None");
        assert!(!p.has_open_project());
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

    // ====== SelectionState（2026-10-17 选择工具 spec §3，M1 Task 1）======

    fn closed_ring() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]
    }

    fn closed_selection() -> SelectionState {
        SelectionState {
            closed: Some(closed_ring()),
            ..Default::default()
        }
    }

    /// <3 点收笔：不成环，草稿清空（`ContinentSketch::try_close` 同语义——
    /// 作者不会得到一个点了两次却没反应的谜）。
    #[test]
    fn closed_selection_requires_three_anchors() {
        let mut s = SelectionState::default();
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        assert!(s.is_drawing(), "两次落点应处于画中");
        assert!(!s.try_close(), "两点不应成环");
        assert!(!s.is_drawing(), "失败的收笔也清掉草稿");
        assert!(!s.is_closed(), "两点不得产生闭合选区");

        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.add_point((10.0, 10.0));
        assert!(s.try_close(), "三点应能闭合");
        assert_eq!(
            s.closed.as_ref().map(Vec::len),
            Some(3),
            "got {:?}",
            s.closed
        );
        assert!(!s.is_drawing(), "闭合后草稿应清空");
    }

    /// Esc 只丢草稿：已闭合选区保留（作者辛苦圈的地不会因为手滑 Esc 蒸发）。
    #[test]
    fn escape_discards_draft_keeps_closed_selection() {
        let mut s = closed_selection();
        s.add_point((50.0, 50.0));
        s.add_point((60.0, 50.0));
        s.discard_draft();
        assert!(!s.is_drawing(), "Esc 后草稿应丢弃");
        assert!(s.is_closed(), "已闭合选区不得被 discard_draft 清掉");
        assert_eq!(s.closed, Some(closed_ring()), "闭合环内容不变");
    }

    /// 点空白 / 切工具：草稿与闭合一起清（selected 一并复位）。
    #[test]
    fn cancel_clears_both_draft_and_closed() {
        let mut s = closed_selection();
        s.selected_anchor = Some(1);
        s.add_point((50.0, 50.0));
        s.cancel();
        assert!(!s.is_drawing() && !s.is_closed(), "cancel 应双清");
        assert_eq!(s.selected_anchor, None, "选中锚点应复位");
        assert!(!s.enabled, "cancel 不动 enabled——工具开关归瓷砖");
    }

    /// 删锚致 <3 点：闭合降级回画中（绝不留下悬空的退化选区喂内核）。
    #[test]
    fn deleting_anchor_below_three_dissolves_back_to_draft() {
        let mut s = closed_selection();
        s.delete_anchor(0);
        assert!(!s.is_closed(), "两点后不得仍是闭合态");
        assert!(s.is_drawing(), "剩余锚点应回到画中草稿");
        assert_eq!(s.draft.len(), 2, "got {:?}", s.draft);
    }

    /// 点边插入：新点精确落在 splice 位（i-1 与 i%len 之间）。
    #[test]
    fn insert_anchor_splices_between_neighbors() {
        // 方形：底边 (0,0)-(10,0)、右边 (10,0)-(10,10)、顶边 (10,10)-(0,10)、左边回到 (0,0)。
        let mut s = SelectionState {
            closed: Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]),
            ..Default::default()
        };
        let p = (5.0, 10.0);
        s.insert_anchor(3, p); // 顶边中点：pts[2] 与 pts[3] 之间
        let ring = s.closed.as_ref().expect("插入后仍闭合");
        assert_eq!(ring.len(), 5, "got {ring:?}");
        assert_eq!(ring[3], p, "新点应落在 splice=3 位");
        assert_eq!(ring[2], (10.0, 10.0), "前驱不动");
        assert_eq!(ring[4], (0.0, 10.0), "后继不动");
    }

    /// 生成即消费：取走环并清空闭合态。
    #[test]
    fn consume_closed_takes_ring_and_clears() {
        let mut s = closed_selection();
        s.selected_anchor = Some(0);
        let ring = s.consume_closed();
        assert_eq!(ring, Some(closed_ring()), "取走的应是原环");
        assert!(!s.is_closed(), "消费后不再闭合");
        assert_eq!(s.selected_anchor, None, "选中锚点应一并复位");
        assert_eq!(s.consume_closed(), None, "二次消费应得 None");
    }

    /// 切图清空（`MeasureState::on_map_change` 同款）——测量不能跨图，选区同理。
    /// 同步系统每帧都会以当前图重申（`sync_selection_map_change`），所以真实
    /// 状态里 `last_map_id` 恒等于当前图；测试先钉住该前提再断言变图分支。
    #[test]
    fn map_change_clears_selection() {
        let world = Id::new_unchecked("world_main");
        let city = Id::new_unchecked("city_zunyi");
        let mut s = closed_selection();
        s.last_map_id = Some(world.clone());
        s.on_map_change(Some(&world));
        assert!(s.is_closed(), "同图重申不得清空");
        s.on_map_change(Some(&city));
        assert!(!s.is_closed(), "切图应清空选区");
        assert!(!s.is_drawing(), "切图应清空草稿");
        s.add_point((1.0, 1.0));
        s.on_map_change(None);
        assert!(!s.is_drawing(), "退到无图也应清空");
    }
}
