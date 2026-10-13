//! 坐标变换链（`docs/MAP_EDITOR_BASELINE_v1.md` §2.15 的**唯一**实现点）。
//!
//! ## 为什么必须集中在一个模块
//!
//! 基线 §2.15 要求「一个 `coord` 模块集中实现两个方向的变换，
//! **禁止多 System 各写一遍**」。散落实现会立刻分叉，本项目已经分叉过一次：
//! 地形外框按**根图** `extent` 绘制（`map_frame.rs`），而图标按**当前图**
//! `extent` 归一化摆位（`drill.rs`）——两套口径不同源，于是 1024×1024 的城市图
//! 上声明 `anchor: [0.5, 0.5]` 的图标，会落在 4096×4096 世界帧的左上四分之一处。
//! 本模块把「图内局部坐标 → 世界坐标」的唯一算法固定下来。
//!
//! ## 三个坐标系
//!
//! | 坐标系 | 用途 | 定义 |
//! |---|---|---|
//! | 世界坐标 | 全局唯一参考系 | 一个工程一个，原点 `(0, 0)`；各图用 [`Extent::origin`] 声明自己在父图中的位置 |
//! | 图内局部坐标 | 单张图的内部绘制 | [`crate::map::GeometryData`] 的点列、[`crate::track::Waypoint::pos`] |
//! | 图内归一化坐标（UV） | 栅格采样、图标锚点 | `[0,1]²`，左上角为原点、y 向下 |
//!
//! ## 变换链
//!
//! ```text
//! 图内局部坐标
//!   ↓ + extent.origin，逐层上溯
//! 世界坐标
//!   ↓ × camera.scale
//! 相机视图坐标
//!   ↓ + camera.translation
//! 屏幕像素坐标
//! ```
//!
//! 本模块只负责**前半段**（局部 ↔ 世界 ↔ UV）。它是纯数据层的职责，
//! 因此零依赖、可独立测试（`AGENTS.md` §2.1）。后半段（世界 → 屏幕）归
//! `wv-app` 的相机，因为那里涉及渲染状态与窗口尺寸。
//!
//! ## 落点从哪来：声明优先，图标派生兜底
//!
//! [`Extent::origin`] 是 `Option`：作者没写 `origin` 时，本模块用
//! **父图上指向该子图的钻入图标**（`IconSpec::drill_into`）反推落点——
//! 图标标记的是子图**中心**，故落点 = 图标点 − 子图 extent/2。
//! 理由见 [`resolve_frame_in`]；只用 [`resolve_frame`] 的调用方拿到的是
//! **纯声明值**，不含派生。
//!
//! ## 地面尺度不在这里
//!
//! 「一格等于多少公里」不在本模块，也不在 [`Extent`] 里——
//! 那是 2026-10-12 用户裁定（甲）撤除 `width_km` 时定下的边界
//! （见 [`Extent`] 文档）。真实地图的米制尺度应由 `geographic_source.bbox`
//! 派生（ADR-0007），派生点属于**几何/地理**职责，不是坐标变换职责。

use crate::geometry::Point;
use crate::id::Id;
use crate::map::{Extent, MapNode};

/// 参与坐标链的**最小视图**：一张图只需「父 ID + 范围」就能参与变换。
///
/// 之所以不直接吃 [`MapNode`]：坐标算法不该依赖图标的形状、图层开关等
/// 与本职责无关的字段，测试也就不必为了验证一个加法而构造整张地图。
#[derive(Debug, Clone, PartialEq)]
pub struct FrameNode {
    /// 父地图 ID。世界层为 `None`。
    pub parent: Option<Id>,
    /// 本图范围（含 `origin`）。未声明 `extent` 的图用 [`Extent::ZERO`]。
    pub extent: Extent,
}

impl MapNode {
    /// 本图对坐标链的贡献。
    ///
    /// 未声明 `extent` 时取 [`Extent::ZERO`]：位置仍可还原（`origin` 为 0），
    /// 但 UV 归一化退化——调用方若要摆位应回退到哈希摆位
    /// （口径同 [`crate::map::IconSpec::anchor`] 的缺省行为）。
    pub fn frame_node(&self) -> FrameNode {
        FrameNode {
            parent: self.parent.clone(),
            extent: self.extent.unwrap_or(Extent::ZERO),
        }
    }
}

/// 一张图在世界坐标中的**还原帧**：逐层上溯的结果。
///
/// 把上溯**预先算好**而不是每次换算都爬一遍父链——渲染是逐帧的，
/// 而层级树在一次编辑内基本不变。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapFrame {
    /// 本图左上角在**世界坐标**中的位置（已逐层累加）。
    pub world_origin: Point,
    /// 本图范围（局部坐标单位）。
    pub extent: Extent,
}

impl MapFrame {
    /// 图内局部坐标 → 世界坐标。
    pub fn local_to_world(&self, local: Point) -> Point {
        Point::new(self.world_origin.x + local.x, self.world_origin.y + local.y)
    }

    /// 世界坐标 → 图内局部坐标。
    pub fn world_to_local(&self, world: Point) -> Point {
        Point::new(world.x - self.world_origin.x, world.y - self.world_origin.y)
    }

    /// 归一化坐标（UV）→ 图内局部坐标。
    ///
    /// 全域函数：`extent` 未声明（宽高为 0）时退化为左上角点，不 panic。
    pub fn uv_to_local(&self, uv: [f32; 2]) -> Point {
        Point::new(uv[0] * self.extent.width, uv[1] * self.extent.height)
    }

    /// 图内局部坐标 → 归一化坐标（UV）。
    ///
    /// 返回 `None` 当 `extent` 未声明（宽或高为 0）——此时除法无意义，
    /// 与其返回 `NaN` 让错误在渲染层显形，不如在这里变成可判定的 `None`。
    pub fn local_to_uv(&self, local: Point) -> Option<[f32; 2]> {
        if self.extent.width == 0.0 || self.extent.height == 0.0 {
            return None;
        }
        Some([local.x / self.extent.width, local.y / self.extent.height])
    }

    /// 归一化坐标（UV）→ 世界坐标。
    ///
    /// 这是**图标摆位**的入口：`anchor` 按本图 `extent` 归一化，
    /// 加上本图在世界中的原点，就得到世界坐标——两套口径由此统一。
    pub fn uv_to_world(&self, uv: [f32; 2]) -> Point {
        self.local_to_world(self.uv_to_local(uv))
    }

    /// 世界坐标 → 归一化坐标（UV）。`extent` 未声明时返回 `None`。
    pub fn world_to_uv(&self, world: Point) -> Option<[f32; 2]> {
        self.local_to_uv(self.world_to_local(world))
    }

    /// 是否声明了可用的 `extent`（UV 归一化是否有效）。
    pub fn has_extent(&self) -> bool {
        self.extent.width != 0.0 && self.extent.height != 0.0
    }
}

/// 沿 `parent` 链把 `id` 上溯到世界坐标，返回它的还原帧。
///
/// `lookup` 只需回答「某个 ID 的父是谁、范围多大」——因此本函数对
/// [`MapNode`] 列表、稀疏索引、甚至测试里的临时表都一视同仁。
///
/// **只看声明值**：落点取 [`Extent::origin`]，未声明即 `(0, 0)`。
/// 需要「未声明时从父图钻入图标派生」的作者口径，用 [`resolve_frame_in`]。
///
/// ## 悬空与环的处理（`AGENTS.md` §7.5 V1：校验只提示，不阻止）
///
/// - **缺父**（`parent` 指向不存在的 ID）：停止上溯，已累加的 `origin` 照常返回。
///   悬空引用由校验面板负责提示，这里**不报错、不 panic**。
/// - **环**（A 的父是 B、B 的父是 A）：上溯遇到重复 ID 即停，不会死循环。
/// - **目标图不存在**：返回 `None`——这不是悬空引用，而是「问错了对象」。
pub fn resolve_frame<F>(id: &Id, lookup: F) -> Option<MapFrame>
where
    F: Fn(&Id) -> Option<FrameNode>,
{
    let mut world_origin = Point::ORIGIN;
    let mut extent: Option<Extent> = None;
    let mut current = id.clone();
    let mut seen: Vec<Id> = Vec::new();

    while !seen.contains(&current) {
        seen.push(current.clone());
        let Some(node) = lookup(&current) else { break };
        // 本图在父图里的落点，累加到世界原点上。
        let local = node.extent.origin_or_global();
        world_origin.x += local.x;
        world_origin.y += local.y;
        if extent.is_none() {
            extent = Some(node.extent);
        }
        let Some(parent) = node.parent else { break };
        current = parent;
    }

    Some(MapFrame {
        world_origin,
        extent: extent?,
    })
}

/// 在工程的全部地图节点里，还原 `id` 的世界坐标帧（**作者口径**）。
///
/// 与 [`resolve_frame`] 的唯一差别：本图未声明 [`Extent::origin`] 时，
/// 用父图上指向它的钻入图标反推落点。优先级：
///
/// 1. **显式 `origin`**（作者写了就以他为准——同一张子图可以有多个入口，
///    哪个是「正门」只有作者知道）；
/// 2. 父图 `icons` 里第一个 `drill_into == 本图` 且**显式声明了 `anchor`** 的图标；
/// 3. `(0, 0)`。
///
/// 第 2 条的依据：世界图上那个城市圆点，本来就是「这座城市在哪」的**唯一**
/// 书面表达——再要求作者把同一个位置写成子图的 `origin`，等于让他维护两份
/// 会各自漂移的数据（工程原则四：只存引用，不重复存储）。只有**显式 `anchor`**
/// 参与反推：哈希回退摆位是渲染层避免图标重叠的权宜，不是作者声明的位置。
///
/// 线性查找：地图数量是**作者手写的层级树**规模（几十张），
/// 不是街道路网那种量级，不值得为它引入索引结构。
pub fn resolve_frame_in(nodes: &[MapNode], id: &Id) -> Option<MapFrame> {
    let find = |query: &Id| nodes.iter().find(|node| &node.id == query);

    // 目标图必须存在——「问错了对象」要返回 None，而不是给出一个空帧。
    find(id)?;

    let mut world_origin = Point::ORIGIN;
    let mut extent: Option<Extent> = None;
    let mut current = id.clone();
    let mut seen: Vec<Id> = Vec::new();

    while !seen.contains(&current) {
        seen.push(current.clone());
        let Some(node) = find(&current) else { break };

        let local = node
            .extent
            .and_then(|extent| extent.origin)
            .unwrap_or_else(|| {
                node.parent
                    .as_ref()
                    .and_then(find)
                    .map(|parent| derived_origin(parent, node))
                    .unwrap_or(Point::ORIGIN)
            });
        world_origin.x += local.x;
        world_origin.y += local.y;
        if extent.is_none() {
            extent = Some(node.extent.unwrap_or(Extent::ZERO));
        }

        let Some(parent) = node.parent.clone() else {
            break;
        };
        current = parent;
    }

    Some(MapFrame {
        world_origin,
        extent: extent?,
    })
}

/// 子图未声明落点时，从**父图上指向它的钻入图标**派生。
///
/// 图标标记的是子图的**中心**（作者点那个城市圆点，说的是「城市在这里」，
/// 不是「城市的左上角在这里」），故落点 = 图标点 − 子图 extent/2。
/// 由此钻入时相机停在子图中心，与 `wv-app::drill::drill_target` 的计算
/// 天然一致（后者把相机放到父图上的图标点）。
///
/// 父图未声明 `extent` 时 `anchor` 无处归一化 → 退回 `(0, 0)`；
/// 缺失的 `extent` 由校验面板提示（§7.5 V1），这里不报错。
fn derived_origin(parent: &MapNode, child: &MapNode) -> Point {
    let Some(anchor) = parent
        .icons
        .iter()
        .find(|icon| icon.drill_into.as_ref() == Some(&child.id))
        .and_then(|icon| icon.anchor)
    else {
        return Point::ORIGIN;
    };
    let Some(parent_extent) = parent.extent else {
        return Point::ORIGIN;
    };

    let icon_point = Point::new(
        parent_extent.width * anchor[0].clamp(0.0, 1.0),
        parent_extent.height * anchor[1].clamp(0.0, 1.0),
    );
    let child_extent = child.extent.unwrap_or(Extent::ZERO);
    Point::new(
        icon_point.x - child_extent.width * 0.5,
        icon_point.y - child_extent.height * 0.5,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{IconShape, IconSpec, MapLevel};

    /// 构造坐标链节点：`(id, parent, width, height, origin)`，落点**已声明**。
    fn n(id: &str, parent: Option<&str>, size: (f32, f32), origin: (f32, f32)) -> (Id, FrameNode) {
        (
            Id::new_unchecked(id),
            FrameNode {
                parent: parent.map(Id::new_unchecked),
                extent: Extent::at(size.0, size.1, Point::new(origin.0, origin.1)),
            },
        )
    }

    /// 构造**未声明落点**的坐标链节点：`(id, parent, width, height)`。
    fn n_free(id: &str, parent: Option<&str>, size: (f32, f32)) -> (Id, FrameNode) {
        (
            Id::new_unchecked(id),
            FrameNode {
                parent: parent.map(Id::new_unchecked),
                extent: Extent::sized(size.0, size.1),
            },
        )
    }

    /// 构造地图节点：`(id, parent, level, extent, icons)`。
    fn map_node(
        id: &str,
        parent: Option<&str>,
        level: MapLevel,
        extent: Option<Extent>,
        icons: Vec<IconSpec>,
    ) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: id.to_string(),
            level,
            parent: parent.map(Id::new_unchecked),
            children: Vec::new(),
            icons,
            layers: Vec::new(),
            extent,
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        }
    }

    /// 构造钻入图标：`(id, anchor, drill_into)`。
    fn drill_icon(id: &str, anchor: Option<[f32; 2]>, into: &str) -> IconSpec {
        IconSpec {
            id: Id::new_unchecked(id),
            anchor,
            shape: IconShape::Diamond,
            size: None,
            color: None,
            drill_into: Some(Id::new_unchecked(into)),
        }
    }

    /// 在一张临时表上做查找。
    fn frame_of(nodes: &[(Id, FrameNode)], id: &str) -> Option<MapFrame> {
        let target = Id::new_unchecked(id);
        resolve_frame(&target, |query| {
            nodes
                .iter()
                .find(|(node_id, _)| node_id == query)
                .map(|(_, frame)| frame.clone())
        })
    }

    #[test]
    fn world_map_origin_is_the_global_origin() {
        let nodes = [n("map_world", None, (4096.0, 4096.0), (0.0, 0.0))];
        let frame = frame_of(&nodes, "map_world").expect("世界图存在");
        assert_eq!(frame.world_origin, Point::ORIGIN, "世界层原点应恒为 (0,0)");
    }

    #[test]
    fn child_origin_accumulates_through_the_parent_chain() {
        // 世界 4096² @(0,0) → 城市 1024² @(512,256) → 城区 100² @(10,20)
        let nodes = [
            n("map_world", None, (4096.0, 4096.0), (0.0, 0.0)),
            n(
                "map_city",
                Some("map_world"),
                (1024.0, 1024.0),
                (512.0, 256.0),
            ),
            n(
                "map_district",
                Some("map_city"),
                (100.0, 100.0),
                (10.0, 20.0),
            ),
        ];
        let frame = frame_of(&nodes, "map_district").expect("城区图存在");
        assert!(
            (frame.world_origin.x - 522.0).abs() < 1e-4
                && (frame.world_origin.y - 276.0).abs() < 1e-4,
            "got ({}, {})，期望 (522, 276)",
            frame.world_origin.x,
            frame.world_origin.y
        );
    }

    #[test]
    fn icon_anchor_is_normalized_against_its_own_extent() {
        // 世界 4096²，城市 1024² 落在 (512,256)：城市里的 anchor 应相对
        // **城市自己的 1024²** 归一化，而不是相对世界帧。
        let nodes = [
            n("map_world", None, (4096.0, 4096.0), (0.0, 0.0)),
            n(
                "map_city",
                Some("map_world"),
                (1024.0, 1024.0),
                (512.0, 256.0),
            ),
        ];
        let frame = frame_of(&nodes, "map_city").expect("城市图存在");
        let world = frame.uv_to_world([0.5, 0.41]);
        assert!(
            (world.x - 1024.0).abs() < 1e-3 && (world.y - 675.84).abs() < 1e-2,
            "got ({}, {})，期望 (1024, 675.84)：512+0.5*1024=1024，256+0.41*1024=675.84",
            world.x,
            world.y
        );
    }

    #[test]
    fn local_to_world_round_trips_through_world_to_local() {
        let nodes = [n("map_city", None, (1024.0, 1024.0), (512.0, 256.0))];
        let frame = frame_of(&nodes, "map_city").expect("地图存在");
        let local = Point::new(37.5, -12.25);
        let back = frame.world_to_local(frame.local_to_world(local));
        assert!(
            (back.x - local.x).abs() < 1e-4 && (back.y - local.y).abs() < 1e-4,
            "got ({}, {})，期望 ({}, {})",
            back.x,
            back.y,
            local.x,
            local.y
        );
    }

    #[test]
    fn world_to_uv_maps_extent_corners_to_zero_and_one() {
        let nodes = [n("map_world", None, (4096.0, 2048.0), (0.0, 0.0))];
        let frame = frame_of(&nodes, "map_world").expect("世界图存在");
        let top_left = frame.world_to_uv(Point::new(0.0, 0.0)).expect("有 extent");
        let bottom_right = frame
            .world_to_uv(Point::new(4096.0, 2048.0))
            .expect("有 extent");
        assert!(top_left == [0.0, 0.0], "got {top_left:?}，左上角应为 [0,0]");
        assert!(
            (bottom_right[0] - 1.0).abs() < 1e-6 && (bottom_right[1] - 1.0).abs() < 1e-6,
            "got {bottom_right:?}，右下角应为 [1,1]"
        );
    }

    #[test]
    fn undeclared_extent_yields_zero_size_and_degenerate_uv() {
        // 图未声明 extent：位置仍可还原，UV 归一化返回 None（不是 NaN）。
        let nodes = [
            n("map_world", None, (4096.0, 4096.0), (0.0, 0.0)),
            n("map_city", Some("map_world"), (0.0, 0.0), (300.0, 100.0)),
        ];
        let frame = frame_of(&nodes, "map_city").expect("地图存在");
        assert!(
            !frame.has_extent(),
            "未声明 extent 时 has_extent 应为 false"
        );
        assert_eq!(
            frame.world_origin,
            Point::new(300.0, 100.0),
            "位置仍应可由 origin 还原"
        );
        assert_eq!(
            frame.local_to_uv(Point::new(5.0, 5.0)),
            None,
            "宽高为 0 时归一化无意义，应为 None"
        );
        assert_eq!(
            frame.uv_to_world([0.5, 0.5]),
            Point::new(300.0, 100.0),
            "UV 退化为左上角点，不 panic"
        );
    }

    #[test]
    fn missing_parent_stops_the_climb_and_keeps_accumulated_origin() {
        // map_city 的父是悬空引用：不报错、不 panic，只停止上溯（§7.5 V1）。
        let nodes = [n(
            "map_city",
            Some("map_does_not_exist"),
            (1024.0, 1024.0),
            (512.0, 256.0),
        )];
        let frame = frame_of(&nodes, "map_city").expect("目标图自身存在");
        assert_eq!(
            frame.world_origin,
            Point::new(512.0, 256.0),
            "悬空父图只截断上溯，已累加的原点必须保留"
        );
    }

    #[test]
    fn parent_cycle_terminates_without_hanging() {
        let nodes = [
            n("map_a", Some("map_b"), (100.0, 100.0), (1.0, 2.0)),
            n("map_b", Some("map_a"), (100.0, 100.0), (4.0, 8.0)),
        ];
        let frame = frame_of(&nodes, "map_a").expect("地图存在");
        // a(1,2) + b(4,8) 一轮后回到 a 已被 seen 拦住 → 合计 (5,10)
        assert_eq!(
            frame.world_origin,
            Point::new(5.0, 10.0),
            "环必须在重复 ID 处截断"
        );
    }

    #[test]
    fn unknown_map_has_no_frame() {
        let nodes = [n("map_world", None, (4096.0, 4096.0), (0.0, 0.0))];
        assert_eq!(
            frame_of(&nodes, "map_missing"),
            None,
            "目标图不存在时返回 None"
        );
    }

    #[test]
    fn resolve_frame_in_reads_the_project_map_list() {
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            Vec::new(),
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::at(1024.0, 1024.0, Point::new(512.0, 256.0))),
            Vec::new(),
        );
        let nodes = vec![world, city];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_city")).expect("城市图存在");
        assert_eq!(
            frame.world_origin,
            Point::new(512.0, 256.0),
            "应从 MapNode 列表正确还原"
        );
        assert!(
            (frame.extent.width - 1024.0).abs() < 1e-4,
            "got {}，期望 1024",
            frame.extent.width
        );
    }

    #[test]
    fn undeclared_origin_is_derived_from_the_parent_drill_icon() {
        // 世界 4096²，武汉图标 anchor (0.5, 0.41) → 图标点 (2048, 1679.36)。
        // 城市图 1024² 未声明 origin → 落点 = 图标点 − 512 = (1536, 1167.36)。
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            vec![drill_icon("icon_wuchang", Some([0.5, 0.41]), "map_city")],
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::sized(1024.0, 1024.0)),
            Vec::new(),
        );
        let nodes = vec![world, city];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_city")).expect("城市图存在");
        assert!(
            (frame.world_origin.x - 1536.0).abs() < 1e-3
                && (frame.world_origin.y - 1167.36).abs() < 1e-2,
            "got ({}, {})，期望 (1536, 1167.36)：2048−512，1679.36−512",
            frame.world_origin.x,
            frame.world_origin.y
        );
        // 图标标记的是子图中心：落在图标的那个世界点必须正好是城市中心。
        let center = frame.local_to_world(Point::new(512.0, 512.0));
        assert!(
            (center.x - 2048.0).abs() < 1e-3 && (center.y - 1679.36).abs() < 1e-2,
            "got ({}, {})，期望图标点 (2048, 1679.36)",
            center.x,
            center.y
        );
    }

    #[test]
    fn declared_origin_wins_over_the_parent_drill_icon() {
        // 同一张子图有两个入口（一个在 (0.5,0.5) 一个在 (0.25,0.25)），
        // 只有作者能决定以哪个为准 → 显式 origin 必须压过派生。
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            vec![
                drill_icon("icon_gate_north", Some([0.5, 0.5]), "map_city"),
                drill_icon("icon_gate_south", Some([0.25, 0.25]), "map_city"),
            ],
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::at(1024.0, 1024.0, Point::new(100.0, 200.0))),
            Vec::new(),
        );
        let nodes = vec![world, city];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_city")).expect("城市图存在");
        assert_eq!(
            frame.world_origin,
            Point::new(100.0, 200.0),
            "显式 origin 优先于任何图标派生"
        );
    }

    #[test]
    fn icon_without_explicit_anchor_does_not_derive_a_position() {
        // 未声明 anchor 的图标由渲染层哈希摆位（权宜，不是作者声明的位置），
        // 不能反过来决定世界坐标 → 记为 (0,0)。
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            vec![drill_icon("icon_unknown", None, "map_city")],
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::sized(1024.0, 1024.0)),
            Vec::new(),
        );
        let nodes = vec![world, city];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_city")).expect("城市图存在");
        assert_eq!(frame.world_origin, Point::ORIGIN, "哈希摆位不参与坐标还原");
    }

    #[test]
    fn icon_drilling_into_another_map_is_ignored() {
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            vec![drill_icon("icon_other", Some([0.9, 0.9]), "map_other")],
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::sized(1024.0, 1024.0)),
            Vec::new(),
        );
        let nodes = vec![world, city];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_city")).expect("城市图存在");
        assert_eq!(
            frame.world_origin,
            Point::ORIGIN,
            "只有 drill_into 指向本图的图标才算入口"
        );
    }

    #[test]
    fn derivation_accumulates_through_grandparents() {
        // 世界 → 城市（由图标派生）→ 城区（由城市图标派生）
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            vec![drill_icon("icon_city", Some([0.5, 0.5]), "map_city")],
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::sized(1024.0, 1024.0)),
            vec![drill_icon(
                "icon_district",
                Some([0.25, 0.25]),
                "map_district",
            )],
        );
        let district = map_node(
            "map_district",
            Some("map_city"),
            MapLevel::Interior,
            Some(Extent::sized(100.0, 100.0)),
            Vec::new(),
        );
        let nodes = vec![world, city, district];
        let frame =
            resolve_frame_in(&nodes, &Id::new_unchecked("map_district")).expect("城区图存在");
        // 城市落点 = (2048,2048) − 512 = (1536,1536)
        // 城区落点 = 城市局部 (256,256) − 50 = (206,206)
        assert!(
            (frame.world_origin.x - 1742.0).abs() < 1e-3
                && (frame.world_origin.y - 1742.0).abs() < 1e-3,
            "got ({}, {})，期望 (1742, 1742)：1536+206",
            frame.world_origin.x,
            frame.world_origin.y
        );
    }

    #[test]
    fn child_without_extent_is_derived_to_the_icon_point() {
        // 子图连 extent 都没声明：半宽为 0，落点即图标点（位置仍可还原）。
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            Some(Extent::sized(4096.0, 4096.0)),
            vec![drill_icon("icon_bare", Some([0.5, 0.25]), "map_bare")],
        );
        let bare = map_node(
            "map_bare",
            Some("map_world"),
            MapLevel::Region,
            None,
            Vec::new(),
        );
        let nodes = vec![world, bare];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_bare")).expect("图存在");
        assert!(
            (frame.world_origin.x - 2048.0).abs() < 1e-3
                && (frame.world_origin.y - 1024.0).abs() < 1e-3,
            "got ({}, {})，期望 (2048, 1024)：0.5*4096，0.25*4096",
            frame.world_origin.x,
            frame.world_origin.y
        );
    }

    #[test]
    fn parent_without_extent_cannot_normalize_the_anchor() {
        let world = map_node(
            "map_world",
            None,
            MapLevel::World,
            None,
            vec![drill_icon("icon_city", Some([0.5, 0.5]), "map_city")],
        );
        let city = map_node(
            "map_city",
            Some("map_world"),
            MapLevel::City,
            Some(Extent::sized(1024.0, 1024.0)),
            Vec::new(),
        );
        let nodes = vec![world, city];
        let frame = resolve_frame_in(&nodes, &Id::new_unchecked("map_city")).expect("城市图存在");
        assert_eq!(
            frame.world_origin,
            Point::ORIGIN,
            "父图无 extent 时 anchor 无处归一化，退回原点而不是 panic"
        );
    }

    #[test]
    fn undeclared_origin_stays_declared_free_in_the_declared_only_view() {
        // resolve_frame 是纯声明口径：不做派生，未声明落点即 (0,0)。
        let nodes = [
            n_free("map_world", None, (4096.0, 4096.0)),
            n_free("map_city", Some("map_world"), (1024.0, 1024.0)),
        ];
        let frame = frame_of(&nodes, "map_city").expect("城市图存在");
        assert_eq!(
            frame.world_origin,
            Point::ORIGIN,
            "纯声明口径不派生：未声明 origin 就是 (0,0)"
        );
    }

    #[test]
    fn frame_node_of_node_without_extent_is_zero_sized() {
        let node = map_node("map_bare", None, MapLevel::Region, None, Vec::new());
        assert_eq!(node.frame_node().extent, Extent::ZERO);
    }
}
