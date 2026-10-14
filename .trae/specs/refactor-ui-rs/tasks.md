# Tasks

> 验证命令（AGENTS §7.1，每个任务完成后必须全绿）：
> 1. `cargo test --workspace`
> 2. `cargo clippy --workspace --all-targets -- -D warnings`
> 3. `cargo fmt --all -- --check`
> 4. `cargo test -p wv-app --features app`
> 5. `cargo clippy -p wv-app --all-targets --features app -- -D warnings`
>
> 提交纪律（AGENTS §13）：本次不主动 commit，全部改动以本地状态交付，等用户明确确认后再开 commit。
>
> 文件操作纪律（AGENTS §13 scope 纪律 + AGENTS §8 命名规范）：
> - `git mv` 保留历史——但本 spec 是"删 ui.rs → 新增 ui/ 目录"，是 git mv 等价操作（先删除原文件、新增所有新文件）
> - 子模块命名小写 snake_case，文件名即职责

---

- [x] Task 1: 预检 + 死代码清零
  - [x] grep `crates/wv-app/src/ui.rs` 内 `danger_button` 全部出现位置（应仅 line 6 / 21 / 45 三处文件头文本提及）——实测：line 6 / 21 / 45 三处，0 处实际定义
  - [x] grep `crates/wv-app/src/ui.rs` 内 `let _ = avail;` 出现位置（应仅 line 1164 一处）——实测：line 1164 一处
  - [x] grep `crate::ui::` 全仓库引用，列出所有外部调用点（4 处代码 + 2 处文档）——实测：`drill.rs:611` / `ui_shell.rs:55` / `ui_shell.rs:87` / `ui_tokens.rs:293`
  - [x] 跑五条验证命令记录基线——实测：workspace 329 passed / app-feature 238 passed / 3 clippy + fmt --check 全绿

- [x] Task 2: 新建 `ui/` 子目录（17 个文件）
  - [x] 新建 `crates/wv-app/src/ui/mod.rs`，含文件头 + 16 个子模块声明 + 27 个 `pub use` re-export
  - [x] 新建 `crates/wv-app/src/ui/shell.rs`（UiShell + TERRAIN_NAMES + terrain_swatch）
  - [x] 新建 `crates/wv-app/src/ui/top_bar.rs`（top_bar + identity / view_mode / readout / status_bar）
  - [x] 新建 `crates/wv-app/src/ui/map_controls.rs`（map_controls + set_map_scale + root_ui + rect_stroke + `#[cfg(test)] mod map_control_tests`）
  - [x] 新建 `crates/wv-app/src/ui/project_menu.rs`（ProjectMenuState + MenuRow + project_menu_inner + menu_section / menu_row + breadcrumb）
  - [x] 新建 `crates/wv-app/src/ui/command_palette_window.rs`（command_palette）
  - [x] 新建 `crates/wv-app/src/ui/timeline.rs`（TimelineLayer + panel_timeline + draw_timeline_tracks + undo_last_street_segment）
  - [x] 新建 `crates/wv-app/src/ui/layers_panel.rs`（panel_map_tree + panel_layers）
  - [x] 新建 `crates/wv-app/src/ui/brush_panel.rs`（panel_brush）
  - [x] 新建 `crates/wv-app/src/ui/minimap_panel.rs`（panel_minimap）
  - [x] 新建 `crates/wv-app/src/ui/compass_panel.rs`（panel_compass + push_orientation_cmd 私有名）
  - [x] 新建 `crates/wv-app/src/ui/inspector_panel.rs`（panel_inspector）
  - [x] 新建 `crates/wv-app/src/ui/situation_panel.rs`（panel_situation）
  - [x] 新建 `crates/wv-app/src/ui/faction_panel.rs`（panel_faction + faction_card）
  - [x] 新建 `crates/wv-app/src/ui/validation_panel.rs`（panel_validation）
  - [x] 新建 `crates/wv-app/src/ui/list_panels_panel.rs`（panel_character + panel_event）
  - [x] 新建 `crates/wv-app/src/ui/widgets.rs`（BadgeKind + badge + empty_state + list_item + float_window + panel_header + terrain_grid + rect_stroke）
  - [x] 拆 `panel_minimap`（line 1728-1838）→ `minimap_panel.rs`（独立）
  - [x] 拆 `terrain_grid`（line 1218-1267）→ `widgets.rs`（被 `panel_brush` 跨子模块调用）

- [x] Task 3: 删除原 `ui.rs` + 跑验证
  - [x] 删除 `crates/wv-app/src/ui.rs`
  - [x] 跑五条验证命令（外部引用点 4 处零改动 → 编译必须通过）
  - [x] 修复编译报错：补 import（`BadgeKind` / `Panel` / `Vec2`）+ 清 9 处 warning（未使用 import）

- [x] Task 4: 文件头规范化（每个新文件 ≤30 行文件头）
  - [x] 17 个文件均带规范文件头（最小 10 行、最大 17 行）
  - [x] grep 确认每个文件头 ≤30 行

- [x] Task 5: 终验
  - [x] 五条验证命令全绿（workspace test 329 / app-feature 238 / 3 clippy 全绿 / fmt --check 全绿）
  - [x] `cargo test -p wv-app --features app` 通过数对比 Task 1 基线**不变**（基线 238，现 238）
  - [x] `git status --short` 仅显示 `ui.rs` 删除 + `ui/` 目录新增，其他文件零改动
  - [x] `grep -r "danger_button\|let _ = avail;" crates/wv-app/src/ui/` 输出为空
  - [x] `grep "crate::ui::" crates/wv-app/src/` 全部命中路径仍能解析（路径 API 完全兼容）
  - [x] 抽查 3 个新文件：`shell.rs` 头 17 行 / `widgets.rs` 头 17 行 / `mod.rs` 头 17 行
  - [x] 更新 checklist.md 勾选状态

# Task Dependencies

Task 1 → Task 2 → Task 3 → Task 4 → Task 5（顺序：先预检 + 死代码基线 → 新建子目录 → 删原文件并修编译 → 文件头规范化 → 终验）

任务边界：
- 本 spec **不**触碰伞型 spec `reduce-ai-reading-cost` 的其他文件（`terrain_view.rs` / `ui_shell.rs` / `state.rs` 等由其独立推进）
- 本 spec **不**触碰 `lib.rs`（`pub mod ui;` 解析为 `ui/mod.rs`，零改动）
- 物理拆分需补 import + 清 warning 6 处，均属"修编译错误"范畴（Task 3 spec 已包含）；未引入新公开 API