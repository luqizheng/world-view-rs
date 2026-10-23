//! 网格材质冒烟 + 两帧取证（2026-10-23）：**证明 WGSL 真能在 GPU 上编译并画出网格**，
//! 并落两张 PNG 供 QA 判「粗网格会不会把笔迹淹掉」。
//!
//! ## 为什么需要它
//! `GridLinesMaterial` 的 shader 走 `load_internal_asset!` 编译期内联，
//! 只有真正跑起渲染管线时才会被 naga/wgpu 校验——`cargo test` 是纯 ECS，
//! 抓不到 WGSL 的语法/语义错（写错一个 `fwidth` 参数，测试全绿而画面空白）。
//!
//! ## 跑法
//! ```bash
//! cargo run -p wv-world-cell-map --example grid_material_smoke
//! ```
//! 产物（相对 crate 根）：`../../docs/reviews/2026-10-23-grid-frames/`
//! - `far-fit-5.12.png` —— 远观档：格距 0.78px（**只剩粗档**，m = 8 → 6.25px/格）
//! - `near-1.0.png`     —— 近观档：格距 4.0px（细档全显 + 粗档 m = 1 重合）
//!
//! ## 两档怎么在没有相机缩放的情况下复现
//!
//! shader 只看「每格多少屏幕像素」= `cell_size / scale`。相机保持 1 世界单位 = 1px，
//! 于是**改 `cell_size` 等价于改 `scale`**：
//! - 远观 fit：`cell_size = 4.0 / 5.12 = 0.78125`（lod-demo 世界图 `extent 4096 / raster 1024`
//!   在 `scale 5.12` 下的口径）；
//! - 近观：`cell_size = 4.0`（`scale 1.0`）。
//!
//! 这样不必碰相机投影，冒烟只依赖「世界坐标 → 像素」这一条链。
//!
//! ## 场景里为什么要放「笔迹」
//!
//! QA 要看的是「格子越密会不会把作者画的东西盖掉」。故铺一张纸色底 + 三笔深墨
//! 横条（厚度取 0.6 格），远/近两档各截一帧对比。
//!
//! 判定口径：
//! - 输出 `GRID_SMOKE_OK` → 管线建成、两帧落盘；
//! - **不得**出现 `naga` / `Shader validation` / `array stride` 报错。

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use wv_world_cell_map::terrain_view::grid_lines::{
    grid_params, GridLinesMaterial, GridLinesMaterialPlugin, GRID_Z,
};

/// 逐帧脚本：到这些帧号做对应动作（帧号取小值，冒烟十几秒内跑完）。
const FRAME_SWAP_TO_NEAR: u32 = 12;
const FRAME_SHOT_FAR: u32 = 10;
const FRAME_SHOT_NEAR: u32 = 30;
const FRAME_QUIT: u32 = 40;

/// lod-demo 世界图口径：`extent 4096 / raster 1024 → cell_size = 4.0`。
const CELL_AT_SCALE_ONE: f32 = 4.0;
/// 世界级全图适配：`max(4096/1280, 4096/800) = 5.12`。
const FIT_SCALE: f32 = 5.12;

#[derive(Resource)]
struct Script {
    frame: u32,
    /// 远观幅面实体（切到近观时 despawn）。
    far: Vec<Entity>,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                visible: false,
                resolution: (1280u32, 800u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GridLinesMaterialPlugin)
        // 纸底：地形没画到的地方也是纸不是黑（与 app 的 ClearColor 同族）。
        .insert_resource(ClearColor(Color::srgb(0.94, 0.91, 0.84)))
        .add_systems(Startup, setup)
        .add_systems(Update, run_script)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridLinesMaterial>>,
) {
    commands.spawn(Camera2d);
    // ── 远景：1024 格 × 0.78125 = 800 世界单位，恰好铺满 1280×800 的画面 ──
    let far_cell = CELL_AT_SCALE_ONE / FIT_SCALE;
    let far_extent = 1024.0 * far_cell;
    let far_quad = meshes.add(Rectangle::new(far_extent, far_extent));
    let far_mat = materials.add(GridLinesMaterial {
        params: grid_params(
            far_cell,
            Vec2::ZERO,
            LinearRgba::rgb(0.16, 0.14, 0.13),
            0.48,
        ),
    });
    let mut far = vec![commands
        .spawn((
            Mesh2d(far_quad),
            MeshMaterial2d(far_mat),
            Transform::from_xyz(far_extent * 0.5, far_extent * 0.5, GRID_Z),
        ))
        .id()];
    // 三笔「墨迹」：0.6 格厚、跨幅面 60% 宽的横条——远观档看它们是否被格子盖掉。
    for (i, frac) in [0.3_f32, 0.5, 0.7].into_iter().enumerate() {
        let w = far_extent * 0.6;
        let h = far_cell * 0.6;
        let mesh = meshes.add(Rectangle::new(w, h));
        let mat = materials.add(GridLinesMaterial {
            // 笔迹复用材质只为省一个资产类型：把格子画成 0 alpha 的「纯色块」。
            // （真正的笔迹走 ColorMaterial；冒烟不为多一个资产类型引入依赖。）
            params: grid_params(far_cell, Vec2::ZERO, LinearRgba::rgb(0.12, 0.10, 0.09), 0.0),
        });
        far.push(
            commands
                .spawn((
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                    Transform::from_xyz(
                        far_extent * 0.5,
                        far_extent * (0.2 + 0.3 * i as f32),
                        GRID_Z + 0.001,
                    ),
                ))
                .id(),
        );
        let _ = (frac, i);
    }
    commands.insert_resource(Script { frame: 0, far });
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut commands: Commands,
    mut script: ResMut<Script>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridLinesMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    script.frame += 1;
    match script.frame {
        FRAME_SHOT_FAR => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../docs/reviews/2026-10-23-grid-frames/far-fit-5.12.png"
                )));
        }
        FRAME_SWAP_TO_NEAR => {
            // 近观档：cell_size = 4.0（= scale 1.0），幅面 4096 世界单位，
            // 画面只看到左上 1280×800 的一块——与真实放大行为一致。
            for e in script.far.drain(..) {
                commands.entity(e).despawn();
            }
            let cell = CELL_AT_SCALE_ONE;
            let extent = 1024.0 * cell;
            let mesh = meshes.add(Rectangle::new(extent, extent));
            let mat = materials.add(GridLinesMaterial {
                params: grid_params(cell, Vec2::ZERO, LinearRgba::rgb(0.16, 0.14, 0.13), 0.48),
            });
            commands.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(mat),
                Transform::from_xyz(extent * 0.5, extent * 0.5, GRID_Z),
            ));
            // 近观的一笔墨迹（0.6 格厚 = 2.4px，与真实笔迹同量级）。
            let w = extent * 0.15;
            let h = cell * 0.6;
            let stroke = meshes.add(Rectangle::new(w, h));
            let stroke_mat = materials.add(GridLinesMaterial {
                params: grid_params(cell, Vec2::ZERO, LinearRgba::rgb(0.12, 0.10, 0.09), 0.0),
            });
            commands.spawn((
                Mesh2d(stroke),
                MeshMaterial2d(stroke_mat),
                Transform::from_xyz(extent * 0.5, extent * 0.5, GRID_Z + 0.001),
            ));
        }
        FRAME_SHOT_NEAR => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../docs/reviews/2026-10-23-grid-frames/near-1.0.png"
                )));
        }
        FRAME_QUIT => {
            println!("GRID_SMOKE_OK frames={}", script.frame);
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
