//! 指纹与确定性散列的**单一真源**（P2 收敛，2026-10-29）。
//!
//! 此前 `grid_bits` ×2、`splitmix64` ×2 在各文件逐字复制（`hash01` 语义
//! 分叉：terrain_decor 走 splitmix64、`wv_terrain::hash::cell_hash01` 走
//! xxhash 风格——**不是**同一函数，后者已由 paper_overlay 示范别名复用）。
//! FNV 折叠各副本提取语义不一（字节/整词），合并会改指纹值，**有意保留**
//! 各自实现；只收敛「逐字相同」的部分。

use wv_world_cell_map::state::GridConfig;

/// 栅格配置的 7 元素位模式指纹（cell_size / 两套原点 / 幅面）。
///
/// 消费方：等值线与网格线的缓存键（`region_outline` / `world_projection`）。
pub(crate) fn grid_bits(grid: &GridConfig) -> [u64; 7] {
    [
        u64::from(grid.cell_size.to_bits()),
        u64::from(grid.raster_origin.x.to_bits()),
        u64::from(grid.raster_origin.y.to_bits()),
        u64::from(grid.map_origin.x.to_bits()),
        u64::from(grid.map_origin.y.to_bits()),
        u64::from(grid.map_size.x.to_bits()),
        u64::from(grid.map_size.y.to_bits()),
    ]
}

/// splitmix64 终端混淆（程序化装饰的确定性种子来源）。
pub(crate) fn splitmix64(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
