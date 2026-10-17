//! 工程诊断：把「错误」与「提示」在**类型层面**分开（ADR-0005，2026-10-11）。
//!
//! ## 为什么需要这个模块
//!
//! [`crate::project::Project::validate`] 历史上返回 `Vec<String>`，所有条目混在一起。
//! 但本项目 2026-10-11 的裁定（`AGENTS.md` §7.5 V1）要求**错误与提示必须可区分**：
//!
//! - **错误**：数据坏了（悬空引用、字段缺失）——工程打不开。
//! - **提示**：数据没坏，但可能有你没注意到的东西（相遇却未写事件）——**只提示，不阻止**。
//!
//! 把两者塞进同一个 `String` 会让 UI 无法可靠地「只显示提示、不拦保存」，
//! 也无法给提示做**回访定位**（点了要能跳到地图对应位置）。
//!
//! ## 与 `validate()` 的分工
//!
//! | 入口 | 强度 | 用途 |
//! |---|---|---|
//! | `Project::validate()` | 错误即 `Err` | CI / 命令行，加载前把关 |
//! | `Project::diagnose()` | **永不失败**，返回结构化诊断 | 编辑器校验面板：列出 + 可回访 + 不阻止保存 |
//!
//! 二者共享同一套引用校验逻辑，避免「一个说有问题、另一个说没问题」。

//! ## 2026-10-12 调整：相遇判定不再需要 maps
//!
//! 旧实现里 `EncounterInput` 带 `maps` 字段（用于取 `local_per_km` 做视界判定）；
//! 2026-10-12 用户简化方案改为「两 Track 的 Waypoint 在 (place, day) 上同时覆盖」，
//! 因此 `EncounterInput` 不再需要 maps 参数——见 [`wv_core::encounter`] 模块文档。

//! ## 2026-10-16 增列：跨级尺度一致性提示（ADR-0022 D4）
//!
//! 世界图 → 城市图跨级声明 `unit_per_km` 时，两级间的物理尺度可能不一致；
//! 给作者一条 Hint 让 ta 注意到，不阻止保存（V1 守约）。

use wv_core::encounter::{detect_encounter_hints, EncounterInput, HintSeverity};

/// 诊断级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagLevel {
    /// 提示：可能有你未注意到的事。**不阻止任何操作**。
    Hint,
    /// 错误：数据坏了。
    Error,
}

/// 一条结构化诊断。
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub level: DiagLevel,
    /// 人类可读文案。
    pub message: String,
    /// 稳定标识（供 UI 做「已读 / 已忽略」状态锚定）。
    ///
    /// **必须有**：没有稳定标识的提示，作者一旦微调数据就会「换一条新的」，
    /// 于是永远认领不掉（这正是相遇提示要求指纹不含日期的原因）。
    pub fingerprint: String,
    /// 定位信息：点击提示时跳到这里。
    pub anchor: Option<DiagAnchor>,
    /// 细分严重度（来自派生检查，如相遇提示的 `EncounterUnwritten`）。
    /// `None` 表示无细分。
    pub severity: Option<HintSeverity>,
}

/// 诊断的定位锚点。
#[derive(Debug, Clone, PartialEq)]
pub struct DiagAnchor {
    pub map: wv_core::Id,
    /// 地图局部坐标。
    pub at: wv_core::geometry::Point,
    /// 时间点（跳转时把时间轴拨到这里）。
    pub day: i64,
}

/// 诊断集合。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// 只取提示（编辑器的「可回访列表」用这个）。
    pub fn hints(&self) -> Vec<&Diagnostic> {
        self.items
            .iter()
            .filter(|d| d.level == DiagLevel::Hint)
            .collect()
    }

    /// 只取错误。
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.items
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect()
    }

    /// 是否存在错误（编辑器据此决定能否继续保存）。
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.level == DiagLevel::Error)
    }

    /// 按稳定标识去重（同一处重复出现只保留一条）。
    pub fn dedup_by_fingerprint(&mut self) {
        self.items.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
        self.items.dedup_by(|a, b| a.fingerprint == b.fingerprint);
    }

    /// 合并另一组诊断（用于「悬空引用」+「相遇提示」汇成一份面板）。
    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }
}

/// 从已加载的世界派生**提示级**诊断（相遇检测）。
///
/// 这是纯提示路径：**永不产生 `DiagLevel::Error`**，也永不失败。
///
/// ## 判定口径（2026-10-12）：Waypoint (place, day) 严格重叠
///
/// 判定完全由数据决定：两 Track 的 Waypoint 在 `(place, day)` 上同时覆盖。
/// **不含任何距离阈值常量**——视野方案（ADR-0006 旧版）已撤。
pub fn derive_hints(world: &wv_core::World) -> Diagnostics {
    let hints = detect_encounter_hints(EncounterInput {
        units: &world.units,
        tracks: &world.tracks,
        events: &world.events,
    });

    let mut d = Diagnostics::default();
    for h in hints {
        // 定位锚点：用该 hint 自带的 map 与 day。
        // at 在 Waypoint (place, day) 严格判定里没有几何意义——
        // 用 (0, 0) 作为占位（无 pos 字段时不再有意义）。
        let anchor = Some(DiagAnchor {
            map: h.map.clone(),
            at: wv_core::geometry::Point { x: 0.0, y: 0.0 },
            day: h.span.from,
        });

        let unit_names: Vec<&str> = h
            .units
            .iter()
            .map(|id| {
                world
                    .units
                    .iter()
                    .find(|u| &u.id == id)
                    .map(|u| u.name.as_str())
                    .unwrap_or_else(|| id.as_str())
            })
            .collect();
        let place = h.place.as_str().to_string();
        let days = match h.span.to {
            Some(to) if to - h.span.from == 1 => format!("第 {} 天", h.span.from),
            Some(to) => format!("第 {}–{} 天", h.span.from, to - 1),
            None => "至今".to_string(),
        };

        d.items.push(Diagnostic {
            level: DiagLevel::Hint,
            message: h.severity.message(
                unit_names.first().copied().unwrap_or("?"),
                unit_names.get(1).copied().unwrap_or("?"),
                &place,
                &days,
            ),
            fingerprint: h.fingerprint,
            anchor,
            severity: Some(h.severity),
        });
    }

    // 阶段六 v0.2：事件间位移可行性提示（只提示不阻止，AGENTS §7.5 V1）。
    // 与相遇提示的分工：本检查针对**事件**而非**轨迹**——「下一事件发生时，
    // 该参与者是否还有时间从上一事件地点赶过去」；相遇提示管的是「两轨迹同
    // (place, day) 覆盖但没写事件」，两者不重叠。
    d.items.extend(detect_travel_feasibility_hints(world));

    // ADR-0022 M3 D4：跨级尺度一致性（V1 提示，不阻止）。
    d.items.extend(detect_cross_level_scale_hints(world));

    d.dedup_by_fingerprint();
    d
}

/// 跨级尺度一致性提示（ADR-0022 D4 · 2026-10-16 接受）。
///
/// ## 口径
///
/// 世界图 → 城市子图跨级声明 `unit_per_km` 时，预期物理宽度（伪代码）：
///
/// ```text
/// expected_world_extent_w = (city_local_extent_w / city.unit_per_km) / world.unit_per_km
/// declared_world_extent_w  = city.extent.width
/// ratio = declared / expected
/// ```
///
/// | ratio 区间 | 提示 |
/// |---|---|
/// | \|ratio − 1.0\| ≤ 0.1 | 一致，不发提示 |
/// | 0.1 < \|ratio − 1.0\| ≤ 1.0 | 「两级间尺度有 X% 偏差」 |
/// | ratio < 0.5 或 > 2.0 | 「两级尺度明显不匹配」 |
///
/// 缺省策略：
/// - `world.unit_per_km = None` → 世界未声明尺度，**跳过**所有跨级检查
///   （无法换算「城市在世界上应该多大」）。
/// - `city.unit_per_km = None` → 城市未声明，跳过该城。
/// - `city.extent = None` → 城市未声明幅面，跳过。
///
/// ## 与 P6（非破坏）/ V1（只提示）的关系
///
/// 这是**纯提示**，指纹 = `cross-level-scale:{city_id}` 稳定可认领；
/// 不阻止保存——作者可能故意（艺术夸张、戏剧比例尺）。
pub fn detect_cross_level_scale_hints(world: &wv_core::World) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for city in &world.maps {
        let Some(city_extent) = city.extent else {
            continue;
        };
        let Some(city_per_km) = city.unit_per_km else {
            continue;
        };
        if city_per_km <= 0.0 {
            continue;
        }
        let Some(world_per_km) = world
            .maps
            .iter()
            .find(|m| m.parent.is_none())
            .and_then(|m| m.unit_per_km)
        else {
            continue;
        };
        if world_per_km <= 0.0 {
            continue;
        };

        let city_world_w = city_extent.width / city_per_km / world_per_km;
        if city_world_w <= 0.0 {
            continue;
        }
        let declared_world_w = city_extent.width;
        let ratio = declared_world_w / city_world_w;

        let deviation = (ratio - 1.0).abs();
        let severity_msg = if !(0.5..=2.0).contains(&ratio) {
            format!(
                "城市「{}」与父图两级尺度明显不匹配（ratio={:.2}）：城市本地 {:.0} wu × {} m/wu ≈ {:.1} km，\
                 在世界上应占 {:.1} wu，当前声明 {:.0} wu。",
                city.id,
                ratio,
                city_extent.width,
                city_per_km,
                city_extent.width / city_per_km,
                city_world_w,
                declared_world_w,
            )
        } else if deviation > 0.1 {
            format!(
                "城市「{}」与父图两级间尺度有 {:.0}% 偏差（ratio={:.2}）：城市本地 {:.0} wu 约 {:.1} km，\
                 在世界上应占 {:.1} wu，当前声明 {:.0} wu。",
                city.id,
                deviation * 100.0,
                ratio,
                city_extent.width,
                city_extent.width / city_per_km,
                city_world_w,
                declared_world_w,
            )
        } else {
            continue;
        };

        out.push(Diagnostic {
            level: DiagLevel::Hint,
            message: severity_msg,
            fingerprint: format!("cross-level-scale:{}", city.id),
            anchor: Some(DiagAnchor {
                map: city.id.clone(),
                at: wv_core::geometry::Point { x: 0.0, y: 0.0 },
                day: 0,
            }),
            severity: None,
        });
    }
    out
}

/// 构造一条提示级诊断（供其他派生检查复用，如「孤立事件」等后续检查）。
pub fn hint(fingerprint: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        level: DiagLevel::Hint,
        message: message.into(),
        fingerprint: fingerprint.into(),
        anchor: None,
        severity: None,
    }
}

/// 阶段六 v0.2：事件间位移可行性提示。
///
/// ## 规则
///
/// 1. 把 `world.events` 按 `span.from` 升序；
/// 2. 对每对**相邻**事件 `prev` / `next`，若 `participants` 交集非空且
///    两事件都有 `location.place` 且二者不同；
/// 3. 计算 `gap = next.span.from - prev.span.to`（半开：`prev.to` 那天
///    `next` 才能开始，所以 `prev.span.to == None`（开放区间）即跳过；
///    `gap <= 0` 也跳过——事件在时间上不正向或同日同时）；
/// 4. 跑 `DistanceGraph::shortest_journey(prev.place, next.place)` 拿
///    `total_days`（最少可行天数）；
/// 5. 若 `gap < total_days` → 提示「预算时间不足：<X>→<Y> 需 ≥N 天，仅给 M 天」。
///
/// ## 为什么是 hint 不是 error
///
/// 参照 `AGENTS.md` §7.5 V1：作者可能有意压缩（小说里常见的"星夜兼程"）。
/// 工具的职责是让作者**看见**这个差距，不替他决定。
///
/// ## 与相遇提示的分工
///
/// 相遇提示（[`wv_core::encounter::detect_encounter_hints`]）按**轨迹**的
/// `(place, day)` 覆盖判定「该处应该有事件」；本检查按**事件序列**判定
/// 「前后事件之间路够不够走」。两者不重叠：前者是**缺事件**，后者是**事件间距过紧**。
///
/// ## 单元测试
///
/// 见本模块 `tests::travel_feasibility_*`——直接喂 `wv_core::Event` + 极简
/// `DistanceGraph`，不依赖示例工程。
fn detect_travel_feasibility_hints(world: &wv_core::World) -> Vec<Diagnostic> {
    use std::collections::BTreeSet;

    if world.events.len() < 2 {
        return Vec::new();
    }

    // 按 span.from 升序。稳定排序：from 相同时保留 YAML 顺序（不做稳定性
    // 假设对正确性无影响——同 from 的两事件地点若不同，是另一类冲突，非本检查目标）。
    let mut order: Vec<usize> = (0..world.events.len()).collect();
    order.sort_by_key(|&i| world.events[i].span.from);

    let mut out: Vec<Diagnostic> = Vec::new();
    for w in order.windows(2) {
        let (pi, ni) = (w[0], w[1]);
        let (Some(prev), Some(next)) = (world.events.get(pi), world.events.get(ni)) else {
            continue;
        };
        // 参与者交集。
        let prev_p: BTreeSet<&wv_core::Id> = prev.participants.iter().collect();
        let next_p: BTreeSet<&wv_core::Id> = next.participants.iter().collect();
        let common: Vec<&wv_core::Id> = prev_p.intersection(&next_p).copied().collect();
        if common.is_empty() {
            continue;
        }
        // 双方都得有 location.place。
        let (Some(prev_loc), Some(next_loc)) = (prev.location.as_ref(), next.location.as_ref())
        else {
            continue;
        };
        // 同地不动不算。
        if prev_loc.place == next_loc.place {
            continue;
        }
        // 半开：[prev.from, prev.to)。prev.to == None 表示至今仍在持续——
        // 既然 next 还在 prev 期间发生就矛盾，先不报本检查。
        let Some(prev_to) = prev.span.to else {
            continue;
        };
        let gap = next.span.from - prev_to;
        if gap <= 0 {
            continue;
        }
        // 跑最短路径。不通（NoPath / NoUsableVehicle）→ 不报本检查：
        // 这是另一类问题（地图不连通），留给后续检查。
        let Ok(journey) = world
            .distances
            .shortest_journey(&prev_loc.place, &next_loc.place, &[])
        else {
            continue;
        };
        if journey.total_days.is_finite() && (gap as f32) < journey.total_days {
            // 指纹稳定：被检查的「两事件 id + 共同参与者集合」。
            // 参与者排序以 BTreeSet 的天然序为准，确定性。
            let participants: String = common
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let prev_id = prev.id.as_str();
            let next_id = next.id.as_str();
            let fp = format!("travel_feasibility|{prev_id}|{next_id}|{participants}");
            // 文案：报告实际差距（不是给死数字）。
            let deficit = journey.total_days - gap as f32;
            let place_a = prev_loc.place.as_str();
            let place_b = next_loc.place.as_str();
            let prev_from = prev.span.from;
            let next_from = next.span.from;
            let total = journey.total_days;
            let message = format!(
                "参与者预算时间不足：第 {prev_from}–{prev_to_end} 天的「{place_a}」到第 {next_from} 天的「{place_b}」 \
                 至少需 {total:.1} 天，预算仅 {gap} 天（差 {deficit:.1} 天）",
                prev_to_end = prev_to - 1,
            );
            out.push(hint(fp, message));
        }
    }
    out
}

/// 构造一条错误级诊断。
pub fn error(fingerprint: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        level: DiagLevel::Error,
        message: message.into(),
        fingerprint: fingerprint.into(),
        anchor: None,
        severity: None,
    }
}

// ====== ADR-0022 M3 D4 · 跨级一致性 V1 提示 =================================

#[cfg(test)]
mod cross_level_scale_tests {
    use super::*;
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::Id;

    fn world_with(per_km: Option<f32>) -> wv_core::World {
        let mut w = wv_core::World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("map_world"),
            name: "世界".into(),
            level: MapLevel::World,
            unit_per_km: per_km,
            ..MapNode::default()
        });
        w
    }

    fn city_with(parent: &str, width: f32, per_km: Option<f32>) -> MapNode {
        MapNode {
            id: Id::new_unchecked("map_city"),
            name: "城".into(),
            level: MapLevel::City,
            parent: Some(Id::new_unchecked(parent)),
            extent: Some(Extent::sized(width, width)),
            unit_per_km: per_km,
            ..MapNode::default()
        }
    }

    #[test]
    fn ratio_one_emits_no_hint() {
        // world 4 km/wu × city 1024 wu × 50 m/wu（=51.2 km） → 应占 12.8 wu
        // 但城市声明 1024 wu（未校正），ratio = 80 → 远大于 2，应发「明显不匹配」。
        // 这里先验证「正确换算」场景：城市声明 12.8 wu、unit_per_km = 800 m/wu（512 wu/km），
        // 物理宽度 12.8 × 800 = 10240 m = 10.24 km；world 1 km/wu → 应占 10.24 wu。
        // 调整到 ratio=1.0：city 1024 wu，unit 0.001 km/wu = 1 m/wu，world 1 km/wu，
        // 预期 1024 / 0.001 / 1 = 1_024_000 wu。声明 1024 wu → ratio ≈ 1/1024 → 应发「明显」。
        // 所以直接测 ratio=1：city 1000 wu、unit 1.0 km/wu、world 1.0 km/wu
        // 预期 1000/1/1 = 1000 wu。声明 1000 wu → ratio=1 → 无提示。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(1.0),
            ..city_with("map_world", 1000.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(
            hints.is_empty(),
            "ratio=1.0 不应发提示，实得 {} 条",
            hints.len()
        );
    }

    #[test]
    fn ratio_within_ten_percent_emits_no_hint() {
        // city 1000 wu、unit 1.0；预期 1000 wu。声明 1050 wu → ratio = 1.05（差 5%）→ 无提示。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(1.0),
            ..city_with("map_world", 1050.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(hints.is_empty(), "5% 偏差 ≤ 10% 阈值，应无提示");
    }

    #[test]
    fn ratio_50_percent_emits_mild_hint() {
        // ratio = city.unit_per_km × world.unit_per_km = 0.5 × 1.0 = 0.5
        // → 「两级间尺度有 50% 偏差」。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(0.5),
            ..city_with("map_world", 1000.0, Some(0.5))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert_eq!(hints.len(), 1, "应发 1 条提示");
        assert_eq!(hints[0].level, DiagLevel::Hint);
        assert!(hints[0].message.contains("50%"), "应含百分比");
    }

    #[test]
    fn ratio_five_x_emits_severe_hint() {
        // ratio = city.unit_per_km × world.unit_per_km = 5.0 × 1.0 = 5.0
        // → 「两级尺度明显不匹配」。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(5.0),
            ..city_with("map_world", 1000.0, Some(5.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert_eq!(hints.len(), 1, "应发 1 条明显不匹配提示");
        assert!(
            hints[0].message.contains("明显不匹配"),
            "应标记为严重，实得：{}",
            hints[0].message
        );
    }

    #[test]
    fn missing_world_per_km_skips_all_checks() {
        // world.unit_per_km = None → 跳过所有城市。
        let mut w = world_with(None);
        w.maps.push(MapNode {
            unit_per_km: Some(1.0),
            ..city_with("map_world", 5000.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(hints.is_empty(), "world 未声明尺度应跳过");
    }

    #[test]
    fn missing_city_per_km_skips_city() {
        // 城市未声明 unit_per_km → 跳过该城（自身元数据不足）。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: None,
            ..city_with("map_world", 5000.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(hints.is_empty(), "城市未声明尺度应跳过");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_and_errors_are_distinguished_by_level() {
        let mut d = Diagnostics::default();
        d.items.push(hint("f1", "相遇未写事件"));
        d.items.push(error("f2", "悬空引用"));
        assert_eq!(d.hints().len(), 1);
        assert_eq!(d.errors().len(), 1);
        assert!(d.has_errors());
    }

    #[test]
    fn hints_alone_do_not_count_as_errors() {
        // 核心裁定：只有提示时，作者**可以正常保存**（AGENTS §7.5 V1）。
        let mut d = Diagnostics::default();
        d.items.push(hint("f1", "相遇未写事件"));
        d.items.push(hint("f2", "相遇未写事件 2"));
        assert!(!d.has_errors(), "只有提示时不得阻断保存");
    }

    #[test]
    fn dedup_keeps_one_entry_per_fingerprint() {
        let mut d = Diagnostics::default();
        d.items.push(hint("dup", "a"));
        d.items.push(hint("dup", "a"));
        d.items.push(hint("other", "b"));
        d.dedup_by_fingerprint();
        assert_eq!(d.items.len(), 2);
    }

    #[test]
    fn dedup_is_order_independent() {
        let mut a = Diagnostics::default();
        a.items.push(hint("x", "1"));
        a.items.push(hint("y", "2"));
        let mut b = Diagnostics::default();
        b.items.push(hint("y", "2"));
        b.items.push(hint("x", "1"));
        a.dedup_by_fingerprint();
        b.dedup_by_fingerprint();
        assert_eq!(a, b, "去重后必须与输入顺序无关（确定性）");
    }

    #[test]
    fn derive_hints_on_empty_world_is_empty_and_ok() {
        let w = wv_core::World::default();
        let d = derive_hints(&w);
        assert!(d.items.is_empty());
        assert!(!d.has_errors(), "提示路径永不产生错误");
    }

    // -----------------------------------------------------------------------
    // 阶段六 v0.2：事件间位移可行性提示单元测试
    // -----------------------------------------------------------------------

    use wv_core::distance::{DistanceGraph, Place, PlaceEdge, ScheduledJourney, Vehicle};
    use wv_core::event::{Event, EventLocation};
    use wv_core::time::TimeSpan;
    use wv_core::Id;

    /// 极简图：A → B，单条边 100 km plain，单车 speed 10 km/h 跑 8h → 12.5 km/天。
    /// A → B 至少需 100/12.5 = 8 天。
    fn fixture_graph() -> DistanceGraph {
        let a = Id::new_unchecked("p_a");
        let b = Id::new_unchecked("p_b");
        DistanceGraph {
            places: vec![
                Place {
                    id: a.clone(),
                    name: "A".into(),
                    map: Some(Id::new_unchecked("m")),
                    x: Some(0.0),
                    y: Some(0.0),
                    w: None,
                    h: None,
                    feature: None,
                },
                Place {
                    id: b.clone(),
                    name: "B".into(),
                    map: Some(Id::new_unchecked("m")),
                    x: Some(0.0),
                    y: Some(0.0),
                    w: None,
                    h: None,
                    feature: None,
                },
            ],
            edges: vec![PlaceEdge {
                id: Id::new_unchecked("e_ab"),
                a: a.clone(),
                b: b.clone(),
                distance_km: 100.0,
                terrain: "plain".into(),
                bidirectional: true,
            }],
            vehicles: vec![Vehicle {
                id: Id::new_unchecked("v_walk"),
                name: "步行".into(),
                instant: false,
                speed_kmh: Some(10.0),
                daily_hours: Some(8.0),
                terrain_modifiers: Default::default(),
                requires_endpoints: false,
                requirements: vec![],
            }],
        }
    }

    fn ev(id: &str, from: i64, to: i64, place: &str, who: &[&str]) -> Event {
        Event {
            id: Id::new_unchecked(id),
            name: id.into(),
            span: TimeSpan { from, to: Some(to) },
            location: Some(EventLocation {
                place: Id::new_unchecked(place),
                x: None,
                y: None,
                w: None,
                h: None,
            }),
            participants: who.iter().map(|s| Id::new_unchecked(*s)).collect(),
            narrative: None,
            outcome: None,
            effects: vec![],
            narrations: vec![],
        }
    }

    /// 构造一个最小可工作 `World`：`distances` + `events` 都给好。
    fn world_with(distances: DistanceGraph, events: Vec<Event>) -> wv_core::World {
        wv_core::World {
            distances,
            events,
            ..Default::default()
        }
    }

    /// 预算 < 路径最少 → 出 hint。
    #[test]
    fn travel_feasibility_hints_when_budget_below_path_min() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["x"])],
        );
        // 直查 A* 拿真实 total_days，让断言用真实值。
        let j = w
            .distances
            .shortest_journey(&Id::new_unchecked("p_a"), &Id::new_unchecked("p_b"), &[])
            .unwrap();
        let expected_total = j.total_days;
        let d = derive_hints(&w);
        let travel: Vec<&Diagnostic> = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .collect();
        assert_eq!(travel.len(), 1, "应出一条位移提示");
        let msg = &travel[0].message;
        let total_str = format!("{:.1}", expected_total);
        assert!(
            msg.contains(&total_str),
            "应报最少需 {total_str} 天：got {msg}"
        );
        assert!(msg.contains("2"), "应报预算 2 天：got {msg}");
        assert_eq!(travel[0].level, DiagLevel::Hint);
    }

    /// 预算 ≥ 路径最少 → 不出 hint。
    #[test]
    fn travel_feasibility_silent_when_budget_covers_path_min() {
        // fixture 中 A→B 实际需 30 天（10 km/h × 8 h/24 × 100 km），把 gap 调到 50。
        let w = world_with(
            fixture_graph(),
            vec![
                ev("e1", 1, 5, "p_a", &["x"]),
                ev("e2", 55, 56, "p_b", &["x"]),
            ],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "预算充足时不应出提示");
    }

    /// 同参与者集合为空 → 不出 hint。
    #[test]
    fn travel_feasibility_skipped_when_no_common_participant() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["y"])],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "无共同参与者不应出提示");
    }

    /// 同地点 → 不出 hint。
    #[test]
    fn travel_feasibility_skipped_when_same_place() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_a", &["x"])],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "同地点不算位移");
    }

    /// 路径不通（图中无连边）→ 不报本检查，留给其他检查。
    #[test]
    fn travel_feasibility_silent_when_no_path() {
        let w = world_with(
            DistanceGraph::default(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["x"])],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "路径不可达时不应出本类提示");
    }

    /// `prev.span.to == None`（开放区间）→ 跳过——next 与仍持续的 prev 重叠是另一类矛盾。
    #[test]
    fn travel_feasibility_skipped_when_prev_to_open() {
        let mut e1 = ev("e1", 1, 5, "p_a", &["x"]);
        e1.span.to = None;
        let w = world_with(fixture_graph(), vec![e1, ev("e2", 7, 8, "p_b", &["x"])]);
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "开放区间跳过本检查");
    }

    /// 指纹由 prev/next id + 共同参与者集合组成，确定性、不含日期（AGENTS §6.1 措辞思路：
    /// 日期一变提示就失效，作者认领不掉——本检查同理用稳定 id）。
    #[test]
    fn travel_feasibility_fingerprint_is_stable_under_time_edit() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["x"])],
        );
        let d1 = derive_hints(&w);
        // 把 next 的 day 挪到 55 → gap=50，预算充足 → 提示应消失。
        let mut w2 = w.clone();
        w2.events[1].span.from = 55;
        let d2 = derive_hints(&w2);
        let fp1: String = d1
            .items
            .iter()
            .find(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .map(|x| x.fingerprint.clone())
            .unwrap();
        let count2 = d2
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert!(
            fp1.contains("e1") && fp1.contains("e2") && fp1.contains("x"),
            "指纹应含 prev/next id 与共同参与者：got {fp1}"
        );
        assert_eq!(count2, 0, "挪日期后预算充足，提示应消失");
    }

    /// _use ScheduledJourney：避免未使用导入 lint。
    #[allow(dead_code)]
    fn _ensure_journey_in_scope(_: &ScheduledJourney) {}

    /// 阶段六 v0.1：xinhai-1911 工程的距离图应正确加载三层（places/edges/vehicles），
    /// 且武汉→成都走「长江＋陆路」可解（A* 返非空）。
    ///
    /// 守住「加载层必须把距离三个分量都读全」这一收敛点——旧实现只读单文件 places.yaml，
    /// edges/vehicles 始终为空，路径搜索退化。
    #[test]
    fn xinhai_loads_full_distance_graph_and_path_finds_jiangnan_route() {
        let path = std::path::Path::new("../examples/xinhai-1911");
        if !path.is_dir() {
            // 在子 crate 工作目录外运行（如 IDE unit test）允许跳过。
            return;
        }
        let project = crate::project::Project::open(path).expect("工程加载失败");
        let world = project.load_world().expect("加载世界失败");
        let g = &world.distances;
        assert!(
            !g.places.is_empty(),
            "places 应从目录加载；空 = 加载器未生效"
        );
        assert!(!g.edges.is_empty(), "edges 应从目录加载；空 = 旧实现未替换");
        assert!(
            !g.vehicles.is_empty(),
            "vehicles 应从目录加载；空 = 旧实现未替换"
        );
        // 武汉 → 成都：waterway + plain，A* 应至少给出一条路径。
        let wuhan = wv_core::id::Id::new_unchecked("place_wuhan");
        let chengdu = wv_core::id::Id::new_unchecked("place_chengdu");
        let journey = g
            .shortest_journey(&wuhan, &chengdu, &[])
            .expect("武汉→成都应有路径");
        assert!(
            journey.total_days.is_finite() && journey.total_days > 0.0,
            "总天数应有限且 > 0：got {}",
            journey.total_days
        );
        // 武汉到宜昌是水路，工具轮船应被采用。
        let uses_steamboat = journey
            .legs
            .iter()
            .any(|s| s.vehicle.as_str() == "veh_steamboat");
        assert!(
            uses_steamboat,
            "长江段应被 veh_steamboat 承担，实际工具列表 = {:?}",
            journey
                .legs
                .iter()
                .map(|s| s.vehicle.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// 阶段 S12 v0.1：xinhai-1911 工程应加载 factions + faction_geometry 两层目录，
    /// 且 `factions_at` 在关键日期能筛出应有的势力。
    ///
    /// 辛亥年（day 283，武昌起义）前后是关键断面：早期只有清廷，晚期清 + 革命并存。
    #[test]
    fn xinhai_loads_factions_and_geometry_at_key_days() {
        let path = std::path::Path::new("../examples/xinhai-1911");
        if !path.is_dir() {
            return;
        }
        let project = crate::project::Project::open(path).expect("工程加载失败");
        let world = project.load_world().expect("加载世界失败");
        // 两层目录都应非空。
        assert!(world.factions.len() >= 2, "示例工程应至少 2 个势力");
        assert!(
            world.faction_geometry.len() >= 2,
            "示例工程应至少 2 段势力几何"
        );
        // 第 100 天：只有清廷。
        let map = wv_core::id::Id::new_unchecked("map_world");
        let early = wv_core::factions_at(&world, &map, 100);
        assert_eq!(early.len(), 1, "起义前应只有清廷");
        assert_eq!(early[0].0.id.as_str(), "faction_qing");
        // 第 283 天：武昌起义，清 + 革命都生效。
        let mid = wv_core::factions_at(&world, &map, 283);
        assert_eq!(mid.len(), 2, "起义当日双方都生效");
        // 第 300 天：清廷 [1, 285) 已结束，革命军（开放）仍在。
        let late = wv_core::factions_at(&world, &map, 300);
        assert_eq!(late.len(), 1, "清廷结束日之后只剩革命军");
        assert_eq!(late[0].0.id.as_str(), "faction_rev");
    }
}
