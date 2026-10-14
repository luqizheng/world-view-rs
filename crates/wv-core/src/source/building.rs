//! 建筑矢量图层的文档模型（ADR-0014 §6 落地项）。
//!
//! 「2.5D 建筑挤出」要求城市内部有一份建筑轮廓 + 高度数据。存储形态与
//! [`crate::street`]（ADR-0010）完全同构：一个 `LayerKind::Building` 图层经
//! [`wv_core::map::Layer::source`] 指向一个 YAML 文件，文件内容即本模块的
//! [`BuildingLayer`]。
//!
//! 设计约束（与 ADR-0010 ⑤⑥ 同口径）：
//! - **几何不进领域 YAML 字段**：`World` / `MapNode` 不持有建筑几何，
//!   建筑按层组织（P5），`Layer.source` 指文件；
//! - **YAML 为主格式**（P3）：矢量轮廓是文本数据，不开二进制例外；
//! - **只存参数不存渲染结论**：挤出方向、夸张系数、阴影样式都是渲染层决策
//!   （README §12-31 待裁），数据里只有轮廓与物理高度（米）。

use crate::base::geometry::Point;

/// 一栋建筑：闭合轮廓 + 物理高度。
///
/// `ring` 是**本图层所属地图的局部坐标**（与 [`crate::street::StreetSegment`]
/// 的 `points` 同一口径），首尾点**不重复**（闭合由渲染侧连接首尾）。
/// 轮廓允许凹多边形（L 形回字等），渲染侧耳切三角化；带洞多边形 v0.1
/// 不支持（与势力几何同限，README §12-8）。
///
/// `height_m` 是**物理高度（米）**——世界单位换算（夸张系数）是渲染层
/// 待决参数（README §12-31(b)），数据不预乘。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Building {
    /// 建筑名（可选；v0.1 不产生标注，字段先立——命名不是门槛）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 物理高度（米）；缺省 9.0（约三层，城市肌理的常见量级）。
    #[serde(default = "default_height_m")]
    pub height_m: f32,
    /// 闭合轮廓顶点（首尾不重复）。少于 3 个不构成面——校验不阻止保存（V1）。
    #[serde(default)]
    pub ring: Vec<Point>,
}

fn default_height_m() -> f32 {
    9.0
}

impl Building {
    /// 矩形建筑的便捷构造（原点 + 尺寸，逆时针）。
    pub fn rect(x: f32, y: f32, w: f32, h: f32, height_m: f32) -> Self {
        Building {
            name: None,
            height_m,
            ring: vec![
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ],
        }
    }

    /// 轮廓质心（顶点算术平均）：渲染层的旋转枢轴 / 标注锚点。
    ///
    /// 顶点分布不均时不是几何学重心，但作为枢轴足够（工程口径：够用、稳定、
    /// 无依赖）；空轮廓回 `None`。
    pub fn centroid(&self) -> Option<Point> {
        if self.ring.is_empty() {
            return None;
        }
        let n = self.ring.len() as f32;
        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
        for p in &self.ring {
            sx += p.x;
            sy += p.y;
        }
        Some(Point::new(sx / n, sy / n))
    }
}

/// 一份建筑图层文件（`Layer.source` 指向的 YAML）的文档根。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BuildingLayer {
    /// 全部建筑；顺序即绘制顺序（先画的在下）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buildings: Vec<Building>,
}

impl BuildingLayer {
    /// 空图层。
    pub fn new() -> Self {
        BuildingLayer::default()
    }

    /// 追加一栋。
    pub fn push(&mut self, building: Building) {
        self.buildings.push(building);
    }

    /// 栋数。
    pub fn len(&self) -> usize {
        self.buildings.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.buildings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 矩形质心 = 对角中点（手算锚点）。
    #[test]
    fn rect_centroid_is_the_diagonal_midpoint() {
        let b = Building::rect(0.0, 0.0, 10.0, 4.0, 12.0);
        let c = b.centroid().expect("矩形必有质心");
        assert!(
            (c.x - 5.0).abs() < 1e-5 && (c.y - 2.0).abs() < 1e-5,
            "质心应为 (5, 2)，实得 ({}, {})",
            c.x,
            c.y
        );
    }

    /// 空轮廓质心回 None（V1：不 panic、不阻止保存）。
    #[test]
    fn empty_ring_centroid_is_none() {
        let b = Building {
            name: None,
            height_m: 9.0,
            ring: vec![],
        };
        assert!(b.centroid().is_none());
    }

    /// L 形凹轮廓：顶点算术平均仍有限且落在轮廓包围盒内（凹多边形可用的证据）。
    #[test]
    fn concave_l_shape_has_centroid_inside_bbox() {
        let b = Building {
            name: None,
            height_m: 9.0,
            ring: vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 4.0),
                Point::new(4.0, 4.0),
                Point::new(4.0, 10.0),
                Point::new(0.0, 10.0),
            ],
        };
        let c = b.centroid().expect("非空轮廓必有质心");
        assert!(
            (0.0..=10.0).contains(&c.x) && (0.0..=10.0).contains(&c.y),
            "质心应落在包围盒内，实得 ({}, {})",
            c.x,
            c.y
        );
    }
}
