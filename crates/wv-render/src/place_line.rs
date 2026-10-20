//! 作者手绘小河（ADR-0046 D6 的交互面）——会话态 + 落笔 + 预览。
//!
//! ## 交互
//!
//! - 开启「画小河」→ **左键逐点连线** → **右键收线**；
//! - 收线时把折线（图内局部 wu）交给**意图桥**（`EditIntents`），应用层翻译成
//!   `AddPlaceCommand` + `AddPlaceLineCommand`（**同一事务 = 一次撤销**）。
//!
//! ## 为什么走意图而不是直接改世界
//!
//! 渲染层**不持有 `EditHistory`**（AGENTS §2.1 / ADR-0020 §2.5）：落笔只发意图，
//! 由应用层翻译器提交命令。街道与测量同款链路（`StreetSegmentIntent` /
//! `MeasurementIntent`）——三条编辑入口共用一条「意图 → 翻译 → 命令」的路。
//!
//! ## 边界
//!
//! - 折线**亚格**：不占格、不写 biome（ADR-0046 D3）——它是一条矢量线，
//!   与一格宽的笔刷河（`wv_terrain::intent::Intent::River`）并存；
//! - 预览（已落的点 + 橡皮筋）**不落数据**，只画在地图上；
//! - 收线至少要 **2 个点**（一条线段）。

#![cfg(feature = "app")]

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::editing_bridge::{EditIntents, PlaceLineIntent};
use crate::state::{DrillState, GridConfig, ViewState, WorldProjection};

/// 预览线宽（屏幕像素）。
const PREVIEW_STROKE_PX: f32 = 2.2;

/// 预览 Z：地形装饰（0.075）之上、道路（0.09）之下——编辑中的线要醒目，
/// 又不能压过道路与标注。
const PREVIEW_Z: f32 = 0.078;

/// 作者手绘小河的**会话态**。
#[derive(Resource, Debug, Default)]
pub struct PlaceLineEditorState {
    /// 是否处于画小河模式（开启时左键不拖地图——与街道手绘同口径）。
    pub enabled: bool,
    /// 已落下的点（**图内局部 wu**）。
    pub points: Vec<Vec2>,
    /// 新河的地点名（空串 = 翻译层给默认名）。
    pub name: String,
    /// 已提交的河数（面板显示用）。
    pub drawn: usize,
}

impl PlaceLineEditorState {
    /// 清掉当前这一笔（不改变 enabled）。
    pub fn clear_stroke(&mut self) {
        self.points.clear();
    }

    /// 当前这一笔是否可以收线（至少两个点）。
    pub fn can_finish(&self) -> bool {
        self.points.len() >= 2
    }
}

/// 作者小河落笔：左键逐点连线，右键收线。
///
/// 与街道手绘同款纪律：只生成 [`PlaceLineIntent`] 交给应用层翻译，
/// **不直接改世界**（AGENTS §3.3）。点取自 `ViewState.cursor_world`
/// （`track_cursor_world` 每帧无条件刷新，便于测试）；世界 → 本图局部走
/// `drill::map_origin` 这唯一的图帧实现点。
#[allow(clippy::too_many_arguments)]
pub fn commit_place_line_click(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    playback: Res<crate::state::PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut editor: ResMut<PlaceLineEditorState>,
    mut intents: ResMut<EditIntents>,
) {
    if !editor.enabled {
        return;
    }
    // 悬在 UI 上 / 播放中只读 / 极低缩放禁画（与街道手绘、地形笔刷同口径）。
    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let origin = crate::drill::map_origin(&projection.world, node, &grid);

    // Esc = 丢弃这一笔（画长了想重来，比右键收线更常见）。
    if keys.just_pressed(KeyCode::Escape) {
        editor.clear_stroke();
        return;
    }
    // 右键 = 收线（提交）。
    if buttons.just_pressed(MouseButton::Right) {
        finish_stroke(&mut editor, &mut intents, node.id.clone(), origin);
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let local = cursor - origin;
    // 忽略与上一点重合的落点（防抖：手抖点两次同一点会出一段零长线）。
    if editor.points.last().map(|p| *p == local).unwrap_or(false) {
        return;
    }
    editor.points.push(local);
}

/// 收线：点数够则发意图，否则清空这一笔。
fn finish_stroke(
    editor: &mut PlaceLineEditorState,
    intents: &mut EditIntents,
    map_id: wv_core::Id,
    origin: Vec2,
) {
    if editor.can_finish() {
        let mut line = Vec::with_capacity(editor.points.len() * 2);
        for p in &editor.points {
            line.extend_from_slice(&[p.x, p.y]);
        }
        intents.push_place_line(PlaceLineIntent {
            map_id,
            name: editor.name.trim().to_string(),
            line,
        });
        editor.drawn = editor.drawn.saturating_add(1);
    }
    editor.clear_stroke();
    let _ = origin;
}

/// 小河预览的运行时资源（合批 mesh + 实体 + 材质）。
#[derive(Resource, Default)]
pub struct PlaceLinePreviewPool {
    batch: Option<PreviewBatch>,
    material: Option<Handle<ColorMaterial>>,
}

struct PreviewBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    fingerprint: u64,
}

/// 小河预览：已落的点 + 到光标的橡皮筋。
///
/// **不落数据**——画到一半的线随时可丢（右键收线 / Esc 取消）。
#[allow(clippy::too_many_arguments)]
pub fn sync_place_line_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    editor: Res<PlaceLineEditorState>,
    mut pool: ResMut<PlaceLinePreviewPool>,
) {
    if !editor.enabled {
        despawn_preview(&mut commands, &mut pool);
        return;
    }
    let Some(node) = drill.current_node(&projection.world) else {
        despawn_preview(&mut commands, &mut pool);
        return;
    };
    let origin = crate::drill::map_origin(&projection.world, node, &grid);
    // 已落的点（局部 → 视图）。
    let mut pts: Vec<Vec2> = editor.points.iter().map(|p| *p + origin).collect();
    // 橡皮筋：最后一个点 → 光标。
    if let Some(cursor) = view.cursor_world {
        if !editor.points.is_empty() {
            pts.push(cursor);
        }
    }
    let segs = polyline_segments(&pts);
    let fp = segments_fingerprint(&segs);
    if segs.is_empty() {
        despawn_preview(&mut commands, &mut pool);
        return;
    }
    if pool.material.is_none() {
        pool.material = Some(materials.add(ColorMaterial::from_color(Color::srgba(
            0.29, 0.62, 0.86, 0.95,
        ))));
    }
    let mat = pool.material.clone().expect("刚建");
    let thickness = PREVIEW_STROKE_PX * view.scale;
    match pool.batch.take() {
        None => {
            let mesh = meshes.add(crate::vector_layers::build_stroke_batch_mesh(
                &segs, thickness,
            ));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(mat),
                    Transform::from_xyz(0.0, 0.0, PREVIEW_Z),
                ))
                .id();
            pool.batch = Some(PreviewBatch {
                entity,
                mesh,
                fingerprint: fp,
            });
        }
        Some(mut b) => {
            if b.fingerprint != fp {
                let fresh = crate::vector_layers::build_stroke_batch_mesh(&segs, thickness);
                if let Some(mut old) = meshes.get_mut(&b.mesh) {
                    *old = fresh;
                }
                b.fingerprint = fp;
            }
            pool.batch = Some(b);
        }
    }
}

/// 回收预览实体。
fn despawn_preview(commands: &mut Commands, pool: &mut PlaceLinePreviewPool) {
    if let Some(b) = pool.batch.take() {
        commands.entity(b.entity).despawn();
    }
}

/// 折线 → 线段列。
fn polyline_segments(pts: &[Vec2]) -> Vec<(Vec2, Vec2)> {
    let mut segs = Vec::new();
    for w in pts.windows(2) {
        if w[0] != w[1] {
            segs.push((w[0], w[1]));
        }
    }
    segs
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cannot_finish_with_fewer_than_two_points() {
        let mut e = PlaceLineEditorState::default();
        assert!(!e.can_finish(), "没点不该能收线");
        e.points.push(Vec2::new(0.0, 0.0));
        assert!(!e.can_finish(), "单点不该能收线");
        e.points.push(Vec2::new(10.0, 5.0));
        assert!(e.can_finish(), "两点应可收线");
    }

    #[test]
    fn finish_stroke_pushes_intent_and_clears_the_stroke() {
        let mut e = PlaceLineEditorState {
            enabled: true,
            points: vec![
                Vec2::new(1.0, 2.0),
                Vec2::new(3.0, 4.0),
                Vec2::new(5.0, 0.0),
            ],
            name: "通天河".into(),
            drawn: 0,
        };
        let mut intents = EditIntents::default();
        finish_stroke(
            &mut e,
            &mut intents,
            wv_core::Id::new_unchecked("map_a"),
            Vec2::ZERO,
        );
        assert_eq!(e.drawn, 1, "提交计数应 +1");
        assert!(e.points.is_empty(), "收线后这一笔应清空");
        let drained = intents.drain_place_lines();
        assert_eq!(drained.len(), 1);
        let (_, intent) = &drained[0];
        assert_eq!(intent.map_id.as_str(), "map_a");
        assert_eq!(intent.name, "通天河");
        assert_eq!(
            intent.line,
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 0.0],
            "折线应按点序扁平化"
        );
    }

    #[test]
    fn finish_with_one_point_discards_without_intent() {
        let mut e = PlaceLineEditorState {
            enabled: true,
            points: vec![Vec2::new(1.0, 2.0)],
            name: String::new(),
            drawn: 0,
        };
        let mut intents = EditIntents::default();
        finish_stroke(
            &mut e,
            &mut intents,
            wv_core::Id::new_unchecked("map_a"),
            Vec2::ZERO,
        );
        assert_eq!(e.drawn, 0, "单点收线不应计数");
        assert!(e.points.is_empty(), "但仍应清空这一笔");
        assert!(intents.drain_place_lines().is_empty(), "不应产生意图");
    }

    #[test]
    fn polyline_segments_skip_zero_length_pairs() {
        let pts = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 0.0),
        ];
        let segs = polyline_segments(&pts);
        assert_eq!(segs.len(), 1, "重合点之间不应出线段");
    }
}
