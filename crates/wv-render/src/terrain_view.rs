#![cfg(feature = "app")]

//! CellMap 渲染域的路径兼容 shim（ADR-0054 D5）。
//!
//! 实体在 [`wv_world_cell_map`]（整建制迁出，2026-10-23）；本模块把旧路径
//! `crate::terrain_view::X` / `crate::terrain_view::terrain_texture::TerrainTexture`
//! 原样接回——wv-render 内部消费点与 wv-app 全部零改动。
//! 读取 `ViewState` 等呈现态的桥接系统在 [`crate::cellmap_bridge`]，
//! 经下方 re-export 保持 `crate::terrain_view::sync_*` 注册路径不变。

pub use wv_world_cell_map::terrain_view::*;

pub use wv_world_cell_map::terrain_view::{
    atmosphere, biome_raw, cells, field, grid_lines, isoline_overlay, rebuild, setup,
    terrain_resolve, terrain_texture, tile_atlas,
};

// ── 桥接系统（读 wv-render 呈现态；ADR-0054 D5） ────────────────────────
pub use crate::cellmap_bridge::{
    conv_pack, conv_palette, conv_sea, conv_treatment, grid_lines_visible,
    land_pending_terrain_synth, rebuild_texture_on_mode_change, setup_terrain_texture,
    shade_mode_of, sync_atmosphere, sync_cell_region, sync_grid_config, sync_grid_lines,
    sync_isoline_overlay, sync_missing_overlay_markers, sync_region_overlay_index,
    sync_terrain_chunks, sync_terrain_field, FieldKey, FieldRenderStats, TerrainFieldRender,
};
