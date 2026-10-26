//! 标签碰撞与摆位（ADR-0017 §2.6 → ADR-0073 候选位摆位）。
//!
//! v0.1（ADR-0017）：屏幕像素 AABB 贪心，但**六类标签系统各自过滤、
//! 互不感知**，且碰撞不过直接丢弃——「放不下就不画」，这是标签排放
//! 混乱的根因（ADR-0073 处境）。
//!
//! ADR-0073 改为**候选位摆位**（参考知乎《奇幻地图程序生成（五）：
//! 名称标签的摆放》，2026-10-26）：
//!
//! 1. **统一占位池**：全帧一个 [`LabelOccupancy`]，六类标签按链序依次
//!    落位，互相避让（文章「碰撞检测统一管理」）。
//! 2. **候选位搜索**：每条候选带一组**按偏好排序的候选偏移**，首选位
//!    放不下就试下一位，全失败才丢弃（文章「候选位择优」）——点标签
//!    8 方位、面标签区域内挪移。
//! 3. **沿旋转标签**：街名等带 `angle` 的候选，碰撞盒取旋转矩形的
//!    保守 AABB（文章「沿河/沿路旋转标注」）。
//! 4. **密度预算**：按 LOD 档 × 标签通路给每帧配额（`MAP_GENERATION_
//!    AND_POI_TYPOGRAPHY_PLAN` Q3 的密度收敛方向），替代散落六处的
//!    `truncate(200)`。
//! 5. **碰撞盒 = 渲染盒**：盒以**最终渲染位置为中心**构造（旧实现把
//!    锚点当左下角、渲染时又整体上移，盒与字错位约半个盒高）。
//!
//! ## 算法（仍是贪心，O(n²)）
//!
//! 按优先级排序（[`priority_for`]）→ 逐条试候选位：区域内约束（若有）
//! → 与占位池重叠检查 → 首个可行位落位并登记占位 → 全部失败丢弃。
//! n 被视口裁剪 + 级别硬门压到屏幕量级，O(n²) 够用（v0.1 口径不变）。

use bevy::math::Vec2;
use bevy::prelude::{Res, ResMut, Resource, SystemSet};

use crate::state::{lod_tier, LodTier, ViewState};

/// 钻取标签链集合（ADR-0073）：drill.rs `register` 的标签系统 chain 挂在
/// 这个集合下。
///
/// 街名系统与钻取标签链分属两条插件的 chain，跨链无隐式顺序；统一占位池
/// 要求「地名 → … → 海名 → 街名」的落位次序，街名侧用 `.after(Self)`
/// 显式接线。游戏路径不装钻取链时本集合为空，约束自然退化为无序。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DrillLabelChain;

/// 标签通路（ADR-0073）：密度预算的记账单位。
///
/// 与渲染 `PassId` 解耦——这里是**摆位**口径（谁占谁的地），一个
/// `PassId` 可对应多条通路，反之 Labels 渲染组下的六类标签各占一档预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPass {
    /// 地名（POI / 城市）。
    Place,
    /// 街名（沿线旋转）。
    Street,
    /// 势力名（面标签）。
    Faction,
    /// 事件名。
    Event,
    /// 区域名（面标签）。
    Region,
    /// 海名。
    Sea,
}

impl LabelPass {
    /// 全部通路（数组序 = [`LabelOccupancy::counts`] / `budgets` 的下标）。
    pub const ALL: [LabelPass; 6] = [
        LabelPass::Place,
        LabelPass::Street,
        LabelPass::Faction,
        LabelPass::Event,
        LabelPass::Region,
        LabelPass::Sea,
    ];

    /// 记账下标。
    pub fn index(self) -> usize {
        match self {
            LabelPass::Place => 0,
            LabelPass::Street => 1,
            LabelPass::Faction => 2,
            LabelPass::Event => 3,
            LabelPass::Region => 4,
            LabelPass::Sea => 5,
        }
    }
}

/// 点标签的候选方位表（**屏幕像素倍率**，乘以各自字号）。
///
/// 首选「正上方 0.9×字号」= 2026-10-14 以来的现行口径——首选位不变，
/// 未放下的标签才有机会落到其余方位（视觉零回归起点）。倍率取值让
/// 文字盒不压锚点图标：下方比上方多让 0.45，斜方位随盒角收。
const POINT_OFFSET_TABLE: [(f32, f32); 8] = [
    (0.0, 0.9),     // 上（现行首选）
    (0.0, -1.35),   // 下
    (1.05, 0.0),    // 右
    (-1.05, 0.0),   // 左
    (0.85, 0.85),   // 右上
    (-0.85, 0.85),  // 左上
    (0.85, -1.25),  // 右下
    (-0.85, -1.25), // 左下
];

/// 点标签的候选偏移（视图空间，偏好序）。
///
/// 屏幕方位表 × 各自字号 → 经相机旋转映回世界方向（与既有「上移避让」
/// 同一条变换链）。地图旋转后「上方」仍是屏幕上方。
pub fn point_label_offsets(font_size_px: f32, camera_rotation: bevy::prelude::Quat) -> Vec<Vec2> {
    POINT_OFFSET_TABLE
        .iter()
        .map(|(x, y)| {
            crate::camera::screen_vec_to_world(
                Vec2::new(x * font_size_px, y * font_size_px),
                camera_rotation,
            )
        })
        .collect()
}

/// 面标签（势力 / 区域）的候选偏移：以质心为基点、按**自身盒宽高**取步长。
///
/// 面标签必须待在自己的区域内（文章「标签放在自己区域里」），步长按盒
/// 尺寸取，挪动一格 ≈ 半个盒——小区域多试几步仍出不去就放弃（区域装不
/// 下这个名，画出去比不画更糟）。
pub fn area_label_offsets(width: f32, height: f32) -> Vec<Vec2> {
    let (dx, dy) = (width * 0.6, height * 1.1);
    vec![
        Vec2::ZERO,
        Vec2::new(dx, dy),
        Vec2::new(-dx, dy),
        Vec2::new(dx, -dy),
        Vec2::new(-dx, -dy),
        Vec2::new(dx * 2.0, 0.0),
        Vec2::new(-dx * 2.0, 0.0),
        Vec2::new(0.0, dy * 2.0),
        Vec2::new(0.0, -dy * 2.0),
    ]
}

/// 标签候选（碰撞检测前）。
#[derive(Debug, Clone)]
pub struct LabelCandidate {
    /// 锚点（视图空间，盒中心基准）。
    pub pos: Vec2,
    /// 标签文案。
    pub name: String,
    /// 该标签的**屏幕字号**（像素）。同屏标签不共用一档字号（ADR-0065）。
    pub font_size_px: f32,
    /// 优先级（数值越小越优先保留）。
    pub priority: u32,
    /// 所属通路（密度预算记账 + 调试归属）。
    pub pass: LabelPass,
    /// 文字屏幕旋转角（弧度）。`None` = 水平点标签；`Some(φ)` = 沿线
    /// 标签（街名），碰撞盒取旋转保守 AABB。
    pub angle: Option<f32>,
    /// 候选偏移（视图空间，偏好序；空 = 只试原位）。
    pub offsets: Vec<Vec2>,
    /// 区域内约束（视图空间扁平环对；`[0]` = 外环，其余 = 洞）。
    /// 非空时候选位盒中心必须在外环内且不在任何洞里（ADR-0073：
    /// 面标签不得画出自己区域）。
    pub inside_rings: Vec<Vec<f32>>,
    /// 命中后选中的实体（ADR-0017 §2.7）。`None` = 不可拾取（无稳定 id，
    /// 如街名）；碰撞过滤把 `None` 与有身份者一视同仁地裁掉——**看不见的
    /// 标签也不可点**。
    pub target: Option<wv_core::SelectedEntity>,
}

impl LabelCandidate {
    /// 最小构造（水平、原位、无区域约束）——点标签的常用形。
    pub fn new(
        pass: LabelPass,
        pos: Vec2,
        name: impl Into<String>,
        font_size_px: f32,
        priority: u32,
        target: Option<wv_core::SelectedEntity>,
    ) -> Self {
        Self {
            pos,
            name: name.into(),
            font_size_px,
            priority,
            pass,
            angle: None,
            offsets: Vec::new(),
            inside_rings: Vec::new(),
            target,
        }
    }
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
    /// 从位置 + 宽高构造（`pos` 为左下角）。
    pub fn from_pos_size(pos: Vec2, width: f32, height: f32) -> Self {
        Self {
            min: pos,
            max: Vec2::new(pos.x + width, pos.y + height),
        }
    }

    /// 从中心 + 宽高构造（ADR-0073 口径：Text2d 以中心为原点，碰撞盒
    /// 与渲染盒必须同锚）。
    pub fn from_center_size(center: Vec2, width: f32, height: f32) -> Self {
        let half = Vec2::new(width * 0.5, height * 0.5);
        Self {
            min: center - half,
            max: center + half,
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

/// 旋转 θ 后矩形 (w × h) 的**保守 AABB** 外扩尺寸。
///
/// 旋转标签（街名）的碰撞盒：精确的旋转矩形相交代价高，取外接轴对齐
/// 盒——略偏保守（斜街多占一点位），贪心摆位下这是可接受的近似。
pub fn rotated_extents(width: f32, height: f32, angle: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (
        width * c.abs() + height * s.abs(),
        width * s.abs() + height * c.abs(),
    )
}

/// 直立化角度（文章「标签翻转」规则）：沿线标签的角度归一到
/// `(-π/2, π/2]`，文字**永远头朝上、从左往右读**。
///
/// 段方向朝左（|φ| > 90°）时翻转 180°——跟随一条朝西的街不应把字倒着写。
pub fn upright_angle(angle: f32) -> f32 {
    let mut a = angle;
    while a > std::f32::consts::FRAC_PI_2 {
        a -= std::f32::consts::PI;
    }
    while a <= -std::f32::consts::FRAC_PI_2 {
        a += std::f32::consts::PI;
    }
    a
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

/// 盒中心是否满足区域约束：外环内 ∧ 不落在任何洞里。
///
/// 用盒中心近似「整个标签在区域内」（文章口径的工程近似）——多边形
/// 贴边检查留给未来真正需要时再收紧。
fn satisfies_inside(rings: &[Vec<f32>], center: Vec2) -> bool {
    match rings.split_first() {
        None => true,
        Some((outer, holes)) => {
            if !wv_core::geometry::point_in_ring(outer, center.x, center.y) {
                return false;
            }
            !holes
                .iter()
                .any(|h| wv_core::geometry::point_in_ring(h, center.x, center.y))
        }
    }
}

/// 全帧共享的标签占位池（ADR-0073）。
///
/// 六类标签系统按链序（place → faction → event → region → sea → street）
/// 依次消费：每类先查自己的预算余额，落位成功则登记 AABB。链首的
/// [`reset_label_occupancy`] 每帧清场并按 LOD 档重设预算。
#[derive(Resource)]
pub struct LabelOccupancy {
    /// 本帧已落位标签的盒（视图空间）。
    pub boxes: Vec<Aabb>,
    /// 各通路本帧已落位数。
    pub counts: [usize; 6],
    /// 各通路本帧预算（reset 时按 LOD 档重设）。
    pub budgets: [usize; 6],
}

impl Default for LabelOccupancy {
    fn default() -> Self {
        // 资源初建的兜底预算 = Edit 档；首帧 reset 前若真有系统跑（链序
        // 保证不会），按最宽档放行，不会静默吞光标签。
        Self {
            boxes: Vec::new(),
            counts: [0; 6],
            budgets: budgets_for_tier(LodTier::Edit),
        }
    }
}

impl LabelOccupancy {
    /// 清场并重设预算。
    pub fn reset(&mut self, budgets: [usize; 6]) {
        self.boxes.clear();
        self.counts = [0; 6];
        self.budgets = budgets;
    }
}

/// 每帧链首清场（ADR-0073）：预算按当前 LOD 档重设。
pub fn reset_label_occupancy(mut occupancy: ResMut<LabelOccupancy>, view: Res<ViewState>) {
    occupancy.reset(budgets_for_tier(lod_tier(view.scale)));
}

/// 密度预算表（ADR-0073）：`[Place, Street, Faction, Event, Region, Sea]`。
///
/// 旧口径是每系统各自 `truncate(200)`（同屏理论上限 1200，实测密度超
/// 行业基准 13–16 倍，见 TYPOGRAPHY PLAN Q3）；新口径按「档越高越密」
/// 给总量约 44 / 68 / 116，其中街名在 Nation 档只留小配额（远观街名
/// 本就该稀，Google 地图同款）、势力留 8（多领土势力一图多块，预算过
/// 紧会掉块名）。数值是首版手工标定，待实机验收后调。
pub fn budgets_for_tier(tier: LodTier) -> [usize; 6] {
    match tier {
        LodTier::Nation => [12, 8, 8, 6, 6, 4],
        LodTier::Region => [24, 16, 8, 8, 8, 4],
        LodTier::Edit => [48, 40, 8, 8, 8, 4],
    }
}

/// 统一候选位摆位（ADR-0073）：在共享占位池上贪心落位。
///
/// 输入：候选列表（无序，`pos` = 锚点）+ `unit_scale`（世界单位 / 屏幕像素，
/// 即 `ViewState::scale` 生效值）+ 全帧占位池。
/// 输出：落位成功的候选，**`pos` 已更新为最终渲染位置**（锚点 + 选中
/// 的候选偏移）——调用方不再做「统一上移」，直接按 `pos` 摆字。
///
/// 每条候选依次试 `offsets`：区域约束 → 占位重叠 → 首个可行位落位；
/// 预算耗尽或全部位失败 → 丢弃（看不见的标签也不可点，与 v0.1 一致）。
pub fn filter_collisions(
    candidates: &mut Vec<LabelCandidate>,
    unit_scale: f32,
    occupancy: &mut LabelOccupancy,
) -> Vec<LabelCandidate> {
    // 按优先级排序（数值越小越优先）。
    candidates.sort_by_key(|c| c.priority);

    let mut selected: Vec<LabelCandidate> = Vec::new();

    for mut candidate in candidates.drain(..) {
        let idx = candidate.pass.index();
        // 通路预算耗尽：后面的候选直接出局（先到先得 = 优先级序）。
        if occupancy.counts[idx] >= occupancy.budgets[idx] {
            continue;
        }
        let font_size = candidate.font_size_px * unit_scale;
        let w = estimate_label_width(&candidate.name, font_size);
        let h = estimate_label_height(font_size);
        let (bw, bh) = match candidate.angle {
            Some(phi) => rotated_extents(w, h, phi),
            None => (w, h),
        };
        // 空偏移表 = 只试原位（街名必须钉在段中点上，无备选方位）。
        let fallback = [Vec2::ZERO];
        let offsets: &[Vec2] = if candidate.offsets.is_empty() {
            &fallback
        } else {
            &candidate.offsets
        };
        for &off in offsets {
            let center = candidate.pos + off;
            if !satisfies_inside(&candidate.inside_rings, center) {
                continue;
            }
            let aabb = Aabb::from_center_size(center, bw, bh);
            if occupancy.boxes.iter().any(|b| b.overlaps(&aabb)) {
                continue;
            }
            // 落位：登记占位、把候选钉到最终渲染位。
            occupancy.boxes.push(aabb);
            occupancy.counts[idx] += 1;
            candidate.pos = center;
            selected.push(candidate);
            break;
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Quat;

    fn candidate(pass: LabelPass, pos: Vec2, name: &str, priority: u32) -> LabelCandidate {
        LabelCandidate::new(pass, pos, name, 12.0, priority, None)
    }

    /// 预算定制占位池（测试用；避免 default 后再赋字段的 clippy 嫌）。
    fn occupancy_with_budgets(budgets: [usize; 6]) -> LabelOccupancy {
        LabelOccupancy {
            budgets,
            ..Default::default()
        }
    }

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

    /// 中心构造（ADR-0073）：min/max 对称分布在中心两侧。
    #[test]
    fn aabb_from_center_size_is_symmetric() {
        let a = Aabb::from_center_size(Vec2::new(10.0, 20.0), 8.0, 4.0);
        assert!((a.min.x - 6.0).abs() < 1e-4 && (a.max.x - 14.0).abs() < 1e-4);
        assert!((a.min.y - 18.0).abs() < 1e-4 && (a.max.y - 22.0).abs() < 1e-4);
    }

    #[test]
    fn estimate_label_width_cjk_is_wider() {
        let latin = estimate_label_width("abc", 12.0);
        let cjk = estimate_label_width("你好", 12.0);
        assert!(cjk > latin, "CJK 应比拉丁宽");
    }

    /// 旋转 90° 时外扩尺寸互换宽高；0° 原样。
    #[test]
    fn rotated_extents_swap_at_quarter_turn() {
        let (w0, h0) = rotated_extents(30.0, 10.0, 0.0);
        assert!((w0 - 30.0).abs() < 1e-4 && (h0 - 10.0).abs() < 1e-4);
        let (w90, h90) = rotated_extents(30.0, 10.0, std::f32::consts::FRAC_PI_2);
        assert!((w90 - 10.0).abs() < 1e-4 && (h90 - 30.0).abs() < 1e-4);
        // 45° 对角外扩：(w+h)·√2/2。
        let (w45, h45) = rotated_extents(30.0, 10.0, std::f32::consts::FRAC_PI_4);
        let expected = (30.0 + 10.0) * std::f32::consts::FRAC_1_SQRT_2;
        assert!((w45 - expected).abs() < 1e-3 && (h45 - expected).abs() < 1e-3);
    }

    /// 直立化：朝左（|φ|>90°）翻转 180°，字永远头朝上（ADR-0073）。
    #[test]
    fn upright_angle_flips_lefthand_directions() {
        let eps = 1e-4;
        assert!(upright_angle(0.0).abs() < eps);
        // 135° 朝左上 → 翻成 -45°（朝右上）。
        assert!(
            (upright_angle(3.0 * std::f32::consts::FRAC_PI_4) + std::f32::consts::FRAC_PI_4).abs()
                < eps
        );
        // 180°（正朝左）→ 0°。
        assert!(upright_angle(std::f32::consts::PI).abs() < eps);
        // -90°（朝下）→ +90°（朝上）。
        assert!(
            (upright_angle(-std::f32::consts::FRAC_PI_2) - std::f32::consts::FRAC_PI_2).abs() < eps
        );
    }

    #[test]
    fn filter_collisions_removes_overlapping_labels() {
        let mut occupancy = occupancy_with_budgets([10; 6]);
        let mut candidates = vec![
            candidate(LabelPass::Place, Vec2::new(0.0, 0.0), "A", 1),
            candidate(LabelPass::Place, Vec2::new(1.0, 1.0), "B", 2), // 与 A 重叠
            candidate(LabelPass::Place, Vec2::new(100.0, 100.0), "C", 3),
        ];
        let result = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert_eq!(result.len(), 2, "应保留 A 和 C");
        assert_eq!(result[0].name, "A", "优先级最高的 A 应保留");
        assert_eq!(result[1].name, "C", "不重叠的 C 应保留");
    }

    #[test]
    fn filter_collisions_respects_priority() {
        let mut occupancy = occupancy_with_budgets([10; 6]);
        let mut candidates = vec![
            candidate(LabelPass::Place, Vec2::new(0.0, 0.0), "low", 10),
            candidate(LabelPass::Place, Vec2::new(1.0, 1.0), "high", 1), // 与 low 重叠
        ];
        let result = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "high", "高优先级应保留");
    }

    /// 字号是候选自身属性：同样的间距下，大字号标签先占位、小字号被挤掉。
    #[test]
    fn filter_collisions_uses_per_candidate_font_size() {
        let mut occupancy = occupancy_with_budgets([10; 6]);
        let mut big = candidate(LabelPass::Place, Vec2::new(0.0, 0.0), "大城市", 0);
        big.font_size_px = 16.0;
        let mut small = candidate(LabelPass::Place, Vec2::new(30.0, 0.0), "微点", 1);
        small.font_size_px = 12.0; // 盒 [18,42] 与大字盒 [-24,24] 重叠
        let mut candidates = vec![big, small];
        let result = filter_collisions(&mut candidates, 1.0, &mut occupancy);
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

    /// ADR-0073 核心：首选位被占 → 退而求其次试候选位，而不是丢弃。
    #[test]
    fn candidate_falls_back_to_next_offset_instead_of_dropping() {
        let mut occupancy = occupancy_with_budgets([10; 6]);
        // 先占住原位。
        let mut first = candidate(LabelPass::Place, Vec2::new(0.0, 0.0), "先到", 1);
        first.offsets = vec![Vec2::ZERO];
        let mut candidates = vec![first];
        filter_collisions(&mut candidates, 1.0, &mut occupancy);

        // 后到者首选位（原位）与先到者重叠 → 应落到右偏移位而非被丢弃。
        let mut second = candidate(LabelPass::Place, Vec2::new(0.0, 0.0), "后到", 2);
        second.offsets = vec![Vec2::ZERO, Vec2::new(500.0, 0.0)];
        let mut candidates = vec![second];
        let result = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert_eq!(result.len(), 1, "候选位可容身时不应丢弃");
        assert!(
            (result[0].pos.x - 500.0).abs() < 1e-4,
            "应落到第二候选位，实得 {:?}",
            result[0].pos
        );
    }

    /// 区域约束：候选位一旦挪出外环（或落进洞）即不可用。
    #[test]
    fn area_candidate_must_stay_inside_its_ring() {
        // 外环 = 100×100 方形（原点角起）。
        let outer = vec![0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0];
        let mut occupancy = occupancy_with_budgets([10; 6]);

        let mut c = candidate(LabelPass::Region, Vec2::new(50.0, 50.0), "大平原", 1);
        c.inside_rings = vec![outer.clone()];
        // 第二候选位远远挪出区域。
        c.offsets = vec![Vec2::ZERO, Vec2::new(1000.0, 0.0)];
        let mut candidates = vec![c];
        let result = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert_eq!(result.len(), 1, "原位在区域内应保留");
        assert!(
            (result[0].pos.x - 50.0).abs() < 1e-4,
            "出区域的候选位应被否决，实得 {:?}",
            result[0].pos
        );

        // 质心本身在区域外（环小于标签基点）→ 整条丢弃。
        let mut outside = candidate(LabelPass::Region, Vec2::new(500.0, 500.0), "越界", 1);
        outside.inside_rings = vec![outer];
        let mut candidates = vec![outside];
        let result = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert!(
            result.is_empty(),
            "区域装不下就不该画，got {}",
            result.len()
        );
    }

    /// 密度预算：通路余额耗尽后，同通路后续候选出局；别的通路不受影响。
    #[test]
    fn per_pass_budget_stops_flooding_without_starving_other_passes() {
        let mut occupancy = occupancy_with_budgets([10; 6]);
        occupancy.budgets[LabelPass::Street.index()] = 1;

        let mut streets: Vec<LabelCandidate> = (0..3)
            .map(|i| candidate(LabelPass::Street, Vec2::new(i as f32 * 500.0, 0.0), "街", 1))
            .collect();
        let kept = filter_collisions(&mut streets, 1.0, &mut occupancy);
        assert_eq!(kept.len(), 1, "街名预算 1，实得 {}", kept.len());

        // 地名通路预算未动，仍可落位。
        let mut places = vec![candidate(LabelPass::Place, Vec2::new(9999.0, 0.0), "城", 1)];
        let kept = filter_collisions(&mut places, 1.0, &mut occupancy);
        assert_eq!(kept.len(), 1, "地名通路不受街名预算影响");
    }

    /// 跨通路共享占位：第一次调用落位的盒，对第二次调用可见（六系统
    /// 互相避让的根基，ADR-0073）。
    #[test]
    fn occupancy_is_shared_across_filter_calls() {
        let mut occupancy = occupancy_with_budgets([10; 6]);

        let mut places = vec![candidate(LabelPass::Place, Vec2::new(0.0, 0.0), "城", 1)];
        filter_collisions(&mut places, 1.0, &mut occupancy);

        // 街名试图落在同一个盒里 → 被拒。
        let mut streets = vec![candidate(LabelPass::Street, Vec2::new(0.0, 0.0), "街", 1)];
        let kept = filter_collisions(&mut streets, 1.0, &mut occupancy);
        assert!(kept.is_empty(), "跨通路也应避让，got {}", kept.len());
    }

    /// 旋转候选的碰撞盒取保守外扩：45° 斜街占的位比水平同名义宽。
    #[test]
    fn rotated_candidate_blocks_diagonal_neighbours() {
        let mut occupancy = occupancy_with_budgets([10; 6]);

        let mut street = candidate(LabelPass::Street, Vec2::new(0.0, 0.0), "斜街", 1);
        street.angle = Some(std::f32::consts::FRAC_PI_4);
        let mut candidates = vec![street];
        let kept = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert_eq!(kept.len(), 1);

        // 水平紧邻位（间距小于外扩宽）应被斜街的保守盒挡住。
        let mut next = candidate(LabelPass::Street, Vec2::new(10.0, 0.0), "邻街", 2);
        next.angle = Some(std::f32::consts::FRAC_PI_4);
        let mut candidates = vec![next];
        let kept = filter_collisions(&mut candidates, 1.0, &mut occupancy);
        assert!(kept.is_empty(), "旋转盒应外扩挡位，got {}", kept.len());
    }

    /// 点偏移表经相机旋转后仍是世界方向向量（长度 = 倍率 × 字号）。
    #[test]
    fn point_label_offsets_rotate_with_camera() {
        let none = point_label_offsets(10.0, Quat::IDENTITY);
        assert_eq!(none.len(), 8);
        // 首选「上」= +Y（视图系 y 向上）。
        assert!((none[0].y - 9.0).abs() < 1e-4 && none[0].x.abs() < 1e-4);
        // 相机转 90° 后，「屏幕上方」应转到世界水平方向（长度不变）。
        let rot = point_label_offsets(10.0, Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        assert!((rot[0].length() - none[0].length()).abs() < 1e-4);
        assert!(rot[0].x.abs() > 8.0, "上方应转为水平方向，got {:?}", rot[0]);
    }
}
