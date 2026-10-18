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
use crate::theme::palette;
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
#[derive(Resource, Default)]
pub struct FactionOverlayEntities {
    /// 当前帧所有填充多边形实体。
    pub polygons: Vec<Entity>,
    /// 当前帧所有边界实体（casing + inner 各一条）。
    pub borders: Vec<Entity>,
}

impl FactionOverlayEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for e in self.polygons.drain(..).chain(self.borders.drain(..)) {
            commands.entity(e).despawn();
        }
    }
}

/// 一条领土的渲染样式（纯函数输出，可单测）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct TerritoryStyle {
    fill_alpha: Option<f32>,
    border_alpha: Option<f32>,
    dashed: bool,
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
        let Some(style) = territory_style(
            is_active,
            display.show_other_borders,
            is_claim,
            display.show_claims,
            in_focus,
        ) else {
            continue;
        };

        // 同 z 的层按解析顺序（z, id）加一个微小偏移，避免边界同深度 z-fight。
        let layer_order = resolved.iter().position(|l| l.id == t.layer).unwrap_or(0);
        let z = layer_z(layer_def.z, layers.influence.z_offset) + layer_order as f32 * 0.0001;
        let color = if layer_def.color_mode == FactionColorMode::Inherit {
            let p = palette::colors(view.map_palette).soften_toward.to_srgba();
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
                    Mesh2d(mesh),
                    MeshMaterial2d(material),
                    Transform::from_translation(Vec3::new(0.0, 0.0, z)),
                ))
                .id();
            layer.polygons.push(entity);
        }

        if let Some(border_alpha) = style.border_alpha {
            let px = border_px_for(t.faction.rank.as_deref(), !t.faction.memberships.is_empty());
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
                    color,
                    z,
                );
            }
        }
    }
}

/// 单条边界环的描边（casing + inner 两层线）。从 `sync_faction_overlays` 提出，
/// 供外环与洞环共用。
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
        // 「墨线夹两色」（各自画自己一侧）。
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
                Mesh2d(casing),
                MeshMaterial2d(casing_mat),
                Transform::from_translation(Vec3::new(0.0, 0.0, z + 0.001)),
            ))
            .id();
        layer.borders.push(casing_entity);

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
                Mesh2d(inner),
                MeshMaterial2d(inner_mat),
                Transform::from_translation(Vec3::new(0.0, 0.0, z + 0.002)),
            ))
            .id();
        layer.borders.push(inner_entity);
    }
}

/// 当前填色层：请求的层可见则用之，否则第一个可见层。
fn active_layer_id(world: &World, requested: Option<&Id>) -> Option<Id> {
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
    flat.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
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
    let flat_ring: Vec<f32> = ring.iter().flat_map(|p| [p[0], p[1]]).collect();
    for hole in &holes {
        if hole.is_empty() {
            continue;
        }
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
            // 数值兜底：残余顶点扇形收尾（不 panic，V1 精神）。
            for w in idx.windows(3) {
                tris.push(ring[w[0]]);
                tris.push(ring[w[1]]);
                tris.push(ring[w[2]]);
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
        tris.chunks_exact(3)
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
        for t in tris.chunks_exact(3) {
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
                !(has_neg == false && has_pos == false) || (has_neg && has_pos),
                "洞心被三角形覆盖"
            );
        }
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
