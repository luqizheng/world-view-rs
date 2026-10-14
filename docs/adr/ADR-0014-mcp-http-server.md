# ADR-0014：LLM 可控的世界编辑——HTTP/MCP 服务与 skill 表面

**状态**：已接受（Accepted，2026-10-14 用户拍板，授权开 worktree 进 v0.1 骨架实现）
**日期**：2026-10-14
**决策者**：用户
**关联**：P1（单机可运行）、P3（YAML 为主格式）、AGENTS §2.1（wv-app 持有工作副本 + 变更经 wv-edit）、AGENTS §3（数据与渲染分离）、AGENTS §3.3（System 不得改世界）、AGENTS §6.4（ADR 纪律）、AGENTS §7.5 V1/V2（校验只提示不阻止 / 历史可重写）、AGENTS §9（依赖新增纪律）、AGENTS §12（待决问题处置）、[ADR-0007](ADR-0007-overture-real-geography.md)（已有「外部接口扩展」先例，可参考其「只读引用 + 平行 crate」格局）、[ADR-0013](ADR-0013-layer-frame-binding.md)（最近一份 ADR 模板）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

作者想让 LLM（Claude Desktop / Cursor / Zed / Continue 等外部客户端）能够**读取**当前打开的世界、**生成** `EditCommand` 修改世界，并**落盘**到 YAML。理由是创作流程里有大量「批量设定 + 一致性维护」类工作（例如按时间线铺一段角色状态、补一批 Place、补一批 Event、改正一致性违例）——这种工作 LLM 比人快很多，而当前编辑器没有任何让外部进程接入的接口。

直接动 `WorldProjection.world` / ECS 资源 是不允许的（AGENTS §3.3、§2.1）。但 `wv-edit` 已经把「变更」抽象成 `EditCommand` + `History` 这条**单向入口**——LLM 能不能接进来，取决于这条入口能不能被另一类生产者（HTTP handler）安全复用，而渲染与 UI 不变。

这条入口**已经**存在。**没有**的是：

1. 一个对 LLM 友好的 skill 表面（LLM 看不到领域结构，需要什么粒度、命名如何与 MCP `tools/list` 对齐）；
2. 一个本地服务，把 MCP 协议接进 `wv-app` 同进程；
3. 一条从 handler → EditCommand → Bevy 主帧的桥；
4. 测试与文档（AGENTS §7.2 / §7.1 强制门槛）。

## 2. 决策（Decision）

### 2.1 模块与特性门控

- 新增 `crates/wv-app/src/http/` 目录，挂在 `#[cfg(feature = "http")]` 下。
- `Cargo.toml` 加 `http = ["axum", "tokio", ...]` 特性，与 `app` 平行；**默认 features 不开 `http`**——保证 `cargo test --workspace` 与 `cargo clippy --workspace` 不编译 HTTP 模块，呼应 ADR-0013 §2⑦ / AGENTS §7.1 的渲染层单独命令规则。
- HTTP 模块结构：
  - `server.rs`：axum 路由与启动钩子（在 Bevy 的 `Startup` schedule 里 `AsyncComputeTaskPool::spawn` 启动监听任务）；
  - `bridge.rs`：handler ↔ Bevy 的命令桥（`tokio::sync::mpsc::Sender<EditCommand>` 注入，主帧一个系统排干并 apply）；
  - `skills/`：每个 skill 一个 schema 文件（`#[derive(serde::Deserialize)]` 输入 + `.json` 文档示例）；
  - `tools.rs`：`tools/list` 聚合点。

### 2.2 协议与传输

- 协议：**Model Context Protocol** `2025-03-26` spec（**锁版本**，不追 latest；spec 演进需新开 ADR 评估）。
- 传输：**HTTP/SSE**（不是 stdio）。Claude Desktop 等客户端在配置里写 `url: http://127.0.0.1:8732/mcp`。
- 绑定：**仅 `127.0.0.1`**，LAN 不开放（最小安全面）。
- 端口：默认 `8732`（"w-v" 谐音，可记）；绑定失败立即 stderr 报错并退出 server 任务（不阻塞 GUI 主循环），用户可通过启动参数 `--http-port <N>` 覆盖。

### 2.3 变更路径（写类 skill 的硬约束）

写类 skill **必须**按以下三步完成：

```text
HTTP request → handler 构造 EditCommand → mpsc::Sender 投递
                                                  ↓
                              Bevy 主帧 `drain_commands` 系统（每帧 1 次）
                                                  ↓
                              History::apply(&mut WorldProjection.world)
                                                  ↓
                              （下一帧）projection 重算 → 渲染拉模式更新
```

- **禁止** handler 直接 `&mut WorldProjection.world`；
- **禁止** handler 通过 `world.resource_mut::<...>()` 改 ECS（AGENTS §3.3）；
- handler 只发命令 + 等待**回执**（`History::apply` 返回的 `ApplyOutcome`）。

回执按 MCP 的 `ToolResult { content, is_error }` 序列化；错误用 `wv-core::CoreError` 而非字符串（thiserror 一致）。

### 2.4 读类 skill（不写世界）

读类 skill **不**经 `EditCommand` 通道，**直接**通过 `wv-core` 的查询函数拿数据：

- `TimelineQuery::at(value, time)` 求「某时刻状态」（P7 唯一入口）；
- `wv-core::map::*` / `wv-core::character::*` / `wv-core::event::*` 等只读迭代器。

返回类型是**领域对象**（`MapSummary` / `Place` / `World`），**不是** ECS Component（AGENTS §3.1）。

### 2.5 Skill 粒度（最小骨架 v0.1）

**判断标准**：每个 skill 应**对应一次用户的可表达意图**（不是每条底层数据列）。v0.1 上 9 个 skill：

| Skill | 类型 | 备注 |
|---|---|---|
| `list_maps` | 读 | 返回 `Vec<MapSummary>`（轻量，见 §2.5.1） |
| `get_map` | 读 | 单图完整 MapNode |
| `list_places` | 读 | 按 map_id 过滤，返 `Vec<PlaceSummary>`（轻量，见 §2.5.1） |
| `get_place` | 读 | 单个 Place 完整 payload |
| `get_state_at_time` | 读 | P7 入口，`(map_id, time_iso)` → 该时刻所有要素投影 |
| `create_map` | 写 | 生成 `CreateMapCommand` |
| `create_place` | 写 | 生成 `AddPlaceCommand` |
| `undo` | 写 | 生成 `UndoCommand`（已存在） |
| `save_project` | 写 | 触发 `wv-io::save_project` |

后续 v0.2+ 再扩 `update_*` / `delete_*` / `apply_terrain_brush` / `validate_project` 等，**新增 skill 必须在本 ADR 的「待裁」节追加后实施**。

#### 2.5.1 list/get 双步契约与响应上限

**核心约束**：`list_*` skill 返回**轻量摘要**（id + 显示字段 + 关键坐标），**不**返回完整领域对象；`get_*` skill 返回**完整 payload**，**每次一条**。理由：LLM 上下文是硬约束——一次 list 返回 100 条 × 5 KB = 500 KB 会污染整个对话；list/get 分步让 LLM 主动控制深度，与 MCP 生态惯例（Filesystem MCP / Postgres MCP）一致。

**每个 `list_*` 的契约**（v0.1 含 `list_maps` / `list_places`；后续 `list_events` / `list_characters` 等遵循同一形态）：

| Skill | 返回字段 | 单条大小 | 默认 limit / 上限 |
|---|---|---|---|
| `list_maps` | `[{id, name, parent?, size_w, size_h}]`（**无** `layers` / `terrain` / `icons`） | ~80 B | 100 |
| `list_places` | `[{id, kind, name, x, y}]`（**无** `state_spans` / `connections` / `notes`） | ~150 B | 100 |

**上限**：`limit` 显式传值时硬顶 100（`limit > 100` → MCP `INVALID_PARAMS`）；结果集 > 100 条时返 `cursor: "<opaque>"`，LLM 续调传 `cursor` 拿下一页（v0.1 cursor 实现 = 进程内偏移，进程重启重拉即可，**不**持久化）。

**filter 参数形态**（v0.1 范围）：**Mini-DSL 字符串**，作用域 = 等值 / 子串匹配 + AND 合取。例：`name~'京' AND kind='city'`；v0.1 **不**支持 OR / NOT / 正则 / 数值范围。解析失败 → MCP `INVALID_PARAMS` + 错误消息列出当前支持的语法。若实现期发现 LLM 表达超出该 DSL 频繁失败，回退到结构化字段（`{"name_contains": "京", "kind": "city"}`）——改动仅限 `tools/list` schema 与 handler 入参类型，list/get 契约不动。

**每个 `get_*` 的契约**：单条调用，**限 1 条 / 调用**；返回完整领域对象（含 `state_spans` / `connections` / `notes` / `layers` 等全部字段）。

`get_state_at_time` **例外**——它本来就该返回该时刻的整图投影，**不分页**；v0.1 不设响应大小硬顶（实测 `examples/xinhai-1911/` 全图投影约 200 KB），若 LLM 报告「上下文爆炸」再加 `place_ids` 过滤参数与大小硬顶，留作 v0.2 待裁。

**LLM 工作流示例**：

```text
list_places(map_id="world", filter="name~'京'", limit=50)
  → 50 条摘要，~7.5 KB

LLM 看到感兴趣的 id（如 "city_qingyun"）
  ↓
get_place(id="city_qingyun")
  → 完整 Place（含 state_spans / connections），~2 KB
```

### 2.6 测试与文档

- AGENTS §7.2 强制测试：9 个 skill 各 1 个 happy-path round-trip；1 个并发写（验证 mpsc 串行化）；1 个 MCP schema 校验测试；1 个时间求值边界用例（沿用既有 `TimelineQuery` 测试集）。
- 渲染层单独命令（AGENTS §7.1）：`cargo http-test` / `cargo http-clippy` 别名加进 `.cargo/config.toml`（与 `cargo app-test` / `cargo app-clippy` 平行）。
- ADR-0014 在 `docs/adr/README.md` §4 表格追加。
- 新增待决问题追加到 `README.md` §12（端口冲突策略、鉴权预留、schema 版本锁定等）。

### 2.7 与七原则的自检（PR 描述模板）

每个触及本 ADR 的 PR **必须**显式声明触及哪条原则：

| 原则 | 触及情况 |
|---|---|
| P1 单机可运行 | ✅ HTTP 仅 loopback、LLM 客户端也是本地进程；**不**引入云端同步 |
| P2 目录即工程 | ✅ HTTP 不引入新存储路径；YAML 仍为 source of truth |
| P3 YAML 为主格式 | ✅ HTTP 返回/接收**不**引入二进制；超大栅格仍是唯一例外 |
| P4 多地图层级 | ✅ 写类 skill 通过 `EditCommand` 走既有层级引用 |
| P5 图层化编辑 | 不触及（skill 表面可日后扩 `set_layer_*`，不属 v0.1） |
| P6 非破坏编辑 | ✅ 变更经 `EditCommand` → `History`，区间不覆盖 |
| P7 时间轴驱动 | ✅ 时间相关读全部走 `TimelineQuery::at` |

## 3. 理由（Rationale）

- **为什么 MCP 而不是 OpenAI Tool Use**：MCP 已被 Claude Desktop / Cursor / Zed / Continue 集成，「App 即插即用」零额外配置；OpenAI Tool Use 通用但需自己管理会话、token 预算、上下文裁剪——是在发明轮子。
- **为什么 HTTP/SSE 而不是 stdio**：stdio 形态需要 `wv-app` 双形态（GUI / headless），与 §2.1「wv-app 持有工作副本 + 渲染」定位冲突；HTTP/SSE 让 GUI 单形态跑完整套。
- **为什么仅 loopback**：P1 单机可运行是硬约束；loopback 默认安全，LAN 鉴权不是本 ADR scope。
- **为什么单线程命令队列而不是乐观并发**：wv-edit 现状零锁、单线程 apply；扩展只要加一个 `mpsc` 通道就够，不引入新并发原语、不破坏现有 `History` 的不变量。
- **为什么最小骨架只上 8 个 skill**：先验证 LLM 真能从工具拿到世界信息（**读**侧），再验证 LLM 真能通过工具改世界且不破坏一致性（**写**侧）。一次铺满 ~30 个 skill 的代价是「全 skill 全测」+「调试 LLM 行为偏差」叠加，定位问题极慢。

## 4. 代价（Consequences）

- **正**：作者可让 LLM 批量维护一致性、补历史区间、生成骨架；架构纪律不破（所有写经 `EditCommand`）；HTTP 模块默认不编入主测试门槛。
- **负①（开发流程）**：HTTP 改动必须用 `cargo http-test` / `cargo http-clippy` 单独验证；门槛命令需在 `.cargo/config.toml` 注册。
- **负②（端口冲突）**：默认 8732 被占时 server 任务退出并在 stderr 打印，**不**自动 +1（避免静默偏移掩盖冲突）。
- **负③（协议演进）**：MCP spec 仍在演进；锁 `2025-03-26` 意味着新客户端可能要走新 spec，需要 `wv-app` 升级——届时新开 ADR 评估。
- **负④（skill 表面积增长）**：每加一个 skill 都要加测试；测试覆盖度跟不上的话，LLM 调用出错时排障极慢（handler 错、参数错、回执错混在一起）。
- **负⑤（回执信息密度）**：MCP `ToolResult.content` 是单层结构，复杂错误（多字段验证问题）需要设计序列化形式（建议沿用 `CoreError` 的 `serde` 形态，不引入新错误协议）。

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **OpenAI Tool Use 自实现** | 通用，但需自写 chat 历史、token 预算、上下文裁剪；MCP 已被主流客户端集成，重复造轮子 |
| **stdio + 独立 `wv-mcp` CLI** | 需双形态 `wv-app`（GUI / stdio 启动）；多一份发布资产；GUI 启动时已加载世界，stdio 子进程得另起一份或共享状态——状态共享比 mpsc 通道复杂 |
| **纯 CLI（无服务）** | LLM 调用 `wv` CLI 子命令；LLM 难以维持「当前打开的世界」会话状态；每一步都要重新 `load`/`save`，效率低且上下文爆炸 |
| **Embedded LLM（llama.cpp / Ollama 嵌入）** | 体积大、依赖重、违反 AGENTS §9「默认拒绝」；作者已在用外部客户端，没必要 |
| **GUI 内置聊天面板** | 需要选/接 LLM、UI 工程、token 管理；与「让 LLM 用自己的客户端」目标冲突；且引入 AGENTS §9 风险面更大 |

## 6. 待裁（Open Questions）

落地 v0.1 时必须先回答：

- **Q1**：默认端口 8732 被占时的兜底——**当前按 stderr + 退出**；是否改为「+1 重试直到成功」？倾向 stderr + 退出（避免静默偏移），但需用户拍板。
- **Q2**：鉴权预留——loopback 默认无 token；将来若扩 LAN，需在 handler 中预留 token middleware 接口位。**当前不实现**，只留 `pub trait AuthMiddleware` 占位与一行 `// TODO(ADR-0014 Q2)`。
- **Q3**：MCP schema 版本锁——锁 `2025-03-26` 还是 follow latest？**当前按 2025-03-26**；Claude Desktop 当前默认支持此版本。
- **Q4**：批量 skill 何时引入——`bulk_create_places` / `bulk_create_events` 是否在 v0.2 必备？倾向**延后**，先让单条 skill 路径走通。
- **Q5**：LLM 上下文裁剪——`list_*` 大结果集是否要分页 / 过滤参数？**【2026-10-14 收敛】**：v0.1 上「list 返摘要 + get 返详情」双步契约（§2.5.1），硬顶 `limit ≤ 100`，filter 用 Mini-DSL（等值/子串 + AND 合取）。本条关闭。
- **Q6**：`save_project` 是否触发校验（AGENTS §7.5 V1）？**不触发**——校验是另一条 skill `validate_project`，LLM 自主决定何时调。

后续若发现 v0.1 不足，按 ADR-0013 / 0012 模式以新 ADR 增补（如 v0.2 把 `update_*` / `apply_terrain_brush` 补齐时新开 ADR-0015）。