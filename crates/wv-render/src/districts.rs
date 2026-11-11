//! 街区 overlay 渲染（2026-11-06 城市街区工具设计 §3.2 / D4）。
//!
//! 街区本体（`District.ring`）要**看得见**：常态 = 淡描边 + 极低透明填充，
//! 选中态 = 金色高亮描边 + 蒙版脉动（与统一选区蒙版语法同族）。不画就
//! 看不见自己框过哪块、也没法点选重摇。
//!
//! ## 层级
//!
//! 两路 mesh：**填充**（耳切三角化，alpha ≈0.08）在下、**描边**（环每边一
//! 条加宽四边形，alpha ≈0.35）在上；选中时描边转金色 + 填充 alpha 抬到
//! ≈0.16。Z 槽位在底图之上、POI 之下（街区是「编辑中的区域」不是要素）。
//!
//! ## 边界
//!
//! - **只读**：不改世界数据（AGENTS §3.3）；编辑走 wv-edit 命令链；
//! - **零新面原则的例外**：本模块是 §3.2 新增的渲染面（设计已登记）；
//! - **空几何不插 mesh**：0 顶点 mesh 会触发 bevy #24874 误报（同 buildings）。

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;

use crate::layer_frame::LayerFrame;
use crate::state::{GridConfig, ViewState, WorldProjection};

/// 街区描边的 Z 槽位：底图（0.0）之上、换级淡化精灵（0.05）之下。
pub const DISTRICT_STROKE_Z: f32 = 0.045;
/// 街区填充的 Z 槽位：描边之下（描边压填充）。
pub const DISTRICT_FILL_Z: f32 = 0.042;

/// 常态描边 alpha。
pub const DISTRICT_STROKE_ALPHA: f32 = 0.35;
/// 常态填充 alpha（极低——只表「这里圈过」）。
pub const DISTRICT_FILL_ALPHA: f32 = 0.08;
/// 选中态描边 alpha（金色高亮）。
pub const DISTRICT_STROKE_ALPHA_SELECTED: f32 = 0.85;
/// 选中态填充 alpha（蒙版脉动的基准值）。
pub const DISTRICT_FILL_ALPHA_SELECTED: f32 = 0.16;
/// 描边世界半宽（× cell_size）：1.6 px 观感的格径半宽。
const DISTRICT_STROKE_CELLS: f32 = 0.06;

/// 街区 overlay 的缓存资源：两路 mesh + 材质 + 键。
#[derive(Resource, Default)]
pub struct DistrictOverlays {
    pub stroke: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    pub fill: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    /// 重建键：地图 id + 街区内容指纹 + 主题 id。
    pub key: Option<(Option<String>, u64, &'static str)>,
}

impl DistrictOverlays {
    pub fn despawn(&mut self, commands: &mut Commands) {
        for (entity, mesh, _mat) in [self.stroke.take(), self.fill.take()].into_iter().flatten() {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        self.key = None;
    }
}

/// 街区内容指纹：图 id + 各街区的 (id, seed, params, ring) 摘要——任何改动
/// 都让键失配重建。
fn fingerprint(projection: &WorldProjection, map: Option<&wv_core::Id>) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = FNV_OFFSET;
    let Some(node) = map.and_then(|id| projection.world.maps.iter().find(|m| &m.id == id)) else {
        return h;
    };
    for l in &node.layers {
        if l.kind != wv_core::map::LayerKind::District {
            continue;
        }
        let Some(doc) = projection.world.city_block_of(&l.id) else {
            continue;
        };
        for d in &doc.districts {
            h ^=
                d.id.as_str()
                    .bytes()
                    .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
            h = h.wrapping_mul(FNV_PRIME);
            h ^= d.seed;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= d.params.density.to_bits() as u64;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= d.ring.len() as u64;
            h = h.wrapping_mul(FNV_PRIME);
            for p in &d.ring {
                h ^= p.x.to_bits() as u64;
                h = h.wrapping_mul(FNV_PRIME);
                h ^= p.y.to_bits() as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }
    }
    h
}

/// 同步街区 overlay（描边 + 填充）。
///
/// 选中态：`selection` 命中某个街区时（`ViewState` 的选区语义）描边转金色。
/// v1 的「选中」= 该街区是**当前编辑目标**（`DistrictForm` 打开的那个）——
/// 选区数据（`wv_render::selection`）的语义是地形格选区，与街区不同源，
/// 故 v1 由 `selected_district` 资源承载（编辑侧写入）。
#[allow(clippy::too_many_arguments)]
pub fn sync_district_overlays(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    selected: Res<SelectedDistrict>,
    mut cache: ResMut<DistrictOverlays>,
) {
    let current = drill.current();
    if !layers.district.visible || grid.cell_size <= 0.0 {
        cache.despawn(&mut commands);
        return;
    }
    let map_id = current.map(|id| id.as_str().to_string());
    let key = (
        map_id.clone(),
        fingerprint(&projection, current),
        view.active_theme.id,
    );
    if cache.key.as_ref() != Some(&key) {
        cache.despawn(&mut commands);
        let mut stroke_quads: Vec<[Vec2; 4]> = Vec::new();
        let mut fill_tris: Vec<[Vec2; 3]> = Vec::new();
        let node = current.and_then(|id| projection.world.maps.iter().find(|m| &m.id == id));
        if let Some(m) = node {
            for l in &m.layers {
                if l.kind != wv_core::map::LayerKind::District {
                    continue;
                }
                let Some(doc) = projection.world.city_block_of(&l.id) else {
                    continue;
                };
                for d in &doc.districts {
                    if d.ring.len() < 3 {
                        continue;
                    }
                    let ring: Vec<Vec2> = d
                        .ring
                        .iter()
                        .map(|p| grid.map_to_view(Vec2::new(p.x, p.y)))
                        .collect();
                    // 填充：耳切三角化（复用 buildings 的耳切器——同款多边形假设）。
                    for t in crate::buildings::ear_clip(&ring) {
                        fill_tris.push([ring[t[0]], ring[t[1]], ring[t[2]]]);
                    }
                    // 描边：环每边一条加宽四边形。
                    let half = grid.cell_size * DISTRICT_STROKE_CELLS;
                    for i in 0..ring.len() {
                        let a = ring[i];
                        let b = ring[(i + 1) % ring.len()];
                        stroke_quads.push(expand_segment(a, b, half));
                    }
                }
            }
        }
        let pal = &view.active_theme.palette;
        let z = layers.district.z_offset;
        let mut spawn = |mesh: Option<Handle<Mesh>>, color: Color, base_z: f32| {
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
        cache.fill = spawn(
            (!fill_tris.is_empty()).then(|| meshes.add(triangles_to_mesh(&fill_tris))),
            pal.building_top.with_alpha(DISTRICT_FILL_ALPHA),
            DISTRICT_FILL_Z,
        );
        cache.stroke = spawn(
            (!stroke_quads.is_empty()).then(|| meshes.add(quads_mesh(&stroke_quads))),
            pal.isoline_ink.with_alpha(DISTRICT_STROKE_ALPHA),
            DISTRICT_STROKE_Z,
        );
        cache.key = Some(key);
    }
    // 选中态：描边转金色 + 填充抬亮（就地补丁，不重建几何）。
    let sel = selected.id.as_ref();
    let pal = &view.active_theme.palette;
    if let Some((entity, _, mat)) = &cache.stroke {
        if let Some(mut m) = materials.get_mut(mat) {
            m.color = if sel.is_some() {
                pal_select_color()
                    .with_alpha(DISTRICT_STROKE_ALPHA_SELECTED * layers.district.opacity)
            } else {
                pal.isoline_ink
                    .with_alpha(DISTRICT_STROKE_ALPHA * layers.district.opacity)
            };
        }
        let _ = entity;
    }
    if let Some((_, _, mat)) = &cache.fill {
        if let Some(mut m) = materials.get_mut(mat) {
            m.color = pal.building_top.with_alpha(
                (if sel.is_some() {
                    DISTRICT_FILL_ALPHA_SELECTED
                } else {
                    DISTRICT_FILL_ALPHA
                }) * layers.district.opacity,
            );
        }
    }
}

/// 选中态金色（与统一选区蒙版的金同族——`ACCENT_SELECT` 0xF2C14E）。
///
/// 走主题包的**常量**而非 palette 字段：街区选中态是「作者正在编辑这块」
/// 的会话高亮，与地形选区蒙版同色系（AGENTS §8 色彩令牌纪律）。
fn pal_select_color() -> Color {
    wv_default_theme::colors::ACCENT_SELECT
}

/// 当前编辑中的街区（编辑侧写入；`None` = 无选中）。
#[derive(Resource, Default, Debug, Clone)]
pub struct SelectedDistrict {
    pub id: Option<wv_core::Id>,
}

/// 线段加宽成四边形（两端沿方向外扩半宽——转角无豁口）。
fn expand_segment(a: Vec2, b: Vec2, half: f32) -> [Vec2; 4] {
    let dir = b - a;
    let len = dir.length();
    if len < 1e-9 {
        return [a, a, a, a];
    }
    let dir = dir / len;
    let nrm = Vec2::new(-dir.y, dir.x);
    let a0 = a - dir * half;
    let b0 = b + dir * half;
    [
        a0 + nrm * half,
        a0 - nrm * half,
        b0 - nrm * half,
        b0 + nrm * half,
    ]
}

/// 三角形集 → mesh（API 同 `buildings::triangles_to_mesh`：无索引表）。
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

/// 四边形集 → mesh（两组三角）。
fn quads_mesh(quads: &[[Vec2; 4]]) -> Mesh {
    let mut tris: Vec<[Vec2; 3]> = Vec::with_capacity(quads.len() * 2);
    for q in quads {
        tris.push([q[0], q[1], q[2]]);
        tris.push([q[0], q[2], q[3]]);
    }
    triangles_to_mesh(&tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_segment_makes_a_quad_of_the_expected_width() {
        let q = expand_segment(Vec2::ZERO, Vec2::new(10.0, 0.0), 1.0);
        // 沿 x 的线段：法向 = (0,1) → 上下各外扩 1。
        assert!((q[0].y - 1.0).abs() < 1e-5 && (q[1].y + 1.0).abs() < 1e-5);
        assert!((q[3].y - 1.0).abs() < 1e-5 && (q[2].y + 1.0).abs() < 1e-5);
        // 两端各外扩半宽。
        assert!((q[1].x - -1.0).abs() < 1e-5);
        assert!((q[2].x - 11.0).abs() < 1e-5);
    }

    #[test]
    fn degenerate_segment_yields_zero_area_quad() {
        let q = expand_segment(Vec2::new(3.0, 3.0), Vec2::new(3.0, 3.0), 1.0);
        assert_eq!(q[0], q[3]);
    }

    #[test]
    fn triangles_mesh_has_three_indices_per_triangle() {
        let tris = vec![
            [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0)],
        ];
        let mesh = triangles_to_mesh(&tris);
        let pos = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("位置属性应存在");
        assert_eq!(pos.len(), 6, "两三角形 = 6 顶点");
    }
}
