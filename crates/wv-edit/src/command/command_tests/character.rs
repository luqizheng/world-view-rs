//! 人物状态片段（等级列表）命令（fixture：sample_character）。（2026-10-30 等级列表编辑器）
use super::*;

/// 人物夹具：两条等级片段，含 place / title——验证整表替换不误退未触字段。
fn sample_character(id: &str, name: &str) -> wv_core::character::Character {
    wv_core::character::Character {
        id: Id::new_unchecked(id),
        name: name.into(),
        alias: None,
        traits: vec!["沉稳".into()],
        attributes: Default::default(),
        born_at: None,
        died_at: None,
        state_spans: vec![
            wv_core::character::CharacterStateSpan {
                span: wv_core::time::TimeSpan {
                    from: 100,
                    to: Some(200),
                },
                place: Some(Id::new_unchecked("city_a")),
                title: Some("外门弟子".into()),
                power: Some(10.0),
            },
            wv_core::character::CharacterStateSpan {
                span: wv_core::time::TimeSpan {
                    from: 200,
                    to: None,
                },
                place: Some(Id::new_unchecked("village_b")),
                title: Some("巡山执事".into()),
                power: Some(35.0),
            },
        ],
    }
}

#[test]
fn set_character_state_spans_replaces_table_then_reverts() {
    let mut w = World::default();
    w.characters.push(sample_character("c1", "沈砚"));

    // 草稿：删首行、改次行等级、追加一段新历史（V2：作者可改已发生历史）。
    let draft = vec![
        wv_core::character::CharacterStateSpan {
            span: wv_core::time::TimeSpan {
                from: 200,
                to: None,
            },
            place: Some(Id::new_unchecked("village_b")),
            title: Some("巡山执事".into()),
            power: Some(50.0),
        },
        wv_core::character::CharacterStateSpan {
            span: wv_core::time::TimeSpan {
                from: 300,
                to: Some(400),
            },
            place: None,
            title: None,
            power: Some(72.5),
        },
    ];
    let cmd = SetCharacterStateSpansCommand::new(Id::new_unchecked("c1"), draft);
    let dirty = cmd.apply(&mut w).unwrap();

    assert_eq!(w.characters[0].state_spans.len(), 2);
    assert_eq!(w.characters[0].state_spans[1].power, Some(72.5));
    assert_eq!(dirty, vec!["characters".to_string()]);

    cmd.revert(&mut w).unwrap();
    assert_eq!(w.characters[0].state_spans.len(), 2);
    assert_eq!(w.characters[0].state_spans[0].power, Some(10.0));
    assert_eq!(
        w.characters[0].state_spans[0].title.as_deref(),
        Some("外门弟子"),
        "revert 应原样退回对话框不暴露的 title 字段"
    );
    assert_eq!(
        w.characters[0].state_spans[0].place,
        Some(Id::new_unchecked("city_a")),
        "revert 应原样退回对话框不暴露的 place 字段"
    );
    assert_eq!(w.characters[0].state_spans[1].power, Some(35.0));
    assert!(
        w.characters[0].state_spans[1].span.to.is_none(),
        "开放区间的 to=None 应原样往返"
    );
}

#[test]
fn set_character_state_spans_missing_target_errors_without_touching_world() {
    let mut w = World::default();
    w.characters.push(sample_character("c1", "沈砚"));
    let before = w.characters[0].state_spans.clone();

    let cmd = SetCharacterStateSpansCommand::new(Id::new_unchecked("ghost"), vec![]);
    let err = cmd.apply(&mut w).unwrap_err();
    assert!(
        err.to_string().contains("ghost"),
        "错误信息应带上目标 ID，got {err}"
    );
    assert_eq!(
        w.characters[0].state_spans, before,
        "apply 失败不得改动任何人物"
    );
}

#[test]
fn set_character_state_spans_revert_before_apply_errors() {
    let mut w = World::default();
    w.characters.push(sample_character("c1", "沈砚"));
    let cmd = SetCharacterStateSpansCommand::new(Id::new_unchecked("c1"), vec![]);
    let err = cmd.revert(&mut w).unwrap_err();
    assert!(
        err.to_string().contains("尚未 apply"),
        "未 apply 先 revert 应报前置缺失，got {err}"
    );
}
