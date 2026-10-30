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
//! ## 文件布局
//!
//! 本模块**自带魔数与版本头**，因此 `terrain.bin` 可独立自描述，
//! 不依赖外层 YAML 才知道怎么读。头之后是 zstd 压缩的 bincode 载荷：
//!
//! ```text
//! ┌───────────────────────────────────────────────┐
//! │ magic   : 4 字节  ASCII "WVTR"                │
//! │ version : u32 LE  格式版本（当前 2）           │
//! │ width   : u32 LE                               │
//! │ height  : u32 LE                               │
//! │ payload : zstd(bincode(…))                     │
//! └───────────────────────────────────────────────┘
//! ```
//!
//! - **v2（当前）**：`payload = zstd(bincode(Vec<(i16, u8, u8)>))`，
//!   三通道 `(height, surface, biome)`——设计稿 D11/D12。
//! - **v1（只读）**：`payload = zstd(bincode(Vec<(i16, u8)>))`，双通道
//!   `(height, kind)`；读取时经 [`migrate_v1_kind`] 按 authoring 真值迁移，
//!   规格记载见 `docs/BINARY_FORMAT_SPEC.md` §2.1。写一律 v2。
//!
//! ## 性能门禁（基线 §2.19）
//!
//! 单次栅格 > 16,777,216 格（4096²）→ **拒绝加载并报错**，要求分块。

use std::path::Path;

use wv_core::map::RasterRef;
use wv_core::terrain::{TerrainCellData, TerrainData};

use crate::error::{IoError, IoResult};

/// `terrain.bin` 的魔数。
pub const TERRAIN_MAGIC: [u8; 4] = *b"WVTR";
/// 当前支持的 `terrain.bin` 格式版本。
///
/// v2（2026-10-18）：载荷升为三通道 `(i16, u8, u8)` = (海拔, 地表, 群系)；
/// v1 `(i16, u8)` 保留**只读迁移分支**（`migrate_v1_kind`），写一律 v2。
pub const TERRAIN_FORMAT_VERSION: u32 = 2;
/// 单张栅格的格数上限（基线 §2.19 门禁：4096×4096）。
pub const MAX_TERRAIN_CELLS: usize = 16_777_216;

/// v1 kind（旧「类型」词汇）→ v2 `(surface, biome)`。**authoring 真值，不看高度**：
/// kind ≤ 1 是水（湖在山上也是水）、其余是陆（被压到海下的陆格仍是陆——
/// 湿判定由读取侧的「海拔 < 0」兜住）。
///
/// **与 `wv_terrain::biome` 的冻结编号表必须一致**（wv-io 运行时不依赖
/// wv-terrain，但测试经 `[dev-dependencies]` 引用其常量——两表一致性由
/// `migration_table_matches_wv_terrain_frozen_biome_ids` 真正钉死；
/// `v1_payload_migrates_each_kind_to_authoring_truth` 的字面量只钉本表）。
pub(crate) fn migrate_v1_kind(kind: u8) -> (u8, u8) {
    const WATER: u8 = 0;
    const LAND: u8 = 1;
    match kind {
        0 | 1 => (WATER, 0),
        2 => (LAND, 5),     // 沙 → 沙漠
        3 => (LAND, 3),     // 草原
        4 => (LAND, 1),     // 森林
        5..=7 => (LAND, 0), // 丘陵/山地/雪：高度派生带，无群系
        8 => (LAND, 6),     // 沼泽
        9 => (LAND, 20),    // 道路（工程扩展编号）
        _ => (LAND, 0),
    }
}

/// 旧 `Place.feature`（TerrainKind 0–9 词汇）→ 冻结群系编号（ADR-0032 D4）。
///
/// **place 侧不继承格子侧的「高度派生带归 0」取舍**：{0,1} 深水/浅水升级为
/// 海洋、{5,6,7} 丘陵/山地/雪升级为对应群系——声明了丘陵的地点在新词汇下
/// 就该是丘陵标记（可进区域面板、可重生成）；{2,3,4,8,9} 与 [`migrate_v1_kind`]
/// 的群系分量**同值**（这几类旧声明的诊断比较逐位不变）。词汇表外的值返回
/// `None`（不迁移 + 未映射提示）。
///
/// 与 `wv_terrain::biome` 冻结编号的一致性由
/// `place_feature_migration_table_covers_old_vocabulary` 钉死（同
/// `migrate_v1_kind` 的 dev-dep 先例）；运行时 wv-io 不依赖 wv-terrain。
pub(crate) fn place_feature_to_biome(feature: u8) -> Option<u8> {
    match feature {
        0 | 1 => Some(14), // 深水/浅水 → 海洋
        2 => Some(5),      // 沙 → 沙漠
        3 => Some(3),      // 草原
        4 => Some(1),      // 森林
        5 => Some(12),     // 丘陵
        6 => Some(13),     // 山地
        7 => Some(9),      // 雪 → 雪原
        8 => Some(6),      // 沼泽
        9 => Some(20),     // 道路（工程扩展编号）
        _ => None,
    }
}

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

    // 打包成 (i16, u8, u8) 序列供 bincode。
    let packed: Vec<(i16, u8, u8)> = data
        .cells
        .iter()
        .map(|c| (c.height, c.surface, c.biome))
        .collect();
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
    if version != 1 && version != 2 {
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

    let cells: Vec<TerrainCellData> = if version == 2 {
        let packed: Vec<(i16, u8, u8)> = bincode::deserialize(&raw)
            .map_err(|e| IoError::Binary(format!("bincode 反序列化失败: {e}")))?;
        if packed.len() != cell_count {
            return Err(IoError::Binary(format!(
                "载荷格数 {} 与头声明 {}x{} = {cell_count} 不符",
                packed.len(),
                width,
                height
            )));
        }
        packed
            .into_iter()
            .map(|(height, surface, biome)| TerrainCellData {
                height,
                surface,
                biome,
            })
            .collect()
    } else {
        // v1 只读迁移：authoring 真值映射（见 `migrate_v1_kind`）。
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
        packed
            .into_iter()
            .map(|(height, kind)| {
                let (surface, biome) = migrate_v1_kind(kind);
                TerrainCellData {
                    height,
                    surface,
                    biome,
                }
            })
            .collect()
    };

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
///
/// 走 [`crate::atomic::write_bytes`]：地形是自动保存里最大的一块，写到一半被
/// 杀进程会让整张图连索引一起对不上（校验和是写完后才算的）——原子替换把
/// 最坏情况钉在「上一版地形」。
pub fn write_terrain(
    project_root: impl AsRef<Path>,
    map_id: &str,
    data: &TerrainData,
) -> IoResult<RasterRef> {
    let bytes = encode_terrain(data)?;
    let dir = project_root.as_ref().join("maps").join(map_id);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("terrain.bin");

    crate::atomic::write_bytes(&path, &bytes)?;

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
                    surface: (x % 2) as u8,
                    biome: ((x * 7 + y) % 21) as u8,
                });
            }
        }
        TerrainData {
            width,
            height,
            cells,
        }
    }

    /// 按 v1 布局手工拼字节（version=1 + `zstd(bincode(Vec<(i16,u8)>))`），
    /// 供迁移测试构造旧格式输入。
    fn v1_encode_for_test(width: u32, height: u32, cells: &[(i16, u8)]) -> Vec<u8> {
        let raw = bincode::serialize(cells).unwrap();
        let compressed = zstd::encode_all(raw.as_slice(), 3).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&TERRAIN_MAGIC);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&compressed);
        out
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
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 10);
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 20);
    }

    #[test]
    fn zstd_compresses_repetitive_data() {
        // 大量重复格应被显著压缩：压缩后应远小于「每格 4 字节」。
        let data = sample(256, 256);
        let bytes = encode_terrain(&data).unwrap();
        let uncompressed = 256usize * 256 * 4;
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
    fn v1_payload_migrates_each_kind_to_authoring_truth() {
        // 表驱动：8 旧 kind + 笔刷扩展 8/9 → (surface, biome)；不看高度
        // （authoring 真值：kind≤1 是水、其余是陆，海拔不翻案）。
        let cases: [(u8, u8, u8); 10] = [
            (0, 0, 0),
            (1, 0, 0),
            (2, 1, 5),
            (3, 1, 3),
            (4, 1, 1),
            (5, 1, 0),
            (6, 1, 0),
            (7, 1, 0),
            (8, 1, 6),
            (9, 1, 20),
        ];
        for (kind, surface, biome) in cases {
            let bytes = v1_encode_for_test(4, 4, &[(100, kind); 16]);
            let data = decode_terrain(&bytes).unwrap();
            let c = &data.cells[0];
            assert_eq!(
                (c.surface, c.biome),
                (surface, biome),
                "kind {kind} 迁移得 {:?}",
                (c.surface, c.biome)
            );
        }
    }

    #[test]
    fn v1_lake_at_positive_height_stays_water_after_migration() {
        // kind=1（浅水）+ 高度 +10：authoring 真值是水，不因正海拔翻案。
        let bytes = v1_encode_for_test(2, 2, &[(10, 1); 4]);
        let data = decode_terrain(&bytes).unwrap();
        assert_eq!(data.cells[0].surface, 0, "got {:?}", data.cells[0]);
        assert_eq!(data.cells[0].height, 10);
    }

    #[test]
    fn v1_read_resave_v2_round_trips_stably() {
        // 10 种 kind 全覆盖、高度跨正负：迁移 → 保存 → 再读必须逐格相等（幂等）。
        let v1_cells: Vec<(i16, u8)> = (0..16).map(|i| (i as i16 * 7 - 50, i as u8 % 10)).collect();
        let v1 = v1_encode_for_test(4, 4, &v1_cells);
        let once = decode_terrain(&v1).unwrap();
        let v2 = encode_terrain(&once).unwrap();
        let twice = decode_terrain(&v2).unwrap();
        assert_eq!(once.cells, twice.cells, "migration must be idempotent");
    }

    /// 跨表一致性（终审 Important #1）：wv-io 迁移表的编号必须等于
    /// `wv_terrain::biome` 的冻结常量——若上游重编号而本表未跟，此测试即红。
    #[test]
    fn migration_table_matches_wv_terrain_frozen_biome_ids() {
        assert_eq!(
            migrate_v1_kind(0),
            (wv_terrain::SURFACE_WATER, wv_terrain::BIOME_NONE)
        );
        assert_eq!(
            migrate_v1_kind(1),
            (wv_terrain::SURFACE_WATER, wv_terrain::BIOME_NONE)
        );
        assert_eq!(
            migrate_v1_kind(2),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_DESERT)
        );
        assert_eq!(
            migrate_v1_kind(3),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_GRASSLAND)
        );
        assert_eq!(
            migrate_v1_kind(4),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_FOREST)
        );
        assert_eq!(
            migrate_v1_kind(5),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_NONE)
        );
        assert_eq!(
            migrate_v1_kind(6),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_NONE)
        );
        assert_eq!(
            migrate_v1_kind(7),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_NONE)
        );
        assert_eq!(
            migrate_v1_kind(8),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_SWAMP)
        );
        assert_eq!(
            migrate_v1_kind(9),
            (wv_terrain::SURFACE_LAND, wv_terrain::BIOME_ROAD)
        );
    }

    /// place 级迁移表（ADR-0032 D4）：旧 feature 全词汇 0–9 逐条钉到冻结常量。
    #[test]
    fn place_feature_migration_table_covers_old_vocabulary() {
        assert_eq!(place_feature_to_biome(0), Some(wv_terrain::BIOME_OCEAN));
        assert_eq!(place_feature_to_biome(1), Some(wv_terrain::BIOME_OCEAN));
        assert_eq!(place_feature_to_biome(2), Some(wv_terrain::BIOME_DESERT));
        assert_eq!(place_feature_to_biome(3), Some(wv_terrain::BIOME_GRASSLAND));
        assert_eq!(place_feature_to_biome(4), Some(wv_terrain::BIOME_FOREST));
        assert_eq!(place_feature_to_biome(5), Some(wv_terrain::BIOME_HILLS));
        assert_eq!(place_feature_to_biome(6), Some(wv_terrain::BIOME_MOUNTAINS));
        assert_eq!(place_feature_to_biome(7), Some(wv_terrain::BIOME_SNOWFIELD));
        assert_eq!(place_feature_to_biome(8), Some(wv_terrain::BIOME_SWAMP));
        assert_eq!(place_feature_to_biome(9), Some(wv_terrain::BIOME_ROAD));
        assert_eq!(place_feature_to_biome(10), None, "词汇表外垃圾值不迁移");
        assert_eq!(place_feature_to_biome(255), None, "词汇表外垃圾值不迁移");
    }

    /// 共同槽位 {2,3,4,8,9} 上，place 侧表必须与格子侧 v1 迁移表同值
    /// （ADR-0032 D4：这几类旧声明的诊断行为逐位不变）。
    #[test]
    fn place_feature_table_agrees_with_terrain_v1_table_on_shared_slots() {
        for f in [2u8, 3, 4, 8, 9] {
            assert_eq!(
                place_feature_to_biome(f),
                Some(migrate_v1_kind(f).1),
                "feature {f} 的 place 迁移值应与格子侧 v1 迁移表同值"
            );
        }
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
