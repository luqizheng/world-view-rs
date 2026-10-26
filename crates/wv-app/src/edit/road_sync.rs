//! 推导路网边的应用侧收口与加载期线数据迁移（ADR-0070 D5 / §4.6）。
//!
//! ## 为什么是普通函数而不是 Bevy System
//!
//! AGENTS §3.3：System 不得直接改世界数据。推导边重建写的是
//! `world.distances.edges`——虽是运行时投影（`derived` 永不落盘），仍按
//! 同一纪律收口在编辑层：由**编辑提交点**（`History::commit` 之后）、
//! **撤销/重做出口**（`apply_history_command_detailed`）与**加载完成点**
//! （`apply_loaded_world`）显式调用，改动时机与编辑事务一一对应，
//! 不存在「每帧偷偷重算」的隐式路径。
//!
//! ## 三个调用点（评审裁定）
//!
//! 1. **加载完成后**：`apply_loaded_world`——迁移（若需要）之后重建一次，
//!    新工程首帧即有正确路网投影；
//! 2. **每次 `History::commit` 之后**：`edit/*` 各编辑提交点（含面板写回
//!    ——它们与命令提交是同一条链）；
//! 3. **撤销 / 重做之后**：`apply_history_command_detailed`——`History::undo`
//!    走 `cmd.revert`、`redo` 走事务重放，地点与手写边随之回退/重放，
//!    推导边必须跟着重建，否则停在上一状态的陈旧投影上。
//!
//! ## 不进脏路径（评审裁定 4）
//!
//! 推导边是投影：改了 `world.distances.edges` 也**不得**
//! `record_dirty(["distances/edges"])`——derived 被 serde skip，永不落盘，
//! 标脏只会造成 `edges.yaml` 的无意义重写。迁移命令自己返回的
//! `distances/edges` 脏路径不受此约束（它删的是作者手写的边，必须落盘）。

#![cfg(feature = "app")]

use std::sync::Arc;

use wv_core::map::road_derive::{rebuild_derived_edges, RoadDeriveReport, ROAD_SNAP_TOLERANCE_WU};
use wv_core::World;
use wv_edit::command::needs_line_migration;
use wv_edit::command::{EditCommand, EditResult, MigrateLegacyLineDataCommand, MigrationReport};
use wv_edit::history::Transaction;

use crate::edit::editing::EditHistory;

/// 推导路网边重建的**唯一入口**（ADR-0070 D5）：全清场上推导边再重算。
///
/// 幂等（重复调用结果稳定）且亚毫秒（编辑器规模 ≤ 数百条路线），调用方
/// 无须判断「这次编辑是否影响路网」——统一在提交/撤销/加载后重建，
/// 不为省一次扫描做脏路径分流（分流漏一条路径就是陈旧投影，代价更高）。
///
/// **不进脏路径**：见模块文档「不进脏路径」一节。
pub fn sync_derived_road_edges(world: &mut World) -> RoadDeriveReport {
    rebuild_derived_edges(world, ROAD_SNAP_TOLERANCE_WU)
}

/// 迁移命令的**持有外壳**：命令本体 `Box` 进事务后就不可及了，而迁移报告
/// （诊断计数）要等 `apply` 跑完才填——`Arc` 让事务与世界各持一份引用，
/// 提交后仍能读 `report()`。纯转发，不改命令语义（label / apply / revert
/// 逐一同名透传，撤销栈里看到的仍是迁移命令本身）。
#[derive(Debug)]
struct MigrateCarrier(Arc<MigrateLegacyLineDataCommand>);

impl EditCommand for MigrateCarrier {
    fn label(&self) -> String {
        self.0.label()
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        self.0.apply(world)
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        self.0.revert(world)
    }
}

/// 加载期自动迁移（ADR-0070 §4.6 / T9）：世界带存量线数据（`needs_line_migration`
/// 为真）时，把 [`MigrateLegacyLineDataCommand`] 经 `Transaction` commit 进
/// [`EditHistory`]——一条命令一次撤销全回（涂格转换 + 链式合并 + 模板河转实体）。
///
/// ## 返回值与裁定口径
///
/// - `Some(report)`：迁移实际发生（计数非全零）——**已入撤销栈**，调用方
///   负责 `record_dirty` 之外的回执展示（[`migration_notice_text`]）；
/// - `None`：无需迁移，**或**计数全零（等价 dirty 为空）——全零事务**不入
///   撤销栈**（评审裁定）：此刻 History 刚在加载时清空、本事务是其唯一
///   条目，整体重置即「未入栈」；调用方也不弹 notice。
///
/// ## 调用前提
///
/// **必须在加载清空 History 之后、任何作者编辑之前**（`apply_loaded_world`
/// 满足）——「全零 → 重置 History」的等价性依赖栈里只有本事务这一事实。
///
/// 迁移的脏路径（改格 / 删手写边 / 增地点）由本函数 `record_dirty`——这些是
/// 作者数据的真实变动，必须落盘；随后的推导边重建（[`sync_derived_road_edges`]）
/// 才是「不进脏路径」的那一半。
pub fn migrate_legacy_lines_on_load(
    world: &mut World,
    edit: &mut EditHistory,
) -> Option<MigrationReport> {
    if !needs_line_migration(world) {
        return None;
    }
    let cmd = Arc::new(MigrateLegacyLineDataCommand::new());
    let mut tx = Transaction::new();
    tx.push(Box::new(MigrateCarrier(cmd.clone())));
    let dirty = match edit.history.commit(world, tx) {
        Ok(dirty) => dirty,
        // 迁移失败（现实路径仅锁中毒）：事务已整体回滚，按「未迁移」处理，
        // 诊断面板的待迁移提示（V1）仍在，作者下次加载会再试。
        Err(_) => return None,
    };
    let report = cmd.report();
    if report == MigrationReport::default() {
        // 计数全零 = 什么都没改成（典型：模板世界低地生成不出河源头）。
        // 不入撤销栈、不弹 notice——见函数文档。
        edit.history = Default::default();
        return None;
    }
    edit.record_dirty(dirty);
    Some(report)
}

/// 迁移回执文案（V1：迁移可见）：**必须包含 `chains_skipped` 计数**（评审
/// 裁定）——跳过的链 = 被删的手写边没有等价线替代（自环 / 退化链 / 完全
/// 重放），作者得知道有数据只删未转。
pub fn migration_notice_text(report: &MigrationReport) -> String {
    format!(
        "已自动迁移存量线数据：合并道路链 {} 条（跳过 {}）、模板河 {} 条、转换涂格 {} 格、删除手写路边 {} 条",
        report.chains, report.chains_skipped, report.rivers, report.converted_cells, report.deleted_edges
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::distance::Place;
    use wv_core::id::Id;
    use wv_core::map::distance::LineKind;
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::terrain::{TerrainCellData, TerrainData};
    use wv_core::time::TimeSpan;
    use wv_edit::command::CreateLinePoiCommand;
    use wv_terrain::{BIOME_NONE, BIOME_RIVER, BIOME_ROAD, SURFACE_LAND};

    const M: &str = "map_world";

    fn vid(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 主图：`unit_per_km = 1.0`（1 wu = 1 km，手算方便）、extent 256×256。
    fn mig_map() -> MapNode {
        MapNode {
            id: vid(M),
            name: "主世界".into(),
            level: MapLevel::World,
            unit_per_km: Some(1.0),
            extent: Some(Extent::sized(256.0, 256.0)),
            ..MapNode::default()
        }
    }

    fn point_place(id: &str, x: f32, y: f32) -> Place {
        Place {
            id: vid(id),
            name: id.into(),
            feature: None,
            map: Some(vid(M)),
            x: Some(x),
            y: Some(y),
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: None,
            kind: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    /// 道路线段 POI（`line_kind = Road`），锚定在主图上。
    fn road_line_place(id: &str, line: Vec<f32>) -> Place {
        Place {
            id: vid(id),
            name: id.into(),
            feature: None,
            map: Some(vid(M)),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(line),
            kind: None,
            line_kind: Some(LineKind::Road),
            line_width_m: None,
        }
    }

    fn hand_road_edge(id: &str, a: &str, b: &str, km: f32) -> wv_core::map::distance::PlaceEdge {
        wv_core::map::distance::PlaceEdge {
            id: vid(id),
            a: vid(a),
            b: vid(b),
            distance_km: km,
            terrain: "road".into(),
            bidirectional: true,
            rank: None,
            derived: false,
        }
    }

    fn flat_terrain(biome_overrides: &[(usize, u8)]) -> TerrainData {
        let mut cells = vec![
            TerrainCellData {
                height: 100,
                surface: SURFACE_LAND,
                biome: 1,
            };
            16
        ];
        for &(idx, biome) in biome_overrides {
            cells[idx].biome = biome;
        }
        TerrainData {
            width: 4,
            height: 4,
            cells,
        }
    }

    /// 存量世界夹具：4×4 图上 4 格旧河 + 2 格旧路涂格、十字手写路网
    /// （4 条边，各 100 km）+ 一条自环边（评裁定观察点：chains_skipped）。
    fn legacy_world() -> World {
        let mut w = World::default();
        w.maps.push(mig_map());
        w.set_terrain(
            vid(M),
            flat_terrain(&[
                (0, BIOME_RIVER),
                (5, BIOME_RIVER),
                (9, BIOME_RIVER),
                (14, BIOME_RIVER),
                (3, BIOME_ROAD),
                (7, BIOME_ROAD),
            ]),
        );
        w.distances.places = vec![
            point_place("cross_center", 128.0, 128.0),
            point_place("north_end", 128.0, 28.0),
            point_place("south_end", 128.0, 228.0),
            point_place("east_end", 228.0, 128.0),
            point_place("west_end", 28.0, 128.0),
        ];
        w.distances.edges = vec![
            hand_road_edge("e_cn", "cross_center", "north_end", 100.0),
            hand_road_edge("e_cs", "cross_center", "south_end", 100.0),
            hand_road_edge("e_ce", "cross_center", "east_end", 100.0),
            hand_road_edge("e_cw", "cross_center", "west_end", 100.0),
            hand_road_edge("e_loop", "cross_center", "cross_center", 5.0),
        ];
        w
    }

    /// 命名测试 1：加载带涂格 16/20 与手写 road 边的世界 → 自动迁移，
    /// 报告计数逐项可断、文案含 chains_skipped、事务入撤销栈、派生边就位。
    #[test]
    fn loading_legacy_world_migrates_and_reports_counts() {
        let mut w = legacy_world();
        assert!(needs_line_migration(&w), "夹具前提：存量线数据在场");
        let mut edit = EditHistory::default();

        let report =
            migrate_legacy_lines_on_load(&mut w, &mut edit).expect("需迁移的世界应返回非全零报告");
        // 计数：4 链 + 1 自环跳过、5 条手写边全消费、6 格涂格转换。
        assert_eq!(report.chains, 4, "实得 {:?}", report);
        assert_eq!(
            report.chains_skipped, 1,
            "自环边应计跳过，实得 {:?}",
            report
        );
        assert_eq!(report.deleted_edges, 5, "实得 {:?}", report);
        assert_eq!(report.converted_cells, 6, "实得 {:?}", report);

        // 非全零 → 入撤销栈（评审裁定：迁移可整体撤销）。
        assert!(edit.history.can_undo(), "迁移事务应已入撤销栈");
        // 迁移的真实变动进脏路径（落盘要重写这些目录）。
        assert!(
            edit.dirty_set.contains("distances/places")
                && edit.dirty_set.contains("distances/edges")
                && edit.dirty_set.contains("maps/map_world/terrain.bin"),
            "实得 {:?}",
            edit.dirty_set
        );

        // 加载完成点的另一半：迁移后重建推导边 → 路网投影直接可渲染。
        sync_derived_road_edges(&mut w);
        let derived = w.distances.edges.iter().filter(|e| e.derived).count();
        assert_eq!(derived, 4, "四条链各恢复一条推导边，实得 {}", derived);
        assert!(!needs_line_migration(&w), "迁移后判据应收敛");

        // 回执文案必须包含 chains_skipped 计数（V1：迁移可见）。
        let text = migration_notice_text(&report);
        assert!(
            text.contains("跳过 1"),
            "文案须含 chains_skipped 计数：{text}"
        );
    }

    /// 命名测试 2：撤销迁移 → 回到存量态；再走一遍加载迁移 → Place 数
    /// 与首迁一致（链 ID 确定性 ⇒ 不增殖）。
    #[test]
    fn undo_migration_then_reload_does_not_duplicate() {
        let mut w = legacy_world();
        let mut edit = EditHistory::default();
        let first = migrate_legacy_lines_on_load(&mut w, &mut edit).expect("首迁应发生");
        sync_derived_road_edges(&mut w);
        let places_after_mig = w.distances.places.len();
        let mig_count = |world: &World| {
            world
                .distances
                .places
                .iter()
                .filter(|p| p.id.as_str().starts_with("road_mig_"))
                .count()
        };
        assert_eq!(mig_count(&w), 4, "首迁应落 4 条链实体");

        // 撤销（应用层 undo 出口 = History::undo，随后跟同一条重建钩子）。
        edit.history.undo(&mut w).expect("迁移事务应可撤销");
        sync_derived_road_edges(&mut w);
        assert_eq!(mig_count(&w), 0, "撤销应删净迁移实体");
        assert!(
            !w.distances.edges.iter().any(|e| e.derived),
            "手写边回场后同端点对推导让位，实得 {:?}",
            w.distances.edges
        );
        assert!(needs_line_migration(&w), "撤销后回到存量态");

        // 构造重载：加载清空 History → 再迁移。
        let mut reloaded_edit = EditHistory::default();
        let second =
            migrate_legacy_lines_on_load(&mut w, &mut reloaded_edit).expect("存量态重载应再次迁移");
        assert_eq!(second.chains, first.chains, "链数应与首迁一致");
        assert_eq!(
            w.distances.places.len(),
            places_after_mig,
            "重载迁移不得增殖 Place（实得 {} → {}）",
            places_after_mig,
            w.distances.places.len()
        );
        assert!(
            reloaded_edit.history.can_undo(),
            "重载迁移同样应入撤销栈（非全零报告）"
        );
    }

    /// 命名测试 3：经命令链「加路 POI」→ 提交点钩子重建出派生边；
    /// 派生边**不进脏路径**，save → 重读工程后 YAML / 加载结果中**无 derived
    /// 边**、且不触发重迁移（重载后 places 数不变）。
    #[test]
    fn derived_edges_are_rebuilt_after_place_edits() {
        // 干净底座：有比例尺的图 + 两个锚定镇，无任何路。
        let mut w = World::default();
        w.maps.push(mig_map());
        w.distances.places = vec![
            point_place("hamlet_alpha", 0.0, 0.0),
            point_place("village_beta", 200.0, 0.0),
        ];
        let mut edit = EditHistory::default();

        // 加路 POI：CreateLinePoiCommand（纯矢量创建，无下切）——与面板写回
        // 同一条命令链（Transaction → History::commit）。
        let road = road_line_place("road_new", vec![0.0, 0.0, 200.0, 0.0]);
        let mut tx = Transaction::new();
        tx.push(Box::new(CreateLinePoiCommand::new(vid(M), road, None, 1.0)));
        let dirty = edit
            .history
            .commit(&mut w, tx)
            .expect("创建线段 POI 应成功");
        edit.record_dirty(dirty);

        // 提交点钩子：edit/* 各 commit 点在 Ok 分支调用的同一函数。
        sync_derived_road_edges(&mut w);
        let derived: Vec<_> = w.distances.edges.iter().filter(|e| e.derived).collect();
        assert_eq!(derived.len(), 1, "两端吸附 + 有比例尺 → 恰一条推导边");
        assert!(
            (derived[0].distance_km - 200.0).abs() < 1e-3,
            "200 wu × 1.0 km/wu = 200 km，实得 {}",
            derived[0].distance_km
        );

        // 推导边不进脏路径（评审裁定 4）：创建命令只标脏 places。
        assert!(
            !edit.dirty_set.contains("distances/edges"),
            "derived 不得标脏，实得 {:?}",
            edit.dirty_set
        );
        assert!(
            edit.dirty_set.contains("distances/places"),
            "创建命令本身应标脏 places"
        );

        // 保存路径不含它：增量 dirty 路由不触发 edges.yaml 重写。
        let dirty = edit.take_dirty();
        assert!(
            !dirty.iter().any(|d| d == "distances/edges"),
            "实得 {:?}",
            dirty
        );

        // 落盘往返完整断言（原计划断言，T9 评审修复后恢复）：save → 重读
        // 工程 → YAML / 加载结果中**无 derived 边**，且不触发重迁移。
        //
        // 契约在 wv-io 侧：`PlaceEdge.derived` 的 `#[serde(skip)]` 只管字段
        // 序列化，管不了实体是否被写——`save_world` 必须按 `!e.derived`
        // 过滤列表成员（ADR-0070 D5：推导边是运行时投影，永不落盘）。
        // 本夹具 0 条手写边、1 条推导边 ⇒ 重载后 edges 应为空、places 数
        // 不变、`needs_line_migration` 不再为真（若推导边落了盘，重载即
        // 退化成手写 road 边，下次加载会重迁移补建 `road_mig_*` 实体）。
        let places_before_save = w.distances.places.len();
        let dir = std::env::temp_dir().join(format!("wv_t9_derived_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let save_result = (|| -> Result<(), String> {
            let mut project = wv_io::Project::create(&dir, "t9").map_err(|e| e.to_string())?;
            project.save_world(&w, &[]).map_err(|e| e.to_string())?;
            // YAML 层：推导边文件不得存在（夹具无手写边，edges 目录应为空）。
            let edges_dir = dir.join("distances/edges");
            let leaked: Vec<String> = std::fs::read_dir(&edges_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter_map(|e| {
                            e.path()
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                        })
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                leaked.is_empty(),
                "edges 目录不得有任何落盘边（推导边永不落盘），实得 {leaked:?}"
            );
            // 加载层：重读工程 → 无 derived 边、places 数不变、不触发重迁移。
            let reopened = wv_io::Project::open(&dir).map_err(|e| e.to_string())?;
            let reloaded = reopened.load_world().map_err(|e| e.to_string())?;
            assert!(
                reloaded.distances.edges.is_empty(),
                "重载不得带任何边（投影边不落盘），实得 {:?}",
                reloaded
                    .distances
                    .edges
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                reloaded.distances.places.len(),
                places_before_save,
                "重载后 places 数不得变（触发重迁移会补建 road_mig_* 实体，{} → {}）",
                places_before_save,
                reloaded.distances.places.len()
            );
            assert!(
                !needs_line_migration(&reloaded),
                "重载不得再触发存量线数据迁移判据"
            );
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&dir);
        save_result.expect("落盘往返应成功");
    }

    /// 全零裁定（评审）：模板世界低地生成不出河源头 → 判据为真而迁移计数
    /// 全零 → 不入撤销栈、不弹 notice（返回 None）。
    #[test]
    fn migration_with_all_zero_report_stays_out_of_undo_stack() {
        let mut w = World::default();
        w.maps.push(mig_map());
        // 全图高 100：generate_rivers 源头门槛 700，一条河也生成不出。
        w.set_terrain(vid(M), flat_terrain(&[]));
        w.meta = Some(wv_core::WorldMeta {
            name: "t9".into(),
            version: "1".into(),
            schema_version: 2,
            root_map: vid(M),
            timeline_range: TimeSpan { from: 0, to: None },
            template: Some(wv_core::WorldTemplate {
                kind: wv_core::WorldTemplateKind::ColdNorth,
                seed: 1,
                rivers_seed: 42,
            }),
        });
        assert!(needs_line_migration(&w), "夹具前提：模板在场而无河线 POI");
        let mut edit = EditHistory::default();
        assert!(
            migrate_legacy_lines_on_load(&mut w, &mut edit).is_none(),
            "全零报告应返回 None"
        );
        assert!(!edit.history.can_undo(), "全零迁移不得入撤销栈（评审裁定）");
        assert!(
            edit.dirty_set.is_empty(),
            "全零迁移不得标脏，实得 {:?}",
            edit.dirty_set
        );
    }

    /// 无存量数据的世界：迁移入口直接放行（None），History 纹丝不动。
    #[test]
    fn clean_world_skips_migration_entirely() {
        let mut w = World::default();
        w.maps.push(mig_map());
        let mut edit = EditHistory::default();
        edit.record_dirty(["distances/places"]);
        assert!(migrate_legacy_lines_on_load(&mut w, &mut edit).is_none());
        // 没动 History：既有 dirty 保留、栈仍为空——未被误重置。
        assert!(edit.history.is_empty());
        assert!(
            edit.dirty_set.contains("distances/places"),
            "实得 {:?}",
            edit.dirty_set
        );
    }

    /// BIOME 常量在场性哨兵：夹具依赖的 16/20 语义不漂移。
    #[test]
    fn biome_constants_match_legacy_paint_codes() {
        assert_eq!(BIOME_RIVER, 16);
        assert_eq!(BIOME_ROAD, 20);
        assert_eq!(BIOME_NONE, 0);
    }
}
