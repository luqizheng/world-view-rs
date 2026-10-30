//! 群系吸附索引（[ADR-0090] D5/M4）：切蛋糕「贴近群系」的几何内核。
//!
//! 国境大多随群系/地貌走（ADR-0090 D4 同源理由）：切刀一笔的采样折线经
//! 本模块逐点吸附到**群系变化锋线**上，切出的脸边界贴自然地貌。不勾
//! （「贴近群系」checkbox 关）= 自由切，笔画原样提交。
//!
//! ## 为什么是「锋线边掩码 + 共享边投影」而不是显式折线提取
//! 锋线在格数据上天然是**阶梯**（cell staircase）；构建时逐格标出「哪条边
//! 与异群系（或图外）相接」，吸附时把笔画点投影到这些边段上取最近点——
//! 半格精度的连续点位，且不需要锋线的连通序（一笔切分只要求每个采样点
//! 各自落在锋线上）。
//!
//! 纯数据 + 纯函数（零 Bevy）；构建 O(格数)，一笔切分构建一次（手势在
//! 落笔时建、整笔复用），预览帧零重建。

/// 边掩码位：该格的哪条边是锋线（与异群系/图外相接）。
mod edge {
    pub const WEST: u8 = 1;
    pub const EAST: u8 = 1 << 1;
    pub const NORTH: u8 = 1 << 2;
    pub const SOUTH: u8 = 1 << 3;
}

/// 群系锋线索引。
pub struct BiomeSnapIndex {
    width: u32,
    height: u32,
    /// 逐格锋线边掩码（[`edge`] 位域）。
    frontier: Vec<u8>,
}

impl BiomeSnapIndex {
    /// 从地形数据构建（O(格数)；一笔一次）。图外邻居按**异群系**处理
    /// ——沿海 / 沿图边都是锋线（切刀常沿轮廓走）。
    pub fn build(data: &wv_core::terrain::TerrainData) -> Self {
        let w = data.width;
        let h = data.height;
        let biome_at = |x: i64, y: i64| -> Option<u8> {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                return None;
            }
            Some(data.cells[(y as u32 * w + x as u32) as usize].biome)
        };
        let mut frontier = vec![0u8; (w as usize) * (h as usize)];
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let b = biome_at(x, y).expect("坐标已在界内");
                let mut mask = 0u8;
                let differs = |o: Option<u8>| o.is_none_or(|ob| ob != b);
                if differs(biome_at(x - 1, y)) {
                    mask |= edge::WEST;
                }
                if differs(biome_at(x + 1, y)) {
                    mask |= edge::EAST;
                }
                if differs(biome_at(x, y - 1)) {
                    mask |= edge::NORTH;
                }
                if differs(biome_at(x, y + 1)) {
                    mask |= edge::SOUTH;
                }
                frontier[(y as u32 * w + x as u32) as usize] = mask;
            }
        }
        Self {
            width: w,
            height: h,
            frontier,
        }
    }

    /// 单点吸附：把 `p`（图内局部 wu）拉到半径 `radius_cells` 内最近的
    /// 锋线边段上（最近点投影，半格精度）；半径内无锋线 → `None`
    /// （调用方保留原点——均匀区内不强扭笔画）。
    pub fn snap_point(&self, p: [f32; 2], cell_size: f32, radius_cells: f32) -> Option<[f32; 2]> {
        let cs = cell_size;
        if cs <= 0.0 || self.width == 0 || self.height == 0 {
            return None;
        }
        let cx = (p[0] / cs).floor() as i64;
        let cy = (p[1] / cs).floor() as i64;
        let r = radius_cells.ceil() as i64;
        let mask_at = |x: i64, y: i64| -> u8 {
            if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
                return 0;
            }
            self.frontier[(y as u32 * self.width + x as u32) as usize]
        };
        let mut best: Option<(f32, [f32; 2])> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let (bx, by) = (cx + dx, cy + dy);
                let mask = mask_at(bx, by);
                if mask == 0 {
                    continue;
                }
                // 该格的四条锋线边段（构建时已判定与异群系/图外相接）。
                let cand = [
                    (
                        edge::WEST,
                        [bx as f32 * cs, by as f32 * cs],
                        [bx as f32 * cs, (by + 1) as f32 * cs],
                    ),
                    (
                        edge::EAST,
                        [(bx + 1) as f32 * cs, by as f32 * cs],
                        [(bx + 1) as f32 * cs, (by + 1) as f32 * cs],
                    ),
                    (
                        edge::NORTH,
                        [bx as f32 * cs, by as f32 * cs],
                        [(bx + 1) as f32 * cs, by as f32 * cs],
                    ),
                    (
                        edge::SOUTH,
                        [bx as f32 * cs, (by + 1) as f32 * cs],
                        [(bx + 1) as f32 * cs, (by + 1) as f32 * cs],
                    ),
                ];
                for (bit, a, b) in cand {
                    if mask & bit == 0 {
                        continue;
                    }
                    let q = closest_on_segment(p, a, b);
                    let d = dist2(p, q);
                    // 真半径截断（盒搜索是粗筛）：超过吸附半径的锋线不认
                    // ——否则「半径」名不副实，远处锋线把笔画硬拽走。
                    if d > radius_cells * radius_cells * cs * cs {
                        continue;
                    }
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, q));
                    }
                }
            }
        }
        best.map(|(_, q)| q)
    }

    /// 整条折线逐点吸附（扁平 `[x0,y0,…]`；半径内无锋线的点保持原位）。
    pub fn snap_polyline(&self, pts: &[f32], cell_size: f32, radius_cells: f32) -> Vec<f32> {
        pts.as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                self.snap_point([c[0], c[1]], cell_size, radius_cells)
                    .unwrap_or([c[0], c[1]])
            })
            .flat_map(|q| [q[0], q[1]])
            .collect()
    }
}

/// 点到线段最近点（垂足钳制）。
fn closest_on_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    if len2 <= f32::EPSILON {
        return a;
    }
    let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0);
    [a[0] + ab[0] * t, a[1] + ab[1] * t]
}

fn dist2(p: [f32; 2], q: [f32; 2]) -> f32 {
    let d = [p[0] - q[0], p[1] - q[1]];
    d[0] * d[0] + d[1] * d[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(b: u8) -> wv_core::terrain::TerrainCellData {
        wv_core::terrain::TerrainCellData {
            height: 0,
            surface: 0,
            biome: b,
        }
    }

    /// 3×1 地形：`[1,2,2]` → 唯一群系锋线 = x=cs 的竖边（图缘也算锋线，
    /// 但都在 2 格开外——本测试半径 ≤1 格内只有中缝一条候选）。
    fn three_wide() -> wv_core::terrain::TerrainData {
        wv_core::terrain::TerrainData {
            width: 3,
            height: 1,
            cells: vec![cell(1), cell(2), cell(2)],
        }
    }

    /// 锋线附近的点吸附到中缝边上（y 钳在段内）；均匀区内不吸附。
    #[test]
    fn snap_pulls_points_to_the_biome_front_and_leaves_uniform_areas() {
        let idx = BiomeSnapIndex::build(&three_wide());
        let cs = 10.0;
        // 点 (15, 5)：中缝右侧贴缝 → 吸附到 x=10 的竖边段（y ∈ 0..10 不钳）。
        let q = idx.snap_point([15.0, 5.0], cs, 1.0).expect("锋线在半径内");
        assert!(
            (q[0] - 10.0).abs() < 1e-3 && (0.0..=10.0).contains(&q[1]),
            "应落在中缝竖边 x=10，实际 {q:?}"
        );
        // 均匀地形（全 biome 1，7×5）：中心点距图缘锋线 ≥ 2 格 > 半径
        // → 不吸附（图缘按约定是锋线，真半径截断保证够不着就不认）。
        let flat = wv_core::terrain::TerrainData {
            width: 7,
            height: 5,
            cells: vec![cell(1); 35],
        };
        let idx0 = BiomeSnapIndex::build(&flat);
        assert!(
            idx0.snap_point([35.0, 25.0], cs, 1.0).is_none(),
            "半径内无锋线应返回 None（原点保留）"
        );
    }

    /// 折线接口逐点 1:1 保序：锋线上的点原样、锋线外的点吸附。
    #[test]
    fn polyline_snaps_pointwise_and_keeps_order() {
        let idx = BiomeSnapIndex::build(&three_wide());
        let out = idx.snap_polyline(&[10.0, 5.0, 15.0, 5.0], 10.0, 1.0);
        assert_eq!(out.len(), 4, "逐点 1:1，长度不变");
        assert_eq!([out[0], out[1]], [10.0, 5.0], "锋线上的点原样");
        assert_eq!([out[2], out[3]], [10.0, 5.0], "锋线外的点吸附到锋线");
    }
}
