//! 测量叠加的渲染层（2026-10-14）。
//!
//! ## 职责
//!
//! - `MeasurementCache` — 当前图的测量文档缓存（仿 `StreetLayerCache`）。
//! - `reload_measurement_cache` — 当前图 / `WorldProjection` 变更时同步缓存。
//! - `commit_measurement_click` — 把鼠标点击派发到对应模式（折线 / 矩形 / 橡皮）。
//! - `delete_measurement_under_cursor` — `Del` 删光标下的那一条（2026-10-23）。
//! - `render_measurements` — 顶置渲染所有可见 measurement + 草稿预览 + 长度 / 面积文字。
//!
//! ## 边界（不做什么）
//!
//! - **不**做「测量图层开关」（用户需求：永远显示）。
//! - **不**做渲染层决策（线宽 / 颜色 / LOD 阈值），本模块只接受 `Measurement` 携带的
//!   `color` 可选字段并把它应用到绘制——其它色值走主题。
//!
//! ## 与 `streets::commit_street_click` 的对照
//!
//! 共用「enabled 闸 → UI / 播放 / 缩放下限闸 → 无工程闸门闸 → 坐标走
//! `drill::map_origin` 同源链 → 命令经 `EditHistory`」纪律，但：
//! - **测量模式更复杂**：三态分支（Line / Rect / Eraser），Line 是多段累积。
//! - **切图时清草稿**：草稿绑 map_id，切图丢弃（不污染新图）。
//! - **橡皮命中用距离容差**：8 wu ≈ 1 栅格（`cell_size=8`），不要求精确点中。

#![cfg(feature = "app")]

use bevy::prelude::*;

use wv_core::{Measurement, MeasurementKind, MeasurementOverlay};

use crate::editing_bridge::{EditIntents, EditReceipts};

use crate::state::{
    DrillState, GridConfig, MeasureMode, MeasureState, PlaybackState, ViewState, WorldProjection,
};

use crate::streets::StreetLabel;

// ---------------------------------------------------------------------------
// 缓存
// ---------------------------------------------------------------------------

/// 当前图的测量文档缓存（仿 `StreetLayerCache`，键 = map_id）。
#[derive(Resource, Default)]
pub struct MeasurementCache {
    /// 已加载的图 id；与当前图不同则整个缓存作废重载。
    pub map_id: Option<wv_core::Id>,
    /// 当前图的全部测量（空 overlay 兜底）。
    pub overlay: MeasurementOverlay,
}

impl MeasurementCache {
    /// 当前缓存里的 measurement 引用。
    pub fn measurements(&self) -> &[Measurement] {
        &self.overlay.measurements
    }
}

/// 每帧对齐缓存：当前图变了 / `World.measurements` 被编辑改过 → 从 `World` 重取。
///
/// 早退：同图且世界未被改过。`WorldProjection::is_changed` 已写进 `DirtySet` 的副作用。
pub fn reload_measurement_cache(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut cache: ResMut<MeasurementCache>,
) {
    let current = drill.current_node(&projection.world);
    let key = current.map(|n| n.id.clone());
    if cache.map_id == key && !projection.is_changed() {
        return;
    }
    cache.map_id = key.clone();
    cache.overlay = key
        .as_ref()
        .and_then(|id| projection.world.measurement_of(id).cloned())
        .unwrap_or_default();
}

// ---------------------------------------------------------------------------
// 渲染：折线 + 矩形 + 草稿 + 橡皮
// ---------------------------------------------------------------------------

/// 测量绘制层的 Z：压在「地名（0.4）」之上、给 `LayerFrame.z_offset` 漂移
/// 留 0.15 余地（实测最高 ~0.6）。用户要求「测量属于最上层」——这条规则
/// 与 z 选择兼容（顶置 + 还能被未来更高层打穿）。
const MEASURE_Z: f32 = 0.45;

/// 命中容差（世界单位）。`8.0` ≈ 1 栅格（`cell_size = 8` 默认）。
const ERASER_TOLERANCE_WU: f32 = 8.0;
/// 长度 / 面积文字的屏幕像素基准（2026-10-14 用户反馈 10 px 偏小，加到 13 px）。
const LABEL_PX: f32 = 13.0;

/// 折线标签（"段长" / "总长" / "长×宽=面积"）。
#[derive(Component)]
pub struct MeasureLabel;

/// 折线标签的描边阴影（黑色 4 方向实体，撑出 ~2 px 实心描边）。
///
/// 与 [`MeasureLabel`] **N : 4** 配对 spawn：每个标签 1 主文字 + 4 阴影实体，
/// 阴影分别位于主文字的左 / 右 / 上 / 下 1 屏幕 px 处，全不透明黑。
/// 形成 2 px 厚的描边，让白字在任何底色上都可读——尤其是黄色测量线穿过处
/// （用户 2026-10-14 反馈：单方向 1 px 半透明黑压不住黄底）。
///
/// `z` 比主文字略低，避免遮挡文字本身。
#[derive(Component)]
pub struct MeasureLabelShadow;

/// 测量文字标签的暗色背板（半透明黑矩形，铺在文字后一层，挡住穿过的黄线）。
///
/// 与 [`MeasureLabel`] **1 : 1** 配对 spawn：每个标签 1 主文字 + 1 背板实体。
///
/// ## 为什么需要背板（用户 2026-10-14 反馈）
///
/// Bevy 没有 CSS `mix-blend-mode: difference` 等价 API；想达到「文字明显
/// 盖在黄色线段上」的视觉效果，最简单等效做法就是：**背板在文字与线之间
/// 铺一块暗色矩形**，让黄线在背板区域被完全遮住。
///
/// 视觉等价于 difference：在文字区显示文字+背板，其他地方保留原色。
/// 渲染代价：每标签 +1 个 mesh 实体 + 1 个 ColorMaterial（小，可接受）。
#[derive(Component)]
pub struct MeasureLabelPlate;

/// 测量叠加的渲染入口（**顶置**，无 LayerFrame 联动）。
///
/// 把 cache + 草稿一起画：
/// - 完成 measurement（committed）：折线 + 矩形外框 + 文字标签；
/// - 草稿（draft）：同色减半，附加「末点 → 光标」的橡皮筋；
/// - 长度 / 面积：纯函数（`wv_core::Measurement::total_length`）手算。
/// - 文字标签带 **1px 描边**：每个标签实际 spawn 2 个 Text2d 实体——黑色阴影
///   在主文字右下方 1 屏幕 px，白色主文字在上。
/// - 文字标签背**暗色矩形背板**（[`MeasureLabelPlate`]）：覆盖穿过的黄线，
///   等效 mix-blend-mode: difference 的视觉效果。
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub fn render_measurements(
    mut commands: Commands,
    mut gizmos: Gizmos,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    cache: Res<MeasurementCache>,
    measure: Res<MeasureState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    fonts: Res<crate::theme::fonts::LoadedFonts>,
    camera: Query<&Transform, With<Camera2d>>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut q_main: Query<
        (
            Entity,
            &mut Transform,
            &mut Text2d,
            &mut TextFont,
            &mut TextColor,
            &mut Visibility,
        ),
        (
            With<MeasureLabel>,
            Without<MeasureLabelShadow>,
            Without<MeasureLabelPlate>,
            Without<Camera2d>,
            Without<StreetLabel>,
        ),
    >,
    mut q_plate: Query<
        (Entity, &mut Transform, &mut Mesh2d, &mut Visibility),
        (
            With<MeasureLabelPlate>,
            Without<MeasureLabel>,
            Without<MeasureLabelShadow>,
            Without<Camera2d>,
            Without<StreetLabel>,
        ),
    >,
    mut q_shadow: Query<
        (
            Entity,
            &mut Transform,
            &mut Text2d,
            &mut TextFont,
            &mut TextColor,
            &mut Visibility,
        ),
        (
            With<MeasureLabelShadow>,
            Without<MeasureLabel>,
            Without<MeasureLabelPlate>,
            Without<Camera2d>,
            Without<StreetLabel>,
        ),
    >,
) {
    // 顶置不受 LayerFrame.z_offset 漂移影响——这是用户原话「最上层」的强约束。

    let label_world = (LABEL_PX * view.scale).max(0.05);
    // 屏幕对齐：取相机旋转的逆，让标签永远水平（与街道标注同款）。
    let label_rot = camera
        .single()
        .map(|t| t.rotation.inverse())
        .unwrap_or(Quat::IDENTITY);

    // 顶置测量色：琥珀黄（避开主题 accent 蓝）。
    let color_done = bevy::color::Color::srgb(1.0, 0.78, 0.18);

    // 当前图的 km 系数（真图走 bbox，虚构地图走自报）。
    // 单位换算只读 [wv-core::MapNode::km_per_wu]，不写回、不进坐标链。
    let km_factor: Option<f64> = (|| {
        let node = drill.current_node(&projection.world)?;
        let (kx, ky) = node.km_per_wu()?;
        // 长度用均值（kx ≈ ky 的世界地图下够准）。
        Some((kx + ky) * 0.5)
    })();

    // ---- 期望标签集（位置 + 文本）----
    let mut wanted: Vec<(Vec2, String)> = Vec::new();
    for m in cache.measurements() {
        match m.kind {
            MeasurementKind::Polyline => {
                if m.points.len() < 2 {
                    continue;
                }
                for w in m.points.windows(2) {
                    let a = Vec2::new(w[0].x, w[0].y);
                    let b = Vec2::new(w[1].x, w[1].y);
                    let seg_len = (b - a).length();
                    let mid = grid.map_to_view((a + b) * 0.5);
                    wanted.push((mid, format_length_label(seg_len, km_factor)));
                }
                // 总长标在第一段中点上方。
                let first_mid = grid.map_to_view({
                    let a = Vec2::new(m.points[0].x, m.points[0].y);
                    let b = Vec2::new(m.points[1].x, m.points[1].y);
                    (a + b) * 0.5
                });
                let above = first_mid + Vec2::new(0.0, label_world * 1.2);
                wanted.push((
                    above,
                    format_total_length_label(m.total_length(), km_factor),
                ));
            }
            MeasurementKind::Rectangle => {
                if m.points.len() != 2 {
                    continue;
                }
                let a = Vec2::new(m.points[0].x, m.points[0].y);
                let b = Vec2::new(m.points[1].x, m.points[1].y);
                let dx = (b.x - a.x).abs();
                let dy = (b.y - a.y).abs();
                let mid = grid.map_to_view((a + b) * 0.5);
                wanted.push((mid, format_rectangle_label(dx, dy, km_factor)));
            }
        }
    }
    // ---- 草稿预览标签 ----
    if measure.mode != MeasureMode::Off && !measure.draft.is_empty() {
        for w in measure.draft.windows(2) {
            let a = w[0];
            let b = w[1];
            let seg_len = (b - a).length();
            let mid = grid.map_to_view((a + b) * 0.5);
            wanted.push((mid, format_length_label(seg_len, km_factor)));
        }
        if measure.draft.len() >= 2 {
            let a = measure.draft[0];
            let b = measure.draft[1];
            let total: f32 = measure
                .draft
                .windows(2)
                .map(|w| (w[1] - w[0]).length())
                .sum();
            let first_mid = grid.map_to_view((a + b) * 0.5);
            wanted.push((
                first_mid + Vec2::new(0.0, label_world * 1.2),
                format_total_length_label(total, km_factor),
            ));
        }
    }

    let font_size = label_world;
    let cjk = fonts.label_handle();
    let label_color = bevy::color::Color::WHITE;
    // 描边：全不透明黑（2026-10-14 用户反馈：半透明黑压不住黄底）。
    let shadow_color = bevy::color::Color::srgba(0.0, 0.0, 0.0, 1.0);
    // 4 方向描边（上下左右），每个方向 1 屏幕 px；屏幕 1 px = `view.scale` 世界单位。
    // 4 个方向叠加 → ~2 px 实心描边，让白字在任何底色（绿底/黄线）上都清晰。
    const SHADOW_OFFSETS: [(f32, f32); 4] = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)];

    // ---- 文字标签对象池（主 + 描边 + 背板）----
    // 每个 wanted 标签需要：
    // - 1 main 文字
    // - 4 shadow 文字（4 方向描边）
    // - 1 plate 矩形（不透明暗色背板，挡住穿过的黄线）
    // 阴影池大小 = 4N，背板池大小 = N，主 mesh 池大小 = N。
    let mut iter_main = q_main.iter_mut();
    let mut iter_shadow = q_shadow.iter_mut();
    let mut iter_plate = q_plate.iter_mut();
    let plate_color = bevy::color::Color::srgba(0.0, 0.0, 0.0, 0.85);
    for (pos, text) in &wanted {
        let text: &str = text.as_str();

        // 0. 背板（在文字与线之间，z = MEASURE_Z + 0.3 — 高于线 z=0，低于字 z=0.5）。
        let plate_size = estimate_text_world_size(text, font_size);
        let plate_mesh = meshes.add(Rectangle::new(plate_size.x, plate_size.y));
        let plate_material = materials.add(ColorMaterial {
            color: plate_color,
            ..Default::default()
        });
        if let Some((_, mut tf, mut m, mut vis)) = iter_plate.next() {
            tf.translation = pos.extend(MEASURE_Z + 0.3);
            tf.rotation = label_rot;
            tf.scale = Vec3::ONE;
            *m = Mesh2d(plate_mesh);
            *vis = Visibility::Visible;
        } else {
            commands.spawn((
                MeasureLabelPlate,
                Mesh2d(plate_mesh),
                MeshMaterial2d(plate_material),
                Transform {
                    translation: pos.extend(MEASURE_Z + 0.3),
                    rotation: label_rot,
                    ..Default::default()
                },
                Visibility::Visible,
            ));
        }

        // 1. 主文字（白色，z = MEASURE_Z + 0.5，确保明显压在线之上）
        if let Some((_, mut tf, mut t, mut style, mut color, mut vis)) = iter_main.next() {
            tf.translation = pos.extend(MEASURE_Z + 0.5);
            tf.rotation = label_rot;
            *t = Text2d::new(text.to_string());
            style.font_size = font_size.into();
            if let Some(f) = &cjk {
                style.font = f.clone().into();
            }
            *color = TextColor(label_color);
            *vis = Visibility::Visible;
        } else {
            commands.spawn((
                MeasureLabel,
                Text2d::new(text.to_string()),
                TextFont {
                    font_size: font_size.into(),
                    font: cjk.clone().unwrap_or_default().into(),
                    ..Default::default()
                },
                TextColor(label_color),
                Transform {
                    translation: pos.extend(MEASURE_Z + 0.5),
                    rotation: label_rot,
                    ..Default::default()
                },
                Visibility::Visible,
            ));
        }

        // 2. 描边阴影（4 方向 1 像素，全不透明黑，z = MEASURE_Z + 0.4 — 紧贴主字下层）
        for (dx, dy) in SHADOW_OFFSETS {
            let shadow_pos = pos + Vec2::new(dx * view.scale, dy * view.scale);
            if let Some((_, mut tf, mut t, mut style, mut color, mut vis)) = iter_shadow.next() {
                tf.translation = shadow_pos.extend(MEASURE_Z + 0.4);
                tf.rotation = label_rot;
                *t = Text2d::new(text.to_string());
                style.font_size = font_size.into();
                if let Some(f) = &cjk {
                    style.font = f.clone().into();
                }
                *color = TextColor(shadow_color);
                *vis = Visibility::Visible;
            } else {
                commands.spawn((
                    MeasureLabelShadow,
                    Text2d::new(text.to_string()),
                    TextFont {
                        font_size: font_size.into(),
                        font: cjk.clone().unwrap_or_default().into(),
                        ..Default::default()
                    },
                    TextColor(shadow_color),
                    Transform {
                        translation: shadow_pos.extend(MEASURE_Z + 0.4),
                        rotation: label_rot,
                        ..Default::default()
                    },
                    Visibility::Visible,
                ));
            }
        }
    }
    // 池里多出的实体隐藏（复用，不 despawn）。
    for (_, mut _tf, mut _t, mut _s, mut _c, mut vis) in iter_main {
        *vis = Visibility::Hidden;
    }
    for (_, mut _tf, mut _t, mut _s, mut _c, mut vis) in iter_shadow {
        *vis = Visibility::Hidden;
    }
    for (_, mut _tf, mut _m, mut vis) in iter_plate {
        let _ = _m;
        *vis = Visibility::Hidden;
    }

    // ---- 几何绘制（Gizmos）----
    let draft_color_dim = color_done.with_alpha(0.7);

    // 已提交的 measurement：折线 / 矩形外框。
    for m in cache.measurements() {
        match m.kind {
            MeasurementKind::Polyline => {
                if m.points.len() < 2 {
                    continue;
                }
                for w in m.points.windows(2) {
                    let a = grid.map_to_view(Vec2::new(w[0].x, w[0].y));
                    let b = grid.map_to_view(Vec2::new(w[1].x, w[1].y));
                    gizmos.line_2d(a, b, color_done);
                }
            }
            MeasurementKind::Rectangle => {
                if m.points.len() != 2 {
                    continue;
                }
                let a = Vec2::new(m.points[0].x, m.points[0].y);
                let b = Vec2::new(m.points[1].x, m.points[1].y);
                let corners = [
                    Vec2::new(a.x.min(b.x), a.y.min(b.y)),
                    Vec2::new(a.x.max(b.x), a.y.min(b.y)),
                    Vec2::new(a.x.max(b.x), a.y.max(b.y)),
                    Vec2::new(a.x.min(b.x), a.y.max(b.y)),
                ];
                let corners_v: Vec<Vec2> = corners.iter().map(|c| grid.map_to_view(*c)).collect();
                for w in corners_v.windows(2) {
                    gizmos.line_2d(w[0], w[1], color_done);
                }
                gizmos.line_2d(corners_v[3], corners_v[0], color_done);
            }
        }
    }
    // 草稿已固定的段（同色减半）。
    for w in measure.draft.windows(2) {
        gizmos.line_2d(
            grid.map_to_view(w[0]),
            grid.map_to_view(w[1]),
            draft_color_dim,
        );
    }
    // 草稿橡皮筋（末段 → 光标）：仅 Line / Rect 模式有。
    if (measure.mode == MeasureMode::Line || measure.mode == MeasureMode::Rect)
        && !measure.draft.is_empty()
    {
        if let Some(cursor) = view.cursor_world {
            gizmos.line_2d(
                grid.map_to_view(*measure.draft.last().unwrap()),
                cursor,
                draft_color_dim,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 输入：把鼠标点击派发到三模式（折线 / 矩形 / 橡皮）
// ---------------------------------------------------------------------------

/// 把光标世界坐标 → 本图局部（与街道同款：`drill::map_origin`）。
fn cursor_to_local(
    cursor_world: Vec2,
    drill: &DrillState,
    projection: &WorldProjection,
    grid: &GridConfig,
) -> Option<(Vec2, wv_core::Id)> {
    let node = drill.current_node(&projection.world)?;
    let origin = crate::drill::map_origin(&projection.world, node, grid);
    Some((cursor_world - origin, node.id.clone()))
}

/// 点到测量图形的最短距离（世界单位）；图形退化到无有效段时返回 `∞`
/// （= 任何有限容差都不命中）。
///
/// 这是**唯一的命中几何**：测量橡皮（点选）与 `Del`（光标下删除）共用它，
/// 命中判定与「最近一条」排序因此不会各写一套、日后漂移。
fn measurement_distance(m: &Measurement, point: Vec2) -> f32 {
    match m.kind {
        MeasurementKind::Polyline => {
            if m.points.is_empty() {
                return f32::INFINITY;
            }
            if m.points.len() == 1 {
                let a = Vec2::new(m.points[0].x, m.points[0].y);
                return (point - a).length();
            }
            m.points
                .windows(2)
                .map(|w| {
                    point_segment_distance(
                        point,
                        Vec2::new(w[0].x, w[0].y),
                        Vec2::new(w[1].x, w[1].y),
                    )
                })
                .fold(f32::INFINITY, f32::min)
        }
        MeasurementKind::Rectangle => {
            if m.points.len() != 2 {
                return f32::INFINITY;
            }
            let a = Vec2::new(m.points[0].x, m.points[0].y);
            let b = Vec2::new(m.points[1].x, m.points[1].y);
            let corners = [
                Vec2::new(a.x.min(b.x), a.y.min(b.y)),
                Vec2::new(a.x.max(b.x), a.y.min(b.y)),
                Vec2::new(a.x.max(b.x), a.y.max(b.y)),
                Vec2::new(a.x.min(b.x), a.y.max(b.y)),
            ];
            // 四条边闭合（bottom / right / top / left）。
            (0..4)
                .map(|i| point_segment_distance(point, corners[i], corners[(i + 1) % 4]))
                .fold(f32::INFINITY, f32::min)
        }
    }
}

/// 容差内**最近**的一条测量（`Del` 与橡皮共用）。
///
/// 按距离取最近而不是「先命中先赢」：两条测量交叉处，光标更靠近哪条就删哪条
/// ——否则删除结果取决于 YAML 里的声明顺序，作者无法预期。
fn nearest_measurement(
    world: &wv_core::World,
    map_id: &wv_core::Id,
    point: Vec2,
    tolerance: f32,
) -> Option<wv_core::Id> {
    let overlay = world.measurement_of(map_id)?;
    overlay
        .measurements
        .iter()
        .filter_map(|m| {
            let d = measurement_distance(m, point);
            (d <= tolerance).then_some((d, m.id.clone()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// 点到线段距离（闭区间）。
fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let projection = a + ab * t;
    (p - projection).length()
}

/// 生成下一条 measurement 的 ID（`measure_<n>`），`n` 取当前已用最大 + 1。
fn next_measurement_id(world: &wv_core::World, map_id: &wv_core::Id) -> wv_core::Id {
    let n = world
        .measurement_of(map_id)
        .map(|o| o.measurements.len())
        .unwrap_or(0);
    wv_core::Id::new_unchecked(format!("measure_{n}"))
}

/// 把当前点击提交为测量命令。
///
/// - Line 模式：累积点。**第二次**点击开始提交「上一段 → 本段」= 新 measurement。
///   简单起见，这里采用「每次点击都新建一条 polyline」的策略——多段折线
///   表达为多条独立 polyline 的串，渲染层各自画各自。后续要做「合并成一条」
///   时改 `commit` 即可。
/// - Rect 模式：第二次点击立刻提交 Rectangle 并清草稿。
/// - Eraser 模式：点中哪条删哪条（容差 `ERASER_TOLERANCE_WU`）。
/// - 右键 / Esc：取消当前模式（回 `Off`），草稿丢弃。
///
/// 系统纪律（与街道 / 笔刷同款）：
/// - enabled 闸（`mode != Off`）；
/// - 不悬 UI / 不在播放 / 不在过低缩放；
/// - 无工程闸门；
/// - 经 [`crate::editing_bridge`] 意图提交（wv-app 翻译成 `EditCommand`），不直改 `World`；
/// - 落点要 `cursor_world` 存在。
#[allow(clippy::too_many_arguments)]
pub fn commit_measurement_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    mut measure: ResMut<MeasureState>,
    mut intents: ResMut<EditIntents>,
    mut acks: ResMut<MeasureCommitAcks>,
) {
    let mode = measure.mode;
    if mode == MeasureMode::Off {
        return;
    }
    // 切图后强制清草稿（避免上一张图的草稿污染当前图）。
    let current_map = drill.current_node(&projection.world).map(|n| n.id.clone());
    measure.on_map_change(current_map.as_ref());
    if mode == MeasureMode::Line
        && measure.draft_map_id.is_some()
        && current_map.as_ref() != Some(measure.draft_map_id.as_ref().unwrap())
    {
        measure.clear_draft();
    }

    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }

    // 右键 = 退出模式（清草稿）。
    if buttons.just_pressed(MouseButton::Right) {
        measure.clear_draft();
        measure.mode = MeasureMode::Off;
        return;
    }
    // Esc 也退出（key input）。
    if keys.just_pressed(KeyCode::Escape) {
        measure.clear_draft();
        measure.mode = MeasureMode::Off;
        return;
    }
    // Enter 在 Line 模式 = 收笔（提交首段就停，后续由用户手动新建下一条）。
    if mode == MeasureMode::Line && keys.just_pressed(KeyCode::Enter) {
        measure.clear_draft();
        measure.mode = MeasureMode::Off;
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor_world) = view.cursor_world else {
        return;
    };
    let Some((local, map_id)) = cursor_to_local(cursor_world, &drill, &projection, &grid) else {
        return;
    };

    match mode {
        MeasureMode::Off => unreachable!(),
        MeasureMode::Line => {
            // 第一次点击：仅记录草稿起笔。
            if measure.draft.is_empty() {
                measure.draft.push(local);
                measure.draft_map_id = Some(map_id.clone());
                return;
            }
            // 第二次起：提交「上一段 → 本段」= 折线一条。
            let prev = *measure.draft.last().expect("已保证非空");
            let seg = wv_core::Measurement::polyline(vec![
                wv_core::Point::new(prev.x, prev.y),
                wv_core::Point::new(local.x, local.y),
            ])
            .with_id(next_measurement_id(&projection.world, &map_id));
            let seq = intents.push_measurement(crate::editing_bridge::MeasurementIntent {
                map_id: map_id.clone(),
                op: crate::editing_bridge::MeasurementOp::Add(seg),
            });
            acks.0.push((
                seq,
                MeasureAck::LinePush {
                    local,
                    map_id: map_id.clone(),
                },
            ));
        }
        MeasureMode::Rect => {
            // 第一次点击：草稿起笔。
            if measure.draft.is_empty() {
                measure.draft.push(local);
                measure.draft_map_id = Some(map_id.clone());
                return;
            }
            // 第二次点击：提交矩形并清草稿。
            let prev = *measure.draft.last().expect("已保证非空");
            let m = wv_core::Measurement::rectangle(
                wv_core::Point::new(prev.x, prev.y),
                wv_core::Point::new(local.x, local.y),
            )
            .with_id(next_measurement_id(&projection.world, &map_id));
            let seq = intents.push_measurement(crate::editing_bridge::MeasurementIntent {
                map_id,
                op: crate::editing_bridge::MeasurementOp::Add(m),
            });
            acks.0.push((seq, MeasureAck::RectClear));
        }
        MeasureMode::Eraser => {
            // 找容差内最近的一条 measurement → 删（与 `Del` 共用
            // `nearest_measurement`，两处删除口径一致）。
            if let Some(id) =
                nearest_measurement(&projection.world, &map_id, local, ERASER_TOLERANCE_WU)
            {
                intents.push_measurement(crate::editing_bridge::MeasurementIntent {
                    map_id,
                    op: crate::editing_bridge::MeasurementOp::Remove(id),
                });
            }
        }
    }
}

/// `Del`：删除**光标下**的那条测量（2026-10-23 用户需求）。
///
/// 与 [`commit_measurement_click`] 的橡皮分支共用同一条命中几何
/// （[`nearest_measurement`]），差别只在触发方式与守卫：
/// - 不需要先切到测量工具（`MeasureMode::Off` 时也生效）——「看到一条测量，
///   想删掉它」不应先被要求进某个模式；
/// - 多守一条 `wants_keyboard`：`Del` 也是打字键，作者在输入框里改名时
///   绝不能顺手删掉图上的测量。
///
/// 容差随缩放走（屏幕上约 6 px），并以 [`ERASER_TOLERANCE_WU`] 为下限——
/// 缩得很小时仍有一格的手感，放大时不会出现「离得老远也算命中」。
///
/// 删除经 [`EditIntents`] 交给应用层提交，**不**直改 `World`（§3.3），
/// 因此 `Ctrl+Z` 可逐步撤销。
#[allow(clippy::too_many_arguments)]
pub fn delete_measurement_under_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    mut intents: ResMut<EditIntents>,
) {
    if !keys.just_pressed(KeyCode::Delete) {
        return;
    }
    if ui.wants_pointer || ui.wants_keyboard || playback.playing || view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }
    let Some(cursor_world) = view.cursor_world else {
        return;
    };
    let Some((local, map_id)) = cursor_to_local(cursor_world, &drill, &projection, &grid) else {
        return;
    };
    const PICK_PX: f32 = 6.0;
    let tolerance = ERASER_TOLERANCE_WU.max(PICK_PX * view.scale);
    if let Some(id) = nearest_measurement(&projection.world, &map_id, local, tolerance) {
        intents.push_measurement(crate::editing_bridge::MeasurementIntent {
            map_id,
            op: crate::editing_bridge::MeasurementOp::Remove(id),
        });
    }
}

/// 待回执的本地动作：提交**成功**后由 [`apply_measurement_receipts`] 回填。
#[derive(Debug, Clone)]
enum MeasureAck {
    /// Line：草稿推进到 `local`（并绑定 `map_id`）。
    LinePush { local: Vec2, map_id: wv_core::Id },
    /// Rect：两点齐，清草稿。
    RectClear,
}

/// 测量提交的挂起回执队列（`commit_measurement_click` 写、`apply_measurement_receipts` 读）。
#[derive(Resource, Default)]
pub struct MeasureCommitAcks(Vec<(u64, MeasureAck)>);

/// 消费编辑回执：**提交成功才**推进测量草稿——失败不动本地，与拆 crate 前行为一致
/// （ADR-0020 §2.5 / 闸门 8）。必须排在 `wv-app` 翻译器之后跑（顺序由 app_bootstrap 钉死）。
pub fn apply_measurement_receipts(
    mut receipts: ResMut<EditReceipts>,
    mut acks: ResMut<MeasureCommitAcks>,
    mut measure: ResMut<MeasureState>,
) {
    for (seq, ack) in std::mem::take(&mut acks.0) {
        if !receipts.take(seq) {
            continue; // 提交失败 / 未回执：与旧行为一致，不推进草稿。
        }
        match ack {
            MeasureAck::LinePush { local, map_id } => {
                measure.draft.push(local);
                measure.draft_map_id = Some(map_id);
            }
            MeasureAck::RectClear => measure.clear_draft(),
        }
    }
}

// ---------------------------------------------------------------------------
// 格式化辅助：长度 / 面积 / km 换算
// ---------------------------------------------------------------------------

/// 把 wu 长度按 `km_factor` 折算成 km。`wu < 0` 视为非法（不会发生）。
#[inline]
fn wu_to_km(wu: f32, km_factor: f64) -> f64 {
    f64::from(wu) * km_factor
}

/// 按字符数粗估文字渲染尺寸（**世界单位**）。
///
/// 不读 `TextLayoutInfo`（存在布局时机竞争），用字符数 × 平均字符宽近似。
/// CJK 与 ASCII 字符宽差异通过「CJK 算 1 单位，ASCII 算 0.6 单位」加权——
/// 估算的宽度略大于真实宽度（避免文字底边露线），高度按行数 × 1.3 字高。
///
/// ## 为什么不用精确测量
///
/// Bevy 的 `TextLayoutInfo` 由 text 系统异步写入，与 render 系统同帧竞争。
/// 第一次 spawn 时拿到的可能是上一帧的缓存值，导致抖动。本估算得是确定性的，
/// 视觉上误差 ±2 px（小于 4 方向描边的总厚度），不会产生抖动。
fn estimate_text_world_size(text: &str, font_world_size: f32) -> Vec2 {
    let line_count = text.matches('\n').count() + 1;
    // 最长一行的「等效字符宽度」（世界单位）：
    // - CJK 字符宽度 ≈ 1.0 × font_world_size
    // - ASCII 字符宽度 ≈ 0.6 × font_world_size
    // 实测字体宽度 ≈ font_world_size × 0.55（Helvetica-like），CJK 字宽 ≈ 1.0×font。
    // 这里取 ASCII ≈ 0.55，CJK ≈ 1.0。
    const ASCII_WIDTH_RATIO: f32 = 0.55;
    const CJK_WIDTH_RATIO: f32 = 1.0;
    let max_eq_width = text
        .lines()
        .map(|line| {
            line.chars()
                .map(|c| {
                    let code = c as u32;
                    let ratio = if (0x4E00..=0x9FFF).contains(&code)
                        || (0x3000..=0x303F).contains(&code)
                        || (0xFF00..=0xFFEF).contains(&code)
                    {
                        CJK_WIDTH_RATIO
                    } else {
                        ASCII_WIDTH_RATIO
                    };
                    ratio * font_world_size
                })
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max);
    // 横向 padding（左右各一点）。
    let pad_x = font_world_size * 0.4;
    // 高度 = 行数 × 字高（×1.3 含上下 padding）。
    let height = font_world_size * 1.3 * line_count as f32;
    Vec2::new(max_eq_width + pad_x, height)
}

/// 单段长度标签（折线）。
///
/// 无 km： `"5.0"`
/// 有 km： `"5.0 wu\n(8.2 km)"`（用换行让 km 显示在第二行，避免水平加宽）。
pub(crate) fn format_length_label(wu: f32, km_factor: Option<f64>) -> String {
    match km_factor {
        None => format!("{wu:.1}"),
        Some(k) => format!("{wu:.1} wu\n({:.2} km)", wu_to_km(wu, k)),
    }
}

/// 总长标签（折线 Σ）。
pub(crate) fn format_total_length_label(wu: f32, km_factor: Option<f64>) -> String {
    match km_factor {
        None => format!("Σ {wu:.1}"),
        Some(k) => format!("Σ {wu:.1} wu\n({:.2} km)", wu_to_km(wu, k)),
    }
}

/// 矩形标签：宽 × 高 = 面积（wu² 与 km² 同时显示）。
pub(crate) fn format_rectangle_label(
    width_wu: f32,
    height_wu: f32,
    km_factor: Option<f64>,
) -> String {
    let area_wu2 = width_wu * height_wu;
    match km_factor {
        None => format!("{width_wu:.1} × {height_wu:.1} = {area_wu2:.1}"),
        Some(k) => {
            let w_km = wu_to_km(width_wu, k);
            let h_km = wu_to_km(height_wu, k);
            let area_km2 = w_km * h_km;
            format!("{width_wu:.1} × {height_wu:.1} wu\n= {area_wu2:.1} wu² ({area_km2:.3} km²)")
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_segment_distance_handles_collinear_and_perpendicular() {
        let a = Vec2::ZERO;
        let b = Vec2::new(10.0, 0.0);
        // 端点上距离 = 0。
        assert!(point_segment_distance(Vec2::new(0.0, 0.0), a, b) < 1e-4);
        assert!(point_segment_distance(Vec2::new(10.0, 0.0), a, b) < 1e-4);
        // 垂直投影在中点，距离 = 3。
        assert!((point_segment_distance(Vec2::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-4);
        // 中点距离 = 3。
        assert!((point_segment_distance(Vec2::new(5.0, -3.0), a, b) - 3.0).abs() < 1e-4);
    }

    #[test]
    fn point_segment_distance_zero_length_segment_uses_point_distance() {
        let a = Vec2::new(1.0, 1.0);
        let b = a;
        assert!((point_segment_distance(Vec2::new(4.0, 5.0), a, b) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn hit_test_polyline_hits_within_tolerance() {
        // L 形折线 (0,0) → (10,0) → (10,5)
        let m = wv_core::Measurement::polyline(vec![
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(10.0, 0.0),
            wv_core::Point::new(10.0, 5.0),
        ]);
        // 垂直偏移 4（容差 5）
        assert!(measurement_distance(&m, Vec2::new(5.0, 4.0)) <= 5.0);
        // 偏移 6（容差 5） → 不命中
        assert!(measurement_distance(&m, Vec2::new(5.0, 6.0)) > 5.0);
    }

    #[test]
    fn hit_test_rectangle_hits_border_not_inside() {
        let m = wv_core::Measurement::rectangle(
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(10.0, 5.0),
        );
        // 边框上：命中。
        assert!(measurement_distance(&m, Vec2::new(0.0, 2.0)) <= 0.5);
        assert!(measurement_distance(&m, Vec2::new(10.0, 2.0)) <= 0.5);
        // 内部：与边框距离 = 2.5 > 容差 0.5 → 不命中。
        assert!(measurement_distance(&m, Vec2::new(5.0, 2.5)) > 0.5);
    }

    /// [`measurement_distance`] 是命中几何的唯一实现：折线取所有段的最近距离，
    /// 矩形只算四条边（量的是「框」不是「面」）；退化图形返回 `∞`。
    #[test]
    fn measurement_distance_covers_polyline_and_rectangle() {
        let poly = wv_core::Measurement::polyline(vec![
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(10.0, 0.0),
        ]);
        assert!((measurement_distance(&poly, Vec2::new(5.0, 3.0)) - 3.0).abs() < 1e-4);

        let rect = wv_core::Measurement::rectangle(
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(10.0, 5.0),
        );
        assert!((measurement_distance(&rect, Vec2::new(5.0, 2.5)) - 2.5).abs() < 1e-4);

        let degenerate = wv_core::Measurement::polyline(vec![]);
        assert!(
            measurement_distance(&degenerate, Vec2::ZERO).is_infinite(),
            "空折线应与任何点都无命中"
        );
    }

    /// `Del` 与橡皮共用的选择逻辑：交叉处删**更近**的那条，而不是先声明的
    /// 那条——否则删除结果取决于 YAML 里的顺序，作者无法预期。
    #[test]
    fn nearest_measurement_prefers_the_closer_line_at_a_crossing() {
        let map = wv_core::Id::new_unchecked("m");
        let mut world = wv_core::World::default();
        {
            let ov = world.measurement_of_mut(&map);
            ov.add(
                wv_core::Measurement::polyline(vec![
                    wv_core::Point::new(0.0, 0.0),
                    wv_core::Point::new(100.0, 0.0),
                ])
                .with_id(wv_core::Id::new_unchecked("horiz")),
            )
            .unwrap();
            ov.add(
                wv_core::Measurement::polyline(vec![
                    wv_core::Point::new(50.0, -100.0),
                    wv_core::Point::new(50.0, 100.0),
                ])
                .with_id(wv_core::Id::new_unchecked("vert")),
            )
            .unwrap();
        }
        // 略偏上（x=50, y=4）：竖线距离 0、横线 4 → 竖线。
        let pick = nearest_measurement(&world, &map, Vec2::new(50.0, 4.0), 8.0);
        assert_eq!(pick.as_ref().map(|i| i.as_str()), Some("vert"));
        // 略偏右（x=56, y=0）：横线 0、竖线 6 → 横线。
        let pick = nearest_measurement(&world, &map, Vec2::new(56.0, 0.0), 8.0);
        assert_eq!(pick.as_ref().map(|i| i.as_str()), Some("horiz"));
        // 两条都在容差外（(200,40) 离横线端点 ≈108、离竖线 150）→ 无命中
        // （不能「附近有就删」）。
        assert!(nearest_measurement(&world, &map, Vec2::new(200.0, 40.0), 8.0).is_none());
    }

    /// 系统级（2026-10-23）：`Del` 命中即产出一条 `Remove` 意图；
    /// 输入框聚焦（`wants_keyboard`）时一条都不产——否则作者改名字时
    /// 顺手就把图上的测量删了。无工程 / 播放中同样静默。
    #[test]
    fn delete_key_pushes_a_remove_intent_only_when_not_typing() {
        use bevy::ecs::system::RunSystemOnce;

        let map = wv_core::Id::new_unchecked("map_root");
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            name: "root".into(),
            ..Default::default()
        });
        world
            .measurement_of_mut(&map)
            .add(
                wv_core::Measurement::polyline(vec![
                    wv_core::Point::new(0.0, 0.0),
                    wv_core::Point::new(100.0, 0.0),
                ])
                .with_id(wv_core::Id::new_unchecked("measure_0")),
            )
            .unwrap();

        let grid = GridConfig::default();
        // 本图原点 → 世界坐标（坐标链与 `cursor_to_local` 同源，测试不自造换算）。
        let origin = {
            let node = world.maps.iter().find(|m| m.id == map).unwrap();
            crate::drill::map_origin(&world, node, &grid)
        };

        let mut app_world = bevy::prelude::World::default();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        app_world.insert_resource(WorldProjection {
            world,
            project_dir: Some("/test/project".into()),
            ..WorldProjection::default()
        });
        app_world.insert_resource(drill);
        app_world.insert_resource(ButtonInput::<KeyCode>::default());
        app_world.insert_resource(ViewState {
            scale: 1.0,
            // 光标落在折线 (0,0)-(100,0) 上（偏离 2 wu，容差内）。
            cursor_world: Some(origin + Vec2::new(50.0, 2.0)),
            ..ViewState::default()
        });
        app_world.insert_resource(PlaybackState::default());
        app_world.insert_resource(crate::camera::PointerOverUi::default());
        app_world.insert_resource(grid);
        app_world.insert_resource(EditIntents::default());

        // ① 正常：Del → 一条 Remove，指向命中那条。
        app_world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Delete);
        app_world
            .run_system_once(delete_measurement_under_cursor)
            .expect("系统运行失败");
        let drained = app_world.resource_mut::<EditIntents>().drain_measurements();
        assert_eq!(drained.len(), 1, "Del 命中应产出 1 条测量意图");
        match &drained[0].1.op {
            crate::editing_bridge::MeasurementOp::Remove(id) => {
                assert_eq!(id.as_str(), "measure_0", "应删除光标下那条")
            }
            other => panic!("Del 应产出 Remove，实得 {other:?}"),
        }

        // ② 输入框聚焦：`wants_keyboard` → 不产意图（Del 归文字编辑）。
        let mut keys = app_world.resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::Delete);
        keys.clear();
        keys.press(KeyCode::Delete);
        app_world
            .resource_mut::<crate::camera::PointerOverUi>()
            .wants_keyboard = true;
        app_world
            .run_system_once(delete_measurement_under_cursor)
            .expect("系统运行失败");
        assert!(
            app_world
                .resource_mut::<EditIntents>()
                .drain_measurements()
                .is_empty(),
            "输入框聚焦时 Del 不得删除测量"
        );
    }

    #[test]
    fn next_measurement_id_starts_at_zero_for_a_fresh_map() {
        let world = wv_core::World::default();
        let id = next_measurement_id(&world, &wv_core::Id::new_unchecked("m"));
        assert_eq!(id.as_str(), "measure_0");
    }

    #[test]
    fn next_measurement_id_increments_after_appending() {
        let mut world = wv_core::World::default();
        let map = wv_core::Id::new_unchecked("m");
        let m = wv_core::Measurement::polyline(vec![
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(1.0, 1.0),
        ])
        .with_id(wv_core::Id::new_unchecked("measure_0"));
        world.measurement_of_mut(&map).add(m).unwrap();
        let id = next_measurement_id(&world, &map);
        assert_eq!(id.as_str(), "measure_1");
    }

    #[test]
    fn measurement_cache_tracks_current_map_id() {
        let mut cache = MeasurementCache::default();
        assert!(cache.map_id.is_none());
        cache.map_id = Some(wv_core::Id::new_unchecked("m1"));
        assert_eq!(cache.map_id.as_ref().unwrap().as_str(), "m1");
    }

    #[test]
    fn measure_state_clear_draft_resets_everything() {
        let mut st = MeasureState {
            mode: MeasureMode::Line,
            draft: vec![Vec2::new(1.0, 1.0)],
            last_map_id: Some(wv_core::Id::new_unchecked("m1")),
            draft_map_id: Some(wv_core::Id::new_unchecked("m1")),
        };
        st.clear_draft();
        assert!(st.draft.is_empty());
        assert!(st.draft_map_id.is_none());
        // last_map_id 不动——切图判断需要它。
        assert!(st.last_map_id.is_some());
    }

    #[test]
    fn measure_state_on_map_change_clears_when_map_differs() {
        let mut st = MeasureState {
            mode: MeasureMode::Line,
            draft: vec![Vec2::new(1.0, 1.0)],
            last_map_id: Some(wv_core::Id::new_unchecked("m1")),
            draft_map_id: Some(wv_core::Id::new_unchecked("m1")),
        };
        st.on_map_change(Some(&wv_core::Id::new_unchecked("m2")));
        assert!(st.draft.is_empty(), "切图应清草稿");
        assert_eq!(st.last_map_id.as_ref().unwrap().as_str(), "m2");
    }

    #[test]
    fn measure_state_on_map_change_keeps_when_same() {
        let mut st = MeasureState {
            mode: MeasureMode::Line,
            draft: vec![Vec2::new(1.0, 1.0)],
            last_map_id: Some(wv_core::Id::new_unchecked("m1")),
            draft_map_id: Some(wv_core::Id::new_unchecked("m1")),
        };
        st.on_map_change(Some(&wv_core::Id::new_unchecked("m1")));
        assert_eq!(st.draft.len(), 1, "同图不清草稿");
    }

    #[test]
    fn format_length_label_no_km() {
        assert_eq!(format_length_label(5.0, None), "5.0");
        assert_eq!(format_length_label(12.34, None), "12.3");
    }

    #[test]
    fn estimate_text_world_size_handles_cjk_vs_ascii() {
        let font = 13.0_f32;
        // ASCII 6 字符：6 × 0.55 × 13 ≈ 42.9 + padding ≈ 48。
        let ascii = estimate_text_world_size("abc 12", font);
        assert!(ascii.x < 60.0, "ASCII 字符估宽应较窄: {}", ascii.x);
        assert!(ascii.x > 40.0, "ASCII 应有合理宽度: {}", ascii.x);
        // CJK 6 字：6 × 1.0 × 13 ≈ 78——显著宽于 ASCII。
        let cjk = estimate_text_world_size("测量三段折线", font);
        assert!(
            cjk.x > ascii.x * 1.3,
            "CJK 应显著宽于 ASCII: cjk={} ascii={}",
            cjk.x,
            ascii.x
        );
        // 多行：高度按行数倍。
        let multi = estimate_text_world_size("5.0 wu\n(8.20 km)", font);
        assert!(
            multi.y > ascii.y,
            "多行标签应比单行高: multi={} single={}",
            multi.y,
            ascii.y
        );
    }

    #[test]
    fn format_length_label_with_km_uses_two_lines() {
        // 8 wu，km_factor = 1.0 → 8.0 km。
        let label = format_length_label(8.0, Some(1.0));
        assert!(label.contains("wu"), "应显示 wu 单位: {label}");
        assert!(label.contains("km"), "应显示 km 换算: {label}");
        assert!(label.contains('\n'), "wu 与 km 应分两行: {label}");
        assert!(label.contains("8.00"), "km 应保留 2 位小数: {label}");
    }

    #[test]
    fn format_total_length_label_includes_sigma() {
        let none = format_total_length_label(15.0, None);
        assert!(none.starts_with('Σ') && none.contains("15.0"));
        let some = format_total_length_label(15.0, Some(2.0));
        assert!(some.starts_with('Σ'));
        assert!(some.contains("km"));
        assert!(some.contains("30.00"), "15 * 2.0 = 30 km");
    }

    #[test]
    fn format_rectangle_label_no_km_uses_wu_squared() {
        let s = format_rectangle_label(3.0, 4.0, None);
        assert!(s.contains("3.0 × 4.0"), "应含宽 × 高: {s}");
        assert!(s.contains("12.0"), "面积 12 wu²：{s}");
    }

    #[test]
    fn format_rectangle_label_with_km_uses_both_units() {
        // km_factor = 1.0：3 wu = 3 km，4 wu = 4 km，面积 12 km²。
        let s = format_rectangle_label(3.0, 4.0, Some(1.0));
        assert!(s.contains("wu"), "应含 wu: {s}");
        assert!(s.contains("km²"), "应含 km²: {s}");
        assert!(s.contains("12.000"), "面积 12.000 km²：{s}");
    }

    #[test]
    fn measure_mode_is_active_and_consumes_left_click() {
        assert!(!MeasureMode::Off.is_active());
        assert!(MeasureMode::Line.is_active());
        assert!(MeasureMode::Rect.is_active());
        assert!(MeasureMode::Eraser.is_active());
        assert!(!MeasureMode::Off.consumes_left_click());
        assert!(MeasureMode::Line.consumes_left_click());
        assert!(MeasureMode::Rect.consumes_left_click());
        // 橡皮不消费——它的左键仍要走地图？
        // 当前实现里橡皮模式不消费左键——它由本系统处理，不是地图。
        // 但「消费」语义保留 Eraser 也算合理。
        assert!(!MeasureMode::Eraser.consumes_left_click());
    }
}
