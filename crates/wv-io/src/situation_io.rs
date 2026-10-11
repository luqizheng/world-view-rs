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
//! ## 强度：**软失败**（2026-10-11 用户裁定 H5 甲）
//!
//! 本模块**返回问题清单**，不返回 `Result`。`Project::load_world()` 不再因
//! 悬空引用而失败——作者可以正常进入工程、编辑、保存，问题在诊断面板中列出。
//!
//! 依据是 `AGENTS.md` §7.5 V1「校验只提示，不阻止」：**「报错但允许存在」**
//! 是用户原话。旧实现（遇错即`Err`）在语义上是「报错且禁止存在，连看都看不到」，
//! 超出裁定范围——那不是宽容，是让作者没有机会去改。

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
/// ## 返回**全部**问题，而不是遇到第一个就停（2026-10-11 用户裁定 H5 甲）
///
/// 旧实现遇错即 `return Err`，导致作者一次只能看到**一个**悬空引用，
/// 改一个再打开看下一个——这与「工具的职责是让作者看见问题」
/// （`AGENTS.md` §7.5V1）相悖。现在**一次性检出全部**，
/// 汇总为 [`Vec<CoreError>`]，由调用方决定如何呈现。
///
/// ## 硬失败 → 软失败
///
/// 本函数**不再让工程打不开**。`Project::load_world()` 不再传播这些错误；
/// 它们被转成 `DiagLevel::Error` 级诊断（`crate::diagnostics`），
/// 在校验面板里列出，作者可**正常进入、编辑、保存**。
///
/// 「报错但允许存在」——这是用户 2026-10-11 的原话。
pub fn collect_situations_and_tracks_dangling(
    situations: &[Situation],
    tracks: &[Track],
    known: KnownIds<'_>,
) -> Vec<CoreError> {
    let mut dangling: Vec<CoreError> = Vec::new();
    let has = |ids: &[Id], id: &Id| ids.iter().any(|x| x == id);

    let track_ids: Vec<Id> = tracks.iter().map(|t| t.id.clone()).collect();

    for track in tracks {
        // 运动主体必须是已知单位（ADR-0004）。旧口径允许「角色或势力」并保持宽松，
        // 现已收敛为 Unit —— 悬空必须被**检出**（§7.2），但不阻断加载（§7.5V1）。
        if !has(known.units, &track.unit) {
            dangling.push(CoreError::DanglingReference {
                referrer: format!("track.{} (field `unit`)", track.id),
                missing: track.unit.to_string(),
            });
        }
        if !has(known.maps, &track.map) {
            dangling.push(CoreError::DanglingReference {
                referrer: format!("track.{} (field `map`)", track.id),
                missing: track.map.to_string(),
            });
        }
        for (i, wp) in track.waypoints.iter().enumerate() {
            if let Some(place) = &wp.place {
                if !has(known.places, place) {
                    dangling.push(CoreError::DanglingReference {
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
                dangling.push(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `steps[{i}].event`)", sit.id),
                    missing: step.event.to_string(),
                });
            }
            for (j, track_id) in step.tracks.iter().enumerate() {
                if !has(&track_ids, track_id) {
                    dangling.push(CoreError::DanglingReference {
                        referrer: format!("situation.{} (field `steps[{i}].tracks[{j}]`)", sit.id),
                        missing: track_id.to_string(),
                    });
                }
            }
        }

        // narrative_order 里的每个 ID 必须存在，且必须属于本事的 steps（基线 §2.21）。
        if let Some(order) = &sit.narrative_order {
            for (i, id) in order.iter().enumerate() {
                // 「不存在」与「不属于本事的 steps」是**两个不同的问题**，
                // 但后者的前提是前者成立。若ID 压根不存在，作者只需解决
                // 「事件缺失」这一件事——再补一条「它不属于 steps」是**噪音**
                // （同一个字段同一处悬空被报两次，作者会困惑该改哪儿）。
                // 故此处 `continue`：不存在的 ID 只报一次。
                if !has(known.events, id) {
                    dangling.push(CoreError::DanglingReference {
                        referrer: format!("situation.{} (field `narrative_order[{i}]`)", sit.id),
                        missing: id.to_string(),
                    });
                    continue;
                }
                if !sit.steps.iter().any(|s| &s.event == id) {
                    dangling.push(CoreError::DanglingReference {
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
                dangling.push(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `maps[{i}]`)", sit.id),
                    missing: map_id.to_string(),
                });
            }
        }

        // 归纳容器持有的单位必须存在（ADR-0004）。
        for (i, unit_id) in sit.units.iter().enumerate() {
            if !has(known.units, unit_id) {
                dangling.push(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `units[{i}]`)", sit.id),
                    missing: unit_id.to_string(),
                });
            }
        }

        if let Some(protagonist) = &sit.protagonist {
            if !has(known.characters, protagonist) {
                dangling.push(CoreError::DanglingReference {
                    referrer: format!("situation.{} (field `protagonist`)", sit.id),
                    missing: protagonist.to_string(),
                });
            }
        }
    }

    dangling
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

    /// 断言「清单里恰好有一条且指向 `missing`」。
    fn assert_one_dangling(errs: &[CoreError], referrer_needle: &str, missing: &str) {
        assert_eq!(
            errs.len(),
            1,
            "应恰好检出一条悬空引用，实得 {:?}",
            errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        match &errs[0] {
            CoreError::DanglingReference {
                referrer,
                missing: m,
            } => {
                assert!(
                    referrer.contains(referrer_needle),
                    "referrer={referrer} 应含 {referrer_needle}"
                );
                assert_eq!(m, missing);
            }
            other => panic!("期望 DanglingReference，实际 {other:?}"),
        }
    }

    #[test]
    fn known_unit_passes_validation() {
        let t = track("unit_a", "map_world");
        let errs = collect_situations_and_tracks_dangling(
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
        assert!(errs.is_empty());
    }

    #[test]
    fn dangling_track_unit_is_reported() {
        // ADR-0004：运动主体必须指向已知单位（旧口径的「角色或势力」已收敛为 Unit）。
        let t = track("unit_missing", "map_world");
        let errs = collect_situations_and_tracks_dangling(
            &[],
            &[t],
            KnownIds {
                events: &[],
                maps: &[id("map_world")],
                characters: &[],
                places: &[],
                units: &[],
            },
        );
        assert_one_dangling(&errs, "field `unit`", "unit_missing");
    }

    #[test]
    fn dangling_situation_unit_is_reported() {
        // ADR-0004：事态作为**归纳容器**持有的单位必须存在。
        let events = vec![id("evt_a")];
        let mut s = sit(vec!["evt_a"], None);
        s.units = vec![id("unit_missing")];
        let errs = collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]));
        assert_one_dangling(&errs, "units[0]", "unit_missing");
    }

    #[test]
    fn valid_situation_passes_validation() {
        let events = vec![id("evt_a"), id("evt_b")];
        let s = sit(vec!["evt_a", "evt_b"], None);
        assert!(collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[])).is_empty());
    }

    #[test]
    fn dangling_step_event_is_reported() {
        let events = vec![id("evt_a")];
        let s = sit(vec!["evt_a", "evt_missing"], None);
        let errs = collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]));
        assert_one_dangling(&errs, "steps[1].event", "evt_missing");
    }

    #[test]
    fn dangling_narrative_order_id_is_reported() {
        let events = vec![id("evt_a")];
        let s = sit(vec!["evt_a"], Some(vec!["evt_missing"]));
        let errs = collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]));
        assert_one_dangling(&errs, "narrative_order[0]", "evt_missing");
    }

    #[test]
    fn narrative_order_id_not_in_steps_is_reported() {
        // 事件存在，但不属于本事的 steps → 仍应检出（README §7.2 精神）。
        let events = vec![id("evt_a"), id("evt_b")];
        let s = sit(vec!["evt_a"], Some(vec!["evt_b"]));
        let errs = collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]));
        assert_one_dangling(&errs, "不属于本事的 steps", "evt_b");
    }

    #[test]
    fn dangling_track_map_is_reported() {
        let events = vec![id("evt_a")];
        let mut s = sit(vec!["evt_a"], None);
        s.maps = vec![id("map_missing")];
        let errs = collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]));
        assert_one_dangling(&errs, "maps[0]", "map_missing");
    }

    #[test]
    fn all_dangling_references_are_collected_not_just_the_first() {
        // H5 甲的核心：旧实现遇错即返回，作者一次只能看到一个。
        // 现在必须**一次性**给出全部，否则「可见全部问题」只是空话。
        let events = vec![id("evt_a")];
        let mut s = sit(
            vec!["evt_missing_1", "evt_missing_2"],
            Some(vec!["evt_missing_3"]),
        );
        s.maps = vec![id("map_missing")];
        s.units = vec![id("unit_missing")];
        let errs = collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]));
        assert_eq!(
            errs.len(),
            5,
            "5 处悬空必须一次报全，实得 {}: {:?}",
            errs.len(),
            errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn dangling_across_tracks_and_situations_are_all_collected() {
        let events = vec![id("evt_a")];
        let mut t1 = track("unit_missing_a", "map_world");
        t1.id = id("track_1");
        let mut t2 = track("unit_missing_b", "map_world");
        t2.id = id("track_2");
        let mut s = sit(vec!["evt_missing"], None);
        s.units = vec![id("unit_missing_c")];
        let errs = collect_situations_and_tracks_dangling(
            &[s],
            &[t1, t2],
            KnownIds {
                events: &events,
                maps: &[id("map_world")],
                characters: &[],
                places: &[],
                units: &[],
            },
        );
        assert_eq!(
            errs.len(),
            4,
            "跨 track 与 situation 的悬空必须一次报全，实得 {:?}",
            errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn collection_order_is_deterministic() {
        // AGENTS §1.4：同样输入必须给出同样顺序的清单。
        let events = vec![id("evt_a")];
        let build = || {
            let mut s = sit(vec!["evt_m1", "evt_m2"], None);
            s.maps = vec![id("map_m")];
            collect_situations_and_tracks_dangling(&[s], &[], known(&events, &[]))
        };
        let a: Vec<String> = build().iter().map(|e| e.to_string()).collect();
        let b: Vec<String> = build().iter().map(|e| e.to_string()).collect();
        assert_eq!(a, b, "悬空清单必须确定性");
    }
}
