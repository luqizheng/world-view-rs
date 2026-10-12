//! 势力多边形叠加层（S12 v0.1）。
//!
//! ## 动机
//!
//! `ViewMode::Faction` 之前只是**淡化底色**的占位（见 [`crate::terrain_view::shade_faded`]）。
//! 真正的势力几何是 `World.faction_geometry`，通过 [`wv_core::factions_at`] 在
//! `current_day` 上切片后得到「当前地图上所有势力 + 多边形」。
//!
//! 本模块把多边形用**世界空间 `Mesh2d`** 画出来——三角形扇（fan）逐顶点三角化，
//! 每势力一材质，颜色取自 `Faction.color`。
//!
//! ## 时间与视图
//!
//! - `ViewState.current_day` 走 `WorldProjection`，**不**直接读 `TimelineQuery`；
//!   `compute_projection` 已经在上一帧把 `frame` 算好，本模块只读它（AGENTS §3.2）。
//! - `view.factions_visible` 控制开关：false → 全部 despawn，0 GPU 开销。
//! - `ViewState.current_map` 决定画哪张地图的势力。
//!
//! ## 数据流
//!
//! 1. `compute_projection` 写 `frame.road_segments` 等；势力几何**不**进 frame——
//!    它的计算成本是 O(N) 切片（`factions_at`），不值得每帧缓存；
//! 2. 本模块每帧现算 `factions_at(world, current_map, current_day)`；
//! 3. 旧实体全部 despawn（factions / multispan / 时间全变，缓存**不能跨帧**）；
//! 4. 为每个多边形 spawn Mesh2d 实体，材质填势力色。
//!
//! ## 与 `terrain_chunks::sync_terrain_chunks` 的关系
//!
//! 那边复用 chunk 实体（稳态零像素拷贝），这边**不**复用——势力多边形数量小、
//! 每帧重新建 mesh 的成本可忽略（典型工程 < 20 个势力）。这是有意的取舍：
//! "不优化还没出问题的事"（AGENTS §11「常见反模式速查」的反面）。
//!
//! ## 不动原则
//!
//! - 零 `wv-core` 改动；
//! - 零新依赖；
//! - 零笔触 / 撤销路径变化——势力几何的写盘由未来的势力编辑面板负责。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::mesh::MeshVertexAttribute;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use crate::state::ViewState;
use wv_core::{factions_at, Faction};

/// 势力叠加层在 Z 轴上的偏移：在矢量层（道路/POI）之上。
const FACTION_Z: f32 = 0.3;
/// 透明度：与「淡化底色」模式对比，势力多边形是**实色**但带 30% alpha，
/// 让地形仍可隐约透出——避免完全遮挡地形导致色块识别失效。
const FACTION_ALPHA: f32 = 0.30;

/// 势力层实体集合（每帧 despawn + spawn，不跨帧复用）。
#[derive(Resource, Default)]
pub struct FactionOverlayEntities {
    /// 当前帧所有势力多边形实体。
    pub polygons: Vec<Entity>,
}

impl FactionOverlayEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for e in self.polygons.drain(..) {
            commands.entity(e).despawn();
        }
    }
}

/// 势力叠加层主 system。
///
/// 在 [`crate::projection::compute_projection`] 之后跑；只读
/// `WorldProjection.world` / `ViewState.current_day` / `ViewState.current_map`。
pub fn sync_faction_overlays(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    playback: Res<crate::state::PlaybackState>,
    projection: Res<crate::state::WorldProjection>,
    mut layer: ResMut<FactionOverlayEntities>,
) {
    // 关闭图层 / 视图模式不是 Faction → 全部 despawn。
    if !view.factions_visible || !is_faction_mode(&view) {
        layer.despawn_all(&mut commands);
        return;
    }
    let map_id = wv_core::id::Id::new_unchecked(&view.current_map);
    let day = playback.current_day;
    let overlays = factions_at(&projection.world, &map_id, day);

    // 整帧 despawn + 重建：势力几何在多 span / 多 (map, day) 下全变，
    // 缓存复用得不偿失。
    layer.despawn_all(&mut commands);

    for (faction, coords) in overlays {
        // 坐标对数 = 顶点数。点数 < 3 无法成多边形，跳过（V1 守约）。
        if coords.len() < 6 {
            continue;
        }
        let color = faction_color(faction);
        let mesh = build_polygon_mesh(coords);
        let mesh_handle = meshes.add(mesh);
        let material = materials.add(ColorMaterial {
            color: Color::srgba(
                color[0] as f32 / 255.0,
                color[1] as f32 / 255.0,
                color[2] as f32 / 255.0,
                FACTION_ALPHA,
            ),
            ..default()
        });
        let entity = commands
            .spawn((
                Mesh2d(mesh_handle),
                MeshMaterial2d(material),
                Transform::from_translation(Vec3::new(0.0, 0.0, FACTION_Z)),
            ))
            .id();
        layer.polygons.push(entity);
    }
}

/// 视图模式是否应画势力叠加。
fn is_faction_mode(view: &ViewState) -> bool {
    matches!(view.mode, crate::state::ViewMode::Faction)
}

/// 解析 `Faction.color` 为 sRGB 三分量；解析失败回退到中性蓝。
fn faction_color(faction: &Faction) -> [u8; 3] {
    faction.color.to_rgb().unwrap_or([128, 128, 200])
}

/// 由扁平 `[x0, y0, x1, y1, ..., xn, yn]` 顶点构造一个**扇形三角化**的 mesh。
///
/// 中心 = 首顶点（v0），所有三角形共享它：`[(v0, v_i, v_{i+1}) for i in 1..n-1]`。
///
/// **为什么是扇形不是 ear clipping**：v0.1 不处理自交/凹多边形；扇形是
/// 「可画」的最简形态——**凹多边形会出现贴错区域**。这是已知妥协：作者
/// 现阶段要么给凸多边形，要么接受不精确。改进路径是引入 `Tessellator`，
/// 但本模块不新增依赖，留作 v0.2。
///
/// **为什么填 CCW 还是 CW 都画**：Bevy 2D Mesh 默认正反面都渲染（无 cull），
/// CCW 错误最多是顺序问题；下面给顶点用 `add_vertex` 不传法线，材质是纯色
/// 不依赖光照方向，无视觉问题。
fn build_polygon_mesh(coords: &[f32]) -> Mesh {
    let n = coords.len() / 2;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    for i in 0..n {
        positions.push([coords[i * 2], coords[i * 2 + 1], 0.0]);
    }
    // 扇形索引。
    let mut indices: Vec<u32> = Vec::with_capacity((n as usize - 2) * 3);
    for i in 1..(n as u32 - 1) {
        indices.push(0);
        indices.push(i);
        indices.push(i + 1);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // 显式声明位置属性（3 floats × f32）。
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    //! `build_polygon_mesh` 纯函数测试：顶点数 / 索引数 / 凸多边形。
    use super::*;
    use wv_core::map::{GeometryData, GeometrySpan};
    use wv_core::time::TimeSpan;
    use wv_core::{Color, Id, World};

    fn square() -> Vec<f32> {
        vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]
    }

    #[test]
    fn build_polygon_mesh_square_has_three_triangles() {
        let m = build_polygon_mesh(&square());
        let positions = m
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在");
        let pos_vec: &[[f32; 3]] = positions.as_float3().expect("应是 f32x3");
        assert_eq!(pos_vec.len(), 4, "正方形 = 4 顶点");
        // 索引：3 个三角形 × 3 顶点 = 9 索引。
        let indices = m.indices().expect("索引应存在");
        assert_eq!(indices.len(), 9, "扇形 = (n-2) 三角形");
    }

    #[test]
    fn build_polygon_mesh_triangle_uses_only_one_triangle() {
        // 三角形是最小可画多边形：3 顶点 → 1 索引三角形。
        let tri = vec![0.0, 0.0, 1.0, 0.0, 0.5, 1.0];
        let m = build_polygon_mesh(&tri);
        let pos_vec: &[[f32; 3]] = m
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(pos_vec.len(), 3);
        assert_eq!(m.indices().unwrap().len(), 3);
    }

    #[test]
    fn faction_color_invalid_falls_back_to_neutral() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("not-a-color".into()));
        assert_eq!(faction_color(&f), [128, 128, 200]);
    }

    #[test]
    fn faction_color_parses_hex() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#FF0000".into()));
        assert_eq!(faction_color(&f), [0xFF, 0x00, 0x00]);
    }

    /// 集成：`sync_faction_overlays` 的同 (map, day) 输入 → 输出。
    /// 这里只验 `factions_at` 给的数据形态正确（与 wv-core 测试同源）。
    #[test]
    fn factions_at_returns_one_polygon_for_one_faction() {
        let mut w = World::default();
        w.factions.push(Faction::new(
            Id::new_unchecked("f1"),
            "F1",
            Color("#00FF00".into()),
        ));
        w.faction_geometry.push(GeometrySpan {
            map: Id::new_unchecked("map_w"),
            owner: Id::new_unchecked("f1"),
            span: TimeSpan { from: 1, to: None },
            geometry: GeometryData::Polygon(square()),
        });
        let map = Id::new_unchecked("map_w");
        let overlays = factions_at(&w, &map, 5);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].1.len(), 8); // 4 顶点 × 2 坐标
    }
}
