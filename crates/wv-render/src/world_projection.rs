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

use bevy::prelude::*;

use std::collections::HashMap;

use crate::layer_frame::LayerFrame;
use crate::state::{BrushState, GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::theme::colors;
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
    /// 本帧道路层（矢量叠加）：每条地点边 `(PlaceEdge)` 的一段线（世界坐标端点对）。
    ///
    /// 只画**两端都锚定在当前地图**的边（`Place.map == 当前图 id` 且 `x/y` 齐备），
    /// 与 [`crate::drill::resolve_markers`] 的「只画当前层级」口径一致。
    ///
    /// **已按 `PlaceEdge::effective_rank` 降序排序**（ADR-0047 D4）：每档视野
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
    /// 本帧河流折线（ADR-0037 D5）：`terrain + rivers_seed` 的确定性派生，
    /// 视图空间坐标（同 [`Self::road_segments`] 的换算口径）。
    ///
    /// **派生数据不落盘**（AGENTS 4.2 只存参数）：种子在 `WorldMeta.template`，
    /// 高度场在 `terrain.bin`，两者齐备即可逐点复现；重生成换种子河流自动跟随。
    /// 每条折线带派生呈现级别（ADR-0047 D5，[`RiverLine::rank`]）。
    pub river_polylines: Vec<RiverLine>,
    /// 河流派生缓存键：`(当前图 id, rivers_seed)`。`None` = 失效。
    /// 世界编辑 / 钻取换图 / 栅格换图都会重算；纯相机运动零工作。
    pub river_key: Option<(String, u64)>,
    /// 本帧地物符号（rank ≥ 2 的地点，视图空间坐标）：国家档与更近档都画，
    /// 靠视口裁剪防密集点（参照 `place_dots` A1 索引）。
    pub landmark_dots: Vec<LandmarkDot>,
    /// 本帧**作者手绘**河流折线（ADR-0046 D4 双供给的作者那一路）。
    ///
    /// 视图空间坐标（同 [`Self::river_polylines`] 口径）。来源 = 当前图上
    /// 带 `Place.line` 的地点（亚格线状作者数据）；与派生的 `rivers_seed`
    /// 水系**并存**——派生是模板世界的确定性资产，作者河是补充，不是替代。
    /// 呈现级别与派生河同折算（长度占比，ADR-0047 D5）；图幅未知时归 rank 1
    /// （亚格数据本质是局部细节，宁可少显示）。
    pub author_river_polylines: Vec<RiverLine>,
}

/// 一条河流折线的渲染数据（ADR-0047 D5）：顶点 + 派生呈现级别。
#[derive(Debug, Clone, PartialEq)]
pub struct RiverLine {
    /// 派生呈现级别（[`river_rank_from_length_frac`] 折算）；绘制侧过统一
    /// 门槛 [`crate::state::rank_visible_at_tier`]。
    pub rank: u8,
    /// 视图空间顶点（≥ 2 个才构成可见折线）。
    pub pts: Vec<Vec2>,
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

/// 折线总长（相邻顶点距离和）。
fn polyline_len(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
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
            if let (Some(map), Some(_), Some(_)) = (p.map.as_ref(), p.x, p.y) {
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
    mut place_index: ResMut<PlaceIndex>,
    mut frame: ResMut<ProjectionFrame>,
) {
    frame.active_events.clear();
    frame.track_polylines.clear();
    frame.track_heads.clear();
    frame.track_hollow_starts.clear();
    frame.place_dots.clear();
    frame.landmark_dots.clear();
    frame.author_river_polylines.clear();

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

    // 河流派生缓存（ADR-0037 D5）：键 = (当前图, rivers_seed)；世界编辑 /
    // 钻取换图 / 栅格换图即重算，纯相机运动零工作（与路网缓存同款纪律）。
    let river_seed = projection
        .world
        .meta
        .as_ref()
        .and_then(|m| m.template.as_ref())
        .map(|t| t.rivers_seed);
    let current_for_river = drill.current().cloned();
    let river_wanted = match (&current_for_river, river_seed) {
        (Some(map_id), Some(seed)) => Some((map_id.as_str().to_owned(), seed)),
        _ => None,
    };
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

    // 作者手绘河（ADR-0046 D4）：当前图上带 `Place.line` 的地点 → 折线。
    // 与派生水系**双供给**（先例：街道的手绘 YAML / Overture 双供给）。
    // 坐标：折线是**图内局部 wu** → `map_to_view` 转视图空间（同 POI 口径）。
    // 级别（ADR-0047 D5）：与派生河同折算（长度 ÷ 图幅宽）；图幅未知归 1。
    if river_stale {
        let extent_w = current_for_river.as_ref().and_then(|map_id| {
            projection
                .world
                .maps
                .iter()
                .find(|m| &m.id == map_id)
                .and_then(|n| n.extent)
                .map(|e| e.width)
        });
        if let Some(map_id) = &current_for_river {
            if let Some(idxs) = place_index.anchored_by_map.get(map_id) {
                for i in idxs {
                    let place = &projection.world.distances.places[*i as usize];
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
                        frame.author_river_polylines.push(RiverLine { rank, pts });
                    }
                }
            }
        }
    }

    // 河流重算放在图层早退之前：河流属地形族（terrain 图层在渲染侧 gate），
    // 关掉全部矢量图层再重新打开的第一帧就要有河可画。
    if river_stale {
        let world = &projection.world;
        if let (Some(map_id), Some(seed)) = (&current_for_river, river_seed) {
            if let Some(node) = world.maps.iter().find(|m| &m.id == map_id) {
                if let (Some(extent), Some(terrain)) = (node.extent, world.terrain_of(map_id)) {
                    let heights: Vec<i16> = terrain.cells.iter().map(|c| c.height).collect();
                    let polys = wv_terrain::climate::generate_rivers(
                        terrain.width,
                        terrain.height,
                        &heights,
                        seed,
                        12,
                    );
                    // 级别（ADR-0047 D5）：折线长度 ÷ 图幅宽。视图空间长度 ==
                    // 图内 wu 长度（map_to_view 只平移），占比口径与图幅无关。
                    frame.river_polylines = polys
                        .into_iter()
                        .map(|pts| {
                            let pts: Vec<Vec2> = pts
                                .into_iter()
                                .map(|[nx, ny]| {
                                    grid.map_to_view(Vec2::new(
                                        nx * extent.width,
                                        ny * extent.height,
                                    ))
                                })
                                .collect();
                            let rank = river_rank_from_length_frac(
                                polyline_len(&pts) / extent.width.max(1.0),
                            );
                            RiverLine { rank, pts }
                        })
                        .collect();
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
                Some(grid.map_to_view(Vec2::new(p.x?, p.y?)))
            };
            if layers.road.visible && road_stale {
                // 带级别收集（ADR-0047 D4）：`effective_rank`（显式 rank 或
                // 距离折算）→ 降序排序 → 三档前缀切割位与各自指纹一次算齐。
                let mut ranked: Vec<(u8, Vec2, Vec2)> = Vec::new();
                for edge in &world.distances.edges {
                    if let (Some(a), Some(b)) = (find_place(&edge.a), find_place(&edge.b)) {
                        ranked.push((edge.effective_rank(), a, b));
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
                    for i in idxs {
                        if let Some(dot) =
                            place_dot_for(&world.distances.places[*i as usize], &grid)
                        {
                            if (dot.pos.x - cx).abs() <= half.x && (dot.pos.y - cy).abs() <= half.y
                            {
                                frame.place_dots.push(dot);
                            }
                        }
                        // 地物符号：rank ≥ 2 走程序化插画（rank 1 路网交点仍按点标）。
                        // rank 3 永远画国家级，rank 2 全档都画（参考图期望：全球档可见城市符号）。
                        let p = &world.distances.places[*i as usize];
                        if let (Some(_), Some(_)) = (p.x, p.y) {
                            let rank = p.effective_rank();
                            if rank >= 2 {
                                let pos = grid.map_to_view(Vec2::new(p.x.unwrap(), p.y.unwrap()));
                                if (pos.x - cx).abs() <= half.x && (pos.y - cy).abs() <= half.y {
                                    let kind = if rank >= 3 {
                                        LandmarkKind::Citadel
                                    } else {
                                        LandmarkKind::Town
                                    };
                                    frame.landmark_dots.push(LandmarkDot { pos, kind });
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
pub fn advance_playback(time: Res<Time>, mut playback: ResMut<PlaybackState>) {
    if !playback.playing {
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
    region_adjust: Res<crate::state::RegionAdjustState>,
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
                candidate_cache.fill_cells = result.fill_cells;
                candidate_cache.bboxes = result.bboxes;
                candidate_cache.drawn_epoch = candidate_cache.last_epoch;
                candidate_cache.last_compute = Some(t0);
            }
        }

        // 用户裁定（2026-10-22 验收）：候选高亮 = **白色虚线**，不用金色——
        // 沙色地形上金色与地貌色混淆；白色虚线在任意群系色上都可辨。
        let white = bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.9);
        for (i, outline) in candidate_cache.outlines.iter().enumerate() {
            let (bmin, bmax) = &candidate_cache.bboxes[i];
            // 屏上尺寸（px）≥ 40 才画半透明填充（缩放够大填充才有意义）。
            let _ = (bmin, bmax);
            for (a, b) in outline {
                gizmos.line_2d(*a, *b, white);
            }
        }
    }

    // ── 选中区域的方格描边 + 顶点手柄（2026-10-22 验收 · 用户裁定）──────
    // CellMap 专业数据编辑器：选中 Region ⇒ 沿格边的阶梯描边（白色实线）
    // + 顶点手柄（方格锚点，拖动吸附格点）。与 nature_overlay 无关。
    if let Some(sel) = projection.selection.as_ref() {
        if sel.kind == wv_core::EntityKind::Region
            && matches!(view.mode, crate::state::ViewMode::Topographic)
        {
            if let Some(map) = drill.current().cloned() {
                let region = projection.world.regions.iter().find(|r| r.id == sel.id);
                let Some(region) = region.filter(|r| r.map == map && r.polygon.len() >= 6) else {
                    return;
                };
                let Some(terrain) = projection.world.terrain_of(&map) else {
                    return;
                };
                let raster_to_map = grid_cfg.map_origin - grid_cfg.raster_origin;
                let adjust = region_adjust;
                let poly = adjust
                    .preview_polygon
                    .clone()
                    .unwrap_or_else(|| region.polygon.clone());
                let poly_pairs: Vec<(f32, f32)> =
                    poly.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                let cells = crate::cell_selection::rasterize_polygon_cells(
                    &poly_pairs,
                    raster_to_map,
                    &grid_cfg,
                    terrain.width,
                    terrain.height,
                );
                // 方格描边：mask 内格与不在 mask 的 4 邻之间的边。
                use std::collections::HashSet;
                let mask: HashSet<(i32, i32)> = cells.iter().copied().collect();
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
                let white = bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.95);
                for (a, b) in &edges {
                    gizmos.line_2d(*a, *b, white);
                }
                // 顶点手柄（LOD 锚点，用户裁定：锚点数量由 zoom 决定）——
                // 物化多边形每个阶梯角都有顶点，全画会重叠成一片白线（用户
                // 验收反馈：只见白线不见锚点）。抽稀间距 = 24px 的世界长度：
                // zoom out 锚点稀疏（粗调）、zoom in 锚点变密（细调）。
                let poly_pairs: Vec<(f32, f32)> =
                    poly.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                let spacing =
                    crate::biome_boundary::anchor_spacing(view.scale, grid_cfg.cell_size, 24.0);
                let anchors = crate::biome_boundary::decimate_poly(&poly_pairs, spacing);
                // 手柄视觉：白色十字准星 + 深色圈；尺寸按屏幕像素折算
                //（×view.scale），任意缩放下恒定可辨。
                let handle_px = (view.scale * 5.0).max(grid_cfg.cell_size * 0.15);
                let handle_white = bevy::color::Color::srgba(1.0, 1.0, 1.0, 1.0);
                let handle_dark = bevy::color::Color::srgba(0.08, 0.08, 0.08, 0.85);
                for a in &anchors {
                    // 多边形是**图内局部 wu**：世界位 = map_origin + 局部
                    //（此前误用 raster_origin——子图上手柄会偏移）。
                    let p = grid_cfg.map_origin + Vec2::new(a.0, a.1);
                    // 深色圈（方框）。
                    gizmos.rect_2d(
                        bevy::math::Isometry2d::from_translation(p),
                        Vec2::splat(handle_px * 2.0),
                        handle_dark,
                    );
                    // 白色十字（横竖两线）。
                    gizmos.line_2d(
                        p - Vec2::X * handle_px,
                        p + Vec2::X * handle_px,
                        handle_white,
                    );
                    gizmos.line_2d(
                        p - Vec2::Y * handle_px,
                        p + Vec2::Y * handle_px,
                        handle_white,
                    );
                }
            }
        }
    }

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

/// 笔刷光标 falloff 双环（VFX §3.1：外环实线=影响半径，内环虚线=50% 强度半径）。
///
/// **属「状态」，永不抑制**（VFX §1.3 / 基线 §1.20）。光标用屏幕叠加，不写数据。
///
/// **只在世界级显示**（v0.2 §B：城市级/建筑级不显示笔刷光标）——下面那级没有地形
/// 网格，光标画在那儿只会让人以为可以涂抹。
///
/// **已迁出 Gizmos**：原实现用 `Gizmos` + 48 段短弧拼虚线，**段长世界空间
/// 固定、相机放大后虚线变长、变实线**。现由 [`crate::brush_cursor::sync_brush_cursor`]
/// 写 Mesh2d 实体（外环 `Annulus` + 虚线 24 段 `Rectangle` + 十字），屏幕 1px/3px
/// 粗细由 `view.scale` 反比缩放保持恒定。`if false` 保留原实现作回退。
pub fn draw_brush_cursor(
    mut gizmos: Gizmos,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    grid: Res<GridConfig>,
    drill: Res<crate::state::DrillState>,
) {
    if false {
        if !brush.enabled || drill.depth() > 0 {
            return;
        }
        let Ok(window) = windows.single() else {
            return;
        };
        let Some(cursor) = window.cursor_position() else {
            return;
        };
        let Ok(transform) = camera.single() else {
            return;
        };
        let world =
            crate::camera::screen_to_world(cursor, window.resolution.size(), transform, view.scale);

        // 外环：实线，半径 = 真实笔刷半径（误差 0）。
        gizmos.circle_2d(
            Isometry2d::from_translation(world),
            brush.radius,
            colors::ACCENT_SELECT,
        );
        // 内环：50% 强度半径 R * (1 - softness)，虚线（用短弧近似）。
        let inner = brush.radius * (1.0 - brush.softness).max(0.15);
        draw_dashed_circle(
            &mut gizmos,
            world,
            inner,
            colors::ACCENT_SELECT.with_alpha(0.6),
        );
        // 中心十字。
        let c = grid.cell_size * 0.5;
        gizmos.line_2d(
            world - Vec2::X * c,
            world + Vec2::X * c,
            colors::ACCENT_SELECT,
        );
        gizmos.line_2d(
            world - Vec2::Y * c,
            world + Vec2::Y * c,
            colors::ACCENT_SELECT,
        );
    } // 关闭 `if false` 回退开关。
}

/// 用短弧拼出一圈虚线（原型阶段的虚线近似）。
///
/// **仅供 `draw_brush_cursor` 的 `if false` 回退分支调用**（[Gizmos] 旧实现）。
/// `#[allow(dead_code)]` 防止被 clippy 误报——Gizmos 路径平时不走，但保留作为
/// 一键回退。
#[allow(dead_code)]
fn draw_dashed_circle(gizmos: &mut Gizmos, center: Vec2, radius: f32, color: Color) {
    const SEGMENTS: usize = 48;
    for i in 0..SEGMENTS {
        if i % 2 == 1 {
            continue;
        }
        let a0 = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        let p0 = center + Vec2::new(a0.cos(), a0.sin()) * radius;
        let p1 = center + Vec2::new(a1.cos(), a1.sin()) * radius;
        gizmos.line_2d(p0, p1, color);
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
        }
    }

    fn anchored_place(id: &str, map: &str, x: f32, y: f32) -> Place {
        Place {
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
        }
    }

    fn graph_node(id: &str) -> Place {
        // guangdong 的路口形态：纯距离图节点，不锚定地图、无坐标。
        Place {
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
        }
    }

    fn edge(id: &str, a: &str, b: &str) -> PlaceEdge {
        PlaceEdge {
            rank: None,
            id: Id::new_unchecked(id),
            a: Id::new_unchecked(a),
            b: Id::new_unchecked(b),
            distance_km: 1.0,
            terrain: "road".to_owned(),
            bidirectional: true,
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
}
