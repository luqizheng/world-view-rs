# ADR-0099: 设计文档直通世界地图——HTTP 工具面暴露既有地形/势力命令

- 状态：已接受（2026-11-01，随 `feat/design-doc-worldmap` 落地）
- 关联：ADR-0014（MCP/HTTP 工具面）、ADR-0085（skill+HTTP 标配入口）、ADR-0047（呈现级别）、ADR-0064（POI 分类色）、ADR-0093（图钉含义图形）、AGENTS §7.5 V1（校验只提示不阻止）、§2（分层纪律）
- 配套：`docs/superpowers/specs/2026-11-01-design-doc-to-worldmap.md`（需求）与 `…-design.md`（技术设计）

## 1. 处境（Context）

作者在 wv 之外维护游戏设定文档（首个实例：`monster/design/world/`，六国档案 + 地理与节点，约 130 个命名节点、7 个势力）。把这份文档变成地图全靠人工逐项搬：建图 → 画大陆 → 画山河 → 切国界 → 一个个建点。

而 wv 的**算法侧早就齐了**：

| 能力 | 既有命令 | 界面上有 |
|---|---|---|
| 大陆生成（海岸线/起伏/森林荒漠/大陆架/小岛） | `GenerateContinentCommand` | 有（生成大陆面板） |
| 山脉/高原/丘陵/火山/湖/河（点状与区域） | `IntentTerrainCommand` | 有（选择工具 + 笔刷） |
| 切势力领土（吸附水陆/河/山脊） | `DivideFactionCommand` | 有（划分势力） |
| 建点 / 改点 / 改类别 | `AddPlaceCommand` / `UpdatePlaceCommand` / `SetPlaceKindCommand` | 有（Inspector） |

**缺的只是「AI 够得着」**：HTTP 工具面 19 个工具里只有 `create_map` / `create_place`，`create_map` 的 description 里还写着「地形走后续 skill」——那个后续一直没来。

同时冒出三个具体堵点（施工前的子代理逐锚点核实，均属实）：

1. **地形三命令与切蛋糕对无栅格的图 hard-fail**，而 HTTP 建的图没有栅格 → 即使把工具暴露出来也跑不起来。
2. **HTTP 落盘地形后截图不变**：`drain_http_commands` 只置 `projection.dirty`，从不失效 `TerrainTexture`（GUI 路径靠各编辑系统显式 `mark_dirty`，HTTP 不经过它们）。
3. **kind 改不回来**：`UpdatePlaceCommand` 的可变面只有 name/rank/x/y，kind 归另一条命令；而本方案没有删除工具，「先建错的类」就永久错了。

## 2. 决策（Decision）

**wv 保持单机、不内嵌任何 LLM。** 智能在外（agent + skill），执行在内（HTTP 工具面 = 既有编辑命令的薄封装）。具体七条：

1. **扩展现有 `create_map`**，不新增初始化工具：可选 `cell_size`；`level == "world"` 且给了 `cell_size` → 按 GUI 同配方预填充全海 `TerrainData`（`new_map_dialog.rs:542` 口径），同时置 `node.base = Some(Ocean)`。不给 `cell_size` = 保持旧行为（不带栅格），不破既有调用方。
2. **新增六个写工具**，每条直译一条既有命令，无合成逻辑：
   `generate_continent`（GenerateContinentCommand）、`paint_terrain_intent`（IntentTerrainCommand）、`create_faction`（AddFactionCommand）、`divide_faction`（DivideFactionCommand）、`update_place`（UpdatePlaceCommand）、`set_place_kind`（SetPlaceKindCommand）。
3. **新增一个读工具 `list_factions`**：切蛋糕对重复 ID 硬失败，而没有势力读面就无从做增量 diff。
4. **`PoiKind` 追加 `Fortress` / `Camp`**（17 → 19），带主题色与 Lucide 含义图形。理由：这套设定里「要塞」与「城市」是一等并列的两种节点（前者给名、后者给钱），混成一类会让图例失去区分度。
5. **地形排水失效贴图**：`drain_messages_with_dirty` 回传脏路径，`drain_http_commands` 见 `*/terrain.bin` 即 `force_rebuild(&mut tex)`——与 GUI「强制重画地形」同一条通路。
6. **覆盖语义 = 按 ID 对齐增量更新，不提供 delete 工具**。v1 增量只覆盖 Place 集；势力边界与地形不在范围（见 §4 代价）。
7. **工具面 19 → 26**，`tools/list` 顺序断言、`smoke-http.sh`、`wv-http` skill 单一真源三处同步。

## 3. 理由（Rationale）

- **为什么不是 wv 内嵌 LLM**（被否决的 C 方案）：那会把网络客户端拖进单机编辑器（违反 P1），且与 ADR-0084 撤回 stdio 代理轮的方向直接相反。智能放外面，agent 换模型/换策略都不用改 wv。
- **为什么复刻 GUI 的预填充配方而非新造**：新建世界图铺全海是 2026-10-16 用户裁定（`MapBase::Ocean` 的 -3500 m 是为了 soften 后仍看得出「这是海」）。照抄同一配方的价值是**GUI 建的图和 AI 建的图长得一样**，作者不必学两套。
- **为什么 `cell_size` 建图时定死**：`TerrainData` 的宽高由幅面/格尺寸决定，之后所有 `cell_size` 都只是「wu → 格坐标」的换算因子——与建图时不一致就会静默算错。与其在每条命令里校验并猜，不如把「这个数被栅格钉死」写进工具描述和 skill，让 agent 只记一次。
- **为什么 `update_place` 的 `name` 可省略**：`UpdatePlaceCommand` 要求整名（无 partial 语义）。让工具在省略时先 `GetPlace` 读当前值，调用方就能「只挪个点」而不必回传名字——这是给 AI 调用方的可用性优化，不是语义放宽。
- **为什么 v1 不做删除**：V1（校验只提示不阻止）与「作者随时可以改」的精神下，删除是**最危险**的一类工具；而增量更新只要「新增/更新」就能覆盖绝大多数改稿场景。宁可让 agent 说「要改国界请新建一张图」，也不给一个能悄悄抹掉作者数据的入口。

## 4. 代价（Consequences）

1. **势力边界改不了**：设定改了国界 → 只能新建一张图重跑，或在 GUI 里手工重切。切蛋糕对已有 ID 报「已存在」，且没有 delete。**已记入 README §12 待裁**（彻底重切面：暴露 `UpsertFactionGeometryCommand` / `UpdateFactionCommand`，M2 候选）。
2. **地形是加法语义**：同一座山跑两次会继续增高。skill 里明确「不要重复下发」，但工具层没有幂等保护——靠 agent 纪律而非代码。
3. **工具面从 19 涨到 26**，`tools/list` 的 prompt 体积变大（LLM 客户端每次多读 7 个 schema）。可接受：schema 顺序稳定，缓存友好。
4. **`PoiKind` 变长会让渲染侧平面数组同步变长**（`[LandmarkPool; COUNT]`、`poi_kind_color` 的穷尽 match）。这是既有设计的意图（编译期逼出每一处落点），代价是「加一类 = 必须同时给图形与配色」。
5. **`create_map` 的契约变了**（多一个可选字段）。旧调用方不受影响（不给就不预填），但「建图不带栅格」从此是一个**需要主动说明**的少数派形态。
6. **截图滞后**已修，但只覆盖 `terrain.bin` 一类脏路径；将来若有别的「渲染资源不跟数据」的情况，需要同样在此处补判据。

## 5. 被否决的备选

| 方案 | 否决理由 |
|---|---|
| **C：wv 内嵌 `import-design` 命令直接调 LLM API** | 网络依赖进单机编辑器，违反 P1；与 ADR-0084 撤回方向相反；模型/密钥/重试全要 wv 背 |
| **新增第 7 个工具 `init_terrain` 专门铺栅格** | `AddMapCommand` 本就带 `terrain: Option<TerrainData>`，多一个工具等于把一条命令拆成两个入口，且「建图」与「铺底」分开做必然有人漏做 |
| **让 `generate_continent` 自己造栅格**（无栅格时按幅面新建） | 破坏「每个工具 = 一条命令直译」的纪律；栅格尺寸是建图决策，不是地形命令的职权 |
| **skill 里用 `set_view_mode` 往返刷新贴图** | 脆弱且易忘；这是刷新管线的缺陷，应在管线里修（本 ADR §2.5） |
| **给 `update_place` 兼任 kind，不单独开 `set_place_kind`** | 一次调用 = 一条命令 = 一条 History 事务，是既有契约；两命令合一等于让工具层开事务嵌套的先例 |
| **提供 `delete_place` / `delete_map` 让「覆盖」= 删了重建** | 见 §3 末条：最危险的入口，且会毁掉作者的增量改动 |
