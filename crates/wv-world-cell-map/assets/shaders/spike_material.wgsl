// R1 spike shader：核验 Bevy 0.19 的 WGSL import 段。
// import 路径来自 bevy_sprite_render-0.19.1/src/mesh2d/color_material.wgsl
// 的实测读源（同版本 crate 源码），不凭记忆。
#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}

struct SpikeMaterial {
    tint: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: SpikeMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var tex_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(tex, tex_sampler, in.uv);
    return material.tint * c;
}
