//! 标签碰撞检测（ADR-0017 §2.6）。
//!
//! v0.1 实现：屏幕像素 AABB 贪心算法。
//! 输入：标签列表（位置 + 文案 + 字号 + 优先级）；
//! 输出：过滤后的标签列表（按优先级保留，无重叠）。
//!
//! ## 算法
//!
//! 1. 按优先级排序（**呈现级别 → 文案长度**，见 [`priority_for`]）。
//! 2. 贪心遍历：逐个检查是否与已选标签的 AABB 重叠。
//! 3. 不重叠 → 保留；重叠 → 跳过。
//!
//! ## 字号逐条给（ADR-0065）
//!
//! 地名按呈现级别分档（国家级 16px → 微点 12px），所以字号是**候选自身
//! 的属性**，不是过滤函数的入参；过滤函数只吃「世界单位 / 屏幕像素」的
//! 换算比，把每条候选的屏幕字号折成视图空间的盒宽高。
//!
//! ## AABB 估算
//!
//! - 宽度 = 字符数 × 字号 × 0.6（拉丁）/ 字符数 × 字号（CJK）
//! - 高度 = 字号 × 1.2
//! - 原点 = 标签位置（左下角）
//!
//! 这是工程口径的估算，不是排版引擎的精确值——v0.1 够用。

use bevy::math::Vec2;

/// 标签候选（碰撞检测前）。
#[derive(Debug, Clone)]
pub struct LabelCandidate {
    /// 标签位置（视图空间，左下角）。
    pub pos: Vec2,
    /// 标签文案。
    pub name: String,
    /// 该标签的**屏幕字号**（像素）。同屏标签不共用一档字号（ADR-0065）。
    pub font_size_px: f32,
    /// 优先级（数值越小越优先保留）。
    pub priority: u32,
    /// 命中后选中的实体（ADR-0017 §2.7）。`None` = 不可拾取（无稳定 id，
    /// 如街名）；碰撞过滤把 `None` 与有身份者一视同仁地裁掉——**看不见的
    /// 标签也不可点**。
    pub target: Option<wv_core::SelectedEntity>,
}

/// 由呈现级别与文案长度合成碰撞优先级（数值越小越优先）。
///
/// **先级别、后长度**：`rank` 越高越先保留（国家级地名不会被一串长 POI 名
/// 挤掉）；同级别里长名优先（注释口径与历史一致，且长名更难被看清）。
/// 名称长度按**字符**计——旧实现用 `len()`（字节），中文被算成 3 倍，
/// 与拉丁名不可比。
pub fn priority_for(rank: u8, name: &str) -> u32 {
    let rank_weight = u32::from(3u8.saturating_sub(rank.min(3)));
    let len_penalty = 255u32.saturating_sub(name.chars().count().min(255) as u32);
    rank_weight * 1_000 + len_penalty
}

/// AABB（轴对齐包围盒）。
#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb {
    /// 从位置 + 宽高构造。
    pub fn from_pos_size(pos: Vec2, width: f32, height: f32) -> Self {
        Self {
            min: pos,
            max: Vec2::new(pos.x + width, pos.y + height),
        }
    }

    /// 是否与另一 AABB 重叠。
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
    }
}

/// 估算标签宽度（屏幕像素）。
///
/// - CJK 字符：宽度 = 字号
/// - 拉丁字符：宽度 = 字号 × 0.6
pub fn estimate_label_width(name: &str, font_size: f32) -> f32 {
    let mut width = 0.0;
    for c in name.chars() {
        if c.is_ascii() {
            width += font_size * 0.6;
        } else {
            width += font_size;
        }
    }
    width
}

/// 估算标签高度（屏幕像素）。
pub fn estimate_label_height(font_size: f32) -> f32 {
    font_size * 1.2
}

/// AABB 碰撞检测（v0.1 贪心算法）。
///
/// 输入：标签候选列表（无序）+ `unit_scale`（世界单位 / 屏幕像素）。
/// 输出：过滤后的标签列表（按优先级保留，无重叠）。
///
/// `unit_scale` 即 `ViewState::scale` 的生效值（含 `.max(0.05)` 下限）——
/// 把每条候选自己的屏幕字号折成视图空间的盒宽高。候选的 `pos` 必须已在
/// 视图空间（世界单位）。
///
/// ## 复杂度
///
/// O(n²)——n = 候选数。v0.1 n ≤ 200，够用。
pub fn filter_collisions(
    candidates: &mut Vec<LabelCandidate>,
    unit_scale: f32,
) -> Vec<LabelCandidate> {
    // 按优先级排序（数值越小越优先）。
    candidates.sort_by_key(|c| c.priority);

    let mut selected: Vec<LabelCandidate> = Vec::new();
    let mut selected_aabbs: Vec<Aabb> = Vec::new();

    for candidate in candidates.drain(..) {
        let font_size = candidate.font_size_px * unit_scale;
        let width = estimate_label_width(&candidate.name, font_size);
        let height = estimate_label_height(font_size);
        let aabb = Aabb::from_pos_size(candidate.pos, width, height);

        // 检查是否与已选标签重叠。
        let overlaps = selected_aabbs
            .iter()
            .any(|existing| existing.overlaps(&aabb));

        if !overlaps {
            selected.push(candidate);
            selected_aabbs.push(aabb);
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_overlaps_detects_intersection() {
        let a = Aabb::from_pos_size(Vec2::new(0.0, 0.0), 10.0, 10.0);
        let b = Aabb::from_pos_size(Vec2::new(5.0, 5.0), 10.0, 10.0);
        assert!(a.overlaps(&b));
    }

    #[test]
    fn aabb_no_overlap_when_separated() {
        let a = Aabb::from_pos_size(Vec2::new(0.0, 0.0), 10.0, 10.0);
        let b = Aabb::from_pos_size(Vec2::new(20.0, 20.0), 10.0, 10.0);
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn aabb_no_overlap_when_adjacent() {
        let a = Aabb::from_pos_size(Vec2::new(0.0, 0.0), 10.0, 10.0);
        let b = Aabb::from_pos_size(Vec2::new(10.0, 0.0), 10.0, 10.0);
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn estimate_label_width_cjk_is_wider() {
        let latin = estimate_label_width("abc", 12.0);
        let cjk = estimate_label_width("你好", 12.0);
        assert!(cjk > latin, "CJK 应比拉丁宽");
    }

    #[test]
    fn filter_collisions_removes_overlapping_labels() {
        let mut candidates = vec![
            LabelCandidate {
                pos: Vec2::new(0.0, 0.0),
                name: "A".into(),
                font_size_px: 12.0,
                priority: 1,
                target: None,
            },
            LabelCandidate {
                pos: Vec2::new(1.0, 1.0), // 与 A 重叠
                name: "B".into(),
                font_size_px: 12.0,
                priority: 2,
                target: None,
            },
            LabelCandidate {
                pos: Vec2::new(100.0, 100.0), // 与 A 不重叠
                name: "C".into(),
                font_size_px: 12.0,
                priority: 3,
                target: None,
            },
        ];
        let result = filter_collisions(&mut candidates, 1.0);
        assert_eq!(result.len(), 2, "应保留 A 和 C");
        assert_eq!(result[0].name, "A", "优先级最高的 A 应保留");
        assert_eq!(result[1].name, "C", "不重叠的 C 应保留");
    }

    #[test]
    fn filter_collisions_respects_priority() {
        let mut candidates = vec![
            LabelCandidate {
                pos: Vec2::new(0.0, 0.0),
                name: "low".into(),
                font_size_px: 12.0,
                priority: 10,
                target: None,
            },
            LabelCandidate {
                pos: Vec2::new(1.0, 1.0), // 与 low 重叠
                name: "high".into(),
                font_size_px: 12.0,
                priority: 1,
                target: None,
            },
        ];
        let result = filter_collisions(&mut candidates, 1.0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "high", "高优先级应保留");
    }

    /// 字号是候选自身属性：同样的间距下，大字号标签先占位、小字号被挤掉。
    #[test]
    fn filter_collisions_uses_per_candidate_font_size() {
        let mut candidates = vec![
            LabelCandidate {
                pos: Vec2::new(0.0, 0.0),
                name: "大城市".into(),
                font_size_px: 16.0,
                priority: 0,
                target: None,
            },
            LabelCandidate {
                // 与 16px 的盒子重叠，但若按 10px 估算则不重叠。
                pos: Vec2::new(40.0, 0.0),
                name: "微点".into(),
                font_size_px: 12.0,
                priority: 1,
                target: None,
            },
        ];
        let result = filter_collisions(&mut candidates, 1.0);
        assert_eq!(result.len(), 1, "大字号标签应占住位置");
        assert_eq!(result[0].name, "大城市");
    }

    /// 级别高的先保留：国家级地名不会被一串低级别长名挤掉。
    #[test]
    fn priority_prefers_higher_rank_before_name_length() {
        let capital = priority_for(3, "京城");
        let long_poi = priority_for(0, "非常非常长的低级别地点名称");
        assert!(
            capital < long_poi,
            "国家级 {} 应优先于低级别长名 {}",
            capital,
            long_poi
        );
        // 同级别：长名优先（penalty 更小）。
        let short = priority_for(2, "镇");
        let long = priority_for(2, "青石桥下游渡口");
        assert!(long < short, "同级长名 {} 应优先于短名 {}", long, short);
    }
}
