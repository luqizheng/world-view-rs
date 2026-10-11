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
    d.dedup_by_fingerprint();
    d
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
}
