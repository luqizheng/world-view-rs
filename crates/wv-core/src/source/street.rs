//! 街道矢量图层的文档模型（ADR-0010）。
//!
//! 「缩放到街道」要求城市内部有一份真实的街道路网几何。本模块定义这份
//! 几何的**存储形态**：一个 `LayerKind::Road` 图层经 [`wv_core::map::Layer::source`]
//! 指向一个 YAML 文件，文件内容即本模块的 [`StreetLayer`]。
//!
//! 设计约束（ADR-0010 决策 ⑤⑥）：
//! - **几何不进领域 YAML 字段**：`World` / `MapNode` 不持有街道几何，避免
//!   4 万段的坐标把 `maps/<id>/index.yaml` 撑成不可读的巨物；分层由
//!   P5（图层化）天然承载——街道就是图上的一层。
//! - **YAML 为主格式**（P3）：不开二进制例外；规模阈值与分片策略见
//!   README §12 第 21 条（单文件 2000–5000 段后分片，尚未收敛）。
//! - **只存几何与类别参数，不存渲染结论**：线宽、颜色、LOD 档位都是
//!   渲染层按 [`RoadClass`] 派生的表现层决策，不在数据里落字段。

use crate::{
    base::geometry::Point,
    time::{TimePoint, TimeSpan},
};

/// 街道类别（ADR-0010 决策 ④ 的 class 集合）。
///
/// 命名对齐 Overture `classes` 的 `ROAD` 主题取值，虚构路网与真实路网
/// 共用同一套词汇（ADR-0010 决策 ②：一个供给接口两个实现）。
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RoadClass {
    Motorway,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    /// 缺省类别（`#[derive(Default)]`）：手绘编辑器最常从住宅街道画起（ADR-0010 ②）。
    #[default]
    Residential,
    Service,
    Path,
    Cycleway,
    Track,
    Unknown,
    Unclassified,
}

impl RoadClass {
    /// 全部类别的静态表（渲染层建材质 / LOD 分档时遍历用）。
    pub const ALL: [RoadClass; 12] = [
        RoadClass::Motorway,
        RoadClass::Trunk,
        RoadClass::Primary,
        RoadClass::Secondary,
        RoadClass::Tertiary,
        RoadClass::Residential,
        RoadClass::Service,
        RoadClass::Path,
        RoadClass::Cycleway,
        RoadClass::Track,
        RoadClass::Unknown,
        RoadClass::Unclassified,
    ];

    /// LOD 分档（ADR-0010 决策 ④）：数字越小越「重要」，远视角先隐藏大数字档。
    ///
    /// 这是**表现层参数**的派生入口之一：分档值只在本方法里定义一次，
    /// 渲染层的 LOD 过滤只消费它，不再各自硬编码类别集合。
    pub fn lod_rank(self) -> u8 {
        match self {
            RoadClass::Motorway => 0,
            RoadClass::Trunk => 1,
            RoadClass::Primary => 2,
            RoadClass::Secondary => 3,
            RoadClass::Tertiary => 4,
            RoadClass::Unclassified | RoadClass::Residential => 5,
            RoadClass::Service | RoadClass::Track => 6,
            RoadClass::Path | RoadClass::Cycleway | RoadClass::Unknown => 7,
        }
    }

    /// `lod_rank` → 统一呈现级别（ADR-0047 D3）。
    ///
    /// 与 `Place::rank` / `PlaceEdge::rank` 同一张呈现级别词表（数值越大越
    /// 重要）：lod 0–2（高速/干道/主干道）→ `3`、3–5（次干/支路/住宅/未定级）
    /// → `2`、6–7（服务/土路/小径类）→ `1`。折算表经
    /// `lod_presentation_mapping_preserves_street_tier_gates` 钉死：换算后走
    /// 统一门槛 `rank_visible_at_tier`，与旧阈值（Nation ≤ 2 / Region ≤ 5 /
    /// Edit 全见）逐档等价——即街道呈现行为零变化，只是并入统一口径。
    pub fn presentation_rank_from_lod(lod_rank: u8) -> u8 {
        match lod_rank {
            0..=2 => 3,
            3..=5 => 2,
            _ => 1,
        }
    }

    /// 编辑器 UI 的中文显示名（**纯显示**，YAML 仍存 snake_case 英文键）。
    pub fn label(self) -> &'static str {
        match self {
            RoadClass::Motorway => "高速公路",
            RoadClass::Trunk => "干道",
            RoadClass::Primary => "主干道",
            RoadClass::Secondary => "次干道",
            RoadClass::Tertiary => "支路",
            RoadClass::Residential => "住宅街道",
            RoadClass::Service => "服务道路",
            RoadClass::Path => "小径",
            RoadClass::Cycleway => "自行车道",
            RoadClass::Track => "土路",
            RoadClass::Unknown => "未知",
            RoadClass::Unclassified => "未定级",
        }
    }
}

/// 一条街道段：一条折线 + 类别。
///
/// `points` 是**本图层所属地图的局部坐标**（与 `MapNode.extent` 同单位）；
/// 上溯到世界坐标走 `wv_core::coord::resolve_frame_in`，与标记/栅格同一条
/// 坐标链，不另立换算。
///
/// 折线允许多点（曲线街道），渲染层按相邻点连成线段流（ADR-0010 的
/// 「路段流」）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreetSegment {
    /// 类别；缺省 [`RoadClass::default`]（住宅街道）。
    #[serde(default)]
    pub class: RoadClass,
    /// 折线顶点，至少 2 个才构成可见线段。校验不阻止保存（V1）。
    #[serde(default)]
    pub points: Vec<Point>,
    /// 街道名（可选，Phase 5 标签引擎的数据源）。手绘的大多数小巷无名，
    /// `None` 不产生任何标注；命名不是门槛，标注不是校验。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 街道段的存在窗口（ADR-0015 · 2026-10-14 接受）。
    ///
    /// `None` = 段跟随所在子图（默认；向后兼容，零迁移）；
    /// `Some(span)` = 仅 `span.contains(t)` 时段可见（渲染与街名标注同口径）。
    ///
    /// **与 `MapNode.time_span` 的关系**：图级窗口**否决**段级——子图窗口
    /// 之外的段一律不可见，无论段自己是否设了窗口。求值走
    /// [`StreetSegment::visible_at`]（与 `wv_core::label_visible_at` 同构），
    /// 调用方不得在调用点重写公式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_span: Option<TimeSpan>,
}

impl StreetSegment {
    /// 两点直段的便捷构造。
    pub fn straight(class: RoadClass, x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        StreetSegment {
            class,
            points: vec![Point::new(x1, y1), Point::new(x2, y2)],
            name: None,
            time_span: None,
        }
    }

    /// 段级可见性（仅看自身 `time_span`，**不**看所在子图）。
    ///
    /// 拿「本段在某时刻是否参与渲染」请用 [`Self::visible_at`] 一站式
    /// 双层求值，不要在调用点自行拼接图级 ∧ 段级。
    pub fn is_visible_at(&self, t: i64) -> bool {
        match &self.time_span {
            None => true,
            Some(span) => span.contains(&TimePoint::from_day(t)),
        }
    }

    /// 双层合并可见性（ADR-0015 §2.3）：图级 ∧ 段级。
    ///
    /// - `map_span == None` ⇒ 图级不限时代（不否决段级）；
    /// - `map_span == Some(s)` 且 `!s.contains(t)` ⇒ 一律 false（图级否决）；
    /// - 否则按段级 `time_span` 判定。
    ///
    /// 与 [`wv_core::label_visible_at`] 同构：标签与几何同生共死——
    /// 街道几何不显示的时段，它的街名标注也不显示。
    pub fn visible_at(&self, map_span: Option<&TimeSpan>, t: i64) -> bool {
        if let Some(map_span) = map_span {
            if !map_span.contains(&TimePoint::from_day(t)) {
                return false;
            }
        }
        self.is_visible_at(t)
    }

    /// 折线中点（标签锚点）：按累计长度取半程处。与起终点距离各半，
    /// 多点折线也不会把标签钉在端点上。
    pub fn polyline_midpoint(&self) -> Option<Point> {
        self.polyline_midpoint_with_dir().map(|(p, _)| p)
    }

    /// 折线中点**连同所在段的行进方向**（单位向量）。
    ///
    /// 中点与 [`Self::polyline_midpoint`] 同一口径（累计长度半程）；方向取
    /// 半程落在的那一段折线的方向——沿街名标注（ADR-0073）要沿路旋转文字，
    /// 需要知道锚点处路往哪边走。零长折线（退化）返回方向 `(1.0, 0.0)`。
    pub fn polyline_midpoint_with_dir(&self) -> Option<(Point, (f32, f32))> {
        if self.points.is_empty() {
            return None;
        }
        let mut total = 0.0_f32;
        for w in self.points.windows(2) {
            total += (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
        }
        if total <= 0.0 {
            return Some((self.points.first().copied()?, (1.0, 0.0)));
        }
        let mut walked = 0.0_f32;
        for w in self.points.windows(2) {
            let dx = w[1].x - w[0].x;
            let dy = w[1].y - w[0].y;
            let leg = dx.hypot(dy);
            if walked + leg >= total * 0.5 {
                let t = ((total * 0.5 - walked) / leg).clamp(0.0, 1.0);
                let dir = if leg <= 0.0 {
                    (1.0, 0.0)
                } else {
                    (dx / leg, dy / leg)
                };
                return Some((Point::new(w[0].x + dx * t, w[0].y + dy * t), dir));
            }
            walked += leg;
        }
        Some((self.points.last().copied()?, (1.0, 0.0)))
    }
}

/// 一份街道图层文件（`Layer.source` 指向的 YAML）的文档根。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreetLayer {
    /// 全部街道段；顺序即绘制顺序（先画的在下）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<StreetSegment>,
}

impl StreetLayer {
    /// 空图层。
    pub fn new() -> Self {
        StreetLayer::default()
    }

    /// 追加一段。
    pub fn push(&mut self, segment: StreetSegment) {
        self.segments.push(segment);
    }

    /// 段数。
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::TimeSpan;

    // YAML round-trip 与 serde 缺省值的测试在 `wv-io/tests/street_layer_yaml.rs`
    // —— wv-core 按 AGENTS §2.1 不依赖 serde_yaml，round-trip 与 IO 同层验证。

    // ─── ADR-0015：双层 TimeSpan 可见性求值 ──────────────────────────────

    fn two_point_seg(time_span: Option<TimeSpan>) -> StreetSegment {
        StreetSegment {
            class: RoadClass::Residential,
            points: vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)],
            name: None,
            time_span,
        }
    }

    #[test]
    fn segment_without_time_span_is_always_segment_visible() {
        let seg = two_point_seg(None);
        assert!(seg.is_visible_at(0), "无 time_span：段级始终可见");
        assert!(seg.is_visible_at(1450), "无 time_span：段级始终可见");
    }

    #[test]
    fn segment_with_time_span_hides_outside_window() {
        let span = TimeSpan::new(1450, 1900).unwrap();
        let seg = two_point_seg(Some(span));
        assert!(
            !seg.is_visible_at(1449),
            "段窗口 [1450, 1900)：t=1449 不可见"
        );
        assert!(
            seg.is_visible_at(1450),
            "段窗口 [1450, 1900)：t=1450 可见（半开含左）"
        );
        assert!(seg.is_visible_at(1899), "段窗口 [1450, 1900)：t=1899 可见");
        assert!(
            !seg.is_visible_at(1900),
            "段窗口 [1450, 1900)：t=1900 不可见（半开不含右）"
        );
    }

    #[test]
    fn map_window_vetoes_segment_even_if_segment_window_covers_t() {
        // 图级窗口 [1200, 1450)，段级窗口 [1400, 1500)：交集仅 [1400, 1450)
        let map_span = TimeSpan::new(1200, 1450).unwrap();
        let seg_span = TimeSpan::new(1400, 1500).unwrap();
        let seg = two_point_seg(Some(seg_span));
        assert!(
            seg.is_visible_at(1425),
            "段级单独看：1425 在 [1400,1500) 内"
        );
        assert!(
            seg.visible_at(Some(&map_span), 1425),
            "双层交集内的 t 应可见"
        );
        // 关键断言：图级窗口外时双层合并一律不可见
        assert!(seg.is_visible_at(1475), "段级单独：1475 在 [1400,1500) 内");
        assert!(
            !seg.visible_at(Some(&map_span), 1475),
            "图级窗口外时双层合并一律不可见（即便段级单独可见）"
        );
        assert!(
            !seg.visible_at(Some(&map_span), 1199),
            "图级 ∧ 段级 同时不在窗口内：不可见"
        );
        assert!(
            seg.visible_at(None, 1475),
            "图级 None 不否决，段级 1475 可见"
        );
    }

    #[test]
    fn segment_window_inside_map_window_yields_strict_subset() {
        // 单调性：图级 ∧ 段级 的可见集 ⊆ 段级单独可见集
        let map_span = TimeSpan::new(1200, 1500).unwrap();
        let seg_span = TimeSpan::new(1300, 1400).unwrap();
        let seg = two_point_seg(Some(seg_span));
        for t in 1200..1500 {
            let single = seg.is_visible_at(t);
            let merged = seg.visible_at(Some(&map_span), t);
            assert!(
                !merged || single,
                "t={t}：双层合并可见 ⇒ 段级单独必可见（合并 ⊆ 单项）"
            );
        }
        for t in 1300..1400 {
            assert!(
                seg.visible_at(Some(&map_span), t),
                "t={t}：图级 ∧ 段级 同时覆盖，应可见"
            );
        }
    }

    #[test]
    fn open_segment_window_keeps_visible_from_start_to_present() {
        let seg = two_point_seg(Some(TimeSpan::open(1500)));
        assert!(
            !seg.is_visible_at(1499),
            "开放窗口 [1500, ∞)：t=1499 不可见"
        );
        assert!(seg.is_visible_at(1500), "开放窗口 [1500, ∞)：t=1500 可见");
        assert!(
            seg.is_visible_at(99_999),
            "开放窗口 [1500, ∞)：t 极大仍可见"
        );
    }

    #[test]
    fn segment_visibility_matches_label_visible_at() {
        // 与 label_visible_at 的同构承诺：街道几何与街名标注同生共死。
        let map = TimeSpan::new(1200, 1450).unwrap();
        let seg_span = TimeSpan::new(1400, 1500).unwrap();
        let seg = two_point_seg(Some(seg_span));
        for t in 1100..1600 {
            assert_eq!(
                seg.visible_at(Some(&map), t),
                crate::label_visible_at(Some(&map), seg.time_span.as_ref(), t),
                "t={t}：两函数结果必须一致（同构承诺）"
            );
        }
    }

    #[test]
    fn lod_rank_orders_motorway_before_service() {
        assert!(
            RoadClass::Motorway.lod_rank() < RoadClass::Secondary.lod_rank(),
            "motorway rank {} 应小于 secondary {}",
            RoadClass::Motorway.lod_rank(),
            RoadClass::Secondary.lod_rank()
        );
        assert!(
            RoadClass::Secondary.lod_rank() < RoadClass::Service.lod_rank(),
            "secondary rank {} 应小于 service {}",
            RoadClass::Secondary.lod_rank(),
            RoadClass::Service.lod_rank()
        );
    }

    /// ADR-0047 D3 折算表逐档钉死：lod 0–2 → 3、3–5 → 2、6–7 → 1，
    /// 且与旧 street 阈值（Nation ≤ 2 / Region ≤ 5 / Edit 全见）**逐档等价**——
    /// 换算本身在这里断言（等价性的渲染侧半段在 wv-render streets.rs）。
    #[test]
    fn lod_to_presentation_mapping_is_total_and_monotone() {
        let expect = [
            (0u8, 3u8),
            (1, 3),
            (2, 3), // 主干族 → 国家级
            (3, 2),
            (4, 2),
            (5, 2), // 次干族 → 区域级
            (6, 1),
            (7, 1), // 支末族 → 局部
        ];
        for (lod, pres) in expect {
            assert_eq!(
                RoadClass::presentation_rank_from_lod(lod),
                pres,
                "lod {lod} 应折算为呈现级别 {pres}"
            );
        }
        // 越界 lod（未来新增类别没跟上折算表）按最末档处理，不 panic。
        assert_eq!(
            RoadClass::presentation_rank_from_lod(8),
            1,
            "未知 lod 归最低呈现档"
        );
    }

    #[test]
    fn polyline_midpoint_splits_the_line_in_half() {
        let seg = StreetSegment {
            class: RoadClass::Primary,
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
            name: Some("中山路".into()),
            time_span: None,
        };
        let m = seg.polyline_midpoint().expect("两点直段必有中点");
        assert!(
            (m.x - 5.0).abs() < 1e-4 && m.y.abs() < 1e-4,
            "中点应为 (5, 0)，实得 ({}, {})",
            m.x,
            m.y
        );
    }

    #[test]
    fn polyline_midpoint_lands_on_the_long_leg_of_a_bend() {
        // L 形：竖边 6 + 横边 4，半程 5 落在竖边上 (0, 5)。
        let seg = StreetSegment {
            class: RoadClass::Path,
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(0.0, 6.0),
                Point::new(4.0, 6.0),
            ],
            name: None,
            time_span: None,
        };
        let m = seg.polyline_midpoint().expect("折线必有中点");
        assert!(
            m.x.abs() < 1e-4 && (m.y - 5.0).abs() < 1e-4,
            "中点应落在长边 (0, 5)，实得 ({}, {})",
            m.x,
            m.y
        );
    }

    /// 中点方向 = 半程所在折线段的方向（沿街旋转标注的数据源，ADR-0073）。
    #[test]
    fn polyline_midpoint_with_dir_returns_direction_of_the_leg() {
        // 直段：中点 (5, 0)，方向 +x。
        let straight = StreetSegment {
            class: RoadClass::Path,
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
            name: None,
            time_span: None,
        };
        let (m, d) = straight
            .polyline_midpoint_with_dir()
            .expect("直段必有中点方向");
        assert!(
            (m.x - 5.0).abs() < 1e-4 && m.y.abs() < 1e-4,
            "got ({}, {})",
            m.x,
            m.y
        );
        assert!((d.0 - 1.0).abs() < 1e-4 && d.1.abs() < 1e-4, "got {:?}", d);

        // L 形：半程落在竖边上，方向应朝 +y 而非横边 +x。
        let bend = StreetSegment {
            class: RoadClass::Path,
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(0.0, 6.0),
                Point::new(4.0, 6.0),
            ],
            name: None,
            time_span: None,
        };
        let (m, d) = bend.polyline_midpoint_with_dir().expect("折线必有中点方向");
        assert!(
            m.x.abs() < 1e-4 && (m.y - 5.0).abs() < 1e-4,
            "got ({}, {})",
            m.x,
            m.y
        );
        assert!(d.0.abs() < 1e-4 && (d.1 - 1.0).abs() < 1e-4, "got {:?}", d);
    }
}
