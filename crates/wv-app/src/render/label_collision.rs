//! 标签碰撞检测（ADR-0017 §2.6）。
//!
//! v0.1 实现：屏幕像素 AABB 贪心算法。
//! 输入：标签列表（位置 + 文案 + 优先级）；
//! 输出：过滤后的标签列表（按优先级保留，无重叠）。
//!
//! ## 算法
//!
//! 1. 按优先级排序（PlaceKind 权重 → 文本长度 → 词典序）。
//! 2. 贪心遍历：逐个检查是否与已选标签的 AABB 重叠。
//! 3. 不重叠 → 保留；重叠 → 跳过。
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
    /// 优先级（数值越小越优先保留）。
    pub priority: u32,
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
/// 输入：标签候选列表（无序）。
/// 输出：过滤后的标签列表（按优先级保留，无重叠）。
///
/// ## 复杂度
///
/// O(n²)——n = 候选数。v0.1 n ≤ 200，够用。
pub fn filter_collisions(
    candidates: &mut Vec<LabelCandidate>,
    font_size: f32,
) -> Vec<LabelCandidate> {
    // 按优先级排序（数值越小越优先）。
    candidates.sort_by_key(|c| c.priority);

    let mut selected: Vec<LabelCandidate> = Vec::new();
    let mut selected_aabbs: Vec<Aabb> = Vec::new();

    for candidate in candidates.drain(..) {
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
                priority: 1,
            },
            LabelCandidate {
                pos: Vec2::new(1.0, 1.0), // 与 A 重叠
                name: "B".into(),
                priority: 2,
            },
            LabelCandidate {
                pos: Vec2::new(100.0, 100.0), // 与 A 不重叠
                name: "C".into(),
                priority: 3,
            },
        ];
        let result = filter_collisions(&mut candidates, 12.0);
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
                priority: 10,
            },
            LabelCandidate {
                pos: Vec2::new(1.0, 1.0), // 与 low 重叠
                name: "high".into(),
                priority: 1,
            },
        ];
        let result = filter_collisions(&mut candidates, 12.0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "high", "高优先级应保留");
    }
}
