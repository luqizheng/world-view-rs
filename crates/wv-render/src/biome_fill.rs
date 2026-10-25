//! 专业制图的群系矢量面（ADR-0062）——把「绿地」从格子块变成平滑多边形。
//!
//! ## 问题
//!
//! 专业制图（`ViewMode::GoogleMap`）的绿地来自 `shade_google` 逐格平涂：
//! 群系是栅格，颜色边界就是格子边界。放大后是台阶；而百度 / Google 的公园
//! 是**矢量多边形**（边平滑、可加描边）。`isoline::kind_segments` 只能给
//! 阶梯线（Chaikin 磨角后仍带梯田感），拿不到可三角化的面。
//!
//! ## 做法：复用既有三段管线，只换输入场
//!
//! 1. **掩码 → 符号距离场**：`wv_world_cell_map::field::mask_sdf`
//!    （Felzenszwalb EDT，`±(d − 0.5)`，零交点恰在格边界）；
//! 2. **等值线**：`wv_terrain::isoline::height_segments` 对任意标量场通用
//!    ——喂距离场、`level = 0` 即得平滑闭合环；再 `assemble_chains` 装配；
//! 3. **平滑**：`simplify_dp(0.30)` 去共线 → `chaikin(2)` 切角；
//! 4. **成面**：DP 按缩放档抽稀 → 环嵌套（奇偶深度）识别洞 → `earcut` 三角化。
//!
//! 全部部件都已存在，本模块只负责「接线 + 缓存 + 洞」。ADR-0061 §5 曾把这条
//! 管线估成「工作量数天」——实测瓶颈不在算法，而在**重建预算**（见下）。
//!
//! ## 只画两个视觉组
//!
//! `shade_google` 只给四类槽上色：水两档 + 森林（`display_kind 4`，公园绿）+
//! 沼泽（`8`，湿绿），其余陆上类一律折叠为近白陆地。所以「有面的」只有这两组
//! ——与 `FILL_KINDS` 一一对应，避免为不可见的颜色造多边形。
//!
//! ## 为什么只在 GoogleMap
//!
//! 艺术主题（水彩 / 水墨）的边界是**手绘语汇**（晕染 / 颗粒 / 纸张），叠一层
//! 硬边多边形会与 ADR-0044 的画法打架。专业制图是唯一需要「干净矢量边」的
//! 范式；其余模式的既有观感逐位不变。
//!
//! ## 缓存与重建预算
//!
//! - **提取缓存**（EDT + marching squares + Chaikin）：键 = `(图, 地形指纹)`。
//!   1024² 图每组建链在几十毫秒量级，只在**内容变化**（编辑 / 换图）或
//!   首次进入时付出；
//! - **成面**（DP + 嵌套 + earcut）：键 = `(图, 指纹, DP 容差档)`。缩放跨档
//!   只重建这一层；档内逐位不变；
//! - 编辑期主要发生在 CellMap（`Topographic`），本模块退场，不参与笔刷热路径。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::mesh::MeshVertexAttribute;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use wv_core::Id;
use wv_terrain::isoline::{assemble_chains, chaikin, height_segments, simplify_dp, Pt};
use wv_terrain::TerrainCell;
use wv_world_cell_map::state::GridConfig;
use wv_world_cell_map::terrain_view::field::signed_mask_distance;
use wv_world_cell_map::terrain_view::isoline_overlay::{
    dp_tol_bucket, dp_tol_for_bucket, terrain_data_hash, IsolineChain,
};
use wv_world_cell_map::terrain_view::terrain_resolve::texture_terrain;
use wv_world_cell_map::terrain_view::terrain_texture::TerrainTexture;

use crate::cellmap_bridge::conv_palette_terrain;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{DrillState, ViewMode, ViewState, WorldProjection};

/// 矢量面 Z：底图（`TERRAIN_Z = 0.0`）之上、等值线（`0.04`）之下。
///
/// 顺序有语义：海岸线必须压在绿地**之上**（水岸线不能被公园色盖住），
/// 而绿地必须压在地形纹理的格子块之上（那是它要盖掉的旧边界）。
pub const BIOME_FILL_Z: f32 = 0.028;

/// 需要面填充的视觉组（`display_kind`）：森林 / 公园绿、沼泽 / 湿地。
///
/// 与 `shade_google` 上色的四个槽（水 0/1、森林 4、沼泽 8）同源；水不在此列
/// （水陆边界由海岸线 + 纹理填充负责，水面本身不是一个「面形地物」）。
const FILL_KINDS: [u8; 2] = [4, 8];

/// 几何失效键（决定 mesh 是否重建）。
#[derive(Debug, Clone, PartialEq)]
struct FillKey {
    map: Option<Id>,
    data_hash: u64,
    tol_bucket: i32,
}

/// 提取缓存输入（决定「链」是否重算 —— EDT 与 marching squares 只在这里付）。
#[derive(Debug, Clone, PartialEq)]
struct RingInputs {
    map: Option<Id>,
    data_hash: u64,
}

/// 一组（一个视觉组）的链缓存：DP 之前的平滑环。
struct RingCache {
    inputs: RingInputs,
    /// 每组一条环列表；下标与 [`FILL_KINDS`] 对齐。
    groups: Vec<Vec<IsolineChain>>,
}

/// 单个视觉组的渲染实体。
struct FillSlot {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<ColorMaterial>>,
    /// 上一帧写入材质的颜色（颜色变了只改材质，不重建 mesh）。
    color: Color,
    /// 上一帧写入的 Z（图层 z 偏移变了才重插 Transform）。
    z: f32,
}

impl Default for FillSlot {
    fn default() -> Self {
        Self {
            entity: None,
            mesh: None,
            material: None,
            color: Color::NONE,
            z: f32::NAN,
        }
    }
}

impl FillSlot {
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(entity) = self.entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(mesh) = self.mesh.take() {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        if let Some(material) = self.material.take() {
            commands.queue(move |world: &mut World| {
                world
                    .resource_mut::<Assets<ColorMaterial>>()
                    .remove(&material);
            });
        }
        self.color = Color::NONE;
    }
}

/// 群系矢量面的运行时状态（每个视觉组一个实体）。
#[derive(Resource, Default)]
pub struct BiomeFills {
    slots: Vec<FillSlot>,
    key: Option<FillKey>,
    cache: Option<RingCache>,
}

impl BiomeFills {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for slot in &mut self.slots {
            slot.despawn(commands);
        }
        // 键清零：重进时重算（地图切换 / 重新开档都要走完整管线）。
        self.key = None;
    }
}

/// 同步群系矢量面（专业制图专属）。
#[allow(clippy::too_many_arguments)]
pub fn sync_biome_fills(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    mut fills: ResMut<BiomeFills>,
) {
    if fills.slots.len() != FILL_KINDS.len() {
        fills.slots.resize_with(FILL_KINDS.len(), FillSlot::default);
    }
    // 解除 `ResMut` 的整结构借用：下面要同时读 `cache`、写 `slots`（不相交字段）。
    // 经 `Deref` 访问时借用检查器看不到字段不相交，这里显式降为 `&mut BiomeFills`。
    let fills = &mut *fills;
    // 门：只在专业制图 + 地形通道可见时存在。
    // `PassId::Terrain` 是底图通道——群系面是底图的一部分（不是独立叠加层），
    // 故不为它单开一个 PassId（`PassId::COUNT` 是编译期常量，加一条要动
    // 全部模式行与测试；语义上也归底图）。
    if view.mode != ViewMode::GoogleMap
        || !layers.terrain.visible
        || !recipe.enabled(PassId::Terrain)
    {
        fills.despawn_all(&mut commands);
        return;
    }
    let Some(data) = texture_terrain(&tex, &projection.world, drill.current()) else {
        fills.despawn_all(&mut commands);
        return;
    };

    let pal = conv_palette_terrain(&view.active_theme.palette, view.mode);
    // 与烘焙纹理同色：`image_from_terrain` 会按 `terrain_soften` 把 core 向纸面
    // 混合；填充不复现这个公式就会比周边「更饱和」，边界处出现一圈色差。
    let color_of = |kind: u8| {
        let base = pal.core.get(kind as usize).copied().unwrap_or(pal.core[2]);
        if view.terrain_soften {
            pal.soften_color(base)
        } else {
            base
        }
    };

    // 指纹只在「可能变了」时算：1024² 图逐格 FNV ≈ 3ms，每帧无条件跑等于
    // 白送一帧预算。`chunks_dirty` 此刻尚未被 `sync_terrain_chunks` 消费
    // （本系统排在它之前，与等值线同款借用），是「内容可能变了」的免费信号。
    let map = tex.last_map.clone();
    let known = fills.key.as_ref();
    let need_hash =
        tex.chunks_dirty.is_some() || known.map(|k| k.map.clone() != map).unwrap_or(true);
    let data_hash = match known {
        Some(k) if !need_hash => k.data_hash,
        _ => terrain_data_hash(data),
    };
    let key = FillKey {
        map,
        data_hash,
        tol_bucket: dp_tol_bucket(view.scale),
    };

    // ── 第一层：提取缓存（内容 / 换图才重算）──
    let ring_inputs = RingInputs {
        map: key.map.clone(),
        data_hash,
    };
    let rebuild_rings = fills
        .cache
        .as_ref()
        .map(|c| c.inputs != ring_inputs)
        .unwrap_or(true);
    if rebuild_rings {
        let groups = FILL_KINDS
            .iter()
            .map(|want| extract_rings(data, *want))
            .collect();
        fills.cache = Some(RingCache {
            inputs: ring_inputs,
            groups,
        });
    }

    // ── 第二层：成面（DP 容差档 / 换图 / 内容变化）──
    let rebuild_mesh = fills.key.as_ref() != Some(&key);
    if rebuild_mesh {
        let tol = dp_tol_for_bucket(key.tol_bucket);
        let cache = fills.cache.as_ref().expect("上面刚建立");
        for (i, slot) in fills.slots.iter_mut().enumerate() {
            let rings = cache.groups.get(i).map(|g| g.as_slice()).unwrap_or(&[]);
            let mesh = build_group_mesh(rings, tol, grid.raster_origin, grid.cell_size);
            slot.despawn(&mut commands);
            let Some(mesh) = mesh else {
                continue; // 空组合法：不插 0 顶点 mesh（bevy #24874）
            };
            let color = color_of(FILL_KINDS[i]);
            let material = materials.add(ColorMaterial::from_color(color));
            let mesh_handle = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Mesh2d(mesh_handle.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0.0, 0.0, BIOME_FILL_Z + layers.terrain.z_offset),
                ))
                .id();
            slot.entity = Some(entity);
            slot.mesh = Some(mesh_handle);
            slot.material = Some(material);
            slot.color = color;
            slot.z = BIOME_FILL_Z + layers.terrain.z_offset;
        }
        fills.key = Some(key);
        return;
    }

    // ── 稳态：只补颜色 / Z（作者改图层绑定或换主题色时）──
    for (i, slot) in fills.slots.iter_mut().enumerate() {
        let color = color_of(FILL_KINDS[i]);
        if slot.color != color {
            if let Some(handle) = &slot.material {
                if let Some(mut mat) = materials.get_mut(handle) {
                    mat.color = color;
                }
            }
            slot.color = color;
        }
        let z = BIOME_FILL_Z + layers.terrain.z_offset;
        if let Some(entity) = slot.entity {
            if slot.z != z {
                commands
                    .entity(entity)
                    .insert(Transform::from_xyz(0.0, 0.0, z));
                slot.z = z;
            }
        }
    }
}

/// 从地形数据提取某视觉组的平滑闭合环（DP 之前的缓存单位）。
///
/// 纯函数（只吃 `TerrainData`，不碰渲染态），与系统、单测共用。
///
/// 管线：`display_kind == want` 掩码 → 符号距离场 → marching squares
/// （`level = 0`）→ 装配 → 小容差去共线 → Chaikin 两轮。
///
/// 只保留**闭合**链：开口链是地图边界处的截断（说明该组延伸到图外），
/// 面填充需要完整边界；真实工程里整片森林压到图边是常态，此处按「不填」
/// 处理——与专业地图在幅面边缘留白一致，且避免半开口多边形三角化出错。
pub(crate) fn extract_rings(data: &wv_core::terrain::TerrainData, want: u8) -> Vec<IsolineChain> {
    let w = data.width as usize;
    let h = data.height as usize;
    let mask = |x: usize, y: usize| {
        let c = &data.cells[y * w + x];
        wv_terrain::biome::display_kind(&TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        }) == want
    };
    // 全图无该组 → 空场（EDT 会退化成饱和值，这里提前省掉 O(W·H) 两趟）。
    if !(0..h).any(|y| (0..w).any(|x| mask(x, y))) {
        return Vec::new();
    }
    let field = signed_mask_distance(w, h, mask);
    let segs = height_segments(
        data.width,
        data.height,
        |x, y| field[y as usize * w + x as usize],
        0.0,
    );
    assemble_chains(&segs)
        .into_iter()
        .filter(|c| c.closed && c.pts.len() >= 3)
        // 格缘阶梯先小容差去共线中点，再 Chaikin——否则平滑后点数翻倍全是
        // 楼梯碎点（与类型边界同一手法）。
        .map(|c| {
            let thin = simplify_dp(&c.pts, true, 0.30);
            (chaikin(&thin, true, 2), true)
        })
        .collect()
}

/// 一组环 → 一个三角网（外环 + 洞）。
///
/// 洞的识别：环嵌套的**奇偶深度**——深度 0 是外环，深度 1 是其洞，
/// 深度 2 是「洞中岛」（再次填色），依此类推。判定用「被多少环包含」，
/// 先过包围盒粗筛（1024² 图上孤立绿地成百上千，逐对点包含判定会退化成
/// O(n²·V)，bbox 预筛把它压回接近 O(n²) 的轻量比较）。
fn build_group_mesh(
    rings: &[IsolineChain],
    tol: f32,
    origin: Vec2,
    cell_size: f32,
) -> Option<Mesh> {
    let mut polys: Vec<Vec<Pt>> = Vec::new();
    for (pts, closed) in rings {
        if !closed {
            continue;
        }
        let thin = simplify_dp(pts, true, tol);
        if thin.len() >= 3 {
            polys.push(thin);
        }
    }
    if polys.is_empty() {
        return None;
    }
    let boxes: Vec<[f32; 4]> = polys.iter().map(|p| bbox_of(p)).collect();
    let depths: Vec<usize> = (0..polys.len())
        .map(|i| {
            (0..polys.len())
                .filter(|&j| {
                    j != i
                        && bbox_contains(&boxes[j], &boxes[i])
                        && point_in_ring(&polys[j], polys[i][0])
                })
                .count()
        })
        .collect();

    let to_world = |p: Pt| origin + Vec2::new(p.x, p.y) * cell_size;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for i in 0..polys.len() {
        if !depths[i].is_multiple_of(2) {
            continue; // 洞：由父环扣掉，不单独成面
        }
        let holes: Vec<Vec<f32>> = (0..polys.len())
            .filter(|&j| {
                depths[j] == depths[i] + 1
                    && bbox_contains(&boxes[i], &boxes[j])
                    && point_in_ring(&polys[i], polys[j][0])
            })
            .map(|j| {
                polys[j]
                    .iter()
                    .flat_map(|p| vec![to_world(*p).x, to_world(*p).y])
                    .collect()
            })
            .collect();
        let outer: Vec<f32> = polys[i]
            .iter()
            .flat_map(|p| vec![to_world(*p).x, to_world(*p).y])
            .collect();
        let hole_refs: Vec<&[f32]> = holes.iter().map(|h| h.as_slice()).collect();
        let tris = crate::faction_overlays::earcut_with_holes(&outer, &hole_refs);
        let base = positions.len() as u32;
        for t in &tris {
            positions.push([t[0], t[1], 0.0]);
        }
        indices.extend(base..positions.len() as u32);
    }
    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn bbox_of(ring: &[Pt]) -> [f32; 4] {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in ring {
        lo[0] = lo[0].min(p.x);
        lo[1] = lo[1].min(p.y);
        hi[0] = hi[0].max(p.x);
        hi[1] = hi[1].max(p.y);
    }
    [lo[0], lo[1], hi[0], hi[1]]
}

fn bbox_contains(outer: &[f32; 4], inner: &[f32; 4]) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

/// 射线法点在环内（环不自交；同 `faction_overlays` 的判定口径）。
fn point_in_ring(ring: &[Pt], p: Pt) -> bool {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(pts: &[(f32, f32)]) -> IsolineChain {
        (pts.iter().map(|(x, y)| Pt { x: *x, y: *y }).collect(), true)
    }

    /// 端到端：4×4 森林块 → 一条平滑闭环，边界贴 `[3,7]²`。
    ///
    /// 两条断言各有针对性：
    /// - **边界贴 3 / 7**：距离场减了半格，零交点恰在格边界；若忘了减半格，
    ///   边界会外扩到 2.5 / 7.5（Chaikin 保直边上的点，故直边坐标可精确断言）；
    /// - **点数远多于 8**：证明 Chaikin 真的在磨角（原始八边形只有 8 点）。
    #[test]
    fn forest_patch_extracts_one_smoothed_ring() {
        let w = 10usize;
        let h = 10usize;
        let mut cells = vec![
            wv_core::terrain::TerrainCellData {
                height: 50,
                surface: 1,
                biome: wv_terrain::biome::BIOME_PLAINS,
            };
            w * h
        ];
        for y in 3..7 {
            for x in 3..7 {
                cells[y * w + x].biome = wv_terrain::biome::BIOME_FOREST;
            }
        }
        let data = wv_core::terrain::TerrainData {
            width: w as u32,
            height: h as u32,
            cells,
        };
        let rings = extract_rings(&data, 4);
        assert_eq!(rings.len(), 1, "单块森林应只有一条闭合环");
        let (pts, closed) = &rings[0];
        assert!(closed, "闭合环");
        assert!(pts.len() > 16, "Chaikin 应显著加密，got {}", pts.len());
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in pts {
            lo[0] = lo[0].min(p.x);
            lo[1] = lo[1].min(p.y);
            hi[0] = hi[0].max(p.x);
            hi[1] = hi[1].max(p.y);
        }
        for (got, want) in [(lo[0], 3.0), (lo[1], 3.0), (hi[0], 7.0), (hi[1], 7.0)] {
            assert!(
                (got - want).abs() < 1e-5,
                "边界应贴格线 {want}，got {got}（整体偏移半格即此断言失败）"
            );
        }
        assert!(
            extract_rings(&data, 8).is_empty(),
            "该图没有湿地带，第二组必须为空"
        );
    }

    /// 无洞单环：三角化出 n−2 个三角形（三角形顶点数 = 3(n−2)）。
    #[test]
    fn convex_ring_triangulates_to_n_minus_two_triangles() {
        let r = ring(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
        let mesh = build_group_mesh(&[r], 0.0, Vec2::ZERO, 1.0).expect("应成面");
        assert_eq!(mesh.count_vertices(), 6, "四边形 → 2 三角形 → 6 顶点");
        assert_eq!(mesh.indices().expect("索引").len(), 6);
    }

    /// 洞被扣掉：外环 4×4 套内环 2×2（深度 1）→ 面积 = 16 − 4 = 12。
    ///
    /// 用**有向面积求和**验证（三角网面积与解析面积一致），比数三角形个数
    /// 更能抓住「洞没扣 / 洞方向错」。
    #[test]
    fn hole_ring_is_subtracted_from_the_fill() {
        let outer = ring(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
        let hole = ring(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)]);
        let mesh = build_group_mesh(&[outer, hole], 0.0, Vec2::ZERO, 1.0).expect("应成面");
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| match a {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => Some(v.clone()),
                _ => None,
            })
            .expect("位置属性");
        let idx = match mesh.indices().expect("索引") {
            bevy::render::mesh::Indices::U32(v) => v.clone(),
            bevy::render::mesh::Indices::U16(v) => v.iter().map(|i| *i as u32).collect(),
        };
        let mut area = 0.0f32;
        for t in idx.chunks_exact(3) {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            area += ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5;
        }
        assert!(
            (area - 12.0).abs() < 1e-3,
            "外环 16 − 洞 4 = 12，实测 {area}"
        );
    }

    /// 洞中岛（深度 2）重新填色：外环 6×6、洞 4×4、岛 2×2 → 36 − 16 + 4 = 24。
    #[test]
    fn island_inside_hole_is_filled_again() {
        let outer = ring(&[(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)]);
        let hole = ring(&[(1.0, 1.0), (5.0, 1.0), (5.0, 5.0), (1.0, 5.0)]);
        let island = ring(&[(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)]);
        let mesh = build_group_mesh(&[outer, hole, island], 0.0, Vec2::ZERO, 1.0).expect("应成面");
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| match a {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => Some(v.clone()),
                _ => None,
            })
            .expect("位置属性");
        let idx = match mesh.indices().expect("索引") {
            bevy::render::mesh::Indices::U32(v) => v.clone(),
            bevy::render::mesh::Indices::U16(v) => v.iter().map(|i| *i as u32).collect(),
        };
        let mut area = 0.0f32;
        for t in idx.chunks_exact(3) {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            area += ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5;
        }
        assert!((area - 24.0).abs() < 1e-3, "36 − 16 + 4 = 24，实测 {area}");
    }

    /// 世界化口径：原点 + 格坐标 × 格径（与等值线同一条换算）。
    #[test]
    fn mesh_coordinates_apply_origin_and_cell_size() {
        let r = ring(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let mesh = build_group_mesh(&[r], 0.0, Vec2::new(100.0, 200.0), 4.0).expect("应成面");
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| match a {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => Some(v.clone()),
                _ => None,
            })
            .expect("位置属性");
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for p in &pos {
            lo[0] = lo[0].min(p[0]);
            lo[1] = lo[1].min(p[1]);
            hi[0] = hi[0].max(p[0]);
            hi[1] = hi[1].max(p[1]);
        }
        assert_eq!(lo, [100.0, 200.0], "左下角 = 原点");
        assert_eq!(hi, [108.0, 208.0], "右上角 = 原点 + 2×4");
    }

    /// 空输入 → 不产 mesh（0 顶点 mesh 会踩 bevy #24874）。
    #[test]
    fn empty_rings_produce_no_mesh() {
        assert!(build_group_mesh(&[], 0.1, Vec2::ZERO, 1.0).is_none());
        // 退化环（< 3 点）同样不出面。
        let degenerate = ring(&[(0.0, 0.0), (1.0, 1.0)]);
        assert!(build_group_mesh(&[degenerate], 0.1, Vec2::ZERO, 1.0).is_none());
    }
}
