//! 投影与叠加层：事件点高亮（S8）、轨迹逐段生长（S8）、笔刷光标与视口网格（S4/S5）。
//!
//! ## 职责
//! - 求值「某时刻的世界状态」：从 `PlaybackState::current_day` 出发，经
//!   `wv_core::time::TimelineQuery::at` 把时间相关的世界片段投影成渲染就绪数据。
//! - 维护轨迹逐段生长（ANIMATION_SPEC §1.2）：起点全亮 + 当前段部分生长，
//!   末端是 `accent.cursor` 6px 圆点。
//! - 把相机网格、笔刷光标、POI 焦点圆等**瞬态高亮**画到 terrain_view 之上。
//!
//! ## 边界（不做什么）
//! - **不写时间比较**——所有时间求值只能走 `TimelineQuery::at` / `Track::sample_at`
//!   （AGENTS §3.2 / 基线 §2.14），System 不准自行 `if time > event.time`。
//! - **不修改世界**——只读 `WorldProjection.world`；改动仍走 `wv-edit` 的 `History`。
//! - **不画地形底色**——那是 [`crate::terrain_view`]；本模块只在上层叠加高亮。
//!
//! ## 主要导出
//! - `ProjectionFrame` — 帧快照（geo ↔ 屏幕相机矩阵）。
//! - `apply_projection_refresh` — 每帧把 `ProjectionFrame` 推给渲染系统。
//! - `PlaceDot` / `PlaceDotKind` — POI 红点组件（含可见性标记）。

use crate::fp::grid_bits;
use bevy::prelude::*;

use std::collections::HashMap;

use crate::layer_frame::LayerFrame;
use crate::state::{GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::theme::colors;
use wv_core::map::distance::LineKind;
use wv_core::map::road_derive::{effective_line_kind, road_line_effective_rank};
use wv_core::time::TimeSpan;
use wv_core::time::TimelineQuery;

/// 一帧的投影结果（事件高亮 / 轨迹位置），供渲染系统只读。
#[derive(Resource, Default)]
pub struct ProjectionFrame {
    /// 本帧「正在发生」的事件 ID 与它们的图内位置（若可定位）。
    pub active_events: Vec<(String, Vec2)>,
    /// 本帧每条轨迹的「已生长折线」点列（世界坐标）。
    pub track_polylines: Vec<Vec<Vec2>>,
    /// 本帧每条轨迹的「生长笔尖」位置。
    pub track_heads: Vec<Vec2>,
    /// 本帧轨迹「空心起点」标记（推算 / 粗轨迹用空心，动画 §1.3）。
    pub track_hollow_starts: Vec<Vec2>,
    /// 本帧道路层（矢量叠加）：一段线（世界坐标端点对）。
    ///
    /// **供给源（ADR-0070 T8 起）**：
    /// ① 当前图上 `effective_line_kind == Road` 的线段 POI——折线切段，
    ///    每线一个 rank（[`road_line_effective_rank`]，km = 线长 × 图比例尺，
    ///    无比例尺 → 1）；
    /// ② `terrain != "road"` 的手写 `PlaceEdge`（海路等）——两端锚定的
    ///    直连段，rank 走 `PlaceEdge::effective_rank`。
    ///    `terrain == "road"` 的边**不再渲染**：道路唯一供给源是路 POI 线，
    ///    存量 road 边由加载期迁移转线（ADR-0070 D5）。
    ///
    /// 只画**锚定在当前地图**的内容（`Place.map == 当前图 id` 且坐标齐备），
    /// 与 [`crate::drill::resolve_markers`] 的「只画当前层级」口径一致。
    ///
    /// **已按 rank 降序排序**（ADR-0047 D4）：每档视野
    /// 可见的段集 = 降序前缀，切片见 [`Self::road_slice_at`]；消费者不得依赖
    /// 段顺序之外的任何排列语义（同 mesh 内顶点序不影响绘制结果）。
    pub road_segments: Vec<(Vec2, Vec2)>,
    /// 每档视野的道路段可见前缀长度（ADR-0047 D4），下标序 `[Edit, Region, Nation]`。
    ///
    /// 与 `road_segments` 的降序排列配套：Nation 档画 `[..cuts[2]]`，
    /// Region 档 `[..cuts[1]]`，Edit 档全量。缩放**不**重算段列（缓存门
    /// `road_key` 不含 scale），档位切换只是 O(1) 前缀切片。
    pub road_rank_cuts: [usize; 3],
    /// 本帧地点（POI）点标（世界坐标），锚定在当前地图的地点。
    ///
    /// 已按视口裁剪（0c01491）；级别门槛与密度上限在 `sync_poi` 按
    /// [`PlaceDot::rank`] 执行（ADR-0030 D2/D3）。
    pub place_dots: Vec<PlaceDot>,
    /// 本帧 [`Self::road_segments`] 的内容指纹（A2，2026-10-18）。
    ///
    /// `sync_vector_layers` 的合批 mesh 以它判「路网内容变没变」：不变则连
    /// mesh 规格比对都不必做。相机平移不改世界系段坐标（指纹不变 → 零重建），
    /// 编辑与钻取切图才会改变它。**语义 = Edit 档（全量）指纹**；分档指纹见
    /// [`Self::road_tier_fingerprints`] / [`Self::road_fingerprint_at`]。
    pub road_fingerprint: u64,
    /// 每档视野的段列内容指纹（ADR-0047 D4），下标序同 [`Self::road_rank_cuts`]。
    ///
    /// 三档各算各的：缩放跨档只换 mesh（前缀内容变了），不重算段列。
    pub road_tier_fingerprints: [u64; 3],
    /// 路段列缓存键（ADR-0030 D5a）。`None` = 失效。段坐标在**根图帧系**，
    /// 不随相机平移/缩放变——只有世界编辑、钻取换图、栅格换图或 Road 图层
    /// 可见性翻转才需要重算段列与指纹，其余帧这一步是零工作。
    pub road_key: Option<bool>,
    /// 本帧河流折线（ADR-0070 T8 单池化）：**唯一供给源 = 当前图上
    /// `effective_line_kind == River` 的线段 POI**——作者手绘（ADR-0046）与
    /// 迁移落地的模板河（ADR-0070 D5）同池，视图空间坐标（同
    /// [`Self::road_segments`] 的换算口径）。
    ///
    /// 每条折线带呈现级别（ADR-0047 D5 长度占比折算，[`RiverLine::rank`]）
    /// 与基准宽（[`RiverLine::width_wu`]，`line_width_m` 经图比例尺折算）。
    /// 旧的 `terrain + rivers_seed` 确定性派生已删——种子河在加载期由
    /// wv-edit 迁移转成河线 Place，渲染侧不再读种子（双供给至此收单池）。
    pub river_polylines: Vec<RiverLine>,
    /// 河池缓存键 = 当前图 id。`None` = 失效。
    /// 世界编辑 / 钻取换图 / 栅格换图都会重算；纯相机运动零工作。
    /// （旧键含 `rivers_seed`；种子派生删除后只剩图 id 一项。）
    pub river_key: Option<String>,
    /// 本帧地物符号（rank ≥ 2 的地点，视图空间坐标）：国家档与更近档都画，
    /// 靠视口裁剪防密集点（参照 `place_dots` A1 索引）。
    pub landmark_dots: Vec<LandmarkDot>,
}

/// 一条河流折线的渲染数据（ADR-0047 D5 定级 + ADR-0070 D4 定宽）。
#[derive(Debug, Clone, PartialEq)]
pub struct RiverLine {
    /// 呈现级别（[`river_rank_from_length_frac`] 折算）；绘制侧过统一
    /// 门槛 [`crate::state::rank_visible_at_tier`]。
    pub rank: u8,
    /// 视图空间顶点（≥ 2 个才构成可见折线）。
    pub pts: Vec<Vec2>,
    /// 基准宽（**世界单位**）：`Place.line_width_m`（米）经图比例尺折算，
    /// 绘制侧沿弧长锥形插值（源头 30% → 河口 100%，ADR-0070 D4）。
    ///
    /// `0.0` = 未声明 `line_width_m`（存量夹具 / 缺省语义）→ 绘制侧退回
    /// 旧屏幕像素缺省宽。宽在**收集时**折算好而不是绘制时换算：图比例尺
    /// 是投影输入，绘制侧不该再摸世界数据。
    pub width_wu: f32,
}

/// 河流呈现级别折算（ADR-0047 D5）：折线长度 ÷ 图幅宽度 ≥ 0.40 → 3（横贯级
/// 主河），≥ 0.15 → 2（区域河），否则 → 1（溪流）。
///
/// 占比口径与图幅尺寸无关——4096² 世界图与 512² 城市图同一把尺；派生河
/// 与作者手绘河共用（双供给同一呈现语义）。阈值是表现层参数，登记在
/// ADR-0047 §D5，调整只动这里 + 对应测试。
pub fn river_rank_from_length_frac(frac: f32) -> u8 {
    if frac >= 0.40 {
        3
    } else if frac >= 0.15 {
        2
    } else {
        1
    }
}

/// 拖动中图钉的**预览位**（视图空间）；非激活或非该钉 → `None`。
///
/// 帧构建（点标 [`ProjectionFrame::place_dots`] / 图钉
/// [`ProjectionFrame::landmark_dots`]）与标签系统（`drill.rs` 的
/// `update_place_labels`）共用这一入口——三处渲染读同一个预览，
/// 拖动时钉、点、标签一起走（[ADR-0074] 追记三）。世界锚点在松手
/// 提交前不动，预览只活在渲染侧。
pub fn pin_preview_view(
    drag: &crate::state::PinDragState,
    place: &wv_core::Id,
    grid: &GridConfig,
) -> Option<Vec2> {
    if !drag.active || drag.place.as_ref() != Some(place) {
        return None;
    }
    drag.preview_local
        .map(|(x, y)| grid.map_to_view(Vec2::new(x, y)))
}

/// 选中图钉**高亮环**的位置（视图空间）；不该亮时 `None`。
///
/// 亮的条件（全部满足）：选中态是 Place ∧ 该钉锚定当前图 ∧ 是点状
/// （线段 POI 无钉）∧ 锚点齐备。拖动中跟预览位（[`pin_preview_view`]）。
/// 供 [`draw_pin_highlight`] 消费——纯函数便于断言。
pub fn pin_highlight_pos(
    world: &wv_core::World,
    selection: Option<&wv_core::SelectedEntity>,
    current_map: Option<&wv_core::Id>,
    grid: &GridConfig,
    drag: &crate::state::PinDragState,
    pin_mode: bool,
) -> Option<Vec2> {
    use wv_core::EntityKind;
    let sel = selection?;
    if sel.kind != EntityKind::Place {
        return None;
    }
    let cur = current_map?;
    let p = world.distances.places.iter().find(|p| p.id == sel.id)?;
    if p.map.as_ref() != Some(cur) || p.line.is_some() {
        return None;
    }
    // pin 模式 ∧ rank≥2：选中钉**换金色版本**（用户裁定「换一个颜色的
    // map-pin」），环只兜底点标（rank<2）与插画形态。
    if pin_mode && p.effective_rank() >= 2 {
        return None;
    }
    let (x, y) = p.effective_anchor()?;
    let base = grid.map_to_view(Vec2::new(x, y));
    Some(pin_preview_view(drag, &p.id, grid).unwrap_or(base))
}

/// 选中图钉的**高亮环**（金 = 本仓库「选中」语义色，与格选区金环同族）：
/// 双圈——外圈实、内圈淡，在图钉与点标外缘一圈，屏幕像素恒定
/// （半径乘 `view.scale`）。拖动中跟预览位走（世界锚点未动也亮在新位）。
pub fn draw_pin_highlight(
    mut gizmos: Gizmos,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    drag: Res<crate::state::PinDragState>,
) {
    // place 层关 = 钉不画 = 不亮（点「看不见的东西」不该有反光）。
    if !layers.place.visible {
        return;
    }
    let pin_mode = crate::vector_layers::landmark_marker(view.mode).is_pin();
    let Some(pos) = pin_highlight_pos(
        &projection.world,
        projection.selection.as_ref(),
        drill.current(),
        &grid,
        &drag,
        pin_mode,
    ) else {
        return;
    };
    let r = crate::drill::PIN_HIT_PX * view.scale.max(0.05);
    let gold = Color::srgb(1.0, 0.78, 0.18);
    gizmos.circle_2d(Isometry2d::from_translation(pos), r, gold.with_alpha(0.95));
    gizmos.circle_2d(
        Isometry2d::from_translation(pos),
        r * 0.72,
        gold.with_alpha(0.5),
    );
}

/// gizmo 线宽 = 1 **屏幕像素**（每帧随缩放同步）。
///
/// Bevy 2D gizmo 默认线宽按**世界单位**计（2.0）——CellMap 深缩放下
/// （1 屏幕像素 ≈ 数世界单位）线会细到亚像素，选中环/选区环整个隐形
/// （2026-10-27 实机：选中图钉无高亮）。全局生效：选区金环
/// （`draw_selection`）等 gizmo 画出的一并回到可见线宽。
pub fn sync_gizmo_line_width(
    mut store: ResMut<bevy::gizmos::config::GizmoConfigStore>,
    view: Res<ViewState>,
) {
    let (cfg, _) = store.config_mut::<bevy::gizmos::config::DefaultGizmoConfigGroup>();
    cfg.line.width = view.scale.max(0.05);
    cfg.enabled = true;
}

/// 折线总长（相邻顶点距离和）。
fn polyline_len(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// 河流锥形宽的逐段系数（ADR-0070 D4）：返回与 `pts.windows(2)` 平行的
/// 系数列，第 i 段宽 = 基准宽 × 系数[i]；系数 = `lerp(0.3, 1.0, 弧长占比)`，
/// 弧长占比 = 段起点累计弧长 ÷ 总线长（源头 30% → 河口 100%）。
///
/// 放投影侧而非绘制侧：纯函数无头可测（计划 T8 的锥形宽测试落在
/// world_projection 测试区），绘制侧只消费——「构建」与「断言」同一实现，
/// 不留两套真相（先例：[`build_road_tier_view`]）。
pub(crate) fn river_taper_factors(pts: &[Vec2]) -> Vec<f32> {
    let total = polyline_len(pts).max(f32::EPSILON);
    let mut acc = 0.0;
    pts.windows(2)
        .map(|w| {
            let f = 0.3 + 0.7 * (acc / total);
            acc += w[0].distance(w[1]);
            f
        })
        .collect()
}

impl ProjectionFrame {
    /// 档位 `tier` 可见的道路段切片（ADR-0047 D4）：降序前缀，O(1)。
    pub fn road_slice_at(&self, tier: crate::state::LodTier) -> &[(Vec2, Vec2)] {
        let i = match tier {
            crate::state::LodTier::Edit => 0,
            crate::state::LodTier::Region => 1,
            crate::state::LodTier::Nation => 2,
        };
        &self.road_segments[..self.road_rank_cuts[i]]
    }

    /// 档位 `tier` 的道路段内容指纹（ADR-0047 D4）。
    pub fn road_fingerprint_at(&self, tier: crate::state::LodTier) -> u64 {
        let i = match tier {
            crate::state::LodTier::Edit => 0,
            crate::state::LodTier::Region => 1,
            crate::state::LodTier::Nation => 2,
        };
        self.road_tier_fingerprints[i]
    }
}

/// 由「带呈现级别的段列」构建档位切片视图（ADR-0047 D4）。
///
/// 按 rank 降序排序后每档可见集恰为降序前缀：Edit 全量、Region 取 rank ≥ 2、
/// Nation 取 rank ≥ 3（[`crate::state::rank_visible_at_tier`] 的前缀化）；
/// 三档指纹各自独立。纯函数，`compute_projection` 与测试共用同一实现，
/// 避免「构建」与「切片」两套真相。
pub(crate) fn build_road_tier_view(
    mut ranked: Vec<(u8, Vec2, Vec2)>,
) -> (Vec<(Vec2, Vec2)>, [usize; 3], [u64; 3]) {
    ranked.sort_unstable_by_key(|x| std::cmp::Reverse(x.0));
    let segments: Vec<(Vec2, Vec2)> = ranked.iter().map(|(_, a, b)| (*a, *b)).collect();
    let min_rank = [0u8, 2, 3]; // Edit 全量 / Region ≥ 2 / Nation ≥ 3
    let mut cuts = [0usize; 3];
    let mut fingerprints = [0u64; 3];
    for (ti, cut) in cuts.iter_mut().enumerate() {
        // 段列按 rank 降序 → 谓词 `rank ≥ min_rank` 的真前缀长度即切割位。
        *cut = ranked.partition_point(|(r, _, _)| *r >= min_rank[ti]);
        fingerprints[ti] = segments_fingerprint(&segments[..*cut]);
    }
    (segments, cuts, fingerprints)
}

/// 一个地物符号的渲染数据。`kind` 决定 `sync_landmarks` 烘哪个顶点流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandmarkKind {
    Citadel,
    Town,
}

/// `LandmarkKind` 稳定数组（合批键 / 遍历用）。
pub const LANDMARK_KINDS: [LandmarkKind; 2] = [LandmarkKind::Citadel, LandmarkKind::Town];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandmarkDot {
    pub pos: Vec2,
    pub kind: LandmarkKind,
    /// 选中态（[ADR-0074] 追记三 followup）：绘制侧把钉染金（换色优先于环）。
    pub selected: bool,
}

/// 一个 POI 点标的渲染数据。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaceDot {
    /// 视图空间坐标（本图局部经 [`GridConfig::map_to_view`] 换算而来）。
    pub pos: Vec2,
    /// 是否为「城市级」地点（`Place.w/h` 齐备）。**只剩材质语义**：城市用
    /// 城市材质；可见性一律走 [`Self::rank`] 的档位门槛（ADR-0030 D2）。
    pub is_city: bool,
    /// 有效呈现级别（`Place::effective_rank` 的投影，ADR-0030）：门槛
    /// `poi_visible_at_rank` 与密度上限的优先级排序都读它。
    pub rank: u8,
    /// 名字字符数（密度上限的第三优先级：更长者信息量更大，与标签碰撞
    /// 优先级同一惯例）。
    pub name_len: u32,
    /// POI 符号类别（ADR-0064）。专业制图按它分色；其余模式仍用统一的
    /// 城市 / 普通两档材质（艺术主题不分把类别符号化）。
    pub kind: wv_core::distance::PoiKind,
}

/// 地点缓存索引（A1，2026-10-18）：端点解析与 POI 收集的 O(1) 查表。
///
/// 背景：guangdong 工程（87,664 边 × 81,530 地点）的道路层每帧对每条边两个
/// 端点各做一次全量线性 `find`，实测 **17.6 秒/帧**（release）——「打开即
/// 卡死」即此。索引把端点解析变哈希查询，POI 收集变按图分桶直取。
///
/// 只缓存「`places` 下标 + 按图分组」，坐标 / 可见性等判定仍读
/// [`WorldProjection::world`] 原始数据——索引不持数据副本，没有第二真相源；
/// 重建以 `Res<WorldProjection>` 变更检测为号（世界替换 / 任何编辑都会命中），
/// 81,530 点整表重建实测亚毫秒，编辑期可忽略。
#[derive(Resource, Default)]
pub struct PlaceIndex {
    /// `Place.id` → `world.distances.places` 下标。**全量**（含无坐标的
    /// 纯距离图节点，如 overture 路口——它们不锚定地图，端点解析时按
    /// `map` 匹配自然落空）。
    by_id: HashMap<wv_core::id::Id, u32>,
    /// 锚定图 id → 该图上坐标齐备的地点下标（POI 点标循环直取，不再全量扫）。
    pub(crate) anchored_by_map: HashMap<wv_core::id::Id, Vec<u32>>,
}

impl PlaceIndex {
    /// 对整个世界的距离图建索引。单遍扫描，先 `by_id` 后分桶。
    pub(crate) fn build(world: &wv_core::World) -> Self {
        let places = &world.distances.places;
        let mut index = Self {
            by_id: HashMap::with_capacity(places.len()),
            anchored_by_map: HashMap::new(),
        };
        for (i, p) in places.iter().enumerate() {
            index.by_id.insert(p.id.clone(), i as u32);
            // 锚点走 [`Place::effective_anchor`]（ADR-0046 D2 定的**唯一入口**）：
            // 线状地点（作者手绘河）通常只写 `line`，`x/y` 缺省时锚点取折线弧长中点。
            // 旧实现只认 `p.x/p.y`，这类地点**整条漏出本图桶**，于是下游的作者河
            // 分支（同走本桶）一根也画不出来——「YAML 省 x/y 河就消失」的成因。
            if let (Some(map), Some(_)) = (p.map.as_ref(), p.effective_anchor()) {
                index
                    .anchored_by_map
                    .entry(map.clone())
                    .or_default()
                    .push(i as u32);
            }
        }
        index
    }
}

/// 段列内容指纹（FNV-1a over 坐标位型）：合批 mesh 的「内容变没变」判据。
///
/// 相机平移不改世界系段坐标（指纹不变 → 零重建）；编辑与钻取切图改变段列
/// 才会改变指纹。空段列有唯一常量指纹。
pub(crate) fn segments_fingerprint(segments: &[(Vec2, Vec2)]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (a, b) in segments {
        for v in [a.x, a.y, b.x, b.y] {
            hash ^= u64::from(v.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 计算本帧投影。**所有时间判断集中在这里**，且走 `wv-core` 的纯函数。
///
/// 图层可见性读 [`LayerFrame`]（ADR-0013：YAML 声明 ∧ 会话开关的合取投影），
/// 不直接读 `ViewState` 的布尔——轨迹无对应 `LayerKind`，仍走会话开关。
#[allow(clippy::too_many_arguments)]
pub fn compute_projection(
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<crate::state::DrillState>,
    projection: Res<WorldProjection>,
    layers: Res<LayerFrame>,
    // 图钉拖动预览（ADR-0074 追记三）：active 时点标/图钉按预览位出。
    drag: Res<crate::state::PinDragState>,
    mut place_index: ResMut<PlaceIndex>,
    mut frame: ResMut<ProjectionFrame>,
) {
    frame.active_events.clear();
    frame.track_polylines.clear();
    frame.track_heads.clear();
    frame.track_hollow_starts.clear();
    frame.place_dots.clear();
    frame.landmark_dots.clear();

    // 路段列缓存门（ADR-0030 D5a）：段坐标在根图帧系，不随相机平移/缩放变，
    // 只有世界编辑 / 钻取换图 / 栅格换图 / Road 图层可见性翻转才重算。
    // grid 变更已含在变更检测里（sync_grid_config 未变时早退，不逐帧置脏）。
    let road_key = Some(layers.road.visible);
    let road_stale = frame.road_key != road_key
        || projection.is_changed()
        || drill.is_changed()
        || grid.is_changed();
    frame.road_key = road_key;
    if road_stale {
        frame.road_segments.clear();
        frame.road_rank_cuts = [0; 3];
        frame.road_tier_fingerprints = [segments_fingerprint(&[]); 3];
        frame.road_fingerprint = segments_fingerprint(&frame.road_segments);
    }

    // 河池缓存门（ADR-0037 D5 的纪律，ADR-0070 T8 收单池后键只剩图 id）：
    // 世界编辑 / 钻取换图 / 栅格换图即重算，纯相机运动零工作（与路网缓存
    // 同款——**不是**每帧无条件重建，合批 mesh 的资产泄漏坑见 AGENTS 历史）。
    let current_for_river = drill.current().cloned();
    let river_wanted = current_for_river.as_ref().map(|m| m.as_str().to_owned());
    let river_stale = frame.river_key != river_wanted
        || projection.is_changed()
        || drill.is_changed()
        || grid.is_changed();
    frame.river_key = river_wanted;
    if river_stale {
        frame.river_polylines.clear();
    }

    // 索引只在世界变更后整表重建（世界替换 / 任何编辑都命中变更检测；
    // 未变更时这一行就是一次 tick 比较）。放在早退之前：图层全关的帧
    // 也保持索引新鲜，重新打开图层的第一帧就能查。
    if projection.is_changed() {
        *place_index = PlaceIndex::build(&projection.world);
    }

    // 河流收集（ADR-0070 T8 单池）：当前图上 `effective_line_kind == River`
    // 的线段 POI → 折线。坐标：折线是**图内局部 wu** → `map_to_view` 转视图
    // 空间（同 POI 口径）。级别（ADR-0047 D5）：折线长度 ÷ 图幅宽；图幅未知
    // 归 1（亚格数据本质是局部细节，宁可少显示）。
    // 基准宽（ADR-0070 D4）：`line_width_m`（米）→ wu——`km_per_wu` 是
    // 「1 wu 有多少 km」，故 1 wu = 1000 × km_per_wu **米**，
    // `width_wu = width_m ÷ 1000 ÷ km_per_wu`；无比例尺 → 数字按 wu 直读
    // （V1 只提示不阻止，宁画勿丢）；未声明 → 0（绘制侧退旧缺省宽）。
    // `line_kind == Road` 的线不进河池——归路池（见下方道路层收集）。
    // 河流重算放在图层早退之前：河流属地形族（terrain 图层在渲染侧 gate），
    // 关掉全部矢量图层再重新打开的第一帧就要有河可画。
    if river_stale {
        let node = current_for_river
            .as_ref()
            .and_then(|map_id| projection.world.maps.iter().find(|m| &m.id == map_id));
        let extent_w = node.as_ref().and_then(|n| n.extent).map(|e| e.width);
        let km_per_wu = node.and_then(|n| n.km_per_wu()).map(|(kx, _)| kx as f32);
        if let Some(map_id) = &current_for_river {
            if let Some(idxs) = place_index.anchored_by_map.get(map_id) {
                for i in idxs {
                    let place = &projection.world.distances.places[*i as usize];
                    // 判别唯一入口（ADR-0070 D2）：缺省（None）与显式 River 同归河。
                    if effective_line_kind(place) != Some(LineKind::River) {
                        continue;
                    }
                    let Some(line) = place.line.as_ref() else {
                        continue;
                    };
                    let pts: Vec<Vec2> = line
                        .chunks(2)
                        .filter(|p| p.len() == 2)
                        .map(|p| grid.map_to_view(Vec2::new(p[0], p[1])))
                        .collect();
                    if pts.len() >= 2 {
                        let rank = extent_w
                            .map(|w| river_rank_from_length_frac(polyline_len(&pts) / w.max(1.0)))
                            .unwrap_or(1);
                        let width_wu = place
                            .line_width_m
                            .map(|m| match km_per_wu {
                                Some(k) => m / 1000.0 / k,
                                None => m,
                            })
                            .unwrap_or(0.0);
                        frame.river_polylines.push(RiverLine {
                            rank,
                            pts,
                            width_wu,
                        });
                    }
                }
            }
        }
    }

    if !layers.event_point.visible
        && !view.tracks_visible
        && !layers.road.visible
        && !layers.place.visible
    {
        return;
    }

    let world = &projection.world;
    let t = playback.current_day;

    // ---- 事件高亮：用 TimelineQuery::at 求「t 时刻正在发生的事件」----
    // （frame 是数据投影，不做模式门——EventDots 的门在绘制侧 draw_map_gizmos。）
    if layers.event_point.visible {
        let spans: Vec<(TimeSpan, usize)> = world
            .events
            .iter()
            .enumerate()
            .map(|(i, e)| (e.span, i))
            .collect();
        for hit in TimelineQuery::at(&spans, t) {
            let event = &world.events[hit.1];
            // 事件位置：原型阶段尚未建立「地点 → 坐标」的解析（属 P2），
            // 因此统一记在世界原点；列表/高亮仍可用。坐标解析就绪后替换此处。
            frame
                .active_events
                .push((event.id.as_str().to_string(), Vec2::ZERO));
        }
    }

    // ---- 轨迹逐段生长 ----
    if view.tracks_visible {
        for track in &world.tracks {
            let poly = grown_polyline(track, t);
            if poly.len() >= 2 {
                frame.track_polylines.push(poly);
            }
            if let Some(head) = track.sample_at(t) {
                frame.track_heads.push(root_to_view(head.x, head.y));
            }
            // 空心起点：轨迹第一个有坐标的 waypoint 且其 day 为推算（None）或为粗轨迹。
            let hollow = track
                .waypoints
                .iter()
                .find_map(|w| w.pos)
                .map(|p| root_to_view(p.x, p.y));
            if let Some(h) = hollow {
                frame.track_hollow_starts.push(h);
            }
        }
    }

    // ---- 道路层：地点边（纯几何叠加，无时间语义，不涉 P7）----
    if layers.road.visible || layers.place.visible {
        if let Some(current) = drill.current_node(world) {
            // 只取锚定在当前地图、且坐标齐备的地点（与标记层「只画当前层级」同口径）。
            // 地点坐标是**本图局部**坐标 → `grid.map_to_view`（不是栅格原点）。
            // 端点解析走 [`PlaceIndex`] 哈希查表——线性 `find` 在 guangdong
            // 规模（8.7 万边 × 8.1 万点）下是 17.6s/帧的卡死源（A1）。
            let find_place = |id: &wv_core::id::Id| -> Option<Vec2> {
                let i = *place_index.by_id.get(id)?;
                let p = &world.distances.places[i as usize];
                if p.map.as_ref() != Some(&current.id) {
                    return None;
                }
                // 锚点唯一入口（同上，ADR-0046 D2）。
                let (x, y) = p.effective_anchor()?;
                Some(grid.map_to_view(Vec2::new(x, y)))
            };
            if layers.road.visible && road_stale {
                // 带级别收集（ADR-0047 D4）→ 降序排序 → 三档前缀切割位与
                // 各自指纹一次算齐。供给源两路（ADR-0070 T8）：
                //
                // ① 路 POI 线：折线切段，每线一个 rank。km = 线长(wu) ×
                //    km_per_wu（视图长度 == 图内 wu 长度，map_to_view 只平移）；
                //    无比例尺 → None → 折算归 1（显式 `place.rank` 仍赢）。
                // ② `terrain != "road"` 的手写边（海路等）照旧直连段。
                //    `terrain == "road"` 的边**不收**：道路唯一供给源是路
                //    POI 线（含推导边——它 terrain 也是 road，同样不画；
                //    推导边只服务路径搜索 / 行程时间，ADR-0070 D5）。
                let mut ranked: Vec<(u8, Vec2, Vec2)> = Vec::new();
                if let Some(idxs) = place_index.anchored_by_map.get(&current.id) {
                    for i in idxs {
                        let place = &world.distances.places[*i as usize];
                        // 判别唯一入口（ADR-0070 D2）：只有显式 Road 是路。
                        if effective_line_kind(place) != Some(LineKind::Road) {
                            continue;
                        }
                        let Some(line) = place.line.as_ref() else {
                            continue;
                        };
                        let pts: Vec<Vec2> = line
                            .chunks(2)
                            .filter(|p| p.len() == 2)
                            .map(|p| grid.map_to_view(Vec2::new(p[0], p[1])))
                            .collect();
                        if pts.len() < 2 {
                            continue;
                        }
                        let km = current
                            .km_per_wu()
                            .map(|(kx, _)| polyline_len(&pts) * kx as f32);
                        let rank = road_line_effective_rank(place, km);
                        for pair in pts.windows(2) {
                            ranked.push((rank, pair[0], pair[1]));
                        }
                    }
                }
                for e in &world.distances.edges {
                    if e.terrain == "road" {
                        continue;
                    }
                    if let (Some(a), Some(b)) = (find_place(&e.a), find_place(&e.b)) {
                        ranked.push((e.effective_rank(), a, b));
                    }
                }
                let (segments, cuts, fingerprints) = build_road_tier_view(ranked);
                frame.road_segments = segments;
                frame.road_rank_cuts = cuts;
                frame.road_tier_fingerprints = fingerprints;
                frame.road_fingerprint = frame.road_tier_fingerprints[0];
            }
            // POI 点标：当前地图上的**视口内**锚定地点；带「是否城市级」标记供 LOD 筛选。
            // 事实地点（`feature = Some`）由 `feature_overlays` 画地界晕与符号，
            // 不再叠 POI 圆点（`place_dot_for` 统一裁决）。
            // 走 [`PlaceIndex::anchored_by_map`] 直取本图桶，不全量扫（A1）。
            //
            // **视口裁剪（zoom-in 卡死回归，2026-10-18）**：`sync_poi` 每个可见点
            // spawn 一对实体 + mesh，若桶里装全量锚定地点，guangdong 的 8.1 万
            // 路口在 zoom in 越过淡出带后会一次性全涌入（16 万实体/帧 = 冻结）。
            // 视口矩形 = 相机中心 ± (窗口逻辑尺寸 × scale / 2)，外加一圈余量
            // （点径最大 ~16px 换算世界单位），拖动相机时边缘点不闪断。
            if layers.place.visible {
                let margin = view.scale * 16.0;
                let half = view.window_size * view.scale * 0.5 + Vec2::splat(margin);
                let (cx, cy) = (view.camera_center.x, view.camera_center.y);
                if let Some(idxs) = place_index.anchored_by_map.get(&current.id) {
                    let sel_place = match projection.selection.as_ref() {
                        Some(s) if s.kind == wv_core::EntityKind::Place => Some(&s.id),
                        _ => None,
                    };
                    for i in idxs {
                        let p = &world.distances.places[*i as usize];
                        // 拖动预览（ADR-0074 追记三）：激活时钉/点按预览位。
                        let preview = pin_preview_view(&drag, &p.id, &grid);
                        if let Some(mut dot) = place_dot_for(p, &grid) {
                            if let Some(prev) = preview {
                                dot.pos = prev;
                            }
                            if (dot.pos.x - cx).abs() <= half.x && (dot.pos.y - cy).abs() <= half.y
                            {
                                frame.place_dots.push(dot);
                            }
                        }
                        // 地物符号：rank ≥ 2 走程序化插画（rank 1 路网交点仍按点标）。
                        // rank 3 永远画国家级，rank 2 全档都画（参考图期望：全球档可见城市符号）。
                        // 线状地点（作者河）不是聚落，不画城市符号——与 `place_dot_for`
                        // 同一条排除口径，避免「河上长出一座城」。
                        if p.line.is_none() {
                            if let Some((x, y)) = p.effective_anchor() {
                                let rank = p.effective_rank();
                                if rank >= 2 {
                                    let pos = preview
                                        .unwrap_or_else(|| grid.map_to_view(Vec2::new(x, y)));
                                    if (pos.x - cx).abs() <= half.x && (pos.y - cy).abs() <= half.y
                                    {
                                        let kind = if rank >= 3 {
                                            LandmarkKind::Citadel
                                        } else {
                                            LandmarkKind::Town
                                        };
                                        frame.landmark_dots.push(LandmarkDot {
                                            pos,
                                            kind,
                                            selected: sel_place == Some(&p.id),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// **根图帧**（世界）坐标 → 视图空间。
///
/// 视图空间**就是**根图坐标系（基线 §2.15：相机、`ViewState::cursor_world`、
/// `drill_target` 全在根帧），所以这里是恒等映射；保留函数是为了把「这份数据
/// 属于根帧」写在调用处，别让读者把它和随手一个 `Vec2::new` 混起来。
/// 本图局部坐标（地点 / 街道）走 [`GridConfig::map_to_view`]。
///
/// 待决 §12-26：轨迹 waypoint 与事项事件的坐标**今天一律按根帧（世界）解释**，
/// 数据里没有「这份坐标属于哪张图」的声明。世界级事件/轨迹因此天然正确；若将来
/// 允许事件或轨迹按子图局部坐标书写，必须先加所属帧字段，再改这里——收敛点是
/// 本函数与 `grown_polyline`，改动面可控。
fn root_to_view(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// 生成「已生长」折线（动画 §1.2）：已知生长位置之前全亮，当前段线性插值到笔尖。
fn grown_polyline(track: &wv_core::track::Track, day: i64) -> Vec<Vec2> {
    // 带时间戳且有坐标的锚点，按 day 升序。
    let mut anchors: Vec<(i64, Vec2)> = track
        .waypoints
        .iter()
        .filter_map(|w| w.day.zip(w.pos))
        .map(|(d, p)| (d, root_to_view(p.x, p.y)))
        .collect();
    anchors.sort_by_key(|(d, _)| *d);
    if anchors.is_empty() {
        return Vec::new();
    }
    if day <= anchors[0].0 {
        return vec![anchors[0].1];
    }
    let mut out = Vec::new();
    out.push(anchors[0].1);
    for i in 0..anchors.len() - 1 {
        let (d0, p0) = anchors[i];
        let (d1, p1) = anchors[i + 1];
        if day >= d1 {
            out.push(p1);
        } else {
            // 部分生长：进度归一化走 wv-core 纯函数（day 空间插值，动画 §5.1），
            // 渲染层不自行比较时间点（AGENTS §3.2）。`d1 == d0`（同天锚点）→ 0.0。
            let u = TimelineQuery::day_progress(day, d0, (d1 - d0) as f32);
            out.push(p0.lerp(p1, u));
            return out;
        }
    }
    out
}

/// 播放推进：每帧 `current_day += speed * dt`（视图状态推进，基线 §2.14 / 动画 §1.1）。
///
/// 只在 Browse 推进（`ViewMode::timeline_shows`，2026-10-27 用户裁定）：
/// 播放器是时间推进的唯一可见表针，表针不在场时时间冻结——否则 Map/CellMap
/// 里时间静走却无处暂停，播放锁（§9.4 Gate-1）也无从解开。
pub fn advance_playback(
    time: Res<Time>,
    view: Res<ViewState>,
    mut playback: ResMut<PlaybackState>,
) {
    if !playback.playing || !view.mode.timeline_shows() {
        return;
    }
    let dt = time.delta_secs();
    // 先加小数进位再取整：60fps 下每帧增量不足 1 天，直接截断会永远推不动。
    let add = playback.speed * dt + playback.carry;
    let whole = add as i64;
    playback.carry = add - whole as f32;
    playback.current_day += whole;
    // 在范围内循环（播到头回绕，动画 §1.1）。
    if playback.current_day >= playback.range_to {
        playback.current_day = playback.range_from;
    }
}

/// 选中区域装饰（阶梯描边 + 顶点手柄 + 候选锚点）的**派生缓存**。
///
/// ## 为什么要有（2026-10-23 · 用户实测「鼠标移动 120→40 FPS」）
///
/// `draw_selected_region` 旧实现**每帧**重做三件全量的事：
/// `region.polygon.clone()`（分配）→ `rasterize_polygon_cells`（按 bbox 扫全格）
/// → `HashSet` + 四邻边扫描。选中态和区域多边形都不变时这三件事**结果一模一样**。
/// 现在按 `(区域 id, 多边形指纹, 网格, 幅面)` 缓存，键不变就只画缓存结果。
///
/// 候选锚点块（`selection` 为空时）同理：旧实现在没有编辑中多边形时**每帧**
/// 把 1024² 地形整份搬进 `TerrainGrid` 再跑 `biome_region_at`。现在按
/// `(候选格, 多边形指纹)` 缓存派生结果。
#[derive(Resource, Default)]
pub struct RegionDecorCache {
    key: Option<DecorKey>,
    /// 视空间阶梯边（白色实线）——合批 Mesh2d 直接消费（`region_decor`）。
    edges: Vec<(Vec2, Vec2)>,
    /// 阶梯边**内容指纹**（只在 `edges` 重算时更新）。
    ///
    /// 为什么不每帧现算：选中一个大区域时边数可上万，逐帧 `4 × E` 次
    /// `to_bits` 会把「改成合批 mesh」省下的钱又花回去。它只随 `DecorKey`
    /// 变，故与 `edges` 一起写；绘制侧（`region_decor`）拿它当重建判据。
    edges_fp: u64,
    /// 派生用的多边形（图内局部 wu，`chunks_exact(2)` → 顶点对）。
    pairs: Vec<(f32, f32)>,
    /// 候选多边形（`RegionCandidateContext.polygon` 缺失时按格派生）。
    cand_key: Option<CandidateKey>,
    cand_pairs: Vec<(f32, f32)>,
    /// 选中区域的**手柄锚点**缓存（世界坐标）与它的键。
    ///
    /// 2026-10-23 QA AC22：`decimate_poly` 原来每帧跑一次（阶梯顶点数千点 ⇒
    /// O(顶点数) 比较 + 一次 `Vec<Vec2>` 分配）。抽稀结果只依赖
    /// `(多边形, spacing, map_origin)`——`spacing` 又只依赖 `view.scale` 与
    /// `cell_size`，所以这三样进键即可（不能只按多边形指纹，缩放会让 spacing 变）。
    anchors_key: Option<AnchorKey>,
    anchors: Vec<Vec2>,
    /// 未物化候选的锚点缓存（同上）。
    cand_anchors_key: Option<AnchorKey>,
    cand_anchors: Vec<Vec2>,
    /// **计数器**（QA 断言口径）：`rasterize_polygon_cells` 调用次数（累计）。
    pub rasterizes: u64,
    /// **计数器**：候选多边形按格派生的次数（累计）。
    pub candidate_derivations: u64,
    /// **计数器**：`decimate_poly`（锚点抽稀）调用次数（累计）。
    pub anchor_decimates: u64,
    /// **计数器**：本帧画出的顶点手柄数（每帧开头归零）。
    pub handles_drawn: u64,
    /// **计数器**：本帧画出的阶梯描边线段数（每帧开头归零）。
    pub outline_segments_drawn: u64,
}

/// 选中区域装饰的缓存键（任何一项变 ⇒ 重算一次）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DecorKey {
    id: String,
    /// 多边形（含拖动中的预览多边形）的位模式指纹。
    poly: u64,
    grid_bits: [u64; 7],
    /// 栅格幅面（`rasterize_polygon_cells` 的边界）。
    raster: (u32, u32),
}

/// 候选锚点的缓存键。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateKey {
    cell: (i32, i32),
    /// 编辑中多边形的指纹（`None` 编码成一个不可能与真实指纹相撞的常量）。
    poly: Option<u64>,
    grid_bits: [u64; 7],
    raster: (u32, u32),
}

/// **手柄锚点**的缓存键：抽稀结果只依赖这四样。
///
/// `src` = 多边形来源指纹（选中取 `DecorKey::poly`，候选取 `CandidateKey::poly`）；
/// **`scale_bucket` = 缩放的 1/2 octave 档号**（`region_outline::scale_bucket`，与轮廓
/// 线宽共用同一把尺子）——**不是**连续 `scale`/`spacing`：手势惯性、zoom-to-bounds
/// 补间每帧只动 1–2%，塞连续值等于没缓存（QA AC22b 抓的就是这个）；
/// `cell_size` = 抽稀间距的第二个输入；`origin` = 图内局部 wu → 世界坐标的平移量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AnchorKey {
    src: u64,
    /// `round(log2(scale) × 2)`：档宽 ×1.414，档内锚点密度差 ±19%（与线宽同一条论证）。
    scale_bucket: i16,
    cell_size: u64,
    origin: [u64; 2],
}

impl AnchorKey {
    fn new(src: u64, scale_bucket: i16, cell_size: f32, origin: Vec2) -> Self {
        Self {
            src,
            scale_bucket,
            cell_size: u64::from(cell_size.to_bits()),
            origin: [u64::from(origin.x.to_bits()), u64::from(origin.y.to_bits())],
        }
    }
}

/// 多边形顶点（扁平 `[x, y, …]`，图内局部 wu）的位模式指纹。
fn polygon_bits_fingerprint(poly: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in poly.iter().map(|f| f.to_bits()) {
        h ^= u64::from(v);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 选中区域的**顶点手柄**是否该画。
///
/// 2026-10-23 用户裁定（缺陷 3）：关掉「群系视图」（`ViewState.nature_overlay`）
/// 后，顶点手柄（2026-10-24 起为**实心白色小方块**，此前是红十字 + 外圈）
/// 与候选高亮**一起隐藏**；**选中态本身保留**
/// （`projection.selection` 不动 → Inspector 仍显示选中的 Region，不写世界、
/// 不清数据）。所以这是一个**纯渲染可见性**判定，放在渲染层。
///
/// 阶梯描边与手柄同源同语义（都是「数据视图里的选中装饰」），一并受此门控。
pub fn region_handles_visible(mode: crate::state::ViewMode, nature_overlay: bool) -> bool {
    matches!(mode, crate::state::ViewMode::Topographic) && nature_overlay
}

/// 派生选中区域的装饰几何（阶梯描边 + 顶点），写进 [`RegionDecorCache`]。
///
/// 返回是否有可画的内容。**键不变 ⇒ 一次都不重算**（`rasterizes` 不增）：
/// 2026-10-23 用户实测「鼠标在地图上移动 120→40 FPS」，旧实现每帧都做
/// `polygon.clone()` + `rasterize_polygon_cells` + `HashSet` + 四邻边扫描。
///
/// 与绘制分离的唯一理由：**可测**——不需要 `Gizmos` 就能断言缓存命中。
fn selected_region_decor(
    projection: &WorldProjection,
    region_adjust: &crate::state::RegionAdjustState,
    grid_cfg: &GridConfig,
    terrain: &wv_core::terrain::TerrainData,
    map: &wv_core::Id,
    decor: &mut RegionDecorCache,
) -> bool {
    let Some(sel) = projection.selection.as_ref() else {
        return false;
    };
    if sel.kind != wv_core::EntityKind::Region {
        return false;
    }
    let Some(region) = projection
        .world
        .regions
        .iter()
        .find(|r| r.id == sel.id && r.map == *map)
        .filter(|r| r.polygon.len() >= 6)
    else {
        return false;
    };
    // 拖动中显示预览多边形；未拖动显示落盘轮廓（借用，不 clone）。
    let poly: &[f32] = region_adjust
        .preview_polygon
        .as_deref()
        .unwrap_or(region.polygon.as_slice());
    let key = DecorKey {
        id: sel.id.as_str().to_string(),
        poly: polygon_bits_fingerprint(poly),
        grid_bits: grid_bits(grid_cfg),
        raster: (terrain.width, terrain.height),
    };
    if decor.key.as_ref() != Some(&key) {
        let raster_to_map = grid_cfg.map_origin - grid_cfg.raster_origin;
        let poly_pairs: Vec<(f32, f32)> = poly.chunks_exact(2).map(|c| (c[0], c[1])).collect();
        let cells = crate::cell_selection::rasterize_polygon_cells(
            &poly_pairs,
            raster_to_map,
            grid_cfg,
            terrain.width,
            terrain.height,
        );
        decor.rasterizes += 1;
        // ── 方格描边：mask 内格与不在 mask 的 4 邻之间的边（白色实线贴格边）──
        let mask: std::collections::HashSet<(i32, i32)> = cells.iter().copied().collect();
        let cs = grid_cfg.cell_size;
        let mut edges: Vec<(Vec2, Vec2)> = Vec::new();
        for &(gx, gy) in &cells {
            let x0 = grid_cfg.raster_origin.x + gx as f32 * cs;
            let y0 = grid_cfg.raster_origin.y + gy as f32 * cs;
            let x1 = x0 + cs;
            let y1 = y0 + cs;
            let c = (gx, gy);
            if !mask.contains(&(c.0, c.1 - 1)) {
                edges.push((Vec2::new(x0, y0), Vec2::new(x1, y0)));
            }
            if !mask.contains(&(c.0, c.1 + 1)) {
                edges.push((Vec2::new(x0, y1), Vec2::new(x1, y1)));
            }
            if !mask.contains(&(c.0 - 1, c.1)) {
                edges.push((Vec2::new(x0, y0), Vec2::new(x0, y1)));
            }
            if !mask.contains(&(c.0 + 1, c.1)) {
                edges.push((Vec2::new(x1, y0), Vec2::new(x1, y1)));
            }
        }
        decor.edges_fp = segments_fp(&edges);
        decor.edges = edges;
        decor.pairs = poly_pairs;
        decor.key = Some(key);
    }
    !decor.edges.is_empty() || !decor.pairs.is_empty()
}

/// 线段列的内容指纹（FNV-1a，与道路 / 小河预览同款口径）。
fn segments_fp(segments: &[(Vec2, Vec2)]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (a, b) in segments {
        for v in [a.x, a.y, b.x, b.y] {
            h ^= u64::from(v.to_bits());
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 手柄方块**半边**长（屏幕 px）。
///
/// 用户裁定（2026-10-24）：锚点改成「实心白色的小方块」。7px 见方在 24px 的
/// 锚点间距下互不粘连，又比 1px 的格线明显——再大就压住它本来要指的那条格边。
const HANDLE_HALF_PX: f32 = 3.5;

/// 一帧的区域手柄**计划**（纯函数产出，`draw_map_gizmos` 只消费）。
///
/// 为什么要这一层（2026-10-23 · 质量第 3 轮）：`draw_map_gizmos` 有 `Gizmos` 参数，
/// 无头测试跑不起来 ⇒「关群系视图后手柄没了」只能靠人工看。把判定 + 派生抽成
/// 纯函数后，测试可以直接断言 `plan.anchors.is_empty()`。
///
/// 另外：三处 gate（选中手柄 / 阶梯描边 / 候选锚点）在这里**收口成一处**（N8）。
#[derive(Debug, Default)]
pub struct RegionHandlePlan<'a> {
    /// 选中区域的阶梯描边（视空间线段；借缓存，不 clone）。
    pub stepped_edges: &'a [(Vec2, Vec2)],
    /// 选中区域的顶点手柄（**世界坐标**，已按 zoom 抽稀；借缓存，不 clone）。
    pub anchors: &'a [Vec2],
    /// 未物化候选的顶点手柄（世界坐标；借缓存）。
    pub candidate_anchors: &'a [Vec2],
    /// 阶梯边的**内容指纹**（借缓存的 [`RegionDecorCache::edges_fp`]）。
    ///
    /// 绘制侧（`region_decor::sync_region_decor`）拿它当合批 mesh 的重建判据——
    /// 键不变 ⇒ 不重建、不重新装配 mesh，稳态每帧只付一次 draw call。
    /// 无阶梯边（未选中 / 关群系视图）时为 `0`。
    pub edges_fp: u64,
    /// 手柄方块**半边长**（世界单位 = 屏幕 3.5px 折算 ⇒ 方块 7px 见方，按缩放档中心求值）。
    ///
    /// 用档中心而不是 `view.scale` 原值：smooth zoom 每帧微动（`×1.02` 一档），
    /// 不然方块 mesh 会跟着每帧重建一次（与轮廓线宽同一条 1/2 octave 口径）。
    pub handle_px: f32,
}

impl RegionHandlePlan<'_> {
    /// 本帧要画的手柄总数（选中 + 候选）。
    pub fn handle_count(&self) -> usize {
        self.anchors.len() + self.candidate_anchors.len()
    }
}

/// 产出本帧的区域手柄计划。
///
/// `nature_overlay == false` ⇒ 三个几何字段全空（**选中态保持不变**，本函数
/// 只读 `projection.selection`）。缓存由调用方持有（`RegionDecorCache`，
/// `Resource` + `Default`）：同一份缓存连续调用、输入不变时 `rasterizes` 不增。
#[allow(clippy::too_many_arguments)]
pub fn plan_region_handles<'a>(
    projection: &WorldProjection,
    drill: &crate::state::DrillState,
    view: &ViewState,
    grid_cfg: &GridConfig,
    region_adjust: &crate::state::RegionAdjustState,
    candidate_ctx: &crate::state::RegionCandidateContext,
    decor: &'a mut RegionDecorCache,
) -> RegionHandlePlan<'a> {
    let mut plan = RegionHandlePlan::default();
    // ← 唯一一处 gate：关群系视图 ⇒ 手柄 / 阶梯描边 / 候选锚点一起隐藏。
    if !region_handles_visible(view.mode, view.nature_overlay) {
        return plan;
    }
    let Some(map) = drill.current().cloned() else {
        return plan;
    };
    let Some(terrain) = projection.world.terrain_of(&map) else {
        return plan;
    };
    // 不适用时清掉上一帧的锚点（否则选中被取消 / 光标离开候选之后，旧手柄会继续被画）。
    if !(projection.selection.is_none() && candidate_ctx.cell.is_some()) {
        decor.cand_anchors.clear();
        decor.cand_anchors_key = None;
    }
    // 抽稀间距按 **1/2 octave 档中心**的 scale 求值：smooth zoom 每帧微动
    // （`×1.02` 这一档）不能把锚点缓存打穿（QA AC22 / REQ-8：同档 Δ==0、跨档恰好 1 次）。
    // 与轮廓线宽共用同一个档函数（同一事实只留一份载体）。
    let scale_bucket = crate::region_outline::scale_bucket(view.scale);
    let bucket_scale = crate::region_outline::scale_of_bucket(scale_bucket);
    let spacing = crate::biome_boundary::anchor_spacing(bucket_scale, grid_cfg.cell_size, 24.0);
    // 手柄方块尺寸同样按档中心求值（见 `handle_px` 的文档：不然方块 mesh 每帧重建）。
    plan.handle_px = (bucket_scale * HANDLE_HALF_PX).max(grid_cfg.cell_size * 0.2);
    // ① 候选（未物化）手柄：`selection` 为空时才画（有选中以选中为准）。
    //    按 `(候选格, 多边形指纹, 网格, 幅面)` 缓存——否则每帧搬一遍整张地形。
    if projection.selection.is_none() {
        if let Some(cell) = candidate_ctx.cell {
            let cand_key = CandidateKey {
                cell,
                poly: candidate_ctx
                    .polygon
                    .as_deref()
                    .map(polygon_bits_fingerprint),
                grid_bits: grid_bits(grid_cfg),
                raster: (terrain.width, terrain.height),
            };
            if decor.cand_key.as_ref() != Some(&cand_key) {
                let pairs: Vec<(f32, f32)> = match candidate_ctx.polygon.as_deref() {
                    Some(p) => p.chunks_exact(2).map(|c| (c[0], c[1])).collect(),
                    None => {
                        decor.candidate_derivations += 1;
                        let raster_to_map = grid_cfg.map_origin - grid_cfg.raster_origin;
                        // 按格派生（与物化同源）：biome 连通区外环 × 格尺寸。
                        let cells: Vec<wv_terrain::TerrainCell> = terrain
                            .cells
                            .iter()
                            .map(|c| wv_terrain::TerrainCell {
                                height: c.height,
                                surface: c.surface,
                                biome: c.biome,
                            })
                            .collect();
                        wv_terrain::TerrainGrid::from_cells(terrain.width, terrain.height, cells)
                            .and_then(|g| {
                                wv_terrain::region::biome_region_at(&g, cell.0, cell.1 as i64)
                            })
                            .map(|r| {
                                r.outer
                                    .iter()
                                    .flat_map(|&(x, y)| {
                                        [
                                            x * grid_cfg.cell_size + raster_to_map.x,
                                            y * grid_cfg.cell_size + raster_to_map.y,
                                        ]
                                    })
                                    .collect::<Vec<f32>>()
                            })
                            .unwrap_or_default()
                            .chunks_exact(2)
                            .map(|c| (c[0], c[1]))
                            .collect()
                    }
                };
                decor.cand_pairs = pairs;
                decor.cand_key = Some(cand_key);
            }
            if decor.cand_pairs.len() >= 3 {
                let akey = AnchorKey::new(
                    decor
                        .cand_key
                        .as_ref()
                        .map(|k| k.poly.unwrap_or(0))
                        .unwrap_or(0),
                    scale_bucket,
                    grid_cfg.cell_size,
                    grid_cfg.map_origin,
                );
                if decor.cand_anchors_key != Some(akey) {
                    decor.cand_anchors_key = Some(akey);
                    decor.anchor_decimates += 1;
                    decor.cand_anchors =
                        crate::biome_boundary::decimate_poly(&decor.cand_pairs, spacing)
                            .into_iter()
                            .map(|(x, y)| grid_cfg.map_origin + Vec2::new(x, y))
                            .collect();
                }
            } else {
                decor.cand_anchors.clear();
                decor.cand_anchors_key = None;
            }
        }
    }
    // ② 选中区域：派生（带缓存）+ 锚点抽稀（**也带缓存**，AC22）。
    if selected_region_decor(projection, region_adjust, grid_cfg, terrain, &map, decor) {
        let akey = AnchorKey::new(
            decor.key.as_ref().map(|k| k.poly).unwrap_or(0),
            scale_bucket,
            grid_cfg.cell_size,
            grid_cfg.map_origin,
        );
        if decor.anchors_key != Some(akey) {
            decor.anchors_key = Some(akey);
            decor.anchor_decimates += 1;
            decor.anchors = crate::biome_boundary::decimate_poly(&decor.pairs, spacing)
                .into_iter()
                .map(|(x, y)| grid_cfg.map_origin + Vec2::new(x, y))
                .collect();
        }
        // 最后才借出（之后本函数不再改 `decor`）。
        plan.stepped_edges = &decor.edges;
        plan.edges_fp = decor.edges_fp;
        plan.anchors = &decor.anchors;
    } else {
        decor.anchors.clear();
        decor.anchors_key = None;
    }
    plan.candidate_anchors = &decor.cand_anchors;
    plan
}

#[cfg(test)]
mod region_handles_tests {
    use super::region_handles_visible;
    use crate::state::ViewMode;

    /// 断言 2 的门控部分：关掉群系视图 ⇒ 手柄不画（选中态本身不动，见下一条）。
    #[test]
    fn handles_hidden_when_nature_overlay_is_off() {
        assert!(
            !region_handles_visible(ViewMode::Topographic, false),
            "关群系视图后顶点手柄必须隐藏（缺陷 3）"
        );
    }

    #[test]
    fn handles_visible_in_cellmap_with_overlay_on() {
        assert!(region_handles_visible(ViewMode::Topographic, true));
    }

    #[test]
    fn handles_never_visible_outside_cellmap() {
        for mode in [ViewMode::VectorFlat, ViewMode::Faction, ViewMode::GoogleMap] {
            assert!(
                !region_handles_visible(mode, true),
                "{mode:?} 不是数据编辑器，不该出手柄"
            );
        }
    }

    /// 门控只看 `nature_overlay`，**不读也不写** `selection`——这条测试钉住
    /// 「选中态保留」的口径：判定函数是纯函数，改开关不会清 selection。
    #[test]
    fn gate_is_pure_and_does_not_touch_selection() {
        // 同一入参反复求值结果恒定（无内部状态）。
        for _ in 0..3 {
            assert!(!region_handles_visible(ViewMode::Topographic, false));
            assert!(region_handles_visible(ViewMode::Topographic, true));
        }
    }

    /// 断言 1 的「选中区域栅格化 == 0」：选中态 / 多边形 / 网格都不变时，
    /// 60 次派生只允许 `rasterize_polygon_cells` **一次**（首帧）。
    #[test]
    fn selected_region_decor_is_derived_once_then_cached() {
        use super::{selected_region_decor, RegionDecorCache};
        use crate::state::{GridConfig, RegionAdjustState, WorldProjection};
        use bevy::math::Vec2;
        use wv_core::map::{Extent, MapLevel, MapNode};
        use wv_core::model::region::{Region, RegionKind};

        let map_id = wv_core::Id::new_unchecked("map_world");
        let region_id = wv_core::Id::new_unchecked("nr_a");
        let mut projection = WorldProjection::default();
        projection.world.maps.push(MapNode {
            id: map_id.clone(),
            name: "世界".into(),
            level: MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(Extent::sized(256.0, 256.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        });
        projection.world.regions.push(Region {
            id: region_id.clone(),
            name: "甲".into(),
            map: map_id.clone(),
            polygon: vec![8.0, 8.0, 40.0, 8.0, 40.0, 40.0, 8.0, 40.0],
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        });
        projection.selection = Some(wv_core::SelectedEntity::new(
            wv_core::EntityKind::Region,
            region_id,
        ));

        let grid = GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(256.0),
        };
        let terrain = wv_core::terrain::TerrainData {
            width: 64,
            height: 64,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 100,
                    surface: 1,
                    biome: 3,
                };
                64 * 64
            ],
        };
        let adjust = RegionAdjustState::default();
        let mut decor = RegionDecorCache::default();

        assert!(
            selected_region_decor(&projection, &adjust, &grid, &terrain, &map_id, &mut decor),
            "首帧应派生成功"
        );
        assert_eq!(decor.rasterizes, 1, "首帧应栅格化一次");
        assert!(!decor.edges.is_empty(), "方形区域应出阶梯边");
        let edges0 = decor.edges.len();
        let pairs0 = decor.pairs.len();

        // 「鼠标在地图上移动 60 帧」= 每帧都重跑派生、输入完全不变。
        for _ in 0..60 {
            assert!(selected_region_decor(
                &projection,
                &adjust,
                &grid,
                &terrain,
                &map_id,
                &mut decor
            ));
        }
        assert_eq!(decor.rasterizes, 1, "键不变时不该再栅格化");
        assert_eq!(decor.edges.len(), edges0, "阶梯边不该变");
        assert_eq!(decor.pairs.len(), pairs0, "多边形不该变");

        // 阳性对照：换选中区域 ⇒ 必须重算一次（证明上一行的 1 不是假绿）。
        projection.world.regions.push(Region {
            id: wv_core::Id::new_unchecked("nr_b"),
            name: "乙".into(),
            map: map_id.clone(),
            polygon: vec![64.0, 64.0, 96.0, 64.0, 96.0, 96.0, 64.0, 96.0],
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        });
        projection.selection = Some(wv_core::SelectedEntity::new(
            wv_core::EntityKind::Region,
            wv_core::Id::new_unchecked("nr_b"),
        ));
        assert!(selected_region_decor(
            &projection,
            &adjust,
            &grid,
            &terrain,
            &map_id,
            &mut decor
        ));
        assert_eq!(decor.rasterizes, 2, "换选中区域应重算一次");
    }

    /// 断言 2 的**计划面**（2026-10-23 质量第 3 轮要求的可断言接口）：
    /// `plan_region_handles` 在关群系视图时必须吐**空**，开时必须有元素。
    ///
    /// 两种取值都断言——否则「恒空」也能过。这条是「关掉开关后红十字还留着」
    /// 在无头环境里能钉到的最硬一层（`draw_map_gizmos` 有 `Gizmos`，跑不起来）。
    #[test]
    fn plan_is_empty_when_overlay_off_and_populated_when_on() {
        use super::{plan_region_handles, RegionDecorCache};
        use crate::state::{
            DrillState, GridConfig, RegionAdjustState, RegionCandidateContext, ViewMode, ViewState,
            WorldProjection,
        };
        use bevy::prelude::Vec2;
        use wv_core::map::{Extent, MapLevel, MapNode};
        use wv_core::model::region::{Region, RegionKind};

        let map_id = wv_core::Id::new_unchecked("map_world");
        let mut projection = WorldProjection::default();
        projection.world.maps.push(MapNode {
            id: map_id.clone(),
            name: "世界".into(),
            level: MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(Extent::sized(256.0, 256.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        });
        projection.world.regions.push(Region {
            id: wv_core::Id::new_unchecked("nr_a"),
            name: "甲".into(),
            map: map_id.clone(),
            polygon: vec![8.0, 8.0, 40.0, 8.0, 40.0, 40.0, 8.0, 40.0],
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        });
        projection.world.set_terrain(
            map_id.clone(),
            wv_core::terrain::TerrainData {
                width: 64,
                height: 64,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height: 100,
                        surface: 1,
                        biome: 3,
                    };
                    64 * 64
                ],
            },
        );
        projection.selection = Some(wv_core::SelectedEntity::new(
            wv_core::EntityKind::Region,
            wv_core::Id::new_unchecked("nr_a"),
        ));
        let mut drill = DrillState::default();
        drill.settle_on(map_id.clone());
        let grid = GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(256.0),
        };
        let adjust = RegionAdjustState::default();
        let cand = RegionCandidateContext::default();
        let mut decor = RegionDecorCache::default();

        // 开：有锚点、有阶梯边。
        let view_on = ViewState {
            mode: ViewMode::Topographic,
            nature_overlay: true,
            scale: 1.0,
            ..Default::default()
        };
        let plan = plan_region_handles(
            &projection,
            &drill,
            &view_on,
            &grid,
            &adjust,
            &cand,
            &mut decor,
        );
        assert!(plan.handle_count() > 0, "开群系视图 + 选中 ⇒ 必须有手柄");
        assert!(!plan.stepped_edges.is_empty(), "选中区域应出阶梯描边");

        // 关：三个几何字段全空，但选中态在 projection 里**原样保留**。
        let view_off = ViewState {
            mode: ViewMode::Topographic,
            nature_overlay: false,
            scale: 1.0,
            ..Default::default()
        };
        let plan = plan_region_handles(
            &projection,
            &drill,
            &view_off,
            &grid,
            &adjust,
            &cand,
            &mut decor,
        );
        assert_eq!(
            plan.handle_count(),
            0,
            "关群系视图 ⇒ 手柄必须为空（缺陷 3）"
        );
        assert!(plan.stepped_edges.is_empty(), "阶梯描边同门控");
        assert!(
            projection.selection.is_some(),
            "门控是纯渲染判定，不得清选中态"
        );
    }
}

/// 绘制视口网格与唯一的地形四边形（S3）。用 `Gizmos` 画（原型轻量方案）。
///
/// 事件高亮读 [`LayerFrame`]（ADR-0013）：visible 门 + opacity 乘进 alpha。
/// 事件属 `Gizmos` 即时线元，**没有每实体 Z**——`EventPoint` 的 `z` 偏移
/// 在 Gizmos 路径不生效（ADR-0013 待裁项）。
#[allow(clippy::too_many_arguments)]
pub fn draw_map_gizmos(
    mut gizmos: Gizmos,
    tex: Res<crate::terrain_view::TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    frame: Res<ProjectionFrame>,
    layers: Res<LayerFrame>,
    recipe: Res<crate::recipe::RenderRecipe>,
    projection: Res<crate::state::WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid_cfg: Res<GridConfig>,
    mut candidate_cache: ResMut<crate::region_overlay::CandidateOverlayCache>,
    boundary: Res<crate::state::BiomeBoundaryState>,
) {
    let _ = &*tex; // suppress unused warning

    // ── CellMap 未命名候选高亮（2026-10-22 内容矩阵 §2.6-3 · T11）──────
    // 仅 Topographic ∧ nature_overlay；重算触发 = 编辑信号**边沿**（脏标记 /
    // undo/redo 栈形 / 换图）且限频 ≥1s——不用 `WorldProjection::is_changed`
    // （selection 写入也置真，O(格数) 的重算会被打成每帧 = 实机 1 FPS 根因）。
    if matches!(view.mode, crate::state::ViewMode::Topographic) && view.nature_overlay {
        let current = drill.current().cloned();
        let stale = candidate_cache.last_epoch != candidate_cache.drawn_epoch;
        let throttled = candidate_cache
            .last_compute
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1));
        let map_changed = candidate_cache.key.as_ref() != current.as_ref();
        if (stale && !throttled) || map_changed {
            if let Some(map) = current {
                let raster_to_map = grid_cfg.map_origin - grid_cfg.raster_origin;
                let t0 = std::time::Instant::now();
                let result = crate::region_overlay::enumerate_candidates(
                    &projection.world,
                    &map,
                    &grid_cfg,
                    raster_to_map,
                );
                candidate_cache.key = Some(map);
                candidate_cache.outlines = result.outlines;
                candidate_cache.drawn_epoch = candidate_cache.last_epoch;
                candidate_cache.last_compute = Some(t0);
            }
        }

        // 用户裁定（2026-10-22 验收）：候选高亮 = **白色虚线**，不用金色——
        // 沙色地形上金色与地貌色混淆；白色虚线在任意群系色上都可辨。
        let white = bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.9);
        for outline in candidate_cache.outlines.iter() {
            for (a, b) in outline {
                gizmos.line_2d(*a, *b, white);
            }
        }
    }

    // ── 选中区域 + 未物化候选的装饰：**已迁出 Gizmos**（2026-10-24）──────
    // 用户实测：开「显示自然区与候选」后点选一个大自然区，锚点一出现 FPS 掉到
    // 20 多。根因是这里**逐条** `gizmos.line_2d` 画阶梯边——大区域的格边数上万，
    // Gizmos 是即时线元（每条一次装配 + 一个实例），每帧重建一遍就吃掉了帧预算。
    //
    // 现在由 `crate::region_decor::sync_region_decor` 用**合批 Mesh2d** 画：
    // 键（内容指纹 + 线宽/方块尺寸档）不变 ⇒ 不重建，稳态每帧只付一个 draw call。
    // 顺带满足用户裁定「锚点改为实心白色小方块」——Gizmos 画不出填充面。
    //
    // 判定与派生仍全在 `plan_region_handles`（纯函数 + 缓存）里，调用点是
    // `sync_region_decor`；`handles_drawn` / `outline_segments_drawn` 两个
    // 计数器也随绘制一起搬过去（同一事实只留一处载体）。

    // ── 群系分界线预览（2026-10-22 内容矩阵 §2.7 · T12）────────────────
    // 工具激活：线（金色实线 + 两端点圆）+ 拖动中高亮「将被改写」的格。
    // 高亮与提交共用 `swept_changes`——预览说什么、松手改什么。
    if boundary.tool_active {
        let local = |p: bevy::math::Vec2| p + grid_cfg.raster_origin;
        if let Some(line) = boundary.drag_line.or(boundary.line) {
            let a = local(bevy::math::Vec2::new(line.a.0, line.a.1));
            let b = local(bevy::math::Vec2::new(line.b.0, line.b.1));
            let side_a_color = crate::terrain_view::biome_raw::pick(line.side_a, 1);
            let side_b_color = crate::terrain_view::biome_raw::pick(line.side_b, 1);
            let ca =
                bevy::color::Color::srgba(side_a_color[0], side_a_color[1], side_a_color[2], 0.9);
            let cb =
                bevy::color::Color::srgba(side_b_color[0], side_b_color[1], side_b_color[2], 0.9);
            // 线段本体：中点分两半，各染侧色（左半 side_a / 右半 side_b）。
            let mid = (a + b) * 0.5;
            gizmos.line_2d(a, mid, ca);
            gizmos.line_2d(mid, b, cb);
            gizmos.circle_2d(
                bevy::math::Isometry2d::from_translation(a),
                grid_cfg.cell_size * 0.4,
                ca,
            );
            gizmos.circle_2d(
                bevy::math::Isometry2d::from_translation(b),
                grid_cfg.cell_size * 0.4,
                cb,
            );

            // 拖动中：高亮将被改写的格（世界系矩形）。
            if let (Some(old_line), true) = (boundary.line, boundary.drag_line.is_some()) {
                if let Some(terrain) = projection
                    .world
                    .terrain_of(&projection.world.maps.first().map(|m| m.id.clone()).unwrap())
                {
                    let changes = crate::biome_boundary::swept_changes(
                        &old_line,
                        &line,
                        terrain,
                        grid_cfg.cell_size,
                    );
                    let warn = bevy::color::Color::srgba(0.95, 0.75, 0.2, 0.4);
                    for (gx, gy, _, _) in changes {
                        let center = bevy::math::Vec2::new(
                            grid_cfg.raster_origin.x + (gx as f32 + 0.5) * grid_cfg.cell_size,
                            grid_cfg.raster_origin.y + (gy as f32 + 0.5) * grid_cfg.cell_size,
                        );
                        gizmos.rect_2d(
                            bevy::math::Isometry2d::from_translation(center),
                            bevy::math::Vec2::splat(grid_cfg.cell_size),
                            warn,
                        );
                    }
                }
            }
        }
    }
    if false {
        if !view.terrain_visible {
            return;
        }
        // 地图轮廓：边框用反白描线（深底上可见——LINE_ON_MID 是给浅底地形用的，
        // 在深色空白工程上等于隐形），并补 1/4 幅面参考网格，避免「一片黑看不到图在哪」。
        // 画的是**当前图**的幅面（与 `map_frame::sync_map_frame` 同口径，现由它接替）。
        let size = grid.map_size.x.max(grid.map_size.y);
        let min = grid.map_origin;
        let max = grid.map_origin + Vec2::splat(size);
        gizmos.rect_2d(
            Isometry2d::from_translation((min + max) * 0.5),
            Vec2::splat(size),
            view.active_theme.palette.line_on_dark,
        );
        let step = size / 4.0;
        for i in 1..4 {
            let x = min.x + step * i as f32;
            gizmos.line_2d(
                Vec2::new(x, min.y),
                Vec2::new(x, max.y),
                view.active_theme.palette.line_on_dark.with_alpha(0.35),
            );
            let y = min.y + step * i as f32;
            gizmos.line_2d(
                Vec2::new(min.x, y),
                Vec2::new(max.x, y),
                view.active_theme.palette.line_on_dark.with_alpha(0.35),
            );
        }
    }

    // 道路：地点边直线（在事件/轨迹之下，视觉上先铺路网再叠动态要素）。
    //
    // **已迁出 Gizmos**：`Gizmos` 线宽是屏幕 1px 常量，放大后变粗、出现锯齿。
    // 现由 [`crate::vector_layers::sync_vector_layers`] 写到世界空间 Mesh2d，
    // 线宽随 `view.scale` 反比缩放（屏幕恒定 1px）。`if false` 保留原实现
    // 作为回退开关——若新实现出问题，把 `false` 改 `true` 即可恢复。
    if false && view.roads_visible {
        for (a, b) in &frame.road_segments {
            gizmos.line_2d(*a, *b, colors::ACCENT_NOTE.with_alpha(0.9));
        }
    }

    // 地点（POI）点标：白圈 + 深芯（导航地图惯例；不占用任何强调色的语义）。
    //
    // **已迁出 Gizmos**：同上路层理由（屏幕 1px 描线 + 抗锯齿圆）。
    // 现由 [`crate::vector_layers::sync_vector_layers`] 写 Circle / Ring mesh。
    if false && view.places_visible {
        for dot in &frame.place_dots {
            let r = grid.cell_size * 0.9;
            gizmos.circle_2d(
                Isometry2d::from_translation(dot.pos),
                r,
                view.active_theme.palette.line_on_dark,
            );
            gizmos.circle_2d(
                Isometry2d::from_translation(dot.pos),
                r * 0.5,
                view.active_theme.palette.line_on_mid,
            );
        }
    }

    // 事件高点亮：常驻底环 + 内芯（特效 §4.2：状态永不抑制）。
    // 图层门 + 透明度走 LayerFrame（ADR-0013）；Gizmos 无逐实体 Z，z 偏移不适用。
    // 2026-10-22 内容矩阵：接 MODE_TABLE(EventDots)——Topographic/Schematic
    // 下不画（事件有时间性，编辑态隐藏；用户裁定）。
    if layers.event_point.visible && recipe.enabled(crate::recipe::PassId::EventDots) {
        let alpha = layers.event_point.opacity;
        for (_id, pos) in &frame.active_events {
            gizmos.circle_2d(
                Isometry2d::from_translation(*pos),
                grid.cell_size * 1.6,
                colors::ACCENT_EVENT.with_alpha(0.25 * alpha),
            );
            gizmos.circle_2d(
                Isometry2d::from_translation(*pos),
                grid.cell_size * 0.7,
                colors::ACCENT_EVENT.with_alpha(alpha),
            );
        }
    }

    // 轨迹：已生长折线（`accent.event`）+ 生长笔尖（`accent.cursor`）。
    if view.tracks_visible {
        for poly in &frame.track_polylines {
            for w in poly.windows(2) {
                gizmos.line_2d(w[0], w[1], colors::ACCENT_EVENT);
            }
        }
        for head in &frame.track_heads {
            gizmos.circle_2d(
                Isometry2d::from_translation(*head),
                grid.cell_size * 0.75,
                colors::ACCENT_CURSOR,
            );
        }
        for s in &frame.track_hollow_starts {
            gizmos.circle_2d(
                Isometry2d::from_translation(*s),
                grid.cell_size * 0.6,
                view.active_theme.palette.line_on_light,
            );
        }
    }
}

/// 一个地点在本帧应生成的 POI 点标；`None` = 不画。
///
/// **标记（`biome = Some(_)`）抑制 POI 圆点**（设计稿 §6.4）：标记由
/// `crate::feature_overlays` 画地界晕与近景符号，再叠一个 POI 圆点只会让
/// 覆盖层中心出现一颗没有语义的红点。普通地点保持原行为：锚定且有坐标 →
/// 点标，`w/h` 齐备 → 城市级。
///
/// 迁移窗口内（ADR-0032 代价节）保留 `feature` 双条件：未经加载器的手工世界若
/// 携带旧字段（含词汇表外垃圾值）仍不冒点；下版本删字段后收敛为单条件。
fn place_dot_for(p: &wv_core::distance::Place, grid: &GridConfig) -> Option<PlaceDot> {
    if p.line.is_some() {
        // 线状地点（ADR-0046 D1）**不是点标**：它的表现是 `place_line` 的折线描边。
        // 旧实现只看 feature / biome，于是作者河会在折线中点白长一个 POI 圆点
        // （小样张夹具此前只能靠 `rank: 0` 把这个点压到 Edit 档来规避）。
        return None;
    }
    if p.feature.is_some() || p.biome.is_some() {
        // 标记（ADR-0029/0032）：覆盖层/标签负责表现，不再生成城市点。
        return None;
    }
    let (x, y) = (p.x?, p.y?);
    Some(PlaceDot {
        pos: grid.map_to_view(Vec2::new(x, y)),
        is_city: p.w.is_some() && p.h.is_some(),
        rank: p.effective_rank(),
        name_len: p.name.chars().count() as u32,
        kind: poi_kind_for(p),
    })
}

/// 有效 POI 类别（ADR-0064）：显式 `kind` 优先；否则聚落折为 `Settlement`。
///
/// 不从 `biome` 折算：带 `biome` 的地点在本函数上游就被排除了（覆盖物负责
/// 表现），此处的输入只会是普通点名点或城市。
fn poi_kind_for(p: &wv_core::distance::Place) -> wv_core::distance::PoiKind {
    use wv_core::distance::PoiKind;
    p.kind.unwrap_or(if p.w.is_some() && p.h.is_some() {
        PoiKind::Settlement
    } else {
        PoiKind::Other
    })
}

#[cfg(test)]
mod tests {
    //! POI 抑制测试：事实地点不生成圆点，普通地点保持原行为。
    use super::*;
    use wv_core::distance::Place;
    use wv_core::id::Id;

    fn place(feature: Option<u8>) -> Place {
        Place {
            kind: None,
            line: None,
            id: Id::new_unchecked("place_test"),
            name: "测试地点".into(),
            feature,
            biome: None,
            envelope: None,
            map: None,
            x: Some(1.0),
            y: Some(2.0),
            w: Some(10.0),
            h: Some(10.0),
            rank: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    /// 生物群系标记（`biome = Some(_)`）→ 不生成 POI 点（ADR-0029）。
    #[test]
    fn biome_marker_produces_no_poi_dot() {
        let mut p = place(None);
        p.biome = Some(wv_terrain::biome::BIOME_FOREST);
        p.feature = None;
        let dot = place_dot_for(&p, &GridConfig::default());
        assert_eq!(dot, None, "标记由覆盖层/标签表现，不得生成城市点");
    }

    /// 事实地点（森林，`feature = Some(4)`）→ 不生成 POI 点。
    #[test]
    fn feature_place_produces_no_poi_dot() {
        let dot = place_dot_for(&place(Some(4)), &GridConfig::default());
        assert_eq!(
            dot, None,
            "feature = Some(4) 的事实地点应由 feature_overlays 表现，不生成 POI 点"
        );
    }

    /// 普通城市（`feature = None`，`w/h` 齐备）→ 点标且标记为城市级。
    #[test]
    fn ordinary_place_still_produces_city_dot() {
        let grid = GridConfig::default();
        let dot = place_dot_for(&place(None), &grid)
            .expect("feature = None 且有坐标的普通地点应生成 POI 点");
        assert!(dot.is_city, "w/h 齐备应标记为城市级");
        assert_eq!(
            dot.pos,
            grid.map_to_view(Vec2::new(1.0, 2.0)),
            "pos 应经地图局部 → 视图空间换算"
        );
    }

    // ---- 线状地点（ADR-0046 作者手绘河）的两条守约（G1 / G2，2026-10-23）----

    /// G2：线状地点**不是点标**——即便它带齐 `x/y`，也不得在折线中点长一个圆点。
    #[test]
    fn linear_place_produces_no_poi_dot() {
        let mut p = place(None);
        p.line = Some(vec![1.0, 2.0, 9.0, 2.0]);
        assert_eq!(
            place_dot_for(&p, &GridConfig::default()),
            None,
            "线状地点由 place_line 画折线，POI 圆点是重复表现"
        );
    }

    /// G1：只写 `line`、不写 `x/y` 的线状地点，也必须进本图锚定桶。
    /// 作者河分支从同一个桶取数据——漏桶 = 整条河一根不画。
    #[test]
    fn line_place_without_xy_enters_the_map_bucket() {
        let mut world = wv_core::World::default();
        let map = Id::new_unchecked("map_a");
        world.distances.places.push(Place {
            kind: None,
            line: Some(vec![0.0, 0.0, 10.0, 0.0]),
            id: Id::new_unchecked("rv_test"),
            name: "测试河".into(),
            map: Some(map.clone()),
            x: None,
            y: None,
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
        });
        let index = PlaceIndex::build(&world);
        assert_eq!(
            index.anchored_by_map.get(&map).map(Vec::len),
            Some(1),
            "锚点应由 effective_anchor 从折线中点派生，地点必须入桶"
        );
    }

    /// 既无锚点又无折线的地点仍不入桶：桶是按图裁剪的入口，不能放垃圾。
    #[test]
    fn place_without_anchor_and_line_stays_out_of_the_bucket() {
        let mut world = wv_core::World::default();
        let map = Id::new_unchecked("map_a");
        world.distances.places.push(Place {
            kind: None,
            line: None,
            id: Id::new_unchecked("p_orphan"),
            name: "无锚点".into(),
            map: Some(map.clone()),
            x: None,
            y: None,
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
        });
        let index = PlaceIndex::build(&world);
        assert_eq!(index.anchored_by_map.get(&map).map(Vec::len), None);
    }

    // ---- compute_projection 道路层 / POI（A1 索引化后的行为锚点）----

    use wv_core::map::distance::PlaceEdge;

    fn map(id: &str) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        }
    }

    fn anchored_place(id: &str, map: &str, x: f32, y: f32) -> Place {
        Place {
            kind: None,
            line: None,
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            feature: None,
            biome: None,
            envelope: None,
            map: Some(Id::new_unchecked(map)),
            x: Some(x),
            y: Some(y),
            w: Some(10.0),
            h: Some(10.0),
            rank: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    fn graph_node(id: &str) -> Place {
        // guangdong 的路口形态：纯距离图节点，不锚定地图、无坐标。
        Place {
            kind: None,
            line: None,
            id: Id::new_unchecked(id),
            name: "路口".into(),
            feature: None,
            biome: None,
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    fn edge(id: &str, a: &str, b: &str) -> PlaceEdge {
        PlaceEdge {
            rank: None,
            id: Id::new_unchecked(id),
            a: Id::new_unchecked(a),
            b: Id::new_unchecked(b),
            distance_km: 1.0,
            // ADR-0070 T8 后直连路径只服务**非 road** 手写边（海路等）：
            // terrain == "road" 的边不再渲染（道路唯一供给源 = 路 POI 线），
            // 夹具因此用海路地形——既有断言（段数 / 端点坐标）原样保留；
            // road 边不渲染由 `legacy_edge_only_world_renders_no_roads` 钉住。
            terrain: "sea".to_owned(),
            bidirectional: true,
            derived: false,
        }
    }

    /// 最小投影装配：单图世界 + 全默认资源，返回 (bevy World, Schedule) 供
    /// 调用方在两次 run 之间改世界（重建触发测试用）。
    fn projection_app(world: wv_core::World) -> (World, Schedule) {
        let mut app = World::default();
        app.insert_resource(PlaybackState::default());
        app.insert_resource(ViewState::default());
        app.insert_resource(GridConfig::default());
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&world);
        app.insert_resource(drill);
        app.insert_resource(WorldProjection {
            world,
            ..Default::default()
        });
        app.insert_resource(LayerFrame::default());
        app.insert_resource(ProjectionFrame::default());
        app.init_resource::<PlaceIndex>();
        // 图钉拖动预览（ADR-0074 追记三）：compute_projection 新增的 Res。
        app.init_resource::<crate::state::PinDragState>();
        let mut schedule = Schedule::default();
        schedule.add_systems(compute_projection);
        (app, schedule)
    }

    fn run_once(app: &mut World, schedule: &mut Schedule) -> ProjectionFrame {
        schedule.run(app);
        app.remove_resource::<ProjectionFrame>().unwrap()
    }

    /// 锚定地点之间的边 → 一条道路段，端点经 `map_to_view` 换算（索引查表口径）。
    #[test]
    fn edge_between_anchored_places_becomes_one_road_segment() {
        let grid = GridConfig::default();
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("pa", "m1", 10.0, 20.0),
            anchored_place("pb", "m1", 30.0, 20.0),
        ];
        world.distances.edges = vec![edge("e1", "pa", "pb")];

        let (mut app, mut schedule) = projection_app(world);
        let frame = run_once(&mut app, &mut schedule);
        assert_eq!(frame.road_segments.len(), 1, "一条边 = 一段");
        assert_eq!(
            frame.road_segments[0],
            (
                grid.map_to_view(Vec2::new(10.0, 20.0)),
                grid.map_to_view(Vec2::new(30.0, 20.0))
            ),
            "端点应是视图空间坐标"
        );
    }

    /// guangdong 回归：不锚定地图、无坐标的路口（纯图节点）参与多少条边，
    /// 都不产生道路段——但扫描成本也必须可忽略（索引未命中即返回）。
    #[test]
    fn edges_with_unanchored_endpoints_draw_nothing() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![graph_node("pi"), graph_node("pj")];
        world.distances.edges = vec![edge("e1", "pi", "pj"), edge("e2", "pj", "pi")];

        let (mut app, mut schedule) = projection_app(world);
        let frame = run_once(&mut app, &mut schedule);
        assert!(
            frame.road_segments.is_empty(),
            "端点未锚定 → 无段，实得 {} 段",
            frame.road_segments.len()
        );
        assert!(
            frame.place_dots.is_empty(),
            "无锚定地点 → 无 POI 点，实得 {} 个",
            frame.place_dots.len()
        );
    }

    /// 世界替换 / 编辑后（`WorldProjection` 变更检测命中）索引必须重建：
    /// 新地点不重建就查不到，新边会静默消失。
    #[test]
    fn place_index_rebuilds_after_world_mutation() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("pa", "m1", 10.0, 20.0),
            anchored_place("pb", "m1", 30.0, 20.0),
        ];
        world.distances.edges = vec![edge("e1", "pa", "pb")];

        let (mut app, mut schedule) = projection_app(world);
        schedule.run(&mut app);
        assert_eq!(
            app.resource::<ProjectionFrame>().road_segments.len(),
            1,
            "首帧一条边"
        );

        // 编辑：新增地点 pc 与边 pb—pc（pc 不在旧索引里，必现重建必要性）。
        let mut projection = app.resource_mut::<WorldProjection>();
        projection
            .world
            .distances
            .places
            .push(anchored_place("pc", "m1", 50.0, 20.0));
        projection
            .world
            .distances
            .edges
            .push(edge("e2", "pb", "pc"));

        schedule.run(&mut app);
        assert_eq!(
            app.resource::<ProjectionFrame>().road_segments.len(),
            2,
            "世界变更后索引重建，新边应可解析"
        );
    }

    /// 档位切片（ADR-0047 D4 手算）：rank [3,2,1,1] 降序前缀——Edit 全量 4 段、
    /// Region 切在 rank<2 处剩 2 段、Nation 剩 1 段；三档指纹互不相同，
    /// 且 Edit 档指纹 == 旧 `road_fingerprint` 语义（全量）。
    #[test]
    fn road_tier_view_cuts_descending_prefixes_per_tier() {
        let seg = |i: f32| (Vec2::new(i, 0.0), Vec2::new(i + 1.0, 1.0));
        let ranked = vec![
            (1u8, seg(3.0).0, seg(3.0).1),
            (3u8, seg(1.0).0, seg(1.0).1),
            (1u8, seg(4.0).0, seg(4.0).1),
            (2u8, seg(2.0).0, seg(2.0).1),
        ];
        let (segments, cuts, fingerprints) = build_road_tier_view(ranked);
        assert_eq!(segments.len(), 4);
        // 降序：rank 3,2,1,1。
        assert_eq!(
            cuts,
            [4, 2, 1],
            "Edit 全量 / Region 取 rank≥2 / Nation 取 rank≥3，实得 {cuts:?}"
        );
        assert_eq!(frame_slice_lens(&segments, cuts), (4, 2, 1));
        assert_ne!(fingerprints[0], fingerprints[1]);
        assert_ne!(fingerprints[1], fingerprints[2]);
        assert_eq!(
            fingerprints[0],
            segments_fingerprint(&segments),
            "Edit 档指纹必须等于全量指纹（road_fingerprint 旧语义）"
        );
    }

    /// 同一排序下三档前缀的切片长度（ road_slice_at 的纯数据影射）。
    fn frame_slice_lens(segments: &[(Vec2, Vec2)], cuts: [usize; 3]) -> (usize, usize, usize) {
        let frame = ProjectionFrame {
            road_segments: segments.to_vec(),
            road_rank_cuts: cuts,
            road_tier_fingerprints: [0; 3],
            ..Default::default()
        };
        use crate::state::LodTier;
        (
            frame.road_slice_at(LodTier::Edit).len(),
            frame.road_slice_at(LodTier::Region).len(),
            frame.road_slice_at(LodTier::Nation).len(),
        )
    }

    /// 全部同级的段列：三档切片同长（全见或全不见），指纹全等——
    /// 「缩放跨档不换 mesh」的前置条件。
    #[test]
    fn uniform_rank_road_view_has_identical_tier_slices() {
        let ranked = vec![(3u8, Vec2::ZERO, Vec2::X), (3u8, Vec2::X, Vec2::X * 2.0)];
        let (segments, cuts, fingerprints) = build_road_tier_view(ranked);
        assert_eq!(cuts, [2, 2, 2], "全 rank 3 时三档同切片");
        assert_eq!(fingerprints[0], fingerprints[1]);
        assert_eq!(fingerprints[1], fingerprints[2]);
        assert_eq!(segments.len(), 2);
    }

    /// 河流长度占比折算（ADR-0047 D5 冻结契约，逐值手算）：
    /// ≥ 0.40 → 3、≥ 0.15 → 2、否则 → 1；边界值恰在门槛上归高档。
    #[test]
    fn river_rank_folds_by_length_fraction() {
        assert_eq!(river_rank_from_length_frac(0.40), 3, "恰在主河门槛上");
        assert_eq!(river_rank_from_length_frac(0.9), 3, "横贯图幅是主河");
        assert_eq!(river_rank_from_length_frac(0.39), 2, "门槛下沿归区域河");
        assert_eq!(river_rank_from_length_frac(0.15), 2, "恰在区域门槛上");
        assert_eq!(river_rank_from_length_frac(0.14), 1, "门槛下沿归溪流");
        assert_eq!(river_rank_from_length_frac(0.0), 1, "退化折线也是溪流");
    }

    /// POI 点标按当前图分桶直取：别图的锚定地点与未锚定图节点都不生成点。
    #[test]
    fn poi_dots_come_only_from_the_current_map_bucket() {
        let grid = GridConfig::default();
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("pa", "m1", 10.0, 20.0),
            anchored_place("pb", "m1", 30.0, 20.0),
            anchored_place("p_other", "m2", 40.0, 40.0),
            graph_node("pi"),
        ];

        let (mut app, mut schedule) = projection_app(world);
        let frame = run_once(&mut app, &mut schedule);
        assert_eq!(
            frame.place_dots.len(),
            2,
            "只有当前图 m1 上的两个锚定地点有点"
        );
        assert_eq!(
            frame.place_dots[0].pos,
            grid.map_to_view(Vec2::new(10.0, 20.0)),
            "点位置经 map_to_view 换算"
        );
    }

    /// POI 桶必须按**相机视口**裁剪（zoom-in 卡死回归，2026-10-18）：
    /// guangdong 有 8.1 万个锚定路口；若不裁剪，zoom in 越过非城市淡出带
    /// （scale < 0.75）后 `sync_poi` 会对**全量**点 spawn 实体 + mesh——
    /// 首帧即 16 万实体，窗口冻结。视口内的保留、视口外的丢弃。
    #[test]
    fn poi_dots_are_culled_to_camera_viewport() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("p_in_a", "m1", 10.0, 20.0),
            anchored_place("p_in_b", "m1", 600.0, 300.0),
            // 1280×800 窗口、scale=1、相机在原点 → 半幅 (640, 400)：
            // 这两个在视口外（右缘 640 + 余量，下缘 400 + 余量）。
            anchored_place("p_out_x", "m1", 5000.0, 20.0),
            anchored_place("p_out_y", "m1", 10.0, 5000.0),
        ];

        let (mut app, mut schedule) = projection_app(world);
        {
            let mut v = app.resource_mut::<ViewState>();
            v.scale = 1.0;
            v.camera_center = Vec2::ZERO;
            v.window_size = Vec2::new(1280.0, 800.0);
        }
        let frame = run_once(&mut app, &mut schedule);
        assert_eq!(
            frame.place_dots.len(),
            2,
            "视口内两点保留、视口外两点裁掉，got {}",
            frame.place_dots.len()
        );
    }

    /// 缩放拉远时视口变大：上一帧被裁掉的点重新入桶（拉动式刷新语义）。
    #[test]
    fn poi_dots_return_when_viewport_widens() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("p_near", "m1", 10.0, 20.0),
            anchored_place("p_far", "m1", 5000.0, 5000.0),
        ];

        let (mut app, mut schedule) = projection_app(world);
        {
            let mut v = app.resource_mut::<ViewState>();
            v.scale = 1.0;
            v.window_size = Vec2::new(1280.0, 800.0);
        }
        let near = run_once(&mut app, &mut schedule);
        assert_eq!(near.place_dots.len(), 1, "近景只有 p_near 可见");
        // run_once 会取走 ProjectionFrame 资源；二次运行前补一份。
        app.insert_resource(ProjectionFrame::default());

        // 拉远：scale=16 → 半幅 (10240, 6400)，p_far(5000,5000) 入镜。
        {
            let mut v = app.resource_mut::<ViewState>();
            v.scale = 16.0;
        }
        let far = run_once(&mut app, &mut schedule);
        assert_eq!(
            far.place_dots.len(),
            2,
            "拉远后两点都应入桶，got {}",
            far.place_dots.len()
        );
    }

    /// 规模回归：guangdong 量级（8.1 万锚定路口）单帧投影的 POI 桶构建
    /// 必须在毫秒级且输出被视口约束——防「不裁剪」以任何形式回归。
    #[test]
    fn guangdong_scale_places_project_within_frame_budget() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        let mut places = Vec::with_capacity(81_509);
        for i in 0..81_509 {
            // 撒在 4096² 图内（网格状），全部锚定当前图。
            let x = (i % 285) as f32 * 14.4;
            let y = (i / 285) as f32 * 14.4;
            places.push(anchored_place(&format!("p{i}"), "m1", x, y));
        }
        world.distances.places = places;

        let (mut app, mut schedule) = projection_app(world);
        {
            let mut v = app.resource_mut::<ViewState>();
            v.scale = 0.5; // zoom-in 越过淡出带——全量可见的场景
            v.window_size = Vec2::new(1280.0, 800.0);
        }
        let t0 = std::time::Instant::now();
        let frame = run_once(&mut app, &mut schedule);
        let dt = t0.elapsed();
        assert!(
            frame.place_dots.len() < 5_000,
            "视口 (1280/0.5)×(800/0.5) 只装得下视口内点，got {}",
            frame.place_dots.len()
        );
        // 预算 250ms：单跑 ~10ms，但全量并行跑测试时会被机器负载击穿
        // （曾 100ms 假失败）。守的意义是「毫秒级而非秒级」（无裁剪时 17s+）。
        assert!(dt.as_millis() < 250, "8.1 万点投影应在毫秒级，实得 {dt:?}");
    }

    // ---- ADR-0070 T8：河池单源 / 锥形宽 / 路 = 线 POI ----

    /// 线段 POI 夹具：折线（图内局部 wu）+ 类别 + 基准宽（米）。
    fn line_place(
        id: &str,
        map_id: &str,
        line: Vec<f32>,
        kind: Option<wv_core::distance::LineKind>,
        width_m: Option<f32>,
    ) -> Place {
        Place {
            kind: None,
            line: Some(line),
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            feature: None,
            biome: None,
            envelope: None,
            map: Some(Id::new_unchecked(map_id)),
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
            line_kind: kind,
            line_width_m: width_m,
        }
    }

    /// 河池单源（ADR-0070 T8）：无模板 world、两条河线 Place → 恰 2 条
    /// RiverLine；`meta.template`（含 `rivers_seed`）在场也**不得**派生出
    /// 第三条——种子河已由加载期迁移转成河线 Place（wv-edit T7），渲染侧
    /// 不再读种子，双供给（ADR-0046 D4）至此收成单池。
    #[test]
    fn river_pool_sources_from_line_places_only() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            // line_kind 缺省（None）= 河（ADR-0070 D2 判别口径）。
            line_place("rv_a", "m1", vec![0.0, 0.0, 30.0, 0.0], None, None),
            line_place(
                "rv_b",
                "m1",
                vec![0.0, 20.0, 30.0, 20.0],
                Some(wv_core::distance::LineKind::River),
                Some(24.0),
            ),
        ];
        world.meta = Some(wv_core::WorldMeta {
            name: "模板世界".into(),
            version: "0.1".into(),
            schema_version: 2,
            root_map: Id::new_unchecked("m1"),
            timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
            template: Some(wv_core::WorldTemplate {
                kind: wv_core::WorldTemplateKind::Ocean,
                seed: 1,
                rivers_seed: 42,
            }),
            comment: None,
        });

        let (mut app, mut schedule) = projection_app(world);
        let frame = run_once(&mut app, &mut schedule);
        assert_eq!(
            frame.river_polylines.len(),
            2,
            "河池只收河线 Place（种子不派生），实得 {}",
            frame.river_polylines.len()
        );
        // 显式宽度折算在场：rv_b 带基准宽，rv_a 缺省 → width_wu == 0（旧缺省宽）。
        assert_eq!(frame.river_polylines[0].width_wu, 0.0, "未声明宽度 → 0");
        assert!(
            frame.river_polylines[1].width_wu > 0.0,
            "声明了 line_width_m 的河线必须带正基准宽，实得 {}",
            frame.river_polylines[1].width_wu
        );
    }

    /// 锥形宽（ADR-0070 D4）：源头段系数恰 0.3（弧长占比 0），沿弧长单调
    /// 增至河口；首段宽/末段宽 ≈ 0.3（±0.05）。10 段等长折线时末段起点
    /// 占比 0.9 → 系数 0.93，比值 0.3/0.93 ≈ 0.323。
    #[test]
    fn river_taper_reaches_thirty_percent_at_source() {
        let pts: Vec<Vec2> = (0..=10).map(|i| Vec2::new(i as f32 * 10.0, 0.0)).collect();
        let f = river_taper_factors(&pts);
        assert_eq!(f.len(), 10, "系数列与段数平行，实得 {}", f.len());
        let first = f[0];
        let last = f[9];
        assert!(
            (first - 0.3).abs() < 1e-6,
            "源头段（弧长占比 0）系数应恰为 0.3，实得 {first}"
        );
        let ratio = first / last;
        assert!(
            (ratio - 0.3).abs() <= 0.05,
            "首段宽/末段宽应 ≈ 0.3（±0.05），实得 {ratio}（first={first}, last={last}）"
        );
        for (i, w) in f.windows(2).enumerate() {
            assert!(
                w[1] > w[0],
                "锥形系数应沿弧长单调递增（第 {i} 段 {} → {}）",
                w[0],
                w[1]
            );
        }
    }

    /// 路 = 线 POI（ADR-0070 T8）：1 条路线 Place（4 点折线）→ 段数 = 3 =
    /// 点数 − 1，且 rank 走 `road_line_effective_rank` 的公里折算。
    /// `unit_per_km = 0.1`（0.1 wu/km）→ 1 wu = 10 km，线长 40 wu = 400 km
    /// ≥ 100 → rank 3；折算方向写反（×unit_per_km）会得 4 km → rank 1，本
    /// 测试即红——钉死 wu→km 是**乘 km_per_wu**。
    #[test]
    fn road_segments_source_from_road_line_places() {
        let grid = GridConfig::default();
        let mut world = wv_core::World::default();
        let mut m1 = map("m1");
        m1.unit_per_km = Some(0.1);
        world.maps.push(m1);
        world.distances.places = vec![line_place(
            "road_1",
            "m1",
            vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 20.0, 10.0],
            Some(wv_core::distance::LineKind::Road),
            None,
        )];

        let (mut app, mut schedule) = projection_app(world);
        let frame = run_once(&mut app, &mut schedule);
        assert_eq!(
            frame.road_segments.len(),
            3,
            "段数 = 点数 − 1，实得 {}",
            frame.road_segments.len()
        );
        assert_eq!(
            frame.road_rank_cuts,
            [3, 3, 3],
            "400 km 线 → rank 3，三档全见，实得 {:?}",
            frame.road_rank_cuts
        );
        assert_eq!(
            frame.road_segments[0],
            (
                grid.map_to_view(Vec2::new(0.0, 0.0)),
                grid.map_to_view(Vec2::new(10.0, 0.0))
            ),
            "段几何 = 折线相邻点对的视图空间坐标"
        );
    }

    /// 只有手写 road 边的存量世界（ADR-0070 之前的形态）→ road_segments
    /// **空**：道路唯一供给源是路 POI 线，手写 road 边不再直连渲染（存量
    /// 边由加载期迁移转线，wv-app T9）；海路等非 road 边才走直连路径
    /// （阳性对照见 `edge_between_anchored_places_becomes_one_road_segment`）。
    #[test]
    fn legacy_edge_only_world_renders_no_roads() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("pa", "m1", 10.0, 20.0),
            anchored_place("pb", "m1", 30.0, 20.0),
        ];
        world.distances.edges = vec![PlaceEdge {
            terrain: "road".to_owned(),
            ..edge("e1", "pa", "pb")
        }];

        let (mut app, mut schedule) = projection_app(world);
        let frame = run_once(&mut app, &mut schedule);
        assert!(
            frame.road_segments.is_empty(),
            "road 边不再直连渲染，实得 {} 段",
            frame.road_segments.len()
        );
        assert_eq!(frame.road_rank_cuts, [0; 3], "无段则三档切割位全零");
    }

    /// 路段列缓存（ADR-0030 D5a）：缩放/平移（只动 ViewState）不触发重算，
    /// 段列与指纹原样保留；世界替换（重插 WorldProjection）才失效重算。
    #[test]
    fn road_segments_cache_survives_view_changes_and_rebuilds_on_world_change() {
        let mut world = wv_core::World::default();
        world.maps.push(map("m1"));
        world.distances.places = vec![
            anchored_place("pa", "m1", 10.0, 20.0),
            anchored_place("pb", "m1", 30.0, 20.0),
        ];
        world.distances.edges = vec![edge("e1", "pa", "pb")];

        // 注意**不得**中途移除 ProjectionFrame：那会重置 road_key（缓存键）
        // 等于强制失效，测不出「跳过重算」。全程原地跑、原地比对。
        let mut app = World::default();
        app.insert_resource(PlaybackState::default());
        app.insert_resource(ViewState::default());
        app.insert_resource(GridConfig::default());
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&world);
        app.insert_resource(drill);
        app.insert_resource(WorldProjection {
            world,
            ..Default::default()
        });
        app.insert_resource(LayerFrame::default());
        app.insert_resource(ProjectionFrame::default());
        app.init_resource::<PlaceIndex>();
        app.init_resource::<crate::state::PinDragState>();
        let mut schedule = Schedule::default();
        schedule.add_systems(compute_projection);

        schedule.run(&mut app);
        let (fp1, seg1) = {
            let f = app.resource::<ProjectionFrame>();
            (f.road_fingerprint, f.road_segments.clone())
        };
        assert_eq!(seg1.len(), 1);

        // 缩放 + 平移：段坐标在根图帧系，缓存必须原样保留（连 Vec 指针级
        // 的重算都不发生——指纹与段列原样）。
        {
            let mut v = app.resource_mut::<ViewState>();
            v.scale = 2.0;
            v.camera_center = Vec2::new(100.0, 100.0);
        }
        schedule.run(&mut app);
        let f2 = app.resource::<ProjectionFrame>();
        assert_eq!(f2.road_fingerprint, fp1, "视图变化不重算段列");
        assert_eq!(f2.road_segments, seg1);
        assert_ne!(f2.road_key, None, "缓存键已建立");

        // 世界替换：失效重算。
        let mut world2 = wv_core::World::default();
        world2.maps.push(map("m1"));
        world2.distances.places = vec![
            anchored_place("pa", "m1", 10.0, 20.0),
            anchored_place("pb", "m1", 30.0, 20.0),
            anchored_place("pc", "m1", 50.0, 60.0),
        ];
        world2.distances.edges = vec![edge("e1", "pa", "pb"), edge("e2", "pb", "pc")];
        app.insert_resource(WorldProjection {
            world: world2,
            ..Default::default()
        });
        schedule.run(&mut app);
        let f3 = app.resource::<ProjectionFrame>();
        assert_eq!(f3.road_segments.len(), 2, "世界替换后重算段列");
        assert_ne!(f3.road_fingerprint, fp1);
    }

    /// [ADR-0074 追记三 followup] 高亮环定位：过滤（非 Place/他图/线段）
    /// + 拖动跟预览位。
    #[test]
    fn pin_highlight_pos_filters_and_follows_drag_preview() {
        let map = Id::new_unchecked("m1");
        let _other = Id::new_unchecked("m2");
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            level: wv_core::map::MapLevel::World,
            ..Default::default()
        });
        world
            .distances
            .places
            .push(anchored_place("p1", "m1", 10.0, 20.0));
        world
            .distances
            .places
            .push(anchored_place("p2", "m2", 30.0, 40.0));
        let mut river = anchored_place("riv", "m1", 50.0, 60.0);
        river.line = Some(vec![0.0, 0.0, 10.0, 10.0]);
        world.distances.places.push(river);
        let grid = GridConfig::default();
        let drag = crate::state::PinDragState::default();

        // 本图 Place → 锚点视图位。
        let sel_p1 =
            wv_core::SelectedEntity::new(wv_core::EntityKind::Place, Id::new_unchecked("p1"));
        assert_eq!(
            pin_highlight_pos(&world, Some(&sel_p1), Some(&map), &grid, &drag, false),
            Some(Vec2::new(10.0, 20.0)),
            "本图点状钉在锚点亮"
        );
        // 他图 Place → 不亮。
        let sel_p2 =
            wv_core::SelectedEntity::new(wv_core::EntityKind::Place, Id::new_unchecked("p2"));
        assert!(
            pin_highlight_pos(&world, Some(&sel_p2), Some(&map), &grid, &drag, false).is_none()
        );
        // 线段 POI → 不亮（无钉）。
        let sel_riv =
            wv_core::SelectedEntity::new(wv_core::EntityKind::Place, Id::new_unchecked("riv"));
        assert!(
            pin_highlight_pos(&world, Some(&sel_riv), Some(&map), &grid, &drag, false).is_none()
        );
        // 非 Place（Region/Faction）→ 不亮。
        let sel_region =
            wv_core::SelectedEntity::new(wv_core::EntityKind::Region, Id::new_unchecked("p1"));
        assert!(
            pin_highlight_pos(&world, Some(&sel_region), Some(&map), &grid, &drag, false).is_none()
        );
        // 无选中 → 不亮。
        assert!(pin_highlight_pos(&world, None, Some(&map), &grid, &drag, false).is_none());
        // 拖动中 → 跟预览位（世界锚点未动也亮在新位）。
        let mut dragging = crate::state::PinDragState {
            place: Some(Id::new_unchecked("p1")),
            map: Some(map.clone()),
            start_world: Some(Vec2::new(10.0, 20.0)),
            active: true,
            preview_local: Some((70.0, 80.0)),
            last_click: None,
        };
        assert_eq!(
            pin_highlight_pos(&world, Some(&sel_p1), Some(&map), &grid, &dragging, false),
            Some(Vec2::new(70.0, 80.0)),
            "拖动中高亮跟预览位"
        );
        dragging.active = false;
        assert_eq!(
            pin_highlight_pos(&world, Some(&sel_p1), Some(&map), &grid, &dragging, false),
            Some(Vec2::new(10.0, 20.0)),
            "未激活回落世界锚点"
        );
    }

    /// [ADR-0074 追记三 followup] pin 模式（Topographic/GoogleMap）∧ rank≥2
    /// 的选中钉：环让位给「换金色 pin」——定位返 None；rank<2 点标仍亮环。
    #[test]
    fn pin_mode_rank_two_plus_pin_gets_no_ring() {
        let map = Id::new_unchecked("m1");
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            level: wv_core::map::MapLevel::World,
            ..Default::default()
        });
        world
            .distances
            .places
            .push(anchored_place("p1", "m1", 10.0, 20.0));
        let grid = GridConfig::default();
        let drag = crate::state::PinDragState::default();
        let sel = wv_core::SelectedEntity::new(wv_core::EntityKind::Place, Id::new_unchecked("p1"));
        // p1 有 w/h → rank 3：pin 模式不画环（换色钉接管），插画模式画环。
        assert!(pin_highlight_pos(&world, Some(&sel), Some(&map), &grid, &drag, true).is_none());
        assert_eq!(
            pin_highlight_pos(&world, Some(&sel), Some(&map), &grid, &drag, false),
            Some(Vec2::new(10.0, 20.0)),
            "插画模式保留环"
        );
        // rank<2 点标：pin 模式也画环（点标不换色）。
        let dot = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id.as_str() == "p1")
            .unwrap();
        dot.w = None;
        dot.h = None;
        dot.rank = Some(1);
        assert_eq!(
            pin_highlight_pos(&world, Some(&sel), Some(&map), &grid, &drag, true),
            Some(Vec2::new(10.0, 20.0)),
            "rank<2 点标在 pin 模式仍亮环"
        );
    }

    /// compute_projection 按选中态给 landmark_dots.selected 置位。
    #[test]
    fn projection_marks_the_selected_pin() {
        let map = Id::new_unchecked("m1");
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            level: wv_core::map::MapLevel::World,
            ..Default::default()
        });
        world
            .distances
            .places
            .push(anchored_place("p1", "m1", 10.0, 20.0));
        let (mut app, mut schedule) = projection_app(world);
        app.resource_mut::<crate::state::WorldProjection>()
            .selection = Some(wv_core::SelectedEntity::new(
            wv_core::EntityKind::Place,
            Id::new_unchecked("p1"),
        ));
        schedule.run(&mut app);
        let frame = app.resource::<ProjectionFrame>();
        assert!(
            frame.landmark_dots.iter().any(|d| d.selected),
            "选中钉应被标记 selected"
        );
    }

    /// 行为探针（临时）：sync_gizmo_line_width 真的把线宽写成 view.scale？
    #[test]
    fn probe_gizmo_line_width_sync() {
        use bevy::gizmos::config::{DefaultGizmoConfigGroup, GizmoConfigStore};
        let mut app = App::new();
        app.insert_resource(ViewState {
            scale: 0.25,
            ..Default::default()
        });
        app.init_resource::<crate::state::PinDragState>();
        app.init_gizmo_group::<DefaultGizmoConfigGroup>();
        let mut schedule = Schedule::default();
        schedule.add_systems(crate::world_projection::sync_gizmo_line_width);
        schedule.run(app.world_mut());
        let store = app.world().resource::<GizmoConfigStore>();
        let (cfg, _) = store.config::<DefaultGizmoConfigGroup>();
        assert!(
            (cfg.line.width - 0.25).abs() < 1e-4,
            "line_width 应=scale 0.25，实际 {}",
            cfg.line.width
        );
    }
}
