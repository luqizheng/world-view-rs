//! 事态 / 轨迹的 YAML 加载与引用完整性校验（S7，基线 §2.7 / §2.9 / §2.21）。
//!
//! ## 目录布局（基线 §2.9）
//!
//! ```text
//! MyWorld/
//! ├── timeline/
//! │   ├── axis.yaml
//! │   ├── events/<id>.yaml
//! │   └── situations/<id>.yaml      ← 事态（与 events/ 平行，不进 events/）
//! └── tracks/<id>.yaml              ← 轨迹
//! ```
//!
//! ## 引用完整性（`AGENTS.md` §7.2 / 基线 §2.21）
//!
//! **悬空引用必须被检出并报错，不得静默忽略。** 本模块负责：
//! 1. `SituationStep.event` 引用的 `EventId` 必须存在；
//! 2. `Situation.narrative_order` 里的每个 `EventId` 必须存在（且必须属于本事的 steps）；
//! 3. `SituationStep.tracks` 引用的 `TrackId` 必须存在；
//! 4. `Track.unit` 引用的 `UnitId` 必须存在（ADR-0004：运动主体是**单位**，
//!    不再是含糊的「角色或势力」）；
//! 5. `Situation.units` 引用的 `UnitId` 必须存在（事态是归纳容器）；
//! 6. `Track.map` / `Waypoint.place` 引用的实体必须存在。
//!
//! 报错必须是 `CoreError::DanglingReference { referrer, missing }`，
//! 且信息里指出 YAML 字段路径（`AGENTS.md` §7.4 精神）。
//!
//! > ⚠️ **待裁（本轮未动）**：本函数返回 `Err` 会**阻断加载**；而 2026-10-11 的裁定
//! > 「校验只提示、不阻止」（`AGENTS.md` §7.5）针对的是**编辑 / 保存**路径。
//! > 「加载期硬失败」与「编辑期软提示」两种强度是否应统一，见 ADR-0004 §待裁。

use std::path::Path;

use wv_core::situation::Situation;
use wv_core::track::Track;
use wv_core::{CoreError, Id};

use crate::error::{IoError, IoResult};

/// 从目录加载所有 `<dir>/*.yaml`，反序列化为 `T`。
///
/// **跳过 `index.yaml`**：目录里的 `index.yaml` 是索引/清单文件
/// （内容形如 `characters: [{id, file}]`），不是单个实体数据，
/// 直接当 `T` 反序列化会因缺字段报错。
pub fn load_dir<T: serde::de::DeserializeOwned>(root: &Path, sub: &str) -> IoResult<Vec<T>> {
    let dir = root.join(sub);
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut entries: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "yaml").unwrap_or(false))
        .filter(|p| p.file_name().map(|n| n != "index.yaml").unwrap_or(true))
        .collect();
    // 排序保证加载顺序确定（diff 友好、测试可复现）。
    entries.sort();
    for path in entries {
        let text = std::fs::read_to_string(&path)?;
        let item: T = serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
            path: path.display().to_string(),
            source,
        })?;
        out.push(item);
    }
    Ok(out)
}

/// 校验所需的「已知 ID 集合」快照。
///
/// 用**命名结构体**而非位置参数：这些集合类型完全相同（都是 `&[Id]`），
/// 位置参数一旦超过两三个，调用方传错顺序**编译器不会报错**——
/// 把 `characters` 当 `maps` 传进来会静默通过。命名字段消除这个风险。
#[derive(Debug, Clone, Copy)]
pub struct KnownIds<'a> {
    /// 已加载的事件 ID。
    pub events: &'a [Id],
    /// 已加载的地图 ID。
    pub maps: &'a [Id],
    /// 已加载的角色 ID。
    pub characters: &'a [Id],
    /// 已加载的地点 ID。
    pub places: &'a [Id],
    /// 已加载的单位 ID（ADR-0004）。
    pub units: &'a [Id],
}

/// 校验事态与轨迹的引用完整性（基线 §2.21）。
///
/// 传入已加载的 world 各部分，返回错误或空。
pub fn validate_situations_and_tracks(
    situations: &[Situation],
    tracks: &[Track],
    known: KnownIds<'_>,
) -> Result<(), CoreError> {
    let has = |ids: &[Id], id: &Id| ids.iter().any(|x| x == id);

    let track_ids: Vec<Id> = tracks.iter().map(|t| t.id.clone()).collect();

    for track in tracks {
        // 运动主体必须是已知单位（ADR-0004）。旧口径允许「角色或势力」并保持宽松，
        // 现已收敛为 Unit —— 悬空即报错（§7.2：不得静默忽略）。
        if !has(known.units, &track.unit) {
            return Err(CoreError::DanglingReference {
                referrer: format!("track.{} (field `unit`)", track.id),
                missing: track.unit.to_string(),
            });
        }
        if !has(known.maps, &track.map) {
            return Err(CoreError::DanglingReference {
                referrer: format!("track.{} (field `map`)", track.id),
                missing: track.map.to_string(),
            });
        }
        for (i, wp) in track.waypoints.iter().enumerate() {
            if let Some(place) = &wp.place {
                if !has(known.places, place) {
                    return Err(CoreError::DanglingReference {
                        referrer: format!("track.{}.waypoints[{i}].place", track.id),
                        missing: place.to_string(),
                    });
                }
            }
        }
    }

    for sit in situations {
        for (i, step) in sit.steps.iter().enumerate() {
            if !has(known.events, &step.event) {
                return Err(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `steps[{i}].event`)", sit.id),
                    missing: step.event.to_string(),
                });
            }
            for (j, track_id) in step.tracks.iter().enumerate() {
                if !has(&track_ids, track_id) {
                    return Err(CoreError::DanglingReference {
                        referrer: format!("situation.{} (field `steps[{i}].tracks[{j}]`)", sit.id),
                        missing: track_id.to_string(),
                    });
                }
            }
        }

        // narrative_order 里的每个 ID 必须存在，且必须属于本事的 steps（基线 §2.21）。
        if let Some(order) = &sit.narrative_order {
            for (i, id) in order.iter().enumerate() {
                if !has(known.events, id) {
                    return Err(CoreError::DanglingReference {
                        referrer: format!("situation.{} (field `narrative_order[{i}]`)", sit.id),
                        missing: id.to_string(),
                    });
                }
                if !sit.steps.iter().any(|s| &s.event == id) {
                    return Err(CoreError::DanglingReference {
                        referrer: format!(
                            "situation.{} (field `narrative_order[{i}]`)：该事件不属于本事的 steps",
                            sit.id
                        ),
                        missing: id.to_string(),
                    });
                }
            }
        }

        for (i, map_id) in sit.maps.iter().enumerate() {
            if !has(known.maps, map_id) {
                return Err(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `maps[{i}]`)", sit.id),
                    missing: map_id.to_string(),
                });
            }
        }

        // 归纳容器持有的单位必须存在（ADR-0004）。
        for (i, unit_id) in sit.units.iter().enumerate() {
            if !has(known.units, unit_id) {
                return Err(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `units[{i}]`)", sit.id),
                    missing: unit_id.to_string(),
                });
            }
        }

        if let Some(protagonist) = &sit.protagonist {
            if !has(known.characters, protagonist) {
                return Err(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `protagonist`)", sit.id),
                    missing: protagonist.to_string(),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::time::TimeSpan;

    fn id(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 构造校验所需的 ID 集合；本模块测试只用到事件与单位两类。
    fn known<'a>(events: &'a [Id], units: &'a [Id]) -> KnownIds<'a> {
        KnownIds {
            events,
            maps: &[],
            characters: &[],
            places: &[],
            units,
        }
    }

    fn track(unit: &str, map: &str) -> Track {
        Track {
            id: id("track_t"),
            unit: id(unit),
            map: id(map),
            span: TimeSpan::new(1, 10).unwrap(),
            waypoints: vec![],
            path: None,
        }
    }

    #[test]
    fn known_unit_passes_validation() {
        let t = track("unit_a", "map_world");
        let ok = validate_situations_and_tracks(
            &[],
            &[t],
            KnownIds {
                events: &[],
                maps: &[id("map_world")],
                characters: &[],
                places: &[],
                units: &[id("unit_a")],
            },
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn dangling_track_unit_is_reported() {
        // ADR-0004：运动主体必须指向已知单位（旧口径的「角色或势力」已收敛为 Unit）。
        let t = track("unit_missing", "map_world");
        let err = validate_situations_and_tracks(
            &[],
            &[t],
            KnownIds {
                events: &[],
                maps: &[id("map_world")],
                characters: &[],
                places: &[],
                units: &[],
            },
        )
        .unwrap_err();
        match err {
            CoreError::DanglingReference { referrer, missing } => {
                assert!(referrer.contains("field `unit`"), "referrer={referrer}");
                assert_eq!(missing, "unit_missing");
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }

    fn sit(steps: Vec<&str>, narrative: Option<Vec<&str>>) -> Situation {
        Situation {
            id: id("sit_x"),
            name: "x".into(),
            summary: None,
            steps: steps
                .into_iter()
                .map(|e| wv_core::SituationStep {
                    event: id(e),
                    focus: false,
                    pause_secs: None,
                    caption: None,
                    tracks: vec![],
                })
                .collect(),
            span: TimeSpan::new(1, 100).unwrap(),
            units: vec![],
            narrative_order: narrative.map(|v| v.into_iter().map(id).collect()),
            maps: vec![],
            protagonist: None,
        }
    }

    #[test]
    fn dangling_situation_unit_is_reported() {
        // ADR-0004：事态作为**归纳容器**持有的单位必须存在。
        let events = vec![id("evt_a")];
        let mut s = sit(vec!["evt_a"], None);
        s.units = vec![id("unit_missing")];
        let err = validate_situations_and_tracks(
            &[s],
            &[],
            KnownIds {
                events: &events,
                maps: &[],
                characters: &[],
                places: &[],
                units: &[],
            },
        )
        .unwrap_err();
        match err {
            CoreError::DanglingReference { referrer, missing } => {
                assert!(referrer.contains("units[0]"), "referrer={referrer}");
                assert_eq!(missing, "unit_missing");
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }

    #[test]
    fn valid_situation_passes_validation() {
        let events = vec![id("evt_a"), id("evt_b")];
        let s = sit(vec!["evt_a", "evt_b"], None);
        assert!(validate_situations_and_tracks(&[s], &[], known(&events, &[])).is_ok());
    }

    #[test]
    fn dangling_step_event_is_reported() {
        let events = vec![id("evt_a")];
        let s = sit(vec!["evt_a", "evt_missing"], None);
        let err = validate_situations_and_tracks(&[s], &[], known(&events, &[])).unwrap_err();
        match err {
            CoreError::DanglingReference { referrer, missing } => {
                assert!(referrer.contains("steps[1].event"), "referrer={referrer}");
                assert_eq!(missing, "evt_missing");
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }

    #[test]
    fn dangling_narrative_order_id_is_reported() {
        let events = vec![id("evt_a")];
        let s = sit(vec!["evt_a"], Some(vec!["evt_missing"]));
        let err = validate_situations_and_tracks(&[s], &[], known(&events, &[])).unwrap_err();
        match err {
            CoreError::DanglingReference { referrer, missing } => {
                assert!(referrer.contains("narrative_order[0]"));
                assert_eq!(missing, "evt_missing");
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }

    #[test]
    fn narrative_order_id_not_in_steps_is_reported() {
        // 事件存在，但不属于本事的 steps → 仍应报错（README §7.2 精神）。
        let events = vec![id("evt_a"), id("evt_b")];
        let s = sit(vec!["evt_a"], Some(vec!["evt_b"]));
        let err = validate_situations_and_tracks(&[s], &[], known(&events, &[])).unwrap_err();
        match err {
            CoreError::DanglingReference { referrer, .. } => {
                assert!(
                    referrer.contains("不属于本事的 steps"),
                    "referrer={referrer}"
                );
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }

    #[test]
    fn dangling_track_map_is_reported() {
        let events = vec![id("evt_a")];
        let mut s = sit(vec!["evt_a"], None);
        s.maps = vec![id("map_missing")];
        let err = validate_situations_and_tracks(&[s], &[], known(&events, &[])).unwrap_err();
        match err {
            CoreError::DanglingReference { referrer, missing } => {
                assert!(referrer.contains("maps[0]"));
                assert_eq!(missing, "map_missing");
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }
}
