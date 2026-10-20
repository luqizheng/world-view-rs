import io

p = 'crates/wv-render/src/cell_selection.rs'
s = io.open(p, encoding='utf-8').read()


def replace_fn(src, header, new_body):
    i = src.index(header)
    j = src.index('{', i)
    depth = 0
    k = j
    while k < len(src):
        if src[k] == '{':
            depth += 1
        elif src[k] == '}':
            depth -= 1
            if depth == 0:
                break
        k += 1
    return src[:i] + new_body + src[k + 1:]


# CellSelectionEntities: 加句柄缓存
old = '''/// 选中格的 mesh 实体集合（复用：稳态 5 实体 = 填充 + 4 边）。
#[derive(Resource, Default)]
pub struct CellSelectionEntities {
    /// 半透填充块。
    fill: Option<Entity>,
    /// 四条边（下 / 上 / 左 / 右）。
    edges: [Option<Entity>; 4],
}

impl CellSelectionEntities {
    /// 全部 despawn + 清空（gate 不满足时调用）。
    fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
        for slot in &mut self.edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }
}'''

new = '''/// 选中格的 mesh 实体集合（复用：稳态 5 实体 = 填充 + 4 边）。
///
/// **资产句柄一并缓存**（2026-10-20 实机卡顿修）：初版每帧
/// `meshes.add()` + `materials.add()` 新建资产——旧句柄永不回收，
/// `Assets` 无限堆积 + 每帧 GPU 重传。现在 mesh/material **只建一次**
/// （尺寸变化才重建 mesh），之后只改 Transform 与材质 alpha。
#[derive(Resource, Default)]
pub struct CellSelectionEntities {
    /// 半透填充块。
    fill: Option<Entity>,
    /// 四条边（下 / 上 / 左 / 右）。
    edges: [Option<Entity>; 4],
    /// 填充 mesh 句柄（复用）。
    fill_mesh: Option<Handle<Mesh>>,
    /// 填充材质句柄（复用）。
    fill_material: Option<Handle<ColorMaterial>>,
    /// 四条边的 mesh 句柄（复用）。
    edge_meshes: [Option<Handle<Mesh>>; 4],
    /// 四条边的材质句柄（复用）。
    edge_materials: [Option<Handle<ColorMaterial>>; 4],
    /// 上次构建时的格尺寸（世界单位）——变了才重建 mesh。
    last_size: Vec2,
}

impl CellSelectionEntities {
    /// 全部 despawn + 清空（gate 不满足时调用）。
    fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
        for slot in &mut self.edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }

    /// 释放已缓存的 mesh 资产（`Assets` 不随实体 despawn 回收）。
    fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        if let Some(h) = self.fill_mesh.take() {
            meshes.remove(&h);
        }
        for slot in &mut self.edge_meshes {
            if let Some(h) = slot.take() {
                meshes.remove(&h);
            }
        }
        self.last_size = Vec2::ZERO;
    }
}'''

assert old in s, 'selection entities struct not found'
s = s.replace(old, new, 1)

new_fn = '''pub fn sync_cell_selection(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    recipe: Res<crate::recipe::RenderRecipe>,
    edit_cell: Res<EditCellState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut entities: ResMut<CellSelectionEntities>,
) {
    let Some((gx, gy)) = edit_cell
        .selected
        .filter(|_| recipe.enabled(PassId::CellSelection))
    else {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    };
    let (center, size) = cell_world_rect(&grid, gx, gy);
    let accent = colors::ACCENT_SELECT;
    // 世界厚度 = 屏幕恒定 2px（策略 A，与笔刷光标一致）。
    let thickness = THICKNESS_PX * view.scale;

    // ── mesh：只在尺寸变化时重建（其余帧复用句柄）──
    if entities.last_size != size {
        entities.release_assets(&mut meshes);
        entities.fill_mesh = Some(meshes.add(Rectangle::new(size.x, size.y)));
        let half = size * 0.5;
        let t2 = thickness * 0.5;
        let edge_defs: [(Vec2, Vec2); 4] = [
            (Vec2::new(0.0, -half.y + t2), Vec2::new(size.x, thickness)),
            (Vec2::new(0.0, half.y - t2), Vec2::new(size.x, thickness)),
            (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, size.y)),
            (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, size.y)),
        ];
        for (i, (_, edge_size)) in edge_defs.into_iter().enumerate() {
            entities.edge_meshes[i] = Some(meshes.add(Rectangle::new(edge_size.x, edge_size.y)));
        }
        entities.last_size = size;
    }

    let half = size * 0.5;
    let t2 = thickness * 0.5;
    let edge_defs: [(Vec2, Vec2); 4] = [
        (Vec2::new(0.0, -half.y + t2), Vec2::new(size.x, thickness)),
        (Vec2::new(0.0, half.y - t2), Vec2::new(size.x, thickness)),
        (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, size.y)),
        (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, size.y)),
    ];

    // ── 材质：只建一次 ──
    if entities.fill_material.is_none() {
        entities.fill_material =
            Some(materials.add(ColorMaterial::from_color(accent.with_alpha(FILL_ALPHA))));
    }
    for slot in &mut entities.edge_materials {
        if slot.is_none() {
            *slot = Some(materials.add(ColorMaterial::from_color(accent)));
        }
    }

    // ── 实体：建一次，之后只挂句柄 + 更新 Transform ──
    let fill_mesh = entities.fill_mesh.clone().expect("尺寸已定,mesh 已建");
    let fill_mat = entities.fill_material.clone().expect("材质已建");
    match entities.fill {
        Some(e) => {
            commands.entity(e).insert((
                Transform::from_translation(center.extend(SELECTION_Z)),
                Mesh2d(fill_mesh),
                MeshMaterial2d(fill_mat),
            ));
        }
        None => {
            entities.fill = Some(
                commands
                    .spawn((
                        Transform::from_translation(center.extend(SELECTION_Z)),
                        Mesh2d(fill_mesh),
                        MeshMaterial2d(fill_mat),
                    ))
                    .id(),
            );
        }
    }

    // ── 四条边：下 (0) / 上 (1) / 左 (2) / 右 (3) ──
    for (i, (offset, _)) in edge_defs.into_iter().enumerate() {
        let Some(mesh) = entities.edge_meshes[i].clone() else {
            continue;
        };
        let Some(mat) = entities.edge_materials[i].clone() else {
            continue;
        };
        let tf = Transform::from_translation((center + offset).extend(SELECTION_Z + 0.001));
        match entities.edges[i] {
            Some(e) => {
                commands.entity(e).insert((tf, Mesh2d(mesh), MeshMaterial2d(mat)));
            }
            None => {
                entities.edges[i] =
                    Some(commands.spawn((tf, Mesh2d(mesh), MeshMaterial2d(mat))).id());
            }
        }
    }
}'''

s = replace_fn(s, 'pub fn sync_cell_selection(', new_fn)
io.open(p, 'w', encoding='utf-8').write(s)
print('sync_cell_selection rewritten')
