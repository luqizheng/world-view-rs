//! 示例工程的集成测试。
//!
//! 覆盖 AGENTS.md §7.2 的两项强制要求：
//! 1. YAML round-trip 稳定（`load(save(w)) == w`）；
//! 2. 引用完整性可被检出。

use std::path::PathBuf;

use wv_core::map::{Layer, LayerKind, MapLevel, MapNode};
use wv_core::time::{Calendar, TimeSpan, Timeline};
use wv_core::WorldMeta;
use wv_io::Project;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/minimal-world")
}

#[test]
fn example_project_opens_and_validates() {
    let project = Project::open(example_dir()).expect("示例工程应可打开");
    assert_eq!(project.meta().name, "最小示例世界");
    assert_eq!(project.meta().schema_version, 2);

    let warnings = project.validate().expect("校验不应报错");
    assert!(
        warnings.is_empty(),
        "示例工程应无结构警告, got {warnings:?}"
    );
}

#[test]
fn v1_project_is_rejected_with_migration_guidance() {
    // 基线 §2.16：schema 1 → 2 不做自动迁移，必须给出可操作的错误。
    let tmp = std::env::temp_dir().join("wv_v1_project_probe");
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("world.yaml"),
        "name: old\nversion: \"0.1\"\nschema_version: 1\nroot_map: world_main\ntimeline_range:\n  from: 1\n  to: 2\n",
    )
    .unwrap();

    let err = Project::open(&tmp).unwrap_err();
    match err {
        wv_io::IoError::SchemaTooOld { found, current, .. } => {
            assert_eq!(found, 1);
            assert_eq!(current, 2);
        }
        other => panic!("期望 SchemaTooOld，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn world_meta_yaml_round_trips() {
    let project = Project::open(example_dir()).unwrap();
    let meta = project.meta().clone();

    let text = serde_yaml::to_string(&meta).expect("序列化应成功");
    let back: WorldMeta = serde_yaml::from_str(&text).expect("反序列化应成功");

    assert_eq!(meta, back, "WorldMeta 的 YAML round-trip 必须稳定");
}

#[test]
fn map_node_yaml_round_trips() {
    let node = MapNode {
        id: wv_core::Id::new_unchecked("world_main"),
        name: "主世界".into(),
        level: MapLevel::World,
        parent: None,
        children: vec![wv_core::Id::new_unchecked("region_north")],
        layers: vec![Layer {
            id: wv_core::Id::new_unchecked("layer_borders"),
            name: "疆界".into(),
            kind: LayerKind::Border,
            visible: true,
            locked: false,
            opacity: 0.8,
            z: 10,
            source: Some("borders.yaml".into()),
        }],
        extent: None,
        raster: None,
    };

    let text = serde_yaml::to_string(&node).unwrap();
    let back: MapNode = serde_yaml::from_str(&text).unwrap();
    assert_eq!(node, back, "MapNode 的 YAML round-trip 必须稳定");
}

#[test]
fn timeline_yaml_round_trips() {
    let timeline = Timeline {
        calendar: Calendar {
            prefix: Some("无仙".into()),
            start_year: 1,
            days_per_year: 360,
            days_per_month: 30,
        },
        timeline_range: TimeSpan::new(1, 180001).unwrap(),
        anchors: vec![wv_core::time::Anchor {
            id: wv_core::Id::new_unchecked("anchor_founding"),
            name: "青云城建城".into(),
            day: 107641,
        }],
    };

    let text = serde_yaml::to_string(&timeline).unwrap();
    let back: Timeline = serde_yaml::from_str(&text).unwrap();
    assert_eq!(timeline, back, "Timeline 的 YAML round-trip 必须稳定");
}

#[test]
fn open_ended_span_omits_to_field_in_yaml() {
    // 开放区间序列化时不应写出 `to:` 空字段——否则 YAML 噪音大且 diff 脏。
    let span = TimeSpan::open(107641);
    let text = serde_yaml::to_string(&span).unwrap();
    assert!(!text.contains("to:"), "开放区间不应输出 to 字段: {text}");
}

#[test]
fn missing_project_dir_is_reported() {
    let err = Project::open(example_dir().join("__not_here__"));
    assert!(matches!(err, Err(wv_io::IoError::NotAProject(_))));
}

#[test]
fn example_project_loads_situations_and_tracks() {
    // S7：事态 / 轨迹必须能从示例工程加载，并通过引用完整性校验。
    let project = Project::open(example_dir()).unwrap();
    let world = project.load_world().expect("加载应成功");

    assert_eq!(world.situations.len(), 1, "示例应有 1 条事态");
    assert_eq!(world.tracks.len(), 1, "示例应有 1 条轨迹");

    let sit = &world.situations[0];
    assert_eq!(sit.id.as_str(), "sit_qingyun_rise");
    // 播放顺序由事件 span.from 升序派生（不按 YAML 书写顺序）。
    let order = sit.playback_order(&world.events);
    assert_eq!(
        order.iter().map(|i| i.as_str()).collect::<Vec<_>>(),
        vec!["evt_qingyun_founding", "evt_shenyan_join"],
        "立城(107641) 应排在入宗(107642) 之前"
    );
}

#[test]
fn example_project_loads_units() {
    // ADR-0004：单位（Unit）必须能从示例工程加载，
    // 且轨迹的**运动主体**指向它（不再是含糊的「角色或势力」）。
    let project = Project::open(example_dir()).unwrap();
    let world = project.load_world().expect("加载应成功");

    assert_eq!(world.units.len(), 1, "示例应有 1 个单位");
    let unit = &world.units[0];
    assert_eq!(unit.id.as_str(), "unit_shenyan");
    // 角色经 members **引用**进来，不复制角色本身（P2：两者分离）。
    assert_eq!(
        unit.members.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
        vec!["char_shenyan"]
    );

    assert_eq!(world.tracks.len(), 1);
    assert_eq!(world.tracks[0].unit.as_str(), "unit_shenyan");
}

#[test]
fn unit_yaml_round_trip() {
    // 原则二：`load(save(w)) == w`。
    let project = Project::open(example_dir()).unwrap();
    let world = project.load_world().unwrap();

    let text = serde_yaml::to_string(&world.units[0]).unwrap();
    let back: wv_core::Unit = serde_yaml::from_str(&text).unwrap();
    assert_eq!(world.units[0], back, "Unit round-trip 必须稳定");
}

#[test]
fn situation_and_track_yaml_round_trip() {
    // S7：事态与轨迹的 YAML round-trip 必须稳定。
    let project = Project::open(example_dir()).unwrap();
    let world = project.load_world().unwrap();

    let sit_text = serde_yaml::to_string(&world.situations[0]).unwrap();
    let sit_back: wv_core::Situation = serde_yaml::from_str(&sit_text).unwrap();
    assert_eq!(
        world.situations[0], sit_back,
        "Situation round-trip 必须稳定"
    );

    let track_text = serde_yaml::to_string(&world.tracks[0]).unwrap();
    let track_back: wv_core::Track = serde_yaml::from_str(&track_text).unwrap();
    assert_eq!(world.tracks[0], track_back, "Track round-trip 必须稳定");
}

#[test]
fn track_sample_interpolates_between_waypoints() {
    // S8：轨迹中点由带 day 的锚点纯函数线性插值求出。
    let project = Project::open(example_dir()).unwrap();
    let world = project.load_world().unwrap();
    let track = &world.tracks[0];

    let start = track.sample_at(107642).unwrap();
    assert_eq!((start.x, start.y), (1280.0, 2048.0), "起点应为黑水村坐标");

    let end = track.sample_at(107646).unwrap();
    assert_eq!((end.x, end.y), (2048.0, 1536.0), "终点应为青云城坐标");

    // 107644 是两个锚点之间的中间锚点，坐标应精确命中。
    let mid = track.sample_at(107644).unwrap();
    assert_eq!((mid.x, mid.y), (1792.0, 1920.0));
}

#[test]
fn dangling_reference_in_situation_is_rejected_on_load() {
    // AGENTS §7.2：悬空引用必须报错，不得静默忽略。
    // 构造一个临时工程，事态引用不存在的事件。
    let tmp = std::env::temp_dir().join("wv_dangling_situation_probe");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("timeline/situations")).unwrap();
    std::fs::create_dir_all(tmp.join("maps/world")).unwrap();
    std::fs::write(
        tmp.join("world.yaml"),
        "name: probe\nversion: \"0.1\"\nschema_version: 2\nroot_map: world_main\ntimeline_range:\n  from: 1\n  to: 2\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("timeline/situations/sit_bad.yaml"),
        "id: sit_bad\nname: bad\nfrom: 1\nto: 2\nsteps:\n  - event: evt_missing\n",
    )
    .unwrap();

    let project = Project::open(&tmp).unwrap();
    let err = project.load_world().unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("evt_missing"),
        "错误信息应指出悬空引用 evt_missing, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

// ────────────────────────────────────────────────────────────────────
// 相遇检测与诊断分层（ADR-0005）
// ────────────────────────────────────────────────────────────────────

#[test]
fn diagnose_on_example_project_yields_no_errors() {
    // 核心裁定：诊断是「永不失败」的提示路径。示例工程即便有相遇未写事件，
    // 也**不得**产生 Error 级条目（否则会阻断保存，违背 AGENTS §7.5 V1）。
    let project = Project::open(example_dir()).expect("示例工程应可打开");
    let diags = project.diagnose().expect("diagnose 永不因提示而失败");
    assert!(
        !diags.has_errors(),
        "示例工程不应有错误级诊断：{:?}",
        diags.items
    );
}

#[test]
fn diagnose_returns_hint_level_items_with_stable_fingerprints() {
    let project = Project::open(example_dir()).unwrap();
    let diags = project.diagnose().unwrap();
    for d in &diags.items {
        assert_eq!(d.level, wv_io::DiagLevel::Hint, "提示路径不得产出 Error");
        assert!(
            !d.fingerprint.is_empty(),
            "每条诊断必须有稳定标识，否则无法认领：{d:?}"
        );
    }
}

#[test]
fn diagnose_is_deterministic_across_calls() {
    // AGENTS §1.4：确定性。同样输入必须给出同样顺序的诊断。
    let project = Project::open(example_dir()).unwrap();
    let a = project.diagnose().unwrap();
    let b = project.diagnose().unwrap();
    assert_eq!(a, b, "诊断必须确定性");
}

#[test]
fn encounter_hints_are_detected_when_two_units_share_a_place_unwritten() {
    // 构造一个最小工程：两个单位的轨迹在同一天抵达同一地点，且**没有**事件。
    let tmp = std::env::temp_dir().join("wv_encounter_probe");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("timeline/events")).unwrap();
    std::fs::create_dir_all(tmp.join("maps/main")).unwrap();
    std::fs::create_dir_all(tmp.join("units")).unwrap();
    std::fs::create_dir_all(tmp.join("tracks")).unwrap();
    std::fs::write(
        tmp.join("world.yaml"),
        "name: probe\nversion: \"0.1\"\nschema_version: 2\nroot_map: world_main\ntimeline_range:\n  from: 1\n  to: 100\n",
    )
    .unwrap();
    // 地图（供轨迹引用）
    std::fs::write(
        tmp.join("maps/main/index.yaml"),
        "id: world_main\nname: 主图\nlevel: world\nchildren: []\nextent:\n  width: 4096.0\n  height: 4096.0\nlayers: []\n",
    )
    .unwrap();
    // 两个单位
    std::fs::write(
        tmp.join("units/index.yaml"),
        "units: [{id: unit_a, file: unit_a.yaml}, {id: unit_b, file: unit_b.yaml}]\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("units/unit_a.yaml"),
        "id: unit_a\nname: 甲队\nmembers: []\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("units/unit_b.yaml"),
        "id: unit_b\nname: 乙队\nmembers: []\n",
    )
    .unwrap();
    // 两条轨迹：同一天（day 10）都在 (5,5) 附近
    for (tid, uid) in [("track_a", "unit_a"), ("track_b", "unit_b")] {
        std::fs::write(
            tmp.join(format!("tracks/{tid}.yaml")),
            format!(
                "id: {tid}\nunit: {uid}\nmap: world_main\nfrom: 10\nto: 11\nwaypoints:\n  - day: 10\n    pos: {{x: 5.0, y: 5.0}}\n  - day: 10\n    pos: {{x: 5.0, y: 5.1}}\n"
            ),
        )
        .unwrap();
    }

    let project = Project::open(&tmp).unwrap();
    let diags = project.diagnose().unwrap();
    let hints = diags.hints();
    assert!(
        !hints.is_empty(),
        "两个单位同地同时且未写事件 → 应产生相遇提示"
    );
    assert!(
        !diags.has_errors(),
        "相遇未写事件**永远不是错误**（AGENTS §7.5 V1），got {:?}",
        diags.items
    );
    // 提示必须带定位锚点，才能「可回访」。
    assert!(
        hints[0].anchor.is_some(),
        "相遇提示必须带定位锚点供回访：{:?}",
        hints[0]
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn encounter_hint_is_suppressed_when_an_event_already_covers_it() {
    // 同一工程，但在同一时空补一个事件 → 提示必须消失（否则最扰民）。
    let tmp = std::env::temp_dir().join("wv_encounter_covered_probe");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("timeline/events")).unwrap();
    std::fs::create_dir_all(tmp.join("maps/main")).unwrap();
    std::fs::create_dir_all(tmp.join("units")).unwrap();
    std::fs::create_dir_all(tmp.join("tracks")).unwrap();
    std::fs::create_dir_all(tmp.join("distances")).unwrap();
    // 相遇地点必须在距离图中存在（waypoints[].place 受 §7.2 引用完整性约束）。
    std::fs::write(
        tmp.join("distances/places.yaml"),
        "- id: place_meeting\n  name: 会合处\n  map: world_main\n  facilities: []\n",
    )
    .unwrap();
    std::fs::write(tmp.join("distances/edges.yaml"), "[]\n").unwrap();
    std::fs::write(tmp.join("distances/vehicles.yaml"), "[]\n").unwrap();
    std::fs::write(
        tmp.join("world.yaml"),
        "name: probe\nversion: \"0.1\"\nschema_version: 2\nroot_map: world_main\ntimeline_range:\n  from: 1\n  to: 100\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("maps/main/index.yaml"),
        "id: world_main\nname: 主图\nlevel: world\nchildren: []\nextent:\n  width: 4096.0\n  height: 4096.0\nlayers: []\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("units/index.yaml"),
        "units: [{id: unit_a, file: unit_a.yaml}, {id: unit_b, file: unit_b.yaml}]\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("units/unit_a.yaml"),
        "id: unit_a\nname: 甲队\nmembers: []\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("units/unit_b.yaml"),
        "id: unit_b\nname: 乙队\nmembers: []\n",
    )
    .unwrap();
    for (tid, uid) in [("track_a", "unit_a"), ("track_b", "unit_b")] {
        std::fs::write(
            tmp.join(format!("tracks/{tid}.yaml")),
            format!(
                "id: {tid}\nunit: {uid}\nmap: world_main\nfrom: 10\nto: 11\nwaypoints:\n  - day: 10\n    pos: {{x: 5.0, y: 5.0}}\n    place: place_meeting\n"
            ),
        )
        .unwrap();
    }
    // 相遇处已写事件 → 应抑制提示。
    std::fs::write(
        tmp.join("timeline/events/evt_meet.yaml"),
        "id: evt_meet\nname: 会面\nfrom: 10\nto: 11\nplace: place_meeting\nparticipants: []\n",
    )
    .unwrap();

    let project = Project::open(&tmp).unwrap();
    let diags = project.diagnose().unwrap();
    assert!(
        diags.hints().is_empty(),
        "相遇处已写事件 → 不应再提示；got {:?}",
        diags.items
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
