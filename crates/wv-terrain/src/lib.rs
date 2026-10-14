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
//! | [`edit`] | 写新笔刷（含 [`TerrainGrid`] 字段访问器）时。 |
//!
//! ## 双通道设计（基线 §2.3）
//!
//! 每格存 `(height: i16, terrain: u8)` 双通道，对应 `terrain.bin` 的
//! 「I16 高度 + U8 类型」。合并在同一个网格里，因为二者总是一起被读写。

pub mod edit;
pub mod generate;

/// 地形类型编号。**刻意用 `u8` 而非枚举**。
///
/// 理由与 D2（势力层）同源：地形类型是**开放集**——Theme Pack 可增减地形种类，
/// 硬编码枚举会逼出「用不上但必须在枚举里出现」的噪声变体。
/// 编号 → 名称 → 色值的映射由 Theme Pack 承载，本层只做载体。
pub type TerrainKind = u8;

/// 高度类型。`i16` 足以表达 -32768..32767 的相对海拔档位。
pub type Height = i16;

/// 一格地形。高度 + 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerrainCell {
    pub height: Height,
    pub kind: TerrainKind,
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
            kind: 7,
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
                kind: 9,
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
        let corner = TerrainCell { height: 1, kind: 2 };
        g.set(3, 3, corner);
        assert_eq!(g.sample_uv(1.0, 1.0), Some(corner), "uv=1.0 应落最后一格");
    }

    #[test]
    fn sample_world_respects_origin_and_cell_size() {
        let mut g = TerrainGrid::new(10, 10);
        let cell = TerrainCell { height: 5, kind: 1 };
        g.set(2, 3, cell);
        // origin (100, 200)，cell_size 10 → 格 (2,3) 覆盖 x∈[120,130), y∈[230,240)
        assert_eq!(
            g.sample_world(125.0, 235.0, (100.0, 200.0), 10.0),
            Some(cell)
        );
    }
}
