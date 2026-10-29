//! 区域轮廓渲染（ADR-0045 追记四）——轮廓线框 + 底色覆盖。
//!
//! ## 为什么要有这个模块
//!
//! 区域实体（`Region`）此前**只有标签、没有轮廓**：作者物化一个自然区之后，
//! 地图上看不出它到底圈了哪块地——「改覆盖物」也就无从下手（改的是哪一片？）。
//! 本模块补上两条：
//!
//! 1. **轮廓线框**：区域多边形（外环 + 洞环）画成线，
//!    Z `REGION_OUTLINE_Z = 0.076`（地形装饰 0.075 之上 / 道路 0.09 之下）——
//!    作者划的边界要压在底图族之上才醒目，又不能压过道路。
//! 2. **底色覆盖**：区域**声明了覆盖物键** → 用该键的主题规则色画一层
//!    半透明色块（Z `REGION_FILL_Z = 0.064`，底图族内、道路之下）。
//!
//! ## 为什么底色覆盖走覆盖层而不是改烘焙
//!
//! 烘焙（`terrain_pixels`）是**全图重算或增量 patch** 两条路；若底色覆盖进
//! 烘焙，则「改一个区域的覆盖物键」要触发纹理重建 + 增量 patch 的失效传播
//! （脏区要按覆盖范围膨胀）。走覆盖层则：**改键即生效**（下一帧重画色块），
//! 纹理烘焙契约（局部重画 = 全图重画，逐字节一致）完全不受影响。
//!
//! ## 边界
//!
//! - **不写世界**：纯渲染投影（P7，AGENTS §3）；
//! - **不改数据**：区域多边形只读；洞环的扣减由三角化（earcut）负责；
//! - **不消费主题之外的规则**：色值来自 `ThemePack::region_overlays`
//!   （经 [`crate::region_overlay::RegionOverlayIndex`] 解析），主题没定义的键
//!   走缺失覆盖物标记（白底红斜线，ADR-0045 追记一），**不在这里**兜底。

#![cfg(feature = "app")]

use crate::fp::grid_bits;
use bevy::prelude::*;

use wv_core::model::region::Region;
use wv_core::Id;

use crate::faction_overlays::earcut_with_holes;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::region_overlay::RegionOverlayIndex;
use crate::state::{DrillState, GridConfig, ViewState, WorldProjection};
use crate::vector_layers::build_stroke_batch_mesh;

/// 区域轮廓线框 Z：地形装饰（0.075）之上、道路（0.09）之下。
pub const REGION_OUTLINE_Z: f32 = 0.076;

/// 区域底色覆盖 Z：雾霭（0.06）附近、地界晕（0.065）之下——属底图族。
pub const REGION_FILL_Z: f32 = 0.064;

/// 轮廓线宽（屏幕像素）。
const OUTLINE_STROKE_PX: f32 = 1.6;

/// 底色覆盖的不透明度（0..1）——**半透明**：底图的地形层次要透出来，
/// 覆盖层是「给这片地定个性」，不是「刷一层漆」。
const FILL_ALPHA: f32 = 0.28;

/// 候选区预览的**帧间输入**（ADR-0045 追记四）。
///
/// app 层每帧刷新（读光标格 → 查 biome 连通区 → 换算成视空间环），本模块的
/// 系统消费。独立 Resource：渲染层不做 UI 拾取（`cell_pick` 在 app 层），
/// 两边的职责靠这个小结构交接。
#[derive(Resource, Debug, Default, Clone)]
pub struct CandidateRegionInput {
    /// 候选区轮廓（**视空间**点列，闭合环；`None` = 光标不在地形上 / 无候选）。
    pub ring: Option<Vec<Vec2>>,
}

/// 区域轮廓的运行时资源（合批 mesh + 实体 + 材质句柄）。
#[derive(Resource, Default)]
pub struct RegionOutlinePool {
    /// 轮廓线框（所有区域的线段合批成一个 mesh）。
    outline: Option<OutlineBatch>,
    /// 底色覆盖（每个有色区域一个 mesh + 实体）。
    fills: Vec<FillBatch>,
    /// 轮廓线材质（固定色，**不随主题色板**——它是「作者划的边界」这一语义）。
    outline_material: Option<Handle<ColorMaterial>>,
    /// **候选区预览**（物化前光标所在 biome 连通区的高亮轮廓）。
    preview: Option<OutlineBatch>,
    /// 候选区预览材质（金色高亮，与选区/意图像一个语义族）。
    preview_material: Option<Handle<ColorMaterial>>,
    /// 底色覆盖材质缓存（色 → 句柄）：同一色的区域共用一个材质。
    fill_materials: Vec<([u8; 3], Handle<ColorMaterial>)>,
    /// 轮廓线段**源**缓存：区域集合 + 多边形 + 网格换算没变 ⇒ 不重算 `segs`。
    ///
    /// 2026-10-23：旧实现每帧把全部区域多边形展开成 `Vec<(Vec2, Vec2)>`
    /// （`push_loop` 里每个环还先分配一个 `Vec<Vec2>`）并重算指纹——
    /// 区域数 200 时这是「开群系视图 26 FPS」的主要来源之一。
    source: Option<OutlineSource>,
    /// 候选预览的输入环（上一帧那份）：相等就完全跳过 `segs` 装配。
    preview_ring: Option<Vec<Vec2>>,
    /// 候选预览的线段缓存（`preview_ring` 的展开结果）。
    preview_segs: Vec<(Vec2, Vec2)>,
    /// 候选预览的线段指纹。
    preview_fp: u64,
    /// **计数器**（QA 断言口径，跨机器同值）：`segs` 重算次数。
    pub seg_recomputes: u64,
    /// **计数器**：`build_stroke_batch_mesh` 调用次数（轮廓 + 候选预览合计）。
    pub stroke_rebuilds: u64,
    /// **计数器**：其中**候选预览**的 mesh 构建次数（与轮廓分开——预览本来就该
    /// 随光标变，「预览重建」不能算成「轮廓重建」）。
    pub preview_stroke_builds: u64,
    /// **计数器**：`OutlineKey` 变化导致的轮廓 mesh 真实重建次数。
    pub outline_rebuilds: u64,
    /// **计数器**：候选预览分支重算 `segs` 的次数。
    pub preview_recomputes: u64,
    /// **计数器**：`region_fill_mesh`（底色覆盖三角化）调用次数。
    pub fill_rasterizes: u64,
}

/// 轮廓线段的源缓存（身份 + 展开结果 + 内容指纹）。
struct OutlineSource {
    key: SegSourceKey,
    segs: Vec<(Vec2, Vec2)>,
    fingerprint: u64,
}

/// `segs` 的重算判据：**只含「线段内容」的输入**。
///
/// 刻意**不含 `view.scale`**：线宽由 [`thickness_bucket`] 量化后进 `OutlineKey`，
/// 缩放不再污染几何缓存（这是 2026-10-23 的第二条病根）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SegSourceKey {
    /// 格尺寸 / 两套原点 / 幅面的位模式（`GridConfig` 变了，视空间坐标就变了）。
    grid_bits: [u64; 7],
    /// 是否走 CellMap 的 Nature-only 过滤（集合口径变了，线也就变了）。
    in_cellmap: bool,
    /// 过滤后的区域数（与 `geom` 一起做「快筛 + 全量」两层）。
    region_count: usize,
    /// 全部区域多边形的 FNV（裸 `f32::to_bits`，无分配）。
    geom: u64,
}

/// 一个合批线框。
struct OutlineBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: OutlineKey,
}

/// 线框的重建判据（内容指纹 + **线宽档** + Z）。
///
/// 2026-10-23：`world_thickness: f32 = 1.6 × view.scale` 是逐档重建的直接原因
/// （缩放每一档都换键）。改成 1/2 octave 的**档号**后，同一个档内连续几档缩放
/// 共用一份几何；屏上宽度落在 `1.6 × [0.84, 1.19] px`（档宽 ×1.414，取档中心），
/// 对一条 1.6px 的边界线是读不出来的差别。
#[derive(Debug, Clone, Copy, PartialEq)]
struct OutlineKey {
    fingerprint: u64,
    thickness_bucket: i16,
    z: f32,
}

/// 缩放 → 1/2 octave 档号（与 `isoline_overlay::dp_tol_bucket` 同口径）。
///
/// 两处消费：① 轮廓线宽（`OUTLINE_STROKE_PX × 档中心 scale`）；
/// ② `world_projection` 的手柄抽稀间距（QA AC22：smooth zoom 每帧微动不能打穿缓存）。
///
/// `round(log2(scale) × 2)`：档宽 ×1.414。为什么是 1/2 octave 而不是 1 octave：
/// 1 octave（×2）在 1.6px 线宽上会让屏上宽度在 0.8–1.6px 之间跳（肉眼可见
/// 粗细跳变）；1/2 octave 的 ±19% 读不出来，而重建次数只翻倍。
pub(crate) fn scale_bucket(scale: f32) -> i16 {
    (scale.max(1e-6).log2() * 2.0).round() as i16
}

/// 档号 → 档中心缩放（几何按它烘焙线宽）。
pub(crate) fn scale_of_bucket(bucket: i16) -> f32 {
    (f32::from(bucket) / 2.0).exp2()
}

/// 一个底色覆盖块。
struct FillBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    fingerprint: u64,
    color: [u8; 3],
    region_id: Id,
}

impl RegionOutlinePool {
    /// 清空所有实体**并回收资产**（图层关闭 / 换图时用）。
    ///
    /// 2026-10-23（QA AC19/AC20 复签）：旧实现只 `despawn` 实体，mesh 句柄一丢，
    /// 资产就成无人引用的死条目常驻到进程结束——「开 → 关」每轮泄漏一份全量合批
    /// mesh（200 区时不小），材质同理。现在实体与资产一起回收，写法照
    /// `grid_lines.rs::GridLinesOverlay::despawn`（上一轮修过的同款泄漏）。
    ///
    /// 为什么用 `commands.queue` 而不是 `ResMut<Assets<..>>`：本函数被
    /// `sync_region_outlines` 调用，那里已经有 `Commands`；多要两个 `Assets<>`
    /// 参数会把签名顶到 Bevy 的参数上限，且这里的回收**不需要**与实体 despawn
    /// 同一 tick 生效（句柄已 drop，晚一帧回收也不影响画面）。
    fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(b) = self.outline.take() {
            commands.entity(b.entity).despawn();
            reclaim_mesh(commands, b.mesh);
        }
        if let Some(b) = self.preview.take() {
            commands.entity(b.entity).despawn();
            reclaim_mesh(commands, b.mesh);
        }
        for f in self.fills.drain(..) {
            commands.entity(f.entity).despawn();
            reclaim_mesh(commands, f.mesh);
        }
        // 材质缓存同样回收（否则「开关 N 次」= N 份死材质）。重建代价是一次 `add`。
        if let Some(m) = self.outline_material.take() {
            reclaim_material(commands, m);
        }
        if let Some(m) = self.preview_material.take() {
            reclaim_material(commands, m);
        }
        for (_, m) in self.fill_materials.drain(..) {
            reclaim_material(commands, m);
        }
        // 线段的**源**缓存故意保留：它是「重新打开时不必重算几何」的依据，
        // 与资产无关（REQ-4 只要求资产不累积）。
    }
}

/// 回收一个 mesh 资产（调用前其实体必须已 despawn）。
fn reclaim_mesh(commands: &mut Commands, mesh: Handle<Mesh>) {
    commands.queue(move |world: &mut World| {
        if let Some(mut assets) = world.get_resource_mut::<Assets<Mesh>>() {
            assets.remove(&mesh);
        }
    });
}

/// 回收一个 `ColorMaterial` 资产。
fn reclaim_material(commands: &mut Commands, material: Handle<ColorMaterial>) {
    commands.queue(move |world: &mut World| {
        if let Some(mut assets) = world.get_resource_mut::<Assets<ColorMaterial>>() {
            assets.remove(&material);
        }
    });
}

/// 区域多边形 → **视空间**线段（外环闭合 + 每个洞环闭合）。
///
/// 坐标链：区域多边形是**图内局部 wu** → `GridConfig::map_to_view` 转视空间
/// （与 POI / 街道 / 标注同一条链）。
pub fn region_segments(region: &Region, grid: &GridConfig) -> Vec<(Vec2, Vec2)> {
    let mut segs = Vec::new();
    push_loop(&region.polygon, grid, &mut segs);
    for hole in &region.holes {
        push_loop(hole, grid, &mut segs);
    }
    segs
}

/// 一条闭合环 → 线段列。
fn push_loop(ring: &[f32], grid: &GridConfig, out: &mut Vec<(Vec2, Vec2)>) {
    let pts: Vec<Vec2> = ring
        .chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| grid.map_to_view(Vec2::new(p[0], p[1])))
        .collect();
    if pts.len() < 2 {
        return;
    }
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        if a != b {
            out.push((a, b));
        }
    }
}

/// 区域多边形 → **底色覆盖 mesh**（外环 − 洞，三角化）。
///
/// 顶点用**视空间坐标**（mesh 直接摆在原点，实体只带 Z）——与线框同坐标系，
/// 避免「mesh 局部坐标 + 实体位移」两套换算出错。
pub fn region_fill_mesh(region: &Region, grid: &GridConfig) -> Option<Mesh> {
    let outer: Vec<f32> = region
        .polygon
        .chunks(2)
        .filter(|p| p.len() == 2)
        .flat_map(|p| {
            let v = grid.map_to_view(Vec2::new(p[0], p[1]));
            [v.x, v.y]
        })
        .collect();
    if outer.len() < 6 {
        return None;
    }
    let holes: Vec<Vec<f32>> = region
        .holes
        .iter()
        .map(|h: &Vec<f32>| {
            h.chunks(2)
                .filter(|p| p.len() == 2)
                .flat_map(|p| {
                    let v = grid.map_to_view(Vec2::new(p[0], p[1]));
                    [v.x, v.y]
                })
                .collect()
        })
        .filter(|h: &Vec<f32>| h.len() >= 6)
        .collect();
    let hole_refs: Vec<&[f32]> = holes.iter().map(|h| h.as_slice()).collect();
    let tris = earcut_with_holes(&outer, &hole_refs);
    if tris.is_empty() {
        return None;
    }
    let positions: Vec<[f32; 3]> = tris.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    Some(mesh)
}

/// 区域轮廓 + 底色覆盖系统。
///
/// **图层可见性**：跟随 `terrain` 层（区域是底图族的地理事实）。
#[allow(clippy::too_many_arguments)]
pub fn sync_region_outlines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    index: Res<RegionOverlayIndex>,
    input: Res<crate::region_outline::CandidateRegionInput>,
    mut pool: ResMut<RegionOutlinePool>,
) {
    // 2026-10-22 内容矩阵：区域层从 TerrainDecor 拆到独立 pass——原 Schematic（已退役，ADR-0069）
    // 下装饰 ✓ 而区域 ✗，共享一格做不到（ADR-0051）。
    // CellMap（Topographic）：nature_overlay 放行 Nature 类（判定入口唯一）。
    let enabled = crate::cellmap_bridge::region_layer_visible(
        view.mode,
        view.nature_overlay,
        wv_core::model::region::RegionKind::Nature,
        recipe.enabled(PassId::RegionLayers),
        layers.terrain.visible,
    );
    let Some(map_id) = drill.current().cloned() else {
        pool.despawn_all(&mut commands);
        return;
    };
    if !enabled {
        pool.despawn_all(&mut commands);
        return;
    }
    let in_cellmap = matches!(view.mode, crate::state::ViewMode::Topographic);
    let regions: Vec<&Region> = projection
        .world
        .regions
        .iter()
        .filter(|r| r.map == map_id && !r.polygon.is_empty())
        // CellMap 只显示 Nature 类（Area 属内容，永远藏）
        .filter(|r| !in_cellmap || matches!(r.kind, wv_core::model::region::RegionKind::Nature))
        .collect();
    if regions.is_empty() {
        pool.despawn_all(&mut commands);
        return;
    }

    // ---- 轮廓线框（所有区域合批）----
    // ① 源缓存：区域集合 + 多边形 + 网格换算不变 ⇒ `segs` 与指纹都不重算。
    //    每帧仍要做一次「裸位模式」的 geom 指纹（无分配、无坐标变换），
    //    量级是 O(总边数) 的整数 xor/mul（200 区域 × 40 点 ≈ 8k 次 ≈ 10µs 级）。
    let in_cellmap = matches!(view.mode, crate::state::ViewMode::Topographic);
    let source_key = SegSourceKey {
        grid_bits: grid_bits(&grid),
        in_cellmap,
        region_count: regions.len(),
        geom: polygon_set_fingerprint(&regions),
    };
    let cached = pool.source.as_ref().is_some_and(|s| s.key == source_key);
    if !cached {
        let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
        for r in &regions {
            segs.extend(region_segments(r, &grid));
        }
        let fingerprint = segments_fingerprint(&segs);
        pool.source = Some(OutlineSource {
            key: source_key,
            segs,
            fingerprint,
        });
        pool.seg_recomputes += 1;
    }
    // 取出来用（`pool.outline` 下面还要改，借 `pool.source` 会冲突）。
    let source = pool.source.take();
    let (fp, segs_ref) = source
        .as_ref()
        .map(|s| (s.fingerprint, s.segs.as_slice()))
        .unwrap_or((0, &[][..]));
    let bucket = scale_bucket(view.scale);
    let world_thickness = OUTLINE_STROKE_PX * scale_of_bucket(bucket);
    let key = OutlineKey {
        fingerprint: fp,
        thickness_bucket: bucket,
        z: REGION_OUTLINE_Z,
    };
    // 2026-10-22 验收：CellMap（Topographic + nature overlay）下轮廓=白色
    // （数据视图不随主题）；其他模式沿用深蓝。材质按模式重建。
    let outline_color = if in_cellmap {
        Color::srgba(1.0, 1.0, 1.0, 0.9)
    } else {
        Color::srgba(0.21, 0.35, 0.48, 0.85)
    };
    let color_changed = pool
        .outline_material
        .as_ref()
        .and_then(|m| materials.get(m))
        .map(|mm| mm.color != outline_color)
        .unwrap_or(true);
    if pool.outline_material.is_none() || color_changed {
        pool.outline_material = Some(materials.add(ColorMaterial::from_color(outline_color)));
    }
    let outline_mat = pool.outline_material.clone().expect("刚建");
    match pool.outline.take() {
        None => {
            if !segs_ref.is_empty() {
                let mesh = meshes.add(build_stroke_batch_mesh(segs_ref, world_thickness));
                pool.stroke_rebuilds += 1;
                let entity = commands
                    .spawn((
                        Mesh2d(mesh.clone()),
                        MeshMaterial2d(outline_mat),
                        Transform::from_xyz(0.0, 0.0, REGION_OUTLINE_Z),
                    ))
                    .id();
                pool.outline = Some(OutlineBatch { entity, mesh, key });
            }
        }
        Some(mut b) => {
            if b.key != key {
                let fresh = build_stroke_batch_mesh(segs_ref, world_thickness);
                pool.stroke_rebuilds += 1;
                pool.outline_rebuilds += 1;
                if let Some(mut old) = meshes.get_mut(&b.mesh) {
                    *old = fresh;
                }
                b.key = key;
            }
            pool.outline = Some(b);
        }
    }
    pool.source = source;

    // ---- 底色覆盖（每个有规则色的区域一块）----
    let wanted: Vec<(&Region, [u8; 3])> = regions
        .iter()
        .filter_map(|r| {
            let entry = crate::region_overlay::entry_of_region(&index, &r.id)?;
            let rule = entry.rule.as_ref()?;
            Some((*r, rule.color))
        })
        .collect();
    // 旧的覆盖块先回收，多余的再补（按 region id + 颜色比对）。
    let mut keep: Vec<FillBatch> = Vec::new();
    for f in pool.fills.drain(..) {
        if wanted
            .iter()
            .any(|(r, c)| r.id == f.region_id && *c == f.color)
        {
            keep.push(f);
        } else {
            commands.entity(f.entity).despawn();
        }
    }
    pool.fills = keep;
    // 计数先攒在局部：循环里 `pool.fills.iter_mut()` 已独占借用 `pool`。
    let mut fill_calls = 0u64;
    for (r, color) in &wanted {
        let fp = region_fill_fingerprint(r, color);
        if let Some(existing) = pool
            .fills
            .iter_mut()
            .find(|f| f.region_id == r.id && f.color == *color)
        {
            if existing.fingerprint != fp {
                fill_calls += 1;
                if let Some(mesh) = region_fill_mesh(r, &grid) {
                    if let Some(mut old) = meshes.get_mut(&existing.mesh) {
                        *old = mesh;
                    }
                }
                existing.fingerprint = fp;
            }
            continue;
        }
        fill_calls += 1;
        let Some(mesh) = region_fill_mesh(r, &grid) else {
            continue;
        };
        let handle = meshes.add(mesh);
        let mat = fill_material(&mut pool.fill_materials, &mut materials, *color);
        let entity = commands
            .spawn((
                Mesh2d(handle.clone()),
                MeshMaterial2d(mat),
                Transform::from_xyz(0.0, 0.0, REGION_FILL_Z),
            ))
            .id();
        pool.fills.push(FillBatch {
            entity,
            mesh: handle,
            fingerprint: fp,
            color: *color,
            region_id: r.id.clone(),
        });
    }
    pool.fill_rasterizes += fill_calls;

    // ---- 候选区预览（物化前光标所在的连通区）----
    sync_candidate_preview(
        commands,
        meshes,
        materials,
        view,
        input.ring.as_deref(),
        &mut pool,
    );
}

/// 区域多边形集合的内容指纹（外环 + 洞环的裸 `f32::to_bits`，**不分配**）。
///
/// 与 [`segments_fingerprint`] 的区别：这里读的是**图内局部 wu 原始顶点**，
/// 不做 `map_to_view`、不建 `Vec`。所以它能当「要不要重算 `segs`」的判据，
/// 代价比展开低一个数量级（无分配、无浮点乘加）。
fn polygon_set_fingerprint(regions: &[&Region]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for r in regions {
        h ^= r.polygon.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        for v in r.polygon.iter().map(|f| f.to_bits()) {
            h ^= u64::from(v);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= r.holes.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        for hole in &r.holes {
            h ^= hole.len() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            for v in hole.iter().map(|f| f.to_bits()) {
                h ^= u64::from(v);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

/// 候选区预览：光标所在 biome 连通区的**金色高亮轮廓**。
///
/// 物化前先看到「要圈的是哪块地」（ADR-0045 D4 的可视化面）。轮廓由调用方
/// 传入（渲染层不做连通区计算——那是 `wv_terrain::region::biome_region_at`
/// 的事，app 层按光标格查一次）。
///
/// 2026-10-23：环内容不变（同一候选区内移动光标）⇒ `segs` / 指纹 / mesh
/// **全部不重算**；只有环真的换了才付装配钱。
fn sync_candidate_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    preview_ring: Option<&[Vec2]>,
    pool: &mut RegionOutlinePool,
) {
    let Some(ring) = preview_ring else {
        if let Some(b) = pool.preview.take() {
            commands.entity(b.entity).despawn();
            reclaim_mesh(&mut commands, b.mesh);
        }
        pool.preview_ring = None;
        return;
    };
    if pool.preview_ring.as_deref() != Some(ring) {
        pool.preview_recomputes += 1;
        pool.preview_segs.clear();
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if a != b {
                pool.preview_segs.push((a, b));
            }
        }
        pool.preview_fp = segments_fingerprint(&pool.preview_segs);
        pool.preview_ring = Some(ring.to_vec());
    }
    let fp = pool.preview_fp;
    let bucket = scale_bucket(view.scale);
    let thickness = CANDIDATE_STROKE_PX * scale_of_bucket(bucket);
    let key = OutlineKey {
        fingerprint: fp,
        thickness_bucket: bucket,
        z: CANDIDATE_Z,
    };
    if pool.preview_material.is_none() {
        pool.preview_material = Some(materials.add(ColorMaterial::from_color(Color::srgba(
            0.96, 0.78, 0.26, 0.9,
        ))));
    }
    let mat = pool.preview_material.clone().expect("刚建");
    match pool.preview.take() {
        None => {
            if !pool.preview_segs.is_empty() {
                let mesh = meshes.add(build_stroke_batch_mesh(&pool.preview_segs, thickness));
                pool.stroke_rebuilds += 1;
                pool.preview_stroke_builds += 1;
                let entity = commands
                    .spawn((
                        Mesh2d(mesh.clone()),
                        MeshMaterial2d(mat),
                        Transform::from_xyz(0.0, 0.0, CANDIDATE_Z),
                    ))
                    .id();
                pool.preview = Some(OutlineBatch { entity, mesh, key });
            }
        }
        Some(mut b) => {
            if b.key != key {
                let fresh = build_stroke_batch_mesh(&pool.preview_segs, thickness);
                pool.stroke_rebuilds += 1;
                pool.preview_stroke_builds += 1;
                if let Some(mut old) = meshes.get_mut(&b.mesh) {
                    *old = fresh;
                }
                b.key = key;
            }
            pool.preview = Some(b);
        }
    }
}

/// 候选区预览线宽（屏幕像素）——比已物化的轮廓粗一点、更醒目。
const CANDIDATE_STROKE_PX: f32 = 2.4;
/// 候选区预览 Z：与已物化轮廓同带（0.076），金色已足够区分「还没物化」。
const CANDIDATE_Z: f32 = 0.077;

/// 底色覆盖块的指纹（几何 + 颜色）。
fn region_fill_fingerprint(r: &Region, color: &[u8; 3]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in r.polygon.iter().map(|f| f.to_bits()) {
        h ^= u64::from(v);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for hole in &r.holes {
        for v in hole.iter().map(|f| f.to_bits()) {
            h ^= u64::from(v);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    for v in color {
        h ^= u64::from(*v);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 线段列指纹（与道路同款 FNV）。
fn segments_fingerprint(segments: &[(Vec2, Vec2)]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (a, b) in segments {
        for v in [a.x, a.y, b.x, b.y] {
            h ^= u64::from(v.to_bits());
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 底色覆盖材质（按色缓存——同色的区域共用一个材质句柄）。
fn fill_material(
    cache: &mut Vec<([u8; 3], Handle<ColorMaterial>)>,
    materials: &mut Assets<ColorMaterial>,
    color: [u8; 3],
) -> Handle<ColorMaterial> {
    if let Some((_, h)) = cache.iter().find(|(c, _)| *c == color) {
        return h.clone();
    }
    let handle = materials.add(ColorMaterial::from_color(Color::srgba(
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
        FILL_ALPHA,
    )));
    cache.push((color, handle.clone()));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::model::region::RegionKind;

    fn region_with(polygon: Vec<f32>, holes: Vec<Vec<f32>>) -> Region {
        Region {
            id: Id::new_unchecked("nr_a"),
            name: "甲".into(),
            map: Id::new_unchecked("map_world"),
            polygon,
            holes,
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        }
    }

    /// 无位移的栅格配置（map_origin = 0）——视空间 = 局部 wu。
    fn grid() -> GridConfig {
        GridConfig {
            cell_size: 8.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        }
    }

    #[test]
    fn region_segments_close_the_outer_loop() {
        let r = region_with(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0], vec![]);
        let segs = region_segments(&r, &grid());
        assert_eq!(segs.len(), 4, "方形外环应出 4 段");
        // 闭合：每段的终点 = 下一段的起点。
        for i in 0..segs.len() {
            let end = segs[i].1;
            let next = segs[(i + 1) % segs.len()].0;
            assert_eq!(end, next, "第 {i} 段未闭合");
        }
    }

    #[test]
    fn region_segments_include_hole_loops() {
        let r = region_with(
            vec![0.0, 0.0, 6.0, 0.0, 6.0, 6.0, 0.0, 6.0],
            vec![vec![1.0, 1.0, 3.0, 1.0, 3.0, 3.0, 1.0, 3.0]],
        );
        let segs = region_segments(&r, &grid());
        assert_eq!(segs.len(), 8, "外环 4 段 + 洞环 4 段");
    }

    #[test]
    fn region_fill_mesh_area_excludes_the_hole() {
        let r = region_with(
            vec![0.0, 0.0, 6.0, 0.0, 6.0, 6.0, 0.0, 6.0],
            vec![vec![1.0, 1.0, 3.0, 1.0, 3.0, 3.0, 1.0, 3.0]],
        );
        let mesh = region_fill_mesh(&r, &grid()).expect("应能三角化");
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("位置属性");
        let area = triangle_signed_area(pos);
        // 外环 6×6 = 36，洞 2×2 = 4 → 净面积 32。
        assert!(
            (area - 32.0).abs() < 0.01,
            "净面积应为 36−4=32，实得 {area}"
        );
    }

    #[test]
    fn region_fill_mesh_without_hole_covers_the_whole_polygon() {
        let r = region_with(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0], vec![]);
        let mesh = region_fill_mesh(&r, &grid()).expect("应能三角化");
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("位置属性");
        let area = triangle_signed_area(pos);
        assert!((area - 16.0).abs() < 0.01, "面积应为 16，实得 {area}");
    }

    #[test]
    fn degenerate_polygon_yields_no_fill_mesh() {
        let r = region_with(vec![0.0, 0.0, 1.0, 0.0], vec![]);
        assert!(
            region_fill_mesh(&r, &grid()).is_none(),
            "退化多边形不该出面"
        );
    }

    #[test]
    fn fill_fingerprint_changes_with_color_and_geometry() {
        let r = region_with(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0], vec![]);
        let base = region_fill_fingerprint(&r, &[1, 2, 3]);
        assert_ne!(
            base,
            region_fill_fingerprint(&r, &[9, 9, 9]),
            "换色应改指纹"
        );
        let moved = region_with(vec![0.0, 0.0, 5.0, 0.0, 5.0, 4.0], vec![]);
        assert_ne!(
            base,
            region_fill_fingerprint(&moved, &[1, 2, 3]),
            "改几何应改指纹"
        );
    }

    /// 三角网**有符号**面积（按三角形绕向求和）。
    ///
    /// 用有符号而非绝对值：洞桥接会产生**绕向相反**的零宽走廊三角形，
    /// 绝对值求和会把它们的面积也算进去（实测 32 → 44）。有符号求和是
    /// 三角网覆盖面积的正确度量。
    fn triangle_signed_area(pos: &bevy::mesh::VertexAttributeValues) -> f32 {
        let bevy::mesh::VertexAttributeValues::Float32x3(v) = pos else {
            panic!("应为 Float32x3");
        };
        let mut area = 0.0f32;
        for tri in v.chunks(3) {
            let (a, b, c) = (
                (tri[0][0], tri[0][1]),
                (tri[1][0], tri[1][1]),
                (tri[2][0], tri[2][1]),
            );
            area += ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)) / 2.0;
        }
        area.abs()
    }
}

/// 稳态零重算（2026-10-23 · 用户症状「群系视图 26 FPS」）。
///
/// 口径：**只数计数器，不测墙钟**——`seg_recomputes` / `stroke_rebuilds` 跨机器同值。
#[cfg(test)]
mod steady_state_tests {
    use super::*;
    use crate::layer_frame::LayerBinding;
    use crate::state::{DrillState, ViewMode};
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::model::region::RegionKind;

    fn map_id() -> Id {
        Id::new_unchecked("map_world")
    }

    fn world_with_regions(n: usize) -> WorldProjection {
        let mut w = WorldProjection::default();
        w.world.maps.push(MapNode {
            id: map_id(),
            name: "世界".into(),
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
            trashed: false,
            trashed_at: None,
            comment: None,
        });
        for i in 0..n {
            let x = (i % 16) as f32 * 200.0;
            let y = (i / 16) as f32 * 200.0;
            w.world.regions.push(Region {
                id: Id::new_unchecked(format!("nr_{i}")),
                name: format!("区{i}"),
                map: map_id(),
                // 8 顶点方形（外环 8 段）。
                polygon: vec![x, y, x + 100.0, y, x + 100.0, y + 100.0, x, y + 100.0],
                holes: Vec::new(),
                kind: RegionKind::Nature,
                parent: None,
                overlay: None,
            });
        }
        w
    }

    fn drill() -> DrillState {
        let mut d = DrillState::default();
        d.settle_on(map_id());
        d
    }

    fn app(n: usize, nature_overlay: bool, scale: f32) -> App {
        let mut app = App::new();
        app.insert_resource(Assets::<Mesh>::default())
            .insert_resource(Assets::<ColorMaterial>::default())
            .insert_resource(world_with_regions(n))
            .insert_resource(drill())
            .insert_resource(GridConfig {
                cell_size: 4.0,
                raster_origin: Vec2::ZERO,
                map_origin: Vec2::ZERO,
                map_size: Vec2::splat(4096.0),
            })
            .insert_resource(ViewState {
                mode: ViewMode::Topographic,
                nature_overlay,
                scale,
                current_map: "map_world".into(),
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
            .init_resource::<RenderRecipe>()
            .init_resource::<RegionOverlayIndex>()
            .init_resource::<CandidateRegionInput>()
            .init_resource::<RegionOutlinePool>()
            .add_systems(Update, sync_region_outlines);
        app
    }

    fn counts(app: &App) -> (u64, u64) {
        let p = app.world().resource::<RegionOutlinePool>();
        (p.seg_recomputes, p.stroke_rebuilds)
    }

    /// 断言 1（轮廓侧）：鼠标在地图上移动 60 帧（选择 / 多边形 / 网格都不变）
    /// ⇒ `segs` 重算 == 0、`build_stroke_batch_mesh` == 0。
    #[test]
    fn mouse_move_over_map_recomputes_nothing() {
        let mut app = app(10, true, 1.0);
        app.update(); // 首帧建一次
        let (seg0, stroke0) = counts(&app);
        assert_eq!(seg0, 1, "首帧应建一次线段集合（实得 {seg0}）");
        assert!(stroke0 >= 1, "首帧应建一次 mesh");
        for i in 0..60 {
            app.world_mut().resource_mut::<ViewState>().cursor_world =
                Some(Vec2::new(i as f32 * 3.0, 17.0));
            app.update();
        }
        let (seg, stroke) = counts(&app);
        assert_eq!(seg, seg0, "鼠标移动不该重算 segs（多了 {}）", seg - seg0);
        assert_eq!(stroke, stroke0, "鼠标移动不该重建 stroke mesh");
    }

    /// 断言 3：区域数 10 与 200 两档，稳态每帧 `segs` 重算 == 0。
    #[test]
    fn steady_state_recomputes_zero_at_ten_and_two_hundred_regions() {
        for n in [10usize, 200] {
            let mut app = app(n, true, 1.0);
            app.update();
            let (seg0, _) = counts(&app);
            assert_eq!(seg0, 1, "{n} 区域：首帧应只建一次");
            for _ in 0..30 {
                app.update();
            }
            let (seg, _) = counts(&app);
            assert_eq!(seg, 1, "{n} 区域：稳态 30 帧不该再重算（实得 {seg}）");
        }
    }

    /// 缩放（同档内）：1/2 octave 档内连续几档不该重建几何。
    #[test]
    fn zoom_inside_one_thickness_bucket_does_not_rebuild() {
        let mut app = app(10, true, 1.0);
        app.update();
        let (seg0, stroke0) = counts(&app);
        for scale in [1.02, 1.08, 1.15] {
            app.world_mut().resource_mut::<ViewState>().scale = scale;
            app.update();
        }
        let (seg, stroke) = counts(&app);
        assert_eq!(seg, seg0, "同档缩放不该重算 segs");
        assert_eq!(stroke, stroke0, "同档缩放不该重建 mesh");
    }

    /// **阳性对照**：跨档缩放必须重建一次——证明上面的「0」不是恒 0 的假绿。
    #[test]
    fn crossing_a_thickness_bucket_rebuilds_once() {
        let mut app = app(10, true, 1.0);
        app.update();
        let (_, stroke0) = counts(&app);
        app.world_mut().resource_mut::<ViewState>().scale = 2.0; // 档 0 → 档 2
        app.update();
        let (seg, stroke) = counts(&app);
        assert_eq!(seg, 1, "线宽档变化不该重算 segs（几何与线宽解耦）");
        assert_eq!(stroke, stroke0 + 1, "线宽档变化应重建 mesh 一次");
    }

    /// 1/2 octave 档号（写死期望值，不引用实现常量）。
    #[test]
    fn scale_bucket_is_half_octave() {
        assert_eq!(scale_bucket(1.0), 0);
        assert_eq!(scale_bucket(1.18), 0, "1.18 < 2^0.25 仍在档 0");
        assert_eq!(scale_bucket(1.42), 1, "1.42 ≈ 2^0.5 → 档 1");
        assert_eq!(scale_bucket(2.0), 2);
        assert_eq!(scale_bucket(0.5), -2);
        // 相邻档的几何线宽比 = ×1.414（±19% 的屏上宽度误差）。
        let ratio = scale_of_bucket(1) / scale_of_bucket(0);
        assert!(
            (ratio - std::f32::consts::SQRT_2).abs() < 1e-5,
            "实得 {ratio}"
        );
    }

    /// 关掉群系视图（关图层）时，轮廓与预览实体都要走 despawn 分支。
    #[test]
    fn overlay_off_despawns_everything() {
        let mut app = app(10, false, 1.0);
        app.update();
        let mut q = app.world_mut().query::<&Mesh2d>();
        assert_eq!(q.iter(app.world()).count(), 0, "关群系视图后不该有轮廓实体");
    }
}
