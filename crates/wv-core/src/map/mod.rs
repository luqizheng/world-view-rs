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
    source::geographic_source::GeographicSourceV1, time::TimeSpan,
};

/// 地图层级。由粗到细。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapLevel {
    World,
    Region,
    City,
    Interior,
}

/// 地图节点。对应 `maps/<id>/index.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
    EventPoint,
    Place,
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
}

/// 几何数据载体。具体编码是待裁问题，这里先给出最小可用表达。
///
/// **`#[serde(untagged)]` 选取**：YAML 写起来最自然——
/// `geometry: [x0, y0, x1, y1, ...]`（裸数组）直接被识别为 `Polygon`。
/// 外部标签（默认）会要求 `geometry: !polygon [...]` 或 `geometry: { polygon: [...] }`，
/// 不可读且对作者不友好。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum GeometryData {
    /// 点列坐标，扁平存储 `[x0, y0, x1, y1, ...]`。
    Points(Vec<f32>),
    /// 多边形环，扁平存储，首尾不重复。
    Polygon(Vec<f32>),
    /// 引用外部几何文件。
    Ref(String),
}

impl GeometryData {
    /// 顶点数：Points/Polygon 是 2D 平面 → `len() / 2`；Ref 引用外部 → 0。
    pub fn point_count(&self) -> usize {
        match self {
            GeometryData::Points(v) | GeometryData::Polygon(v) => v.len() / 2,
            GeometryData::Ref(_) => 0,
        }
    }
}

pub mod coord;
pub mod distance;
pub mod unit;
