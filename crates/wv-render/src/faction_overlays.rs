//! 势力图层（ADR-0027）：分层填充 + 边界 + 声称/实控 + focus。
//!
//! ## 职责
//! - 每帧按 `(current_map, current_day)` 求值生效的势力领土
//!   （[`wv_core::faction_territories_at`]；渲染层不比较时间，AGENTS §3）；
//! - **当前层**填色 + 画边界；**其他可见层**只画边界（细线、低 alpha）；
//! - `relation = claim` 的领土用低 alpha + 虚线；`control`/`None` 用实填 + 实线；
//! - `FactionDisplay.focus` 选中势力时，自身 + 直接下级满强度，其余淡化。
//!
//! ## 边界（不做什么）
//! - **不写世界**：只读 `WorldProjection.world` / `LayerFrame` / `FactionDisplay`；
//! - **不做时间比较**：某刻生效的领土由 `faction_territories_at` 求值；
//! - **不推导领地**：贵族自持多边形（ADR-0027 D2=A），不从 Membership 派生几何。
//!
//! ## 每帧重建
//!
//! 势力领土数量小（典型工程 < 20 个势力），每帧 despawn + spawn 的成本可忽略；
//! 缓存跨帧复用会在「层 / 时间 / focus 全变」时制造失效逻辑，得不偿失。

#![cfg(feature = "app")]

use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::mesh::MeshVertexAttribute;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use crate::layer_frame::LayerFrame;
use crate::state::{PlaybackState, ViewState, WorldProjection};
use crate::vector_layers::{build_stroke_batch_mesh, stroke_world_thickness};
use wv_core::{
    faction_territories_at, memberships_of, resolved_faction_layers, subordinates_of, Faction,
    FactionColorMode, Id, World,
};

/// 势力填充在 Z 轴上的基调：在矢量层（道路/POI）之上。
const FACTION_Z: f32 = 0.3;
/// 当前层填充 alpha（与地形透出平衡）。
const FACTION_ALPHA: f32 = 0.30;
/// 其他可见层的边界 alpha（只画边界，不填色）。
const OTHER_LAYER_BORDER_ALPHA: f32 = 0.55;
/// focus 外势力的填充 alpha（ADR-0027 D6：降到 0.06）。
const FOCUS_FILL_ALPHA: f32 = 0.06;
/// focus 外势力的边界 alpha（ADR-0027 D6：降到 0.20）。
const FOCUS_BORDER_ALPHA: f32 = 0.20;
/// 声称（claim）填充 = 当前层 alpha × 该系数 = 0.12。
const CLAIM_FILL_MUL: f32 = 0.40;
/// 每个势力层 z 档位的世界 Z 增量（再乘 `layer.z`）。
const LAYER_Z_STEP: f32 = 0.002;
/// 边界 casing 的墨色（深褐，不用纯黑）。
const BORDER_INK: [u8; 3] = [0x2B, 0x27, 0x22];
/// 专业制图的边界线宽（屏幕像素）——固定细线，不随势力 rank 变粗。
const CARTO_BORDER_PX: f32 = 1.2;
/// 专业制图边界线的 Z：压在河（0.082/0.084）之上、道路（0.09）之下。
///
/// 不能沿用势力图的 `FACTION_Z = 0.3`——那是「填色 + 墨线」的槽位，会盖住
/// POI（0.21）；普通地图里一条行政界必须让路给道路与注记。
const CARTO_BORDER_Z: f32 = 0.086;
/// 虚线：每段屏幕像素长。
const DASH_PX: f32 = 6.0;
/// 虚线：间隙屏幕像素。
const GAP_PX: f32 = 4.0;

/// 势力图呈现的会话状态（ADR-0027 D6）。不落盘。
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct FactionDisplay {
    /// 当前填色的层；`None` = 第一个可见层。
    pub active_layer: Option<Id>,
    /// 其他可见层只画边界（不填色）。
    pub show_other_borders: bool,
    /// 是否画 `relation = claim` 的声称层。
    pub show_claims: bool,
    /// 选中势力：自身 + 直接下级高亮，其余淡化。
    pub focus: Option<Id>,
}

impl Default for FactionDisplay {
    fn default() -> Self {
        Self {
            active_layer: None,
            show_other_borders: true,
            show_claims: true,
            focus: None,
        }
    }
}

/// 势力层实体集合（每帧 despawn + spawn，不跨帧复用）。
///
/// **本帧资产必须记账**（2026-10-29）：每帧重建意味着每帧新 `meshes.add` /
/// `materials.add`；`Assets` 没有自动 GC，只 despawn 实体不 `remove` 句柄
/// 会**无界增长**（领土数 × 每帧 × 60fps）。实体与资产在同一次 `despawn_all`
/// 里成对释放。
#[derive(Resource, Default)]
pub struct FactionOverlayEntities {
    /// 当前帧所有填充多边形实体。
    pub polygons: Vec<Entity>,
    /// 当前帧所有边界实体（casing + inner 各一条）。
    pub borders: Vec<Entity>,
    /// 本帧建出的 Mesh 句柄（随 `despawn_all` 回收）。
    frame_meshes: Vec<Handle<Mesh>>,
    /// 本帧建出的材质句柄（同上）。
    frame_materials: Vec<Handle<ColorMaterial>>,
}

impl FactionOverlayEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for e in self.polygons.drain(..).chain(self.borders.drain(..)) {
            commands.entity(e).despawn();
        }
        let meshes = std::mem::take(&mut self.frame_meshes);
        let materials = std::mem::take(&mut self.frame_materials);
        if meshes.is_empty() && materials.is_empty() {
            return;
        }
        commands.queue(move |world: &mut bevy::ecs::world::World| {
            let mut mesh_assets = world.resource_mut::<Assets<Mesh>>();
            for handle in &meshes {
                mesh_assets.remove(handle);
            }
            let mut material_assets = world.resource_mut::<Assets<ColorMaterial>>();
            for handle in &materials {
                material_assets.remove(handle);
            }
        });
    }

    /// 登记本帧新建的一对资产句柄，供下一次 `despawn_all` 回收。
    fn record_assets(&mut self, mesh: Handle<Mesh>, material: Handle<ColorMaterial>) {
        self.frame_meshes.push(mesh);
        self.frame_materials.push(material);
    }
}

/// 一条领土的渲染样式（纯函数输出，可单测）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct TerritoryStyle {
    fill_alpha: Option<f32>,
    border_alpha: Option<f32>,
    dashed: bool,
}

/// 专业制图（`ViewMode::GoogleMap`）下的边界样式：**只画线，不填色**。
///
/// 三条判据，都是为了「一眼看懂」而不是「看懂谁的领土」：
/// - **只取当前层的实控边界**：其他层 / 声称层不画——普通地图不表达司法争议，
///   多层边界叠在一起会变成灰网；
/// - **不填色**：势力色是「政治地图」的语汇，专业底图不按势力染色；
/// - **虚线**：照样张图例（边界 · 虚线 #C4C4C4）。本模式既然不画声称，
///   虚线在这里不会与「claim」语义冲突（Faction 模式仍维持实线 / 虚线二分）。
fn cartographic_style(is_active: bool, is_claim: bool) -> Option<TerritoryStyle> {
    if !is_active || is_claim {
        return None;
    }
    Some(TerritoryStyle {
        fill_alpha: None,
        border_alpha: Some(1.0),
        dashed: true,
    })
}

/// 势力图层主 system。
#[allow(clippy::too_many_arguments)]
pub fn sync_faction_overlays(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    projection: Res<WorldProjection>,
    layers: Res<LayerFrame>,
    recipe: Res<crate::recipe::RenderRecipe>,
    display: Res<FactionDisplay>,
    mut layer: ResMut<FactionOverlayEntities>,
) {
    // 关闭图层（YAML ∨ 会话）/ recipe 不画势力（仅 Faction 模式） → 全部 despawn。
    if !view.factions_visible
        || !layers.influence.visible
        || !recipe.enabled(crate::recipe::PassId::Faction)
    {
        layer.despawn_all(&mut commands);
        return;
    }

    let world = &projection.world;
    let map_id = Id::new_unchecked(&view.current_map);
    let day = playback.current_day;
    let resolved = resolved_faction_layers(world);
    let active = active_layer_id(world, display.active_layer.as_ref());
    // UX-027：选中势力的画布高亮——「选中即高亮」从列表行推广到画布本体
    // （金色 = ACCENT_SELECT，与 CellMap 选中钉/蚂蚁线同语言）。
    let selected_faction = projection
        .selection
        .as_ref()
        .filter(|s| s.kind == wv_core::EntityKind::Faction)
        .map(|s| s.id.clone());
    let sel_rgb = {
        let c = wv_default_theme::colors::ACCENT_SELECT.to_srgba();
        [
            (c.red * 255.0).round() as u8,
            (c.green * 255.0).round() as u8,
            (c.blue * 255.0).round() as u8,
        ]
    };

    // focus 集合 = 选中势力 + 直接下级（ADR-0027 D6）。
    let focus_set: Option<HashSet<Id>> = display.focus.as_ref().map(|fid| {
        let mut set = HashSet::new();
        set.insert(fid.clone());
        for s in subordinates_of(world, fid, day) {
            set.insert(s.id.clone());
        }
        set
    });

    layer.despawn_all(&mut commands);

    // 专业制图（ADR-0061 追记一 / ADR-0063）：边界层退化为「国境线」——
    // 只画当前层实控边界的一条灰虚线，不填色、不画声称与其他层。
    let cartographic = view.mode == crate::state::ViewMode::GoogleMap;
    let carto_grey = {
        let c = wv_default_theme::colors_google::GOOGLE_BORDER.to_srgba();
        [
            (c.red * 255.0).round() as u8,
            (c.green * 255.0).round() as u8,
            (c.blue * 255.0).round() as u8,
        ]
    };

    let opacity = layers.influence.opacity;
    for t in faction_territories_at(world, &map_id, day) {
        if t.vertices.len() < 6 {
            // 顶点数 < 3 无法成多边形，跳过（V1 守约）。
            continue;
        }
        let Some(layer_def) = resolved.iter().find(|l| l.id == t.layer) else {
            continue;
        };
        if !layer_def.visible {
            continue;
        }
        let is_active = active.as_ref() == Some(&t.layer);
        let is_claim = t.relation == Some("claim");
        let in_focus = focus_set.as_ref().map(|s| s.contains(&t.faction.id));
        let style = if cartographic {
            cartographic_style(is_active, is_claim)
        } else {
            territory_style(
                is_active,
                display.show_other_borders,
                is_claim,
                display.show_claims,
                in_focus,
            )
        };
        let is_selected = selected_faction.as_ref() == Some(&t.faction.id);
        // UX-027：选中态压过其余样式规则（active/claim/focus/制图）——
        // 「我在看你」必须任何模式下都可见。制图模式维持不填色纪律，
        // 只上金色描边。
        let style = if is_selected {
            Some(TerritoryStyle {
                fill_alpha: if cartographic {
                    None
                } else {
                    Some(FACTION_ALPHA * 0.6)
                },
                border_alpha: Some(1.0),
                dashed: false,
            })
        } else {
            style
        };
        let Some(style) = style else {
            continue;
        };

        // 同 z 的层按解析顺序（z, id）加一个微小偏移，避免边界同深度 z-fight。
        let layer_order = resolved.iter().position(|l| l.id == t.layer).unwrap_or(0);
        let z = if cartographic {
            CARTO_BORDER_Z
        } else {
            layer_z(layer_def.z, layers.influence.z_offset) + layer_order as f32 * 0.0001
        };
        let color = if cartographic {
            carto_grey
        } else if layer_def.color_mode == FactionColorMode::Inherit {
            let p = view.active_theme.palette.soften_toward.to_srgba();
            let paper = [
                (p.red * 255.0).round() as u8,
                (p.green * 255.0).round() as u8,
                (p.blue * 255.0).round() as u8,
            ];
            inherited_color(world, t.faction, day, paper)
                .unwrap_or_else(|| faction_color(t.faction))
        } else {
            faction_color(t.faction)
        };

        if let Some(fill) = style.fill_alpha {
            let mesh = meshes.add(if t.holes.is_empty() {
                build_polygon_mesh(t.vertices)
            } else {
                build_polygon_with_holes_mesh(t.vertices, &t.holes)
            });
            let material = materials.add(ColorMaterial {
                color: Color::srgba(
                    color[0] as f32 / 255.0,
                    color[1] as f32 / 255.0,
                    color[2] as f32 / 255.0,
                    (fill * opacity).clamp(0.0, 1.0),
                ),
                ..default()
            });
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, z)),
                ))
                .id();
            layer.polygons.push(entity);
            layer.record_assets(mesh, material);
        }

        if let Some(border_alpha) = style.border_alpha {
            // 专业制图用固定细线（1.2px 屏幕）：一条国家界不该按势力 rank 变粗，
            // 也不该有手绘 casing——那会让底图显得比道路还重。
            let px = if cartographic {
                CARTO_BORDER_PX
            } else {
                border_px_for(t.faction.rank.as_deref(), !t.faction.memberships.is_empty())
            };
            // UX-027：选中势力的边界 = 金色加粗（其余情况沿用势力色）。
            let (px, ring_color) = if is_selected {
                (px.max(SELECTED_BORDER_PX), sel_rgb)
            } else {
                (px, color)
            };
            // 外环 + 每个洞环都要描边（国中国的「洞沿」就是内国边界）。
            for ring in territory_rings(t.vertices, &t.holes) {
                draw_border_ring(
                    ring,
                    &mut meshes,
                    &mut materials,
                    &mut layer,
                    &mut commands,
                    &view,
                    style,
                    px,
                    border_alpha,
                    opacity,
                    ring_color,
                    z,
                    cartographic,
                );
            }
        }
    }
}

/// 单条边界环的描边（casing + inner 两层线）。从 `sync_faction_overlays` 提出，
/// 供外环与洞环共用。
///
/// `single_line = true`（专业制图）时**只画 inner 一层**：普通地图的行政边界
/// 是一条低对比灰线，双层 casing 会让它压过道路与注记。
#[allow(clippy::too_many_arguments)]
fn draw_border_ring(
    ring: &[f32],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    layer: &mut FactionOverlayEntities,
    commands: &mut Commands,
    view: &ViewState,
    style: TerritoryStyle,
    px: f32,
    border_alpha: f32,
    opacity: f32,
    color: [u8; 3],
    z: f32,
    single_line: bool,
) {
    {
        let edges = polygon_edges(ring);
        if edges.is_empty() {
            return;
        }
        let segments = if style.dashed {
            dash_segments(
                &edges,
                stroke_world_thickness(DASH_PX, view.scale),
                stroke_world_thickness(GAP_PX, view.scale),
            )
        } else {
            edges
        };
        if segments.is_empty() {
            return;
        }
        // casing：深墨外圈；inner：势力色内线。相邻两国共用边界时自然形成
        // 「墨线夹两色」（各自画自己一侧）。专业制图只要单线，跳过 casing。
        if !single_line {
            let casing = meshes.add(build_stroke_batch_mesh(
                &segments,
                stroke_world_thickness(px + 2.0, view.scale),
            ));
            let casing_mat = materials.add(ColorMaterial {
                color: Color::srgba(
                    BORDER_INK[0] as f32 / 255.0,
                    BORDER_INK[1] as f32 / 255.0,
                    BORDER_INK[2] as f32 / 255.0,
                    (border_alpha * 0.85 * opacity).clamp(0.0, 1.0),
                ),
                ..default()
            });
            let casing_entity = commands
                .spawn((
                    Mesh2d(casing.clone()),
                    MeshMaterial2d(casing_mat.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, z + 0.001)),
                ))
                .id();
            layer.borders.push(casing_entity);
            layer.record_assets(casing, casing_mat);
        }

        let inner = meshes.add(build_stroke_batch_mesh(
            &segments,
            stroke_world_thickness(px, view.scale),
        ));
        let inner_mat = materials.add(ColorMaterial {
            color: Color::srgba(
                color[0] as f32 / 255.0,
                color[1] as f32 / 255.0,
                color[2] as f32 / 255.0,
                (border_alpha * opacity).clamp(0.0, 1.0),
            ),
            ..default()
        });
        let inner_entity = commands
            .spawn((
                Mesh2d(inner.clone()),
                MeshMaterial2d(inner_mat.clone()),
                Transform::from_translation(Vec3::new(0.0, 0.0, z + 0.002)),
            ))
            .id();
        layer.borders.push(inner_entity);
        layer.record_assets(inner, inner_mat);
    }
}

/// 当前填色层：请求的层可见则用之，否则第一个可见层。
pub(crate) fn active_layer_id(world: &World, requested: Option<&Id>) -> Option<Id> {
    let layers = resolved_faction_layers(world);
    if let Some(id) = requested {
        if layers.iter().any(|l| &l.id == id && l.visible) {
            return Some(id.clone());
        }
    }
    layers.iter().find(|l| l.visible).map(|l| l.id.clone())
}

/// 单条领土的渲染决策（纯函数，可单测）。
///
/// - 非当前层不填色；`show_other_borders = false` 时连边界也不画；
/// - 声称层可被 `show_claims` 整体关掉；
/// - focus 外：填充 0.06、边界 0.20；focus 内保持原强度。
fn territory_style(
    is_active: bool,
    show_other_borders: bool,
    is_claim: bool,
    show_claims: bool,
    in_focus: Option<bool>,
) -> Option<TerritoryStyle> {
    if is_claim && !show_claims {
        return None;
    }
    let mut fill = if is_active { Some(FACTION_ALPHA) } else { None };
    let mut border = if is_active {
        Some(1.0)
    } else if show_other_borders {
        Some(OTHER_LAYER_BORDER_ALPHA)
    } else {
        None
    };
    if fill.is_none() && border.is_none() {
        return None;
    }
    if let Some(inside) = in_focus {
        if !inside {
            fill = fill.map(|_| FOCUS_FILL_ALPHA);
            border = border.map(|_| FOCUS_BORDER_ALPHA);
        }
    }
    if is_claim {
        fill = fill.map(|a| a * CLAIM_FILL_MUL);
    }
    Some(TerritoryStyle {
        fill_alpha: fill,
        border_alpha: border,
        dashed: is_claim,
    })
}

/// 边界线宽（屏幕像素）：先看 `rank` 提示，再看有无上级。
fn border_px_for(rank: Option<&str>, has_superior: bool) -> f32 {
    match rank {
        Some("empire") | Some("kingdom") | Some("sovereign") => 3.0,
        Some("duchy") | Some("principality") => 2.4,
        Some("province") | Some("county") | Some("prefecture") => 1.8,
        Some("noble_house") => 2.0,
        Some("free_city") => 1.6,
        _ => {
            if has_superior {
                1.8
            } else {
                2.6
            }
        }
    }
}

/// 闭合环的边（首尾相接）。
fn polygon_edges(vertices: &[f32]) -> Vec<(Vec2, Vec2)> {
    let n = vertices.len() / 2;
    if n < 2 {
        return Vec::new();
    }
    (0..n)
        .map(|i| {
            let j = (i + 1) % n;
            (
                Vec2::new(vertices[i * 2], vertices[i * 2 + 1]),
                Vec2::new(vertices[j * 2], vertices[j * 2 + 1]),
            )
        })
        .filter(|(a, b)| a.distance(*b) > 1e-6)
        .collect()
}

/// 把闭合边切成虚线段（世界单位；调用方已把 px 换算成世界长）。
fn dash_segments(edges: &[(Vec2, Vec2)], dash: f32, gap: f32) -> Vec<(Vec2, Vec2)> {
    let dash = dash.max(1e-4);
    let period = (dash + gap.max(0.0)).max(1e-4);
    let mut out = Vec::new();
    for &(a, b) in edges {
        let delta = b - a;
        let len = delta.length();
        if len <= 1e-6 {
            continue;
        }
        let dir = delta / len;
        let mut t = 0.0;
        while t < len {
            let end = (t + dash).min(len);
            if end > t {
                out.push((a + dir * t, a + dir * end));
            }
            t += period;
        }
    }
    out
}

/// 层 z → 世界 Z：基调 + 层档位 + 图层绑定的槽位偏移。
/// 选中势力边界线的屏幕像素（UX-027）：比任何 rank 档都粗一档——
/// 选中态是「我在看你」，不该被帝国 3.0px 的国境线淹没。
pub(crate) const SELECTED_BORDER_PX: f32 = 4.0;

/// 偶奇规则点含测试（扁平 `[x0, y0, x1, y1, ...]` 环，首尾不重复）。
///
/// 与 [`crate::intent`]（wv-terrain）同算法但**不跨 crate 复用**——那是
/// `pub(crate)`，且本仓库惯例是渲染侧几何小工具留在渲染侧（约 20 行，
/// 抽公共 crate 反而是新增依赖理由）。
fn flat_point_in_ring(ring: &[f32], p: Vec2) -> bool {
    let n = ring.len() / 2;
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i * 2], ring[i * 2 + 1]);
        let (xj, yj) = (ring[j * 2], ring[j * 2 + 1]);
        let crosses = (yi > p.y) != (yj > p.y) && p.x < (xj - xi) * (p.y - yi) / (yj - yi) + xi;
        if crosses {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 外环 + 洞环的含点判定：外环内且不在任何洞内。
fn flat_poly_contains(outer: &[f32], holes: &[&[f32]], p: Vec2) -> bool {
    flat_point_in_ring(outer, p) && !holes.iter().any(|h| flat_point_in_ring(h, p))
}

/// UX-027：势力面画布拾取——点选工具下点中势力疆界面 = 选中该势力。
///
/// **z 序单真源**（审计前置统一项）：候选按「渲染可见顺序」从上往下取
/// 第一个命中，与渲染算 z 用的是同一份输入——
/// - 分区脸（ADR-0045 partition，切蛋糕主路径）：渲染按可见层解析序 +
///   `order * 0.0001` 微偏移（`faction_partition.rs`），拾取反序扫同一份
///   `resolved_faction_layers` 的可见子集；
/// - 旧 territory spans（ADR-0027 `faction_geometry`，多数只剩占位三角形）：
///   渲染 z 同源（`layer_z + order`），拾取在其后兜底。
///
/// hidden 层不可选（与渲染逐层 gate 同源）；`factions_visible` /
/// `layers.influence` 会话总开关由调用方先判（与渲染早退同门）。
///
/// 坐标：`world_pos` 是视图坐标。分区顶点是栅格局部 wu、渲染加
/// `raster_origin`（`partition_offset`）——拾取先减回去再判；旧 spans
/// 渲染不加偏移，直接判。无主脸（`owner: None`，含自然层）不产出命中。
pub fn pick_faction_face(
    world: &World,
    map_id: &Id,
    world_pos: Vec2,
    grid: &crate::state::GridConfig,
    day: i64,
) -> Option<Id> {
    let resolved = resolved_faction_layers(world);
    let visible: Vec<_> = resolved.iter().filter(|l| l.visible).collect();
    let raster_local = world_pos - grid.raster_origin;

    // 主路径：分区脸（渲染顺序 = 可见层序）。
    for layer_def in visible.iter().rev() {
        let Some(span) = wv_core::partition_span_at(world, map_id, &layer_def.id, day) else {
            continue;
        };
        for face in &span.faces {
            let Some(owner) = face.owner.as_ref() else {
                continue;
            };
            if partition_face_contains(span, face, raster_local) {
                return Some(owner.clone());
            }
        }
    }

    // 兜底：旧 territory spans（渲染顺序 = faction_territories_at 扫描序，
    // 层序由 z 决定；反序扫描 + 跳 hidden 层与视觉最上层同源）。
    for t in faction_territories_at(world, map_id, day).iter().rev() {
        let Some(layer_def) = resolved.iter().find(|l| l.id == t.layer) else {
            continue;
        };
        if !layer_def.visible || t.vertices.len() < 6 {
            continue;
        }
        if flat_poly_contains(t.vertices, &t.holes, world_pos) {
            return Some(t.faction.id.clone());
        }
    }
    None
}

/// 分区脸含点判定（栅格局部坐标）：环顶点按 id 解析，悬空引用整脸不命中
/// （V1：渲染整脸跳过，拾取同口径——「看不见的点不中」）。
fn partition_face_contains(
    span: &wv_core::model::faction_partition::PartitionSpan,
    face: &wv_core::model::faction_partition::PartitionFace,
    p: Vec2,
) -> bool {
    let at = |vid: u32| -> Option<Vec2> {
        span.vertices
            .iter()
            .find(|v| v.id == vid)
            .map(|v| Vec2::new(v.xy[0], v.xy[1]))
    };
    let mut outer = Vec::with_capacity(face.ring.len());
    for vid in &face.ring {
        match at(*vid) {
            Some(v) => outer.push(v),
            None => return false,
        }
    }
    let mut holes = Vec::with_capacity(face.holes.len());
    for hole in &face.holes {
        let mut ring = Vec::with_capacity(hole.len());
        for vid in hole {
            match at(*vid) {
                Some(v) => ring.push(v),
                None => return false,
            }
        }
        holes.push(ring);
    }
    point_in_polygon(outer, p) && !holes.iter().any(|h| point_in_polygon(h.clone(), p))
}

/// `Vec<Vec2>` 版偶奇含点（与 [`flat_point_in_ring`] 同算法的坐标形态变体）。
fn point_in_polygon(ring: Vec<Vec2>, p: Vec2) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i].x, ring[i].y);
        let (xj, yj) = (ring[j].x, ring[j].y);
        let crosses = (yi > p.y) != (yj > p.y) && p.x < (xj - xi) * (p.y - yi) / (yj - yi) + xi;
        if crosses {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn layer_z(layer_z: i32, binding_offset: f32) -> f32 {
    FACTION_Z + (layer_z.clamp(-4, 4) as f32) * LAYER_Z_STEP + binding_offset
}

/// 解析 `Faction.color` 为 sRGB 三分量；解析失败回退到中性蓝。
fn faction_color(faction: &Faction) -> [u8; 3] {
    faction.color.to_rgb().unwrap_or([128, 128, 200])
}

/// `color_mode = Inherit`：取第一条生效上级边的势力色，向主题纸面混 35%。
///
/// 无上级 / 上级不存在 → `None`，调用方回落自身色（V1：缺数据不崩）。
fn inherited_color(world: &World, faction: &Faction, day: i64, paper: [u8; 3]) -> Option<[u8; 3]> {
    let m = memberships_of(world, &faction.id, day).into_iter().next()?;
    let parent = world.factions.iter().find(|f| f.id == m.target)?;
    let base = faction_color(parent);
    let mix = |a: u8, b: u8| (a as f32 * 0.65 + b as f32 * 0.35).round() as u8;
    Some([
        mix(base[0], paper[0]),
        mix(base[1], paper[1]),
        mix(base[2], paper[2]),
    ])
}

/// 由扁平 `[x0, y0, x1, y1, ..., xn, yn]` 顶点构造一个**扇形三角化**的 mesh。
///
/// 中心 = 首顶点（v0），所有三角形共享它：`[(v0, v_i, v_{i+1}) for i in 1..n-1]`。
///
/// **为什么是扇形不是 ear clipping**：v0.1 不处理自交/凹多边形；扇形是
/// 「可画」的最简形态——**凹多边形会出现贴错区域**。这是已知妥协：作者
/// 现阶段要么给凸多边形，要么接受不精确。改进路径是引入 `Tessellator`，
/// 但本模块不新增依赖，留作 v0.2。
pub(crate) fn build_polygon_mesh(coords: &[f32]) -> Mesh {
    let n = coords.len() / 2;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    for i in 0..n {
        positions.push([coords[i * 2], coords[i * 2 + 1], 0.0]);
    }
    // 扇形索引。
    let mut indices: Vec<u32> = Vec::with_capacity((n - 2) * 3);
    for i in 1..(n as u32 - 1) {
        indices.push(0);
        indices.push(i);
        indices.push(i + 1);
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
    mesh
}

// ---------------------------------------------------------------------------
// 带洞三角化（ADR-0028 D3；自写 earcut，零新依赖）
// ---------------------------------------------------------------------------

/// 环的有符号面积（shoelace）；正 = 逆时针。
fn ring_signed_area(ring: &[[f32; 2]]) -> f32 {
    let n = ring.len();
    let mut a = 0.0f32;
    for i in 0..n {
        let p = ring[i];
        let q = ring[(i + 1) % n];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

fn flat_to_ring(flat: &[f32]) -> Vec<[f32; 2]> {
    flat.as_chunks::<2>()
        .0
        .iter()
        .map(|c| [c[0], c[1]])
        .collect()
}

/// 点是否在三角形**严格内部**（不含边界）。耳剪的 contains 测试必须用
/// 严格版：桥接产生的重复点落在候选耳顶点/边上时不算「环内其他点」，
/// 否则所有耳被毙、三角化退化成兜底扇形。
fn point_in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let d1 = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let d2 = (c[0] - b[0]) * (p[1] - b[1]) - (c[1] - b[1]) * (p[0] - b[0]);
    let d3 = (a[0] - c[0]) * (p[1] - c[1]) - (a[1] - c[1]) * (p[0] - c[0]);
    (d1 > 0.0 && d2 > 0.0 && d3 > 0.0) || (d1 < 0.0 && d2 < 0.0 && d3 < 0.0)
}

/// 带洞三角化：洞环按 max-x 桥接进外环（桥上顶点各复制一次形成零宽走廊），
/// 合成单一环后经典耳剪。输出三角形 soup（顶点重复，渲染等价）。
///
/// 确定性：无随机、无浮点排序不稳定项（max-x 平局按环序）。
pub(crate) fn earcut_with_holes(outer: &[f32], holes: &[&[f32]]) -> Vec<[f32; 2]> {
    let mut ring = flat_to_ring(outer);
    if ring.len() < 3 {
        return Vec::new();
    }
    if ring_signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    let mut holes: Vec<Vec<[f32; 2]>> = holes
        .iter()
        .map(|h| {
            let mut v = flat_to_ring(h);
            // 洞环统一顺时针（面积符号与外环相反）。
            if ring_signed_area(&v) > 0.0 {
                v.reverse();
            }
            v
        })
        .collect();
    // max-x 大的洞先桥（earcut 惯例：桥不穿过未处理洞的概率更高）。
    holes.sort_by(|a, b| {
        let am = a.iter().fold(f32::MIN, |m, p| m.max(p[0]));
        let bm = b.iter().fold(f32::MIN, |m, p| m.max(p[0]));
        bm.partial_cmp(&am).unwrap_or(std::cmp::Ordering::Equal)
    });
    for hole in &holes {
        if hole.is_empty() {
            continue;
        }
        // 可见性测试必须用**当前**（已桥接过的）环：第二、第三个洞的桥不能
        // 穿过先前洞的零宽走廊（终审 M2）。
        let flat_ring: Vec<f32> = ring.iter().flat_map(|p| [p[0], p[1]]).collect();
        // 洞的 max-x 顶点。
        let (hi, _) = hole
            .iter()
            .enumerate()
            .fold((0usize, f32::MIN), |(bi, bx), (i, p)| {
                if p[0] > bx {
                    (i, p[0])
                } else {
                    (bi, bx)
                }
            });
        let h = hole[hi];
        // 可见桥点：外环顶点里离洞点最近、且连线中点在（当前）环内部者。
        let mut best: Option<(usize, f32)> = None;
        for (k, o) in ring.iter().enumerate() {
            let mid = [(o[0] + h[0]) / 2.0, (o[1] + h[1]) / 2.0];
            if !wv_core::geometry::point_in_ring(&flat_ring, mid[0], mid[1]) {
                continue;
            }
            let d2 = (o[0] - h[0]) * (o[0] - h[0]) + (o[1] - h[1]) * (o[1] - h[1]);
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some((k, d2));
            }
        }
        let Some(k) = best.map(|(k, _)| k) else {
            continue; // 无可见桥点（退化）——跳过该洞（V1：不 panic，宁可多填一块）
        };
        // 拼接：外环 0..=k + 洞 hi..end + 洞 0..=hi + 外环 k（桥走两遍）。
        let mut next: Vec<[f32; 2]> = Vec::with_capacity(ring.len() + hole.len() + 2);
        next.extend_from_slice(&ring[..=k]);
        next.extend_from_slice(&hole[hi..]);
        next.extend_from_slice(&hole[..=hi]);
        next.push(ring[k]);
        next.extend_from_slice(&ring[k + 1..]);
        ring = next;
    }
    // 经典耳剪。
    let mut tris: Vec<[f32; 2]> = Vec::new();
    let mut idx: Vec<usize> = (0..ring.len()).collect();
    let mut guard = 0usize;
    while idx.len() >= 3 && guard < idx.len() * idx.len() + 64 {
        guard += 1;
        let mut clipped = false;
        let n = idx.len();
        for i in 0..n {
            let (ia, ib, ic) = (idx[i], idx[(i + 1) % n], idx[(i + 2) % n]);
            let (a, b, c) = (ring[ia], ring[ib], ring[ic]);
            // 凸性（严格；零面积耳跳过——桥的退化点在这里自然消解）。
            let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if cross <= 1e-9 {
                continue;
            }
            let contains_other = idx.iter().enumerate().any(|(pos, &j)| {
                let is_ear_vertex = pos == i || pos == (i + 1) % n || pos == (i + 2) % n;
                !is_ear_vertex && point_in_triangle(ring[j], a, b, c)
            });
            if contains_other {
                continue;
            }
            tris.push(a);
            tris.push(b);
            tris.push(c);
            idx.remove((i + 1) % n);
            clipped = true;
            break;
        }
        if !clipped {
            // 数值兜底：以 idx[0] 为轴的**真扇形**收尾（不 panic，V1 精神）。
            // 逐窗口连发是条带不是扇形，会产出重叠三角形。
            let pivot = ring[idx[0]];
            for w in idx.windows(2).skip(1) {
                tris.push(pivot);
                tris.push(ring[w[0]]);
                tris.push(ring[w[1]]);
            }
            break;
        }
    }
    tris
}

/// 领土的全部描边环：外环 + 每个洞环。
pub(crate) fn territory_rings<'a>(vertices: &'a [f32], holes: &'a [&'a [f32]]) -> Vec<&'a [f32]> {
    let mut rings = vec![vertices];
    rings.extend(holes.iter().copied());
    rings
}

/// 带洞填充网格：三角 soup → TriangleList（顺序索引）。
pub(crate) fn build_polygon_with_holes_mesh(outer: &[f32], holes: &[&[f32]]) -> Mesh {
    let tris = earcut_with_holes(outer, holes);
    let positions: Vec<[f32; 3]> = tris.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod hole_tessellation_tests {
    use super::*;

    fn outer_square() -> Vec<f32> {
        vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]
    }

    fn hole_square() -> Vec<f32> {
        vec![2.0, 2.0, 6.0, 2.0, 6.0, 6.0, 2.0, 6.0]
    }

    fn triangle_area_sum(tris: &[[f32; 2]]) -> f32 {
        tris.as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let (a, b, c) = (t[0], t[1], t[2]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn earcut_rectangles_outer_with_one_hole_covers_area_excluding_hole() {
        let hole = hole_square();
        let tris = earcut_with_holes(&outer_square(), &[&hole]);
        // 面积不变式：三角 soup 总面积 == 外环 - 洞（10×10 - 4×4 = 84）。
        let area = triangle_area_sum(&tris);
        assert!((area - 84.0).abs() < 0.5, "三角化总面积应 ≈84，实得 {area}");
        // 洞心不被任何三角形覆盖（点在三角内测试）。
        let hole_center = [4.0f32, 4.0];
        for t in tris.as_chunks::<3>().0 {
            let (a, b, c) = (t[0], t[1], t[2]);
            let sign = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
                (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
            };
            let (d1, d2, d3) = (
                sign(a, b, hole_center),
                sign(b, c, hole_center),
                sign(c, a, hole_center),
            );
            let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            assert!(
                !(has_neg || has_pos) || (has_neg && has_pos), // 全零面积三角形（点恰在边上）不视为覆盖
                "洞心被三角形覆盖"
            );
        }
    }

    #[test]
    fn earcut_covers_outer_minus_two_holes() {
        // 两个 2×2 洞：面积 = 100 − 4 − 4 = 92（多洞桥接不得穿透已有洞）。
        let h1 = vec![2.0, 2.0, 4.0, 2.0, 4.0, 4.0, 2.0, 4.0];
        let h2 = vec![7.0, 7.0, 9.0, 7.0, 9.0, 9.0, 7.0, 9.0];
        let tris = earcut_with_holes(&outer_square(), &[&h1, &h2]);
        let area = triangle_area_sum(&tris);
        assert!(
            (area - 92.0).abs() < 0.5,
            "双洞三角化面积应 ≈92，实得 {area}"
        );
    }

    #[test]
    fn polygon_with_holes_mesh_is_a_triangle_list_with_soup_indices() {
        let hole = hole_square();
        let mesh = build_polygon_with_holes_mesh(&outer_square(), &[&hole]);
        assert!(matches!(
            mesh.primitive_topology(),
            bevy::render::render_resource::PrimitiveTopology::TriangleList
        ));
        let indices = mesh.indices().expect("应有索引");
        assert_eq!(indices.len() % 3, 0, "索引必须是三角形倍数");
    }

    #[test]
    fn territory_rings_include_outer_and_every_hole() {
        let hole = hole_square();
        let outer = outer_square();
        let holes: Vec<&[f32]> = vec![&hole];
        let rings = territory_rings(&outer, &holes);
        assert_eq!(rings.len(), 2, "外环 + 1 洞 = 2 条描边环");
        assert_eq!(rings[1], hole.as_slice());
    }
}

#[cfg(test)]
mod tests {
    //! 纯函数测试：层选择 / 渲染决策 / rank 线宽 / 边与虚线 / 扇形网格。
    use super::*;
    use wv_core::map::{GeometryData, GeometrySpan};
    use wv_core::time::TimeSpan;
    use wv_core::{Color, FactionLayer, Membership};

    fn square() -> Vec<f32> {
        vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]
    }

    fn layer(id: &str, z: i32, visible: bool) -> FactionLayer {
        FactionLayer {
            id: Id::new_unchecked(id),
            name: id.into(),
            z,
            color_mode: FactionColorMode::Own,
            visible,
            usage: wv_core::model::faction::LayerUsage::Political,
        }
    }

    #[test]
    fn build_polygon_mesh_square_fan_yields_two_triangles() {
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
        let indices = m.indices().expect("索引应存在");
        assert_eq!(indices.len(), 6, "扇形 = (n-2) 三角形，4 顶点应为 2 个");
    }

    #[test]
    fn build_polygon_mesh_triangle_uses_only_one_triangle() {
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

    #[test]
    fn inherit_takes_parent_color_mixed_with_paper() {
        let mut w = World::default();
        let parent = Faction::new(Id::new_unchecked("parent"), "P", Color("#000000".into()));
        let mut child = Faction::new(Id::new_unchecked("child"), "C", Color("#FFFFFF".into()));
        child.memberships.push(Membership {
            target: Id::new_unchecked("parent"),
            kind: None,
            span: TimeSpan::open(1),
        });
        w.factions.push(parent);
        w.factions.push(child);
        let c = inherited_color(&w, &w.factions[1], 5, [255, 255, 255]).expect("parent exists");
        // parent 0 × 0.65 + paper 255 × 0.35 = 89.25 → 89。
        assert!((c[0] as i32 - 89).abs() <= 1, "got {c:?}");
    }

    #[test]
    fn inherit_without_parent_falls_back_to_none() {
        let mut w = World::default();
        let child = Faction::new(Id::new_unchecked("child"), "C", Color("#FFFFFF".into()));
        w.factions.push(child);
        assert_eq!(
            inherited_color(&w, &w.factions[0], 5, [255, 255, 255]),
            None
        );
    }

    #[test]
    fn active_layer_prefers_requested_then_first_visible() {
        let mut w = World::default();
        w.faction_layers.push(layer("layer_a", 0, true));
        w.faction_layers.push(layer("layer_b", 1, true));
        let requested = Id::new_unchecked("layer_b");
        assert_eq!(
            active_layer_id(&w, Some(&requested)).unwrap().as_str(),
            "layer_b"
        );
        let missing = Id::new_unchecked("layer_missing");
        assert_eq!(
            active_layer_id(&w, Some(&missing)).unwrap().as_str(),
            "layer_a",
            "请求的层不可见/不存在时回落第一个可见层"
        );
    }

    #[test]
    fn hidden_layer_is_never_active() {
        let mut w = World::default();
        w.faction_layers.push(layer("layer_a", 0, true));
        w.faction_layers.push(layer("layer_b", 1, false));
        let requested = Id::new_unchecked("layer_b");
        assert_eq!(
            active_layer_id(&w, Some(&requested)).unwrap().as_str(),
            "layer_a"
        );
    }

    #[test]
    fn active_layer_fills_and_others_border_only() {
        let active = territory_style(true, true, false, true, None).unwrap();
        assert_eq!(active.fill_alpha, Some(FACTION_ALPHA));
        assert_eq!(active.border_alpha, Some(1.0));
        assert!(!active.dashed);

        let other = territory_style(false, true, false, true, None).unwrap();
        assert_eq!(other.fill_alpha, None, "其他层不填色");
        assert_eq!(other.border_alpha, Some(OTHER_LAYER_BORDER_ALPHA));

        assert!(
            territory_style(false, false, false, true, None).is_none(),
            "show_other_borders=false 时其他层完全不画"
        );
    }

    #[test]
    fn claims_are_dashed_and_can_be_hidden() {
        let claim = territory_style(true, true, true, true, None).unwrap();
        assert!(claim.dashed, "claim 必须虚线");
        assert!(
            (claim.fill_alpha.unwrap() - FACTION_ALPHA * CLAIM_FILL_MUL).abs() < 1e-6,
            "claim 填充 = 0.30 × 0.40，got {:?}",
            claim.fill_alpha
        );
        assert!(
            territory_style(true, true, true, false, None).is_none(),
            "show_claims=false 时 claim 完全不画"
        );
    }

    #[test]
    fn focus_fades_outsiders_to_documented_alphas() {
        let inside = territory_style(true, true, false, true, Some(true)).unwrap();
        assert_eq!(inside.fill_alpha, Some(FACTION_ALPHA));
        assert_eq!(inside.border_alpha, Some(1.0));

        let outside = territory_style(false, true, false, true, Some(false)).unwrap();
        assert_eq!(outside.fill_alpha, None, "其他层本来就不填");
        assert_eq!(outside.border_alpha, Some(FOCUS_BORDER_ALPHA));

        let outside_active = territory_style(true, true, false, true, Some(false)).unwrap();
        assert_eq!(outside_active.fill_alpha, Some(FOCUS_FILL_ALPHA));
        assert_eq!(outside_active.border_alpha, Some(FOCUS_BORDER_ALPHA));
    }

    #[test]
    fn border_px_follows_rank_then_superior_depth() {
        assert_eq!(border_px_for(Some("kingdom"), false), 3.0);
        assert_eq!(border_px_for(Some("province"), false), 1.8);
        assert_eq!(border_px_for(Some("noble_house"), true), 2.0);
        assert_eq!(border_px_for(Some("free_city"), true), 1.6);
        assert_eq!(border_px_for(None, false), 2.6, "无 rank 的根势力");
        assert_eq!(border_px_for(None, true), 1.8, "无 rank 的子势力");
        assert_eq!(
            border_px_for(Some("unknown_rank"), true),
            1.8,
            "未知 rank 走默认档，不报错"
        );
    }

    #[test]
    fn polygon_edges_close_the_ring() {
        let edges = polygon_edges(&square());
        assert_eq!(edges.len(), 4);
        assert_eq!(edges[0].0, Vec2::new(0.0, 0.0));
        assert_eq!(edges[3].1, Vec2::new(0.0, 0.0), "最后一条边回到首点");
    }

    #[test]
    fn dash_segments_stay_within_edge_length() {
        let edges = vec![(Vec2::ZERO, Vec2::new(10.0, 0.0))];
        let segs = dash_segments(&edges, 2.0, 2.0);
        assert_eq!(
            segs.len(),
            3,
            "10 长边：0-2 / 4-6 / 8-10，got {}",
            segs.len()
        );
        let total: f32 = segs.iter().map(|(a, b)| a.distance(*b)).sum();
        assert!(total <= 10.0 + 1e-6, "虚线总长不得超出原边，got {total}");
        for (a, b) in segs {
            assert!(a.x >= 0.0 && b.x <= 10.0 + 1e-6);
        }
    }

    #[test]
    fn layer_z_is_clamped_and_stacked() {
        assert!((layer_z(0, 0.0) - FACTION_Z).abs() < 1e-6);
        assert!((layer_z(3, 0.0) - (FACTION_Z + 3.0 * LAYER_Z_STEP)).abs() < 1e-6);
        assert!((layer_z(99, 0.0) - (FACTION_Z + 4.0 * LAYER_Z_STEP)).abs() < 1e-6);
    }

    /// 专业制图的边界样式：只取当前层实控，**不填色**、虚线。
    ///
    /// 三条断言对应三条设计判据：不填势力色（普通地图不按政治染色）、
    /// 其他层不画（多层边界叠成灰网）、声称层不画（普通地图不表达争议）。
    #[test]
    fn cartographic_style_is_border_only_for_active_control() {
        let s = cartographic_style(true, false).expect("当前层实控应画边界");
        assert_eq!(s.fill_alpha, None, "专业制图不填势力色");
        assert_eq!(s.border_alpha, Some(1.0), "边界满强度");
        assert!(s.dashed, "样张图例：边界为虚线");
        assert!(cartographic_style(false, false).is_none(), "其他层不画");
        assert!(cartographic_style(true, true).is_none(), "声称层不画");
    }

    /// 集成：`faction_territories_at` 的同 (map, day) 输入 → 输出。
    #[test]
    fn faction_territories_carry_relation() {
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
            relation: Some("claim".into()),
        });
        let map = Id::new_unchecked("map_w");
        let overlays = faction_territories_at(&w, &map, 5);
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].relation, Some("claim"));
    }
}

/// Vec2 版多边形填充 mesh（分区渲染 T10 用——分区环已是 Vec2 视图坐标）。
pub(crate) fn build_polygon_mesh_flat(outer: &[Vec2]) -> Mesh {
    let flat: Vec<f32> = outer.iter().flat_map(|v| [v.x, v.y]).collect();
    build_polygon_mesh(&flat)
}

/// Vec2 版带洞填充 mesh。
pub(crate) fn build_polygon_with_holes_mesh_flat(outer: &[Vec2], holes: &[Vec<Vec2>]) -> Mesh {
    let flat: Vec<f32> = outer.iter().flat_map(|v| [v.x, v.y]).collect();
    let owned: Vec<Vec<f32>> = holes
        .iter()
        .map(|h| h.iter().flat_map(|v| [v.x, v.y]).collect())
        .collect();
    let refs: Vec<&[f32]> = owned.iter().map(|h| h.as_slice()).collect();
    build_polygon_with_holes_mesh(&flat, &refs)
}

/// 两点线段矩形 mesh（宽 = 世界单位）。
pub(crate) fn build_line_mesh_flat(a: Vec2, b: Vec2, width: f32) -> Mesh {
    let dir = (b - a).normalize_or_zero();
    let n = Vec2::new(-dir.y, dir.x) * width * 0.5;
    let mut mesh = Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        bevy::render::mesh::Mesh::ATTRIBUTE_POSITION,
        vec![
            [a.x + n.x, a.y + n.y, 0.0],
            [b.x + n.x, b.y + n.y, 0.0],
            [b.x - n.x, b.y - n.y, 0.0],
            [a.x - n.x, a.y - n.y, 0.0],
        ],
    );
    mesh.insert_indices(bevy::render::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

#[cfg(test)]
mod pick_tests {
    //! UX-027：势力面拾取（z 序单真源 / hidden 不可选 / 洞环）。
    use super::*;
    use crate::state::GridConfig;
    use wv_core::model::faction_partition::{
        FactionPartition, PartitionFace, PartitionSpan, PartitionVertex,
    };
    use wv_core::{Color, FactionLayer};

    /// 两个层各一势力的最小世界：layer_low（z=0）含 faction_low 全图，
    /// layer_high（z=1）含 faction_high 全图——同点两层叠放。
    fn two_layer_world() -> World {
        let mut w = World::default();
        w.faction_layers.push(FactionLayer {
            id: Id::new_unchecked("layer_low"),
            name: "低层".into(),
            z: 0,
            color_mode: Default::default(),
            visible: true,
            usage: Default::default(),
        });
        w.faction_layers.push(FactionLayer {
            id: Id::new_unchecked("layer_high"),
            name: "高层".into(),
            z: 1,
            color_mode: Default::default(),
            visible: true,
            usage: Default::default(),
        });
        w.factions.push(Faction::new(
            Id::new_unchecked("faction_low"),
            "低",
            Color("#111111".into()),
        ));
        w.factions.push(Faction::new(
            Id::new_unchecked("faction_high"),
            "高",
            Color("#222222".into()),
        ));
        // 每层一张满幅脸（0..100 方形，无洞）。
        for (layer, owner) in [
            (
                Id::new_unchecked("layer_low"),
                Id::new_unchecked("faction_low"),
            ),
            (
                Id::new_unchecked("layer_high"),
                Id::new_unchecked("faction_high"),
            ),
        ] {
            w.partitions.push(FactionPartition {
                map: Id::new_unchecked("map_1"),
                layer: layer.clone(),
                spans: vec![PartitionSpan {
                    from: 0,
                    to: None,
                    vertices: vec![
                        PartitionVertex {
                            id: 1,
                            xy: [0.0, 0.0],
                        },
                        PartitionVertex {
                            id: 2,
                            xy: [100.0, 0.0],
                        },
                        PartitionVertex {
                            id: 3,
                            xy: [100.0, 100.0],
                        },
                        PartitionVertex {
                            id: 4,
                            xy: [0.0, 100.0],
                        },
                    ],
                    faces: vec![PartitionFace {
                        id: 1,
                        owner: Some(owner),
                        kind: None,
                        name: None,
                        ring: vec![1, 2, 3, 4],
                        holes: Vec::new(),
                    }],
                }],
            });
        }
        w
    }

    fn grid() -> GridConfig {
        GridConfig::default()
    }

    /// 点中叠放的两层 → 视觉最上层（z 大者）的势力。
    #[test]
    fn pick_returns_the_topmost_layer_faction() {
        let w = two_layer_world();
        let map = Id::new_unchecked("map_1");
        let hit = pick_faction_face(&w, &map, Vec2::new(50.0, 50.0), &grid(), 1);
        assert_eq!(
            hit.map(|i| i.as_str().to_owned()),
            Some("faction_high".into()),
            "两层叠放同点，应命中 z 大的高层"
        );
    }

    /// hidden 的最上层被跳过 → 命中落到下层（isolate-by-visibility 的
    /// 「隐藏 → 点不到」半边；显隐驱动选取可达）。
    #[test]
    fn hidden_top_layer_is_skipped_and_lower_layer_is_hit() {
        let mut w = two_layer_world();
        w.faction_layers
            .iter_mut()
            .find(|l| l.id.as_str() == "layer_high")
            .expect("高层存在")
            .visible = false;
        let map = Id::new_unchecked("map_1");
        let hit = pick_faction_face(&w, &map, Vec2::new(50.0, 50.0), &grid(), 1);
        assert_eq!(
            hit.map(|i| i.as_str().to_owned()),
            Some("faction_low".into()),
            "高层隐藏后应落到低层"
        );
    }

    /// 空白处（所有势力面之外）→ None（不吞空白清除选中的语义）。
    #[test]
    fn blank_point_outside_all_faces_misses() {
        let w = two_layer_world();
        let map = Id::new_unchecked("map_1");
        assert!(pick_faction_face(&w, &map, Vec2::new(150.0, 50.0), &grid(), 1).is_none());
    }

    /// 洞环里的点不命中（湖心不选）。
    #[test]
    fn point_inside_a_hole_misses() {
        let mut w = two_layer_world();
        w.partitions.clear();
        w.partitions.push(FactionPartition {
            map: Id::new_unchecked("map_1"),
            layer: Id::new_unchecked("layer_high"),
            spans: vec![PartitionSpan {
                from: 0,
                to: None,
                vertices: vec![
                    PartitionVertex {
                        id: 1,
                        xy: [0.0, 0.0],
                    },
                    PartitionVertex {
                        id: 2,
                        xy: [100.0, 0.0],
                    },
                    PartitionVertex {
                        id: 3,
                        xy: [100.0, 100.0],
                    },
                    PartitionVertex {
                        id: 4,
                        xy: [0.0, 100.0],
                    },
                    // 洞：40..60 方形。
                    PartitionVertex {
                        id: 5,
                        xy: [40.0, 40.0],
                    },
                    PartitionVertex {
                        id: 6,
                        xy: [60.0, 40.0],
                    },
                    PartitionVertex {
                        id: 7,
                        xy: [60.0, 60.0],
                    },
                    PartitionVertex {
                        id: 8,
                        xy: [40.0, 60.0],
                    },
                ],
                faces: vec![PartitionFace {
                    id: 1,
                    owner: Some(Id::new_unchecked("faction_high")),
                    kind: None,
                    name: None,
                    ring: vec![1, 2, 3, 4],
                    holes: vec![vec![5, 6, 7, 8]],
                }],
            }],
        });
        let map = Id::new_unchecked("map_1");
        assert!(
            pick_faction_face(&w, &map, Vec2::new(50.0, 50.0), &grid(), 1).is_none(),
            "洞心不该命中"
        );
        assert_eq!(
            pick_faction_face(&w, &map, Vec2::new(20.0, 20.0), &grid(), 1)
                .map(|i| i.as_str().to_owned()),
            Some("faction_high".into()),
            "洞外仍命中"
        );
    }

    /// 悬空环引用（渲染整脸跳过）→ 拾取同口径不命中。
    #[test]
    fn dangling_ring_reference_misses_instead_of_panicking() {
        let mut w = two_layer_world();
        w.partitions.clear();
        w.partitions.push(FactionPartition {
            map: Id::new_unchecked("map_1"),
            layer: Id::new_unchecked("layer_high"),
            spans: vec![PartitionSpan {
                from: 0,
                to: None,
                vertices: vec![PartitionVertex {
                    id: 1,
                    xy: [0.0, 0.0],
                }],
                faces: vec![PartitionFace {
                    id: 1,
                    owner: Some(Id::new_unchecked("faction_high")),
                    kind: None,
                    name: None,
                    ring: vec![1, 2, 3], // 2、3 不在顶点表
                    holes: Vec::new(),
                }],
            }],
        });
        let map = Id::new_unchecked("map_1");
        assert!(
            pick_faction_face(&w, &map, Vec2::new(50.0, 50.0), &grid(), 1).is_none(),
            "悬空环 = 渲染不画 = 拾取不命中"
        );
    }

    /// 偶奇规则环形判定（含点/不含点的边界形态）。
    #[test]
    fn flat_point_in_ring_even_odd() {
        let square = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        assert!(flat_point_in_ring(&square, Vec2::new(5.0, 5.0)));
        assert!(!flat_point_in_ring(&square, Vec2::new(15.0, 5.0)));
        assert!(!flat_point_in_ring(&square, Vec2::new(-1.0, 5.0)));
    }
}
