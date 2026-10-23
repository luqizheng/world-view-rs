#![cfg(feature = "app")]
//! 独立验收测试 —— `ViewState::field_render` 开关的**独占性 / 回退完整性 / 资产不泄漏**
//! （QA 严过关，2026-10-23 · ADR-0053 GPU 场渲染路线接线的过程审）。
//!
//! ## 本轮验什么、**不**验什么
//!
//! 验：开 ⇒ 场 quad 独占（chunk 实体 0、quad 1、稳态不重打包）；关 ⇒ 完整回退
//! （quad 0、场材质与四张数据纹理回收、chunk 实体与资产回到「从未开过」的纯 chunk
//! 基线）；编辑 ⇒ 恰好一次 repack；20 轮开关 / 20 笔编辑均无资产累积；
//! 负向（地形图层隐藏 ⇒ 开关开着也 quad 0）。
//!
//! **不验「逐字节等价」**。等价性是「同一条管线做优化」的防线；这里是**换渲染器 +
//! 回退保险**，两种渲染器的输出本来就不等价。验收对象是「开关的独占性、回退的完整性、
//! 资产不泄漏」，不是「两条路径画出同一张图」。
//!
//! ## 本轮**测不到**什么（诚实标注，别当成「验过了」）
//!
//! 本机无窗口、无 GPU 呈现、未录屏。以下**全部由用户真人验收**，本文件一条都没覆盖：
//! - 近观分辨率（4× 放大是否还糊）、远观噪点、海岸三段色带、显存实际占用；
//! - **180° 旋转回归目检** —— 该 bug 在库里修过一次（P1 起就存在，因程序化地形近似
//!   对称在截图验收中漏过）。换接线正是它**复发最典型**的时机。本文件只能验到它的
//!   必要条件（quad 的 UV 是 V 翻转、且 V 翻转只翻一次），**验不到它是否真的没转 180°**。
//!
//! ## 口径（写断言前先过的四条）
//!
//! - **独立复算**：期望值一律是字面量或本文件内手算式（`4` 块 / `60` 帧 / `20` 轮），
//!   不引用被测常量、不用被测函数算期望。
//! - **计数器而非墙钟**：数实体数、asset 条数、repack 次数。毫秒不作判据
//!   （`FieldRenderStats::last_repack_ms` 一个都不用）。
//! - **负向断言**：该消失的东西必须**计数为 0**，不是「没报错」。
//! - **红绿约定**：`req_` = 需求断言（实现不达标时**应当失败**，不许 `#[ignore]`、
//!   不许放宽）；`neg_` / `ctl_` = 负向与阳性对照（现在就必须绿）。
//! - **一致性**：同一对象的两个独立口径（`TerrainChunk` 组件数 / `by_coord` 注册表 /
//!   主线的 `field_render_on_chunk_entities`）不得互相否定；不一致即判红。
//!
//! ## 自验（判定工具先自验才许对他人下结论）
//!
//! 本 harness 的计数器先证明「能数出数」才允许用来数 0：
//! - `ctl_chunk_path_spawns_expected_visible_chunks`（**应命中** ⇒ 数出 4）；
//! - `ctl_chunk_path_is_stable_across_steady_frames`（**不命中** ⇒ 稳态不增）；
//! - `ctl_field_on_spawns_exactly_one_quad`（**应命中** ⇒ 数出 1）。
//!
//! ## 两个**实现约定**（不是我发明的，是被测代码写明的；断言必须按它们写）
//!
//! 1. **`sync_terrain_field` 排在 `sync_terrain_chunks` 之前**
//!    （`app_bootstrap.rs:423-431` 的显式顺序）。故 `field_render_on_chunk_entities`
//!    读到的是**上一帧末尾**的块数（`cellmap_bridge.rs:1459` 在 gate 之前取值），
//!    过渡帧有**一帧滞后**。本文件所有涉及该计数器的断言都在「切换后第 2 帧」取，
//!    这不是放宽——它在**稳态**必须精确相等，滞后只在切换的那一帧。
//! 2. **「可能有编辑」的唯一廉价信号是 `TerrainTexture.chunks_dirty`**
//!    （`cellmap_bridge.rs` 的 `needs_hash`）。故虚构一次笔刷提交 =
//!    「改一格数据 + 标脏」，只改数据不标脏，实现有权不重算。
//!
//! 运行：`cargo test -p wv-render --features app --test acceptance_field_render`

use bevy::prelude::*;
use bevy::window::WindowResolution;
use wv_core::map::{Extent, MapLevel, MapNode};
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::{Id, World};
use wv_render::cellmap_bridge::sync_terrain_chunks;
use wv_render::layer_frame::{LayerBinding, LayerFrame};
use wv_render::state::{DrillState, GridConfig, ViewState, WorldProjection};
use wv_render::terrain_view::TerrainTexture;
use wv_world_cell_map::terrain_chunks::{TerrainChunk, TerrainChunkEntities};

/// 手算：`4096.0 / 1024`（lod-demo 口径 4 世界单位/格）。
const CELL_SIZE: f32 = 4.0;
/// 手算：`max(4096/1280, 4096/800)` = 5.12（世界级「全图适配」）。
const FIT_SCALE: f32 = 5.12;
/// 地形逻辑格边长（512 / `CHUNK_SIZE` 256 = 2 块每边）。
const MAP_CELLS: u32 = 512;
/// 每逻辑格 1 像素：源图 512²×4B = 1MB，判定不受 `res` 影响。
const TEX_RES: u32 = 1;
/// 手算：`chunk_grid(512, 512) = [2, 2]`，fit 视口覆盖全图 ⇒ 可见块 = **4**。
const EXPECTED_CHUNKS: usize = 4;
/// 稳态帧数（沿用上一轮同一口径）。
const FRAMES: usize = 60;
/// 开关轮次（沿用上一轮同一口径）。
const TOGGLE_ROUNDS: usize = 20;
/// 视口分辨率（`app_bootstrap.rs:77` 同值）。
const WIN_W: u32 = 1280;
const WIN_H: u32 = 800;

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
        extent: Some(Extent::sized(
            MAP_CELLS as f32 * CELL_SIZE,
            MAP_CELLS as f32 * CELL_SIZE,
        )),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    }
}

/// 确定性地形：16 格棋盘状陆/水交错，海拔两档。没有随机、没有时间依赖。
fn terrain_data() -> TerrainData {
    let mut cells = Vec::with_capacity((MAP_CELLS * MAP_CELLS) as usize);
    for y in 0..MAP_CELLS {
        for x in 0..MAP_CELLS {
            let land = ((x / 16) + (y / 16)) % 2 == 0;
            cells.push(TerrainCellData {
                height: if land { 140 } else { 40 },
                surface: u8::from(land),
                biome: if land { 3 } else { 2 },
            });
        }
    }
    TerrainData {
        width: MAP_CELLS,
        height: MAP_CELLS,
        cells,
    }
}

fn world_with_terrain() -> World {
    let id = Id::new_unchecked(MAP);
    let mut world = World {
        maps: vec![map_node()],
        ..Default::default()
    };
    world.terrains.insert(id, terrain_data());
    world
}

/// 基线 App：只装**现行 chunk 路径**（`sync_terrain_chunks`），不装任何渲染插件。
///
/// 注入口按该系统的实际 `SystemParam` 清单逐个给；`Window` 是必须的——系统在
/// `windows.single()` 失败时早退（`cellmap_bridge.rs:966`），无头环境必须显式 spawn。
fn chunk_app(terrain_visible: bool, scale: f32) -> App {
    let mut drill = DrillState::default();
    drill.settle_on(Id::new_unchecked(MAP));

    let mut app = App::new();
    app.insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<ColorMaterial>::default())
        .insert_resource(Assets::<Image>::default())
        .insert_resource(WorldProjection {
            world: world_with_terrain(),
            ..Default::default()
        })
        .insert_resource(drill)
        .insert_resource(ViewState {
            scale,
            camera_center: Vec2::splat(MAP_CELLS as f32 * CELL_SIZE * 0.5),
            ..Default::default()
        })
        .insert_resource(GridConfig {
            cell_size: CELL_SIZE,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(MAP_CELLS as f32 * CELL_SIZE),
        })
        .insert_resource(LayerFrame {
            terrain: LayerBinding {
                visible: terrain_visible,
                opacity: 1.0,
                z_offset: 0.0,
            },
            ..Default::default()
        })
        .init_resource::<TerrainChunkEntities>()
        .add_systems(Update, sync_terrain_chunks);

    // 源图：`size × res` 见方。chunk 路径只做像素搬运，不看颜色语义。
    let side = MAP_CELLS * TEX_RES;
    let image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        vec![96u8; (side * side * 4) as usize],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.insert_resource(TerrainTexture {
        handle,
        size: MAP_CELLS,
        height: MAP_CELLS,
        res: TEX_RES,
        dirty: None,
        chunks_dirty: None,
        field_dirty: None,
        last_mode: None,
        last_soften: false,
        last_theme_id: "acceptance",
        last_map: None,
    });

    app.world_mut().spawn(Window {
        resolution: WindowResolution::new(WIN_W, WIN_H),
        ..default()
    });
    app
}

/// 被验收对象 App：基线 + 场渲染桥接系统（顺序与 `app_bootstrap` 一致）。
fn field_app(terrain_visible: bool, scale: f32, field_render: bool) -> App {
    let mut app = chunk_app(terrain_visible, scale);
    iface::install(&mut app);
    iface::set_field_render(&mut app, field_render);
    app
}

// ══════════════════════════ 计数器 ══════════════════════════

/// chunk 实体数：**按 `TerrainChunk` 组件数**，不按 `Mesh2d`。
///
/// 理由：场渲染的 quad 也是 `Mesh2d`，按 `Mesh2d` 计数会把 quad 混进块数，
/// 「开 ⇒ chunk 实体 0」就永远数不出 0 —— 这是本文件最容易自欺的一处。
fn chunk_entities(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<&TerrainChunk>();
    q.iter(app.world()).count()
}

/// 独立口径：`TerrainChunkEntities::by_coord` 登记表长度。与组件数必须相等。
fn chunk_registry_len(app: &App) -> usize {
    app.world().resource::<TerrainChunkEntities>().len()
}

fn mesh_assets(app: &App) -> usize {
    app.world().resource::<Assets<Mesh>>().len()
}

fn color_material_assets(app: &App) -> usize {
    app.world().resource::<Assets<ColorMaterial>>().len()
}

fn image_assets(app: &App) -> usize {
    app.world().resource::<Assets<Image>>().len()
}

// ══════════════════════════ 新接口（唯一耦合点） ══════════════════════════

/// 新接口的**全部**耦合点。实现侧名字若变，只改本模块，8 条断言正文一字不动。
mod iface {
    use super::*;
    use wv_render::cellmap_bridge::{sync_terrain_field, FieldRenderStats, TerrainFieldRender};
    use wv_render::terrain_view::field::TerrainFieldMaterial;

    /// 接进 App。
    ///
    /// **与生产的顺序不同是有意的**：`app_bootstrap.rs` 里
    /// `sync_terrain_field` 排在 `sync_terrain_chunks` **之后**（为了让
    /// `field_render_on_chunk_entities` 读到帧末块数）；本 harness 排**之前**，
    /// 是为了让「快照与期望在同一帧内可判」，不依赖命令 flush 的时机。
    /// 本文件全部断言都只用 Δ（不依赖绝对帧号），与顺序无关：
    /// 场路径的编辑信号走 `TerrainTexture::field_dirty`，chunk 路径走
    /// `chunks_dirty`，两份副本互不相抢（见 `terrain_texture.rs` 字段文档）。
    pub fn install(app: &mut App) {
        app.init_resource::<TerrainFieldRender>()
            .init_resource::<FieldRenderStats>()
            .insert_resource(Assets::<TerrainFieldMaterial>::default())
            .add_systems(Update, sync_terrain_field.before(sync_terrain_chunks));
    }

    /// 运行时翻开关（命令面板 `CommandId::ToggleFieldRender` 的同一入口）。
    pub fn set_field_render(app: &mut App, on: bool) {
        app.world_mut().resource_mut::<ViewState>().field_render = on;
    }

    pub fn field_render(app: &App) -> bool {
        app.world().resource::<ViewState>().field_render
    }

    /// 场 quad 实体数：**自己数实体**，不读计数器（独立复算）。
    pub fn quads(app: &mut App) -> usize {
        let mut q = app
            .world_mut()
            .query::<&MeshMaterial2d<TerrainFieldMaterial>>();
        q.iter(app.world()).count()
    }

    /// 累计 repack 次数（单调累加；断言一律用 Δ）。
    pub fn repacks(app: &App) -> u64 {
        app.world().resource::<FieldRenderStats>().field_repacks
    }

    /// 累计**增量补丁**次数（与 `repacks` 互斥；断言一律用 Δ）。
    pub fn patches(app: &App) -> u64 {
        app.world().resource::<FieldRenderStats>().field_patches
    }

    /// 主线自报的「场开着时仍存在的 chunk 实体数」。
    /// 已核语义：`cellmap_bridge.rs:1459` 取 `chunk_entities.len()`，即**活着的块数**
    /// （不是「被压掉的块数」）——与本文件 [`chunk_entities`] 口径同义，故必须逐帧相等
    /// （过渡帧允许一帧滞后，见文件头「实现约定 1」）。
    pub fn on_chunk_entities(app: &App) -> u64 {
        app.world()
            .resource::<FieldRenderStats>()
            .field_render_on_chunk_entities as u64
    }

    /// 主线自报的 quad 数（与 [`quads`] 独立复算的结果互为印证）。
    pub fn reported_quads(app: &App) -> usize {
        app.world().resource::<FieldRenderStats>().field_quads
    }

    /// 场材质资产条数（关掉后必须回收为 0）。
    pub fn field_material_assets(app: &App) -> usize {
        app.world().resource::<Assets<TerrainFieldMaterial>>().len()
    }

    /// 「笔刷提交一次」：改一格数据 **+ 标脏**（走生产路径的 `mark_dirty`）。
    ///
    /// 两件都必须做：只改数据不标脏，`needs_hash` 为假（`cellmap_bridge.rs` 只把
    /// 脏区当「可能有编辑」信号），实现有权不重算——那不是缺陷。
    /// 数值来回翻（140 ⇄ 100），保证每次提交都真的改了内容（否则第二次是空操作）。
    ///
    /// 用 `mark_dirty` 而不是直接写 `chunks_dirty`：生产侧（`editing.rs` /
    /// inspector / 群系分界线）全部走 `mark_dirty`，它会**同时**写 `dirty` /
    /// `chunks_dirty` / `field_dirty` 三份副本。只写 `chunks_dirty` 在旧实现里
    /// 恰好够用，现在只代表「没有可信局部脏区」那条回退路径（见
    /// `neg_missing_patch_rect_falls_back_to_full_repack`）。
    pub fn commit_one_cell_edit(app: &mut App) {
        commit_region_edit(app, [0, 0, 1, 1]);
    }

    /// 同 [`commit_one_cell_edit`]，可指定脏矩形（逻辑格，半开）。
    ///
    /// 格子改在矩形内，保证「脏区 ⊇ 实际改动」这个生产侧契约成立。
    pub fn commit_region_edit(app: &mut App, rect: [u32; 4]) {
        {
            let mut p = app.world_mut().resource_mut::<WorldProjection>();
            let data = p
                .world
                .terrains
                .get_mut(&Id::new_unchecked(MAP))
                .expect("harness 应已插入地形");
            let (gx, gy) = (rect[0], rect[1]);
            let i = (gy * MAP_CELLS + gx) as usize;
            data.cells[i].height = if data.cells[i].height == 140 {
                100
            } else {
                140
            };
        }
        app.world_mut()
            .resource_mut::<TerrainTexture>()
            .mark_dirty(rect);
    }

    /// 「改了数据但**没有**可信脏区」：只标记 chunk 层副本。
    ///
    /// 模拟 `mark_all_dirty` 之外的边缘路径（外部直接改 `World`、插件写入）——
    /// 场渲染此时必须**整份重打**，不许猜一个矩形去补丁。
    pub fn commit_edit_without_field_rect(app: &mut App) {
        {
            let mut p = app.world_mut().resource_mut::<WorldProjection>();
            let data = p
                .world
                .terrains
                .get_mut(&Id::new_unchecked(MAP))
                .expect("harness 应已插入地形");
            data.cells[0].height = if data.cells[0].height == 140 {
                100
            } else {
                140
            };
        }
        let mut tex = app.world_mut().resource_mut::<TerrainTexture>();
        tex.chunks_dirty = Some([0, 0, 1, 1]);
        tex.field_dirty = None;
    }
}

// ══════════════════════════ 基线自验（现在必须绿） ══════════════════════════

/// CTL-1（自验·**命中**样本）：纯 chunk 路径 + 视口覆盖全图 ⇒ 恰好
/// `EXPECTED_CHUNKS` 个 chunk 实体，两条独立口径一致，每块一份 mesh 与材质。
///
/// 没有这条，「关掉场渲染后 chunk 实体 0」可能只是 harness 根本 spawn 不出块。
#[test]
fn ctl_chunk_path_spawns_expected_visible_chunks() {
    let mut app = chunk_app(true, FIT_SCALE);
    app.update();
    assert_eq!(
        chunk_entities(&mut app),
        EXPECTED_CHUNKS,
        "基线：512 格 = 2×2 块，fit 视口覆盖全图 ⇒ 应有 {EXPECTED_CHUNKS} 个 chunk 实体"
    );
    assert_eq!(
        chunk_registry_len(&app),
        EXPECTED_CHUNKS,
        "两个口径（TerrainChunk 组件数 / by_coord 注册表）必须一致"
    );
    assert_eq!(
        mesh_assets(&app),
        EXPECTED_CHUNKS,
        "每块一份 mesh，实得 {} 份",
        mesh_assets(&app)
    );
    assert_eq!(
        color_material_assets(&app),
        EXPECTED_CHUNKS,
        "每块一份 ColorMaterial，实得 {} 份",
        color_material_assets(&app)
    );
}

/// CTL-2（自验·**不命中**样本）：chunk 路径稳态连续帧**不再 spawn**。
///
/// 与 CTL-1 成对：一条证明数得出 4，一条证明稳态不会变成 5 / 8 / 12。
#[test]
fn ctl_chunk_path_is_stable_across_steady_frames() {
    let mut app = chunk_app(true, FIT_SCALE);
    app.update();
    let base = chunk_entities(&mut app);
    let spawns0 = app.world().resource::<TerrainChunkEntities>().chunk_spawns;
    let meshes0 = mesh_assets(&app);
    for _ in 0..FRAMES {
        app.update();
    }
    assert_eq!(
        chunk_entities(&mut app),
        base,
        "稳态 {FRAMES} 帧块数不得变化"
    );
    assert_eq!(
        app.world().resource::<TerrainChunkEntities>().chunk_spawns,
        spawns0,
        "稳态 {FRAMES} 帧又 spawn 了 {} 次",
        app.world().resource::<TerrainChunkEntities>().chunk_spawns - spawns0
    );
    assert_eq!(
        mesh_assets(&app),
        meshes0,
        "稳态 {FRAMES} 帧 mesh 资产不得增长"
    );
}

/// NEG-0：图层关 ⇒ chunk 实体 0（回退路径自身的地基，必须先立）。
#[test]
fn neg_terrain_layer_hidden_yields_zero_chunks() {
    let mut app = chunk_app(false, FIT_SCALE);
    app.update();
    assert_eq!(
        chunk_entities(&mut app),
        0,
        "地形图层隐藏 ⇒ chunk 实体必须 0"
    );
    assert_eq!(chunk_registry_len(&app), 0, "注册表也必须清空");
    assert_eq!(mesh_assets(&app), 0, "块被 despawn ⇒ mesh 资产必须回收");
}

// ══════════════════════════ 接口/默认值 ══════════════════════════

/// REQ-0（默认值）：`field_render` **默认 false** —— 开箱走现行烘焙路径。
///
/// ADR-0053 §3 的「一行回退保险」只有在默认关时才算保险：默认开 = 用户第一次
/// 打开看到的是新管线，回退变成「用户自己去找开关」。
#[test]
fn req_field_render_defaults_to_off() {
    assert!(
        !ViewState::default().field_render,
        "`ViewState::default().field_render` 必须是 false（回退保险的默认态）"
    );
    let app = field_app(true, FIT_SCALE, false);
    assert!(
        !iface::field_render(&app),
        "harness 置 false 后应读到 false"
    );
    assert!(
        !ViewState::default().field_render,
        "同一事实的第二条陈述必须一致"
    );
}

// ══════════════════════════ 阳性对照（现在必须绿） ══════════════════════════

/// CTL-3（自验·**命中**样本）：开 ⇒ quad **恰好 1**、主线计数器同值。
///
/// 证明本 harness 数得出 1，所以后面「关 ⇒ quad 0」不是恒 0 的假绿。
#[test]
fn ctl_field_on_spawns_exactly_one_quad() {
    let mut app = field_app(true, FIT_SCALE, true);
    app.update();
    assert_eq!(
        iface::quads(&mut app),
        1,
        "开 ⇒ 场渲染只用**一个** quad 铺满整图（不是每块一个，那等于没换架构）"
    );
    assert_eq!(
        iface::reported_quads(&app),
        1,
        "主线 `field_quads` 与实体口径必须一致"
    );
    assert_eq!(
        iface::field_material_assets(&app),
        1,
        "开 ⇒ 恰好一份场材质（这是 `field_material_assets` 这个计数器的阳性对照：\
         它数得出 1，所以「关 ⇒ 0」不是恒 0）"
    );
}

/// CTL-4（自验·**命中**样本，专打「资产账目」类判据）：故意泄漏一张图 / 一份 mesh，
/// 计数器必须**看得见**。
///
/// 没有这条，「回退后资产与纯 chunk 基线相等」有可能只是两边都恒等于同一个常数
/// （例如两边都数是 0），那就什么都没验。故意加一份不回收的资产，两个计数必须各 +1。
#[test]
fn ctl_asset_counters_detect_a_deliberate_leak() {
    let mut app = chunk_app(true, FIT_SCALE);
    app.update();
    let (images0, meshes0) = (image_assets(&app), mesh_assets(&app));

    // 泄漏一张图（不进任何实体、永不回收）
    let img = Image::new(
        bevy::render::render_resource::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        vec![0u8; 4 * 4 * 4],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    let _leaked_image = app.world_mut().resource_mut::<Assets<Image>>().add(img);
    assert_eq!(
        image_assets(&app),
        images0 + 1,
        "`image_assets` 看不见一张新增的图 —— 「回退后 Image 资产相等」这条判据是假的"
    );

    // 泄漏一份 mesh（同上）
    let _leaked_mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(1.0, 1.0));
    assert_eq!(
        mesh_assets(&app),
        meshes0 + 1,
        "`mesh_assets` 看不见一份新增的 mesh —— 同理"
    );
}

// ══════════════════════════ 需求断言 ══════════════════════════

/// REQ-1（AC1 · 开 ⇒ 独占）：开 ⇒ chunk 实体 **0**、quad **1**、块级资产清零，
/// 且稳态 `FRAMES` 帧 `field_repacks` Δ == **0**（首帧恰好 1，作计数器自证）。
///
/// 「独占」是第一判据：两块地形同时在画 = 双重叠加（色差、Z 抖动、双份显存），
/// 比不开还糟。
#[test]
fn req_field_on_is_exclusive() {
    // 先让 chunk 路径真的活起来，再翻开关——这样「开」是真的**切换**，不是从未建过。
    let mut app = field_app(true, FIT_SCALE, false);
    app.update();
    assert_eq!(
        chunk_entities(&mut app),
        EXPECTED_CHUNKS,
        "前置：关 ⇒ {EXPECTED_CHUNKS} 块"
    );

    iface::set_field_render(&mut app, true);
    app.update();
    app.update(); // 第 2 帧：`field_render_on_chunk_entities` 有一帧滞后，见文件头约定 1

    assert_eq!(
        chunk_entities(&mut app),
        0,
        "开 ⇒ chunk 实体必须 0（实得 {}）——否则两块地形叠加",
        chunk_entities(&mut app)
    );
    assert_eq!(
        chunk_registry_len(&app),
        0,
        "开 ⇒ `TerrainChunkEntities` 注册表也必须清空（残留会使关回时错判「块还在」）"
    );
    assert_eq!(iface::quads(&mut app), 1, "开 ⇒ 场 quad 必须恰好 1");
    assert_eq!(
        iface::on_chunk_entities(&app),
        0,
        "主线计数器与实体口径必须一致（活着的块数 = 0）"
    );
    assert_eq!(
        mesh_assets(&app),
        1,
        "开 ⇒ 只剩场 quad 的那一份 mesh（实得 {}）——块的 mesh 必须回收",
        mesh_assets(&app)
    );
    assert_eq!(
        color_material_assets(&app),
        0,
        "开 ⇒ 块的 ColorMaterial 必须全部回收（实得 {}）",
        color_material_assets(&app)
    );

    // 首帧允许（且必须）打包一次：否则计数器不可信。
    let r0 = iface::repacks(&app);
    assert_eq!(
        r0, 1,
        "开 ⇒ 应恰好 repack 1 次（自证计数器可信，实得 {r0}）"
    );

    for _ in 0..FRAMES {
        app.update();
    }
    let delta = iface::repacks(&app) - r0;
    assert_eq!(
        delta, 0,
        "稳态 {FRAMES} 帧又 repack 了 {delta} 次 —— 没有编辑却在重打包地形（≈8MB/次）"
    );
    assert_eq!(chunk_entities(&mut app), 0, "稳态帧里 chunk 实体不得回来");
    assert_eq!(iface::quads(&mut app), 1, "稳态帧里 quad 数不得变化");
}

/// REQ-2（AC2 · 关 ⇒ 完整回退）：**关 → 开 → 关**完整来回之后，
/// quad **0**、场材质回收 **0**、chunk 实体恢复 **4**，且
/// mesh / ColorMaterial / Image 三类资产**逐项等于一个从未开过场渲染的 App**。
///
/// 「完整」的口径不是「看起来回来了」，而是**资源账目与从未开过的 App 相等**——
/// oracle 是独立构造的纯 chunk App（独立复算，不与被测同源）。
#[test]
fn req_field_off_restores_the_chunk_path() {
    let mut app = field_app(true, FIT_SCALE, false);
    app.update();
    assert_eq!(chunk_entities(&mut app), EXPECTED_CHUNKS, "前置：关 ⇒ 4 块");

    iface::set_field_render(&mut app, true);
    app.update();
    app.update();
    assert_eq!(iface::quads(&mut app), 1, "前置：开 ⇒ 1 个 quad");
    assert_eq!(chunk_entities(&mut app), 0, "前置：开 ⇒ 0 块");

    iface::set_field_render(&mut app, false);
    app.update();
    // 这一条单独立：关掉场的**那一帧** chunk 层必须已按新状态 spawn
    // （`app_bootstrap.rs:428-430` 声称「中间不留两条路径都空的空窗」）。
    // 若块晚一帧才回来，用户会看到一帧无底图的地图——闪一下。
    assert_eq!(
        chunk_entities(&mut app),
        EXPECTED_CHUNKS,
        "关掉场的**当帧** chunk 实体就必须回来（实得 {}）——否则闪一帧无底图",
        chunk_entities(&mut app)
    );
    app.update(); // 第 2 帧：块数与 `field_render_on_chunk_entities` 都落定

    assert_eq!(iface::quads(&mut app), 0, "关 ⇒ quad 实体必须 0");
    assert_eq!(
        iface::field_material_assets(&app),
        0,
        "关 ⇒ 场材质资产必须回收（实得 {}）——despawn 实体不回收资产就是泄漏",
        iface::field_material_assets(&app)
    );
    assert_eq!(
        chunk_entities(&mut app),
        EXPECTED_CHUNKS,
        "关 ⇒ chunk 实体必须恢复应有的 {EXPECTED_CHUNKS} 个可见块（实得 {}）",
        chunk_entities(&mut app)
    );
    assert_eq!(
        chunk_registry_len(&app),
        EXPECTED_CHUNKS,
        "注册表也必须恢复 {EXPECTED_CHUNKS} 项——否则下次开/关数不出来"
    );
    assert_eq!(
        iface::on_chunk_entities(&app),
        EXPECTED_CHUNKS as u64,
        "主线计数器与实体口径必须一致（同为 {EXPECTED_CHUNKS}）"
    );

    // oracle：从未开过场渲染的纯 chunk App，资产账目必须与本 App **相等**。
    let mut oracle = chunk_app(true, FIT_SCALE);
    oracle.update();
    assert_eq!(
        mesh_assets(&app),
        mesh_assets(&oracle),
        "回退后 mesh 资产 {} ≠ 纯 chunk 基线 {}（差 = 未回收的块/quad mesh）",
        mesh_assets(&app),
        mesh_assets(&oracle)
    );
    assert_eq!(
        color_material_assets(&app),
        color_material_assets(&oracle),
        "回退后 ColorMaterial 资产 {} ≠ 纯 chunk 基线 {}",
        color_material_assets(&app),
        color_material_assets(&oracle)
    );
    assert_eq!(
        image_assets(&app),
        image_assets(&oracle),
        "回退后 Image 资产 {} ≠ 纯 chunk 基线 {}（场的四张数据纹理没被回收）",
        image_assets(&app),
        image_assets(&oracle)
    );
}

/// REQ-3（AC2 防泄漏 · 可达性极值）：连续开关 `TOGGLE_ROUNDS` 次
/// ⇒ 四类资产**不得累积增长**（每轮回到同一基线）。
#[test]
fn neg_twenty_toggle_rounds_do_not_accumulate() {
    let mut app = field_app(true, FIT_SCALE, false);
    app.update();
    let mut baseline: Option<(usize, usize, usize, usize)> = None;

    for round in 0..TOGGLE_ROUNDS {
        iface::set_field_render(&mut app, true);
        app.update();
        app.update();
        assert_eq!(iface::quads(&mut app), 1, "第 {round} 轮：开 ⇒ 1 quad");
        assert_eq!(chunk_entities(&mut app), 0, "第 {round} 轮：开 ⇒ 0 chunk");

        iface::set_field_render(&mut app, false);
        app.update();
        app.update();
        assert_eq!(iface::quads(&mut app), 0, "第 {round} 轮：关 ⇒ 0 quad");
        assert_eq!(
            chunk_entities(&mut app),
            EXPECTED_CHUNKS,
            "第 {round} 轮：关 ⇒ {EXPECTED_CHUNKS} 块"
        );

        let now = (
            mesh_assets(&app),
            color_material_assets(&app),
            image_assets(&app),
            iface::field_material_assets(&app),
        );
        let base = *baseline.get_or_insert(now);
        assert!(
            now.0 <= base.0 && now.1 <= base.1 && now.2 <= base.2 && now.3 <= base.3,
            "第 {round} 轮关闭后资产 (Mesh, ColorMaterial, Image, 场材质) = {now:?} \
             超过首轮基线 {base:?} —— 开关一次泄漏一批资产"
        );
    }
}

/// REQ-4（AC3 · 编辑恰好一次写入）：一笔局部编辑 ⇒ **`field_patches` +1、
/// `field_repacks` +0**。
///
/// 三个方向都要钉死：
/// - **两边都 0 = 编辑没生效**（画了不显示，作者会以为笔刷坏了）；
/// - **合计 2 = 重复搬数据**（一次编辑付两遍）；
/// - **走了全量重打 = 白付 48 ms**（1024² 实测）。局部编辑必须只补脏区，
///   这是「笔刷手感」的直接来源，不是可选优化。
///
/// 随后稳态必须 +0；第二笔必须再 +1（否则「只在首帧动过」的实现也能过）。
#[test]
fn req_one_edit_yields_exactly_one_incremental_write() {
    let mut app = field_app(true, FIT_SCALE, true);
    app.update();
    let (r0, p0) = (iface::repacks(&app), iface::patches(&app));
    assert_eq!(
        r0, 1,
        "前置：开 ⇒ 恰好 1 次全量重打（自证计数器可信，实得 {r0}）"
    );
    assert_eq!(p0, 0, "前置：开局不该有补丁（实得 {p0}）");

    iface::commit_one_cell_edit(&mut app);
    app.update();
    assert_eq!(
        iface::patches(&app) - p0,
        1,
        "局部编辑 ⇒ 必须**恰好** 1 次增量补丁（实得 {}；0 = 没生效）",
        iface::patches(&app) - p0
    );
    assert_eq!(
        iface::repacks(&app),
        r0,
        "局部编辑**不得**触发全量重打（实得 +{}；多打一次就白付 48 ms）",
        iface::repacks(&app) - r0
    );

    let (r1, p1) = (iface::repacks(&app), iface::patches(&app));
    for _ in 0..FRAMES {
        app.update();
    }
    assert_eq!(
        (iface::repacks(&app), iface::patches(&app)),
        (r1, p1),
        "编辑后稳态 {FRAMES} 帧又写了 {} 次（数据没再变，不该再动纹理）",
        (iface::repacks(&app) - r1) + (iface::patches(&app) - p1)
    );

    iface::commit_one_cell_edit(&mut app);
    app.update();
    assert_eq!(
        iface::patches(&app) - p1,
        1,
        "第二次编辑必须同样恰好 +1 补丁（实得 {}）",
        iface::patches(&app) - p1
    );
    assert_eq!(iface::repacks(&app), r1, "第二次编辑也不得升级成全量重打");
}

/// REQ-4b（回退路径 · 无脏区 ⇒ 全量）：数据变了但**没有可信局部脏区** ⇒
/// `field_repacks` +1、`field_patches` +0。
///
/// 这条与 REQ-4 是一对反向约束：没有局部信号时**不许猜**一个矩形去补丁
/// （猜小了就是静默画错——岸线色带残留旧值，且没有任何报错），宁可整份重打。
#[test]
fn neg_missing_patch_rect_falls_back_to_full_repack() {
    let mut app = field_app(true, FIT_SCALE, true);
    app.update();
    let (r0, p0) = (iface::repacks(&app), iface::patches(&app));

    iface::commit_edit_without_field_rect(&mut app);
    app.update();
    assert_eq!(
        iface::repacks(&app) - r0,
        1,
        "无脏区 ⇒ 必须整份重打 1 次（实得 {}）",
        iface::repacks(&app) - r0
    );
    assert_eq!(
        iface::patches(&app),
        p0,
        "无脏区**不得**走补丁（实得 +{}；猜矩形是静默画错的来源）",
        iface::patches(&app) - p0
    );
}

/// REQ-4c（脏区形状无关 · 四个角 + 跨全幅）：补丁路径对脏区位置不敏感。
///
/// 只测正中间一块的实现会在「贴着幅面边界」时崩（窗口外扩被裁）或写越界。
#[test]
fn req_patch_works_for_rects_at_every_corner() {
    let mut app = field_app(true, FIT_SCALE, true);
    app.update();
    let mut p = iface::patches(&app);
    for rect in [
        [0, 0, 4, 4],                                         // 左上
        [MAP_CELLS - 4, 0, MAP_CELLS, 4],                     // 右上
        [0, MAP_CELLS - 4, 4, MAP_CELLS],                     // 左下
        [MAP_CELLS - 4, MAP_CELLS - 4, MAP_CELLS, MAP_CELLS], // 右下
        [0, MAP_CELLS / 2, MAP_CELLS, MAP_CELLS / 2 + 2],     // 横贯全幅
    ] {
        iface::commit_region_edit(&mut app, rect);
        app.update();
        assert_eq!(
            iface::patches(&app) - p,
            1,
            "脏区 {rect:?} 应走补丁（实得 Δ={}）",
            iface::patches(&app) - p
        );
        p = iface::patches(&app);
        assert_eq!(
            iface::quads(&mut app),
            1,
            "补丁不得弄丢 quad（rect={rect:?}）"
        );
    }
}

/// REQ-5（AC4 · 负向）：`layers.terrain.visible = false` ∧ `field_render = true`
/// ⇒ quad **0**，且 chunk 实体也 **0**（两边都不画）。
///
/// 意义：开关是「用哪条管线」，图层可见性是「画不画地形」——两者正交。
/// 若 quad 不看图层开关，作者关掉地形层后整块底图仍留在屏幕上。
#[test]
fn neg_terrain_layer_hidden_yields_zero_quads() {
    let mut app = field_app(false, FIT_SCALE, true);
    app.update();
    assert_eq!(
        iface::quads(&mut app),
        0,
        "地形图层隐藏 + 场开关开着 ⇒ quad 必须 0（实得 {}）",
        iface::quads(&mut app)
    );
    assert_eq!(
        chunk_entities(&mut app),
        0,
        "同时 chunk 实体也必须 0（不是二选一）"
    );
    assert_eq!(mesh_assets(&app), 0, "两条路径都退场 ⇒ 不得留 mesh 资产");

    // 反向：把图层打开，quad 必须回来 —— 否则上面那条「恒 0」也能过。
    app.world_mut().resource_mut::<LayerFrame>().terrain.visible = true;
    app.update();
    assert_eq!(
        iface::quads(&mut app),
        1,
        "图层打开后 quad 必须恢复为 1（否则负向断言是恒真的假绿）"
    );
}

/// REQ-6（一致性 · 表 vs 行为）：开关关时**不得**再有场 quad，开关开时
/// **不得**再有 chunk 实体——两条路径不同时活着，四种状态各断一次。
///
/// 与 REQ-1/REQ-2 的区别：这条不依赖任何资产账目，是独占性的**最小内核**；
/// 即使资产回收还没做完，独占性也能独立判红/绿。
#[test]
fn req_two_paths_are_never_alive_in_the_same_frame() {
    let mut app = field_app(true, FIT_SCALE, true);
    for (i, on) in [true, false, true, false].iter().enumerate() {
        iface::set_field_render(&mut app, *on);
        app.update();
        app.update();
        let chunks = chunk_entities(&mut app);
        let quads = iface::quads(&mut app);
        let expect = if *on { (0, 1) } else { (EXPECTED_CHUNKS, 0) };
        assert_eq!(
            (chunks, quads),
            expect,
            "第 {i} 步（field_render = {on}）：期望 (chunk, quad) = {expect:?}，\
             实得 ({chunks}, {quads})"
        );
    }
}

/// REQ-7（AC3 的**资产面** · 本文件认为最容易漏的一条）：连续编辑
/// `TOGGLE_ROUNDS`（= 20）笔，每笔恰好一次写入（补丁或重打），但
/// **Image / Mesh / 场材质资产不得累积**。
///
/// 为什么单列：`build_material` 每调一次就往 `Assets<Image>` 里 add
/// height/coast/attr/line_dash **四张**纹理。全量 replack 若只重建不回收上一份，
/// **每次笔刷提交泄漏一份全量数据纹理**——单帧完全看不出，连画 30 笔就吃掉
/// 上百 MB 显存。开关轮次断言（REQ-3）**抓不到这条**：它只翻开关，不编辑。
///
/// 增量补丁路径天然不建新资产，这条断言因此同时守住「补丁没有被悄悄降级成
/// 全量重建」——降级会让资产计数虽不涨（旧材质有回收）但笔刷手感回到 48 ms，
/// 那种回归靠 `field_patches` 的 Δ 断言（REQ-4）兜住。
#[test]
fn neg_twenty_edits_do_not_accumulate_field_assets() {
    let mut app = field_app(true, FIT_SCALE, true);
    app.update();
    let (r0, p0) = (iface::repacks(&app), iface::patches(&app));
    let base = (
        image_assets(&app),
        mesh_assets(&app),
        iface::field_material_assets(&app),
    );

    for round in 0..TOGGLE_ROUNDS {
        iface::commit_one_cell_edit(&mut app);
        app.update();
        let writes = (iface::repacks(&app) - r0) + (iface::patches(&app) - p0);
        assert_eq!(
            writes,
            round as u64 + 1,
            "第 {round} 笔：累计写入应为 {} 次，实得 {writes}",
            round + 1
        );
        let now = (
            image_assets(&app),
            mesh_assets(&app),
            iface::field_material_assets(&app),
        );
        assert!(
            now.0 <= base.0 && now.1 <= base.1 && now.2 <= base.2,
            "第 {round} 笔后资产 (Image, Mesh, 场材质) = {now:?} 超过首笔基线 {base:?} ——\
             repack 泄漏：每笔留下上一份数据纹理/材质（单帧看不出，30 笔就爆显存）"
        );
    }
}
