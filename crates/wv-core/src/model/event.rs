//! 事件维度。每实体一文件（§5.4），对应 `timeline/events/<id>.yaml`。
//!
//! 事件是**状态变化的驱动源**：事件产生状态变化，
//! 状态变化反过来约束事件的合理性（§3.4）。
//!
//! ## 事件的三个描述层（2026-10-11 用户裁定）
//!
//! 用户原话：「**event 更像是一个描述过程，如他们相见发生什么，结果是什么**」。
//!
//! | 层 | 字段 | 写什么 |
//! |---|---|---|
//! | **过程** | `narrative` | 怎么发生的：谁说了什么、关键转折在哪 |
//! | **结果** | `outcome` | 留下了什么：谈成了什么、代价是什么 |
//! | **后果** | `effects` | 声明式的状态变化（可校验、可求值）|
//!
//! `narrative` / `outcome` 都**可选且可留白**——留白是创作手段，
//! 不是疏漏（与 ADR-0004「相遇却未写事件只提示」同源）。
//!
//! ## 事件的位置是「范围」，不是「点」（2026-01 修订）
//!
//! `Event.place: Option<Id>` 已改为 `Event.location: Option<EventLocation>`。
//! 理由：作者的操作用户动作是「在地图上放一个事件标记，填信息、选时间、选参与单位」，
//! 而事件常常**有范围**（围城、街市血案），不是一个数学点。
//!
//! `EventLocation` 引用一个 `Place`，并可选地带自己的 `x/y/w/h`：
//! - 只写 `place` → 事件范围 = 该 `Place` 的范围（如「成都血案」覆盖成都城区）；
//! - 连 `x/y/w/h` 一起写 → 范围是 `Place` 内的**子矩形**（如「督署街血案」只在成都一条街）。
//!
//! 这个几何信息是「单位进入了事件范围却没列为参与者」这类校验的依据。

use crate::{base::id::Id, map::distance::Place, time::TimeSpan};

/// 事件发生地：地图上的矩形范围（或点）。
///
/// 形状 `EventLocation { place, x, y, w, h }`：
/// - `place` 引用 `Place.id`（`Place` 自身带 `(map, x, y, w, h)`）；
/// - `x/y/w/h` 可选，用于把范围**收窄到 `Place` 内的一个子矩形**。
///
/// **为什么不直接用 `Id` 指 `Place`**：因为「事件范围」与「地点范围」不必相等。
/// 「成都血案」覆盖整个成都城区，「督署街血案」只在成都的一个街区；
/// 只存 `Id` 无法表达「地点范围的子区域」。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventLocation {
    /// 引用 `Place.id`（`Place` 自身带矩形/点范围）。
    pub place: Id,
    /// 中心点 X（地图局部坐标）。`None` 表示沿用 `Place` 的 `x`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// 中心点 Y（地图局部坐标）。`None` 表示沿用 `Place` 的 `y`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// 范围宽度（须与 `h` 成对出现）。`None` 表示沿用 `Place` 的 `w`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f32>,
    /// 范围高度（须与 `w` 成对出现）。`None` 表示沿用 `Place` 的 `h`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
}

impl EventLocation {
    /// 解析出事件的实际范围：中心点 `(x, y)` + 宽高 `(w, h)`。
    ///
    /// `x/y/w/h` 各自「本字段优先，缺失则回退到 `Place`」。
    /// 返回 `None` 表示连中心点都取不到（`Place` 没锚定到地图）。
    ///
    /// `w`/`h` 只有一个给得出 → 返回 `Some` 但其中一项为 `None`，
    /// 由校验层报「必须成对」；几何判定按点处理（宽进严出）。
    pub fn resolve(&self, place: &Place) -> Option<(f32, f32, Option<f32>, Option<f32>)> {
        let px = self.x.or(place.x)?;
        let py = self.y.or(place.y)?;
        Some((px, py, self.w.or(place.w), self.h.or(place.h)))
    }

    /// 点是否落在事件范围内（含边界）。
    ///
    /// - `w`/`h` 都可得 → 矩形判定（含边界）；
    /// - 否则 → 点判定（`EPS = 1e-3`）。
    pub fn contains(&self, place: &Place, px: f32, py: f32) -> bool {
        let Some((cx, cy, w, h)) = self.resolve(place) else {
            return false;
        };
        match (w, h) {
            (Some(w), Some(h)) => (px - cx).abs() <= w / 2.0 && (py - cy).abs() <= h / 2.0,
            _ => {
                const EPS: f32 = 1e-3;
                (px - cx).abs() <= EPS && (py - cy).abs() <= EPS
            }
        }
    }
}

/// 历史事件。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub id: Id,
    pub name: String,
    /// 发生时间。用 `TimeSpan` 而非 `TimePoint`：
    /// 需要支持「持续一个月」「持续三年」这类事件，而非只有瞬时点。
    ///
    /// 半开语义 `[from, to)`：事件在其 `span` 内「正在发生」。
    #[serde(flatten)]
    pub span: TimeSpan,
    /// 发生地点（地图上的**范围**锚点，2026-01 修订）。
    ///
    /// `None` 表示事件不锚定具体地点（如全国性的政令）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<EventLocation>,
    /// 参与人物。
    #[serde(default)]
    pub participants: Vec<Id>,
    /// **过程描述**：这件事是怎么发生的（2026-10-11 用户裁定）。
    ///
    /// 与 `outcome` 的分工：`narrative` 写**过程**（谁说了什么、怎么谈的、
    /// 关键转折在哪），`outcome` 写**结果**（谈成了什么、留下了什么）。
    /// 两者都可选，都可为空——留白是创作手段（ADR-0004 相遇提示同源）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative: Option<String>,
    /// 事件结果描述。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// 本事件结束后触发的状态变化。
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// 本事件引用的旁白（**只存 ID 引用**，实体在 `timeline/narrations/`）。
    ///
    /// 为什么是引用不是内联文案（P4 同款纪律）：同一段旁白可被多个节点复用，
    /// 且旁白可携带图片资产，资产路径跟着实体走。
    /// 悬空引用由 `wv-io` 的 `diagnose` 检出（只提示不阻止，§7.5 V1）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrations: Vec<Id>,
}

/// 事件引发的状态变化。
///
/// 设计取舍：这里刻意保持**声明式**而非脚本式。
/// 脚本式（内嵌表达式求值）更灵活，但会让 YAML 不可静态校验、
/// 也会让「事件合理性反向约束」无从实现。声明式先覆盖高频场景。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    /// 移动角色到某地点。
    MoveCharacter { character: Id, to: Id },
    /// 改变角色身份。
    SetTitle { character: Id, title: String },
    /// 势力在某时间点起控制某区域。`at` 是天数计数器。
    ClaimRegion { region: Id, at: i64 },
    /// 自定义效果，保留扩展口，但需在 GLOSSARY 登记语义。
    Custom { note: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(
        id_s: &str,
        map: Option<&str>,
        x: Option<f32>,
        y: Option<f32>,
        w: Option<f32>,
        h: Option<f32>,
    ) -> Place {
        Place {
            id: Id::new_unchecked(id_s),
            name: id_s.into(),
            map: map.map(Id::new_unchecked),
            x,
            y,
            w,
            h,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
        }
    }

    fn loc(
        place_id: &str,
        x: Option<f32>,
        y: Option<f32>,
        w: Option<f32>,
        h: Option<f32>,
    ) -> EventLocation {
        EventLocation {
            place: Id::new_unchecked(place_id),
            x,
            y,
            w,
            h,
        }
    }

    /// 成都城区：中心 (100, 200)，40×20 → x ∈ [80, 120]，y ∈ [190, 210]。
    fn chengdu() -> Place {
        place(
            "city_chengdu",
            Some("map_world"),
            Some(100.0),
            Some(200.0),
            Some(40.0),
            Some(20.0),
        )
    }

    #[test]
    fn event_location_without_own_extent_inherits_place_geometry() {
        let p = chengdu();
        let l = loc("city_chengdu", None, None, None, None);
        assert_eq!(
            l.resolve(&p),
            Some((100.0, 200.0, Some(40.0), Some(20.0))),
            "every missing dimension must fall back to the place"
        );
        assert!(
            l.contains(&p, 118.0, 208.0),
            "inside inherited rect must hit"
        );
        assert!(
            !l.contains(&p, 130.0, 200.0),
            "outside inherited rect must miss"
        );
    }

    #[test]
    fn event_location_own_extent_narrows_within_place() {
        // Place 是整城 40×20；事件只覆盖城内一个 10×10 的子街区。
        let p = chengdu();
        let l = loc(
            "city_chengdu",
            Some(100.0),
            Some(200.0),
            Some(10.0),
            Some(10.0),
        );
        assert!(
            l.contains(&p, 104.0, 200.0),
            "point inside the sub-rect must hit"
        );
        assert!(
            !l.contains(&p, 118.0, 200.0),
            "point inside the place but outside the event sub-rect must miss \
             (that is the whole point of the sub-rect)"
        );
    }

    #[test]
    fn event_location_own_center_with_inherited_extent() {
        let p = chengdu();
        let l = loc("city_chengdu", Some(130.0), Some(200.0), None, None);
        assert_eq!(
            l.resolve(&p),
            Some((130.0, 200.0, Some(40.0), Some(20.0))),
            "x/y come from the event, w/h from the place"
        );
        assert!(l.contains(&p, 148.0, 200.0), "148 lies in [110, 150]");
        assert!(!l.contains(&p, 108.0, 200.0), "108 lies outside [110, 150]");
    }

    #[test]
    fn event_location_without_any_anchor_resolves_to_none() {
        let p = place("place_nomap", None, None, None, None, None);
        let l = loc("place_nomap", None, None, None, None);
        assert_eq!(
            l.resolve(&p),
            None,
            "neither the event nor the place is anchored, so nothing can be judged"
        );
        assert!(
            !l.contains(&p, 0.0, 0.0),
            "contains must be false when nothing can be judged"
        );
    }

    #[test]
    fn event_location_boundary_inherits_inclusive_rect_rule() {
        // 含边界：子矩形四角必须命中。
        let p = chengdu();
        let l = loc(
            "city_chengdu",
            Some(100.0),
            Some(200.0),
            Some(10.0),
            Some(10.0),
        );
        for (px, py) in [(95.0, 195.0), (105.0, 195.0), (95.0, 205.0), (105.0, 205.0)] {
            assert!(
                l.contains(&p, px, py),
                "sub-rect boundary point ({px}, {py}) must be accepted (inclusive)"
            );
        }
    }

    #[test]
    fn event_without_location_serializes_without_the_field() {
        let e = Event {
            id: Id::new_unchecked("evt_railway_nationalization"),
            name: "railway nationalization".into(),
            span: TimeSpan::new(1, 2).unwrap(),
            location: None,
            participants: vec![],
            narrative: None,
            outcome: None,
            effects: vec![],
            narrations: vec![],
        };
        assert_eq!(
            e.location, None,
            "location stays optional so nationwide decrees need no anchor"
        );
    }
}
