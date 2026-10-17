# 二进制格式规格 · `terrain.bin`

> **依据** `AGENTS.md` §5（二进制分块纪律）+ `docs/MAP_EDITOR_BASELINE_v1.md` §2.3 / §2.18。
> **性质** 本项目**唯一**的二进制一等源格式。除「超大地图网格数组」外，任何数据类型都不得使用二进制。

---

## 1. 为什么只有这一个二进制格式

P3（YAML 为主格式）的例外只开一扇门：**超大地图的高度场 / 地形类型栅格**。

| 数据 | 形态 | 存储 | 理由 |
|---|---|---|---|
| 高度场 / 地形类型 | **栅格** | **`terrain.bin`** | 连续标量场，笔刷涂抹的本质是逐格写值；1024² 工程的 YAML 会爆 |
| 道路 / 势力边界 | 矢量 | YAML `GeometrySpan` | 折线 / 多边形有拓扑语义，且离散；进栅格会违反 P3 并丧失拓扑 |
| 其他一切 | — | YAML | — |

---

## 2. 文件布局（`format_version = 1`）

文件**自描述**：读取不依赖外层 YAML。

```
偏移   长度    内容
0x00   4       magic  = ASCII "WVTR"
0x04   4       version = u32 LE（当前 1）
0x08   4       width   = u32 LE
0x0C   4       height  = u32 LE
0x10   ...     payload = zstd( bincode( Vec<(i16, u8)> ) )
```

- **载荷**：行优先的 `(height: i16, kind: u8)` 元组序列，长度必须 `== width * height`。
  双通道设计对应基线 §2.3「I16 高度 + U8 类型」。
- **压缩**：`zstd` level 3（默认）。手写 CRC 而非引入 `crc32fast`（`AGENTS.md` §9）。
- **字节序**：头部一律小端（LE），跨平台一致。

---

## 2.1 `format_version = 2`（2026-10-18 追写）

三通道化（设计稿 `docs/superpowers/specs/2026-10-18-world-from-zero-map-design.md` D11/D12）：

```
偏移   长度    内容
0x00   4       magic  = ASCII "WVTR"
0x04   4       version = u32 LE（当前 2）
0x08   4       width   = u32 LE
0x0C   4       height  = u32 LE
0x10   ...     payload = zstd( bincode( Vec<(i16, u8, u8)> ) )
```

- **载荷**：行优先的 `(height: i16, surface: u8, biome: u8)` 元组序列。
  `surface` = 地表二值（0 水 / 1 陆，authoring 真值，不由高度推断）；
  `biome` = 生物群系编号（开放集，冻结表见 `wv_terrain::biome`）。
- **v1 迁移（只读）**：读 v1 时按 **authoring 真值**换算，**不看高度**——

  | v1 kind | → (surface, biome) | 语义 |
  |---|---|---|
  | 0 / 1 | (0, 0) | 水（湖在山上仍是水） |
  | 2 | (1, 5) | 沙 → 沙漠 |
  | 3 | (1, 3) | 草原 |
  | 4 | (1, 1) | 森林 |
  | 5 / 6 / 7 | (1, 0) | 丘陵/山地/雪（高度派生带） |
  | 8 | (1, 6) | 沼泽 |
  | 9 | (1, 20) | 道路（工程扩展编号） |
  | 其他 | (1, 0) | 未知编号按陆面兜底 |

  v1 的「海拔 < 0 即湿」行为由读取侧湿判定（`wv_terrain::edit::is_water_cell`：
  surface 水 **或** 海拔 < 0）延续，无需在数据上翻案。
  实现唯一入口 `wv_io::terrain_bin::migrate_v1_kind`，与 `wv_terrain::biome`
  冻结表的一致性由表驱动迁移测试钉死。
- **写一律 v2**：`write_terrain` 回写 `RasterRef.format_version: 2`。

---

## 3. 版本策略

| 版本 | 变更 | 兼容 |
|---|---|---|
| `1` | 初版：`(i16, u8)` 双通道 + zstd | — |
| `2` | 三通道 `(i16, u8, u8)`（surface/biome） | 读 1（迁移）与 2，写 2 |

- ~~读取时 `version != 1` → `IoError::Binary`，**拒绝加载**（不做静默降级）。~~
  **修正（2026-10-18，§2.1）**：v2 落地后读取接受 1 与 2，其余版本照旧拒绝；
  v1 走只读迁移分支，v1 记载（§2）不因此改写。
- 未来若载荷布局变化，**升 version 并保留旧版本解码分支**；
  不得原地改写既有版本的语义。

---

## 4. 元数据引用（`RasterRef`）

索引 YAML（`maps/<id>/index.yaml`）中**只存引用**，不内联数据：

```yaml
raster:
  path: maps/world_main/terrain.bin
  encoding: i16
  width: 1024
  height: 1024
  chunk: 256
  format_version: 1
  checksum: 3f2a9c1e     # CRC-32，`crc32()` 输出的小写十六进制
```

- 读取时校验 `checksum`；不符 → 报错（检测文件被外部改动）。
- 读取时校验文件内 `width/height` 与索引声明一致性；不符 → 报错。

---

## 5. 性能门禁（基线 §2.19）

| 指标 | 阈值 | 超阈处置 |
|---|---|---|
| 单张栅格格数 | `width * height <= 16,777,216`（4096²） | **拒绝加载并报错**，要求分块 |

门禁在**编码与解码两侧**都检查（`encode_terrain` / `decode_terrain`），
防止「索引声明合规、文件实际超限」的绕过。
