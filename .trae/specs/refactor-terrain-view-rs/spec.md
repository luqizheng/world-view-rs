# `terrain_view.rs` 物理拆分 Spec

## Why

`crates/wv-app/src/terrain_view.rs` 现 96KB / 2328 行，是 `wv-app` **最大**模块（拆分前 ui.rs 第一，已拆分完成；现在 terrain_view.rs 是第一）。

伞型 spec `reduce-ai-reading-cost` Task 3 原本给"温和路线"（文件头 + 段注释），用户 2026-10-14 在 ui.rs 拆分时已确认偏好"同款物理拆分"——umbrella 的温和路线被推翻，改走物理拆分。

外部对 `crate::terrain_view::*` 的引用面（grep 全部命中）：

| 调用方 | 路径 |
|---|---|
| `editing.rs:25` | `image_from_terrain` / `TerrainTexture` / `TileAtlas`（多方法：`texture_terrain` / `patch_terrain_image`） |
| `ui_shell.rs:43` | `TerrainTexture` / `TileAtlas` |
| `ui_shell.rs:679/685/686` | `active_terrain_or_placeholder` / `image_from_terrain` / `replace_terrain_texture` |
| `ui_shell.rs:865` | `TerrainTexture` |
| `terrain_chunks.rs:58` | `TerrainTexture` |
| `world_projection.rs:230` | `TerrainTexture` |
| `ui/map_controls.rs:38` | `TerrainTexture` |
| `ui/minimap_panel.rs:35` | `TerrainTexture` |
| 文档引用（不需改） | `drill.rs:257` / `editing.rs:140,184` / `faction_overlays.rs:6` / `map_frame.rs:81` / `terrain_chunks.rs:215` |

共 **10 处代码引用点 + 5 处文档引用**。路径完全兼容：拆 `terrain_view/` 子目录 + `mod.rs` 重导出，`pub mod terrain_view;` 仍解析为 `terrain_view/mod.rs`，`lib.rs` 零改动。

## What Changes

把 `terrain_view.rs` 物理拆分为 `crates/wv-app/src/terrain_view/` 子目录 + 一个新 `terrain_view/mod.rs`（文件头 + 子模块声明 + re-export）。新结构：

```
crates/wv-app/src/
├── terrain_view.rs                       # 删除
└── terrain_view/
    ├── mod.rs                            # 新：文件头 + mod 声明 + pub use re-export
    ├── tile_atlas.rs                     # TileAtlas + load_tile_atlas + ensure_atlas（已弃用路径占位）
    ├── terrain_texture.rs                # TerrainTexture 资源 + image_from_terrain + patch_terrain_image + placeholder_terrain
    ├── terrain_resolve.rs                # texture_terrain + active_terrain + active_terrain_or_placeholder
    ├── setup.rs                          # setup_terrain_texture + sync_grid_config
    ├── rebuild.rs                        # rebuild_texture_on_mode_change + LEVEL_FADE_SECS + BasemapFadeOut + tick_basemap_fadeout
    ├── replace.rs                        # replace_terrain_texture
    └── cells.rs                          # CellRenderer 像素合成纯函数（地形 → 像素）
```

### 模块切分依据

| 文件 | 公开符号（来源行号） |
|---|---|
| `tile_atlas.rs` | `TileAtlas` (L51) / `load_tile_atlas` (L92) / `ensure_atlas` (L149) |
| `terrain_texture.rs` | `TerrainTexture` (L307) / `image_from_terrain` (L1054) / `patch_terrain_image` (L1096) / `placeholder_terrain` (L1162) |
| `terrain_resolve.rs` | `texture_terrain` (L403) / `active_terrain` (L490) / `active_terrain_or_placeholder` (L518) |
| `setup.rs` | `setup_terrain_texture` (L1141) / `sync_grid_config` (L1381) |
| `rebuild.rs` | `rebuild_texture_on_mode_change` (L1229) / `LEVEL_FADE_SECS` (L1286) / `BasemapFadeOut` (L1295) / `tick_basemap_fadeout` (L1339) |
| `replace.rs` | `replace_terrain_texture`（未列在 16 个 pub 内——可能由 terrain_texture 收编） |
| `cells.rs` | CellRenderer 系列纯函数（地形 → 像素） |

### 路径兼容（关键约束）

`mod.rs` 必须 `pub use` 全部 16 个公开符号，确保以下 10 处代码引用零改动：

- `crate::terrain_view::TerrainTexture` (6 处：editing / ui_shell / terrain_chunks / world_projection / ui/map_controls / ui/minimap_panel)
- `crate::terrain_view::TileAtlas` (2 处：editing / ui_shell)
- `crate::terrain_view::image_from_terrain` (editing / ui_shell)
- `crate::terrain_view::texture_terrain` (editing)
- `crate::terrain_view::patch_terrain_image` (editing)
- `crate::terrain_view::active_terrain_or_placeholder` (ui_shell)
- `crate::terrain_view::replace_terrain_texture` (ui_shell)

### 顺手清理（属 terrain_view.rs 内务）

- 段注释序号对齐（如 `// ── 段 N · name ──`）
- 死代码识别（伞型 spec Task 1 已列：`load_tile_atlas` 注释"已弃用，ensure_atlas 恒 None"；`TileAtlas` 保留作 placeholder 占位）—— **保留** `TileAtlas` 类型（`ui_shell.rs:43` 仍引用）

### 非目标（明确排除）

- **不**改伞型 spec `reduce-ai-reading-cost` 的其他文件（`ui_shell.rs` / `state.rs` 等由其独立推进）。
- **不**删任何代码 / 资源 / 公开 API。
- **不**做函数级重构。
- **不**改任何公开 API、Bevy 系统签名、YAML 格式、用户可见行为。

## Impact

- **Affected code**：
  - 删 `crates/wv-app/src/terrain_view.rs`（单文件）
  - 新增 `crates/wv-app/src/terrain_view/` 目录（约 6-8 个文件 + 1 个 mod.rs）
  - **`crates/wv-app/src/lib.rs` 零改动**（`pub mod terrain_view;` 解析为 `terrain_view/mod.rs`，路径完全等价）
  - 外部引用点（10 处代码 + 5 处文档）零改动
- **Affected specs**：与伞型 spec `reduce-ai-reading-cost` 边界明确——本 spec **实体化其 Task 3 的物理拆分**（取代其温和路线），伞型 spec Task 1 的死代码清单本 spec 不重新碰。
- **测试影响**：本次**纯物理拆分 + 文件头规范化**，零代码变更。`cargo test -p wv-app --features app` 通过数**不变**。

### Requirement: 物理拆分到 `terrain_view/` 子目录

`crates/wv-app/src/terrain_view.rs` SHALL 被删除；其内容 SHALL 拆入 `crates/wv-app/src/terrain_view/` 子目录；`crates/wv-app/src/lib.rs` 的 `pub mod terrain_view;` 声明零改动。

#### Scenario: 单文件搜索落到正确子模块
- **WHEN** AI 搜 `image_from_terrain` 实现
- **THEN** `grep "fn image_from_terrain"` 在 `crates/wv-app/src/terrain_view/terrain_texture.rs` 一行命中（不再在 96KB 文件中跳转）

### Requirement: 路径 API 完全兼容

`crates/wv-app/src/terrain_view/mod.rs` SHALL 用 `pub use` re-export 全部 16 个公开符号；外部对 `crate::terrain_view::TerrainTexture` / `TileAtlas` / `image_from_terrain` / `texture_terrain` / `patch_terrain_image` / `active_terrain_or_placeholder` / `replace_terrain_texture` 的 10 处代码引用 SHALL 零改动编译通过。

#### Scenario: 引用方零改动
- **WHEN** 物理拆分完成后跑 `cargo build -p wv-app --features app`
- **THEN** `editing.rs` / `ui_shell.rs` / `terrain_chunks.rs` / `world_projection.rs` / `ui/map_controls.rs` / `ui/minimap_panel.rs` 不需要任何路径修改

### Requirement: 文件头规范

每个新文件 SHALL 以 ≤30 行的规范 `//!` 文件头开头（一句话职责 + 职责清单 + 边界 + 主要导出），与 ui/ 子目录模板一致。

### Requirement: 死代码保留

`TileAtlas` 类型 SHALL 保留（`ui_shell.rs:43` 仍以 `Option<Res<TileAtlas>>` 引用）。`load_tile_atlas` / `ensure_atlas` 已弃用但**保留作为 placeholder**（伞型 spec Task 1 决定保留，本 spec 不动）。

## MODIFIED Requirements

无。

## REMOVED Requirements

无。