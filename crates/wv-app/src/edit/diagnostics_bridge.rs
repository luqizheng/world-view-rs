//! 校验面板：派生 `Diagnostics`（错误 + 提示）的「桥」层。
//!
//! ## 职责
//!
//! `Project::diagnose`（`wv-io`）是**磁盘侧**全量重算——CI 用、加载后调用。
//! 编辑器在工作副本（`WorldProjection.world`）上**不**应该每帧都去敲盘；
//! 但**也不**应该只看 hints（`derive_hints`）而忽视悬空引用——
//! 否则 §7.5V1「让作者看见问题」就是空话。
//!
//! 本模块提供 [`derive_diagnostics`]，对**内存里的** `World` 跑同一套
//! 引用校验逻辑，**与** `Project::diagnose` 共用底层收集器，保证两边
//! 看到的悬空引用集合一致。
//!
//! ## 决策
//!
//! - 复用 `wv_io::situation_io::collect_situations_and_tracks_dangling`、
//!   `wv_io::project::collect_map_icons_dangling` 与 `diagnose` 中的
//!   「事件 → 旁白引用」循环——**单一真相**。
//! - ~~错误**无 anchor**~~ **UX-019（2026-10-30）修正**：错误行现在也派生
//!   锚点——[`anchor_for_dangling`] 从 referrer 的 `kind.id` 前缀反查
//!   referrer 实体在内存世界中的位置（轨迹 / 事态 / 事件 / 地图图标四类；
//!   反查不到的仍无锚点，退化为不可点的普通 label）。原结论「错误无 anchor
//!   不可点」自此废止；`Project::diagnose` 的磁盘侧口径不受影响（那边只产
//!   文案，不产锚点）。
//! - 提示走 `derive_hints`（已有 anchor），点击跳转继续生效。
//!
//! ## 与 `Project::diagnose` 的边界
//!
//! `Project::diagnose` 走 `self.load_world()` → 重新解析磁盘；本模块只
//! 看 `&World` 内存视图。两者**不**互相调用——同一份校验逻辑的两次实现
//! 而已。

#![cfg(feature = "app")]

use wv_core::World;
use wv_default_theme::theme_pack::ThemePack;
use wv_io::diagnostics::{self, derive_hints, Diagnostics};

/// 对内存里的 `World` 派生一份 `Diagnostics`：错误 + 提示 + 去重。
///
/// 与 `Project::diagnose` **共享**底层收集器（`collect_situations_and_tracks_dangling`、
/// `collect_map_icons_dangling`、事件→旁白检查），保证内存视图与磁盘视图看到的
/// 悬空引用集合一致。
///
/// **为什么不**直接调 `Project::diagnose`：编辑器不应当每帧都敲盘；内存视图足够。
pub fn derive_diagnostics(world: &World, theme: &ThemePack) -> Diagnostics {
    let mut d = Diagnostics::default();

    // 引用完整性 → Error 级诊断。**一次性检出全部**，不是遇到第一个就停。
    let event_ids: Vec<wv_core::Id> = world.events.iter().map(|e| e.id.clone()).collect();
    let map_ids: Vec<wv_core::Id> = world.maps.iter().map(|m| m.id.clone()).collect();
    let character_ids: Vec<wv_core::Id> = world.characters.iter().map(|c| c.id.clone()).collect();
    let place_ids: Vec<wv_core::Id> = world
        .distances
        .places
        .iter()
        .map(|p| p.id.clone())
        .collect();
    let unit_ids: Vec<wv_core::Id> = world.units.iter().map(|u| u.id.clone()).collect();

    let mut dangling: Vec<wv_core::CoreError> =
        wv_io::situation_io::collect_situations_and_tracks_dangling(
            &world.situations,
            &world.tracks,
            wv_io::situation_io::KnownIds {
                events: &event_ids,
                maps: &map_ids,
                characters: &character_ids,
                places: &place_ids,
                units: &unit_ids,
            },
        );
    // 地图图标的 `drill_into` 悬空（README §12 第 12 条）。
    dangling.extend(wv_io::project::collect_map_icons_dangling(world));
    // 事件 → 旁白引用的悬空（旁白是独立实体，事件只存 ID；2026-01 用户裁定）。
    for e in &world.events {
        for nid in &e.narrations {
            if !world.narrations.iter().any(|n| &n.id == nid) {
                dangling.push(wv_core::CoreError::DanglingReference {
                    referrer: format!("event.{} (field `narrations`)", e.id),
                    missing: nid.as_str().to_owned(),
                });
            }
        }
    }
    for err in dangling {
        // 与 `Project::diagnose` 同源：指纹取自错误文案本身，含 `referrer` 与
        // `missing`，同一处悬空在多次诊断间保持同一指纹，可被「已读/已忽略」认领。
        // UX-019：错误行同样派生锚点（referrer 实体的图 / 坐标 / 起始日），
        // 校验面板「最需要跳转的悬空引用」自此可点。
        let anchor = anchor_for_dangling(world, &err);
        d.items.push(diagnostics::Diagnostic {
            level: diagnostics::DiagLevel::Error,
            message: err.to_string(),
            fingerprint: format!("dangling:{}", err),
            anchor,
            severity: None,
        });
    }

    // 派生提示（相遇却未写事件）——**永不**产生 Error。
    d.extend(derive_hints(world));

    // 区域覆盖物键在当前主题中无规则（ADR-0045 追记一）——**提示级**，
    // 不阻止保存（V1）：地图上该区域画的是「缺失覆盖物标记」（白底红斜线），
    // 面板这一行告诉作者「是哪个键、哪个区域」。
    for region in &world.regions {
        let Some(key) = region.declared_overlay() else {
            continue;
        };
        if theme.region_overlays.rule_for(key).is_none() {
            d.items.push(diagnostics::hint(
                format!("region-overlay-missing:{}:{}", region.id, key),
                format!(
                    "区域「{}」的覆盖物键「{}」在当前主题中无规则（地图上以缺失标记标出）",
                    region.name, key
                ),
            ));
        }
    }

    // 地点折线的三条提示（ADR-0046 D8）——**提示级**，不阻止保存（V1）。
    for place in &world.distances.places {
        let Some(line) = place.line.as_ref() else {
            continue;
        };
        if line.len() < 4 {
            d.items.push(diagnostics::hint(
                format!("place-line-degenerate:{}", place.id),
                format!("地点「{}」的折线退化（点数 < 2），画不出来", place.name),
            ));
        }
        if place.map.is_none() {
            d.items.push(diagnostics::hint(
                format!("place-line-unanchored:{}", place.id),
                format!("地点「{}」的折线未锚定到地图，不可见", place.name),
            ));
        }
        if place.envelope.is_some() {
            d.items.push(diagnostics::hint(
                format!("place-line-and-envelope:{}", place.id),
                format!(
                    "地点「{}」同时有折线与面包络（互斥），折线优先生效",
                    place.name
                ),
            ));
        }
    }

    // ── ADR-0070 §4.6 / D5（T9）：存量线数据与推导边跳过的可见性 ──
    // 都是**提示级**（V1 只提示不阻止）：迁移由加载期自动完成（见
    // `edit::road_sync::migrate_legacy_lines_on_load`），这里的职责是让
    // 「为什么路上没线 / 为什么还没迁移」在校验面板有处可查。
    if wv_edit::command::needs_line_migration(world) {
        // 待迁移明细计数（从世界数据可直接 recover 的三判据，逐条列出）。
        let painted_cells: usize = world
            .terrains
            .values()
            .map(|t| {
                t.cells
                    .iter()
                    .filter(|c| {
                        c.biome == wv_terrain::BIOME_RIVER || c.biome == wv_terrain::BIOME_ROAD
                    })
                    .count()
            })
            .sum();
        let hand_roads = world
            .distances
            .edges
            .iter()
            .filter(|e| e.terrain == "road" && !e.derived)
            .count();
        let template_pending = world
            .meta
            .as_ref()
            .and_then(|m| m.template)
            .is_some_and(|_| {
                // 与迁移判据同口径：只认河线 POI（缺省与显式 River 同归河）。
                !world.distances.places.iter().any(|p| {
                    wv_core::map::road_derive::effective_line_kind(p)
                        == Some(wv_core::map::distance::LineKind::River)
                })
            });
        let mut parts: Vec<String> = Vec::new();
        if painted_cells > 0 {
            parts.push(format!("旧河/旧路涂格 {painted_cells} 格"));
        }
        if hand_roads > 0 {
            parts.push(format!("手写路网边 {hand_roads} 条"));
        }
        if template_pending {
            parts.push("未转实体的模板河".into());
        }
        d.items.push(diagnostics::hint(
            "line-data-pending-migration",
            format!(
                "世界还带存量线数据（{}），下次加载时将自动迁移为线段 POI（可整体撤销）",
                parts.join("、")
            ),
        ));
    }
    // 道路线因**所在图无比例尺**被推导跳过（D5：只显示 wu，不产边）——
    // 按当前世界现算一次 derive 取计数（纯函数，亚毫秒）。
    let road_report = wv_core::map::road_derive::derive_road_edges(
        world,
        wv_core::map::road_derive::ROAD_SNAP_TOLERANCE_WU,
    );
    if road_report.skipped_no_scale > 0 {
        d.items.push(diagnostics::hint(
            "road-derive-no-scale",
            format!(
                "{} 条道路线因所在地图没有比例尺（km_per_wu），未生成路网边，只显示线本身",
                road_report.skipped_no_scale
            ),
        ));
    }

    // 水陆群系错配（2026-10-27 用户裁定 B 案）——**提示级**，不阻止保存（V1）。
    // 逐图扫地形栅格；地形笔触提交后 `needs_diagnose` 会触发重算，计数自动跟随。
    for (map_id, data) in &world.terrains {
        let n = count_water_biome_mismatch(data.cells.iter());
        if n == 0 {
            continue;
        }
        let name = world
            .maps
            .iter()
            .find(|m| &m.id == map_id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| map_id.to_string());
        d.items.push(diagnostics::hint(
            format!("water-biome-mismatch:{map_id}"),
            format!(
                "地图「{name}」水陆群系错配：{n} 格——水格配了陆生群系，或陆格配了水域群系（选中格后在群系面板手改）"
            ),
        ));
    }

    d.dedup_by_fingerprint();
    d
}

/// 水陆群系错配计数（2026-10-27 B 案 · 纯函数）。
///
/// 判据（湿判定与 [`wv_terrain::edit::is_water_cell`] 同一口径）：
/// - 湿格（水面或海拔 <0）配了**陆生群系**——非 `BIOME_NONE` 且非水域族；
/// - 干格配了**水域族**（咸水 14 / 淡水 15 / 河流 16）。
///
/// `BIOME_NONE` 是中性态（显示按 surface 回退），不计错配。
pub fn count_water_biome_mismatch<'a>(
    cells: impl Iterator<Item = &'a wv_core::terrain::TerrainCellData>,
) -> usize {
    use wv_terrain::biome::{family_of, BiomeFamily, BIOME_NONE};
    cells
        .filter(|c| {
            let wet = wv_terrain::edit::is_water_cell(wv_terrain::TerrainCell {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            });
            let water_family = family_of(c.biome) == Some(BiomeFamily::Water);
            if wet {
                c.biome != BIOME_NONE && !water_family
            } else {
                water_family
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::id::Id;
    use wv_core::time::TimeSpan;
    use wv_core::track::{Track, Waypoint};
    use wv_core::unit::Unit;
    use wv_core::World;

    fn track_of(unit: &str, map: &str) -> Track {
        Track {
            id: Id::new_unchecked("t_test"),
            unit: Id::new_unchecked(unit),
            map: Id::new_unchecked(map),
            span: TimeSpan::new(1, 10).expect("span"),
            waypoints: vec![Waypoint {
                day: Some(1),
                pos: Some(wv_core::geometry::Point { x: 0.0, y: 0.0 }),
                place: None,
            }],
            path: None,
        }
    }

    fn unit_of(id: &str) -> Unit {
        Unit {
            id: Id::new_unchecked(id),
            name: id.into(),
            owner: None,
            members: vec![],
        }
    }

    fn map_of(id: &str) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: id.into(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            ..Default::default()
        }
    }

    /// 最简单的 happy world：unit 已声明、map 已声明、track 引用两者——
    /// 不应当产生错误。
    #[test]
    fn happy_world_has_no_errors() {
        let mut w = World::default();
        w.units.push(unit_of("u_xinhai"));
        w.maps.push(map_of("map_main"));
        w.tracks.push(track_of("u_xinhai", "map_main"));
        let d = derive_diagnostics(&w, &wv_render::theme::theme_pack::builtin::DEFAULT);
        assert!(
            d.errors().is_empty(),
            "happy world 不应产生错误，实际：{:?}",
            d.errors()
        );
    }

    /// 悬空单位：track.unit 指向不存在的 unit → 错误 + 指纹稳定。
    /// map_main 已声明，避免悬空 map 干扰。
    #[test]
    fn dangling_track_unit_produces_error() {
        let mut w = World::default();
        w.units.push(unit_of("u_real"));
        w.maps.push(map_of("map_main"));
        // 第一个 track 引用真实 unit，第二个引用悬空 unit。
        w.tracks.push(track_of("u_real", "map_main"));
        w.tracks.push(Track {
            id: Id::new_unchecked("t_orphan"),
            ..track_of("u_ghost", "map_main")
        });
        let d = derive_diagnostics(&w, &wv_render::theme::theme_pack::builtin::DEFAULT);
        let errs = d.errors();
        assert_eq!(errs.len(), 1, "应恰好一个悬空单位错误，实际：{:?}", errs);
        assert!(
            errs[0].message.contains("u_ghost"),
            "错误文案应包含 missing id：{}",
            errs[0].message
        );
        // 指纹稳定——同一份 world 重跑，错误仍能被认领。
        let d2 = derive_diagnostics(&w, &wv_render::theme::theme_pack::builtin::DEFAULT);
        let f1 = &d.items[0].fingerprint;
        let f2 = &d2.items[0].fingerprint;
        assert_eq!(f1, f2, "指纹应稳定");
    }

    /// UX-019 修正：悬空引用错误**有 anchor**（referrer 实体反查），可点跳转。
    #[test]
    fn dangling_errors_carry_an_anchor_now() {
        let mut w = World::default();
        w.maps.push(map_of("map_main"));
        w.tracks.push(track_of("u_ghost", "map_main"));
        let d = derive_diagnostics(&w, &wv_render::theme::theme_pack::builtin::DEFAULT);
        let errs = d.errors();
        assert!(!errs.is_empty(), "测试前提：应有错误");
        for err in errs {
            assert!(
                err.anchor.is_some(),
                "悬空引用错误应有锚点（referrer 实体可反查）"
            );
        }
    }
    /// 区域覆盖物键在当前主题无规则 → **提示级**诊断（V1：不阻止保存）。
    #[test]
    fn region_overlay_key_missing_in_theme_yields_hint_not_error() {
        use wv_core::model::region::{Region, RegionKind};
        let mut w = World::default();
        w.regions.push(Region {
            id: Id::new_unchecked("nr_miyansenlin"),
            name: "迷幻深林".into(),
            map: Id::new_unchecked("map_world"),
            polygon: vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0],
            holes: vec![],
            kind: RegionKind::Nature,
            parent: None,
            overlay: Some("no_such_key".into()),
        });
        let theme = wv_render::theme::theme_pack::builtin::DEFAULT;
        let d = derive_diagnostics(&w, &theme);
        assert!(
            d.hints().iter().any(|h| h.message.contains("no_such_key")),
            "应提示该键在当前主题无规则"
        );
        assert!(d.errors().is_empty(), "V1：只提示不阻止（不得产生错误）");
    }

    /// 键在主题中存在 → 不产生提示。
    #[test]
    fn resolvable_region_overlay_key_produces_no_hint() {
        use wv_core::model::region::{Region, RegionKind};
        let mut w = World::default();
        w.regions.push(Region {
            id: Id::new_unchecked("nr_ok"),
            name: "针叶林".into(),
            map: Id::new_unchecked("map_world"),
            polygon: vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0],
            holes: vec![],
            kind: RegionKind::Nature,
            parent: None,
            overlay: Some("conifer".into()),
        });
        let theme = wv_render::theme::theme_pack::builtin::DEFAULT;
        let d = derive_diagnostics(&w, &theme);
        assert!(
            !d.hints().iter().any(|h| h.message.contains("conifer")),
            "可解析的键不应产生提示"
        );
    }

    /// 水陆群系错配计数（2026-10-27 B 案）：湿格配陆生群系 / 干格配水域族
    /// 各计 1；NONE 中性格与水陆一致格不计。
    #[test]
    fn water_biome_mismatch_counts_both_directions() {
        use wv_core::terrain::TerrainCellData;
        let cells = [
            TerrainCellData {
                height: -50,
                surface: 0,
                biome: 12,
            }, // 湿格+丘陵 → 错配
            TerrainCellData {
                height: -50,
                surface: 0,
                biome: 14,
            }, // 湿格+咸水 → 一致
            TerrainCellData {
                height: -50,
                surface: 0,
                biome: 0,
            }, // 湿格+NONE → 中性
            TerrainCellData {
                height: 80,
                surface: 1,
                biome: 14,
            }, // 干格+咸水 → 错配
            TerrainCellData {
                height: 80,
                surface: 1,
                biome: 3,
            }, // 干格+草原 → 一致
        ];
        assert_eq!(count_water_biome_mismatch(cells.iter()), 2);
    }

    /// derive_diagnostics 给有错配的图推提示（指纹按图 id 稳定）；无错配
    /// 的图不推——面板提示计数随之自动涨落。
    #[test]
    fn derive_diagnostics_reports_water_biome_mismatch_per_map() {
        use wv_core::terrain::{TerrainCellData, TerrainData};
        let mut w = World::default();
        let map = Id::new_unchecked("map_mismatch");
        w.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            name: "演示图".into(),
            ..Default::default()
        });
        w.set_terrain(
            map,
            TerrainData {
                width: 2,
                height: 1,
                cells: vec![
                    TerrainCellData {
                        height: -50,
                        surface: 0,
                        biome: 12,
                    }, // 湿格+丘陵 → 错配
                    TerrainCellData {
                        height: 80,
                        surface: 1,
                        biome: 3,
                    }, // 干格+草原 → 一致
                ],
            },
        );
        let theme = wv_render::theme::theme_pack::builtin::DEFAULT;
        let d = derive_diagnostics(&w, &theme);
        assert!(
            d.hints()
                .iter()
                .any(|h| h.fingerprint == "water-biome-mismatch:map_mismatch"
                    && h.message.contains('1')),
            "错配图应推一条指纹稳定的提示"
        );

        // 修完（湿格改回咸水）后提示消失。
        w.terrains
            .get_mut(&Id::new_unchecked("map_mismatch"))
            .unwrap()
            .cells[0]
            .biome = 14;
        let d = derive_diagnostics(&w, &theme);
        assert!(
            !d.hints()
                .iter()
                .any(|h| h.fingerprint.starts_with("water-biome-mismatch")),
            "无错配不应推提示"
        );
    }
}

/// UX-019：从悬空引用错误的 referrer 反查 referrer 实体的位置，派生跳转锚点。
///
/// referrer 形如「track.t1 (field `unit`)」或「map.m1.icons[3]」——前两段
/// 「kind.id」足以定位实体（id 在第一个空白或点号处截断）。四类可反查：
/// 轨迹（图 + 首个带坐标途经点 + 轨迹起点日）、事态（第一张涉及图 + 区间
/// 起点）、事件（定位地点的图与锚点 + 事件起点日）、地图（图标所在图，
/// 坐标 (0,0)）。反查不到（实体已被删 / referrer 是未知 kind）→ None，
/// 错误行退化为不可点的普通 label（旧行为，不阻塞 V1）。
fn anchor_for_dangling(
    world: &World,
    err: &wv_core::CoreError,
) -> Option<wv_io::diagnostics::DiagAnchor> {
    use wv_io::diagnostics::DiagAnchor;
    let wv_core::CoreError::DanglingReference { referrer, .. } = err else {
        return None;
    };
    let (kind, rest) = referrer.split_once('.')?;
    let id = &rest[..rest.find([' ', '.']).unwrap_or(rest.len())];
    let at0 = wv_core::geometry::Point { x: 0.0, y: 0.0 };
    match kind {
        "track" => {
            let t = world.tracks.iter().find(|t| t.id.as_str() == id)?;
            let (x, y) = t
                .waypoints
                .iter()
                .find_map(|w| w.pos.map(|p| (p.x, p.y)))
                .unwrap_or((0.0, 0.0));
            Some(DiagAnchor {
                map: t.map.clone(),
                at: wv_core::geometry::Point { x, y },
                day: t.span.from,
            })
        }
        "situation" => {
            let s = world.situations.iter().find(|s| s.id.as_str() == id)?;
            Some(DiagAnchor {
                map: s.maps.first()?.clone(),
                at: at0,
                day: s.span.from,
            })
        }
        "event" => {
            let e = world.events.iter().find(|e| e.id.as_str() == id)?;
            let loc = e.location.as_ref()?;
            let place = world.distances.places.iter().find(|p| p.id == loc.place)?;
            Some(DiagAnchor {
                map: place.map.clone()?,
                at: wv_core::geometry::Point {
                    x: loc.x.or(place.x).unwrap_or(0.0),
                    y: loc.y.or(place.y).unwrap_or(0.0),
                },
                day: e.span.from,
            })
        }
        "map" => {
            let m = world.maps.iter().find(|m| m.id.as_str() == id)?;
            Some(DiagAnchor {
                map: m.id.clone(),
                at: at0,
                day: 0,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    /// 轨迹悬空（unit 不存在）→ 错误行带锚点（轨迹的图 + 起点日 + 首个途经点）。
    #[test]
    fn dangling_track_error_derives_an_anchor() {
        use wv_core::map::{MapLevel, MapNode};
        let mut world = World::default();
        world.maps.push(MapNode {
            id: wv_core::Id::new_unchecked("map_1"),
            level: MapLevel::World,
            ..Default::default()
        });
        world.tracks.push(wv_core::time::track::Track {
            id: wv_core::Id::new_unchecked("track_1"),
            unit: wv_core::Id::new_unchecked("unit_ghost"),
            map: wv_core::Id::new_unchecked("map_1"),
            span: wv_core::time::TimeSpan { from: 12, to: None },
            waypoints: vec![wv_core::time::track::Waypoint {
                day: Some(12),
                pos: Some(wv_core::geometry::Point { x: 30.0, y: 40.0 }),
                place: None,
            }],
            path: None,
        });
        let d = derive_diagnostics(
            &world,
            wv_render::theme::theme_pack::builtin::for_palette(Default::default()),
        );
        let err = d
            .errors()
            .into_iter()
            .find(|e| e.fingerprint.starts_with("dangling:"))
            .expect("应有悬空错误");
        let anchor = err.anchor.as_ref().expect("UX-019：错误行应有锚点");
        assert_eq!(anchor.map.as_str(), "map_1");
        assert_eq!(anchor.day, 12);
        assert_eq!((anchor.at.x, anchor.at.y), (30.0, 40.0));
    }

    /// 事件旁白悬空（bridge 自产的 referrer）→ 锚点指向事件定位地点。
    #[test]
    fn dangling_narration_error_derives_an_anchor_from_the_event_place() {
        use wv_core::map::{MapLevel, MapNode};
        let mut world = World::default();
        world.maps.push(MapNode {
            id: wv_core::Id::new_unchecked("map_1"),
            level: MapLevel::World,
            ..Default::default()
        });
        world.distances.places.push(wv_core::distance::Place {
            id: wv_core::Id::new_unchecked("place_1"),
            name: "城".into(),
            map: Some(wv_core::Id::new_unchecked("map_1")),
            x: Some(5.0),
            y: Some(6.0),
            w: None,
            h: None,
            kind: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
            line: None,
        });
        world.events.push(wv_core::model::event::Event {
            id: wv_core::Id::new_unchecked("event_1"),
            name: "战役".into(),
            span: wv_core::time::TimeSpan { from: 3, to: None },
            location: Some(wv_core::model::event::EventLocation {
                place: wv_core::Id::new_unchecked("place_1"),
                x: None,
                y: None,
                w: None,
                h: None,
            }),
            participants: Vec::new(),
            narrative: None,
            outcome: None,
            effects: Vec::new(),
            narrations: vec![wv_core::Id::new_unchecked("narration_ghost")],
        });
        let d = derive_diagnostics(
            &world,
            wv_render::theme::theme_pack::builtin::for_palette(Default::default()),
        );
        let err = d
            .errors()
            .into_iter()
            .find(|e| e.fingerprint.starts_with("dangling:"))
            .expect("应有悬空错误");
        let anchor = err.anchor.as_ref().expect("事件悬空应有锚点");
        assert_eq!(anchor.map.as_str(), "map_1");
        assert_eq!(anchor.day, 3);
        assert_eq!((anchor.at.x, anchor.at.y), (5.0, 6.0));
    }
}
