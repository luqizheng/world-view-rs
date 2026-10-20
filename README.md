# World View

> 以地理为底、时间轴为骨、人物与事件为魂的单机世界编辑器。

**版本** v0.1（草案） · **状态** 项目起点，用于对齐方向，后续可迭代

---

## 1. 这是什么

World View 是一款**单机优先、目录化存储、YAML 驱动**的世界观构建工具。

面向对象：

- 网络小说作者
- TRPG 主持人
- 游戏策划
- 世界观创作者

**它要解决的问题：** 创作者脑中通常同时存在地理、时间、人物、事件四条线索，但它们散落在笔记、脑图、表格里，彼此无法互相校验。写小说时常见的问题是——

> 「这个角色三天前还在北境，今天怎么就到南疆了？」
> 「这个国家在 300 年建国，为什么 280 年就有它的使节出使？」

World View 把四个维度放进同一个工程里，让它们互相引用、互相驱动、互相验证。

---

## 2. 五条维度

它们不是五个独立模块，而是**同一份世界数据的五种视图**。

| 维度 | 内容 | 关键约束 |
|---|---|---|
| **地理空间** | 世界 → 区域 → 城市 → 建筑，逐层引用；图层化编辑 | 层级间通过 ID 引用，不重复存储 |
| **时间维度** | 纪元、单位、锚点；历史事件的发生时间与状态变化 | 拖动时间轴，地图/人物/势力随之变化 |
| **人物维度** | 基础信息、性格、自定义属性；关系网 | 人物状态随时间变化，不存「当前值」 |
| **事件维度** | 时间、地点、参与人物、结果；**事态**（事件的组织层 / 归纳容器）与**单位**（在地图上运动的主体） | 事件驱动状态变化；状态变化反向约束事件合理性 |
| **距离维度** | 地点构成图，边带距离与地形；交通工具定义速度与条件 | **不存结论，只存参数** |

---

## 3. 七条不可动摇的设计原则

> 改任何设计前必须先理解以下原则。**违反任一条的方案不予采纳。**

**原则一 · 单机可运行**
无需服务器，直接读写本地目录。不依赖网络、不依赖账号、不依赖云。

**原则二 · 目录即工程**
一个文件夹 = 一个完整世界工程。可拷贝、可压缩、可 Git 版本管理。工程内部不应有隐藏状态存在于目录之外。

**原则三 · YAML 为主格式**
元数据、配置、矢量与所有对象一律 YAML。人类可读、可手动编辑、可 Git 版本管理。

> 唯一例外：超大地图的网格数组（地形/高度）允许以二进制分块作为**一等源格式**（体积与加载性能所需）。但必须同时满足三条：
> 1. 遵循《二进制格式规格》；
> 2. 在索引 YAML 中显式声明；
> 3. 元数据使用二进制**引用**，不得内联。

**原则四 · 多地图层级**
世界 → 区域 → 城市 → 建筑/室内，逐层引用。层级之间通过 ID 引用，不重复存储。

**原则五 · 图层化编辑**
地形、疆界、影响力、道路、事件点等分层。每层可独立开关、锁定、调透明度、调整顺序。

**原则六 · 非破坏编辑**
笔刷/选区可撤销、可重做、可保存快照。编辑不是覆盖，是可回溯的操作历史。

> **边界（2026-10-11 裁定）**：本原则约束的是**存储层实现**——用区间增量存储、不用覆盖式存储把旧值弄丢。
> 它**不是**「禁止作者修改历史」。作者可以回头改已发生事件的参与者/时间/地点，工具不阻止、不追问。
> 详见 `AGENTS.md` §7.5。

**原则七 · 时间轴驱动**
拖动时间轴，地图图层、人物状态、势力范围随之变化。历史状态以**时间区间**存储，当前视图是某时刻的**投影**，而非直接修改的数据。

---

## 4. 架构：数据与渲染分离

World View 使用 Bevy 作为渲染与交互引擎，但**世界数据不存放在 Bevy ECS 组件里**。

```
        ┌──────────────────────────────────────┐
        │  wv-app   应用层（Bevy 渲染 / UI / 输入）│
        │  把编辑命令转发给 wv-edit               │
        └───────────────┬──────────────────────┘
                        │ EditCommand
        ┌───────────────▼──────────────────────┐
        │  wv-edit  编辑层（命令 / Undo-Redo / 事务）│
        └───────────────┬──────────────────────┘
                        │
        ┌───────────────▼──────────────────────┐
        │  wv-io    IO 层（YAML / 二进制 / 快照）  │
        └───────────────┬──────────────────────┘
                        │
        ┌───────────────▼──────────────────────┐
        │  wv-core  数据层（纯 Rust，零 Bevy 依赖）│
        └──────────────────────────────────────┘
```

**理由：** YAML 是 source of truth，ECS 只是投影。这样数据层可以独立测试、独立跑 CLI、独立做校验器。

| Crate | 职责 | 依赖约束 |
|---|---|---|
| `wv-core` | 领域模型：Map、Timeline、Character、Relation、Event、Place、Edge、Vehicle；时间点/区间；行程计算；路径搜索 | **禁止依赖 Bevy 及任何渲染库** |
| `wv-io` | 工程目录加载/保存、YAML 序列化、二进制分块、快照、Schema 迁移 | 唯一允许触碰文件系统 |
| `wv-edit` | 编辑命令 trait、Undo/Redo 栈、事务、脏数据集合、写回策略 | 不直接读盘 |
| `wv-app` | Bevy 应用、地图渲染、UI、输入处理、时间轴求值触发刷新 | 不持有数据、不直接读盘改盘 |

---

## 5. 工程目录结构

```
MyWorld/
├── world.yaml                 # 工程索引：名称、版本、schema version、地图树、时间轴范围
├── maps/
│   ├── world/
│   │   ├── index.yaml         # 层级、引用子区域、图层清单
│   │   ├── terrain.bin        # 二进制分块（可选）
│   │   ├── borders.yaml
│   │   ├── roads.yaml
│   │   └── layers.yaml        # 图层开关/顺序/透明度/锁定
│   └── region_xxx/
├── timeline/
│   ├── axis.yaml              # 纪元、时间单位、锚点
│   ├── events/
│   │   └── <id>.yaml
│   └── situations/
│       └── <id>.yaml          # 事态：事件的组织层（归纳容器）
├── units/
│   └── <id>.yaml              # 单位：在地图上运动的主体（ADR-0004）
├── tracks/
│   └── <id>.yaml              # 轨迹：运动主体是 unit，事态节点只引用它
├── characters/
│   ├── index.yaml
│   └── <id>.yaml
├── relations/
│   └── graph.yaml
├── distances/
│   ├── places.yaml            # 地点节点
│   ├── edges.yaml             # 地点之间的边
│   └── vehicles.yaml          # 交通工具
└── snapshots/                 # 非破坏编辑的快照
```

---

## 6. 关键设计决策

### 6.1 时间区间存储

历史状态不以「当前值」存储，而以「带时间区间的状态片段」存储：

```yaml
- region: kingdom_a
  from: { era: 1, year: 300 }
  to:   { era: 1, year: 450 }
  geometry: [...]
```

当前视图 = 某时刻对全部区间的求值结果。这样「非破坏编辑」与「时间轴驱动」天然统一：**历史从不被覆盖，只被追加。**

### 6.2 距离不存结论

不存「A 到 B 要 3 天」，只存：

- 地点之间的物理距离与地形
- 交通工具的速度与条件

行程时间由计算得出。交通工具调整、地形修改、路径新增后，所有结论**自动更新**。

```text
有效日速度 = speed_kmh × terrain_mult × (daily_hours / 24)
耗时(天)   = distance_km / 有效日速度
```

`instant: true` 的交通工具（如传送阵）不参与速度公式，作为零耗时边处理。路径搜索使用 Dijkstra，边权为该段在可用交通工具下的最短耗时，节点状态扩展为 `(PlaceId, VehicleId)` 以支持**中途换乘**。

### 6.3 每实体一文件

角色、事件等实体，每个一个文件，而不是一个巨型文件。理由：Git diff 干净、冲突少、人可读、可手动编辑。

### 6.4 图而非表

距离维度使用图结构：地点是节点，路径是边。这样才能回答「路径可不可达」「最短时间是多少」「中途能否换乘」。

### 6.5 运动归于单位，事态只是归纳容器

会动的主体是**单位（Unit）**——一支队伍在地图上走，**不因为被写进某个事态才开始动**。事态（Situation）是 **group**：把若干事件与单位**归纳**到一起，它**引用**运动，**不拥有**运动。

因此 `Track.unit` 指向单位，事态节点通过 `tracks: Vec<Id>` **多对多引用**轨迹——「一段行军横跨两个事态节点该归谁」这个问题**不存在**。

> 详见 `docs/adr/ADR-0004-unit-ontology.md`。

---

## 7. 时间轴与视图求值

这是项目的核心机制。

1. 每个图层的数据以「带时间区间的状态片段」存储。
2. 时间轴滑块选定时刻 `t`。
3. `TimelineQuery::at(t)` 遍历所有区间，求出该时刻的投影。
4. 投影结果更新到 Bevy 的渲染资源。
5. 拖动时间轴 → 重新求值 → 视图刷新。

> **推论：** 编辑历史不等于修改历史。作者在时刻 `t` 编辑，实际是**追加或修改某个区间**，而不是覆盖。

---

## 8. 技术选型

| 层 | 技术 | 说明 |
|---|---|---|
| 渲染与交互 | Bevy | 引擎 |
| UI | bevy_egui | 工具类应用，egui 更合适 |
| 数据模型 | Rust + serde | 纯数据层，不依赖 Bevy |
| 主格式 | YAML / serde_yaml | — |
| 二进制分块 | bincode 或 rkyv + zstd | 大数组 |
| 反射/序列化 | serde（优先） | 保证 YAML round-trip 稳定 |
| 版本管理 | Git | 目录即工程 |

关于 `bevy_reflect`：**优先使用纯 serde**，因为 YAML round-trip 的稳定性比反射便利更重要。

### 8.1 打包字体（`crates/wv-render/assets/fonts/`，编译期嵌入）

| 字体 | 版本 | 许可 | 用途 |
|---|---|---|---|
| **Inter** Regular | 4.001 | SIL OFL-1.1 | UI 拉丁（`Proportional` 首位） |
| **Noto Sans SC** Regular | 2.004 | SIL OFL-1.1 | UI 中文（回退）+ 画布标注兜底 |
| **霞鹜文楷（LXGW WenKai）** Regular | 1.530 | SIL OFL-1.1 | **画布标签书法体**：地名 / 街道名 / 测量标注（ADR-0042 D4） |

三份字体全部为 SIL Open Font License 1.1（可自由嵌入 / 再分发）。
来源、更新方法与获取直链见 `crates/wv-render/assets/fonts/README.md`；
字体装配决议见 ADR-0019（Inter + Noto）/ ADR-0042 追记（文楷）。

---

## 9. 开发路线图

按优先级排列，每一步都应可独立运行、可验证。

> **当前进度**（对齐基线 `docs/MAP_EDITOR_BASELINE_v1.md` v1.1，2026-10-11）：
> 本节按**能力里程碑**组织（阶段一至七），与基线内的 Phase 划分（Phase 1 设计/架构 → Phase 2 美术/动画/特效/音效 → Phase 3 可运行原型 → Phase 4 质量监管）是两套视角，**结构保持不动**，仅在下方如实标注已落地能力。
>
> - ✅ **Phase 1（设计 + 架构）已完成**：产出 `docs/MAP_EDITOR_BASELINE_v1.md`（v1.1，1558 行），含 5 项用户裁定 D1–D5。
> - ✅ **Phase 2（美术 / 动画 / 特效 / 音效）已完成**：产出 4 份规格——`ART_STYLE_SPEC_v0.1.md`（593 行）/ `ANIMATION_SPEC_v0.1.md`（712 行）/ `VFX_SPEC_v0.1.md`（936 行）/ `AUDIO_SPEC_v0.1.md`（522 行）。
> - ✅ **Phase 3（可运行原型 S0–S9）已完成**：GUI 可运行（Bevy 0.15 + bevy_egui 0.32），测试基线 **83 passed / 0 failed**，`clippy -D warnings` 与 `fmt --check` 干净。
> - ⬜ **Phase 4（质量监管 / 验收）待启动**：验收清单 + 风险定级（对照基线 §1.18 的 V15–V37）。

**阶段一 · 数据层打底** ✅
- [x] `wv-core` 定义全部领域模型
- [x] YAML round-trip 测试
- [x] CLI：`wv load <dir>` 打印世界摘要

**阶段二 · 最小可视应用** ✅
- [x] Bevy + egui 骨架
- [x] 加载工程，显示地图树、角色列表、时间轴滑块
- [x] 暂无地图渲染

**阶段三 · 地图渲染** ✅
- [x] 世界地图外框 + 地点标记
- [x] 图层开关与顺序

**阶段四 · 编辑与撤销** ✅
- [x] 编辑命令 trait
- [x] Undo/Redo 栈
- [x] 脏数据集合与写回

**阶段五 · 时间轴驱动** ✅
- [x] 时间区间求值
- [x] 拖动滑块刷新视图

**阶段六 · 距离与校验** ⬜ 待启动
- [ ] 交通工具与地点边
- [ ] 路径搜索（含换乘）
- [ ] 事件间位移校验
- [ ] 校验结果可视化

**阶段七 · 细化** 🟡 部分落地
- [x] 二进制分块（地形 `terrain.bin` 读写已随 S2 落地）
- [ ] 快照
- [ ] Schema 迁移

---

## 10. 快速开始

```bash
# 编译数据层与 CLI（不含 Bevy，秒级完成）
cargo build

# 运行测试
cargo test --workspace

# 加载一个工程并打印摘要
cargo run -p wv-app -- load examples/minimal-world

# 校验工程结构
cargo run -p wv-app -- validate examples/minimal-world
```

图形界面需显式开启 feature（Bevy 依赖树庞大，阶段一不应对其产生构建依赖）：

```bash
cargo run -p wv-app --features app -- gui
```

### 10.1 用 LLM 控制编辑器（MCP，ADR-0014）

编辑器内置一个 **MCP `2025-03-26` HTTP server**（仅 `127.0.0.1`，默认端口 `8732`），让 Claude Desktop / Cursor / Zed / Continue 等支持 MCP 的 LLM 客户端能**读取、修改、保存**当前打开的世界。设计决策见 [`docs/adr/ADR-0014-mcp-http-server.md`](docs/adr/ADR-0014-mcp-http-server.md)。

**① 启动带 MCP 服务的编辑器**（`http` feature 隐含 `app`，窗口照常打开）：

```bash
# 默认 127.0.0.1:8732
cargo run -p wv-app --features http -- gui

# 自定义端口 / 绑定地址
cargo run -p wv-app --features http -- gui --http-port 9000
cargo run -p wv-app --features http -- gui --http-bind 127.0.0.1 --http-port 9000
```

启动后窗口内先「新建工程」或打开一个工程（LLM 的读写作用于**当前打开**的世界；未开工程时 `save_project` 会报错）。日志出现 `[wv-app http] MCP server 监听 127.0.0.1:8732（POST /mcp）` 即就绪；端口被占时进程报错退出（**不**自动换端口），换一个 `--http-port` 再启动。

**② 配置 MCP 客户端**（任选一种）：

- **Claude Desktop / Cursor / 通用 JSON 配置**（streamable HTTP 形态）：

  ```json
  {
    "mcpServers": {
      "worldview": {
        "url": "http://127.0.0.1:8732/mcp"
      }
    }
  }
  ```

  写进 `claude_desktop_config.json`（Claude Desktop）或 `.cursor/mcp.json`（Cursor）后重启客户端即可。

- **curl 直接调**（调试 / 脚本）：

  ```bash
  # 列出 9 个 skill 的 schema
  curl -s http://127.0.0.1:8732/mcp -X POST -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'

  # 调用：列出所有地图摘要
  curl -s http://127.0.0.1:8732/mcp -X POST -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"list_maps","arguments":{}},"id":2}'
  ```

  真机烟测一键跑通三条链路（9 skill schema / 读路径 / 错误路径）：

  ```bash
  bash scripts/smoke-http.sh        # 默认用 18732 端口，避开你可能已在跑的 8732
  ```

**③ 9 个 skill**（`tools/list` 会给出完整入参 schema）：

| 读（不修改世界） | 写（经撤销栈，可 Ctrl+Z 撤销） |
|---|---|
| `list_maps` — 地图摘要列表 | `create_map` — 新建地图节点 |
| `get_map` — 单图完整节点 | `create_place` — 新增地点 |
| `list_places` — 地点摘要（支持过滤） | `undo` — 撤销最后一次事务 |
| `get_place` — 单地点详情 | `save_project` — 全量保存到工程目录 |
| `get_state_at_time` — 某时刻的世界快照 | |

列表类 skill 返回**轻量摘要**、`limit ≤ 100` 超出分页；详情用 `get_*` 单条取——避免撑爆 LLM 上下文（ADR-0014 §2.5.1）。

**④ 安全与语义边界**：

- 服务**仅监听 loopback**，局域网不可见；无鉴权（本地单机场景，鉴权留待 LAN 支持时另开 ADR）。
- 所有写操作经编辑命令栈（`EditCommand → History`），与 GUI 的 Ctrl+Z **共用同一撤销时间线**——LLM 改坏了直接 Ctrl+Z；任何改动在 `save_project`（或 GUI Ctrl+S）之前不会落盘。
- 校验只提示不阻止（悬空引用等照常可在校验面板看到）；`save_project` 不触发校验。

---

## 11. 术语表

> **本项目的术语唯一权威源是 [`GLOSSARY.md`](GLOSSARY.md)。**
> 此处不再重复维护术语定义——同一术语在两处各写一份必然漂移（本项目已发生过：本表此前的 `TimePoint` 定义仍是时间模型重做前的旧口径）。
>
> 登记规约见 `AGENTS.md` §6.1「术语即时登记」，完整词条见 `GLOSSARY.md`（核心术语 / 事态与编辑 / 动效特效音效 / 设计上不做 / 作废降级 / 待决）。

---

## 12. 待决问题

以下问题尚未定论，后续设计时需要回答：

1. **路径是否允许中途换交通工具？** 这决定 Dijkstra 的状态空间。（当前骨架已按「支持换乘」建模）
2. **时间精度到哪一级？** 年到日、到小时、还是更细？（当前结构可选存到分钟）
3. **`requirements` 的表达能力：** 需要一个小的条件表达式引擎，语法范围待定。
4. **二进制分块的格式版本策略：** 如何声明与迁移。
5. **渲染方式：** 矢量路径自建 Mesh，还是走 `bevy_sprite`。
6. **UI 交互模型：** 作者是在地图上直接编辑，还是在属性面板编辑。
7. **多世界引用：** 是否支持一个工程引用另一个工程。
8. **带洞多边形：** 第一版**不支持**。作者用多 sub-span 近似（如「环形地带」拆成若干块），未来可加 `PolygonWithHoles` 变体。（待裁原因：势力层的几何是多边形集合，而真实的行政区可能带洞——如「被城郭包围的飞地」。）
9. **反向索引失效策略：** 第一版按「**每次查询时重建**」实现（N 小时可接受）。（待裁：N 超过 1 万时是否改为增量维护。基线 §2.5 已标注 `TODO(阶段X)`。）
10. **地形双通道编码：** I16 高度 + U8 类型两个通道，是否合并为单个 `terrain.bin` 文件。（涉及二进制格式设计与 `docs/BINARY_FORMAT_SPEC.md` 的对应条目。）**2026-10-14 追记**：高度值语义已裁——1 单位 = 1 米、海平面 = 0、负值水下（`docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §8 · Q1）；本条剩余待裁仍为「两通道是否合并为单个 `terrain.bin`」。**【2026-10-14 收敛】**：实现已在 `crates/wv-io/src/terrain_bin.rs` 落实——`WVTR` magic + `format_version = 1` + `width × height` + `zstd(bincode(Vec<(i16, u8)>))`，两通道合并为单个 `terrain.bin`、门禁 4096²、`crc32` 校验和（`RasterRef.checksum`）。详见 plan v0.1 §6 M0 对账 §5。本条关闭。
11. **单位本体相关（`docs/adr/ADR-0004-unit-ontology.md` §待裁 U1–U7）：** 加载期硬失败 vs 编辑期软提示的强度统一（U1）／「相遇提示」的留痕与回访形态（U2）／`Situation.units` 落地时机（U3）／低 LOD 事态标签的防重叠（U4）／`Character.died_at` 卒年（U5）／`Event` 的「过程描述」字段（U6）／事态嵌套的落地条件（U7）。
    - **2026-10-12 修整**：与 `ADR-0004` §待裁 同步——U1（加载硬失败 vs 软提示强度）/**U2（相遇提示留痕与回访）**/**U3（Situation.units 落地）**/**U5（Character.died_at）**/**U6（Event 过程描述字段）** 均已闭合，决议散见 `ADR-0004` §待裁 / `ADR-0005` 决策（5）。**U4（事态标签防重叠）** 与 **U7（事态嵌套）** 仍为待裁。本行做的是**文档一致性**而非新决议——以前散落在本 README 与各 ADR 的「已闭合」标注有 drift，按 `AGENTS.md` §6.3「追加、不修改」补记一节。
12. **地图上的空间落点（子图入口与标记）**：第一版按「**`MapNode.icons` 里带 `drill_into` 的图标即子图入口**」实现——锚点归**每个图标自己**所有（`IconSpec.anchor`，本图 `extent` 内的归一化 `[0,1]²`），`MapNode.children` 仍只存 `Id`，原则四「只存引用」不受影响。未声明 `anchor` 的图标按 `id` 确定性哈希摆位（同次运行稳定、可复现）。（待裁：(a) 哈希回退是否长期保留，还是最终应强制作者为每个子图显式声明 `anchor`——收敛点是 `wv-app` 的 `drill::resolve_markers` ② 分支，改动面可控；(b) 是否把「子图入口」从 `icons` 里拆成独立字段——当前 `icons` 同时承担「注释标记」与「钻入入口」两种语义。涉及 `docs/MAP_EDITOR_BASELINE_v1.md` §5 的坐标系统条目与 `docs/STYLE_SWAPPABLE_ARCHITECTURE.md` 的地图层级渲染条目。）
13. **【已收敛 · 2026-01】U6「Event 的过程描述字段」**：原 U6 待裁已收敛为——`Event.narrative`（单句过程）+ `Narration` 独立实体（`timeline/narrations/`，多段旁白，`Event.narrations` 多对多引用）。因果链的可视化**用旁白文案 + 时间派生播放顺序表达，不设箭头字段**（ADR-0002 排序口径不变）。历法显示采用双历制 `CalendarKind{simple, gregorian}`（`docs/adr/ADR-0006-gregorian-calendar.md`）；Place/Event 落点矩形几何化见 GLOSSARY「事件落点 / 矩形地点」条目。示例工程 `examples/xinhai-1911/` 为上述能力的综合演示。
14. **地形栅格图层的瓦片化（2026-10-11 提出）**：地图拆为**矢量图层**（地点/道路/边界/势力/事件叠加，类比百度矢量地图）与**地形栅格图层**（底图）两层。栅格底图升级为 **JRPG 瓦片图集**渲染：按 `f(kind, height, 邻居)` 查图集选瓦片。待裁：(a) 图集素材来源——先 AI 生成素材包验证管线，还是直接采用 CC0 素材（Kenney / 0x72）；(b) 默认底图策略——编辑模式默认色块、浏览模式默认瓦片是否成立；(c) 过渡瓦片（autotiling）的落地时机。第一版按「直铺整瓦、无过渡、图集路径可配置」实现，收敛点为 `wv-app` 渲染层的瓦片选择函数，`wv-core`/`wv-terrain` 不变。
    - **(b) 已收敛（2026-10-14，[ADR-0012](docs/adr/ADR-0012-terrain-default-vector-flat-shading.md)）**：浏览态默认底图 = **等值线矢量平涂**（既非瓦片也非色块）；瓦片图集与亚像素纹理均降为可切换模式（归 `TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` D3 的 B / C 线）。(a) 素材来源与 (c) 过渡瓦片仍待裁，归模式 B 线不受影响。
15. **真实地理底图引入路径（ADR-0007，2026-10-13 已接受）**：以 Overture Maps 作为真实地理底图（[外部资源](https://github.com/OvertureMaps)，非引入 OSM PBF 服务端转换）。决议：(a) ✅ 引入 Overture Maps 作为外部真实地理资源；(b) ✅ 领域模型 `Place` / `Edge` 只存 `OvertureId`（GERS 字符串）引用、**不落几何**；(c) ✅ 引入新平行 crate `wv-overture`（与 `wv-io` 同级、不依赖 Bevy）作为外部资源访问入口，而非 `wv-io` 子模块；(d) ✅ `Map.geographic_source: Option<GeographicSourceV1>` 元数据字段（声明主题/bbox/路径）走 `RasterRef` 风格声明、不开 P3 例外口子；(e) ✅ **不加** `Map.geographic_kind: Real|Fictional` 字段——真假地图由 `Option` 的 `None`/`Some` 派生（避免枚举扩张，参见 ADR-0007 §决策 (2) 注脚）；(f) ✅ 字段命名 = **`overture_id`**（`gers_id` 因术语透明度差被否决、`real_world_id` 因与 `geographic_kind` 字段语义撞车被否决） + **§4.4 UUID 例外条款落地**（GERS 实测为 hex UUID，**仅 `OvertureId` 类型豁免** `AGENTS.md` §4.4 字符集约束，领域其余 ID 仍受约束）。关闭条件：ADR-0007「已接受」后据决议删/改本条（§6.2 不允许未决问题静默通过）。

    > **数据流注释（一次性投影，非同步）**——明确边界，避免误读为「同步嵌入 GPS」：
    >
    > ```text
    > ┌─────────────────┐  bbox 切片下载      ┌────────────────┐
    > │ Overture S3     │ ─────────────────► │ maps/<id>/data/ │
    > │ (GeoParquet)    │   CLI 一次性触发   │ *.parquet      │
    > └─────────────────┘                    └───────┬────────┘
    >                                                │ 按 overture_id 查询（只读）
    >                                                ▼
    > ┌─────────────────────────────────────────────────────────────────┐
    > │  wv-overture（运行时读取层，不写领域）                              │
    > │  - 读 Parquet → Haversine → 几何/路径元数据                       │
    > │  - segment_length_m() 工具函数（回归用例 440.8 m）                │
    > └───────────────────────────────┬─────────────────────────────────┘
    >                                 │ 单向注入（Overture → 领域字段）
    >                                 ▼
    > ┌─────────────────────────────────────────────────────────────────┐
    > │  领域模型（YAML 落盘形式）                                          │
    > │  PlaceEdge { distance_km: 0.44, terrain: "residential" }         │
    > │  Place { id: "space_needle", overture_id: "2f099609-...",          │
    > │          names: "Space Needle", basic_category: "landmark" }      │
    > │                                                                 │
    > │  ✅ 领域 YAML 里**没有几何坐标**                                  │
    > │  ✅ 仅有自定义字段（distance_km、terrain）和引用（overture_id）   │
    > └─────────────────────────────────────────────────────────────────┘
    > ```
    >
    > **关键边界**：
    > - Parquet 落在 `data/`，是「资源」不是「数据源同步」（**不**双向同步、**不**周期拉取）。
    > - Overture 几何坐标（Lon/Lat 列）**一次性** Haversine → `distance_km`，落进领域 YAML 后**不再保留**坐标。
    > - Overture `class` **一次性**转写 `terrain` 字符串，**不留** Overture 原值。
    > - `overture_id` **保留**作为引用（跨版本稳定指向同一实体）。
    > - **不会再写 Overture**——一进领域，不再回流 Overture 数据库。
    > - **运行时仍可能读 Parquet**——校验面板 / 渲染层按 `overture_id` 查询，是「只读路径」不是「写路径」。
    > - **不是几何投影**（WGS84→墨卡托那种）——是**语义意义**上的「转换」：GPS 坐标 → 领域距离参数，**不**是坐标系转换。
16. **外部 Parquet 仅作资源、不入 P3 例外**：当 Overture Parquet 进 `maps/<id>/data/` 时，**领域 YAML 仍只存 `OvertureId` 引用**，Parquet 文件作为外部资源存在，**不开 P3 二进制例外口子**（地形栅格仍是 P3 例外的唯一受益者）。`Map.geographic_source.data` 走 `RasterRef` 风格声明路径。关闭条件：`wv-overture` 第一版能跑 fetch + 渲染层几何转换。**【2026-10-13 已关闭】**：第一版已落地为「Overture 几何坐标一次性 Haversine → `distance_km`」，Parquet 走 `wv-overture::fetch::parse_geojson_segments`（GeoJSON 中间格式，因 `parquet 59` MSRV 不兼容 + `duckdb` 体积过大），领域 YAML 仍只存 `OvertureId` 引用。详见 ADR-0007 §决策 (5) + `crates/wv-overture/src/fetch.rs`。**待办残留**：`--emit-yaml` 输出**写回草稿**（V3）—— 不直写盘，遵循 AGENTS §7.5 V1「校验只提示不阻止」+ §3.3「System 不得改世界」。
17. **Parquet schema 升级策略**：Overture schema 升级时，`wv-overture` 转换表需同步；版本号如何存、迁移策略如何定（路径：`wv-overture` 内部 `overture_schema_version` 字段；不写入领域模型）。关闭条件：`wv-overture` 实现中明确版本号策略且 README/CLAUDE.md 已登记。**【2026-10-13 决议】**：版本号存 `wv-overture::OVERTURE_SCHEMA_VERSION` 常量（`crates/wv-overture/src/lib.rs`），**不**写入领域 YAML（ADR-0007 §决策 (6)「不开 P3 例外」），由 `wv-overture` 内部映射表（`class_to_terrain` / `should_keep_subtype`）与 Overture 官方 release 同步。CLI `--release <TAG>` 暂未真正传给 `overturemaps-py`，是**扩展位**（避免 AGENTS §9「依赖默认拒绝」之 `clap` 化与提前抽象）。
18. **检索路径是「领域 + 外部 Parquet」双源**：是否上本地索引？何时倒排？是否按 bbox 切表？检索时是否需走 SPI（Service Provider Interface）以支持多源（Overture / 自建 / 未来 OSM 直采）？关闭条件：ADR-0007 接受且 `wv-overture` 实现中明确索引路径。**【2026-10-13 部分关闭】**：检索**第一版**走**直读**（`wv-overture::fetch::parse_geojson_str` 不建索引），双源在「`Place.overture_id: OvertureId` 字段」+「`Place.id: Id` 字段」分离（UUID 例外条款）。**SPI 多源扩展**仍待裁——第一版**仅** Overture 一源；未来引入 OSM 直采 / 自建库时再开 ADR，本 README §18 不动。
19. **多级栅格的驻留策略**（ADR-0009 落地项）：每张图可以自带 `raster` 之后，同时驻留几级栅格？不能退化成「一张 67 MB × 层数」。待定的是：懒加载范围（只加载祖先链？还是祖先链 + 相邻级预取？）、同时驻留上限、卸载时机（切图即卸 / LRU / 邻域保留）。关闭条件：`wv-app` 的地图切换代码中明确该策略，并写进基线性能章节。
    > **待裁边界**：ADR-0009 已定「每图自带栅格 + 自带原点」**这一方向**不动；本条待决的只是**内存管理策略**，不影响数据模型。
    >
    > **进展（2026-10-13，Phase 2 落地）**：数据侧已按图存放（`World.terrains: BTreeMap<Id, TerrainData>`），`wv-io` 逐图读、按脏路径逐图写；`wv-app` 显示「当前图或它向上继承到的第一张」栅格。当前实现选**最容易回退**的形态：**加载时全部读入、不淘汰**——换懒加载时键值结构不必再改（要撤的是「何时填、何时清」）。故本条仍**未收敛**。缺省语义（无自带栅格 → 向上继承）见 ADR-0009 §7。
20. **地面尺度（一格 = 多少公里）的表示与来源**（ADR-0009 落地项）：2026-10-12 用户裁定（甲）撤除了 `Extent.width_km`，相遇判定也改为不依赖坐标距离，**该裁定不变**。但「缩放到街道」的可用性需要尺度：比例尺读数、`class` LOD 的尺度阈值、Overture 米制几何贴图都要用到它。两条路：真实地图由 `geographic_source.bbox` 派生（bbox 本身就是现成挂点）；虚构地图没有真值，只能由作者声明一个可选尺度参数。待定的是：虚构地图是否引入可选尺度字段、以及派生与声明冲突时谁优先。关闭条件：比例尺读数能显示真实公里数，且虚构地图有明确口径。
    > **待裁边界**：本节**不是**要恢复 `width_km`。是否恢复由本条收敛时决定，收敛前不得在代码中做不可回退的选择。
21. **街道几何的存放与规模阈值**（ADR-0010 落地项）：虚构世界的手绘路网放 `Layer.source` 指向的图层数据文件（YAML），真实世界走 Overture 只存引用——**方向已定**。待定的是：单个手绘图层文件容纳多少段（建议 2000–5000 段）之后分片、分片的目录与命名约定、以及分片文件是否也需要一个索引 YAML。关闭条件：第一版手绘街道编辑器能保存超过单文件阈值的路网且加载不退化。
    > **待裁边界**：**不**为街道几何开二进制例外（P3 的唯一例外仍是超大栅格网格数组，`AGENTS.md` §5）。若将来实测 YAML 无法承载，须**新开 ADR**，不得直接落实现。
22. **子图落点派生遇到多入口时的歧义**（ADR-0009 §6 追加项）：子图未声明 `Extent.origin` 时，落点由**父图上指向它的钻入图标**派生（图标 = 子图中心）。方向已定，待定的是歧义消解：父图有**多个** `drill_into` 指向同一子图且都带显式 `anchor` 时，今天取 `icons` 声明顺序里的第一个——是应当（a）保持「取第一个」，（b）由校验面板提示「本图有多个入口，建议显式声明 `origin`」（§7.5 V1：只提示不阻止），还是（c）定义入口优先级（如 `size` 最大者 / 带特殊图层者）。关闭条件：多入口子图的落点行为有明确口径，且校验面板能提示该情形。
    > **待裁边界**：无论选哪条，「显式 `origin` 压过派生」这条优先级不动；本条只影响**未声明** `origin` 时的兜底。
23. **渲染层不在测试闸门内**（流程问题，2026-10-13 发现）：`AGENTS.md` §7.1 强制门槛的三条命令——`cargo test --workspace`、`cargo clippy --workspace --all-targets`、`cargo fmt --all -- --check`——**都不会编译** `wv-app` 的渲染/UI 模块：它们全部是 `#[cfg(feature = "app")]`（`crates/wv-app/src/lib.rs:24-75`），而 `wv-app` 的 `default = []`。实测 `cargo test -p wv-app` 只跑出 `0 passed`（三个目标全空），改动 `drill.rs` / `vector_layers.rs` 这类文件时闸门完全沉默。待定的是门槛怎么补：给 §7.1 的三条命令加 `--features app`（构建代价明显上升，Bevy 依赖树要重编），还是在 §7.1 之外单列一条「渲染层检查」命令。关闭条件：§7.1（或新增条目）明确覆盖渲染层，且 CI/本地都按新命令执行。
  > **收敛（2026-10-13，用户裁定）**：§7.1 已单列渲染层两条命令（不带 feature 的闸门维持原样，Bevy 树不进常规构建），并配 `.cargo/config.toml` 别名 `cargo app-test` / `cargo app-clippy`。本条待决关闭。
    > **待裁边界**：本条**不改**任何代码；在收敛前，本轮已手动用 `cargo test -p wv-app --features app` 自行验证。改 `AGENTS.md` 属用户裁定的约束文档，须明确确认后再动。
24. **继承来的底图在街道级缩放的精度上限**（ADR-0009 §7 落地项）：图没有自带栅格时，底图**向上继承**最近一张（ADR-0009 §7 已定），代价是继承来的精度就是祖先的精度——世界图（20 km 见方 / 4096 格 ≈ 4.9 m/格）放到街道级缩放会糊成色块。待定的是：何时、以何种形式提示作者「本图需要自己的栅格或更高精度底图」（校验面板提示？状态栏读数？地图树上的角标？），以及是否需要一个「按父图 bbox 一键生成子图栅格骨架」的便利功能。关闭条件：街道级视图下作者能明确知道当前底图精度是否够用。
  > **进展（2026-10-13，切片落地）**：「按父图 bbox 一键生成子图栅格骨架」已实现——`wv raster-slice <工程> <map_id> [size]`（默认 512²），从来源图栅格按坐标链切出子图覆盖区域（最近邻采样，出界取海平面），落盘 `maps/<id>/terrain.bin` 并回写 `raster` 引用；切片后该图用上自己的底图（继承退场）。**提示形式仍未定**（本条剩余部分继续待裁）。
    > **待裁边界**：按 §7.5 V1，**只提示、不阻止**——不因底图精度不足而拒绝钻取或拒绝保存；也不为「精度」引入任何会阻断编辑的校验。
25. **换级的视觉过渡形式**（ADR-0009 §2⑦ / §8 落地项）：`§2⑦` 要求换级「只做缩放与淡化、不做位移插值」，但**淡化部分尚未实现**——今天换级是整张纹理重建的硬切换（`rebuild_texture_on_mode_change`）。待定的是：淡化用什么形式（新旧两张底图交叉淡入 / 仅新底图淡入 / 短暂降低不透明度），以及时长与是否可关。关闭条件：换底图时能看到明确的过渡而非瞬间跳变，且「不做位移插值」这条不变。
    > **待裁边界**：淡化**不得**引入位移插值（§2⑦ 已否决），也不得为了让过渡好看而延迟层级切换（切换必须同一帧生效，否则路径与画面不一致）。
    > **收敛（2026-10-14）**：形式定为「**旧底图整幅淡出**」——换图时新底图同帧即时重建（切换不延迟），旧底图以克隆像素的淡出精灵（z=0.05）在 `LEVEL_FADE_SECS = 0.4s` 内线性降透明至归零回收；换模式 / 换 soften 是「换眼镜」，不触发淡化。是否可关未做开关（0.4s 足够短，不构成干扰；需要时再登记）。详见 ADR-0009 §5 追写「换级淡化」。
26. **轨迹与事项事件坐标的所属帧**（ADR-0009 §9 落地项 · 2026-10-14 发现）：视图空间**就是**根图（世界）坐标系（基线 §2.15）；本图局部内容（地点 / 街道 / 街道名 / 地图外框）经 `GridConfig::map_to_view`（图帧原点）换算，而**轨迹 waypoint 与事件位置今天走恒等映射**（`crates/wv-app/src/projection.rs` 的 `root_to_view`）——即**一律按根帧解释**，数据里没有「这份坐标属于哪张图」的声明。世界级事件 / 轨迹因此天然正确，且这是**最容易回退**的形态（收敛点就是 `root_to_view` 与 `grown_polyline`）。待定的是：是否允许事件 / 轨迹按**子图局部**坐标书写（需先加所属帧字段，如 `Event.map` + 局部 `x`/`y`），以及城市内的事件在**父图视野**下该如何呈现（不显示 / 用钻入图标位置 / 按子图图帧原点折算）。关闭条件：数据模型对「坐标属于哪张图」有明确口径，`root_to_view` 的恒等映射不再是隐含约定。
    >
    > **勘误（2026-10-14）**：原文提到 `crates/wv-app/src/projection.rs` 的 `root_to_view` —— 该文件在 2026-10-14 的"文件名即用途"重构中已重命名为 `world_projection.rs`（见 `reduce-ai-reading-cost` spec，Task 2）。`root_to_view` 函数本身仍在；此处指向的文件名已变。
    > **待裁边界**：本条**不改**已定的两套原点口径（栅格原点负责地形、图帧原点负责本图局部内容，见 ADR-0009 §9）；它只补「根帧内容」这一类的所属帧声明。
27. **线宽是否按 LOD 分级**（ADR-0008 §8 落地项 · 2026-10-14 提出）：2026-10-14 修复「屏幕像素 → 世界单位」换算方向反了的问题（`px / view.scale` → `px * view.scale`，详见 ADR-0008 §8）之后，线宽在所有缩放级别恒为屏幕常量——世界级的城市连线从发丝线（0.6px）变为 6px casing + 3.5px fill 的醒目连线。待定的是：是否为道路 / 街道引入**按 `lod_tier`（Edit / Region / Nation）分级的线宽常量**（如 Nation 级把 casing/fill 各乘一个 <1 的系数），让远观更含蓄、近看保持清晰。关闭条件：世界级与街道级视图的线宽观感都经过确认，且有明确的分级口径（收敛点是 `vector_layers.rs` 的 `*_PX` 常量组，改动面小）。
    > **待裁边界**：换算方向（乘）**不动**——那是语义修复；本条只讨论「常量取值是否分级」。收敛前不得再把换算改回除法。
28. **示例工程 `examples/xinhai-1911/` 的范围澄清（AGENTS.md §6.3 追写，2026-10-14）**：
    - 示例世界图的标题是「**1911 年中华大地**」（`maps/world/index.yaml`），不是「通史中国」也不是「现代中国」——`examples/xinhai-1911/` 与 `name: 1911 年中华大地` 标题对应该目录涵盖的 1911 年清末因果链（保路 → 武昌 → 退位）。年代之外的内容不在本示例范围内。其他示例（如未来「贞观-628」「现代北京」）应使用对应时代范围的标题，不复用本名。
    - 示例世界图已声明 `Map.geographic_source: Option<GeographicSourceV1>`（ADR-0007 §决策 (2)）——`theme: places` + bbox 覆盖 1911 年清帝国内地 22 省核心区域。`geographic_source` **仅声明元数据**，**不**把 Overture 几何写进本 yaml；本工程仍以手写 Perlin 栅格做地形演示，`geographic_source` 在该示例中作为「声明性骨架」存在，不是已落地的真实几何数据。
29. **地形边界渲染与草图式生成参数族（2026-10-14 计划）**：方向（D1–D9）已定——等值线矢量平涂为浏览态地基、网格仅编辑态可见、多边形地物工具按「手势即命令」落地、派生几何永不落盘、生成 = (数据, 参数, 种子, 坐标) 确定性纯函数、地形**不时间轴化**（未来历史化须新开 ADR）。**Q1–Q17 已于 2026-10-14 全部收敛**（Q1–Q6 逐项裁定，Q7–Q17 用户授权批量采纳缺省建议），逐条裁决记录见 `docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §8，要点：高度 = 1 单位 1 米海平面 0；类型词汇两表分离（名称/色值留 Theme Pack，优先序/分带为语义表）；配方参数第一版用格数；等值线格心采样；类型优先序 深水<岸<沼泽<平原<沙漠<森林<丘陵<山地<雪原；鞍点角点仲裁；类型场 8 邻域；雪走生成期分带；**浏览态默认切等值线平涂（Q11 → ADR-0012 已接受，2026-10-14）**；模式 C 延后；类型写入分区处理；融合复用 Falloff；fBm 缺省参数；圈选 = 一条宏命令；配方四条起步。**进展（2026-10-14）**：Q11 触发的 ADR 落地已完成——[ADR-0012](docs/adr/ADR-0012-terrain-default-vector-flat-shading.md)（浏览态地形默认底图由亚像素纹理切到等值线矢量平涂）已接受，Q11 裁决门解除。**剩余收口 = M0 实现对账**（`plan v0.1` §6：差异清单回填 plan v0.1——含 `lib.rs: Height` 注释衔接、tile_atlas / terrain_texture / theme 色板词汇、`wv-io/terrain_bin.rs` 字节布局、ViewMode 挂点、示例工程高度值是否按米重标定）；M0 清单回填后即可关闭本条。
30. **LLM 可控的世界编辑（ADR-0014，2026-10-14 已接受）**：`wv-app` 新增 `http/` 模块走 MCP `2025-03-26` HTTP/SSE 仅 loopback（默认端口 8732），让外部 LLM 客户端（Claude Desktop / Cursor / Zed / Continue 等）能读 + 改 + 存当前打开的世界；写类 skill 强制经 `EditCommand → mpsc → Bevy 主帧 drain_commands → History.apply`（AGENTS §3.3 不破）；v0.1 上 9 个 skill（list_maps / get_map / list_places / get_place / get_state_at_time / create_map / create_place / undo / save_project），「list 返摘要 + get 返详情」双步契约（§2.5.1），硬顶 `limit ≤ 100`，filter 用 Mini-DSL（等值/子串 + AND 合取）。**剩余待裁**：(Q1) 默认端口 8732 被占时 stderr + 退出 vs +1 自动重试（**已默认 stderr + 退出**，实现期如需调整再开 ADR）；(Q2) 鉴权预留——loopback 默认无 token，`AuthMiddleware` trait 占位**不**预抽（等扩 LAN 时再开 ADR）；(Q3) MCP schema 版本锁 `2025-03-26` 是否长期接受（不追 latest，需新开 ADR 评估升级窗口）；(Q4) 批量 skill（`bulk_create_*`）何时引入（v0.2 后再说）；(Q6) `save_project` 是否触发校验（不触发，校验走 `validate_project` skill）。**(Q5) 已收敛**（v0.1 用 §2.5.1 双步契约）。关闭条件：v0.1 九 skill 全部测试在 `cargo http-test` 下绿。
31. **子图时代窗口的可见语义**（ADR-0015 落地项 · 2026-10-14 提议）：`MapNode.time_span: Option<TimeSpan>` 表达「该子图所代表的时代窗口」——窗口外时 UI 是**自动隐藏**（子图不出现在清单 / 不可钻入）还是**灰显但保留入口**（让作者看见子图存在，只是非当前时代）？当前倾向灰显（保留作者对项目结构的可视性），但子图清单面板的交互尚未验证——若灰显给作者的"我能切到任何时代看任何图"心智带来违和，则改为隐藏。关闭条件：子图清单 + 钻取 UI 的交互过作者验收。
32. **跨时代城市的多子图命名规约**（ADR-0015 落地项 · 2026-10-14 提议）：同一座城市跨越多个时代（如宋代武昌 / 清代武昌 / 现代武昌）且选择「多子图分时代」方案（ADR-0015 §2.1 第 2 写法）时，`MapNode.id` 的人工命名易撞车。建议命名规约（如 `city_wuchang_1200_1450` 后缀带时代窗口，或 `<国家>_<城市>_<era>` 三段式），但不在 ADR-0015 scope 内强制——本条仅登记待裁。关闭条件：示例工程（如 xinhai-1911）首次出现跨时代子图时给出命名规约，或决定不强制。

33. **palette.yaml Theme Pack 加载机制（2026-10-14 显式延期，2026-10-16 接续）**：规格已写（`docs/ART_STYLE_SPEC_v0.1.md` §1.4 / §2.B、`docs/STYLE_SWAPPABLE_ARCHITECTURE.md`、基线 §3.3），代码未实现——色板目前全是 Rust 常量。2026-10-14 用户裁定（[ADR-0016](docs/adr/ADR-0016-jrpg-antique-map-default-theme.md)）：古舆图主题先以 Rust 常量落地（色值尚需实机视觉迭代，常量最快、最易回退），机制**显式延期**。**2026-10-16 接续**：[ADR-0023](docs/adr/ADR-0023-theme-pack-injection-contract.md) 把契约骨架移植到 Rust 现状，分 M1（契约立柱 + 零回归）/ M2（注入 + 持久化 + Plugin 拆分）/ M3（YAML 化 + 校验 + 热切换，推迟）—— M1+M2 闭环后本条 M3 即可推进；§5 C 显式记录「Web 向 STYLE 文档不在 Rust 端照搬」。待定：schema 字段与校验规则（沿用 STYLE §2.3 V1–V9 还是精简）、热切换语义（沿用 A1 运行时 / 还是切重启）、与 `MapPalette` 字段的归并（沿用字段名零迁移 vs 改名 `theme_pack_id`）。关闭条件：M1+M2 在 main 跑 1-2 个版本未发现契约缺陷后开 M3 ADR，loader 替换常量数据源、接口不变。
34. **建筑数据来源与高度夸张系数（ADR-0016 §6 落地项）**：2.5D 建筑挤出的数据 v1 走「街道同款」作者数据路径（ADR-0010 模式：`BuildingLayer` YAML 图层文件 + `wv-edit` 命令），Overture Polygon 导入（`wv-overture/fetch.rs` 现显式跳过 Polygon）与经纬度→本图坐标换算**待裁是否引入**。待定：(a) Overture buildings 导入的落地时机；(b) 挤出高度的世界单位换算——`height_m × 夸张系数`，系数取值（含是否随缩放变化）未定，标注 `TODO(阶段X)`；(c) 手绘建筑的工具入口（多边形绘制工具复用草图式生成的意图包络？）未定。关闭条件：建筑渲染对「数据从哪来、挤出多高」有明确口径。
35. **ui-layout.yaml 的 theme/density 只写不读（2026-10-14 发现）**：`LayoutFile::load` 全仓无调用点——软件壳明暗与密度每会话从 `Default` 起步（硬编码 Dark），磁盘值从不恢复；只有「写」没有「读回」。本次（ADR-0016）新增的 `map_theme` 字段**读写都接线**，但 theme/density 的读回不在该批次范围。待定：读回时机（启动 or 打开工程后）与缺省合并规则。关闭条件：`ui-layout.yaml` 的 `theme`/`density` 在重启后恢复。
36. **标签层 v0.1 碰撞算法的精度上限**（[ADR-0017](docs/adr/ADR-0017-label-layer.md) 落地项 · 2026-10-14 提议）：v0.1 用屏幕像素 AABB 贪心（`max_labels = 200` 默认，优先级 = 文本长度）。在标签密集区（>200 实体同窗口）可能漏放最优解；Place 实体无 `PlaceKind` 字段，per-kind 权重（首都 > 城市 > 村庄）待数据模型扩展后接入。待定的是：是否在 v0.2 升级为 R-tree 视口裁剪 / 力导布局 / Voronoi 锚点避让。关闭条件：示例工程实测标签密集区观感通过验收。
37. **图标主题切换能力（2026-10-15 发现）**：本次 v0.3 视觉重做把 25 个 `tool_icons/*.svg` 都从 `stroke="currentColor"` 改成 `stroke="#ffffff"`，**根因**是 `egui::Image::tint()` 在 egui 0.36 实为 vertex 色**乘法**（`bevy_egui::render/egui.wgsl` `color_gamma = texture_color_gamma * in.color`）；`usvg` 把缺失 `color` 属性的 SVG `currentColor` fallback 为黑（维护者注释明确「black and not white」）；`egui_extras::SvgLoader` 用裸默认 `usvg::Options`，**无 color 注入钩子**——三者叠加导致任何 tint 都等于恒等映射，白图标实际就是黑像素 × 任意 tint = 黑。改固定白后暗主题立即可见，**但亮色主题下白图标在白底上消失**——目前没有主题感知的图标着色，亮色主题还是不可见（实践中暂未有人切换到亮色）。待定：(a) 是否自实现一个 `ImageLoader` 在 `usvg::Options::style_sheet` 注入 CSS `svg { color: ... }` 把 `currentColor` 解析为运行时颜色（保留主题切换）；(b) 是否允许每个 SVG 文件按主题各备一份（双套文件，loader 选其一，规模化大）；(c) 是否放弃图标主题切换改固定白 + 让主题用整体亮度统一抑制（不优雅但简单）。关闭条件：暗主题图标仍可见，且亮主题有可接受观感（不论走哪条路）。
    > **待裁边界**：本条**不改** 25 个 SVG 的固定白（那是当前可见性保证）；本条只决定**未来**主题切换路径。
38. **最近工程 `time_ago` 是否真的不持久化（2026-10-15 发现）**：本次新增的 `RecentProject.last_opened_at: Instant` 是会话内 `std::time::Instant`，**不可** Serialize——所以跨会话时间戳被吞。AGENTS §2.1 P2「目录即工程」禁写 AppData/全局配置，所以落盘若要写就得进工程根目录内的某个文件（README 待决候选「`view.yaml` 增 `recent_projects` 字段」或新建 `recent.yaml`）。本次决定**纯会话内、跨会话空状态 fallback**，未来要持久化单独立项。关闭条件：决定了持久化（落到工程根目录内/全局/不持久化三条选其一）且真的改了落盘路径。
    > **待裁边界**：本条不触动 P2 边界，但触发本 README §12 "Lastopened persistence" 待裁项（如 §28、§35 的同类）。

39. **panel 层何时脱离 `app` feature 门（2026-10-17，ADR-0026 遗留）**：控件层已随 [ADR-0026](docs/adr/ADR-0026-wv-ui-ui-widget-crate.md) 迁入 `wv-ui`、由标准 `cargo test --workspace` 门禁覆盖；但 panel / dialog 层读 `WorldProjection` / `PanelState` / 派发 EditCommand，仍挂在 `#[cfg(feature = "app")]` 之后，`cargo test --workspace` 不编译它们，`app-test` / `app-clippy` 双闸门继续承担。收敛方向：panel 的状态注入解耦（`RenderInputs` 式 trait 或事件桥），成本收益待评估。
    > **待裁边界**：本条不要求任何数据层改动；仅 UI 测试基建。关闭条件：panel 层被标准门禁覆盖（迁 wv-ui 或解耦状态注入后去 feature 门）。
39. **世界图尺度只读提示（2026-10-17 裁定方向，未实现）**：世界图声明 `unit_per_km`（ADR-0022 D4）后，校验面板显示**只读换算**：图幅 = X×Y km、按高度场估算的大陆近似尺度、常见交通方式（步行 / 骑马 / 船）横穿天数。换算复用 `Vehicle::effective_daily_speed` + `DistanceGraph::segment_days` 唯一入口，不新增实现（§4.2）。定位是「叙事换算自动化」——作者按「横穿大陆要几天」反推 `unit_per_km`，工具不给正确答案；V1 只提示不阻止。同日配套裁定（记录于 `docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §8 2026-10-17 节）：气候带**不区分**（生成配方不感知纬度，类型分带仍只按高度）、世界图生成缺省宽高比 **16:9 可改**（落点 M4 配方参数 + 新建地图流程缺省值）。关闭条件：校验面板出现上述换算行，且天数 / 公里数值经手算期望值测试锚定（AGENTS §7.2）。

40. **大陆生成配方与意图包络工作流（2026-10-17 裁定方向，M4 落地）**：多边形工具圈 1..N 意图包络 → 点「生成世界」弹属性面板（八参：种子 / 海岸蜿蜒度 / 海岸细节 / 起伏强度 / 森林覆盖 / 荒漠覆盖 / 大陆架宽度 / 离岸小岛）→ 「生成」= 距离场 + fBm + 高度分带 + Q14 高斯融合，双通道写入打包为**一条宏命令**（Q16 原子撤销 + Q13 分区），**生成即消费**意图圈；同区调整走「撤销（地形回滚 + 意图圈回驻留）→ 改参 → 再生成」，**不做叠加 / 重骰语义**。（**2026-10-18 追记（ADR-0033）**：本句的「生成即消费 / 撤销回驻留」已被取代——意图生效后选区保留、可继续编辑锚点再生成；撤销只回滚地形，不触碰选区。其余裁定不变。）新建世界图**默认海洋世界**（全格深水 + 负高度基准，替代 `TerrainGrid::new` 全零格——「height=0 深水」在 Q1 海平面契约下是歧义态）。「大陆配方」为 Q17 四条之后的第五条内置配方。civ-atlas 仅参考交互形态，内核不照搬（整球种子 / 球面 / 气候带均不做），**AGPL 源码零接触**。裁决全文与参数表见 `docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §8 2026-10-17 大陆生成节。关闭条件：M4 大陆配方 + 属性面板 + 默认海洋落地，全门禁绿 + GUI 烟测过，八参数缺省经实机校准确认。

41. **多根图（并列世界）的导航形态（2026-10-17 提出）**：新建地图对话框修复「世界层级新图挂到当前图下、zoom-out 撞外界钻回旧图」（`new_map_dialog.rs` 现在 `parent = None` + `DrillState::settle_on`）后，一工程允许存在**多张独立根图**；但导航层只有面包屑 / 钻取标记 / `drill_to`（BFS 一律从 `meta.root_map` 出发），**没有跨根图切换入口**——新世界图创建后，旧图树在本会话内不可达（重启后 `ensure_root` 落回 `meta.root_map` 才回来）。待裁：(a) 是否为多根图提供切换 UI（面包屑根段下拉 / 工程菜单地图列表 / 命令面板按名跳图），及其是否需要把「根图集合」显式化（`meta.root_map` 单值 vs `roots: Vec<Id>`——后者动 `world.yaml` schema 与 round-trip，若做需开 ADR）；(b) 「新建世界图 = 新建工程」的引导是否替代多根图支持。当前按「多根图合法、切换 UI 后补」实现（改动面最小、可回退：收掉 dialog 里 `parent = None` 分支即回到单根挂接）。关闭条件：跨根图导航有了明确形态并落地，或裁定单工程单世界并引导新建工程。

> **编号勘误（2026-10-14，合并修订）**：`feat/label-layer` 分支与 main 侧各自
> 追加了 §12 条目，合并后 30–32 双双撞号（本文件曾同时存在两组 30/31/32）。
> 现将 main 侧三条**重编号为 33/34/35**（palette.yaml / 建筑 / ui-layout），
> 分支侧 30–32 不变，标签碰撞条目补录为 36。内容均未改动。

42. **势力归属反向索引的失效策略（ADR-0027 T1）**：v1 `memberships_of` / `subordinates_of` 直接扫描全部势力（N 小）；`MembershipIndex` 反向索引的 build 时机 / 增量失效未定。关闭条件：势力/边数量超过 1 万的工程实测出现卡顿，或决定永久直接扫描。
43. **贵族领地是否从归属派生（ADR-0027 T2）**：v1 贵族自持多边形（与省份重叠合法）；「territory = 自有 ∪ 下属领地」的派生（不重复几何）未做。关闭条件：作者反馈重复绘制同一块地不可接受，或边界并集算法落地。
44. **每个势力层是否生成独立图层条目（ADR-0027 T3）**：v1 用 `Influence` 总开关 + 面板层选择器；ADR-0001 原定的「每个 FactionLayer 生成一个 Layer 条目」未做。关闭条件：面板层选择器无法满足「逐层显隐」需求。
45. **人物是否也能有 memberships（ADR-0027 T6）**：v1 只做 Faction → Faction；`Character.memberships` 扩展未做。关闭条件：作者需要在人物面板里表达「某人效忠/加盟某势力」。
46. **同一 (map, owner, from) 多条 relation 的文件名（ADR-0027 T7）**：v1 用 FNV-1a 稳定哈希后缀；若作者频繁同日起算 claim/control，文件名可读性差。关闭条件：出现实际工程依赖可读文件名，或改为给 span 加显式 id。

---

## 许可证

MIT OR Apache-2.0
