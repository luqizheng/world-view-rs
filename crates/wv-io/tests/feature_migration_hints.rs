//! 词汇迁移提示（ADR-0032 D3/D7）：Place 仍携带旧 `feature` 字段时的
//! 「已迁移 / 未映射」两条提示级诊断（V1：只提示，不阻止保存）。
//!
//! 提示的生命周期由数据驱动：加载迁移把群系写入 `biome` 后，feature 原值
//! 作为证据驻留内存 → 提示在场；作者首次保存 → feature 从 YAML 消失 →
//! 重载后提示自然消散（指纹可认领，跨会话稳定）。

use wv_core::distance::Place;
use wv_core::{Id, World};
use wv_io::diagnostics::{derive_hints, DiagLevel};

fn id(s: &str) -> Id {
    Id::new_unchecked(s)
}

fn place_with(feature: Option<u8>, biome: Option<u8>) -> Place {
    Place {
        kind: None,
        line: None,
        id: id("place_legacy"),
        name: "临安".into(),
        feature,
        biome,
        envelope: None,
        map: None,
        x: None,
        y: None,
        w: None,
        h: None,
        rank: None,
        line_kind: None,
        line_width_m: None,
    }
}

fn world_with(place: Place) -> World {
    let mut w = World::default();
    w.distances.places.push(place);
    w
}

/// 旧工程加载后的典型形态（biome=Some(1)，feature=Some(4) 证据驻留）：
/// 应发 feature-migrated 提示；保存后（feature 消失）提示随之消失。
#[test]
fn migrated_feature_hints_until_first_save_then_dissipates() {
    let loaded = world_with(place_with(Some(4), Some(1)));
    let d = derive_hints(&loaded);
    let hints: Vec<_> = d
        .items
        .iter()
        .filter(|x| x.fingerprint == "feature-migrated|place_legacy")
        .collect();
    assert_eq!(hints.len(), 1, "加载迁移后应恰有 1 条迁移提示");
    assert_eq!(hints[0].level, DiagLevel::Hint, "迁移提示不得是错误（V1）");
    assert!(
        hints[0].message.contains("临安"),
        "文案应含地点名：got {}",
        hints[0].message
    );
    assert!(
        hints[0].message.contains("biome=1"),
        "文案应以迁移目标群系为准：got {}",
        hints[0].message
    );
    assert!(
        hints[0].message.contains("保存"),
        "文案应声明不阻止保存（V1）：got {}",
        hints[0].message
    );

    // 首次保存后（模拟：旧字段从磁盘消失，重载内存不再携带）。
    let saved = world_with(place_with(None, Some(1)));
    let d = derive_hints(&saved);
    assert!(
        !d.items
            .iter()
            .any(|x| x.fingerprint.starts_with("feature-")),
        "保存收敛后迁移/未映射提示都应消失，实得 {:?}",
        d.items
    );
}

/// 手工构造的 feature-only 世界（未经加载器）：映射目标仍由诊断层兜底给出。
#[test]
fn feature_without_biome_still_hints_migration_target() {
    let w = world_with(place_with(Some(4), None));
    let d = derive_hints(&w);
    let h = d
        .items
        .iter()
        .find(|x| x.fingerprint == "feature-migrated|place_legacy")
        .expect("feature=4 应发迁移提示");
    assert!(
        h.message.contains("biome=1"),
        "森林应映射为 biome=1：got {}",
        h.message
    );
}

/// 词汇表外的垃圾值（如 42）→ feature-unmapped 提示，按普通地点处理。
#[test]
fn unmapped_feature_value_emits_unmapped_hint() {
    let w = world_with(place_with(Some(42), None));
    let d = derive_hints(&w);
    let h = d
        .items
        .iter()
        .find(|x| x.fingerprint == "feature-unmapped|place_legacy")
        .expect("垃圾值应发未映射提示");
    assert_eq!(h.level, DiagLevel::Hint);
    assert!(
        h.message.contains("42"),
        "文案应含实际值：got {}",
        h.message
    );

    // 普通地点（无 feature）不发任何迁移提示。
    let plain = world_with(place_with(None, None));
    assert!(
        derive_hints(&plain)
            .items
            .iter()
            .all(|x| !x.fingerprint.starts_with("feature-")),
        "无旧字段的地点不应被提示"
    );
}
