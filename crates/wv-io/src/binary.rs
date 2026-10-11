//! 二进制分块读写。
//!
//! 原则三的唯一例外：超大地图的网格数组允许以二进制作为一等源格式。
//!
//! 强约束（违反即不予采纳）：
//! 1. 必须在索引 YAML 中显式声明（`map::RasterRef`）；
//! 2. 必须有 `format_version`，且版本策略需在《二进制格式规格》中固定；
//! 3. 元数据必须使用二进制**引用**，不得内联进 YAML。
//!
//! TODO(阶段七): 接入 bincode / rkyv + zstd，实现分块读写。

use wv_core::map::RasterRef;

use crate::error::IoResult;

/// 二进制分块的抽象读写接口。
///
/// 先定义接口、后补实现：这样地图层可以在实现落地前
/// 就按「引用」而非「内联数据」的方式建模。
pub trait RasterStore {
    /// 读取整个栅格。
    fn read(&self, r: &RasterRef) -> IoResult<Vec<u8>>;
    /// 按块读取，用于大地图的按需加载。
    fn read_chunk(&self, r: &RasterRef, cx: u32, cy: u32) -> IoResult<Vec<u8>>;
    /// 写入整个栅格，并返回更新后的引用（含 checksum）。
    fn write(&self, r: &RasterRef, data: &[u8]) -> IoResult<RasterRef>;
}
