#![cfg(feature = "app")]
//! 独立验收测试 —— 群系视图（`ViewState::nature_overlay`）稳态零重建 / 关闭即清零
//! （QA 严过关，2026-10-23 第 3 轮 · 用户实测：静止 120 FPS / 鼠标移动 40 FPS /
//! 开群系视图 26 FPS / 关群系视图后顶点手柄残留）。
//!
//! ## 口径（写断言前先过的四条）
//! - **独立复算**：期望值一律是字面量（`0` / `1` / `60` / `200`），不引用被测常量、
//!   不用被测函数算期望。
//! - **计数器而非墙钟**：数的是 `RegionOutlinePool::seg_recomputes` /
//!   `stroke_rebuilds`、实体数、`Assets<Mesh>` 条数、网格内容指纹。跨机器同值，
//!   debug 构建的毫秒**不作判据**。
//! - **负向断言**：该消失的东西必须**计数为 0**，不是「没报错」。
//! - **红绿约定**：`req_` = 需求断言（修复前**应当失败**，就是缺陷证据，不许
//!   `#[ignore]`、不许放宽）；`neg_` / `ctl_` = 负向与阳性对照（现在就必须绿）。
//!
//! ## 自验（判定工具先自验才许对他人下结论）
//! 计数器本身要先证明「能数出数」：`ctl_counter_moves_on_a_real_data_change`
//! （**应命中**：改多边形 ⇒ `seg_recomputes` +1、`stroke_rebuilds` ≥1）与
//! `ctl_steady_frames_do_not_move_the_counters`（**应不命中**：什么都不改 ⇒ Δ 为 0）
//! 是一对。两条都绿，才说明后面「数出 0」不是恒 0 的假绿。
//!
//! ## 本文件**测不到**什么（诚实标注，别当成「验过了」）
//! 2026-10-24 起 `RegionDecorCache::handles_drawn`（每帧手柄数）与
//! `outline_segments_drawn`（每帧描边数）改由 `region_decor::sync_region_decor`
//! 写入（阶梯描边 + 实心白方块锚点已从 `draw_map_gizmos` 迁出成合批 mesh）——
//! 该系统**无头可跑**，故这两项的「绘制数」现由
//! `region_decor::tests::*`（8 条）负责，本文件仍只管判定谓词
//! [`region_handles_visible`] 与计划（`RegionHandlePlan`）的形态。
//!
//! 运行：`cargo test -p wv-render --features app --test acceptance_nature_overlay`

use bevy::prelude::*;
use wv_core::map::{Extent, MapLevel, MapNode};
use wv_core::model::region::{Region, RegionKind};
use wv_core::{EntityKind, Id, SelectedEntity, World};
use wv_render::layer_frame::{LayerBinding, LayerFrame};
use wv_render::recipe::{PassId, RenderRecipe};
use wv_render::region_outline::{sync_region_outlines, CandidateRegionInput, RegionOutlinePool};
use wv_render::region_overlay::RegionOverlayIndex;
use wv_render::state::{
    DrillState, GridConfig, RegionAdjustState, RegionCandidateContext, ViewMode, ViewState,
    WorldProjection,
};
use wv_render::world_projection::{
    plan_region_handles, region_handles_visible, RegionDecorCache, RegionHandlePlan,
};

/// 手算：lod-demo `extent 4096 / raster 1024` = 4.0 世界单位/格。
const CELL_SIZE: f32 = 4.0;
/// 手算：`max(4096/1280, 4096/800)` = 5.12（世界级「全图适配」）。
const FIT_SCALE: f32 = 5.12;
/// 区域数两档（需求原文：10 与 200）。
const N_SMALL: usize = 10;
const N_LARGE: usize = 200;
/// 移动 / 静止帧数（需求原文：60 帧）。
const FRAMES: usize = 60;
/// 开关轮次（需求原文：20 次）。
const TOGGLE_ROUNDS: usize = 20;

const MAP: &str = "map_world";

// ══════════════════════════ harness ══════════════════════════

fn map_node() -> MapNode {
    MapNode {
        id: Id::new_unchecked(MAP),
        name: MAP.to_owned(),
        level: MapLevel::World,
        parent: None,
        children: Vec::new(),
        layers: Vec::new(),
        icons: Vec::new(),
        extent: Some(Extent::sized(4096.0, 4096.0)),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    }
}

/// 第 `i` 个自然区：一个平移过的方形环（每区几何都不同）。
fn nature_region(i: usize) -> Region {
    let x = 40.0 + (i % 40) as f32 * 96.0;
    let y = 40.0 + (i / 40) as f32 * 96.0;
    Region {
        id: Id::new_unchecked(format!("nr_{i}")),
        name: format!("自然区{i}"),
        map: Id::new_unchecked(MAP),
        polygon: vec![x, y, x + 64.0, y, x + 64.0, y + 64.0, x, y + 64.0],
        holes: Vec::new(),
        kind: RegionKind::Nature,
        parent: None,
        overlay: None,
    }
}

fn world_with(n_regions: usize) -> World {
    World {
        maps: vec![map_node()],
        regions: (0..n_regions).map(nature_region).collect(),
        ..Default::default()
    }
}

/// 只装 `sync_region_outlines` 的最小 App（无渲染插件，纯 ECS + Assets）。
fn region_app(n_regions: usize, nature_overlay: bool, scale: f32) -> App {
    let world = world_with(n_regions);
    let mut drill = DrillState::default();
    drill.settle_on(Id::new_unchecked(MAP));

    let mut recipe = RenderRecipe::default();
    recipe.passes[PassId::Terrain as usize].enabled = true;
    recipe.passes[PassId::RegionLayers as usize].enabled = true;

    let mut app = App::new();
    app.insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<ColorMaterial>::default())
        .insert_resource(WorldProjection {
            world,
            ..Default::default()
        })
        .insert_resource(drill)
        .insert_resource(GridConfig {
            cell_size: CELL_SIZE,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        })
        .insert_resource(ViewState {
            mode: ViewMode::Topographic,
            nature_overlay,
            scale,
            ..Default::default()
        })
        .insert_resource(LayerFrame {
            terrain: LayerBinding {
                visible: true,
                opacity: 1.0,
                z_offset: 0.0,
            },
            ..Default::default()
        })
        .insert_resource(recipe)
        .insert_resource(RegionOverlayIndex::default())
        .init_resource::<CandidateRegionInput>()
        .init_resource::<RegionOutlinePool>()
        .add_systems(Update, sync_region_outlines);
    app
}

// ══════════════════════════ 计数器 ══════════════════════════

/// `(segs 重算次数, build_stroke_batch_mesh 调用次数)`。
fn counters(app: &App) -> (u64, u64) {
    let p = app.world().resource::<RegionOutlinePool>();
    (p.seg_recomputes, p.stroke_rebuilds)
}

/// 区域轮廓相关的 `Mesh2d` 实体数（本 App 里 `Mesh2d` 的唯一来源就是轮廓层）。
fn mesh_entities(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<&Mesh2d>();
    q.iter(app.world()).count()
}

/// 轮廓层的 mesh 句柄（取第一个；无实体返回 `None`）。
fn first_mesh_id(app: &mut App) -> Option<AssetId<Mesh>> {
    let mut q = app.world_mut().query::<&Mesh2d>();
    q.iter(app.world()).next().map(|m| m.0.id())
}

/// `Assets<Mesh>` 条数——**不是句柄数**：这是「删没删」的唯一判据。
fn mesh_assets(app: &App) -> usize {
    app.world().resource::<Assets<Mesh>>().len()
}

/// 网格**内容指纹**（顶点位置 FNV）。内容变了 ⇒ 几何被重写过（重建）。
///
/// 为什么还要它：`stroke_rebuilds` 只证明「函数被调了」，指纹证明「几何真的变了」。
/// 两个独立口径互证，任一说谎都能被对方抓到。
fn mesh_signature(app: &App, id: AssetId<Mesh>) -> Option<u64> {
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mesh = meshes.get(id)?;
    let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |f: f32| {
        h ^= u64::from(f.to_bits());
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    match pos {
        bevy::mesh::VertexAttributeValues::Float32x3(v) => {
            for p in v {
                for c in p {
                    mix(*c);
                }
            }
        }
        bevy::mesh::VertexAttributeValues::Float32x2(v) => {
            for p in v {
                for c in p {
                    mix(*c);
                }
            }
        }
        _ => return None,
    }
    Some(h)
}

fn selection_of(id: &str) -> Option<SelectedEntity> {
    Some(SelectedEntity::new(
        EntityKind::Region,
        Id::new_unchecked(id),
    ))
}

/// 一帧「鼠标在地图上移动」：视图态每帧都变（光标位置），数据不变。
fn move_mouse(app: &mut App, frame: usize) {
    app.world_mut().resource_mut::<ViewState>().cursor_world = Some(Vec2::new(
        100.0 + frame as f32 * 7.0,
        200.0 + frame as f32 * 3.0,
    ));
    // 光标不在待物化候选上：避免把「候选预览本来就该跟着光标变」混进来。
    app.world_mut().resource_mut::<CandidateRegionInput>().ring = None;
}

// ══════════════════════════ 计数器自验（现在必须绿） ══════════════════════════

/// CTL-1（自验·**命中**样本）：真的改了多边形 ⇒ `seg_recomputes` 与
/// `stroke_rebuilds` 必须都动，且 mesh 内容指纹必须变。
///
/// 没有这条，「Δ 为 0」可能只是计数器坏掉的假绿。
#[test]
fn ctl_counter_moves_on_a_real_data_change() {
    let mut app = region_app(1, true, FIT_SCALE);
    app.update();
    let (seg0, stroke0) = counters(&app);
    let id = first_mesh_id(&mut app).expect("应有轮廓 mesh");
    let sig0 = mesh_signature(&app, id).expect("应能取指纹");

    app.world_mut()
        .resource_mut::<WorldProjection>()
        .world
        .regions[0]
        .polygon = vec![8.0, 8.0, 72.0, 8.0, 72.0, 72.0, 8.0, 72.0];
    app.update();

    let (seg1, stroke1) = counters(&app);
    assert_eq!(seg1 - seg0, 1, "改一个区应触发 1 次 segs 重算");
    assert!(stroke1 > stroke0, "改几何必须重建 stroke mesh");
    assert_ne!(
        sig0,
        mesh_signature(&app, id).expect("应还能取指纹"),
        "改了多边形而轮廓 mesh 内容没变 —— 指纹判据失效"
    );
}

/// CTL-2（自验·**不命中**样本）：视图态、数据都不变的连续帧 ⇒ 两个计数器都不动。
#[test]
fn ctl_steady_frames_do_not_move_the_counters() {
    let mut app = region_app(N_LARGE, true, FIT_SCALE);
    app.update();
    let (seg0, stroke0) = counters(&app);
    for _ in 0..FRAMES {
        app.update();
    }
    assert_eq!(
        counters(&app),
        (seg0, stroke0),
        "什么都没变却在重算/重建（Δsegs / Δstroke 必须都是 0）"
    );
}

// ══════════════════════════ 阳性对照（现在必须绿） ══════════════════════════

/// CTL-3：群系视图开 + 1 个自然区 ⇒ 恰好 1 个轮廓实体、1 份 mesh。
///
/// 意义：证明本 harness **数得出 1**，所以「数出 0」不是恒真的假绿。
#[test]
fn ctl_overlay_on_spawns_exactly_one_outline_entity() {
    let mut app = region_app(1, true, FIT_SCALE);
    app.update();
    assert_eq!(mesh_entities(&mut app), 1, "1 个自然区 ⇒ 1 个合批轮廓实体");
    assert_eq!(mesh_assets(&app), 1, "1 个自然区 ⇒ 1 份 mesh");
}

// ══════════════════════════ 负向断言（现在必须绿） ══════════════════════════

/// NEG-1：关 `nature_overlay` ⇒ 轮廓实体数必须 **0**。
#[test]
fn neg_overlay_off_clears_all_outline_entities() {
    for n in [N_SMALL, N_LARGE] {
        let mut app = region_app(n, true, FIT_SCALE);
        app.update();
        assert_eq!(mesh_entities(&mut app), 1, "{n} 区：前置应有 1 个合批实体");

        app.world_mut().resource_mut::<ViewState>().nature_overlay = false;
        app.update();
        assert_eq!(
            mesh_entities(&mut app),
            0,
            "{n} 区：关群系视图后轮廓实体必须 0（不是「没报错」）"
        );
    }
}

/// NEG-2：关 `nature_overlay` ⇒ **选中态必须原样保留**（需求原文：
/// 「隐藏顶点手柄与候选高亮，但选中态保留（`projection.selection` 不清）」）。
///
/// 反向也钉一条：清了 `selection` = 作者「关一次视图就得重新选中」，是可用性倒退。
#[test]
fn neg_overlay_off_keeps_projection_selection() {
    let mut app = region_app(3, true, FIT_SCALE);
    let want = selection_of("nr_1");
    app.world_mut().resource_mut::<WorldProjection>().selection = want.clone();
    app.update();

    app.world_mut().resource_mut::<ViewState>().nature_overlay = false;
    app.update();

    assert_eq!(
        app.world().resource::<WorldProjection>().selection.clone(),
        want,
        "关群系视图不得清掉选中态"
    );
    assert_eq!(mesh_entities(&mut app), 0, "同时轮廓实体仍须为 0");
}

/// NEG-3（用户症状 4 的**判定**面）：手柄可见性谓词必须与 `nature_overlay` 同生共死。
///
/// 两种取值都断言（否则「恒 false」也能过）——这就是「关群系视图后红十字还留着」
/// 这条症状在无头环境里能钉到的最硬一层。
#[test]
fn neg_handle_visibility_follows_the_nature_overlay_switch() {
    assert!(
        !region_handles_visible(ViewMode::Topographic, false),
        "Topographic + 关群系视图 ⇒ 手柄必须不画（用户实测残留的就是它）"
    );
    assert!(
        region_handles_visible(ViewMode::Topographic, true),
        "Topographic + 开群系视图 ⇒ 手柄必须画（否则恒 false 也能过上面那条）"
    );
    assert!(
        !region_handles_visible(ViewMode::VectorFlat, true),
        "非 CellMap 模式不画格点手柄"
    );
}

/// NEG-4：鼠标移动 60 帧 ⇒ 轮廓 mesh **不得被重写**，实体数与 mesh 条数不得变化。
#[test]
fn neg_sixty_mouse_move_frames_do_not_rewrite_the_outline_mesh() {
    let mut app = region_app(N_LARGE, true, FIT_SCALE);
    app.update();
    let id = first_mesh_id(&mut app).expect("应有轮廓 mesh");
    let before = mesh_signature(&app, id).expect("应能取指纹");
    let (seg0, stroke0) = counters(&app);
    let assets_before = mesh_assets(&app);

    for f in 0..FRAMES {
        move_mouse(&mut app, f);
        app.update();
    }

    assert_eq!(
        counters(&app).1,
        stroke0,
        "移动 {FRAMES} 帧触发了 {} 次 stroke 重建（必须 0）",
        counters(&app).1 - stroke0
    );
    assert_eq!(counters(&app).0, seg0, "移动帧里 segs 重算次数不得变化");
    assert_eq!(
        mesh_signature(&app, id),
        Some(before),
        "轮廓几何被重写了（视图变化不该重建几何）"
    );
    assert_eq!(
        mesh_assets(&app),
        assets_before,
        "移动帧里 mesh 条数不得变化"
    );
    assert_eq!(mesh_entities(&mut app), 1, "移动帧里实体数不得变化");
}

// ══════════════════════════ 需求断言（修复前应当失败 = 缺陷证据） ══════════════════════════

/// REQ-1（本轮核心）：「鼠标移动 60 帧」与「静止 60 帧」在**同两个计数器**上
/// 必须**完全相等且都为 0**。
///
/// 前提全部按住：`selection` / 多边形 / 相机（`scale`、`camera_center`）/ 区域数都不变。
/// 只变「光标在世界里的位置」——真实鼠标移动就是这个。
#[test]
fn req_moving_and_stationary_frames_have_identical_zero_recompute_counts() {
    // 静止 60 帧
    let mut still = region_app(N_LARGE, true, FIT_SCALE);
    still.update(); // 首帧允许重算一次（建立缓存）
    let (s0, sb0) = counters(&still);
    for _ in 0..FRAMES {
        still.update();
    }
    let still_delta = (counters(&still).0 - s0, counters(&still).1 - sb0);

    // 移动 60 帧（同一套资源，只有光标在动）
    let mut moving = region_app(N_LARGE, true, FIT_SCALE);
    moving.update();
    let (m0, mb0) = counters(&moving);
    for f in 0..FRAMES {
        move_mouse(&mut moving, f);
        moving.update();
    }
    let moving_delta = (counters(&moving).0 - m0, counters(&moving).1 - mb0);

    assert_eq!(
        still_delta,
        (0, 0),
        "静止 {FRAMES} 帧仍在重算/重建：Δsegs={} Δstroke={}",
        still_delta.0,
        still_delta.1
    );
    assert_eq!(
        moving_delta,
        (0, 0),
        "鼠标移动 {FRAMES} 帧在重算/重建：Δsegs={} Δstroke={}（用户症状：移动掉到 40 FPS）",
        moving_delta.0,
        moving_delta.1
    );
    assert_eq!(
        still_delta, moving_delta,
        "移动与静止的计数不相等——说明「光标位置」被错误地当成了重算的输入"
    );
}

/// REQ-2（规模无关）：`nature_overlay` 开，区域数 10 与 200 两档，稳态每帧
/// `segs` 重算 == **0**；只有首帧（或数据变更）> 0。
#[test]
fn req_steady_state_seg_recompute_is_zero_at_both_scales() {
    for n in [N_SMALL, N_LARGE] {
        let mut app = region_app(n, true, FIT_SCALE);
        let (seg_before_first, _) = counters(&app);
        app.update();
        let (seg_after_first, _) = counters(&app);
        assert_eq!(
            seg_after_first - seg_before_first,
            1,
            "{n} 区：首帧必须恰好重算 1 次（否则计数器本身不可信）"
        );

        for _ in 0..FRAMES {
            app.update();
        }
        assert_eq!(
            counters(&app).0,
            seg_after_first,
            "{n} 区：稳态 {FRAMES} 帧里又重算了 {} 次 segs（必须 0）",
            counters(&app).0 - seg_after_first
        );
    }
}

/// REQ-3：关 `nature_overlay` 必须**回收**轮廓 mesh 资产（`Assets<Mesh>` 归 0）。
///
/// 现状：`despawn_all` 只 `despawn` 实体、**不 `remove` 资产**；句柄一丢资产就成了
/// 无人引用的死条目，常驻到进程结束。200 区时每轮开关泄漏一份全量合批 mesh。
#[test]
fn req_overlay_off_reclaims_outline_mesh_assets() {
    let mut app = region_app(N_LARGE, true, FIT_SCALE);
    app.update();
    assert_eq!(mesh_assets(&app), 1, "前置：开 ⇒ 1 份 mesh");

    app.world_mut().resource_mut::<ViewState>().nature_overlay = false;
    app.update();
    assert_eq!(
        mesh_assets(&app),
        0,
        "关群系视图后 mesh 资产必须回收为 0，实得 {} 份（实体已 despawn，资产没人 remove）",
        mesh_assets(&app)
    );
}

/// REQ-4（防泄漏·可达性极值）：连续开关 `TOGGLE_ROUNDS` 次 + `N_LARGE` 区
/// ⇒ `Assets<Mesh>` **不得累积增长**（每轮关闭后回到同一基线）。
#[test]
fn req_twenty_toggle_rounds_must_not_accumulate_mesh_assets() {
    let mut app = region_app(N_LARGE, true, FIT_SCALE);
    let mut baseline: Option<usize> = None;
    for round in 0..TOGGLE_ROUNDS {
        app.world_mut().resource_mut::<ViewState>().nature_overlay = true;
        app.update();
        assert_eq!(mesh_entities(&mut app), 1, "第 {round} 轮：开 ⇒ 1 实体");
        app.world_mut().resource_mut::<ViewState>().nature_overlay = false;
        app.update();
        assert_eq!(mesh_entities(&mut app), 0, "第 {round} 轮：关 ⇒ 0 实体");

        let off = mesh_assets(&app);
        let base = *baseline.get_or_insert(off);
        assert!(
            off <= base,
            "第 {round} 轮关闭后 mesh 资产 {off} 超过首轮基线 {base}——\
             开关一次泄漏一份全量合批 mesh"
        );
    }
}

// ══════════════════════════ 手柄计划（AC15 / AC17 的计数面） ══════════════════════════
//
// 2026-10-23 第二轮：主程序把 `draw_selected_region` 与候选锚点块抽成纯函数
// `plan_region_handles(...) -> RegionHandlePlan`，缓存由调用方持有的
// `RegionDecorCache` 承载。于是「画了几个手柄」「栅格化了几次」终于可以无头断言
// ——在此之前它们只写在 `draw_map_gizmos`（要 `Gizmos` 参数，无头跑不起来）。

/// 计划用的世界：**一个大方块自然区**（4096 见方 → fit 下 800px，抽稀后仍有多个
/// 锚点，不会被抽成 1 个）+ 32×32 全同 biome 地形（候选派生需要）。
fn plan_world() -> World {
    let id = Id::new_unchecked(MAP);
    let big = Region {
        id: Id::new_unchecked("nr_big"),
        name: "大方块".into(),
        map: id.clone(),
        polygon: vec![0.0, 0.0, 4096.0, 0.0, 4096.0, 4096.0, 0.0, 4096.0],
        holes: Vec::new(),
        kind: RegionKind::Nature,
        parent: None,
        overlay: None,
    };
    let side = 32u32;
    let cell = wv_core::terrain::TerrainCellData {
        height: 100,
        surface: 1,
        biome: 3,
    };
    let mut world = World {
        maps: vec![map_node()],
        regions: vec![big],
        ..Default::default()
    };
    world.terrains.insert(
        id,
        wv_core::terrain::TerrainData {
            width: side,
            height: side,
            cells: vec![cell; (side * side) as usize],
        },
    );
    world
}

/// 计划函数的一整套只读输入（不含缓存——缓存必须由测试自己持有才能数 Δ）。
struct PlanFixture {
    projection: WorldProjection,
    drill: DrillState,
    view: ViewState,
    grid: GridConfig,
    adjust: RegionAdjustState,
    candidate: RegionCandidateContext,
}

impl PlanFixture {
    fn new(nature_overlay: bool, selected: bool, candidate_cell: Option<(i32, i32)>) -> Self {
        let mut drill = DrillState::default();
        drill.settle_on(Id::new_unchecked(MAP));
        Self {
            projection: WorldProjection {
                world: plan_world(),
                selection: if selected {
                    selection_of("nr_big")
                } else {
                    None
                },
                ..Default::default()
            },
            drill,
            view: ViewState {
                mode: ViewMode::Topographic,
                nature_overlay,
                scale: FIT_SCALE,
                ..Default::default()
            },
            grid: GridConfig {
                cell_size: CELL_SIZE,
                raster_origin: Vec2::ZERO,
                map_origin: Vec2::ZERO,
                map_size: Vec2::splat(4096.0),
            },
            adjust: RegionAdjustState::default(),
            candidate: RegionCandidateContext {
                cell: candidate_cell,
                polygon: None,
            },
        }
    }

    fn plan<'a>(&'a self, cache: &'a mut RegionDecorCache) -> RegionHandlePlan<'a> {
        plan_region_handles(
            &self.projection,
            &self.drill,
            &self.view,
            &self.grid,
            &self.adjust,
            &self.candidate,
            cache,
        )
    }
}

/// CTL-4（自验·**命中**样本）：开群系视图 + 选中一个人工区 ⇒ 计划里必须有手柄
/// 与阶梯描边。没有这条，「关 overlay ⇒ 0 个手柄」可能只是计划恒空。
#[test]
fn ctl_handle_plan_is_populated_when_overlay_on() {
    let f = PlanFixture::new(true, true, None);
    let mut cache = RegionDecorCache::default();
    let plan = f.plan(&mut cache);
    assert!(
        plan.handle_count() > 0,
        "开群系视图 + 有选中 ⇒ 手柄数必须 > 0（实得 0 = 计划恒空，后面的 0 就不可信）"
    );
    assert!(
        !plan.stepped_edges.is_empty(),
        "选中区域应有阶梯描边（白色实线贴格边）"
    );
}

/// REQ-5（用户症状 3 的**计数**面）：关 `nature_overlay` ⇒ 本帧要画的手柄数
/// **0**、阶梯描边 **0**、候选锚点 **0**，而 `projection.selection` 仍是原值。
#[test]
fn req_handle_plan_is_empty_when_overlay_off() {
    // ① 有选中：关 overlay ⇒ 手柄/描边全空
    {
        let f = PlanFixture::new(false, true, Some((4, 4)));
        let mut cache = RegionDecorCache::default();
        let plan = f.plan(&mut cache);
        assert_eq!(plan.handle_count(), 0, "关群系视图后手柄数必须 0");
        assert!(
            plan.stepped_edges.is_empty(),
            "关群系视图后阶梯描边必须 0 条"
        );
        assert!(
            f.projection.selection.is_some(),
            "关群系视图**不得**清掉选中态（用户裁定：选中保留）"
        );
    }
    // ② 只有候选（无选中）：关 overlay ⇒ 候选锚点也空
    {
        let f = PlanFixture::new(false, false, Some((4, 4)));
        let mut cache = RegionDecorCache::default();
        let plan = f.plan(&mut cache);
        assert_eq!(
            plan.candidate_anchors.len(),
            0,
            "关群系视图后候选锚点必须 0 个（旧实现这段只判 Topographic，红十字照样画）"
        );
        assert_eq!(plan.handle_count(), 0);
    }
}

/// REQ-6（AC15）：稳态重复求计划 **不得再栅格化**（`rasterize_polygon_cells` 是
/// 「鼠标移动掉帧」的候选热点：每帧扫 bbox + 建 HashSet + 四邻边判定）。
#[test]
fn req_steady_plans_do_not_rasterize_again() {
    let f = PlanFixture::new(true, true, None);
    let mut cache = RegionDecorCache::default();
    {
        let plan = f.plan(&mut cache);
        assert!(plan.handle_count() > 0, "前置：开 overlay + 选中应有手柄");
    }
    assert_eq!(
        cache.rasterizes, 1,
        "首帧应恰好栅格化 1 次（自证计数器可信）"
    );
    let r1 = cache.rasterizes;
    let d1 = cache.anchor_decimates;

    for _ in 0..FRAMES {
        let _ = f.plan(&mut cache);
    }
    assert_eq!(
        cache.rasterizes,
        r1,
        "稳态 {FRAMES} 帧又栅格化了 {} 次（选中态/多边形/网格/幅面都没变）",
        cache.rasterizes - r1
    );
    assert_eq!(
        cache.anchor_decimates,
        d1,
        "稳态 {FRAMES} 帧又抽稀了 {} 次手柄（同理不该重算）",
        cache.anchor_decimates - d1
    );
}

/// REQ-8（AC22 · 阳性对照）：抽稀缓存必须**按 scale 分档**，不能只按多边形指纹一刀切，
/// 也不能按 `scale` 的原始位模式（否则 smooth zoom 每帧都算「变了」= 等于没缓存）。
///
/// 判据与具体档宽无关（任何 ≥×2 的分档都能过）：
/// - 同 scale 稳态 60 帧 ⇒ Δ == 0
/// - `scale × 1.02`（同档内微动，滚动惯性/补间就是这个量级）⇒ Δ == 0
/// - `scale × 4`（明确跨档）⇒ Δ == **1**（恰好一次，不是每帧一次）
#[test]
fn req_anchor_decimation_cache_is_bucketed_by_scale() {
    let mut f = PlanFixture::new(true, true, None);
    let mut cache = RegionDecorCache::default();
    let _ = f.plan(&mut cache);
    let d0 = cache.anchor_decimates;
    assert_eq!(d0, 1, "首帧应恰好抽稀 1 次（自证计数器可信）");

    for _ in 0..FRAMES {
        let _ = f.plan(&mut cache);
    }
    assert_eq!(cache.anchor_decimates, d0, "稳态 {FRAMES} 帧不得再抽稀");

    f.view.scale *= 1.02;
    let _ = f.plan(&mut cache);
    assert_eq!(
        cache.anchor_decimates, d0,
        "scale ×1.02 属同一档却重算了 —— 键里塞了原始 scale，smooth zoom 下等于没缓存"
    );

    f.view.scale *= 4.0;
    let _ = f.plan(&mut cache);
    assert_eq!(
        cache.anchor_decimates,
        d0 + 1,
        "scale ×4 已跨档，应**恰好**重算 1 次（实得 {} 次）",
        cache.anchor_decimates - d0
    );
}

/// REQ-9：**条件不再成立时锚点必须清空**——不得把上一帧的锚点继续画出来。
///
/// 这是「缓存」最典型的副作用：缓存命中就画，缓存没被失效就画错东西。主程序自己
/// 提到补了这段清理，我不采信口头说明，直接钉两条最容易被漏的失效路径。
#[test]
fn req_stale_anchors_are_cleared_when_the_condition_goes_away() {
    // ① 取消选中：选中区域的手柄与阶梯描边必须一起消失
    let mut f = PlanFixture::new(true, true, None);
    let mut cache = RegionDecorCache::default();
    {
        let p = f.plan(&mut cache);
        assert!(p.handle_count() > 0, "前置：选中 + 开 overlay 应有手柄");
    }
    f.projection.selection = None;
    {
        let p = f.plan(&mut cache);
        assert_eq!(p.handle_count(), 0, "取消选中后仍在画手柄（缓存没失效）");
        assert!(p.stepped_edges.is_empty(), "取消选中后仍在画阶梯描边");
    }

    // ② 候选格消失（光标移出地形）：候选锚点必须清空
    let mut g = PlanFixture::new(true, false, Some((4, 4)));
    let mut cache2 = RegionDecorCache::default();
    {
        let p = g.plan(&mut cache2);
        assert!(
            !p.candidate_anchors.is_empty(),
            "前置：候选格存在时应有候选锚点"
        );
    }
    g.candidate.cell = None;
    {
        let p = g.plan(&mut cache2);
        assert_eq!(
            p.candidate_anchors.len(),
            0,
            "候选格消失后仍在画候选锚点（上一帧结果被继续消费）"
        );
    }
}

/// REQ-7：候选锚点的「按格派生」也必须缓存 + 受同一道 overlay 门控。
#[test]
fn req_candidate_derivation_is_cached_and_gated() {
    let f = PlanFixture::new(true, false, Some((4, 4)));
    let mut cache = RegionDecorCache::default();
    {
        let plan = f.plan(&mut cache);
        assert!(
            !plan.candidate_anchors.is_empty(),
            "开 overlay + 有候选格（无选中）⇒ 派生出的候选锚点必须非空"
        );
    }
    assert_eq!(
        cache.candidate_derivations, 1,
        "首帧应恰好派生 1 次（自证计数器可信）"
    );
    let d1 = cache.candidate_derivations;
    for _ in 0..FRAMES {
        let _ = f.plan(&mut cache);
    }
    assert_eq!(
        cache.candidate_derivations,
        d1,
        "稳态 {FRAMES} 帧又搬了 {} 次整张地形（1024² 场景就是这 26 FPS 的候选来源）",
        cache.candidate_derivations - d1
    );
}
