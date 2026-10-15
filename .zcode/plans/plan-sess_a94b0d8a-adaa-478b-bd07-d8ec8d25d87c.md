按上一轮提交的方案执行。第一步：在新 worktree 中工作（用户要求），再按顺序落 10 个步骤。
1. `git worktree add .worktrees/measurement-tool -b feature/measurement-tool` （验证 `.worktrees` 在 .gitignore 中）
2. 数据层：`wv-core/src/source/measurement.rs` 新建
3. World 集成：加 `measurements` 字段与 `LatLonBbox::km_per_wu`
4. `wv-edit`：三个 `Measurement*Command` + 测试
5. `wv-io`：load/write/round-trip + 删除传播
6. `wv-app/src/state.rs`：`MeasureMode` / `MeasureState`
7. `wv-app/src/render/measurement.rs`：cache + click + render
8. `wv-app/src/ui/map_controls.rs`：三按钮
9. `wv-app/src/app_bootstrap.rs`：注册系统
10. 跑 `cargo test --workspace` + `cargo app-clippy` 全绿
11. git commit（不推送）