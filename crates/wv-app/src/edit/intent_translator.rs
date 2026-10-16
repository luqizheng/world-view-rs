//! 编辑意图翻译器（ADR-0020 §2.5）：渲染层的「想做什么」→ `wv-edit` 的命令提交。
//!
//! `wv-render` 不持有 `EditHistory`、不构造 `EditCommand`（分层红线，AGENTS §2.1）。
//! 本系统是唯一翻译点：排空 [`EditIntents`] → 构造命令 → `History.commit` →
//! 成功者写 [`EditReceipts`] 回执 + `projection.dirty`。
//!
//! ## 单帧时序（app_bootstrap 用 `.chain()` 钉死）
//!
//! `commit_street_click` / `commit_measurement_click`（wv-render，产意图）
//! → **本系统**（翻译 + 提交 + 回执）
//! → `apply_street_receipts` / `apply_measurement_receipts`（wv-render，回填本地）。

use bevy::prelude::*;
use wv_edit::{AddMeasurementCommand, AddStreetSegmentCommand, RemoveMeasurementCommand};
use wv_render::editing_bridge::{
    EditIntents, EditReceipts, MeasurementIntent, MeasurementOp, StreetSegmentIntent,
};
use wv_render::state::WorldProjection;

use crate::edit::editing;

/// 排空渲染层编辑意图 → 构造 `EditCommand` → `History.commit` → 回执。
///
/// 提交失败时只记开发者日志、**不**回执——渲染侧「失败不动本地」的语义
/// 由回执缺失表达（与拆 crate 前的 `match Err` 分支逐位等价）。
pub fn translate_edit_intents(
    mut intents: ResMut<EditIntents>,
    mut receipts: ResMut<EditReceipts>,
    mut edit: ResMut<editing::EditHistory>,
    mut projection: ResMut<WorldProjection>,
) {
    // 冲掉上一帧残余（正常情况下回执同帧即被取走；此处是防御）。
    receipts.clear();

    for (seq, StreetSegmentIntent { layer_id, segment }) in intents.drain_streets() {
        let cmd = AddStreetSegmentCommand::new(layer_id, segment);
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(cmd));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                receipts.commit(seq);
            }
            Err(e) => bevy::log::warn!("街道编辑命令失败: {e}"),
        }
    }

    for (seq, MeasurementIntent { map_id, op }) in intents.drain_measurements() {
        let cmd: Box<dyn wv_edit::EditCommand> = match op {
            MeasurementOp::Add(m) => Box::new(AddMeasurementCommand::new(map_id, m)),
            MeasurementOp::Remove(id) => Box::new(RemoveMeasurementCommand::new(map_id, id)),
        };
        let mut tx = wv_edit::Transaction::new();
        tx.push(cmd);
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                receipts.commit(seq);
            }
            Err(e) => bevy::log::warn!("测量命令失败: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「意图 → History 提交 → dirty_set → 回执」那半段的闭环测试
    /// （渲染侧半段见 `wv_render::streets` 的
    /// `street_clicks_emit_intents_and_receipts_advance_local_state`）。
    #[test]
    fn street_intent_commits_through_history_and_marks_dirty() {
        let world = wv_core::World {
            maps: vec![wv_core::map::MapNode {
                id: wv_core::Id::new_unchecked("map_city"),
                name: "城市".into(),
                level: wv_core::map::MapLevel::World,
                parent: None,
                children: Vec::new(),
                icons: Vec::new(),
                layers: vec![wv_core::map::Layer {
                    id: wv_core::Id::new_unchecked("roads"),
                    name: "街道".into(),
                    kind: wv_core::map::LayerKind::Road,
                    visible: true,
                    locked: false,
                    opacity: 1.0,
                    z: 0,
                    source: Some("maps/city/roads.yaml".into()),
                }],
                extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
                raster: None,
                geographic_source: None,
                time_span: None,
                unit_per_km: None,
                orientation: 0.0,
                base: None,
                outline_polygon: None,
            }],
            ..wv_core::World::default()
        };

        let mut app_world = bevy::prelude::World::default();
        app_world.insert_resource(WorldProjection {
            world,
            project_dir: Some("/test/project".into()),
            ..WorldProjection::default()
        });
        app_world.insert_resource(editing::EditHistory::default());
        let mut intents = EditIntents::default();
        let seq = intents.push_street(StreetSegmentIntent {
            layer_id: wv_core::Id::new_unchecked("roads"),
            segment: wv_core::StreetSegment::straight(
                wv_core::RoadClass::Primary,
                100.0,
                200.0,
                150.0,
                180.0,
            ),
        });
        app_world.insert_resource(intents);
        app_world.insert_resource(EditReceipts::default());

        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems(translate_edit_intents);
        schedule.run(&mut app_world);

        // 段落进世界。
        let projection = app_world.resource::<WorldProjection>();
        let doc = projection
            .world
            .street_of(&wv_core::Id::new_unchecked("roads"))
            .expect("翻译器应经命令建出街道文档");
        assert_eq!(doc.len(), 1, "一条意图应落 1 段，实际 {}", doc.len());
        let s = &doc.segments[0];
        assert_eq!(s.points[0], wv_core::geometry::Point::new(100.0, 200.0));
        assert_eq!(s.points[1], wv_core::geometry::Point::new(150.0, 180.0));

        // 脏路径与脏标记。
        let edit = app_world.resource::<editing::EditHistory>();
        assert!(
            edit.dirty_set.contains("streets/roads"),
            "脏路径应进 dirty_set，实际 {:?}",
            edit.dirty_set
        );
        assert!(projection.dirty, "成功提交应点亮 projection.dirty");

        // 回执。
        let mut receipts = app_world.resource_mut::<EditReceipts>();
        assert!(receipts.take(seq), "成功提交应回执 seq");
        assert!(!receipts.take(seq), "回执只应被消费一次");
    }

    /// 失败意图不回执——渲染侧据此不动本地状态。
    ///
    /// 「失败」用例选**删除不存在的 measurement**（命令内 position 失配 →
    /// ）：Add 系命令按 V1 守约不拒收缺失图层（第一笔即建文档），
    /// 不能当失败用例。
    #[test]
    fn failing_intent_is_not_receipted() {
        let mut app_world = bevy::prelude::World::default();
        app_world.insert_resource(WorldProjection::default());
        app_world.insert_resource(editing::EditHistory::default());
        let mut intents = EditIntents::default();
        let seq = intents.push_measurement(MeasurementIntent {
            map_id: wv_core::Id::new_unchecked("map_city"),
            op: MeasurementOp::Remove(wv_core::Id::new_unchecked("no_such_measurement")),
        });
        app_world.insert_resource(intents);
        app_world.insert_resource(EditReceipts::default());

        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems(translate_edit_intents);
        schedule.run(&mut app_world);

        let mut receipts = app_world.resource_mut::<EditReceipts>();
        assert!(!receipts.take(seq), "失败意图不得回执");
        assert!(
            !app_world.resource::<WorldProjection>().dirty,
            "失败不得点亮脏标记"
        );
    }
}
