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

## 3. 版本策略

| 版本 | 变更 | 兼容 |
|---|---|---|
| `1` | 初版：`(i16, u8)` 双通道 + zstd | — |

- 读取时 `version != 1` → `IoError::Binary`，**拒绝加载**（不做静默降级）。
- 未来若载荷布局变化（如新增第三通道），**升 version 并保留旧版本解码分支**；
  不得原地改写 version 1 的语义。

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
