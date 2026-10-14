# Checklist

## 基线与死代码
- [ ] Task 0 已记录五条验证命令的绿色基线与 wv-app（app feature）测试通过数
- [ ] 每一项死代码删除前都做过 grep 引用复核（不是只凭扫描报告行号）
- [ ] `TileAtlas` 类型在删除图集加载路径后仍保留（`ui_shell.rs` 引用不破）
- [ ] `palette_jrpg.rs` 的去留有 grep 证据支撑
- [ ] 死代码删除后五条验证命令全绿

## 重命名
- [ ] 四次 `git mv` 完成：`ui_cmdk.rs→ui_command_palette.rs`、`palette*.rs→colors*.rs`、`gui.rs→app_bootstrap.rs`、`projection.rs→world_projection.rs`
- [ ] 全仓库 grep 无旧路径字符串残留（含注释）
- [ ] "命令面板"与"颜色"两个语义不再共用 `palette` 一词

## 上帝模块拆分
- [ ] `ui/` 目录拆分后，`crate::ui::command_palette`、`crate::ui::ProjectMenuState` 等既有路径不变（`ui_shell.rs` / `app_bootstrap.rs` 引用点零改动）
- [ ] `terrain_view.rs` 拆分后，迁移的 `pub fn` 可见性收紧为 `pub(crate)`，外部引用点路径已同步
- [ ] `project_io.rs` 承载全部落盘/加载收口；`ui_shell.rs` 文件头"边界"段指路
- [ ] `drill_state.rs` 从 `state.rs` 迁出 `DrillState` 及其方法与测试
- [ ] 拆分迁移的测试全部随迁并通过，无测试丢失（对照 Task 0 基线）

## 文件头与导航
- [ ] `wv-app/src` 下每个 `.rs` 文件头符合 spec 模板（一句话职责 / 职责 / 边界 / 主要导出）且 ≤30 行
- [ ] `lib.rs` 顶部有覆盖全部子模块的导航表（模块 → 职责 → 关键类型）
- [ ] 其他四个 crate（wv-core / wv-io / wv-edit / wv-terrain）文件头巡检完成；发现的问题记录未擅动

## 文档同步
- [ ] AGENTS.md / README / docs 中的旧文件路径引用已同步
- [ ] 未修改任何既有设计结论（AGENTS §6.3）；需改述的条目已列出待用户裁定
- [ ] 新术语（若有）已登记 GLOSSARY.md

## 终验
- [ ] 五条验证命令全绿（workspace test / clippy / fmt + app-feature test / clippy）
- [ ] wv-app（app feature）测试通过数对比基线：减少项均对应已删死代码
- [ ] 抽查 3 个重构后文件：文件名准确、文件头合规、无旧路径残留
- [ ] 未发生任何行为变更、公开 API 语义变更、数据格式变更
- [ ] 全程未未经确认擅自 commit
