//! GPU 地形场子系统（ADR-0053）：FieldKey / TerrainFieldRender / ping-pong 同步
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

/// 场渲染的缓存键——**绝不许含任何连续量**。
///
/// ## 为什么键里不能有 scale / `px_per_cell`
///
/// 本项目已经在同一口坑里栽过**两次**：网格的 `GridKey.scale_bits`、锚点的连续
/// `spacing`。smooth zoom 每帧 ×1.02，键每帧都变 = 每帧重建整层（实测 2050 条
/// 网格链 + 全量网格上传）。
///
/// 缩放对场只影响 uniform（`shape.x` = `px_per_cell`、`pad0.x` = 域扭曲幅度），
/// **一次 ping-pong 都不该发生**。故键里只有「离散的结构量 + 内容指纹」。
#[derive(Clone, PartialEq, Eq)]
pub struct FieldKey {
    /// 栅格来源图（`active_terrain` 给出的图，沿 `parent` 向上继承）。
    pub map: Option<wv_core::Id>,
    /// 地形内容指纹（[`terrain_data_hash`]，与等值线 / 装饰缓存同源）。
    /// 只有**编辑提交**（或换图）才会变。
    pub terrain_hash: u64,
    /// 取色分支：`Topographic → BiomeRaw`（ADR-0051 §5），其余 → `Themed`。
    pub branch: ViewBranch,
    /// 主题色板 id。换主题只换 uniform，**不必重打数据纹理**。
    pub palette_id: &'static str,
}

/// 场渲染的资产与缓存键（单 quad，最多一份材质）。
///
/// 资产句柄集中在此，是为了「关掉 → 全部 `Assets::remove`」能一处收口
/// ——开关连续 20 轮不许有资产累积（验收口径 2）。
#[derive(Resource, Default)]
pub struct TerrainFieldRender {
    /// 场 quad 实体（`None` = 未建 / 已回收）。
    pub quad: Option<Entity>,
    /// quad 的 mesh 句柄（单位方片，跨缩放恒定，建一次终身复用）。
    pub mesh: Option<Handle<Mesh>>,
    /// 当前材质句柄（重建时整份替换，旧的走 `remove`）。
    pub material: Option<Handle<TerrainFieldMaterial>>,
    /// 上次构建用的键（`None` = 尚未构建）。
    pub key: Option<FieldKey>,
    /// 上次写进 uniform 的 `px_per_cell`——用来判断「要不要重传 uniform」。
    /// **只做比较、不进 [`FieldKey`]**。
    last_ppc: Option<f32>,
    /// 上次写进 uniform 的「米/格」（ADR-0077 晕渲米制槽 `light_dir.w`）。
    /// 同 `last_ppc`：只做比较、不进键——比例尺（换图）或格距变化只改
    /// uniform，不必重打 12 MB 数据纹理。
    last_mpc: Option<f32>,
    /// 上次写进 uniform 的分层设色强度（ADR-0055）。同 `last_ppc`：只做比较，
    /// **不进键**——拖滑条只改 uniform，不重打 12 MB 数据纹理。
    last_hypsometric: Option<f32>,
    /// 上次算出的分层设色内边界（归一化，7 条；ADR-0055 / ADR-0056）。
    ///
    /// 绝大数图是**绝对阶梯常量**，但窄域图（陆地跨度 < 300 m）会兜底为自己
    /// 区间的等分，故与 `field.pack` 同寿命：重打时刷新，补丁 / 换缩放时原样复用。
    cuts: Option<[f32; 7]>,
    /// 上一份**打包结果**（三张数据纹理的字节缓冲）——增量补丁的唯一数据源。
    ///
    /// ## 为什么渲染层要留这 12 MB
    ///
    /// 留的是「上次上传给 GPU 的那份字节」，用途只有一个：给一次局部编辑
    /// （brush / 单格 / 群系分界线）**补几百个 texel**，而不是把整张图按
    /// `FieldPack::from_terrain` 重打一遍（1024² 实测 48.4 ms，其中 ~40 ms 是
    /// 海岸 EDT）。没有它，增量补丁无从比对「补哪些格」。
    ///
    /// 它**不是**世界状态：权威仍是磁盘 YAML，`wv-app` 的 `World` 是工作副本，
    /// 这里只是 GPU 上那张纹理的 CPU 影子副本（关掉场渲染即随
    /// [`teardown_field`] 一起释放）。
    ///
    /// 换图 / 幅面变化 / 编码变化时**必须整份替换**（见 `sync_terrain_field`
    /// 的「可增量」判据）——拿上一张图的字节去补这一张，是错位的静默画错。
    pack: Option<wv_world_cell_map::terrain_view::field::FieldPack>,
}

/// 场渲染的计数器（QA 断言口径：跨机器同值、只增不减）。
///
/// 三个字段对应验收 1–3：
/// - `field_repacks`：数据纹理重打次数。稳态 60 帧必须 **Δ == 0**；笔刷提交
///   一次必须**恰好 +1**（指纹 + 编辑信号共同判定，纯换色不重打）。
/// - `field_quads`：场 quad 实体数（开 1 / 关 0）。
/// - `field_render_on_chunk_entities`：**帧末**仍活着的 chunk 实体数
///   （场开着时必须是 0——两条路径不许同时上屏）。本系统排在
///   `sync_terrain_chunks` 之后，故读到的是本帧 spawn / despawn 之后的真值。
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct FieldRenderStats {
    /// 数据纹理**全量重打**次数（累计）。
    pub field_repacks: u64,
    /// 数据纹理**增量补丁**次数（累计）。
    ///
    /// 与 `field_repacks` 是**互斥**的两条路径：一次编辑要么重打要么补丁，
    /// 不会同时 +1（补丁失败会退回重打，那时只有 `field_repacks` 动）。
    /// 验收口径：一笔 brush 必须 **`field_patches` +1 且 `field_repacks` 不变**；
    /// 换图 / 载档 / `mark_all_dirty` 必须 `field_repacks` +1。
    pub field_patches: u64,
    /// 场 quad 实体数（0 或 1）。
    pub field_quads: usize,
    /// 场开着时残留的 chunk 实体数（应为 0）。
    pub field_render_on_chunk_entities: usize,
    /// 最近一次重打的**墙钟毫秒**（`Instant` 实测，含打包 + 纹理建资产）。
    pub last_repack_ms: f32,
    /// 最近一次补丁的**墙钟毫秒**（含窗口 EDT + 三张纹理的行段拷贝）。
    pub last_patch_ms: f32,
    /// 最近一次重打上传的字节数（三张数据纹理 + 折线通道）。
    pub last_repack_bytes: usize,
}

/// 场 quad 的 mesh：**单位方片 + 显式 V 翻转的 UV**。
///
/// ## 为什么不能照抄 demo 的默认 UV（本项目的高危点）
///
/// 坑不在「谁对谁错」，在**两套数据约定不同**：
///
/// | | 数据行 0 在哪 | 依据 |
/// |---|---|---|
/// | demo | 世界 **+y**（北 / 上） | demo 自己按 `g/2 − cy` 换算格→世界 |
/// | 主 App | 世界 **y=0**（南 / 下） | ADR-0034 D2「DEM 行 0 = 最南行」；`cell_pick` / `TerrainGrid::sample_world` 的 `gy = (y − origin)/cell_size` |
///
/// 而 shader 的约定是 `pos = in.uv × grid`，`pos.y` **就是数据行号**（见
/// `terrain_field.wgsl` 顶部「实测探针」表：`world.y ↑ ⇒ in.uv.y ↓`）。
/// 于是：
///
/// - demo 的做法（默认 quad）→ 行 0 落在世界**上**，对主 App 是**整幅上下镜像**；
/// - chunk 路径（回退路径）用 `ColorMaterial.uv_transform` 做 V 翻转，把行 0
///   落到世界**下**——`sync_terrain_chunks` 里那段「V 翻转」注释就是这件事。
///
/// 自定义 `Material2d` **没有** `uv_transform` 这个 uniform（那是 `ColorMaterial`
/// 的字段），故翻转只能落在**顶点属性**上。`in.uv.y` 随世界 y 增大而增大 ⇒ 行 0
/// 落在世界下，与 chunk 路径、笔刷落点、等值线描边同源。
///
/// 180° 旋转（而不是镜像）是这条路线历史上真实发生过的缺陷（SPEC 追记九：
/// `uv0 = (1−u, 1−v)` 造成整幅旋转 180°，因程序化地形近似对称，P1–P4 全部
/// 截图验收都漏过）——故这里把 UV 写成**显式顶点属性**，不靠「默认值 + 猜」。
fn field_quad_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    // 顶点顺序与 Bevy `Rectangle` 一致（左下 → 右下 → 右上 → 左上，双三角）：
    // 世界 y 向上为正，故 `y = −0.5` 是世界**下**边。
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    );
    // uv.y = 0 在世界**下**边 ⇒ 数据行 0 落在世界下（见上方长注释）。
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    );
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// 回收场 quad 的 mesh 资产（调用前实体必须已 despawn）。
fn reclaim_field_mesh(commands: &mut Commands, mesh: Handle<Mesh>) {
    commands.queue(move |world: &mut World| {
        if let Some(mut assets) = world.get_resource_mut::<Assets<Mesh>>() {
            assets.remove(&mesh);
        }
    });
}

/// 回收场材质**及其四张数据纹理**。
///
/// 材质句柄只是入口：真正的显存是 `height/coast/attr/line_dash` 四张图
/// （1024² 图 = 8 MB）。只 remove 材质会让纹理永远留在 `Assets<Image>` 里
/// ——开关 20 轮 = 160 MB。故先把句柄抄出来，再在同一个命令里一并 remove。
fn reclaim_field_material(
    commands: &mut Commands,
    material: Handle<TerrainFieldMaterial>,
    materials: &Assets<TerrainFieldMaterial>,
) {
    let mut textures: [Option<Handle<Image>>; 6] = [const { None }; 6];
    if let Some(mat) = materials.get(&material) {
        textures = [
            Some(mat.height.clone()),
            Some(mat.coast.clone()),
            Some(mat.attr.clone()),
            Some(mat.line_dash.clone()),
            // ADR-0072：biome 距离场（Linear）+ 同字节 Nearest 视图。
            // 漏回收会被「关场后 Image 资产账目 == 纯 chunk 基线」的验收
            // 测试当场抓住（15 ≠ 13）。
            Some(mat.biome.clone()),
            Some(mat.biome_ids.clone()),
        ];
    }
    commands.queue(move |world: &mut World| {
        if let Some(mut assets) = world.get_resource_mut::<Assets<TerrainFieldMaterial>>() {
            assets.remove(&material);
        }
        if let Some(mut images) = world.get_resource_mut::<Assets<Image>>() {
            for texture in textures.into_iter().flatten() {
                images.remove(&texture);
            }
        }
    });
}

/// 场路径退场：quad despawn + mesh / 材质 / 数据纹理全部回收 + 清键。
///
/// 三条退场理由共用这一段（用户关开关 / 图层被隐藏 / 世界还没有任何栅格地形），
/// 保证「退场」在任何路径下都是**同一件事**（否则资产会从某一条路漏出去）。
fn teardown_field(
    commands: &mut Commands,
    field: &mut TerrainFieldRender,
    materials: &Assets<TerrainFieldMaterial>,
    stats: &mut FieldRenderStats,
) {
    if let Some(entity) = field.quad.take() {
        commands.entity(entity).despawn();
    }
    if let Some(mesh) = field.mesh.take() {
        reclaim_field_mesh(commands, mesh);
    }
    if let Some(material) = field.material.take() {
        reclaim_field_material(commands, material, materials);
    }
    field.key = None;
    field.last_ppc = None;
    field.last_mpc = None;
    field.last_hypsometric = None;
    field.cuts = None;
    // 12 MB 的 CPU 影子副本随场一起释放：关掉场还攥着上一张图的字节，
    // 既白占内存，也会让「下次开场的第一次补丁」用上过期数据。
    field.pack = None;
    stats.field_quads = 0;
}

/// 桥接侧「格距 + 比例尺 → 米/格」的纯缝（ADR-0077 D2）。
///
/// 从 [`sync_terrain_field`] 的内联填充抽出——本体委托 [`meters_per_cell_of`]：
/// 比例尺（`unit_per_km`，地图声明的 km/wu）精确换算，未声明走「1 wu = 1 m」
/// 回落（README §12-53 待决）。抽这一层不引入新语义，为的是让「桥接用的是
/// 哪条入口、喂的是**来源图**的比例尺」这件事不经 Bevy App 就能被测试锁住
/// （终审 M-1 修法 A / plan T5 声明的 bridge mpc 测试落点）。
fn bridge_meters_per_cell(cell_size: f32, unit_per_km: Option<f32>) -> f32 {
    meters_per_cell_of(cell_size, unit_per_km)
}

/// CPU 烘焙路径的米/格接线（ADR-0077 T4 遗留项落地）：给 wv-app 的
/// `image_from_terrain_with_scale` / `patch_terrain_image_with_scale` 用。
/// 来源图（`tex.last_map`，否则 `active_terrain` 沿 parent 链解析出的那张）的
/// `unit_per_km` 精确换算，未声明走「1 wu = 1 m」回落（README §12-53）——
/// 与 [`sync_terrain_field`] 的 GPU 填充同一条入口 [`meters_per_cell_of`]，
/// 两管线同一假设、同一公式，禁止第二份换算。
pub fn bake_meters_per_cell(
    world: &wv_core::World,
    source_map: Option<&wv_core::id::Id>,
    cell_size: f32,
) -> f32 {
    let unit_per_km = source_map
        .and_then(|id| world.maps.iter().find(|n| &n.id == id))
        .and_then(|n| n.unit_per_km);
    bridge_meters_per_cell(cell_size, unit_per_km)
}

/// 主 App 的场渲染同步（ADR-0053 D1，Phase 1 接线 2026-10-24）。
///
/// 取色分支的唯一选择点（ADR-0079 T5）。
///
/// - Topographic（CellMap）恒走 [`ViewBranch::BiomeRaw`]——方格数据视图，
///   自然图开关不介入（ADR-0051 §5）；
/// - 其余模式：地形层可见 → [`ViewBranch::Themed`]（主题色板）；
///   **不可见 → [`ViewBranch::Flat`]（政治图集页素底）**——不退场，
///   地图全空则势力填色无处可压。
pub fn field_branch_of(mode: crate::state::ViewMode, terrain_visible: bool) -> ViewBranch {
    if mode == crate::state::ViewMode::Topographic {
        ViewBranch::BiomeRaw
    } else if terrain_visible {
        ViewBranch::Themed
    } else {
        ViewBranch::Flat
    }
}

/// ## 职责
/// 把「当前活动地形 + 视图状态」翻译成一张场 quad：数据变了重打纹理，视图变了
/// 只改 uniform。**不自己做任何画法决策**——颜色公式全在 WGSL 与 `FieldParams`。
///
/// ## 关键流程
/// 1. gate：`view.field_render && layers.terrain.visible`，不满足 → [`teardown_field`]；
/// 2. 取 `active_terrain`（沿 `parent` 向上继承，与 chunk 路径同源）；
/// 3. 算 [`FieldKey`]：结构量直接读，内容指纹**只在可能有变化的帧**算
///    （`chunks_dirty` 待消费 / 结构变了 / 首次）——与 `sync_isoline_overlay`
///    同款纪律，稳态零成本；
/// 4. 键的内容部分变了 → 重打 [`FieldPack`] + 换材质（`Instant` 计量毫秒）；
///    只有分支 / 色板变了 → 只重写 uniform；
/// 5. quad 的变换每帧校正（`raster_origin` / `cell_size` 随图变）。
///
/// ## 对外接口
/// - 资源 [`TerrainFieldRender`]（资产与键）、[`FieldRenderStats`]（计数器）；
/// - 读取 `Res<TerrainChunkEntities>`：只在计数器里反映「场开着时还有多少块」。
///
/// ## 为什么排在 `sync_terrain_chunks` **之后**
///
/// 1. **计数器口径**：`field_render_on_chunk_entities` 必须等于**帧末**活着的
///    chunk 数（QA 把它与 `TerrainChunk` 组件数逐帧比对，两个口径不许互相否定）。
///    块的 despawn / spawn 都由 chunk 系统在**本帧**完成，故本系统读到的
///    `TerrainChunkEntities` 才是帧末真值。排在前面读到的是上一帧的残留。
/// 2. **不抢信号**：编辑信号取自**变更戳**（`WorldProjection` / `TerrainTexture`
///    的 `is_changed`），不依赖 `tex.chunks_dirty` 的消费顺序——那个旗标由
///    chunk 系统照旧消费清零，本系统**一行都不碰**。
/// 3. **独占性不靠顺序**：场开着时 chunk 系统自己早退（回退开关在它函数开头），
///    故「两条路径同帧只活一条」与谁先谁后无关。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_field(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainFieldMaterial>>,
    mut tex: ResMut<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    chunk_entities: Res<TerrainChunkEntities>,
    mut field: ResMut<TerrainFieldRender>,
    mut stats: ResMut<FieldRenderStats>,
    mut q_quads: Query<&mut Transform>,
) {
    // 帧末块数：本系统排在 `sync_terrain_chunks` 之后，块的 spawn / despawn
    // 都已在**本帧**完成（`by_coord` 是立即写，实体是延迟命令但同一帧落地），
    // 故这里读到的就是「这一帧结束时还活着几块」。
    stats.field_render_on_chunk_entities = chunk_entities.len();

    // 素底政治页（ADR-0079 T5）：地形层**隐藏**不退场——Flat 分支的平面
    // 海陆底就是这一层的替身，势力填色要压在它上面。退场只听
    // `field_render` 回退开关（真关场渲染 = chunk 路径接管，chunk 系统
    // 会在隐藏时 despawn——政治页随之真空白，与旧行为一致）。
    if !view.field_render {
        teardown_field(&mut commands, &mut field, &materials, &mut stats);
        return;
    }

    // 世界还没有任何带栅格的地形（启动早期 / 空世界）：无数据可求值，退场。
    let Some((source, data)) = active_terrain(&projection.world, drill.current()) else {
        teardown_field(&mut commands, &mut field, &materials, &mut stats);
        return;
    };

    // ── 分支：CellMap 看原始群系色 / 其余按地形层可见否选主题或素底 ──
    let branch = field_branch_of(view.mode, layers.terrain.visible);
    let map = Some(source.id.clone());
    let palette_id: &'static str = view.active_theme.id;

    let cached_hash = field.key.as_ref().map(|k| k.terrain_hash);
    let structure_changed = match &field.key {
        Some(k) => k.map != map || k.branch != branch || k.palette_id != palette_id,
        None => true,
    };
    // 「可能有编辑」的变更戳：世界被改（笔刷 / 撤销 / 换图 / 载入）或纹理被写过
    // （`flush_terrain_texture` 的 patch、整图重建）。
    // **不用 `tex.chunks_dirty`**：本系统排在 chunk 之后，那旗标已被消费。
    let edit_signal = projection.is_changed() || tex.is_changed();
    // 内容指纹 1M 格 ≈3 ms：只在「可能会有变化」的帧算（与 `sync_isoline_overlay`
    // 同款纪律），稳态零成本。
    let needs_hash = structure_changed || edit_signal || cached_hash.is_none();
    let terrain_hash = if needs_hash {
        // **必须用 `pack_content_hash`（裸 height/surface/biome），不能用
        // `terrain_data_hash`**：后者折叠的是 `display_kind` 显示槽位，而 attr
        // 纹理 G 通道存的是裸 biome 字节——两者不同源，会漏打（详见该函数文档）。
        wv_world_cell_map::terrain_view::field::pack_content_hash(data)
    } else {
        cached_hash.unwrap_or_default()
    };

    // ── 世界幅面：与 chunk 路径同一口径（`raster_origin` + 格数 × `cell_size`）──
    let cell_size = grid.cell_size.max(1e-6);
    let cols = data.width.max(1) as f32;
    let rows = data.height.max(1) as f32;
    let extent = Vec2::new(cols, rows) * cell_size;
    let center = grid.raster_origin + extent * 0.5;
    let z = TERRAIN_Z + layers.terrain.z_offset;
    // 屏幕像素 / 格：`ViewState.scale` 的语义是「世界单位 / 屏幕像素」，
    // 故 ppc = 格的世界尺寸 / scale（demo 里一格 = 1 世界单位，同一个式子）。
    let px_per_cell = (cell_size / view.scale.max(1e-6)).max(1e-4);
    // ── 米/格（ADR-0077 D2）：晕渲法线米制换算的唯一入口 ──
    // 地形来源图（`active_terrain` 沿 parent 向上继承的那张，`source` 即其
    // MapNode）声明了比例尺 → 精确换算；未声明走「1 wu = 1 m」回落
    // （README §12-53 待决；与 CPU 烘焙路径同一条入口、同一回落假设）。
    let meters_per_cell = bridge_meters_per_cell(cell_size, source.unit_per_km);
    let mpc_changed = field
        .last_mpc
        .is_none_or(|last| (last - meters_per_cell).abs() > 1e-6);

    let content_changed =
        cached_hash != Some(terrain_hash) || field.key.as_ref().is_some_and(|k| k.map != map);

    // 重打判据：**只看内容指纹**（没有 quad / 材质缺失 / 指纹不等）。
    //
    // ## 为什么 `edit_signal` 不能进这个判据（2026-10-24 修，N1）
    // 曾经的判据是 `... || edit_signal`，理由是「多打一次只花 ~10ms，漏打是
    // 画了不显示」——代价不对称的直觉。但它有两个实测后果：
    // 1. **一次编辑打两遍**：`edit_signal` 含 `tex.is_changed()`，而 chunk 系统
    //    排在本系统之后、会擦 `chunks_dirty`，那次写把 changed tick 推到下一帧
    //    ⇒ N+1 再命中一次（QA 实测 `left: 3, right: 2`）；
    // 2. **选区变更也重打**：`projection.is_changed()` 在「选中某个 region /
    //    place」时同样置真 ⇒ 点一下地图就付一次全量重打（48ms @1024²）。这正是
    //    `world_projection.rs` 里记过的「selection 写入置真 = 实机 1 FPS 根因」
    //    的同款陷阱。
    //
    // 正解是**比内容，不比通知**：指纹相同 ⇒ 打包结果逐字节相同 ⇒ 重打只是把
    // 同一份数据再上传一遍，没有任何视觉收益。`edit_signal` 现在唯一的职责是
    // 上面的 `needs_hash`——省掉「没变时也算一遍 hash」的开销，不许自己成为
    // 触发条件。（同类教训第三次：网格键 `scale_bits` → 锚点键连续 `spacing`
    // → 这里的 change tick。）
    if field.quad.is_none() || field.material.is_none() || content_changed {
        let started = std::time::Instant::now();
        // ── ① 先试增量补丁：局部编辑只碰脏矩形，不重打整张图 ──
        //
        // 可增量的三个条件（缺一就走下面的全量重打）：
        // 1. **同一张图**：`field.key.map` 与本次 `map` 相同——拿上一张图的
        //    字节补这一张，幅面碰巧相同就会静默画错；
        // 2. **有可信脏区**：`tex.field_dirty` 由 `mark_dirty` / `mark_all_dirty`
        //    写入。`None` 意味着「本帧没有局部信号」（载档 / 撤销全表 / 首次），
        //    不许猜一个矩形；
        // 3. **脏区不太大**：超过全场 1/4 时窗口 EDT 已接近全场规模，重建材质
        //    反而更省，也少一条「半新半旧」的路径。
        //
        // `bypass_change_detection`：消费脏区**不是**「纹理内容变了」，若照常
        // 标记 changed，下一帧 `edit_signal` 会假真一次、白算一遍内容指纹
        // （N1 那次「一次编辑打两遍」就是同款误信号）。
        let mut patched = false;
        if content_changed && field.material.is_some() {
            let same_map = field.key.as_ref().is_some_and(|k| k.map == map);
            let rect = tex
                .bypass_change_detection()
                .take_field_dirty()
                .map(CellRect::from_bounds);
            let full_area = u64::from(data.width.max(1)) * u64::from(data.height.max(1));
            if let (true, Some(rect)) = (same_map, rect) {
                if !rect.is_empty() && rect.area() * 4 <= full_area {
                    let handle = field.material.clone().expect("刚判过 is_some");
                    if let Some(pack) = field.pack.as_mut() {
                        if pack.patch_region(data, rect) {
                            if let Some(material) = materials.get(&handle) {
                                patched = patch_material_images(&mut images, material, pack, rect);
                            }
                        }
                    }
                    if patched {
                        let ms = started.elapsed().as_secs_f32() * 1000.0;
                        stats.field_patches += 1;
                        stats.last_patch_ms = ms;
                        // `last_repack_bytes` 不动：它记的是「最近一次**全量**重打的
                        // 字节数」，混进补丁字节会让这条诊断读数自相矛盾。
                        info!(
                            "地形场：增量补丁 {}×{} 格 @({}, {})（{:.2} ms）",
                            rect.width(),
                            rect.height(),
                            rect.x0,
                            rect.y0,
                            ms
                        );
                    }
                }
            }
        }

        if !patched {
            // ── ② 全量重打：三张数据纹理全量重建（1024² = 12 MB）──
            // 海岸编码显式取 **Signed**（SPEC §3.8.4 追记一）：Unsigned 的
            // 符号取自 per-texel 掩码，水陆交界是逐格硬切——深缩放下是
            // 一格宽的正交台阶。Signed 让 sd 经双线性插值在岸线处穿过 0，
            // 配合 shader 的屏幕封顶过渡（`COAST_EDGE_HALF_PX`）得到
            // 亚格连续的锐利 AA 边。增量补丁路径经 `patch_region` 沿用
            // `self.encoding`，无需另改。
            let pack = FieldPack::from_terrain_with(data, CoastEncoding::Signed);
            let mut params = default_params(
                [pack.width as f32, pack.height as f32],
                px_per_cell,
                branch,
                FieldPalette::default(),
            );
            // ADR-0072 T3：群系填色强度随主题（pad0.z，shader 侧 p.pad0.z）。
            params.pad0.z = view.active_theme.biome_tint;
            // pack 与 shader 的解码开关必须同源（唯一耦合点，见 `set_coast_encoding`）。
            set_coast_encoding(&mut params, pack.encoding);
            // 分层设色（ADR-0056）：内边界 = **绝对地形学阶梯**；窄域图（跨度
            // < 300 m）兜底为自身区间等分。与 CPU 烘焙路径同一函数——同一张图
            // 两条路径必须得出同一组阶。归一化口径与 height 纹理同源
            // （`H_MIN` / `H_MAX`），否则阶会整体偏移。
            //
            // **只在 CellMap 分支付这笔钱**：Themed 分支的 shader 根本不读
            // `elev_*`；且本函数只求 min/max（O(N)，不排序），成本可忽略。
            let cuts_norm = if branch == ViewBranch::BiomeRaw {
                let cuts_m = hypsometric::cuts_for_cells(&data.cells);
                hypsometric::normalized_cuts(&cuts_m, H_MIN, H_MAX - H_MIN)
            } else {
                [0.0; 7]
            };
            apply_hypsometric(&mut params, &cuts_norm, view.hypsometric_tint);
            // ADR-0077 D2：米/格写进 `light_dir.w`（WGSL 侧 `mpc` 槽；≤ 0 = 未
            // 接线回落 1.0——`default_params` 的 0 占位在此被真值替换）。
            params.light_dir.w = meters_per_cell;
            let material = materials.add(build_material(&mut images, &pack, params));

            if let Some(entity) = field.quad {
                // 实体留着，只换材质——mesh 是单位方片，跨图跨缩放恒定。
                commands
                    .entity(entity)
                    .insert(MeshMaterial2d(material.clone()));
            } else {
                let mesh = match field.mesh.clone() {
                    Some(mesh) => mesh,
                    None => {
                        let mesh = meshes.add(field_quad_mesh());
                        field.mesh = Some(mesh.clone());
                        mesh
                    }
                };
                let entity = commands
                    .spawn((
                        Mesh2d(mesh),
                        MeshMaterial2d(material.clone()),
                        Transform::from_translation(center.extend(z))
                            .with_scale(extent.extend(1.0)),
                    ))
                    .id();
                field.quad = Some(entity);
            }
            if let Some(previous) = field.material.replace(material) {
                reclaim_field_material(&mut commands, previous, &materials);
            }
            // 留一份 CPU 影子副本给后续的增量补丁（见 `TerrainFieldRender::pack`）。
            field.pack = Some(pack);
            // uniform 已随新材质写入，记下当前 ppc 免得下面再传一次。
            field.last_ppc = Some(px_per_cell);
            field.last_mpc = Some(meters_per_cell);
            field.last_hypsometric = Some(view.hypsometric_tint);
            field.cuts = Some(cuts_norm);

            stats.field_repacks += 1;
            stats.last_repack_ms = started.elapsed().as_secs_f32() * 1000.0;
            let pack = field.pack.as_ref().expect("刚写入");
            stats.last_repack_bytes =
                pack.height_tex.len() + pack.coast_tex.len() + pack.attr_tex.len();
            info!(
                "地形场：重打 {}×{} 格（{:.2} MB，{:.2} ms）ppc={:.2} 分支={:?}",
                pack.width,
                pack.height,
                stats.last_repack_bytes as f32 / (1024.0 * 1024.0),
                stats.last_repack_ms,
                px_per_cell,
                branch
            );
        }
    }

    // ── 变换每帧校正：换图 / 换格距都会改 `raster_origin` 与幅面 ──
    if let Some(entity) = field.quad {
        if let Ok(mut transform) = q_quads.get_mut(entity) {
            let translation = center.extend(z);
            if transform.translation != translation || transform.scale.truncate() != extent {
                transform.translation = translation;
                transform.scale = extent.extend(1.0);
            }
        }
    }

    // ── uniform：缩放是**连续量**，只在这里落地（不进键）──
    let ppc_changed = field
        .last_ppc
        .is_none_or(|last| (last - px_per_cell).abs() > 1e-6);
    // 分层设色强度（ADR-0055）：拖滑条只改 uniform——数据纹理逐字节不变，
    // 重打是纯浪费（12 MB 上传）。
    let hyps_changed = field
        .last_hypsometric
        .is_none_or(|last| (last - view.hypsometric_tint).abs() > 1e-6);
    if (ppc_changed || structure_changed || hyps_changed || mpc_changed) && field.material.is_some()
    {
        let handle = field.material.clone().expect("刚判过 is_some");
        if let Some(mut material) = materials.get_mut(&handle) {
            // shape: x = px_per_cell / y = 分支 / z = z_exag / w = grad_step（后两者
            // 由 `default_params` 填锁值 T15/T14，缩放不动它们）。
            material.params.shape[0] = px_per_cell;
            material.params.shape[1] = branch.code();
            // ADR-0077 D2：米/格槽随格距 / 比例尺变化重写（真源与重打路径
            // 同一条 `meters_per_cell_of`；增量补丁路径不动 uniform，槽值
            // 沿用建材质时的真值，无需重填）。
            material.params.light_dir.w = meters_per_cell;
            // T19 域扭曲幅度（P1 预留槽 `pad0.x`）：随缩放重算。
            material.params.pad0[0] = warp_amp(px_per_cell);
            // 主题色板（T35）：P1 用 `TopographicPalette` 四色的默认转录——
            // 与 CPU 回退路径同色族，不同色就是回归（`FieldPalette::default` 的
            // 文档写着逐值抄录日期与出处）。
            let palette = FieldPalette::default();
            let rgba = |c: LinearRgba| Vec4::new(c.red, c.green, c.blue, c.alpha);
            material.params.theme = [
                rgba(palette.water),
                rgba(palette.land_low),
                rgba(palette.land_mid),
                rgba(palette.land_high),
            ];
            material.params.ink = rgba(palette.ink);
            material.params.river_ink = rgba(palette.river_ink);
            material.params.paper = rgba(palette.paper);
            // 分层设色（ADR-0055）：内边界复用重打时的缓存（百分位与缩放无关），
            // 强度随滑条逐帧变——两项都只落在 uniform 上。
            if let Some(cuts) = field.cuts {
                apply_hypsometric(&mut material.params, &cuts, view.hypsometric_tint);
            }
            field.last_hypsometric = Some(view.hypsometric_tint);
            field.last_ppc = Some(px_per_cell);
            field.last_mpc = Some(meters_per_cell);
        }
    }

    field.key = Some(FieldKey {
        map,
        terrain_hash,
        branch,
        palette_id,
    });
    stats.field_quads = usize::from(field.quad.is_some());
}

// ══════ from src/region_overlay.rs :: sync_region_overlay_index ══════
