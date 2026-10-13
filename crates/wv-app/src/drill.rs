//! 地图三级钻取（v0.2 §B）的**导航层**：标记布局、命中测试、相机过渡。
//!
//! ## 为什么单独一个模块
//!
//! [`crate::state::DrillState`] 只回答「我现在在哪一级」；本模块回答
//! 「下一级画在哪、怎么点进去、进去时相机怎么动」。前者是状态，后者是交互，
//! 混进 [`crate::projection`]（那一模块的职责是**时间**投影，见其模块文档的「铁律」）
//! 会让两个正交的关注点互相纠缠。
//!
//! ## 数据真相：落点来自 `MapNode::icons`
//!
//! [`wv_core::map::MapNode`] 有 `parent` / `children` 的 **ID 引用**（原则四），
//! 也有 `extent`（尺寸），但**没有**「子图在父图里的位置」字段。
//! 早期设想是给 `MapNode` 加一个落点字段，但那会让**一个地图节点只能有一个位置**，
//! 而作者要在同一张图上摆很多标记（城市、港口、隘口、事件点、钻入入口……）。
//! 于是落点属于**图标自己**：`IconSpec::anchor` 是本图 `extent` 内的归一化坐标
//! （`[0,1]²`，左上角为原点、y 向下），`IconSpec::drill_into` 把某个图标变成钻入入口。
//!
//! 两类标记共存：
//!
//! 1. **显式图标**——按 `anchor` 摆位，作者说了算；
//! 2. **没有图标的子图**——回退到「ID 确定性哈希」摆位（[`marker_local`]），
//!    好处是同一次运行里标记不动、可测、可复现；代价是它与任何真实地理布局无关。
//!
//! 显式 `anchor` **不加边距**：写 `0.0` 就是贴着左边缘（描边可能被裁掉，那是作者的选择）；
//! 只有哈希回退留 12% 边距，避免标记贴边被裁。
//!
//! 图标机制的取舍与 YAML 写法见 `README.md` §12 第 12 条。
//!
//! ## 视觉规格（v0.2 §B.2 + `IconSpec`）
//!
//! 钻入标记**用界面蓝（`ui.accent`），不用金**——金是「选中地块」的语义，
//! 钻入是**导航**不是选中，两者撞色会让作者误判。已访问态转 `ui.text_3` 灰；
//! 纯注释标记（无 `drill_into`）也用这个灰，不抢导航语义。
//!
//! `IconSpec::color` 显式声明的颜色**优先于以上全部**——作者定了色就按作者的来。
//! 尺寸按目标层级分：城市 14px、建筑 12px + 中心实心点，`IconSpec::size` 可覆盖；
//! 形状由 `IconSpec::shape` 定，缺省菱形。

use bevy::prelude::*;

use crate::state::{lod_tier, DrillState, GridConfig, LodTier, ViewState, WorldProjection};
use crate::ui_theme::UiShellConfig;
use crate::ui_tokens::UiTheme;
use wv_core::id::Id;
use wv_core::map::{Extent, IconShape, IconSpec, MapLevel, MapNode};

/// 钻取过渡时长（v0.2 §B：三层过渡 380ms ease-out）。
///
/// 与 `ANIMATION_SPEC` AD-3「相机零惯性」不冲突：那条约束针对**连续拖拽**
/// （平移/滚轮要指哪打哪），而钻取是**离散的层级切换**，没有「拖到一半」的中间态。
pub const DRILL_TRANSITION_SECS: f32 = 0.380;

/// 标记命中区（v0.2 §B.2：24×24）。返回**世界单位**，故乘 `cell_size`。
const HIT_CELLS: f32 = 3.0;

/// 子图没有 `extent` 时的兜底尺寸（与示例工程的 4096×4096 一致）。
const FALLBACK_EXTENT: Extent = Extent::sized(4096.0, 4096.0);

/// 双击判定的时间窗（秒）与位移容差（世界单位）。
const DOUBLE_CLICK_SECS: f64 = 0.40;
const DOUBLE_CLICK_SLOP: f32 = 6.0;

/// 上次左键点击的时间与位置，用于自建双击判定。
///
/// Bevy 没有内建双击事件，而 egui 的 `double_clicked()` 只在指针被面板捕获时可用；
/// 地图区的拾取发生在我们自己的系统里，故自行记录。
#[derive(Resource, Default)]
pub struct DrillClick {
    last: Option<(f64, Vec2)>,
}

/// 把钻取相关的资源与系统注册进 `App`。
pub fn register(app: &mut App) {
    app.init_resource::<DrillClick>()
        .init_resource::<crate::zoom_level::ZoomCrossRequest>()
        .add_systems(
            Update,
            (
                // 换级必须排在 `drill_transition` **之前**：它要吃掉本帧的
                // 「深度刚变」标记，否则过渡会在滚轮换级时把相机飞向标记点
                // （换级的锚点是指针，不是标记）。
                apply_zoom_cross,
                drill_pick,
                drill_transition,
                draw_drill_markers,
                // 地名标注在标记之后对齐（读同一份钻取状态，画在标记上方）。
                update_place_labels,
            )
                .chain(),
        );
}

/// 消费滚轮换级请求：撞到当前级的边界后继续同向滚动 = 换到相邻级（ADR-0009 §2 决策 ⑤/⑦）。
///
/// ## 为什么换级**不改缩放**
///
/// 子图 `extent` 比父图小，故 `child.min < parent.min`——滚到父图内界时那个 scale
/// 早已落在子图区间内（[`crate::zoom_level`] 的模块文档有推导）。所以换级只需换
/// 「现在显示哪张图的哪一块」，相机一个像素都不用动，手感才是连续的。
///
/// ## 为什么吃掉深度变化标记
///
/// `drill_transition` 的语义是「双击/面包屑跳转 → 飞向目标标记」。滚轮换级的锚点
/// 是**指针**（指针下的世界点必须原地不动），若也飞向标记，画面会从指针处滑走。
/// 于是本系统在真正换了级之后把 `take_depth_change` 的标记拿走，并自己同步相机。
#[allow(clippy::too_many_arguments)]
fn apply_zoom_cross(
    mut cross: ResMut<crate::zoom_level::ZoomCrossRequest>,
    mut drill: ResMut<DrillState>,
    projection: Res<WorldProjection>,
    windows: Query<&Window>,
    mut view: ResMut<ViewState>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let Some(request) = cross.pending.take() else {
        return;
    };
    let moved = match request {
        // 继续往外：回到父级。根图再往外没有父级，`up()` 自己会拒绝。
        crate::zoom_level::ZoomCross::Out => drill.up(),
        // 继续往里：进到光标所在的那个子图（`enter` 只接受真子图，越级会被拒）。
        crate::zoom_level::ZoomCross::Into(child) => drill.enter(&projection.world, &child),
    };
    if !moved {
        return;
    }
    // 换级由本系统接管取景，不让 `drill_transition` 再飞一次。
    drill.take_depth_change();

    let window_size = windows
        .single()
        .map(|w| w.resolution.size())
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let zoom = crate::zoom_level::level_zoom(
        &projection.world,
        drill.current(),
        window_size,
        Vec2::splat(crate::zoom_level::FALLBACK_MAP_SIZE),
    );
    // 新级区间比旧级更宽（往外）或更深（往里），夹一下以防数据异常把 scale
    // 留在新级之外；正常情况下 `clamp` 是恒等变换。
    view.scale = zoom.clamp(view.scale);
    crate::camera::apply_view_to_camera(view.camera_center, view.scale, &mut camera);
}

// ---------------------------------------------------------------------------
// 标记布局（纯函数，可测）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位哈希（无依赖、跨平台稳定，`std` 的 `DefaultHasher` 不保证跨版本稳定），
/// **末尾接一段 MurmurHash3 的 `fmix32` 雪崩混合**。
///
/// 为什么要补这一步：纯 FNV-1a 对「只差一个字节」的输入扩散很弱。
/// ID 是 `city_a` / `city_b`、`bld_01` / `bld_02` 这种**编号命名**（AGENTS §4.4），
/// 只做 FNV 的话两个标记会落在几十世界单位内、叠成一坨——实测 `city_a` 与 `city_b`
/// 相距仅 38 单位，在 4096 的图上是同一个点。加 `fmix32` 后单字节差异也会散开。
fn fnv1a(bytes: &[u8], salt: u32) -> u32 {
    let mut h = 2_166_136_261u32 ^ salt;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    // MurmurHash3 finalizer（公有领域算法，整数运算，跨平台逐位一致）。
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h
}

/// 哈希到 `[0,1)`。
fn hash01(id: &str, salt: u32) -> f32 {
    fnv1a(id.as_bytes(), salt) as f32 / (u32::MAX as f32 + 1.0)
}

/// 子图在父图 `extent` 内的**局部**落点（父图左上角为原点，y 向下同 `extent` 语义）。
///
/// 留 12% 边距：标记贴着图边时描边会被裁掉，且与地形四边形边框视觉粘连。
pub fn marker_local(extent: Option<Extent>, id: &Id) -> Vec2 {
    let e = extent.unwrap_or(FALLBACK_EXTENT);
    let x = hash01(id.as_str(), 0x9E37_79B9);
    let y = hash01(id.as_str(), 0x85EB_CA6B);
    Vec2::new(e.width * (0.12 + 0.76 * x), e.height * (0.12 + 0.76 * y))
}

/// 把「屏幕像素」口径的标记尺寸换算成世界单位的半边长。
///
/// v0.2 §B.2 给的 14 / 12 是**屏幕像素**，而 gizmo 画在世界空间；默认 1 世界单位 =
/// 1 像素（`TILE_BASE = 32`、`cell_size = 8` 时一格 = 8 像素），故用 `cell_size` 换算，
/// 与 [`crate::projection::draw_brush_cursor`] 的既有约定一致。
pub fn half_from_px(px: f32, cell_size: f32) -> f32 {
    px * 0.5 * (cell_size / 8.0)
}

/// 钻入标记按目标层级选的**缺省**半边长（世界单位）。
///
/// 仅在图标没写 `size` 时使用；作者写了的就按作者的来。
pub fn marker_half(cell_size: f32, level: MapLevel) -> f32 {
    let px = match level {
        MapLevel::City => 14.0,
        MapLevel::Interior => 12.0,
        // 区域与世界级子图原型里没有，按城市尺寸画即可。
        _ => 14.0,
    };
    half_from_px(px, cell_size)
}

/// 纯注释标记在作者没给 `size` 时的缺省屏幕像素（与建筑级同量级，不抢视觉）。
const ANNOTATION_ICON_PX: f32 = 12.0;

/// 当前层级的一个**待绘制 / 可命中**的标记。
///
/// 声明顺序即绘制顺序（先声明的在下层），故本结构不携带 `z`。
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// 图标 id；哈希回退的标记也用子图 id（此时它就是图标 id）。
    pub id: Id,
    /// 双击可钻入的目标子图；`None` = 纯注释标记，只画不可点。
    pub target: Option<Id>,
    /// 世界坐标中心。
    pub center: Vec2,
    /// 半边长（世界单位），已把 `size` 覆盖与层级缺省都算好。
    pub half: f32,
    /// 形状。
    pub shape: IconShape,
    /// 图标显式声明的颜色；`None` = 用主题色（界面蓝 / 已访问灰）。
    pub color: Option<[u8; 3]>,
    /// 是否画中心实心点——建筑级标记在缩小时与城市级难分辨（§7 非颜色依赖）。
    pub dot: bool,
}

/// 图标的**局部**落点：显式 `anchor` 优先，缺省回退到 [`marker_local`]。
///
/// 显式 `anchor` 是 `extent` 内的**归一化**坐标（左上角为原点、y 向下），
/// 作者写什么就是什么——`[0,0]` 是左上角、`[0.5,0.5]` 是正中心；越界值夹回 `[0,1]`
/// （数据层不拒收，`AGENTS.md` §7.5V1）。注意这里**不叠加**哈希回退的 12% 边距，
/// 因为那是兜底方案才需要防裁边的。
pub fn icon_local(extent: Option<Extent>, icon: &IconSpec) -> Vec2 {
    match icon.anchor {
        Some([x, y]) => {
            let e = extent.unwrap_or(FALLBACK_EXTENT);
            Vec2::new(e.width * x.clamp(0.0, 1.0), e.height * y.clamp(0.0, 1.0))
        }
        None => marker_local(extent, &icon.id),
    }
}

/// 一张图在世界坐标里的**图帧原点**（`MapFrame.world_origin`）。
///
/// 这是全项目「图帧原点」的**唯一实现点**（类型转换层）：`GridConfig::map_origin`
/// 由 [`crate::terrain_view::sync_grid_config`] 调它填、街道编辑（`streets`）
/// 反向换算本图局部坐标也调它，谁都别再手搓 `resolve_frame_in`。
///
/// 与栅格原点**不是一回事**，这是本模块此前最深的坑：
///
/// | 量 | 含义 | 今天的取值 |
/// |---|---|---|
/// | `grid.raster_origin` | 当前显示的**栅格纹理**在世界里的原点 | 恒为根图栅格的 `(0,0)`（子图未自带底图时） |
/// | 本函数 | **当前这张图**在世界里的位置 | 逐层累积，可由父图钻入图标派生 |
///
/// 旧代码把两者当成同一个（都取栅格原点），于是 1024×1024 的城市图上
/// 声明了 `anchor: [0.5, 0.5]` 的图标，落到了 4096² 世界帧的左上四分之一处；
/// 后来标记层改用了本函数，但**街道 / 地点 / 外框没跟着改**（同一个坑只填了一半）。
/// 坐标链的**唯一**数学实现点是 [`wv_core::coord`]（基线 §2.15），本函数只做类型转换。
///
/// 坐标链断掉（图不在工程列表里 / 无 `extent`）时退回 `grid.raster_origin`——
/// 摆位宁可回到旧行为（「本图就是栅格那张图」），也不能让整层标记消失。
pub(crate) fn map_origin(world: &wv_core::World, node: &MapNode, grid: &GridConfig) -> Vec2 {
    match wv_core::resolve_frame_in(&world.maps, &node.id) {
        Some(frame) => Vec2::new(frame.world_origin.x, frame.world_origin.y),
        None => grid.raster_origin,
    }
}

/// 当前层级下所有标记（显式图标 + 没有图标的子图）。
///
/// 顺序 = 先**父图 `icons` 的声明顺序**，再**`children` 顺序里还没被图标覆盖的子图**。
/// 这一顺序就是绘制顺序，也是命中测试「重叠时取最近」的前提。
pub fn resolve_markers(
    world: &wv_core::World,
    drill: &DrillState,
    grid: &GridConfig,
    // 当前缩放（世界单位 / 屏幕像素）。图标像素尺寸按「屏幕像素」定义
    // （v0.2 §B.2），旧实现隐含 1 世界单位 = 1 像素；缩放后必须乘回 zoom
    // 才能保持屏幕大小恒定，否则整图缩小时标记缩成一个点。
    screen_scale: f32,
) -> Vec<Marker> {
    let Some(current) = drill.current_node(world) else {
        return Vec::new();
    };
    let origin = map_origin(world, current, grid);
    let level_of = |target: Option<&Id>| {
        target.and_then(|t| world.maps.iter().find(|m| &m.id == t).map(|m| m.level))
    };
    let mut out = Vec::new();
    let mut claimed: Vec<Id> = Vec::new();

    // ① 父图显式声明的图标。
    // TODO(阶段二): 待决 §12-12（README §12 第 12 条）—— `icons` 目前同时承担
    // 「注释标记」与「钻入入口」两种语义（`drill_into` 有值即可点）。若最终决定
    // 把子图入口拆成独立字段，收敛点就是下面这个循环的 `target` 取值，
    // `children` 保持 `Vec<Id>` 不动，改动面可控。
    for icon in &current.icons {
        let target = icon.drill_into.clone();
        let target_level = level_of(target.as_ref());
        // 尺寸优先级：显式 `size` > 目标层级缺省 > 注释标记缺省。
        let half = match icon.size {
            Some(px) => half_from_px(px, grid.cell_size) * screen_scale,
            None => match target_level {
                Some(level) => marker_half(grid.cell_size, level) * screen_scale,
                None => half_from_px(ANNOTATION_ICON_PX, grid.cell_size) * screen_scale,
            },
        };
        if let Some(t) = &target {
            claimed.push(t.clone());
        }
        out.push(Marker {
            id: icon.id.clone(),
            target,
            center: origin + icon_local(current.extent, icon),
            half,
            shape: icon.shape,
            color: icon.color.as_ref().and_then(|c| c.to_rgb()),
            dot: target_level == Some(MapLevel::Interior),
        });
    }

    // ② 声明在 `children` 里但父图上没有任何图标的子图——回退到哈希摆位，
    //    保证它仍然有入口、仍然能钻进去。
    // TODO(阶段二): 待决 §12-12（README §12 第 12 条）—— 这个哈希回退是否
    // 长期保留，还是最终应强制作者为每个子图显式声明 `anchor`？若最终决定
    // 强制声明，收敛点就是下面这个循环（连同 `marker_local` 一起删除），
    // 改动面可控。
    for child in drill.child_nodes(world) {
        if claimed.iter().any(|c| c == &child.id) {
            continue;
        }
        out.push(Marker {
            id: child.id.clone(),
            target: Some(child.id.clone()),
            center: origin + marker_local(current.extent, &child.id),
            half: marker_half(grid.cell_size, child.level) * screen_scale,
            shape: IconShape::Diamond,
            color: None,
            dot: child.level == MapLevel::Interior,
        });
    }
    out
}

/// 命中测试：给定世界坐标，返回它落在哪个钻入标记上。
///
/// 命中区是**方形**（24×24，v0.2 §B.2），比菱形本身大——菱形是视觉符号，
/// 目标大小必须够点击（§7：触控目标 ≥28×28 的同类要求）。
pub fn marker_hit(
    world: &wv_core::World,
    drill: &DrillState,
    grid: &GridConfig,
    cursor: Vec2,
    screen_scale: f32,
) -> Option<Id> {
    let mut best: Option<(Id, f32)> = None;
    for marker in resolve_markers(world, drill, grid, screen_scale) {
        // 纯注释标记只画不可点——它是作者的批注，不是导航入口。
        let Some(target) = marker.target else {
            continue;
        };
        // 命中区不小于标记本身，也不小于 24×24 的方块下限（屏幕像素口径，随 zoom 换算）。
        let half = marker
            .half
            .max(grid.cell_size * HIT_CELLS * 0.5 * screen_scale);
        let d = cursor - marker.center;
        if d.x.abs() <= half && d.y.abs() <= half {
            // 重叠时取最近的，避免摆位撞车后选中「不是最上面那个」。
            let dist = d.length_squared();
            if best.as_ref().is_none_or(|(_, b)| dist < *b) {
                best = Some((target, dist));
            }
        }
    }
    best.map(|(id, _)| id)
}

/// 相机在某一层级的目标**位置**（世界坐标）。
///
/// 位置 = 当前图在**父图**里的标记落点。这样「双击菱形钻入」之后
/// 视野落点与刚才点的位置一致，不会跳到一个和新画面无关的地方。
/// 根图没有父图，落点取图心。
///
/// 缩放**不在**这里给：目标缩放必须落在**新一级的缩放区间**内（ADR-0009 §2⑤），
/// 见 [`drill_fly_scale`]。早期的硬编码 `4.0` 在分级后可能越出子图区间，
/// 于是双击进城后第一次滚轮会被 clamp 硬拽一下——那是本条拆开的原因。
pub fn drill_target(world: &wv_core::World, drill: &DrillState, grid: &GridConfig) -> Vec2 {
    let Some(current) = drill.current_node(world) else {
        return Vec2::ZERO;
    };
    // 从父图钻进来：落点 = **父图的图帧原点** + 本图在父图局部坐标里的落点
    // （即父图上那个入口图标的位置）。**不是** `grid.raster_origin`——后者是栅格纹理的
    // 原点，见 `map_origin`。用错会让第二级起的钻取落点整体偏到世界帧左上角。
    if drill.depth() > 0 {
        if let Some(parent) = current
            .parent
            .as_ref()
            .and_then(|pid| world.maps.iter().find(|m| &m.id == pid))
        {
            let parent_origin = map_origin(world, parent, grid);
            return parent_origin + marker_local_for(parent, &current.id);
        }
    }
    // 其余情形（首次进入、父图悬空）：把相机放到**本图自己的中心**。
    let origin = map_origin(world, current, grid);
    origin + extent_center(current)
}

/// 相机过渡的目标**缩放**：落在当前级别的缩放区间内（ADR-0009 §2⑤）。
///
/// - 根级：保持历史行为 `1.0`（点击面包屑回世界）。
/// - 子级：`fit`（整幅子图铺满窗口），但**不把已经放大的视野拉远**；
///   最后再 clamp 进本级区间，保证过渡终点一定合法。
pub fn drill_fly_scale(
    world: &wv_core::World,
    drill: &DrillState,
    window_size: Vec2,
    from_scale: f32,
) -> f32 {
    if drill.depth() == 0 {
        return 1.0;
    }
    let size = crate::zoom_level::map_size_of(
        world,
        drill.current(),
        Vec2::splat(crate::zoom_level::FALLBACK_MAP_SIZE),
    );
    crate::zoom_level::fly_in_scale(size, window_size, from_scale)
}

/// 当前图在**父图**里的局部落点：优先用指向它的显式图标，没有就回退到哈希摆位。
///
/// 必须与 [`resolve_markers`] 走同一条路径——否则「双击的落点」与「钻入后相机的落点」
/// 会对不上，画面会跳一下。
fn marker_local_for(parent: &MapNode, id: &Id) -> Vec2 {
    parent
        .icons
        .iter()
        .find(|i| i.drill_into.as_ref() == Some(id))
        .map(|i| icon_local(parent.extent, i))
        .unwrap_or_else(|| marker_local(parent.extent, id))
}

fn extent_center(node: &MapNode) -> Vec2 {
    let e = node.extent.unwrap_or(FALLBACK_EXTENT);
    Vec2::new(e.width, e.height) * 0.5
}

// ---------------------------------------------------------------------------
// 输入：双击钻入
// ---------------------------------------------------------------------------

/// 双击钻入标记（v0.2 §B：双击菱形钻入）。
///
/// 命名上只管「双击之后钻进去」这一件事；**该不该吃这次点击归笔刷那一侧判**
/// （[`crate::camera::collect_brush_stroke`]）。原因：钻取是「双击」语义，第一下按下时
/// 还不知道用户要不要双击，但**要不要落笔在第一下就必须决定**，所以命中标记的判定
/// 必须在按下的那一刻、在笔刷的入口处完成，而不能等到第二下。
///
/// 本系统自己只需要守一条：**指针在面板上时不吃**（基线 §2.13，`PointerOverUi`）。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn drill_pick(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    time: Res<Time>,
    ui: Res<crate::camera::PointerOverUi>,
    projection: Res<WorldProjection>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut drill: ResMut<DrillState>,
    mut clicks: ResMut<DrillClick>,
) {
    if ui.wants_pointer || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let (Ok(window), Ok(transform)) = (windows.single(), camera.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let world_pos = crate::camera::screen_to_world(
        cursor,
        window.resolution.size(),
        transform.translation,
        view.scale,
    );

    let now = time.elapsed_secs_f64();
    let is_double = clicks
        .last
        .map(|(t, p)| now - t <= DOUBLE_CLICK_SECS && p.distance(world_pos) <= DOUBLE_CLICK_SLOP)
        .unwrap_or(false);
    // 无论是否双击都要记下这一次，否则「三连击」会被判成两段双击。
    clicks.last = if is_double {
        None
    } else {
        Some((now, world_pos))
    };

    if !is_double {
        return;
    }
    if let Some(child) = marker_hit(&projection.world, &drill, &grid, world_pos, view.scale) {
        drill.enter(&projection.world, &child);
    }
}

// ---------------------------------------------------------------------------
// 相机过渡（v0.2 §B：380ms ease-out）
// ---------------------------------------------------------------------------

/// 层级变化时把相机缓动到目标（位置 + 缩放同时插值）。
///
/// 触发点是 [`DrillState::take_depth_change`]：它只在深度**真的**变了的那一帧返回
/// `true`，所以「点了已有层级的面包屑」不会白播一段动画。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn drill_transition(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    projection: Res<WorldProjection>,
    grid: Res<GridConfig>,
    mut drill: ResMut<DrillState>,
    mut view: ResMut<ViewState>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    mut elapsed: Local<f32>,
) {
    let Ok((mut transform, mut cam_proj)) = camera.single_mut() else {
        return;
    };

    if drill.take_depth_change() {
        drill.transition_from = Some((transform.translation.truncate(), view.scale));
        *elapsed = 0.0;
    }
    let Some((from_pos, from_scale)) = drill.transition_from else {
        return;
    };

    // 用户中途插手（拖动 / 缩放）就放弃过渡，交给相机系统独占控制权：
    // 两个系统同帧写同一个 transform，谁后写谁赢，会让画面抖。
    if buttons.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle]) {
        drill.transition_from = None;
        return;
    }

    *elapsed += time.delta_secs();
    let k = (*elapsed / DRILL_TRANSITION_SECS).clamp(0.0, 1.0);
    let e = ease_out_cubic(k);
    let window_size = windows
        .single()
        .map(|w| w.resolution.size())
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let to_pos = drill_target(&projection.world, &drill, &grid);
    let to_scale = drill_fly_scale(&projection.world, &drill, window_size, from_scale);
    let pos = from_pos.lerp(to_pos, e);
    let scale = from_scale + (to_scale - from_scale) * e;

    transform.translation.x = pos.x;
    transform.translation.y = pos.y;
    // 投影写回：`cam_proj` 是 `Projection` 枚举（`Camera2d` 的 require，见 ADR-0011）。
    crate::camera::set_ortho_scale(&mut cam_proj, scale);
    view.scale = scale;
    view.camera_center = pos;

    if k >= 1.0 {
        drill.transition_from = None;
    }
}

/// `easeOutCubic`（v0.2 §B 指定的缓动，与原型 `animateCam` 一致）。
pub fn ease_out_cubic(k: f32) -> f32 {
    let inv = 1.0 - k;
    1.0 - inv * inv * inv
}

// ---------------------------------------------------------------------------
// 绘制
// ---------------------------------------------------------------------------

/// 画当前层级的全部钻入标记（v0.2 §B.2）。
///
/// 颜色取**界面令牌**而不是 [`crate::palette`] 的世界色：钻入是导航，属外壳语义
/// （`STYLE_SWAPPABLE_ARCHITECTURE` §2 的 L2/L1 分界；界面色不随 Theme Pack 变）。
pub fn draw_drill_markers(
    mut gizmos: Gizmos,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    shell_cfg: Res<UiShellConfig>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
) {
    let shell = crate::ui::UiShell::new(&shell_cfg);
    let accent = to_bevy_color(shell.accent());
    let visited = to_bevy_color(shell.t3());

    for marker in resolve_markers(&projection.world, &drill, &grid, view.scale) {
        let color = if let Some([r, g, b]) = marker.color {
            // 图标显式声明的颜色优先——作者定了色就按作者的来，已访问态也不覆盖它。
            Color::srgb_u8(r, g, b)
        } else if let Some(target) = &marker.target {
            if drill.is_visited(target) {
                visited
            } else {
                accent
            }
        } else {
            // 纯注释标记不抢导航语义：与已访问态同一个灰。
            visited
        };
        draw_shape(&mut gizmos, marker.center, marker.half, marker.shape, color);
        if marker.dot {
            // 建筑级多一个中心实心点：菱形在缩小后与城市级难分辨（§7 非颜色依赖）。
            gizmos.circle_2d(
                Isometry2d::from_translation(marker.center),
                marker.half * 0.36,
                color,
            );
        }
    }
}

/// 地点名称标注（矢量图层的一部分）。
///
/// 用**对象池**：每帧把池内实体的位置/文案对齐到「当前地图的锚定地点」，
/// 多余的隐藏、不够的补生——避免每帧 spawn/despawn 抖动（与纹理增量更新同一哲学）。
#[derive(Component)]
pub struct PlaceLabel;

/// 地名文字的屏幕像素基准（v0.2 §B.2 的 12px 标记同量级，不抢标记视觉）。
const LABEL_PX: f32 = 12.0;
/// 地名标注层：高于势力面（0.3）与 POI（0.22），低于笔刷光标（0.9）。
const LABEL_Z: f32 = 0.4;

/// 当前 LOD 下是否保留地名标注。
///
/// Google 式地图在国家级视野只给“城市级”地点留标签；渡口、关隘等点状小地点
/// 等用户拉近后再出现，避免世界图被一屏小字淹没。
fn place_label_kept_at_tier(w: Option<f32>, h: Option<f32>, tier: LodTier) -> bool {
    tier != LodTier::Nation || matches!((w, h), (Some(_), Some(_)))
}

/// 更新当前层级的地名标注池。
#[allow(clippy::type_complexity)]
pub fn update_place_labels(
    mut commands: Commands,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    fonts: Res<crate::fonts::LoadedFonts>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut Text2d,
            &mut TextFont,
            &mut TextColor,
            &mut Visibility,
        ),
        With<PlaceLabel>,
    >,
) {
    // 期望标注集：锚定在当前地图、坐标齐备的地点（与道路层同一可见口径）。
    let tier = lod_tier(view.scale);
    let mut wanted: Vec<(Vec2, &str)> = Vec::new();
    if let Some(current) = drill.current_node(&projection.world) {
        for p in &projection.world.distances.places {
            if p.map.as_ref() == Some(&current.id) && place_label_kept_at_tier(p.w, p.h, tier) {
                if let (Some(x), Some(y)) = (p.x, p.y) {
                    // 地点坐标是本图局部 → 视图空间（`map_to_view`，非栅格原点）。
                    wanted.push((grid.map_to_view(Vec2::new(x, y)), p.name.as_str()));
                }
            }
        }
    }

    // 文字尺寸按屏幕像素恒定（与 draw_drill_markers 的 screen_scale 口径一致）。
    let font_size = LABEL_PX * view.scale.max(0.05);
    // Text2d 默认以文字中心为 transform 原点；上移约一个字高，避免压住 POI。
    let label_offset = font_size * 0.9;
    for (pos, _) in &mut wanted {
        pos.y += label_offset;
    }
    // CJK 字体（fonts.rs 在 Startup 注册）：缺省字体无中文字形，地名会成方块。
    let cjk = fonts.cjk_font.clone();
    // 文字色按视图模式走 palette：Google Default Map 用深灰标签；Text2d
    // 光晕描边需自定义 shader，后续再补白 halo。
    let label_color = match view.mode {
        crate::state::ViewMode::GoogleMap => crate::palette_google::GOOGLE_LABEL_FILL,
        _ => bevy::color::Color::WHITE,
    };

    let mut iter = q.iter_mut();
    for (pos, name) in &wanted {
        if let Some((_, mut tf, mut text, mut style, mut color, mut vis)) = iter.next() {
            tf.translation = pos.extend(LABEL_Z);
            // Bevy 0.19：字号是 `TextFont.font_size: FontSize`、字体是 `font: FontSource`。
            *text = Text2d::new(*name);
            style.font_size = font_size.into();
            // TextFont.font 非 Option：缺 CJK 时回落内置字体（拉丁可用，中文方块）。
            if let Some(f) = &cjk {
                style.font = f.clone().into();
            }
            *color = TextColor(label_color);
            *vis = Visibility::Visible;
        } else {
            commands.spawn((
                PlaceLabel,
                Text2d::new(*name),
                TextFont {
                    font_size: font_size.into(),
                    font: cjk.clone().unwrap_or_default().into(),
                    ..Default::default()
                },
                TextColor(label_color),
                Transform::from_translation(pos.extend(LABEL_Z)),
                Visibility::Visible,
            ));
        }
    }
    // 池里多出来的实体隐藏（保留复用，不 despawn）。
    for (_, mut _tf, mut _text, mut _style, mut _color, mut vis) in iter {
        *vis = Visibility::Hidden;
    }
}

/// 按形状分发绘制。五种形状共用同一套「中心 + 半边长」口径——命中测试不关心形状。
fn draw_shape(gizmos: &mut Gizmos, center: Vec2, half: f32, shape: IconShape, color: Color) {
    match shape {
        IconShape::Diamond => draw_diamond(gizmos, center, half, color),
        IconShape::Circle => {
            gizmos.circle_2d(Isometry2d::from_translation(center), half, color);
        }
        IconShape::Square => {
            gizmos.rect_2d(
                Isometry2d::from_translation(center),
                Vec2::splat(half * 2.0),
                color,
            );
        }
        IconShape::Cross => {
            gizmos.line_2d(
                center + Vec2::new(-half, 0.0),
                center + Vec2::new(half, 0.0),
                color,
            );
            gizmos.line_2d(
                center + Vec2::new(0.0, -half),
                center + Vec2::new(0.0, half),
                color,
            );
        }
        // 四角星：上下左右四个外点 + 四个内点（缩到 0.4 倍），首尾相接。
        IconShape::Star => {
            let i = half * 0.4;
            let v = [
                center + Vec2::new(0.0, half),
                center + Vec2::new(i, -i),
                center + Vec2::new(half, 0.0),
                center + Vec2::new(i, i),
                center + Vec2::new(0.0, -half),
                center + Vec2::new(-i, i),
                center + Vec2::new(-half, 0.0),
                center + Vec2::new(-i, -i),
            ];
            for n in 0..v.len() {
                gizmos.line_2d(v[n], v[(n + 1) % v.len()], color);
            }
        }
    }
}

/// 四个顶点连成一个菱形。
fn draw_diamond(gizmos: &mut Gizmos, center: Vec2, half: f32, color: Color) {
    let top = center + Vec2::new(0.0, half);
    let right = center + Vec2::new(half, 0.0);
    let bottom = center - Vec2::new(0.0, half);
    let left = center - Vec2::new(half, 0.0);
    for (a, b) in [(top, right), (right, bottom), (bottom, left), (left, top)] {
        gizmos.line_2d(a, b, color);
    }
}

/// `ui_tokens::Rgb` → Bevy `Color`（gizmo 用的世界色）。
fn to_bevy_color(c: bevy_egui::egui::Color32) -> Color {
    Color::srgb_u8(c.r(), c.g(), c.b())
}

/// 当前主题下的界面强调色（供 [`draw_drill_markers`] 之外的调用者复用）。
pub fn accent_for(theme: UiTheme) -> Color {
    let c = crate::ui_tokens::ShellPalette::for_theme(theme)
        .accent
        .to_color32();
    to_bevy_color(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    use wv_core::World;

    #[test]
    fn nation_tier_keeps_only_city_labels() {
        assert!(place_label_kept_at_tier(
            Some(12.0),
            Some(8.0),
            LodTier::Nation
        ));
        assert!(!place_label_kept_at_tier(None, None, LodTier::Nation));
        assert!(!place_label_kept_at_tier(Some(12.0), None, LodTier::Nation));
        assert!(place_label_kept_at_tier(None, None, LodTier::Region));
        assert!(place_label_kept_at_tier(None, None, LodTier::Edit));
    }

    fn node(id: &str, level: MapLevel, parent: Option<&str>, children: &[&str]) -> MapNode {
        node_with_icons(id, level, parent, children, &[])
    }

    /// 同上，但可指定 `extent`——用于验证「子图落点由父图钻入图标派生」。
    fn node_of_size(
        id: &str,
        level: MapLevel,
        parent: Option<&str>,
        children: &[&str],
        icons: &[IconSpec],
        extent: Option<Extent>,
    ) -> MapNode {
        MapNode {
            orientation: 0.0,
            extent,
            ..node_with_icons(id, level, parent, children, icons)
        }
    }

    /// 同上，但父图带显式图标——用于验证 `anchor` / `shape` / `size` / `color` 的落点。
    fn node_with_icons(
        id: &str,
        level: MapLevel,
        parent: Option<&str>,
        children: &[&str],
        icons: &[IconSpec],
    ) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            level,
            parent: parent.map(Id::new_unchecked),
            children: children.iter().map(|c| Id::new_unchecked(*c)).collect(),
            layers: Vec::new(),
            icons: icons.to_vec(),
            extent: Some(Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        }
    }

    /// 一个显式图标的最小构造器（只填必要的字段，其余取默认）。
    fn icon(id: &str, anchor: Option<[f32; 2]>, drill_into: Option<&str>) -> IconSpec {
        IconSpec {
            id: Id::new_unchecked(id),
            anchor,
            shape: IconShape::Diamond,
            size: None,
            color: None,
            drill_into: drill_into.map(Id::new_unchecked),
        }
    }

    fn world_with_hierarchy() -> World {
        World {
            maps: vec![
                node("world_main", MapLevel::World, None, &["city_a", "city_b"]),
                node("city_a", MapLevel::City, Some("world_main"), &["bld_x"]),
                node("city_b", MapLevel::City, Some("world_main"), &[]),
                node("bld_x", MapLevel::Interior, Some("city_a"), &[]),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn marker_placement_is_deterministic_and_inside_the_extent() {
        let id = Id::new_unchecked("city_a");
        let a = marker_local(Some(FALLBACK_EXTENT), &id);
        let b = marker_local(Some(FALLBACK_EXTENT), &id);
        assert_eq!(a, b, "同一 id 两次求值必须一致，否则标记每帧乱跳");
        assert!(a.x > 0.0 && a.x < 4096.0, "x 越界: {}", a.x);
        assert!(a.y > 0.0 && a.y < 4096.0, "y 越界: {}", a.y);
    }

    #[test]
    fn different_children_land_on_different_spots() {
        let a = marker_local(Some(FALLBACK_EXTENT), &Id::new_unchecked("city_a"));
        let b = marker_local(Some(FALLBACK_EXTENT), &Id::new_unchecked("city_b"));
        assert!(a.distance(b) > 100.0, "两个城市几乎重叠: {} vs {}", a, b);
    }

    #[test]
    fn marker_placement_survives_a_missing_extent() {
        // 数据缺 `extent` 时不能 panic，也不能把所有标记叠在原点。
        let a = marker_local(None, &Id::new_unchecked("city_a"));
        assert!(a.x > 0.0 && a.y > 0.0, "缺 extent 时落到了边界外: {}", a);
    }

    #[test]
    fn interior_markers_are_smaller_than_city_markers() {
        let city = marker_half(8.0, MapLevel::City);
        let interior = marker_half(8.0, MapLevel::Interior);
        assert!(interior < city, "建筑级应更小: {interior} vs {city}");
        // 8 世界单位/格时 14px → 7 世界单位半边。
        assert!((city - 7.0).abs() < 1e-3, "城市菱形半边 {} ≠ 7", city);
    }

    #[test]
    fn explicit_anchor_places_the_marker_where_the_author_wrote_it() {
        // 归一化锚点：0.25 / 0.5 落在 1000×500 的图上就是 (250, 250)，不加 12% 边距。
        let at = icon_local(
            Some(Extent::sized(1000.0, 500.0)),
            &icon("icon_a", Some([0.25, 0.5]), None),
        );
        assert!(
            (at.x - 250.0).abs() < 1e-3 && (at.y - 250.0).abs() < 1e-3,
            "anchor [0.25, 0.5] 应落在 (250, 250)；实得 {at}"
        );
    }

    #[test]
    fn anchor_outside_the_unit_square_is_clamped_not_rejected() {
        // 越界值夹回 [0,1]，不 panic、不拒收（AGENTS §7.5V1）。
        let at = icon_local(None, &icon("icon_bad", Some([1.5, -0.3]), None));
        assert!(
            (at.x - FALLBACK_EXTENT.width).abs() < 1e-3 && at.y.abs() < 1e-3,
            "越界锚点应夹到边界；实得 {at}"
        );
    }

    #[test]
    fn icon_without_anchor_falls_back_to_the_deterministic_hash() {
        let a = icon_local(Some(FALLBACK_EXTENT), &icon("icon_b", None, None));
        let b = icon_local(Some(FALLBACK_EXTENT), &icon("icon_b", None, None));
        assert_eq!(a, b, "同一图标两次求值必须一致，否则标记每帧乱跳");
        assert!(
            a.x > 0.0 && a.x < 4096.0 && a.y > 0.0 && a.y < 4096.0,
            "回退摆位越界: {a}"
        );
    }

    #[test]
    fn size_override_wins_over_the_level_default() {
        // 作者写 size: 40 就是 40px → 20px 半边 → cell_size 8 时 20 世界单位。
        let mut spec = icon("icon_city_a", Some([0.5, 0.5]), Some("city_a"));
        spec.size = Some(40.0);
        let world = World {
            maps: vec![
                node_with_icons("world_main", MapLevel::World, None, &["city_a"], &[spec]),
                node("city_a", MapLevel::City, Some("world_main"), &[]),
            ],
            ..Default::default()
        };
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let grid = GridConfig::default();
        let markers = resolve_markers(&world, &drill, &grid, 1.0);
        assert_eq!(markers.len(), 1, "city_a 已被图标覆盖，不应再出现哈希标记");
        assert!(
            (markers[0].half - 20.0).abs() < 1e-3,
            "size: 40 应给出 20 世界单位半边（cell_size 8）；实得 {}",
            markers[0].half
        );
        assert!(
            markers[0].half > marker_half(grid.cell_size, MapLevel::City),
            "显式尺寸应覆盖层级缺省"
        );
    }

    #[test]
    fn annotation_icon_is_drawn_but_not_clickable() {
        // 无 `drill_into` 的图标是作者的批注：要画出来，但不能双击钻入。
        let world = World {
            maps: vec![node_with_icons(
                "world_main",
                MapLevel::World,
                None,
                &[],
                &[icon("icon_note", Some([0.5, 0.5]), None)],
            )],
            ..Default::default()
        };
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let grid = GridConfig::default();
        let markers = resolve_markers(&world, &drill, &grid, 1.0);
        assert_eq!(markers.len(), 1, "纯注释标记也要画出来");
        assert!(markers[0].target.is_none());
        assert!(
            marker_hit(&world, &drill, &grid, markers[0].center, 1.0).is_none(),
            "纯注释标记必须被命中测试跳过"
        );
    }

    #[test]
    fn a_child_without_an_icon_still_gets_a_marker() {
        // 父图只给 city_a 写了图标——city_b 仍必须有入口（哈希回退）。
        let world = World {
            maps: vec![
                node_with_icons(
                    "world_main",
                    MapLevel::World,
                    None,
                    &["city_a", "city_b"],
                    &[icon("icon_city_a", Some([0.2, 0.2]), Some("city_a"))],
                ),
                node("city_a", MapLevel::City, Some("world_main"), &[]),
                node("city_b", MapLevel::City, Some("world_main"), &[]),
            ],
            ..Default::default()
        };
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let markers = resolve_markers(&world, &drill, &GridConfig::default(), 1.0);
        assert_eq!(markers.len(), 2, "一个显式图标 + 一个哈希回退标记");
        let targets: Vec<String> = markers
            .iter()
            .map(|m| {
                m.target
                    .clone()
                    .expect("这里两个标记都应可钻入")
                    .to_string()
            })
            .collect();
        assert!(targets.iter().any(|t| t == "city_a"), "city_a: {targets:?}");
        assert!(targets.iter().any(|t| t == "city_b"), "city_b: {targets:?}");
    }

    #[test]
    fn explicit_shape_and_color_survive_resolution() {
        let mut spec = icon("icon_port", Some([0.4, 0.6]), Some("city_a"));
        spec.shape = IconShape::Star;
        spec.color = Some(wv_core::color::Color("#FF6B35".into()));
        let world = World {
            maps: vec![
                node_with_icons("world_main", MapLevel::World, None, &["city_a"], &[spec]),
                node("city_a", MapLevel::City, Some("world_main"), &[]),
            ],
            ..Default::default()
        };
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let markers = resolve_markers(&world, &drill, &GridConfig::default(), 1.0);
        assert_eq!(markers[0].shape, IconShape::Star, "形状必须原样传到渲染层");
        assert_eq!(
            markers[0].color,
            Some([0xFF, 0x6B, 0x35]),
            "显式颜色必须原样传到渲染层，由渲染层优先于主题色"
        );
    }

    #[test]
    fn drill_target_follows_the_icon_anchor_not_the_hash() {
        // 「双击的落点」与「钻入后相机的落点」必须走同一条路径，否则画面会跳。
        let world = World {
            maps: vec![
                node_with_icons(
                    "world_main",
                    MapLevel::World,
                    None,
                    &["city_a"],
                    &[icon("icon_city_a", Some([0.3, 0.4]), Some("city_a"))],
                ),
                node("city_a", MapLevel::City, Some("world_main"), &[]),
            ],
            ..Default::default()
        };
        let grid = GridConfig::default();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let marker = &resolve_markers(&world, &drill, &grid, 1.0)[0];
        assert!(drill.enter(&world, &Id::new_unchecked("city_a")));
        let got = drill_target(&world, &drill, &grid);
        assert!(
            got.distance(marker.center) < 1e-3,
            "相机落点 {got} 必须与双击时看到的标记位置一致"
        );
        // 反证：若与哈希回退位置重合，说明图标锚点根本没生效。
        let hash =
            grid.raster_origin + marker_local(Some(FALLBACK_EXTENT), &Id::new_unchecked("city_a"));
        assert!(
            got.distance(hash) > 1.0,
            "相机落点与哈希位置重合，图标锚点未生效"
        );
    }

    #[test]
    fn city_markers_land_in_the_citys_own_region_of_the_world_frame() {
        // 回归（本轮修复的可见 bug）：世界图 4096² 上用 anchor [0.5,0.5] 标记某城，
        // 城自己的图是 1024² 且**没有**声明 origin。旧实现把城市图的图标按 1024²
        // 归一化后直接画在世界帧的 (512,512)——即左上四分之一处。正确落点是
        // 世界图上那个图标的位置 (2048,2048)：图标标记的是城市的**中心**，
        // 城市图左上角因此落在 (2048-512, 2048-512) = (1536,1536)。
        let world = World {
            maps: vec![
                node_with_icons(
                    "world_main",
                    MapLevel::World,
                    None,
                    &["city_wuhan"],
                    &[icon("icon_wuhan", Some([0.5, 0.5]), Some("city_wuhan"))],
                ),
                node_of_size(
                    "city_wuhan",
                    MapLevel::City,
                    Some("world_main"),
                    &[],
                    &[icon("icon_hankou", Some([0.5, 0.5]), None)],
                    Some(Extent::sized(1024.0, 1024.0)),
                ),
            ],
            ..Default::default()
        };
        let grid = GridConfig::default();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);

        // 世界级：城市图标按**世界** extent(4096) 归一化 → (2048,2048)。
        let world_marker = &resolve_markers(&world, &drill, &grid, 1.0)[0];
        assert!(
            world_marker.center.distance(Vec2::new(2048.0, 2048.0)) < 1e-3,
            "世界级图标应落在 (2048,2048)；实得 {}",
            world_marker.center
        );

        assert!(
            drill.enter(&world, &Id::new_unchecked("city_wuhan")),
            "应当能钻入城市"
        );

        // 城市级：图标按**城市** extent(1024) 归一化，但原点必须上溯到城市在世界里的
        // 位置 (1536,1536)，而不是栅格原点 (0,0) → (1536+512, 1536+512)。
        let city_marker = &resolve_markers(&world, &drill, &grid, 1.0)[0];
        assert!(
            city_marker.center.distance(Vec2::new(2048.0, 2048.0)) < 1e-3,
            "城市级图标应落在城市中心 (2048,2048)；实得 {}（旧实现在 (512,512)）",
            city_marker.center
        );

        // 钻入落点 = 城市中心，与图标位置一致：双击看到哪，钻进去就停在哪。
        let target = drill_target(&world, &drill, &grid);
        assert!(
            target.distance(Vec2::new(2048.0, 2048.0)) < 1e-3,
            "钻入落点应为城市中心 (2048,2048)；实得 {target}"
        );
    }

    #[test]
    fn hit_test_finds_the_marker_under_the_cursor() {
        let world = world_with_hierarchy();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let grid = GridConfig::default();
        let markers = resolve_markers(&world, &drill, &grid, 1.0);
        assert_eq!(markers.len(), 2, "世界级应有 2 个可钻入城市");
        let hit = marker_hit(&world, &drill, &grid, markers[0].center, 1.0);
        assert_eq!(hit, markers[0].target, "正中心必须命中");
        // 远离所有标记时不应命中。
        assert!(marker_hit(
            &world,
            &drill,
            &grid,
            markers[0].center + Vec2::splat(900.0),
            1.0
        )
        .is_none());
    }

    #[test]
    fn drill_target_moves_to_the_parents_marker_and_not_to_the_hash() {
        let world = world_with_hierarchy();
        let grid = GridConfig::default();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);

        // 落点 = 该城市在父图里的标记位置（不跳到无关坐标）。
        assert!(drill.enter(&world, &Id::new_unchecked("city_a")));
        let parent = world.maps.iter().find(|m| m.id.as_str() == "world_main");
        let expect = grid.raster_origin
            + marker_local(parent.and_then(|p| p.extent), &Id::new_unchecked("city_a"));
        assert!(drill_target(&world, &drill, &grid).distance(expect) < 1e-3);
    }

    /// 过渡的目标缩放必须落在**新一级**的区间内：历史上硬编码的 `4.0` 在
    /// 一座 1024 的城市（区间 `[0.016, 2.0]`）里是越界的，进城后第一次滚轮
    /// 会被 clamp 硬拽一下，LOD / 比例尺读数也跟着矛盾。
    #[test]
    fn drill_fly_scale_stays_inside_the_child_levels_band() {
        let window = Vec2::new(1280.0, 800.0);
        let world = World {
            maps: vec![
                node_with_icons(
                    "world_main",
                    MapLevel::World,
                    None,
                    &["city_a"],
                    &[icon("icon_city_a", Some([0.5, 0.5]), Some("city_a"))],
                ),
                node_of_size(
                    "city_a",
                    MapLevel::City,
                    Some("world_main"),
                    &[],
                    &[],
                    Some(Extent::sized(1024.0, 1024.0)),
                ),
            ],
            ..Default::default()
        };
        let mut drill = DrillState::default();
        drill.ensure_root(&world);

        // 根级保持历史行为：回世界时缩放到 1.0。
        assert_eq!(drill_fly_scale(&world, &drill, window, 5.12), 1.0);

        assert!(drill.enter(&world, &Id::new_unchecked("city_a")));
        let got = drill_fly_scale(&world, &drill, window, 5.12);
        let band = crate::zoom_level::LevelZoom::for_map(Vec2::splat(1024.0), window);
        assert!(
            (band.min..=band.max).contains(&got),
            "目标缩放 {got} 必须落在城市级区间 [{}, {}] 内",
            band.min,
            band.max
        );
        assert!(
            (got - band.fit).abs() < 1e-4,
            "应把整座城放进窗口，got {got}"
        );
    }

    #[test]
    fn nested_levels_are_enterable_and_step_back() {
        let world = world_with_hierarchy();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        // 越级（直接进孙节点）必须被拒：路径与 children 引用的一致性。
        assert!(!drill.enter(&world, &Id::new_unchecked("bld_x")));
        assert!(drill.enter(&world, &Id::new_unchecked("city_a")));
        assert!(drill.enter(&world, &Id::new_unchecked("bld_x")));
        assert_eq!(drill.depth(), 2);
        assert!(drill.up());
        assert_eq!(drill.depth(), 1);
        assert!(drill.up());
        assert_eq!(drill.depth(), 0);
        assert!(!drill.up(), "已到世界级，不能再钻出");
    }

    #[test]
    fn ease_out_cubic_hits_both_ends() {
        assert!((ease_out_cubic(0.0)).abs() < 1e-6);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
        // 缓出：过半时应该已经走完一半以上的路程。
        assert!(ease_out_cubic(0.5) > 0.5, "缓出曲线前半段应更快");
    }

    #[test]
    fn accent_differs_between_themes_but_is_never_gold() {
        let dark = accent_for(UiTheme::Dark);
        let light = accent_for(UiTheme::Light);
        assert_ne!(dark, light, "两套主题的界面蓝应不同");
        // 金色（选中地块语义）绝不能出现在钻入标记上（v0.2 §B.2 明令）。
        let gold = Color::srgb_u8(0xF2, 0xC1, 0x4E);
        assert_ne!(dark, gold);
        assert_ne!(light, gold);
    }

    // -----------------------------------------------------------------------
    // 滚轮换级（ADR-0009 §2 决策 ⑤/⑦）：`camera_zoom` 只发请求，这里消费。
    // -----------------------------------------------------------------------

    /// 两级世界：世界 4096²、城市 1024²（城市区间整体位于世界区间内侧）。
    fn two_level_world() -> World {
        World {
            maps: vec![
                node_of_size(
                    "world_main",
                    MapLevel::World,
                    None,
                    &["city_a"],
                    &[],
                    Some(Extent::sized(4096.0, 4096.0)),
                ),
                node_of_size(
                    "city_a",
                    MapLevel::City,
                    Some("world_main"),
                    &[],
                    &[],
                    Some(Extent::sized(1024.0, 1024.0)),
                ),
            ],
            ..Default::default()
        }
    }

    /// 只装 `apply_zoom_cross` 的最小 App：资源齐全 + 一台相机 + 一个窗口。
    fn zoom_cross_app(world: World) -> App {
        let mut app = App::new();
        app.init_resource::<DrillState>()
            .init_resource::<ViewState>()
            .init_resource::<GridConfig>()
            .init_resource::<crate::zoom_level::ZoomCrossRequest>()
            .insert_resource(WorldProjection {
                world,
                ..Default::default()
            })
            .add_systems(Update, apply_zoom_cross);
        app.world_mut().spawn((
            Camera2d,
            Transform::default(),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));
        app.world_mut().spawn(Window::default());
        app
    }

    /// 滚轮往外换级：回到父级，且**缩放与相机中心一个像素都不动**
    /// （连续性来自「换级不改 scale」，见 `zoom_level.rs`），
    /// 同时吃掉深度变化标记，避免 `drill_transition` 再飞一次。
    #[test]
    fn zoom_cross_out_steps_up_a_level_and_leaves_the_camera_alone() {
        let mut app = zoom_cross_app(two_level_world());
        {
            let world = app.world().resource::<WorldProjection>().world.clone();
            let mut drill = app.world_mut().resource_mut::<DrillState>();
            drill.ensure_root(&world);
            assert!(
                drill.enter(&world, &Id::new_unchecked("city_a")),
                "前置条件：应先站在城市级"
            );
            assert_eq!(drill.depth(), 1);
        }
        app.world_mut().resource_mut::<ViewState>().scale = 1.0;
        app.world_mut().resource_mut::<ViewState>().camera_center = Vec2::new(1500.0, 1200.0);
        app.world_mut()
            .resource_mut::<crate::zoom_level::ZoomCrossRequest>()
            .pending = Some(crate::zoom_level::ZoomCross::Out);

        app.update();

        let drill = app.world().resource::<DrillState>();
        assert_eq!(drill.depth(), 0, "应回到世界级");
        assert_eq!(drill.current(), Some(&Id::new_unchecked("world_main")));
        let view = app.world().resource::<ViewState>();
        assert!(
            (view.scale - 1.0).abs() < 1e-6,
            "换级不得改缩放，got {}",
            view.scale
        );
        assert_eq!(
            view.camera_center,
            Vec2::new(1500.0, 1200.0),
            "换级不得移动相机中心"
        );
        assert!(
            !app.world_mut()
                .resource_mut::<DrillState>()
                .take_depth_change(),
            "换级后不该给 drill_transition 留下深度变化标记"
        );
    }

    /// 滚轮往里换级：进入到请求指定的子图，缩放同样不动。
    #[test]
    fn zoom_cross_into_enters_the_requested_child() {
        let mut app = zoom_cross_app(two_level_world());
        {
            let world = app.world().resource::<WorldProjection>().world.clone();
            app.world_mut()
                .resource_mut::<DrillState>()
                .ensure_root(&world);
        }
        app.world_mut().resource_mut::<ViewState>().scale = 1.0;
        app.world_mut()
            .resource_mut::<crate::zoom_level::ZoomCrossRequest>()
            .pending = Some(crate::zoom_level::ZoomCross::Into(Id::new_unchecked(
            "city_a",
        )));

        app.update();

        let drill = app.world().resource::<DrillState>();
        assert_eq!(drill.depth(), 1, "应进入城市级");
        assert_eq!(drill.current(), Some(&Id::new_unchecked("city_a")));
        let view = app.world().resource::<ViewState>();
        assert!(
            (view.scale - 1.0).abs() < 1e-6,
            "换级不得改缩放，got {}",
            view.scale
        );
    }

    /// 根级再往外没有父级：请求被吃掉、状态不变，不留悬挂请求
    /// （否则下一次往外滚动会莫名其妙地生效）。
    #[test]
    fn zoom_cross_out_at_the_root_is_consumed_and_ignored() {
        let mut app = zoom_cross_app(two_level_world());
        {
            let world = app.world().resource::<WorldProjection>().world.clone();
            app.world_mut()
                .resource_mut::<DrillState>()
                .ensure_root(&world);
        }
        app.world_mut()
            .resource_mut::<crate::zoom_level::ZoomCrossRequest>()
            .pending = Some(crate::zoom_level::ZoomCross::Out);

        app.update();

        assert_eq!(app.world().resource::<DrillState>().depth(), 0);
        assert!(
            app.world()
                .resource::<crate::zoom_level::ZoomCrossRequest>()
                .pending
                .is_none(),
            "无论是否真的换级，请求都必须被消费"
        );
    }

    /// **端到端**：真滚一次滚轮（`camera_zoom` 生产请求 + `apply_zoom_cross` 消费）
    /// → 真的从世界级进到城市级。
    ///
    /// 为什么必须在这里钉住：合成鼠标输入进不了 winit（实测 `SendInput` 双击
    /// 无法触发钻取），所以「滚轮换级」这条核心行为在 GUI 里无法自动化验证，
    /// 只能让生产端与消费端在同一 App 里真跑一遍。
    #[test]
    fn a_real_wheel_event_past_the_inner_limit_enters_the_child_level() {
        let mut world = two_level_world();
        // 世界图上给城市一个入口图标，摆在正中 (2048,2048)。
        if let Some(node) = world
            .maps
            .iter_mut()
            .find(|m| m.id.as_str() == "world_main")
        {
            node.icons
                .push(icon("icon_city_a", Some([0.5, 0.5]), Some("city_a")));
        }

        let mut app = App::new();
        app.init_resource::<DrillState>()
            .init_resource::<ViewState>()
            .init_resource::<GridConfig>()
            .init_resource::<crate::zoom_level::ZoomCrossRequest>()
            .init_resource::<crate::camera::PointerOverUi>()
            .add_message::<MouseWheel>()
            .insert_resource(WorldProjection {
                world,
                ..Default::default()
            })
            .add_systems(
                Update,
                (crate::camera::camera_zoom, apply_zoom_cross).chain(),
            );

        let world = app.world().resource::<WorldProjection>().world.clone();
        app.world_mut()
            .resource_mut::<DrillState>()
            .ensure_root(&world);
        // 相机停在城市图标上；指针在窗口正中，于是「指针处的世界坐标」也是 (2048,2048)。
        app.world_mut().spawn((
            Camera2d,
            Transform::from_xyz(2048.0, 2048.0, 0.0),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));
        let mut window = Window::default();
        window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));
        let window_id = app.world_mut().spawn(window).id();

        // 先把缩放踩到世界级内界。
        let inner = crate::zoom_level::level_zoom(
            &world,
            Some(&Id::new_unchecked("world_main")),
            Vec2::new(1280.0, 720.0),
            Vec2::splat(crate::zoom_level::FALLBACK_MAP_SIZE),
        )
        .min;
        app.world_mut().resource_mut::<ViewState>().scale = inner;

        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: -1.0,
            window: window_id,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();

        let drill = app.world().resource::<DrillState>();
        assert_eq!(
            drill.current(),
            Some(&Id::new_unchecked("city_a")),
            "内界上继续往里滚，应真的进入城市级"
        );
        assert_eq!(drill.depth(), 1);
    }
}
