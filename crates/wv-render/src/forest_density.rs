//! 森林密度场的 **Chunk 级生成**（设计文档 §九-3，2026-10-22）。
//!
//! ## 职责
//!
//! 按需（视口扫到哪算哪）计算森林密度场的一个块，并把最近用过的块留在内存里。
//! 这是 [`crate::terrain_decor`]（程序化林冠点疏密）与
//! [`crate::forest_canopy`]（位图树冠落点）共用的**唯一密度来源**——
//! 两处各算一片会「底色一片林、树长在别处」。
//!
//! ## 为什么可以按块算而不出接缝
//!
//! 密度公式 [`wv_terrain::forest::forest_density_cell`] 是**点态**的：
//! 一格的值只依赖该格自己的 `(群系, 海拔)` 与格坐标上的两层 Perlin 噪声，
//! **不读邻格**。因此「分块算」与「整图算」逐字节相等，块边界天然无接缝
//! （单测 `chunked_values_match_the_pointwise_formula` 钉死）。
//!
//! **被否决的做法**：Codex 原文的泊松盘采样（`TreeInstance` 点列）——那种
//! 采样必须读邻块的点才能保证最小间距，块边界要么留缝要么要共享邻域信息。
//! 本仓库的落点不同：**树不是生成出来的点列，而是逐格确定性判定的结果**
//! （见 `forest_canopy`：保留率 = 密度场 × 主题增益，逐格哈希），
//! 于是「共享邻域」这件事在因果上不需要。
//!
//! ## 用法（帧首同步一次、帧内逐格取值）
//!
//! ```ignore
//! forest_cache.sync(data);                       // 每帧一次：hash 变了才清块（O(N)）
//! let d = forest_cache.density_at(data, cx, cy); // 逐格：块内只做数组索引
//! ```
//!
//! ## 边界（不做什么）
//!
//! - **不落盘**（P7）：密度场是派生量，重开工程按同一张地形重算出同一片林；
//! - **不持世界**：只吃 `&TerrainData`（渲染层只读投影）；
//! - **不做增量脏块**：地形一变（`terrain_data_hash` 变）就清空全部块——
//!   反正重算只发生在**视口扫到的块**上，代价有界。逐块脏区（作者改一格只
//!   重算那一块）是后续优化，收敛点就是本文件的 [`ForestDensityCache::sync`]。

#![cfg(feature = "app")]

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use wv_core::terrain::TerrainData;

/// 每块的边长（格）。
///
/// **为什么是 128**：一块 128² × 4 B = 64 KB，恰好落在 L2 缓存量级；
/// 与地形纹理分块（256 px）同数量级但更细——密度场只被视口内的格采样，
/// 块越细「算了却没人读」的浪费越少。
pub const CHUNK_CELLS: u32 = 128;

/// 常驻块上限（块）。
///
/// 512 块 × 64 KB = 32 MB 上界；4096² 整图是 1024 块，但**视口永远只看得到
/// 十几到几十块**——上限只是兜底，正常使用远达不到。
pub const CHUNK_BUDGET: usize = 512;

/// 一块密度值（行优先，边长 [`CHUNK_CELLS`]；靠边的块是满尺寸，
/// 越界格留 `0.0`——调用方本来就会先判格范围）。
type Chunk = Arc<[f32]>;

/// 地形身份：`terrain_data_hash` + 尺寸。任一变化 ⇒ 全部块作废。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct MapKey {
    hash: u64,
    w: u32,
    h: u32,
}

/// 森林密度场缓存（**块级惰性计算**）。
///
/// 用法：帧首 [`Self::sync`]，帧内 [`Self::density_at`]——内部维护「当前热块」，
/// 同块内连续取值只做一次数组索引（渲染层按步距扫格，天然贴块局部性）。
#[derive(Resource)]
pub struct ForestDensityCache {
    map: MapKey,
    /// 噪声置换表（种子固定，见 [`wv_terrain::forest::DENSITY_SEED`]）。
    noise: wv_terrain::generate::Perlin,
    /// 已算出的块（键 = 块坐标）。
    chunks: HashMap<(u32, u32), (Chunk, u64)>,
    /// 当前热块的值（`Arc` 克隆，切块不拷 64 KB 数据）。
    current: Option<Chunk>,
    /// 当前热块的坐标。
    current_key: Option<(u32, u32)>,
    /// 逻辑时钟：每次取块自增，用于长期未用块的淘汰。
    stamp: u64,
    /// 累计**算过**的块数（诊断 / 单测：证明没有重算整图）。
    computed: u64,
}

impl Default for ForestDensityCache {
    fn default() -> Self {
        Self {
            map: MapKey::default(),
            noise: wv_terrain::generate::Perlin::new(wv_terrain::forest::DENSITY_SEED),
            chunks: HashMap::new(),
            current: None,
            current_key: None,
            stamp: 0,
            computed: 0,
        }
    }
}

impl ForestDensityCache {
    /// **每帧一次的同步**：地形身份（`terrain_data_hash` + 尺寸）变了就清空全部块。
    ///
    /// **为什么单独一步**：`terrain_data_hash` 是 O(N) 的，不能进逐格快路径
    /// （那会变成每格一次全图哈希）。约定是**帧首同步一次、帧内逐格取值**——
    /// 两个系统（装饰 / 树冠）各自在拿到 `active_terrain` 后立刻调本函数。
    ///
    /// 返回「是否发生了变化」，供调试 / 单测断言；正常帧内可忽略。
    pub fn sync(&mut self, data: &TerrainData) -> bool {
        let key = MapKey {
            hash: crate::terrain_view::isoline_overlay::terrain_data_hash(data),
            w: data.width,
            h: data.height,
        };
        if self.map == key {
            return false;
        }
        self.map = key;
        self.chunks.clear();
        self.current = None;
        self.current_key = None;
        true
    }

    /// 取某格的森林密度（`0..=1`）；格越界返回 `0.0`。
    ///
    /// **为什么是逐格入口而不是「取整场」**：整场接口会把大地图的
    /// 16M 格一次性算出来并常驻（4096² 就是 64 MB），而渲染层每帧只扫视口
    /// ——分块惰性计算把成本与内存都压到「看得见的部分」。
    ///
    /// **前置条件**：本帧已 [`Self::sync`] 过（系统里一次）。未同步时读到的是
    /// 上次同步时刻的块——块键相同即复用，不会误算，但可能落后于最新地形。
    pub fn density_at(&mut self, data: &TerrainData, cx: u32, cy: u32) -> f32 {
        if cx >= data.width || cy >= data.height {
            return 0.0;
        }
        let key = (cx / CHUNK_CELLS, cy / CHUNK_CELLS);
        if self.current_key != Some(key) {
            self.load_chunk(data, key);
        }
        let Some(chunk) = self.current.as_ref() else {
            return 0.0;
        };
        // 块内局部下标：块靠边时仍是满尺寸网格，越界格留 0，
        // 故这里不必再判地图边界（上面已判过）。
        let lx = cx - key.0 * CHUNK_CELLS;
        let ly = cy - key.1 * CHUNK_CELLS;
        let idx = (ly as usize) * CHUNK_CELLS as usize + lx as usize;
        chunk.get(idx).copied().unwrap_or(0.0)
    }

    /// 累计算过的块数（诊断 / 单测）。
    pub fn computed_chunks(&self) -> u64 {
        self.computed
    }

    /// 当前常驻块数（诊断 / 单测）。
    pub fn resident_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// 换块：先查缓存，没有就算一块。
    fn load_chunk(&mut self, data: &TerrainData, key: (u32, u32)) {
        debug_assert!(
            self.map.w == data.width && self.map.h == data.height,
            "取密度前必须先 sync（尺寸不符说明忘了同步）"
        );
        self.stamp += 1;
        let stamp = self.stamp;

        if let Some((chunk, touched)) = self.chunks.get_mut(&key) {
            *touched = stamp;
            self.current = Some(chunk.clone());
            self.current_key = Some(key);
            return;
        }

        let chunk = self.compute_chunk(data, key);
        self.computed += 1;
        self.chunks.insert(key, (chunk.clone(), stamp));
        self.current = Some(chunk);
        self.current_key = Some(key);
        self.evict_if_needed(key);
    }

    /// 算一块：逐格调 [`wv_terrain::forest::forest_density_cell`]（点态公式，
    /// 与整图场逐字节相等）。
    fn compute_chunk(&self, data: &TerrainData, key: (u32, u32)) -> Chunk {
        let side = CHUNK_CELLS as usize;
        let mut values = vec![0.0_f32; side * side];
        let x0 = key.0 * CHUNK_CELLS;
        let y0 = key.1 * CHUNK_CELLS;
        for ly in 0..side as u32 {
            let cy = y0 + ly;
            if cy >= data.height {
                break;
            }
            for lx in 0..side as u32 {
                let cx = x0 + lx;
                if cx >= data.width {
                    break;
                }
                let Some(cell) = data
                    .cells
                    .get((cy as usize) * data.width as usize + cx as usize)
                else {
                    continue;
                };
                values[(ly as usize) * side + lx as usize] =
                    wv_terrain::forest::forest_density_cell(
                        &self.noise,
                        cell.biome,
                        cell.height,
                        cx as usize,
                        cy as usize,
                    );
            }
        }
        Arc::from(values)
    }

    /// 超预算 ⇒ 淘汰最久未用的块（当前块永不淘汰）。
    fn evict_if_needed(&mut self, keep: (u32, u32)) {
        while self.chunks.len() > CHUNK_BUDGET {
            let victim = self
                .chunks
                .iter()
                .filter(|(k, _)| **k != keep)
                .min_by_key(|(_, (_, touched))| *touched)
                .map(|(k, _)| *k);
            match victim {
                Some(k) => {
                    self.chunks.remove(&k);
                }
                // 只剩当前块（预算被调到 0 的退化情形）→ 停止淘汰。
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::{TerrainCellData, TerrainData};
    use wv_terrain::biome::{BIOME_DESERT, BIOME_FOREST, BIOME_GRASSLAND};

    fn fixture(w: u32, h: u32) -> TerrainData {
        // 一半森林（低地）、四分之一草原、四分之一荒漠——密度分级覆盖三档。
        let cells = (0..w * h)
            .map(|i| {
                let x = i % w;
                let biome = if x < w / 2 {
                    BIOME_FOREST
                } else if x < w * 3 / 4 {
                    BIOME_GRASSLAND
                } else {
                    BIOME_DESERT
                };
                TerrainCellData {
                    height: 400 + (i % 7) as i16 * 40,
                    surface: 1,
                    biome,
                }
            })
            .collect();
        TerrainData {
            width: w,
            height: h,
            cells,
        }
    }

    fn cached(data: &TerrainData) -> ForestDensityCache {
        let mut cache = ForestDensityCache::default();
        cache.sync(data);
        cache
    }

    /// 分块取值 == 逐格公式（块边界无接缝的**因果**断言）。
    #[test]
    fn chunked_values_match_the_pointwise_formula() {
        let w = CHUNK_CELLS * 2 + 17;
        let h = CHUNK_CELLS + 5;
        let data = fixture(w, h);
        let noise = wv_terrain::generate::Perlin::new(wv_terrain::forest::DENSITY_SEED);
        let mut cache = cached(&data);
        for cy in (0..h).step_by(13) {
            for cx in (0..w).step_by(11) {
                let got = cache.density_at(&data, cx, cy);
                let cell = &data.cells[(cy * w + cx) as usize];
                let want = wv_terrain::forest::forest_density_cell(
                    &noise,
                    cell.biome,
                    cell.height,
                    cx as usize,
                    cy as usize,
                );
                assert_eq!(got, want, "({cx},{cy}) 块内值与逐格公式不符");
            }
        }
    }

    /// 只算**扫到的**块：同块内多次取值只算一次，不整图预算。
    #[test]
    fn only_touched_chunks_are_computed() {
        let w = CHUNK_CELLS * 4;
        let h = CHUNK_CELLS * 4; // 整图 16 块
        let data = fixture(w, h);
        let mut cache = cached(&data);
        // 只扫第一块里的几个格。
        for cx in (0..CHUNK_CELLS).step_by(37) {
            let _ = cache.density_at(&data, cx, 0);
        }
        assert_eq!(cache.computed_chunks(), 1, "同块内多次取值只该算一次");
        assert_eq!(cache.resident_chunks(), 1, "只常驻扫到的块");
    }

    /// 同块内连续取值不再触发计算（热块快路径）。
    #[test]
    fn repeated_reads_in_the_same_chunk_hit_the_cache() {
        let data = fixture(CHUNK_CELLS, CHUNK_CELLS);
        let mut cache = cached(&data);
        for _ in 0..50 {
            for cx in (0..CHUNK_CELLS).step_by(9) {
                let _ = cache.density_at(&data, cx, cx % CHUNK_CELLS);
            }
        }
        assert_eq!(cache.computed_chunks(), 1, "50 轮同块扫描只该算 1 块");
    }

    /// 地形变了 ⇒ 块作废重算（键 = terrain hash）；同地形反复 sync 不清块。
    #[test]
    fn terrain_change_invalidates_every_chunk() {
        let mut data = fixture(CHUNK_CELLS, CHUNK_CELLS);
        let mut cache = ForestDensityCache::default();
        assert!(cache.sync(&data), "首次 sync 应标记为变化");
        let before = cache.density_at(&data, 5, 5);
        assert_eq!(cache.computed_chunks(), 1);
        assert!(!cache.sync(&data), "地形没变时 sync 不应清块");

        // 改一格的高度（hash 变）→ 再取应重算，且值随之改变。
        let idx = 5 * CHUNK_CELLS as usize + 5;
        data.cells[idx].height = 2500;
        assert!(cache.sync(&data), "地形变化后 sync 应标记为变化");
        let after = cache.density_at(&data, 5, 5);
        assert_eq!(cache.computed_chunks(), 2, "地形变化后应重算该块");
        assert!(
            after != before || after == 0.0,
            "林线之上密度应为 0（before={before}, after={after}）"
        );
    }

    /// 同一地形反复 sync 不产生新块（帧内每系统一次 sync 是廉价的）。
    #[test]
    fn repeated_sync_on_unchanged_terrain_keeps_chunks() {
        let data = fixture(CHUNK_CELLS, CHUNK_CELLS);
        let mut cache = cached(&data);
        let _ = cache.density_at(&data, 3, 3);
        let computed = cache.computed_chunks();
        for _ in 0..10 {
            assert!(!cache.sync(&data));
        }
        let _ = cache.density_at(&data, 4, 4);
        assert_eq!(cache.computed_chunks(), computed, "重复 sync 不该触发重算");
    }

    /// 预算兜底：常驻块数不超过 [`CHUNK_BUDGET`]。
    #[test]
    fn resident_chunks_stay_within_budget() {
        // 竖着排「预算 + 2」块。
        let w = CHUNK_CELLS;
        let h = CHUNK_CELLS * (CHUNK_BUDGET as u32 + 2);
        let data = fixture(w, h);
        let mut cache = cached(&data);
        for by in 0..(CHUNK_BUDGET as u32 + 2) {
            let _ = cache.density_at(&data, 0, by * CHUNK_CELLS);
        }
        assert!(
            cache.resident_chunks() <= CHUNK_BUDGET,
            "常驻块应被预算约束，got {}",
            cache.resident_chunks()
        );
        assert!(
            cache.computed_chunks() >= CHUNK_BUDGET as u64,
            "应真的算了这么多块"
        );
    }

    /// 越界格给 0（调用方不必先判边界，但也不能读出脏值）。
    #[test]
    fn out_of_bounds_cells_read_as_zero() {
        let data = fixture(16, 16);
        let mut cache = cached(&data);
        assert_eq!(cache.density_at(&data, 16, 0), 0.0);
        assert_eq!(cache.density_at(&data, 0, 16), 0.0);
        assert_eq!(cache.density_at(&data, u32::MAX, u32::MAX), 0.0);
        assert_eq!(cache.computed_chunks(), 0, "越界取值不该触发计算");
    }

    /// 荒漠格密度恒为 0（群系事实，与分块无关）。
    #[test]
    fn desert_cells_are_always_zero() {
        let data = fixture(CHUNK_CELLS, CHUNK_CELLS);
        let mut cache = cached(&data);
        let x = CHUNK_CELLS - 1; // fixture 里最右一档 = 荒漠
        for cy in (0..CHUNK_CELLS).step_by(17) {
            assert_eq!(cache.density_at(&data, x, cy), 0.0);
        }
    }
}
