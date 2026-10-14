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
> - 子模块命名小写 snake_case，文件名即职责

---

- [x] Task 1: 预检 + 基线
  - [x] 跑五条验证命令记录基线——实测：workspace 329 / app-feature 238 / 3 clippy + fmt --check 全绿
  - [x] grep `crates/wv-app/src/terrain_view.rs` 内 `pub` 全部公开符号——实测：16 个 `pub` + 2 个 `pub(crate)` (`replace_terrain_texture` / `new_terrain_texture` 实际被 `app_bootstrap.rs` 与 `ui_shell.rs` 跨模块调用，需 `pub`)
  - [x] grep `crate::terrain_view::` 全仓库引用——实测：10 处代码 + 5 处文档引用

- [x] Task 2: 新建 `terrain_view/` 子目录（8 个文件 + mod.rs）
  - [x] 新建 `crates/wv-app/src/terrain_view/mod.rs`，含文件头 + 7 个子模块声明 + 15 个 `pub use` re-export + 32 测试函数（843 行 tests 块）
  - [x] 新建 `crates/wv-app/src/terrain_view/tile_atlas.rs`（TileAtlas + load_tile_atlas + ensure_atlas + tile_res_for + TileTint + sample_tile_block + blit_block_tinted，281 行）
  - [x] 新建 `crates/wv-app/src/terrain_view/terrain_texture.rs`（TerrainTexture + mark_* + rect_union + terrain_res_for，104 行）
  - [x] 新建 `crates/wv-app/src/terrain_view/terrain_resolve.rs`（texture_terrain + new_terrain_texture ... in setup + replace_terrain_texture + active_terrain + active_terrain_or_placeholder，149 行）
  - [x] 新建 `crates/wv-app/src/terrain_view/cells.rs`（shade_for_mode + CellRenderer + shade_* + hash01/vnoise/wobble_px + write_terrain_pixels + a_tile + image_from_terrain + patch_terrain_image，618 行）
  - [x] 新建 `crates/wv-app/src/terrain_view/setup.rs`（setup_terrain_texture + new_terrain_texture + placeholder_terrain，115 行）
  - [x] 新建 `crates/wv-app/src/terrain_view/rebuild.rs`（rebuild_texture_on_mode_change + LEVEL_FADE_SECS + FADEOUT_Z + BasemapFadeOut + advance_fade + spawn_basemap_fadeout + tick_basemap_fadeout，173 行）
  - [x] 新建 `crates/wv-app/src/terrain_view/grid_sync.rs`（sync_grid_config，93 行）
  - [x] 注：实际切片与 spec 略有不同——`new_terrain_texture` 落 setup.rs 而非 `terrain_resolve.rs`（避免 `placeholder_terrain` 在 resolve 内循环依赖）；测试块留 mod.rs 顶层以保留 `super::*` 语义

- [x] Task 3: 删除原 `terrain_view.rs` + 跑验证
  - [x] 删除 `crates/wv-app/src/terrain_view.rs`
  - [x] 跑五条验证命令（外部引用点 10 处零改动 → 编译必须通过）
  - [x] 修复编译报错：
    - `replace_terrain_texture` / `new_terrain_texture` 改为 `pub`（跨模块调用）
    - `rebuild_texture_on_mode_change` 补入 mod.rs re-export
    - `sample_tile_block` / `tile_tint_for_mode` / `TileTint` / `cell_rect` 改为 `pub`（`super::*` 测试访问）
    - `advance_fade` / `spawn_basemap_fadeout` / `FADEOUT_Z` 改为 `pub`（测试访问）
    - 补 `use bevy::prelude::*;` 测试 mod prelude
    - 补 `ViewMode` / `WorldProjection` / `colors` / `colors_google` import
  - [x] 修复 32 个测试丢失——从 git history 9c9e520 取回完整测试块追加到 mod.rs

- [x] Task 4: 文件头规范化（每个新文件 ≤30 行文件头）
  - [x] 8 个新文件均带规范文件头（最小 12 行、最大 17 行、mod.rs 16 行）
  - [x] grep 确认每个文件头 ≤30 行

- [x] Task 5: 终验
  - [x] 五条验证命令全绿
    - workspace test 329 = 329
    - workspace clippy 全绿
    - fmt --check 全绿
    - app-feature test 238 = 238
    - app-feature clippy 全绿
  - [x] `git status --short` 显示 `terrain_view.rs` 删除 + `terrain_view/` 目录新增（其他文件零改动）
  - [x] 路径 API 完全兼容：`crate::terrain_view::*` 全部命中（10 处代码 + 5 处文档）
  - [x] 抽查 3 个新文件：mod.rs / cells.rs / grid_sync.rs 文件头合规
  - [x] 更新 checklist.md 勾选状态

# Task Dependencies

Task 1 → Task 2 → Task 3 → Task 4 → Task 5

任务边界：
- 本 spec **不**触碰伞型 spec `reduce-ai-reading-cost` 的其他文件
- 本 spec **不**触碰 `lib.rs`（`pub mod terrain_view;` 解析为 `terrain_view/mod.rs`，零改动）
- 物理拆分需补 import + 清 warning，均属"修编译错误"范畴（Task 3 spec 已包含）；未引入新公开 API 之外的语义变更