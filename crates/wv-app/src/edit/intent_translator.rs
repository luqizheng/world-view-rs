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
use wv_core::distance::Place;
use wv_edit::{
    AddMeasurementCommand, AddPlaceCommand, AddPlaceLineCommand, AddStreetSegmentCommand,
    RemoveMeasurementCommand,
};
use wv_render::editing_bridge::{
    EditIntents, EditReceipts, MeasurementIntent, MeasurementOp, PlaceLineIntent,
    StreetSegmentIntent,
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
    screen: Res<crate::app_screen::AppScreen>,
) {
    // 冲掉上一帧残余（正常情况下回执同帧即被取走；此处是防御）。
    receipts.clear();
    // 加载期间丢弃全部意图（2026-10-22 C 档）：世界即将被替换，此刻提交的
    // 命令会随旧世界一起消失，且**不能**留到加载完再补——那时它们会落到
    // 新世界上（错的图、错的点）。
    if screen.shows_loading_overlay() {
        // 三类意图全排空（`EditIntents` 没有 `clear`，只有分类型 drain）。
        intents.drain_streets();
        intents.drain_place_lines();
        intents.drain_measurements();
        return;
    }

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

    // 作者手绘小河（ADR-0046）：**一个事务两条命令**——先建地点、再挂折线，
    // 一次撤销整体回退（作者不会看到「有地点没线」的中间态）。
    for (seq, PlaceLineIntent { map_id, name, line }) in intents.drain_place_lines() {
        let place_id = wv_core::Id::new_unchecked(format!("place_river_{}", now_millis()));
        let place = Place {
            id: place_id.clone(),
            name: if name.is_empty() {
                "小河".to_string()
            } else {
                name
            },
            feature: None,
            map: Some(map_id),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: None,
        };
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(AddPlaceCommand::new(place)));
        tx.push(Box::new(AddPlaceLineCommand::new(place_id, line)));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                receipts.commit(seq);
            }
            Err(e) => bevy::log::warn!("作者小河命令失败: {e}"),
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

/// 毫秒时间戳（地点 id 后缀；id 由**应用层**生成——渲染层不造 id）。
fn now_millis() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod place_line_tests {
    use super::*;

    /// 小河意图 = **一个事务两条命令**：撤销一次，地点与折线一起回退。
    #[test]
    fn place_line_intent_creates_place_with_line_and_undo_reverts_both() {
        let world = wv_core::World::default();
        let mut app_world = bevy::prelude::World::default();
        app_world.insert_resource(WorldProjection {
            world,
            project_dir: Some("/test/project".into()),
            ..WorldProjection::default()
        });
        app_world.insert_resource(editing::EditHistory::default());
        let mut intents = EditIntents::default();
        intents.push_place_line(PlaceLineIntent {
            map_id: wv_core::Id::new_unchecked("map_world"),
            name: "通天河".into(),
            line: vec![0.0, 0.0, 10.0, 5.0],
        });
        app_world.insert_resource(intents);
        app_world.insert_resource(EditReceipts::default());
        app_world.insert_resource(crate::app_screen::AppScreen::default());

        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems(translate_edit_intents);
        schedule.run(&mut app_world);

        let projection = app_world.resource::<WorldProjection>();
        assert_eq!(projection.world.distances.places.len(), 1, "应建一个地点");
        let place = &projection.world.distances.places[0];
        assert_eq!(place.name, "通天河");
        assert!(place.line.is_some(), "地点应带折线");
        assert_eq!(place.line.as_deref(), Some(&[0.0, 0.0, 10.0, 5.0][..]));
        assert!(projection.dirty, "提交后应标脏");

        // 撤销一次：地点整体消失（地点 + 折线同一事务）。
        {
            let mut edit = app_world
                .remove_resource::<editing::EditHistory>()
                .expect("历史");
            let mut projection = app_world
                .remove_resource::<WorldProjection>()
                .expect("投影");
            assert!(edit.history.undo(&mut projection.world).is_ok(), "应可撤销");
            app_world.insert_resource(projection);
            app_world.insert_resource(edit);
        }
        let projection = app_world.resource::<WorldProjection>();
        assert!(
            projection.world.distances.places.is_empty(),
            "撤销后地点（连同折线）应整体消失"
        );
    }

    /// 空名 → 默认名「小河」。
    #[test]
    fn place_line_intent_with_empty_name_gets_default() {
        let world = wv_core::World::default();
        let mut app_world = bevy::prelude::World::default();
        app_world.insert_resource(WorldProjection {
            world,
            project_dir: Some("/test/project".into()),
            ..WorldProjection::default()
        });
        app_world.insert_resource(editing::EditHistory::default());
        let mut intents = EditIntents::default();
        intents.push_place_line(PlaceLineIntent {
            map_id: wv_core::Id::new_unchecked("map_world"),
            name: String::new(),
            line: vec![0.0, 0.0, 1.0, 1.0],
        });
        app_world.insert_resource(intents);
        app_world.insert_resource(EditReceipts::default());
        app_world.insert_resource(crate::app_screen::AppScreen::default());

        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems(translate_edit_intents);
        schedule.run(&mut app_world);

        let projection = app_world.resource::<WorldProjection>();
        assert_eq!(projection.world.distances.places[0].name, "小河");
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
        app_world.insert_resource(crate::app_screen::AppScreen::default());

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
        app_world.insert_resource(crate::app_screen::AppScreen::default());

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
