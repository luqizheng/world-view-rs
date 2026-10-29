//! 测量线/矩形/清除（fixture：m_line/m_rect/m_kind）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;
use wv_core::MeasurementKind;

fn m_line(id: &str) -> Measurement {
    Measurement::polyline(vec![
        wv_core::Point::new(0.0, 0.0),
        wv_core::Point::new(10.0, 0.0),
        wv_core::Point::new(10.0, 5.0),
    ])
    .with_id(Id::new_unchecked(id))
}

fn m_rect(id: &str) -> Measurement {
    Measurement::rectangle(
        wv_core::Point::new(0.0, 0.0),
        wv_core::Point::new(20.0, 10.0),
    )
    .with_id(Id::new_unchecked(id))
}

fn m_kind(m: &Measurement) -> MeasurementKind {
    m.kind
}

#[test]
fn add_measurement_pushes_and_reports_dirty_path() {
    let mut w = World::default();
    let cmd = AddMeasurementCommand::new(Id::new_unchecked("world_main"), m_line("m1"));
    let dirty = cmd.apply(&mut w).unwrap();
    assert_eq!(
        dirty,
        vec!["measurements/world_main".to_string()],
        "脏路径应为虚拟命名空间 measurements/<map>"
    );
    let overlay = w.measurement_of(&Id::new_unchecked("world_main")).unwrap();
    assert_eq!(overlay.len(), 1);
    assert_eq!(overlay.measurements[0].id.as_str(), "m1");
    assert_eq!(m_kind(&overlay.measurements[0]), MeasurementKind::Polyline);

    cmd.revert(&mut w).unwrap();
    let overlay = w.measurement_of(&Id::new_unchecked("world_main")).unwrap();
    assert!(overlay.is_empty(), "revert 后该图 overlay 应清空");
}

#[test]
fn add_measurement_rejects_duplicate_id() {
    let mut w = World::default();
    AddMeasurementCommand::new(Id::new_unchecked("m1"), m_line("m1"))
        .apply(&mut w)
        .unwrap();
    // 同 ID 二次添加 = Precondition 错。
    let err = AddMeasurementCommand::new(Id::new_unchecked("m1"), m_rect("m1")).apply(&mut w);
    assert!(matches!(err, Err(EditError::Precondition(_))));
    // overlay 长度仍是 1（失败的 apply 不得增改）
    assert_eq!(w.measurement_of(&Id::new_unchecked("m1")).unwrap().len(), 1);
}

#[test]
fn add_measurement_revert_before_apply_is_precondition() {
    let mut w = World::default();
    let cmd = AddMeasurementCommand::new(Id::new_unchecked("m1"), m_line("m1"));
    assert!(matches!(
        cmd.revert(&mut w),
        Err(EditError::Precondition(_))
    ));
}

#[test]
fn remove_measurement_then_revert_restores_at_same_index() {
    let mut w = World::default();
    let map = Id::new_unchecked("world_main");
    AddMeasurementCommand::new(map.clone(), m_line("m1"))
        .apply(&mut w)
        .unwrap();
    AddMeasurementCommand::new(map.clone(), m_rect("m2"))
        .apply(&mut w)
        .unwrap();
    AddMeasurementCommand::new(map.clone(), m_line("m3"))
        .apply(&mut w)
        .unwrap();
    assert_eq!(w.measurement_of(&map).unwrap().len(), 3);

    // 删中间那条 m2。
    let rm = RemoveMeasurementCommand::new(map.clone(), Id::new_unchecked("m2"));
    let dirty = rm.apply(&mut w).unwrap();
    assert_eq!(dirty, vec!["measurements/world_main".to_string()]);
    let overlay = w.measurement_of(&map).unwrap();
    assert_eq!(overlay.len(), 2);
    assert!(overlay.find(&Id::new_unchecked("m2")).is_none());

    rm.revert(&mut w).unwrap();
    let overlay = w.measurement_of(&map).unwrap();
    assert_eq!(overlay.len(), 3);
    assert_eq!(
        overlay.measurements[1].id.as_str(),
        "m2",
        "应按原位置 1 还原"
    );
    assert_eq!(
        m_kind(&overlay.measurements[1]),
        MeasurementKind::Rectangle,
        "还原的应是矩形那条"
    );
}

#[test]
fn remove_measurement_missing_is_target_error() {
    let mut w = World::default();
    let cmd =
        RemoveMeasurementCommand::new(Id::new_unchecked("world_main"), Id::new_unchecked("ghost"));
    assert!(matches!(
        cmd.apply(&mut w),
        Err(EditError::TargetMissing(_))
    ));
}

#[test]
fn clear_measurements_drops_and_revert_restores_full_overlay() {
    let mut w = World::default();
    let map = Id::new_unchecked("world_main");
    AddMeasurementCommand::new(map.clone(), m_line("m1"))
        .apply(&mut w)
        .unwrap();
    AddMeasurementCommand::new(map.clone(), m_rect("m2"))
        .apply(&mut w)
        .unwrap();
    let before = w.measurement_of(&map).unwrap().measurements.clone();

    let cmd = ClearMeasurementsCommand::new(map.clone());
    let dirty = cmd.apply(&mut w).unwrap();
    assert!(!dirty.is_empty(), "非空→空 应记脏路径");
    assert!(w.measurement_of(&map).unwrap().is_empty());

    cmd.revert(&mut w).unwrap();
    let overlay = w.measurement_of(&map).unwrap();
    assert_eq!(overlay.measurements, before, "整份还原");
}

#[test]
fn clear_measurements_on_empty_or_missing_overlay_reports_no_dirty() {
    let mut w = World::default();
    let map = Id::new_unchecked("world_main");
    // 第一次：根本没 overlay（连 BTreeMap 键都没）→ apply 应记空脏路径。
    let cmd = ClearMeasurementsCommand::new(map.clone());
    let dirty = cmd.apply(&mut w).unwrap();
    assert!(dirty.is_empty(), "原本就空→应不写盘");
    // revert 也应合法（缓存的「None」可还原）。
    cmd.revert(&mut w).unwrap();

    // 第二次：键存在但内部空 → apply 空。revert 不报错。
    w.measurement_of_mut(&map);
    let cmd = ClearMeasurementsCommand::new(map.clone());
    let dirty = cmd.apply(&mut w).unwrap();
    assert!(dirty.is_empty());
    cmd.revert(&mut w).unwrap();
}

#[test]
fn three_measurement_commands_undo_independently() {
    let mut w = World::default();
    let map = Id::new_unchecked("world_main");
    let a = AddMeasurementCommand::new(map.clone(), m_line("m1"));
    let b = AddMeasurementCommand::new(map.clone(), m_rect("m2"));
    a.apply(&mut w).unwrap();
    b.apply(&mut w).unwrap();
    assert_eq!(w.measurement_of(&map).unwrap().len(), 2);

    b.revert(&mut w).unwrap();
    assert_eq!(w.measurement_of(&map).unwrap().len(), 1);
    a.revert(&mut w).unwrap();
    // 两个 revert 之后：BTreeMap 键里仍留有该键，但 measurements 为空。
    let overlay = w.measurement_of(&map).unwrap();
    assert!(overlay.is_empty(), "overlay 应留空文档兜底");
}

#[test]
fn measurement_dirty_path_does_not_collide_with_streets_or_maps() {
    // 关键不变量：脏路径不与 streets/、maps/ 共享前缀 → 不应误触发
    // terrain.bin / index.yaml / roads.yaml 重写。
    let mut w = World::default();
    let map = Id::new_unchecked("world_main");
    AddMeasurementCommand::new(map.clone(), m_line("m1"))
        .apply(&mut w)
        .unwrap();
    let dirty_paths = RemoveMeasurementCommand::new(map.clone(), Id::new_unchecked("m1"))
        .apply(&mut w)
        .unwrap();
    assert!(dirty_paths.iter().all(|p| p.starts_with("measurements/")));
    assert!(!dirty_paths.iter().any(|p| p.starts_with("maps/")));
    assert!(!dirty_paths.iter().any(|p| p.starts_with("streets/")));
}

// ── Region 命令测试 ────────────────────────────────────────────────────

use wv_core::Region;

fn sample_region(id: &str, name: &str) -> Region {
    Region {
        id: Id::new_unchecked(id),
        name: name.into(),
        map: Id::new_unchecked(TEST_MAP),
        polygon: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
        // ADR-0045 D3：洞环 / 类别 / 父区 / 覆盖物键。
        holes: Vec::new(),
        kind: wv_core::model::region::RegionKind::Area,
        parent: None,
        overlay: None,
    }
}

#[test]
fn add_region_pushes_to_world_and_reverts() {
    let mut w = World::default();
    let cmd = AddRegionCommand::new(sample_region("r1", "关中平原"));
    let dirty = cmd.apply(&mut w).unwrap();
    assert_eq!(w.regions.len(), 1);
    assert_eq!(w.regions[0].id.as_str(), "r1");
    assert!(dirty.iter().any(|p| p == "regions"));

    cmd.revert(&mut w).unwrap();
    assert!(w.regions.is_empty(), "revert 应移除区域");
}

#[test]
fn add_region_rejects_duplicate_id() {
    let mut w = World::default();
    w.regions.push(sample_region("r1", "A"));
    let cmd = AddRegionCommand::new(sample_region("r1", "B"));
    assert!(
        cmd.apply(&mut w).is_err(),
        "重复 ID 应返回 Precondition 错误"
    );
}

#[test]
fn remove_region_removes_and_reverts_to_original_position() {
    let mut w = World::default();
    w.regions.push(sample_region("r0", "先"));
    w.regions.push(sample_region("r1", "目标"));
    w.regions.push(sample_region("r2", "后"));

    let cmd = RemoveRegionCommand::new(Id::new_unchecked("r1"));
    cmd.apply(&mut w).unwrap();
    assert_eq!(w.regions.len(), 2);
    assert_eq!(w.regions[0].id.as_str(), "r0");
    assert_eq!(w.regions[1].id.as_str(), "r2");

    cmd.revert(&mut w).unwrap();
    assert_eq!(w.regions.len(), 3);
    assert_eq!(w.regions[1].id.as_str(), "r1", "应恢复到原位置");
    assert_eq!(w.regions[1].name, "目标");
}

#[test]
fn remove_region_on_missing_id_fails() {
    let mut w = World::default();
    let cmd = RemoveRegionCommand::new(Id::new_unchecked("ghost"));
    assert!(cmd.apply(&mut w).is_err(), "不存在的 ID 应报 TargetMissing");
}

#[test]
fn update_region_changes_name_and_polygon_and_reverts() {
    let mut w = World::default();
    w.regions.push(sample_region("r1", "旧名"));

    let cmd = UpdateRegionCommand::new(
        Id::new_unchecked("r1"),
        "新名".into(),
        vec![0.0, 0.0, 5.0, 0.0, 5.0, 5.0],
    );
    cmd.apply(&mut w).unwrap();
    assert_eq!(w.regions[0].name, "新名");
    assert_eq!(w.regions[0].polygon, vec![0.0, 0.0, 5.0, 0.0, 5.0, 5.0]);

    cmd.revert(&mut w).unwrap();
    assert_eq!(w.regions[0].name, "旧名");
    assert_eq!(
        w.regions[0].polygon,
        vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
        "revert 应恢复原始 polygon"
    );
}

#[test]
fn update_region_on_missing_id_fails() {
    let mut w = World::default();
    let cmd = UpdateRegionCommand::new(Id::new_unchecked("ghost"), "X".into(), vec![]);
    assert!(cmd.apply(&mut w).is_err());
}

#[test]
fn region_dirty_path_is_regions_prefix() {
    let mut w = World::default();
    let dirty = AddRegionCommand::new(sample_region("r1", "R"))
        .apply(&mut w)
        .unwrap();
    assert!(dirty.iter().all(|p| p.starts_with("regions")));
    assert!(!dirty.iter().any(|p| p.starts_with("maps/")));
    assert!(!dirty.iter().any(|p| p.starts_with("geometry/")));
}
