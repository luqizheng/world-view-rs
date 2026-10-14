# Tasks

> 验证命令（AGENTS §7.1，每个任务完成后必须全绿）：
> 1. `cargo test --workspace`
> 2. `cargo clippy --workspace --all-targets -- -D warnings`
> 3. `cargo fmt --all -- --check`
> 4. `cargo test -p wv-app --features app`
> 5. `cargo clippy -p wv-app --all-targets --features app -- -D warnings`
>
> 提交纪律（AGENTS §13）：每阶段可独立成 commit，但是否提交、何时提交由用户确认，助手不得自行 commit。

- [ ] Task 0: 基线验证——记录五条命令的当前绿色状态与 wv-app（带 app feature）的测试通过数，作为后续对比基线
  - [ ] 依次运行五条验证命令，确认全绿
  - [ ] 记录 `cargo test -p wv-app --features app` 的通过测试数（终验时对比：只允许因删除"只测死代码"的测试而减少）

- [ ] Task 1: 死代码清理（纯删除，零行为变更）
  - [ ] `terrain_view.rs`：删除 JRPG 图集加载路径（`load_tile_atlas` 等，扫描定位 L42-142）；**保留 `TileAtlas` 类型**（`ui_shell.rs` 仍以 `Option<Res<TileAtlas>>` 引用）——动手前 grep `TileAtlas`、`load_tile_atlas` 逐项确认引用面
  - [ ] `terrain_view.rs`：删除 `sync_terrain_sprite`（整体 `if false`，L1414-1444）及仅被它引用的类型（`TerrainSprite` 等，grep 复核）
  - [ ] `projection.rs`：删除 `draw_map_gizmos` / `draw_brush_cursor` 的 `if false` 块、`draw_dashed_circle`、配套 `let _ = &*tex;` 抑制语句（L228-419 区域）
  - [ ] `terrain_chunks.rs`：删除 `pending_dirty_or_diag`（L173-176）与 `if false { eprintln! }`（L522-535）
  - [ ] `ui_tokens.rs`：grep 复核 `motion::FAST/BASE/SLOW` 无引用后删除（L141-147）
  - [ ] `ui.rs`：删除 `avail` 死绑定（L1106 与 L1133）
  - [ ] `gui.rs`：去重 `setup_terrain_texture_after_world`（L272-294）——Startup 改调 `terrain_view::setup_terrain_texture`，删除 gui 侧版本
  - [ ] `palette_jrpg.rs`：grep 全仓库引用；无引用则整文件删除，有引用则保留并在 Task 2 一并重命名
  - [ ] 删除只覆盖上述死代码的测试；逐文件跑五条验证命令

- [ ] Task 2: 文件重命名（`git mv` 保留历史）+ 引用同步
  - [ ] `git mv crates/wv-app/src/ui_cmdk.rs crates/wv-app/src/ui_command_palette.rs`
  - [ ] `git mv crates/wv-app/src/palette.rs crates/wv-app/src/colors.rs`；`palette_google.rs` → `colors_google.rs`；（`palette_jrpg.rs` 若在 Task 1 存活则 → `colors_jrpg.rs`）
  - [ ] `git mv crates/wv-app/src/gui.rs crates/wv-app/src/app_bootstrap.rs`
  - [ ] `git mv crates/wv-app/src/projection.rs crates/wv-app/src/world_projection.rs`
  - [ ] 更新 `lib.rs` 的 `mod` 声明 + grep 旧名（`ui_cmdk`、`palette_google`、`crate::gui`、`crate::projection` 等）同步全部 `use` 路径与注释引用
  - [ ] grep 确认仓库内无旧路径字符串残留（含注释）；跑五条验证命令

- [ ] Task 3: `terrain_view.rs` 不拆物理，写文件头 + 职责段注释（温和路线，2026-10-14 用户裁定）
  - **理由**：原 spec 想拆 `terrain_render.rs` + `terrain_resolve.rs`，但跨模块搬迁需 45 处调用点改 `pub(crate)`，现评估风险/收益不划算。改用"文件名不换 + 文件头 ≤30 行 + 内部按职责用 `// ── 段名 ──` 分段注释"——AI 读文件头即获得"职责、边界、主要导出"，再按分段注释跳转。
  - [ ] 写 `terrain_view.rs` 规范文件头（一句话职责 / 职责清单 / 边界 / 主要导出）
  - [ ] 内部按职责段加注释分隔（建议段：1. TileAtlas & 瓦片合成；2. TerrainTexture 资源；3. 地形数据解析（active_terrain 系列）；4. 像素合成纯函数（CellRenderer 系列）；5. 启动与重建系统；6. 换级淡化；7. Grid 同步）；每段之间空一行方便 grep
  - [ ] 跑五条验证命令（零行为变更，必须全绿）

- [ ] Task 4: `ui.rs`（102KB）不拆物理，写文件头 + 职责段注释（温和路线）
  - **理由**：与 Task 3 同——单一文件拆出需 re-export 大量 pub(crate) 表面，性价比偏低。先把 102KB 缩到「文件头 + 段注释」是更高性价比的路径。
  - [ ] 写 `ui.rs` 规范文件头（≤30 行）
  - [ ] 内部按职责段加注释分隔（建议段：1. 工具条 / 状态栏；2. 项目菜单；3. 命令面板窗口；4. 时间轴 / 笔刷 / 小地图 / 罗盘；5. 检视器 / 势力 / 校验面板；6. 一致性固定 / 单元格渲染辅助）
  - [ ] 跑五条验证命令

- [ ] Task 5: `ui_shell.rs`（44KB）+ `state.rs`（51KB）不拆物理，写文件头（温和路线）
  - **理由**：同上；两文件均承载核心领域逻辑（命令分发、面板状态、播放时间轴），物理拆需新增 pub(crate) 表面，性价比偏低。
  - [ ] 写 `state.rs` 文件头与"边界"段，并显式列出它装哪些资源（`PlaybackState` / `ViewState` / `DrillState` / `BrushState` 等）
  - [ ] 写 `ui_shell.rs` 文件头，并**显式指明**未来可能拆出的 `project_io.rs` / `drill_state.rs` 已是 spec 的远期项
  - [ ] 跑五条验证命令

- [ ] Task 6: 文件头规范化 + `lib.rs` 导航表（wv-app 全模块）
  - **前提**：Task 3-5 的温和路线文件头是 Task 6 的一部分；额外补全小文件
  - [ ] 按 spec 模板为 `wv-app/src` 下**每个** `.rs` 文件写/改文件头（≤30 行：一句话职责 / 职责 / 边界 / 主要导出）；已有文件头（如 `ui_shell.rs`）按模板补"边界"与"主要导出"段
  - [ ] `lib.rs` 顶部写模块导航表：`| 模块 | 职责 | 关键类型 |`，覆盖全部子模块
  - [ ] 检查每个文件头 ≤30 行（文件头本身必须是廉价的）

- [ ] Task 7: 其他 crate 文件头巡检（`wv-core` / `wv-io` / `wv-edit` / `wv-terrain`）
  - [ ] 逐文件检查文件头是否清晰归纳用途；缺失或含混的按同一模板补写
  - [ ] 若发现明显死代码或名不副实文件，**记录不擅动**（scope 纪律），汇总报告给用户另行裁定
  - [ ] 跑五条验证命令

- [ ] Task 8: 文档路径同步（仅路径，不改结论）
  - [ ] grep `AGENTS.md`、`README.md`、`docs/` 中对旧文件名（`gui.rs`、`ui_cmdk`、`palette.rs`、`projection.rs`、`ui.rs`、`terrain_view.rs` 的职责描述）的引用，同步为新路径
  - [ ] 遵守 AGENTS §6.3：只追加/修正路径引用，不改既有设计结论；若某结论因拆分需要改述，列出来等用户裁定
  - [ ] `GLOSSARY.md`：如出现新术语（如"模块导航表"），按 §6.1 登记

- [ ] Task 9: 终验
  - [ ] 五条验证命令全绿
  - [ ] 对比基线：`cargo test -p wv-app --features app` 通过数只减不增，且减少项均有"测试对象是已删死代码"的对应关系
  - [ ] 抽查 3 个重构后文件：文件名准确归纳内容、文件头符合模板、grep 无旧路径残留
  - [ ] 更新 checklist.md 勾选状态

# Task Dependencies

- Task 0 → Task 1 → Task 2 → (Task 3 ∥ Task 4 ∥ Task 5 → Task 6) → (Task 7 ∥ Task 8) → Task 9
- Task 3/4/5 都是"对**未拆分**的单文件写规范文件头 + 职责段注释"，可任意顺序委派
- Task 6 收尾：lib.rs 导航表 + 复检其他小文件头；可与 Task 7/8 并行
- Task 4/5/6 完成 = Task 1 (死代码清理) → Task 2 (重命名) → "文件头/段注释"全部就位 → Task 7 (其他 crate) + Task 8 (文档) → Task 9 (终验)
