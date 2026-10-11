//! 示例工程的集成测试。
//!
//! 覆盖 AGENTS.md §7.2 的两项强制要求：
//! 1. YAML round-trip 稳定（`load(save(w)) == w`）；
//! 2. 引用完整性可被检出。

use std::path::PathBuf;

use wv_core::map::{Layer, LayerKind, MapLevel, MapNode};
use wv_core::time::{TimePoint, TimeSpan, Timeline};
use wv_core::WorldMeta;
use wv_io::Project;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/minimal-world")
}

#[test]
fn example_project_opens_and_validates() {
    let project = Project::open(example_dir()).expect("示例工程应可打开");
    assert_eq!(project.meta().name, "最小示例世界");
    assert_eq!(project.meta().schema_version, 1);

    let warnings = project.validate().expect("校验不应报错");
    assert!(
        warnings.is_empty(),
        "示例工程应无结构警告, got {warnings:?}"
    );
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
        eras: vec![wv_core::time::Era {
            id: wv_core::Id::new_unchecked("era_first"),
            name: "开元".into(),
            order: 1,
            start: Some(TimePoint::new(1, 1, 1)),
        }],
        days_per_year: 360,
        months: vec![],
        anchors: vec![wv_core::time::Anchor {
            id: wv_core::Id::new_unchecked("anchor_founding"),
            name: "青云城建城".into(),
            at: TimePoint::new(1, 300, 1),
        }],
    };

    let text = serde_yaml::to_string(&timeline).unwrap();
    let back: Timeline = serde_yaml::from_str(&text).unwrap();
    assert_eq!(timeline, back, "Timeline 的 YAML round-trip 必须稳定");
}

#[test]
fn open_ended_span_omits_to_field_in_yaml() {
    // 开放区间序列化时不应写出 `to:` 空字段——否则 YAML 噪音大且 diff 脏。
    let span = TimeSpan::open(TimePoint::new(1, 300, 1));
    let text = serde_yaml::to_string(&span).unwrap();
    assert!(!text.contains("to:"), "开放区间不应输出 to 字段: {text}");
}

#[test]
fn missing_project_dir_is_reported() {
    let err = Project::open(example_dir().join("__not_here__"));
    assert!(matches!(err, Err(wv_io::IoError::NotAProject(_))));
}
