//! 纸面氛围层（ADR-0016 M2）：**纸面雾霭**（缩放联动降饱和）+ **纸张噪点**。
//!
//! ## 职责
//! - `sync_atmosphere` —— 在地形栅格幅面上铺两层半透明四边形：
//!   - **雾霭**（Z=0.06）：纸面色，alpha 随相机缩放**连续**变化——远景要素向
//!     纸面退散的手绘空气感（用户渲染规则 3）。每帧只改材质 alpha，
//!     **不触发 CPU 纹理重建**（那是 O(格数) 的整图重烘，雾是逐帧连续量）；
//!   - **纸张噪点**（Z=0.5）：世界锁定的程序化噪声平铺，模拟旧纸纤维
//!     （用户渲染规则 2）。UV 按世界坐标计算，随图平移缩放——如真实纸张。
//! - **编辑态免雾**（2026-10-15 用户反馈）：笔刷开着时雾霭整体退场——雾是
//!   **浏览**氛围，而编辑态作者最需要看清的恰恰是自己的笔迹（填充 + 描边都
//!   压在雾下，满雾时深褐墨线被洗到 ΔRGB≈10 的浅灰褐，远观等于没画）。
//!   纸张噪点不退：α0.035 的纤维不参与可见性问题，且纸感属于编辑态也是纸。
//!
//! ## 边界（不做什么）
//! - **不改地形纹理内容**——那是 [`super::cells`]（烘焙路径）+ `terrain_soften`
//!   静态开关；雾霭是叠加在烘焙结果之上的连续量，两者互不替代。
//! - **不遮关键要素**——雾 Z 低于道路（0.09+），POI / 标注 / 势力全部在雾上。
//! - **例外**：GoogleMap（自有完整观感）与 Coastline（黑底分析视图）不铺氛围。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::render::layer_frame::LayerFrame;
use crate::state::{BrushState, GridConfig, ViewMode, ViewState};
use crate::theme::palette;

use super::terrain_texture::TerrainTexture;

/// 雾霭层 Z：等值线（0.04）与换级淡出精灵（0.05）之上、道路（0.09+）之下
/// ——雾只属于底图，道路 / POI / 标注从雾上穿出保持清晰。
pub const VEIL_Z: f32 = 0.06;
/// 雾霭峰值不透明度：拉到最远时地形向纸面压 35%——山川骨架仍可辨、
/// 「水蓝 / 陆绿」色相在缩放 4-5× 仍可分辨（2026-10-16 用户反馈：旧 0.55 + 远缩放档
/// 提前吃满，会让海陆都退成纸色，违背「选了海洋必须显蓝」的契约）。
/// 1.0 会把底图整个洗掉（等于关掉地形），0.20 以下基本看不出雾。
pub const VEIL_MAX_ALPHA: f32 = 0.35;
/// 雾霭起雾缩放锚点（= LOD 分界）：≤ 1.0（编辑级）无雾。
///
/// `VEIL_FAR_SCALE = 6.0`（2026-10-16 旧值 3.0）：旧值在 3× 已吃满雾，
/// 浏览器/画布的常用缩放区间（3-5×）直接退成纸色；新值 6.0 让 1-4× 仍保留
/// 大部分颜色，只有 6× 以上的「超远观感」才进入最大雾——与古舆图「放大看
/// 细节 / 缩小看全局」的语义吻合。
pub const VEIL_NEAR_SCALE: f32 = 1.0;
pub const VEIL_FAR_SCALE: f32 = 6.0;

/// 噪点平铺的世界边长：一张 256² 噪声贴图管 320 世界单位，
/// 常用缩放跨距内噪点始终是「细颗粒」而不是大色块。
pub const GRAIN_TILE_WORLD: f32 = 320.0;
/// 噪点贴图像素边长。
pub const GRAIN_TEX_SIZE: u32 = 256;
/// 噪点峰值不透明度：极淡（用户规则 2「微弱，不要太强」）。
pub const GRAIN_ALPHA: f32 = 0.035;
/// 噪点层 Z：地名（0.4）之上、笔刷光标（0.9）之下——纤维覆盖纸上的一切印刷物。
pub const GRAIN_Z: f32 = 0.5;

/// 纯函数：给定缩放返回雾霭 alpha（测试锚点：1.0→0，2.0→0.275，3.0→0.55，
/// 两端钳制；线性插值——空气感的「缓急」留给实机调参，先不上曲线）。
pub fn veil_alpha(scale: f32) -> f32 {
    let t = ((scale - VEIL_NEAR_SCALE) / (VEIL_FAR_SCALE - VEIL_NEAR_SCALE)).clamp(0.0, 1.0);
    VEIL_MAX_ALPHA * t
}

/// 雾霭的最终 alpha：**编辑态免雾**（2026-10-15 用户裁定）。
///
/// 笔刷开着时恒 0——不是减淡。半截雾仍会把刚画的低对比填充（古舆图淡化
/// 之后本就贴近纸面）洗到不可辨，「看清笔迹」没有折中可言；浏览氛围在
/// 编辑态让位。噪点层不受此开关影响（见模块文档）。
pub fn veil_alpha_for(scale: f32, brush_enabled: bool) -> f32 {
    if brush_enabled {
        0.0
    } else {
        veil_alpha(scale)
    }
}

/// 确定性生成纸张噪点图：灰度 112..=144 微扰 + 均匀低 alpha 通道。
///
/// `seed` 相同则字节级一致（纯整数 xorshift，跨平台一致）。
/// 以中灰为中心的亮暗双向微扰让叠加层既轻微提亮也轻微压暗——纸纤维不是滤色镜。
pub fn paper_grain_image(seed: u64) -> Image {
    let mut px = Vec::with_capacity((GRAIN_TEX_SIZE * GRAIN_TEX_SIZE * 4) as usize);
    let alpha = (GRAIN_ALPHA * 255.0).round() as u8;
    let mut h = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for _ in 0..GRAIN_TEX_SIZE * GRAIN_TEX_SIZE {
        // xorshift64：确定性、跨平台一致（同 cells::hash01 家族）。
        h ^= h << 13;
        h ^= h >> 7;
        h ^= h << 17;
        // 112..=144：以 128 为中心的灰度微扰（× alpha 0.035 后实际波动 ±1.3/255）。
        let g = 112 + (h % 33) as u8;
        px.extend_from_slice(&[g, g, g, alpha]);
    }
    let mut img = Image::new(
        Extent3d {
            width: GRAIN_TEX_SIZE,
            height: GRAIN_TEX_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    // 世界锁定平铺的前提：UV 越界重复采样（默认 Clamp 会把 1.0 外全拉成边缘像素）。
    // 线性过滤 + Repeat 寻址；mag/min 保持默认（线性）。
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        ..Default::default()
    });
    img
}

/// 幅面四边形 mesh（局部原点系 + 世界锁定 UV）。
///
/// `uv_scale = 幅面 / GRAIN_TILE_WORLD`：世界坐标 ÷ 平铺边长，噪点因此钉在
/// 世界坐标上——平移地图时纤维跟着走、缩放时纤维跟纸一起放大（真实纸张行为）。
/// 原点走 `Transform`（mesh 保持局部系以便幅面变化时仍可复用逻辑）。
pub(crate) fn extent_quad_mesh(size: Vec2, uv_scale: Vec2) -> Mesh {
    let (w, h) = (size.x, size.y);
    let mut mesh = Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0, 0.0, 0.0], [w, 0.0, 0.0], [0.0, h, 0.0], [w, h, 0.0]],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![
            [0.0, 0.0],
            [uv_scale.x, 0.0],
            [0.0, uv_scale.y],
            [uv_scale.x, uv_scale.y],
        ],
    );
    mesh.insert_indices(bevy::render::mesh::Indices::U32(vec![0, 2, 1, 1, 2, 3]));
    mesh
}

/// 幅面键：栅格原点 / 尺寸 / 格径 / 缩放档任一变化即重建（与 `grid_lines` 同触发口径）。
///
/// `scale_bucket` 把连续 `view.scale` 量化到 1/8 octaves 档——避免每帧
/// scale 微变都重建（grain mesh 创建有 GPU 开销）；近看纸浆细节变化
///（手绘地图 #6，2026-10-15）的视觉差一档即可辨。
#[derive(Debug, Clone, PartialEq)]
struct AtmosphereKey {
    origin: (u32, u32),
    extent: (u32, u32),
    scale_bucket: u32,
}

/// 把 `view.scale` 量化到 1/8 octaves（`bucket = (log2(scale) × 8).round() as i32`）。
fn scale_bucket(scale: f32) -> u32 {
    if scale <= 0.0 {
        return 0;
    }
    (scale.log2() * 8.0).round().max(0.0) as u32
}

/// 氛围层实体缓存。
#[derive(Resource, Default)]
pub struct AtmosphereLayer {
    veil: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    grain: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    /// 噪点贴图跨幅面复用（幅面变了不用重生成像素）。
    grain_texture: Option<Handle<Image>>,
    key: Option<AtmosphereKey>,
}

impl AtmosphereLayer {
    fn despawn(&mut self, commands: &mut Commands) {
        for slot in [&mut self.veil, &mut self.grain] {
            if let Some((entity, mesh, _)) = slot.take() {
                commands.entity(entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&mesh);
                });
            }
        }
        self.key = None;
    }
}

/// 同步氛围层。独立注册（主链已顶满 20 参元组上限），排在 `sync_map_frame` 之后。
///
/// 幅面 = **栅格幅面**（`raster_origin` + `size × cell_size`）——雾与噪点是
/// 「这张纸」的属性，与底图完全重合。
#[allow(clippy::too_many_arguments)]
pub fn sync_atmosphere(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    brush: Res<BrushState>,
    mut atm: ResMut<AtmosphereLayer>,
) {
    // GoogleMap 有完整自有观感、Coastline 是黑底分析视图——都不铺纸面氛围。
    if !layers.terrain.visible
        || matches!(view.mode, ViewMode::GoogleMap | ViewMode::Coastline)
        || tex.size == 0
    {
        atm.despawn(&mut commands);
        return;
    }
    let key = AtmosphereKey {
        origin: (
            grid.raster_origin.x.to_bits(),
            grid.raster_origin.y.to_bits(),
        ),
        extent: (tex.size, grid.cell_size.to_bits()),
        scale_bucket: scale_bucket(view.scale),
    };
    if atm.key.as_ref() != Some(&key) {
        atm.despawn(&mut commands);
        let size = Vec2::splat(tex.size as f32 * grid.cell_size);
        // 手绘地图 #6：grain uv 随 `view.scale` 微调——近看 grain 翻倍密、远看
        // 减半；连续变化用 `scale_bucket` 量化到 1/8 octaves 档避免逐帧重建。
        let grain_tile = (GRAIN_TILE_WORLD / view.scale.max(0.05)).max(1.0);
        let uv = Vec2::splat(size.x / grain_tile);
        // —— 雾霭（初色 NONE，末尾逐帧更新段马上会写入当前值）——
        let veil_mesh = meshes.add(extent_quad_mesh(size, Vec2::ZERO));
        let veil_mat = materials.add(ColorMaterial::from_color(Color::NONE));
        let veil = commands
            .spawn((
                Mesh2d(veil_mesh.clone()),
                MeshMaterial2d(veil_mat.clone()),
                Transform::from_xyz(grid.raster_origin.x, grid.raster_origin.y, VEIL_Z),
            ))
            .id();
        // —— 纸张噪点 ——
        let grain_tex = atm
            .grain_texture
            .get_or_insert_with(|| images.add(paper_grain_image(0x5EED_5EED)))
            .clone();
        let grain_mesh = meshes.add(extent_quad_mesh(size, uv));
        let grain_mat = materials.add(ColorMaterial {
            texture: Some(grain_tex),
            color: Color::WHITE.with_alpha(GRAIN_ALPHA),
            ..Default::default()
        });
        let grain = commands
            .spawn((
                Mesh2d(grain_mesh.clone()),
                MeshMaterial2d(grain_mat.clone()),
                Transform::from_xyz(grid.raster_origin.x, grid.raster_origin.y, GRAIN_Z),
            ))
            .id();
        atm.veil = Some((veil, veil_mesh, veil_mat));
        atm.grain = Some((grain, grain_mesh, grain_mat));
        atm.key = Some(key);
    }
    // 雾霭 alpha 逐帧连续更新（缩放的空气感 + 编辑态免雾）；颜色随主题纸面
    //（ADR-0016）。免雾只动 alpha 不拆实体：笔刷一关，下一帧雾即回来。
    let pal = palette::colors(view.map_palette);
    if let Some((_, _, mat)) = &atm.veil {
        if let Some(mut m) = materials.get_mut(mat) {
            m.color = pal
                .soften_toward
                .with_alpha(veil_alpha_for(view.scale, brush.enabled));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 雾霭曲线锚点：编辑级无雾、远级全雾、中点线性、两端钳制。
    ///
    /// **2026-10-16 用户裁定改 FAR_SCALE 6.0 后**：中点不再是 2.0（旧的 `mid`），
    /// 而是 `(NEAR + FAR) / 2 = 3.5`；测试按当前常量计算，避免常量改了测试
    /// 不跟手。
    #[test]
    fn veil_alpha_is_zero_near_and_maxed_far() {
        assert_eq!(veil_alpha(0.1), 0.0, "最近端应无雾");
        assert_eq!(veil_alpha(VEIL_NEAR_SCALE), 0.0, "编辑级应无雾");
        assert_eq!(veil_alpha(VEIL_FAR_SCALE), VEIL_MAX_ALPHA, "国家级应全雾");
        assert_eq!(veil_alpha(99.0), VEIL_MAX_ALPHA, "越界应钳制");
        let mid_scale = (VEIL_NEAR_SCALE + VEIL_FAR_SCALE) * 0.5;
        let mid = veil_alpha(mid_scale);
        assert!(
            (mid - VEIL_MAX_ALPHA / 2.0).abs() < 1e-6,
            "中点（scale={mid_scale}）应线性一半：got {mid}"
        );
    }

    /// 远级退色场景（2026-10-16 用户反馈「选了海洋但颜色没变蓝」回归）：
    /// 在用户实际缩放的 4.73×（顶栏 473%）下，veil_alpha 必须 < 0.45——
    /// 旧 FAR_SCALE = 3.0 时 4.73× 直接吃满 0.55 雾，海陆都退成纸色，违背
    /// 「选了海洋必须显蓝」的契约；新 FAR_SCALE = 6.0 把 4.73× 压到 0.261。
    #[test]
    fn veil_alpha_at_user_zoom_does_not_saturate() {
        let alpha_at_user_zoom = veil_alpha(4.73);
        assert!(
            alpha_at_user_zoom < 0.45,
            "缩放 4.73× 下雾应 < 0.45（实际 {alpha_at_user_zoom}），避免海陆退成纸色"
        );
        // 同理：在 5× 缩放下也保持在 0.5 以下。
        assert!(
            veil_alpha(5.0) < 0.50,
            "缩放 5× 下雾应 < 0.50，got {}",
            veil_alpha(5.0)
        );
    }

    /// 编辑态免雾（2026-10-15）：笔刷开着雾恒 0——再远也不许把笔迹洗掉；
    /// 笔刷关着则与纯缩放曲线逐点一致（开关只该切雾，不该改曲线形状）。
    #[test]
    fn brush_enabled_disables_the_veil_at_every_zoom() {
        for scale in [0.1, VEIL_NEAR_SCALE, 2.0, 3.5, VEIL_FAR_SCALE, 99.0] {
            assert_eq!(
                veil_alpha_for(scale, true),
                0.0,
                "scale={scale} 编辑态应无雾"
            );
            assert_eq!(
                veil_alpha_for(scale, false),
                veil_alpha(scale),
                "scale={scale} 浏览态应走原曲线"
            );
        }
    }

    /// 噪点确定性：同种子字节一致、异种子不同（跨帧闪变即此缺陷）。
    #[test]
    fn paper_grain_image_is_deterministic_per_seed() {
        let a = paper_grain_image(42);
        let b = paper_grain_image(42);
        let c = paper_grain_image(43);
        assert_eq!(a.data, b.data, "同种子应逐字节一致");
        assert_ne!(a.data, c.data, "异种子应不同");
    }

    /// 噪点幅面：尺寸 / 通道数正确，alpha 为 GRAIN_ALPHA 的量化值。
    #[test]
    fn paper_grain_image_has_expected_layout_and_low_alpha() {
        let img = paper_grain_image(7);
        assert_eq!(img.width(), GRAIN_TEX_SIZE);
        let data = img.data.as_ref().expect("应有像素");
        assert_eq!(data.len(), (GRAIN_TEX_SIZE * GRAIN_TEX_SIZE * 4) as usize);
        let alpha = data[3];
        assert!(
            (alpha as f32 / 255.0 - GRAIN_ALPHA).abs() < 1e-3,
            "alpha 应为 GRAIN_ALPHA 的 u8 量化值（got {alpha}）"
        );
    }

    /// 幅面四边形：4 顶点 6 索引，UV 缩放正确（世界锁定平铺的前提）。
    #[test]
    fn extent_quad_mesh_has_world_scaled_uvs() {
        let mesh = extent_quad_mesh(Vec2::new(640.0, 640.0), Vec2::splat(2.0));
        assert_eq!(mesh.count_vertices(), 4);
        assert_eq!(mesh.indices().expect("索引应存在").len(), 6);
        let bevy::render::mesh::VertexAttributeValues::Float32x2(uv) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("UV 应存在")
        else {
            panic!("UV 应为 Float32x2");
        };
        assert_eq!(uv[3], [2.0, 2.0], "右上限应等于 uv_scale");
    }

    // -----------------------------------------------------------------------
    // 纸纹缩放（手绘地图呈现 #6，2026-10-15）
    // -----------------------------------------------------------------------

    /// `scale_bucket` 把连续 scale 量化到 1/8 octaves 档：scale=1 → 0、scale=2 → 8。
    #[test]
    fn scale_bucket_quantizes_to_eighth_octaves() {
        assert_eq!(scale_bucket(1.0), 0, "scale=1 应归 0 档");
        assert_eq!(scale_bucket(2.0), 8, "scale=2 应归 8 档（+1 oct）");
        // scale=0.5 时 log2 = -1, ×8 = -8, max(0, ...) = 0 档（与 scale=0 同档）
        assert_eq!(scale_bucket(0.5), 0, "scale=0.5 应归 0 档（log2=-1 截断）");
        // 极端 / 非法值不 panic。
        assert_eq!(scale_bucket(0.0), 0, "scale=0 应归 0");
    }

    /// grain_tile 随 scale 缩小：近（scale=4）tile 缩 4× → grain 翻 4 倍密；
    /// 远（scale=0.25）tile 放 4× → grain 变 1/4 疏。
    #[test]
    fn grain_tile_scales_inversely_with_view_scale() {
        // 复用 sync_atmosphere 内部的同一公式（行内复制避免外露细节）。
        let near = (GRAIN_TILE_WORLD / 4.0_f32.max(0.05)).max(1.0);
        let far = (GRAIN_TILE_WORLD / 0.25_f32.max(0.05)).max(1.0);
        let base = GRAIN_TILE_WORLD;
        assert!(
            near < base,
            "近（scale=4）tile 应 < 基础 tile：near={near} base={base}"
        );
        assert!(
            far > base,
            "远（scale=0.25）tile 应 > 基础 tile：far={far} base={base}"
        );
        assert!((near * 4.0 - base).abs() < 0.5, "近 = 基础/4 关系");
    }
}
