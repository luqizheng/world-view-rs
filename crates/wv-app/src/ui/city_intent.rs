//! 「生成城市」意图（2026-10-17 选择工具 spec §5；M2）。
//!
//! ## 职责
//! - 几何纯函数：[`ring_centroid`]（质心落点）/ [`ring_bbox_area`]（面积预填）/
//!   [`area_to_16_9`]（面积 → 16:9 画布）。
//! - （Task 3）[`CityIntentDialog`] 对话框资源 + 预填 + 确认系统：
//!   确认 → 16:9 extent + 局部矩形轮廓 → `AddCityCommand`（锚点显式落质心）
//!   → 自动 `drill.enter` 进城市编辑器。
//!
//! ## 边界（不做什么）
//! - **不选底图地形**——城市图画布走既有继承/预填口径，本模块只建 `MapNode`。
//! - **不碰选区**——确认后钻入新图，选区由 `sync_selection_map_change` 随切图清空。
//! - 坐标口径：质心 / 包围盒为**父图局部 wu**（与选区同源链）。

#![cfg(feature = "app")]

/// 多边形质心（鞋带公式，面积加权）；退化（零面积）回退包围盒中心。
///
/// 作者圈的环不保证凸也不保证不自交——鞋带公式对两类都给出确定值，
/// 零面积（共线 / 一字形）时加权分母为零，回退包围盒中心最不意外。
pub fn ring_centroid(ring: &[(f32, f32)]) -> (f32, f32) {
    let n = ring.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut a2 = 0.0f32; // 2×有向面积
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        a2 += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if a2.abs() < 1e-6 {
        // 退化：包围盒中心。
        let (min_x, max_x, min_y, max_y) = bbox(ring);
        return ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    }
    (cx / (3.0 * a2), cy / (3.0 * a2))
}

/// 包络包围盒面积（wu²）。
pub fn ring_bbox_area(ring: &[(f32, f32)]) -> f32 {
    let (min_x, max_x, min_y, max_y) = bbox(ring);
    (max_x - min_x) * (max_y - min_y)
}

/// 面积 → 16:9 画布 (w, h)：`w = √(A·16/9)`、`h = √(A·9/16)`（spec §5）。
pub fn area_to_16_9(area: f32) -> (f32, f32) {
    let w = (area * 16.0f32 / 9.0).sqrt();
    let h = (area * 9.0f32 / 16.0).sqrt();
    (w, h)
}

/// 环的包围盒（空环返回零盒）。
fn bbox(ring: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in ring {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if !min_x.is_finite() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (min_x, max_x, min_y, max_y)
}

#[cfg(test)]
mod tests {
    use super::{
        area_to_16_9, commit_city, parse_area_text, prefill, ring_bbox_area, ring_centroid,
        validate_city_id, CityIntentDialog,
    };
    use crate::edit::editing::EditHistory;
    use crate::state::{DrillState, WorldProjection};
    use crate::ui_shell::ShellNotice;
    use bevy::prelude::*;
    use wv_core::map::{Extent, MapBase, MapLevel};
    use wv_core::World;

    fn square() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
    }

    #[test]
    fn square_ring_centroid_is_its_center() {
        let c = ring_centroid(&square());
        assert!(
            (c.0 - 5.0).abs() < 1e-4 && (c.1 - 5.0).abs() < 1e-4,
            "got {c:?}"
        );
    }

    /// 共线三点面积为零 → 回退包围盒中心（不产生 NaN 质心）。
    #[test]
    fn degenerate_ring_centroid_falls_back_to_bbox_center() {
        let collinear = vec![(0.0, 0.0), (5.0, 5.0), (10.0, 10.0)];
        let c = ring_centroid(&collinear);
        assert!(
            (c.0 - 5.0).abs() < 1e-4 && (c.1 - 5.0).abs() < 1e-4,
            "got {c:?}"
        );
    }

    #[test]
    fn ring_bbox_area_measures_envelope() {
        assert!(
            (ring_bbox_area(&square()) - 100.0).abs() < 1e-4,
            "10×10 方环面积 100"
        );
    }

    /// 手算：A=14400 → w=√(14400·16/9)=√25600=160，h=√(14400·9/16)=√8100=90。
    #[test]
    fn area_maps_to_16_9_extent() {
        let (w, h) = area_to_16_9(14400.0);
        assert!((w - 160.0).abs() < 1e-3, "got w={w}");
        assert!((h - 90.0).abs() < 1e-3, "got h={h}");
        assert!((w * h - 14400.0).abs() < 1e-2, "面积守恒 w·h={}", w * h);
    }

    // ====== 预填 / 校验 / 提交（Task 3）======

    fn closed_selection(ring: Vec<(f32, f32)>) -> wv_render::state::SelectionState {
        wv_render::state::SelectionState {
            enabled: true,
            closed: Some(ring),
            ..Default::default()
        }
    }

    /// 方形环 → 质心 (5,5)、面积预填 100（≥ 下限）、缺省 id/name 按图数派生。
    #[test]
    fn prefill_fills_centroid_area_and_default_ids() {
        let mut d = CityIntentDialog::default();
        let selection = closed_selection(square());
        prefill(
            &mut d,
            &selection,
            wv_core::Id::new_unchecked("world_main"),
            None,
            3,
        );
        assert!(d.open, "预填即打开");
        assert_eq!(
            d.parent_map.as_ref().map(wv_core::Id::as_str),
            Some("world_main")
        );
        assert!((d.centroid.0 - 5.0).abs() < 1e-4, "got {:?}", d.centroid);
        assert!((d.area - 100.0).abs() < 1e-4, "got area={}", d.area);
        assert_eq!(d.id, "city_3", "缺省 id 按图数派生");
        assert_eq!(d.name, "新城 3");
        assert_eq!(d.base, MapBase::Ocean, "底图缺省海洋");
    }

    /// anchor = 质心 ÷ 父图 extent（icon_local 同款，无 origin 项）。
    #[test]
    fn prefill_normalizes_anchor_against_parent_extent() {
        let mut d = CityIntentDialog::default();
        let selection = closed_selection(square());
        prefill(
            &mut d,
            &selection,
            wv_core::Id::new_unchecked("region_a"),
            Some(Extent::sized(100.0, 200.0)),
            0,
        );
        assert!(
            (d.anchor[0] - 0.05).abs() < 1e-4,
            "5/100 → got {:?}",
            d.anchor
        );
        assert!(
            (d.anchor[1] - 0.025).abs() < 1e-4,
            "5/200 → got {:?}",
            d.anchor
        );
    }

    /// 父图无 extent（世界图常态）→ 按 FALLBACK_EXTENT 4096² 归一化（与
    /// `icon_local` 渲染口径一致，图标不得漂到哈希位）。
    #[test]
    fn prefill_uses_fallback_extent_when_parent_has_none() {
        let mut d = CityIntentDialog::default();
        // 100×100 方环质心 (5,5)，选区挪到世界图中央附近再算：直接用大坐标环。
        let ring = vec![
            (2048.0, 2048.0),
            (2148.0, 2048.0),
            (2148.0, 2148.0),
            (2048.0, 2148.0),
        ];
        let selection = closed_selection(ring);
        prefill(
            &mut d,
            &selection,
            wv_core::Id::new_unchecked("world_main"),
            None,
            0,
        );
        assert!(
            (d.anchor[0] - 2098.0 / 4096.0).abs() < 1e-4,
            "got {:?}",
            d.anchor
        );
        assert!(
            (d.anchor[1] - 2098.0 / 4096.0).abs() < 1e-4,
            "got {:?}",
            d.anchor
        );
    }

    #[test]
    fn area_input_parse_failure_keeps_previous_value() {
        assert_eq!(parse_area_text("abc", 250.0), 250.0, "非法输入保持旧值");
        assert_eq!(parse_area_text("3600", 250.0), 3600.0, "合法输入采纳");
        assert_eq!(
            parse_area_text("  1  ", 250.0),
            100.0,
            "过小值夹到 MIN_CITY_AREA"
        );
        assert_eq!(parse_area_text("-5", 250.0), 100.0, "负值夹到下限");
    }

    #[test]
    fn city_id_validation_rejects_empty_charset_and_duplicates() {
        let mut world = World::default();
        world.maps.push(wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked("city_dup"),
            ..Default::default()
        });
        assert_eq!(
            validate_city_id("", &world).as_deref(),
            Some("ID 不能为空"),
            "空 ID 拒收"
        );
        assert_eq!(
            validate_city_id("城-1", &world).as_deref(),
            Some("ID 只能含 [A-Za-z0-9_]"),
            "非法字符拒收"
        );
        assert_eq!(
            validate_city_id("city_dup", &world).as_deref(),
            Some("ID `city_dup` 已存在"),
            "重名拒收"
        );
        assert_eq!(validate_city_id("city_new", &world), None, "合法 ID 放行");
    }

    /// 提交：16:9 extent（origin=质心居中）+ 钻入图标锚点显式落质心 +
    /// 自动钻入新图（spec §5 / Review Focus 2-3）。
    #[test]
    fn commit_city_creates_16_9_node_explicit_anchor_and_drills_in() {
        let mut world = World::default();
        world.maps.push(wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked("world_main"),
            name: "世界".into(),
            level: MapLevel::World,
            ..Default::default()
        });
        let mut projection = WorldProjection {
            world,
            project_dir: Some("t".into()),
            dirty: false,
            load_error: None,
            selection: None,
        };
        let mut edit = EditHistory::default();
        let mut drill = DrillState::default();
        drill.ensure_root(&projection.world);
        let mut notice = ShellNotice::default();

        let mut d = CityIntentDialog::default();
        d.open = true;
        d.parent_map = Some(wv_core::Id::new_unchecked("world_main"));
        d.centroid = (1000.0, 500.0);
        d.anchor = [1000.0 / 4096.0, 500.0 / 4096.0];
        d.id = "city_test".into();
        d.name = "试作城".into();
        d.base = MapBase::Land;
        d.area = 14400.0;

        assert!(
            commit_city(&d, &mut projection, &mut edit, &mut drill, &mut notice, 0),
            "提交应成功"
        );

        let node = projection
            .world
            .maps
            .iter()
            .find(|m| m.id.as_str() == "city_test")
            .expect("城市节点应已建");
        assert_eq!(node.level, MapLevel::City);
        let e = node.extent.expect("应有 extent");
        assert!(
            (e.width - 160.0).abs() < 1e-2 && (e.height - 90.0).abs() < 1e-2,
            "16:9 画布"
        );
        let o = e.origin.expect("应有 origin");
        assert!(
            (o.x - 920.0).abs() < 1e-2 && (o.y - 455.0).abs() < 1e-2,
            "质心居中"
        );
        // 钻入图标锚点 = 预填值（不得落哈希位）。
        let icon = node
            .icons
            .iter()
            .find(|i| i.drill_into.is_some())
            .expect("应有钻入图标");
        assert_eq!(icon.anchor, Some(d.anchor), "锚点显式落质心");
        // 自动钻入。
        assert_eq!(
            drill.trail().last().map(|i| i.as_str()),
            Some("city_test"),
            "确认后应自动钻入城市图"
        );
    }
}
