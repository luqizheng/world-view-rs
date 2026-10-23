//! 选中自然区的装饰几何 —— 阶梯描边 + 顶点锚点，**合批 Mesh2d**。
//!
//! ## 为什么不再用 Gizmos（2026-10-24 用户实测）
//!
//! 症状：开「显示自然区与候选」→ 点选一个自然区 → 锚点出现时 FPS 掉到 20 多。
//!
//! 根因：`draw_map_gizmos` 里**逐条** `gizmos.line_2d` 画阶梯边（mask 内格与
//! 4 邻之间每条边界一条）。选中一个大区域时格边数可达**上万**，而 Bevy 的
//! Gizmos 是即时线元——每条都要装配一个 `LineStrip2d` + 一个渲染实例，且
//! **每帧重来一遍**。选中前这条路径不跑（门控在 `nature_overlay`），所以症状
//! 是「一点选就掉帧」；取消选中立刻恢复。
//!
//! 处置：改成与道路 / 区域轮廓 / 小河预览同款的**合批 mesh**——
//! 整组阶梯边合成 1 个 mesh、整组锚点合成 1 个 mesh，键不变就**不重建**。
//! 稳态每帧只付 2 个 draw call，与边数 / 锚点数无关（REQ-2 的规模无关口径）。
//!
//! ## 视觉裁定
//!
//! 锚点从「红十字 + 外圈」改为**实心白色小方块**（用户 2026-10-24 裁定）。
//! Gizmos 画不出填充面，这也是必须走 mesh 的第二个理由。
//!
//! ## 边界
//!
//! - 只**读**世界（判定与派生在 `world_projection::plan_region_handles`），
//!   不写世界、不经 `EditCommand`。
//! - 不持有编辑器会话态：输入全是 wv-render 自己的资源。
//! - 门控只有一处（`region_handles_visible`），本模块不重复判 `nature_overlay`。

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec2;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use crate::state::{
    DrillState, GridConfig, RegionAdjustState, RegionCandidateContext, ViewState, WorldProjection,
};
use crate::world_projection::{plan_region_handles, RegionDecorCache, RegionHandlePlan};

/// 阶梯描边线宽（屏幕 px）。
///
/// 1.4px：比区域轮廓（1.6px）细一档——阶梯边是「选中装饰」，不该盖过区域本身
/// 的轮廓；再细会掉到亚像素，白线在沙色地形上开始闪。
const EDGE_STROKE_PX: f32 = 1.4;

/// z：压在区域轮廓（0.076）/ 候选预览（0.077）之上，道路（0.09）之下。
///
/// 描边与锚点错开 0.0005：两块都是不透明白，同 z 时排序不稳定（闪烁）。
pub const REGION_DECOR_EDGE_Z: f32 = 0.0785;
/// 锚点方块 z（见 [`REGION_DECOR_EDGE_Z`]）。
pub const REGION_DECOR_ANCHOR_Z: f32 = 0.0790;

/// 装饰 mesh 的运行时池（两块合批 mesh + 共享材质 + 计数器）。
///
/// `Default` 即可用（`init_resource`）——没有可画内容时两块都是 `None`。
#[derive(Resource, Default)]
pub struct RegionDecorPool {
    edges: Option<DecorBatch>,
    anchors: Option<DecorBatch>,
    material: Option<Handle<ColorMaterial>>,
    /// **计数器**（QA 口径）：阶梯边 mesh 重建次数（累计）。
    pub edge_rebuilds: u64,
    /// **计数器**：锚点 mesh 重建次数（累计）。
    pub anchor_rebuilds: u64,
    /// **计数器**：本帧是否有可画的装饰（0/1，每帧刷新）。
    pub visible: u8,
}

/// 一块合批 mesh 的句柄 + 重建键。
struct DecorBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: DecorKey,
}

/// mesh 重建键：内容指纹 + 尺寸（线宽 / 方块边长）的位模式。
///
/// 尺寸取**缩放档中心**（`scale_of_bucket`）而不是 `view.scale` 原值，
/// 否则 smooth zoom 的每帧微动都会换键 = 等于没缓存（与轮廓线宽同一条口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DecorKey {
    fingerprint: u64,
    size_bits: u64,
}

impl RegionDecorPool {
    /// 回收两块 mesh（实体 + 资产）。关层 / 选中被取消 / 无可画内容时调用。
    fn clear(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        for batch in [self.edges.take(), self.anchors.take()]
            .into_iter()
            .flatten()
        {
            commands.entity(batch.entity).despawn();
            // 只 despawn 不 remove 会把资产留成无人引用的死条目（区域轮廓
            // 那一轮的教训：200 区反复开关就泄漏 200 份 mesh）。
            meshes.remove(&batch.mesh);
        }
    }
}

/// 选中区域 / 未物化候选的装饰绘制（阶梯描边 + 实心白方块锚点）。
///
/// 每帧流程：求计划（带缓存）→ 比对键 → 只在键变时重建 mesh → 刷新计数器。
/// **不做逐条图元调用**：这是本系统存在的全部理由。
#[allow(clippy::too_many_arguments)]
pub fn sync_region_decor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    region_adjust: Res<RegionAdjustState>,
    candidate_ctx: Res<RegionCandidateContext>,
    mut decor: ResMut<RegionDecorCache>,
    mut pool: ResMut<RegionDecorPool>,
) {
    // ── ① 求计划 + 决定「要不要重建」（计划活着时借住 `decor`，故先算完再写）──
    // 三态而不是 Option：**「本帧不用改」与「本帧要清空」是两件事**。
    // 混成一个 `Option` 会把稳态帧（键没变）当成空态，于是每帧 despawn 一次、
    // 下一帧再重建——实测就是这个写法让 `steady_state_never_rebuilds` 数出
    // 「60 帧重建 31 次」（隔帧重建），比不修还糟。
    let mut edge_update = BatchUpdate::Clear;
    let mut anchor_update = BatchUpdate::Clear;
    let (segments, handles);
    {
        let plan = plan_region_handles(
            &projection,
            &drill,
            &view,
            &grid,
            &region_adjust,
            &candidate_ctx,
            &mut decor,
        );
        segments = plan.stepped_edges.len() as u64;
        handles = plan.handle_count() as u64;

        let bucket = crate::region_outline::scale_bucket(view.scale);
        let bucket_scale = crate::region_outline::scale_of_bucket(bucket);

        if !plan.stepped_edges.is_empty() {
            let thickness = EDGE_STROKE_PX * bucket_scale;
            let key = DecorKey {
                fingerprint: plan.edges_fp,
                size_bits: u64::from(thickness.to_bits()),
            };
            edge_update = match pool.edges.as_ref().map(|b| b.key) {
                Some(k) if k == key => BatchUpdate::Keep,
                _ => BatchUpdate::Rebuild(
                    key,
                    Box::new(crate::vector_layers::build_stroke_batch_mesh(
                        plan.stepped_edges,
                        thickness,
                    )),
                ),
            };
        }
        if handles > 0 {
            // 方块边长 = `handle_px`（档中心求值的半边） × 2，即屏幕约 7px 见方。
            let half = plan.handle_px;
            let key = DecorKey {
                fingerprint: anchors_fp(&plan),
                size_bits: u64::from(half.to_bits()),
            };
            anchor_update = match pool.anchors.as_ref().map(|b| b.key) {
                Some(k) if k == key => BatchUpdate::Keep,
                _ => BatchUpdate::Rebuild(
                    key,
                    Box::new(build_anchor_quad_mesh(
                        plan.anchors.iter().chain(plan.candidate_anchors.iter()),
                        half,
                    )),
                ),
            };
        }
    }
    // 计划已释放：此时才能写计数器（`RegionDecorCache` 的借用冲突）。
    decor.outline_segments_drawn = segments;
    decor.handles_drawn = handles;
    pool.visible = u8::from(segments > 0 || handles > 0);

    // ── ② 无可画内容 ⇒ 回收两块 mesh，直接返回（关层 / 取消选中都走这里）──
    if segments == 0 && handles == 0 {
        pool.clear(&mut commands, &mut meshes);
        return;
    }

    if pool.material.is_none() {
        // 纯白（数据视图不随主题）：实心方块要 opaque，描边也吃同一个材质。
        pool.material = Some(materials.add(ColorMaterial::from_color(Color::WHITE)));
    }
    let material = pool.material.clone().expect("刚建");

    // ── ③ 落地：有重建就换 mesh，没有就什么都不做（稳态零开销）──
    // 计数器先落在局部量上再用（`apply_batch` 同时借 `pool` 的槽位，不能再借
    // 同一个 `pool` 的计数字段）。
    let mut edge_rebuilt = 0u64;
    apply_batch(
        &mut commands,
        &mut meshes,
        &mut pool.edges,
        edge_update,
        &material,
        REGION_DECOR_EDGE_Z,
        &mut edge_rebuilt,
    );
    pool.edge_rebuilds += edge_rebuilt;

    let mut anchor_rebuilt = 0u64;
    apply_batch(
        &mut commands,
        &mut meshes,
        &mut pool.anchors,
        anchor_update,
        &material,
        REGION_DECOR_ANCHOR_Z,
        &mut anchor_rebuilt,
    );
    pool.anchor_rebuilds += anchor_rebuilt;
}

/// 单块合批 mesh 的本帧意图。
///
/// **三态是必须的**：`Rebuild` 与 `Clear` 都会动实体，`Keep` 一个字节都不动——
/// 用 `Option` 表达会退化成两态，稳态帧被当成空态（实测隔帧重建）。
enum BatchUpdate {
    /// 键没变：本帧什么都不做（稳态路径，必须零开销）。
    Keep,
    /// 键变了或首次：换 mesh（有实体就原地换，没有就 spawn）。
    ///
    /// `Box<Mesh>`：`Mesh` 有几百字节（顶点 / 索引 Vec + 属性表），直接内联会让
    /// 这个枚举整体 240 字节，而 `Keep` / `Clear` 是每帧都会构造的常态。
    /// 装箱的代价只在**键变那一帧**付一次（重建本来就伴随一次分配）。
    Rebuild(DecorKey, Box<Mesh>),
    /// 本帧没有这类内容：回收实体与资产。
    Clear,
}

/// 把本帧意图落到实体上。
fn apply_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    slot: &mut Option<DecorBatch>,
    update: BatchUpdate,
    material: &Handle<ColorMaterial>,
    z: f32,
    rebuilds: &mut u64,
) {
    match update {
        BatchUpdate::Keep => {}
        BatchUpdate::Clear => {
            if let Some(batch) = slot.take() {
                commands.entity(batch.entity).despawn();
                meshes.remove(&batch.mesh);
            }
        }
        BatchUpdate::Rebuild(key, mesh) => {
            *rebuilds += 1;
            match slot.take() {
                None => {
                    let handle = meshes.add(*mesh);
                    let entity = commands
                        .spawn((
                            Mesh2d(handle.clone()),
                            MeshMaterial2d(material.clone()),
                            Transform::from_xyz(0.0, 0.0, z),
                        ))
                        .id();
                    *slot = Some(DecorBatch {
                        entity,
                        mesh: handle,
                        key,
                    });
                }
                Some(mut batch) => {
                    if let Some(mut old) = meshes.get_mut(&batch.mesh) {
                        *old = *mesh;
                    }
                    batch.key = key;
                    *slot = Some(batch);
                }
            }
        }
    }
}

/// 锚点位置指纹（选中 + 候选两串一起算）。
///
/// 锚点数量是**抽稀后**的（屏幕 24px 一个），量级几百，逐帧现算可接受；
/// 与之相对，阶梯边上万条，指纹随派生缓存一起算（`RegionDecorCache::edges_fp`）。
fn anchors_fp(plan: &RegionHandlePlan<'_>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for p in plan.anchors.iter().chain(plan.candidate_anchors.iter()) {
        for v in [p.x, p.y] {
            h ^= u64::from(v.to_bits());
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    // 两串的分界也进指纹：`[[a],[b]]` 与 `[[a,b],[]]` 的点序相同时必须不同。
    h ^= plan.anchors.len() as u64;
    h.wrapping_mul(0x0000_0100_0000_01b3)
}

/// 每个锚点一个**实心方块**（4 顶点 / 6 索引），整组合成一个 mesh。
///
/// `half` = 半边长（世界单位）。`Mesh::new` 与描边批同口径：
/// `RENDER_WORLD | MAIN_WORLD`（主世界留一份，`meshes.get_mut` 才能原地换）。
fn build_anchor_quad_mesh<'a>(centers: impl Iterator<Item = &'a Vec2>, half: f32) -> Mesh {
    let centers: Vec<Vec2> = centers.copied().collect();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(centers.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(centers.len() * 6);
    for c in &centers {
        let base = positions.len() as u32;
        positions.push([c.x - half, c.y - half, 0.0]);
        positions.push([c.x + half, c.y - half, 0.0]);
        positions.push([c.x + half, c.y + half, 0.0]);
        positions.push([c.x - half, c.y + half, 0.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// 手柄方块半边的屏幕像素只有一份载体（`world_projection::HANDLE_HALF_PX`）——
// 本模块只消费 `plan.handle_px`（世界单位），不再重复定义「6px / 7px」这类数字。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ViewMode, WorldProjection};
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::model::region::{Region, RegionKind};
    use wv_core::{EntityKind, Id, SelectedEntity, World};

    const MAP: &str = "map_world";
    const CELL: f32 = 4.0;
    const SCALE: f32 = 5.12;

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

    /// 64×64 格的人工自然区（覆盖 256×256 世界单位）。
    fn region() -> Region {
        Region {
            id: Id::new_unchecked("nr_big"),
            name: "大自然区".into(),
            map: Id::new_unchecked(MAP),
            polygon: vec![
                64.0, 64.0, 320.0, 64.0, 320.0, 320.0, 64.0, 320.0, 64.0, 64.0,
            ],
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        }
    }

    fn app(selected: bool, nature_overlay: bool) -> App {
        let mut drill = DrillState::default();
        drill.settle_on(Id::new_unchecked(MAP));
        let mut world = World {
            maps: vec![map_node()],
            regions: vec![region()],
            ..Default::default()
        };
        // 128×128 格（cell 4 ⇒ 512×512 wu）：区域多边形 64..320 落在栅格内。
        // 没有 `TerrainData` 时 `plan_region_handles` 会早退（无地形 = 无格边可贴合），
        // 那样测出来的「0 次重建」是假绿。
        let id = Id::new_unchecked(MAP);
        world.terrains.insert(
            id,
            wv_core::terrain::TerrainData {
                width: 128,
                height: 128,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height: 100,
                        surface: 1,
                        biome: 3,
                    };
                    128 * 128
                ],
            },
        );
        let selection =
            selected.then(|| SelectedEntity::new(EntityKind::Region, Id::new_unchecked("nr_big")));
        let mut app = App::new();
        app.insert_resource(Assets::<Mesh>::default())
            .insert_resource(Assets::<ColorMaterial>::default())
            .insert_resource(WorldProjection {
                world,
                selection,
                ..Default::default()
            })
            .insert_resource(drill)
            .insert_resource(GridConfig {
                cell_size: CELL,
                raster_origin: Vec2::ZERO,
                map_origin: Vec2::ZERO,
                map_size: Vec2::splat(4096.0),
            })
            .insert_resource(ViewState {
                mode: ViewMode::Topographic,
                nature_overlay,
                scale: SCALE,
                ..Default::default()
            })
            .init_resource::<RegionAdjustState>()
            .init_resource::<RegionCandidateContext>()
            .init_resource::<RegionDecorCache>()
            .init_resource::<RegionDecorPool>()
            .add_systems(Update, sync_region_decor);
        app
    }

    fn counts(app: &App) -> (u64, u64) {
        let p = app.world().resource::<RegionDecorPool>();
        (p.edge_rebuilds, p.anchor_rebuilds)
    }

    fn decor_meshes(app: &mut App) -> usize {
        let mut q = app.world_mut().query::<&Mesh2d>();
        q.iter(app.world()).count()
    }

    /// 阳性对照（自证计数器可信）：选中一个人工区 ⇒ 首帧必须建两块 mesh。
    #[test]
    fn first_frame_builds_edges_and_anchors() {
        let mut app = app(true, true);
        app.update();
        assert_eq!(counts(&app), (1, 1), "首帧应各建一次（描边 + 锚点）");
        assert_eq!(decor_meshes(&mut app), 2, "应有 2 个装饰 mesh 实体");
        let cache = app.world().resource::<RegionDecorCache>();
        assert!(cache.outline_segments_drawn > 0, "选中区应有阶梯描边");
        assert!(cache.handles_drawn > 0, "选中区应有锚点方块");
    }

    /// REQ（用户症状的计数面）：稳态 60 帧 ⇒ 重建次数 **0**（边数与帧数无关）。
    #[test]
    fn steady_state_never_rebuilds() {
        let mut app = app(true, true);
        app.update();
        let (e0, a0) = counts(&app);
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(
            counts(&app),
            (e0, a0),
            "稳态 60 帧又重建了 mesh（用户症状：选中大区域 FPS 掉到 20 多）"
        );
        assert_eq!(decor_meshes(&mut app), 2, "稳态不该新增/回收实体");
    }

    /// 负向断言：关群系视图 ⇒ 两块 mesh 全回收（实体 + 资产都不留）。
    #[test]
    fn overlay_off_reclaims_meshes() {
        let mut app = app(true, true);
        app.update();
        assert_eq!(decor_meshes(&mut app), 2, "前置：开 ⇒ 2 块");

        app.world_mut().resource_mut::<ViewState>().nature_overlay = false;
        app.update();
        assert_eq!(decor_meshes(&mut app), 0, "关层后实体必须归 0");
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            0,
            "关层后 mesh 资产也必须归 0（只 despawn 会泄漏）"
        );
        let cache = app.world().resource::<RegionDecorCache>();
        assert_eq!(cache.handles_drawn, 0, "关层后手柄计数必须 0");
        assert_eq!(cache.outline_segments_drawn, 0, "关层后描边计数必须 0");
    }

    /// 选中被取消 ⇒ 装饰一起消失（不留残影）。
    #[test]
    fn deselecting_removes_the_decor() {
        let mut app = app(true, true);
        app.update();
        app.world_mut().resource_mut::<WorldProjection>().selection = None;
        app.update();
        assert_eq!(decor_meshes(&mut app), 0, "取消选中后装饰必须消失");
    }

    /// 方块几何口径：N 个锚点 = 4N 顶点 / 6N 索引，且是**以锚点为中心的正方形**。
    #[test]
    fn anchor_mesh_is_one_filled_square_per_anchor() {
        let centers = [Vec2::new(10.0, 20.0), Vec2::new(-3.5, 7.25)];
        let mesh = build_anchor_quad_mesh(centers.iter(), 3.0);
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("有位置属性");
        let bevy::mesh::VertexAttributeValues::Float32x3(pts) = pos else {
            panic!("顶点应为 Float32x3");
        };
        assert_eq!(pts.len(), 8, "2 个锚点 × 4 顶点");
        // 手算：第 0 个锚点 (10,20) 半边长 3 ⇒ 角点 (7,17)/(13,17)/(13,23)/(7,23)
        let want = [[7.0, 17.0], [13.0, 17.0], [13.0, 23.0], [7.0, 23.0]];
        for (got, want) in pts[..4].iter().zip(want) {
            assert_eq!([got[0], got[1]], want, "方块角点必须中心对齐");
        }
        let idx = mesh.indices().expect("有索引");
        assert_eq!(idx.len(), 12, "2 个锚点 × 6 索引");
    }

    /// 尺寸走缩放档：同档内 scale 微动**不得**重建（否则 smooth zoom 每帧重建）。
    #[test]
    fn same_zoom_bucket_does_not_rebuild() {
        let mut app = app(true, true);
        app.update();
        let (e0, a0) = counts(&app);
        // ×1.02 是滚动惯性的量级（同 1/2 octave 档内）。
        app.world_mut().resource_mut::<ViewState>().scale = SCALE * 1.02;
        app.update();
        assert_eq!(
            counts(&app),
            (e0, a0),
            "同档内微动不该重建装饰 mesh（键必须用档中心的 scale）"
        );
    }

    /// 跨档必须**恰好**重建一次（证明键真的跟缩放走，不是恒不重建）。
    #[test]
    fn crossing_a_zoom_bucket_rebuilds_once() {
        let mut app = app(true, true);
        app.update();
        let (e0, a0) = counts(&app);
        // ×3 跨过 1/2 octave 的若干档。
        app.world_mut().resource_mut::<ViewState>().scale = SCALE * 3.0;
        app.update();
        let (e1, a1) = counts(&app);
        assert_eq!(e1 - e0, 1, "跨档后描边应恰好重建 1 次（实得 {}）", e1 - e0);
        assert_eq!(a1 - a0, 1, "跨档后锚点应恰好重建 1 次（实得 {}）", a1 - a0);
    }
}
