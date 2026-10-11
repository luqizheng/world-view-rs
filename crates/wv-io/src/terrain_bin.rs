//! 地形栅格的二进制编解码（`terrain.bin`）。
//!
//! ## 依据与纪律（`AGENTS.md` §5 二进制分块纪律）
//!
//! 二进制是 P3 的**唯一例外**，门槛最高：
//! 1. 确实属于「超大地图网格数组」——**其他任何数据类型都不得使用二进制**；
//! 2. 在索引 YAML 中通过 [`RasterRef`] 显式声明；
//! 3. 声明 `format_version`，并在 `docs/` 中补充《二进制格式规格》对应条目；
//! 4. 元数据只存引用（路径 + 尺寸 + 编码 + 校验和），**不得内联数据**。
//!
//! ## 文件布局（`format_version = 1`）
//!
//! 本模块**自带魔数与版本头**，因此 `terrain.bin` 可独立自描述，
//! 不依赖外层 YAML 才知道怎么读。头之后是 zstd 压缩的 bincode 载荷：
//!
//! ```text
//! ┌───────────────────────────────────────────────┐
//! │ magic   : 4 字节  ASCII "WVTR"                │
//! │ version : u32 LE  格式版本（当前 1）           │
//! │ width   : u32 LE                               │
//! │ height  : u32 LE                               │
//! │ payload : zstd(bincode(Vec<(i16, u8)>))        │
//! └───────────────────────────────────────────────┘
//! ```
//!
//! 双通道 `(height: i16, kind: u8)` 打包为一个元组序列，对应基线 §2.3 的
//! 「I16 高度 + U8 类型，双通道」。
//!
//! ## 性能门禁（基线 §2.19）
//!
//! 单次栅格 > 16,777,216 格（4096²）→ **拒绝加载并报错**，要求分块。

use std::io::Write;
use std::path::Path;

use wv_core::map::RasterRef;
use wv_core::terrain::{TerrainCellData, TerrainData};

use crate::error::{IoError, IoResult};

/// `terrain.bin` 的魔数。
pub const TERRAIN_MAGIC: [u8; 4] = *b"WVTR";
/// 当前支持的 `terrain.bin` 格式版本。
pub const TERRAIN_FORMAT_VERSION: u32 = 1;
/// 单张栅格的格数上限（基线 §2.19 门禁：4096×4096）。
pub const MAX_TERRAIN_CELLS: usize = 16_777_216;

/// 把一段字节序列化为 `terrain.bin` 格式。
///
/// 只做编码，不碰文件系统——这样测试无需落盘即可验证 round-trip。
pub fn encode_terrain(data: &TerrainData) -> IoResult<Vec<u8>> {
    if data.cell_count() > MAX_TERRAIN_CELLS {
        return Err(IoError::Binary(format!(
            "栅格过大: {} 格（上限 {MAX_TERRAIN_CELLS}），请分块",
            data.cell_count()
        )));
    }
    if data.cells.len() != (data.width as usize) * (data.height as usize) {
        return Err(IoError::Binary(format!(
            "cells 长度 {} 与尺寸 {}x{} 不符",
            data.cells.len(),
            data.width,
            data.height
        )));
    }

    // 打包成 (i16, u8) 序列供 bincode。
    let packed: Vec<(i16, u8)> = data.cells.iter().map(|c| (c.height, c.kind)).collect();
    let raw = bincode::serialize(&packed)
        .map_err(|e| IoError::Binary(format!("bincode 序列化失败: {e}")))?;
    let compressed = zstd::encode_all(raw.as_slice(), 3)
        .map_err(|e| IoError::Binary(format!("zstd 压缩失败: {e}")))?;

    let mut out = Vec::with_capacity(compressed.len() + 16);
    out.extend_from_slice(&TERRAIN_MAGIC);
    out.extend_from_slice(&TERRAIN_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&data.width.to_le_bytes());
    out.extend_from_slice(&data.height.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// 从 `terrain.bin` 字节解码。返回中立数据 + 更新后的 `RasterRef` 元数据。
pub fn decode_terrain(bytes: &[u8]) -> IoResult<TerrainData> {
    if bytes.len() < 16 || bytes[0..4] != TERRAIN_MAGIC {
        return Err(IoError::Binary("terrain.bin 魔数不符".into()));
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != TERRAIN_FORMAT_VERSION {
        return Err(IoError::Binary(format!(
            "terrain.bin 格式版本 {version} 不受支持（当前 {TERRAIN_FORMAT_VERSION}）"
        )));
    }
    let width = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let height = u32::from_le_bytes(bytes[12..16].try_into().unwrap());

    let cell_count = (width as usize) * (height as usize);
    if cell_count > MAX_TERRAIN_CELLS {
        return Err(IoError::Binary(format!(
            "栅格过大: {cell_count} 格（上限 {MAX_TERRAIN_CELLS}），请分块"
        )));
    }

    let raw = zstd::decode_all(&bytes[16..])
        .map_err(|e| IoError::Binary(format!("zstd 解压失败: {e}")))?;
    let packed: Vec<(i16, u8)> = bincode::deserialize(&raw)
        .map_err(|e| IoError::Binary(format!("bincode 反序列化失败: {e}")))?;

    if packed.len() != cell_count {
        return Err(IoError::Binary(format!(
            "载荷格数 {} 与头声明 {}x{} = {cell_count} 不符",
            packed.len(),
            width,
            height
        )));
    }

    let cells = packed
        .into_iter()
        .map(|(height, kind)| TerrainCellData { height, kind })
        .collect();
    Ok(TerrainData {
        width,
        height,
        cells,
    })
}

/// 把地形写入工程目录下的 `maps/<map_id>/terrain.bin`，返回更新后的 [`RasterRef`]。
///
/// `RasterRef` 是 `RasterRef` 在索引 YAML 中的体现（`AGENTS.md` §5 第 2 条）：
/// 元数据只存引用与校验和，不内联数据。
pub fn write_terrain(
    project_root: impl AsRef<Path>,
    map_id: &str,
    data: &TerrainData,
) -> IoResult<RasterRef> {
    let bytes = encode_terrain(data)?;
    let dir = project_root.as_ref().join("maps").join(map_id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("terrain.bin");

    let mut file = std::fs::File::create(&path)?;
    file.write_all(&bytes)?;
    file.flush()?;

    let checksum = crc32(&bytes);
    Ok(RasterRef {
        path: format!("maps/{map_id}/terrain.bin"),
        encoding: wv_core::map::RasterEncoding::I16,
        width: data.width,
        height: data.height,
        chunk: Some(256),
        format_version: TERRAIN_FORMAT_VERSION,
        checksum: Some(checksum),
    })
}

/// 从工程目录读取 `terrain.bin`。会校验 `RasterRef.checksum`（若声明）。
pub fn read_terrain(project_root: impl AsRef<Path>, raster: &RasterRef) -> IoResult<TerrainData> {
    let path = project_root.as_ref().join(&raster.path);
    let bytes = std::fs::read(&path)?;

    if let Some(expected) = &raster.checksum {
        let actual = crc32(&bytes);
        if &actual != expected {
            return Err(IoError::Binary(format!(
                "`{}` 校验和不符：声明 {expected}，实际 {actual}",
                raster.path
            )));
        }
    }

    let data = decode_terrain(&bytes)?;
    if data.width != raster.width || data.height != raster.height {
        return Err(IoError::Binary(format!(
            "`{}` 尺寸与索引声明不符：文件 {}x{}，索引 {}x{}",
            raster.path, data.width, data.height, raster.width, raster.height
        )));
    }
    Ok(data)
}

/// CRC-32（IEEE 802.3）实现。手写以**避免引入 `crc32fast` 依赖**
/// （`AGENTS.md` §9：已有等价方案时不新增依赖；~20 行即可）。
pub fn crc32(data: &[u8]) -> String {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    format!("{:08x}", !crc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(width: u32, height: u32) -> TerrainData {
        let mut cells = Vec::new();
        for y in 0..height {
            for x in 0..width {
                cells.push(TerrainCellData {
                    height: (y as i16) * 10 - 5,
                    kind: (x % 5) as u8,
                });
            }
        }
        TerrainData {
            width,
            height,
            cells,
        }
    }

    #[test]
    fn encode_decode_round_trips_exactly() {
        let data = sample(64, 48);
        let bytes = encode_terrain(&data).unwrap();
        let back = decode_terrain(&bytes).unwrap();
        assert_eq!(data, back, "terrain.bin round-trip 必须无损");
    }

    #[test]
    fn header_carries_magic_version_and_dimensions() {
        let data = sample(10, 20);
        let bytes = encode_terrain(&data).unwrap();
        assert_eq!(&bytes[0..4], &TERRAIN_MAGIC);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 10);
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 20);
    }

    #[test]
    fn zstd_compresses_repetitive_data() {
        // 大量重复格应被显著压缩：压缩后应远小于「每格 3 字节」。
        let data = sample(256, 256);
        let bytes = encode_terrain(&data).unwrap();
        let uncompressed = 256usize * 256 * 3;
        assert!(
            bytes.len() < uncompressed / 2,
            "重复数据压缩率不足: {} vs {}",
            bytes.len(),
            uncompressed
        );
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = encode_terrain(&sample(4, 4)).unwrap();
        bytes[0] = b'X';
        assert!(matches!(decode_terrain(&bytes), Err(IoError::Binary(_))));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let mut bytes = encode_terrain(&sample(4, 4)).unwrap();
        bytes[4..8].copy_from_slice(&99u32.to_le_bytes());
        let err = decode_terrain(&bytes).unwrap_err();
        assert!(format!("{err}").contains("99"));
    }

    #[test]
    fn oversized_grid_is_rejected_on_encode() {
        // 4097×4097 > 上限（不真的分配，构造头部即可）。
        let data = TerrainData {
            width: 4097,
            height: 4097,
            cells: Vec::new(), // 先触发尺寸检查
        };
        assert!(matches!(encode_terrain(&data), Err(IoError::Binary(_))));
    }

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32("123456789") = 0xCBF43926（标准测试向量）。
        assert_eq!(crc32(b"123456789"), "cbf43926");
    }

    #[test]
    fn write_and_read_terrain_file() {
        let tmp = std::env::temp_dir().join("wv_terrain_io_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let data = sample(32, 16);

        let raster = write_terrain(&tmp, "world_main", &data).unwrap();
        assert_eq!(raster.width, 32);
        assert_eq!(raster.height, 16);
        assert!(raster.checksum.is_some());
        assert!(tmp.join("maps/world_main/terrain.bin").is_file());

        let back = read_terrain(&tmp, &raster).unwrap();
        assert_eq!(data, back);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn checksum_mismatch_is_reported() {
        let tmp = std::env::temp_dir().join("wv_terrain_cksum_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let data = sample(8, 8);
        let mut raster = write_terrain(&tmp, "m", &data).unwrap();
        raster.checksum = Some("deadbeef".into());
        assert!(matches!(
            read_terrain(&tmp, &raster),
            Err(IoError::Binary(_))
        ));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
