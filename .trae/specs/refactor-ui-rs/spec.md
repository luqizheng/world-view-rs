# `ui.rs` 物理拆分 Spec

## Why

`crates/wv-app/src/ui.rs` 现 102KB / 2687 行，是 `wv-app` 第二大模块。用户明确要求物理拆分以降低 AI / 新人通读成本。

外部对 `crate::ui::*` 的引用面窄（grep 全部命中）：
- `crate::ui::UiShell::new`（`drill.rs:611`）
- `crate::ui::ProjectMenuState`（`ui_shell.rs:55`）
- `crate::ui::command_palette`（`ui_shell.rs:87`）
- `crate::ui::BadgeKind`（`ui_tokens.rs:293`）

**只有 4 个公开符号被外部依赖**——这是物理拆分的最佳时机：拆分后用 re-export 保持路径兼容，调用方零改动。

## What Changes

把 `ui.rs` 物理拆分为 `crates/wv-app/src/ui/` 子目录 + 一个新 `ui/mod.rs`（仅含文件头 + 子模块声明 + re-export）。新结构：

```
crates/wv-app/src/
├── ui.rs                          # 删除
└── ui/
    ├── mod.rs                     # 新：文件头 + mod 声明 + pub use re-export
    ├── shell.rs                   # UiShell 样式快照 + TERRAIN_NAMES + terrain_swatch
    ├── top_bar.rs                 # 顶栏（top_bar / identity / view_mode / readout） + 状态栏（status_bar）
    ├── map_controls.rs            # Google 式地图控件 + set_map_scale + root_ui + rect_stroke
    ├── project_menu.rs            # ProjectMenuState / MenuRow / project_menu / menu_section / menu_row / breadcrumb
    ├── command_palette_window.rs  # command_palette（egui 绘制入口）
    ├── timeline.rs                # TimelineLayer + panel_timeline + draw_timeline_tracks + undo_last_street_segment + panel_minimap + terrain_grid
    ├── layers_panel.rs             # panel_layers + panel_map_tree
    ├── brush_panel.rs             # panel_brush
    ├── compass_panel.rs           # panel_compass + push_cmd
    ├── inspector_panel.rs         # panel_inspector
    ├── situation_panel.rs          # panel_situation
    ├── faction_panel.rs           # panel_faction + faction_card
    ├── validation_panel.rs        # panel_validation
    ├── list_panels_panel.rs       # panel_character + panel_event
    └── widgets.rs                 # BadgeKind + badge + empty_state + list_item + float_window + panel_header
```

### 模块切分依据

| 新模块 | 行数（估） | 共享依赖 |
|---|---|---|
| `shell.rs` | ~110 | `crate::ui_tokens` / `crate::ui_theme` / `crate::colors` |
| `top_bar.rs` | ~280 | `shell.rs` 的 `UI` 命令 + `crate::ui_command_palette::CommandId` |
| `map_controls.rs` | ~225 | `shell.rs` + `crate::camera` + `crate::zoom_level` + `crate::colors_google` |
| `project_menu.rs` | ~250 | `shell.rs` + `crate::ui_command_palette::CommandId` |
| `command_palette_window.rs` | ~115 | `shell.rs` + `crate::ui_command_palette` |
| `timeline.rs` | ~325 | `shell.rs` + `crate::editing` + `crate::streets` |
| `layers_panel.rs` | ~45 | `shell.rs` |
| `brush_panel.rs` | ~145 | `shell.rs` + `crate::editing` + `crate::streets` |
| `compass_panel.rs` | ~175 | `shell.rs` + `crate::editing` |
| `inspector_panel.rs` | ~30 | `shell.rs` + `widgets.rs::empty_state` |
| `situation_panel.rs` | ~60 | `shell.rs` + `widgets.rs::list_item` |
| `faction_panel.rs` | ~220 | `shell.rs` + `crate::editing` + `crate::faction_panel_actions` |
| `validation_panel.rs` | ~165 | `shell.rs` + `crate::validation_actions` |
| `list_panels_panel.rs` | ~60 | `shell.rs` + `widgets.rs::list_item` |
| `widgets.rs` | ~150 | `shell.rs` |
| `mod.rs` | ~50 | — |

### 路径兼容（关键约束）

`mod.rs` 必须 `pub use` 全部 27 个公开符号，确保以下引用点零改动：

- `crate::ui::UiShell` (`drill.rs:611`)
- `crate::ui::ProjectMenuState` (`ui_shell.rs:55`)
- `crate::ui::command_palette` (`ui_shell.rs:87`)
- `crate::ui::BadgeKind` (`ui_tokens.rs:293`)
- `crate::ui::top_bar`、`status_bar`、`breadcrumb`、`map_controls`、`float_window`、`badge`、`list_item`、`empty_state`、`terrain_grid`、`panel_*` 等（`ui_shell.rs` 在系统注册里使用，本 spec 落地后由伞型 spec `reduce-ai-reading-cost` 推动改路径，但本 spec **保持兼容**）

### 顺手清理（属 ui.rs 内务）

- 文件头"主要导出"移除不存在的 `danger_button`（grep 复核：文件内仅 3 处文本提及，0 处实际定义）。
- 删除 line 1164 死绑定 `let _ = avail;`。
- 段序号对齐 1→6（拆分后每文件自带文件头，不再需要段序号，但仍保留段首注释作为该文件内"职责段"分隔符——若文件长度>100 行）。

### 非目标（明确排除）

- **不**改伞型 spec `reduce-ai-reading-cost` 的其他文件（`terrain_view.rs` / `ui_shell.rs` / `state.rs` 等由其独立推进）。
- **不**重命名 `ui.rs`（删除它，由 `ui/` 目录替代；`lib.rs` 仍 `pub mod ui;`，路径兼容靠 `ui/mod.rs`）。
- **不**做函数级重构（`WorldCtx` 参数对象、`set_command` helper 等）——降低本 spec 风险。
- **不**改任何公开 API、Bevy 系统签名、YAML 格式、用户可见行为。

## Impact

- **Affected code**：
  - 删 `crates/wv-app/src/ui.rs`（单文件）
  - 新增 `crates/wv-app/src/ui/` 目录（15 个文件 + 1 个 mod.rs）
  - **`crates/wv-app/src/lib.rs` 零改动**（`pub mod ui;` 解析为 `ui/mod.rs`，路径完全等价）
  - 外部引用点（4 处，参见 What Changes 路径兼容段）零改动
- **Affected specs**：与伞型 spec `reduce-ai-reading-cost` 边界明确——本 spec 落地其 Task 1（ui.rs 死代码剔除）+ 实体化其 Task 4 物理拆分（而非温和路线）。
- **测试影响**：本次仅删 line 1164 死绑定（无测试覆盖）；其余测试原样搬迁。`cargo test -p wv-app --features app` 通过数**不变**。

### Requirement: 物理拆分到 `ui/` 子目录

`crates/wv-app/src/ui.rs` SHALL 被删除；其内容 SHALL 拆入 `crates/wv-app/src/ui/` 子目录；`crates/wv-app/src/lib.rs` 的 `pub mod ui;` 声明零改动。

#### Scenario: 单文件搜索落到正确子模块
- **WHEN** AI 搜 `panel_brush` 实现
- **THEN** `grep "fn panel_brush"` 在 `crates/wv-app/src/ui/brush_panel.rs` 一行命中（不再在 102KB 文件中跳转）

#### Scenario: 模块边界清晰
- `ui/widgets.rs` 仅承载共享 widget helper（`BadgeKind` / `badge` / `empty_state` / `list_item` / `float_window` / `panel_header`）
- `ui/shell.rs` 仅承载样式快照（`UiShell` / `TERRAIN_NAMES` / `terrain_swatch`）
- `ui/command_palette_window.rs` 仅承载 `command_palette` 一个 entry（`ui_command_palette.rs` 是后端注册表，不重复）

### Requirement: 路径 API 完全兼容

`crates/wv-app/src/ui/mod.rs` SHALL 用 `pub use` re-export 全部 27 个公开符号；外部对 `crate::ui::UiShell::new` / `crate::ui::ProjectMenuState` / `crate::ui::command_palette` / `crate::ui::BadgeKind` 的 4 处引用 SHALL 零改动编译通过。

#### Scenario: 引用方零改动
- **WHEN** 物理拆分完成后跑 `cargo build -p wv-app --features app`
- **THEN** `drill.rs` / `ui_shell.rs` / `ui_tokens.rs` 不需要任何路径修改

### Requirement: 文件头规范

每个新文件 SHALL 以 ≤30 行的规范 `//!` 文件头开头（一句话职责 + 职责清单 + 边界 + 主要导出），与伞型 spec `reduce-ai-reading-cost` Task 6 模板一致。

#### Scenario: 文件头廉价
- **WHEN** AI 读任一 ui 子模块
- **THEN** 前 ≤30 行即获得职责 / 边界 / 主要导出，无需读全文

### Requirement: 死代码清零

`danger_button` 在 ui 目录内 SHALL 零文本提及（删除文件头导出段 3 处 + 移除 line 1164 `let _ = avail;` 死绑定）。

#### Scenario: 死代码彻底清除
- **WHEN** `grep -r "danger_button\|let _ = avail;" crates/wv-app/src/ui/`
- **THEN** 输出为空

## MODIFIED Requirements

无（保留伞型 spec `reduce-ai-reading-cost` 的 MODIFIED Requirements 段）。

## REMOVED Requirements

无。