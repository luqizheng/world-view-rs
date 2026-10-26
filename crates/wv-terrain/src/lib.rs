//! # wv-terrain —— World View 地形层
//!
//! 栅格地形网格：高度场 + 地形类型，以及与之配套的噪声生成与笔刷编辑。
//!
//! ## 分层位置（基线 §2.1）
//!
//! ```text
//! wv-app -> wv-edit -> wv-io -> wv-core
//!             \-----> wv-terrain （与 wv-core 平行，互不依赖）
//! ```
//!
//! - **本 crate 不依赖 `wv-core`，`wv-core` 也不依赖本 crate**（平行关系）。
//! - **零第三方依赖**：Perlin 手写（基线 §2.18），栅格数据自描述。
//! - **不感知 Undo/Redo**：撤销是 `wv-edit` 的事；本层只提供「改像素」的纯函数。
//!
//! ## 两个模块（基线 §2.2）
//!
//! - [`generate`]：从噪声参数**生成**高度场（v0.2 的「生成」职责）；
//! - [`edit`]：按用户笔刷轨迹**编辑**网格（本轮的「编辑」职责）。
//!
//! 两者共享 [`TerrainGrid`]：都操作同一栅格网格，拆两个 crate 会制造循环依赖。
//!
//! ### 模块导航速查
//!
//! | 模块 | 何时打开 |
//! |---|---|
//! | [`generate`] | 写噪声 / lerp / 新的填充策略时。 |
//! | [`hash`] | 写确定性格点哈希（摆位 / 抖动 / 纸纹）时。 |
//! | [`edit`] | 写新笔刷（含 [`TerrainGrid`] 字段访问器）时。 |
//! | [`intent`] | 写「意图笔刷」——按意图生成山脉 / 高原 / 火山 / 湖 / 河时。 |
//! | [`isoline`] | 等值线提取 / 折线平滑抽稀（渲染派生，plan v0.1 M2）时。 |
//! | [`forest`] | 写森林覆盖物生成（适宜度 / 密度场 / 树木采样，codex_forest_design.md）时。 |
//! | [`line_path`] | 写线段 POI 的起终点路径算法（绕行 / 下切，ADR-0070 D3）时。 |
//!
//! ## 三通道设计（设计稿 `2026-10-18-world-from-zero-map-design.md` D11/D12）
//!
//! 每格存 `(height: i16, surface: u8, biome: u8)` 三通道：
//! **surface** = 水/陆二值（authoring 真值，由生成动作写入、不由高度推断）、
//! **height** = 纯海拔、**biome** = 生物群系开放编号。`terrain.bin` 的
//! 「I16 高度 + U8 地表 + U8 群系」与之对应。

pub mod biome;
pub mod cellops;
pub mod climate;
pub mod continent;
pub mod divide;
pub mod edit;
pub mod forest;
pub mod generate;
pub mod hash;
pub mod intent;
pub mod isoline;
pub mod line_path;
pub mod marker;
pub mod region;
pub mod shore;

/// 地表二值通道（authoring 真值）。**刻意用 `u8` 常量而非枚举**：
/// 与栅格二进制逐字节对应，`0` 与全零格（海平面深海）的 `Default` 语义对齐。
///
/// 水/陆由**生成动作**（标记、海生成、笔刷）写入，不由高度推断——
/// 山上的湖（海拔 +10 m）是水，被塑笔刷压到海下的陆格仍是陆
/// （渲染/笔刷的「湿判定」见 [`edit::is_water_cell`]，两口径分层）。
pub type Surface = u8;
/// 水面。
pub const SURFACE_WATER: Surface = 0;
/// 陆地。
pub const SURFACE_LAND: Surface = 1;

pub use biome::*;

/// 高度类型。`i16` 足以表达 ±32767 m，覆盖地球极值（plan v0.1 §8 Q1）。
///
/// **单位 = 米**，**海平面 = 0**，负值即为水下。Q1 锚点定下后该契约冻结；
/// `wv-terrain` 不感知定标层，**所有写入/读取均按 1 单位 = 1 米**解释。
pub type Height = i16;

/// 一格地形。海拔 + 地表二值 + 生物群系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainCell {
    pub height: Height,
    pub surface: Surface,
    pub biome: Biome,
}

/// 栅格地形网格。本 crate 的核心数据结构。
///
/// 存储为**行优先的平坦 `Vec`**：`index = y * width + x`。
/// 行优先而非列优先，是为了让「按行扫描的纹理上传」与「按行脏块 patch」都连续。
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainGrid {
    width: u32,
    height: u32,
    cells: Vec<TerrainCell>,
}

impl TerrainGrid {
    /// 用给定尺寸建一张全零（海拔 0、类型 0）的网格。
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cells: vec![TerrainCell::default(); (width as usize) * (height as usize)],
        }
    }

    /// 从已有的单元格向量重建网格。
    ///
    /// 返回 `None` 表示 `cells.len() != width * height`（数据不一致）。
    /// 加载外部数据时用这个入口，避免构造出「尺寸与内容不符」的网格。
    pub fn from_cells(width: u32, height: u32, cells: Vec<TerrainCell>) -> Option<Self> {
        if cells.len() != (width as usize) * (height as usize) {
            return None;
        }
        Some(Self {
            width,
            height,
            cells,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// 单元格总数。用于性能门禁（基线 §2.19：> 16,777,216 → 拒绝加载）。
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 全部单元格的只读视图。
    pub fn cells(&self) -> &[TerrainCell] {
        &self.cells
    }

    /// 消耗自身，取出单元格向量用于序列化。
    pub fn into_cells(self) -> Vec<TerrainCell> {
        self.cells
    }

    /// 坐标是否在网格内。
    pub fn contains_xy(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
    }

    /// 读一格。越界返回 `None`（而不是 panic）——笔刷会扫过边缘。
    pub fn get(&self, x: i32, y: i32) -> Option<TerrainCell> {
        if !self.contains_xy(x, y) {
            return None;
        }
        Some(self.cells[(y as u32 * self.width + x as u32) as usize])
    }

    /// 写一格。越界静默忽略（笔刷扫过边缘时常见的合法情形）。
    pub fn set(&mut self, x: i32, y: i32, cell: TerrainCell) {
        if !self.contains_xy(x, y) {
            return;
        }
        self.cells[(y as u32 * self.width + x as u32) as usize] = cell;
    }

    /// 用世界坐标（图内局部坐标）采样。
    ///
    /// `cell_size` 是每格的物理边长；`origin` 是网格左上角在图内局部坐标中的位置。
    /// 坐标变换集中在这里，禁止各处重复实现（基线 §2.15）。
    pub fn sample_world(
        &self,
        x: f32,
        y: f32,
        origin: (f32, f32),
        cell_size: f32,
    ) -> Option<TerrainCell> {
        if cell_size <= 0.0 {
            return None;
        }
        let gx = ((x - origin.0) / cell_size).floor() as i32;
        let gy = ((y - origin.1) / cell_size).floor() as i32;
        self.get(gx, gy)
    }

    /// 按 0..=1 的归一化坐标（UV）采样。栅格渲染与笔刷拾取共用。
    pub fn sample_uv(&self, u: f32, v: f32) -> Option<TerrainCell> {
        let gx = (u * self.width as f32).floor() as i32;
        let gy = (v * self.height as f32).floor() as i32;
        // u/v 恰好为 1.0 时钳制到最后一格，避免边界落空。
        let gx = gx.min(self.width as i32 - 1);
        let gy = gy.min(self.height as i32 - 1);
        self.get(gx, gy)
    }

    /// 按给定分位求 `height` 通道分位值（米）。
    ///
    /// 给 Topographic viewmode（[ADR-0039]）用：抽 33% / 66% 两个阈值，
    /// 把陆格分三档着色。**只读 `height`**，biome / surface 不参与。
    /// 全零高度格（默认网格）两个分位都返 `0`。
    ///
    /// 排序 O(N log N)；对 1024² ≈ 1 ms 量级，大图调用点
    /// 缓存结果（`ViewState::topographic_thresholds`，[ADR-0039] 决策 5）。
    ///
    /// [ADR-0039]: ../docs/adr/ADR-0039-topographic-viewmode.md
    pub fn compute_height_percentiles(&self, p_low: f32, p_high: f32) -> (i16, i16) {
        assert!(
            (0.0..=1.0).contains(&p_low) && (0.0..=1.0).contains(&p_high) && p_low <= p_high,
            "p_low ({p_low}) 和 p_high ({p_high}) 必须在 [0, 1] 且 p_low ≤ p_high"
        );
        if self.cells.is_empty() {
            return (0, 0);
        }
        let mut heights: Vec<i16> = self.cells.iter().map(|c| c.height).collect();
        heights.sort_unstable();
        let len = heights.len();
        let idx_low = ((len as f32 * p_low).floor() as usize).min(len - 1);
        let idx_high = ((len as f32 * p_high).floor() as usize).min(len - 1);
        (heights[idx_low], heights[idx_high])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grid_is_zero_filled() {
        let g = TerrainGrid::new(4, 3);
        assert_eq!(g.width(), 4);
        assert_eq!(g.height(), 3);
        assert_eq!(g.cell_count(), 12);
        assert_eq!(g.get(0, 0), Some(TerrainCell::default()));
    }

    #[test]
    fn row_major_indexing_is_consistent() {
        let mut g = TerrainGrid::new(4, 3);
        let cell = TerrainCell {
            height: 42,
            surface: SURFACE_LAND,
            biome: BIOME_NONE,
        };
        g.set(2, 1, cell);
        // 行优先：2 + 1*4 = 6
        assert_eq!(g.cells()[6], cell);
        assert_eq!(g.get(2, 1), Some(cell));
    }

    #[test]
    fn out_of_bounds_access_is_safe() {
        let mut g = TerrainGrid::new(2, 2);
        assert_eq!(g.get(-1, 0), None);
        assert_eq!(g.get(0, -1), None);
        assert_eq!(g.get(2, 0), None);
        assert_eq!(g.get(0, 2), None);
        // set 越界不 panic、不改动
        g.set(
            -5,
            -5,
            TerrainCell {
                height: 99,
                surface: SURFACE_LAND,
                biome: BIOME_NONE,
            },
        );
        assert_eq!(g.get(0, 0), Some(TerrainCell::default()));
    }

    #[test]
    fn from_cells_rejects_size_mismatch() {
        let cells = vec![TerrainCell::default(); 5];
        assert!(TerrainGrid::from_cells(2, 2, cells).is_none());
        let cells = vec![TerrainCell::default(); 4];
        assert!(TerrainGrid::from_cells(2, 2, cells).is_some());
    }

    #[test]
    fn sample_uv_clamps_at_far_edge() {
        let mut g = TerrainGrid::new(4, 4);
        let corner = TerrainCell {
            height: 1,
            surface: SURFACE_LAND,
            biome: BIOME_DESERT,
        };
        g.set(3, 3, corner);
        assert_eq!(g.sample_uv(1.0, 1.0), Some(corner), "uv=1.0 应落最后一格");
    }

    #[test]
    fn sample_world_respects_origin_and_cell_size() {
        let mut g = TerrainGrid::new(10, 10);
        let cell = TerrainCell {
            height: 5,
            surface: SURFACE_WATER,
            biome: BIOME_NONE,
        };
        g.set(2, 3, cell);
        // origin (100, 200)，cell_size 10 → 格 (2,3) 覆盖 x∈[120,130), y∈[230,240)
        assert_eq!(
            g.sample_world(125.0, 235.0, (100.0, 200.0), 10.0),
            Some(cell)
        );
    }

    /// `compute_height_percentiles` 在 100 格有序高度场上正确切分
    /// （3 阶切分用的 33% / 66% 阈值，[ADR-0039] 决策 4）。
    #[test]
    fn compute_height_percentiles_on_sorted_grid() {
        let mut g = TerrainGrid::new(10, 10);
        for (i, h) in (0i16..100).enumerate() {
            let cell = TerrainCell {
                height: h,
                surface: SURFACE_LAND,
                biome: BIOME_NONE,
            };
            g.set((i % 10) as i32, (i / 10) as i32, cell);
        }
        // 100 格按 sort_unstable 后下标 [floor(100*0.33)=33]=33, [floor(100*0.66)=66]=66
        assert_eq!(g.compute_height_percentiles(0.33, 0.66), (33, 66));
    }

    /// `compute_height_percentiles` 在空网格上不 panic、返 `(0, 0)`。
    /// 0×0 与全零（非空但 height 都为 0）两种 fixture 都覆盖。
    #[test]
    fn compute_height_percentiles_handles_zero_cells() {
        // 真正的 0×0
        let g = TerrainGrid::new(0, 0);
        assert_eq!(g.compute_height_percentiles(0.33, 0.66), (0, 0));
        // 全零非空网格
        let g = TerrainGrid::new(4, 4);
        assert_eq!(g.compute_height_percentiles(0.33, 0.66), (0, 0));
    }

    /// `compute_height_percentiles` 严守分位参数边界（参数非法则 panic）。
    #[test]
    #[should_panic(expected = "p_low")]
    fn compute_height_percentiles_rejects_out_of_range_p_low() {
        let g = TerrainGrid::new(4, 4);
        let _ = g.compute_height_percentiles(1.5, 0.66);
    }
}
