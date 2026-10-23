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
    asset::AssetPlugin,
    camera::ScalingMode,
    mesh::Mesh2d,
    prelude::*,
    render::view::screenshot::{save_to_disk, Screenshot},
    sprite_render::MeshMaterial2d,
    window::PrimaryWindow,
};

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_render::terrain_view::field::{
    build_material, default_params, set_coast_encoding, CoastEncoding, FieldPack,
    TerrainFieldMaterial, TerrainFieldMaterialPlugin, ViewBranch,
};

/// 地形网格边长（格）。512 = 262144 格，与 P0 锁 T5 的计时口径一致。
const GRID: u32 = 512;

/// 初始每格屏幕像素（落在 T32 过渡档 1.0~3.0 之间，便于观察墨线淡出）。
const INIT_PX_PER_CELL: f32 = 6.0;

/// 缩放上下限（每格屏幕像素）。下限压到 0.4 是为了能看**远观档**
/// （P0 锁 T33：`cell_px < 1.0` 属远观，墨线全关只留色域）。
const ZOOM_MIN: f32 = 0.4;
const ZOOM_MAX: f32 = 48.0;

/// **一格占多少世界单位（固定）**。
///
/// 缩放**不能**改 quad 的世界尺寸——若 quad 与相机视野同比例缩放，
/// 二者相消，画面纹丝不动（实测踩过：8 档验收图长得一模一样）。
/// 正确模型：quad 恒定 `GRID × WORLD_PER_CELL` 世界单位，
/// 缩放只改相机视野高度＝`窗口像素 / px_per_cell`。
const WORLD_PER_CELL: f32 = 1.0;

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
        // T44：纸底铺底——「地形没画到的地方也是纸不是黑」。
        // 用 `Color::NONE`（透明）会让远观档地图外的空白显示成黑框。
        .insert_resource(ClearColor(Color::srgb(0.94, 0.91, 0.84)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (zoom_input, key_input, sync_material, acceptance_capture),
        )
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
    /// 预先找出的海岸格（世界坐标）——验收截图的对准点。
    coast_world: Vec2,
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

/// 找一格「陆地且四邻有水」的格，取最靠近图幅中心的一个。
///
/// 验收截图要对准海岸看（岸线连续性 / 三段色带 / 线宽恒定），
/// 对着地图中心只能拍到内陆。
///
/// 返回**世界坐标**：格 (cx, cy)（y 自上而下）→ 世界
/// `(cx + 0.5 − GRID/2, GRID/2 − cy − 0.5)`，与 quad 的 1 单位/格对齐。
fn find_coast_world(terrain: &TerrainData) -> Vec2 {
    let w = terrain.width as usize;
    let h = terrain.height as usize;
    let is_land = |x: usize, y: usize| terrain.cells[y * w + x].surface != 0;
    let center = Vec2::new(terrain.width as f32 * 0.5, terrain.height as f32 * 0.5);
    let mut best: Option<(f32, usize, usize)> = None;

    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            if !is_land(x, y) {
                continue;
            }
            let coast = !is_land(x - 1, y)
                || !is_land(x + 1, y)
                || !is_land(x, y - 1)
                || !is_land(x, y + 1);
            if !coast {
                continue;
            }
            let d = (Vec2::new(x as f32, y as f32) - center).length_squared();
            if best.is_none_or(|(bd, _, _)| d < bd) {
                best = Some((d, x, y));
            }
        }
    }

    let (cx, cy) = best
        .map(|(_, x, y)| (x as f32, y as f32))
        // 找不到海岸（全水或全陆）就退回图幅中心。
        .unwrap_or((terrain.width as f32 * 0.5, terrain.height as f32 * 0.5));
    let g = GRID as f32 * WORLD_PER_CELL;
    Vec2::new(cx + 0.5 - g * 0.5, g * 0.5 - cy - 0.5)
}

/// **受控合成地形**：一条 45° 直岸线。
///
/// 存在的唯一理由是**可测量**。真实 fbm 岸线是曲线，放大后「哪一段是阶梯、
/// 哪一段是数据本来就直」分不清；45° 直线则一目了然：无符号编码必然渲成
/// 正交阶梯，有符号编码必然是斜线。
///
/// 判据：沿 y 逐行取水陆边界 x，斜率应恒为 1、且每行都变。
fn make_terrain_synth(n: u32) -> TerrainData {
    let mut cells = Vec::with_capacity((n * n) as usize);
    for y in 0..n {
        for x in 0..n {
            // x + y < n 的斜带 ⇒ 45° 岸线（左下为陆、右上为水）
            let is_land = (x + y) < n;
            let height = if is_land { 240i16 } else { -1500i16 };
            cells.push(TerrainCellData {
                height,
                surface: u8::from(is_land),
                biome: if is_land { 2 } else { 15 },
            });
        }
    }
    TerrainData {
        width: n,
        height: n,
        cells,
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
    // `FIELD_SYNTH=1` 换受控合成地形（45° 直岸线），用于可测量的 A/B。
    let terrain = if std::env::var("FIELD_SYNTH").is_ok() {
        info!("地形：受控合成（45° 直岸线）");
        make_terrain_synth(GRID)
    } else {
        make_terrain(GRID)
    };

    // 海岸编码：`FIELD_SIGNED=1` 切到有符号距离（T3/T5 的实验分支），
    // 默认保持 T3 原值的无符号编码。用于 A/B 对比「台阶是否可消」。
    let encoding = if std::env::var("FIELD_SIGNED").is_ok() {
        info!("海岸编码：Signed（有符号距离，实验分支）");
        CoastEncoding::Signed
    } else {
        info!("海岸编码：Unsigned（T3 原值）");
        CoastEncoding::Unsigned
    };

    // 打印打包统计——P0 锁 T4 的显存口径（1024² 图 8 MB；512² 图 = 2 MB）。
    let pack = FieldPack::from_terrain_with(&terrain, encoding);
    let coast_world = find_coast_world(&terrain);
    info!(
        "海岸对准点（世界坐标）: ({:.1}, {:.1})",
        coast_world.x, coast_world.y
    );
    let bytes = pack.height_tex.len() + pack.coast_tex.len() + pack.attr_tex.len();
    info!(
        "field pack: {}×{} 格，三纹理合计 {} KB（{:.2} MB）",
        pack.width,
        pack.height,
        bytes / 1024,
        bytes as f32 / (1024.0 * 1024.0)
    );

    // 一格 = INIT_PX_PER_CELL 世界单位；相机视野按网格铺满。
    let mut params = default_params(
        [GRID as f32, GRID as f32],
        INIT_PX_PER_CELL,
        ViewBranch::Themed,
        Default::default(),
    );
    // pack 与 shader 的编码开关必须同时设，否则距离场整体错一格（不报错）。
    set_coast_encoding(&mut params, encoding);
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
    let extent = GRID as f32 * WORLD_PER_CELL;
    let mesh = meshes.add(Rectangle::new(1.0, 1.0));
    let quad = commands
        .spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material.clone()),
            Transform::from_scale(Vec3::splat(extent)),
        ))
        .id();

    commands.spawn(Camera2d);

    // 验收截图模式：`FIELD_ACCEPT=1` 时按固定缩放序列逐档出图并退出。
    // 让 P1 的四条验收（岸线连续 / 墨线恒定 / 分支无关 / 远观不闪）可复跑，
    // 不依赖人肉滚轮。
    if std::env::var("FIELD_ACCEPT").is_ok() {
        commands.insert_resource(AcceptCapture::new());
        info!("FIELD_ACCEPT 已启用：将按预设缩放序列截图到 ./snapshots-demo/accept/");
    }

    commands.insert_resource(DemoState {
        px_per_cell: INIT_PX_PER_CELL,
        branch: ViewBranch::Themed,
        hillshade_on: true,
        auto_zoom: false,
        zoom_dir: 1.0,
        material,
        quad,
        coast_world,
    });
}

/// 滚轮缩放（2× ~ 40×）。
fn zoom_input(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut state: ResMut<DemoState>,
) {
    for ev in wheel.read() {
        let factor = if ev.y > 0.0 { 1.12 } else { 1.0 / 1.12 };
        state.px_per_cell = (state.px_per_cell * factor).clamp(ZOOM_MIN, ZOOM_MAX);
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
        state.px_per_cell = state.px_per_cell.clamp(ZOOM_MIN, ZOOM_MAX);
        if state.px_per_cell <= ZOOM_MIN + 0.01 || state.px_per_cell >= ZOOM_MAX - 0.01 {
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
    windows: Query<&Window, With<PrimaryWindow>>,
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
    // T19 域扭曲幅度：随缩放重算（远观 0.45 格手绘起伏 / 近观 0.05 格保点选精度）。
    // 写进 `pad0.x`——P1 就预留好的槽位，接线不改绑定布局。
    mat.params.pad0[0] = wv_render::terrain_view::field::warp_amp(state.px_per_cell);
    // T28 水彩幅度：pw 槽 `shore_edges.w`（P1 预留位）。
    mat.params.shore_edges[3] = 1.0;
    // relief: xy = 明暗钳制。H 键把它压成 1.0 = 关掉 hillshade（对比观察用）
    mat.params.relief[0] = if state.hillshade_on { 0.86 } else { 1.0 };
    mat.params.relief[1] = if state.hillshade_on { 1.14 } else { 1.0 };

    // quad 世界尺寸**恒定**（1 世界单位/格）——缩放只动相机，不动 quad。
    //
    // ⚠️ 这里踩过一个大坑：曾经让 quad 世界尺寸随 `px_per_cell` 变、
    // 同时把相机视野也设成 quad 尺寸，二者同比例相消 → **画面完全不缩放**，
    // 8 档验收图长得一模一样。正确模型是「quad 固定、只改相机视野」。
    let extent = GRID as f32 * WORLD_PER_CELL;
    if let Ok(mut tf) = transforms.get_mut(state.quad) {
        tf.scale = Vec3::splat(extent);
    }

    // 相机视野高度（世界单位）= 窗口像素高 / 每格屏幕像素。
    // 这样「一格」严格占 `px_per_cell` 个屏幕像素，与 WGSL 里
    // `COAST_LINE_PX / px_per_cell` 的线宽折算口径一致（T11 屏幕像素口径）。
    let win_h = windows
        .single()
        .map(|w| w.height())
        .unwrap_or(900.0)
        .max(1.0);
    let view_h = win_h / state.px_per_cell.max(1e-3);
    // Bevy 0.19 的 `Projection` 是**枚举**（`Projection::Orthographic(..)`），
    // `scaling_mode` / `scale` 在内层的 `OrthographicProjection` 上。
    for mut proj in projections.iter_mut() {
        if let Projection::Orthographic(o) = &mut *proj {
            o.scaling_mode = ScalingMode::FixedVertical {
                viewport_height: view_h,
            };
        }
    }
}

// ── 验收截图模式（FIELD_ACCEPT=1）─────────────────────────────────────────────

/// 一档验收截图：标签 + 每格屏幕像素 + 取色分支。
///
/// 档位对应 P0 锁 T31–T34 的三档判据（近观 ≥3 / 过渡 1–3 / 远观 <1），
/// 外加一档极限近观看岸线连续性（T11 验收：放大 4× 无阶梯）。
/// 相机瞄准点——验收要看的东西不一样：岸线连续性要对着**海岸**看，
/// 远观噪点要对着**全图**看。
#[derive(Clone, Copy, PartialEq)]
enum Aim {
    /// 对准预先找出的海岸格（近观 / 过渡档）。
    Coast,
    /// 对准图幅中心（远观档、CellMap 档）。
    Center,
}

struct AcceptShot {
    label: &'static str,
    px_per_cell: f32,
    branch: ViewBranch,
    aim: Aim,
}

const ACCEPT_SHOTS: &[AcceptShot] = &[
    // 近观极限：每格 32 px。**已知限制**：水陆的**符号**来自 attr 的
    // `surface` 通道（per-texel / Nearest），故海陆交界在格边界处必然有
    // 一格宽的台阶——这是 ADR-0053 T5「不用带符号 SDF」的直接后果。
    // 三段色带（滩/浅/深）由距离场连续驱动、是平滑的；待 P2 域扭曲
    // （T19–T22）把格网对齐打散后，这条台阶才会变成手绘式抖动。
    AcceptShot {
        label: "01-extreme-32px-cell-step-known",
        px_per_cell: 32.0,
        branch: ViewBranch::Themed,
        aim: Aim::Coast,
    },
    // 规格参考档：诊断基线是 4 px/格，「放大 4×」= 16 px/格。
    AcceptShot {
        label: "02-close-16px",
        px_per_cell: 16.0,
        branch: ViewBranch::Themed,
        aim: Aim::Coast,
    },
    // 近观档（T31：cell_px ≥ 3）。
    AcceptShot {
        label: "03-near-8px",
        px_per_cell: 8.0,
        branch: ViewBranch::Themed,
        aim: Aim::Coast,
    },
    // 过渡档上边界（T32：1 ≤ cell_px < 3）。
    AcceptShot {
        label: "04-mid-3px",
        px_per_cell: 3.0,
        branch: ViewBranch::Themed,
        aim: Aim::Coast,
    },
    // 过渡档下边界。
    AcceptShot {
        label: "05-mid-1p5px",
        px_per_cell: 1.5,
        branch: ViewBranch::Themed,
        aim: Aim::Coast,
    },
    // 远观档（T33：cell_px < 1）——墨线应淡出，只留色域，无彩色噪点。
    AcceptShot {
        label: "06-far-0p8px",
        px_per_cell: 0.8,
        branch: ViewBranch::Themed,
        aim: Aim::Center,
    },
    // 极远观。
    AcceptShot {
        label: "07-far-0p4px",
        px_per_cell: 0.4,
        branch: ViewBranch::Themed,
        aim: Aim::Center,
    },
    // CellMap 分支：颜色须**与主题无关**（ADR-0051 §5）。
    AcceptShot {
        label: "08-cellmap-8px",
        px_per_cell: 8.0,
        branch: ViewBranch::BiomeRaw,
        aim: Aim::Center,
    },
    AcceptShot {
        label: "09-cellmap-0p8px",
        px_per_cell: 0.8,
        branch: ViewBranch::BiomeRaw,
        aim: Aim::Center,
    },
];

/// 验收截图驱动器状态。
#[derive(Resource)]
struct AcceptCapture {
    idx: usize,
    /// 0 = 刚切档、等渲染稳定；≥1 计数到 `SETTLE_FRAMES` 后截图。
    frames: u32,
    /// 截图后等落盘的帧数。
    after_shot: u32,
    /// 启动预热帧：等首帧的纹理上传 / 材质管线编译完成。
    /// 不足会让**第一张**截出全黑（实测踩过）。
    warmup: u32,
    done: bool,
}

impl AcceptCapture {
    fn new() -> Self {
        Self {
            idx: 0,
            frames: 0,
            after_shot: 0,
            warmup: WARMUP_FRAMES,
            done: false,
        }
    }
}

/// 每档先等渲染稳定若干帧（纹理上传、材质重编都是异步的），
/// 再发截图请求，再等落盘——太快会拍到上一档的画面。
/// 启动预热：纹理上传 + 材质管线编译都在前若干帧内完成，
/// 没等够会拍出全黑的第一张。
const WARMUP_FRAMES: u32 = 90;
const SETTLE_FRAMES: u32 = 40;
const AFTER_SHOT_FRAMES: u32 = 20;

fn acceptance_capture(
    mut commands: Commands,
    mut cap: Option<ResMut<AcceptCapture>>,
    mut state: ResMut<DemoState>,
    mut cams: Query<&mut Transform, With<Camera2d>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(cap) = cap.as_mut() else {
        return;
    };
    if cap.done {
        return;
    }
    if cap.warmup > 0 {
        cap.warmup -= 1;
        return;
    }

    // 应用当前档位
    let shot = &ACCEPT_SHOTS[cap.idx];
    state.px_per_cell = shot.px_per_cell;
    state.branch = shot.branch;
    state.auto_zoom = false;

    // 瞄准：近观/过渡档对着海岸看（岸线连续性与线宽恒定才有意义），
    // 远观与 CellMap 档对着全图中心。
    let aim = match shot.aim {
        Aim::Coast => state.coast_world,
        Aim::Center => Vec2::ZERO,
    };
    for mut tf in cams.iter_mut() {
        tf.translation.x = aim.x;
        tf.translation.y = aim.y;
    }

    if cap.after_shot > 0 {
        cap.after_shot -= 1;
        if cap.after_shot == 0 {
            cap.idx += 1;
            cap.frames = 0;
            if cap.idx >= ACCEPT_SHOTS.len() {
                cap.done = true;
                info!("验收截图完成（{} 档）", ACCEPT_SHOTS.len());
                exit.write(AppExit::Success);
            }
        }
        return;
    }

    cap.frames += 1;
    if cap.frames == SETTLE_FRAMES {
        // 按编码分桶，A/B 两套图互不覆盖。
        let sub = match (
            std::env::var("FIELD_SIGNED").is_ok(),
            std::env::var("FIELD_SYNTH").is_ok(),
        ) {
            (false, false) => "accept",
            (true, false) => "accept-signed",
            (false, true) => "accept-synth",
            (true, true) => "accept-synth-signed",
        };
        let path = format!("snapshots-demo/{}/{}.png", sub, shot.label);
        if let Some(dir) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        info!("capture: {} → {}", shot.label, path);
        commands
            .spawn(Screenshot::primary_window())
            // `save_to_disk` 的闭包持有路径（`impl AsRef<Path>` 须为拥有所有权的），
            // 传 `&String` 活不过本帧作用域。
            .observe(save_to_disk(path.clone()));
        cap.after_shot = AFTER_SHOT_FRAMES;
    }
}
