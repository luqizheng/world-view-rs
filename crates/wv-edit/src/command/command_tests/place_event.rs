//! 地点属性/类别 与 事件字段（fixture：sample_place/sample_event）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

// ====== 地点属性命令（统一 Inspector · 2026-10-22）======================

/// 地点夹具：锚点已就位，便于验证「锚点条件可写」。
fn sample_place(id: &str, name: &str) -> wv_core::distance::Place {
    wv_core::distance::Place {
        kind: None,
        line: None,
        id: Id::new_unchecked(id),
        name: name.into(),
        feature: None,
        biome: Some(wv_terrain::biome::BIOME_HILLS),
        envelope: None,
        map: Some(Id::new_unchecked(TEST_MAP)),
        x: Some(10.0),
        y: Some(20.0),
        w: Some(4.0),
        h: Some(6.0),
        rank: Some(1),
        line_kind: None,
        line_width_m: None,
    }
}

#[test]
fn update_place_changes_name_rank_anchor_then_reverts() {
    let mut w = World::default();
    w.distances.places.push(sample_place("p1", "旧名"));

    let cmd = UpdatePlaceCommand::new(
        Id::new_unchecked("p1"),
        "新名".into(),
        Some(3),
        Some(99.0),
        Some(88.0),
    );
    cmd.apply(&mut w).unwrap();
    assert_eq!(w.distances.places[0].name, "新名");
    assert_eq!(w.distances.places[0].rank, Some(3));
    assert_eq!(w.distances.places[0].x, Some(99.0));
    assert_eq!(w.distances.places[0].y, Some(88.0));

    cmd.revert(&mut w).unwrap();
    assert_eq!(w.distances.places[0].name, "旧名");
    assert_eq!(w.distances.places[0].rank, Some(1));
    assert_eq!(
        w.distances.places[0].x,
        Some(10.0),
        "revert 应恢复原始锚点 X"
    );
    assert_eq!(w.distances.places[0].y, Some(20.0));
}

#[test]
fn update_place_keeps_anchor_when_xy_none() {
    let mut w = World::default();
    w.distances.places.push(sample_place("p1", "旧名"));

    // new_x / new_y 为 None = 「不改」——**不是**「写成 None」。
    let cmd = UpdatePlaceCommand::new(Id::new_unchecked("p1"), "新名".into(), Some(2), None, None);
    cmd.apply(&mut w).unwrap();
    assert_eq!(
        w.distances.places[0].x,
        Some(10.0),
        "new_x = None 时锚点 X 应保持原值，不得被清空"
    );
    assert_eq!(w.distances.places[0].y, Some(20.0));
    assert_eq!(w.distances.places[0].name, "新名");
    assert_eq!(w.distances.places[0].rank, Some(2));
}

#[test]
fn update_place_on_missing_id_fails() {
    let mut w = World::default();
    let cmd = UpdatePlaceCommand::new(Id::new_unchecked("ghost"), "X".into(), None, None, None);
    assert!(cmd.apply(&mut w).is_err());
}

#[test]
fn update_place_dirty_path_is_places_prefix() {
    let mut w = World::default();
    w.distances.places.push(sample_place("p1", "旧名"));
    let dirty = UpdatePlaceCommand::new(Id::new_unchecked("p1"), "新名".into(), None, None, None)
        .apply(&mut w)
        .unwrap();
    assert_eq!(dirty, vec!["distances/places".to_string()]);
}

#[test]
fn set_place_kind_writes_and_reverts_the_category() {
    use wv_core::distance::PoiKind;
    let mut w = World::default();
    w.distances.places.push(sample_place("p1", "医院"));
    let cmd = SetPlaceKindCommand::new(Id::new_unchecked("p1"), Some(PoiKind::Medical));

    let dirty = cmd.apply(&mut w).unwrap();
    assert_eq!(
        w.distances.places[0].kind,
        Some(PoiKind::Medical),
        "apply 应写入类别"
    );
    assert_eq!(dirty, vec!["distances/places".to_string()]);

    cmd.revert(&mut w).unwrap();
    assert_eq!(
        w.distances.places[0].kind, None,
        "revert 应恢复原类别（此处为缺省 None）"
    );
}

/// `kind = None` 是「清空」而不是「不改」——与 `UpdatePlaceCommand` 的
/// `Option` 语义不同，这条测试钉住区别。
#[test]
fn set_place_kind_none_clears_the_category() {
    use wv_core::distance::PoiKind;
    let mut w = World::default();
    let mut p = sample_place("p1", "医院");
    p.kind = Some(PoiKind::Medical);
    w.distances.places.push(p);

    SetPlaceKindCommand::new(Id::new_unchecked("p1"), None)
        .apply(&mut w)
        .unwrap();
    assert_eq!(w.distances.places[0].kind, None, "None 应清空类别");
}

#[test]
fn set_place_kind_on_missing_id_fails() {
    use wv_core::distance::PoiKind;
    let mut w = World::default();
    let cmd = SetPlaceKindCommand::new(Id::new_unchecked("ghost"), Some(PoiKind::Food));
    assert!(cmd.apply(&mut w).is_err());
}

// ====== 事件属性命令（统一 Inspector · 2026-10-22）======================

/// 事件夹具：带时间跨度 + 一个参与者。
fn sample_event(id: &str, name: &str) -> wv_core::model::event::Event {
    wv_core::model::event::Event {
        id: Id::new_unchecked(id),
        name: name.into(),
        span: TimeSpan {
            from: 100,
            to: Some(200),
        },
        location: None,
        participants: vec![Id::new_unchecked("char_a")],
        narrative: Some("旧叙述".into()),
        outcome: None,
        effects: Vec::new(),
        narrations: Vec::new(),
    }
}

#[test]
fn update_event_changes_fields_then_reverts() {
    let mut w = World::default();
    w.events.push(sample_event("e1", "旧名"));

    let cmd = UpdateEventCommand::new(
        Id::new_unchecked("e1"),
        "新名".into(),
        TimeSpan {
            from: 300,
            to: None,
        },
        None,
        vec![Id::new_unchecked("char_b")],
        Some("新叙述".into()),
        Some("新结果".into()),
    );
    cmd.apply(&mut w).unwrap();
    assert_eq!(w.events[0].name, "新名");
    assert_eq!(w.events[0].span.from, 300);
    assert_eq!(w.events[0].span.to, None);
    assert_eq!(w.events[0].participants, vec![Id::new_unchecked("char_b")]);
    assert_eq!(w.events[0].narrative.as_deref(), Some("新叙述"));
    assert_eq!(w.events[0].outcome.as_deref(), Some("新结果"));

    cmd.revert(&mut w).unwrap();
    assert_eq!(w.events[0].name, "旧名");
    assert_eq!(w.events[0].span.from, 100);
    assert_eq!(w.events[0].span.to, Some(200));
    assert_eq!(w.events[0].participants, vec![Id::new_unchecked("char_a")]);
    assert_eq!(w.events[0].narrative.as_deref(), Some("旧叙述"));
    assert_eq!(w.events[0].outcome, None);
}

#[test]
fn update_event_revert_preserves_untouched_effects() {
    let mut w = World::default();
    w.events.push(sample_event("e1", "旧名"));
    // effects 不在本命令的编辑面内——revert **不得**把它一并退掉。
    w.events[0]
        .effects
        .push(wv_core::model::event::Effect::Custom {
            note: "作者手写的效果".into(),
        });

    let cmd = UpdateEventCommand::new(
        Id::new_unchecked("e1"),
        "新名".into(),
        TimeSpan {
            from: 100,
            to: Some(200),
        },
        None,
        Vec::new(),
        None,
        None,
    );
    cmd.apply(&mut w).unwrap();
    cmd.revert(&mut w).unwrap();

    assert_eq!(w.events[0].name, "旧名", "本命令改的字段应回退");
    assert_eq!(
        w.events[0].effects.len(),
        1,
        "revert 只写回本命令触及的字段——effects 不在其列，应保持不变"
    );
}

#[test]
fn update_event_on_missing_id_fails() {
    let mut w = World::default();
    let cmd = UpdateEventCommand::new(
        Id::new_unchecked("ghost"),
        "X".into(),
        TimeSpan { from: 0, to: None },
        None,
        Vec::new(),
        None,
        None,
    );
    assert!(cmd.apply(&mut w).is_err());
}

#[test]
fn update_event_dirty_path_is_events_prefix() {
    let mut w = World::default();
    w.events.push(sample_event("e1", "旧名"));
    let dirty = UpdateEventCommand::new(
        Id::new_unchecked("e1"),
        "新名".into(),
        TimeSpan { from: 0, to: None },
        None,
        Vec::new(),
        None,
        None,
    )
    .apply(&mut w)
    .unwrap();
    assert_eq!(dirty, vec!["timeline/events".to_string()]);
}

// ====== 登记事件命令（UX-002 · 2026-10-29）==============================

#[test]
fn add_event_appends_reports_dirty_path_and_reverts_to_empty() {
    let mut w = World::default();
    let cmd = AddEventCommand::new(sample_event("e_new", "新事件"));
    let dirty = cmd.apply(&mut w).expect("登记应成功");
    assert_eq!(
        dirty,
        vec!["timeline/events".to_string()],
        "脏路径应指向事件目录"
    );
    assert_eq!(w.events.len(), 1, "事件应已入列");
    assert_eq!(w.events[0].name, "新事件");

    cmd.revert(&mut w).expect("撤销登记应成功");
    assert!(
        w.events.is_empty(),
        "撤销后事件应完全消失（登记过 = 没登记过）"
    );
}

#[test]
fn add_event_rejects_duplicate_id_without_touching_the_world() {
    let mut w = World::default();
    w.events.push(sample_event("e_dup", "既有"));
    let cmd = AddEventCommand::new(sample_event("e_dup", "重名"));
    let err = cmd.apply(&mut w).expect_err("撞 ID 应报错");
    assert!(err.to_string().contains("已存在"), "got {err}");
    assert_eq!(w.events.len(), 1, "失败不得改动世界");
    assert_eq!(w.events[0].name, "既有", "既有事件不得被触碰");
}

#[test]
fn add_event_revert_without_apply_reports_missing_target() {
    let mut w = World::default();
    let cmd = AddEventCommand::new(sample_event("ghost", "未登记"));
    let err = cmd.revert(&mut w).expect_err("未 apply 先 revert 应报错");
    assert!(err.to_string().contains("ghost"), "got {err}");
}
