# Checklist

## 死代码
- [x] `danger_button` 在 ui 目录内零提及（grep `crates/wv-app/src/ui/` 输出空）
- [x] `let _ = avail;` 在 ui 目录内零提及
- [x] 删除后五条验证命令全绿

## 物理拆分
- [x] `crates/wv-app/src/ui.rs` 已删除（`git status` 标记为 `D`）
- [x] `crates/wv-app/src/ui/` 目录已创建（17 个文件：1 个 mod.rs + 16 个子模块）
- [x] `crates/wv-app/src/ui/mod.rs` 含 16 个子模块声明 + 27 个 `pub use` re-export
- [x] `crates/wv-app/src/lib.rs` 零改动（`pub mod ui;` 解析为 `ui/mod.rs`，`git status` 验证）

## 路径 API 兼容
- [x] `crates/wv-app/src/drill.rs:611` 的 `crate::ui::UiShell::new` 无改动
- [x] `crates/wv-app/src/ui_shell.rs:55` 的 `crate::ui::ProjectMenuState` 无改动
- [x] `crates/wv-app/src/ui_shell.rs:87` 的 `crate::ui::command_palette` 无改动
- [x] `crates/wv-app/src/ui_tokens.rs:293` 的 `crate::ui::BadgeKind` 无改动
- [x] 其他 23 个公开符号（`top_bar` / `status_bar` / `breadcrumb` / `map_controls` / `float_window` / `badge` / `list_item` / `empty_state` / `terrain_grid` / `panel_*`）路径全部 `pub use` 重导出

## 文件头
- [x] 17 个新文件均带规范文件头（一句话职责 / 职责 / 边界 / 主要导出）
- [x] 每个文件头 ≤30 行（实测：最小 10 行、最大 17 行、mod.rs 17 行）
- [x] 每个文件头保留 `#![cfg(feature = "app")]`

## 验证
- [x] 五条验证命令全绿（workspace test / clippy / fmt + app-feature test / clippy）
- [x] `cargo test -p wv-app --features app` 通过数对比 Task 1 基线**不变**（238 = 238）
- [x] `git status --short` 显示 ui.rs 删除 + ui/ 目录新增（其他文件零改动）
- [x] 无公开 API 变更、Bevy 系统签名变更、YAML 格式变更、用户可见行为变更

## 边界
- [x] 未改动其他文件（伞型 spec `reduce-ai-reading-cost` 的其他文件由其独立推进）
- [x] 未做函数级重构（仅做物理拆分 + 文件头 + 死代码清理）
- [x] 未提交（AGENTS §13：不主动 commit，等用户确认）

## 文件清单（拆分对照）

| 源行号 | 新文件 | 新行数 |
|---|---|---|
| 67-170 | `ui/shell.rs` | 102 |
| 175-528 | `ui/project_menu.rs` | 289 |
| 533-1121 | `ui/widgets.rs` | 219 |
| 586-953 | `ui/map_controls.rs` | 201 |
| 615-1030 | `ui/top_bar.rs` | 263 |
| 1218-1267 | `ui/widgets.rs`（terrain_grid） | — |
| 1269-1726 | `ui/timeline.rs` | 277 |
| 1519-1588 | `ui/layers_panel.rs` | 65 |
| 1591-1697 | `ui/brush_panel.rs` | 137 |
| 1728-1838 | `ui/minimap_panel.rs` | 153 |
| 1840-2015 | `ui/compass_panel.rs` | 219 |
| 2018-2049 | `ui/inspector_panel.rs` | 49 |
| 2051-2105 | `ui/situation_panel.rs` | 65 |
| 2108-2305 | `ui/faction_panel.rs` | 225 |
| 2307-2471 | `ui/validation_panel.rs` | 177 |
| 2473-2572 | `ui/list_panels_panel.rs` | 87 |
| 2577-2686 | `ui/command_palette_window.rs` | 113 |
| mod.rs | `ui/mod.rs` | 57 |

## 体积对比

| 文件 | 重构前 | 重构后 |
|---|---|---|
| `ui.rs` | 102 KB / 2687 行 | 0（已删） |
| `ui/mod.rs` | — | 57 行 |
| `ui/` 子文件合计 | — | 2810 行（16 文件，单文件最大 289 行，最小 49 行） |
| **总量** | 2687 行 | 2867 行（+180 行 = 文件头 + use + mod 声明）