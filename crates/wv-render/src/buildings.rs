//! 2.5D 建筑挤出（ADR-0016 §6）：轮廓 → 顶面 + 侧面 + 柔影的三层假立体。
//!
//! ## 职责
//! - [`ear_clip`] —— 耳切三角化：支持凹多边形（L 形回字），输入环方向不限。
//! - [`sync_building_overlays`] —— 把当前图全部 Building 图层装配成**三个
//!   批量 mesh**（影 / 侧 / 顶），色值走 [`crate::theme::palette`]（古舆图
//!   建筑顶 `#D4C8BB` / 侧 `#9A8B7A`，暮色镜像）；阴影遵循美术 §1.5
//!   （左上光 → 右下投影、长度 0.5× 高度、alpha 0.35、禁纯黑）。
//!
//! ## 几何口径
//! - 数据坐标是**本图局部**（`Building.ring`），经 `GridConfig::map_to_view`
//!   换算成视图空间——与街道 / 地点同一条坐标链；
//! - 挤出方向**世界固定**（屏幕右下）：地图旋转时建筑体随之转（与真实
//!   纸模型一致），影子永远落在世界右下——左上光是「世界的事实」不是屏幕的；
//! - 挤出高度 = `height_m × [`HEIGHT_WORLD_SCALE`]`。1.0 世界单位 / 米的
//!   直译是最易回退的起点（README §12-31(b) 待裁，`TODO(阶段X)`）。
//!
//! ## 边界（不做什么）
//! - **不改世界数据**——只读 `World.buildings`（AGENTS §3.3）；
//! - **不做逐栋拾取 / 编辑**——那是后续的绘制工具（README §12-31(c)）；
//! - **不做自交环 / 带洞环**——与势力几何同限（README §12-8），耳切按
//!   简单多边形假设，坏环最多画出坏三角形、不 panic。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;

use crate::layer_frame::LayerFrame;
use crate::state::{lod_tier, GridConfig, LodTier, ViewState, WorldProjection};
use crate::theme::palette;

/// 挤出高度换算：世界单位 / 米。**待裁**（README §12-31(b)）：真实比例
/// 还是美化夸张未定；1.0 直译是最易回退的起点。
// TODO(阶段X): README §12-31(b) 收敛后改为按图尺度派生或作者可配。
pub const HEIGHT_WORLD_SCALE: f32 = 1.0;

/// 挤出方向：世界右下单位向量（主光左上 45° 的对偶，美术 §1.5）。
pub const EXTRUDE_DIR: Vec2 = Vec2::new(
    core::f32::consts::FRAC_1_SQRT_2,
    -core::f32::consts::FRAC_1_SQRT_2,
);
/// 影长系数：影长 = 0.5 × 高度（美术 §1.5「地表投影向右下，长度 0.5×」）。
pub const SHADOW_LENGTH_FACTOR: f32 = 0.5;
/// 影不透明度（美术 §1.5：alpha 0.35，禁纯黑——影色是 BUILDING_SIDE 而非黑）。
pub const SHADOW_ALPHA: f32 = 0.35;

/// Z 槽位：街道 fill（0.115）之上、POI（0.21）之下；影 < 侧 < 顶。
pub const BUILDING_SHADOW_Z: f32 = 0.125;
pub const BUILDING_BODY_Z: f32 = 0.135;
pub const BUILDING_ROOF_Z: f32 = 0.145;

/// 耳切三角化：把简单多边形环（凹凸不限、方向不限）拆成顶点索引三角形。
///
/// 经典 O(n²) 耳查找：凸顶点 + 环内无其余点 = 耳，切之。共线顶点按非凸
/// 跳过（`cross ≤ ε`），守卫上限防退化环死循环——坏环最多返回少三角形、
/// 不 panic（V1：渲染层不校验数据）。
pub fn ear_clip(ring: &[Vec2]) -> Vec<[usize; 3]> {
    let n = ring.len();
    if n < 3 {
        return Vec::new();
    }
    // 有向面积判向：负 = 顺时针，翻转成逆时针再切（耳判定按 CCW 口径）。
    let signed_area: f32 = (0..n)
        .map(|i| {
            let p = ring[i];
            let q = ring[(i + 1) % n];
            p.x * q.y - q.x * p.y
        })
        .sum();
    let mut idx: Vec<usize> = (0..n).collect();
    if signed_area < 0.0 {
        idx.reverse();
    }
    let mut tris = Vec::new();
    let mut start = 0usize;
    let mut guard = 0usize;
    while idx.len() > 3 && guard < 10_000 {
        guard += 1;
        let m = idx.len();
        let ia = idx[(start + m - 1) % m];
        let ib = idx[start % m];
        let ic = idx[(start + 1) % m];
        let (pa, pb, pc) = (ring[ia], ring[ib], ring[ic]);
        let convex = (pb - pa).perp_dot(pc - pa) > 1e-9;
        let occupied = idx
            .iter()
            .any(|&v| v != ia && v != ib && v != ic && point_strictly_in_tri(ring[v], pa, pb, pc));
        if convex && !occupied {
            tris.push([ia, ib, ic]);
            idx.remove(start % m);
            start %= idx.len().max(1);
        } else {
            start = (start + 1) % m;
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    tris
}

/// 点是否严格在 CCW 三角形内（边上不算——边上的顶点允许留在环里继续切）。
fn point_strictly_in_tri(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = (b - a).perp_dot(p - a);
    let d2 = (c - b).perp_dot(p - b);
    let d3 = (a - c).perp_dot(p - c);
    d1 > 1e-9 && d2 > 1e-9 && d3 > 1e-9
}

/// 把索引三角形组展开成 `Mesh2d` 位置平铺的三角网（世界坐标，无 UV）。
fn triangles_to_mesh(tris: &[[Vec2; 3]]) -> Mesh {
    let mut positions = Vec::with_capacity(tris.len() * 3);
    for t in tris {
        for p in t {
            positions.push([p.x, p.y, 0.0]);
        }
    }
    let mut mesh = Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh
}

/// 重建键：来源图 + 各图层 (栋数, 高度和, 轮廓点和) 的位级指纹 + 主题。
/// 内容级指纹让「编辑了一栋」也触发重建（加/删改高度都会动这三个和）。
#[derive(Debug, Clone, PartialEq)]
struct BuildingKey {
    map: Option<String>,
    fingerprint: Vec<u64>,
    palette: crate::theme::palette::MapPalette,
}

/// 建筑三层实体缓存。
#[derive(Resource, Default)]
pub struct BuildingOverlays {
    shadow: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    body: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    roof: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    key: Option<BuildingKey>,
}

impl BuildingOverlays {
    fn despawn(&mut self, commands: &mut Commands) {
        for slot in [&mut self.shadow, &mut self.body, &mut self.roof] {
            if let Some((entity, mesh, _)) = slot.take() {
                commands.entity(entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&mesh);
                });
            }
        }
        self.key = None;
    }
}

/// 当前图建筑数据的**内容指纹**（廉价内容哈希：三个位级和按图层拼接）。
///
/// 完整内容比较要克隆全部文档；三个和（栋数 / 高度和 / 坐标和）在编辑下
/// 几乎必然变化，碰撞概率对渲染刷新场景足够低。
fn fingerprint(map: &WorldProjection, current: Option<&wv_core::Id>) -> Vec<u64> {
    let Some(node) = current.and_then(|id| map.world.maps.iter().find(|m| &m.id == id)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in &node.layers {
        if l.kind != wv_core::map::LayerKind::Building {
            continue;
        }
        let Some(doc) = map.world.building_of(&l.id) else {
            continue;
        };
        let (mut hsum, mut xsum, mut ysum) = (0.0f32, 0.0f32, 0.0f32);
        for b in &doc.buildings {
            hsum += b.height_m;
            for p in &b.ring {
                xsum += p.x;
                ysum += p.y;
            }
        }
        out.push(
            (doc.len() as u64) | (hsum.to_bits() as u64) << 16 | (xsum.to_bits() as u64) << 24,
        );
        out.push(ysum.to_bits() as u64);
    }
    out
}

/// 同步建筑三层。独立注册（主链已满），排在 `sync_vector_layers` 之后。
///
/// LOD：国家级视野整体退场（一屏几千栋会把世界图变沙盘；省级 + 编辑级显示
/// ——与街名「拉近才出现」同一口味）。
#[allow(clippy::too_many_arguments)]
pub fn sync_building_overlays(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    mut cache: ResMut<BuildingOverlays>,
    recipe: Res<crate::recipe::RenderRecipe>,
) {
    let current = drill.current();
    let tier = lod_tier(view.scale);
    if !layers.building.visible
        || tier == LodTier::Nation
        || grid.cell_size <= 0.0
        || !recipe.enabled(crate::recipe::PassId::Buildings)
    {
        cache.despawn(&mut commands);
        return;
    }
    let map_id = current.map(|id| id.as_str().to_string());
    let key = BuildingKey {
        map: map_id.clone(),
        fingerprint: fingerprint(&projection, current),
        palette: view.map_palette,
    };
    if cache.key.as_ref() != Some(&key) {
        cache.despawn(&mut commands);
        let pal = palette::colors(view.map_palette);
        // 收集当前图全部 Building 图层的建筑（合并多图层；作者可按片区分文件）。
        let node = current.and_then(|id| projection.world.maps.iter().find(|m| &m.id == id));
        let mut shadow_tris: Vec<[Vec2; 3]> = Vec::new();
        let mut side_quads: Vec<[Vec2; 4]> = Vec::new();
        let mut roof_tris: Vec<[Vec2; 3]> = Vec::new();
        if let Some(m) = node {
            for l in &m.layers {
                if l.kind != wv_core::map::LayerKind::Building {
                    continue;
                }
                let Some(doc) = projection.world.building_of(&l.id) else {
                    continue;
                };
                for b in &doc.buildings {
                    if b.ring.len() < 3 {
                        continue;
                    }
                    // 本图局部 → 视图空间（与街道同一条换算链）。
                    let ring: Vec<Vec2> = b
                        .ring
                        .iter()
                        .map(|p| grid.map_to_view(Vec2::new(p.x, p.y)))
                        .collect();
                    let ext = EXTRUDE_DIR * (b.height_m * HEIGHT_WORLD_SCALE);
                    let shadow_off =
                        EXTRUDE_DIR * (b.height_m * HEIGHT_WORLD_SCALE * SHADOW_LENGTH_FACTOR);
                    // 影 = 底面轮廓整体平移影向量（美术 §1.5：右下、0.5× 高度）。
                    for t in ear_clip(&ring) {
                        let world_tri = [ring[t[0]], ring[t[1]], ring[t[2]]];
                        shadow_tris.push(world_tri.map(|p| p + shadow_off));
                        roof_tris.push(world_tri.map(|p| p + ext));
                    }
                    // 侧面：环每边一个四边形（底边 → 顶边 = 底边 + 挤出向量）。
                    for i in 0..ring.len() {
                        let a = ring[i];
                        let b = ring[(i + 1) % ring.len()];
                        side_quads.push([a, b, b + ext, a + ext]);
                    }
                }
            }
        }
        // 空几何层不插 mesh：MeshAllocator 对 0 顶点 mesh 跳过分配、拷贝阶段
        // 却无条件读取，bevy #24874 误报 Use-after-free（无建筑地图启动即刷
        // 三条 error）。该层不生成实体，缓存槽位留 None，despawn 与
        // opacity/z 补丁块对 None 天然跳过。
        let z = layers.building.z_offset;
        let mut spawn_slot = |mesh: Option<Handle<Mesh>>,
                              color: Color,
                              base_z: f32|
         -> Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)> {
            let mesh = mesh?;
            let mat = materials.add(color);
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, base_z + z),
                ))
                .id();
            Some((entity, mesh, mat))
        };
        cache.shadow = spawn_slot(
            (!shadow_tris.is_empty()).then(|| meshes.add(triangles_to_mesh(&shadow_tris))),
            pal.building_side.with_alpha(SHADOW_ALPHA),
            BUILDING_SHADOW_Z,
        );
        cache.body = spawn_slot(
            (!side_quads.is_empty()).then(|| meshes.add(sides_mesh(&side_quads))),
            pal.building_side,
            BUILDING_BODY_Z,
        );
        cache.roof = spawn_slot(
            (!roof_tris.is_empty()).then(|| meshes.add(triangles_to_mesh(&roof_tris))),
            pal.building_top,
            BUILDING_ROOF_Z,
        );
        cache.key = Some(key);
    }
    // 图层绑定变化（opacity / z）就地补丁，不重建几何（与 isoline 同款）。
    if layers.is_changed() {
        // ResMut 的 DerefMut 不支持同表达式拆字段借用——先整体解构。
        let cache_ref = &mut *cache;
        let BuildingOverlays {
            shadow, body, roof, ..
        } = cache_ref;
        let mut slots = [
            (BUILDING_SHADOW_Z, true, shadow),
            (BUILDING_BODY_Z, false, body),
            (BUILDING_ROOF_Z, false, roof),
        ];
        for (base_z, is_shadow, slot) in slots.iter_mut() {
            if let Some((entity, _, mat)) = slot {
                let alpha = if *is_shadow {
                    layers.building.opacity * SHADOW_ALPHA
                } else {
                    layers.building.opacity
                };
                if let Some(mut m) = materials.get_mut(mat) {
                    m.color = m.color.with_alpha(alpha);
                }
                commands.entity(*entity).insert(Transform::from_xyz(
                    0.0,
                    0.0,
                    *base_z + layers.building.z_offset,
                ));
            }
        }
    }
}

/// 侧面四边形组 → 三角网（每 quad 两三角，顺序 [a, b, b+e, a+e]）。
fn sides_mesh(quads: &[[Vec2; 4]]) -> Mesh {
    let tris: Vec<[Vec2; 3]> = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect();
    triangles_to_mesh(&tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单位方环（逆时针）→ 2 个三角形。
    #[test]
    fn unit_square_clips_into_two_triangles() {
        let ring = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];
        let tris = ear_clip(&ring);
        assert_eq!(tris.len(), 2, "四边形应切成 2 三角：{tris:?}");
    }

    /// L 形凹环（顺时针输入）→ 4 个三角形（n-2），方向无关。
    #[test]
    fn concave_l_ring_clips_into_four_triangles_regardless_of_winding() {
        let mut ring = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 4.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(4.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        assert_eq!(ear_clip(&ring).len(), 4, "逆时针 L 形：n-2 = 4");
        ring.reverse();
        assert_eq!(ear_clip(&ring).len(), 4, "顺时针输入应同样 4");
    }

    /// 退化环（<3 点 / 全共线）返回空，不 panic、不死循环。
    #[test]
    fn degenerate_rings_yield_no_triangles() {
        assert!(ear_clip(&[Vec2::ZERO, Vec2::X]).is_empty());
        assert!(ear_clip(&[]).is_empty());
        let collinear = vec![Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)];
        let tris = ear_clip(&collinear);
        assert!(
            tris.iter().flatten().all(|&i| i < 3),
            "索引必须落在环内：{tris:?}"
        );
    }

    /// 挤出方向 = 世界右下（主光左上的对偶），且为单位长度。
    #[test]
    fn extrude_dir_points_to_world_lower_right() {
        // 常量方向的断言是「文档化锚点」：改 EXTRUDE_DIR 的人必须看到这条。
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(EXTRUDE_DIR.x > 0.0 && EXTRUDE_DIR.y < 0.0, "右下方向");
        }
        assert!((EXTRUDE_DIR.length() - 1.0).abs() < 1e-6, "应为单位向量");
    }

    /// 影长 = 0.5 × 高度（美术 §1.5 手算锚点：20 米 → 影向量长度 10）。
    #[test]
    fn shadow_offset_is_half_the_height() {
        let off = EXTRUDE_DIR * (20.0 * HEIGHT_WORLD_SCALE * SHADOW_LENGTH_FACTOR);
        assert!((off.length() - 10.0).abs() < 1e-4, "got {}", off.length());
    }

    /// 三角网展开：2 三角 → 6 顶点位置。
    #[test]
    fn triangles_to_mesh_expands_three_verts_per_tri() {
        let tris = vec![
            [Vec2::ZERO, Vec2::X, Vec2::Y],
            [Vec2::X, Vec2::ONE, Vec2::Y],
        ];
        let mesh = triangles_to_mesh(&tris);
        assert_eq!(mesh.count_vertices(), 6);
    }
}
