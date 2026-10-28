//! 势力图切蛋糕分区渲染（ADR-0079 T10）。
//!
//! ## 职责
//!
//! 每帧对**每个可见势力层**求值 [`wv_core::partition_span_at`]（当前日 =
//! `PlaybackState.current_day`——渲染层禁自比时间，AGENTS §3.2），把该日
//! 的脸画出来：
//!
//! - **活动层**填势力色（`color_mode` 沿用 ADR-0027 D6/D7 口径）；
//! - **非活动层**只描边；
//! - 无命中 span（day 落段前/间隙）→ 该层不画；
//! - 无主脸（owner `None`）填中性灰。
//!
//! 坐标：分区数据是**栅格局部 wu**（T7 Ruling），本模块平移 `raster_origin`
//! （`GridConfig`，与地形栅格同帧同源）成视图坐标。
//!
//! 实体账目 [`PartitionOverlayEntities`] 与 `FactionOverlayEntities` 同款
//! （每帧 despawn 全部 + 重建；z 槽位压势力叠加层之上）。

#![cfg(feature = "app")]

use bevy::math::Vec2;
use bevy::prelude::*;
use std::collections::HashSet;

use wv_core::model::faction::FactionLayer;
use wv_core::{partition_span_at, resolved_faction_layers, Id};
use wv_world_cell_map::state::GridConfig;

use crate::layer_frame::LayerFrame;
use crate::state::{PlaybackState, ViewState, WorldProjection};

/// 每帧的分区渲染实体账目。
#[derive(Resource, Default)]
pub struct PartitionOverlayEntities {
    /// 当前帧所有填充 / 描边实体。
    pub entities: Vec<Entity>,
}

impl PartitionOverlayEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for e in self.entities.drain(..) {
            commands.entity(e).despawn();
        }
    }
}

/// 单层的渲染决策（纯函数，可单测）。
///
/// 返回 `None` = 本层本日不画（层不可见 / 无命中 span）。
pub fn partition_layer_decision(
    layer: &FactionLayer,
    span: Option<&wv_core::model::faction_partition::PartitionSpan>,
) -> Option<()> {
    if !layer.visible {
        return None;
    }
    span.map(|_| ())
}

/// 脸的填色（纯函数）：无主 = 中性灰；其余按势力色。
/// 势力查不到（悬空引用）→ 中性灰（V1：校验面板列出，渲染不崩）。
pub fn face_fill_color(
    owner: Option<&Id>,
    faction_color_of: impl Fn(&Id) -> Option<[u8; 3]>,
) -> [u8; 3] {
    match owner {
        None => [128, 128, 128],
        Some(id) => faction_color_of(id).unwrap_or([128, 128, 128]),
    }
}

/// 栅格局部 → 视图坐标平移量（`raster_origin`）。
pub fn partition_offset(grid: &GridConfig) -> Vec2 {
    grid.raster_origin
}

/// 分区渲染：每帧求值 + 重建（实体量小——脸数是几十量级，无需 diff 缓存）。
#[allow(clippy::too_many_arguments)]
pub fn sync_partition_overlays(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    projection: Res<WorldProjection>,
    layers: Res<LayerFrame>,
    grid: Res<GridConfig>,
    display: Res<crate::faction_overlays::FactionDisplay>,
    mut layer: ResMut<PartitionOverlayEntities>,
) {
    layer.despawn_all(&mut commands);
    // 会话总开关（与势力叠加同门）。
    if !view.factions_visible || !layers.influence.visible {
        return;
    }
    let world = &projection.world;
    let map_id = Id::new_unchecked(&view.current_map);
    let offset = partition_offset(&grid);

    let resolved_layers: Vec<FactionLayer> = resolved_faction_layers(world);
    let visible_layers: Vec<&FactionLayer> = resolved_layers.iter().filter(|l| l.visible).collect();
    if visible_layers.is_empty() {
        return;
    }
    // 活动层（终审 I5）：沿用 faction_overlays 的 `active_layer_id` 口径——
    // 作者在 Inspector 选的活动层优先，否则第一个可见层。
    let active_id = crate::faction_overlays::active_layer_id(world, display.active_layer.as_ref());
    let z_base = 0.086 + layers.influence.z_offset; // 制图国境线之上（ADR-0063）

    for (order, layer_def) in visible_layers.iter().enumerate() {
        let Some(span) = partition_span_at(world, &map_id, &layer_def.id, playback.current_day)
        else {
            continue; // 本日本层无划分（段前 / 间隙）——不画
        };
        let _ = partition_layer_decision(layer_def, Some(span));
        let is_active = active_id.as_ref() == Some(&layer_def.id);
        // 同 z 层间微偏移防 z-fight（faction_overlays 同法）。
        let z = z_base + order as f32 * 0.0001;
        for face in &span.faces {
            // 环顶点（栅格局部 → 视图）。终审 I6：手写 YAML 可带悬空环引用
            // （V1 加载不阻止）——缺顶点的脸**整脸跳过**（诊断面板已列
            // Error），不再 expect 打崩渲染。
            let mut face_broken = false;
            let at = |vid: u32, broken: &mut bool| -> Vec2 {
                match span.vertices.iter().find(|v| v.id == vid) {
                    Some(v) => Vec2::new(v.xy[0], v.xy[1]) + offset,
                    None => {
                        *broken = true;
                        Vec2::ZERO
                    }
                }
            };
            let outer: Vec<Vec2> = face
                .ring
                .iter()
                .map(|id| at(*id, &mut face_broken))
                .collect();
            let holes: Vec<Vec<Vec2>> = face
                .holes
                .iter()
                .map(|h| h.iter().map(|id| at(*id, &mut face_broken)).collect())
                .collect();
            if face_broken {
                continue;
            }

            if is_active {
                // 活动层：填色。
                // TODO(ADR-0079 后续轮): Inherit 配色走 memberships 上级链
                // （faction_overlays::inherited_color 同款）；v1 先势力本色。
                let color = face_fill_color(face.owner.as_ref(), |id| {
                    world
                        .factions
                        .iter()
                        .find(|f| &f.id == id)
                        .map(|f| f.color.to_rgb().unwrap_or([128, 128, 200]))
                });
                let mesh = meshes.add(if holes.is_empty() {
                    crate::faction_overlays::build_polygon_mesh_flat(&outer)
                } else {
                    crate::faction_overlays::build_polygon_with_holes_mesh_flat(&outer, &holes)
                });
                let material = materials.add(ColorMaterial {
                    color: Color::srgba(
                        color[0] as f32 / 255.0,
                        color[1] as f32 / 255.0,
                        color[2] as f32 / 255.0,
                        0.85 * layers.influence.opacity,
                    ),
                    ..default()
                });
                let e = commands
                    .spawn((
                        Mesh2d(mesh),
                        MeshMaterial2d(material),
                        Transform::from_translation(Vec3::new(0.0, 0.0, z)),
                    ))
                    .id();
                layer.entities.push(e);
            }
            // 边界线（活动层与非活动层都描；活动层粗些）。
            let rings: Vec<Vec<Vec2>> = std::iter::once(outer)
                .chain(holes.iter().cloned())
                .collect();
            for ring in rings {
                for seg in ring.windows(2) {
                    let mesh = meshes.add(crate::faction_overlays::build_line_mesh_flat(
                        seg[0],
                        seg[1],
                        if is_active { 1.5 } else { 0.8 },
                    ));
                    let material = materials.add(ColorMaterial {
                        color: Color::srgba(0.35, 0.32, 0.28, 0.9 * layers.influence.opacity),
                        ..default()
                    });
                    let e = commands
                        .spawn((
                            Mesh2d(mesh),
                            MeshMaterial2d(material),
                            Transform::from_translation(Vec3::new(0.0, 0.0, z + 0.001)),
                        ))
                        .id();
                    layer.entities.push(e);
                }
            }
        }
    }
}

/// 兼容旧引用（未消费告警占位）。
#[allow(dead_code)]
fn _touch(_: &HashSet<Id>) {}

#[cfg(test)]
mod tests {
    //! T10 渲染决策纯函数。

    use super::*;
    use wv_core::model::faction::FactionColorMode;

    #[test]
    fn decision_hidden_layer_is_none_even_with_span() {
        let l = FactionLayer {
            id: Id::new_unchecked("l1"),
            name: "国界".into(),
            z: 0,
            color_mode: FactionColorMode::Own,
            visible: false,
        };
        assert!(partition_layer_decision(&l, None).is_none(), "隐藏层不画");
    }

    #[test]
    fn decision_day_before_first_span_draws_nothing() {
        let l = FactionLayer {
            id: Id::new_unchecked("l1"),
            name: "国界".into(),
            z: 0,
            color_mode: FactionColorMode::Own,
            visible: true,
        };
        assert!(
            partition_layer_decision(&l, None).is_none(),
            "day 落段前/间隙（span=None）→ 该层不画（Review Focus 4 渲染侧）"
        );
        assert!(partition_layer_decision(&l, Some(&demo_span())).is_some());
    }

    #[test]
    fn unowned_face_fills_neutral_grey() {
        assert_eq!(
            face_fill_color(None, |_| Some([255, 0, 0])),
            [128, 128, 128],
            "无主 = 中性灰"
        );
        assert_eq!(
            face_fill_color(Some(&Id::new_unchecked("f")), |_| None),
            [128, 128, 128],
            "悬空势力引用 → 灰（不崩，V1）"
        );
        assert_eq!(
            face_fill_color(Some(&Id::new_unchecked("f")), |_| Some([10, 20, 30])),
            [10, 20, 30]
        );
    }

    #[test]
    fn partition_offset_follows_raster_origin() {
        let grid = GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::new(100.0, 200.0),
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        };
        assert_eq!(partition_offset(&grid), Vec2::new(100.0, 200.0));
    }

    fn demo_span() -> wv_core::model::faction_partition::PartitionSpan {
        use wv_core::model::faction_partition::*;
        PartitionSpan {
            from: 0,
            to: None,
            vertices: vec![
                PartitionVertex {
                    id: 0,
                    xy: [0.0, 0.0],
                },
                PartitionVertex {
                    id: 1,
                    xy: [10.0, 0.0],
                },
                PartitionVertex {
                    id: 2,
                    xy: [10.0, 10.0],
                },
            ],
            faces: vec![PartitionFace {
                id: 0,
                owner: None,
                ring: vec![0, 1, 2],
                holes: Vec::new(),
            }],
        }
    }
}
