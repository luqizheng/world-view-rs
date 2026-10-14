# ZCode 模型路由规约 v0.1

> 范围：本规约约束**规划/设计类产出**与**代码实现类产出**在两种模型之间的路由边界，以及不应当拆分派发的场景。
> 适用对象：所有由 zcode 宿主派出的 workflow 子代理、以及会话内直做的工作。
> 性质：**工作约束**，与 `AGENTS.md` §13「scope 纪律」并列；变更需新开一份规约并标注版本号。

---

## 1. 背景与目标

### 1.1 现状

本仓库（worldview-rs）在 zcode 宿主中由两类模型协作：

| 模型 | 角色 |
|---|---|
| **MiniMax-M3**（`minimax/MiniMax-M3`，会话模型） | 代码实现、修红循环、命令执行 |
| **GLM-5.3$max**（`account:bigmodel-individual-coding-plan/GLM-5.3$max`） | 规划、设计、评审、规约撰写 |

两类任务的**思维工作密度**差异极大：纯编辑类不需要长链推理，而 ADR 论证、规则对比、风险识别吃最高推理档。把后者留在 MiniMax 内联做，既浪费推理预算、也违背用户的显式指定。

### 1.2 目标

> **规划/设计类产出（架构、计划、评审、规约）经 CreateWorkflow 的 `subagent_model` 显式派给 GLM-5.3（`$max` 推理档）子代理；代码实现类产出留在 MiniMax-M3 上（会话内直做或显式派发）。一切模型切换只发生在 workflow 子任务边界上。**

### 1.3 不在范围

- 模型本身的选型与档位定价（属宿主策略，不在本规约讨论）。
- 单个 `subagent_model` 字段的语法细节（见 zcode `CreateWorkflow` 工具说明）。
- 与本规约无关的其它 subagent 参数（如 `sandbox`、`background`）。

---

## 2. 模型清单与定位

| 字段 | 值 | 备注 |
|---|---|---|
| 模型 ID（GLM） | `account:bigmodel-individual-coding-plan/GLM-5.3$max` | 档位后缀 `$<level>` 必须取自 `ListModels` 该行 `reasoningLevels` 实测值，不得臆造 |
| 模型 ID（MiniMax） | `minimax/MiniMax-M3` | 与会话模型同名；会话内直做时省略 `subagent_model` 字段 |
| 会话模型可被谁改 | **仅用户** | `ListModels` 工具说明原文：「session model 只有用户能改」 |
| 切模型的唯一杠杆 | `CreateWorkflow` / `AmendWorkflow` 的 `subagent_model` | 子任务粒度生效 |
| Skill 与模型的关系 | 加载技能 ≠ 换模型 | `Skill` 工具只注入指令；模型配对发生在**派发时** |
| 不可用信号 | `ListModels` 行标 `disabled` 或目录整体 `model_catalog_unavailable` | 见 §3.4 |

### 2.1 派发粒度建议

| 任务类别 | 推荐派发方式 |
|---|---|
| MiniMax 类、会话内连续上下文 | **省略 `subagent_model`**，会话直做 |
| MiniMax 类、需要并行扇出或隔离 | `subagent_model: "minimax/MiniMax-M3"` 显式派发 |
| GLM 类、必须推理档 | `subagent_model: "account:bigmodel-individual-coding-plan/GLM-5.3$max"` 显式派发，**不得省略** |

---

## 3. 决策规则（核心）

下列规则按**优先级**排列；冲突时以上位为准。

### 3.1 产出物分类优先

按产出物**分类**，不按话题分类：

- 产出是「**决策 / 规格 / 计划 / 评审清单**」→ **路由 GLM**。
- 产出是「**代码 diff / 命令执行 / 修红迭代**」→ **路由 MiniMax**。

> 这条是 §3 的总闸；其余规则是它的实现细节。

### 3.2 切模型仅在 workflow 子任务边界

- 任何 agent 工具（`Agent`、`Skill` 等）**不能**修改会话模型。
- 切换模型的唯一途径：`CreateWorkflow` 或 `AmendWorkflow` 的 `subagent_model` 指向目标子代理。
- 同一 workflow run 中，子代理的**实际模型**必须在结果中标注；**实际模型 ≠ 路由意图**（例如回落到会话模型）即视为**路由失败**，必须显式上报，不算完成。

### 3.3 subagent_model 字段书写规范

- **GLM 派发必须显式写** `subagent_model: "account:bigmodel-individual-coding-plan/GLM-5.3$max"`。
- 档位后缀 `$<level>` 只能取 `ListModels` 该行 `reasoningLevels` 中的值；不得臆造档位名。
- **MiniMax 任务**：会话模型已是 `minimax/MiniMax-M3` 时默认**内联直做**；仅在需要并行 / 隔离时以 `subagent_model: "minimax/MiniMax-M3"` 显式派发。

### 3.4 不可用与降级

- 模型 ID **首次使用前必须用 `ListModels` 核对**。
- `disabled` 行**不得绕开**，应交由用户处理。
- 本会话实测 `ListModels` 报 `model_catalog_unavailable`（host 未提供目录）。该信号下的指引为「**省略 `subagent_model` 则子代理回落会话模型**」：
  - 对 **MiniMax 任务**：回落可接受。
  - 对 **GLM 任务**：等于**路由失败**，必须显式报告，**禁止静默降级**到 MiniMax 做设计。

### 3.5 混合需求的拆分边界

混合需求**先拆后派**：

1. 由 GLM 产出**任务分解**（如本规约即此类产出）；
2. 由 MiniMax 子任务**并行落码**；
3. **高风险改动**再由 GLM 复审。

**拆分边界是「决策点」**，不是文件数 / 代码行数。

### 3.6 门槛谓词 / 路由分支的试跑

- 写进 workflow 脚本的门槛谓词 / 路由分支，**先用 `EvalWorkflowSnippet` 在真实仓库状态上试跑再定稿**。
- 该工具用途在工具说明中明示：「exercise a gate predicate on real repository state before putting it in a CreateWorkflow script」。
- 调用前置条件：**先加载 `dynamic-workflows` skill**。

> 备注：本会话未运行此验证（该 skill 不在可用列表），此规则依据为工具说明而非实测。

### 3.7 运行中 workflow 的修正

- 运行中的 workflow 脚本出错：用 **`AmendWorkflow` 传 run ID + 修正脚本**，继承已完成步骤为热缓存（warm cache）。
- 修正脚本**保持原 `subagent_model` 路由不变**，不整跑重写。
- `AmendWorkflow` 工具说明原文：「imports the old run's finished work as a warm cache」。

### 3.8 Skill 配对指引

Skill 是**模型无关**的：加载技能不改变模型（`Skill` 工具只注入指令），模型配对发生在**派发时**。

| 任务类别 | 推荐 Skill 配对 |
|---|---|
| GLM 规划 / 设计 / 评审 | `writing-plans`、`domain-modeling` 类 |
| MiniMax 编码 / 修红 / 执行 | `executing-plans`、`test-driven-development` 类 |

### 3.9 模型降级显式上报

- GLM 不可用（行被 `disabled`、或如本会话目录整体缺失）时**不得静默降级**到 MiniMax 做设计。
- 路由降级必须**显式上报**到调用方，并在结果中标注「**实际模型 ≠ 路由意图**」。

---

## 4. 典型场景示例

### 4.1 场景 A：新增子模块并入主线

**需求**：在 `wv-terrain` 中新增一个工具模块（按既有 ADR-0012 的等值线管线派生），并接进 `wv-app` 的编辑面板。

**派发序列**：

1. **GLM 子代理（`subagent_model: GLM-5.3$max`）**
   - 任务：模块契约草拟（公共 API、错误类型、与既有 `TerrainEdit` 的边界）。
   - 产出：`docs/<name>-CONTRACT_v0.1.md`、ADR 草稿。
   - 配套 Skill：`writing-plans`、`domain-modeling`。

2. **MiniMax 子代理（`subagent_model: MiniMax-M3`，并行扇出）**
   - 任务 A：`wv-terrain` 内实现模块 + 单元测试。
   - 任务 B：`wv-app` 编辑面板接线。
   - 配套 Skill：`test-driven-development`、`executing-plans`。

3. **GLM 子代理（再次，复审）**
   - 任务：合并前评审——分层纪律（P1–P7 符合性）、区间化存储、依赖方向。
   - 产出：评审清单 + 风险点。

**模型切换次数**：2 次（GLM → MiniMax → GLM），均发生在 workflow 子任务边界上。

### 4.2 场景 B：批量依赖对齐与文档同步

**需求**：把根 `Cargo.toml` 的若干依赖对齐到 `[workspace.dependencies]`，并同步 `AGENTS.md` §8 / §9 的描述。

**派发序列**：

- **MiniMax，会话内直做**（省略 `subagent_model`）
  - 任务：批量修改 `Cargo.toml` + 文档段落同步。
  - 理由：纯编辑、零决策点、按规约 §6「不应拆分」处理。

> 本场景中**不**调用 GLM——把它派给推理档是 §5 第 3 条反模式。

---

## 5. 反模式

| # | 反模式 | 后果 |
|---|---|---|
| 1 | 试图「切换会话模型」——不存在该操作：`ListModels` 工具说明明文「session model 只有用户能改」，唯一杠杆是子任务的 `subagent_model` | 操作无效；浪费派发预算 |
| 2 | 用 `Agent` 工具派任务却期待它换模型——`Agent` 无显式模型选择字段；模型控制只存在于 `CreateWorkflow`/`AmendWorkflow` 的 `subagent_model` | 任务跑在错误模型上，结果不可信 |
| 3 | 把机械编码派给 GLM-5.3$max——烧最高推理档做纯编辑，同时违背用户「写代码用 MiniMax」的显式指定 | 预算错配 |
| 4 | 把架构决策留在 MiniMax 内联做以省派发开销——同样违背用户指定，且分层纪律对照与 ADR 论证正是 GLM 推理档的用武之地 | 决策质量下降；与用户意图冲突 |
| 5 | 臆造模型 ID 或推理档位——档位名必须来自 `ListModels` 行的 `reasoningLevels` 实测数据；本会话因目录缺失未能核实 `$max` 的确切拼写，更说明不能拍脑袋写 | 派发失败；不可逆时间成本 |
| 6 | GLM 不可用（行被 `disabled`、或如本会话目录整体缺失）时静默降级到 MiniMax 做设计而不告知 | 违反 §3.9，路由失败必须显式上报 |
| 7 | 门槛谓词 / 路由分支未过 `EvalWorkflowSnippet` 就写死进 `CreateWorkflow` 脚本——该工具存在的意义就是防这类静默错误 | 错误判据静默放行坏改动 |
| 8 | workflow 脚本出错后从零重写而非 `AmendWorkflow` 续修——丢掉已完成步骤的热缓存（`AmendWorkflow` 工具说明：「imports the old run's finished work as a warm cache」） | 已完成步骤重做，浪费算力与时间 |
| 9 | 以「文件数 / 代码行数」为拆分边界——边界应是决策点（哪里产生设计判断），不是代码量 | 拆得过细；派发开销超过收益 |
| 10 | 为守规约把五分钟小任务包成多模型 workflow——派发与确认开销超过收益，见 §6「不应拆分」 | 派发成本 > 任务成本 |

---

## 6. 不应拆分的场景

下列场景**不**应走 workflow 派发；留在会话内直做或省略。

| # | 场景 | 理由 |
|---|---|---|
| 1 | 设计已定的小型单点编辑（改名、typo、单文件小改） | 走 MiniMax，零派发 |
| 2 | 答疑 / 解释类只读问题 | 既不产出代码也不产出新决策，无需派发 |
| 3 | 快速检查命令（`git status`、跑单个测试） | 只是工具调用，不构成「任务」 |
| 4 | 需要与用户来回交互的工作 | workflow 子代理无法中途询问用户（`escalate` 每问仅 3 次预算且阻塞），对话必须留在会话层 |
| 5 | 不值得走确认流程的微任务 | `CreateWorkflow` 起跑需用户确认；确认成本高于任务本身时**不建 workflow** |
| 6 | 一个 run 已接近完成时中途换模型 | `AmendWorkflow` 的价值是修脚本续跑而非换脑；换模型应等到下一个自然边界 |
| 7 | 极小规模的设计-实现强耦合 spike | 来回派发延迟超过收益——先内联做，结论落盘后正式产物再按规约路由 |

---

## 7. 版本与变更

### 7.1 当前版本

- **版本号**：v0.1
- **状态**：草案
- **关联任务**：由 zcode 宿主中 MiniMax 主会话经 `CreateWorkflow` 派给 GLM-5.3$max 子代理产出；本子代理即在该会话中执行。

### 7.2 来源依据

| 规则 | 来源 |
|---|---|
| 会话模型不可改 | `ListModels` 工具说明 |
| 子任务切模型杠杆 | `CreateWorkflow` / `AmendWorkflow` 工具说明 |
| 试跑门槛谓词 | `EvalWorkflowSnippet` 工具说明 |
| 修脚本续跑、继承热缓存 | `AmendWorkflow` 工具说明 |
| 写作语言、目录落盘、scope 纪律 | `AGENTS.md` §13 |

### 7.3 待裁与未定项

- **ListModels 目录缺失**：本会话实测报 `model_catalog_unavailable`，导致无法核实 `GLM-5.3$max` 的确切拼写与档位集合。规约中沿用任务给定字面量；待目录恢复后须以 `ListModels` 实测结果复核。
- **`dynamic-workflows` skill 加载前置**：本会话该 skill 不在可用列表，§3.6 规则依据为工具说明而非实测；恢复可用后须补一次 `EvalWorkflowSnippet` 试跑作为规约的实测支撑。
- **降级路径**：当 GLM 不可用且目录持续缺失时，是否启用「**降级路径 + 用户确认**」（而非一律报失败）尚未收敛；本规约按 §3.4 一律报失败处理，待 ADR 化时再行讨论。

### 7.4 变更规则

- 本规约按 `AGENTS.md` §6.3 / §6.4「**只增不改**」原则维护：
  - 一旦 v0.1「已接受」，正文不再修改；
  - 决策被推翻时**新开一份** `zcode-model-routing-v0.2.md`，并在本文档的「变更记录」一节标注修正关系。
- 任何推翻本规约结论的改动，若无对应的 v0.2 / ADR，视为**未完成**（`AGENTS.md` §6.4「不许沉默推翻」）。

### 7.5 变更记录

| 版本 | 日期 | 摘要 |
|---|---|---|
| v0.1 | 2026-10-14 | 初稿：GLM-5.3$max ↔ MiniMax-M3 路由决策规则、反模式、不应拆分场景清单 |
