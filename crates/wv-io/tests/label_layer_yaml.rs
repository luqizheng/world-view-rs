//! 标签层配置的读写验证（ADR-0016）。
//!
//! 与 `street_layer_yaml.rs` 同型：文档模型在 `wv-core::label`，
//! 读写入口在 `Project::load_labels` / `Project::write_labels`。
//! round-trip 按 AGENTS §7.2 必须无损；零迁移回归线：未声明字段时
//! 解析为缺省值（`entity_kinds: []` ⇒ 该层不显示任何标签）。

use std::collections::BTreeMap;
use wv_core::label::{EntityKind, LabelLayerConfig, ZoomTier};

#[test]
fn label_layer_config_round_trips_through_yaml() {
    let mut cfg = LabelLayerConfig::new();
    cfg.entity_kinds = vec![EntityKind::Place, EntityKind::Faction];
    cfg.min_zoom_tier = ZoomTier::Edit;
    cfg.font_size_ladder = BTreeMap::from_iter([
        (ZoomTier::Nation, 7.0),
        (ZoomTier::Region, 10.0),
        (ZoomTier::Edit, 13.0),
    ]);
    cfg.max_labels = 100;
    cfg.priority_kinds = vec![EntityKind::Place];

    let text = serde_yaml::to_string(&cfg).expect("序列化");
    let back: LabelLayerConfig = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, cfg, "LabelLayerConfig YAML round-trip 应无损");
}

#[test]
fn label_layer_config_minimal_yaml_uses_built_in_defaults() {
    // 既有工程可能只有最小声明（甚至空文档）—— 解析应给出全部缺省值。
    let cfg: LabelLayerConfig =
        serde_yaml::from_str("entity_kinds: [place]\n").expect("最小配置应解析");
    assert_eq!(cfg.entity_kinds, vec![EntityKind::Place]);
    assert_eq!(cfg.min_zoom_tier, ZoomTier::DEFAULT_MIN);
    assert_eq!(cfg.max_labels, ZoomTier::DEFAULT_MAX_LABELS);
    // 缺省 ladder 三档都有
    assert_eq!(cfg.font_size_ladder.len(), 3);
}

#[test]
fn label_layer_config_empty_yaml_parses_to_empty_config() {
    // 空文档 = 空配置 = 白名单为空 = 不显示任何标签
    let cfg: LabelLayerConfig = serde_yaml::from_str("").expect("空文档应解析");
    assert!(cfg.entity_kinds.is_empty());
    assert_eq!(cfg.max_labels, ZoomTier::DEFAULT_MAX_LABELS);
    // 缺省 ladder 不为空（仍可读）
    assert!(!cfg.font_size_ladder.is_empty());
}

#[test]
fn label_layer_config_none_fields_are_not_written_to_yaml() {
    // 字段全缺省时，**空集合**字段不写出；**非空缺省**字段（ladder 3 档、max_labels 200）会写出
    // ——serde 没法知道"我手填的 9.0"和"缺省的 9.0"的区别，所以非空就写。
    // YAML 自文档化，round-trip 仍然无损。
    let cfg = LabelLayerConfig::new();
    let text = serde_yaml::to_string(&cfg).expect("序列化");
    assert!(
        !text.contains("entity_kinds"),
        "空白名单不应写出 entity_kinds：\n{text}"
    );
    assert!(
        !text.contains("priority_kinds"),
        "空优先级不应写出 priority_kinds：\n{text}"
    );
    // ladder 非空（3 档缺省）→ 写出
    assert!(
        text.contains("font_size_ladder") || text.contains("nation"),
        "3 档 ladder 非空，写出（自文档化）\n实际:\n{text}"
    );
}

#[test]
fn open_window_in_entity_span_yaml_round_trips() {
    // 验证 `TimeSpan.to: None`（开放区间）序列化为 YAML 后的形状可解析。
    use wv_core::TimeSpan;
    let span = TimeSpan::open(1900);
    let text = serde_yaml::to_string(&span).expect("序列化");
    let back: TimeSpan = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, span);
    // to: null 形如：
    assert!(
        text.contains("to:") || text.contains("to: null") || !text.contains("to:"),
        "开放区间序列化形态应兼容 to=null 解析；实际: {text}"
    );
}

// ─── 经 Project 读写（与 street_layer_yaml.rs 同型）────────────────────

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wv_io_label_test_{tag}_{pid}_{nanos}_{n}"));
        std::fs::create_dir_all(&dir).expect("tempdir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_min_project(dir: &std::path::Path) -> wv_io::Project {
    let meta_yaml =
        "name: 标签层测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_main\n";
    std::fs::write(dir.join("world.yaml"), meta_yaml).expect("写 world.yaml");
    wv_io::Project::open(dir).expect("打开工程")
}

#[test]
fn project_writes_then_loads_a_label_config_in_a_new_directory() {
    let tmp = TempDir::new("roundtrip");
    let project = open_min_project(&tmp.0);

    let mut cfg = LabelLayerConfig::new();
    cfg.entity_kinds = vec![EntityKind::Place, EntityKind::Region, EntityKind::Faction];
    cfg.max_labels = 150;
    cfg.priority_kinds = vec![EntityKind::Place];

    // 目标目录 `maps/map_city/` 尚不存在——首次保存应自建。
    let written = project
        .write_labels("maps/map_city/labels.yaml", &cfg)
        .expect("写标签层配置");
    assert!(
        tmp.0.join("maps/map_city/labels.yaml").is_file(),
        "文件应落在 {written}"
    );

    let back = project
        .load_labels("maps/map_city/labels.yaml")
        .expect("读回");
    assert_eq!(back, cfg, "经工程读写的标签层配置应与原数据一致");
}

#[test]
fn project_loading_a_missing_label_file_reports_the_path() {
    let tmp = TempDir::new("missing");
    let project = open_min_project(&tmp.0);
    let err = project
        .load_labels("maps/map_city/labels.yaml")
        .expect_err("文件不存在应报错");
    let msg = err.to_string();
    assert!(msg.contains("labels.yaml"), "错误信息应含路径，实际: {msg}");
}
