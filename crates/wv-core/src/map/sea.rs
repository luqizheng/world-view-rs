//! 海名（手绘地图呈现 #13，2026-10-15）。
//!
//! ## 边界
//!
//! - **不参与距离图**——海名是纯视觉标注（`world.seas`），与 `distances.places` 平级但独立。
//! - **不与 Place 合并**——Place 走距离图 + 几何，Sea 仅是水域中的文字；两套模型独立。
//!
//! ## 形状简化（v0.1）
//!
//! - 单中心点（`x`, `y`），无矩形范围——海名是水域上的"字"，不是一个面。
//! - 仅一字段：id + name + map + (x, y) + 可选 `scale_3tier`（文字梯度）。
//! - `text_overrides` 是预留位（M3 留）：与 #2 字重梯度同款（region/place/water），
//!   当前海名走 water 默认值。

use crate::id::Id;
use serde::{Deserialize, Serialize};

/// 海名。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sea {
    /// 海名 ID。
    pub id: Id,
    /// 显示名。
    pub name: String,
    /// 所属地图。
    pub map: Id,
    /// 中心点 X（地图局部坐标）。
    pub x: f32,
    /// 中心点 Y（地图局部坐标）。
    pub y: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sea {
        Sea {
            id: Id::new_unchecked("sea_pacific"),
            name: "沧浪洋".into(),
            map: Id::new_unchecked("world_main"),
            x: 1024.0,
            y: 512.0,
        }
    }

    /// 海名字段读写：name 与坐标独立访问，id 同款。
    #[test]
    fn sea_fields_read_and_write() {
        let s = sample();
        assert_eq!(s.id.as_str(), "sea_pacific");
        assert_eq!(s.name, "沧浪洋");
        assert_eq!(s.map.as_str(), "world_main");
        assert_eq!(s.x, 1024.0);
        assert_eq!(s.y, 512.0);
    }

    /// 海名与 Place 互不干扰：可在同一 World 中并存。
    #[test]
    fn sea_coexists_with_place_in_world() {
        let mut world = crate::World::default();
        let place = crate::map::distance::Place {
            id: Id::new_unchecked("p1"),
            name: "落云城".into(),
            map: None,
            x: Some(0.0),
            y: Some(0.0),
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
        };
        world.distances.places.push(place);
        world.seas.push(sample());
        assert_eq!(world.distances.places.len(), 1);
        assert_eq!(world.seas.len(), 1);
    }

    /// 海名 Clone/PartialEq 派生正常：避免误改字段破坏派生。
    #[test]
    fn sea_clone_eq_compile() {
        let s1 = sample();
        let s2 = s1.clone();
        assert_eq!(s1, s2);
    }
}
