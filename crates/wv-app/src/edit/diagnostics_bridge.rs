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
//! - 错误**无 anchor**（`CoreError::DanglingReference` 不携带 map/day），
//!   因此 §A 延展的「点击跳转」对错误**不**触发——这与 `Project::diagnose`
//!   一致，校验面板错误行只会显示成普通 label。
//! - 提示走 `derive_hints`（已有 anchor），点击跳转继续生效。
//!
//! ## 与 `Project::diagnose` 的边界
//!
//! `Project::diagnose` 走 `self.load_world()` → 重新解析磁盘；本模块只
//! 看 `&World` 内存视图。两者**不**互相调用——同一份校验逻辑的两次实现
//! 而已。

#![cfg(feature = "app")]

use wv_core::World;
use wv_io::diagnostics::{self, derive_hints, Diagnostics};
use wv_theme::theme_pack::ThemePack;

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
        d.items.push(diagnostics::error(
            format!("dangling:{}", err),
            err.to_string(),
        ));
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

    d.dedup_by_fingerprint();
    d
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

    /// 错误无 anchor（与 derive_hints 的 EncounterUnwritten 形态不同）——
    /// §A 延展的「点击跳转」对错误**不**触发。
    #[test]
    fn dangling_errors_have_no_anchor() {
        let mut w = World::default();
        w.maps.push(map_of("map_main"));
        w.tracks.push(track_of("u_ghost", "map_main"));
        let d = derive_diagnostics(&w, &wv_render::theme::theme_pack::builtin::DEFAULT);
        let errs = d.errors();
        assert!(!errs.is_empty(), "测试前提：应有错误");
        for err in errs {
            assert!(
                err.anchor.is_none(),
                "悬空引用错误无 anchor（CoreError 不携带 map/day）"
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
}
