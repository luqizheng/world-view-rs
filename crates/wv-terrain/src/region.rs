//! 自然区域的几何内核（ADR-0045 D1 / D5）——biome 连通区切分与区域合并。
//!
//! ## 职责
//! - [`partition_by_biome`]：把活动栅格按 **biome 连通分量**切成若干平铺区
//!   （每格恰属一区，相邻区共边由构造保证）；
//! - [`union_regions`]：两个环组的**几何并集**（合并自然区，ADR-0045 D5）。
//!
//! ## 边界（不做什么）
//! - **不感知实体**：本模块只出格坐标几何；`Region` 实体（名字 / 类别 / 父区 /
//!   覆盖物键）归 `wv-core`，命令层归 `wv-edit`；
//! - **不落盘**：分区是**派生视图**（ADR-0045 D4——命名即物化，未命名的候选
//!   区域不产生 YAML）；
//! - **零第三方依赖**（与本 crate 其余部分同纪律）。
//!
//! ## 为什么用掩膜取环而不是 `isoline::kind_segments`
//!
//! `kind_segments` 只在**异类格共享边**出线，**地图外边界无线**——贴图边的
//! 连通区会得到开链（ADR-0045 D1 记的坑）。掩膜法把「网格外」也当作异类，
//! 边界沿图幅矩形闭合，贴图边的区同样是闭合环。

use crate::divide::{mask_boundary_rings, rasterize_ring, split_outers_and_holes};
use crate::TerrainGrid;

/// 环组：外环 + 洞环（**格坐标**，格 (x,y) 覆盖 `[x,x+1)²`，格心 `x+0.5`）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RingSet {
    /// 外环（扁平点列，首尾不重复）。
    pub outer: Vec<(f32, f32)>,
    /// 洞环列表（外环内部的空洞，如「林中湖」）。
    pub holes: Vec<Vec<(f32, f32)>>,
}

impl RingSet {
    /// 是否空（无外环）。
    pub fn is_empty(&self) -> bool {
        self.outer.len() < 3
    }
}

/// 一个 biome 连通区（ADR-0045 D1 的分区单元）。
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeRegion {
    /// 该区所有格的**群系编号**（同区同群系——这是分区依据）。
    pub biome: u8,
    /// 外环（格坐标）。
    pub outer: Vec<(f32, f32)>,
    /// 洞环（格坐标）。
    pub holes: Vec<Vec<(f32, f32)>>,
    /// 该区包含的格数（供过滤碎块 / UI 展示）。
    pub cells: usize,
}

/// 区域几何操作失败原因（V1：调用方给提示，不 panic）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionError {
    /// 两区不相邻——并集得到多块外环，v1 不支持（ADR-0028 D2 的 `Fragmented` 同款）。
    Disjoint,
    /// 输入环退化（点数不足 3）。
    Degenerate,
    /// 网格尺寸为零。
    EmptyGrid,
}

/// 按 biome 连通区切分栅格（ADR-0045 D1）。
///
/// **平铺与共边由构造保证**：biome 是全函数，每格恰属一个连通分量；相邻区
/// 的边界来自同一段异类格边（4 邻域，对角相触算两块）→ 共边无缝。
///
/// **确定性**：无 seed 依赖——同栅格必得同分区（重生成后分区逐格一致，
/// ADR-0037 M4 的重生成契约要求）。
///
/// **复杂度**：O(格数)——单次扫描 + 一次 BFS；掩膜缓冲复用，不按连通块重复分配。
pub fn partition_by_biome(grid: &TerrainGrid) -> Vec<BiomeRegion> {
    let (w, h) = (grid.width(), grid.height());
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let cells = grid.cells();
    let total = (w as usize) * (h as usize);
    let mut visited = vec![false; total];
    let mut mask = vec![false; total];
    let mut queue: Vec<u32> = Vec::new();
    let mut component: Vec<u32> = Vec::new();
    let mut out: Vec<BiomeRegion> = Vec::new();

    for start in 0..total {
        if visited[start] {
            continue;
        }
        let biome = cells[start].biome;
        // ---- BFS：4 邻域同群系连通块 ----
        queue.clear();
        component.clear();
        queue.push(start as u32);
        visited[start] = true;
        while let Some(idx) = queue.pop() {
            component.push(idx);
            let x = idx % w;
            let y = idx / w;
            // 左 / 右 / 上 / 下（对角不算：仅角相触的两块在几何上只交于一点）
            if x > 0 {
                push_same(cells, w, &mut visited, &mut queue, (x - 1, y), biome);
            }
            if x + 1 < w {
                push_same(cells, w, &mut visited, &mut queue, (x + 1, y), biome);
            }
            if y > 0 {
                push_same(cells, w, &mut visited, &mut queue, (x, y - 1), biome);
            }
            if y + 1 < h {
                push_same(cells, w, &mut visited, &mut queue, (x, y + 1), biome);
            }
        }
        // ---- 掩膜 → 环 ----
        for &idx in &component {
            mask[idx as usize] = true;
        }
        if let Some(rs) = ring_set_from_mask(&mask, w, h) {
            out.push(BiomeRegion {
                biome,
                outer: rs.outer,
                holes: rs.holes,
                cells: component.len(),
            });
        }
        for &idx in &component {
            mask[idx as usize] = false;
        }
    }
    out
}

/// 邻居同群系且未访问 → 入队。
fn push_same(
    cells: &[crate::TerrainCell],
    w: u32,
    visited: &mut [bool],
    queue: &mut Vec<u32>,
    (x, y): (u32, u32),
    biome: u8,
) {
    let idx = (y * w + x) as usize;
    if visited[idx] || cells[idx].biome != biome {
        return;
    }
    visited[idx] = true;
    queue.push(idx as u32);
}

/// 掩膜 → 环组（外环 + 该外环的洞）。
///
/// 取**面积最大的外环**（分区的连通块理论上只有一个外环；取最大是稳妥兜底，
/// 与 `marker::rings_of` 同款纪律）。
pub fn ring_set_from_mask(mask: &[bool], w: u32, h: u32) -> Option<RingSet> {
    let rings = mask_boundary_rings(mask, w, h);
    if rings.is_empty() {
        return None;
    }
    let (outers, grouped) = split_outers_and_holes(rings);
    let (idx, _) = outers.iter().enumerate().max_by(|a, b| {
        ring_area(a.1)
            .partial_cmp(&ring_area(b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    Some(RingSet {
        outer: outers[idx].clone(),
        holes: grouped.get(idx).cloned().unwrap_or_default(),
    })
}

/// 某格所在的 **biome 连通区**（外环 + 洞，格坐标）。
///
/// 自动分区候选的**按格查询**入口（ADR-0045 D4：作者点中一个候选区 → 命名 →
/// 物化）。走的是与 [`partition_by_biome`] **同一套**连通块判定（4 邻域同群系），
/// 因此查到的区与分区列表里的区逐格一致。
///
/// 格子越界 / 该格无群系 → `None`。
pub fn biome_region_at(grid: &TerrainGrid, gx: i32, gy: i64) -> Option<BiomeRegion> {
    let cell = grid.get(gx, gy as i32)?;
    let biome = cell.biome;
    let (w, h) = (grid.width(), grid.height());
    let mask = component_mask(grid, gx, gy as i32, biome);
    let rs = ring_set_from_mask(&mask, w, h)?;
    Some(BiomeRegion {
        biome,
        outer: rs.outer,
        holes: rs.holes,
        cells: mask.iter().filter(|m| **m).count(),
    })
}

/// 从起点出发的同群系连通块掩膜（4 邻域）。
fn component_mask(grid: &TerrainGrid, gx: i32, gy: i32, biome: u8) -> Vec<bool> {
    let (w, h) = (grid.width(), grid.height());
    let cells = grid.cells();
    let mut mask = vec![false; cells.len()];
    if w == 0 || h == 0 {
        return mask;
    }
    let Some(start) = grid.get(gx, gy) else {
        return mask;
    };
    if start.biome != biome {
        return mask;
    }
    let mut queue: Vec<u32> = vec![(gy as u32 * w + gx as u32)];
    mask[queue[0] as usize] = true;
    while let Some(idx) = queue.pop() {
        let x = idx % w;
        let y = idx / w;
        let try_push = |nx: u32, ny: u32, mask: &mut Vec<bool>, queue: &mut Vec<u32>| {
            if nx >= w || ny >= h {
                return;
            }
            let n = (ny * w + nx) as usize;
            if mask[n] || cells[n].biome != biome {
                return;
            }
            mask[n] = true;
            queue.push(n as u32);
        };
        if x > 0 {
            try_push(x - 1, y, &mut mask, &mut queue);
        }
        if x + 1 < w {
            try_push(x + 1, y, &mut mask, &mut queue);
        }
        if y > 0 {
            try_push(x, y - 1, &mut mask, &mut queue);
        }
        if y + 1 < h {
            try_push(x, y + 1, &mut mask, &mut queue);
        }
    }
    mask
}

/// 识别：掩膜内的群系连通块 → 新边界（ADR-0045 D6 重生成用；语义同 ADR-0029 D3
/// 的「识别」分支）。
///
/// - 掩膜内**占多数**的群系 = 目标群系；
/// - 目标群系格经 4 邻域 BFS 取连通块（**不限掩膜内**——连通块可能长出旧边界，
///   这正是「重生成」的语义：地形改了，区域跟着长）；
/// - 连通块掩膜 → 环组。
///
/// 掩膜内没有格子 → `None`（调用方给提示，不 panic，V1）。
pub fn identify_region(grid: &TerrainGrid, mask: &[bool]) -> Option<RingSet> {
    let (w, h) = (grid.width(), grid.height());
    if w == 0 || h == 0 {
        return None;
    }
    let cells = grid.cells();
    // 多数群系（掩膜内）。
    let mut counts: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
    for (i, &inside) in mask.iter().enumerate() {
        if !inside || i >= cells.len() {
            continue;
        }
        *counts.entry(cells[i].biome).or_insert(0) += 1;
    }
    let biome = counts.into_iter().max_by_key(|(_, c)| *c).map(|(b, _)| b)?;

    // 连通块（4 邻域，同群系）。
    let mut comp = vec![false; cells.len()];
    let mut queue: Vec<u32> = Vec::new();
    for i in 0..cells.len() {
        if mask.get(i).copied().unwrap_or(false) && cells[i].biome == biome {
            comp[i] = true;
            queue.push(i as u32);
        }
    }
    while let Some(idx) = queue.pop() {
        let x = idx % w;
        let y = idx / w;
        let try_push = |nx: u32, ny: u32, comp: &mut Vec<bool>, queue: &mut Vec<u32>| {
            if nx >= w || ny >= h {
                return;
            }
            let n = (ny * w + nx) as usize;
            if comp[n] || cells[n].biome != biome {
                return;
            }
            comp[n] = true;
            queue.push(n as u32);
        };
        if x > 0 {
            try_push(x - 1, y, &mut comp, &mut queue);
        }
        if x + 1 < w {
            try_push(x + 1, y, &mut comp, &mut queue);
        }
        if y > 0 {
            try_push(x, y - 1, &mut comp, &mut queue);
        }
        if y + 1 < h {
            try_push(x, y + 1, &mut comp, &mut queue);
        }
    }
    ring_set_from_mask(&comp, w, h)
}

/// 环组 → 掩膜（外环栅格化后**减去**洞）。
pub fn mask_of_ring_set(rs: &RingSet, w: u32, h: u32) -> Vec<bool> {
    let mut mask = rasterize_ring(w, h, &rs.outer);
    for hole in &rs.holes {
        let hm = rasterize_ring(w, h, hole);
        for (i, v) in hm.iter().enumerate() {
            if *v {
                mask[i] = false;
            }
        }
    }
    mask
}

/// 两个环组的**几何并集**（ADR-0045 D5 合并）。
///
/// - 掩膜并 → 重新取环：并集边界由构造得出，**共边自动消失**（内部边不再是
///   边界），无需 ADR-0028 那样的弧段拼接；
/// - 两区**不相邻**时并集得到多块外环 → [`RegionError::Disjoint`]（v1 不支持，
///   同 ADR-0028 D2 的 `Fragmented` 处置：调用方给作者提示）。
pub fn union_regions(a: &RingSet, b: &RingSet, w: u32, h: u32) -> Result<RingSet, RegionError> {
    if w == 0 || h == 0 {
        return Err(RegionError::EmptyGrid);
    }
    if a.is_empty() || b.is_empty() {
        return Err(RegionError::Degenerate);
    }
    let ma = mask_of_ring_set(a, w, h);
    let mb = mask_of_ring_set(b, w, h);
    let mut union = vec![false; (w as usize) * (h as usize)];
    for (i, u) in union.iter_mut().enumerate() {
        *u = ma[i] || mb[i];
    }
    let rings = mask_boundary_rings(&union, w, h);
    let (outers, grouped) = split_outers_and_holes(rings);
    if outers.len() != 1 {
        return Err(RegionError::Disjoint);
    }
    Ok(RingSet {
        outer: outers.into_iter().next().unwrap_or_default(),
        holes: grouped.into_iter().next().unwrap_or_default(),
    })
}

/// 环的有向面积（绝对值）。
fn ring_area(ring: &[(f32, f32)]) -> f32 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0f32;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        a += x0 * y1 - x1 * y0;
    }
    (a / 2.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TerrainCell, SURFACE_LAND};

    /// 按行优先的群系表建栅格（陆地）。
    fn grid_of(w: u32, h: u32, biomes: &[u8]) -> TerrainGrid {
        assert_eq!(biomes.len(), (w * h) as usize, "群系表长度应等于格数");
        let cells = biomes
            .iter()
            .map(|&biome| TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome,
            })
            .collect();
        TerrainGrid::from_cells(w, h, cells).expect("尺寸与内容一致")
    }

    fn bbox(ring: &[(f32, f32)]) -> (f32, f32, f32, f32) {
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for &(x, y) in ring {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        (min_x, min_y, max_x, max_y)
    }

    #[test]
    fn partition_splits_two_biomes_into_two_regions() {
        // 4×2：左两列群系 1，右两列群系 2。
        let g = grid_of(4, 2, &[1, 1, 2, 2, 1, 1, 2, 2]);
        let parts = partition_by_biome(&g);
        assert_eq!(parts.len(), 2, "两个群系应得两个区");
        let a = parts.iter().find(|p| p.biome == 1).expect("群系 1 的区");
        let b = parts.iter().find(|p| p.biome == 2).expect("群系 2 的区");
        assert_eq!(a.cells, 4, "群系 1 占 4 格");
        assert_eq!(b.cells, 4, "群系 2 占 4 格");
    }

    #[test]
    fn partition_keeps_disconnected_same_biome_as_two_regions() {
        // 3×1：中间是别的群系 → 左右两块同群系不相连 = 两区。
        let g = grid_of(3, 1, &[1, 2, 1]);
        let parts = partition_by_biome(&g);
        assert_eq!(parts.len(), 3, "两块孤立的群系 1 各成一区");
        assert_eq!(parts.iter().filter(|p| p.biome == 1).count(), 2);
    }

    #[test]
    fn partition_region_ring_bounds_match_its_cells() {
        // 4×2 全同群系 → 一个区，环的外接盒覆盖全部格（平滑内缩 < 1 格）。
        let g = grid_of(4, 2, &[1; 8]);
        let parts = partition_by_biome(&g);
        assert_eq!(parts.len(), 1);
        let (min_x, min_y, max_x, max_y) = bbox(&parts[0].outer);
        assert!((-0.01..=0.6).contains(&min_x), "左界应贴近 0，实得 {min_x}");
        assert!((-0.01..=0.6).contains(&min_y), "上界应贴近 0，实得 {min_y}");
        assert!((max_x - 4.0).abs() <= 0.6, "右界应贴近 4，实得 {max_x}");
        assert!((max_y - 2.0).abs() <= 0.6, "下界应贴近 2，实得 {max_y}");
    }

    #[test]
    fn partition_region_touching_map_border_still_closes_its_ring() {
        // 贴图边的区必须闭合（这是不走 kind_segments 的原因）。
        let g = grid_of(2, 2, &[1; 4]);
        let parts = partition_by_biome(&g);
        assert_eq!(parts.len(), 1);
        let ring = &parts[0].outer;
        assert!(
            ring.len() >= 4,
            "贴图边的区也应成环，实得 {} 点",
            ring.len()
        );
        // 环沿图幅边界走：左界贴 0、右界贴 2。
        let (min_x, _, max_x, _) = bbox(ring);
        assert!(min_x <= 0.6, "左界应贴图幅左缘，实得 {min_x}");
        assert!((max_x - 2.0).abs() <= 0.6, "右界应贴图幅右缘，实得 {max_x}");
    }

    #[test]
    fn partition_region_with_lake_inside_reports_a_hole() {
        // 5×5：中心是群系 2（湖），其余群系 1（林）→ 林区带一个洞。
        let mut biomes = vec![1u8; 25];
        biomes[2 * 5 + 2] = 2;
        let g = grid_of(5, 5, &biomes);
        let parts = partition_by_biome(&g);
        let forest = parts.iter().find(|p| p.biome == 1).expect("林区");
        assert_eq!(forest.cells, 24, "林区 24 格");
        assert_eq!(
            forest.holes.len(),
            1,
            "林中湖应报一个洞，实得 {}",
            forest.holes.len()
        );
        let lake = parts.iter().find(|p| p.biome == 2).expect("湖区");
        assert_eq!(lake.cells, 1);
        assert!(lake.holes.is_empty(), "单格湖无洞");
    }

    #[test]
    fn union_of_adjacent_regions_yields_one_outer_ring() {
        // 两个相邻方块（格坐标）→ 并集是一个 4×2 的外环。
        let a = RingSet {
            outer: vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)],
            holes: Vec::new(),
        };
        let b = RingSet {
            outer: vec![(2.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0)],
            holes: Vec::new(),
        };
        let u = union_regions(&a, &b, 4, 2).expect("相邻应可并");
        let (min_x, min_y, max_x, max_y) = bbox(&u.outer);
        assert!(min_x <= 0.6 && (max_x - 4.0).abs() <= 0.6, "并集应横跨两块");
        assert!(
            min_y <= 0.6 && (max_y - 2.0).abs() <= 0.6,
            "并集应纵跨一块高"
        );
    }

    #[test]
    fn union_of_disjoint_regions_is_rejected() {
        let a = RingSet {
            outer: vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)],
            holes: Vec::new(),
        };
        let b = RingSet {
            outer: vec![(6.0, 0.0), (8.0, 0.0), (8.0, 2.0), (6.0, 2.0)],
            holes: Vec::new(),
        };
        let err = union_regions(&a, &b, 8, 2).expect_err("不相邻应拒绝");
        assert_eq!(err, RegionError::Disjoint);
    }

    #[test]
    fn union_preserves_inner_hole() {
        // 带洞的环 + 贴边的一块 → 并集仍带那个洞。
        let a = RingSet {
            outer: vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)],
            holes: vec![vec![(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)]],
        };
        let b = RingSet {
            outer: vec![(4.0, 0.0), (6.0, 0.0), (6.0, 4.0), (4.0, 4.0)],
            holes: Vec::new(),
        };
        let u = union_regions(&a, &b, 6, 4).expect("相邻应可并");
        assert_eq!(u.holes.len(), 1, "并集应保留原洞，实得 {}", u.holes.len());
    }

    #[test]
    fn union_of_degenerate_ring_is_rejected() {
        let a = RingSet {
            outer: vec![(0.0, 0.0), (1.0, 1.0)],
            holes: Vec::new(),
        };
        let b = RingSet {
            outer: vec![(2.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0)],
            holes: Vec::new(),
        };
        assert_eq!(
            union_regions(&a, &b, 4, 2).expect_err("退化环应拒绝"),
            RegionError::Degenerate
        );
    }

    #[test]
    fn biome_region_at_returns_the_component_containing_the_cell() {
        // 4×2：左两列群系 1，右两列群系 2。
        let g = grid_of(4, 2, &[1, 1, 2, 2, 1, 1, 2, 2]);
        let left = biome_region_at(&g, 0, 0).expect("左格有区");
        assert_eq!(left.biome, 1);
        assert_eq!(left.cells, 4, "左区含 4 格");
        let right = biome_region_at(&g, 3, 1).expect("右格有区");
        assert_eq!(right.biome, 2);
        assert_eq!(right.cells, 4);
    }

    #[test]
    fn biome_region_at_matches_the_partition_list() {
        // 同一格：按格查询的区应与分区列表里同群系的区一致（逐格一致）。
        let g = grid_of(4, 2, &[1, 1, 2, 2, 1, 1, 2, 2]);
        let parts = partition_by_biome(&g);
        let at = biome_region_at(&g, 0, 0).expect("有区");
        let listed = parts
            .iter()
            .find(|p| p.biome == at.biome)
            .expect("列表里有同群系区");
        assert_eq!(listed.cells, at.cells, "按格查询与分区列表应一致");
    }

    #[test]
    fn biome_region_at_out_of_bounds_returns_none() {
        let g = grid_of(2, 2, &[1; 4]);
        assert!(biome_region_at(&g, 9, 9).is_none());
    }

    #[test]
    fn partition_of_empty_grid_is_empty() {
        let g = TerrainGrid::new(0, 0);
        assert!(partition_by_biome(&g).is_empty());
    }
}
