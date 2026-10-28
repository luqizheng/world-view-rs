# ADR-0082：自然层单例与 CellMap 编辑 + comment 字段 + LLM 命名

- **状态**：已接受（Accepted，2026-10-28，用户裁定四连：单例 / CellMap 编辑 / comment 字段 / LLM 命名接口）
- **决策者**：用户对话拍板（2026-10-28，自然蛋糕实机验收轮）
- **关联**：修订 [ADR-0081](ADR-0081-nature-layer-partition.md)（入口与分层职责）、扩展 [ADR-0078](ADR-0078-http-ui-control-tools.md)（工具面 15）、新增数据字段（P3）

## 1. 处境

ADR-0081 落地后的实机验收反馈四条：

1. 自然层不该能建多层——**全工程唯一**（一个世界一套自然区划）；
2. 自然层的编辑不该在 Map——**CellMap 才是编辑地**（那里有「显示自然区」与
   候选）；Map 只能**看或不看**；
3. LLM 自动命名自然区域需要**风格来源**：工程与世界地图都缺一个自由描述字段；
4. 需要 HTTP 接口让外部 LLM 取上下文、写回名字。

## 2. 决策

| # | 裁定 | 落点 |
|---|---|---|
| ① | 自然层单例 | Inspector「+ Add nature layer」在已存在自然层时**置灰 + 悬停说明**（UI 级守卫；数据层不设硬闸——YAML 手写两层不崩，校验面板留待办） |
| ② | 编辑归 CellMap，Map 只读 | Inspector 层列表的**自然层行不出 pen**（政治层照旧）；笔刷窗（CellMap 编辑枢纽）新增「自然区划分」段（仅 Topographic 且自然层存在时显示）：「划分自然区 / 退出划分」开关分区编辑态（HUD + 切/拖/改群系三工具与政治层同一套），进入时顺带开 `view.factions_visible` 让分区叠加可见 |
| ③ | comment 字段 | `WorldMeta.comment` + `MapNode.comment`（`Option<String>`，serde skip_if_none，**存量零迁移**）；地图属性对话框加**多行长文本**录入（沿用该对话框中文直写先例）；工程备注的 GUI 录入入口本轮不做（YAML 手改可编辑，登记待办） |
| ④ | LLM 命名两接口 | HTTP 工具面 13→**15**：`get_naming_context`（读：两级备注 + 自然层全部脸 face_id/族名/存名/派生名）+ `set_nature_names`（写：`[{face_id, name}]` 批量，空串=清名回退派生名）。工具描述内嵌**确认流程**：先把备注展示给用户、确认后再提案写回 |

### 命名模型

`PartitionFace.name: Option<String>`（存名）；显示名 = **存名优先**，否则派生名
`<族名><N>`（ADR-0081 规则）。单一实现 `PartitionSpan::face_display_name(face_id,
kind_names)`（wv-core；族名表由调用方传入——core 不认识具体语言）。

**命名是呈现元数据非历史**：`SetNatureFaceNamesCommand` **就地**写（不经 fork），
且**跨全部含该脸的 span**（face id 在 fork 链上恒定 → 名字跨时间一致）。「草原N」
的 N 与存名互不干扰：清名即回派生名。

### 命名流程（agent 编排）

```
用户（对 LLM 代理）：给我的自然区域起名
  → 代理调 get_naming_context（拿到 project_comment / map_comment / faces）
  → 代理把两级备注**展示给用户**（风格来源），等确认          ← 客户确认闸
  → 用户确认后代理按风格提案名字
  → 代理调 set_nature_names 写回 + save_project
```

## 3. 理由

- **单例**：一个世界一套自然区划，两套会互抢格语义（用户直裁）；
- **编辑在 CellMap**：自然区划的语境是地形（画的时候看着格子改），Map 是
  呈现档——与 ADR-0080 的三工作区分区一致（CellMap=地形地貌+POI+自然区编辑）；
- **name 存脸不存层**：名字跟地块走（切出来的两块各自有名）；存名优先 +
  派生名兜底 = LLM 没接也能用；
- **就地批命令**：命名不该产生时间轴噪音（每次改名 fork 一段历史是灾难）。

## 4. 硬约束

- 命名写入必须走 `EditCommand` 链（AGENTS §3.3），dirty = `"partitions"`；
- `face_display_name` 是显示名**唯一**口径（wv-app 手势/易主窗、HTTP 回执同源）；
- comment 是自由文本，不进任何校验 / 派生逻辑（与 `unit_per_km` 的纯展示
  纪律同款）。

## 5. 后果

**正面**：LLM 命名闭环（取上下文→确认→写回）；自然层的职责归属清晰
（CellMap 编辑 / Map 呈现）；风格来源可沉淀在工程里随 Git 走。

**负面 / 代价**：自然层单例是 UI 级守卫（手写 YAML 双层不拦截——校验条目
待办）；工程备注无 GUI 录入（待办）；工具描述里的确认流程靠 LLM 自觉
（协议级强制做不到，MCP 无交互审批原语）。

**被否决的备选**：名字挂在层上的 `HashMap<face_id, String>`（脸被 fork 出
新 id 时映射失锚）；命名走 fork span（时间轴噪音）；GUI 内嵌 LLM 调用
（P1 单机可运行——API key/网络进核心是红线）。

## 6. 与既有 ADR 的关系

- **修订** ADR-0081（入口：Map 层列表 pen → CellMap 笔刷窗；单例约束）；
- **扩展** ADR-0078 工具面（13→15，命名两工具归数据类 skill）；
- 数据字段新增遵循 §5 二进制分块纪律的反面——纯 YAML（P3），零迁移。
