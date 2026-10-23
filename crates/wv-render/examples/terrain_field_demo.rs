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

use bevy::{
    asset::AssetPlugin, camera::ScalingMode, mesh::Mesh2d, prelude::*,
    sprite_render::MeshMaterial2d,
};

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
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "地形场 demo · ADR-0053 P1（GPU 逐像素）".into(),
                        ..default()
                    }),
                    ..default()
                })
                // `Material2d::fragment_shader()` 返回的是**资源路径**，
                // 相对 asset 根解析。而 example 二进制的可执行文件在
                // `target/debug/examples/`，默认 asset 根会落到那里
                // （实测报错：`Path not found: ...\target\debug\examples\assets\…`）。
                // 故显式指向 crate 的 `assets/`——用 `CARGO_MANIFEST_DIR`
                // 在编译期定死，避免依赖运行目录。
                .set(AssetPlugin {
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").to_string(),
                    ..default()
                }),
        )
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

/// 确定性哈希（整数 → `[0,1)`）。**不用随机数**——守护项 8 / SPEC §3.8.6：
/// 同一格坐标两次运行必须给出同一个值。
fn hash2(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x9E37_79B1)
        ^ (iy as u32).wrapping_mul(0x85EB_CA77)
        ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// 平滑插值曲线（smoothstep 的多项式核），去格子感。
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < 1e-9 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    smooth(((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0))
}

/// 二维值噪声（整数格点哈希 + 双线性 smooth 插值）。
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = smooth(x - x0 as f32);
    let fy = smooth(y - y0 as f32);
    let a = hash2(x0, y0, seed);
    let b = hash2(x0 + 1, y0, seed);
    let c = hash2(x0, y0 + 1, seed);
    let d = hash2(x0 + 1, y0 + 1, seed);
    let ab = a + (b - a) * fx;
    let cd = c + (d - c) * fx;
    ab + (cd - ab) * fy
}

/// 分形叠加（fBm）：每层频率 ×2、振幅 ×0.5，归一化到 `[0,1]`。
fn fbm(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for o in 0..octaves {
        sum += value_noise(x * freq, y * freq, seed ^ (o.wrapping_mul(7919))) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

/// 程序化生成一份**确定性**的测试地形（大陆 + 山脉 + 内海）。
///
/// 布局意图（让 demo 能同时看到四种画法）：
/// - **大陆居中、四周入海** → 海岸线（T7–T9 三段色带 + T23 岸线墨线）；
/// - **内部山脉**（ridged fbm）→ hillshade（T13 双三次 + T14–T17 明暗）；
/// - **低地平原** → 主题四阶底色与 biome 边界（T24/T27）；
/// - **内海/湖泊** → 水域色带与水陆边界的闭合形态。
///
/// 高度落在 `pack.rs` 的 `H_MIN/H_MAX`（−4000..6000 米）量程内。
fn make_terrain(n: u32) -> TerrainData {
    let mut cells = Vec::with_capacity((n * n) as usize);
    let inv = 1.0 / n as f32;

    for y in 0..n {
        for x in 0..n {
            let nx = x as f32 * inv;
            let ny = y as f32 * inv;

            // ── 大陆轮廓：低频 fbm + 径向衰减（居中成陆、边缘入海）──
            let cont = fbm(nx * 3.2, ny * 3.2, 1337, 5);
            let dx = nx - 0.5;
            let dy = ny - 0.5;
            let r = (dx * dx + dy * dy).sqrt() * 2.0; // 0 = 中心，1 = 边缘
            let island = 1.0 - smoothstep(0.45, 0.98, r);
            // 基准高程：cont 主导，island 保证边缘沉入水下
            let base = cont * 0.85 + island * 0.55 - 0.62;

            // ── 山脉：ridged fbm（|2n−1| 取脊），只在高地长山 ──
            let rn = fbm(nx * 7.0, ny * 7.0, 4242, 4);
            let ridge = 1.0 - (rn * 2.0 - 1.0).abs();
            let mountain = ridge * ridge * ridge * 1.45 * smoothstep(0.30, 0.62, base);

            let h01 = base + mountain;
            // 米：把 h01 的典型区间映射到 −3000..+5200
            let height = (h01 * 5200.0 - 240.0).clamp(-4000.0, 5900.0) as i16;

            let surface = u8::from(height >= 0);

            // biome：按海拔带 + 纬度（用真实冻结编号，让 CellMap 分支有层次）
            let biome = if height < 0 {
                if height < -1800 {
                    14
                } else {
                    15
                } // 海洋 / 湖泊（浅）
            } else if height > 3000 {
                9 // 雪原
            } else if height > 1900 {
                13 // 山地
            } else if height > 900 {
                12 // 丘陵
            } else if ny < 0.3 {
                1 // 森林（北）
            } else if ny > 0.72 {
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
    mut meshes: ResMut<Assets<Mesh>>,
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
    // **必须是 `Mesh2d`，不能用 `Sprite`**：`Sprite` 走 sprite 渲染管线，
    // 只认 `Image` 纹理，**不消费 `MeshMaterial2d`**——把自定义材质挂在
    // `Sprite` 上会被静默忽略，画出一个无纹理白块（实测踩过）。
    // 自定义 `Material2d` 只能配 `Mesh2d`。
    //
    // mesh 建成 1×1 单位，世界尺寸靠 `Transform::scale` 给——
    // 这样缩放时无需每帧改 mesh 资产。
    let extent = GRID as f32 * INIT_PX_PER_CELL;
    let mesh = meshes.add(Rectangle::new(1.0, 1.0));
    let quad = commands
        .spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material.clone()),
            Transform::from_scale(Vec3::splat(extent)),
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
    // `state` 在本系统里只读（改的是 material / transform / projection），
    // 故不取 `mut`。
    state: Res<DemoState>,
    mut materials: ResMut<Assets<TerrainFieldMaterial>>,
    mut transforms: Query<&mut Transform>,
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

    // quad 世界尺寸随缩放变化：每格 px_per_cell 屏幕像素。
    // mesh 是 1×1 单位，故缩放系数即世界尺寸。
    let extent = GRID as f32 * state.px_per_cell;
    if let Ok(mut tf) = transforms.get_mut(state.quad) {
        tf.scale = Vec3::splat(extent);
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
