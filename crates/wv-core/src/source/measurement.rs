//! # wv-core::source::measurement —— 地图测量产物
//!
//! 测量是**工具产物**（与阵营、事件等「领域实体」对立）：作者为了量算地图
//! 距离 / 面积而落的几何标注。**不带时间区间**——撤销 / 重做走 `wv-edit`
//! 的命令链（AGENTS §3.3），不需要「区间不覆盖」语义（P6 对存储层的要求，
//! 这里由命令层的 `apply` / `revert` 兑现）。
//!
//! ## 边界（不做什么）
//!
//! - **不**做渲染：线宽、颜色、z 序、标注字号都是渲染层决策，不进数据。
//! - **不**做单位换算：「一格 / 一 wu 等于多少公里」是 [`super::geographic_source::LatLonBbox`]
//!   的派生职责（ADR-0007），不是本模块职责。
//!
//! ## 主要导出
//!
//! - [`MeasurementKind`]：折线 / 矩形两种形状（`serde` 平铺为 snake_case 标签）。
//! - [`Measurement`]：一条测量（ID + 类型 + 顶点 + 可选显示色）。
//! - [`MeasurementOverlay`]：某张地图的全部测量（对应 `maps/<id>/measurements.yaml`）。

use serde::{Deserialize, Serialize};

use crate::base::geometry::Point;
use crate::base::id::Id;

/// 测量的形状。
///
/// 序列化走 untagged + 内部判别字段——YAML 写起来像
/// `kind: polyline` / `kind: rectangle`，snake_case；读取时按
/// `kind` 字段分支（外部显式标签比裸 untagged 更稳，磁盘结构少一次调整）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementKind {
    /// 多段折线（≥ 2 个顶点）。
    Polyline,
    /// 矩形（恰好 2 个顶点：对角 a / b；本模块不强校验，由渲染层兜底）。
    Rectangle,
}

/// 一条测量。
///
/// ## 关于 ID 字符集
///
/// 与 `Faction.id` 同源：限 `[A-Za-z0-9_]`（AGENTS §4.4）。测量是**作者手
/// 写的标注**之一，ID 也会进 YAML / Git diff，必须可读。
///
/// ## 关于颜色
///
/// `color` 是「作者定的渲染色」——若缺省，渲染层走主题黄。允许是 hex
/// 字符串（`"#RRGGBB"`）同 `wv_core::Color`，但本模块不依赖 `Color` 类型
/// 以免把渲染表示耦合进数据（与 `IconSpec::color` 同款解耦策略）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub id: Id,
    pub kind: MeasurementKind,
    /// 顶点序列（**本图局部坐标**）。
    ///
    /// - `Polyline`：`len() ≥ 2`；落点少于 2 个视为非法（命令层不强制，渲染层兜底）。
    /// - `Rectangle`：恰 2 个，对角 `points[0]` / `points[1]`。
    pub points: Vec<Point>,
    /// 显示色（hex `"#RRGGBB"`），缺省时由渲染层取主题黄。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl Measurement {
    /// 折线便捷构造。
    pub fn polyline(points: Vec<Point>) -> Self {
        Self {
            id: Id::new_unchecked(""),
            kind: MeasurementKind::Polyline,
            points,
            color: None,
        }
    }

    /// 矩形便捷构造（`a` / `b` 是两对角点）。
    pub fn rectangle(a: Point, b: Point) -> Self {
        Self {
            id: Id::new_unchecked(""),
            kind: MeasurementKind::Rectangle,
            points: vec![a, b],
            color: None,
        }
    }

    /// 设 ID（链式），返回 `self`。
    pub fn with_id(mut self, id: Id) -> Self {
        self.id = id;
        self
    }

    /// 设显示色（链式），返回 `self`。
    pub fn with_color(mut self, color: String) -> Self {
        self.color = Some(color);
        self
    }

    /// 总长度（**世界单位**）。
    ///
    /// - `Polyline`：相邻点两两距离求和。
    /// - `Rectangle`：周长 = 2 × (|dx| + |dy|)。
    ///
    /// 不在数据层换算成公里——那是渲染层的展示决策（带
    /// `geographic_source` 的图才补 km，参考
    /// [`super::geographic_source::LatLonBbox::km_per_wu`]）。
    pub fn total_length(&self) -> f32 {
        match self.kind {
            MeasurementKind::Polyline => self
                .points
                .windows(2)
                .map(|w| {
                    let (a, b) = (w[0], w[1]);
                    let dx = b.x - a.x;
                    let dy = b.y - a.y;
                    (dx * dx + dy * dy).sqrt()
                })
                .sum(),
            MeasurementKind::Rectangle => {
                if self.points.len() != 2 {
                    return 0.0;
                }
                let dx = (self.points[1].x - self.points[0].x).abs();
                let dy = (self.points[1].y - self.points[0].y).abs();
                2.0 * (dx + dy)
            }
        }
    }

    /// 面积（**世界单位²**）。仅 `Rectangle` 有意义，折线返 0。
    pub fn area(&self) -> f32 {
        match self.kind {
            MeasurementKind::Rectangle => {
                if self.points.len() != 2 {
                    return 0.0;
                }
                let dx = (self.points[1].x - self.points[0].x).abs();
                let dy = (self.points[1].y - self.points[0].y).abs();
                dx * dy
            }
            MeasurementKind::Polyline => 0.0,
        }
    }
}

/// 一张地图上的全部测量（对应 `maps/<map_id>/measurements.yaml`）。
///
/// 与 [`crate::source::street::StreetLayer`] 同源结构：单一文档、单文件落盘、
/// 加载期一次性读入内存。**不**走「每条测量一个文件」——地图层级的辅助产物
/// 共用一份 YAML 更便于人翻动。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeasurementOverlay {
    pub measurements: Vec<Measurement>,
}

impl MeasurementOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.measurements.is_empty()
    }

    pub fn len(&self) -> usize {
        self.measurements.len()
    }

    /// 按 ID 查找。
    pub fn find(&self, id: &Id) -> Option<&Measurement> {
        self.measurements.iter().find(|m| &m.id == id)
    }

    /// 按 ID 查找（可变）。
    pub fn find_mut(&mut self, id: &Id) -> Option<&mut Measurement> {
        self.measurements.iter_mut().find(|m| &m.id == id)
    }

    /// 追加；已存在同 ID 视为 Precondition 错误——返回 `Err(id)`。
    pub fn add(&mut self, m: Measurement) -> Result<(), Id> {
        if self.measurements.iter().any(|x| x.id == m.id) {
            return Err(m.id);
        }
        self.measurements.push(m);
        Ok(())
    }

    /// 按 ID 删除，返回被删的那条。
    pub fn remove(&mut self, id: &Id) -> Option<Measurement> {
        let pos = self.measurements.iter().position(|m| &m.id == id)?;
        Some(self.measurements.remove(pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn polyline_total_length_sums_segment_distances() {
        // L 形 3 点折线：(0,0)→(3,0)→(3,4)→(0,4) = 3 + 4 + 3 = 10
        let m = Measurement::polyline(vec![p(0.0, 0.0), p(3.0, 0.0), p(3.0, 4.0), p(0.0, 4.0)]);
        assert!(
            (m.total_length() - 10.0).abs() < 1e-4,
            "got {}",
            m.total_length()
        );
        assert_eq!(m.area(), 0.0, "折线面积恒为 0");
    }

    #[test]
    fn rectangle_total_length_is_perimeter_and_area_is_lw() {
        // 3×4 矩形：周长 14，面积 12
        let m = Measurement::rectangle(p(0.0, 0.0), p(3.0, 4.0));
        assert!(
            (m.total_length() - 14.0).abs() < 1e-4,
            "got {}",
            m.total_length()
        );
        assert!((m.area() - 12.0).abs() < 1e-4, "got {}", m.area());
    }

    #[test]
    fn add_rejects_duplicate_id() {
        let mut ov = MeasurementOverlay::new();
        let id = Id::new_unchecked("m1");
        ov.add(Measurement::rectangle(p(0.0, 0.0), p(1.0, 1.0)).with_id(id.clone()))
            .expect("首次添加应成功");
        let err = ov.add(Measurement::polyline(vec![p(0.0, 0.0), p(2.0, 0.0)]).with_id(id.clone()));
        assert!(err.is_err(), "同 ID 二次添加应报错");
        assert_eq!(ov.len(), 1, "失败后长度不应增长");
    }

    #[test]
    fn remove_returns_the_measurement_and_shrinks_len() {
        let mut ov = MeasurementOverlay::new();
        let id1 = Id::new_unchecked("m1");
        let id2 = Id::new_unchecked("m2");
        ov.add(Measurement::rectangle(p(0.0, 0.0), p(1.0, 1.0)).with_id(id1.clone()))
            .unwrap();
        ov.add(Measurement::rectangle(p(2.0, 2.0), p(3.0, 3.0)).with_id(id2.clone()))
            .unwrap();
        let removed = ov.remove(&id1);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, id1);
        assert_eq!(ov.len(), 1);
        assert!(ov.find(&id1).is_none(), "id1 应已移除");
        assert!(ov.find(&id2).is_some(), "id2 应仍在");
    }

    #[test]
    fn remove_missing_id_is_none() {
        let mut ov = MeasurementOverlay::new();
        assert!(ov.remove(&Id::new_unchecked("ghost")).is_none());
    }

    #[test]
    fn empty_overlay_is_empty_and_zero_len() {
        let ov = MeasurementOverlay::new();
        assert!(ov.is_empty());
        assert_eq!(ov.len(), 0);
    }
}
