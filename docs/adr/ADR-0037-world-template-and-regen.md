# ADR-0037（草案）: 世界模板（冷北/干旱/默认海洋）+ 完全重生成

- **状态**：已接受（Accepted，2026-10-19，用户按推荐拍板 A1+A1+A3 +「模板可以完全重新生成」）
- **关联**：ADR-0007（外部地理数据；模板走离线生成不与之冲突）、ADR-0029（生物群系冻结表）、ADR-0030（POI rank 与密度预算）、ADR-0036（默认观感已就位，本 ADR 是它的内容侧）、ADR-0017（标签层，模板城市初始标签走同一条管线）、README §12-40（"默认海洋"悬而未决项）

## 处境

M1（ADR-0036）把渲染观感做到位：全局档有装饰、有浮雕、有沙漠。但新建工程仍然是空海洋（`MapBase::Ocean`），用户第一次打开工具时看到的是一张蓝图——需要手刷或外部导入才能开始。参考图是"一打开就有一块大陆"，这与目前的产品体验差距明显。

`crates/wv-app/examples/gen_jrpg_world.rs` 已经验证了「冷北—绿中—干旱南」生成算法的可行性（1024² ≈ 1.3MB 落盘）。M2 要把它从一次性工具升级为新建工程的**模板选项**，并允许作者完全重生成。

## 决策

**D1 · 三个模板**

| 模板 | 落盘结果 | 适用范围 |
|---|---|---|
| `Ocean`（默认，沿用） | 1024² 全海 0 高度格 | 作者只想要一张空海图 |
| `ColdNorth`（新） | 冷北—绿中—干旱南双大陆（`gen_jrpg_world` 的算法搬家） | 想要 JRPG 参考图式起点 |
| `AridBelt`（新，预留） | 单一赤道带大陆，南北全海 | 后续可加，不在本 ADR 实现 |

**D2 · 入口（Q1=A）**

新建工程对话框加「模板」下拉（与"地图"下拉并列），选项 = D1 三档。
选 `ColdNorth` 时：写 `terrain.bin` + 在 `world.yaml` 写一个 `template` 字段（含 `template_name=cold_north` / `seed=<u64>` / `rivers_seed=<u64>`）。`Ocean` 不写 `template` 字段（V1 兼容）。

**D3 · 气候轴（Q2=A）**

承认**单一纬度温度轴**：`lat = 1.0 - yn`（yn=0 是北）。群系落表：

```
height >= 2400   → SNOW
height >= 1250   → MOUNTAINS
height >=  620   → HILLS
lat >= 0.86  → SNOW
lat >= 0.74  → TUNDRA
east_arid 0.52+ → DESERT
east_arid 0.34+ → GOBI
forest noise 0.55+ → FOREST
forest noise 0.44+ → GRASSLAND
else → PLAINS
```

（直接搬 `gen_jrpg_world::classify`，函数体进 `wv_terrain::climate::classify_band`。）

「east_arid」是模板的内部参数（西陆恒 0、东陆按 y 居中带算），**不暴露给作者**。第二期若加「干燥度轴」再开 `east_arid` 走更通用的双轴。

**D4 · 完全重生成（Q3=A + 重生成）**

- 模板 = 一次性种子 + 一次性落盘。`save_world` 写出来的 `terrain.bin` 与现有一致——作者手刷盖掉就走现有 UX。
- **完全重生成**走新 `RegenWorldCommand`：命令面板入口「重新生成世界地形」（同种子同气候带 = 同结果；用户可改 `seed` / `rivers_seed` 重抽）。命令是单事务原子应用，进 `History` 走撤销。
- 旧 `terrain.bin` 落 `terrain.bin.bak.<unix_ts>` 一份（最多保留 5 份，超出删最旧），避免误改。
- `template` 字段**不被命令覆盖**——重生成只换 `terrain.bin` + 河流 Place 的 `envelope`，不动地图节点/疆界/事件。

**D5 · 河流初生成（M2-2，2026-10-19 实现期修订：派生数据，不落盘）**

- 提河链骨架：高地源头（≥700m）+ 8 邻域最陡下降 + 种子抖动蜿蜒；入海 / 入坑即止，细流丢弃。算法收口在 `wv_terrain::climate::generate_rivers`（纯函数）。
- **落地形态（实现期修订）**：河流是 `terrain + rivers_seed` 的**确定性派生**，归一化折线由 `compute_projection` 现算进 `ProjectionFrame.river_polylines`（`river_key` 缓存，世界 / 换图变更才重算），渲染走新 `PassId::Rivers` 合批描线。**不进 `place_geometry`**——草案原打算存 `GeometryData::River` span，但 `place_geometry` 的 owner 必须是 Place（孤儿 owner 会触发 `feature-orphan-territory` 诊断噪声），且派生形态符合「只存参数」（§4.2）与 P7；代价是作者暂不能**手改单条河**（手绘河流走 ADR-0010 作者数据路径，后续另立）。
- **不**自动写 Place——只画河，不放城市。
- 河流 seed 独立于地形 seed（`rivers_seed`）；重生成换种子河流自动跟随，零存储迁移。

**D6 · 城市初布点（M2-3 单独子任务，ADR 占位）**

- 群系密度撒 6 城市（rank 2）+ 2 大城（rank 3），全部走 `AddPlaceCommand` 进 `History`。
- 默认位置可手动拖（无 AI 优化，纯种子散布）。
- 城市名走 placeholder（`city_<i>`），由作者手动改——自动化起名超出本 ADR。

**D7 · 默认示例工程（M2-4 单独子任务）**

把现 `examples/jrpg-world` 升为「冷北模板」官方示例：放在仓库随发行；首图加 `template: cold_north` 字段；提供 `gen_jrpg_world` 一次性工具与文档说明。

## 理由

- **Q1=A**：新建工程是用户的零号动作，模板下拉最贴合"打开就能看到东西"的诉求；命令面板 B 项要多走一步，C 项违背「用户一进软件就该看到自己的工程」的产品定位。
- **Q2=A**：单温度轴已能渲染参考图的「冷北—干旱南」；双轴（B）首次实现成本翻倍，且对作者"调"的能力边际收益低；四档简化（C）锁死粒度，反而更难扩展。
- **Q3=A + 重生成**：模板=种子语义最干净（一次落盘、走现有 UX）；重生成是用户**显式**触发的高成本动作（命令面板入口），不该在「新建/打开」时自动跑。落 `terrain.bin.bak.*` 是因为重生成是破坏性的，必须给作者退路。
- **河流/城市初生成走单独子任务**：与模板落盘解耦，作者用 Ocean 模板时不应被自动写河/城「污染」。

## 代价

- 新建工程对话框多一行下拉，UI 测试要加。
- `world.yaml` 多一个 `template` 字段，旧工程零迁移（V1 兼容：字段缺失 = Ocean 默认）。
- 重生成命令要走 `History` 撤销栈 → `EditCommand` 新变体 + `drain_commands` 测试。
- `terrain.bin.bak.*` 占用磁盘；上限 5 份由工具自查清理。
- 河流 `GeometryData::River` 变体可能影响 `wv_render` 的几何分类（待勘察）。

## 被否决的备选

- **B：独立「生成世界」命令**——和新建工程分两件事，多走一步。
- **B：温度+干燥度两轴**——首版成本翻倍；扩展收益低。
- **B：模板生成落为独立底图层**——多一层 + 优先级机制，与现图层架构冲突。
- **C：模板 = 八参预设**——把"模板"翻译成"八参"是过度抽象，作者只想"换一份世界"，不想理解 ContinentParams。

## 落地计划（待你拍板后写 superpowers 计划文件）

| 任务 | 内容 | 范围 |
|---|---|---|
| **M2-1** | 抽 `wv_terrain::climate::{generate_cold_north, classify_band}`，新建工程对话框加模板下拉，Ocean/ColdNorth 两档，演示工程升为「冷北模板」 | 1–2 天 |
| **M2-2** | 河流初生成：`GeometryData::River` 变体 + `gen_rivers(&TerrainData, seed)` + Place 不写、`place_geometry` 落多边形 | 1 天 |
| **M2-3** | 城市初布点：群系密度撒 6+2 城市，`AddPlaceCommand` 进 `History` | 半天 |
| **M2-4** | `RegenWorldCommand` + 落 `terrain.bin.bak.<ts>` + 命令面板入口 | 半天 |
| **M2-5** | `examples/jrpg-world` 升为「冷北模板」官方示例 + `gen_jrpg_world` 重命名 `gen_cold_north` 并只导出 | 半天 |

每个任务结束都跑 §7.1 全门禁（cargo test / clippy / fmt + app-test / app-clippy / smoke-gui）。

## 待裁

- 河流 `GeometryData::River` 在 `wv-render` 的几何分类（Isoline / VectorFlat / RealTerrain 三种视图模式下如何渲染）。M2-2 内解决。
- `template` 字段在 `wv_io` 加载时的兜底行为：缺失 = Ocean（V1 兼容）。
- 城市名是否走 `place_geometry` 内的 locale 派生（zh/en）。本 ADR 不做，由 M2-3 单独裁。
