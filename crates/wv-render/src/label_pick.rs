//! 标签命中注册与拾取（ADR-0017 §2.7 交互闭环）。
//!
//! 三个标签系统（place / faction / event）每帧把**实际可见**的标签的
//! 世界坐标包围盒 + 实体身份写进 [`LabelHits`]；`drill_pick` 在左键按下的
//! 时刻查表：命中 → 设 `WorldProjection.selection` 并吃掉这次点击
//! （不参与双击钻取计数）；空白单击 → 清除选中。
//!
//! ## 时序口径（一帧陈旧是有意的）
//!
//! `drill_pick` 在标签系统**之前**跑（同链 chain），查到的是上一帧注册的
//! 包围盒。这是有意的：标签只随相机移动，而相机平移由「按住拖动」驱动，
//! 按下那一刻相机还没动——上一帧包围盒与本帧实际位置一致。
//!
//! ## 街名不参与拾取
//!
//! `StreetSegment` 没有稳定 id（ADR-0010 数据模型），无法构造
//! `SelectedEntity`，故街名标签不注册命中——点它等于点空白（清除选中）。

use bevy::ecs::prelude::*;
use bevy::math::Vec2;
use wv_core::SelectedEntity;

/// 一个可点击标签的世界坐标包围盒与实体身份。
#[derive(Debug, Clone)]
pub struct LabelHit {
    /// 标签文字中心（视图/世界坐标，与 `Text2d` transform 同源）。
    pub center: Vec2,
    /// 包围盒半宽半高（世界单位，由字号 × 文本长度估算）。
    pub half: Vec2,
    /// 命中后要选中的实体。
    pub target: SelectedEntity,
}

impl LabelHit {
    /// 光标点是否落在包围盒内。
    pub fn contains(&self, p: Vec2) -> bool {
        (p.x - self.center.x).abs() <= self.half.x && (p.y - self.center.y).abs() <= self.half.y
    }
}

/// 本帧可点击标签注册表（每帧被标签系统重建）。
#[derive(Resource, Default)]
pub struct LabelHits {
    pub items: Vec<LabelHit>,
}

impl LabelHits {
    /// 清空并重建（地名系统每帧第一个跑，负责清场）。
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// 追加一个命中项。
    pub fn push(&mut self, hit: LabelHit) {
        self.items.push(hit);
    }
}

/// 纯函数命中测试：返回**最上层**（注册序最后，即绘制序最上）的命中实体。
///
/// 标签按绘制顺序注册，后面的画在上面——命中检测同样从后往前找第一个，
/// 与「点最上面的字」的直觉一致。
pub fn hit_test(cursor: Vec2, items: &[LabelHit]) -> Option<SelectedEntity> {
    items
        .iter()
        .rev()
        .find(|hit| hit.contains(cursor))
        .map(|hit| hit.target.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::{EntityKind, Id};

    fn hit(x: f32, y: f32, half: f32, name: &str) -> LabelHit {
        LabelHit {
            center: Vec2::new(x, y),
            half: Vec2::splat(half),
            target: SelectedEntity::new(EntityKind::Place, Id::new_unchecked(name)),
        }
    }

    #[test]
    fn hit_test_returns_entity_when_cursor_inside_box() {
        let items = vec![hit(100.0, 100.0, 20.0, "a")];
        let got = hit_test(Vec2::new(110.0, 105.0), &items);
        assert!(got.is_some(), "包围盒内应命中");
        assert_eq!(got.unwrap().id.as_str(), "a");
    }

    #[test]
    fn hit_test_misses_outside_the_box() {
        let items = vec![hit(100.0, 100.0, 20.0, "a")];
        assert!(
            hit_test(Vec2::new(140.0, 100.0), &items).is_none(),
            "盒外 x 不命中"
        );
        assert!(
            hit_test(Vec2::new(100.0, 140.0), &items).is_none(),
            "盒外 y 不命中"
        );
    }

    #[test]
    fn hit_test_returns_the_topmost_label_on_overlap() {
        // 两个盒重叠：后注册（画在上层）者胜。
        let items = vec![
            hit(100.0, 100.0, 20.0, "bottom"),
            hit(110.0, 100.0, 20.0, "top"),
        ];
        let got = hit_test(Vec2::new(105.0, 100.0), &items).expect("重叠区应命中");
        assert_eq!(got.id.as_str(), "top", "应取注册序最后的（最上层）");
    }

    #[test]
    fn hit_test_on_empty_registry_is_none() {
        assert!(hit_test(Vec2::ZERO, &[]).is_none());
    }
}
