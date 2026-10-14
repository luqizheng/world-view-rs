//! 街道矢量图层的文档模型（ADR-0010）。
//!
//! 「缩放到街道」要求城市内部有一份真实的街道路网几何。本模块定义这份
//! 几何的**存储形态**：一个 `LayerKind::Road` 图层经 [`wv_core::map::Layer::source`]
//! 指向一个 YAML 文件，文件内容即本模块的 [`StreetLayer`]。
//!
//! 设计约束（ADR-0010 决策 ⑤⑥）：
//! - **几何不进领域 YAML 字段**：`World` / `MapNode` 不持有街道几何，避免
//!   4 万段的坐标把 `maps/<id>/index.yaml` 撑成不可读的巨物；分层由
//!   P5（图层化）天然承载——街道就是图上的一层。
//! - **YAML 为主格式**（P3）：不开二进制例外；规模阈值与分片策略见
//!   README §12 第 21 条（单文件 2000–5000 段后分片，尚未收敛）。
//! - **只存几何与类别参数，不存渲染结论**：线宽、颜色、LOD 档位都是
//!   渲染层按 [`RoadClass`] 派生的表现层决策，不在数据里落字段。

use crate::base::geometry::Point;

/// 街道类别（ADR-0010 决策 ④ 的 class 集合）。
///
/// 命名对齐 Overture `classes` 的 `ROAD` 主题取值，虚构路网与真实路网
/// 共用同一套词汇（ADR-0010 决策 ②：一个供给接口两个实现）。
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RoadClass {
    Motorway,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    /// 缺省类别（`#[derive(Default)]`）：手绘编辑器最常从住宅街道画起（ADR-0010 ②）。
    #[default]
    Residential,
    Service,
    Path,
    Cycleway,
    Track,
    Unknown,
    Unclassified,
}

impl RoadClass {
    /// 全部类别的静态表（渲染层建材质 / LOD 分档时遍历用）。
    pub const ALL: [RoadClass; 12] = [
        RoadClass::Motorway,
        RoadClass::Trunk,
        RoadClass::Primary,
        RoadClass::Secondary,
        RoadClass::Tertiary,
        RoadClass::Residential,
        RoadClass::Service,
        RoadClass::Path,
        RoadClass::Cycleway,
        RoadClass::Track,
        RoadClass::Unknown,
        RoadClass::Unclassified,
    ];

    /// LOD 分档（ADR-0010 决策 ④）：数字越小越「重要」，远视角先隐藏大数字档。
    ///
    /// 这是**表现层参数**的派生入口之一：分档值只在本方法里定义一次，
    /// 渲染层的 LOD 过滤只消费它，不再各自硬编码类别集合。
    pub fn lod_rank(self) -> u8 {
        match self {
            RoadClass::Motorway => 0,
            RoadClass::Trunk => 1,
            RoadClass::Primary => 2,
            RoadClass::Secondary => 3,
            RoadClass::Tertiary => 4,
            RoadClass::Unclassified | RoadClass::Residential => 5,
            RoadClass::Service | RoadClass::Track => 6,
            RoadClass::Path | RoadClass::Cycleway | RoadClass::Unknown => 7,
        }
    }

    /// 编辑器 UI 的中文显示名（**纯显示**，YAML 仍存 snake_case 英文键）。
    pub fn label(self) -> &'static str {
        match self {
            RoadClass::Motorway => "高速公路",
            RoadClass::Trunk => "干道",
            RoadClass::Primary => "主干道",
            RoadClass::Secondary => "次干道",
            RoadClass::Tertiary => "支路",
            RoadClass::Residential => "住宅街道",
            RoadClass::Service => "服务道路",
            RoadClass::Path => "小径",
            RoadClass::Cycleway => "自行车道",
            RoadClass::Track => "土路",
            RoadClass::Unknown => "未知",
            RoadClass::Unclassified => "未定级",
        }
    }
}

/// 一条街道段：一条折线 + 类别。
///
/// `points` 是**本图层所属地图的局部坐标**（与 `MapNode.extent` 同单位）；
/// 上溯到世界坐标走 `wv_core::coord::resolve_frame_in`，与标记/栅格同一条
/// 坐标链，不另立换算。
///
/// 折线允许多点（曲线街道），渲染层按相邻点连成线段流（ADR-0010 的
/// 「路段流」）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreetSegment {
    /// 类别；缺省 [`RoadClass::default`]（住宅街道）。
    #[serde(default)]
    pub class: RoadClass,
    /// 折线顶点，至少 2 个才构成可见线段。校验不阻止保存（V1）。
    #[serde(default)]
    pub points: Vec<Point>,
    /// 街道名（可选，Phase 5 标签引擎的数据源）。手绘的大多数小巷无名，
    /// `None` 不产生任何标注；命名不是门槛，标注不是校验。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl StreetSegment {
    /// 两点直段的便捷构造。
    pub fn straight(class: RoadClass, x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        StreetSegment {
            class,
            points: vec![Point::new(x1, y1), Point::new(x2, y2)],
            name: None,
        }
    }

    /// 折线中点（标签锚点）：按累计长度取半程处。与起终点距离各半，
    /// 多点折线也不会把标签钉在端点上。
    pub fn polyline_midpoint(&self) -> Option<Point> {
        if self.points.is_empty() {
            return None;
        }
        let mut total = 0.0_f32;
        for w in self.points.windows(2) {
            total += (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
        }
        if total <= 0.0 {
            return self.points.first().copied();
        }
        let mut walked = 0.0_f32;
        for w in self.points.windows(2) {
            let leg = (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
            if walked + leg >= total * 0.5 {
                let t = ((total * 0.5 - walked) / leg).clamp(0.0, 1.0);
                return Some(Point::new(
                    w[0].x + (w[1].x - w[0].x) * t,
                    w[0].y + (w[1].y - w[0].y) * t,
                ));
            }
            walked += leg;
        }
        self.points.last().copied()
    }
}

/// 一份街道图层文件（`Layer.source` 指向的 YAML）的文档根。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreetLayer {
    /// 全部街道段；顺序即绘制顺序（先画的在下）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<StreetSegment>,
}

impl StreetLayer {
    /// 空图层。
    pub fn new() -> Self {
        StreetLayer::default()
    }

    /// 追加一段。
    pub fn push(&mut self, segment: StreetSegment) {
        self.segments.push(segment);
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // YAML round-trip 与 serde 缺省值的测试在 `wv-io/tests/street_layer_yaml.rs`
    // —— wv-core 按 AGENTS §2.1 不依赖 serde_yaml，round-trip 与 IO 同层验证。

    #[test]
    fn lod_rank_orders_motorway_before_service() {
        assert!(
            RoadClass::Motorway.lod_rank() < RoadClass::Secondary.lod_rank(),
            "motorway rank {} 应小于 secondary {}",
            RoadClass::Motorway.lod_rank(),
            RoadClass::Secondary.lod_rank()
        );
        assert!(
            RoadClass::Secondary.lod_rank() < RoadClass::Service.lod_rank(),
            "secondary rank {} 应小于 service {}",
            RoadClass::Secondary.lod_rank(),
            RoadClass::Service.lod_rank()
        );
    }

    #[test]
    fn polyline_midpoint_splits_the_line_in_half() {
        let seg = StreetSegment {
            class: RoadClass::Primary,
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
            name: Some("中山路".into()),
        };
        let m = seg.polyline_midpoint().expect("两点直段必有中点");
        assert!(
            (m.x - 5.0).abs() < 1e-4 && m.y.abs() < 1e-4,
            "中点应为 (5, 0)，实得 ({}, {})",
            m.x,
            m.y
        );
    }

    #[test]
    fn polyline_midpoint_lands_on_the_long_leg_of_a_bend() {
        // L 形：竖边 6 + 横边 4，半程 5 落在竖边上 (0, 5)。
        let seg = StreetSegment {
            class: RoadClass::Path,
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(0.0, 6.0),
                Point::new(4.0, 6.0),
            ],
            name: None,
        };
        let m = seg.polyline_midpoint().expect("折线必有中点");
        assert!(
            m.x.abs() < 1e-4 && (m.y - 5.0).abs() < 1e-4,
            "中点应落在长边 (0, 5)，实得 ({}, {})",
            m.x,
            m.y
        );
    }
}
