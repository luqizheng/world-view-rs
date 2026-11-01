# Design：设计文档直通世界地图（design-doc → worldmap）

> 配套需求：[2026-11-01-design-doc-to-worldmap.md](2026-11-01-design-doc-to-worldmap.md)（意图 / 裁定 / 验收在此，不重复）。
> 本文写给**执行本方案的 AI / 工程师**：所有「怎么做」的落点、代码锚点、
> schema、任务分解都在这里。锚点行号为 2026-11-01 快照，执行时以符号搜索为准。

## 1. 架构

```
设定文档 (monster/design/world/*.md)          ←— LLM 读（agent 侧）
        │
        ▼  提取（M0 靠 skill 方法论；M2 固化为 world_manifest.yaml）
地理中间表示（地图尺寸 / 地形包络 / 势力 / 节点）
        │
        ▼  tools/call（JSON-RPC 2.0 over HTTP, 127.0.0.1:8732）
wv-app HTTP 面（19 → 24 工具）  ──bridge.apply──▶  wv-edit EditCommand  ──▶  World（YAML source of truth）
        │                                              ▲
        ▼  screenshot / get_ui_state                    │ 全部走命令，无直改
agent 截图复核 → save_project
```

**分工红线**（与既有架构零冲突）：
- wv 侧改动只落在 `wv-app`（HTTP 工具薄封装）与 `wv-core`（`PoiKind`
  枚举追加两变体 + 配色）。算法本体全部已在 `wv-edit` / `wv-terrain`，
  **不写新算法**。
- LLM 调用永远在 agent 侧。wv 不加任何网络/SDK 依赖（P1；spec A-1）。
- 智能方法论（怎么读文档、怎么规划包络、何时问用户）全部沉淀在
  **skill 文本**里，不在代码里（代码里出现「未定论实现」才需要待决标注，
  本方案的「智能」不在代码里）。

## 2. 已核实的代码锚点（执行起点，勿重查）

| 事实 | 位置 |
|---|---|
| HTTP 工具三组注册（read/write/ui）+ `dispatch` 按名路由 | `crates/wv-app/src/http/tools.rs:47`（`all_tools`）、`:65`（`dispatch`） |
| 写类桥：handler 组装 `Box<dyn EditCommand>` → `bridge.apply`，回执 = 脏路径 | `crates/wv-app/src/http/bridge.rs:279`（`apply`） |
| 写工具 schema/handler 现状（5 个：create_map / create_place / undo / save_project / set_nature_names） | `crates/wv-app/src/http/skills/write.rs` |
| `create_map` 工具描述**已预留**「地形走后续 skill」——本设计即那个后续 | `write.rs:24` |
| 大陆生成命令：`GenerateContinentCommand{map, envelopes: Vec<Vec<(f32,f32)>>, params: ContinentParams, seed, cell_size}` | `crates/wv-edit/src/command/terrain.rs:358` |
| `ContinentParams` 八参数（有 `Default`：wiggle 0.40 / octaves 4 / relief Mountains / forest 0.30 / desert 0.15 / shelf 8 / islands Off） | `crates/wv-terrain/src/continent.rs:76` |
| 地形意图命令：`IntentTerrainCommand{map, intent: Intent[山脉/高原/丘陵/火山/湖/河], cell_size}`（seed 自动派生，幂等） | `crates/wv-edit/src/command/terrain.rs:159` |
| 切蛋糕：`DivideFactionCommand{map, host: Option<Id>, intent: Vec<(f32,f32)>, mode: DivisionMode[Snap/Straight], faction: Faction, from: i64, cell_size}` | `crates/wv-edit/src/command/faction.rs:828`（`DivisionMode` 在 `:785`） |
| 建势力：`AddFactionCommand{faction}`；`Faction{id,name,color,layer?,rank?,seat?,memberships}` | `crates/wv-edit/src/command/faction.rs:30`；`crates/wv-core/src/model/faction.rs:43` |
| 建点：`AddPlaceCommand`；`Place{id,name,map?,x?,y?,w?,h?,biome?,envelope?,rank?,kind?…}` | `crates/wv-edit/src/command/`（settlement 组）；`crates/wv-core/src/map/distance.rs:295` |
| 改点：`UpdatePlaceCommand`（增量更新要用）；删点命令存在但**不暴露**（spec D2/N-5） | 同上 |
| `PoiKind` 17 变体 + `ALL: [Self; 17]` 数组 + `Self::index`（渲染表按它索引） | `crates/wv-core/src/map/distance.rs:158`、`:196` 附近 |
| 工具计数哨兵测试（19 + 名单） | `crates/wv-app/src/http/tools.rs` tests（两处：len 19 / 名单 vec） |
| skill 单一真源（`include_str!` 三处同源：SKILL.md / `wv skill` / 设置按钮） | `crates/wv-app/src/http/skills/`（ADR-0085） |
| **地形三命令与切蛋糕都要求图已有栅格**，`terrain_of(map) == None` 即硬失败（拒绝臆造尺寸） | `crates/wv-edit/src/command/terrain.rs:69`、`:187`、`:394`；`faction.rs:964` |
| GUI 新建世界图时的海洋预填充配方（extent/cell_size → 全海 `TerrainData`） | `crates/wv-app/src/ui/new_map_dialog.rs:542` |
| HTTP 建图走 `AddMapCommand::new(node, None)`——**不带栅格**（builder check 问题 1 根源） | `crates/wv-app/src/http/skills/write.rs:221` |
| `UpdatePlaceCommand` 可变面只有 name/rank/x/y（w/h 明确排除）；kind 归 `SetPlaceKindCommand` | `crates/wv-edit/src/command/attribute.rs:304`、`:391` |
| HTTP 命令排水只置 `projection.dirty`，**不失效 `TerrainTexture`**（builder check 问题 3 根源） | `crates/wv-app/src/http_integration.rs:350` |
| 贴图重建的门：mode/map/theme/soften 变了才重建 | `crates/wv-render/src/cellmap_bridge/terrain_texture.rs:82` |

> 以上带「builder check」标注的六行是 2026-11-01 子代理逐锚点核实后**追写**
> 的（AGENTS.md §6.3 追写不改机制）；由它们推出的修正见 §3 前置修正、§4
> 范围修订、§8 T1.2b。

**坐标口径（两套原点陷阱的预防）**：`envelopes` / `intent` 用**图内局部
wu**（map-local），`Place.x/y` 也是图内局部坐标。skill 里必须写明：规划
多边形前先 `get_map` 拿图的尺寸与原点约定，所有顶点用同一坐标系生成，
不许混入屏幕像素或根图帧坐标。

## 3. M1 工具面扩展（19 → 26）

全部走既有模式：`skills/write.rs` 加 schema + handler，handler 组装命令
`bridge.apply`。**每个工具都是一条命令的直译**，无合成逻辑。

**前置修正（builder check 问题 1）**：地形三命令与切蛋糕都要求图已有
栅格（`terrain_of == None` 即硬失败），而 HTTP `create_map` 现在建的是
无栅格图——照原样 T1/T2/T4 在 HTTP 建的图上根本跑不起来。修法 =
**扩展现有 `create_map`**（仍是 `AddMapCommand` 直译——命令本就带
`terrain: Option<TerrainData>`，GUI 的世界图就是这么预填充的，
`new_map_dialog.rs:542`）：schema 增加可选 `cell_size`；`level == "world"`
且给了 `cell_size` → handler 按同配方构造全海 `TerrainData` 预填充。
`cell_size` 从此被栅格尺寸钉死，后续所有地形/势力调用的 `cell_size`
必须与之一致——**manifest 是这个数的唯一载体**（§6）。

| # | 新工具 | 直译命令 | 关键入参（schema 要点） |
|---|---|---|---|
| T1 | `generate_continent` | `GenerateContinentCommand` | `map_id`；`envelopes: [][]number`（每条 ≥3 点，图内局部 wu）；`params?`（八参数，缺省 = `ContinentParams::default`）；`seed: u64`；`cell_size: number` |
| T2 | `paint_terrain_intent` | `IntentTerrainCommand` | `map_id`；`intent: {kind: mountain/plateau/hills/volcano/lake/river, polygon: []number}`（`Intent` 的枚举与字段**执行时按 `wv_terrain` 实际定义对齐**，勿凭本文猜）；`cell_size` |
| T3 | `create_faction` | `AddFactionCommand` | `faction: {id, name, color: "#RRGGBB", rank?: kingdom/…, seat?: place_id}` |
| T4 | `divide_faction` | `DivideFactionCommand` | `map_id`；`host?: faction_id`；`intent: []number`；`mode: "snap"\|"straight"`；`faction: {…同 T3}`；`from: i64`（缺省 0）；`cell_size`。回执带 `snapped` 标志（命令已内置） |
| T5 | `update_place` | `UpdatePlaceCommand` | `place_id` + name/rank/x/y（命令可变面就这四样——w/h 明确排除，kind 另有命令；builder check 问题 4）。世界图 v1 全部用点图标，不用 w/h |
| T6 | `set_place_kind` | `SetPlaceKindCommand` | `place_id` + `kind`。没有它，建错的 kind 永远改不回来（无删除工具，spec D2）——M0 占位的 ~40 个节点也靠它转正 |
| R1 | `list_factions`（读） | 直读 `world.factions` | 无入参。**增量更新 diff 的前提**：`AddFactionCommand` 对重复 ID 硬失败，agent 必须先知道工程里已有哪些势力 |

约束：
- 所有工具 `additionalProperties: false`（既有惯例，skill 故障表也这么教）。
- 回执统一 JSON：至少 `{ok, dirty_paths: [...]}`，写工具回执脏路径是既有
  契约（`bridge.apply` 返回值直通）。
- **不做** `delete_*`、不做批量事务包装（一次调用 = 一条命令 = 一条
  History 事务；批量 = agent 顺序调多次，天然逐条可 undo）。
- **贴图失效接线（builder check 问题 3）**：`drain_http_commands` 现在
  只置 `projection.dirty`，地形命令落盘后 `screenshot` 仍是旧海面——
  「每阶段截图复核」会撒谎。修法 = 排水时脏路径含 `terrain.bin` →
  对应图失效 `TerrainTexture`（改 `crates/wv-app/src/http_integration.rs`
  排水处，经既有消息通道；**不走** skill 里 `set_view_mode` 往返的脆弱
  绕行）。属刷新管线缺陷修复，验收归 `cargo http-test` + `smoke-gui.sh`。
- 计数哨兵测试同步：19 → 26，名单 vec 追加七名（T1–T6 归写类组内、
  R1 归读类组尾；`all_tools` 顺序断言与名单一起改，`smoke-http.sh`
  硬编码计数同步）。

### PoiKind 扩展（G3）

- `Fortress`（要塞/堡垒/关口——本设定第一等节点）与 `Camp`（营地/临时
  聚落）追加到**枚举尾部**（`Danger` 之后），`ALL` 数组 17 → 19。
- 渲染表（配色/图标）按 `Self::index` 索引，追加即得，执行时跑
  `cargo app-test` 确认渲染表测试计数同步。
- 图标遵守 Lucide 规约（AGENTS.md §8）：`tower-control.svg` /
  `tent-tree.svg`（执行时在 Lucide 站内选，落盘 kebab-case + `stroke="#ffffff"`）。

## 4. 覆盖语义：按 ID 对齐增量更新（spec D2）

算法（skill 里写死，代码不实现——这是 agent 侧流程）：

```
对 manifest 里每个实体 e：
  工程已有同 ID → 字段有差 → T5/相应工具更新（一条事务）
  工程没有      → 建图/建势力/建点工具新增
工程有、manifest 没有 → 不动，回执里列出「未触及的既有 ID」提示用户
```

- 幂等性来源：**ID 由 manifest 唯一持有**（`city_tide_gate` 这类可读 ID，
  §4.4），LLM 每次提取同一文档必须产出同一 ID——skill 方法论里规定
  ID 生成规则：`<类别>_<中文名拼音>`，同一文档重读不漂移。
- `undo` 逐条可回退，History 事务粒度 = 单命令（既有行为，无新增）。

**范围修订（builder check 问题 2，spec D6）**：v1 的增量更新**只覆盖
Place 集**（T5 改 / T6 转 kind / 新建）——势力**边界重切**与**地形重做**
不在 v1 范围：切蛋糕对已有 ID 报「已存在」，地形意图是**加法语义**
（重跑同一山系会继续增高、漂移），且按 D2 无删除工具。设定文档改了
势力边界或山川走势 → skill 的既定话术 = **新建一张图重跑**，或 GUI
手工重切；未跟着更新的既有势力/地形一律进差集提示。彻底的重切面
（R1 已给 + 暴露 `UpsertFactionGeometryCommand` / `UpdateFactionCommand`）
列 M2 候选，不在本轮。

## 5. 设定 → wv 数据映射表（skill 的核心知识）

### 5.1 势力（7 个 Faction）

| 设定 | Faction.rank | 色（默认，用户可改） |
|---|---|---|
| 霜冠王国 | `kingdom` | 钢蓝 |
| 自由市同盟 | `kingdom`（渲染提示自由发挥，`free_city` 也行——字段是自由串） | 暖金 |
| 长明灯塔 | `kingdom` | 苍白 |
| 圣辉教国 | `kingdom` | 象牙 |
| 熔炉矿邦 | `kingdom` | 锈红 |
| 草原诸部 | `tribe` | 草绿 |
| 魔军控制区 | `kingdom`（无 seat） | 暗紫 |

势力**领土 = `divide_faction` 的切蛋糕面**，不是 Place 属性。顺序：六国
从宿主 `None` 依次切（先大后小），魔军控制区最后切西线大块。首府 `seat`
指向对应 Place ID。

### 5.2 节点 → Place 映射（kind / rank）

rank 用呈现级别词汇（3 国家级 / 2 市镇级 / 1 普通 / 0 微点）：

| 设定节点 | kind | rank | 量（monster 实例） |
|---|---|---|---|
| 各国王城/中心（灰岩城、回声城等） | `Settlement` | 3 | 7 |
| 城市（铁闸城、潮门港、粮仓城…） | `Settlement` | 2 | ~35 |
| 要塞/堡/关（铁闸堡、哑钟堡、铜钟堡、霜门关…） | `Fortress`（新） | 2 | ~20 |
| 村落（采石村…30 个） | `Settlement` | 1 | ~52 |
| 草原流动营地（一号营地…） | `Camp`（新） | 1 | 20 |
| 矿邦坑口 ×2 | `Station` | 1 | 2 |
| 灯塔塔链（主塔/三副塔/塔基市/云市） | 主塔+塔基市 `Settlement` rank2；副塔 `Education` rank1 | — | 6~8 |
| 失地 5 处 | 按上表各自类别，正常建 | — | 5 |

**原则（写进 skill）**：文档没写地理坐标的节点，LLM 按文档方位叙述
（§6.10 示意图 + 各国相对位置）排布；排布依据写进 manifest 注释字段，
便于用户校对，不追求一次到位（V2：作者随时可改）。

> **M0 占位口径**：M0 时 `Fortress`/`Camp` 尚不存在（M1 才加），要塞/
> 营地先以 `Settlement` 落点，M1 后经 T6 `set_place_kind` 转正——M0
> 验收按此口径，不算缺陷。

### 5.3 地形

| 设定 | 工具链 |
|---|---|
| 大陆整体（海岸线、大陆架、森林/荒漠倾向） | T1 `generate_continent`：一到多条包络圈出主大陆；`seed` 记进 manifest |
| 钟壁山（大陆最重山脉）+ 北境高原 | T2 `paint_terrain_intent`（mountain / plateau），多边形走图内局部 wu |
| 南方大河、东部港口链水域 | T2（river / lake） |

## 6. world_manifest.yaml（M2 固化，schema v1）

落盘位置：工程目录 `world_manifest.yaml`（与 `world.yaml` 并列，**不进**
`wv-io` 的领域模型——它是 agent 的输入快照，wv 不读它，P2/P3 不受触）。

```yaml
version: 1
source: D:/kudi-project/monster/design/world/   # 提取来源，追溯用
map: {id: world_root, name: "怪物大陆", level: world, size: [1200, 1500], cell_size: 2.0}  # km/格执行时换算
terrain:
  envelopes: [[[x,y], ...], ...]
  params: {coast_wiggle: 0.4, ...}   # ContinentParams 直译，可缺省
  seed: 20261101
  intents:
    - note: "钟壁山"
      kind: mountain
      polygon: [x, y, x, y, ...]
factions:
  - {id: faction_shuangguan, name: 霜冠王国, color: "#4a6fa5", rank: kingdom,
     seat: place_huiyancheng, division: {host: null, intent: [...], mode: snap, order: 1}}
places:
  - {id: place_tiezhacheng, name: 铁闸城, kind: Settlement, rank: 2, x: 812.0, y: 402.5, note: "东部关口集散地"}
```

- ID 规则：`place_<拼音>` / `faction_<拼音>`，重读同一文档不漂移（§4）。
- 往返一致性：manifest → 工程是单向生成；「工程 → manifest」不做（文档
  仍是 source of truth，manifest 只是快照）。
- `note` 字段：LLM 的排布依据/原文摘句，人校对用，wv 不读。

## 7. skill：`wv-worldmap`（新，agent 侧方法论）

落盘 `.agents/skills/wv-worldmap/SKILL.md`（与 `wv-http` 同级；描述风格
对齐 `wv-http` 的 frontmatter）。内容大纲：

1. **触发**：用户给设定文档 + 「做地图」类意图时加载。
2. **流程**（对应 spec §2 用户故事）：
   a. `get_ui_state` + `list_maps` → 已有世界图 → **停，问用户**（新建/
      增量更新/放弃，给三选项）；无 → 提案新建等确认；
   b. 读设定文档，产出 manifest（M2 落盘；M0 直接在对话里给出提取摘要
      征求确认——**地形包络和势力清单必须过用户眼睛**再动手）；
   c. 生成顺序固定：`create_map` → T1 地形 → T2 山河 → T3/T4 势力 →
      批量 `create_place`；每阶段 `screenshot`（管道落盘，不进上下文）；
   d. `save_project`；汇报：建了什么 / 未触及什么（§4 差集提示）。
3. **约束**：坐标口径（§2 末尾）、ID 规则（§4）、增量更新算法（§4）、
   特例三则（spec D3）、映射表（§5 精简版）。
4. **故障表**：复用 `wv-http` 的，加「文档字段缺失 → 留空并汇报，不硬造」。

`wv-http` SKILL.md 同步：工具计数 15 → 26（既有欠账 19 也一并修），
新工具一行一条补进工具表。三处同源由 `include_str!` 保证。

## 8. 里程碑与任务分解

### M0（零代码，半天）
- T0.1 起实例工程（用户启动 GUI `--features http`；或 agent 用
  `examples/` 生成器思路建空工程——以实机为准）；
- T0.2 skill 草稿先写（指导 M0 自己）；批量建点跑 monster 文档全清单；
- T0.3 截图复核 + `save_project` + 重开抽查；记录映射表 §5 的实测修正。

### M1（代码轮，按 AGENTS.md §13：分支 `feat/design-doc-worldmap` →
worktree `.worktrees/feat/design-doc-worldmap` → 动工）
- T1.1 `PoiKind` + `Fortress`/`Camp`（wv-core，含 `ALL`/渲染表/图标）；
- T1.2 七个新工具（T1–T6 写 + R1 `list_factions` 读）+ `create_map`
  预填充扩展（wv-app `skills/write.rs` + 哨兵计数 19→26 +
  `smoke-http.sh` 计数同步）；
- T1.2b 地形排水 → `TerrainTexture` 失效接线（§3 前置修正 / builder
  check 问题 3；`http_integration.rs`，`cargo http-test` + `smoke-gui`
  兜底）；
- T1.3 `wv-http` SKILL.md 修正 + `wv-worldmap` skill 定稿；
- T1.4 留痕四件：ADR-0099（处境=LLM 建图链路缺地形/势力写面；决策=
  薄封装既有命令不写新算法；理由/代价+被否决的 C 内嵌方案）、GLOSSARY
  三术语、README §12 三待裁（spec §7）、本 spec 状态行更新；
- T1.5 全门禁 + `smoke-http.sh` + 实机 monster 文档全量跑一遍。

### M2（manifest 固化）
- T2.1 manifest schema 定稿 + 校验小工具（agent 侧脚本或 skill 检查单，
  **不进 wv crate**）；
- T2.2 幂等重跑实测（spec M2 验收三条）；
- T2.3 （可选）`PlaceEdge`/速度表字段启用——动 wv-core 数据面，需另过
  一遍 §10 检查清单（P6/P7 敏感），本轮默认不做。

## 9. 测试与门禁（M1 提交前逐条）

- [ ] `cargo test --workspace`（含 wv-core 新变体、wv-app http 哨兵 24）
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`（本地内环可
  `bash scripts/clippy.sh`，全量留 CI）
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo http-test`（wv-app `--features http`）+ `bash scripts/smoke-http.sh`
      （工具数变为 26，脚本硬编码计数同步）
- [ ] `bash scripts/cargo-test.sh`（受限环境下写盘类测试的口径，§2.2）
- [ ] §2.2 五条 `cargo tree` 断言（本轮没动依赖方向，跑一遍兜底）
- [ ] GUI 烟测 `bash scripts/smoke-gui.sh`（渲染表加了变体，属渲染改动）

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| `Intent` 枚举实际字段与本文 schema 出入（本文未逐字段核实） | T2 工具实现前先读 `wv_terrain` 源码对齐，schema 以命令真实可变面为准（§3 已标） |
| LLM 排布坐标不理想（节点重叠/压山） | manifest `note` 记依据 + 用户可改 + 截图复核环节兜底；不追求自动避让（YAGNI） |
| 切蛋糕顺序影响结果（后切吃先切的面） | manifest `division.order` 显式排序，六国→魔军固定序 |
| 地形意图是加法语义，重跑漂移增高 | v1 增量不含地形（§4 范围修订）；skill 话术 = 势力/地形变更走「新建一张图」；重切面列 M2 候选 |
| 130+ 次 `create_place` 逐条慢 | 实测为准；若慢，M1 不加批量工具（History 粒度优先），M2 再议 |
| 世界图 `cell_size` 换算（1200×1500 km ↔ wu/格） | 参照 `examples/jrpg-world` 现值执行时定标，写进 manifest，不硬编码 |
| 并行会话改 main 导致锚点漂移 | 全部锚点以符号搜索复核，行号仅作参考 |

## 11. 本设计的作者拍板项（用户未逐条过目，执行前可否决）

- 五工具命名与入参形态（T1–T5，§3）；
- 势力默认色板（§5.1）；
- 灯塔塔链 kind 选择（主塔 `Settlement`、副塔 `Education`，§5.2）；
- 矿邦坑口 = `Station`（§5.2）；
- manifest 落工程目录根部、wv 不读（§6）；
- M1 分支名 `feat/design-doc-worldmap`（§8）；
- **builder check（2026-11-01 子代理逐锚点核实）后我拍板的四条**：
  ① `create_map` 扩展预填充而非新增初始化工具（D5）；② 增量更新 v1
  只管 Place 集，势力/地形重做走「新建一张图」（D6）；③ 贴图失效走
  排水接线修复而非 skill 绕行（D7）；④ T5 拆 `update_place` +
  `set_place_kind`、补读工具 `list_factions`，工具面 19→26（D8）。
