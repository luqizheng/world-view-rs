# Checklist

## 死代码
- [x] `TileAtlas` 类型保留（`ui_shell.rs:43` 仍引用 `Option<Res<TileAtlas>>`）
- [x] `load_tile_atlas` / `ensure_atlas` 保留作 placeholder（伞型 spec Task 1 决定保留）
- [x] 删除后五条验证命令全绿

## 物理拆分
- [x] `crates/wv-app/src/terrain_view.rs` 已删除（`git status` 标记为 `D`）
- [x] `crates/wv-app/src/terrain_view/` 目录已创建（8 个文件 + 1 mod.rs）
- [x] `crates/wv-app/src/terrain_view/mod.rs` 含 7 个子模块声明 + 15 个 `pub use` re-export + 32 测试函数
- [x] `crates/wv-app/src/lib.rs` 零改动（`pub mod terrain_view;` 解析为 `terrain_view/mod.rs`，`git status` 验证）

## 路径 API 兼容
- [x] `crates/wv-app/src/editing.rs:25` 的 `crate::terrain_view::{image_from_terrain, TerrainTexture, TileAtlas}` 无改动
- [x] `crates/wv-app/src/editing.rs:201/221/289/298` 的 `crate::terrain_view::texture_terrain` / `patch_terrain_image` 无改动
- [x] `crates/wv-app/src/ui_shell.rs:43` 的 `crate::terrain_view::{TerrainTexture, TileAtlas}` 无改动
- [x] `crates/wv-app/src/ui_shell.rs:679/685/686` 的 `crate::terrain_view::active_terrain_or_placeholder` / `image_from_terrain` / `replace_terrain_texture` 无改动
- [x] `crates/wv-app/src/terrain_chunks.rs:58` 的 `crate::terrain_view::TerrainTexture` 无改动
- [x] `crates/wv-app/src/world_projection.rs:230` 的 `crate::terrain_view::TerrainTexture` 无改动
- [x] `crates/wv-app/src/ui/map_controls.rs:38` 的 `crate::terrain_view::TerrainTexture` 无改动
- [x] `crates/wv-app/src/ui/minimap_panel.rs:35` 的 `crate::terrain_view::TerrainTexture` 无改动
- [x] `crates/wv-app/src/app_bootstrap.rs:159` 的 `terrain_view::rebuild_texture_on_mode_change` 无改动
- [x] `crates/wv-app/src/app_bootstrap.rs:290` 的 `terrain_view::new_terrain_texture` 无改动

## 文件头
- [x] 8 个新文件均带规范文件头（一句话职责 / 职责 / 边界 / 主要导出）
- [x] 每个文件头 ≤30 行（实测：12-17 行）
- [x] 每个文件头保留 `#![cfg(feature = "app")]`

## 验证
- [x] 五条验证命令全绿
  - workspace test 329 = 329（基线一致）
  - workspace clippy 全绿
  - fmt --check 全绿
  - app-feature test 238 = 238（基线一致）
  - app-feature clippy 全绿
- [x] `git status --short` 显示 terrain_view.rs 删除 + terrain_view/ 目录新增（其他文件零改动）
- [x] 无公开 API 变更、BevyY系统签名变更、YAML 格式变更、用户可见行为变更

## 边界
- [x] 未改动其他文件（伞型 spec `reduce-ai-reading-cost` 的其他文件由其独立推进）
- [x] 未做函数级重构（仅做物理拆分 + 文件头 + 可见性调整以保持原 API）
- [x] 未提交（AGENTS §13：不主动 commit，等用户确认）

## 文件清单（拆分对照）

| 源行号 | 新文件 | 行数 |
|---|---|---|
| 1-300 | `terrain_view/tile_atlas.rs` | 281 |
| 300-389 | `terrain_view/terrain_texture.rs` | 104 |
| 389-542 | `terrain_view/terrain_resolve.rs` | 149 |
| 542-1133 | `terrain_view/cells.rs` | 618 |
| 1133-1277 | `terrain_view/setup.rs` | 115 |
| 1277-1358 | `terrain_view/rebuild.rs` | 173 |
| 1358-1433 | `terrain_view/grid_sync.rs` | 93 |
| mod.rs + tests | `terrain_view/mod.rs` | 944 |
| — 合计 | — | 2477 |

## 体积对比

| 文件 | 重构前 | 重构后 |
|---|---|---|
| `terrain_view.rs` | 96 KB / 2328 行 | 0（已删） |
| `terrain_view/mod.rs` | — | 944 行（含 32 测试 843 行） |
| `terrain_view/` 子文件合计 | — | 1533 行（7 文件，单文件最大 618 行、最小 93 行） |
| **总量** | 2328 行 | 2477 行（+149 行 = 文件头 + use + mod 声明） |

## 测试保持

- 32 个测试函数全部从 `9c9e520` 取回追加到 `mod.rs::tests`
- `use super::*` 配以 `use super::tile_atlas::*` 等显式 helper import
- `pub` 提升：`tile_res_for` / `sample_tile_block` / `blit_block_tinted` / `TileTint` / `cell_rect` / `advance_fade` / `spawn_basemap_fadeout` / `FADEOUT_Z` 由 `pub(crate)` 改 `pub`（`super::*` 测试访问需要；不影响外部用户，因为这些函数未外部引用）