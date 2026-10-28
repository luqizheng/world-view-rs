# 实施计划：HTTP UI 操控工具（ADR-0078 / spec 2026-10-28-http-ui-control-design）

> 执行环境约定：worktree `feat/http-ui-control`；构建目录隔离 `CARGO_TARGET_DIR=.target`
> （每条命令重发，Bash 不留 env）；测试 `TEMP/TMP` 指向本 worktree `.target/testtmp`
> （受限环境 %TEMP% 写入假失败，AGENTS §2.2 追记）；**勿用** `scripts/cargo-test.sh`（worktree 纪律）。
> 渲染/UI 闸门：本改动全在 `--features http`（隐含 `app`）门后，标准 workspace 三闸门**不编译它**
> ——必须额外跑 `http-test` / `http-clippy` 与 `app-test` / `app-clippy`。

- [x] **T1 ADR + spec + plan 落盘**（本目录三份文档；ADR 索引/GLOSSARY 收口在 T8）
- [x] **T2 协议层**：`mcp.rs` 增 `ToolContent::Image`（serde 形态测试）；`tools.rs` 增 `ToolOutput` 枚举、`call_tool` 按 `Json|Image` 组 content
- [x] **T3 桥**：`bridge.rs` 增 `UiRequest` / `UiReply` / `UiStateSnapshot` / `ScreenshotPayload`；`drain_messages_with` + 旧签名包装（回「UI 桥未接入」）；两条桥测试
- [x] **T4 UI 工具纯函数**：新文件 `http/skills/ui.rs`——`parse_view_mode`（与 `debug_probe::apply_env_view_mode` 同源词表，后者改调它）、`parse_panel`（15 词）、`handle_ui_request`（get/set×2，set echo 新快照）、`png_dimensions`（IHDR 16..24）、`shot_path`（默认 temp/wv-screenshots + 毫秒名）；全词表测试
- [x] **T5 Bevy 接线**：`http_integration.rs`——`drain_http_commands` 增参（ViewState / PanelState / Commands / PendingShots）；`PendingShots` 资源 + `poll_pending_shots` 系统（文件出现→base64→回执；60 帧兜底 Error）；`skills/ui.rs` 挂 `mod.rs`
- [x] **T6 截图 skill 接线**：`skills/ui.rs` 的 `screenshot` handler 产出 `ToolOutput::Image`（base64 + meta）；`tools.rs` 注册 4 schema；`server.rs` 契约测试 9→13 + 端到端 `set_view_mode`（带 UI 钩子 drain）
- [x] **T7 门禁**：`cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets -- -D warnings`；`cargo test --workspace`；`cargo clippy -p wv-app --all-targets --features http -- -D warnings` + 同 feature test；`cargo app-test` / `app-clippy`；`bash scripts/smoke-gui.sh`
- [x] **T8 文档收口**：README §10.1 工具表 9→13 + 用法示例；`docs/adr/README.md` 索引行；`GLOSSARY.md` 登记「UI 操控工具」；提交（feat 分支，不合 main）
- [x] **T9 实机 e2e**：`cargo run -p wv-app --features http --bin wv -- gui -m examples/jrpg-world` 后台起，curl 四工具各一发（`get_ui_state` → `set_panel_visible brush=false` → `set_view_mode cellmap` → `screenshot`），截图 base64 落文件验 PNG 头 + 尺寸合理

## 风险与回退

- `drain_http_commands` 参数增长不触 Bevy 16 参数上限（现 4→8 个以内）；
- `Screenshot` spawn 依赖 winit 窗口存在——headless / 欢迎页（无地图）时窗口仍在，能出图；出不了图走 60 帧兜底 Error，不挂死 handler；
- 回退点：整个特性挂在 `--features http` 门后，关 feature 即零影响。
