# 降低 AI 读码成本：文件级重构 Spec

## Why

AI 助手（与新人）定位代码时最贵的操作是通读大文件。当前 `wv-app` 有四个 5 万字节以上的上帝模块（`ui.rs` 102KB、`terrain_view.rs` 98KB、`ui_shell.rs` 51KB、`state.rs` 51KB），以及一批名不副实或语义撞车的文件名（`ui_cmdk` 是命令面板却用黑话、`palette.rs` 是颜色却与"命令面板"撞车、`gui.rs` 实为 Bevy 装配层、`projection.rs` 与 Bevy 相机 `Projection` 撞名）。AI 只能通读全文才能判断"逻辑在哪个文件"，token 成本高且易误读。

**目标**：建立「文件名 → 文件头注释 → 按需读正文」三级漏斗，AI 在前两级就能决定要不要读、读哪个文件。

## What Changes

- **统一文件头规范**：每个 `.rs` 模块以规范 `//!` 头开头（一句话职责 + 职责清单 + 边界 + 主要导出，≤30 行）
- **重命名 6 个语义不清/撞车文件**（用 `git mv` 保留历史）
- **拆分 4 个上帝模块**为按职责命名的子文件
- **删除已确认死代码**约 250 行（`if false` 回退块、已弃用图集加载等）
- **`lib.rs` 顶部补模块导航表**（模块 → 职责一句话 → 关键类型）
- **同步文档中的文件路径引用**（AGENTS.md / README / docs，仅改路径，不改任何结论）
- **非目标（明确排除）**：函数级重构（`WorldCtx` 参数对象、`dispatch` 切块、样板 helper 抽取等此前清单中的条目）、任何行为变更、任何数据格式变更

### 文件结构总表

| 现文件 | 动作 | 结果 |
|---|---|---|
| `ui.rs`（102KB，约 17 个面板/系统混居） | 拆分 | `ui/` 目录：`mod.rs`（共享 helper + re-export）+ 每个面板一个文件 |
| `terrain_view.rs`（98KB，6 类职责） | 拆分 | `terrain_render.rs`（像素合成纯函数）+ `terrain_resolve.rs`（active_terrain/占位解析）+ `terrain_view.rs`（资源与系统） |
| `ui_shell.rs`（51KB） | 拆分 | 抽出 `project_io.rs`（落盘/加载收口）；`ui_shell.rs` 保留装配/快捷键/分发 |
| `state.rs`（51KB，8 个资源类型） | 拆分 | 抽出 `drill_state.rs`（`DrillState` 及其方法与测试）；其余留 `state.rs` |
| `ui_cmdk.rs` | 重命名 | `ui_command_palette.rs` |
| `palette.rs` / `palette_google.rs` / `palette_jrpg.rs` | 重命名 | `colors.rs` / `colors_google.rs` / `colors_jrpg.rs`（`colors_jrpg` 先验证是否死代码，是则删除） |
| `gui.rs` | 重命名 | `app_bootstrap.rs`（Bevy App 装配层：插件、相机、Startup） |
| `projection.rs` | 重命名 | `world_projection.rs`（`WorldProjection` 的求值系统；死代码删除后仅剩求值） |
| 其余文件 | 补文件头 | 文件名保持不变 |

### 死代码清单（位置来自只读扫描报告，动手前必须逐项 grep 复核）

| 位置 | 内容 | 处置 |
|---|---|---|
| `terrain_view.rs:42-142` | JRPG 图集加载路径（`load_tile_atlas` 等，注释明示已弃用，`ensure_atlas` 恒 `None`） | 删加载路径；`TileAtlas` **类型保留**（`ui_shell.rs` 仍引用） |
| `terrain_view.rs:1414-1444` | `sync_terrain_sprite` 整体 `if false`（已被 `terrain_chunks` 接替） | 删（含仅被它引用的 `TerrainSprite` 等） |
| `projection.rs:228-292 / 344-419` | `draw_map_gizmos` / `draw_brush_cursor` 的 `if false` 块 + `draw_dashed_circle` | 删（含 `let _ = &*tex;` 抑制语句） |
| `terrain_chunks.rs:173-176 / 522-535` | `pending_dirty_or_diag`（恒 false 无调用方）、`if false { eprintln! }` | 删 |
| `ui_tokens.rs:141-147` | `motion::FAST/BASE/SLOW`（无引用） | grep 复核后删 |
| `ui.rs:1106 / 1133` | `avail` 死绑定 | 删两行 |
| `gui.rs:272-294` | `setup_terrain_texture_after_world` 与 `terrain_view::setup_terrain_texture` 重复 | 去重：Startup 改用后者，删前者 |
| `palette_jrpg.rs` 全文件 | JRPG 配色（疑似随图集一同弃用） | grep 复核：无引用则整文件删 |

## Impact

- **Affected code**：`wv-app` 全部模块（均在 `#![cfg(feature = "app")]` 下）；`wv-core` / `wv-io` / `wv-edit` / `wv-terrain` 仅补文件头，不改名不拆分
- **不受影响**：公开 API 语义、Bevy 系统签名、YAML 数据格式（round-trip 不变）、任何用户可见行为
- **触及原则**：无（P1–P7 均不涉及；纯 `wv-app` 内部结构调整 + 注释 + 文档路径同步）
- **测试影响**：仅删除"只测死代码"的测试；其余测试（含 app-feature 集成测试）必须原样通过

## ADDED Requirements

### Requirement: 模块文件头导航

每个 `.rs` 模块 SHALL 以如下格式的 `//!` 文件头开头，且不超过 30 行：

```
//! <一句话：这个模块是什么。>
//!
//! ## 职责
//! - <1–5 条要点>
//!
//! ## 边界（不做什么）
//! - <相邻模块负责的事，1–3 条；这是 AI 的"别来这找"路由信息>
//!
//! ## 主要导出
//! - [`Type`] / `fn name` — <一句话>
```

文件头用中文（项目工作语言），标识符保持英文。

#### Scenario: AI 通过文件名和文件头定位代码
- **WHEN** AI 需要修改"罗盘面板"的绘制行为
- **THEN** 它从 `lib.rs` 导航表或文件名 `ui/compass_panel.rs` 直接锁定文件，读 ≤30 行文件头确认职责与边界，无需通读 102KB 的旧 `ui.rs`

#### Scenario: AI 排除错误文件
- **WHEN** AI 查找"保存工程到磁盘"的逻辑
- **THEN** `project_io.rs` 的文件头"职责"段命中，而 `ui_shell.rs` 的"边界"段明确写着落盘在 `project_io.rs`，AI 不会误读装配层

### Requirement: 文件名与职责一一对应

文件名 SHALL 准确归纳文件内容；语义撞车的命名 SHALL 消除（命令面板 vs 颜色、世界投影 vs 相机投影）。一个文件承载多类不相关职责时 SHALL 拆分。

#### Scenario: 撞车命名消除
- **WHEN** AI 看到 `ui_command_palette.rs` 与 `colors.rs`
- **THEN** 前者必是 Ctrl+K 命令面板，后者必是颜色定义，无需读正文消歧

### Requirement: lib.rs 模块导航表

`wv-app/src/lib.rs` 顶部 SHALL 包含模块导航表（模块 → 职责一句话 → 关键类型），作为 AI 的第一入口。

#### Scenario: 全局导航
- **WHEN** AI 首次进入代码库，不知从何读起
- **THEN** 读 `lib.rs` 导航表（<100 行）即可获得全部模块的一句话地图

### Requirement: 死代码清零

已确认的死代码（`if false` 回退块、无引用的弃用路径、死绑定）SHALL 删除；删除后全量测试必须通过。

#### Scenario: 删除不破坏行为
- **WHEN** 删除 `sync_terrain_sprite` 等死代码后运行五条验证命令
- **THEN** 全部通过，且 `cargo test -p wv-app --features app` 的通过数仅因"只测死代码"的测试移除而减少

## MODIFIED Requirements

> **2026-10-14 临时注**：上面的"关闭/新建工程接管"原本提交在工作区未保存 state、git revert 时被一起回滚。已从 spec 移除（不留悬空条目）。**接管代码并未真的落地**——如果回归修复仍有需求，请单独开 spec/ADR，本 spec 不预设。
>
> 当前分支仅为 spec.md / spec 所描述的"文件名 + 文件头 + lib.rs 导航表 + 死代码清理 + 4 个文件重命名"。

## REMOVED Requirements

无。
