//! CellMap 会话态（自 wv-render/src/state.rs 迁入，ADR-0054 D1）。
//!
//! `EditCellState` / `CellSelection` / `GridConfig` 是 CellMap 工作台**专属**的
//! 会话资源；`ViewState` 等全渲染层共享的呈现态仍留 wv-render（经其 re-export，
//! `wv_render::state::EditCellState` 等旧路径零改动）。

use bevy::prelude::*;

/// EditCell 工具的会话态（ADR-0040 决策 5；ADR-0052 扩展；ADR-0057 D3 瘦身）。
///
/// ADR-0057 D3 锁死职责分野：**选区一律由选择工具（S/R/V/W）产出，EditCell
/// 只画不选**——原 `selected` / `region_cells` / `region_fingerprint` /
/// `allow_drag` 四字段随之退役（选区归 [`CellSelection`]；`allow_drag` 存在的
/// 唯一理由是区分 EditCell / CellSelect 两形态，CellSelect 改名 PointSelect
/// 并独立激活态后即成死代码）。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct EditCellState {
    /// EditCell 工具是否激活（与 wv-render `BrushState::enabled` 同款会话布尔）。
    /// 由命令分发器在 `SelectMapTool(EditCell)` 翻 true / 切走其它工具翻 false。
    pub tool_active: bool,
    /// 当前鼠标压住的格 (gx, gy)；`None` 表示鼠标不在地图内。
    pub hover: Option<(i32, i32)>,
    /// 单格表单海拔输入的**暂存值** `(gx, gy, height)`（2026-10-25 P0-1
    /// 方案 A）：拖绘 scrub 每帧写值，若逐帧落盘会把 1 个拖拽手势拆成
    /// 一串撤销命令——暂存区让面板在「手势完成帧」才提交 1 条
    /// `EditCellCommand`。坐标标签用于手势中途换格 / 换图时的失效判定。
    pub pending_height: Option<(i32, i32, i16)>,
    /// UX-017（2026-10-30）：单格表单「地表 / 群系」的**草稿**——点击只改
    /// 草稿，「应用」才提交（1 次表单会话 = 1 条命令 = 1 个撤销单位；
    /// 此前每击一命令，点 N 格要 N 次 Ctrl+Z）。海拔仍走手势完成帧
    /// （P0-1 方案 A 不变），手势帧提交时若草稿在则并成同一条命令。
    /// 坐标 / 图标签用于换格 / 换图失效判定（与 `pending_height` 同法）。
    pub form_draft: Option<CellFormDraft>,
}

/// 单格表单草稿（见 [`EditCellState::form_draft`]）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CellFormDraft {
    /// 所属图 id（字符串形态，避免 wv-world-cell-map 依赖core 的 Id 序列化面）。
    pub map: String,
    /// 目标格。
    pub cell: (i32, i32),
    /// 草稿地表码。
    pub surface: u8,
    /// 草稿群系码。
    pub biome: u8,
}

impl EditCellState {
    /// **只关工具**（[ADR-0052] §9.1 S0-1 口径在 ADR-0057 下的延续）。
    ///
    /// 切走 EditCell 工具时只翻 `tool_active = false` 并清 `hover`
    /// （hover 是瞬时态，留着会画出「工具已关但还有 hover 高亮」的鬼影）。
    /// 选区不在本结构——切工具是否保留选区由 [`CellSelection`] 的生命周期
    /// 口径回答（保留；ADR-0057 D1 / D5）。
    pub fn close_tool(&mut self) {
        self.tool_active = false;
        self.hover = None;
        // 海拔暂存属于单格表单的会话态：表单随工具关闭而消失，残留的
        // 暂存值会在下次打开时把旧值当「当前值」展示并误导提交。
        self.pending_height = None;
        // UX-017：表单草稿同理——工具关了草稿就没了着落。
        self.form_draft = None;
    }
}

/// **统一选区**（ADR-0057 D1）：CellMap 工作区的一等会话对象——当前图上的格子集合。
///
/// 取代 `EditCellState.selected`（1 格）与 `region_cells`（N 格）双轨：
/// 单格 = 1 格选区，多边形区域 = N 格选区，面板与渲染只面对这一个对象。
///
/// - 物理形态：`Vec<u64>` 位图按图幅格数定容（默认 512² 格 ≈ 32 KB；
///   即便 4096² 格亦仅 2 MB），手摆位运算，零新依赖（AGENTS §9）；
/// - 会话态（D5）：**不进撤销栈、不落盘**；Esc / 切图清空，切工具保留；
/// - 集合运算：[`replace`](Self::replace) / [`union`](Self::union) /
///   [`subtract`](Self::subtract) 对应修饰键语义「无修饰 = 替换、
///   Shift = 并、Alt = 差」（D2）。
#[derive(Resource, Debug, Default, Clone)]
pub struct CellSelection {
    /// 位图：格 (gx, gy) 的位 = `bits[(gy * width + gx) / 64]` 的第
    /// `(gy * width + gx) % 64` 位。
    bits: Vec<u64>,
    /// 图幅格数（宽 × 高）；由 [`bind_map`](Self::bind_map) 惰性定容。
    width: u32,
    height: u32,
    /// 选中格计数——读数条每帧要「N 格」，增量维护免每帧全图 popcount。
    count: u32,
    /// 绑定的图 ID；变图即清空（D1 生命周期）。
    last_map_id: Option<wv_core::Id>,
}

impl CellSelection {
    /// 绑定图与图幅（惰性定容）：图变**或**图幅变 → 位图重定容并清空；
    /// 同图同尺寸 → 不动（每帧调用安全）。
    ///
    /// 「切图清空」（ADR-0057 D1 / A7）收敛在这一个入口——调用方
    /// （cellmap_bridge 的选区同步系统）每帧把当前图喂进来即可。
    pub fn bind_map(&mut self, map_id: &wv_core::Id, width: u32, height: u32) {
        let unchanged = self.last_map_id.as_ref() == Some(map_id)
            && self.width == width
            && self.height == height;
        if unchanged {
            return;
        }
        self.width = width;
        self.height = height;
        let words = (width as usize * height as usize).div_ceil(64);
        self.bits.clear();
        self.bits.resize(words, 0);
        self.count = 0;
        self.last_map_id = Some(map_id.clone());
    }

    /// 清空选区（Esc 入口；保留图幅绑定与位图容量）。
    pub fn clear(&mut self) {
        self.bits.fill(0);
        self.count = 0;
    }

    /// 选区是否为空（空 = 无选区；拖绘蒙版判据 D4 直接消费它）。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 选中格数（读数条「N 格」）。
    pub fn len(&self) -> u32 {
        self.count
    }

    /// 恰好选中 1 格时返回该格（`None` = 空选区或多格选区）。
    ///
    /// ADR-0057 D1「单格 = 1 格选区」的读侧入口：单格表单、候选命名判定都靠
    /// 它把「1 格选区」还原成一个坐标，**不必再维护「当前格」字段**
    /// （原 `EditCellState.selected` 的职责收敛在此）。
    pub fn only_cell(&self) -> Option<(i32, i32)> {
        if self.count != 1 {
            return None;
        }
        self.iter_cells().next()
    }

    /// 图幅宽（格）。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 图幅高（格）。
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 格 (gx, gy) 是否选中；越界一律 `false`（图幅外 = 未选中，
    /// 边界边提取依赖这个口径把图幅边沿算作边界）。
    pub fn contains(&self, gx: i32, gy: i32) -> bool {
        if gx < 0 || gy < 0 || gx >= self.width as i32 || gy >= self.height as i32 {
            return false;
        }
        let idx = gy as usize * self.width as usize + gx as usize;
        self.bits[idx / 64] & (1u64 << (idx % 64)) != 0
    }

    /// 置位 / 复位单格（越界静默忽略——选择工具的栅格化产出可能贴图幅边）。
    fn set(&mut self, gx: i32, gy: i32, on: bool) {
        if gx < 0 || gy < 0 || gx >= self.width as i32 || gy >= self.height as i32 {
            return;
        }
        let idx = gy as usize * self.width as usize + gx as usize;
        let mask = 1u64 << (idx % 64);
        let was = self.bits[idx / 64] & mask != 0;
        match (on, was) {
            (true, false) => {
                self.bits[idx / 64] |= mask;
                self.count += 1;
            }
            (false, true) => {
                self.bits[idx / 64] &= !mask;
                self.count -= 1;
            }
            _ => {}
        }
    }

    /// **替换**（无修饰）：选区 = `cells`。
    pub fn replace(&mut self, cells: impl IntoIterator<Item = (i32, i32)>) {
        self.clear();
        self.union(cells);
    }

    /// **并集**（Shift 加选）：选区 ∪= `cells`。
    pub fn union(&mut self, cells: impl IntoIterator<Item = (i32, i32)>) {
        for (gx, gy) in cells {
            self.set(gx, gy, true);
        }
    }

    /// **差集**（Alt 减选）：选区 −= `cells`。
    pub fn subtract(&mut self, cells: impl IntoIterator<Item = (i32, i32)>) {
        for (gx, gy) in cells {
            self.set(gx, gy, false);
        }
    }

    /// 选区内容指纹（FNV-1a over 位图 + 图幅 + 计数）——渲染侧
    /// 「选区变了才重建 mesh」的重建键一半（另一半是缩放档）。
    /// O(位图字数)：512² 格 = 512 词，每帧可忽略。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for w in &self.bits {
            h ^= *w;
            h = h.wrapping_mul(0x100000001b3);
        }
        for v in [self.width as u64, self.height as u64, self.count as u64] {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 稀疏迭代选中格（gy 主序、gx 次序——位图扫描天然有序）。
    pub fn iter_cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let w = self.width as i32;
        self.bits.iter().enumerate().flat_map(move |(wi, &word)| {
            (0..64usize).filter_map(move |b| {
                if word & (1u64 << b) != 0 {
                    let idx = (wi * 64 + b) as i32;
                    // width = 0 时 bits 为空，不会走到这里——除零不可达。
                    Some((idx % w, idx / w))
                } else {
                    None
                }
            })
        })
    }

    /// **外围边界边提取**（ADR-0057 D8 蚂蚁线的几何源）。
    ///
    /// 只产出「选中格与未选中格（或图幅外）相邻」的边；内部共享边不产
    /// （否则虚线叠虚线，大选区糊成一片）。Alt 减选挖出的空洞，其内边界
    /// 由同一邻接判定天然覆盖。O(选区格数)，邻接查位图 O(1)。
    pub fn boundary_edges(&self) -> Vec<BoundaryEdge> {
        let mut out = Vec::new();
        for (gx, gy) in self.iter_cells() {
            if !self.contains(gx, gy - 1) {
                out.push(BoundaryEdge {
                    orientation: EdgeOrientation::Horizontal,
                    gx,
                    gy,
                });
            }
            if !self.contains(gx, gy + 1) {
                out.push(BoundaryEdge {
                    orientation: EdgeOrientation::Horizontal,
                    gx,
                    gy: gy + 1,
                });
            }
            if !self.contains(gx - 1, gy) {
                out.push(BoundaryEdge {
                    orientation: EdgeOrientation::Vertical,
                    gx,
                    gy,
                });
            }
            if !self.contains(gx + 1, gy) {
                out.push(BoundaryEdge {
                    orientation: EdgeOrientation::Vertical,
                    gx: gx + 1,
                    gy,
                });
            }
        }
        out
    }
}

/// 边界边朝向：水平边（东西走向，隔开南北两格）或垂直边（南北走向）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeOrientation {
    /// 线段 `(gx, gy)-(gx + 1, gy)`（格坐标系）。
    Horizontal,
    /// 线段 `(gx, gy)-(gx, gy + 1)`（格坐标系）。
    Vertical,
}

/// 一条选区边界边（格坐标系，轴对齐——「阶梯线」是天然形态，ADR-0057 D8
/// 不做曲线化 / 圆角）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryEdge {
    /// 朝向（水平 / 垂直）。
    pub orientation: EdgeOrientation,
    /// 边起点的格坐标 x（水平边 = 左端点；垂直边 = 下端点）。
    pub gx: i32,
    /// 边起点的格坐标 y。
    pub gy: i32,
}

/// 蚂蚁线相位（ADR-0057 D8）：同一组边界边切成实/空段后建两套 mesh，
/// 计时器 ≈0.3 s 整组切换可见性——A 实段即 B 空段，反之亦然。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashPhase {
    /// 相位 A：取段索引为偶数的段。
    A,
    /// 相位 B：取奇数段。
    B,
}

impl DashPhase {
    /// 该相位下段索引 `segment_index` 是否为实段（要画）。
    pub fn is_solid(self, segment_index: usize) -> bool {
        match self {
            DashPhase::A => segment_index.is_multiple_of(2),
            DashPhase::B => segment_index % 2 == 1,
        }
    }
}

/// 把长度为 `length`（屏幕像素）的边按节距 `pitch` 切成 `[start, end)` 偏移段。
///
/// 段索引奇偶 = 相位归属（配合 [`DashPhase::is_solid`]）：逐边独立切段，
/// A/B 互补由构造保证——无需对齐全局相位网格。`length ≤ 0` 或
/// `pitch ≤ 0` 返回空（调用方不应产生这样的边，防御兜底）。
pub fn dash_segments(length: f32, pitch: f32) -> Vec<(f32, f32)> {
    if length <= 0.0 || pitch <= 0.0 {
        return Vec::new();
    }
    let n = (length / pitch).ceil() as usize;
    (0..n)
        .map(|i| {
            let s = i as f32 * pitch;
            (s, (s + pitch).min(length))
        })
        .collect()
}

/// **泛洪判据**的会话态（[ADR-0052] D6；ADR-0086 收窄）。
///
/// 历史上它是「泛洪选择工具」的工具态（带 `tool_active`）；ADR-0086 退役
/// 该工具后，本结构只剩**判据**一份——CellMap 里点选工具的**默认点击 =
/// 泛洪**（ADR-0074 追记四）仍按此判据圈同值连通区，浮窗「泛洪判据」
/// 段编辑的也是它。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct FloodSelectState {
    /// **判据**：按地表 / 按群系 / 按海拔容差。
    ///
    /// 默认按**群系**——CellMap 的主要用途是「选出一片同群系的地」，
    /// 按地表（水/陆）粒度太粗，按海拔容差需要额外输入。
    pub criterion: FloodCriterion,
}

/// **点选工具**的会话态（ADR-0057 补记 6：原 `MapToolId::CellSelect`
/// 改名 `PointSelect`，激活态独立为小 Resource，不再与 EditCell 共用
/// `tool_active`——D3 职责分野在状态层的落实）。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct PointSelectState {
    /// 点选工具是否激活。
    pub tool_active: bool,
}

impl PointSelectState {
    /// 关工具（切走时调用）——只翻 `tool_active`，不清选区。
    pub fn close_tool(&mut self) {
        self.tool_active = false;
    }
}

/// **泛洪判据**（D6）：什么算「同值」。
///
/// 与 `wv_terrain::cellops::FloodCriterion` 同款语义——此处独立定义是为了
/// 让 `wv-world-cell-map` 不依赖 `wv-terrain`（[ADR-0054] D2/D4 的 crate
/// 分层纪律：地形域的 crate 只依赖 core）。
///
/// **Off 为默认**（ADR-0090 D3 实施裁量，2026-10-30）：圈选「单格」形状的
/// 点击默认**直选单格**；判据切到 Surface/Biome/HeightTolerance 时同一点击
/// 变成泛洪（ADR-0074 追记四 / ADR-0088 的泛洪手感由判据档位承载）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FloodCriterion {
    /// 关：点击 = 直选单格（不泛洪）。
    #[default]
    Off,
    /// 按**地表**判同（水 / 陆）。
    Surface,
    /// 按**群系**判同。
    Biome,
    /// 按**海拔容差**判同（`|h − h₀| ≤ tol`）。
    HeightTolerance(i16),
}

/// 地形栅格的视图标定（自 wv-render 迁入；`grid_sync` 桥接系统在 wv-render
/// 侧每帧从呈现态投影写入）。
///
/// ⚠️ 本仓库有**两套原点**（详见原 wv-render state.rs 的对照表）：
/// `raster_origin` 服务**地形栅格**、`map_origin` 服务**本图局部内容**；
/// 两者只在「当前图自带底图」时相等。子图继承父图底图时栅格仍是父图那一张。
#[derive(Resource)]
pub struct GridConfig {
    /// 每格世界边长。
    pub cell_size: f32,
    /// 栅格空间原点的视图位置（**地形**专用）。
    pub raster_origin: Vec2,
    /// 当前图图帧原点的视图位置（**本图局部内容**专用）。
    pub map_origin: Vec2,
    /// 当前图幅面（`extent` 的宽 × 高，世界单位）：地图外框与 1/4 参考网格用。
    /// 16:9 缺省后不再是正方形——`x` / `y` 分开。
    pub map_size: Vec2,
}

impl Default for GridConfig {
    /// 缺省栅格：4096² 世界图口径（`cell_size = 8.0`，自 wv-render state.rs 迁入）。
    fn default() -> Self {
        Self {
            cell_size: 8.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        }
    }
}

impl GridConfig {
    /// 本图局部坐标 → 视图空间。
    ///
    /// **本图局部内容（地点 / 街道 / 街道名 / 外框）一律走这里**——绕过去手写
    /// `map_origin + local` 就等于把两套原点的对照表丢在一边。
    pub fn map_to_view(&self, local: Vec2) -> Vec2 {
        self.map_origin + local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // ── EditCellState（ADR-0057 D3 瘦身后）──────────────────────────

    /// `close_tool` 只关工具 + 清瞬时 hover；选区不在本结构，无可清。
    #[test]
    fn close_tool_turns_off_tool_and_clears_hover() {
        let mut s = EditCellState {
            form_draft: None,
            tool_active: true,
            hover: Some((3, 4)),
            pending_height: Some((3, 4, 100)),
        };
        s.close_tool();
        assert!(!s.tool_active, "工具应关闭");
        assert_eq!(s.hover, None, "hover 是瞬时态，关工具即清");
        assert_eq!(
            s.pending_height, None,
            "海拔暂存是单格表单会话态，关工具即清"
        );
    }

    // ── CellSelection 位图运算（A1）─────────────────────────────────

    /// 确定性伪随机（LCG）——不引 rand（AGENTS §9 依赖纪律）。
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn cell(&mut self, w: i32, h: i32) -> (i32, i32) {
            ((self.0 % w as u64) as i32, (self.next() % h as u64) as i32)
        }
        fn cells(&mut self, n: usize, w: i32, h: i32) -> Vec<(i32, i32)> {
            (0..n)
                .map(|_| {
                    let c = self.cell(w, h);
                    self.next();
                    c
                })
                .collect()
        }
    }

    /// **A1**：`replace` / `union` / `subtract` / `is_empty` / `iter_cells`
    /// 对伪随机格集与 `BTreeSet` 参照实现逐格一致。
    #[test]
    fn bitmap_ops_match_btree_set_reference_on_random_sets() {
        let (w, h) = (97u32, 53u32); // 故意取非 64 倍数的图幅
        let map = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map, w, h);
        let mut rng = Lcg(0x5eed);
        let mut reference: BTreeSet<(i32, i32)> = BTreeSet::new();

        for round in 0..24 {
            let batch = rng.cells(64, w as i32, h as i32);
            match round % 3 {
                0 => {
                    sel.replace(batch.iter().copied());
                    reference = batch.iter().copied().collect();
                }
                1 => {
                    sel.union(batch.iter().copied());
                    reference.extend(batch.iter().copied());
                }
                _ => {
                    sel.subtract(batch.iter().copied());
                    for c in &batch {
                        reference.remove(c);
                    }
                }
            }
            // 逐格一致
            for gy in 0..h as i32 {
                for gx in 0..w as i32 {
                    assert_eq!(
                        sel.contains(gx, gy),
                        reference.contains(&(gx, gy)),
                        "round {} 格 ({},{}) 不一致",
                        round,
                        gx,
                        gy
                    );
                }
            }
            // 计数与迭代一致
            assert_eq!(
                sel.len() as usize,
                reference.len(),
                "round {} count 不一致",
                round
            );
            let iterated: BTreeSet<_> = sel.iter_cells().collect();
            assert_eq!(iterated, reference, "round {} iter_cells 不一致", round);
            assert_eq!(sel.is_empty(), reference.is_empty());
        }
    }

    /// 越界格：置位静默忽略，`contains` 恒 false。
    #[test]
    fn out_of_bounds_cells_are_ignored_and_read_as_unselected() {
        let map = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map, 4, 4);
        sel.union([(-1, 0), (0, -1), (4, 0), (0, 4), (2, 2)]);
        assert_eq!(sel.len(), 1, "越界格不应计入，got {}", sel.len());
        assert!(sel.contains(2, 2));
        assert!(!sel.contains(-1, 0));
        assert!(!sel.contains(4, 0));
    }

    // ── 生命周期（A7）────────────────────────────────────────────────

    /// **A7 · 切图清空**：`bind_map` 检测到图变 → 选区清空。
    #[test]
    fn bind_map_clears_selection_on_map_change() {
        let map_a = wv_core::Id::new_unchecked("map_a");
        let map_b = wv_core::Id::new_unchecked("map_b");
        let mut sel = CellSelection::default();
        sel.bind_map(&map_a, 8, 8);
        sel.union([(1, 1), (2, 2)]);
        assert_eq!(sel.len(), 2);
        sel.bind_map(&map_b, 8, 8);
        assert!(sel.is_empty(), "换图必须清空选区");
        assert_eq!((sel.width(), sel.height()), (8, 8), "图幅应随新图");
    }

    /// **A7 · 同图保留**：每帧 `bind_map` 同图同尺寸 → 选区不动。
    #[test]
    fn bind_map_keeps_selection_on_same_map() {
        let map_a = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map_a, 8, 8);
        sel.union([(1, 1), (2, 2)]);
        sel.bind_map(&map_a, 8, 8);
        assert_eq!(sel.len(), 2, "同图重复 bind 不得清选区");
        assert!(sel.contains(1, 1) && sel.contains(2, 2));
    }

    /// 图幅变化（同图）→ 重定容并清空（旧位图索引对新尺寸无意义）。
    #[test]
    fn bind_map_clears_selection_on_resize() {
        let map_a = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map_a, 8, 8);
        sel.union([(7, 7)]);
        sel.bind_map(&map_a, 16, 16);
        assert!(sel.is_empty(), "图幅变化必须清空（索引体系已变）");
        assert!(!sel.contains(15, 15) && !sel.contains(0, 0));
        sel.union([(15, 15)]);
        assert!(sel.contains(15, 15), "新图幅下应可写到新边界");
    }

    /// **A7 · Esc 清空**：`clear()` 清选区、留图幅绑定。
    #[test]
    fn clear_empties_selection_but_keeps_binding() {
        let map_a = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map_a, 8, 8);
        sel.union([(1, 1), (2, 2), (3, 3)]);
        sel.clear();
        assert!(sel.is_empty());
        assert_eq!((sel.width(), sel.height()), (8, 8), "clear 不动图幅绑定");
        // 清空后立即可用（Esc 后马上再选）。
        sel.union([(0, 0)]);
        assert!(sel.contains(0, 0));
    }

    // ── 边界边提取（A2）──────────────────────────────────────────────

    fn edge_set(sel: &CellSelection) -> BTreeSet<(EdgeOrientation, i32, i32)> {
        sel.boundary_edges()
            .into_iter()
            .map(|e| (e.orientation, e.gx, e.gy))
            .collect()
    }

    /// **A2 · L 形只出外围边**：内部共享边零产出。
    ///
    /// L 形 = (0,0) (1,0) (0,1)：3 格共 12 条格边，内部共享边 2 条
    /// （(0,0)-(1,0) 之间、(0,0)-(0,1) 之间）各被两格各报一次，
    /// 外围边 = 12 − 2×2 = 8 条。
    #[test]
    fn l_shape_selection_yields_only_outer_boundary_edges() {
        let map = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map, 8, 8);
        sel.union([(0, 0), (1, 0), (0, 1)]);
        let edges = edge_set(&sel);
        assert_eq!(edges.len(), 8, "L 形外围边数应为 8，got {}", edges.len());
        use EdgeOrientation::*;
        let expected: BTreeSet<_> = [
            (Horizontal, 0, 0), // (0,0) 南
            (Horizontal, 1, 0), // (1,0) 南
            (Horizontal, 1, 1), // (1,0) 北
            (Horizontal, 0, 2), // (0,1) 北
            (Vertical, 0, 0),   // (0,0) 西
            (Vertical, 2, 0),   // (1,0) 东
            (Vertical, 0, 1),   // (0,1) 西
            (Vertical, 1, 1),   // (0,1) 东
        ]
        .into_iter()
        .collect();
        assert_eq!(edges, expected, "L 形边界边集合不符");
        // 内部共享边显式缺席：(0,0)-(1,0) 之间的垂直边 (1,0)，
        // (0,0)-(0,1) 之间的水平边 (0,1)。
        assert!(
            !edges.contains(&(Vertical, 1, 0)),
            "(0,0)-(1,0) 内部共享边不得产出"
        );
        assert!(
            !edges.contains(&(Horizontal, 0, 1)),
            "(0,0)-(0,1) 内部共享边不得产出"
        );
    }

    /// **A2 · 空洞内边界**：矩形 Alt 减矩形挖出洞，洞的 4 条内边界边同样产出。
    #[test]
    fn subtracted_hole_produces_inner_boundary_edges() {
        let map = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map, 8, 8);
        // 3×3 方块减中心格 → 环。
        let block: Vec<_> = (0..3).flat_map(|y| (0..3).map(move |x| (x, y))).collect();
        sel.replace(block);
        sel.subtract([(1, 1)]);
        assert_eq!(sel.len(), 8, "环应有 8 格，got {}", sel.len());
        let edges = edge_set(&sel);
        use EdgeOrientation::*;
        // 外周 12 条 + 内孔 4 条 = 16 条。
        assert_eq!(edges.len(), 16, "环的边界边数应为 16，got {}", edges.len());
        for e in [
            (Horizontal, 1, 1), // 孔南边
            (Horizontal, 1, 2), // 孔北边
            (Vertical, 1, 1),   // 孔西边
            (Vertical, 2, 1),   // 孔东边
        ] {
            assert!(edges.contains(&e), "空洞内边界边 {:?} 必须产出", e);
        }
    }

    /// 单格选区 = 4 条边（ADR-0057 D1「单格 = 1 格选区」的边界形态）。
    #[test]
    fn single_cell_selection_yields_its_four_edges() {
        let map = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map, 8, 8);
        sel.replace([(3, 4)]);
        let edges = edge_set(&sel);
        use EdgeOrientation::*;
        let expected: BTreeSet<_> = [
            (Horizontal, 3, 4),
            (Horizontal, 3, 5),
            (Vertical, 3, 4),
            (Vertical, 4, 4),
        ]
        .into_iter()
        .collect();
        assert_eq!(edges, expected);
    }

    /// 贴图幅边的选区：图幅外 = 未选中 → 图幅边沿也算边界。
    #[test]
    fn map_edge_counts_as_boundary() {
        let map = wv_core::Id::new_unchecked("map_a");
        let mut sel = CellSelection::default();
        sel.bind_map(&map, 4, 4);
        sel.replace([(0, 0)]);
        let edges = edge_set(&sel);
        use EdgeOrientation::*;
        assert!(edges.contains(&(Horizontal, 0, 0)), "图幅南沿应出边");
        assert!(edges.contains(&(Vertical, 0, 0)), "图幅西沿应出边");
        assert_eq!(edges.len(), 4);
    }

    // ── 蚂蚁线分段与相位（A3 的纯函数半）────────────────────────────

    /// 段覆盖全长、首尾相接、无重叠；段数 = ceil(length / pitch)。
    #[test]
    fn dash_segments_tile_full_length_without_gaps_or_overlap() {
        let segs = dash_segments(10.0, 3.0);
        assert_eq!(segs.len(), 4, "ceil(10/3)=4，got {}", segs.len());
        assert_eq!(segs[0].0, 0.0);
        assert_eq!(segs[3].1, 10.0, "末段必须收在全长，got {}", segs[3].1);
        for w in segs.windows(2) {
            assert_eq!(w[0].1, w[1].0, "相邻段必须首尾相接");
        }
        // 整除情形：不多产零长段。
        let exact = dash_segments(9.0, 3.0);
        assert_eq!(exact.len(), 3, "整除应得 3 段，got {}", exact.len());
    }

    /// **A3 · 相位互补**：A 实段集合与 B 实段集合不相交、并集 = 全部段。
    #[test]
    fn dash_phases_are_strictly_complementary() {
        let segs = dash_segments(10.0, 3.0);
        let a: Vec<_> = (0..segs.len())
            .filter(|&i| DashPhase::A.is_solid(i))
            .collect();
        let b: Vec<_> = (0..segs.len())
            .filter(|&i| DashPhase::B.is_solid(i))
            .collect();
        assert_eq!(a, vec![0, 2], "A 应取偶数段，got {:?}", a);
        assert_eq!(b, vec![1, 3], "B 应取奇数段，got {:?}", b);
        assert!(a.iter().all(|i| !b.contains(i)), "A/B 不得相交");
        assert_eq!(a.len() + b.len(), segs.len(), "A/B 并集 = 全部段");
    }

    /// 防御：非法入参返回空（不 panic）。
    #[test]
    fn dash_segments_reject_non_positive_inputs() {
        assert!(dash_segments(0.0, 3.0).is_empty());
        assert!(dash_segments(-1.0, 3.0).is_empty());
        assert!(dash_segments(10.0, 0.0).is_empty());
        assert!(dash_segments(10.0, -2.0).is_empty());
    }

    // ── PointSelectState（ADR-0057 补记 6）───────────────────────────

    /// 点选工具激活态独立：关工具只翻 `tool_active`，与 EditCell 互不染指。
    #[test]
    fn point_select_close_tool_only_turns_off_itself() {
        let mut p = PointSelectState { tool_active: true };
        p.close_tool();
        assert!(!p.tool_active);
        // 与 EditCellState 是两个 Resource——类型层面即不可共用，无需断言。
    }
}
