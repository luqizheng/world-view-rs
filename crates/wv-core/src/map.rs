//! 地理空间维度：多层级地图与图层。
//!
//! 原则四（多地图层级）：世界 → 区域 → 城市 → 建筑，逐层引用，不重复存储。
//! 原则五（图层化编辑）：每层可独立开关、锁定、调透明度、调整顺序。

use crate::{id::Id, time::TimeSpan};

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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryData {
    /// 点列坐标，扁平存储 `[x0, y0, x1, y1, ...]`。
    Points(Vec<f32>),
    /// 多边形环，扁平存储，首尾不重复。
    Polygon(Vec<f32>),
    /// 引用外部几何文件。
    Ref(String),
}
