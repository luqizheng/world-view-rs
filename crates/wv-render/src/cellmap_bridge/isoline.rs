//! 等值线矢量叠加（ADR-0012 / plan M2）：借 `chunks_dirty` 感知地形内容变化
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

// ══════ from src/terrain_view/isoline_overlay.rs :: sync_isoline_overlay ══════
/// 同步等值线叠加层。排在 [`super::terrain_chunks::sync_terrain_chunks`]
/// **之前**：借它还没消费的 `tex.chunks_dirty` 感知「地形内容变了」
/// （笔刷提交 / 整图重建都会置位），结构键本身不吸收这个瞬态位。
#[allow(clippy::too_many_arguments)]
pub fn sync_isoline_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    recipe: Res<crate::recipe::RenderRecipe>,
    mut overlay: ResMut<IsolineOverlay>,
) {
    // 专业制图（GoogleMap）：海岸线是**底图要素**而不是「修饰线」——恒开，
    // 不受 `view.isolines` 会话开关支配（ADR-0061 追记；样张图例的水岸线）。
    let cartographic = view.mode == crate::state::ViewMode::GoogleMap;
    // 模式轴：recipe 表（VectorFlat / Topographic / GoogleMap 开档）；
    // 会话轴：`view.isolines` 旗标（map controls 瓷砖，GridLines 同款，
    // 2026-10-23 默认关——ADR-0051 追记）。普通地图例外，理由见上。
    // 关图层（YAML ∨ 会话，ADR-0013）/ 换模式即退场（键随之清零，重进重算）。
    if !layers.terrain.visible
        || !recipe.enabled(crate::recipe::PassId::Isoline)
        || !(view.isolines || cartographic)
    {
        overlay.despawn(&mut commands);
        return;
    }
    let Some(data) = texture_terrain(&tex, &projection.world, drill.current()) else {
        overlay.despawn(&mut commands);
        return;
    };
    // 处理旗标必须与 `conv_pack_with` **同源**：专业制图强制 EMPTY（海岸晕 /
    // 河中线 / 亚线性描边都是手绘语汇）。此前这里直读主题旗标——一旦给
    // GoogleMap 开 Isoline，水彩海岸晕会立刻回来。
    let treatment = if cartographic {
        conv_treatment(&wv_default_theme::theme_pack::TreatmentFlags::EMPTY)
    } else {
        conv_treatment(&view.active_theme.treatment)
    };
    let nation = view.scale >= crate::state::LOD_NATION_SCALE;
    // 海岸线 only：远观（Nation 档）与专业制图都不画类型边界场——后者的
    // 群系边界由**矢量面填充**（`biome_fill`）承担，再叠一层阶梯线是双线。
    let coast_only = nation || cartographic;
    let key = IsolineKey {
        map: tex.last_map.clone(),
        size: tex.size,
        height: tex.height,
        res: tex.res,
        soften: tex.last_soften,
        treatment: [
            treatment.coast_halo,
            treatment.river_centerline,
            treatment.ink_sublinear,
        ],
        tol_bucket: dp_tol_bucket(view.scale),
        // 键里记 `coast_only` 而不是 `nation`：两者产出同一套几何（只有海岸链），
        // 且专业制图切换必须让键失配——否则从 VectorFlat 切到 GoogleMap 会命中
        // 上一模式的类型边界链（双线 / 残留）。
        nation: coast_only,
    };
    let structural_same = overlay.key.as_ref() == Some(&key);
    let dirty_pending = tex.chunks_dirty.is_some();
    // 「几何不必重算」判定：结构同键，且（无脏区 ∨ 脏区来自纯换色——地形内容
    // 指纹未变）。palette 切换的整图重建会置全图 `chunks_dirty`，但 cells 逐格
    // 未动：指纹比对（~3ms）换掉整条派生管线（1024² 实测 ~75ms）。
    let geometry_unchanged = structural_same
        && (overlay.entity.is_some() || overlay.empty_settled())
        && (!dirty_pending || overlay.data_hash == Some(terrain_data_hash(data)));
    if geometry_unchanged {
        // 结构没变也可能只是图层绑定变了（作者改 opacity/z）：就地补丁，
        // 不重算派生几何。palette 变了同样只重上色（`ink_palette` 快路径）。
        if layers.is_changed() || overlay.ink_palette != Some(view.active_theme.id) {
            let mut pal = conv_palette(&view.active_theme.palette);
            if cartographic {
                pal.isoline_ink = wv_default_theme::colors_google::GOOGLE_COAST_LINE_COLOR;
            }
            apply_material_colors(&mut materials, &overlay, &pal, layers.terrain.opacity);
            overlay.ink_palette = Some(view.active_theme.id);
        }
        if layers.is_changed() {
            if let Some(entity) = overlay.entity {
                commands.entity(entity).insert(Transform::from_xyz(
                    0.0,
                    0.0,
                    ISOLINE_Z + layers.terrain.z_offset,
                ));
            }
        }
        return;
    }

    // 派生管线（plan §2.4：提取 → 装配 → Chaikin → DP）。
    let w = data.width;
    let h = data.height;
    let cell = |x: u32, y: u32| &data.cells[(y * w + x) as usize];
    // 容差取**档中心**（不用连续式）：否则键说「同档」而容差随 scale 漂移，
    // 几何变了却不重建（2026-10-23 D 项修复的配套约束）。
    let tol = dp_tol_for_bucket(key.tol_bucket);
    // 装饰旗标（ADR-0023 D4）：从当前主题包派生。内置两包都是 HAND_DRAWN（全
    // `true`）——所有 gate 分支在 `true` 时走与历史恒开**逐位相同**的路径。
    let treatment = conv_treatment(&view.active_theme.treatment);
    // 描边世界半宽（手绘地图 #10，2026-10-15）：**亚线性**缩放——
    // 世界半宽 = px × scale^0.78 而非 px × scale（ADR-0008 §8 乘口径的
    // 修正式）。civ-atlas GLYPH_SHRINK=0.22 同款思路：放大 4 倍时墨线的
    // *世界*尺寸只长 ~3 倍 → *屏幕*观感随放大反而变纤细（k^-0.22），地图
    // 细节浮现而墨线不霸道；缩小时屏幕上线变粗，远观仍可辨。
    // 世界下限保留：极远时防消隐。
    // 装饰旗标 gate：`ink_sublinear = false` 的主题回退 ADR-0008 §8 的严格
    // 乘口径（屏幕恒定宽）。
    let thin_half = if treatment.ink_sublinear {
        stroke_half_world(STROKE_PX, view.scale, grid.cell_size * 0.03)
    } else {
        (STROKE_PX * 0.5 * view.scale).max(grid.cell_size * 0.03)
    };
    // 类型边界加宽：格缘阶梯被 Chaikin 磨角后，平滑曲线偏离原阶梯至多约
    // 0.3 格——细线盖不住露出的「错色台阶」，按格径给足半宽才压得平。
    let kind_half = thin_half.max(grid.cell_size * KIND_STROKE_CELL);

    // ---- 提取 + 装配 + Chaikin 缓存（Task 3）----
    // 这些与缩放无关；只有 DP 容差 / Nation 档消费它们。缓存输入不含 tol_bucket：
    // 缩放只在缓存上重跑 DP（毫秒级），笔刷改地形 / 切图 / 改 treatment 才重建链。
    let data_hash = terrain_data_hash(data);
    let chain_inputs = ChainCacheInputs {
        map: key.map.clone(),
        data_hash,
        treatment: key.treatment,
        nation,
    };
    let rebuild_chains = overlay.chains.as_ref().map(|c| &c.inputs) != Some(&chain_inputs);
    if rebuild_chains {
        let coast_segs = height_segments(w, h, |x, y| cell(x, y).height as f32, 0.0);
        // 只画海岸线：远观（Nation 档）与专业制图（群系边界走矢量面填充）。
        // 类型边界在远观读不出差异，却让描边总长翻数倍。
        let kinds_segs = (!coast_only).then(|| {
            kind_segments(w, h, |x, y| {
                wv_terrain::biome::display_kind(&wv_terrain::TerrainCell {
                    height: cell(x, y).height,
                    surface: cell(x, y).surface,
                    biome: cell(x, y).biome,
                })
            })
        });
        let coast_smoothed: Vec<IsolineChain> = assemble_chains(&coast_segs)
            .into_iter()
            .map(|c| (chaikin(&c.pts, c.closed, 2), c.closed))
            .collect();
        let kinds_smoothed: Vec<IsolineChain> = kinds_segs
            .as_deref()
            .map(|segs| {
                assemble_chains(segs)
                    .into_iter()
                    // 类型场边界是格缘阶梯：先小容差预抽稀去共线中点，再 Chaikin，
                    // 否则平滑后的点数翻两倍全是楼梯碎点。
                    .map(|c| {
                        let pts = simplify_dp(&c.pts, c.closed, 0.30);
                        (chaikin(&pts, c.closed, 2), c.closed)
                    })
                    .collect()
            })
            .unwrap_or_default();
        overlay.chains = Some(CachedChains {
            inputs: chain_inputs,
            coast_smoothed,
            kinds_smoothed,
        });
    }
    // 缩放热路径：只跑 DP + 半宽。
    let (strokes, coast_halo_inner, coast_halo_outer, coast_river) = {
        let chains = overlay.chains.as_ref().expect("上面刚建立");
        assemble_strokes(
            &chains.coast_smoothed,
            &chains.kinds_smoothed,
            tol,
            thin_half,
            kind_half,
            treatment,
        )
    };

    let Some(mesh) = build_stroke_mesh(&strokes, grid.raster_origin, grid.cell_size) else {
        // 空几何（无等值线）也是稳态：0 顶点 mesh 会踩 bevy #24874
        // （MeshAllocator 误报 Use-after-free），退场不插；键记为「空已定」，
        // 下一帧走 `empty_settled` 快路，不重算。
        overlay.despawn(&mut commands);
        overlay.key = Some(key);
        overlay.data_hash = Some(data_hash);
        overlay.ink_palette = Some(view.active_theme.id);
        return;
    };
    let handle = meshes.add(mesh);
    let mut pal = conv_palette(&view.active_theme.palette);
    if cartographic {
        pal.isoline_ink = wv_default_theme::colors_google::GOOGLE_COAST_LINE_COLOR;
    }
    let material = overlay
        .material
        .get_or_insert_with(|| materials.add(ColorMaterial::from_color(pal.isoline_ink)))
        .clone();
    // 材质跨重建复用（get_or_insert）：重算后也要按当前绑定刷新 alpha。
    apply_material_colors(&mut materials, &overlay, &pal, layers.terrain.opacity);

    overlay.despawn(&mut commands);
    let entity = commands
        .spawn((
            Mesh2d(handle.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, ISOLINE_Z + layers.terrain.z_offset),
        ))
        .id();
    overlay.entity = Some(entity);
    overlay.mesh = Some(handle);

    // 水彩外晕（手绘地图 #4）：仅当海岸链非空时生成。晕在主线下层（z 更小），
    // 避免遮住主线。alpha 渐变（`HALO_*_ALPHA`）实现水彩晕染感。
    if !coast_halo_inner.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_halo_inner, grid.raster_origin, grid.cell_size)
        {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.isoline_ink
                    .with_alpha(HALO_1_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z - 0.002 + layers.terrain.z_offset),
                ))
                .id();
            overlay.halo_outer = Some((e, h, mat));
        }
    }
    if !coast_halo_outer.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_halo_outer, grid.raster_origin, grid.cell_size)
        {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.isoline_ink
                    .with_alpha(HALO_2_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z - 0.004 + layers.terrain.z_offset),
                ))
                .id();
            overlay.halo_outer_outer = Some((e, h, mat));
        }
    }
    // 河水中线（手绘地图 #8）：在主线上方，z+0.001，宽度 0.5× main，
    // `label_water` 颜色 + 0.5 alpha——"水边反光"观感（civ-atlas 风格）。
    if !coast_river.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_river, grid.raster_origin, grid.cell_size) {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.label_water
                    .with_alpha(RIVER_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z + 0.001 + layers.terrain.z_offset),
                ))
                .id();
            overlay.coast_river = Some((e, h, mat));
        }
    }

    overlay.key = Some(key);
    overlay.data_hash = Some(terrain_data_hash(data));
    overlay.ink_palette = Some(view.active_theme.id);
}
