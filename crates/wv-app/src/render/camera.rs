//! 相机、缩放、平移（S4）与笔刷拾取（S5）。
//!
//! ## 职责
//! - 处理 Bevy `Camera2d` 的缩放、平移；为 geo 坐标 ↔ 屏幕坐标提供 `ProjectionFrame`。
//! - 收集 `ActiveStroke`：按住鼠标拖拽时把世界坐标点列进栈，
//!   左键释放后由 [`crate::editing`] 把这一段落笔画成 `EditCommand`。
//!
//! ## 边界（不做什么）
//! - **不直接改世界**——笔刷拖拽只收集笔触，松手后才落命令（AGENTS §3.3）。
//! - **不吃落在 egui 面板上的输入**——查 `EguiContexts::ctx_mut().egui_wants_pointer_input()`
//!   让面板优先（基线 §2.13）。
//! - **零惯性 / 无缓动**（ANIMATION_SPEC AD-3 / §4.3）——编辑要指哪打哪。
//!
//! ## 主要导出
//! - `apply_view_to_camera` — 把 `ViewState` 写进 `Transform` / `Projection`。
//! - `collect_stroke_points` — 笔刷拾取 / 笔触收集。
//! - `ActiveStroke` 组件、`CameraZoom` 资源。

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::state::{BrushState, DrillState, GridConfig, ViewState, WorldProjection};

// 缩放区间不再用全局常量：它按**当前级**（`drill` 所在那张图的 `extent`）实时算，
// 见 `crate::zoom_level` 与 ADR-0009 §2 决策 ⑤。历史上的
// `SCALE_MIN = 0.1` / `SCALE_MAX = 8.0` 已删除——它们是单级地图时代的产物，
// 在分级缩放里会把深处挡在门外（世界级的外界 8.0 与历史值逐值相同，
// 由 `LEVEL_OVERVIEW` 标定保证，见 `zoom_level.rs`）。

/// 正在拖拽的笔触（世界坐标点列）。松手时交给 `editing` 生成命令。
#[derive(Resource, Default)]
pub struct ActiveStroke {
    pub points: Vec<Vec2>,
}

/// 写相机投影的缩放。非正交投影静默忽略（编辑器不会出现，且此处不该 panic）。
pub(crate) fn set_ortho_scale(projection: &mut Projection, scale: f32) {
    if let Projection::Orthographic(ortho) = projection {
        ortho.scale = scale;
    }
}

/// 把 `ViewState` 的外部写点落到相机（主真源）。
///
/// 小地图点击 / FitMap / Zoom 命令 / 校验锚点跳转改的都是 `ViewState`，
/// 但相机的 `Transform` / 投影 `scale` 才是渲染真源——
/// 只写 ViewState 相机纹丝不动，且 chunk 视口拣选按假视口抖动
/// （despawn/respawn 重建画面，表现为「render 很慢」）。凡是运行期
/// 想改取景的入口，写完 ViewState 后必须经本函数同步相机。
pub(crate) fn apply_view_to_camera(
    center: Vec2,
    scale: f32,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    for (mut tf, mut projection) in cameras.iter_mut() {
        tf.translation.x = center.x;
        tf.translation.y = center.y;
        set_ortho_scale(&mut projection, scale);
    }
}

/// egui 是否想要指针输入（读上一帧结果，基线 §2.13）。
#[derive(Resource, Default)]
pub struct PointerOverUi {
    pub wants_pointer: bool,
    pub wants_keyboard: bool,
}

/// 每帧开头更新「指针是否在 UI 上」。放在 `PreUpdate`。
///
/// **必须排在 `EguiPreUpdateSet::InitContexts` 之后**（见 gui.rs 的 `.after()`）：
/// 否则首帧 egui 上下文尚未建立。bevy_egui 0.42 起 `ctx_mut()` 返回 `Result`
/// （旧版是直接 panic），故这里按 `Err` 直接返回、保留上一帧的判定——
/// 启动首帧的「指针不在 UI 上」是安全默认。
pub fn poll_egui_wants_input(mut contexts: EguiContexts, mut flag: ResMut<PointerOverUi>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    flag.wants_pointer = ctx.egui_wants_pointer_input();
    flag.wants_keyboard = ctx.egui_wants_keyboard_input();
}

/// 滚轮缩放：锚点 = 鼠标指针（零惯性）。
///
/// 缩放区间按当前级实时算（`crate::zoom_level`），故需要 `projection`（读 `extent`）
/// 与 `drill`（读「现在在哪一级」）。撞到级的边界后**继续同向滚动**表示「换级」，
/// 本函数只把意图写进 [`crate::render::zoom_level::ZoomCrossRequest`]，真正的换级由
/// [`crate::render::drill::apply_zoom_cross`] 执行（地图路径的写入口只有 `DrillState`）。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `drill.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn camera_zoom(
    mut scroll: MessageReader<MouseWheel>,
    windows: Query<&Window>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    mut view: ResMut<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    mut cross: ResMut<crate::render::zoom_level::ZoomCrossRequest>,
    ui: Res<PointerOverUi>,
) {
    if ui.wants_pointer {
        // 指针在面板上时不吃滚轮（面板自己可能滚动）。
        scroll.clear();
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let mut scroll_y = 0.0;
    for ev in scroll.read() {
        scroll_y += ev.y;
    }
    if scroll_y == 0.0 {
        return;
    }

    let Ok((mut transform, mut ortho)) = camera.single_mut() else {
        return;
    };

    let old_scale = view.scale;
    let factor = (1.0 + scroll_y * 0.12).clamp(0.5, 2.0);
    let size = window.resolution.size();
    let zoom = crate::render::zoom_level::level_zoom(
        &projection.world,
        drill.current(),
        size,
        Vec2::splat(crate::render::zoom_level::FALLBACK_MAP_SIZE),
    );
    let new_scale = zoom.clamp(old_scale * factor);

    // 指针处的世界坐标在缩放前后保持不变（锚点 = 指针）。
    let cursor_ndc = Vec2::new(
        (cursor.x / size.x) * 2.0 - 1.0,
        -((cursor.y / size.y) * 2.0 - 1.0),
    );
    let cursor_world_before =
        transform.translation.truncate() + cursor_ndc * (size * 0.5) * old_scale;

    // 撞界后继续同向滚动 = 请求换级。**必须在「值没变就返回」之前判**：
    // 撞界时 `new_scale == old_scale`，恰恰是下面那个提前返回会吞掉的情形。
    if factor > 1.0 && zoom.at_outer_limit(old_scale) {
        // 继续往外：回到父级（世界级再往外没有父级，消费方会自行忽略）。
        cross.pending = Some(crate::render::zoom_level::ZoomCross::Out);
    } else if factor < 1.0 && zoom.at_inner_limit(old_scale) {
        // 继续往里：光标所在的那个子图。用与双击同一套命中测试——不然
        // 「双击能进的图，滚轮进不去」会让人以为坏了。
        if let Some(child) = crate::render::drill::marker_hit(
            &projection.world,
            &drill,
            &grid,
            cursor_world_before,
            old_scale,
        ) {
            cross.pending = Some(crate::render::zoom_level::ZoomCross::Into(child));
        }
    }

    if (new_scale - old_scale).abs() < f32::EPSILON {
        return;
    }

    // `Camera2d` 的投影是 `Projection::Orthographic(..)`（0.19 起统一为枚举）。
    set_ortho_scale(&mut ortho, new_scale);
    view.scale = new_scale;

    let cursor_world_after =
        transform.translation.truncate() + cursor_ndc * (size * 0.5) * new_scale;
    let delta = cursor_world_before - cursor_world_after;
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
    view.camera_center = transform.translation.truncate();
}

/// 平移：右键 / 中键拖拽（零惯性，当帧跟随）。
/// 左键拖拽平移地图（右键/中键始终可平移；画笔或街道模式开启时左键让渡）。
#[allow(clippy::too_many_arguments)]
pub fn camera_pan(
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
    mut view: ResMut<ViewState>,
    brush: Res<BrushState>,
    streets: Res<crate::render::streets::StreetEditorState>,
    ui: Res<PointerOverUi>,
    windows: Query<&Window>,
) {
    if ui.wants_pointer {
        return;
    }
    // 画街道模式下左键让渡给街道点取（与笔刷同一条手势让渡规则）。
    if !pan_button_pressed(&buttons, brush.enabled || streets.enabled) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    // 屏幕像素 → 世界单位：乘以 scale，且 y 轴反向。
    let world_delta = Vec2::new(-delta.x, delta.y) * view.scale;
    transform.translation.x += world_delta.x;
    transform.translation.y += world_delta.y;
    view.camera_center = transform.translation.truncate();
    let _ = window;
}

/// 当前按下的按钮是否应该平移地图。
///
/// 左键遵循 Google Maps 手势：默认拖动平移；只有作者显式启用笔刷时才让渡给
/// 笔刷。右键 / 中键始终保留给平移，作为精确编辑时的逃生通道。
fn pan_button_pressed(buttons: &ButtonInput<MouseButton>, brush_enabled: bool) -> bool {
    buttons.pressed(MouseButton::Right)
        || buttons.pressed(MouseButton::Middle)
        || (!brush_enabled && buttons.pressed(MouseButton::Left))
}

/// 屏幕坐标 → 世界坐标（基线 §2.15 变换链，唯一实现）。
pub fn screen_to_world(
    cursor: Vec2,
    window_size: Vec2,
    camera_translation: Vec3,
    scale: f32,
) -> Vec2 {
    let ndc = Vec2::new(
        (cursor.x / window_size.x) * 2.0 - 1.0,
        -((cursor.y / window_size.y) * 2.0 - 1.0),
    );
    camera_translation.truncate() + ndc * (window_size * 0.5) * scale
}

/// 更新光标世界坐标（顶栏坐标显示）。
pub fn track_cursor_world(
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    mut state: ResMut<ViewState>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(transform) = camera.single() else {
        return;
    };
    let scale = state.scale;
    state.window_size = window.resolution.size();
    state.cursor_world = window
        .cursor_position()
        .map(|c| screen_to_world(c, window.resolution.size(), transform.translation, scale));
}

/// 笔刷拾取：左键按下/拖拽时收集笔触点列（**不改世界**）。
///
/// 两条与地图钻取（v0.2 §B）的接口约定：
///
/// 1. **只在世界级落笔**。原型 `index.html:1837` 的判定是
///    `brushOn && !immersive && mapState.level===0`——城市级/建筑级没有地形网格，
///    在那里涂抹既无对象也易误解。
/// 2. **点在钻入标记上不落笔**。原型里标记是独立的 DOM 元素、`dblclick` 自带
///    `stopPropagation`，画布收不到那两下点击；Bevy 没有 DOM 挡刀，只能自己判，
///    否则「双击钻入」会先在地上刷一笔。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn collect_brush_stroke(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    ui: Res<PointerOverUi>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    mut stroke: ResMut<ActiveStroke>,
) {
    if ui.wants_pointer || !brush.enabled || drill.depth() > 0 {
        stroke.points.clear();
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(transform) = camera.single() else {
        return;
    };
    let world = screen_to_world(
        cursor,
        window.resolution.size(),
        transform.translation,
        view.scale,
    );

    if buttons.just_pressed(MouseButton::Left) {
        if crate::render::drill::marker_hit(&projection.world, &drill, &grid, world, view.scale)
            .is_some()
        {
            // 这一下是冲着标记去的：不落笔（见上文第 2 条）。
            stroke.points.clear();
            return;
        }
        stroke.points.clear();
        stroke.points.push(world);
    } else if buttons.pressed(MouseButton::Left) {
        // 采样去抖：与上一点距离小于半格则不重复记录。
        let cell = grid.cell_size;
        if stroke
            .points
            .last()
            .is_none_or(|p| p.distance(world) > cell * 0.5)
        {
            stroke.points.push(world);
        }
    }
}

/// 笔刷光标世界坐标（供光标绘制与顶栏半径显示）。
pub fn brush_cursor_world(
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
) -> Option<Vec2> {
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let transform = camera.single().ok()?;
    Some(screen_to_world(
        cursor,
        window.resolution.size(),
        transform.translation,
        view.scale,
    ))
}

/// 键盘平移：WASD / 方向键（零惯性）。
pub fn camera_pan_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
    mut view: ResMut<ViewState>,
    ui: Res<PointerOverUi>,
) {
    if ui.wants_keyboard {
        return;
    }
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir.y -= 1.0;
    }
    if dir == Vec2::ZERO {
        return;
    }
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let speed = 900.0 * view.scale; // 世界单位/秒
    let delta = dir.normalize() * speed * time.delta_secs();
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
    view.camera_center = transform.translation.truncate();
    let _ = &mut transform;
}

/// Ctrl+滚轮 = 只缩放笔刷大小（基线 §1.16）。
pub fn brush_radius_scroll(
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: MessageReader<MouseWheel>,
    mut brush: ResMut<BrushState>,
    mut grid: ResMut<GridConfig>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let mut sy = 0.0;
    for ev in scroll.read() {
        sy += ev.y;
    }
    if sy == 0.0 {
        return;
    }
    let factor = (1.0 + sy * 0.15).clamp(0.5, 2.0);
    brush.radius = (brush.radius * factor).clamp(grid.cell_size * 2.0, 600.0);
    // grid 保持可变以允许后续同步其它参数。
    let _ = &mut grid;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::mouse::MouseScrollUnit;
    use wv_core::id::Id;
    use wv_core::map::{IconShape, IconSpec, MapLevel, MapNode};

    /// Google 式地图的默认手势是左键拖拽平移；只有显式启用笔刷时左键才归笔刷。
    #[test]
    fn left_drag_pans_when_brush_is_disabled() {
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Left);
        assert!(pan_button_pressed(&buttons, false));
        assert!(!pan_button_pressed(&buttons, true));
    }

    /// 世界 4096²（`fit` 5.12 → 区间 `[0.064, 8.0]`）+ 一座 1024² 的城市，
    /// 城市入口图标摆在世界图正中 (2048,2048)。
    fn two_level_world() -> wv_core::World {
        let icon = IconSpec {
            id: Id::new_unchecked("icon_city_a"),
            anchor: Some([0.5, 0.5]),
            shape: IconShape::Diamond,
            size: None,
            color: None,
            drill_into: Some(Id::new_unchecked("city_a")),
        };
        let node = |id: &str,
                    level: MapLevel,
                    parent: Option<&str>,
                    children: &[&str],
                    extent: f32,
                    icons: Vec<IconSpec>| MapNode {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            level,
            parent: parent.map(Id::new_unchecked),
            children: children.iter().map(|c| Id::new_unchecked(*c)).collect(),
            layers: Vec::new(),
            icons,
            extent: Some(wv_core::map::Extent::sized(extent, extent)),
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        };
        wv_core::World {
            maps: vec![
                node(
                    "map_world",
                    MapLevel::World,
                    None,
                    &["city_a"],
                    4096.0,
                    vec![icon],
                ),
                node(
                    "city_a",
                    MapLevel::City,
                    Some("map_world"),
                    &[],
                    1024.0,
                    Vec::new(),
                ),
            ],
            ..Default::default()
        }
    }

    /// 一个只装了 `camera_zoom` 的 App：相机在世界图正中，指针停在窗口正中，
    /// 于是「指针处的世界坐标」恰好是城市入口图标 (2048,2048)。
    fn zoom_app(scale: f32) -> App {
        let world = two_level_world();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);

        let mut app = App::new();
        let view = ViewState {
            scale,
            ..Default::default()
        };
        app.init_resource::<PointerOverUi>()
            .init_resource::<GridConfig>()
            .init_resource::<crate::render::zoom_level::ZoomCrossRequest>()
            .add_message::<MouseWheel>()
            .insert_resource(view)
            .insert_resource(drill)
            .insert_resource(WorldProjection {
                world,
                ..Default::default()
            })
            .add_systems(Update, camera_zoom);

        app.world_mut().spawn((
            Camera2d,
            Transform::from_xyz(2048.0, 2048.0, 0.0),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));
        let mut window = Window::default();
        // `Window::default()` = 1280×720，正中即缩放锚点。
        window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));
        let window_id = app.world_mut().spawn(window).id();
        app.insert_resource(TestWindow(window_id));
        app
    }

    #[derive(Resource)]
    struct TestWindow(Entity);

    /// 世界级区间。**必须**从 `zoom_level` 现算而不是写死常量：窗口尺寸决定了 `fit`
    /// （`Window::default()` 是 1280×720，`fit = 4096/720 ≈ 5.688`，不是 800 高时的 5.12）。
    fn world_band() -> crate::render::zoom_level::LevelZoom {
        crate::render::zoom_level::level_zoom(
            &two_level_world(),
            Some(&Id::new_unchecked("map_world")),
            Vec2::new(1280.0, 720.0),
            Vec2::splat(crate::render::zoom_level::FALLBACK_MAP_SIZE),
        )
    }

    fn scroll(app: &mut App, y: f32) {
        let window = app.world().resource::<TestWindow>().0;
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y,
            window,
            // 0.19 起 `MouseWheel` 带触控板阶段字段；鼠标滚轮恒为 `Moved`。
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }

    /// 已在**外界**还继续往外滚 = 请求上溯一级。
    #[test]
    fn camera_zoom_requests_a_step_out_at_the_outer_limit() {
        let mut app = zoom_app(world_band().max);
        scroll(&mut app, 1.0); // y > 0 → factor > 1 → 缩小（往外）
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            Some(crate::render::zoom_level::ZoomCross::Out),
            "踩在外界上继续往外滚，必须发出「上溯一级」的请求"
        );
    }

    /// 已在**内界**且指针压在城市入口上时继续往里滚 = 请求进入该城。
    #[test]
    fn camera_zoom_requests_a_step_into_the_child_under_the_cursor() {
        let mut app = zoom_app(world_band().min);
        scroll(&mut app, -1.0); // y < 0 → factor < 1 → 放大（往里）
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            Some(crate::render::zoom_level::ZoomCross::Into(
                Id::new_unchecked("city_a")
            )),
            "指针在城市图标上继续往里滚，必须请求进入该城"
        );
    }

    /// 区间**中间**的滚动只是缩放，不得换级——否则正常缩放会莫名换图。
    #[test]
    fn camera_zoom_does_not_cross_in_the_middle_of_the_band() {
        let fit = world_band().fit;
        let mut app = zoom_app(fit);
        scroll(&mut app, -1.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            None
        );
        let view = app.world().resource::<ViewState>();
        assert!(view.scale < fit, "中间地带只缩放，got {}", view.scale);
    }

    /// 指针在面板上时不吃滚轮（面板自己要滚），也不得因此换级。
    #[test]
    fn camera_zoom_ignores_scroll_over_ui() {
        let max = world_band().max;
        let mut app = zoom_app(max);
        app.world_mut()
            .resource_mut::<PointerOverUi>()
            .wants_pointer = true;
        scroll(&mut app, 1.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            None
        );
        assert_eq!(app.world().resource::<ViewState>().scale, max);
    }
}
