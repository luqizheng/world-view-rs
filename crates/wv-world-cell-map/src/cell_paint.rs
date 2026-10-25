//! 单格连续绘制的状态与纯函数（[ADR-0052] D3 / D4 / D5 · §9.1）。
//!
//! ## 定位
//!
//! CellMap 工作台的**连续绘制**（拖动涂画）的数据内核：
//!
//! - [`CellPaintValue`] —— **落笔值**（D4）：拖动时写进每一格的三个通道值。
//! - [`CellStrokeState`] —— **笔触态**（D3）：拖动期间累积的格集合 + 快照。
//! - [`bresenham_cells`] —— **补间**（D3）：两帧采样点之间补齐中间的格，
//!   保证「快速拖动不漏格」。
//!
//! ## 硬边圆盘（D5 后半句 + ADR-0057 D9 修正）
//!
//! 笔刷形态 = **硬边圆盘、无 falloff、半径可调**（R ∈ 1–8 格，见
//! [`MAX_STROKE_RADIUS`]）——不复用 `wv_terrain::Brush` 的圆形软边。理由：
//! 地形编辑要的是「这格是森林 / 那格是沼泽」的确切控制，软边会引入
//! 「边界那格到底算不算」的歧义。R = 1 时退化为单格笔触（与 ADR-0052
//! 原状逐格一致）。
//!
//! ## 快照拒收（D3-E）
//!
//! 起笔时记下每格的 `before`；松手提交前逐格复核——任一格的当前值与快照
//! 不符（说明笔触期间被别的路径改了），**整笔拒收**（不是部分提交）。
//! 理由：部分提交会让「我画了 50 格，实际生效 47 格」不可察觉。

use bevy::prelude::*;

/// **落笔值**（D4）。
///
/// **地表 / 群系是绝对值，海拔是相对增量**——三个通道语义不同：
///
/// | 通道 | 语义 | 拖绘时的效果 |
/// |---|---|---|
/// | `surface` | **绝对**（0 = 水 / 1 = 陆） | 涂到的格一律变成这个地表 |
/// | `biome` | **绝对**（群系编号） | 涂到的格一律变成这个群系 |
/// | `height_delta` | **相对**（米，`0` = 不动） | 在原海拔上加这个增量 |
///
/// 之所以海拔用相对而不用绝对：连续绘制时作者想表达的是「这整片抬高 20 m」，
/// 不是「这整片都变成 20 m」。绝对海拔用**单格面板**的「此格海拔」设定
/// （D4 明确要求两个控件分区标注，不得无标签并排）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPaintValue {
    /// 地表（绝对）：0 = 水，1 = 陆。
    pub surface: u8,
    /// 群系（绝对）。
    pub biome: u8,
    /// 海拔增量（相对，米）：`0` = 不动海拔。
    pub height_delta: i16,
}

impl Default for CellPaintValue {
    fn default() -> Self {
        Self {
            surface: 1,
            biome: 4,
            height_delta: 0,
        }
    }
}

impl CellPaintValue {
    /// 把落笔值应用到某格的快照上，得到该格的 `after`（纯函数）。
    ///
    /// **海拔增量带钳制**：`i16` 加法溢出在 debug 模式会 panic，用
    /// `saturating_add` 兜住（`32767 + 100` → `32767`）。
    pub fn apply(
        &self,
        before: wv_core::terrain::TerrainCellData,
    ) -> wv_core::terrain::TerrainCellData {
        wv_core::terrain::TerrainCellData {
            height: before.height.saturating_add(self.height_delta),
            surface: self.surface,
            biome: self.biome,
        }
    }

    /// 本落笔值是否会改变这格（纯函数）。
    ///
    /// 用于「无变化就不进变更集」——避免提交 500 个实际没变的格
    /// （撤销栈里躺一条 500 格的空命令）。
    pub fn changes(&self, before: wv_core::terrain::TerrainCellData) -> bool {
        self.apply(before) != before
    }
}

/// **拖绘笔半径上限**（ADR-0057 D9）：`R ∈ 1..=8` 格。
///
/// 上限 8 的取法：R = 8 → 圆盘 ≤ 197 格／单采样点，Bresenham 补间后单笔仍
/// 在千格量级（1 条 `BatchEditCellsCommand` 的合理区间）；再大会让「一笔」
/// 的含义从「描边」滑向「刷整片」，撤销粒度也失去意义。
pub const MAX_STROKE_RADIUS: u8 = 8;

/// **落笔圆盘**（ADR-0057 D9 / 补记 4）：以 `center` 为圆心、**格心距严格
/// 小于 `radius`** 的格集合。
///
/// 严格小于（不是 ≤）是补记 4 钉死的口径：`R = 1` 只含圆心 1 格
/// （退化为 ADR-0052 的单格笔触，行为零变化）；`R = 2` 含正交 + 对角共 9 格。
/// `radius = 0` 按 1 兜底（防御，不改变正常路径）。
///
/// 纯函数——A5 的测试锚。返回序为 (dy, dx) 行主序，跨帧稳定。
pub fn disk_cells(center: (i32, i32), radius: u8) -> Vec<(i32, i32)> {
    let r = radius.max(1) as i32;
    // 格心距 < R ⇔ dx² + dy² < R²（整数精确比较，无浮点误差）。
    let limit = r * r;
    let (cx, cy) = center;
    let mut out = Vec::with_capacity((2 * r + 1) as usize * (2 * r + 1) as usize);
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy < limit {
                out.push((cx + dx, cy + dy));
            }
        }
    }
    out
}

/// **笔触态**（D3）——拖动期间累积的格集合。
///
/// 生命周期（`text` 标记：这是 ASCII 流程图，不是可编译的 Rust 代码——
/// 不加标记时 rustdoc 会把它当 doctest 编译，报「unknown start of token ─」）：
///
/// ```text
/// press ──▶ 起笔（记快照）──▶ 拖动（累积格 + 待落色块）──▶ release ──▶ 提交
///                │                                                    │
///                └─ Esc / 右键 / 切工具 ──▶ 丢弃整笔 ────────────────┘
/// ```
#[derive(Debug, Default, Resource)]
pub struct CellStrokeState {
    /// 已采样到的格（去重，插入序稳定）。
    ///
    /// **待落**——这些格**还没写进世界**，只画红橙 `STROKE_PENDING` 色块。
    pub cells: Vec<(i32, i32)>,
    /// 每格起笔时的值（快照）——用于 D3-E 的 stale 复核。
    ///
    /// 与 `cells` **一一对应**（同序）：`snapshots[i]` 是 `cells[i]` 的快照。
    pub snapshots: Vec<wv_core::terrain::TerrainCellData>,
    /// 起笔时的目标图 ID（提交时要写回哪张图）。
    pub map_id: Option<wv_core::Id>,
    /// 是否正在拖绘（`true` = 笔触活跃）。
    pub active: bool,
}

impl CellStrokeState {
    /// 起笔：清空上一笔的残留（若有），标记活跃。
    pub fn begin(&mut self) {
        self.cells.clear();
        self.snapshots.clear();
        self.active = true;
    }

    /// 丢弃整笔（D3-B 右键 / D3-C Esc / 切工具 / 切图）。
    pub fn discard(&mut self) {
        self.cells.clear();
        self.snapshots.clear();
        self.map_id = None;
        self.active = false;
    }

    /// 已累积的格数（D8-B 底栏进度读数）。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 是否为空（无格）。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// **追加一格**（去重）——若该格已在本笔中则**不重复追加**。
    ///
    /// 返回 `true` = 新增了一格（调用方据此标脏 / 画待落块）。
    pub fn push_cell(
        &mut self,
        gx: i32,
        gy: i32,
        snapshot: wv_core::terrain::TerrainCellData,
    ) -> bool {
        if self.cells.iter().any(|(x, y)| *x == gx && *y == gy) {
            return false;
        }
        self.cells.push((gx, gy));
        self.snapshots.push(snapshot);
        true
    }

    /// **stale 复核**（D3-E）：逐格比对快照与当前世界值。
    ///
    /// 返回 `None` = 全部一致（可提交）；`Some(不一致的格)` = 整笔拒收。
    pub fn stale_cells(
        &self,
        current: &dyn Fn(i32, i32) -> Option<wv_core::terrain::TerrainCellData>,
    ) -> Option<Vec<(i32, i32)>> {
        let mut bad = Vec::new();
        for (i, (gx, gy)) in self.cells.iter().enumerate() {
            let now = current(*gx, *gy);
            let before = self.snapshots.get(i).copied();
            if now != before {
                bad.push((*gx, *gy));
            }
        }
        if bad.is_empty() {
            None
        } else {
            Some(bad)
        }
    }
}

/// **Bresenham 补间**（D3）：两格之间补齐中间的格。
///
/// 快速拖动时，两帧的采样点可能隔好几格——不补间就会「漏格」（画出一条
/// 虚线）。本函数返回**含两端点**的连续格序列。
///
/// 走**八向**连通（允许对角）——对角补出来的格也在笔画覆盖范围内，
/// 视觉上更连续（沿斜坡画线不会断）。
///
/// 纯函数——测试锚。
pub fn bresenham_cells(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (x0, y0) = from;
    let (x1, y1) = to;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    let mut out = Vec::new();
    loop {
        out.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

/// **补间路径**（D3）：把上一采样点到本次采样点之间的所有格补进笔触。
///
/// 返回**新增**的格（已在本笔中的不重复返回）。
///
/// 纯函数（`seen` 是调用方传入的已累积集合）——测试锚。
pub fn interpolate_stroke_cells(
    from: (i32, i32),
    to: (i32, i32),
    seen: &[(i32, i32)],
) -> Vec<(i32, i32)> {
    bresenham_cells(from, to)
        .into_iter()
        .filter(|c| !seen.contains(c))
        .collect()
}

/// **落笔值 + 笔触态**的会话资源（跨 crate 共享）。
///
/// 拆分理由：**渲染层要读笔触格**（画待落色块），**应用层要读写笔触**。
/// `wv-render` 不依赖 `wv-app`，故这份状态必须落在两者都依赖的
/// `wv-world-cell-map` 里（[ADR-0054] 的 crate 分层纪律）。
///
/// - `wv-app::edit::cell_stroke` 写笔触（起笔 / 累积 / 提交 / 丢弃）；
/// - `wv-render::cellmap_bridge::sync_cell_stroke` 读笔触（画待落色块）。
#[derive(Debug, Resource)]
pub struct CellPaintSession {
    /// **落笔值**（D4）：拖绘时写进每一格的值（会话态，不落世界）。
    pub paint: CellPaintValue,
    /// **笔触态**（D3）：格集合 + 快照。
    pub stroke: CellStrokeState,
    /// **拖绘笔半径**（ADR-0057 D9）：`1..=MAX_STROKE_RADIUS` 格。
    /// 会话参数（面板滑条写），不落世界——半径是笔杆属性，不是地形数据。
    pub radius: u8,
}

impl Default for CellPaintSession {
    fn default() -> Self {
        Self {
            paint: CellPaintValue::default(),
            stroke: CellStrokeState::default(),
            // 默认 1 格 = ADR-0052 原状，行为零变化（D9）。
            radius: 1,
        }
    }
}

impl CellPaintSession {
    /// 当前是否在拖绘。
    pub fn is_stroking(&self) -> bool {
        self.stroke.active
    }

    /// 已落格数（D8-B 底栏进度读数）。
    pub fn stroke_len(&self) -> usize {
        self.stroke.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::TerrainCellData;

    fn cell(height: i16, surface: u8, biome: u8) -> TerrainCellData {
        TerrainCellData {
            height,
            surface,
            biome,
        }
    }

    // ── D4 · 落笔值语义 ──────────────────────────────────────────────

    /// 地表 / 群系是**绝对**，海拔是**相对增量**。
    #[test]
    fn paint_value_applies_absolute_surface_biome_and_relative_height() {
        let paint = CellPaintValue {
            surface: 0,
            biome: 7,
            height_delta: 20,
        };
        let before = cell(100, 1, 2);
        let after = paint.apply(before);
        assert_eq!(after.surface, 0, "地表是绝对值");
        assert_eq!(after.biome, 7, "群系是绝对值");
        assert_eq!(after.height, 120, "海拔是相对增量（100 + 20）");
    }

    /// `height_delta = 0` 时海拔不动，但地表 / 群系照写。
    #[test]
    fn paint_value_with_zero_delta_keeps_height() {
        let paint = CellPaintValue {
            surface: 1,
            biome: 3,
            height_delta: 0,
        };
        let after = paint.apply(cell(345, 0, 9));
        assert_eq!(after.height, 345, "delta = 0 → 海拔不动");
        assert_eq!(after.surface, 1);
        assert_eq!(after.biome, 3);
    }

    /// 海拔增量溢出用 `saturating_add` 兜住（不能 panic）。
    #[test]
    fn paint_value_height_delta_saturates() {
        let paint = CellPaintValue {
            surface: 1,
            biome: 1,
            height_delta: 300,
        };
        let after = paint.apply(cell(i16::MAX, 1, 1));
        assert_eq!(after.height, i16::MAX, "溢出应饱和而非 panic");
    }

    /// 无变化 → `changes` 为 false（不进变更集）。
    #[test]
    fn paint_value_reports_no_change_when_identical() {
        let paint = CellPaintValue {
            surface: 1,
            biome: 4,
            height_delta: 0,
        };
        assert!(!paint.changes(cell(100, 1, 4)), "完全相同 → 无变化");
        assert!(paint.changes(cell(100, 0, 4)), "地表不同 → 有变化");
        // 海拔是**相对增量**：`height_delta = 0` 时无论原海拔多少都不改，
        // 故不是「海拔不同就有变化」——只有增量非 0 才动海拔。
        assert!(
            !paint.changes(cell(99, 1, 4)),
            "增量为 0 → 原海拔多高都不变，不算变化"
        );
        let raise = CellPaintValue {
            surface: 1,
            biome: 4,
            height_delta: 20,
        };
        assert!(raise.changes(cell(99, 1, 4)), "增量非 0 → 海拔会变");
    }

    // ── D3 · 笔触累积与去重 ──────────────────────────────────────────

    /// 同一格不重复累积（快照与格序列保持一一对应）。
    #[test]
    fn stroke_state_dedupes_cells() {
        let mut s = CellStrokeState::default();
        assert!(s.push_cell(3, 4, cell(10, 1, 1)));
        assert!(!s.push_cell(3, 4, cell(10, 1, 1)), "重复格不应再进");
        assert_eq!(s.len(), 1);
        assert_eq!(s.snapshots.len(), 1, "快照与格序列必须同长");
    }

    /// 丢弃整笔 → 格与快照都清空，笔触失活。
    #[test]
    fn stroke_state_discard_clears_everything() {
        let mut s = CellStrokeState::default();
        s.begin();
        s.push_cell(1, 1, cell(10, 1, 1));
        s.map_id = Some(wv_core::Id::new_unchecked("m1"));
        s.discard();
        assert!(s.is_empty());
        assert!(s.snapshots.is_empty());
        assert!(s.map_id.is_none());
        assert!(!s.active);
    }

    /// 起笔清残留（上一笔没丢干净也不该污染下一笔）。
    #[test]
    fn stroke_state_begin_clears_previous_residue() {
        let mut s = CellStrokeState::default();
        s.push_cell(9, 9, cell(10, 1, 1));
        s.begin();
        assert!(s.is_empty(), "起笔应清空上一笔残留");
        assert!(s.active);
    }

    // ── D9 · 落笔圆盘（ADR-0057）────────────────────────────────────

    /// `R = 1` 退化为单格（与 ADR-0052 单格笔触逐格一致）。
    #[test]
    fn disk_radius_one_is_single_cell() {
        assert_eq!(disk_cells((5, 7), 1), vec![(5, 7)], "R=1 只含圆心");
        assert_eq!(disk_cells((5, 7), 0), vec![(5, 7)], "R=0 按 1 兜底");
    }

    /// `R = 2` 严格小于 2 → 正交 + 对角共 9 格（补记 4 钉死的口径）。
    #[test]
    fn disk_radius_two_contains_diagonals() {
        let cells = disk_cells((0, 0), 2);
        assert_eq!(cells.len(), 9, "R=2 应含 3×3 全部 9 格，实际 {cells:?}");
        assert!(cells.contains(&(1, 1)), "对角格在内");
        assert!(cells.contains(&(-1, -1)), "对角格在内");
    }

    /// 严格小于：`R = 3` 不含格心距恰为 3 的格（(3,0) / (0,-3)）。
    #[test]
    fn disk_excludes_cells_at_exact_radius_distance() {
        let cells = disk_cells((0, 0), 3);
        assert!(!cells.contains(&(3, 0)), "距 3 不在严格小于口径内");
        assert!(!cells.contains(&(0, -3)), "距 3 不在严格小于口径内");
        assert!(cells.contains(&(2, 2)), "距 2.83 < 3 应入选");
        assert_eq!(cells.len(), 25, "R=3 圆盘应为 25 格，实际 {cells:?}");
    }

    /// 圆心偏移：所有格整体平移，格数不变。
    #[test]
    fn disk_is_translated_with_center() {
        let a = disk_cells((10, 20), 2);
        let b = disk_cells((0, 0), 2);
        let mut shifted: Vec<(i32, i32)> = b.iter().map(|(x, y)| (x + 10, y + 20)).collect();
        let mut a_sorted = a.clone();
        shifted.sort();
        a_sorted.sort();
        assert_eq!(a_sorted, shifted, "平移不改变圆盘形状");
    }

    /// 上限半径不越界（只做形状自洽检查，不引性能断言）。
    #[test]
    fn disk_max_radius_is_bounded() {
        let cells = disk_cells((0, 0), MAX_STROKE_RADIUS);
        assert!(cells.len() < 4 * MAX_STROKE_RADIUS as usize * MAX_STROKE_RADIUS as usize);
        assert!(cells.contains(&(0, 0)));
    }

    // ── D3-E · stale 复核 ───────────────────────────────────────────

    /// 快照与当前值一致 → 可提交（`stale_cells` 返回 None）。
    #[test]
    fn stroke_state_stale_check_passes_when_unchanged() {
        let mut s = CellStrokeState::default();
        s.push_cell(1, 1, cell(10, 1, 1));
        s.push_cell(2, 1, cell(20, 1, 1));
        let world = |gx: i32, gy: i32| match (gx, gy) {
            (1, 1) => Some(cell(10, 1, 1)),
            (2, 1) => Some(cell(20, 1, 1)),
            _ => None,
        };
        assert!(s.stale_cells(&world).is_none(), "一致 → 可提交");
    }

    /// 任一格被外部改动 → 整笔拒收（返回不一致的格）。
    #[test]
    fn stroke_state_stale_check_rejects_whole_stroke() {
        let mut s = CellStrokeState::default();
        s.push_cell(1, 1, cell(10, 1, 1));
        s.push_cell(2, 1, cell(20, 1, 1));
        // 第 2 格被外部改了
        let world = |gx: i32, gy: i32| match (gx, gy) {
            (1, 1) => Some(cell(10, 1, 1)),
            (2, 1) => Some(cell(999, 1, 1)),
            _ => None,
        };
        let bad = s.stale_cells(&world).expect("应拒收");
        assert_eq!(bad, vec![(2, 1)], "只报被改的那格");
    }

    // ── D3 · Bresenham 补间 ─────────────────────────────────────────

    /// 水平直线补间：连续、无漏格。
    #[test]
    fn bresenham_fills_horizontal_gap() {
        let cells = bresenham_cells((0, 0), (5, 0));
        assert_eq!(cells.len(), 6, "含两端点");
        assert_eq!(cells.first(), Some(&(0, 0)));
        assert_eq!(cells.last(), Some(&(5, 0)));
        for (i, c) in cells.iter().enumerate() {
            assert_eq!(c.0, i as i32, "x 应连续");
        }
    }

    /// **快速拖动不漏格**（D3 验收 ②）：从 (0,0) 到 (10,0) 补出 11 格。
    #[test]
    fn bresenham_has_no_gaps_on_fast_drag() {
        let cells = bresenham_cells((0, 0), (10, 0));
        assert_eq!(cells.len(), 11);
        let mut sorted = cells.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 11, "不应有重复格");
    }

    /// 对角线也连通（八向）。
    #[test]
    fn bresenham_handles_diagonal() {
        let cells = bresenham_cells((0, 0), (3, 3));
        assert_eq!(cells.first(), Some(&(0, 0)));
        assert_eq!(cells.last(), Some(&(3, 3)));
        // 相邻两格必须八向相邻（max(|dx|,|dy|) == 1）
        for w in cells.windows(2) {
            let dx = (w[1].0 - w[0].0).abs();
            let dy = (w[1].1 - w[0].1).abs();
            assert!(dx <= 1 && dy <= 1 && (dx + dy) > 0, "相邻格须八向邻接");
        }
    }

    /// 起点 = 终点 → 只返回一格。
    #[test]
    fn bresenham_same_point_returns_single_cell() {
        assert_eq!(bresenham_cells((2, 2), (2, 2)), vec![(2, 2)]);
    }

    /// 负坐标方向也成立。
    #[test]
    fn bresenham_handles_negative_direction() {
        let cells = bresenham_cells((0, 0), (-3, 0));
        assert_eq!(cells.len(), 4);
        assert_eq!(cells.last(), Some(&(-3, 0)));
    }

    /// 补间只返回**新增**的格（已累积的不重复）。
    #[test]
    fn interpolate_returns_only_new_cells() {
        let seen = vec![(0, 0), (1, 0)];
        let fresh = interpolate_stroke_cells((0, 0), (3, 0), &seen);
        assert_eq!(fresh, vec![(2, 0), (3, 0)], "只返回新增的");
    }
}
