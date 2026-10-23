//! R1 spike：核验 Bevy 0.19 自定义 `Material2d` + WGSL 的 import 路径与装配方式。
//!
//! 全仓此前只有 `ColorMaterial`，无自定义材质先例（任务背景事实）。
//! ADR-0051 R1 要求「不凭记忆写」——本 example 用一个三角形跑通：
//! - `AsBindGroup` derive（uniform + texture + sampler 三类绑定）
//! - WGSL `#import bevy_sprite::mesh2d_vertex_output::VertexOutput`
//! - `Material2dPlugin::<M>` 装配
//!
//! 验证结论以编译 + 运行（窗口出现三角形）为准，记录进施工报告。

use bevy::{
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    sprite_render::{Material2d, Material2dPlugin, MeshMaterial2d},
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(Material2dPlugin::<SpikeMaterial>::default())
        .add_systems(Startup, setup)
        .run();
}

/// 最小自定义材质：uniform 颜色 + texture/sampler 两个绑定，覆盖三类属性。
#[derive(AsBindGroup, Debug, Clone, Asset, TypePath)]
pub struct SpikeMaterial {
    #[uniform(0)]
    tint: LinearRgba,
    #[texture(1)]
    #[sampler(2)]
    tex: Handle<Image>,
}

impl Material2d for SpikeMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/spike_material.wgsl".into()
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SpikeMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut shaders: ResMut<Assets<Shader>>,
) {
    // 手造 4×4 纹理（不经 AssetServer，验证「Assets 手动注入」路径）。
    let tex = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &[255, 255, 255, 255],
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    let tex_handle = images.add(tex);

    // 手造 shader（不经 AssetServer，验证「Shader::from_wgsl + shaders.add」路径）。
    let wgsl = include_str!("../assets/shaders/spike_material.wgsl");
    let shader = Shader::from_wgsl(wgsl, "shaders/spike_material.wgsl");
    let shader_handle = shaders.add(shader);

    commands.spawn(Camera2d);
    commands.spawn((
        Mesh2d(meshes.add(Triangle2d::new(
            Vec2::new(-100.0, -50.0),
            Vec2::new(100.0, -50.0),
            Vec2::new(0.0, 80.0),
        ))),
        MeshMaterial2d(materials.add(SpikeMaterial {
            tint: LinearRgba::rgb(0.2, 0.9, 0.4),
            tex: tex_handle,
        })),
    ));
    // 引用 shader_handle 防 unused 警告：R1 的备选路径（返回 Handle 而非路径）。
    let _ = &shader_handle;
}
