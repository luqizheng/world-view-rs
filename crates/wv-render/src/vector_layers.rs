//! Google 式矢量叠加层：道路 casing/fill + 红点白环 POI。
//!
//! ## 职责
//! - 在地形之上叠画 Google Maps 风格的视觉层：道路外壳（casing）+ 道路主体（fill），
//!   以及 POI 的红点白环 marker。
//! - 用 `PlaceDot` 组件驱动可见性 / LOD：在当前 zoom 下超界的点会随缩放增删，
//!   避免 `wgpu::Batch` 节点数随 POI 总数失控。
//!
//! ## 边界（不做什么）
//! - **不画地形底图**——那是 [`crate::terrain_view`] + [`crate::theme::palette`]。
//! - **不投影地理坐标**——只消费 [`crate::world_projection::ProjectionFrame`]。
//! - **不维护世界数据**——POI 经 [`crate::state::ViewState`] 当前地图与可见层过滤。
//!
//! ## 主要导出
//! - `VectorLayerEntities` — Resource，记录当前帧的 casing / fill / dot 实体。
//! - `setup_place_dots` / `render_place_dots` — POI 红点系统。
//! - `draw_streets_casing` / `draw_streets_fill` — 道路外壳 / 道路主体绘制系统。
//!
//! ## 资产纪律
//!
//! 本系统每帧运行，但 **Mesh / Material 资产不能每帧新增**：
//! - Material 按视图模式缓存，模式切换才重建；
//! - Mesh 只在几何规格变化时重建，旧 Mesh 用延迟命令在实体换绑后移除；
//! - Transform 仍每帧对齐投影，保证视图拉动式刷新。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{lod_tier, GridConfig, ViewState};
use crate::theme::colors_google::{
    GOOGLE_PLACE_DOT, GOOGLE_POI_ARENA, GOOGLE_POI_DANGER, GOOGLE_POI_EDUCATION, GOOGLE_POI_FOOD,
    GOOGLE_POI_GOVERNMENT, GOOGLE_POI_LODGING, GOOGLE_POI_MEDICAL, GOOGLE_POI_PORTAL,
    GOOGLE_POI_RING, GOOGLE_POI_RUINS, GOOGLE_POI_SCENIC, GOOGLE_POI_SHOP, GOOGLE_POI_SHRINE,
    GOOGLE_POI_TRANSIT, GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL, GOOGLE_ROAD_GRADES,
};
use crate::theme::palette::{self, MapPalette, PaletteColors};
use crate::world_projection::{
    river_taper_factors, segments_fingerprint, PlaceDot, ProjectionFrame,
};
use wv_core::distance::PoiKind;

/// Google 式道路外缘宽度（屏幕像素）——**非分级主题**（古舆图等）的兜底值。
/// Google 模式按 [`GOOGLE_ROAD_GRADES`] 三档取宽，不再用这两个常量。
const GOOGLE_ROAD_CASING_PX: f32 = 6.0;
/// Google 式道路主体宽度（屏幕像素）——同上，仅兜底用。
const GOOGLE_ROAD_FILL_PX: f32 = 3.5;

/// 道路分级档数：与 `PlaceEdge::effective_rank` 的 1/2/3 三档一一对应。
///
/// 索引 0 = 最高等级（`rank ≥ 3`，Nation 档起可见），2 = 最低（仅 Edit 档）。
/// 段列已按 rank 降序排列，故三档在切片内是**连续区间**——渲染侧只需三个
/// 前缀切点，零额外存储（ADR-0047 D4 的 `road_rank_cuts` 正好就是这三个点）。
const ROAD_GRADES: usize = 3;

/// 第 `grade` 档的 (casing, fill) 屏幕像素宽度。
fn road_grade_px(mode: crate::state::ViewMode, grade: usize) -> (f32, f32) {
    match mode {
        crate::state::ViewMode::GoogleMap => {
            let g = GOOGLE_ROAD_GRADES[grade.min(ROAD_GRADES - 1)];
            (g.casing_px, g.fill_px)
        }
        _ => (GOOGLE_ROAD_CASING_PX, GOOGLE_ROAD_FILL_PX),
    }
}

/// 当前档位视野内，三档道路在段切片中的 `[start, end)` 区间。
///
/// `cuts` 是 [`ProjectionFrame::road_rank_cuts`]（下标序 Edit/Region/Nation，
/// 值是各档的可见前缀长度）。因为段列按 rank 降序，`rank ≥ 3` 一定落在
/// `[0, cuts[2])`、`rank = 2` 落在 `[cuts[2], cuts[1])`、`rank = 1` 落在
/// `[cuts[1], cuts[0])`。Nation 档前缀只有 `cuts[2]`，故后两档天然为空。
fn road_grade_spans(
    tier: crate::state::LodTier,
    cuts: [usize; 3],
) -> [(usize, usize); ROAD_GRADES] {
    let len = match tier {
        crate::state::LodTier::Edit => cuts[0],
        crate::state::LodTier::Region => cuts[1],
        crate::state::LodTier::Nation => cuts[2],
    };
    let hi = cuts[2].min(len);
    let mid = cuts[1].min(len);
    [(0, hi), (hi, mid), (mid, len)]
}
/// POI 点标在屏幕上的最小可见直径（像素）。
const POI_MIN_SCREEN_PX: f32 = 5.0;

/// 道路 casing 在 Z 轴上的偏移：在 fill 之下、地形之上。
const ROAD_CASING_Z: f32 = 0.09;
/// 道路 fill 在 Z 轴上的偏移。
const ROAD_Z: f32 = 0.1;

/// 河流折线 Z（ADR-0037 D5）：地形装饰（0.075）之上、道路 casing（0.09）之下
/// ——河是底图水系，从地形上长出来，但让路桥压在河上。
const RIVER_Z: f32 = 0.082;
/// 河流描线宽（屏幕像素）：比道路 fill（3.5px）细一档，参考图的细蜿蜒蓝线。
/// 仅作**未声明 `line_width_m` 的河线**的缺省宽（ADR-0070 T8：声明了宽度
/// 的河按 `width_wu` 世界宽描画，不再随屏幕缩放）。
const RIVER_STROKE_PX: f32 = 2.2;
/// 街道主干档（rank ≤ 2）宽度（屏幕像素）。
const STREET_MAJOR_PX: f32 = 4.5;
/// 街道次干 / 支路档宽度（屏幕像素）。
const STREET_MINOR_PX: f32 = 2.75;
/// 街道 casing 相对 fill 的外扩量（屏幕像素，两侧各半）。
/// 与道路（casing 6.0 / fill 3.5）同一套「粗细差即路缘」的口径。
const STREET_CASING_EXTRA_PX: f32 = 2.5;
/// 街道矢量层 Z 偏移：压在 `distance.edges` 行程连线（ROAD_Z=0.1）之上、POI 之下。
const STREET_MAJOR_Z: f32 = 0.11;
const STREET_MINOR_Z: f32 = 0.115;
/// 街道 casing 的 Z：紧贴在本档 fill 之下、上一档 fill 之上。
///
/// 街道必须画 casing：地图（GoogleMap）模式下 fill 是**白色**，而底图土地是
/// `#f8f9fa`——只画白色 fill 等于白线落在近白底图上，肉眼不可见（用户实机反馈
/// 「钻进武昌城看不到街道」的一半原因就是它）。casing 是浅灰 `#dadce0`，
/// 靠它与 fill 的宽度差勾出可见路缘。
const STREET_MAJOR_CASING_Z: f32 = 0.108;
const STREET_MINOR_CASING_Z: f32 = 0.113;

/// Z 序是**编译期**约束：底图 0.0 < 道路 fill < 主干 casing < 主干 fill < 支路 casing
/// < 支路 fill。破了它就会出现「casing 被底图盖掉」或「casing 反压在 fill 上」，
/// 两种表现都是街道在地图模式下不可见——正是这条约束要防的那个 bug。
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(
    ROAD_Z < STREET_MAJOR_CASING_Z
        && STREET_MAJOR_CASING_Z < STREET_MAJOR_Z
        && STREET_MAJOR_Z < STREET_MINOR_CASING_Z
        && STREET_MINOR_CASING_Z < STREET_MINOR_Z,
    "矢量层 Z 序被破坏：道路 fill < 主干 casing < 主干 fill < 支路 casing < 支路 fill"
);
/// 橡皮筋预览线：比街道细、压在最上（仍低于 POI 0.21）。
const STREET_PREVIEW_PX: f32 = 1.75;
const STREET_PREVIEW_Z: f32 = 0.12;
/// POI 外环 / 内芯 Z 偏移。
const POI_OUTER_Z: f32 = 0.21;
const POI_INNER_Z: f32 = 0.22;

/// 合批 stroke mesh 的重建判据（A2）：内容指纹 + 世界线宽 + Z，任一变化才换 mesh。
///
/// `world_thickness` 是「几何赖以烘成的世界尺寸参数」：道路 = 线宽，
/// 地物符号 = 缩放因子 `s`（2026-10-24 教训：符号 key 漏掉 `s`，缩放后
/// mesh 不重建，pin 保持旧世界尺寸随相机放大成巨型墨团）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct StrokeBatchKey {
    fingerprint: u64,
    world_thickness: f32,
    z: f32,
}

/// 把整组段合成**一个** Mesh：每段一个世界空间矩形（4 顶点 / 6 索引）。
///
/// A2（2026-10-18）：旧实现每段一个实体 + 一个 `Rectangle` mesh，规格含
/// center 与随缩放的线宽——guangdong 若有 87k 段，相机平移/缩放的每一帧
/// 都会全量 add+remove 87k 个 mesh 资产。合批后平移（纯相机运动，段坐标
/// 不变）零重建；缩放 / 编辑整组换一个 mesh。z 由实体 Transform 承担，
/// 顶点保持在 z=0 平面。
///
/// 线宽口径沿旧实现：屏幕像素 → 世界单位 `world = px * view_scale`
/// （`view.scale` 是世界单位/像素，乘而不是除——历史 bug 见 git blame）。
pub(crate) fn build_stroke_batch_mesh(segments: &[(Vec2, Vec2)], world_thickness: f32) -> Mesh {
    let n = segments.len();
    let half = world_thickness * 0.5;

    // ---- 接头（2026-10-23 修 G3b）：逐段四边形 + butt 端会在交汇处留楔形缺口 ----
    // 两段在一处相接时，外角必然缺一块（角度越尖缺得越大），且线宽是**屏幕常量**，
    // 缺口在任何缩放级别都一样明显。补法：按端点度数统计，度数 ≥2 的位置补一块
    // 半径 = 半线宽的小圆盘（内接 6 边形）。内角的重叠本来无害，外角的缺口被填掉。
    // 成本：只在 mesh 重建时做一次（内容/线宽/Z 变化才重建，见 `sync_road_strokes`
    // 的 `StrokeBatchKey`），稳态零开销；顶点增量 = 接头数 × 7。
    let joins = stroke_join_centers(segments, half);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4 + joins.len() * 7);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6 + joins.len() * 6 * 3);
    for (a, b) in segments {
        let delta = *b - *a;
        let length = delta.length().max(f32::EPSILON);
        let dir = delta / length;
        let side = Vec2::new(-dir.y, dir.x) * half;
        let center = (*a + *b) * 0.5;
        let ext = dir * (length * 0.5);
        let base = positions.len() as u32;
        for offset in [-ext - side, ext - side, ext + side, -ext + side] {
            positions.push((center + offset).extend(0.0).to_array());
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    for center in &joins {
        let base = positions.len() as u32;
        positions.push([center.x, center.y, 0.0]);
        for i in 0..STROKE_JOIN_SEGMENTS {
            let ang = i as f32 / STROKE_JOIN_SEGMENTS as f32 * std::f32::consts::TAU;
            positions.push([
                center.x + half * ang.cos(),
                center.y + half * ang.sin(),
                0.0,
            ]);
        }
        for i in 0..STROKE_JOIN_SEGMENTS as u32 {
            let a = base + 1 + i;
            let b = base + 1 + (i + 1) % STROKE_JOIN_SEGMENTS as u32;
            indices.extend([base, a, b]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 接头圆盘的内接边数。半径 ≤3px 时 6 边的圆度损失不可见（POI 圆周取 12 段，
/// 那是因为点标半径可到 10px；接头比点标小一档，取一半）。
const STROKE_JOIN_SEGMENTS: usize = 6;

/// 找出**有 ≥2 段共用**的端点，返回需要补圆盘的中心点。
///
/// 重合判定用**位型相等**（`f32::to_bits` 当 key），不做 epsilon 合并：
/// - 同一条折线的相邻段由 `windows(2)` 产出，共享的那个顶点是**同一个 f32 克隆**；
/// - 路网边共享的地点坐标来自同一次 `map_to_view` 计算——位型必然相等；
/// - 反过来，两条「几乎但不真的相接」的路若被 epsilon 粘住，会凭空多出一块
///   圆盘（假接头），比缺口更难看。
fn stroke_join_centers(segments: &[(Vec2, Vec2)], half: f32) -> Vec<Vec2> {
    if segments.len() < 2 || half <= 0.0 {
        return Vec::new();
    }
    use std::collections::HashMap;
    let key = |p: Vec2| (p.x.to_bits(), p.y.to_bits());
    let mut degree: HashMap<(u32, u32), u32> = HashMap::with_capacity(segments.len() * 2);
    for (a, b) in segments {
        *degree.entry(key(*a)).or_insert(0) += 1;
        *degree.entry(key(*b)).or_insert(0) += 1;
    }
    degree
        .into_iter()
        .filter(|(_, d)| *d >= 2)
        .map(|((x, y), _)| Vec2::new(f32::from_bits(x), f32::from_bits(y)))
        .collect()
}

/// 带宽版合批 mesh：每段一个世界空间矩形，**厚度逐段独立**。
///
/// 为什么不并进 [`build_stroke_batch_mesh`]：那个签名里的 `world_thickness`
/// 是整组一个标量（道路三档各自均匀），河是**逐段锥形宽**（ADR-0070 D4，
/// 源头 30% → 河口 100%）——同一 mesh 内厚度各不相同。顶点布局 / 接头补盘
/// / 索引模式逐行对齐均匀版，只有 `half` 从参数变成了逐段值。
///
/// `strokes` 元组第三位 = 该段的**世界厚度**（已含锥形系数）。
pub(crate) fn build_stroke_batch_mesh_widths(strokes: &[(Vec2, Vec2, f32)]) -> Mesh {
    let n = strokes.len();

    // 接头补盘同款（G3b）：度数 ≥2 的共点补圆盘，半径取共点段中**最宽**的
    // 半宽——锥形相邻段宽差有限，宁可盘略大不可留缺口（缺口是硬伤，大盘
    // 被同色段盖住不可见）。
    let joins = stroke_join_centers_widths(strokes);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4 + joins.len() * 7);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6 + joins.len() * 6 * 3);
    for (a, b, w) in strokes {
        let half = w * 0.5;
        let delta = *b - *a;
        let length = delta.length().max(f32::EPSILON);
        let dir = delta / length;
        let side = Vec2::new(-dir.y, dir.x) * half;
        let center = (*a + *b) * 0.5;
        let ext = dir * (length * 0.5);
        let base = positions.len() as u32;
        for offset in [-ext - side, ext - side, ext + side, -ext + side] {
            positions.push((center + offset).extend(0.0).to_array());
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    for (center, half) in &joins {
        let base = positions.len() as u32;
        positions.push([center.x, center.y, 0.0]);
        for i in 0..STROKE_JOIN_SEGMENTS {
            let ang = i as f32 / STROKE_JOIN_SEGMENTS as f32 * std::f32::consts::TAU;
            positions.push([
                center.x + half * ang.cos(),
                center.y + half * ang.sin(),
                0.0,
            ]);
        }
        for i in 0..STROKE_JOIN_SEGMENTS as u32 {
            let a = base + 1 + i;
            let b = base + 1 + (i + 1) % STROKE_JOIN_SEGMENTS as u32;
            indices.extend([base, a, b]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// [`stroke_join_centers`] 的带宽版：返回共点位置与该点应补的**最大半宽**。
///
/// 位型相等判定（`f32::to_bits` 当 key）与均匀版同一条论证：同一条折线的
/// 相邻段由 `windows(2)` 产出，共享顶点是同一个 f32 克隆；epsilon 粘合只会
/// 制造假接头。
fn stroke_join_centers_widths(strokes: &[(Vec2, Vec2, f32)]) -> Vec<(Vec2, f32)> {
    if strokes.len() < 2 {
        return Vec::new();
    }
    use std::collections::HashMap;
    let key = |p: Vec2| (p.x.to_bits(), p.y.to_bits());
    let mut degree: HashMap<(u32, u32), (u32, f32)> = HashMap::with_capacity(strokes.len() * 2);
    for (a, b, w) in strokes {
        let half = w * 0.5;
        let da = degree.entry(key(*a)).or_insert((0, 0.0));
        da.0 += 1;
        da.1 = da.1.max(half);
        let db = degree.entry(key(*b)).or_insert((0, 0.0));
        db.0 += 1;
        db.1 = db.1.max(half);
    }
    degree
        .into_iter()
        .filter(|(_, (d, _))| *d >= 2)
        .map(|((x, y), (_, half))| (Vec2::new(f32::from_bits(x), f32::from_bits(y)), half))
        .collect()
}

/// 屏幕密度预算（ADR-0030 D3）：本帧点标上限。视口只是第一道门，「看到的
/// 地方」在 PRD 这种高密度簇里本身就有上万个——成本预算必须与屏幕挂钩。
/// 超出按优先级裁剪，**纯渲染投影**：不写数据、不阻编辑（V1 无涉）。
const POI_MAX_DOTS: usize = 400;
/// POI 圆周段数：12 段近似（半径 ≤10px 时圆度损失不可见，ADR-0030 D4）。
const POI_CIRCLE_SEGMENTS: usize = 12;

/// POI 合批 mesh 的重建判据（ADR-0030 D4）：集合指纹 + 半径 + Z。
#[derive(Debug, Clone, Copy, PartialEq)]
struct PoiBatchKey {
    fingerprint: u64,
    radius: f32,
    z: f32,
}

/// POI 材质桶池：一个材质一个合批 mesh + 一个实体（ADR-0030 D4）。
///
/// 旧实现每点一对实体 + 逐点 mesh，半径 ∝ `view.scale` 让缩放期间每帧重建
/// 上万个小 mesh 资产（guangdong 实测实体 704 → 35,988、1.8 FPS）。合批后
/// 「每点成本」变「每桶成本」：可见集或半径变化才换 ≤4 个 mesh。
#[derive(Default)]
struct PoiBatchPool {
    batch: Option<PoiBatch>,
}

/// 一个 POI 材质桶的实体与 Mesh 句柄。
struct PoiBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: PoiBatchKey,
}

impl PoiBatchPool {
    /// 整组下线（Place 图层不可见时）。实体与 Mesh 资产一起清。
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

/// 屏幕像素 → 世界线宽：`world = px * view_scale`。`view.scale` 的单位是
/// **世界单位/像素**（见 `ViewState::scale` 的文档与 `terrain_chunks.rs` 的
/// 视口换算），所以恒定的屏幕线宽必须**乘** scale。历史上曾写成
/// `px / view_scale`，屏幕宽度实际变成 `stroke_px / scale²`：远观时细到
/// 几乎消失，深缩时胀成占屏 40% 的巨带——这个纯函数就是防它复发的单点。
pub(crate) fn stroke_world_thickness(stroke_px: f32, view_scale: f32) -> f32 {
    (stroke_px * view_scale).max(f32::EPSILON)
}

/// 线宽量化（ADR-0030 D5b）：相对漂移超过 4% 才算「变了」。
///
/// 屏幕线宽误差 ≤ 4%（6px casing 差 0.24px，不可见）；连续缩放（~1.5%/帧）
/// 下重建频率降约 3 倍。`b` 是已烘焙进几何的线宽，恒 > 0（`max(EPSILON)`）。
pub(crate) fn thickness_drifted(next: f32, baked: f32) -> bool {
    (next - baked).abs() > 0.04 * baked.max(f32::EPSILON)
}

/// POI 屏幕网格边长（屏幕像素）。96px ≈ 一个街区尺度：1600×900 视口约
/// 17×10 格，每格 1~2 个点，正好是检索类地图的观感。
const POI_GRID_PX: f32 = 96.0;

/// 每格保留点数：Edit 档 2（作者要看细节），更远的档 1（只留地标）。
const fn poi_cell_quota(tier: crate::state::LodTier) -> usize {
    use crate::state::LodTier;
    match tier {
        LodTier::Edit => 2,
        _ => 1,
    }
}

/// POI 门槛 + 密度选择（ADR-0030 D2/D3；ADR-0064 D2 加屏幕网格配额）。
///
/// 两步：
/// 1. **级别硬门** [`crate::state::poi_visible_at_rank`]；
/// 2. **屏幕网格配额**：按视图坐标落进 `POI_GRID_PX` 的格，格内按优先级
///    只留 [`poi_cell_quota`] 个，最后整体截到 [`POI_MAX_DOTS`]。
///
/// **为什么不能只做全局截断**（旧行为）：全局排序后截 400，密集城区的高 rank
/// 点会把名额全吃掉，稀疏区域反而空着；观感是「城里糊成一片、乡下什么都没有」。
/// 检索类地图的密度控制本来就是**空间均匀**的——每屏网格留几个，而不是全局
/// 留几百个。
///
/// 优先级序：`rank` > `is_city` > 名字长度（更长者信息量更大，与标签碰撞
/// 优先级同一惯例）。排序稳定，同优先级保持原序（确定性好测试）。
///
/// `view_scale` 是「世界单位 / 屏幕像素」：乘出格的世界边长，保证配额按
/// **屏幕**均匀而不是按世界坐标均匀（放大时格子随视口变小，点才不会挤在一起）。
fn select_pois(dots: &[PlaceDot], tier: crate::state::LodTier, view_scale: f32) -> Vec<&PlaceDot> {
    let mut visible: Vec<&PlaceDot> = dots
        .iter()
        .filter(|d| crate::state::poi_visible_at_rank(d.rank, tier))
        .collect();
    visible.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| b.is_city.cmp(&a.is_city))
            .then_with(|| b.name_len.cmp(&a.name_len))
    });
    let cell = (POI_GRID_PX * view_scale).max(f32::EPSILON);
    let quota = poi_cell_quota(tier);
    let mut per_cell: std::collections::HashMap<(i32, i32), usize> =
        std::collections::HashMap::new();
    let mut out: Vec<&PlaceDot> = Vec::with_capacity(visible.len().min(POI_MAX_DOTS));
    for d in visible {
        let key = (
            (d.pos.x / cell).floor() as i32,
            (d.pos.y / cell).floor() as i32,
        );
        let used = per_cell.entry(key).or_insert(0);
        if *used >= quota {
            continue;
        }
        *used += 1;
        out.push(d);
        if out.len() >= POI_MAX_DOTS {
            break;
        }
    }
    out
}

/// 点集内容指纹（ADR-0030 D4）：合批 mesh 的「集合变没变」判据。
/// 只哈希坐标位型——半径 / Z 在 [`PoiBatchKey`] 里单独成字段。
pub(crate) fn dots_fingerprint(dots: &[Vec2]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for p in dots {
        for v in [p.x, p.y] {
            hash ^= u64::from(v.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 把点集合批成**一个**环带 Mesh：每点一个 12 段 annulus（内外圈各 12 顶点
/// 共享，`24N` 顶点 / `72N` 索引），圆心即点位置，z=0（实体 Transform 承担）。
pub(crate) fn build_poi_ring_batch_mesh(
    dots: &[Vec2],
    inner_radius: f32,
    outer_radius: f32,
) -> Mesh {
    let n = dots.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * POI_CIRCLE_SEGMENTS * 2);
    let mut indices: Vec<u32> = Vec::with_capacity(n * POI_CIRCLE_SEGMENTS * 6);
    for pos in dots {
        let base = positions.len() as u32;
        // 内外圈各 12 顶点共享：偶数下标 = 内圈，奇数下标 = 外圈。
        for s in 0..POI_CIRCLE_SEGMENTS {
            let a = std::f32::consts::TAU * s as f32 / POI_CIRCLE_SEGMENTS as f32;
            let (c, s_) = (a.cos(), a.sin());
            positions.push([pos.x + inner_radius * c, pos.y + inner_radius * s_, 0.0]);
            positions.push([pos.x + outer_radius * c, pos.y + outer_radius * s_, 0.0]);
        }
        for s in 0..POI_CIRCLE_SEGMENTS {
            let i0 = base + 2 * s as u32;
            let i1 = base + 2 * ((s + 1) % POI_CIRCLE_SEGMENTS) as u32;
            let (o0, o1) = (i0 + 1, i1 + 1);
            indices.extend([i0, i1, o1, i0, o1, o0]);
        }
    }
    finish_batch_mesh(positions, indices)
}

/// 把点集合批成**一个**扇形圆 Mesh：每点中心 + 12 圈顶点（`13N` 顶点 /
/// `36N` 索引），圆心即点位置。
pub(crate) fn build_poi_dot_batch_mesh(dots: &[Vec2], radius: f32) -> Mesh {
    let n = dots.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * (POI_CIRCLE_SEGMENTS + 1));
    let mut indices: Vec<u32> = Vec::with_capacity(n * POI_CIRCLE_SEGMENTS * 3);
    for pos in dots {
        let base = positions.len() as u32;
        positions.push([pos.x, pos.y, 0.0]);
        for s in 0..POI_CIRCLE_SEGMENTS {
            let a = std::f32::consts::TAU * s as f32 / POI_CIRCLE_SEGMENTS as f32;
            positions.push([pos.x + radius * a.cos(), pos.y + radius * a.sin(), 0.0]);
        }
        for s in 0..POI_CIRCLE_SEGMENTS {
            let q = base + 1 + s as u32;
            let q_next = if s + 1 == POI_CIRCLE_SEGMENTS {
                base + 1
            } else {
                q + 1
            };
            indices.extend([base, q, q_next]);
        }
    }
    finish_batch_mesh(positions, indices)
}

/// 合批 Mesh 的公共收尾：三角形 + 双世界驻留（与 `build_stroke_batch_mesh`
/// 同款资产口径）。
fn finish_batch_mesh(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 按模式缓存的道路材质。
struct RoadMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    /// 三档路缘材质（索引 0 = 最高等级，见 [`ROAD_GRADES`]）。
    casing: [Handle<ColorMaterial>; ROAD_GRADES],
    /// 三档道路主体材质。
    fill: [Handle<ColorMaterial>; ROAD_GRADES],
}

/// 按模式缓存的 POI 材质。
struct PoiMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    /// 非城市 POI：随 `lod_fade` 淡出（远观趋 0）。
    ring: Handle<ColorMaterial>,
    dot: Handle<ColorMaterial>,
    /// 城市 POI：恒全显（Nation 档保留是既有语义）。
    city_ring: Handle<ColorMaterial>,
    city_dot: Handle<ColorMaterial>,
}

/// 专业制图的分类 POI 池（ADR-0064）：每类一个 map-pin 合批。
///
/// 形状统一（Lucide map-pin 实心化），类别只换**针体色**——这是主理人
/// 2026-10-25 的裁定：先统一形状，后续再补各类图标。颜色烘焙在顶点色里，
/// 材质统一白色，故一个池一个 mesh。
#[derive(Default)]
struct PoiKindBucket {
    pin: LandmarkPool,
}

impl PoiKindBucket {
    fn despawn(&mut self, commands: &mut Commands) {
        self.pin.despawn(commands);
    }
}

/// 按模式 + 主题缓存的街道材质（ADR-0014：主干 / 次干在古舆图里是两种色）。
struct StreetMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    major_casing: Handle<ColorMaterial>,
    major_fill: Handle<ColorMaterial>,
    minor_casing: Handle<ColorMaterial>,
    minor_fill: Handle<ColorMaterial>,
}

/// 道路 stroke 合批池：一个 pass 一个合并 mesh + 一个实体（A2，2026-10-18）。
///
/// 旧实现每段一个实体 + 一个 `Rectangle` mesh，规格含 center 与随缩放的
/// 线宽——guangdong 若有 87k 段，相机平移/缩放的每一帧都全量 add+remove
/// 资产。现在 [`StrokeBatchKey`]（内容指纹 / 线宽 / Z）任一变化才整组换
/// 一个 mesh；平移（纯相机运动，段坐标不变）零重建。
#[derive(Default)]
struct RoadLayerPool {
    batch: Option<StrokeBatch>,
}

/// 一个合批 stroke 的实体与 Mesh 句柄。
struct StrokeBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: StrokeBatchKey,
}

/// 地物符号合批池（每类一个 mesh）。与道路 stroke 同款「单 mesh 单实体」，
/// 改几何走 `Assets::get_mut` 原地改写顶点。
#[derive(Default)]
struct LandmarkPool {
    batch: Option<StrokeBatch>,
}

impl LandmarkPool {
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

impl RoadLayerPool {
    /// 整组下线（图层不可见时）。实体与 Mesh 资产一起清，下次可见重新合批。
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

/// 矢量层实体与可复用资产规格缓存。
#[derive(Resource, Default)]
pub struct VectorLayerEntities {
    /// 三档道路的 casing 合批池（索引 0 = 最高等级）：分级后每档线宽不同，
    /// 必须各自成 mesh——同一个 mesh 只能有一个世界厚度。
    road_casings: [RoadLayerPool; ROAD_GRADES],
    /// 三档道路的 fill 合批池。
    roads: [RoadLayerPool; ROAD_GRADES],
    /// 街道矢量层（ADR-0010）：主干档（rank ≤ 2），画得粗、压在行程连线上。
    street_major: RoadLayerPool,
    /// 街道矢量层：次干/支路档，画得细。
    street_minor: RoadLayerPool,
    /// 街道主干档的 casing（浅灰路缘）：压在本档 fill 之下。
    street_major_casing: RoadLayerPool,
    /// 街道次干/支路档的 casing。
    street_minor_casing: RoadLayerPool,
    /// 街道编辑器的橡皮筋预览（上一落点 → 光标）。
    street_preview: RoadLayerPool,
    /// 河流折线合批（ADR-0037 D5 建池；ADR-0070 T8 起单池单材质——
    /// 唯一供给源 = 河线 Place，作者手绘与迁移落地的模板河同池）。
    rivers: RoadLayerPool,
    /// 河流材质（惰性建一次；固定水蓝，主题化随 README §12-30 后继）。
    river_material: Option<Handle<ColorMaterial>>,
    /// 地物符号合批（每类一个 mesh + 一个实体，共 `PoiKind::COUNT` 个）；
    /// 与 decor 同款「原地改写」纪律。顶点色已在 mesh 端烘好 → 配白色
    /// ColorMaterial（ADR-0093：图标钉按类别配色 + 白色含义图形）。
    landmarks: [LandmarkPool; wv_core::distance::PoiKind::COUNT],
    /// 地物符号材质（白色，顶点色承载全部颜色；惰性建一次防资产累积）。
    landmark_material: Option<Handle<ColorMaterial>>,
    /// POI 外环合批（非城市材质，ADR-0030 D4）。
    poi_ring_plain: PoiBatchPool,
    /// POI 外环合批（城市材质）。
    poi_ring_city: PoiBatchPool,
    /// POI 内芯合批（非城市材质）。
    poi_dot_plain: PoiBatchPool,
    /// POI 内芯合批（城市材质）。
    poi_dot_city: PoiBatchPool,
    /// 分类 POI 池（ADR-0064）：**仅 GoogleMap 使用**，索引 = `PoiKind::index()`。
    /// 两档池（上四条）在专业制图下退场。
    poi_kind_pools: Vec<PoiKindBucket>,
    /// 分类 POI 的统一材质（白色；类别色烘焙在顶点色里）。惰性建一次。
    poi_pin_material: Option<Handle<ColorMaterial>>,
    road_materials: Vec<RoadMaterialSet>,
    road_material_mode: Option<crate::state::ViewMode>,
    poi_materials: Vec<PoiMaterialSet>,
    poi_material_mode: Option<crate::state::ViewMode>,
    /// 街道分级材质：主干 / 次干各 casing+fill（古舆图双色，ADR-0016）。
    street_materials: Vec<StreetMaterialSet>,
    street_material_key: Option<(crate::state::ViewMode, MapPalette)>,
}

/// 每帧同步道路、街道与 POI。系统只消费 `ProjectionFrame` 与街道缓存，
/// 不修改世界数据。
///
/// 可见 / 透明度 / Z 偏移读 [`LayerFrame`]（ADR-0013）：Road 组与 Place 组
/// 各自独立绑定，组内 casing < fill < 标注的次序不变，整组随 `z_offset` 平移。
#[allow(clippy::too_many_arguments)]
pub fn sync_vector_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    frame: Res<ProjectionFrame>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    streets: Res<crate::streets::StreetLayerCache>,
    editor: Res<crate::streets::StreetEditorState>,
    mut layer: ResMut<VectorLayerEntities>,
) {
    let road_materials_changed = ensure_road_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.road.opacity,
        &mut layer,
    );
    let _poi_materials_changed = ensure_poi_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.place.opacity,
        &mut layer,
    );
    let street_materials_changed = ensure_street_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.road.opacity,
        &mut layer,
    );
    // 绑定变化（作者改 YAML 的 opacity）：缓存材质的 alpha 就地刷新。
    // z 偏移不需要这步——sync_road_strokes / sync_poi 每帧全量重写 Transform。
    if layers.is_changed() {
        apply_road_material_alpha(&mut materials, &layer, layers.road.opacity);
    }
    // POI 的 alpha 只依赖 Place 图层 opacity（级别门槛是硬门，不再有缩放
    // 淡出，ADR-0030 D2）：每帧刷 4 个材质句柄，代价可忽略。
    apply_poi_material_alpha(&mut materials, &layer, layers.place.opacity);
    let road_z = layers.road.z_offset;
    let place_z = layers.place.z_offset;

    if !layers.road.visible {
        for pool in &mut layer.road_casings {
            pool.despawn(&mut commands);
        }
        for pool in &mut layer.roads {
            pool.despawn(&mut commands);
        }
        layer.rivers.despawn(&mut commands);
        layer.river_material = None;
        for p in &mut layer.landmarks {
            p.despawn(&mut commands);
        }
        layer.street_major.despawn(&mut commands);
        layer.street_minor.despawn(&mut commands);
        layer.street_major_casing.despawn(&mut commands);
        layer.street_minor_casing.despawn(&mut commands);
        layer.street_preview.despawn(&mut commands);
    } else {
        let (casings, fills) = layer
            .road_materials
            .iter()
            .find(|m| m.mode == view.mode)
            .map(|m| (m.casing.clone(), m.fill.clone()))
            .expect("ensure_road_materials 必须已初始化");
        let (major_casing, major_fill, minor_casing, minor_fill) = layer
            .street_materials
            .iter()
            .find(|m| m.mode == view.mode && m.palette == view.map_palette)
            .map(|m| {
                (
                    m.major_casing.clone(),
                    m.major_fill.clone(),
                    m.minor_casing.clone(),
                    m.minor_fill.clone(),
                )
            })
            .expect("ensure_street_materials 必须已初始化");
        // 河流（ADR-0037 D5 建制；ADR-0070 T8 单池化）：地形族水系，
        // gate = MODE_TABLE(Rivers) ∧ terrain 图层。与 road_stale 无关——
        // river_polylines 的生命周期由 compute_projection 的 river_key 缓存管；
        // 这里只做「可见才合批」。
        // 级别门槛（ADR-0047 D5）：河线带 rank（长度占比折算），过统一门槛
        // ——Nation 档只画横贯级主河，Region 档加区域河，Edit 全见。
        // 锥形宽（ADR-0070 D4）：逐段宽 = 基准宽 × lerp(0.3, 1.0, 弧长占比)
        // （源头 30% → 河口 100%）；未声明 `line_width_m`（width_wu == 0，
        // 存量夹具）退旧屏幕像素缺省宽。
        if recipe.enabled(PassId::Rivers) && layers.terrain.visible {
            let mat = layer.river_material.get_or_insert_with(|| {
                // 2026-10-22 验收：CellMap 用固定深青（数据视图，不随主题）；
                // 其他模式沿用原水色。
                materials.add(ColorMaterial::from_color(
                    if matches!(view.mode, crate::state::ViewMode::Topographic) {
                        Color::srgb(0.12, 0.32, 0.45)
                    } else {
                        Color::srgb(0.29, 0.53, 0.68)
                    },
                ))
            });
            let river_tier = lod_tier(view.scale);
            let mut river_strokes: Vec<(Vec2, Vec2, f32)> = Vec::new();
            for line in &frame.river_polylines {
                if !crate::state::rank_visible_at_tier(line.rank, river_tier) {
                    continue;
                }
                let base = if line.width_wu > 0.0 {
                    line.width_wu
                } else {
                    stroke_world_thickness(RIVER_STROKE_PX, view.scale)
                };
                let factors = river_taper_factors(&line.pts);
                for (pair, f) in line.pts.windows(2).zip(factors) {
                    river_strokes.push((pair[0], pair[1], base * f));
                }
            }
            let fp = river_strokes_fingerprint(&river_strokes);
            sync_river_strokes(
                &mut commands,
                &mut meshes,
                &river_strokes,
                fp,
                mat.clone(),
                RIVER_Z,
                false,
                &mut layer.rivers,
            );
        } else {
            layer.rivers.despawn(&mut commands);
        }
        // 世界道路（ADR-0047 D4）：按当前视野档位取降序前缀切片——Nation 档
        // 只画城际干线（rank ≥ 3），Region 加区域级，Edit 全量。缩放不重算
        // 段列（缓存门 road_key 不含 scale），跨档只换 mesh（分档指纹）。
        // 2026-10-22 内容矩阵：接 MODE_TABLE(Roads)——Topographic 下不画。
        let road_tier = lod_tier(view.scale);
        let road_segments = frame.road_slice_at(road_tier);
        let road_fingerprint = frame.road_fingerprint_at(road_tier);
        if recipe.enabled(PassId::Roads) {
            // 道路分级（2026-10-25）：段列按 rank 降序，三档是连续前缀区间
            // （`rank ≥ 3` / `= 2` / `= 1`）。每档线宽不同 → 必须各自成 mesh。
            // 低档视野天然只含高档（Nation 只有 `[0, cuts[2])`），所以
            // 「放大才看得见小路」由切片本身保证，不需要额外的可见性判据。
            let spans = road_grade_spans(road_tier, frame.road_rank_cuts);
            for (grade, (start, end)) in spans.iter().enumerate() {
                let segs = &road_segments[*start..*end];
                // 分档指纹 = 全量指纹 ⊕ 区间端点：段列内容或分区任一变化都重建。
                // 只用全量指纹会在「只改 rank」时漏判（坐标集合没变、分区变了）。
                let fp = road_fingerprint
                    ^ (*start as u64).rotate_left(17)
                    ^ (*end as u64).rotate_left(41);
                let z_jitter = grade as f32 * 0.0006;
                let (casing_px, fill_px) = road_grade_px(view.mode, grade);
                sync_road_strokes(
                    &mut commands,
                    &mut meshes,
                    segs,
                    fp,
                    view.scale,
                    casings[grade].clone(),
                    casing_px,
                    ROAD_CASING_Z + road_z + z_jitter,
                    road_materials_changed,
                    &mut layer.road_casings[grade],
                );
                sync_road_strokes(
                    &mut commands,
                    &mut meshes,
                    segs,
                    fp,
                    view.scale,
                    fills[grade].clone(),
                    fill_px,
                    ROAD_Z + road_z + z_jitter,
                    road_materials_changed,
                    &mut layer.roads[grade],
                );
            }
        } else {
            for pool in &mut layer.road_casings {
                pool.despawn(&mut commands);
            }
            for pool in &mut layer.roads {
                pool.despawn(&mut commands);
            }
        }
        // 橡皮筋预览：画线中的「上一落点 → 光标」（编辑器反馈，与 roads_visible
        // 无关——预览属于编辑会话，不属于图层可见性）。两端都是**视图空间**坐标
        // （光标来自 `view.cursor_world`，落点由 `streets` 存同一空间），
        // 不需要任何换算——历史上这里用 `grid.origin + (世界 − 本图帧原点)` 兜了
        // 一圈，城级（继承底图）时那一圈会把预览推到视野外。
        let preview: Vec<(Vec2, Vec2)> =
            match (editor.enabled, editor.last_click, view.cursor_world) {
                (true, Some(a), Some(b)) => vec![(a, b)],
                _ => Vec::new(),
            };
        let preview_fp = segments_fingerprint(&preview);
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &preview,
            preview_fp,
            view.scale,
            fills[ROAD_GRADES - 1].clone(),
            STREET_PREVIEW_PX,
            STREET_PREVIEW_Z + road_z,
            road_materials_changed,
            &mut layer.street_preview,
        );
        // 2026-10-22 内容矩阵：接 MODE_TABLE(Streets)——Topographic 下不画。
        if recipe.enabled(PassId::Streets) {
            sync_street_layers(
                &mut commands,
                &mut meshes,
                &grid,
                &streets,
                view.scale,
                major_casing,
                major_fill,
                minor_casing,
                minor_fill,
                road_z,
                street_materials_changed,
                &mut layer,
            );
        } else {
            layer.street_major.despawn(&mut commands);
            layer.street_minor.despawn(&mut commands);
            layer.street_major_casing.despawn(&mut commands);
            layer.street_minor_casing.despawn(&mut commands);
        }
    }

    // 地物符号（rank ≥ 2）的档位先算：POI 层要据此让位（见下）。
    //
    // 画法按模式分流（[`landmark_marker`]）：
    // - **CellMap（Topographic）**：map-pin——数据视图的地点标记，独立于
    //   FeatureOverlay（该行 pass 是 false，pin 是唯一例外）；
    // - **专业制图（Map / GoogleMap）**：同样 map-pin，但按 rank 分尺寸档
    //   （[`landmark_pin_scale`]）——普通地图没有插画可画，城市的分量只能靠
    //   「符号大小 + 字级」两级承载；
    // - **其余模式**：城堡 / 小镇程序化插画（艺术主题的观感）。
    //
    // place 图层关时都不画。注意**不得**嵌进下面的 POI 门：Topographic 下
    // Poi=false，嵌进去 pin 永远画不出（首启实测 mesh_count=0 的根因）。
    let landmark_marker = landmark_marker(view.mode);
    // ADR-0093：图标钉**全模式**在场（用户裁定「POI 图标全部地图都要」）——
    // 城堡/小镇插画的 FeatureOverlay 门随插画退役；只剩 place 图层门。
    let landmarks_want = layers.place.visible;

    // 2026-10-22 内容矩阵：接 MODE_TABLE(Poi)——Topographic 下不画。
    if !layers.place.visible || !recipe.enabled(PassId::Poi) {
        layer.poi_ring_plain.despawn(&mut commands);
        layer.poi_ring_city.despawn(&mut commands);
        layer.poi_dot_plain.despawn(&mut commands);
        layer.poi_dot_city.despawn(&mut commands);
        for b in &mut layer.poi_kind_pools {
            b.despawn(&mut commands);
        }
    } else {
        sync_poi(
            &mut commands,
            &mut meshes,
            &frame.place_dots,
            view.scale,
            &grid,
            place_z,
            landmarks_want && landmark_marker == LandmarkMarker::Pin,
            &mut layer,
        );
    }

    if landmarks_want {
        let white = layer
            .landmark_material
            .get_or_insert_with(|| materials.add(ColorMaterial::from_color(Color::WHITE)))
            .clone();
        let VectorLayerEntities { landmarks, .. } = &mut *layer;
        sync_landmarks(
            &mut commands,
            &mut meshes,
            landmarks,
            &frame.landmark_dots,
            view.scale,
            crate::landmark_symbols::LANDMARK_Z + layers.place.z_offset,
            white,
        );
    } else {
        for p in &mut layer.landmarks {
            p.despawn(&mut commands);
        }
        layer.landmark_material = None;
    }
}

/// 道路三档的基础色 `(casing, fill)`（索引 0 = 最高等级）。
///
/// Google 模式取 [`GOOGLE_ROAD_GRADES`]：橙黄干线 / 浅黄干道 / 白支路——这张
/// 「粗细 + 色阶」是普通地图可读性的主要来源（2026-10-25 之前全档同色，
/// 路网糊成一片）。非 Google 主题保持单色：古舆图的路不分级是刻意的美术选择，
/// 三档填同一个色即退回原观感。
fn road_grade_colors(
    mode: crate::state::ViewMode,
    pal: &PaletteColors,
) -> [(Color, Color); ROAD_GRADES] {
    match mode {
        crate::state::ViewMode::GoogleMap => GOOGLE_ROAD_GRADES.map(|g| (g.casing, g.fill)),
        _ => [(pal.road_casing.with_alpha(0.28), pal.road_stroke); ROAD_GRADES],
    }
}

/// POI 组两种模式的基础色（ring, dot）。
fn poi_base_colors(mode: crate::state::ViewMode, pal: &PaletteColors) -> (Color, Color) {
    match mode {
        crate::state::ViewMode::GoogleMap => (GOOGLE_POI_RING, GOOGLE_PLACE_DOT),
        _ => (pal.poi_ring, pal.poi_dot),
    }
}

/// 专业制图的 POI 类别色（ADR-0064）。未分类 / 聚落用默认红点。
///
/// 两处「借用」是有意的：山峰用景点绿（地标 = 可游览）、港口并入交通蓝
/// （同为「到达点」语义）。新增类别必须在这里给出落点——`PoiKind::COUNT`
/// 变了而这里漏写会编译失败（match 覆盖全部变体）。
fn poi_kind_color(kind: PoiKind) -> Color {
    use PoiKind::*;
    match kind {
        Food => GOOGLE_POI_FOOD,
        Lodging => GOOGLE_POI_LODGING,
        Medical => GOOGLE_POI_MEDICAL,
        Scenic | Peak => GOOGLE_POI_SCENIC,
        Station | Port => GOOGLE_POI_TRANSIT,
        Government => GOOGLE_POI_GOVERNMENT,
        Education => GOOGLE_POI_EDUCATION,
        Shop => GOOGLE_POI_SHOP,
        Ruins => GOOGLE_POI_RUINS,
        Arena => GOOGLE_POI_ARENA,
        Shrine => GOOGLE_POI_SHRINE,
        Portal => GOOGLE_POI_PORTAL,
        Danger => GOOGLE_POI_DANGER,
        Settlement | Other => GOOGLE_PLACE_DOT,
    }
}

/// 街道两级的基础色（[(主干 casing, 主干 fill), (次干 casing, 次干 fill)]）。
///
/// 古舆图分级双色（ADR-0016）：主干官道棕粗、次干浅卡其细；casing 同为纸面色。
fn street_base_colors(mode: crate::state::ViewMode, pal: &PaletteColors) -> [(Color, Color); 2] {
    match mode {
        crate::state::ViewMode::GoogleMap => [(GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL); 2],
        _ => [
            (pal.road_casing.with_alpha(0.28), pal.street_major),
            (pal.road_casing.with_alpha(0.28), pal.street_minor),
        ],
    }
}

/// 把 Road 图层 opacity 刷进所有已缓存的道路材质（按各模式的基础色重算）。
fn apply_road_material_alpha(
    materials: &mut Assets<ColorMaterial>,
    layer: &VectorLayerEntities,
    opacity: f32,
) {
    for set in &layer.road_materials {
        let grades = road_grade_colors(set.mode, palette::colors(set.palette));
        for (i, (casing, fill)) in grades.iter().enumerate() {
            if let Some(mut mat) = materials.get_mut(&set.casing[i]) {
                mat.color = casing.with_alpha(casing.alpha() * opacity);
            }
            if let Some(mut mat) = materials.get_mut(&set.fill[i]) {
                mat.color = fill.with_alpha(fill.alpha() * opacity);
            }
        }
    }
}

/// 把 Place 图层 opacity 刷进所有已缓存的 POI 材质。
///
/// 级别可见性是档位硬门（ADR-0030 D2），不再有缩放淡出系数——过了门槛的
/// 点标全显，城市/非城市材质只差配色。
fn apply_poi_material_alpha(
    materials: &mut Assets<ColorMaterial>,
    layer: &VectorLayerEntities,
    opacity: f32,
) {
    for set in &layer.poi_materials {
        let (ring, dot) = poi_base_colors(set.mode, palette::colors(set.palette));
        if let Some(mut mat) = materials.get_mut(&set.ring) {
            mat.color = ring.with_alpha(ring.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.dot) {
            mat.color = dot.with_alpha(dot.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.city_ring) {
            mat.color = ring.with_alpha(ring.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.city_dot) {
            mat.color = dot.with_alpha(dot.alpha() * opacity);
        }
    }
    // 分类池（GoogleMap）：统一白材质，类别色在顶点色里，同吃图层 opacity。
    if let Some(handle) = &layer.poi_pin_material {
        if let Some(mut mat) = materials.get_mut(handle) {
            mat.color = Color::WHITE.with_alpha(opacity);
        }
    }
}

fn ensure_road_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    map_palette: MapPalette,
    road_opacity: f32,
    layer: &mut VectorLayerEntities,
) -> bool {
    if layer.road_material_mode == Some(mode) {
        return false;
    }
    if !layer
        .road_materials
        .iter()
        .any(|set| set.mode == mode && set.palette == map_palette)
    {
        let grades = road_grade_colors(mode, palette::colors(map_palette));
        let with_opacity = |c: Color| c.with_alpha(c.alpha() * road_opacity);
        // 逐档建材质：casing 与 fill 各 ROAD_GRADES 个（2026-10-25 道路分级）。
        // `Handle` 不是 Copy，不能用 `[Handle::default(); N]` 铺开。
        let casing: [Handle<ColorMaterial>; ROAD_GRADES] =
            std::array::from_fn(|i| materials.add(with_opacity(grades[i].0)));
        let fill: [Handle<ColorMaterial>; ROAD_GRADES] =
            std::array::from_fn(|i| materials.add(with_opacity(grades[i].1)));
        layer.road_materials.push(RoadMaterialSet {
            mode,
            palette: map_palette,
            casing,
            fill,
        });
    }
    layer.road_material_mode = Some(mode);
    true
}

fn ensure_poi_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    map_palette: MapPalette,
    place_opacity: f32,
    layer: &mut VectorLayerEntities,
) -> bool {
    if layer.poi_material_mode == Some(mode) {
        return false;
    }
    if !layer
        .poi_materials
        .iter()
        .any(|set| set.mode == mode && set.palette == map_palette)
    {
        let (ring, dot) = poi_base_colors(mode, palette::colors(map_palette));
        let with_opacity = |c: Color| c.with_alpha(c.alpha() * place_opacity);
        layer.poi_materials.push(PoiMaterialSet {
            mode,
            palette: map_palette,
            ring: materials.add(with_opacity(ring)),
            dot: materials.add(with_opacity(dot)),
            city_ring: materials.add(with_opacity(ring)),
            city_dot: materials.add(with_opacity(dot)),
        });
    }
    // 分类池（ADR-0064）：12 组 map-pin 合批 + 一个白色材质（顶点色承载类别色）。
    // **建一次、常驻**——切走只退场批次（`sync_poi` 的非专业分支），避免每次
    // 进出专业制图都新建资产。
    if layer.poi_kind_pools.is_empty() {
        layer.poi_kind_pools = (0..PoiKind::COUNT)
            .map(|_| PoiKindBucket::default())
            .collect();
    }
    if layer.poi_pin_material.is_none() {
        layer.poi_pin_material = Some(materials.add(ColorMaterial::from_color(
            Color::WHITE.with_alpha(place_opacity),
        )));
    }
    layer.poi_material_mode = Some(mode);
    true
}

/// 确保街道两级材质已按 (mode, palette) 建好；键变了返回 true（mesh 材质句柄要刷）。
fn ensure_street_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    map_palette: MapPalette,
    road_opacity: f32,
    layer: &mut VectorLayerEntities,
) -> bool {
    let key = (mode, map_palette);
    if layer.street_material_key == Some(key) {
        return false;
    }
    if !layer
        .street_materials
        .iter()
        .any(|set| set.mode == mode && set.palette == map_palette)
    {
        let [(mc, mf), (nc, nf)] = street_base_colors(mode, palette::colors(map_palette));
        let with_opacity = |c: Color| c.with_alpha(c.alpha() * road_opacity);
        layer.street_materials.push(StreetMaterialSet {
            mode,
            palette: map_palette,
            major_casing: materials.add(with_opacity(mc)),
            major_fill: materials.add(with_opacity(mf)),
            minor_casing: materials.add(with_opacity(nc)),
            minor_fill: materials.add(with_opacity(nf)),
        });
    }
    layer.street_material_key = Some(key);
    true
}

/// 同步街道矢量层（ADR-0010 待做第 2 项）。
///
/// 路段流按 LOD 档筛过后分两档宽度画出；坐标变换与地点/连线同一条链
/// （[`GridConfig::map_to_view`]，本图局部 → 视图空间），不另立换算。
/// 缓存为空时两池照常走 `sync_road_strokes` 的空列表路径——既清干净又无资产泄漏。
///
/// **每档都画 casing + fill 两层**（与道路同一套口径）：只画 fill 时，地图模式下
/// 白 fill 落在近白底图上等于不可见（casing 的浅灰才是肉眼看到的那条线）。
#[allow(clippy::too_many_arguments)]
fn sync_street_layers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    grid: &GridConfig,
    streets: &crate::streets::StreetLayerCache,
    view_scale: f32,
    major_casing: Handle<ColorMaterial>,
    major_fill: Handle<ColorMaterial>,
    minor_casing: Handle<ColorMaterial>,
    minor_fill: Handle<ColorMaterial>,
    z_offset: f32,
    material_changed: bool,
    layer: &mut VectorLayerEntities,
) {
    let tier = lod_tier(view_scale);
    let mut major: Vec<(Vec2, Vec2)> = Vec::new();
    let mut minor: Vec<(Vec2, Vec2)> = Vec::new();
    for (rank, segments) in crate::streets::street_streams(&streets.sources) {
        if !crate::streets::street_kept_at_tier(rank, tier) {
            continue;
        }
        // 本图局部坐标 → 视图空间（`GridConfig::map_to_view`，与地点同一条链；
        // 城市继承父图底图时栅格原点不是本图帧原点，绝不能拿 `raster_origin` 顶替）。
        let target = if rank <= 2 { &mut major } else { &mut minor };
        target.extend(
            segments
                .iter()
                .map(|(a, b)| (grid.map_to_view(*a), grid.map_to_view(*b))),
        );
    }
    // casing 先画（Z 更低），fill 后画：两层的宽度差就是可见路缘。
    // 组内次序是编译期约束；整组再叠加 Road 图层的 z_offset（ADR-0013）。
    // 内容指纹各算一次，casing / fill 两个 pass 共用。
    let major_fp = segments_fingerprint(&major);
    let minor_fp = segments_fingerprint(&minor);
    sync_road_strokes(
        commands,
        meshes,
        &major,
        major_fp,
        view_scale,
        major_casing,
        STREET_MAJOR_PX + STREET_CASING_EXTRA_PX,
        STREET_MAJOR_CASING_Z + z_offset,
        material_changed,
        &mut layer.street_major_casing,
    );
    sync_road_strokes(
        commands,
        meshes,
        &major,
        major_fp,
        view_scale,
        major_fill,
        STREET_MAJOR_PX,
        STREET_MAJOR_Z + z_offset,
        material_changed,
        &mut layer.street_major,
    );
    sync_road_strokes(
        commands,
        meshes,
        &minor,
        minor_fp,
        view_scale,
        minor_casing,
        STREET_MINOR_PX + STREET_CASING_EXTRA_PX,
        STREET_MINOR_CASING_Z + z_offset,
        material_changed,
        &mut layer.street_minor_casing,
    );
    sync_road_strokes(
        commands,
        meshes,
        &minor,
        minor_fp,
        view_scale,
        minor_fill,
        STREET_MINOR_PX,
        STREET_MINOR_Z + z_offset,
        material_changed,
        &mut layer.street_minor,
    );
}

/// 同步一组道路 stroke 的**合批** mesh：内容指纹 / 线宽 / Z 任一变化才换
/// mesh，材质变化只换绑 `MeshMaterial2d`（A2，2026-10-18）。
///
/// `fingerprint` 由调用方给定（世界道路取档位切片的 `road_fingerprint_at`；
/// 街道 / 预览 / 河流段列就地 [`segments_fingerprint`]）——同一组段在
/// roads 与 casings 两个 pass 间不重复哈希。
#[allow(clippy::too_many_arguments)]
fn sync_road_strokes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    segments: &[(Vec2, Vec2)],
    fingerprint: u64,
    view_scale: f32,
    material: Handle<ColorMaterial>,
    stroke_px: f32,
    z: f32,
    material_changed: bool,
    pool: &mut RoadLayerPool,
) {
    let key = StrokeBatchKey {
        fingerprint,
        world_thickness: stroke_world_thickness(stroke_px, view_scale),
        z,
    };
    match pool.batch.take() {
        None => {
            if segments.is_empty() {
                return;
            }
            let mesh = meshes.add(build_stroke_batch_mesh(segments, key.world_thickness));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material),
                    Transform::from_xyz(0.0, 0.0, key.z),
                ))
                .id();
            pool.batch = Some(StrokeBatch { entity, mesh, key });
        }
        Some(mut batch) => {
            if segments.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            // 线宽量化（ADR-0030 D5b）：内容与 Z 不变、线宽漂移 ≤4% 时不换
            // 几何——连续缩放下把 87k 段 × 2 pass 的重建频率降约 3 倍，屏幕
            // 线宽误差 ≤ 0.24px（6px casing）。`batch.key` 保留**已烘焙**值，
            // 漂移永远相对几何实际线宽累计。
            let rebuilt = batch.key.fingerprint != key.fingerprint
                || batch.key.z != key.z
                || thickness_drifted(key.world_thickness, batch.key.world_thickness);
            if rebuilt {
                batch.key = key;
                let next = meshes.add(build_stroke_batch_mesh(segments, key.world_thickness));
                let old = std::mem::replace(&mut batch.mesh, next.clone());
                commands
                    .entity(batch.entity)
                    .insert((Mesh2d(next), Transform::from_xyz(0.0, 0.0, key.z)));
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&old);
                });
            }
            if material_changed {
                commands
                    .entity(batch.entity)
                    .insert(MeshMaterial2d(material));
            }
            pool.batch = Some(batch);
        }
    }
}

/// 河流专用：**带宽段列**的合批同步（ADR-0070 T8）。
///
/// 与 [`sync_road_strokes`] 同一套缓存纪律（内容指纹 / Z / 线宽漂移任一变
/// 才换 mesh，材质变化只换绑——**不许**改成每帧无条件重建，AGENTS 历史：
/// 每帧重建 mesh 是资产泄漏坑），差异只在两处：
/// - mesh 构建走 [`build_stroke_batch_mesh_widths`]（逐段锥形厚）；
/// - `StrokeBatchKey::world_thickness` 槽位存**最大段宽**：显式 `width_wu`
///   段不随缩放变（指纹即覆盖），缺省宽段（px × scale）随缩放漂移，复用
///   道路的 4% 量化门（[`thickness_drifted`]）——连续缩放下不逐帧重建。
///
/// 为什么不并成一个函数：道路是「整组一个标量线宽 × 多个池」，河是「一个
/// 池内宽度各异」——mesh 构建器与漂移门的线宽来源都不同；缓存门语义逐
/// 分支对齐即毕，强行参数化只会让两条路径经闭包绕着读。
#[allow(clippy::too_many_arguments)]
fn sync_river_strokes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    strokes: &[(Vec2, Vec2, f32)],
    fingerprint: u64,
    material: Handle<ColorMaterial>,
    z: f32,
    material_changed: bool,
    pool: &mut RoadLayerPool,
) {
    let max_width = strokes.iter().map(|s| s.2).fold(0.0f32, f32::max);
    let key = StrokeBatchKey {
        fingerprint,
        world_thickness: max_width,
        z,
    };
    match pool.batch.take() {
        None => {
            if strokes.is_empty() {
                return;
            }
            let mesh = meshes.add(build_stroke_batch_mesh_widths(strokes));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material),
                    Transform::from_xyz(0.0, 0.0, key.z),
                ))
                .id();
            pool.batch = Some(StrokeBatch { entity, mesh, key });
        }
        Some(mut batch) => {
            if strokes.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            let rebuilt = batch.key.fingerprint != key.fingerprint
                || batch.key.z != key.z
                || thickness_drifted(key.world_thickness, batch.key.world_thickness);
            if rebuilt {
                batch.key = key;
                let next = meshes.add(build_stroke_batch_mesh_widths(strokes));
                let old = std::mem::replace(&mut batch.mesh, next.clone());
                commands
                    .entity(batch.entity)
                    .insert((Mesh2d(next), Transform::from_xyz(0.0, 0.0, key.z)));
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&old);
                });
            }
            if material_changed {
                commands
                    .entity(batch.entity)
                    .insert(MeshMaterial2d(material));
            }
            pool.batch = Some(batch);
        }
    }
}

/// 带宽段列的内容指纹（FNV-1a over 坐标 + 宽度位型）。
///
/// 与 [`segments_fingerprint`] 分开的原因：锥形宽让每段厚度各不相同，
/// 指纹必须覆盖宽度位型，否则「只改宽不改坐标」（改 `line_width_m`）会
/// 漏重建，河就停在旧宽度上。
fn river_strokes_fingerprint(strokes: &[(Vec2, Vec2, f32)]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (a, b, w) in strokes {
        for v in [a.x, a.y, b.x, b.y, *w] {
            hash ^= u64::from(v.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 同步红点白环 POI（ADR-0030 D4 合批版）。
///
/// 门槛 + 密度上限（[`select_pois`]）幸存的点标按 4 个材质桶各合批成
/// **一个** mesh（外环/内芯 × 城市/非城市）；半径或集合变化才换该桶 mesh
/// （旧资产延迟移除，A2 同款）。`z_offset` 是 Place 图层的槽位偏移；
/// 外环/内芯各占一个 Z 槽，桶内共享。
#[allow(clippy::too_many_arguments)]
fn sync_poi(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[PlaceDot],
    view_scale: f32,
    grid: &GridConfig,
    z_offset: f32,
    landmark_pins: bool,
    layer: &mut VectorLayerEntities,
) {
    let tier = crate::state::lod_tier(view_scale);
    let selected = select_pois(dots, tier, view_scale);
    let radius_outer = (grid.cell_size * 0.45).max(POI_MIN_SCREEN_PX * view_scale * 0.5);
    let radius_inner = radius_outer * 0.52;
    let ring_thickness = (1.5 * view_scale).max(f32::EPSILON);
    let ring_inner = (radius_outer - ring_thickness).max(f32::EPSILON);
    let mode = layer
        .poi_material_mode
        .expect("ensure_poi_materials 必须已初始化");
    let (ring_plain, dot_plain, city_ring, city_dot) = layer
        .poi_materials
        .iter()
        .find(|m| m.mode == mode)
        .map(|m| {
            (
                m.ring.clone(),
                m.dot.clone(),
                m.city_ring.clone(),
                m.city_dot.clone(),
            )
        })
        .expect("ensure_poi_materials 必须已初始化");

    // 按材质分桶取坐标（桶内几何只依赖坐标 + 统一半径）。
    let mut plain: Vec<Vec2> = Vec::new();
    let mut city: Vec<Vec2> = Vec::new();
    for d in &selected {
        if d.is_city {
            city.push(d.pos);
        } else {
            plain.push(d.pos);
        }
    }
    let plain_fp = dots_fingerprint(&plain);
    let city_fp = dots_fingerprint(&city);
    let z_outer = POI_OUTER_Z + z_offset;
    let z_inner = POI_INNER_Z + z_offset;

    // ── 专业制图：按 POI 类别分桶（ADR-0064）──
    //
    // 检索地图的「分类符号」= 每类一个颜色。合批 mesh 一个材质只能一种色，
    // 所以类别必须分池。非 Google 模式仍走下面的城市 / 普通两档（艺术主题
    // 刻意不把类别符号化）。
    //
    // **rank ≥ 2 让位给地物符号层**（2026-10-26）：城市由
    // [`sync_landmarks`] 画带尺寸档的 map-pin，此处再画一颗就是双符号。
    // 由此普通地图的分层与百度一致：远观只见城市，分类符号只在近档出现。
    if mode == crate::state::ViewMode::GoogleMap {
        // 模式切换的残留清理：两档池可能还挂着上一个模式的 mesh。
        layer.poi_ring_plain.despawn(commands);
        layer.poi_ring_city.despawn(commands);
        layer.poi_dot_plain.despawn(commands);
        layer.poi_dot_city.despawn(commands);
        let Some(pin_mat) = layer.poi_pin_material.clone() else {
            return;
        };
        // 尺寸口径与地物符号同源：局部坐标 = 屏幕像素，只乘 `view_scale`
        // （符号总高 22.5px + LOD 档微调）。
        let s = landmark_size_tier(view_scale) * view_scale;
        let mut buckets: Vec<Vec<Vec2>> = vec![Vec::new(); PoiKind::COUNT];
        for d in &selected {
            // rank ≥ 2 的地名已由地物符号层画（更大的 map-pin），这里让位。
            if landmark_pins && poi_pin_owner_is_landmark(d.rank) {
                continue;
            }
            if let Some(b) = buckets.get_mut(d.kind.index()) {
                b.push(d.pos);
            }
        }
        for (i, pos) in buckets.iter().enumerate() {
            let Some(b) = layer.poi_kind_pools.get_mut(i) else {
                continue;
            };
            sync_poi_pin_batch(
                commands,
                meshes,
                pos,
                s,
                z_outer,
                poi_kind_color(PoiKind::ALL[i]),
                pin_mat.clone(),
                &mut b.pin,
            );
        }
        return;
    }

    // 非专业制图：分类池退场（材质保留，切回来不用重建）。
    for b in &mut layer.poi_kind_pools {
        b.despawn(commands);
    }

    sync_poi_batch(
        commands,
        meshes,
        &plain,
        plain_fp,
        (ring_inner, radius_outer),
        z_outer,
        ring_plain,
        &mut layer.poi_ring_plain,
        build_poi_ring_batch_mesh,
    );
    sync_poi_batch(
        commands,
        meshes,
        &city,
        city_fp,
        (ring_inner, radius_outer),
        z_outer,
        city_ring,
        &mut layer.poi_ring_city,
        build_poi_ring_batch_mesh,
    );
    sync_poi_batch(
        commands,
        meshes,
        &plain,
        plain_fp,
        (radius_inner, radius_inner),
        z_inner,
        dot_plain,
        &mut layer.poi_dot_plain,
        |dots, r, _| build_poi_dot_batch_mesh(dots, r),
    );
    sync_poi_batch(
        commands,
        meshes,
        &city,
        city_fp,
        (radius_inner, radius_inner),
        z_inner,
        city_dot,
        &mut layer.poi_dot_city,
        |dots, r, _| build_poi_dot_batch_mesh(dots, r),
    );
}

/// 同步一个 POI 材质桶的合批 mesh：指纹/半径/Z 任一变化才换 mesh。
///
/// `build` 由调用方给定（外环要 (inner, outer) 双半径，内芯只用单半径——
/// 传 `(r, r)` 复用同一条同步路径，两个桶字段不必各写一份）。
#[allow(clippy::too_many_arguments)]
fn sync_poi_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[Vec2],
    fingerprint: u64,
    radii: (f32, f32),
    z: f32,
    material: Handle<ColorMaterial>,
    pool: &mut PoiBatchPool,
    build: impl Fn(&[Vec2], f32, f32) -> Mesh,
) {
    let key = PoiBatchKey {
        fingerprint,
        radius: radii.1,
        z,
    };
    match pool.batch.take() {
        None => {
            if dots.is_empty() {
                return;
            }
            spawn_poi_batch(commands, meshes, dots, key, radii, material, pool, build);
        }
        Some(mut batch) => {
            if dots.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            if batch.key != key {
                let next = meshes.add(build(dots, radii.0, radii.1));
                let old = std::mem::replace(&mut batch.mesh, next.clone());
                commands
                    .entity(batch.entity)
                    .insert((Mesh2d(next), Transform::from_xyz(0.0, 0.0, key.z)));
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&old);
                });
                batch.key = key;
            }
            pool.batch = Some(batch);
        }
    }
}

/// 一类 POI 的 map-pin 合批（顶点色承载类别色，材质统一白色）。
///
/// 与地物符号同一套「单 mesh 单实体 + 原地换网格」纪律。key 吃
/// （位置指纹、尺寸、Z）——尺寸不进 key 会让缩放后 pin 保持旧世界尺寸
/// （`landmark_batch_key` 的同一条教训）。
#[allow(clippy::too_many_arguments)]
fn sync_poi_pin_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[Vec2],
    s: f32,
    z: f32,
    color: Color,
    material: Handle<ColorMaterial>,
    pool: &mut LandmarkPool,
) {
    use crate::landmark_symbols::{build_landmark_mesh, LandmarkBatch};
    let key = StrokeBatchKey {
        fingerprint: dots_fingerprint(dots),
        world_thickness: s,
        z,
    };
    let build = || {
        let c = color.to_srgba();
        let body = [c.red, c.green, c.blue, c.alpha];
        let mut batch = LandmarkBatch::new();
        for p in dots {
            batch.push_map_pin_tinted(*p, s, body);
        }
        build_landmark_mesh(batch)
    };
    match pool.batch.take() {
        None => {
            if dots.is_empty() {
                return;
            }
            let Some(mesh) = build() else {
                return;
            };
            let mesh_handle = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Mesh2d(mesh_handle.clone()),
                    MeshMaterial2d(material),
                    Transform::from_xyz(0.0, 0.0, z),
                ))
                .id();
            pool.batch = Some(StrokeBatch {
                entity,
                mesh: mesh_handle,
                key,
            });
        }
        Some(mut batch) => {
            if dots.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            if batch.key != key {
                let Some(mesh) = build() else {
                    return;
                };
                let next = meshes.add(mesh);
                let old = std::mem::replace(&mut batch.mesh, next.clone());
                commands
                    .entity(batch.entity)
                    .insert((Mesh2d(next), Transform::from_xyz(0.0, 0.0, z)));
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&old);
                });
                batch.key = key;
            }
            pool.batch = Some(batch);
        }
    }
}

/// 地物符号（rank ≥ 2 的城市）画法（2026-10-26 专业制图收口）。
///
/// 三种模式族共用一条合批路径，只有「画什么形状」不同——抽成枚举而不是
/// 布尔，是为了让「加第四种画法」时编译器把每个 match 都点出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LandmarkMarker {
    /// 城堡 / 小镇程序化插画：艺术主题（Browse / Edit）的观感。
    Illustration,
    /// 统一 map-pin：数据视图（CellMap）与专业制图（Map）。
    Pin,
}

impl LandmarkMarker {
    pub(crate) fn is_pin(self) -> bool {
        matches!(self, Self::Pin)
    }
}

/// 模式 → 地物符号画法。
pub(crate) fn landmark_marker(mode: crate::state::ViewMode) -> LandmarkMarker {
    use crate::state::ViewMode;
    match mode {
        // 数据视图与普通地图都「不要画风」：地点就是一颗 pin。
        ViewMode::Topographic | ViewMode::GoogleMap => LandmarkMarker::Pin,
        _ => LandmarkMarker::Illustration,
    }
}

/// 专业制图的城市 pin 尺寸档：rank ≥ 3（国都 / 大城）比 rank 2 略大。
///
/// 普通地图没有插画可用，城市的分量只能靠「符号大小 + 字级」两级承载
/// （字级见 ADR-0065）。系数只乘在专业制图的 pin 上；CellMap 的 pin 保持
/// 专业制图的 POI pin 选点：rank ≥ 2 的地名**归地物符号层**（带尺寸档的
/// map-pin），POI 层必须让位——否则同一座城市会叠两颗 pin（2026-10-26 实机
/// 截图发现：城堡插画与分类 pin 同时压在城址上）。
fn poi_pin_owner_is_landmark(rank: u8) -> bool {
    rank >= 2
}

#[allow(clippy::too_many_arguments)]
fn sync_landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    landmark: &mut [LandmarkPool; wv_core::distance::PoiKind::COUNT],
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    view_scale: f32,
    z: f32,
    material: Handle<ColorMaterial>,
) {
    // 尺寸口径由派发处一次算定（局部坐标 = 屏幕像素，只乘 `view_scale`）：
    // 合批函数只拿最终的 `s`，不再自己推。
    let s = landmark_size_tier(view_scale) * view_scale;
    for (i, kind) in wv_core::distance::PoiKind::ALL.iter().copied().enumerate() {
        sync_landmark_mesh_batches(
            commands,
            meshes,
            &mut landmark[i],
            frame_landmarks,
            kind,
            s,
            z,
            material.clone(),
        );
    }
}

/// 地物符号的 LOD 尺寸微调：档位越近稍大（Nation 1.0 / Region 1.25 /
/// Edit 1.5），贴近 Google Maps marker 的恒定手感。独立成函数供回归测试
/// 推导「世界高 ∝ tier × scale」的精确期望值。
fn landmark_size_tier(view_scale: f32) -> f32 {
    match lod_tier(view_scale) {
        crate::state::LodTier::Nation => 1.0,
        crate::state::LodTier::Region => 1.25,
        crate::state::LodTier::Edit => 1.5,
    }
}

/// 地物符号合批的复用 key（纯函数，测试锚定）。
///
/// 几何 = f(位置, kind, 形态, s)，故 key 必须吃全四样：
/// - `s` 进 `world_thickness`——否则缩放后位置指纹不变、mesh 不重建，
///   符号保持旧世界尺寸随相机缩放（2026-10-24 CellMap 实测：455% 下
///   pin 放大 ~4.5 倍）。与 POI 的 `radius` 进 `PoiBatchKey` 同一条纪律。
/// - **形态（pin）必须混进指纹**——切 Browse ↔ Topographic 翻转 pin，
///   位置与 `s` 都不变时旧 key 仍命中池子，mesh 维持旧形态：实机表现
///   即「进 CellMap 还是城堡，缩放一下才变 pin」（2026-10-25 验收发现）。
///   混入用黄金比例奇常数：pin=false 不变，pin=true 加一个充分离散的
///   非零 delta，避免只翻指纹低 1 位的意外相撞。
fn landmark_batch_key(
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    kind: wv_core::distance::PoiKind,
    s: f32,
    z: f32,
) -> StrokeBatchKey {
    StrokeBatchKey {
        fingerprint: points_fingerprint_dots(frame_landmarks, kind),
        world_thickness: s,
        z,
    }
}

/// 单类符号的顶点色 mesh 合批（从 sync_landmarks 拆出的唯一绘制路径）。
#[allow(clippy::too_many_arguments)]
fn sync_landmark_mesh_batches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    pool: &mut LandmarkPool,
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    kind: wv_core::distance::PoiKind,
    s: f32,
    z: f32,
    material: Handle<ColorMaterial>,
) {
    use crate::landmark_symbols::{build_landmark_mesh, LandmarkBatch};
    // 尺寸 `s` 由 [`sync_landmarks`] 统一算定（**屏幕像素恒定** + LOD 档微调）。
    // 符号局部坐标单位就是屏幕像素，缩放系数只乘 `view_scale`——2026-10-23
    // 实测教训：曾在本地坐标外再乘一档画布基准（26），符号被放大成墨团。
    // ADR-0093：图标钉 = 分类色钉身（选中金）+ 白色含义图形；图形查表失败
    // 回退白芯 pin（生成表与枚举失步时的静默兜底，V1 不阻断）。
    let glyph = crate::glyphs::glyph_of(kind);
    let body_color = poi_kind_color(kind);
    let mut batch = LandmarkBatch::new();
    for d in frame_landmarks {
        if d.poi == kind {
            let body = if d.selected {
                crate::landmark_symbols::SELECTED_PIN_BODY
            } else {
                {
                    let c = body_color.to_srgba();
                    [c.red, c.green, c.blue, 1.0]
                }
            };
            match glyph {
                Some(g) => batch.push_icon_pin(g, d.pos, s, body),
                None => batch.push_map_pin_tinted(d.pos, s, body),
            }
        }
    }
    let key = landmark_batch_key(frame_landmarks, kind, s, z);
    match pool.batch.take() {
        None => {
            let Some(mesh) = build_landmark_mesh(batch) else {
                return;
            };
            let mesh_handle = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Mesh2d(mesh_handle.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0.0, 0.0, z),
                ))
                .id();
            pool.batch = Some(StrokeBatch {
                entity,
                mesh: mesh_handle,
                key,
            });
        }
        Some(mut existing) => {
            let Some(new_mesh) = build_landmark_mesh(batch) else {
                commands.entity(existing.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&existing.mesh);
                });
                return;
            };
            if existing.key != key {
                if let Some(mut m) = meshes.get_mut(&existing.mesh) {
                    *m = new_mesh;
                }
                commands
                    .entity(existing.entity)
                    .insert(Transform::from_xyz(0.0, 0.0, z));
                existing.key = key;
            }
            pool.batch = Some(existing);
        }
    }
}

/// 与 `segments_fingerprint` 同款，但用 `LandmarkDot` 字段做内容指纹。
fn points_fingerprint_dots(
    dots: &[crate::world_projection::LandmarkDot],
    kind: wv_core::distance::PoiKind,
) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for d in dots {
        if d.poi != kind {
            continue;
        }
        for v in [d.pos.x, d.pos.y] {
            hash ^= u64::from(v.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // 选中态入指纹（[ADR-0074] 追记三 followup）：选中/取消选中必须
        // 触发重批——否则金色钉要等位置变化（拖动）才换上（2026-10-28 实机）。
        if d.selected {
            hash ^= 0xFF00_FF00_FF00_FF00;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 首次生成一个 POI 桶的合批实体。
#[allow(clippy::too_many_arguments)]
fn spawn_poi_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[Vec2],
    key: PoiBatchKey,
    radii: (f32, f32),
    material: Handle<ColorMaterial>,
    pool: &mut PoiBatchPool,
    build: impl Fn(&[Vec2], f32, f32) -> Mesh,
) {
    let mesh = meshes.add(build(dots, radii.0, radii.1));
    let entity = commands
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, key.z),
        ))
        .id();
    pool.batch = Some(PoiBatch { entity, mesh, key });
}

#[cfg(test)]
mod tests {
    /// **类别必须换 key**（ADR-0093）：同位置不同 `poi` 的地标分属不同
    /// 池子——指纹按类别过滤后（空集 vs 非空集）天然不同，钉身配色随之。
    #[test]
    fn landmark_batch_key_changes_when_poi_kind_differs() {
        use crate::world_projection::LandmarkDot;
        let dots = vec![LandmarkDot {
            pos: Vec2::new(10.0, 20.0),
            poi: PoiKind::Settlement,
            selected: false,
        }];
        let settlement = landmark_batch_key(&dots, PoiKind::Settlement, 1.0, 5.0);
        let food = landmark_batch_key(&dots, PoiKind::Food, 1.0, 5.0);
        assert_ne!(
            settlement.fingerprint, food.fingerprint,
            "类别必须分池（钉身配色不同）"
        );
        // 位置 / 尺寸 / z 的既有纪律不回归：任一变化都换 key。
        let mut moved = dots.clone();
        moved[0].pos = Vec2::new(11.0, 20.0);
        assert_ne!(
            settlement.fingerprint,
            landmark_batch_key(&moved, PoiKind::Settlement, 1.0, 5.0).fingerprint
        );
        assert_ne!(
            settlement.world_thickness,
            landmark_batch_key(&dots, PoiKind::Settlement, 2.0, 5.0).world_thickness
        );
    }
    /// [ADR-0074 追记三 followup] selected 翻转必须改 batch key——否则金色
    /// 选中钉要等位置变化才出现（2026-10-28 实机症状的回归守点）。
    #[test]
    fn landmark_batch_key_changes_when_selection_flips() {
        use crate::world_projection::LandmarkDot;
        let mk = |selected: bool| LandmarkDot {
            pos: Vec2::new(10.0, 20.0),
            poi: PoiKind::Settlement,
            selected,
        };
        let dots_plain = vec![mk(false)];
        let dots_selected = vec![mk(true)];
        let a = landmark_batch_key(&dots_plain, PoiKind::Settlement, 1.0, 0.0);
        let b = landmark_batch_key(&dots_selected, PoiKind::Settlement, 1.0, 0.0);
        assert_ne!(a, b, "选中翻转应改变 batch key");
        // 位置也要参与（既有语义不回退）。
        let mut moved = dots_selected.clone();
        moved[0].pos = Vec2::new(99.0, 99.0);
        assert_ne!(
            b,
            landmark_batch_key(&moved, PoiKind::Settlement, 1.0, 0.0),
            "位置变化仍须改 key"
        );
    }

    /// 画法分流（2026-10-26 追记三）：数据视图与普通地图都是 pin，艺术主题
    /// 才画城堡 / 小镇插画。
    #[test]
    fn cartographic_and_data_views_mark_places_with_pins() {
        use crate::state::ViewMode;
        assert_eq!(
            landmark_marker(ViewMode::GoogleMap),
            LandmarkMarker::Pin,
            "专业制图不许再画手绘城堡"
        );
        assert_eq!(landmark_marker(ViewMode::Topographic), LandmarkMarker::Pin);
        assert_eq!(
            landmark_marker(ViewMode::VectorFlat),
            LandmarkMarker::Illustration
        );
    }

    /// POI 让位规则：rank ≥ 2 归地物符号层。Region 档恰好只放行 rank ≥ 2，
    /// 故该档下分类 pin 全数让位（远观只看得见城市），Edit 档才轮到分类符号。
    #[test]
    fn poi_pins_cede_rank_two_and_up_to_landmarks() {
        use crate::state::LodTier;
        assert!(!poi_pin_owner_is_landmark(0), "普通点仍归 POI 层");
        assert!(!poi_pin_owner_is_landmark(1));
        assert!(poi_pin_owner_is_landmark(2), "市镇级归地物符号层");
        assert!(poi_pin_owner_is_landmark(3), "国家级归地物符号层");

        let dot = |rank: u8, x: f32| PlaceDot {
            pos: Vec2::new(x, 0.0),
            is_city: true,
            rank,
            name_len: 3,
            kind: wv_core::distance::PoiKind::Settlement,
        };
        // 拉开距离，避开 96px 网格配额。
        let dots = vec![dot(3, 0.0), dot(2, 500.0), dot(1, 1000.0)];
        let region: Vec<&PlaceDot> = select_pois(&dots, LodTier::Region, 1.0);
        assert!(
            region.iter().any(|d| d.rank == 2),
            "Region 档应放行 rank 2，实得 {region:?}"
        );
        assert!(
            region.iter().all(|d| poi_pin_owner_is_landmark(d.rank)),
            "Region 档放行的每一点都应让位给地物符号层"
        );
        let edit = select_pois(&dots, LodTier::Edit, 1.0);
        assert!(
            edit.iter().any(|d| d.rank == 1),
            "Edit 档应出现分类 pin，实得 {edit:?}"
        );
    }

    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

    /// Google 式道路必须有灰 casing + 白 fill 双层，粗细差即路缘。
    ///
    /// 2026-10-25 道路分级后改为**逐档**校验：三档都必须 casing > fill，
    /// 且档位越高线越粗（否则「放大才看得见小路」的层次会倒过来）。
    #[test]
    fn google_road_casing_is_thicker_than_fill_for_every_grade() {
        for grade in 0..ROAD_GRADES {
            let (casing_px, fill_px) = road_grade_px(crate::state::ViewMode::GoogleMap, grade);
            let casing = stroke_world_thickness(casing_px, 1.0);
            let fill = stroke_world_thickness(fill_px, 1.0);
            assert!(
                casing > fill,
                "第 {grade} 档 casing 应厚于 fill：{casing_px}px 应 > {fill_px}px"
            );
            if grade + 1 < ROAD_GRADES {
                let (_, next_fill) = road_grade_px(crate::state::ViewMode::GoogleMap, grade + 1);
                assert!(
                    fill_px > next_fill,
                    "档位越高线越粗：第 {grade} 档 {fill_px}px 应 > 第 {} 档 {next_fill}px",
                    grade + 1
                );
            }
        }
    }

    /// 三档颜色必须互不相同——全同色等于没分级（这正是改造前的状态）。
    #[test]
    fn google_road_grades_have_distinct_fills() {
        let pal = palette::colors(MapPalette::default());
        let grades = road_grade_colors(crate::state::ViewMode::GoogleMap, pal);
        let a = grades[0].1.to_srgba();
        let b = grades[1].1.to_srgba();
        let c = grades[2].1.to_srgba();
        assert_ne!(a, b, "干线 / 干道填充色不能相同");
        assert_ne!(b, c, "干道 / 支路填充色不能相同");
    }

    /// 三档切片：`rank ≥ 3` / `= 2` / `= 1` 在排序后的段列里是连续前缀区间。
    #[test]
    fn road_grade_spans_split_prefix_by_rank() {
        // 段列按 rank 降序：3,3,2,1,1 → cuts = [5(Edit 全量), 3(≥2), 2(≥3)]
        let cuts = [5usize, 3, 2];
        let edit = road_grade_spans(crate::state::LodTier::Edit, cuts);
        assert_eq!(edit, [(0, 2), (2, 3), (3, 5)], "Edit 三档都要有");
        let region = road_grade_spans(crate::state::LodTier::Region, cuts);
        assert_eq!(region, [(0, 2), (2, 3), (3, 3)], "Region 不含 rank=1");
        let nation = road_grade_spans(crate::state::LodTier::Nation, cuts);
        assert_eq!(nation, [(0, 2), (2, 2), (2, 2)], "Nation 只有干线");
    }

    /// 街道与道路同口径：每档都必须有 casing，粗细差即路缘；且 Z 序要保证 casing
    /// 既不被底图（Z=0.0）盖掉、也不越过自己那档 fill——地图模式下 fill 是白色，
    /// 落在近白底图上唯一看得见的就是这层浅灰 casing。
    #[test]
    fn street_casing_is_thicker_than_its_fill_and_keeps_the_z_order() {
        for (tier, fill_px, fill_z, casing_z) in [
            (
                "主干",
                STREET_MAJOR_PX,
                STREET_MAJOR_Z,
                STREET_MAJOR_CASING_Z,
            ),
            (
                "支路",
                STREET_MINOR_PX,
                STREET_MINOR_Z,
                STREET_MINOR_CASING_Z,
            ),
        ] {
            let casing = stroke_world_thickness(fill_px + STREET_CASING_EXTRA_PX, 1.0);
            let fill = stroke_world_thickness(fill_px, 1.0);
            assert!(
                casing > fill,
                "{tier} 街道 casing 应厚于 fill：casing={} fill={}，期望 {}px > {}px",
                casing,
                fill,
                fill_px + STREET_CASING_EXTRA_PX,
                fill_px
            );
            assert!(
                casing_z < fill_z,
                "{tier} 街道 casing 的 Z（{casing_z}）必须低于自己那档 fill（{fill_z}）"
            );
        }
        // 全序（底图 0.0 < 道路 fill < 主干 casing < …）由模块顶部的
        // `const _: () = assert!(…)` 在**编译期**把关，这里不再重复。
    }

    /// 合批 mesh 的四个角必须以线段两端为「端帽中点」：p0+p3 == 2a，p1+p2 == 2b。
    /// 水平段与斜段各验一遍（斜段防「忘了旋转矩形」的回归）。
    #[test]
    fn batch_mesh_caps_center_on_the_segment_endpoints() {
        let cases = [
            (Vec2::new(100.0, 200.0), Vec2::new(300.0, 200.0)), // 水平
            (Vec2::ZERO, Vec2::new(3.0, 4.0)),                  // 3-4-5 斜段
        ];
        for (a, b) in cases {
            let mesh = build_stroke_batch_mesh(&[(a, b)], 2.0);
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("合批 mesh 必有 Float32x3 POSITION");
            };
            assert_eq!(positions.len(), 4, "一段 = 4 顶点");
            let v = |i: usize| Vec2::new(positions[i][0], positions[i][1]);
            assert!(
                (v(0) + v(3) - a * 2.0).length() < 1e-4,
                "起端帽中点应落在 a={a:?}，实得 {:?}",
                (v(0) + v(3)) * 0.5
            );
            assert!(
                (v(1) + v(2) - b * 2.0).length() < 1e-4,
                "末端帽中点应落在 b={b:?}，实得 {:?}",
                (v(1) + v(2)) * 0.5
            );
        }
    }

    // ---- 接头（G3b，2026-10-23）：交汇处补圆盘，填掉 butt 端的外角缺口 ----

    fn batch_positions(mesh: &Mesh) -> Vec<Vec2> {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        positions.iter().map(|p| Vec2::new(p[0], p[1])).collect()
    }

    /// 两段共用端点 → 该端点补一块圆盘（1 中心 + 6 边），外角缺口由此填平。
    /// 断言之眼：顶点数 = 2 段 × 4 + 7，且圆心精确落在共用端点上。
    #[test]
    fn shared_endpoint_gets_a_join_disc() {
        let corner = Vec2::new(10.0, 0.0);
        let segs = [
            (Vec2::new(0.0, 0.0), corner),
            (corner, Vec2::new(10.0, 10.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        let pos = batch_positions(&mesh);
        assert_eq!(
            pos.len(),
            2 * 4 + 1 + STROKE_JOIN_SEGMENTS,
            "共用端点应补一块 {STROKE_JOIN_SEGMENTS} 边形圆盘"
        );
        assert!(
            pos.iter().any(|p| (*p - corner).length() < 1e-5),
            "圆盘中心必须是共用端点 {corner:?}"
        );
        // 半径 = 半线宽：只看圆盘自己的顶点（前 8 个是两段的四边形角点，
        // 它们的端帽角到 corner 的距离也正好是半线宽，不能混进来数）。
        let disc = &pos[2 * 4..];
        assert_eq!(disc.len(), 1 + STROKE_JOIN_SEGMENTS, "圆盘 = 圆心 + 环");
        assert!((disc[0] - corner).length() < 1e-5, "disc[0] 是圆心");
        let ring = disc[1..]
            .iter()
            .filter(|p| ((**p - corner).length() - 1.0).abs() < 1e-5)
            .count();
        assert_eq!(ring, STROKE_JOIN_SEGMENTS, "环半径应等于半线宽 1.0");
    }

    /// 互不相接的两段**不得**长接头：否则每个端点都白多一块圆盘。
    #[test]
    fn disjoint_segments_get_no_join_disc() {
        let segs = [
            (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
            (Vec2::new(20.0, 0.0), Vec2::new(30.0, 0.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        assert_eq!(batch_positions(&mesh).len(), 2 * 4, "无共用端点 → 无圆盘");
    }

    /// 折线（3 点 2 段）只有一个拐点 → 只补一块圆盘；顺带钉死「不按点数平方补」。
    #[test]
    fn polyline_corner_gets_exactly_one_join_disc() {
        let p = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(20.0, 5.0),
        ];
        let segs = [(p[0], p[1]), (p[1], p[2])];
        let mesh = build_stroke_batch_mesh(&segs, 4.0);
        assert_eq!(
            batch_positions(&mesh).len(),
            2 * 4 + 1 + STROKE_JOIN_SEGMENTS,
            "唯一拐点只补一块圆盘"
        );
    }

    /// **行为断言**（不只是数顶点）：直角拐点的**外角**必须在网格覆盖范围内。
    ///
    /// 几何事实：厚 2 的两段在 (10,0) 直角相接，水平段的 butt 端止于 x=10，
    /// 竖直段止于 y=0 —— 右下外角 `(10.6, -0.6)` 落在两段四边形之外（旧实现的缺口）。
    /// 补圆盘（半径 1）后该点距圆心 0.849 < 1，必须被覆盖。
    #[test]
    fn join_disc_covers_the_outer_notch_at_a_right_angle() {
        let corner = Vec2::new(10.0, 0.0);
        let segs = [
            (Vec2::new(0.0, 0.0), corner),
            (corner, Vec2::new(10.0, 10.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        assert!(
            mesh_covers(&mesh, Vec2::new(10.6, -0.6)),
            "直角外角的楔形缺口必须被接头圆盘填掉"
        );
        // 对照：圆盘半径外的点不该被覆盖（防止「整块糊上去」式修法）。
        assert!(
            !mesh_covers(&mesh, Vec2::new(11.6, -1.6)),
            "半径之外不应有几何——否则是过度填充"
        );
    }

    /// 点在网格（三角形并集）内判定：逐三角形重心坐标同号。
    fn mesh_covers(mesh: &Mesh, p: Vec2) -> bool {
        let pos = batch_positions(mesh);
        let Some(Indices::U32(idx)) = mesh.indices() else {
            panic!("合批 mesh 必有 U32 索引");
        };
        idx.chunks(3).any(|t| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            let s = |u: Vec2, v: Vec2| (v.x - u.x) * (p.y - u.y) - (v.y - u.y) * (p.x - u.x);
            let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
            let has_neg = d1 < -1e-6 || d2 < -1e-6 || d3 < -1e-6;
            let has_pos = d1 > 1e-6 || d2 > 1e-6 || d3 > 1e-6;
            !(has_neg && has_pos)
        })
    }

    /// 接头是 **O(n) 增量、只在重建时发生**：guangdong 规模（17.4 万段网格路网，
    /// 共享端点极密）补完接头后，顶点数严格等于 `4n + 7×接头数`，构建仍在亚秒级。
    /// 稳态每帧不重建（`StrokeBatchKey` 用指纹/线宽/Z 判变），所以这条预算守的是
    /// 「重建一次的代价」，不是帧时间。
    #[test]
    fn join_fill_stays_linear_at_guangdong_scale() {
        let mut segs: Vec<(Vec2, Vec2)> = Vec::with_capacity(295 * 295 * 2);
        for i in 0..295u32 {
            for j in 0..295u32 {
                let a = Vec2::new(i as f32 * 8.0, j as f32 * 8.0);
                segs.push((a, a + Vec2::new(8.0, 0.0)));
                segs.push((a, a + Vec2::new(0.0, 8.0)));
            }
        }
        let joins = stroke_join_centers(&segs, 1.0).len();
        assert!(
            joins * 2 >= segs.len(),
            "网格路网应有大量共享端点，实得 {joins} / {} 段",
            segs.len()
        );
        let t0 = std::time::Instant::now();
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        let dt = t0.elapsed();
        assert_eq!(
            batch_positions(&mesh).len(),
            segs.len() * 4 + joins * (1 + STROKE_JOIN_SEGMENTS),
            "顶点数必须严格 = 4n + 7×接头数（无隐性重复）"
        );
        // 预算 1s：debug 构建实测百毫秒级；守的是「不退化到秒级」。
        assert!(
            dt.as_secs_f32() < 1.0,
            "17.4 万段补接头应在亚秒级，实得 {dt:?}"
        );
    }

    /// 线宽公式：`world_thickness = stroke_px * view.scale`，屏幕宽度恒定
    /// （`world_thickness / view.scale == stroke_px`），不随缩放变化。
    #[test]
    fn stroke_screen_width_stays_constant_across_zoom() {
        for scale in [0.1, 1.0, 8.0] {
            let world_thickness = stroke_world_thickness(3.0, scale);
            let screen_px = world_thickness / scale;
            assert!(
                (screen_px - 3.0).abs() < 1e-4,
                "scale={scale}: 屏幕宽度应恒为 3px，实得 {screen_px}px"
            );
        }
    }

    #[test]
    fn batch_mesh_handles_zero_length_segment_without_nan() {
        let mesh = build_stroke_batch_mesh(
            &[(Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0))],
            GOOGLE_ROAD_FILL_PX,
        );
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        assert!(positions.iter().all(|p| p.iter().all(|c| c.is_finite())));
    }

    /// N 段 = 4N 顶点 / 6N 索引：合批不是丢段。
    #[test]
    fn batch_mesh_carries_one_quad_per_segment_and_one_disc_per_join() {
        let segs = [
            (Vec2::ZERO, Vec2::X * 10.0),
            (Vec2::X * 10.0, Vec2::new(15.0, 3.0)),
            (Vec2::new(15.0, 3.0), Vec2::new(-4.0, 8.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 1.5);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        // G3b 之后：每段一个四边形（4 顶点 / 6 索引），每处交汇（度数 ≥2 的端点）
        // 再加一块圆盘（1 + 6 顶点 / 18 索引）。这条折线有 2 处交汇。
        let joins = 2;
        assert_eq!(
            positions.len(),
            segs.len() * 4 + joins * (1 + STROKE_JOIN_SEGMENTS),
            "{} 段 = 12 顶点 + {joins} 处接头 × {}",
            segs.len(),
            1 + STROKE_JOIN_SEGMENTS
        );
        match mesh.indices() {
            Some(Indices::U32(idx)) => assert_eq!(
                idx.len(),
                segs.len() * 6 + joins * STROKE_JOIN_SEGMENTS * 3,
                "{} 段 = 18 索引 + {joins} 处接头 × {}",
                segs.len(),
                STROKE_JOIN_SEGMENTS * 3
            ),
            other => panic!("应为 U32 索引，实得 {other:?}"),
        }
    }

    /// 呈现门槛端到端（ADR-0030 D2/D3）：Nation 档只放行 rank 3；全局上限
    /// 仍按优先级（rank > is_city > 名字长）截断。
    ///
    /// 位置分散到不同屏幕格（x 相隔 200 > 96），避免 ADR-0064 的网格配额
    /// 干扰本测试要钉的两件事。
    #[test]
    fn select_pois_gates_by_rank_then_caps_by_priority() {
        use crate::state::LodTier;
        let dot = |pos: Vec2, rank: u8, is_city: bool, name_len: u32| PlaceDot {
            pos,
            is_city,
            rank,
            name_len,
            kind: wv_core::distance::PoiKind::Other,
        };
        let dots = vec![
            dot(Vec2::new(0.0, 0.0), 1, false, 4), // 普通点：Nation 档出局
            dot(Vec2::new(200.0, 0.0), 3, false, 6), // 国家级
            dot(Vec2::new(400.0, 0.0), 2, false, 99), // 市镇级
            dot(Vec2::new(600.0, 0.0), 3, true, 2), // 城市级材质的国家级
        ];
        let nation: Vec<&PlaceDot> = select_pois(&dots, LodTier::Nation, 1.0);
        assert_eq!(nation.len(), 2, "Nation 档只放行 rank 3，实得 {nation:?}");
        assert!(nation.iter().all(|d| d.rank == 3));
        // 上限截断：rank 优先保 3，其次 is_city，再次名字长。
        // 450 个点各占一格（x 步进 120 > 96），配额不参与，只测全局上限。
        let many: Vec<PlaceDot> = (0..(POI_MAX_DOTS + 50))
            .map(|i| dot(Vec2::new(i as f32 * 120.0, 0.0), 2, false, i as u32))
            .collect();
        let capped = select_pois(&many, LodTier::Edit, 1.0);
        assert_eq!(capped.len(), POI_MAX_DOTS, "超预算必须截断到上限");
        let min_kept = capped.iter().map(|d| d.name_len).min();
        assert_eq!(
            min_kept,
            Some(50),
            "450 个候选截到 400，保的是名字最长的 400 个（最短 50）"
        );
    }

    /// 分类色必须互不相同（否则「分类符号」等于没分），未分类落默认红点。
    #[test]
    fn poi_kind_colors_are_distinct_for_classified_categories() {
        use PoiKind::*;
        let classified = [
            Food, Lodging, Medical, Scenic, Station, Government, Education, Shop, Ruins, Arena,
            Shrine, Portal, Danger,
        ];
        let mut seen: Vec<Color> = Vec::new();
        for k in classified {
            let c = poi_kind_color(k);
            assert!(!seen.contains(&c), "{k:?} 与已用类别色重复");
            seen.push(c);
        }
        assert_eq!(
            poi_kind_color(Other),
            GOOGLE_PLACE_DOT,
            "未分类回退默认红点"
        );
        assert_eq!(
            poi_kind_color(Settlement),
            GOOGLE_PLACE_DOT,
            "聚落沿用红点（城市与未分类同色，靠字号/级别区分）"
        );
        assert_eq!(poi_kind_color(Peak), poi_kind_color(Scenic), "山峰借景点绿");
        assert_eq!(
            poi_kind_color(Port),
            poi_kind_color(Station),
            "港口并入交通蓝"
        );
    }

    /// 屏幕网格配额（ADR-0064 D2）：同一格内只留配额数，且保留的是高优先级；
    /// 稀疏区域的点不受密集区影响。
    ///
    /// 这是「像检索地图」的核心：没有它，全局排序截断会让城区吃掉所有名额。
    #[test]
    fn dense_cluster_keeps_quota_per_screen_cell_and_spares_sparse_areas() {
        use crate::state::LodTier;
        let dot = |pos: Vec2, rank: u8, name_len: u32| PlaceDot {
            pos,
            is_city: false,
            rank,
            name_len,
            kind: wv_core::distance::PoiKind::Other,
        };
        // 同一个 96px 格内 10 个点（rank 从 1 到 3 混排）+ 远处 1 个点。
        let mut dots: Vec<PlaceDot> = (0..10)
            .map(|i| {
                dot(
                    Vec2::new(i as f32 * 2.0, 0.0),
                    if i == 9 { 3 } else { 1 },
                    i,
                )
            })
            .collect();
        dots.push(dot(Vec2::new(500.0, 0.0), 2, 7));

        let nation = select_pois(&dots, LodTier::Nation, 1.0);
        assert_eq!(
            nation.len(),
            1,
            "Nation 档只剩 rank 3 那一个，got {nation:?}"
        );

        let region = select_pois(&dots, LodTier::Region, 1.0);
        assert_eq!(
            region.len(),
            2,
            "Region 档：密集格留 1 个 + 远处 1 个（稀疏区不被挤掉），got {region:?}"
        );
        assert!(
            region.iter().any(|d| d.pos.x == 500.0),
            "远处稀疏点必须保留"
        );

        let edit = select_pois(&dots, LodTier::Edit, 1.0);
        assert_eq!(
            edit.len(),
            3,
            "Edit 档：密集格留 2 个 + 远处 1 个，got {edit:?}"
        );
        // 密集格保留的是 rank 最高 / 名字最长的两个（rank 3 与 name_len 9）。
        let kept_in_cluster: Vec<u32> = edit
            .iter()
            .filter(|d| d.pos.x < 100.0)
            .map(|d| d.name_len)
            .collect();
        assert_eq!(
            kept_in_cluster,
            vec![9, 8],
            "格内按优先级保留：{kept_in_cluster:?}"
        );
    }

    /// POI 点集指纹：坐标位型变化才变，顺序无关性不要求（合批列表本身有序）。
    #[test]
    fn dots_fingerprint_changes_with_positions_only() {
        let a = vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)];
        let b = vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.5)];
        assert_ne!(dots_fingerprint(&a), dots_fingerprint(&b), "坐标变指纹必变");
        assert_eq!(
            dots_fingerprint(&a),
            dots_fingerprint(&a.clone()),
            "同集合指纹稳定"
        );
    }

    /// 环带合批的顶点账：N 点 = 24N 顶点 / 72N 索引（12 段共享内外圈）。
    #[test]
    fn poi_ring_batch_carries_twelve_segments_per_dot() {
        let dots = [Vec2::ZERO, Vec2::new(50.0, 20.0), Vec2::new(-30.0, 8.0)];
        let mesh = build_poi_ring_batch_mesh(&dots, 3.0, 5.0);
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("环带合批 mesh 必有 Float32x3 POSITION");
        };
        assert_eq!(positions.len(), dots.len() * 24, "每点 12 段 × 2 圈顶点");
        match mesh.indices() {
            Some(bevy::render::mesh::Indices::U32(idx)) => {
                assert_eq!(idx.len(), dots.len() * 72, "每点 12 段 × 6 索引")
            }
            other => panic!("应为 U32 索引，实得 {other:?}"),
        }
        // 外圈顶点半径必须是 outer_radius（角 0 处的外圈顶点 = 下标 1）。
        assert!(
            (positions[1][0] - 5.0).abs() < 1e-5 && positions[1][1].abs() < 1e-5,
            "外圈顶点应落在 outer_radius 上，实得 {:?}",
            positions[1]
        );
    }

    /// 内芯合批的顶点账：N 点 = 13N 顶点 / 36N 索引（中心 + 12 圈）。
    #[test]
    fn poi_dot_batch_carries_center_fan_per_dot() {
        let dots = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
        let mesh = build_poi_dot_batch_mesh(&dots, 2.5);
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("内芯合批 mesh 必有 Float32x3 POSITION");
        };
        assert_eq!(positions.len(), dots.len() * 13, "每点中心 + 12 圈顶点");
        match mesh.indices() {
            Some(bevy::render::mesh::Indices::U32(idx)) => {
                assert_eq!(idx.len(), dots.len() * 36, "每点 12 扇区 × 3 索引")
            }
            other => panic!("应为 U32 索引，实得 {other:?}"),
        }
    }

    /// 线宽量化（ADR-0030 D5b）：漂移 ≤4% 视为未变，>4% 才触发重建。
    #[test]
    fn thickness_quantization_tolerates_four_percent_drift() {
        assert!(!thickness_drifted(6.0, 6.0));
        assert!(!thickness_drifted(6.2, 6.0), "3.3% 漂移在容差内");
        assert!(thickness_drifted(6.3, 6.0), "5% 漂移必须重建");
        assert!(thickness_drifted(5.0, 6.0), "缩小的漂移同样计入");
    }

    /// 同一投影连续两帧不应继续新增 Mesh / Material 资产。
    #[test]
    fn vector_sync_does_not_accumulate_assets_when_projection_is_unchanged() {
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            vec![(3, Vec2::ZERO, Vec2::X * 100.0)],
            vec![PlaceDot {
                kind: wv_core::distance::PoiKind::Other,
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
        ));
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.insert_resource(flat_recipe());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        let material_count = world.resource::<Assets<ColorMaterial>>().iter().count();

        schedule.run(&mut world);
        let next_mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        let next_material_count = world.resource::<Assets<ColorMaterial>>().iter().count();
        assert_eq!(
            next_mesh_count, mesh_count,
            "第二帧 Mesh 资产应稳定：first={mesh_count} second={next_mesh_count}"
        );
        assert_eq!(
            next_material_count, material_count,
            "第二帧 Material 资产应稳定：first={material_count} second={next_material_count}"
        );
    }

    /// 门控（2026-10-22 内容矩阵 T4，**2026-10-28 修订**）：Topographic 下
    /// **Roads 开**（用户裁定②：CellMap 要看道路和河流——改地形要有线网
    /// 参照，修订 ADR-0039 关灯表）⇒ 道路产 mesh；Streets/Poi 仍 false ⇒
    /// 街网/POI 标签不产 mesh，地物符号只留 landmark pin 合批。
    #[test]
    fn topographic_recipe_builds_road_but_no_street_or_poi_meshes() {
        use crate::recipe::resolve_render_recipe;
        use crate::state::ViewMode;

        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        let view = ViewState {
            mode: ViewMode::Topographic,
            ..Default::default()
        };
        let layers = crate::layer_frame::LayerFrame::default();
        // 真实 recipe：按 Topographic 行解析（非全关 init_resource——那测不出
        // 「接了线」还是「恰好没画」）。view 已在上面对象化，直接借用。
        let recipe = resolve_render_recipe(
            &view,
            &layers,
            &crate::state::WorldProjection::default(),
            &crate::state::DrillState::default(),
            &crate::terrain_view::terrain_texture::TerrainTexture {
                handle: bevy::asset::Handle::default(),
                size: 4,
                height: 4,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "",
                last_hypsometric: 0.0,
                last_map: None,
            },
            None,
        );
        world.insert_resource(view);
        world.insert_resource(layers);
        world.insert_resource(recipe);
        world.insert_resource(GridConfig::default());
        let mut frame = projection_frame_with_roads(
            vec![(3, Vec2::ZERO, Vec2::X * 100.0)],
            vec![PlaceDot {
                kind: wv_core::distance::PoiKind::Other,
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
        );
        // 地物符号给了数据：CellMap 里以 **map-pin** 形式在场（2026-10-23
        // 二轮裁定：pin 只用于 CellMap），道路/街网/POI 仍不得产 mesh。
        frame
            .landmark_dots
            .push(crate::world_projection::LandmarkDot {
                pos: Vec2::new(30.0, 30.0),
                poi: PoiKind::Settlement,
                selected: false,
            });
        world.insert_resource(frame);
        // 街道缓存**有数据**——门控必须让它在 Topographic 下画不出来。
        world.insert_resource(crate::streets::StreetLayerCache {
            sources: vec![(
                "test".to_string(),
                wv_core::StreetLayer {
                    segments: vec![wv_core::StreetSegment::straight(
                        wv_core::source::street::RoadClass::Residential,
                        0.0,
                        0.0,
                        50.0,
                        50.0,
                    )],
                },
            )],
            ..Default::default()
        });
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetEditorState>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        // 道路天生 casing + fill 双层（文件头「道路 casing/fill」——双层是
        // 「粗细差即路缘」的口径，非 Google 主题也是双层），故 3 = 路 casing
        // + 路 fill + pin。20f48d8 写「道路 1 条 mesh」数漏了一层；本测试
        // 当时被 20 处编译债挡着从未运行，2026-10-29 清债首跑即暴露。
        assert_eq!(
            mesh_count, 3,
            "Topographic 下道路 casing+fill 2 条 mesh + 地物符号 1 个 map-pin 合批 \
             （2026-10-28 裁定②后 Roads 开灯；街网/POI 标签仍不得产 mesh）"
        );
    }

    /// 对照组（防「全关假绿」）：同一夹具切 VectorFlat（三 pass 全 true）
    /// ⇒ mesh 必须产出来。证明上一条的 0 是门控起效，不是夹具坏了。
    /// （原用 Schematic 作对照；该模式已退役——ADR-0069。）
    #[test]
    fn flat_recipe_still_builds_road_street_and_poi_meshes() {
        use crate::recipe::resolve_render_recipe;
        use crate::state::ViewMode;

        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        let view = ViewState {
            mode: ViewMode::VectorFlat,
            ..Default::default()
        };
        let layers = crate::layer_frame::LayerFrame::default();
        let recipe = resolve_render_recipe(
            &view,
            &layers,
            &crate::state::WorldProjection::default(),
            &crate::state::DrillState::default(),
            &crate::terrain_view::terrain_texture::TerrainTexture {
                handle: bevy::asset::Handle::default(),
                size: 4,
                height: 4,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "",
                last_hypsometric: 0.0,
                last_map: None,
            },
            None,
        );
        world.insert_resource(view);
        world.insert_resource(layers);
        world.insert_resource(recipe);
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            vec![(3, Vec2::ZERO, Vec2::X * 100.0)],
            vec![PlaceDot {
                kind: wv_core::distance::PoiKind::Other,
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
        ));
        let cache = crate::streets::StreetLayerCache {
            map_id: None,
            sources: vec![(
                "test".to_string(),
                wv_core::StreetLayer {
                    segments: vec![wv_core::StreetSegment::straight(
                        wv_core::source::street::RoadClass::Residential,
                        0.0,
                        0.0,
                        50.0,
                        50.0,
                    )],
                },
            )],
        };
        world.insert_resource(cache);
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetEditorState>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert!(
            mesh_count > 0,
            "Schematic 下道路/街网/POI 必须照画（对照组）"
        );
    }

    /// 回归（用户报告：钻入武昌城「看不到武昌」）：城市**继承**父图底图时，
    /// 栅格原点仍是世界图原点 (0,0)，而街道坐标是**本图局部**坐标，必须按
    /// **本图图帧原点**换算。曾用 `raster_origin` 换算，1024² 城市的街道被画到
    /// (0..1024)——即世界图左上角，钻进去之后的视野之外，于是「一条街都看不到」。
    #[test]
    fn street_strokes_use_the_map_frame_origin_when_the_raster_is_inherited() {
        // 城市图帧原点（由父图钻入图标派生）= (1536,1536)；显示的栅格仍是世界图 → (0,0)。
        let map_origin = Vec2::new(1536.0, 1536.0);
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::ZERO,
            map_origin,
            map_size: Vec2::splat(1024.0),
        });
        world.insert_resource(ProjectionFrame::default());
        world.init_resource::<VectorLayerEntities>();
        world.insert_resource(crate::streets::StreetLayerCache {
            map_id: Some(wv_core::Id::new_unchecked("map_city")),
            sources: vec![(
                "maps/city_wuchang/roads.yaml".to_owned(),
                wv_core::StreetLayer {
                    segments: vec![wv_core::StreetSegment::straight(
                        wv_core::RoadClass::Primary,
                        100.0,
                        200.0,
                        300.0,
                        200.0,
                    )],
                },
            )],
        });
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.insert_resource(flat_recipe());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        // 断言走合批 mesh 的顶点：1 段 = 4 顶点，端帽中点必须落在
        // 「本图局部坐标 + 城市帧原点」换算后的端点上（若误用栅格原点，
        // 会落在 (200,200)/(400,200)——即世界图左上角，视野之外）。
        let layer = world.resource::<VectorLayerEntities>();
        let batch = layer
            .street_major
            .batch
            .as_ref()
            .expect("应画出 1 段主干道（合批）");
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&batch.mesh)
            .expect("主干道合批 mesh 应在资产表中");
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        assert_eq!(positions.len(), 4, "1 段 = 4 顶点");
        let v = |i: usize| Vec2::new(positions[i][0], positions[i][1]);
        let a = map_origin + Vec2::new(100.0, 200.0);
        let b = map_origin + Vec2::new(300.0, 200.0);
        assert!(
            (v(0) + v(3) - a * 2.0).length() < 1e-3,
            "起端帽中点应落在城市帧 {a:?}；实得 {:?}（若误用栅格原点会得到 (200,200)）",
            (v(0) + v(3)) * 0.5
        );
        assert!(
            (v(1) + v(2) - b * 2.0).length() < 1e-3,
            "末端帽中点应落在城市帧 {b:?}；实得 {:?}",
            (v(1) + v(2)) * 0.5
        );
    }

    /// 合批世界（A2 测试基建）：路网段列 + 全默认资源，跑一次 sync 前的最小装配。
    /// 测试夹具专用：VectorFlat 行的 recipe（Roads/Streets/Poi 全开）——
    /// 合批/档位/原点换算类测试的意图是「画法」，不是门控。
    /// （原为 Schematic 行；该模式已退役——ADR-0069，两行三格同值。）
    fn flat_recipe() -> crate::recipe::RenderRecipe {
        crate::recipe::resolve_render_recipe(
            &ViewState::default(),
            &crate::layer_frame::LayerFrame::default(),
            &crate::state::WorldProjection::default(),
            &crate::state::DrillState::default(),
            &crate::terrain_view::terrain_texture::TerrainTexture {
                handle: bevy::asset::Handle::default(),
                size: 4,
                height: 4,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "",
                last_hypsometric: 0.0,
                last_map: None,
            },
            None,
        )
    }

    fn world_with_roads(segments: Vec<(Vec2, Vec2)>) -> World {
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            segments.into_iter().map(|(a, b)| (3u8, a, b)).collect(),
            Vec::new(),
        ));
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        // 2026-10-22 内容矩阵后 sync_vector_layers 消费 recipe——夹具按
        // VectorFlat 行解析（Roads/Streets/Poi 全 true），保住「合批 / 档位」
        // 类测试的意图；门控行为由本模块新加的两条专测覆盖。
        world.insert_resource(flat_recipe());
        world
    }

    /// 带道路的投影帧（ADR-0047 D4 测试装配）：带级别段列经
    /// `build_road_tier_view` 算齐切片位与三档指纹——与 `compute_projection`
    /// 同一构建入口，测试数据不会绕过档位切片语义。
    fn projection_frame_with_roads(
        ranked: Vec<(u8, Vec2, Vec2)>,
        place_dots: Vec<PlaceDot>,
    ) -> ProjectionFrame {
        let (segments, cuts, fingerprints) = crate::world_projection::build_road_tier_view(ranked);
        ProjectionFrame {
            road_segments: segments,
            road_rank_cuts: cuts,
            road_tier_fingerprints: fingerprints,
            road_fingerprint: fingerprints[0],
            place_dots,
            ..Default::default()
        }
    }

    /// 3 段路网只产生 2 个 Mesh（casing + fill 各一个合批），不是每段 2 个。
    /// 旧实现（每段一个 Rectangle mesh）在同装配下是 6 个——本测试钉死合批语义。
    #[test]
    fn road_batch_uses_one_mesh_per_pass_not_one_per_segment() {
        let mut world = world_with_roads(vec![
            (Vec2::ZERO, Vec2::X * 100.0),
            (Vec2::X * 100.0, Vec2::new(140.0, 30.0)),
            (Vec2::new(140.0, 30.0), Vec2::new(-20.0, 80.0)),
        ]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            mesh_count, 2,
            "3 段应合批成 casing + fill 两个 mesh，实得 {mesh_count}"
        );
    }

    /// 档位门槛端到端（ADR-0047 D4）：同一份混合级别路网（1 干线 + 2 局部），
    /// Nation 档 fill 合批只含 rank ≥ 3 前缀——顶点数必须比 Edit 档少。
    #[test]
    fn nation_tier_draws_only_high_rank_road_prefix() {
        // 2026-10-25 道路分级后「可见段集」不再等于单一 mesh：三档各一个合批，
        // 故比较的是**三档顶点总和**（分级前是一个 mesh 装全部）。
        let fill_vertices = |world: &World| -> usize {
            let layer = world.resource::<VectorLayerEntities>();
            let assets = world.resource::<Assets<Mesh>>();
            layer
                .roads
                .iter()
                .filter_map(|pool| pool.batch.as_ref())
                .map(|batch| {
                    assets
                        .get(&batch.mesh)
                        .expect("fill mesh 在册")
                        .count_vertices()
                })
                .sum()
        };

        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            vec![
                (3u8, Vec2::ZERO, Vec2::X * 100.0),
                (1u8, Vec2::X * 100.0, Vec2::new(140.0, 30.0)),
                (1u8, Vec2::new(140.0, 30.0), Vec2::new(-20.0, 80.0)),
            ],
            Vec::new(),
        ));
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.insert_resource(flat_recipe());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        // ViewState::default() 的 scale=1.0 恰落在 Region 档（阈值含闭区间），
        // 先显式拉到 Edit 档再对比。
        world.resource_mut::<ViewState>().scale = 0.5;
        schedule.run(&mut world);
        let edit_vertices = fill_vertices(&world);

        world.resource_mut::<ViewState>().scale = 4.0; // Nation 档
        schedule.run(&mut world);
        let nation_vertices = fill_vertices(&world);

        assert!(
            nation_vertices < edit_vertices,
            "Nation 档只画 rank≥3 前缀（1/3 段），顶点应更少：edit={edit_vertices} nation={nation_vertices}"
        );
    }

    /// 段列内容变化：整组换 mesh（旧的移除、新的加入），资产数不累积。
    #[test]
    fn road_batch_replaces_its_mesh_when_segments_change() {
        let mut world = world_with_roads(vec![(Vec2::ZERO, Vec2::X * 100.0)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let old_handle = world.resource::<VectorLayerEntities>().roads[0]
            .batch
            .as_ref()
            .expect("首帧应有 fill 合批")
            .mesh
            .clone();

        let next = [(Vec2::ZERO, Vec2::new(50.0, 60.0))];
        *world.resource_mut::<ProjectionFrame>() =
            projection_frame_with_roads(vec![(3u8, next[0].0, next[0].1)], Vec::new());
        schedule.run(&mut world);

        let layer = world.resource::<VectorLayerEntities>();
        let new_handle = &layer.roads[0]
            .batch
            .as_ref()
            .expect("次帧仍应有 fill 合批")
            .mesh;
        assert_ne!(
            new_handle, &old_handle,
            "内容变了必须换 mesh（否则画的是旧几何）"
        );
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(mesh_count, 2, "换 mesh 是替换不是累积，实得 {mesh_count}");
    }

    /// 缩放一步（线宽 key 变化）：整组换 mesh，资产数不累积。
    #[test]
    fn road_batch_swaps_mesh_on_zoom_without_accumulating() {
        let mut world = world_with_roads(vec![(Vec2::ZERO, Vec2::X * 100.0)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let old_handle = world.resource::<VectorLayerEntities>().roads[0]
            .batch
            .as_ref()
            .expect("首帧应有 fill 合批")
            .mesh
            .clone();

        world.resource_mut::<ViewState>().scale = 2.0;
        schedule.run(&mut world);

        let layer = world.resource::<VectorLayerEntities>();
        let new_handle = &layer.roads[0]
            .batch
            .as_ref()
            .expect("缩放后仍应有 fill 合批")
            .mesh;
        assert_ne!(new_handle, &old_handle, "线宽变了必须换 mesh");
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            mesh_count, 2,
            "缩放换 mesh 是替换不是累积，实得 {mesh_count}"
        );
    }

    /// 线宽量化（ADR-0030 D5b）：缩放漂移 ≤4% 时**不**换 mesh（几何保留），
    /// 超过 4% 的下一步才换——连续缩放下重建频率因此降约 3 倍。
    #[test]
    fn road_batch_keeps_mesh_within_four_percent_thickness_drift() {
        let mut world = world_with_roads(vec![(Vec2::ZERO, Vec2::X * 100.0)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let baked = world.resource::<VectorLayerEntities>().roads[0]
            .batch
            .as_ref()
            .expect("首帧应有 fill 合批")
            .mesh
            .clone();

        world.resource_mut::<ViewState>().scale = 1.02;
        schedule.run(&mut world);
        let after_small = world.resource::<VectorLayerEntities>().roads[0]
            .batch
            .as_ref()
            .expect("2% 缩放后仍应有 fill 合批")
            .mesh
            .clone();
        assert_eq!(after_small, baked, "2% 线宽漂移在容差内，不得换 mesh");

        world.resource_mut::<ViewState>().scale = 1.10;
        schedule.run(&mut world);
        let after_big = world.resource::<VectorLayerEntities>().roads[0]
            .batch
            .as_ref()
            .expect("10% 缩放后仍应有 fill 合批")
            .mesh
            .clone();
        assert_ne!(after_big, baked, "10% 线宽漂移必须换 mesh");
    }

    /// 带地物符号的夹具：复用道路夹具的资源装配，只把 `landmark_dots`
    /// 填上（道路留空——本组测试只关心符号 mesh）。
    fn world_with_landmarks(dots: Vec<crate::world_projection::LandmarkDot>) -> World {
        let mut world = world_with_roads(Vec::new());
        let mut frame = world
            .remove_resource::<ProjectionFrame>()
            .expect("道路夹具必已插入投影帧");
        frame.landmark_dots = dots;
        world.insert_resource(frame);
        world
    }

    /// Settlement 桶（`landmarks[Settlement.index()]`）mesh 的世界顶高。
    fn landmark_mesh_top_y(world: &World) -> f32 {
        let handle = world.resource::<VectorLayerEntities>().landmarks
            [wv_core::distance::PoiKind::Settlement.index()]
        .batch
        .as_ref()
        .expect("符号合批应已生成")
        .mesh
        .clone();
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .expect("mesh 资产应存在");
        batch_positions(mesh)
            .iter()
            .fold(f32::MIN, |m, p| m.max(p.y))
    }

    /// 缩放必须换符号 mesh（屏幕恒定口径的落地）：符号几何烘在世界单位
    /// `s = tier × view_scale`，位置不变而缩放变化时 key 必须变——否则
    /// mesh 沿用旧世界尺寸随相机缩放（2026-10-24 CellMap 实测：455% 下
    /// pin 放大 ~4.5 倍成巨型墨团）。
    #[test]
    fn landmark_batch_rebuilds_mesh_when_zoom_changes_symbol_scale() {
        // 锚点贴 y=0：符号向上生长（y+），顶点最大 y = 局部高 × s，不含锚点偏移。
        let mut world = world_with_landmarks(vec![crate::world_projection::LandmarkDot {
            pos: Vec2::ZERO,
            poi: PoiKind::Settlement,
            selected: false,
        }]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let baked_height = landmark_mesh_top_y(&world);

        // 455% ⇒ scale ≈ 0.22（百分比 = 1 / scale × 100）。
        world.resource_mut::<ViewState>().scale = 0.22;
        schedule.run(&mut world);
        let zoomed_height = landmark_mesh_top_y(&world);

        // 屏幕恒定 ⇒ 世界高度严格 ∝ tier × scale：Region(1.0) → Edit(0.22)。
        let expected_ratio = (landmark_size_tier(0.22) * 0.22) / (landmark_size_tier(1.0) * 1.0);
        let actual_ratio = zoomed_height / baked_height;
        assert!(
            (actual_ratio - expected_ratio).abs() < 1e-4,
            "缩放后符号 mesh 必须按 tier×scale 重烘：期望比例 {expected_ratio:.4}，\
             实得 {actual_ratio:.4}（首帧高 {baked_height:.2} → 缩放后高 {zoomed_height:.2}；\
             比例≈1 说明 mesh 未重建，即本 bug 的症状）"
        );
    }
}
