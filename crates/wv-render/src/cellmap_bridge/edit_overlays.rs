//! 编辑选区与工具即时视觉：单格选区 / 笔触 / hover 准星 / 撤销白闪
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

// ══════ from src/cell_selection.rs :: sync_cell_region ══════
/// **统一选区高亮**（ADR-0057 D8）：面 = 金色半透填充（脉动）；形 = 外围边界
/// **白色虚线蚂蚁线**（两相位 mesh，≈0.3 s 计时整组互换）。
///
/// 承载三件事：
/// 1. **`CellSelection` 的图绑定**——每帧以当前图幂等 `bind_map`（切图清空 +
///    惰性定容收敛在这一个入口，ADR-0057 D1 / A7）；
/// 2. **填充**——选区指纹变才重建合并 mesh，材质只改 alpha（脉动）；
/// 3. **蚂蚁线**——重建键 = (选区指纹, 缩放档)；命中重建两套相位 mesh，
///    其余帧零重建，相位钟到点只切 `Visibility`。
///
/// gate = `recipe.enabled(CellSelection) ∧ !sel.is_empty()`；不满足时面与
/// 蚂蚁线一起 despawn（无残留）。
///
/// ADR-0057 退役：单格 outline（旧 `sync_cell_selection`）与外包矩形四边——
/// 单格 = 1 格选区，由蚂蚁线统一覆盖。
pub fn sync_cell_region(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    recipe: Res<crate::recipe::RenderRecipe>,
    mut sel: ResMut<CellSelection>,
    drill: Res<DrillState>,
    tex: Res<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    time: Res<Time>,
    mut region: ResMut<CellRegionEntities>,
    mut ants: ResMut<CellAntsEntities>,
) {
    // 1. 绑定：图变 / 图幅变 → 清空重定容；同图同尺寸 → 零成本返回。
    if let Some(map_id) = drill.current() {
        sel.bind_map(map_id, tex.size, tex.height);
    }
    let active = recipe.enabled(PassId::CellSelection) && !sel.is_empty();
    if !active {
        region.despawn_all(&mut commands);
        region.release_assets(&mut meshes);
        ants.despawn_all(&mut commands);
        ants.release_assets(&mut meshes);
        return;
    }
    // 脉动：0.22 + 0.12·sin(3.5t) → alpha ∈ [0.10, 0.34]，肉眼可辨的呼吸感。
    let alpha = 0.22 + 0.12 * (time.elapsed_secs() * 3.5).sin();
    let accent = crate::theme::colors::ACCENT_SELECT;
    let fingerprint = sel.fingerprint();

    // ── 面：填充材质只建一次，之后每帧只改 alpha ──
    if region.fill_material.is_none() {
        region.fill_material =
            Some(materials.add(ColorMaterial::from_color(accent.with_alpha(alpha))));
    } else if let Some(h) = region.fill_material.clone() {
        if let Some(mut m) = materials.get_mut(&h) {
            m.color = accent.with_alpha(alpha);
        }
    }
    let fill_mat = region.fill_material.clone().expect("材质已建");

    // ── 面：填充 mesh 只在格集合变化（或实体缺位）时重建 ──
    if fingerprint != region.last_fingerprint || region.fill_mesh.is_none() || region.fill.is_none()
    {
        region.release_assets(&mut meshes);
        let cells: Vec<(i32, i32)> = sel.iter_cells().collect();
        let Some((mesh, origin, _bbox)) = build_region_fill_mesh(&cells, &grid) else {
            region.despawn_all(&mut commands);
            return;
        };
        let mesh_h = meshes.add(mesh);
        region.fill_mesh = Some(mesh_h.clone());
        region.last_fingerprint = fingerprint;
        let tf = Transform::from_translation(origin.extend(0.899));
        match region.fill {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh_h), MeshMaterial2d(fill_mat)));
            }
            None => {
                region.fill = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh_h), MeshMaterial2d(fill_mat)))
                        .id(),
                );
            }
        }
    }

    // ── 形：蚂蚁线（重建键 = (选区指纹, 缩放档)）──
    let bucket = crate::region_outline::scale_bucket(view.scale);
    let key = (fingerprint, bucket);
    if ants.last_key != Some(key) || ants.mesh_a.is_none() || ants.mesh_b.is_none() {
        let s = crate::region_outline::scale_of_bucket(bucket);
        let edges = sel.boundary_edges();
        if let Some((ma, mb)) =
            build_ants_meshes(&edges, &grid, ANTS_PITCH_PX * s, ANTS_THICKNESS_PX * s)
        {
            ants.release_assets(&mut meshes);
            // 白材质两相位共用，只建一次。
            if ants.material.is_none() {
                ants.material = Some(materials.add(ColorMaterial::from_color(Color::WHITE)));
            }
            let mat = ants.material.clone().expect("材质已建");
            let (ha, hb) = (meshes.add(ma), meshes.add(mb));
            ants.mesh_a = Some(ha.clone());
            ants.mesh_b = Some(hb.clone());
            ants.last_key = Some(key);
            let show_a = ants.clock.visible_phase() == DashPhase::A;
            let (vis_a, vis_b) = if show_a {
                (Visibility::Visible, Visibility::Hidden)
            } else {
                (Visibility::Hidden, Visibility::Visible)
            };
            // 先解构出 disjoint 字段借用——同一 `ants` 的 phase_a / phase_b
            // 不能在同一条 for 初始化式里各取 `&mut`（E0499）。
            let CellAntsEntities {
                phase_a, phase_b, ..
            } = &mut *ants;
            for (slot, mesh_h, vis) in [(phase_a, ha, vis_a), (phase_b, hb, vis_b)] {
                match *slot {
                    Some(e) => {
                        commands.entity(e).insert((
                            Mesh2d(mesh_h),
                            MeshMaterial2d(mat.clone()),
                            vis,
                        ));
                    }
                    None => {
                        *slot = Some(
                            commands
                                .spawn((
                                    Transform::from_xyz(0.0, 0.0, ANTS_Z),
                                    Mesh2d(mesh_h),
                                    MeshMaterial2d(mat.clone()),
                                    vis,
                                ))
                                .id(),
                        );
                    }
                }
            }
        }
    }
    // 相位钟：到点整组翻转可见性（零重建）。
    if ants.clock.tick(time.delta_secs()) {
        let show_a = ants.clock.visible_phase() == DashPhase::A;
        if let Some(e) = ants.phase_a {
            commands.entity(e).insert(if show_a {
                Visibility::Visible
            } else {
                Visibility::Hidden
            });
        }
        if let Some(e) = ants.phase_b {
            commands.entity(e).insert(if show_a {
                Visibility::Hidden
            } else {
                Visibility::Visible
            });
        }
    }
}

// ══════ from crates/wv-world-cell-map/src/cell_selection.rs :: sync_cell_stroke ══════
/// 每帧同步 **待落笔触色块**（[ADR-0052] D3 / §9.5 四态视觉分工）。
///
/// gate = 笔触活跃 ∧ 笔触格非空；不满足时全 despawn。
///
/// 视觉：红橙 `STROKE_PENDING` 半透填充（alpha 0.26，**不脉动**）——
/// 表示「已采样但尚未写入世界」的格。
///
/// Z 低于选中（`STROKE_Z = 0.85 < SELECTION_Z = 0.9`）：笔触是**未提交**
/// 暂态，选中才是已提交态（§9.5 绘制序 region → stroke → selected → hover）。
///
/// **mesh 只在格集合变化时重建**——笔触可能几百格，每帧重建会吃帧
/// （与 `sync_cell_region` 同款策略）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn sync_cell_stroke(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    session: Res<wv_world_cell_map::cell_paint::CellPaintSession>,
    grid: Res<GridConfig>,
    mut entities: ResMut<CellStrokeEntities>,
) {
    let cells = &session.stroke.cells;
    if !session.stroke.active || cells.is_empty() {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    }
    let fingerprint = cells_fingerprint(cells);
    if fingerprint != entities.last_fingerprint || entities.fill_mesh.is_none() {
        entities.release_assets(&mut meshes);
        let Some((mesh, origin, bbox)) = build_stroke_fill_mesh(cells, &grid) else {
            entities.despawn_all(&mut commands);
            return;
        };
        entities.fill_mesh = Some(meshes.add(mesh));
        entities.last_fingerprint = fingerprint;
        // 重建 mesh → 实体也要重建（mesh 换了）。
        entities.despawn_all(&mut commands);
        if entities.fill_material.is_none() {
            entities.fill_material = Some(materials.add(ColorMaterial::from_color(
                crate::theme::colors::STROKE_PENDING.with_alpha(STROKE_FILL_ALPHA),
            )));
        }
        let mesh = entities.fill_mesh.clone().expect("刚建");
        let mat = entities.fill_material.clone().expect("刚建");
        entities.fill = Some(
            commands
                .spawn((
                    Transform::from_translation(origin.extend(STROKE_Z)),
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                    // bbox 不进 Transform（mesh 顶点已是绝对世界坐标的相对偏移，
                    // 与 `sync_cell_region` 同款：mesh 原点 + 顶点偏移 = 世界位置）。
                ))
                .id(),
        );
        let _ = bbox;
        return;
    }
    // 格集合未变 → 材质与实体保持不变（避免每帧重建）。
    let Some(e) = entities.fill else {
        return;
    };
    let mesh = entities.fill_mesh.clone().expect("已建");
    let mat = entities.fill_material.clone().expect("已建");
    commands
        .entity(e)
        .insert((Mesh2d(mesh), MeshMaterial2d(mat)));
}

// ══════ from crates/wv-world-cell-map/src/cell_selection.rs :: sync_cell_hover ══════
/// 每帧同步 **hover 准星**（[ADR-0052] D2 / §9.5 四态视觉分工）。
///
/// gate = `(edit_cell.tool_active ∨ point_select.tool_active) ∧ edit_cell.hover.is_some()`
/// （ADR-0057：点选工具与单格编辑共用 hover 准星）；
/// 不满足时全 despawn（无残留）。
///
/// **双色描边**（D2）：每条边两层——外层深墨 `LINE_ON_DARK`（2px）、
/// 内层纯白（1px）。深墨在下、白线在上，构成「白内线 + 深墨外晕」的
/// 地图准星观感：在水面（浅蓝）与陆地（绿/褐）两种底色上都能读出来
/// （[ADR-0052] §5 M1 验收 ③）。
///
/// **不脉动**（D2）：hover 是瞬时态，脉动会与 selected 的金色脉动混淆。
/// 脉动是 selected 独占的信号。
///
/// **Z 高于选区**（`HOVER_Z = 0.95 > SELECTION_Z = 0.9`）：
/// hover 是「此刻光标在哪」的瞬时反馈，必须画在最上层
/// （[ADR-0052] §9.5 绘制序 region → stroke → selected → hover）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn sync_cell_hover(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    edit_cell: Res<EditCellState>,
    point_select: Res<PointSelectState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut entities: ResMut<CellHoverEntities>,
) {
    let Some((gx, gy)) = edit_cell
        .hover
        .filter(|_| edit_cell.tool_active || point_select.tool_active)
    else {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    };
    let (center, size) = cell_world_rect(&grid, gx, gy);
    // 世界厚度 = 屏幕恒定像素 × view.scale（与选中 outline 同策略 A）。
    let outer_t = HOVER_OUTER_PX * view.scale;
    let inner_t = HOVER_INNER_PX * view.scale;
    // 准星长度 = 格的 1/3（居中十字），不与边缘描边打架。
    let cross_len = size.x * 0.34;

    // ── mesh：只在尺寸变化时重建（其余帧复用句柄）──
    if entities.last_size != size || entities.outer_h_mesh.is_none() {
        entities.release_assets(&mut meshes);
        // 外晕比内线更宽——墨晕在格沿两侧铺开，白线正好落在格沿上。
        entities.outer_h_mesh = Some(meshes.add(Rectangle::new(size.x, outer_t)));
        entities.outer_v_mesh = Some(meshes.add(Rectangle::new(outer_t, size.y)));
        entities.inner_h_mesh = Some(meshes.add(Rectangle::new(size.x, inner_t)));
        entities.inner_v_mesh = Some(meshes.add(Rectangle::new(inner_t, size.y)));
        // 准星 2 条（横 / 竖）+ 白芯 2 条（比芯短一档，露出墨头）。
        entities.cross_h_mesh = Some(meshes.add(Rectangle::new(cross_len, outer_t)));
        entities.cross_v_mesh = Some(meshes.add(Rectangle::new(outer_t, cross_len)));
        let core_len = cross_len * 0.5;
        entities.core_h_mesh = Some(meshes.add(Rectangle::new(core_len, inner_t)));
        entities.core_v_mesh = Some(meshes.add(Rectangle::new(inner_t, core_len)));
    }
    entities.last_size = size;

    // ── 材质：只建一次 ──
    if entities.outer_material.is_none() {
        entities.outer_material = Some(materials.add(ColorMaterial::from_color(
            crate::theme::colors::LINE_ON_DARK,
        )));
    }
    if entities.inner_material.is_none() {
        entities.inner_material =
            Some(materials.add(ColorMaterial::from_color(bevy::color::Color::WHITE)));
    }
    let outer_mat = entities.outer_material.clone().expect("材质已建");
    let inner_mat = entities.inner_material.clone().expect("材质已建");

    let half = size * 0.5;
    let ot2 = outer_t * 0.5;
    // `inner_t` 不需要偏移量：内线直接压在格沿上（外晕才向外让半个厚度）。

    // ── 外晕 4 条边：下 (0) / 上 (1) / 左 (2) / 右 (3) ──
    // 外晕中心比格边界再外移半个墨晕厚度（`ot2`），墨线铺在格沿两侧。
    let outer_offsets: [Vec2; 4] = [
        Vec2::new(0.0, -half.y - ot2),
        Vec2::new(0.0, half.y + ot2),
        Vec2::new(-half.x - ot2, 0.0),
        Vec2::new(half.x + ot2, 0.0),
    ];
    let outer_meshes = [
        entities.outer_h_mesh.clone(),
        entities.outer_h_mesh.clone(),
        entities.outer_v_mesh.clone(),
        entities.outer_v_mesh.clone(),
    ];
    for (i, offset) in outer_offsets.into_iter().enumerate() {
        let Some(mesh) = outer_meshes[i].clone() else {
            continue;
        };
        let tf = Transform::from_translation((center + offset).extend(HOVER_Z));
        match entities.outer_edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())));
            }
            None => {
                entities.outer_edges[i] = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())))
                        .id(),
                );
            }
        }
    }

    // ── 内线 4 条边：白线压格沿（比外晕更靠内半个墨晕厚度）──
    let inner_offsets: [Vec2; 4] = [
        Vec2::new(0.0, -half.y),
        Vec2::new(0.0, half.y),
        Vec2::new(-half.x, 0.0),
        Vec2::new(half.x, 0.0),
    ];
    let inner_meshes = [
        entities.inner_h_mesh.clone(),
        entities.inner_h_mesh.clone(),
        entities.inner_v_mesh.clone(),
        entities.inner_v_mesh.clone(),
    ];
    for (i, offset) in inner_offsets.into_iter().enumerate() {
        let Some(mesh) = inner_meshes[i].clone() else {
            continue;
        };
        let tf = Transform::from_translation((center + offset).extend(HOVER_Z + 0.001));
        match entities.inner_edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())));
            }
            None => {
                entities.inner_edges[i] = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())))
                        .id(),
                );
            }
        }
    }

    // ── 准星：格中心十字（外晕层 + 白芯层）──
    // 外晕横 / 竖：格中心。
    //
    // 先把 mesh 句柄克隆出来再可变借用 slot——数组元素与整体克隆写在
    // 同一条 `for` 的初始化式里会同时可变 + 不可变借用 `entities`
    // （编译期 E0499/E0502）。逐个处理（而非一次遍历两个 slot）是为
    // 避开「同时可变借用 `entities` 两次」。
    let cross_outer_meshes = [entities.cross_h_mesh.clone(), entities.cross_v_mesh.clone()];
    for (idx, mesh) in cross_outer_meshes.into_iter().enumerate() {
        let Some(mesh) = mesh else { continue };
        let tf = Transform::from_translation(center.extend(HOVER_Z));
        let slot = &mut entities.cross_outer[idx];
        match slot {
            Some(e) => {
                commands
                    .entity(*e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())));
            }
            None => {
                *slot = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())))
                        .id(),
                );
            }
        }
    }
    // 白芯（比墨晕短一档，露不出头）。
    let cross_inner_meshes = [entities.core_h_mesh.clone(), entities.core_v_mesh.clone()];
    for (idx, mesh) in cross_inner_meshes.into_iter().enumerate() {
        let Some(mesh) = mesh else { continue };
        let tf = Transform::from_translation(center.extend(HOVER_Z + 0.002));
        let slot = &mut entities.cross_inner[idx];
        match slot {
            Some(e) => {
                commands
                    .entity(*e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())));
            }
            None => {
                *slot = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())))
                        .id(),
                );
            }
        }
    }
}

// ══════ from crates/wv-world-cell-map/src/cell_flash.rs :: sync_cell_flash ══════
/// 每帧同步 **撤销白闪 / 拒收红闪**（[ADR-0052] D8-A / D3-E）。
///
/// gate = `CellFlashState.is_active(now)`；到期即全 despawn（无残留）。
///
/// ## 颜色分工
///
/// - `Undo` → `UNDO_FLASH`（纯白，`0xFFFFFF`）；
/// - `Reject` → `UI_UNREACHABLE`（红，`0xB5453A`）——与
///   `UI_UNREACHABLE` 同源：语义都是「这一处不可用 / 没生效」。
///
/// ## Z 最顶层
///
/// `FLASH_Z = 1.0` **高于 hover（`HOVER_Z = 0.95`）**——闪是「刚刚发生」
/// 的强信号，必须压过所有常驻视觉（[ADR-0052] §9.9「白闪 z 最顶层
/// 250ms 短促」）。
///
/// ## mesh 只在格集合变化时重建
///
/// 闪期间格集合不变（撤销已定局），故 mesh 复用；**alpha 每帧改**
/// （材质句柄内的颜色随时间衰减）。
pub fn sync_cell_flash(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    flash: Res<CellFlashState>,
    grid: Res<GridConfig>,
    time: Res<Time>,
    mut entities: ResMut<CellFlashEntities>,
) {
    let now = time.elapsed_secs();
    if !flash.is_active(now) {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    }
    let fingerprint = cells_fingerprint(&flash.cells);
    if fingerprint != entities.last_fingerprint || entities.fill_mesh.is_none() {
        entities.release_assets(&mut meshes);
        let Some((mesh, origin)) = build_flash_mesh(&flash.cells, &grid) else {
            entities.despawn_all(&mut commands);
            return;
        };
        entities.fill_mesh = Some(meshes.add(mesh));
        entities.last_fingerprint = fingerprint;
        entities.despawn_all(&mut commands);
        // 材质：颜色固定（按 kind），alpha 每帧改。
        let base = match flash.kind {
            CellFlashKind::Undo => crate::theme::colors::UNDO_FLASH,
            CellFlashKind::Reject => crate::theme::colors::UI_UNREACHABLE,
        };
        if entities.fill_material.is_none() {
            entities.fill_material =
                Some(materials.add(ColorMaterial::from_color(base.with_alpha(flash.alpha(now)))));
        }
        let mesh = entities.fill_mesh.clone().expect("刚建");
        let mat = entities.fill_material.clone().expect("刚建");
        entities.fill = Some(
            commands
                .spawn((
                    Transform::from_translation(origin.extend(FLASH_Z)),
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                ))
                .id(),
        );
        return;
    }
    // 格集合未变 → 只改 alpha（材质句柄内的颜色）。
    let Some(e) = entities.fill else {
        return;
    };
    let Some(mat) = entities.fill_material.clone() else {
        return;
    };
    let base = match flash.kind {
        CellFlashKind::Undo => crate::theme::colors::UNDO_FLASH,
        CellFlashKind::Reject => crate::theme::colors::UI_UNREACHABLE,
    };
    if let Some(mut m) = materials.get_mut(&mat) {
        m.color = base.with_alpha(flash.alpha(now));
    }
    commands.entity(e).insert(MeshMaterial2d(mat));
}

// ══════ from crates/wv-world-cell-map/src/region_overlay.rs :: region_layer_visible ══════
/// 区域层在本帧是否可见（2026-10-22 内容矩阵 §2.6 的唯一判定入口）。
///
/// - **CellMap（Topographic）**：只看 `nature_overlay` 开关，且**仅 Nature 类**
///   放行（Area 区属内容，CellMap 永远藏）；recipe 格（false）被旁路。
/// - **其他模式**：走 recipe（RegionLayers 行）∧ 图层。
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
