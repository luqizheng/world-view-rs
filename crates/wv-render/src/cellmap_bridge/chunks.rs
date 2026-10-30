//! 地形分块 + mip + 视口拣选：稳态零像素拷贝，笔刷提交后 patch 相交块
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

// ══════ from src/terrain_chunks.rs :: sync_terrain_chunks ══════
/// 主 system：在 `flush_terrain_texture`（wv-app::edit） 与
/// [`crate::terrain_view::rebuild_texture_on_mode_change`] 之后跑。
///
/// 输入：整图 `TerrainTexture` + `view.scale` / `view.camera_center` / 窗口大小。
/// 输出：spawn / despawn chunk 实体；对已存在块按需 patch 像素或切 mip。
///
/// ## 每帧开销
///
/// 稳态（无编辑、无 spawn）：只做视口 AABB 与哈希查找，**零像素拷贝、零资产新建**。
/// 笔刷提交后一帧：克隆整图源像素一次，patch 相交块，重建其 mip1/mip2。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_chunks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut tex: ResMut<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    mut chunk_entities: ResMut<TerrainChunkEntities>,
    mut q_chunks: Query<(&mut TerrainChunk, &Mesh2d)>,
) {
    // ── 底图载体路由（ADR-0053 回退开关 + ADR-0088 模式路由）────────────────
    // 场 quad 上屏 = 底图由 `sync_terrain_field` 的 GPU quad 负责。两条路径并存，
    // 但**同时只能有一条上屏**：整图烘焙纹理与场 quad 叠在一起就是双重底图。
    // 故这里整层退场（回退路径本身一行不动，见下方全部逻辑）。
    //
    // 判据只有一份（`should_render_field`，ADR-0088 D2）：意愿开关 × 模式
    // 支持档。CPU 留守档（Faction/Event/Coastline）落到这里**不**早退、chunk
    // 正常铺——此前本层只读 `view.field_render` 一位，切 CPU 留守档会错画成
    // Themed 场（ADR-0088 §1 缺陷①，本 gate 与 field 侧同消费一个函数兜底）。
    //
    // `chunks_dirty` 照旧在这里消费清零（语义不变）：`sync_terrain_field` 排在
    // **本系统之后**，它用的编辑信号是变更戳而不是这个旗标，故不需要抢。
    // 关回场时块按最新像素 spawn（与下方 `!visible` 分支同口径）。
    if should_render_field(&view) {
        let entities: Vec<Entity> = chunk_entities.by_coord.drain().map(|(_, e)| e).collect();
        for entity in entities {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
        if tex.chunks_dirty.is_some() {
            tex.chunks_dirty = None;
        }
        return;
    }

    // 关闭图层（YAML 声明 ∨ 会话开关，ADR-0013）→ 全部 despawn；
    // 丢掉待消费脏矩形（重开时 spawn 会拷最新像素）。
    if !layers.terrain.visible {
        let entities: Vec<Entity> = chunk_entities.by_coord.drain().map(|(_, e)| e).collect();
        for entity in entities {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
        if tex.chunks_dirty.is_some() {
            tex.chunks_dirty = None;
        }
        return;
    }
    // 绑定变化（作者改 YAML 的 opacity/z）：已存在块就地补丁——不重建像素，
    // 只写材质 alpha 与 Transform.z（重 spawn 的块在下方 spawn 路径取新值）。
    // 借**延迟命令**写：本系统的 `q_chunks` 已持有 `&mut TerrainChunk`，
    // 再开一个读它的查询会触发 Bevy B0001 访问冲突（启动即 panic）。
    if layers.is_changed() {
        let z = TERRAIN_Z + layers.terrain.z_offset;
        let alpha = layers.terrain.opacity;
        for &entity in chunk_entities.by_coord.values() {
            let Ok((chunk, _)) = q_chunks.get(entity) else {
                continue;
            };
            let material = chunk.material.clone();
            commands.queue(move |world: &mut World| {
                if let Some(mut tf) = world.get_mut::<Transform>(entity) {
                    tf.translation.z = z;
                }
                if let Some(mut mat) = world
                    .resource_mut::<Assets<ColorMaterial>>()
                    .get_mut(&material)
                {
                    mat.color = Color::WHITE.with_alpha(alpha);
                }
            });
        }
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let (size_w, size_h) = (tex.size, tex.height);
    let google_mode = view.mode == crate::state::ViewMode::GoogleMap;
    let cell_size = grid.cell_size;
    let [cw, ch] = chunk_grid(size_w, size_h);
    let res = tex.res;

    // 视口 AABB 与当前 mip 级。相机旋转（地图朝向）决定视口斜矩形的朝向。
    let cam_rot = camera
        .single()
        .map(|t| t.rotation)
        .unwrap_or(Quat::IDENTITY);
    let (vp_min, vp_max) = viewport_world_aabb(
        view.camera_center,
        view.scale,
        Vec2::new(window.resolution.width(), window.resolution.height()),
        cam_rot,
    );
    let mip_now = select_mip(view.scale, cell_size / res as f32);
    let chunk_world_size = CHUNK_SIZE as f32 * cell_size;

    // 1) 视口外 chunk despawn，**并强制清掉超出当前块网格的旧图残块**。
    //    换图后新图更小（如 1024² → 512×288）时，旧图右下角的块坐标已不在
    //    新网格内；它们的 `chunk_cell_rect` 退化成空矩形、原点又可能仍在
    //    视口内，只靠可见性判断会把这些旧像素留在画面上（评审用户报告的
    //    「新建地图后旧图还留在主画面」）。
    let mut to_despawn: Vec<[u32; 2]> = Vec::new();
    for &coord in chunk_entities.by_coord.keys() {
        if chunk_out_of_grid(coord, [cw, ch]) {
            to_despawn.push(coord);
            continue;
        }
        let [gx0, gy0, _, _] = chunk_cell_rect(coord, size_w, size_h);
        let cmin = grid.raster_origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
            to_despawn.push(coord);
        }
    }
    for coord in to_despawn {
        if let Some(entity) = chunk_entities.by_coord.remove(&coord) {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
    }

    // 2) 收集视口内块；判断本帧是否需要源像素——有待 spawn 块，或有与 dirty 相交的块。
    let pending_dirty = tex.chunks_dirty;
    let mut visible: Vec<([u32; 2], [u32; 4])> = Vec::new();
    let mut needs_src = false;
    for cy in 0..ch {
        for cx in 0..cw {
            let coord = [cx, cy];
            let cells = chunk_cell_rect(coord, size_w, size_h);
            let cmin = grid.raster_origin + Vec2::new(cells[0] as f32, cells[1] as f32) * cell_size;
            if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
                continue;
            }
            if !chunk_entities.by_coord.contains_key(&coord)
                || pending_dirty.is_some_and(|d| rects_intersect(d, cells))
            {
                needs_src = true;
            }
            visible.push((coord, cells));
        }
    }

    // 源像素只在"确实要写"时克隆——整图 4096²×4B ≈ 67MB，每帧克隆不可接受。
    let src_data: Option<Vec<u8>> = if needs_src {
        match images.get(&tex.handle) {
            Some(source) if source.data.as_ref().is_some_and(|d| !d.is_empty()) => {
                source.data.clone()
            }
            _other => {
                // TODO(诊断 2026-10-14): "只看到等值线、没有底图填充" 回归。
                // 触发条件是 needs_src=true 但 source.data 为空/缺。常见原因：
                // ① tex.handle 资产被 replace_terrain_texture 用 *img=image 整
                //    体替换——bevy 0.15 的 Assets<Image> 是 Arc-backed，clone 仍
                //    指向旧值（这是已知的迁移期坑）。
                // ② handle 与 images 里其他槽位撞键。
                // 关闭诊断后移除本分支。
                if std::env::var("WV_CHUNK_DIAG").is_ok() {
                    let has_handle = images.get(&tex.handle).is_some();
                    let data_len = images
                        .get(&tex.handle)
                        .and_then(|i| i.data.as_ref())
                        .map(|d| d.len())
                        .unwrap_or(0);
                    eprintln!(
                        "[chunk-diag] needs_src=true but source unusable: handle_valid={has_handle} data_len={data_len} tex.size={} tex.res={} chunk_entities={} visible_chunks={}",
                        tex.size,
                        tex.res,
                        chunk_entities.by_coord.len(),
                        visible.len(),
                    );
                }
                // `&Image` 是 Copy，match 块结束自动释放——不需要手动 drop。
                None
            }
        }
    } else {
        None
    };
    // 需要像素但源未就绪：保留 chunks_dirty，下帧重试。
    if needs_src && src_data.is_none() {
        if std::env::var("WV_CHUNK_DIAG").is_ok() {
            eprintln!("[chunk-diag] bailing: needs_src but src_data=None, keeping chunks_dirty");
        }
        return;
    }
    let src = src_data.as_deref();

    // 3) spawn / patch / 切 mip。
    for (coord, cells) in visible {
        let [gx0, gy0, gx1, gy1] = cells;
        let cmin = grid.raster_origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        // 块的**世界矩形**按实际格数算：16:9 地图的右下尾块只有 32 格高，
        // 不能拿 256×256 的方形去铺（会拉伸并画出地图边界）。
        let chunk_rect = chunk_world_rect(cells, cell_size);
        let center = cmin + chunk_rect * 0.5;

        if let Some(&entity) = chunk_entities.by_coord.get(&coord) {
            // 3a-0) 尺寸失配检测：tex.size/res 随世界切换变化（如 `-m` 加载
            // 1024 格工程，res 8→4）后，旧块的 mip 资产 extent 仍是旧 res 的。
            // `patch_chunk_mips` 用 `Vec::clone_from` 写 mip（长度可变），会把
            // 新 res 的数据塞进旧 extent 的 Image——GPU 上传即 panic
            // （wgpu device.rs「range end index out of range」）。整块作废，
            // 移出 by_coord 落到下方重生路径，按新尺寸重建。
            let mip0_matches = q_chunks
                .get(entity)
                .ok()
                .and_then(|(c, _)| c.mip_handles.first())
                .and_then(|h| images.get(h))
                .map(|mip0| {
                    mip0.texture_descriptor.size.width == (gx1 - gx0) * res
                        && mip0.texture_descriptor.size.height == (gy1 - gy0) * res
                        && q_chunks
                            .get(entity)
                            .map(|(c, _)| c.linear_filter == google_mode)
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if !mip0_matches {
                chunk_entities.by_coord.remove(&coord);
                despawn_chunk(&mut commands, entity, &q_chunks);
            } else {
                let Ok((mut chunk, _)) = q_chunks.get_mut(entity) else {
                    continue;
                };
                // 3a) dirty 相交 → patch mip0 像素，并逐级重建 mip1/mip2。
                if let (Some(dirty), Some(src)) = (pending_dirty, src) {
                    if rects_intersect(dirty, cells) {
                        patch_chunk_mips(&mut images, &chunk, src, size_w, res, cells, dirty);
                        // Bevy 0.15：就地改 `Image` 数据不使既有 `ColorMaterial` 的
                        // bindgroup 失效——GPU 上仍采样旧纹理。触碰材质资产强制
                        // 重建 bindgroup，让本帧 patch 的像素真正上屏（模式切换 /
                        // 笔刷提交共用此路径）。
                        if materials.get_mut(&chunk.material).is_none() {
                            continue;
                        }
                    }
                }
                // 3b) mip 级随缩放切换——只换材质纹理句柄，几何与资产不动。
                if chunk.current_mip != mip_now {
                    if let Some(mut mat) = materials.get_mut(&chunk.material) {
                        mat.texture = chunk.mip_handles.get(mip_now as usize).cloned();
                    }
                    chunk.current_mip = mip_now;
                }
                continue;
            }
        }

        // spawn：拷贝本块像素 + 建 mip 链 + 建几何与材质。
        let Some(src) = src else { continue };
        let chunk_pixels = copy_chunk_pixels(src, size_w, res, gx0, gy0, gx1, gy1);
        let pw = (gx1 - gx0) * res;
        let ph = (gy1 - gy0) * res;
        let mip_handles: Vec<Handle<Image>> = build_mip_chain(chunk_pixels, pw, ph, google_mode)
            .into_iter()
            .map(|img| images.add(img))
            .collect();
        let mesh_handle = meshes.add(Rectangle::new(chunk_rect.x, chunk_rect.y));
        let material = materials.add(ColorMaterial {
            // 图层透明度（ADR-0013）：作者 YAML 的 `opacity` 落在材质 alpha 上。
            color: Color::WHITE.with_alpha(layers.terrain.opacity),
            texture: mip_handles.get(mip_now as usize).cloned(),
            // V 翻转：Mesh2d + Rectangle 的默认 UV 把 image data 第 0 行映射到世界 +
            // Y 顶点，与 `apply_brush` / `isoline_overlay` 的「cell row 0 = 世界底部」
            // 约定相反。翻转后 data 行 0 落在世界底部，与笔刷和等值线描边同源——
            // 修复「VectorFlat 填充与描边呈 90° 错位 / 笔刷落点与网格点对不上」
            // （M2 提交 5f8cb73 后浮现的回归：色块模式没有描边时不可见）。
            uv_transform: Affine2::from_translation(Vec2::new(0.0, 1.0))
                * Affine2::from_scale(Vec2::new(1.0, -1.0)),
            ..default()
        });
        let entity = commands
            .spawn((
                Mesh2d(mesh_handle),
                MeshMaterial2d(material.clone()),
                Transform::from_translation(center.extend(TERRAIN_Z + layers.terrain.z_offset)),
                TerrainChunk {
                    _coord: coord,
                    current_mip: mip_now,
                    mip_handles,
                    material,
                    linear_filter: google_mode,
                },
            ))
            .id();
        chunk_entities.by_coord.insert(coord, entity);
    }

    // 4) 脏矩形已消费（只在确有写入时触碰资源，避免无谓的 changed 标记）。
    if tex.chunks_dirty.is_some() {
        tex.chunks_dirty = None;
    }
}

// ══════ ADR-0053 :: sync_terrain_field（GPU 逐像素底图，Phase 1 接线）══════
//
// 管线本体（数据打包 / 材质 / WGSL 逐像素求值）在
// `wv-world-cell-map::terrain_view::field`，且已过截图验收；本段只做**主 App
// 接线**——ADR-0053 §7 的 Phase 1「只接线、不加特性」。
//
// 唯一的接线示范是 `wv-world-cell-map/examples/terrain_field_demo.rs`；
// 本段与它的差别只有三处，都是「主 App 的数据约定与 demo 不同」造成的：
//
// | 项 | demo | 主 App（本段） |
// |---|---|---|
// | 地形数据 | 现造的合成 / fbm 512² | `active_terrain` 的真实栅格 |
// | 格 → 世界 | 1 世界单位/格，quad 居中在原点 | `grid.cell_size` × `grid.raster_origin` |
// | UV 方向 | 默认 quad（行 0 = 世界 **上**） | **V 翻转**（行 0 = 世界 **下**，见 [`field_quad_mesh`]） |
//
// ## 已知边界（本阶段刻意不做，别顺手补）
// - **小地图仍消费 CPU 烘焙纹理**：`rebuild_texture_on_mode_change` 的合成
//   照常跑（异步、不挡帧）。低分辨率 minimap 的场路径是后续项。
// - **T35 海面未接 SeaRecipe 全套**：P1 用 `TopographicPalette` 四色（照搬现状）。
// - **T25/T26 折线（河 / 路）未接**：走 `build_material`（空折线纹理，解码后
//   距离饱和 → shader 不画线）。接 `build_material_with_lines` 属下一轮。
// - **图层 opacity 未接**：`TerrainFieldMaterial` 没有 alpha 通道，Phase 1 只跟
//   `LayerFrame.terrain.visible` 与 `z_offset`。
