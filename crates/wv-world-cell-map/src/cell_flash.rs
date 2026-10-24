//! 撤销 / 拒收的**短时闪视觉**（[ADR-0052] D8-A 白闪 / D3-E 红闪）。
//!
//! ## 两种闪
//!
//! | 类型 | 触发 | 时长 | 语义 |
//! |---|---|---|---|
//! | [`CellFlashKind::Undo`] | 撤销 / 重做成功（D8-A） | 250ms | 「这几格刚刚变了」的定位信号 |
//! | [`CellFlashKind::Reject`] | 整笔拒收（D3-E） | 200ms | 「这一笔没生效」的红色警示 |
//!
//! ## 为何是「格集合 + 计时」而非一次性事件
//!
//! 闪视觉要**持续 250ms**——一帧的事件脉冲人眼几乎看不见。
//! 故它不是事件而是**会话态**：写入方（`wv-app`）记下「哪些格 + 何时开始」，
//! 渲染方（`wv-render`）每帧按 `elapsed` 算 alpha 并在到期后自行清理。
//!
//! ## crate 分层
//!
//! 本 crate **不依赖 theme**（[ADR-0054] D2/D4）——颜色由 `wv-render` 决定
//! （`UNDO_FLASH` 白 / `ERROR` 红），本 crate 只管「哪些格、亮多久、亮多浓」。
//!
//! [ADR-0052]: ../docs/adr/ADR-0052-cellmap-terrain-workbench.md
//! [ADR-0054]: ../docs/adr/ADR-0054-world-cell-map-crate.md

use bevy::prelude::*;

use crate::cell_selection::build_region_fill_mesh;
use crate::state::GridConfig;

/// 闪视觉类型（决定颜色，由渲染层消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellFlashKind {
    /// 撤销 / 重做（白色脉冲，[ADR-0052] D8-A）。
    #[default]
    Undo,
    /// 整笔拒收（红色，[ADR-0052] D3-E）。
    Reject,
}

/// **白闪时长**（D8-A 原文：250ms 白色脉冲）。
pub const UNDO_FLASH_DURATION: f32 = 0.25;
/// **红闪时长**（D3-E 原文：画面红闪 200ms）。
pub const REJECT_FLASH_DURATION: f32 = 0.20;
/// 闪视觉峰值 alpha（起点最亮，随后平方衰减）。
pub const FLASH_MAX_ALPHA: f32 = 0.85;
/// 单次闪视觉**逐格标记**的格数上限。
///
/// 超过此数则退化为「变化区包围盒的边框环」——一次撤销若推翻了整张地图，
/// 逐格铺 mesh 会炸掉显存，而作者此时需要知道的只是「这一片变了」。
pub const MAX_FLASH_CELLS: usize = 2048;

/// 闪视觉 Z 偏移：**最顶层**（`1.0 > HOVER_Z = 0.95 > SELECTION_Z = 0.9`）。
///
/// 闪是「刚刚发生」的强信号，必须压过所有常驻视觉
/// （[ADR-0052] §9.9「白闪 z 最顶层 250ms 短促」）。
pub const FLASH_Z: f32 = 1.0;

/// **闪视觉会话态**（跨 crate 共享）。
///
/// 写入方：`wv-app`（撤销成功后 / 整笔拒收时）。
/// 读取方：`wv-render`（`sync_cell_flash` 每帧画闪）。
#[derive(Resource, Debug, Default)]
pub struct CellFlashState {
    /// 待闪的格集合（格坐标）。
    pub cells: Vec<(i32, i32)>,
    /// 闪的类型（决定颜色）。
    pub kind: CellFlashKind,
    /// 起始时刻（`Time::elapsed_secs`）。
    pub started_at: f32,
    /// 持续时长（秒）。
    pub duration: f32,
}

impl CellFlashState {
    /// **触发一次闪**。
    ///
    /// `cells` 为空 → 不触发（无格可闪）。
    pub fn trigger(&mut self, kind: CellFlashKind, cells: Vec<(i32, i32)>, now: f32) {
        if cells.is_empty() {
            return;
        }
        self.kind = kind;
        self.cells = cells;
        self.started_at = now;
        self.duration = match kind {
            CellFlashKind::Undo => UNDO_FLASH_DURATION,
            CellFlashKind::Reject => REJECT_FLASH_DURATION,
        };
    }

    /// 是否仍在闪（`now` 在 `[started_at, started_at + duration)` 区间）。
    pub fn is_active(&self, now: f32) -> bool {
        self.duration > 0.0 && now >= self.started_at && now - self.started_at < self.duration
    }

    /// 进度（0 = 刚开始，1 = 结束）。非活跃时为 1。
    pub fn progress(&self, now: f32) -> f32 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        ((now - self.started_at) / self.duration).clamp(0.0, 1.0)
    }

    /// 当前 alpha（峰值 → 平方衰减 → 0）。
    ///
    /// 平方衰减而非线性：白闪要在最初几十毫秒内「亮得明确」，
    /// 线性衰减会让它看起来像「渐隐的残影」而不是「闪了一下」。
    pub fn alpha(&self, now: f32) -> f32 {
        if !self.is_active(now) {
            return 0.0;
        }
        let remain = 1.0 - self.progress(now);
        FLASH_MAX_ALPHA * remain * remain
    }

    /// 清空（不再闪）。
    pub fn clear(&mut self) {
        self.cells.clear();
        self.duration = 0.0;
    }
}

/// 构建闪视觉的覆盖 mesh。
///
/// - 格数 ≤ [`MAX_FLASH_CELLS`] → **逐格填充**（精确指出「哪几格变了」）；
/// - 超过 → **包围盒边框环**（1 格宽的四条带），避免整图撤销炸显存。
///
/// 返回 `(mesh, origin)`——顶点是相对 `origin` 的局部偏移（与
/// `build_region_fill_mesh` 同款坐标链）。
pub fn build_flash_mesh(cells: &[(i32, i32)], grid: &GridConfig) -> Option<(Mesh, Vec2)> {
    if cells.is_empty() {
        return None;
    }
    if cells.len() <= MAX_FLASH_CELLS {
        let (mesh, origin, _bbox) = build_region_fill_mesh(cells, grid)?;
        return Some((mesh, origin));
    }
    // 超限 → 包围盒边框环。
    let (mut gx0, mut gy0) = (i32::MAX, i32::MAX);
    let (mut gx1, mut gy1) = (i32::MIN, i32::MIN);
    for (gx, gy) in cells {
        gx0 = gx0.min(*gx);
        gy0 = gy0.min(*gy);
        gx1 = gx1.max(*gx);
        gy1 = gy1.max(*gy);
    }
    build_ring_mesh(gx0, gy0, gx1, gy1, grid)
}

/// 包围盒的**边框环** mesh（1 格宽的四条带，纯色）。
fn build_ring_mesh(
    gx0: i32,
    gy0: i32,
    gx1: i32,
    gy1: i32,
    grid: &GridConfig,
) -> Option<(Mesh, Vec2)> {
    let cs = grid.cell_size;
    if cs <= 0.0 {
        return None;
    }
    let origin = grid.raster_origin + Vec2::new(gx0 as f32 * cs, gy0 as f32 * cs);
    let w = (gx1 - gx0 + 1) as f32 * cs;
    let h = (gy1 - gy0 + 1) as f32 * cs;
    // 四条带（局部坐标，环宽 = 1 格）。
    let bands = [
        // 上
        (0.0, h - cs, w, h),
        // 下
        (0.0, 0.0, w, cs),
        // 左
        (0.0, 0.0, cs, h),
        // 右
        (w - cs, 0.0, w, h),
    ];
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(bands.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(bands.len() * 6);
    for (x0, y0, x1, y1) in bands {
        let base = positions.len() as u32;
        positions.push([x0, y0, 0.0]);
        positions.push([x1, y0, 0.0]);
        positions.push([x1, y1, 0.0]);
        positions.push([x0, y1, 0.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
    Some((mesh, origin))
}

/// **闪视觉的实体集合**（渲染层持有）。
#[derive(Resource, Default)]
pub struct CellFlashEntities {
    /// 覆盖 mesh 实体。
    pub fill: Option<Entity>,
    /// 覆盖 mesh 句柄。
    pub fill_mesh: Option<Handle<Mesh>>,
    /// 覆盖材质句柄（颜色固定，alpha 每帧改）。
    pub fill_material: Option<Handle<ColorMaterial>>,
    /// 上次构建时的格集合指纹——变了才重建 mesh。
    pub last_fingerprint: u64,
}

impl CellFlashEntities {
    /// 全部 despawn（闪结束 / 无格时调用）。
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
    }

    /// 释放已缓存的 mesh 资产（`Assets` 不会随实体 despawn 回收）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        if let Some(h) = self.fill_mesh.take() {
            meshes.remove(&h);
        }
        self.last_fingerprint = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_selection::cells_fingerprint;
    use crate::state::GridConfig;

    fn grid() -> GridConfig {
        GridConfig {
            cell_size: 8.0,
            raster_origin: Vec2::new(100.0, 200.0),
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        }
    }

    // ── D8-A · 白闪时序 ────────────────────────────────────────────

    #[test]
    fn undo_flash_lasts_250ms_then_stops() {
        let mut s = CellFlashState::default();
        s.trigger(CellFlashKind::Undo, vec![(1, 1)], 10.0);
        assert!(s.is_active(10.0), "刚触发应活跃");
        assert!(s.is_active(10.249), "249ms 仍活跃");
        assert!(!s.is_active(10.25), "250ms 应到期");
        assert!(!s.is_active(11.0), "之后不活跃");
    }

    #[test]
    fn reject_flash_lasts_200ms() {
        let mut s = CellFlashState::default();
        s.trigger(CellFlashKind::Reject, vec![(0, 0)], 0.0);
        assert!(s.is_active(0.199));
        assert!(!s.is_active(0.2));
    }

    #[test]
    fn alpha_starts_at_peak_and_decays_to_zero() {
        let mut s = CellFlashState::default();
        s.trigger(CellFlashKind::Undo, vec![(0, 0)], 0.0);
        assert!((s.alpha(0.0) - FLASH_MAX_ALPHA).abs() < 1e-6, "起点为峰值");
        let mid = s.alpha(0.125);
        assert!(mid > 0.0 && mid < FLASH_MAX_ALPHA, "中途介于 0 与峰值");
        assert!(s.alpha(0.25).abs() < 1e-6, "到期为 0");
    }

    #[test]
    fn empty_cells_do_not_trigger() {
        let mut s = CellFlashState::default();
        s.trigger(CellFlashKind::Undo, vec![], 0.0);
        assert!(!s.is_active(0.0), "无格不闪");
    }

    #[test]
    fn clear_stops_the_flash() {
        let mut s = CellFlashState::default();
        s.trigger(CellFlashKind::Undo, vec![(0, 0)], 0.0);
        s.clear();
        assert!(!s.is_active(0.0));
    }

    // ── mesh 构建 ──────────────────────────────────────────────────

    #[test]
    fn flash_mesh_of_small_set_is_per_cell() {
        let (mesh, origin) = build_flash_mesh(&[(0, 0), (1, 0)], &grid()).expect("应建出");
        // 2 格 = 8 顶点。
        assert_eq!(mesh.count_vertices(), 8);
        assert_eq!(origin, Vec2::new(100.0, 200.0));
    }

    #[test]
    fn flash_mesh_of_huge_set_falls_back_to_ring() {
        let cells: Vec<(i32, i32)> = (0..(MAX_FLASH_CELLS as i32 + 1)).map(|i| (i, 0)).collect();
        let (mesh, _origin) = build_flash_mesh(&cells, &grid()).expect("应建出");
        // 边框环 = 4 条带 = 16 顶点（远小于逐格的 4N）。
        assert_eq!(mesh.count_vertices(), 16);
    }

    #[test]
    fn flash_mesh_of_empty_set_is_none() {
        assert!(build_flash_mesh(&[], &grid()).is_none());
    }

    #[test]
    fn fingerprint_changes_with_cells() {
        assert_ne!(
            cells_fingerprint(&[(0, 0)]),
            cells_fingerprint(&[(0, 1)]),
            "不同格集合指纹应不同"
        );
        assert_eq!(
            cells_fingerprint(&[(0, 0), (1, 1)]),
            cells_fingerprint(&[(0, 0), (1, 1)]),
            "同格集合指纹应稳定"
        );
    }
}
