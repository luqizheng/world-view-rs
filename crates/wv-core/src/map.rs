//! 地理空间维度：多层级地图与图层。
//!
//! 原则四（多地图层级）：世界 → 区域 → 城市 → 建筑，逐层引用，不重复存储。
//! 原则五（图层化编辑）：每层可独立开关、锁定、调透明度、调整顺序。

use crate::{color::Color, id::Id, time::TimeSpan};

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
    /// 图标 ID。字符集同 [`crate::id::Id`]（`[A-Za-z0-9_]`）。
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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extent {
    pub width: f32,
    pub height: f32,
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
