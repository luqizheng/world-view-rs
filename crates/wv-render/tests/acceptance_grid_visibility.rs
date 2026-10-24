#![cfg(feature = "app")]
//! 独立验收测试 —— 网格实体数 / 资产回收 / 12 档重建计数（QA 严过关，2026-10-23）。
//!
//! ## 口径
//! - 期望值来自手算，**不引用被测常量**（`GRID_FADE_*` / `GRID_LINE_PX` /
//!   `GridConfig::default()` 都不参与期望值构造）。
//! - `cell_size = 4.0` = lod-demo `extent 4096 / raster 1024`；
//!   `fit = max(4096/1280, 4096/800) = 5.12`；世界级带 = `[5.12/32, 5.12×1.5625]`
//!   = `[0.16, 8.0]`（32× / 1.5625× 是 ADR-0038 的要求值，不是从代码读的）。
//! - 判据用**计数器**（实体数 / asset 数 / handle 变化次数），不用墙钟毫秒。
//!
//! ## 红绿说明（重要）
//! 标 `req_` 的测试是**需求断言**：修复前**应当失败**，它就是本轮缺陷证据，
//! 不许加 `#[ignore]` 掩盖。标 `ctl_` / `neg_` 的是对照与负向断言：现在就必须绿。
//!
//! 运行：`cargo test -p wv-render --features app --test acceptance_grid_visibility`
//!
//! ## 2026-10-23 主程序的改动（仅注入口，**断言一条未改**）
//! 网格渲染从「CPU 三角网 + ColorMaterial」换成「单 quad + 自定义材质
//! `GridLinesMaterial`（shader 双档）」，且系统新增读 `RenderRecipe`
//! （`PassId::GridLines` 的一致性收口）。故本 harness 的资源注入口与
//! 材质计数口径随之调整：
//! - `Assets<ColorMaterial>` → `Assets<GridLinesMaterial>`
//! - 新增 `RenderRecipe`（`GridLines` 开）——不注入则系统缺资源 panic
//! - `material_assets()` 改数 `Assets<GridLinesMaterial>`

use bevy::prelude::*;
use wv_render::cellmap_bridge::sync_grid_lines;
use wv_render::layer_frame::{LayerBinding, LayerFrame};
use wv_render::recipe::{PassId, RenderRecipe};
use wv_render::state::{GridConfig, ViewState};
use wv_render::terrain_view::{
    grid_lines::GridLinesMaterial, grid_lines_visible, GridLinesOverlay, TerrainTexture,
};

/// 手算：4.0 世界单位/格（`4096.0 / 1024`）。
const CELL_SIZE: f32 = 4.0;
/// 手算：`max(4096/1280, 4096/800)`。
const FIT_SCALE: f32 = 5.12;
/// 手算：`5.12 / 32`（ADR-0038 的 32× 内界）。
const BAND_MIN: f32 = 0.16;
/// 手算：`5.12 × 1.5625`（LEVEL_OVERVIEW 的外界）。
const BAND_MAX: f32 = 8.0;

/// 只装 `sync_grid_lines` 的最小 App（不装任何渲染插件，纯 ECS + Assets）。
fn grid_app(grid_lines: bool, terrain_visible: bool, scale: f32) -> App {
    let mut app = App::new();
    app.insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<GridLinesMaterial>::default())
        .insert_resource(recipe_with_grid(terrain_visible))
        .insert_resource(ViewState {
            grid_lines,
            scale,
            ..Default::default()
        })
        .insert_resource(GridConfig {
            cell_size: CELL_SIZE,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        })
        .insert_resource(fake_texture(8))
        .insert_resource(LayerFrame {
            terrain: LayerBinding {
                visible: terrain_visible,
                opacity: 1.0,
                z_offset: 0.0,
            },
            ..Default::default()
        })
        .init_resource::<GridLinesOverlay>()
        .add_systems(Update, sync_grid_lines);
    app
}

/// 最小 `RenderRecipe`：只开 `GridLines`（另开 `Terrain`，因为
/// `resolve_render_recipe` 里「地形层不可见 ⇒ GridLines 也关」是同一条级联；
/// 这里直接给终态，不在 harness 里重跑级联）。
fn recipe_with_grid(terrain_visible: bool) -> RenderRecipe {
    let mut r = RenderRecipe::default();
    r.passes[PassId::Terrain as usize].enabled = terrain_visible;
    r.passes[PassId::GridLines as usize].enabled = terrain_visible;
    r
}

/// 最小 `TerrainTexture`：`size` 取 8 省算力（可见性判定只依赖 `cell_size/scale`，
/// 与幅面无关；幅面只影响链数，见 `a5` 的量级断言）。
fn fake_texture(size: u32) -> TerrainTexture {
    TerrainTexture {
        handle: Handle::default(),
        size,
        height: size,
        res: 1,
        dirty: None,
        chunks_dirty: None,
        field_dirty: None,
        last_mode: None,
        last_soften: false,
        last_theme_id: "acceptance",
        last_hypsometric: 0.0,
        last_map: None,
    }
}

/// 网格实体数（本 App 里唯一的 `Mesh2d` 来源就是网格）。
fn grid_entities(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<&Mesh2d>();
    q.iter(app.world()).count()
}

/// 网格实体当前用的 mesh handle（无实体返回 `None`）。
fn grid_handle(app: &mut App) -> Option<AssetId<Mesh>> {
    let mut q = app.world_mut().query::<&Mesh2d>();
    q.iter(app.world()).next().map(|m| m.0.id())
}

fn mesh_assets(app: &App) -> usize {
    app.world().resource::<Assets<Mesh>>().len()
}

fn material_assets(app: &App) -> usize {
    app.world().resource::<Assets<GridLinesMaterial>>().len()
}

// ══════════════════════ 负向断言（现在必须绿） ══════════════════════

/// NEG-1：开关关 → 网格实体数必须 **0**（不是「没报错」）。
#[test]
fn neg_grid_off_yields_zero_entities_without_erroring() {
    let mut app = grid_app(false, true, 1.0);
    app.update();
    assert_eq!(grid_entities(&mut app), 0, "开关关：实体必须 0");
    assert_eq!(mesh_assets(&app), 0, "开关关：不得留下 mesh 资产");
}

/// NEG-2：地形图层隐藏 → 网格实体数必须 **0**（即便会话开关开着）。
/// 语义依据：网格是地形的编辑参照，地形藏了，格线浮着只会误导「这里可以画」。
#[test]
fn neg_terrain_layer_hidden_yields_zero_entities() {
    let mut app = grid_app(true, false, 1.0);
    app.update();
    assert_eq!(grid_entities(&mut app), 0, "地形层隐藏：实体必须 0");
    assert_eq!(mesh_assets(&app), 0, "地形层隐藏：不得留下 mesh 资产");
    // 纯谓词也独立复算一遍（开关自身不看图层）：两者都不否认可达性。
    assert!(!grid_lines_visible(&ViewState {
        grid_lines: false,
        ..Default::default()
    }));
}

/// NEG-3：关闭后资产回收 —— mesh 必须归零；material 允许保留 1 个缓存句柄
/// （`overlay.material` 是跨重建复用的，不是每帧增长）。
#[test]
fn neg_turning_grid_off_reclaims_mesh_assets() {
    let mut app = grid_app(true, true, 1.0);
    app.update();
    assert_eq!(
        grid_entities(&mut app),
        1,
        "前置：scale 1.0（cell_px 4.0px）应有 1 个实体"
    );
    assert_eq!(mesh_assets(&app), 1);

    app.world_mut().resource_mut::<ViewState>().grid_lines = false;
    app.update();
    assert_eq!(grid_entities(&mut app), 0, "关闭后：实体必须 0");
    assert_eq!(mesh_assets(&app), 0, "关闭后：mesh 资产必须回收");
    assert!(
        material_assets(&app) <= 1,
        "material 至多保留 1 个缓存句柄，实得 {}",
        material_assets(&app)
    );
}

// ══════════════════════ 阳性对照（现在必须绿） ══════════════════════

/// CTL-1：近景（scale 1.0 → cell_px = 4.0/1.0 = 4.0px ≥ 4px）必须有实体，
/// 且结构不变的连续帧不得重建（handle 不变、asset 数不增）。
/// 这条同时是 harness 的有效性证明：它数得出 1，所以「数出 0」不是永远为真。
#[test]
fn ctl_close_zoom_draws_one_grid_and_does_not_rebuild_on_idle_frames() {
    let mut app = grid_app(true, true, 1.0);
    app.update();
    assert_eq!(
        grid_entities(&mut app),
        1,
        "cell_px 4.0px → 应有 1 个网格实体"
    );
    let first = grid_handle(&mut app).expect("刚建好");

    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        grid_handle(&mut app),
        Some(first),
        "同 scale 连续帧不得重建 mesh"
    );
    assert_eq!(mesh_assets(&app), 1, "同 scale 连续帧 mesh 资产应保持 1");
}

// ══════════════════════ 需求断言（修复前应当失败 = 缺陷证据） ══════════════════════

/// REQ-1：世界级「全图适配」缩放（5.12）下必须有可见网格输出。
///
/// 手算：cell_px = 4.0/5.12 = 0.78125px < 2px → 细档按设计退场，
/// 因此**必须**有粗档（m ≥ 8 时 6.25px ≥ 4px）承接。当前实现无双档 → 0 实体。
#[test]
fn req_world_fit_scale_must_still_show_a_grid() {
    let mut app = grid_app(true, true, FIT_SCALE);
    app.update();
    assert_eq!(
        grid_entities(&mut app),
        1,
        "缺陷 D1：scale 5.12（cell_px 0.78px）下必须由粗档给出 1 个可见网格实体；\
         当前 fade=0 整层退场（实得 0）"
    );
}

/// REQ-2：世界级整带 12 档（0.16 → 8.0 等比分档）**每一档**都要有可见网格。
///
/// 12 档序列：`scale_i = 0.16 × 50^(i/11)`，i = 0..11（两端恰为带界）。
#[test]
fn req_every_one_of_twelve_world_band_notches_shows_a_grid() {
    let mut app = grid_app(true, true, BAND_MIN);
    let mut invisible = Vec::new();
    for i in 0..12 {
        let scale = BAND_MIN * (BAND_MAX / BAND_MIN).powf(i as f32 / 11.0);
        app.world_mut().resource_mut::<ViewState>().scale = scale;
        app.update();
        if grid_entities(&mut app) == 0 {
            invisible.push(scale);
        }
    }
    assert!(
        invisible.is_empty(),
        "缺陷 D1：世界带 12 档中 {} 档无网格：{:?}",
        invisible.len(),
        invisible
    );
}

/// REQ-3：12 档缩放过程中的 mesh 重建次数上限 **≤ 2**。
///
/// 判据：按档推进时记录「有实体时的 mesh handle」，统计 handle 变化次数
/// （变化 = 重建）。要求 ≤ 2 —— 越档只允许改变换/材质参数，不许重建几何。
/// 现实现把 `view.scale` 写进重算键且线宽按 scale 烘焙 → 每个可见档必重建。
#[test]
fn req_mesh_rebuilds_across_twelve_notches_must_not_exceed_two() {
    let mut app = grid_app(true, true, BAND_MIN);
    let mut rebuilds = 0usize;
    let mut prev: Option<AssetId<Mesh>> = None;
    for i in 0..12 {
        let scale = BAND_MIN * (BAND_MAX / BAND_MIN).powf(i as f32 / 11.0);
        app.world_mut().resource_mut::<ViewState>().scale = scale;
        app.update();
        let cur = grid_handle(&mut app);
        if let (Some(p), Some(c)) = (prev, cur) {
            if p != c {
                rebuilds += 1;
            }
        }
        if cur.is_some() {
            prev = cur;
        }
    }
    assert!(
        rebuilds <= 2,
        "缺陷 D3：12 档内重建 {rebuilds} 次（上限 2）。\
         若「线宽按 scale 烘焙进几何」，≤2 与「1px 屏幕常量线宽」数学上不可兼得——\
         须把 scale 从几何里剥出（单位空间网格 + 变换/材质线宽）"
    );
}

// ══════════════════════ 2 轮补充断言（2026-10-23 修复后新增） ══════════════════════

/// REQ-4：12 档缩放的**结构性重建计数**必须为 0（用公开计数器 `rebuilds`，
/// 不数 handle），且与 handle 口径**互相印证**（两个独立计数器不得互相否定）。
///
/// `rebuilds` 只在「幅面/格径/原点/主题变化」时自增，uniform 写入不计——
/// 这正是「每帧可承受」与「每档不可承受」的分界。要求：Δrebuilds = 0。
#[test]
fn req_structural_rebuild_counter_must_not_move_across_twelve_notches() {
    let mut app = grid_app(true, true, BAND_MIN);
    app.update();
    let before = app.world().resource::<GridLinesOverlay>().rebuilds;
    let handle_before = grid_handle(&mut app);

    let mut handle_changes = 0usize;
    for i in 1..12 {
        let scale = BAND_MIN * (BAND_MAX / BAND_MIN).powf(i as f32 / 11.0);
        app.world_mut().resource_mut::<ViewState>().scale = scale;
        app.update();
        if grid_handle(&mut app) != handle_before {
            handle_changes += 1;
        }
    }
    let after = app.world().resource::<GridLinesOverlay>().rebuilds;
    assert_eq!(
        after - before,
        0,
        "12 档缩放触发了 {} 次结构重建（要求 0）",
        after - before
    );
    assert_eq!(
        handle_changes, 0,
        "handle 口径数出 {handle_changes} 次变化，与 rebuilds 计数器（0）不一致——\
         两个计数器互相否定，说明有一个在说谎"
    );
}

/// NEG-4：**recipe 口必须真的生效**（`PassId::GridLines = false` 但地形层可见
/// → 实体 0）。
///
/// 存在意义：`recipe_with_grid(terrain_visible)` 会把 `Terrain` 与 `GridLines`
/// 两格同时置成同一个值，于是"层隐藏"那条负向断言无法区分「是 recipe 关的」
/// 还是「是 layers 关的」。缺了这条，本轮新加的 recipe 收口（D4）**没有覆盖**。
#[test]
fn neg_recipe_gridlines_off_yields_zero_entities_even_with_terrain_visible() {
    let mut app = grid_app(true, true, 1.0);
    {
        let mut r = app.world_mut().resource_mut::<RenderRecipe>();
        r.passes[PassId::GridLines as usize].enabled = false;
        r.passes[PassId::Terrain as usize].enabled = true;
    }
    app.update();
    assert_eq!(
        grid_entities(&mut app),
        0,
        "recipe 说 GridLines 关（地形层可见）→ 实体必须 0；否则系统没读 recipe"
    );
    assert_eq!(mesh_assets(&app), 0);
}

/// NEG-5（AC8 替代判据，制作人 2026-10-23 裁定）：「关→开→关」连续 3 轮后
/// **材质句柄数不增长**（防真泄漏，不防缓存）。
///
/// 注：现实现 `despawn` 连材质一起回收（`GridLinesOverlay::despawn`），
/// 所以预期每轮关闭后均为 0；本断言对「缓存 1 个」与「回收为 0」两种设计
/// 都成立——它只禁止**增长**。
#[test]
fn neg_material_handles_do_not_grow_across_three_toggle_rounds() {
    let mut app = grid_app(true, true, 1.0);
    let mut baseline: Option<usize> = None;
    for round in 0..3 {
        app.world_mut().resource_mut::<ViewState>().grid_lines = true;
        app.update();
        let on = material_assets(&app);
        assert_eq!(grid_entities(&mut app), 1, "第 {round} 轮：开 → 1 实体");
        app.world_mut().resource_mut::<ViewState>().grid_lines = false;
        app.update();
        assert_eq!(grid_entities(&mut app), 0, "第 {round} 轮：关 → 0 实体");
        let off = material_assets(&app);
        let base = *baseline.get_or_insert(off);
        assert!(
            off <= base,
            "第 {round} 轮关闭后材质句柄 {off} 超过首轮基线 {base}——真泄漏（开/关累积死资产）"
        );
        assert!(
            on <= base + 1,
            "第 {round} 轮开启时材质句柄 {on} 超过基线 +1——每轮都新建材质"
        );
    }
}

/// NEG-6（D6 回归）：`entity` 存在但 `mesh` 为 `None` 时也必须 despawn 实体。
///
/// 旧实现 `if let (Some(e), Some(m)) = (self.entity.take(), self.mesh.take())`
/// 在 mesh 缺失时会把 entity 从字段里 `take()` 掉却不 despawn → 实体泄漏。
#[test]
fn neg_despawn_without_mesh_still_despawns_the_entity() {
    let mut app = grid_app(false, true, 1.0);
    app.update();
    let dangling = app.world_mut().spawn_empty().id();

    // 把 overlay 借出来（避免与 `app.world()` 的不可变借用打架），故意制造
    // 「有 entity、无 mesh」的不完整状态再调 despawn。
    let mut overlay = app
        .world_mut()
        .remove_resource::<GridLinesOverlay>()
        .unwrap();
    overlay.entity = Some(dangling);
    overlay.mesh = None;
    overlay.material = None;

    let mut queue = bevy::ecs::world::CommandQueue::default();
    {
        let world = app.world();
        let mut commands = Commands::new(&mut queue, world);
        overlay.despawn(&mut commands);
    }
    queue.apply(app.world_mut());
    app.world_mut().insert_resource(overlay);

    assert!(
        app.world().get_entity(dangling).is_err(),
        "mesh 为 None 时实体 {dangling:?} 必须被 despawn（旧实现在这里泄漏）"
    );
}
