//! P1 demo：地形场 GPU 逐像素求值的独立可运行程序（ADR-0053 D1）。
//!
//! 跑法：
//! ```bash
//! cargo run -p wv-render --example terrain_field_demo --features app
//! ```
//!
//! ## 它演示什么
//!
//! 现行底图是 CPU 烘焙的 RGBA8 纹理，分辨率被 `tile_res_for()` 钉死
//! （1024 图 = 4 px/格），**近观放大 8 倍即见像素块**——这是位图分辨率的
//! 架构上限，调参修不动。本 demo 走 field 路径：三张数据纹理（每格一 texel，
//! 共 8 MB）进 GPU，颜色/明暗/墨线逐像素求值。
//!
//! ## 交互（证明「连续性优先于格子精度」）
//!
//! | 操作 | 效果 | 对应规格 |
//! |---|---|---|
//! | 滚轮 | 缩放（2× ~ 40×） | §3.8.1 线宽屏幕像素口径 |
//! | `1` | 切 Themed 分支（主题四阶 + hillshade） | T7–T18 |
//! | `2` | 切 BiomeRaw 分支（群系色块，**主题无关**） | ADR-0051 §5 |
//! | `Space` | 暂停/继续自动缩放 | 观察墨线粗细是否恒定 |
//! | `H` | 开关 hillshade 对比 | T15–T17 |
//! | `Esc` | 退出 | — |
//!
//! ## 自检点（对应 P1 验收）
//!
//! - **放大 4×**：海岸线应是**连续曲线**，不是阶梯（SPEC §3.8.2）。
//! - **任意缩放**：岸线墨线粗细**恒定**（屏幕像素口径，不是格单位）。
//! - **切分支**：CellMap 分支的颜色**不随主题**变。
//! - **近观**：山体明暗可辨、不过曝、不出规则格纹折痕（T13 双三次的判据）。

use bevy::{camera::ScalingMode, prelude::*, sprite_render::MeshMaterial2d};

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_render::terrain_view::field::{
    build_material, default_params, FieldPack, TerrainFieldMaterial, TerrainFieldMaterialPlugin,
    ViewBranch,
};

/// 地形网格边长（格）。512 = 262144 格，与 P0 锁 T5 的计时口径一致。
const GRID: u32 = 512;

/// 初始每格屏幕像素（落在 T32 过渡档 1.0~3.0 之间，便于观察墨线淡出）。
const INIT_PX_PER_CELL: f32 = 6.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "地形场 demo · ADR-0053 P1（GPU 逐像素）".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerrainFieldMaterialPlugin)
        .insert_resource(ClearColor(Color::NONE))
        .add_systems(Startup, setup)
        .add_systems(Update, (zoom_input, key_input, sync_material))
        .run();
}

/// demo 的相机/缩放状态。
#[derive(Resource)]
struct DemoState {
    /// 每格屏幕像素。
    px_per_cell: f32,
    branch: ViewBranch,
    hillshade_on: bool,
    auto_zoom: bool,
    zoom_dir: f32,
    material: Handle<TerrainFieldMaterial>,
    quad: Entity,
}

/// 程序化生成一份**确定性**测试地形（守护项 8：同输入两次运行逐字节一致）。
///
/// 用整数 hash（不用随机数——`wgpu` 侧的确定性同样重要，SPEC §3.8.6）。
/// 高度场 = 多倍频正弦叠加 + 山地掩膜；水陆由 `height < 0` 决定
/// （与 pack.rs 的湿判定口径一致：水面即 `height < 0`）。
fn make_terrain(n: u32) -> TerrainData {
    let mut cells = Vec::with_capacity((n * n) as usize);
    for y in 0..n {
        for x in 0..n {
            let fx = x as f32 / n as f32;
            let fy = y as f32 / n as f32;

            // 大陆掩膜：两个低频正弦的乘积 → 大陆与内海的分布
            let cont = (fx * 6.0).sin() * (fy * 5.0 + 0.7).cos() + (fx * 2.3 + 1.1).sin() * 0.6;

            // 山地：高频 ridged noise 风格的绝对值折返
            let m = ((fx * 14.0).sin() * (fy * 11.0).cos()).abs();
            let ridge = 1.0 - m.min(1.0);
            let mountain = ridge * ridge * 3.2;

            let h01 = cont * 0.55 + mountain;
            // 米：−3000 ~ +5200，落在 pack.rs 的 H_MIN/H_MAX 量程内
            let height = (h01 * 4100.0 - 1100.0).clamp(-4000.0, 5900.0) as i16;

            let surface = u8::from(height >= 0);
            // biome：按高度带 + 纬度给 ID（0 = NONE，让 pack 走 surface 回退）
            let biome = if height < 0 {
                14 // 海洋
            } else if height > 2600 {
                9 // 雪原
            } else if height > 1200 {
                13 // 山地
            } else if fy < 0.28 {
                1 // 森林（北）
            } else if fy > 0.72 {
                5 // 沙漠（南）
            } else {
                2 // 平原
            };

            cells.push(TerrainCellData {
                height,
                surface,
                biome,
            });
        }
    }
    TerrainData {
        width: n,
        height: n,
        cells,
    }
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainFieldMaterial>>,
) {
    let terrain = make_terrain(GRID);

    // 打印打包统计——P0 锁 T4 的显存口径（1024² 图 8 MB；512² 图 = 2 MB）。
    let pack = FieldPack::from_terrain(&terrain);
    let bytes = pack.height_tex.len() + pack.coast_tex.len() + pack.attr_tex.len();
    info!(
        "field pack: {}×{} 格，三纹理合计 {} KB（{:.2} MB）",
        pack.width,
        pack.height,
        bytes / 1024,
        bytes as f32 / (1024.0 * 1024.0)
    );

    // 一格 = INIT_PX_PER_CELL 世界单位；相机视野按网格铺满。
    let params = default_params(
        [GRID as f32, GRID as f32],
        INIT_PX_PER_CELL,
        ViewBranch::Themed,
        Default::default(),
    );
    let material = materials.add(build_material(&mut images, &pack, params));

    // 一格 = INIT_PX_PER_CELL 世界单位；quad 覆盖全图，UV 铺满 0..1。
    //
    // 用 `Sprite`（其内建 quad 的 UV 恰好铺满 0..1）而非 `Mesh2d`——
    // field 路径要的是「一张连续四边形 + 铺满 UV」，不需要自定义 mesh；
    // 尺寸走 `custom_size`，缩放走 `Transform`，避免每帧重建 mesh 资产。
    let extent = GRID as f32 * INIT_PX_PER_CELL;
    let quad = commands
        .spawn((
            Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::splat(extent)),
                ..default()
            },
            MeshMaterial2d(material.clone()),
        ))
        .id();

    commands.spawn(Camera2d);

    commands.insert_resource(DemoState {
        px_per_cell: INIT_PX_PER_CELL,
        branch: ViewBranch::Themed,
        hillshade_on: true,
        auto_zoom: false,
        zoom_dir: 1.0,
        material,
        quad,
    });
}

/// 滚轮缩放（2× ~ 40×）。
fn zoom_input(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut state: ResMut<DemoState>,
) {
    for ev in wheel.read() {
        let factor = if ev.y > 0.0 { 1.12 } else { 1.0 / 1.12 };
        state.px_per_cell = (state.px_per_cell * factor).clamp(2.0, 40.0);
    }
}

fn key_input(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut state: ResMut<DemoState>) {
    if keys.just_pressed(KeyCode::Digit1) {
        state.branch = ViewBranch::Themed;
    }
    if keys.just_pressed(KeyCode::Digit2) {
        state.branch = ViewBranch::BiomeRaw;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        state.hillshade_on = !state.hillshade_on;
    }
    if keys.just_pressed(KeyCode::Space) {
        state.auto_zoom = !state.auto_zoom;
    }

    if state.auto_zoom {
        // 缓慢往复缩放，穿过 1.0/3.0 两个档位阈值（P0 锁 T34 的连续性判据）
        state.px_per_cell *= 1.0 + state.zoom_dir * time.delta_secs() * 0.35;
        state.px_per_cell = state.px_per_cell.clamp(2.0, 40.0);
        if state.px_per_cell <= 2.01 || state.px_per_cell >= 39.99 {
            state.zoom_dir = -state.zoom_dir;
        }
    }
}

/// 把状态写回材质 uniform + 缩放 quad。
///
/// **拉动式**（AGENTS §3.4）：状态变化 → 重算 uniform → 更新渲染资源，
/// 不做「数据变了自动通知渲染」的反向耦合。
fn sync_material(
    // `state` 在本系统里只读（改的是 material / sprite / projection），
    // 故不取 `mut`。
    state: Res<DemoState>,
    mut materials: ResMut<Assets<TerrainFieldMaterial>>,
    mut sprites: Query<&mut Sprite>,
    mut projections: Query<&mut Projection, With<Camera2d>>,
) {
    // `get_mut` 返回 `Option<&mut T>`；经 `let Some(mat) = ...` 模式绑定后
    // 拿到的是 `&mut TerrainFieldMaterial`，写 `mat.params.*` 需要 mat 可变。
    let Some(mut mat) = materials.get_mut(&state.material) else {
        return;
    };
    // shape: x = px_per_cell, y = branch, z = z_exag, w = grad_step
    mat.params.shape[0] = state.px_per_cell;
    mat.params.shape[1] = match state.branch {
        ViewBranch::BiomeRaw => 0.0,
        ViewBranch::Themed => 1.0,
    };
    // relief: xy = 明暗钳制。H 键把它压成 1.0 = 关掉 hillshade（对比观察用）
    mat.params.relief[0] = if state.hillshade_on { 0.86 } else { 1.0 };
    mat.params.relief[1] = if state.hillshade_on { 1.14 } else { 1.0 };

    // quad 世界尺寸随缩放变化：每格 px_per_cell 屏幕像素
    let extent = GRID as f32 * state.px_per_cell;
    if let Ok(mut sprite) = sprites.get_mut(state.quad) {
        sprite.custom_size = Some(Vec2::splat(extent));
    }

    // 相机视野必须与 quad 同步缩放，否则放大后只看到中央一小块。
    // 显式正交投影（`Projection` 是独立组件，`Camera2d` 经 `#[require]` 自动挂）：
    // 视野高度 = quad 高度（世界单位），这样「每格占多少屏幕像素」才等于
    // `px_per_cell`，与 shader 的线宽折算口径一致。
    // Bevy 0.19 的 `Projection` 是**枚举**（`Projection::Orthographic(..)`），
    // `scaling_mode` / `scale` 在内层的 `OrthographicProjection` 上。
    for mut proj in projections.iter_mut() {
        // Bevy 0.19 的 `ScalingMode::FixedVertical` 显式给视野**高度**
        // （宽度按窗口宽高比自动推）。取 `extent × 1.02` 作 2% 边距，
        // 避免 quad 边缘露纸底。
        if let Projection::Orthographic(o) = &mut *proj {
            o.scaling_mode = ScalingMode::FixedVertical {
                viewport_height: extent * 1.02,
            };
        }
    }
}
