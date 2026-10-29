//! 势力分割/几何 upsert/成员变更（fixture：f/geo/layer/mem/square_ring）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

fn f(id: &str, name: &str) -> Faction {
    Faction::new(
        Id::new_unchecked(id),
        name,
        wv_core::color::Color("#112233".into()),
    )
}

fn geo(map: &str, owner: &str, from: i64, to: Option<i64>) -> GeometrySpan {
    GeometrySpan {
        map: Id::new_unchecked(map),
        owner: Id::new_unchecked(owner),
        span: TimeSpan { from, to },
        geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
        relation: None,
    }
}

fn layer(id: &str) -> FactionLayer {
    FactionLayer {
        id: Id::new_unchecked(id),
        name: id.into(),
        z: 0,
        color_mode: FactionColorMode::Own,
        visible: true,
        usage: wv_core::model::faction::LayerUsage::Political,
    }
}

fn mem(target: &str, from: i64) -> Membership {
    Membership {
        target: Id::new_unchecked(target),
        kind: Some("实控".into()),
        span: TimeSpan::open(from),
    }
}

// ---- 势力划分（ADR-0028 / 势力交互 M1 Task 5） ----

fn square_ring(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<f32> {
    vec![x0, y0, x1, y0, x1, y1, x0, y1]
}

/// 一张带方形宿主势力 `faction_host` 的世界（地图 = TEST_MAP，格 1 wu）。
fn world_with_host_faction() -> World {
    let mut w = world_with_terrain(64);
    w.factions
        .push(f(Id::new_unchecked("faction_host").as_str(), "宿主国"));
    w.factions.last_mut().expect("刚推入").id = Id::new_unchecked("faction_host");
    w.faction_geometry.push(GeometrySpan {
        map: test_map(),
        owner: Id::new_unchecked("faction_host"),
        span: TimeSpan { from: 0, to: None },
        geometry: GeometryData::Polygon(square_ring(4.0, 4.0, 60.0, 60.0)),
        relation: None,
    });
    w
}

/// 宿主疆界的外环（武: 带洞时取 outer），wu。
fn faction_ring(w: &World, owner: &str) -> Vec<(f32, f32)> {
    let g = w
        .faction_geometry
        .iter()
        .find(|g| g.owner.as_str() == owner && g.relation.is_none())
        .expect("宿主疆界应存在");
    let flat = match &g.geometry {
        GeometryData::Polygon(v) => v.clone(),
        GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
        other => panic!("意外的几何形态：{other:?}"),
    };
    flat.as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect()
}

/// 连续两次划分：第二次没碰到的宿主外沿必须**逐点**不变（M2 切弧拼接），
/// 且两次 undo 逐位回到划分前。
#[test]
fn two_divisions_keep_untouched_host_edges_pointwise_and_undo() {
    let mut w = world_with_host_faction();
    let host = Id::new_unchecked("faction_host");
    let first = DivideFactionCommand::new(
        test_map(),
        Some(host.clone()),
        vec![(4.0, 4.0), (24.0, 4.0), (24.0, 24.0), (4.0, 24.0)],
        DivisionMode::Straight,
        f("faction_a", "A 国"),
        0,
        1.0,
    );
    first.apply(&mut w).expect("第一次划分");
    let rem1 = faction_ring(&w, "faction_host");
    assert!(rem1.len() >= 4, "余部应是环，实得 {rem1:?}");

    let second = DivideFactionCommand::new(
        test_map(),
        Some(host.clone()),
        // 完全在宿主内部的切块（不碰任何外沿）——这样外沿的「未触碰」语义
        // 才没有歧义。
        vec![(30.0, 30.0), (50.0, 30.0), (50.0, 50.0), (30.0, 50.0)],
        DivisionMode::Straight,
        f("faction_b", "B 国"),
        0,
        1.0,
    );
    second.apply(&mut w).expect("第二次划分");
    let rem2 = faction_ring(&w, "faction_host");

    // 「未触碰的边」= rem1 中两端都落在宿主原外沿线上的边段。
    let on_outer = |p: (f32, f32)| {
        (p.0 - 4.0).abs() < 1e-4
            || (p.1 - 4.0).abs() < 1e-4
            || (p.0 - 60.0).abs() < 1e-4
            || (p.1 - 60.0).abs() < 1e-4
    };
    let n = rem1.len();
    let mut checked = 0usize;
    for i in 0..n {
        let pair = [rem1[i], rem1[(i + 1) % n]];
        if !(on_outer(pair[0]) && on_outer(pair[1])) {
            continue;
        }
        checked += 1;
        assert!(
            rem2.windows(2)
                .any(|w| w == pair || (w[0] == pair[1] && w[1] == pair[0])),
            "未触碰的宿主外沿边段 {pair:?} 必须在第二次划分后逐点不变；rem2={rem2:?}"
        );
    }
    assert!(checked >= 4, "应有足够多的未触碰边段，实得 {checked}");

    // undo 第二次：宿主疆界必须逐点回到 rem1。
    second.revert(&mut w).expect("撤销第二次划分");
    assert_eq!(
        faction_ring(&w, "faction_host"),
        rem1,
        "撤销第二次划分后宿主疆界必须与第一次的余部逐点一致"
    );
    // undo 第一次：回到原始方形。
    first.revert(&mut w).expect("撤销第一次划分");
    let back = faction_ring(&w, "faction_host");
    assert_eq!(
        back,
        vec![(4.0, 4.0), (60.0, 4.0), (60.0, 60.0), (4.0, 60.0)],
        "撤销第一次划分后宿主应回到原方形，实得 {back:?}"
    );
    assert!(w.factions.iter().all(|f| f.id.as_str() == "faction_host"));
}

#[test]
fn divide_creates_faction_and_shrinks_host_atomically() {
    let mut w = world_with_host_faction();
    let cmd = DivideFactionCommand::new(
        test_map(),
        Some(Id::new_unchecked("faction_host")),
        vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_new").as_str(), "新国"),
        0,
        1.0,
    );
    let dirty = cmd.apply(&mut w).expect("划分应成功");
    assert!(dirty.contains(&"factions".to_string()));
    assert_eq!(w.factions.len(), 2, "新势力应入场");
    assert_eq!(w.faction_geometry.len(), 2, "新势力疆界应入场");
    // 宿主余部变成带洞环：1 个洞。
    let host = w
        .faction_geometry
        .iter()
        .find(|g| g.owner == Id::new_unchecked("faction_host"))
        .expect("宿主疆界仍在");
    let GeometryData::PolygonWithHoles { holes, .. } = &host.geometry else {
        panic!("宿主余部应为带洞环，实得 {:?}", host.geometry);
    };
    assert_eq!(holes.len(), 1, "宿主余部应有 1 个洞");
    // 共边（D3）：新势力外环 == 宿主洞沿反序（逐点相等）。
    let new_geo = w
        .faction_geometry
        .iter()
        .find(|g| g.owner == Id::new_unchecked("faction_new"))
        .expect("新势力疆界");
    let GeometryData::Polygon(new_outer) = &new_geo.geometry else {
        panic!("新势力应为单环，实得 {:?}", new_geo.geometry);
    };
    let mut reversed = new_outer.clone();
    let n = reversed.len() / 2;
    let mut pts: Vec<[f32; 2]> = reversed
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| [c[0], c[1]])
        .collect();
    pts.reverse();
    reversed = pts.into_iter().flatten().collect();
    assert_eq!(&holes[0], &reversed, "共边必须逐点相等（D3）");
    assert_eq!(n * 2, new_outer.len(), "顶点数自洽");
}

#[test]
fn divide_at_real_cell_size_keeps_remainder_inside_host() {
    // C1 回归：宿主几何是 wu，内核吃格坐标——cell_size=8 时宿主必须
    // 先除再进内核。宿主 80..480 wu（格 10..60），意图 160..320 wu（格 20..40）。
    let mut w = world_with_land_terrain();
    let cell = 8.0;
    let land: Vec<TerrainCellData> = vec![
        TerrainCellData {
            height: 100,
            surface: 1,
            biome: 3,
        };
        64 * 64
    ];
    w.set_terrain(
        test_map(),
        TerrainData {
            width: 64,
            height: 64,
            cells: land,
        },
    );
    w.factions
        .push(f(Id::new_unchecked("faction_host").as_str(), "宿主国"));
    w.factions.last_mut().expect("刚推入").id = Id::new_unchecked("faction_host");
    w.faction_geometry.push(GeometrySpan {
        map: test_map(),
        owner: Id::new_unchecked("faction_host"),
        span: TimeSpan { from: 0, to: None },
        geometry: GeometryData::Polygon(square_ring(80.0, 80.0, 480.0, 480.0)),
        relation: None,
    });
    let cmd = DivideFactionCommand::new(
        test_map(),
        Some(Id::new_unchecked("faction_host")),
        vec![
            (160.0, 160.0),
            (320.0, 160.0),
            (320.0, 320.0),
            (160.0, 320.0),
        ],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_new").as_str(), "新国"),
        0,
        cell,
    );
    cmd.apply(&mut w).expect("cell_size=8 的划分应成功");

    let host = w
        .faction_geometry
        .iter()
        .find(|g| g.owner == Id::new_unchecked("faction_host"))
        .expect("宿主疆界仍在");
    let bbox = |flat: &[f32]| -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in flat.as_chunks::<2>().0 {
            b.0 = b.0.min(p[0]);
            b.1 = b.1.min(p[1]);
            b.2 = b.2.max(p[0]);
            b.3 = b.3.max(p[1]);
        }
        b
    };
    let host_outer = match &host.geometry {
        GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
        GeometryData::Polygon(v) => v.clone(),
        other => panic!("宿主余部应为多边形，实得 {other:?}"),
    };
    let hb = bbox(&host_outer);
    assert!(
        hb.0 >= 72.0 && hb.1 >= 72.0 && hb.2 <= 488.0 && hb.3 <= 488.0,
        "宿主余部必须留在原宿主 80..480 wu 内，实得 bbox {hb:?}"
    );
    let new_geo = w
        .faction_geometry
        .iter()
        .find(|g| g.owner == Id::new_unchecked("faction_new"))
        .expect("新势力疆界");
    let new_outer = match &new_geo.geometry {
        GeometryData::Polygon(v) => v.clone(),
        GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
        other => panic!("新势力应为多边形，实得 {other:?}"),
    };
    let nb = bbox(&new_outer);
    assert!(
        nb.0 >= 152.0 && nb.1 >= 152.0 && nb.2 <= 328.0 && nb.3 <= 328.0,
        "新势力必须落在意图 160..320 wu 附近，实得 bbox {nb:?}"
    );
    // 余部面积应约为宿主减新势力（不是整张图）。
    let area = |flat: &[f32]| -> f32 {
        let n = flat.len() / 2;
        let mut a = 0.0;
        for i in 0..n {
            let (x0, y0) = (flat[2 * i], flat[2 * i + 1]);
            let (x1, y1) = (flat[2 * ((i + 1) % n)], flat[2 * ((i + 1) % n) + 1]);
            a += x0 * y1 - x1 * y0;
        }
        (a / 2.0).abs()
    };
    let host_area = area(&host_outer);
    assert!(
        (150_000.0..=170_000.0).contains(&host_area),
        "宿主余部外环应 ≈400×400=160000 wu²（不是整图），实得 {host_area}"
    );
    let GeometryData::PolygonWithHoles { holes, .. } = &host.geometry else {
        panic!("居中切应产生带洞余部，实得 {:?}", host.geometry);
    };
    let hole_area = area(&holes[0]);
    assert!(
        (24_000.0..=28_000.0).contains(&hole_area),
        "洞应 ≈160×160=25600 wu²，实得 {hole_area}"
    );
}

#[test]
fn divide_undo_restores_host_and_removes_new_faction_bitwise() {
    let mut w = world_with_host_faction();
    let factions_before = w.factions.clone();
    let geometry_before = w.faction_geometry.clone();
    let cmd = DivideFactionCommand::new(
        test_map(),
        Some(Id::new_unchecked("faction_host")),
        vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_new").as_str(), "新国"),
        0,
        1.0,
    );
    cmd.apply(&mut w).unwrap();
    cmd.revert(&mut w).unwrap();
    assert_eq!(w.factions, factions_before, "revert 应移除新势力");
    assert_eq!(
        w.faction_geometry, geometry_before,
        "revert 应逐位还原宿主疆界"
    );
}

/// 全陆地地形（格 1 wu）：空势力宿主的划分对象。
fn world_with_land_terrain() -> World {
    let mut w = world_with_terrain(64);
    let land: Vec<TerrainCellData> = vec![
        TerrainCellData {
            height: 100,
            surface: 1,
            biome: 3,
        };
        64 * 64
    ];
    w.set_terrain(
        test_map(),
        TerrainData {
            width: 64,
            height: 64,
            cells: land,
        },
    );
    w
}

#[test]
fn divide_from_null_host_writes_no_host_span() {
    let mut w = world_with_land_terrain();
    let before_count = w.faction_geometry.len();
    let cmd = DivideFactionCommand::new(
        test_map(),
        None,
        vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_new").as_str(), "新国"),
        0,
        1.0,
    );
    cmd.apply(&mut w).expect("空势力划分应成功");
    assert_eq!(w.factions.len(), 1);
    assert_eq!(w.faction_geometry.len(), before_count + 1, "只加新势力疆界");
}

#[test]
fn null_host_excludes_water_from_the_new_faction() {
    // 意图正中有一片水（湖/海）：新势力应带洞（水域归空势力，D4/Q3）。
    let mut w = world_with_land_terrain();
    let mut cells = w.terrain_of(&test_map()).cloned().unwrap().cells;
    for y in 26..38 {
        for x in 26..38 {
            let c = &mut cells[y * 64 + x];
            c.height = -20;
            c.surface = 0;
            c.biome = 0;
        }
    }
    w.set_terrain(
        test_map(),
        TerrainData {
            width: 64,
            height: 64,
            cells,
        },
    );
    let cmd = DivideFactionCommand::new(
        test_map(),
        None,
        vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_new").as_str(), "环湖国"),
        0,
        1.0,
    );
    cmd.apply(&mut w).expect("环湖划分应成功");
    let geo = &w.faction_geometry[0];
    let GeometryData::PolygonWithHoles { holes, .. } = &geo.geometry else {
        panic!("环湖国应带洞，实得 {:?}", geo.geometry);
    };
    assert_eq!(holes.len(), 1, "湖应成为一个洞");
}

#[test]
fn divide_fragmenting_intent_is_precondition_error() {
    let mut w = world_with_host_faction();
    // 宿主改成哑铃形：两间房 + 1 格走廊（横切走廊 → 余部碎块化）。
    w.faction_geometry[0].geometry = GeometryData::Polygon(vec![
        4.0, 4.0, 24.0, 4.0, 24.0, 24.0, 26.0, 24.0, 26.0, 4.0, 46.0, 4.0, 46.0, 44.0, 26.0, 44.0,
        26.0, 24.0, 24.0, 24.0, 24.0, 44.0, 4.0, 44.0,
    ]);
    let cmd = DivideFactionCommand::new(
        test_map(),
        Some(Id::new_unchecked("faction_host")),
        vec![(22.0, 0.0), (28.0, 0.0), (28.0, 50.0), (22.0, 50.0)],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_new").as_str(), "新国"),
        0,
        1.0,
    );
    let err = cmd.apply(&mut w).expect_err("碎块化必须被拒绝");
    assert!(
        matches!(err, EditError::Precondition(_)),
        "应是 Precondition，实得 {err:?}"
    );
    assert_eq!(w.factions.len(), 1, "失败不得改动世界");
    assert_eq!(w.faction_geometry.len(), 1, "失败不得改动疆界");
}

#[test]
fn divide_duplicate_faction_id_is_precondition_error() {
    let mut w = world_with_host_faction();
    let cmd = DivideFactionCommand::new(
        test_map(),
        Some(Id::new_unchecked("faction_host")),
        vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
        DivisionMode::Straight,
        f(Id::new_unchecked("faction_host").as_str(), "重名"),
        0,
        1.0,
    );
    assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
}

#[test]
fn add_faction_layer_apply_then_revert_removes() {
    let mut w = World::default();
    let cmd = AddFactionLayerCommand::new(layer("layer_admin"));
    let dirty = cmd.apply(&mut w).unwrap();
    assert_eq!(w.faction_layers.len(), 1);
    assert!(dirty.contains(&"faction_layers".to_string()));
    cmd.revert(&mut w).unwrap();
    assert!(w.faction_layers.is_empty());
}

#[test]
fn update_faction_layer_round_trip() {
    let mut w = World::default();
    AddFactionLayerCommand::new(layer("layer_admin"))
        .apply(&mut w)
        .unwrap();
    let cmd = UpdateFactionLayerCommand::new(
        Id::new_unchecked("layer_admin"),
        "行政区".into(),
        5,
        FactionColorMode::Inherit,
        false,
    );
    cmd.apply(&mut w).unwrap();
    assert_eq!(w.faction_layers[0].name, "行政区");
    assert_eq!(w.faction_layers[0].z, 5);
    assert_eq!(w.faction_layers[0].color_mode, FactionColorMode::Inherit);
    assert!(!w.faction_layers[0].visible);
    cmd.revert(&mut w).unwrap();
    assert_eq!(w.faction_layers[0].name, "layer_admin");
    assert_eq!(w.faction_layers[0].z, 0);
}

#[test]
fn remove_faction_layer_reassigns_and_undo_restores() {
    let mut w = World::default();
    AddFactionLayerCommand::new(layer("layer_admin"))
        .apply(&mut w)
        .unwrap();
    let mut faction = f("f1", "F1");
    faction.layer = Some(Id::new_unchecked("layer_admin"));
    w.factions.push(faction);

    let cmd = RemoveFactionLayerCommand::new(Id::new_unchecked("layer_admin"));
    cmd.apply(&mut w).unwrap();
    assert!(w.faction_layers.is_empty());
    assert_eq!(w.factions[0].layer, None, "删层不得删势力，成员回落默认层");

    cmd.revert(&mut w).unwrap();
    assert_eq!(w.faction_layers.len(), 1);
    assert_eq!(
        w.factions[0].layer.as_ref().map(|i| i.as_str()),
        Some("layer_admin")
    );
}

#[test]
fn set_faction_layer_rank_and_seat_round_trip() {
    let mut w = World::default();
    w.factions.push(f("f1", "F1"));

    let set_layer = SetFactionLayerCommand::new(
        Id::new_unchecked("f1"),
        Some(Id::new_unchecked("layer_admin")),
    );
    set_layer.apply(&mut w).unwrap();
    assert_eq!(
        w.factions[0].layer.as_ref().map(|i| i.as_str()),
        Some("layer_admin")
    );
    set_layer.revert(&mut w).unwrap();
    assert_eq!(w.factions[0].layer, None);

    let set_rank = SetFactionRankCommand::new(Id::new_unchecked("f1"), Some("province".into()));
    set_rank.apply(&mut w).unwrap();
    assert_eq!(w.factions[0].rank.as_deref(), Some("province"));
    set_rank.revert(&mut w).unwrap();
    assert_eq!(w.factions[0].rank, None);

    let set_seat =
        SetFactionSeatCommand::new(Id::new_unchecked("f1"), Some(Id::new_unchecked("p1")));
    set_seat.apply(&mut w).unwrap();
    assert_eq!(w.factions[0].seat.as_ref().map(|i| i.as_str()), Some("p1"));
    set_seat.revert(&mut w).unwrap();
    assert_eq!(w.factions[0].seat, None);
}

#[test]
fn add_membership_keeps_same_target_with_different_from() {
    let mut w = World::default();
    w.factions.push(f("f1", "F1"));
    AddMembershipCommand::new(Id::new_unchecked("f1"), mem("f2", 1))
        .apply(&mut w)
        .unwrap();
    AddMembershipCommand::new(Id::new_unchecked("f1"), mem("f2", 2))
        .apply(&mut w)
        .unwrap();
    assert_eq!(
        w.factions[0].memberships.len(),
        2,
        "同一 target 不同 from 是两条独立边，不得合并"
    );
}

#[test]
fn remove_membership_by_target_and_from_round_trip() {
    let mut w = World::default();
    let mut faction = f("f1", "F1");
    faction.memberships.push(mem("f2", 1));
    faction.memberships.push(mem("f2", 2));
    w.factions.push(faction);

    let cmd = RemoveMembershipCommand::new(Id::new_unchecked("f1"), Id::new_unchecked("f2"), 1);
    cmd.apply(&mut w).unwrap();
    assert_eq!(w.factions[0].memberships.len(), 1);
    assert_eq!(w.factions[0].memberships[0].span.from, 2);
    cmd.revert(&mut w).unwrap();
    assert_eq!(w.factions[0].memberships.len(), 2);
}

#[test]
fn upsert_geometry_keeps_claim_and_control_with_same_from() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    let mut control = geo("m", "f1", 1, None);
    control.relation = None;
    let mut claim = geo("m", "f1", 1, None);
    claim.relation = Some("claim".into());
    UpsertFactionGeometryCommand::new(control)
        .apply(&mut w)
        .unwrap();
    UpsertFactionGeometryCommand::new(claim)
        .apply(&mut w)
        .unwrap();
    assert_eq!(
        w.faction_geometry.len(),
        2,
        "同 map+owner+from 的 claim/control 必须并存（ADR-0027 D5）"
    );
}

#[test]
fn add_faction_apply_then_revert_removes() {
    let mut w = World::default();
    let cmd = AddFactionCommand::new(f("f1", "F1"));
    let dirty = cmd.apply(&mut w).unwrap();
    assert_eq!(w.factions.len(), 1);
    assert!(dirty.contains(&"factions".to_string()));
    cmd.revert(&mut w).unwrap();
    assert!(w.factions.is_empty());
}

#[test]
fn add_faction_duplicate_id_is_precondition_error() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    let err = AddFactionCommand::new(f("f1", "F1-again")).apply(&mut w);
    assert!(matches!(err, Err(EditError::Precondition(_))));
}

#[test]
fn remove_faction_also_clears_its_geometry() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
        .apply(&mut w)
        .unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
        .apply(&mut w)
        .unwrap();
    assert_eq!(w.faction_geometry.len(), 2);

    let rm = RemoveFactionCommand::new(Id::new_unchecked("f1"));
    rm.apply(&mut w).unwrap();
    assert!(w.factions.is_empty(), "Faction 自身被删");
    assert!(w.faction_geometry.is_empty(), "名下几何连带清空");

    rm.revert(&mut w).unwrap();
    assert_eq!(w.factions.len(), 1, "Faction 还原");
    assert_eq!(w.faction_geometry.len(), 2, "几何还原");
}

#[test]
fn remove_faction_missing_is_target_error() {
    let mut w = World::default();
    let cmd = RemoveFactionCommand::new(Id::new_unchecked("ghost"));
    assert!(matches!(
        cmd.apply(&mut w),
        Err(EditError::TargetMissing(_))
    ));
}

#[test]
fn update_faction_round_trips_via_revert() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    let upd = UpdateFactionCommand::new(
        Id::new_unchecked("f1"),
        "F1-Renamed".into(),
        wv_core::color::Color("#FF0000".into()),
    );
    upd.apply(&mut w).unwrap();
    assert_eq!(w.factions[0].name, "F1-Renamed");
    assert_eq!(w.factions[0].color.as_str(), "#FF0000");
    upd.revert(&mut w).unwrap();
    assert_eq!(w.factions[0].name, "F1");
    assert_eq!(w.factions[0].color.as_str(), "#112233");
}

#[test]
fn upsert_geometry_inserts_when_new() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    let upsert = UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)));
    let dirty = upsert.apply(&mut w).unwrap();
    assert_eq!(w.faction_geometry.len(), 1);
    assert!(dirty.contains(&"geometry/faction".to_string()));
    // revert：应能移除刚 push 的那条
    upsert.revert(&mut w).unwrap();
    assert!(w.faction_geometry.is_empty());
}

#[test]
fn upsert_geometry_replaces_when_same_key() {
    // (map, owner, from) 一致 → 替换。`from` 不同的算作"另一版本" → 共存。
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
        .apply(&mut w)
        .unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(20)))
        .apply(&mut w)
        .unwrap();
    assert_eq!(w.faction_geometry.len(), 1, "from 相同 → 替换");
    assert_eq!(w.faction_geometry[0].span.to, Some(20));
    UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
        .apply(&mut w)
        .unwrap();
    assert_eq!(w.faction_geometry.len(), 2, "from 不同 → 共存");
}

#[test]
fn upsert_geometry_revert_restores_replaced_value() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    let original = geo("m", "f1", 1, Some(10));
    UpsertFactionGeometryCommand::new(original.clone())
        .apply(&mut w)
        .unwrap();
    let updated = geo("m", "f1", 1, Some(99));
    let upd_cmd = UpsertFactionGeometryCommand::new(updated);
    upd_cmd.apply(&mut w).unwrap();
    assert_eq!(w.faction_geometry[0].span.to, Some(99));
    upd_cmd.revert(&mut w).unwrap();
    assert_eq!(w.faction_geometry[0].span.to, Some(10), "应恢复为 10");
}

#[test]
fn upsert_geometry_rejects_orphan_owner() {
    let mut w = World::default();
    // 没有 add Faction。
    let cmd = UpsertFactionGeometryCommand::new(geo("m", "f_ghost", 1, Some(10)));
    assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
}

#[test]
fn remove_geometry_apply_then_revert_restores_at_original_index() {
    let mut w = World::default();
    AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
        .apply(&mut w)
        .unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
        .apply(&mut w)
        .unwrap();
    UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
        .apply(&mut w)
        .unwrap();
    // 删中间那条 (from=20)。
    let rm = RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 20);
    rm.apply(&mut w).unwrap();
    assert_eq!(w.faction_geometry.len(), 2);
    assert!(w.faction_geometry.iter().all(|g| g.span.from != 20));
    rm.revert(&mut w).unwrap();
    assert_eq!(w.faction_geometry.len(), 3);
    assert_eq!(w.faction_geometry[1].span.from, 20, "按原位置 1 还原");
}

#[test]
fn remove_geometry_missing_is_target_error() {
    let mut w = World::default();
    let cmd =
        RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 999);
    assert!(matches!(
        cmd.apply(&mut w),
        Err(EditError::TargetMissing(_))
    ));
}

// ---------------------- 街道命令（ADR-0010 ③） ----------------------
