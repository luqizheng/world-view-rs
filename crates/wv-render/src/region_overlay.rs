//! 区域覆盖物解析索引 + 缺失覆盖物标记（ADR-0045 D8 / D9 / D10 · 追记一）。
//!
//! ## 两条来源（ADR-0045 D2）
//!
//! | 来源 | 载荷 | 落盘 | 优先级 |
//! |---|---|---|---|
//! | **cell 派生**（ADR-0039） | 格子的海拔带 / 归一化 y 气候带 | 不落盘（P7 纯投影） | 低 |
//! | **region 显式声明**（本模块） | `Region.overlay` 的覆盖物键 | 落盘（作者声明的事实） | **高** |
//!
//! ## 缺失覆盖物标记（ADR-0045 追记一）
//!
//! region 声明的键在**当前主题无规则** → **白底红斜线接管该区域**。
//!
//! **不静默回退**：回退到 cell 派生默认 = 拿另一个变体（阔叶）冒充作者声明
//! （针叶）——工具不该替作者决定「你的声明不重要」。缺的是**主题规则**，
//! 数据本身完好（键还在 YAML 里）。
//!
//! ## 边界
//!
//! - **不画装饰符号本身**：符号仍由 `terrain_decor` 画；本模块只提供
//!   「每格用哪条规则」的解析结果 + 缺失标记的几何；
//! - **不写世界**：纯渲染投影（P7，AGENTS §3）。
//! - **不依赖作者 z 偏移**：缺失标记属底图族（Z 0.078），随 `terrain` 层显隐。

#![cfg(feature = "app")]

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use wv_core::model::region::Region;
use wv_core::Id;
use wv_theme::overlays::RegionOverlayRule;
use wv_theme::theme_pack::ThemePack;

use crate::faction_overlays::earcut_with_holes;
use crate::layer_frame::LayerFrame;
use crate::recipe::RenderRecipe;
use crate::state::{DrillState, GridConfig, ViewState, WorldProjection};

/// 区域层在本帧是否可见（2026-10-22 内容矩阵 §2.6 的唯一判定入口）。
///
/// - **CellMap（Topographic）**：只看 `nature_overlay` 开关，且**仅 Nature 类**
///   放行（Area 区属内容，CellMap 永远藏）；recipe 格（false）被旁路。
/// - **其他模式**：走 recipe（RegionLayers 行）∧ 图层。
///   Schematic 行=false ⇒ 编辑态区域消失（用户裁定：区域有时间性）。
pub(crate) fn region_layer_visible(
    mode: crate::state::ViewMode,
    nature_overlay: bool,
    kind: wv_core::model::region::RegionKind,
    recipe_enabled: bool,
    layer_visible: bool,
) -> bool {
    use crate::state::ViewMode;
    use wv_core::model::region::RegionKind;
    if !layer_visible {
        return false;
    }
    if matches!(mode, ViewMode::Topographic) {
        return nature_overlay && matches!(kind, RegionKind::Nature);
    }
    recipe_enabled
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    use crate::state::ViewMode;
    use wv_core::model::region::RegionKind;

    #[test]
    fn region_layer_in_cellmap_shows_nature_only_when_overlay_on() {
        let kinds = [RegionKind::Nature, RegionKind::Area];
        // 开关开：Nature ✓、Area ✗
        assert!(region_layer_visible(
            ViewMode::Topographic,
            true,
            kinds[0],
            false,
            true
        ));
        assert!(!region_layer_visible(
            ViewMode::Topographic,
            true,
            kinds[1],
            false,
            true
        ));
        // 开关关：全隐（recipe 格本就 false）
        assert!(!region_layer_visible(
            ViewMode::Topographic,
            false,
            kinds[0],
            false,
            true
        ));
    }

    #[test]
    fn region_layer_outside_cellmap_follows_recipe() {
        let nature = RegionKind::Nature;
        assert!(region_layer_visible(
            ViewMode::VectorFlat,
            false,
            nature,
            true,
            true
        ));
        assert!(!region_layer_visible(
            ViewMode::VectorFlat,
            false,
            nature,
            false,
            true
        ));
        // Schematic：recipe 格=false ⇒ 无论开关都藏（开关只在 CellMap 有意义）
        assert!(!region_layer_visible(
            ViewMode::Schematic,
            true,
            nature,
            false,
            true
        ));
    }

    #[test]
    fn region_layer_respects_layer_visibility_everywhere() {
        assert!(!region_layer_visible(
            ViewMode::VectorFlat,
            false,
            RegionKind::Nature,
            true,
            false
        ));
        assert!(!region_layer_visible(
            ViewMode::Topographic,
            true,
            RegionKind::Nature,
            false,
            false
        ));
    }
}
use crate::theme::palette::MapPalette;

/// 缺失覆盖物标记的 Z：地形装饰（0.075）之上、道路（0.09）之下——属底图族，
/// 不压过 POI 与标注（ADR-0045 追记一）。
pub const MISSING_MARKER_Z: f32 = 0.078;

/// 斜纹的世界周期（wu）：贴图按世界空间 UV 平铺，条纹粗细随缩放变化。
const HATCH_PERIOD_WU: f32 = 24.0;

/// 缺失标记的固定诊断红（**不随主题色板**——古舆图等主题的土红系会与斜线
/// 混成"装饰"，问题就隐形了；ADR-0045 追记一）。
pub const MISSING_MARKER_RED: [u8; 3] = [0xD6, 0x45, 0x45];

/// 一个参与覆盖物解析的自然区域（ADR-0045 D8/D9）。
#[derive(Debug, Clone)]
pub struct OverlayEntry {
    /// 区域 ID。
    pub region_id: Id,
    /// 作者声明的覆盖物键。
    pub key: String,
    /// 当前主题的规则；`None` = 主题**无此键** → 缺失覆盖物标记。
    pub rule: Option<RegionOverlayRule>,
    /// 嵌套深度（0 = 顶层；最深优先）。
    pub depth: usize,
}

/// 缺失标记的几何（**图内局部 wu**）。
#[derive(Debug, Clone)]
pub struct MissingShape {
    /// 区域 ID（诊断面板用）。
    pub region_id: Id,
    /// 覆盖物键（诊断面板用）。
    pub key: String,
    /// 外环（扁平点列）。
    pub outer: Vec<f32>,
    /// 洞环。
    pub holes: Vec<Vec<f32>>,
}

/// 区域覆盖物解析索引（ADR-0045 D8/D9）。
///
/// **稀疏**：只有**声明了覆盖物**的 region 参与查表（通常个位数），
/// 其余 region 仅在作者显式查询"这格属于哪个区"时栅格化。
#[derive(Resource, Debug, Default)]
pub struct RegionOverlayIndex {
    /// 参与解析的区域（按深度**降序** → 最深优先）。
    pub entries: Vec<OverlayEntry>,
    /// 格索引 → 条目下标。
    pub cell_owner: HashMap<u32, u32>,
    /// 缺失标记覆盖的格（装饰采样在这些格上跳过符号——白底接管）。
    pub missing_cells: HashSet<u32>,
    /// 缺失标记的几何。
    pub missing_shapes: Vec<MissingShape>,
    /// 缓存键：当前图 id（换图即重算）。
    pub map_id: Option<String>,
    /// 缓存键：主题（换主题即重算——**键存在性**变了，缺失标记随之增减）。
    pub theme_palette: MapPalette,
}

impl RegionOverlayIndex {
    /// 该格是否被缺失标记接管（装饰符号应跳过）。
    pub fn is_missing_cell(&self, cell_index: u32) -> bool {
        self.missing_cells.contains(&cell_index)
    }

    /// 该格生效的覆盖物规则（`None` = 无 region 声明，或声明的键主题不认）。
    pub fn rule_at(&self, cell_index: u32) -> Option<&RegionOverlayRule> {
        let idx = *self.cell_owner.get(&cell_index)?;
        self.entries[idx as usize].rule.as_ref()
    }
}

/// 区域 id → 条目下标（诊断面板用）。
pub fn entry_of_region<'a>(index: &'a RegionOverlayIndex, region: &Id) -> Option<&'a OverlayEntry> {
    index.entries.iter().find(|e| &e.region_id == region)
}

/// 构建解析索引（**纯函数**，可脱离 Bevy 单测）。
///
/// 步骤：
/// 1. 挑出当前图上 `kind = nature` 且**声明了覆盖物键**的区域；
/// 2. 按嵌套深度**降序**（最深优先，ADR-0045 D8）排序；
/// 3. 逐个栅格化区域多边形 → 掩膜 → 逐格归属（**先到先得** = 最深优先）；
/// 4. 主题查无此键的条目 → 缺失标记：其掩膜**减去**「更深的、能正常解析的
///    子区掩膜」（父区的缺失标记不得盖住子区已正确渲染的部分，ADR-0045 追记一）。
pub fn build_index(
    regions: &[Region],
    map: &Id,
    cell_size: f32,
    grid_w: u32,
    grid_h: u32,
    theme: &ThemePack,
    raster_to_map: Vec2,
) -> RegionOverlayIndex {
    let mut index = RegionOverlayIndex::default();
    if cell_size <= 0.0 || grid_w == 0 || grid_h == 0 {
        return index;
    }
    // ---- 1. 候选 + 深度 ----
    let mut cands: Vec<(&Region, usize)> = regions
        .iter()
        .filter(|r| &r.map == map && r.is_nature() && r.declared_overlay().is_some())
        .map(|r| (r, r.depth(regions)))
        .collect();
    // 最深优先（深度大的先处理 → 先占格 → 浅的盖不过深的）。
    cands.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
    if cands.is_empty() {
        return index;
    }

    // ---- 2. 逐个区域：掩膜 → 归属 / 缺失 ----
    let mut resolved_cells: HashSet<u32> = HashSet::new();
    for (region, depth) in cands {
        let Some(key) = region.declared_overlay() else {
            continue;
        };
        let rule = theme.region_overlays.rule_for(key).copied();
        let mask = region_mask(region, cell_size, grid_w, grid_h, raster_to_map);
        if mask.is_empty() {
            continue;
        }
        let entry_idx = index.entries.len() as u32;
        index.entries.push(OverlayEntry {
            region_id: region.id.clone(),
            key: key.to_string(),
            rule,
            depth,
        });
        match rule {
            Some(_) => {
                // 能正常解析：占据格（最深优先 = 先到先得），并记为「已解析覆盖」。
                for (i, &inside) in mask.iter().enumerate() {
                    if inside {
                        index.cell_owner.entry(i as u32).or_insert(entry_idx);
                        resolved_cells.insert(i as u32);
                    }
                }
            }
            None => {
                // 缺失：掩膜减去「更深的、已解析的区域」→ 缺失标记的实际覆盖。
                let mut missing = vec![false; mask.len()];
                for (i, &inside) in mask.iter().enumerate() {
                    if inside && !resolved_cells.contains(&(i as u32)) {
                        missing[i] = true;
                    }
                }
                for (i, &m) in missing.iter().enumerate() {
                    if m {
                        index.missing_cells.insert(i as u32);
                    }
                }
                if let Some(shape) =
                    mask_to_shape(&missing, grid_w, grid_h, cell_size, raster_to_map)
                {
                    index.missing_shapes.push(MissingShape {
                        region_id: region.id.clone(),
                        key: key.to_string(),
                        outer: shape.0,
                        holes: shape.1,
                    });
                }
            }
        }
    }
    index.map_id = Some(map.as_str().to_string());
    index
}

/// 区域多边形（+洞）→ 格掩膜。
///
/// **`raster_to_map` = `map_origin - raster_origin`**：区域多边形是**图内局部
/// wu**，而地形格属于**栅格所在的图**（子图上两者不是同一张图——两套原点，
/// 见 `GridConfig`）。换算必须先减掉这个偏移，否则子图的区域会整体错位。
fn region_mask(
    region: &Region,
    cell_size: f32,
    grid_w: u32,
    grid_h: u32,
    raster_to_map: Vec2,
) -> Vec<bool> {
    let outer = wu_to_cells(&region.polygon, cell_size, raster_to_map);
    if outer.len() < 3 {
        return Vec::new();
    }
    let holes: Vec<Vec<(f32, f32)>> = region
        .holes
        .iter()
        .map(|h| wu_to_cells(h, cell_size, raster_to_map))
        .filter(|h| h.len() >= 3)
        .collect();
    wv_terrain::region::mask_of_ring_set(
        &wv_terrain::region::RingSet { outer, holes },
        grid_w,
        grid_h,
    )
}

/// wu 扁平点列 → 格坐标点列（先减 `raster_to_map` 偏移）。
fn wu_to_cells(ring: &[f32], cell_size: f32, raster_to_map: Vec2) -> Vec<(f32, f32)> {
    ring.chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| {
            (
                (p[0] - raster_to_map.x) / cell_size,
                (p[1] - raster_to_map.y) / cell_size,
            )
        })
        .collect()
}

/// 掩膜 → (外环 wu, 洞环 wu)：掩膜取环（沿图幅矩形闭合）→ 换算回 wu。
fn mask_to_shape(
    mask: &[bool],
    grid_w: u32,
    grid_h: u32,
    cell_size: f32,
    raster_to_map: Vec2,
) -> Option<(Vec<f32>, Vec<Vec<f32>>)> {
    let rs = wv_terrain::region::ring_set_from_mask(mask, grid_w, grid_h)?;
    // 格坐标 → wu：乘回格尺寸，**加回** `raster_to_map` 偏移（图内局部 wu）。
    let outer = rs
        .outer
        .iter()
        .flat_map(|&(x, y)| {
            [
                x * cell_size + raster_to_map.x,
                y * cell_size + raster_to_map.y,
            ]
        })
        .collect();
    let holes = rs
        .holes
        .iter()
        .map(|h| {
            h.iter()
                .flat_map(|&(x, y)| {
                    [
                        x * cell_size + raster_to_map.x,
                        y * cell_size + raster_to_map.y,
                    ]
                })
                .collect()
        })
        .collect();
    Some((outer, holes))
}

// ---------------------------------------------------------------------------
// 渲染系统
// ---------------------------------------------------------------------------

/// 求值区域覆盖物索引（每帧检查变更，变了才重算）。
///
/// **重算条件**：世界变更 / 钻取换图 / 栅格换图 / 主题切换。
/// 纯相机运动**零工作**（同路网缓存纪律）。
pub fn sync_region_overlay_index(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut index: ResMut<RegionOverlayIndex>,
) {
    let map_id = drill.current().cloned();
    let theme = view.active_theme;
    let (Some(map_id), Some((w, h))) = (map_id, terrain_dims(&projection.world, &drill)) else {
        index.entries.clear();
        index.cell_owner.clear();
        index.missing_cells.clear();
        index.missing_shapes.clear();
        return;
    };
    let stale = projection.is_changed()
        || drill.is_changed()
        || grid.is_changed()
        || view.map_palette != index.theme_palette
        || index.map_id.as_deref() != Some(map_id.as_str());
    if !stale {
        return;
    }
    // 两套原点的差：区域多边形是**图内局部 wu**，地形格属于**栅格所在的图**
    // （子图上不是同一张图）——换算必须带这个偏移。
    let raster_to_map = grid.map_origin - grid.raster_origin;
    *index = build_index(
        &projection.world.regions,
        &map_id,
        grid.cell_size,
        w,
        h,
        theme,
        raster_to_map,
    );
    index.map_id = Some(map_id.as_str().to_string());
    index.theme_palette = view.map_palette;
}

/// 活动地形栅格的尺寸（当前图的栅格；无则回退根图）。
fn terrain_dims(world: &wv_core::World, drill: &DrillState) -> Option<(u32, u32)> {
    let map_id = drill.current()?;
    crate::terrain_view::terrain_resolve::active_terrain(world, Some(map_id))
        .map(|(_, data)| (data.width, data.height))
}

/// 缺失覆盖物标记的运行时资源（贴图 / 材质句柄 + 已生成实体）。
#[derive(Resource, Default)]
pub struct MissingMarkerAssets {
    /// 斜纹贴图句柄（白底红斜线，repeat 采样）。
    pub texture: Option<Handle<Image>>,
    /// 材质句柄。
    pub material: Option<Handle<ColorMaterial>>,
    /// 当前帧的标记实体。
    pub entities: Vec<Entity>,
    /// 已生成形状的指纹（内容不变则不重建）。
    pub fingerprint: u64,
}

/// 缺失覆盖物标记系统：白底红斜线**接管**该区域。
///
/// - **三档 LOD 全画**（不可被降档隐藏：装饰在 Nation 档本就不画，缺失标记
///   若跟着降档，作者切到全局视图反而在最需要发现时看不到）；
/// - **固定诊断色**（不随主题色板）；
/// - 绘制在装饰（0.075）之上、道路（0.09）之下；
/// - 底层装饰符号在该区域**跳过**（白底接管，见 `RegionOverlayIndex::is_missing_cell`）。
#[allow(clippy::too_many_arguments)]
pub fn sync_missing_overlay_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    index: Res<RegionOverlayIndex>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    recipe: Res<RenderRecipe>,
    layers: Res<LayerFrame>,
    mut assets: ResMut<MissingMarkerAssets>,
) {
    // 2026-10-22 内容矩阵：区域层从 TerrainDecor 拆到独立 pass（ADR-0051）。
    // CellMap（Topographic）：nature_overlay 放行 Nature 类（判定入口唯一）。
    let enabled = crate::region_overlay::region_layer_visible(
        view.mode,
        view.nature_overlay,
        wv_core::model::region::RegionKind::Nature,
        recipe.enabled(crate::recipe::PassId::RegionLayers),
        layers.terrain.visible,
    );
    if !enabled || index.missing_shapes.is_empty() {
        for e in assets.entities.drain(..) {
            commands.entity(e).despawn();
        }
        assets.fingerprint = 0;
        return;
    }
    // 指纹：形状内容不变则不重建（相机平移零工作）。
    let fp = shapes_fingerprint(&index.missing_shapes);
    if fp == assets.fingerprint && !assets.entities.is_empty() {
        return;
    }
    for e in assets.entities.drain(..) {
        commands.entity(e).despawn();
    }
    assets.fingerprint = fp;

    // 斜纹贴图 + 材质（一次）。
    if assets.texture.is_none() {
        assets.texture = Some(hatch_texture(&mut images));
    }
    let texture = assets.texture.clone().expect("刚建");
    if assets.material.is_none() {
        // Bevy 0.19：ColorMaterial 无 `from_texture`，直接构造（贴图 + 全白染色）。
        let mat = ColorMaterial {
            color: Color::WHITE,
            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
            uv_transform: bevy::math::Affine2::default(),
            texture: Some(texture.clone()),
        };
        assets.material = Some(materials.add(mat));
    }
    let material = assets.material.clone().expect("刚建");

    let map_origin = Vec3::new(grid.map_origin.x, grid.map_origin.y, 0.0);
    for shape in &index.missing_shapes {
        if shape.outer.len() < 6 {
            continue;
        }
        let hole_refs: Vec<&[f32]> = shape.holes.iter().map(|h| h.as_slice()).collect();
        let mesh = build_hatch_mesh(&shape.outer, &hole_refs, HATCH_PERIOD_WU);
        let entity = commands.spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(map_origin + Vec3::Z * MISSING_MARKER_Z),
        ));
        assets.entities.push(entity.id());
    }
}

/// 形状指纹（内容变没变）。
fn shapes_fingerprint(shapes: &[MissingShape]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for s in shapes {
        for v in s.outer.iter().map(|f| f.to_bits()) {
            hash ^= u64::from(v);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for hole in &s.holes {
            for v in hole.iter().map(|f| f.to_bits()) {
                hash ^= u64::from(v);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    hash
}

/// 白底红斜线贴图（**repeat 采样**——否则边缘拉伸成糊条）。
fn hatch_texture(images: &mut Assets<Image>) -> Handle<Image> {
    const SIZE: u32 = 32;
    const STRIPE: u32 = 4; // 4 像素一条 → 32px 贴图内 4 个斜纹周期
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let is_red = ((x + y) / STRIPE).is_multiple_of(2);
            let c = if is_red {
                MISSING_MARKER_RED
            } else {
                [0xFF, 0xFF, 0xFF]
            };
            let i = ((y * SIZE + x) * 4) as usize;
            data[i] = c[0];
            data[i + 1] = c[1];
            data[i + 2] = c[2];
            data[i + 3] = 0xFF;
        }
    }
    let mut image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    // Repeat 采样：否则 UV 超出 [0,1] 时边缘被拉伸成糊条（ADR-0045 追记一）。
    let desc = bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        ..Default::default()
    };
    image.sampler = bevy::image::ImageSampler::Descriptor(desc);
    images.add(image)
}

/// 斜纹 mesh：**世界空间 UV**（静态 mesh，不逐帧重建——ADR-0030 的
/// "每帧重建 mesh = 资产泄漏"教训）。
fn build_hatch_mesh(outer: &[f32], holes: &[&[f32]], period: f32) -> Mesh {
    let tris = earcut_with_holes(outer, holes);
    let positions: Vec<[f32; 3]> = tris.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let uvs: Vec<[f32; 2]> = tris
        .iter()
        .map(|p| [p[0] / period, p[1] / period])
        .collect();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::theme_pack::builtin;
    use wv_core::model::region::RegionKind;

    fn nature(id: &str, name: &str, polygon: Vec<f32>, overlay: Option<&str>) -> Region {
        Region {
            id: Id::new_unchecked(id),
            name: name.into(),
            map: Id::new_unchecked("map_world"),
            polygon,
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: None,
            overlay: overlay.map(str::to_string),
        }
    }

    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<f32> {
        vec![x0, y0, x1, y0, x1, y1, x0, y1]
    }

    /// 单元尺寸 = 1 wu → wu 坐标即格坐标（简化断言）。
    fn build(regions: &[Region], theme: &ThemePack) -> RegionOverlayIndex {
        build_with_offset(regions, theme, Vec2::ZERO)
    }

    /// 带两套原点偏移的构造（子图场景）。
    fn build_with_offset(
        regions: &[Region],
        theme: &ThemePack,
        raster_to_map: Vec2,
    ) -> RegionOverlayIndex {
        build_index(
            regions,
            &Id::new_unchecked("map_world"),
            1.0,
            4,
            4,
            theme,
            raster_to_map,
        )
    }

    #[test]
    fn index_is_empty_when_no_region_declares_overlay() {
        let regions = vec![nature("nr_a", "甲", square(0.0, 0.0, 2.0, 2.0), None)];
        let idx = build(&regions, &builtin::DEFAULT);
        assert!(idx.entries.is_empty(), "无声明 → 无条目");
        assert!(idx.cell_owner.is_empty());
        assert!(idx.missing_cells.is_empty());
    }

    #[test]
    fn area_kind_region_never_declares_overlay() {
        let mut r = nature(
            "nr_area",
            "普通区",
            square(0.0, 0.0, 2.0, 2.0),
            Some("conifer"),
        );
        r.kind = RegionKind::Area;
        let idx = build(&[r], &builtin::DEFAULT);
        assert!(idx.entries.is_empty(), "普通区域不承载覆盖物");
    }

    #[test]
    fn resolved_key_yields_rule_on_covered_cells() {
        let regions = vec![nature(
            "nr_a",
            "迷幻深林",
            square(0.0, 0.0, 2.0, 2.0),
            Some("conifer"),
        )];
        let idx = build(&regions, &builtin::DEFAULT);
        assert_eq!(idx.entries.len(), 1);
        assert!(idx.entries[0].rule.is_some(), "内置主题有 conifer 规则");
        // (0,0) 在区域内 → 有规则；(3,3) 在区域外 → 无。
        assert!(idx.rule_at(0).is_some(), "区内格应有规则");
        assert!(idx.rule_at(3 * 4 + 3).is_none(), "区外格无规则");
        assert!(idx.missing_cells.is_empty(), "可解析 → 无缺失标记");
    }

    #[test]
    fn unknown_key_marks_cells_and_builds_missing_shape() {
        let regions = vec![nature(
            "nr_a",
            "无名林",
            square(0.0, 0.0, 2.0, 2.0),
            Some("no_such_overlay"),
        )];
        let idx = build(&regions, &builtin::DEFAULT);
        assert_eq!(idx.entries.len(), 1);
        assert!(idx.entries[0].rule.is_none(), "主题无此键 → 规则缺省");
        assert!(idx.is_missing_cell(0), "区内格应被缺失标记接管");
        assert!(!idx.is_missing_cell(3 * 4 + 3), "区外格不应被接管");
        assert_eq!(idx.missing_shapes.len(), 1, "应有一块缺失标记几何");
        assert_eq!(idx.missing_shapes[0].key, "no_such_overlay");
    }

    #[test]
    fn deeper_region_wins_over_shallower_one() {
        let mut parent = nature("nr_p", "山脉", square(0.0, 0.0, 4.0, 4.0), Some("conifer"));
        let mut child = nature(
            "nr_c",
            "山峰",
            square(0.0, 0.0, 2.0, 2.0),
            Some("broadleaf"),
        );
        child.parent = Some(Id::new_unchecked("nr_p"));
        parent.parent = None;
        let idx = build(&[parent, child], &builtin::DEFAULT);
        // 子区（更深）优先：区内格的规则来自 child。
        let child_entry = idx
            .entries
            .iter()
            .find(|e| e.region_id.as_str() == "nr_c")
            .expect("子条目");
        assert_eq!(child_entry.depth, 1);
        assert_eq!(
            idx.cell_owner
                .get(&0)
                .map(|i| idx.entries[*i as usize].region_id.as_str()),
            Some("nr_c"),
            "最深优先：子区应夺格"
        );
    }

    #[test]
    fn missing_parent_does_not_swallow_resolving_child() {
        // 父区键主题不认（缺失），子区键可解析 → 父区缺失标记不得盖住子区。
        let mut parent = nature(
            "nr_p",
            "父区",
            square(0.0, 0.0, 4.0, 4.0),
            Some("no_such_overlay"),
        );
        let mut child = nature("nr_c", "子区", square(0.0, 0.0, 2.0, 2.0), Some("conifer"));
        child.parent = Some(Id::new_unchecked("nr_p"));
        parent.parent = None;
        let idx = build(&[parent, child], &builtin::DEFAULT);
        assert!(!idx.is_missing_cell(0), "子区格可解析 → 不该被缺失标记接管");
        assert!(idx.is_missing_cell(3 * 4 + 3), "父区独有格 → 应被接管");
        assert_eq!(idx.missing_shapes.len(), 1);
        // 缺失标记几何应小于父区（扣掉了子区）。
        let shape = &idx.missing_shapes[0];
        assert!(
            shape.outer.len() >= 6,
            "缺失标记应有外环，实得 {} 点",
            shape.outer.len()
        );
    }

    /// **两套原点**：区域多边形是图内局部 wu，格子属于栅格所在的图——子图上
    /// 两者不是同一坐标系，偏移必须参与换算（否则区域整体错位）。
    #[test]
    fn raster_to_map_offset_shifts_the_covered_cells() {
        // 多边形落在 wu (10..12)——4×4 栅格之外。
        let regions = vec![nature(
            "nr_a",
            "深林",
            square(10.0, 10.0, 12.0, 12.0),
            Some("conifer"),
        )];
        // 无偏移：多边形在栅格外 → 不覆盖任何格。
        let idx = build_with_offset(&regions, &builtin::DEFAULT, Vec2::ZERO);
        assert!(
            idx.cell_owner.is_empty(),
            "无偏移时多边形落在 4×4 栅格外，不应覆盖任何格"
        );
        // 偏移 (10,10)：换算后落在格 (0..2)×(0..2) → 覆盖 (0,0)。
        let idx = build_with_offset(&regions, &builtin::DEFAULT, Vec2::new(10.0, 10.0));
        assert!(idx.rule_at(0).is_some(), "带偏移时 (0,0) 格应被区域覆盖");
    }

    #[test]
    fn region_of_another_map_is_ignored() {
        let mut r = nature("nr_a", "别图", square(0.0, 0.0, 2.0, 2.0), Some("conifer"));
        r.map = Id::new_unchecked("map_other");
        let idx = build(&[r], &builtin::DEFAULT);
        assert!(idx.entries.is_empty(), "别的图的区域不参与本图解析");
    }
}

// ── 全图未命名候选高亮（2026-10-22 内容矩阵 §2.6-3 · T11）──────────────

/// 未命名候选的**缓存**：重算代价 O(格数)（`partition_by_biome` 全图 BFS），
/// 不能每帧跑——按 (图, 世界是否变过) 缓存；`WorldProjection.is_changed()`
/// 在任一命令/选择写入时翻真（点击级重算，~10ms 一次，仅 CellMap+开关开时）。
/// 编辑信号快照：脏标记 + undo/redo 栈长与顶 seq（ADR-0051）。
pub type EditSignal = (bool, usize, usize, Option<u64>, Option<u64>);

#[derive(Resource, Debug, Default)]
pub struct CandidateOverlayCache {
    /// 缓存键 = 当前图 id。图变 ⇒ 重算。
    pub key: Option<wv_core::Id>,
    /// 每个候选的外环折线（**视图空间**，供 gizmos 直绘）。
    pub outlines: Vec<Vec<(bevy::math::Vec2, bevy::math::Vec2)>>,
    /// 上次重算的编辑信号快照：脏标记 + (undo 长, redo 长, undo 顶 seq, redo 顶 seq)。
    /// 任一变化 ⇒ 候选过期。不用 `WorldProjection::is_changed`——selection 写入
    /// 也会置真，会把 O(格数) 的重算打成每帧（实机 1 FPS 回归根因）。
    pub last_signal: Option<EditSignal>,
    /// 上次重算时刻（限频：重算 ≥1s 一次，避免连续编辑每帧 93ms）。
    pub last_compute: Option<std::time::Instant>,
    /// 编辑信号 epoch（app 层从 EditHistory 形状派生写入；变化 ⇒ 候选过期）。
    pub last_epoch: u64,
    /// 已绘制候选对应的 epoch。
    pub drawn_epoch: u64,
}

/// 全图枚举未命名候选（region_overlay 系统尾部调用）。
///
/// 排除：已被命名 Nature 区覆盖的候选（代表点 = 外环首点，`point_in_geometry`
/// 测命中）；NONE 群系（无生态语义的格子不构成「自然区」）。
#[derive(Default)]
pub(crate) struct CandidateOutlines {
    pub outlines: Vec<Vec<(bevy::math::Vec2, bevy::math::Vec2)>>,
}

pub(crate) fn enumerate_candidates(
    world: &wv_core::World,
    map: &wv_core::Id,
    grid: &GridConfig,
    raster_to_map: bevy::math::Vec2,
) -> CandidateOutlines {
    use wv_core::base::geometry::point_in_geometry;
    let Some(data) = world.terrain_of(map) else {
        return CandidateOutlines::default();
    };
    let cells: Vec<wv_terrain::TerrainCell> = data
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let Some(g) = wv_terrain::TerrainGrid::from_cells(data.width, data.height, cells) else {
        return CandidateOutlines::default();
    };
    let named: Vec<wv_core::map::GeometryData> = world
        .regions
        .iter()
        .filter(|r| r.map == *map && matches!(r.kind, wv_core::model::region::RegionKind::Nature))
        .filter(|r| !r.polygon.is_empty())
        .map(|r| wv_core::map::GeometryData::Polygon(r.polygon.clone()))
        .collect::<Vec<wv_core::map::GeometryData>>();

    // 预算（性能验收：Zoom 1 FPS 回归）：噪声产生的碎块（<16 格）不构成
    // 「自然区」（对齐 marker 的 ≥16 惯例），且候选总量封顶——
    // 全图分区在噪声群系下可能有上千碎块，无预算会拖垮帧率。
    const MIN_CANDIDATE_CELLS: usize = 16;
    const MAX_CANDIDATES: usize = 300;
    let mut candidates: Vec<_> = wv_terrain::region::partition_by_biome(&g)
        .into_iter()
        .filter(|r| r.cells >= MIN_CANDIDATE_CELLS)
        .filter(|r| r.biome != wv_terrain::BIOME_NONE && r.outer.len() >= 3)
        .collect();
    candidates.sort_by_key(|r| std::cmp::Reverse(r.cells));
    candidates.truncate(MAX_CANDIDATES);

    let mut out = CandidateOutlines::default();
    for region in candidates {
        // 代表点 = 外环首点（格坐标 → 图内局部 wu）。
        let (fx, fy) = region.outer[0];
        let px = fx * grid.cell_size + raster_to_map.x;
        let py = fy * grid.cell_size + raster_to_map.y;
        if named.iter().any(|geo| point_in_geometry(geo, px, py)) {
            continue;
        }
        // 外环折线（视图空间）+ 简易虚线（隔段保留）。
        let ring: Vec<bevy::math::Vec2> = region
            .outer
            .iter()
            .map(|&(x, y)| {
                grid.map_to_view(bevy::math::Vec2::new(
                    x * grid.cell_size + raster_to_map.x,
                    y * grid.cell_size + raster_to_map.y,
                ))
            })
            .collect();
        let mut solid: Vec<(bevy::math::Vec2, bevy::math::Vec2)> = Vec::new();
        // 白色实线直接落在方格边——数据视图无修饰（用户验收裁定）。
        for pair in ring.windows(2) {
            solid.push((pair[0], pair[1]));
        }
        if !solid.is_empty() {
            out.outlines.push(solid);
        }
    }
    out
}
