//! # wv-core::map —— 地图层级、坐标系与单位
//!
//! **AGENTS.md §3（P4）引用的关键路径**：wv_core::map::MapNode / MapLevel 在此处。
//!
//! ## 职责
//! - [MapNode] / [MapLevel] / [Layer] / [LayerKind]（地图层级树）
//! - [GeometryData] / [GeometrySpan] / [RasterRef]（几何 + 时间区间）
//! - [coord]：坐标系与帧解析（resolve_frame / resolve_frame_in / MapFrame）
//! - [distance]：行程时间计算（DistanceGraph，**唯一速度换算入口**）
//! - [unit]：在地图上独立运动的主体（ADR-0004）
//!
//! ## 边界（不做什么）
//! - **不**含「地图上的事」。事态是 [crate::derived::situation]，事件是 [crate::model::event]。
//!
//! ## 主要导出
//! - [MapNode] / [MapLevel] / [Layer]
//! - [coord::resolve_frame] / [coord::MapFrame]
//! - [distance::DistanceGraph::segment_days]（行程时间唯一计算入口）

use crate::{
    base::color::Color, base::geometry::Point, base::id::Id,
    source::geographic_source::GeographicSourceV1, terrain::TerrainCellData, time::TimeSpan,
};

/// 地图层级。由粗到细。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapLevel {
    #[default]
    World,
    Region,
    City,
    Interior,
}

/// 地图节点。对应 `maps/<id>/index.yaml`。
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MapNode {
    pub id: Id,
    pub name: String,
    pub level: MapLevel,
    /// 父地图 ID。`World` 层级为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    /// 子地图 ID 列表。**只存引用，不复制子图内容**（原则四）。
    #[serde(default)]
    pub children: Vec<Id>,
    /// 本图上显示的标记（地图图标）。
    ///
    /// **声明顺序即绘制顺序**（先声明的在下层）——同层无 `z` 字段，
    /// 因为图标数量少、且叠放问题用调整声明顺序即可解决。
    ///
    /// 图标是**声明**，不是渲染结果：形状、尺寸、颜色都在这里定，
    /// 由渲染层（`wv-app`）解释。见 README §12 第 12 条。
    #[serde(default)]
    pub icons: Vec<IconSpec>,
    /// 本图的图层清单。
    #[serde(default)]
    pub layers: Vec<Layer>,
    /// 本图的绘制范围，用于坐标变换。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<Extent>,
    /// 地形/高度网格的二进制引用（原则三的唯一例外）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raster: Option<RasterRef>,
    /// 真实地理底图引用（ADR-0007）。
    ///
    /// `None` 即虚构地图，`Some(...)` 即声明本图引用 Overture Maps 真实资源。
    /// 真假地图由 `Option` 的 `None`/`Some` 派生——**不**另设 `geographic_kind` 字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geographic_source: Option<GeographicSourceV1>,

    /// 子图所代表的时代窗口（ADR-0015 · 2026-10-14 接受）。
    ///
    /// `None` = 不限定时代，子图在任意 `t` 都存在（向后兼容，零迁移）。
    /// `Some(span)` = 子图仅在 `span.contains(t)` 时存在；窗口外的时段
    /// 钻取被 UI 驳回、底图给"空"标记（**只提示不阻止**，V1）。
    ///
    /// **与 `StreetSegment.time_span` 的关系**：图级窗口**否决**段级——
    /// 即便段设了 `time_span`，图级窗口外的段也不可见。求值由
    /// `wv_core::label_visible_at` 单一实现点完成。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_span: Option<TimeSpan>,

    /// 「1 km = 多少 wu」（2026-10-14 用户需求：虚构地图自报尺度）。
    ///
    /// 与 §4.2「数据层不存换算结论」不冲突——`Extent` / `PlaceEdge` /
    /// `Track.waypoint.pos` 都没动，本字段是**地图级声明**，仅供测量显示使用，
    /// 不进入坐标链、不影响任何派生计算。
    ///
    /// - `None` = 作者未声明，渲染层只显示 wu；真图另有 `geographic_source` 路径。
    /// - `Some(8.0)` = 「1 km = 8 wu」（例：把 8 wu/格 的栅格地图当成 1 km/格）。
    ///
    /// **不**参与坐标换算（写进 `MapNode` 只为「让作者能描述自己的世界」），
    /// **不**进任何派生 / 验证 / 路径搜索逻辑——纯展示用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_per_km: Option<f32>,

    /// 地图朝向（屏幕上的旋转角，单位：度）。
    ///
    /// **0.0 = 正北朝上**（地图顶边即北方）；正值 = 顺时针偏转（如 90.0 = 北方朝右）。
    /// 未声明时由 serde 默认值补 0.0，既有 YAML 不受影响。
    ///
    /// ## 为什么在这里而不是在渲染层
    ///
    /// 地图朝向是**地图的属性**（作者决定这张图"北"朝哪），不是相机状态
    /// （AGENTS §3：YAML 是 source of truth，渲染层只是投影）。
    /// 指南针面板读取此字段以告诉用户"地图的正确方向"；若渲染层需要
    /// 旋转地图，也只读这个值——**禁止**在渲染层另设一个"朝向"字段。
    ///
    /// ## 与 `Extent.origin` 的关系
    ///
    /// `origin` 描述**子图在父图中的位置**（平移），`orientation` 描述
    /// **本图在屏幕上的旋转**（旋转）。两者独立，互不替代。
    ///
    /// ## 追记（2026-10-14）
    ///
    /// `wv-app` 渲染层已消费此值：`camera::apply_map_orientation` 每帧把它写进
    /// 相机旋转，视图随之转动；文字标注取相机旋转的逆保持屏幕水平（用户裁定
    /// 「屏幕对齐文字」）。本段为追加说明，上文语义不变。
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub orientation: f32,

    /// 底图（2026-10-16 用户需求）：每图一个「全部海 / 全部陆」的初始态。
    ///
    /// - `None` = 旧地图 / 没声明。**重置按钮按 [`MapBase::Ocean`] 兜底**，
    ///   不锁住旧世界的「重置」入口（向后兼容，零迁移）。
    /// - `Some(Ocean | Land)` = 新建时声明的底图，重置按钮一键回到此态。
    ///
    /// ## 底图 ≠ 当前地形
    ///
    /// 创建后所有笔刷、街道、意图工具都会改 `World.terrains[map_id]`；
    /// **`base` 字段本身不被修改**——它只是「重置按钮」的回退锚点。
    /// 与 P6（非破坏编辑）正交：作者在地形上的每一笔都按 `EditCommand` 走
    /// `History`，底图字段不参与。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<MapBase>,

    /// 城市子图在世界图上的轮廓（ADR-0022 · 2026-10-16 接受）。
    ///
    /// 多边形顶点 = **本图 LOCAL 坐标**（与街道同口径），父图渲染时
    /// 经 `coord::resolve_frame_in`（ADR-0009 §6 派生）一次性平移到
    /// 父图坐标系——**复用单一换算入口**。
    ///
    /// - `None` = 没有显式轮廓（旧城市、点状标记）。运行时退化为
    ///   `IconSpec` 渲染，**不**自动派生伪数据（守 P3 / §6.2 待决）。
    /// - `Some(polygon)` = 父图上可见的轮廓区域；首写即定
    ///   （`AddCityCommand`），后续子图内部编辑**不**回写父图轮廓——
    ///   父图轮廓是声明，非派生。
    ///
    /// 与 `MapNode.extent` 正交：`extent` 描述子图**幅面**，`outline_polygon`
    /// 描述子图**在父图上呈现的形状**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_polygon: Option<GeometryData>,
}

/// 每张地图的「底图」语义（2026-10-16）。
///
/// **不**替代 `TerrainData`——它是「新建地图时铺满整图的初始单元格」与
/// 「重置按钮回填的目标态」这两件事的**口径源**，仅此而已。`Apply` /
/// `Intent` / `Paint` 三类笔刷都不读这个字段；它们写 `World.terrains`，
/// `base` 字段始终是它**被声明那一刻**的值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapBase {
    /// 全海：高度压在海平面下、类型深水——适合先画大陆的作者。
    Ocean,
    /// 全陆：高度抬在海平面上，类型草原——适合先画内海的作者。
    Land,
}

impl MapBase {
    /// 该底图的单格「`(height, kind)`」。
    ///
    /// 数值与 `wv-app::NewMapPrefill::cell()` 历史口径同源：
    /// - 海洋取 `-3500` m：`water_shade_at(-3500)` 走 t = 1.0 深海冷蓝端
    ///   （2026-10-15 引入的水域渐变），即便 `terrain_soften = true`（默认）
    ///   与纸面色 55% 混合后仍是清晰蓝 ≈ (186, 197, 233)；旧 `-120` 只走到
    ///   t ≈ 0.034 的浅水端、过 soften 后退成中性灰 ≈ (201, 205, 202)，
    ///   肉眼辨不出「海」（2026-10-16 用户反馈）。
    /// - 陆地取 `+80` m：稳居海平面之上、留出向下挖河的余量。
    ///
    /// **整图同值**（每个 cell 高度一致），因此 `VectorFlat` 边界格的双线性
    /// 高度采样仍是常数、无夸张等值线带——可在 `-3500` 与 `-120` 之间安全切。
    pub fn cell(self) -> TerrainCellData {
        match self {
            MapBase::Ocean => TerrainCellData {
                height: -3500,
                surface: 0,
                // 咸水（群系 14；2026-10-27 B 案：水域群系表盐度，海洋底图
                // 整格带咸水——与陆地底图带草原(3)对称）。wv-core 零依赖
                // 不引 wv-terrain，编号按冻结表字面量。
                biome: 14,
            },
            MapBase::Land => TerrainCellData {
                height: 80,
                surface: 1,
                biome: 3,
            },
        }
    }

    /// 中文 UI 标签（2026-10-16 新建对话框预填充段用）。
    ///
    /// 故意放在 `wv-core`：i18n key 在 `wv-app` 里登记，但「海洋 / 陆地」
    /// 是中文里这俩枚举的标准译名，与 CLI 报错 / 文档 / YAML 注释的口径
    /// 完全一致——把它钉在核心层省得到处复制。
    pub const fn zh_label(self) -> &'static str {
        match self {
            MapBase::Ocean => "海洋",
            MapBase::Land => "陆地",
        }
    }
}

/// Serde `skip_serializing_if` 谓词：f32 零值不写出，保持既有 YAML 干净。
fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}

/// 地图图标的形状。
///
/// 数据层只存**形状的名字**，怎么画由渲染层决定——这是「数据与渲染分离」
/// 在图标上的同一约束（`AGENTS.md` §3）。新增形状是**枚举扩展**，
/// 不需要动任何已有 YAML（未声明时落到 [`IconShape::Diamond`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconShape {
    /// 菱形。v0.2 §B.2 给钻入标记定的形状，也作为所有图标的缺省值。
    #[default]
    Diamond,
    /// 圆形。
    Circle,
    /// 十字。常用于标记地点 / 事件点。
    Cross,
    /// 方形。
    Square,
    /// 四角星。
    Star,
}

/// 地图上显示的一个标记（图标）的声明。
///
/// ## 为什么锚点在这里，而不是在 `MapNode` 上
///
/// 早期做法是想给 `MapNode` 加一个「子图在父图上的位置」字段，
/// 但那样**一个地图节点只能有一个落点**，而作者显然需要在同一张地图上
/// 摆很多标记（城市、港口、隘口、事件点、钻入入口……）。
///
/// 把这些标记抽成独立的 `icons` 列表后，「子图落点」只是其中一种用法
/// （带 `drill_into` 的图标），落点字段随之自然属于**每个图标自己**——
/// 这也让 `children` 保持 `Vec<Id>` 不动，原则四「只存引用」完全不受影响。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IconSpec {
    /// 图标 ID。字符集同 [`crate::base::id::Id`]（`[A-Za-z0-9_]`）。
    pub id: Id,

    /// 在**本图 `extent`** 内的归一化位置，`[0,1]²`，左上角为原点、y 向下。
    ///
    /// 用归一化坐标而非世界单位，是为了父图 `extent` 缩放时锚点不失效——
    /// 作者写一次 `0.35`，改 `extent` 不需要跟着改坐标。
    ///
    /// 缺省时渲染层用「ID 确定性哈希」摆位（同一次运行里稳定、可复现），
    /// 代价是与真实地理布局无关。越界值（`<0` 或 `>1`）由渲染层夹回 `[0,1]`，
    /// 数据层不拒收（`AGENTS.md` §7.5V1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<[f32; 2]>,

    /// 形状。未声明时用 [`IconShape::Diamond`]。
    #[serde(default)]
    pub shape: IconShape,

    /// 标记的屏幕尺寸（像素）。
    ///
    /// 口径同 v0.2 §B.2 的 14 / 12——那是「整颗标记的宽度」，
    /// 渲染层按半边长绘制。缺省时渲染层按目标层级选缺省值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,

    /// 颜色，十六进制字符串（`"#RRGGBB"`）。
    ///
    /// **显式声明的颜色优先于主题色**——作者定了色就按作者的来，
    /// 不再因为「已访问」而转灰。缺省时渲染层用主题界面蓝 / 已访问灰。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,

    /// 双击钻入的目标子图。
    ///
    /// `Some(id)` = 这是一个**钻入入口**（可双击进入该子图）；
    /// `None` = 纯注释标记，只画不可点。悬空引用由 `wv-io` 的
    /// [`crate::project::Project::diagnose`] 检出为 Error 级诊断，
    /// 但**不阻止**加载（`AGENTS.md` §7.5V1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drill_into: Option<Id>,
}

/// 图层。原则五的操作单元。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    pub kind: LayerKind,
    /// 是否可见。
    #[serde(default = "default_true")]
    pub visible: bool,
    /// 是否锁定（锁定后不可编辑，但仍可见）。
    #[serde(default)]
    pub locked: bool,
    /// 透明度 0.0..=1.0。
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// 在同级图层中的绘制顺序，数值越大越靠上。
    #[serde(default)]
    pub z: i32,
    /// 本图层数据的来源文件（相对工程根目录）。
    ///
    /// TODO(阶段三·街道图层): 本字段目前是**声明了但没人读**的空壳——
    /// 全仓没有任何生产代码读取 `source`，示例 `maps/world/index.yaml` 里写的
    /// `kind: road, source: roads.yaml` 也没有对应的 `roads.yaml` 文件。
    /// 「缩放到街道」要求把这条链路补齐（手绘路网落 YAML / 真实路网走
    /// Overture 引用，两者共用同一段流接口），方案见 ADR-0010 与 README §12 第 21 条。
    /// 在补齐之前，渲染层画的「道路」来自 `distance.edges`（城市间连线），
    /// **不是**街道路网。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_opacity() -> f32 {
    1.0
}

/// 图层类别。
///
/// TODO(阶段三·街道图层): [`LayerKind::Road`] 同为**未被消费**的枚举值——
/// 生产代码里没有任何分支处理它（只有 `wv-io` 的测试构造过 `Border`）。
/// 「缩放到街道」需要它真正对应一份街道路网几何，见 ADR-0010。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    Terrain,
    Border,
    Influence,
    Road,
    /// 建筑轮廓层（ADR-0016 §6 落地项）：`Layer.source` 指向
    /// [`crate::building::BuildingLayer`] YAML，供 2.5D 建筑挤出渲染。
    Building,
    EventPoint,
    Place,
    /// 标签层（ADR-0017 · 2026-10-14 接受）。
    ///
    /// 标签是**实体在地图上的文本投影**——不是作者手摆的文本，也不是图层
    /// 文件里存的字符串。`Layer.source` 指向的 YAML 只存**配置**（白名单 /
    /// 字号阶梯 / 最大标签数 / 优先级），文本内容由 `Place` / `Region` /
    /// `Faction` / `StreetSegment` / `Event` 等实体经 `label_visible_at`
    /// 派生而来。详见 `docs/adr/ADR-0017-label-layer.md`。
    Label,
    Custom,
}

/// 地图绘制范围（世界坐标）。
///
/// **本结构只描述**几何范围（宽高）。轨迹走的是地图局部坐标（`Waypoint.pos`），
/// 「局部坐标 ↔ 公里」的换算**不在数据层**——这是 2026-10-12 用户裁定（甲）。
///
/// 决定：相遇判定改为「两 Track 的 Waypoint 在 `(place, day)` 上同时覆盖」，
/// 完全不依赖坐标距离与换算。`Extent` 因此不再携带 `width_km`，相关字段全部撤除。
///
/// ## 追加（2026-10-13）：补上 `origin`，层级坐标嵌套由此可还原
///
/// 上文的裁定（撤除 `width_km`）**不变**：地面尺度依然不是本结构的职责。
/// 但「多地图层级」（原则四）在**坐标**上一直缺一环——子图只知道自己的宽高，
/// 不知道自己落在父图的哪个位置，于是「子图的局部坐标」无法上溯为世界坐标。
///
/// `origin` 补的就是这一环：**本图左上角在父图局部坐标系中的位置**。
/// 它是 `Option`，`None` 表示**作者没有声明**（既有 YAML 无需改动）：
///
/// - 世界层没有父图，落点恒为 `(0, 0)`；
/// - 子图未声明 `origin` 时，由**父图上指向它的钻入图标**派生
///   （图标标记的是子图的中心，故落点 = 图标点 − 本图 extent/2）——
///   作者在世界图上摆的那个城市图标，本来就是「这座城市在哪」的**唯一**书面表达，
///   不该要求他把同一个位置再写一遍。显式声明时以声明为准（多入口场景）。
///   派生规则与全部优先级见 [`crate::map::coord::resolve_frame_in`]。
///
/// ```text
/// map_world  extent { width: 4096, height: 4096 }              origin 缺省
///   └ map_city extent { width: 1024, height: 1024, origin:(512, 256) }
///       └ 该城某点局部 (100, 40) → 世界坐标 (512+100, 256+40) = (612, 296)
/// ```
///
/// 变换算法**只有一个实现点**：`crate::coord`（见 `docs/MAP_EDITOR_BASELINE_v1.md`
/// §2.15「禁止多 System 各写一遍」）。本结构只存参数，不做计算。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extent {
    pub width: f32,
    pub height: f32,
    /// 本图左上角在**父图局部坐标系**中的位置；`None` = 未声明（见上文派生规则）。
    ///
    /// 未声明时序列化会跳过本字段（`skip_serializing_if`），
    /// 不会给既有地图 YAML 平添噪声。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
}

impl Extent {
    /// 零范围。用于「图未声明 `extent`」的情形——此时坐标链仍可还原**位置**
    /// （`origin` 由父图图标派生或为 0），但归一化坐标（UV）退化。见 `crate::coord`。
    pub const ZERO: Extent = Extent {
        width: 0.0,
        height: 0.0,
        origin: None,
    };

    /// 声明宽高、**不**声明落点：位置交给父图钻入图标派生（见 [`crate::coord`]）。
    pub const fn sized(width: f32, height: f32) -> Extent {
        Extent {
            width,
            height,
            origin: None,
        }
    }

    /// 显式声明落点：本图左上角落在父图局部坐标 `origin` 处。
    ///
    /// 显式声明**优先于**从钻入图标派生——同一张子图有多个入口
    /// （父图上有多个 `drill_into` 指向它）时，只有作者能决定以哪个为准。
    pub const fn at(width: f32, height: f32, origin: Point) -> Extent {
        Extent {
            width,
            height,
            origin: Some(origin),
        }
    }

    /// 落点，未声明时为 `(0, 0)`。**不含**任何派生逻辑——
    /// 需要派生请走 [`crate::map::coord::resolve_frame_in`]。
    pub fn origin_or_global(&self) -> Point {
        self.origin.unwrap_or(Point::ORIGIN)
    }
}

impl MapNode {
    /// 「1 wu = 多少 km」的最长派生链（2026-10-14 测量显示用）。
    ///
    /// 优先级：
    /// 1. **真实底图**：`geographic_source.bbox` 经纬度按 cos 修正（ADR-0007）。
    /// 2. **虚构地图**：`unit_per_km` 显式声明（作者自报）。
    /// 3. **未声明**：返回 `None`——调用方只显示 wu，不假装有 km。
    ///
    /// 返回 `(km_per_wu_x, km_per_wu_y)`。对**长度**取均值（x≈y 的世界地图下
    /// 等价于真实尺度；对高纬度图存在轻微偏差，是可接受的近似）。
    ///
    /// **不**进任何坐标链 / 距离换算（与 `Extent` / `PlaceEdge` / `Track` 的
    /// 单位无关），仅供「测量标签」的展示使用。
    ///
    /// **ADR-0070 追记（2026-10-26，§6.3 追写不改）**：上句结论已被 ADR-0070
    /// D5 收窄——线 POI 推导边的 `distance_km` 换算（线长 ÷ `unit_per_km`）
    /// 与渲染宽度换算亦经本入口；正文结论的修订记录见
    /// `docs/adr/ADR-0070-rivers-roads-as-line-poi.md` 追记一。
    pub fn km_per_wu(&self) -> Option<(f64, f64)> {
        if let Some(src) = &self.geographic_source {
            // 真图：用 bbox + extent 折算（已实现，见 geographic_source 模块）。
            if let Some(ext) = &self.extent {
                return src.bbox.km_per_wu(ext);
            }
        }
        if let Some(per_km) = self.unit_per_km {
            if per_km <= 0.0 {
                return None;
            }
            // 作者自报：x 与 y 用同一系数。`isotropic = 1.0 / per_km`。
            let k = 1.0 / f64::from(per_km);
            return Some((k, k));
        }
        None
    }
}

/// 二进制分块引用。
///
/// 原则三的**唯一例外**：超大地图的网格数组允许以二进制作为一等源格式。
/// 但必须满足三个条件：
/// 1. 遵循《二进制格式规格》；
/// 2. 在索引 YAML 中显式声明（即本结构）；
/// 3. 元数据使用二进制引用（即 `path` + 校验信息），而非内联数据。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RasterRef {
    /// 相对工程根目录的路径，如 `terrain.bin`。
    pub path: String,
    /// 编码格式标识。
    pub encoding: RasterEncoding,
    /// 网格宽高。
    pub width: u32,
    pub height: u32,
    /// 分块尺寸（每块边长像素）。便于按需加载。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk: Option<u32>,
    /// 格式版本，用于迁移判定。
    pub format_version: u32,
    /// 内容校验和，用于检测文件被外部改动。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// 栅格编码方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RasterEncoding {
    /// 每格单字节枚举（地形类型编号）。
    U8,
    /// 每格 16 位高度。
    I16,
    /// 每格 32 位浮点高度。
    F32,
}

/// 带时间区间的几何片段。原则六/七的统一表达。
///
/// 「疆界随时间变化」与「非破坏编辑」通过同一机制表达：
/// 历史不被覆盖，只被追加。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeometrySpan {
    pub map: Id,
    /// 本帧几何所属的图层或实体。
    pub owner: Id,
    /// 有效区间。
    #[serde(flatten)]
    pub span: TimeSpan,
    /// 几何数据，WKT 风格或点列，格式待定（见 §13 待决问题）。
    pub geometry: GeometryData,
    /// 势力领土的语义关系（ADR-0027 D5）：`None` = 实控（control）；
    /// `"claim"` = 声称；其余自定义字符串。仅 `owner` 为势力时有意义。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
}

/// 几何数据载体。具体编码是待裁问题，这里先给出最小可用表达。
///
/// **`#[serde(untagged)]` 选取**：YAML 写起来最自然——
/// `geometry: [x0, y0, x1, y1, ...]`（裸数组）直接被识别为 `Polygon`。
/// 外部标签（默认）会要求 `geometry: !polygon [...]` 或 `geometry: { polygon: [...] }`，
/// 不可读且对作者不友好。
///
/// **变体顺序约束**：`Polygon` 必须排在 `Points` 之前。`untagged` 按声明顺序逐个
/// 尝试反序列化，而 `Polygon(Vec<f32>)` 和 `Points(Vec<f32>)` 在 serde 眼里是
/// 相同类型（浮点序列）——谁在前谁就能匹配裸 YAML 数组。所有磁盘上的几何数据
/// 语义上都是多边形，因此 `Polygon` 在前保证 YAML round-trip 正确。
/// `Points` 仅在 Rust 代码中构造使用，不会出现在 YAML 里。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum GeometryData {
    /// 多边形环，扁平存储，首尾不重复。
    Polygon(Vec<f32>),
    /// 带洞多边形（ADR-0028 D3）：外环 + 若干洞环（国中国的宿主余部）。
    /// untagged 序居 `Polygon` 之后——它的 YAML 形态是映射而非裸数组，
    /// 不会与 `Polygon` 抢匹配；旧 YAML 零迁移。
    PolygonWithHoles {
        /// 外环，扁平坐标，首尾不重复。
        outer: Vec<f32>,
        /// 洞环列表（每个都是扁平坐标，首尾不重复）。
        holes: Vec<Vec<f32>>,
    },
    /// 点列坐标，扁平存储 `[x0, y0, x1, y1, ...]`。
    /// 仅限 Rust 代码构造——YAML 裸数组会被 `Polygon` 优先匹配。
    Points(Vec<f32>),
    /// 引用外部几何文件。
    Ref(String),
}

impl GeometryData {
    /// 顶点数：Points/Polygon 是 2D 平面 → `len() / 2`；带洞环 = 外环 + 全部洞；
    /// Ref 引用外部 → 0。
    pub fn point_count(&self) -> usize {
        match self {
            GeometryData::Points(v) | GeometryData::Polygon(v) => v.len() / 2,
            GeometryData::PolygonWithHoles { outer, holes } => {
                outer.len() / 2 + holes.iter().map(|h| h.len() / 2).sum::<usize>()
            }
            GeometryData::Ref(_) => 0,
        }
    }
}

pub mod coord;
pub mod distance;
pub mod road_derive;
pub mod scale_hint;
pub mod sea;
pub mod unit;

#[cfg(test)]
mod geometry_data_tests {
    use super::*;

    #[test]
    fn polygon_point_count_is_half_length() {
        let g = GeometryData::Polygon(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0]);
        assert_eq!(g.point_count(), 3, "三对坐标 = 3 个顶点");
    }

    #[test]
    fn points_point_count_is_half_length() {
        let g = GeometryData::Points(vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(g.point_count(), 2, "两对坐标 = 2 个顶点");
    }

    #[test]
    fn ref_point_count_is_zero() {
        let g = GeometryData::Ref("geometry/ext.bin".into());
        assert_eq!(g.point_count(), 0, "Ref 引用不计顶点");
    }

    #[test]
    fn polygon_with_holes_round_trips_through_yaml() {
        let g = GeometryData::PolygonWithHoles {
            outer: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            holes: vec![vec![2.0, 2.0, 4.0, 2.0, 4.0, 4.0, 2.0, 4.0]],
        };
        let yaml = serde_yaml::to_string(&g).expect("序列化");
        let back: GeometryData = serde_yaml::from_str(&yaml).expect("反序列化");
        assert_eq!(g, back, "带洞环 YAML round-trip 必须无损");
        // 顶点数 = outer 4 + hole 4。
        assert_eq!(g.point_count(), 8, "got {}", g.point_count());
    }

    #[test]
    fn legacy_polygon_yaml_still_loads_and_serializes_as_before() {
        let yaml = "map: world_main
owner: faction_a
from: 100
geometry:
  - 0.0
  - 0.0
  - 10.0
  - 0.0
  - 10.0
  - 10.0
";
        let span: GeometrySpan = serde_yaml::from_str(yaml).expect("旧形态必须照常加载");
        assert!(
            matches!(span.geometry, GeometryData::Polygon(_)),
            "裸数组必须仍命中 Polygon 变体（untagged 顺序红线），got {:?}",
            span.geometry
        );
    }
}

#[cfg(test)]
mod km_per_wu_tests {
    use super::*;
    use crate::source::geographic_source::{GeographicSourceV1, LatLonBbox};
    use crate::Id;

    fn stub_node() -> MapNode {
        MapNode {
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            ..MapNode::default()
        }
    }

    #[test]
    fn km_per_wu_returns_none_when_neither_declared() {
        let node = stub_node();
        assert!(node.km_per_wu().is_none());
    }

    #[test]
    fn km_per_wu_uses_unit_per_km_for_fictional_maps() {
        // 「1 km = 8 wu」→ km/wu = 1/8 = 0.125。
        let node = MapNode {
            unit_per_km: Some(8.0),
            ..stub_node()
        };
        let (kx, ky) = node.km_per_wu().expect("应有 km/wu");
        assert!((kx - 0.125).abs() < 1e-9);
        assert!((ky - 0.125).abs() < 1e-9);
    }

    #[test]
    fn km_per_wu_rejects_non_positive_unit_per_km() {
        let node = MapNode {
            unit_per_km: Some(0.0),
            ..stub_node()
        };
        assert!(node.km_per_wu().is_none(), "0.0 应被拒绝");
        let node = MapNode {
            unit_per_km: Some(-1.0),
            ..stub_node()
        };
        assert!(node.km_per_wu().is_none(), "负数应被拒绝");
    }

    #[test]
    fn km_per_wu_prefers_geographic_source_over_unit_per_km() {
        // 同时声明两者 → 真实底图优先。
        let node = MapNode {
            unit_per_km: Some(8.0),
            extent: Some(Extent::sized(1024.0, 1024.0)),
            geographic_source: Some(GeographicSourceV1 {
                theme: "places".into(),
                bbox: LatLonBbox {
                    west: -0.25,
                    south: -0.25,
                    east: 0.25,
                    north: 0.25,
                },
                ..GeographicSourceV1::default()
            }),
            ..stub_node()
        };
        let (kx, _) = node.km_per_wu().expect("应走 geographic_source");
        // 赤道 ±0.25° ≈ 55.566 km / 1024 wu ≈ 0.054 km/wu（远小于 1/8 = 0.125）
        assert!(kx < 0.125, "真图系数应小于虚构地图自报: kx={kx}");
    }

    #[test]
    fn km_per_wu_falls_back_to_unit_per_km_when_geographic_source_has_no_extent() {
        // 有 geographic_source 但 extent 为空 → 不能折算，退回 unit_per_km。
        let node = MapNode {
            unit_per_km: Some(8.0),
            extent: None,
            geographic_source: Some(GeographicSourceV1::default()),
            ..stub_node()
        };
        let (kx, _) = node.km_per_wu().expect("应退回 unit_per_km");
        assert!((kx - 0.125).abs() < 1e-9);
    }
}
