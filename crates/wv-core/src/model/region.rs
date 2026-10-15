//! 命名区域实体（ADR-0017 §2.2 · `EntityKind::Region`）。
//!
//! 区域是作者划定的**命名多边形**——「楚河汉界」「关中平原」「武昌城垣范围」
//! 这类有名字的面状范围。它是**领域实体**（有 id、有名字、可被选中、
//! 可被引用），不是纯渲染叠加（那是 `source::measurement` 的事）。
//!
//! ## 存储形态（§4.5 每实体一文件）
//!
//! 每个 `Region` 一个 `regions/<id>.yaml`；几何内联为本图局部坐标多边形
//! （区域数量少、顶点不多，YAML 可读可 diff——P3 不需要二进制例外）。
//!
//! ## 为什么不用 `GeometryData`
//!
//! 区域语义上永远是多边形（不像 `GeometryData` 还有点列和引用变体），
//! 直接存 `Vec<f32>` 更简单、类型更精确。`GeometryData` 的变体顺序问题
//! 已修正（`Polygon` 在前），但 Region 仍保持内联——归并无收益、徒增复杂度。
//!
//! ## 时间维度（v0.1 不带）
//!
//! ADR-0017 §2.3 明确：Region v0.1 **永久可见**（只受图级
//! `MapNode.time_span` 否决）。未来加 `state_spans` 时，`label_visible_at`
//! 的调用方把 `entity_span` 从 `None` 换成实体窗口即可，渲染侧零改动。

use crate::base::{geometry::Point, id::Id};

/// 命名区域：一个有名字的多边形范围，锚定在某张地图上。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub id: Id,
    /// 显示名（标签渲染的文本来源——标签是实体的投影，不存第二份）。
    pub name: String,
    /// 所属地图（`polygon` 用该图的局部坐标）。
    pub map: Id,
    /// 区域轮廓：扁平顶点 `[x0, y0, x1, y1, ...]`，首尾不重复。
    /// 空轮廓 = 未画完（校验可提示，不阻止保存——V1）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polygon: Vec<f32>,
}

impl Region {
    /// 多边形质心（简单平均）——标签锚点（ADR-0017 §2.5「几何中心」）。
    ///
    /// 顶点分布不均时不是几何学重心，但作为枢轴足够（工程口径：够用、
    /// 稳定、无依赖）；空轮廓回 `None`。
    pub fn centroid(&self) -> Option<Point> {
        if self.polygon.len() < 2 {
            return None;
        }
        let n = (self.polygon.len() / 2) as f32;
        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
        for pair in self.polygon.chunks(2) {
            sx += pair[0];
            sy += pair[1];
        }
        Some(Point::new(sx / n, sy / n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(polygon: Vec<f32>) -> Region {
        Region {
            id: Id::new_unchecked("region_guanzhong"),
            name: "关中平原".into(),
            map: Id::new_unchecked("map_world"),
            polygon,
        }
    }

    #[test]
    fn centroid_is_the_vertex_average_of_the_polygon() {
        // 方形四顶点 (0,0)(4,0)(4,4)(0,4) → 质心 (2,2)。
        let r = region(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0]);
        let c = r.centroid().expect("四边形有质心");
        assert!(
            (c.x - 2.0).abs() < 1e-4 && (c.y - 2.0).abs() < 1e-4,
            "质心应为 (2, 2)，实得 ({}, {})",
            c.x,
            c.y
        );
    }

    #[test]
    fn centroid_is_none_for_empty_polygon() {
        assert!(region(vec![]).centroid().is_none(), "空轮廓无锚点");
        assert!(region(vec![3.0]).centroid().is_none(), "半对顶点无锚点");
    }
}
