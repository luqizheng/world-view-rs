//! 标签层的数据模型与可见性求值（ADR-0016 · 2026-10-14 接受）。
//!
//! ## 职责
//! - [`EntityKind`]：标签投影源的实体类型白名单（§2.2）。
//! - [`ZoomTier`]：标签最小显示档位（§2.4），与 `wv-app` 的 `LodTier` 同形但
//!   **解耦**——`wv-core` 不能依赖 `wv-app`（AGENTS §2.1 分层约束）。
//! - [`LabelLayerConfig`]：`Layer.source` 指向的 YAML 根（§2.1），**只存配置不存文本**。
//! - [`label_visible_at`]：可见性求值的**单一实现点**（§2.3），与
//!   [`crate::source::street::StreetSegment::visible_at`] 同构。
//!
//! ## 边界（不做什么）
//! - **不**存文本本身——文本是实体（`Place.name` / `Region.name` / `Faction.name` /
//!   `StreetSegment.name`）的属性，标签层**派生**而非输入（§2.1）。
//! - **不**实现渲染 / 碰撞 / 排版——这些在 `wv-app` 的标签渲染管线（待落地）。
//! - **不**存字体 / 颜色 / 主题——这些走 ADR-0013 的主题系统（§2.8）。
//!
//! ## 与 `StreetLayer` 的对称
//! 两个「图层数据文件」走同一种组织：
//! - `StreetLayer` = `Vec<StreetSegment>`（段级数据 + 段级 `time_span`）；
//! - `LabelLayerConfig` = 配置集合（白名单 + 字号阶梯 + 优先级），**没有**段级
//!   列表——段级列表由实体集合在求值时构造。`label_visible_at` 是这个**派生**
//!   过程的第一步（可见性），后续渲染在 `wv-app` 完成。
//!
//! ## 与 `MapNode.time_span` 的关系
//! `MapNode.time_span` 是**图级时代窗口**（ADR-0015），`label_visible_at` 把
//! 它作为第一个参数——窗口外时**一律不可见**，无论实体自己的时间状态如何。
//! 这与 `StreetSegment::visible_at(map_span, t)` 完全同构。

use std::collections::BTreeMap;

use crate::time::{TimePoint, TimeSpan};

/// 标签投影源的实体类型（ADR-0016 §2.2）。
///
/// 白名单用：`LabelLayerConfig.entity_kinds: Vec<EntityKind>`。
/// 渲染管线遍历工程内的实体集合，仅对白名单内的实体生成标签。
///
/// ## 与领域 `PlaceKind` / `Faction` / `StreetSegment` 的关系
/// `EntityKind` 是**标签投影这一层**的分类（决定"要不要画文本"），与领域
/// 实体的精细分类（`PlaceKind::City` / `PlaceKind::Village` 等）**正交**——
/// 一个 `Place` 实体如果 kind 在 `EntityKind::Place` 白名单内，就参与标签投影；
/// 其具体的 `PlaceKind` 决定字号权重，不决定"画不画"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// `Place` 实体（点标签：城市 / 村庄 / 地标等）。
    Place,
    /// `Region` 实体（多边形中心标签）。
    Region,
    /// `Faction` 实体（势力面中心标签）。
    Faction,
    /// `StreetSegment` 实体（折线半程标签，沿用 `polyline_midpoint()`）。
    StreetSegment,
    /// `Event` 实体（事件点位置标签）。
    Event,
}

impl EntityKind {
    /// 全部实体类型的静态表。
    pub const ALL: [EntityKind; 5] = [
        EntityKind::Place,
        EntityKind::Region,
        EntityKind::Faction,
        EntityKind::StreetSegment,
        EntityKind::Event,
    ];

    /// 中文显示名（纯显示，YAML 仍存 snake_case 英文键）。
    pub fn label(self) -> &'static str {
        match self {
            EntityKind::Place => "地点",
            EntityKind::Region => "区域",
            EntityKind::Faction => "势力",
            EntityKind::StreetSegment => "街道",
            EntityKind::Event => "事件",
        }
    }
}

/// 标签最小显示档位（ADR-0016 §2.4）。
///
/// 数据层概念——`wv-core` 不依赖 `wv-app` 的 `LodTier`，因此独立定义。
/// 三个值按"放大方向"递增：`Nation` < `Region` < `Edit`；`min_zoom_tier` 表示
/// "该档位及更近才显示"。消费时由 `wv-app` 翻译成自己的 `LodTier`。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ZoomTier {
    /// 国家/世界级（远观）。
    Nation,
    /// 省级。
    Region,
    /// 编辑级（局部放大）。
    Edit,
}

impl ZoomTier {
    /// 全部档位的静态表。
    pub const ALL: [ZoomTier; 3] = [ZoomTier::Nation, ZoomTier::Region, ZoomTier::Edit];

    /// 缺省最小档位（§2.4 默认）：`Region`——国家级不画文字，省级及以上开始。
    pub const DEFAULT_MIN: ZoomTier = ZoomTier::Region;

    /// 缺省字号（§2.4 默认值；屏幕像素 × `view.scale`）。
    pub const DEFAULT_FONT_SIZES: &'static [(ZoomTier, f32)] = &[
        (ZoomTier::Nation, 9.0),
        (ZoomTier::Region, 11.0),
        (ZoomTier::Edit, 14.0),
    ];

    /// 缺省最大标签数（§2.6 默认值）。
    pub const DEFAULT_MAX_LABELS: usize = 200;

    /// 中文显示名。
    pub fn label(self) -> &'static str {
        match self {
            ZoomTier::Nation => "国家级",
            ZoomTier::Region => "省级",
            ZoomTier::Edit => "编辑级",
        }
    }
}

/// 标签层配置（ADR-0016 §2.1 / §2.2 / §2.4 / §2.6）。
///
/// `Layer.source` 指向的 YAML 根。**只存配置，不存文本**——文本由实体投影。
///
/// ## YAML 形态示例
///
/// ```yaml
/// entity_kinds: [place, region, faction]
/// min_zoom_tier: region
/// font_size_ladder:
///   nation: 9.0
///   region: 11.0
///   edit: 14.0
/// max_labels: 200
/// priority_kinds: [place]
/// ```
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LabelLayerConfig {
    /// 实体类型白名单（§2.2）。空 = 该层不显示任何标签。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_kinds: Vec<EntityKind>,

    /// 最低显示档位（§2.4）。该档位及"更近"才显示标签。
    /// 缺省 = [`ZoomTier::DEFAULT_MIN`] = `Region`。
    #[serde(default = "default_min_zoom_tier")]
    pub min_zoom_tier: ZoomTier,

    /// 字号阶梯（§2.4）。屏幕像素 = 数值 × `view.scale`。
    /// 缺省 = [`ZoomTier::DEFAULT_FONT_SIZES`]（9 / 11 / 14）。
    /// 声明则全权覆盖——缺失的档位由渲染层用上一个"更近"的档位兜底。
    #[serde(
        default = "default_font_size_ladder",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub font_size_ladder: BTreeMap<ZoomTier, f32>,

    /// 最大标签数（§2.6）。超限时按优先级截断。
    /// 缺省 = [`ZoomTier::DEFAULT_MAX_LABELS`] = 200。
    #[serde(default = "default_max_labels")]
    pub max_labels: usize,

    /// 碰撞时优先级（§2.6）。**白名单内的** `EntityKind` 集合；空 = 全部等同。
    /// 列表顺序 = 优先级顺序（先列先保留）。细分到 `PlaceKind` 的优先级在
    /// `wv-app` 落地（v0.1 scope 之外，本 ADR 不展开）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub priority_kinds: Vec<EntityKind>,
}

impl Default for LabelLayerConfig {
    fn default() -> Self {
        Self {
            entity_kinds: Vec::new(),
            min_zoom_tier: ZoomTier::DEFAULT_MIN,
            font_size_ladder: ZoomTier::DEFAULT_FONT_SIZES.iter().copied().collect(),
            max_labels: ZoomTier::DEFAULT_MAX_LABELS,
            priority_kinds: Vec::new(),
        }
    }
}

impl LabelLayerConfig {
    /// 空配置（白名单为空 → 不显示任何标签）。等价于 `Default::default()`。
    pub fn new() -> Self {
        Self::default()
    }

    /// `entity_kinds` 是否包含某实体类型——白名单查表。
    pub fn includes(&self, kind: EntityKind) -> bool {
        self.entity_kinds.contains(&kind)
    }

    /// 给定档位的字号；缺省时回退到下一个"更近"档位（§2.4 兜底）。
    pub fn font_size_at(&self, tier: ZoomTier) -> f32 {
        // 自身或更近档位（tier 自身在 BTreeMap 中；BTreeMap 用 Ord，ZoomTier
        // 已按 Nation < Region < Edit 实现 PartialOrd/Ord）。
        self.font_size_ladder
            .range(tier..)
            .next()
            .map(|(_, size)| *size)
            .unwrap_or_else(|| {
                // 表为空或全是"更远"档位——回退到缺省值的最近档位
                ZoomTier::DEFAULT_FONT_SIZES
                    .iter()
                    .rev()
                    .find(|(t, _)| *t <= tier)
                    .map(|(_, s)| *s)
                    .unwrap_or(11.0) // 终极兜底：Region 缺省 11
            })
    }

    /// 档位是否"足够近"以显示标签（§2.4 入口）。
    pub fn tier_visible(&self, current: ZoomTier) -> bool {
        current >= self.min_zoom_tier
    }
}

fn default_min_zoom_tier() -> ZoomTier {
    ZoomTier::DEFAULT_MIN
}

fn default_max_labels() -> usize {
    ZoomTier::DEFAULT_MAX_LABELS
}

fn default_font_size_ladder() -> BTreeMap<ZoomTier, f32> {
    ZoomTier::DEFAULT_FONT_SIZES.iter().copied().collect()
}

/// 标签可见性求值（ADR-0016 §2.3）——**单一实现点**。
///
/// 与 [`crate::source::street::StreetSegment::visible_at`] 完全同构：
/// 图级窗口 ∧ 实体级时间区间。**所有调用方必须走此函数**，不得在调用点
/// 重写 `time_span.contains(t)` 的公式。
///
/// ## 参数
///
/// - `map_span`：当前 `MapNode.time_span`（ADR-0015）。`None` = 子图不限时代。
/// - `entity_span`：实体的存在窗口。语义因实体类型而异：
///   - `Place` / `Faction`：取 `state_spans` 中**任一**覆盖 `t` 的片段（多片段并存按 P7 规则）；
///   - `Region`：当前 v0.1 无 `state_spans` 字段，传 `None`（永远可见）；
///   - `StreetSegment`：取 `seg.time_span`；
///   - `Event`：由调用方把 `event.time` 包成 `TimeSpan::new(event.time, event.time + 1)`
///     或 `TimeSpan::open(event.time)`（事件瞬间 vs 事件"在场"窗口，scope 由调用方决定）。
/// - `t`：当前时间（day 计数器）。
///
/// ## 返回
///
/// `true` = 标签参与本帧渲染；`false` = 不参与。
pub fn label_visible_at(
    map_span: Option<&TimeSpan>,
    entity_span: Option<&TimeSpan>,
    t: i64,
) -> bool {
    // 图级否决（图级窗口外一律不可见，无论实体如何）。
    if let Some(map_span) = map_span {
        if !map_span.contains(&TimePoint::from_day(t)) {
            return false;
        }
    }
    // 实体级（None = 永远可见；Some = 被窗口包住才可见）。
    match entity_span {
        None => true,
        Some(span) => span.contains(&TimePoint::from_day(t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── 枚举：基本性质 ──────────────────────────────────────────────────

    #[test]
    fn entity_kind_label_is_chinese() {
        // 显示名不是空串、不含 ASCII 噪声。
        for kind in EntityKind::ALL {
            assert!(!kind.label().is_empty(), "{kind:?} 应有显示名");
            assert!(!kind.label().is_ascii(), "{kind:?} 显示名应含中文");
        }
    }

    #[test]
    fn zoom_tier_orders_nation_before_region_before_edit() {
        // Ord 决定 font_size_at 的回退方向——绝不能错。
        assert!(ZoomTier::Nation < ZoomTier::Region);
        assert!(ZoomTier::Region < ZoomTier::Edit);
        assert!(ZoomTier::Nation < ZoomTier::Edit);
    }

    // ─── LabelLayerConfig：白名单与查表 ──────────────────────────────────

    #[test]
    fn empty_config_includes_nothing() {
        let cfg = LabelLayerConfig::new();
        for kind in EntityKind::ALL {
            assert!(!cfg.includes(kind), "空配置不应包含任何 kind");
        }
    }

    #[test]
    fn whitelist_includes_only_listed_kinds() {
        let mut cfg = LabelLayerConfig::new();
        cfg.entity_kinds = vec![EntityKind::Place, EntityKind::Faction];
        assert!(cfg.includes(EntityKind::Place));
        assert!(cfg.includes(EntityKind::Faction));
        assert!(!cfg.includes(EntityKind::Region));
        assert!(!cfg.includes(EntityKind::StreetSegment));
        assert!(!cfg.includes(EntityKind::Event));
    }

    // ─── LabelLayerConfig：字号与档位 ─────────────────────────────────────

    #[test]
    fn tier_visible_respects_min_zoom_tier() {
        let mut cfg = LabelLayerConfig::new();
        cfg.min_zoom_tier = ZoomTier::Region;
        assert!(
            !cfg.tier_visible(ZoomTier::Nation),
            "min=Region 时 Nation 不可见"
        );
        assert!(
            cfg.tier_visible(ZoomTier::Region),
            "min=Region 时 Region 可见"
        );
        assert!(cfg.tier_visible(ZoomTier::Edit), "min=Region 时 Edit 可见");
    }

    #[test]
    fn default_font_size_ladder_has_all_three_tiers() {
        let cfg = LabelLayerConfig::new();
        for tier in ZoomTier::ALL {
            let size = cfg.font_size_at(tier);
            assert!(size > 0.0, "{tier:?} 字号应 > 0，实得 {size}");
        }
    }

    #[test]
    fn font_size_at_falls_back_to_closer_tier_when_missing() {
        // 自定义 ladder：只声明 Nation 与 Edit，Region 缺位。
        let mut cfg = LabelLayerConfig::new();
        cfg.font_size_ladder =
            BTreeMap::from_iter([(ZoomTier::Nation, 7.0), (ZoomTier::Edit, 15.0)]);
        // Region 缺位 → 兜底到 Nation（更远档位 + BTreeMap range(tier..) 跳过自身）？
        // 不：BTreeMap::range(Region..) 的第一个 key 是 Edit（>= Region），
        // 因为 Edit 在 BTreeMap 里排在 Region 之后。验证回退落到 Edit。
        let region_size = cfg.font_size_at(ZoomTier::Region);
        assert!(
            (region_size - 15.0).abs() < 1e-6,
            "Region 缺位时回退到 Edit=15.0；实得 {region_size}"
        );
    }

    #[test]
    fn font_size_at_returns_declared_value_when_present() {
        let mut cfg = LabelLayerConfig::new();
        cfg.font_size_ladder = BTreeMap::from_iter([
            (ZoomTier::Nation, 7.0),
            (ZoomTier::Region, 10.0),
            (ZoomTier::Edit, 13.0),
        ]);
        assert!((cfg.font_size_at(ZoomTier::Nation) - 7.0).abs() < 1e-6);
        assert!((cfg.font_size_at(ZoomTier::Region) - 10.0).abs() < 1e-6);
        assert!((cfg.font_size_at(ZoomTier::Edit) - 13.0).abs() < 1e-6);
    }

    #[test]
    fn empty_ladder_falls_back_to_built_in_default() {
        let mut cfg = LabelLayerConfig::new();
        cfg.font_size_ladder = BTreeMap::new();
        // 全部档位应回退到 DEFAULT_FONT_SIZES 的最近匹配。
        assert!((cfg.font_size_at(ZoomTier::Nation) - 9.0).abs() < 1e-6);
        assert!((cfg.font_size_at(ZoomTier::Region) - 11.0).abs() < 1e-6);
        assert!((cfg.font_size_at(ZoomTier::Edit) - 14.0).abs() < 1e-6);
    }

    // ─── label_visible_at：双层求值（与 street_visible_at 同构）─────────

    #[test]
    fn both_none_means_always_visible() {
        assert!(label_visible_at(None, None, 0));
        assert!(label_visible_at(None, None, 99_999));
    }

    #[test]
    fn only_entity_span_filters_by_entity_window() {
        let span = TimeSpan::new(1200, 1500).unwrap();
        assert!(label_visible_at(None, Some(&span), 1300));
        assert!(!label_visible_at(None, Some(&span), 1199));
        assert!(!label_visible_at(None, Some(&span), 1500));
    }

    #[test]
    fn only_map_span_filters_by_map_window() {
        let span = TimeSpan::new(1200, 1500).unwrap();
        assert!(label_visible_at(Some(&span), None, 1300));
        assert!(!label_visible_at(Some(&span), None, 1199));
    }

    #[test]
    fn map_window_vetoes_even_when_entity_window_covers_t() {
        // 图级窗口外时一律不可见（与 StreetSegment::visible_at 同构）。
        let map_span = TimeSpan::new(1200, 1450).unwrap();
        let entity_span = TimeSpan::new(1400, 1500).unwrap();
        // t=1475：entity 单独可见；map 窗口外 → 合并不可见
        assert!(!label_visible_at(Some(&map_span), Some(&entity_span), 1475));
        // t=1199：两者都不覆盖 → 不可见
        assert!(!label_visible_at(Some(&map_span), Some(&entity_span), 1199));
        // t=1425：两者都覆盖 → 可见
        assert!(label_visible_at(Some(&map_span), Some(&entity_span), 1425));
    }

    #[test]
    fn open_window_keeps_visible_from_start_to_present() {
        let map = TimeSpan::open(1900);
        assert!(label_visible_at(Some(&map), None, 99_999));
        assert!(!label_visible_at(Some(&map), None, 1899));
    }

    #[test]
    fn intersects_visibility_holds_for_500_day_window() {
        // 用 500 天的稠密网格验证合并规则的连续性（无突变）。
        let map = TimeSpan::new(1200, 1450).unwrap();
        let entity = TimeSpan::new(1400, 1500).unwrap();
        // 期望可见集 = map ∩ entity = [1400, 1450)（半开）
        let mut count_visible = 0_usize;
        for t in 1100..1600 {
            if label_visible_at(Some(&map), Some(&entity), t) {
                count_visible += 1;
            }
        }
        assert_eq!(count_visible, 50, "交集 [1400,1450) 长度 50，应可见 50 次");
    }
}
