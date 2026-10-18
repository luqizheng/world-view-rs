//! 覆盖物地界的**提示级**校验（AGENTS §7.5 V1：只提示，不阻止保存）。
//!
//! 校验对象是「`Place.feature` 声明了某类地理事实，但地界内没有对应的地形格」：
//! 作者圈了一片森林，却没刷森林笔刷——远带晕与名字照常可见，近景符号则**回退**
//! 到按陆地格（`height > 0`）泛化布点（`forest_sites` 的两遍规则），而不是只落在
//! 森林格上。这属于「你可能没注意到」，不是数据损坏，因此永远是 `DiagLevel::Hint`。
//!
//! 另外守住孤儿地界（`owner` 不在 `distances.places` 里）也要被**列出**而
//! 不是静默丢弃（与 `place_territory_at` 的「静默跳过」分工：求值不报错，
//! 校验面板要看见）。

use wv_core::distance::Place;
use wv_core::map::{Extent, GeometryData, GeometrySpan, MapLevel, MapNode};
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::{Id, TimeSpan, World};
use wv_io::diagnostics::{detect_feature_territory_hints, DiagLevel};

/// 森林的地理事实编号（`wv_render::theme::overlays::FEATURE_FOREST` 的字面量；
/// wv-io 不依赖 wv-render，测试同样只引用语义编号）。
const FOREST: u8 = 4;
/// 草原（非森林）格子的群系编号（biome 冻结表）。
const GRASSLAND_BIOME: u8 = 3;
/// 森林格子的群系编号（biome 冻结表；feature 声明值 4 经 v1 迁移表映射到此）。
const FOREST_BIOME: u8 = 1;

fn id(s: &str) -> Id {
    Id::new_unchecked(s)
}

fn map_node() -> MapNode {
    MapNode {
        id: id("map_world"),
        name: "世界".into(),
        level: MapLevel::World,
        extent: Some(Extent::sized(100.0, 100.0)),
        ..MapNode::default()
    }
}

fn forest_place() -> Place {
    Place {
        id: id("place_forest"),
        name: "黑森林".into(),
        feature: Some(FOREST),
        biome: None,
        envelope: None,
        map: Some(id("map_world")),
        x: Some(50.0),
        y: Some(50.0),
        w: Some(60.0),
        h: Some(60.0),
        rank: None,
    }
}

/// 地图局部坐标 `[20, 80]²` 的正方形地界（格心 25/35/…/75 严格落在环内）。
fn square_span(owner: &str) -> GeometrySpan {
    GeometrySpan {
        map: id("map_world"),
        owner: id(owner),
        span: TimeSpan { from: 1, to: None },
        geometry: GeometryData::Polygon(vec![20.0, 20.0, 80.0, 20.0, 80.0, 80.0, 20.0, 80.0]),
        relation: None,
    }
}

/// 10×10 全同群系的陆地栅格（世界 100×100 → 每格 10 wu，格心 5/15/…/95）。
fn uniform_terrain(biome: u8) -> TerrainData {
    TerrainData {
        width: 10,
        height: 10,
        cells: vec![
            TerrainCellData {
                height: 10,
                surface: 1,
                biome,
            };
            100
        ],
    }
}

/// 10×10 栅格：只有地界内的中央 6×6 格是森林，其余草原。
fn patched_forest_terrain() -> TerrainData {
    let mut t = uniform_terrain(GRASSLAND_BIOME);
    for gy in 2..8usize {
        for gx in 2..8usize {
            t.cells[gy * t.width as usize + gx].biome = FOREST_BIOME;
        }
    }
    t
}

fn world_with(place: Option<Place>, span: GeometrySpan, terrain: Option<TerrainData>) -> World {
    let mut w = World::default();
    w.maps.push(map_node());
    if let Some(p) = place {
        w.distances.places.push(p);
    }
    w.place_geometry.push(span);
    if let Some(t) = terrain {
        w.set_terrain(id("map_world"), t);
    }
    w
}

/// (a) 全草原地形 → 恰好 1 条 Hint，指纹含 place id，文案含名字且声明不阻止保存。
#[test]
fn forest_territory_without_forest_cells_emits_hint() {
    let w = world_with(
        Some(forest_place()),
        square_span("place_forest"),
        Some(uniform_terrain(GRASSLAND_BIOME)),
    );
    let hints = detect_feature_territory_hints(&w);
    assert_eq!(
        hints.len(),
        1,
        "零森林格应恰好 1 条提示，实得 {}",
        hints.len()
    );
    let h = &hints[0];
    assert_eq!(h.level, DiagLevel::Hint, "覆盖物缺失只能提示，不能是错误");
    assert!(
        h.fingerprint.contains("place_forest"),
        "指纹应含 place id 以便认领：got {}",
        h.fingerprint
    );
    assert!(!h.message.is_empty(), "文案不得为空");
    assert!(
        h.message.contains("黑森林"),
        "文案应含地点名：got {}",
        h.message
    );
    assert!(
        h.message.contains("保存"),
        "文案应声明不阻止保存（V1）：got {}",
        h.message
    );
    assert!(
        h.message.contains("回退") && h.message.contains("陆地"),
        "文案应说明近景回退到陆地格布点，而不是声称没有符号：got {}",
        h.message
    );
}

/// 标记（ADR-0029）：有 `biome` 时按群系编号直接比较，不走旧 feature 词汇。
#[test]
fn biome_marker_compares_cell_biome_directly() {
    let mut place = forest_place();
    place.biome = Some(GRASSLAND_BIOME); // 声称草原
    place.feature = None;
    let w = world_with(
        Some(place),
        square_span("place_forest"),
        Some(uniform_terrain(GRASSLAND_BIOME)),
    );
    assert!(
        detect_feature_territory_hints(&w).is_empty(),
        "包络内已是草原 → 不应提示缺失"
    );

    // 同一标记换成森林群系，而地形是草原 → 应提示。
    let mut place = forest_place();
    place.biome = Some(FOREST_BIOME);
    place.feature = None;
    let w = world_with(
        Some(place),
        square_span("place_forest"),
        Some(uniform_terrain(GRASSLAND_BIOME)),
    );
    assert_eq!(
        detect_feature_territory_hints(&w).len(),
        1,
        "群系不匹配应提示"
    );
}

/// (b) 地界内有森林格 → 不发提示。
#[test]
fn forest_territory_with_forest_cells_emits_no_hint() {
    let w = world_with(
        Some(forest_place()),
        square_span("place_forest"),
        Some(patched_forest_terrain()),
    );
    let hints = detect_feature_territory_hints(&w);
    assert!(
        hints.is_empty(),
        "地界内存在森林格时不应发提示，实得 {} 条",
        hints.len()
    );
}

/// (c) 孤儿地界（owner 不在 places）→ 1 条 Hint（不是 Error）。
#[test]
fn orphan_territory_emits_hint_without_blocking() {
    let w = world_with(None, square_span("place_ghost"), None);
    let hints = detect_feature_territory_hints(&w);
    assert_eq!(hints.len(), 1, "孤儿地界应恰好 1 条提示");
    let h = &hints[0];
    assert_eq!(
        h.level,
        DiagLevel::Hint,
        "孤儿地界是提示，不得升级为错误（AGENTS §7.5 V1）"
    );
    assert!(
        h.fingerprint.contains("place_ghost"),
        "指纹应含孤儿 owner：got {}",
        h.fingerprint
    );
    assert!(!h.message.is_empty(), "文案不得为空");
}

/// (d) 该图没有自带栅格（继承底图不在提示范围）→ 不发提示。
#[test]
fn no_terrain_emits_no_hint() {
    let w = world_with(Some(forest_place()), square_span("place_forest"), None);
    let hints = detect_feature_territory_hints(&w);
    assert!(
        hints.is_empty(),
        "无栅格时无法判断 kind，应静默跳过（晕仍可见），实得 {} 条",
        hints.len()
    );
}

/// `derive_hints` 必须把覆盖物提示并入统一面板（去重后仍只有一条）。
#[test]
fn derive_hints_includes_feature_territory_hints() {
    let w = world_with(
        Some(forest_place()),
        square_span("place_forest"),
        Some(uniform_terrain(GRASSLAND_BIOME)),
    );
    let d = wv_io::derive_hints(&w);
    let matching: Vec<_> = d
        .items
        .iter()
        .filter(|x| x.fingerprint == "feature-no-cells|place_forest")
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "derive_hints 应并入覆盖物提示且按指纹去重，实得 {} 条",
        matching.len()
    );
    assert_eq!(matching[0].level, DiagLevel::Hint);
    assert!(
        !d.has_errors(),
        "派生提示路径永不产生错误（AGENTS §7.5 V1）"
    );
}

/// (e) 极端有限坐标 × 极小 extent：坐标 × 缩放溢出 f32 时不得 panic。
///
/// `3e30 * 1e31` 溢出为正无穷，`as i64` 饱和到 `i64::MAX`；
/// 旧实现的 `+ 1` / `- 1` 会在 debug 构建溢出 panic。畸形 YAML 不能炸掉诊断。
#[test]
fn extreme_coordinates_do_not_panic_on_cell_range_math() {
    let mut span = square_span("place_forest");
    span.geometry = GeometryData::Polygon(vec![
        -3.0e30, -3.0e30, 3.0e30, -3.0e30, 3.0e30, 3.0e30, -3.0e30, 3.0e30,
    ]);
    let mut w = world_with(
        Some(forest_place()),
        span,
        Some(uniform_terrain(GRASSLAND_BIOME)),
    );
    // 极小正 extent：scale = 10 / 1e-30 = 1e31，与巨大环坐标相乘必然溢出。
    w.maps[0].extent = Some(Extent::sized(1.0e-30, 1.0e-30));
    let hints = detect_feature_territory_hints(&w);
    assert_eq!(
        hints.len(),
        1,
        "全草原地形应得 1 条提示（且不得 panic），实得 {} 条",
        hints.len()
    );
    assert_eq!(hints[0].level, DiagLevel::Hint);
}
