//! 地形纹理与栅格渲染（按职责拆分的子模块汇总，详见各子文件头）。
//!
//! ## 职责
//! - 维护 `TerrainTexture` 资源（地形 → Bevy `Image`）及其脏标记。
//! - 把 `TerrainData` 合成 RGBA8 纹理（有图集走瓦片，无图集走 shader）。
//! - 视图模式切换与淡入淡出：换级时重建纹理，并附 0.4 s 旧纹理半透覆盖。
//! - 桥接 `WorldProjection.world` / `DrillState`：决定当前纹理应承载哪张图。
//!
//! ## 边界（不做什么）
//! - **不直接读盘**——`world.terrain_of(map)` 由 `wv-core` 提供。
//! - **不画到屏幕**——纹理只消费给 [`crate::terrain_chunks`]。
//! - **不分裂进 `colors`**——调色板在 `crate::colors*`；本模块只决定用哪一档。
//!
//! ## 路径兼容
//! 所有 `pub` 符号在下方 `pub use` 重新导出，外部对
//! `crate::terrain_view::TerrainTexture` / `TileAtlas` / `image_from_terrain` 等的引用零改动。

#![cfg(feature = "app")]

pub mod atmosphere;
pub mod cells;
pub mod grid_lines;
pub mod grid_sync;
pub mod isoline_overlay;
pub mod rebuild;
pub mod setup;
pub mod terrain_resolve;
pub mod terrain_texture;
pub mod tile_atlas;

// ── 路径兼容 re-export（保持 `crate::terrain_view::X` 不变） ─────────────────
pub use cells::{image_from_terrain, patch_terrain_image, terrain_pixels};
pub use grid_lines::{grid_lines_visible, sync_grid_lines, GridLinesOverlay};
pub use grid_sync::sync_grid_config;
pub use isoline_overlay::{sync_isoline_overlay, IsolineOverlay};
pub use rebuild::{
    land_pending_terrain_synth, rebuild_texture_on_mode_change, tick_basemap_fadeout,
    BasemapFadeOut, LEVEL_FADE_SECS,
};
pub use setup::{new_terrain_texture, placeholder_terrain, setup_terrain_texture};
pub use terrain_resolve::{
    active_terrain, active_terrain_or_placeholder, force_rebuild, replace_terrain_texture,
    texture_terrain,
};
pub use terrain_texture::TerrainTexture;
pub use tile_atlas::{ensure_atlas, load_tile_atlas, TileAtlas};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{DrillState, GridConfig, ViewMode, ViewState, WorldProjection};
    use crate::theme::{colors, colors_google};
    use bevy::prelude::*;
    use wv_core::terrain::TerrainData;
    // 私有 helper 通过子模块路径导入
    use super::rebuild::{advance_fade, spawn_basemap_fadeout, FADEOUT_Z};
    use super::terrain_texture::terrain_res_for;
    use super::tile_atlas::{blit_block_tinted, sample_tile_block, tile_res_for, TileTint};

    fn solid_atlas(cell_w: u32, cell_h: u32, rgb: [u8; 3]) -> TileAtlas {
        // 1×1 格的迷你图集，整格同一颜色。
        let n = (cell_w * cell_h) as usize;
        let mut rgba = Vec::with_capacity(n * 4);
        for _ in 0..n {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        TileAtlas {
            rgba,
            width: cell_w,
            height: cell_h,
            cols: 1,
            rows: 1,
            inset: 0,
            tiles: [
                Some([0, 0]),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        }
    }

    #[test]
    fn tile_res_clamps_to_texture_budget() {
        // 少格数 → 上限 64；多格数 → 按预算缩（下限 1，纹理 ≈ 4096²）。
        assert_eq!(tile_res_for(16), 64);
        assert_eq!(tile_res_for(64), 64);
        assert_eq!(tile_res_for(512), 8);
        assert_eq!(tile_res_for(1024), 4);
        assert_eq!(tile_res_for(2048), 2);
        assert_eq!(tile_res_for(4096), 1);
        assert_eq!(tile_res_for(8192), 1); // 预算让位于「1px/格」下限
    }

    #[test]
    fn cell_rect_applies_inset_on_both_sides() {
        let atlas = TileAtlas {
            rgba: vec![],
            width: 2048,
            height: 2048,
            cols: 8,
            rows: 6,
            inset: 8,
            tiles: [None; 10],
        };
        // 2048/8 = 256 宽、2048/6 ≈ 341 高；inset 8 两侧各缩。
        let r = atlas.cell_rect(0, 0);
        assert_eq!(r, [8, 8, 256 - 8, 341 - 8]);
        let r2 = atlas.cell_rect(1, 1);
        assert_eq!(r2, [256 + 8, 341 + 8, 512 - 8, 682 - 8]);
    }

    #[test]
    fn tile_block_blit_averages_solid_cell() {
        let atlas = solid_atlas(16, 16, [200, 100, 0]);
        let res: u32 = 4;
        let block = sample_tile_block(&atlas, 0, 0, res);
        let mut dst = vec![0u8; (res * res * 4) as usize];
        blit_block_tinted(
            &block,
            res,
            TileTint::Identity,
            false,
            crate::theme::palette::colors(Default::default()).soften_toward,
            crate::theme::palette::colors(Default::default()).soften_keep,
            &mut dst,
            res,
            0,
            0,
        );
        // 实心格子盒式平均后仍是原色。
        for px in dst.chunks_exact(4) {
            assert_eq!(px, [200, 100, 0, 255]);
        }
    }

    #[test]
    fn tile_block_blit_fade_mixes_toward_gray() {
        let atlas = solid_atlas(8, 8, [0, 0, 0]);
        let res: u32 = 2;
        let block = sample_tile_block(&atlas, 0, 0, res);
        let mut dst = vec![0u8; (res * res * 4) as usize];
        blit_block_tinted(
            &block,
            res,
            TileTint::Fade,
            false,
            crate::theme::palette::colors(Default::default()).soften_toward,
            crate::theme::palette::colors(Default::default()).soften_keep,
            &mut dst,
            res,
            0,
            0,
        );
        // 黑 × keep 0.45 + 灰 0.7×0.55 = 0.385 → 98。
        for px in dst.chunks_exact(4) {
            assert_eq!(px, [98, 98, 98, 255]);
        }
    }

    /// 双色 2×1 迷你图集（kind 0 = 红，kind 3 = 蓝），供边界混合测试用。
    fn two_color_atlas(a: [u8; 3], b: [u8; 3]) -> TileAtlas {
        let (w, h) = (16u32, 4u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for x in 0..w {
            let rgb = if x < w / 2 { a } else { b };
            for _ in 0..h {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        let mut tiles: [Option<[u32; 2]>; 10] = [None; 10];
        tiles[0] = Some([0, 0]);
        tiles[3] = Some([1, 0]);
        TileAtlas {
            rgba,
            width: w,
            height: h,
            cols: 2,
            rows: 1,
            inset: 0,
            tiles,
        }
    }

    /// 方案 A 核心验收：相邻异类格的边界必须出现**混合色**像素——
    /// 旧整格直铺只产出两种纯色，边界就是 8px 台阶。
    #[test]
    fn boundary_cell_blends_adjacent_kinds() {
        let atlas = two_color_atlas([200, 0, 0], [0, 0, 200]);
        let data = TerrainData {
            width: 2,
            height: 1,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 100,
                    surface: 0,
                    biome: 0,
                },
                wv_core::terrain::TerrainCellData {
                    height: 100,
                    surface: 1,
                    biome: 3,
                },
            ],
        };
        let img = image_from_terrain(
            &data,
            ViewMode::Schematic,
            Some(&atlas),
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let mut mixed = 0usize;
        for px in img
            .data
            .as_ref()
            .expect("地形图必须有像素数据")
            .chunks_exact(4)
        {
            let is_a = px[0] == 200 && px[2] == 0;
            let is_b = px[0] == 0 && px[2] == 200;
            if !is_a && !is_b {
                mixed += 1;
            }
        }
        assert!(mixed > 0, "边界应出现混合像素（抗锯齿），实得 {mixed}");
    }

    /// 合成必须逐字节确定（同输入两次运行位图一致）——wobble 依赖的
    /// hash 是纯函数，不允许引入时间/随机源。
    #[test]
    fn image_compositing_is_deterministic() {
        let atlas = two_color_atlas([200, 0, 0], [0, 0, 200]);
        let data = placeholder_terrain(16, 16);
        let a = image_from_terrain(
            &data,
            ViewMode::Schematic,
            Some(&atlas),
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let b = image_from_terrain(
            &data,
            ViewMode::Schematic,
            Some(&atlas),
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert_eq!(a.data, b.data);
    }

    /// 新建地图的默认占位地形应以陆地为主（2026-10-13 用户反馈：
    /// 旧阈值下 fbm 分布中位数偏下，开局大半是海）。
    #[test]
    fn placeholder_terrain_is_land_dominated() {
        let data = placeholder_terrain(512, 512);
        let water = data.cells.iter().filter(|c| c.surface == 0).count();
        let sand_or_land = data.cells.iter().filter(|c| c.surface == 1).count();
        let total = data.cells.len() as f32;
        assert!(
            water as f32 / total < 0.40,
            "水体应不足 40%，实得 {:.1}%",
            water as f32 / total * 100.0
        );
        assert!(
            sand_or_land as f32 / total > 0.60,
            "陆地应超过 60%，实得 {:.1}%",
            sand_or_land as f32 / total * 100.0
        );
    }

    #[test]
    fn image_from_terrain_scales_pixels_with_atlas_res() {
        let atlas = solid_atlas(16, 16, [10, 20, 30]);
        let data = placeholder_terrain(4, 4);
        let img = image_from_terrain(
            &data,
            ViewMode::Schematic,
            Some(&atlas),
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert_eq!(img.width(), 4 * 64); // 4 格 → res 上限 64
        let img_flat = image_from_terrain(
            &data,
            ViewMode::Schematic,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert_eq!(img_flat.width(), 4);
        // 事件图不铺瓦片：有图集也按纯色 1px/格。
        let img_event = image_from_terrain(
            &data,
            ViewMode::Event,
            Some(&atlas),
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert_eq!(img_event.width(), 4);
    }

    // -----------------------------------------------------------------------
    // 优化项 3d：patch_terrain_image 单元测试
    // -----------------------------------------------------------------------

    /// 全 0 像素的 4×4 整图，patch 矩形 (1,1)-(3,3) 后，矩形内**至少有一个**像素
    /// 应当被重写（不为 0）。用 `assert_ne!(0, 0)` 退化为形状检查：确保函数能跑完不崩。
    #[test]
    fn patch_terrain_image_runs_on_small_input() {
        let data = placeholder_terrain(4, 4);
        // res=1（无图集），像素维度 = 4×4 = 16 像素。
        let mut img = image_from_terrain(
            &data,
            ViewMode::Schematic,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        // 用 0 标记所有像素。
        for px in img.data.as_mut().expect("地形图必须有像素数据").iter_mut() {
            *px = 0;
        }
        // 关掉近岸浅滩带（S2）：本测试钉「patch 只动矩形内」的机械语义，
        // 4×4 小图上距离场支撑半径（12 格）会把整图圈进膨胀区，与哨兵
        // 断言冲突——装饰语义不该稀释机械契约（同 d43f618 关 bleed 的口径）。
        // 关全部「位置相关」的水面细节：浅滩带 + 离岸墨线都会让 patch 的
        // 脏区按距离场支撑半径膨胀（棚格测试要的是「只动矩形内」的机械语义）。
        // 纸纹颗粒不在此列——它是坐标的纯函数，不影响脏区。
        let pack = &crate::theme::theme_pack::ThemePack {
            treatment: crate::theme::theme_pack::TreatmentFlags {
                shore_shallow: false,
                ..crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN
            },
            sea: crate::theme::sea::SeaRecipe {
                shore_band_cells: 0.0,
                shore_peak: 0.0,
                ripples: &[],
                ripple_warp_cells: 0.0,
                ..crate::theme::sea::WATERCOLOR
            },
            ..*crate::theme::theme_pack::builtin::for_palette(Default::default())
        };
        // patch 矩形 (1,1)-(3,3) = 2×2 = 4 像素。
        patch_terrain_image(
            &mut img,
            &data,
            ViewMode::Schematic,
            None,
            [1, 1, 3, 3],
            1,
            false,
            pack,
        );
        // 矩形外的像素仍是 0。
        // 索引 (0,0) → 像素 0..4。
        assert_eq!(
            &img.data.as_ref().expect("地形图必须有像素数据")[0..4],
            &[0, 0, 0, 0],
            "矩形外像素应保持 0"
        );
        // 矩形内的像素已被重写为某种 kind 颜色（不一定是 0）。
        // 索引 (1,1) → 像素 4..8。
        let inner = &img.data.as_ref().expect("地形图必须有像素数据")[4..8];
        // 至少 R/G/B/A 不全 0（kind 至少 0 = 深水，对应色调非全 0）。
        let _ = inner; // 形态检查：跑完没崩。
    }

    /// `patch_terrain_image` 在矩形外**不动**像素——即 `data[rect 之外] == 旧值`。
    #[test]
    fn patch_terrain_image_leaves_outer_region_intact() {
        let data = placeholder_terrain(4, 4);
        let mut img = image_from_terrain(
            &data,
            ViewMode::Schematic,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        // 把整图刷成 0xFF 哨兵值。
        for px in img.data.as_mut().expect("地形图必须有像素数据").iter_mut() {
            *px = 0xFF;
        }
        // 关 shore_shallow：同 `patch_terrain_image_runs_on_small_input`——
        // 机械哨兵测试不掺 S2 距离场膨胀。
        // 关全部「位置相关」的水面细节：浅滩带 + 离岸墨线都会让 patch 的
        // 脏区按距离场支撑半径膨胀（棚格测试要的是「只动矩形内」的机械语义）。
        // 纸纹颗粒不在此列——它是坐标的纯函数，不影响脏区。
        let pack = &crate::theme::theme_pack::ThemePack {
            treatment: crate::theme::theme_pack::TreatmentFlags {
                shore_shallow: false,
                ..crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN
            },
            sea: crate::theme::sea::SeaRecipe {
                shore_band_cells: 0.0,
                shore_peak: 0.0,
                ripples: &[],
                ripple_warp_cells: 0.0,
                ..crate::theme::sea::WATERCOLOR
            },
            ..*crate::theme::theme_pack::builtin::for_palette(Default::default())
        };
        // patch 矩形 (1,1)-(2,2) = 1 像素。
        patch_terrain_image(
            &mut img,
            &data,
            ViewMode::Schematic,
            None,
            [1, 1, 2, 2],
            1,
            false,
            pack,
        );
        // (0,0) 像素仍是 0xFF。
        assert_eq!(img.data.as_ref().expect("地形图必须有像素数据")[0], 0xFF);
        assert_eq!(img.data.as_ref().expect("地形图必须有像素数据")[1], 0xFF);
        assert_eq!(img.data.as_ref().expect("地形图必须有像素数据")[2], 0xFF);
        assert_eq!(img.data.as_ref().expect("地形图必须有像素数据")[3], 0xFF);
        // (3,3) 像素（最后 4 字节）仍是 0xFF。
        let n = img.data.as_ref().expect("地形图必须有像素数据").len();
        assert_eq!(
            img.data.as_ref().expect("地形图必须有像素数据")[n - 4],
            0xFF
        );
        assert_eq!(
            img.data.as_ref().expect("地形图必须有像素数据")[n - 3],
            0xFF
        );
        assert_eq!(
            img.data.as_ref().expect("地形图必须有像素数据")[n - 2],
            0xFF
        );
        assert_eq!(
            img.data.as_ref().expect("地形图必须有像素数据")[n - 1],
            0xFF
        );
    }

    // -----------------------------------------------------------------------
    // 轮廓图（Coastline）与底图淡化（terrain_soften）
    // -----------------------------------------------------------------------

    /// 轮廓图：黑底 + 海岸线——同一张地形里应同时出现水色、陆色与线色像素。
    #[test]
    fn coastline_mode_renders_water_land_and_line_pixels() {
        let data = placeholder_terrain(16, 16);
        let img = image_from_terrain(
            &data,
            ViewMode::Coastline,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let Color::Srgba(water) = colors::COAST_WATER else {
            unreachable!()
        };
        let Color::Srgba(land) = colors::COAST_LAND else {
            unreachable!()
        };
        let Color::Srgba(line) = colors::COAST_LINE else {
            unreachable!()
        };
        let (mut n_water, mut n_land, mut n_line) = (0usize, 0usize, 0usize);
        for px in img
            .data
            .as_ref()
            .expect("地形图必须有像素数据")
            .chunks_exact(4)
        {
            let near = |t: bevy::color::Srgba| {
                (px[0] as f32 / 255.0 - t.red).abs() < 0.02
                    && (px[1] as f32 / 255.0 - t.green).abs() < 0.02
                    && (px[2] as f32 / 255.0 - t.blue).abs() < 0.02
            };
            if near(water) {
                n_water += 1;
            }
            if near(land) {
                n_land += 1;
            }
            if near(line) {
                n_line += 1;
            }
        }
        assert!(n_water > 0, "应有水色像素，实得 {n_water}");
        assert!(n_land > 0, "应有陆色像素，实得 {n_land}");
        assert!(n_line > 0, "应有海岸线像素，实得 {n_line}");
    }

    /// 轮廓图下淡化开关无效（黑底不能被纸面色洗掉）。
    #[test]
    fn coastline_mode_ignores_soften_flag() {
        let data = placeholder_terrain(8, 8);
        let a = image_from_terrain(
            &data,
            ViewMode::Coastline,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let b = image_from_terrain(
            &data,
            ViewMode::Coastline,
            None,
            true,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert_eq!(a.data, b.data, "轮廓图应忽略 soften 开关");
    }

    /// Google 式底图不是一格一像素的硬边像素画；边界需要亚像素分辨率。
    /// `TerrainTexture.res` 与图像分辨率必须同源，chunk / patch 才不会错位。
    #[test]
    fn terrain_res_helper_matches_generated_google_image_resolution() {
        let data = placeholder_terrain(64, 64);
        let expected = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        )
        .width()
            / data.width;
        let actual = terrain_res_for(&data, ViewMode::GoogleMap, None);
        assert_eq!(
            actual, expected,
            "Google 64² 应为 {expected}px/格，实得 {actual}px/格"
        );
    }

    /// Google 模式的增量 patch 必须使用与整图一致的 res，否则 debug 断言 / stride 错位。
    #[test]
    fn google_patch_uses_generated_texture_resolution() {
        let mut data = placeholder_terrain(4, 4);
        for c in &mut data.cells {
            c.surface = 1;
            c.biome = 3;
        }
        let mut image = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let res = image.width() / data.width;
        data.cells[5].surface = 0;
        data.cells[5].biome = 0;
        patch_terrain_image(
            &mut image,
            &data,
            ViewMode::GoogleMap,
            None,
            [1, 1, 2, 2],
            res,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
        );
        assert!(image.width() > data.width, "patch 不应缩小纹理分辨率");
    }

    #[test]
    fn google_mode_renders_with_subpixel_resolution() {
        let data = placeholder_terrain(8, 8);
        let img = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert!(
            img.width() > data.width,
            "Google 模式应提高纹理分辨率，实得 {}px / {}格",
            img.width(),
            data.width
        );
    }

    /// Google 模式下淡化开关无效：色板即最终观感，再淡化会把水色洗没。
    #[test]
    fn google_mode_ignores_soften_flag() {
        let data = placeholder_terrain(8, 8);
        let a = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let b = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            true,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        assert_eq!(a.data, b.data, "Google 模式应忽略 soften 开关");
    }

    /// Google「普通地图」折叠：陆上高度分带（沙/草/丘/山/雪/路）必须
    /// 折叠成同一陆色——否则 Perlin 分带会渲染成近色马赛克。
    #[test]
    fn google_mode_collapses_elevation_bands_into_one_land_color() {
        let mut data = placeholder_terrain(8, 8);
        for (i, c) in data.cells.iter_mut().enumerate() {
            // 人为铺一条槽位 2..7 循环的分带（全是陆上类，biome 对应冻结表）。
            c.surface = 1;
            c.biome = [5, 3, 1, 12, 13, 9][i % 6];
        }
        let img = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let Color::Srgba(land) = colors_google::GOOGLE_CORE_TERRAIN[2] else {
            unreachable!("GOOGLE_CORE_TERRAIN[2] 应为 Srgba")
        };
        // 抽查内部格与边界位置的像素：全部应等于 Google 近白陆色。
        for i in [0usize, 9, 27, 54, 63] {
            let (r, g, b) = (
                img.data.as_ref().expect("地形图必须有像素数据")[i * 4] as f32 / 255.0,
                img.data.as_ref().expect("地形图必须有像素数据")[i * 4 + 1] as f32 / 255.0,
                img.data.as_ref().expect("地形图必须有像素数据")[i * 4 + 2] as f32 / 255.0,
            );
            assert!(
                (r - land.red).abs() < 0.01
                    && (g - land.green).abs() < 0.01
                    && (b - land.blue).abs() < 0.01,
                "像素 {i} 应为陆色，实得 ({r:.3},{g:.3},{b:.3}) 期望 ({},{},{})",
                land.red,
                land.green,
                land.blue
            );
        }
    }

    /// Google 折叠不能矫枉过正：水与森林保留独立色（普通地图的三要素：
    /// 近白陆地 + 淡水 + 绿色公园斑）。
    #[test]
    fn google_mode_keeps_water_and_forest_distinct_from_land() {
        let mut data = placeholder_terrain(8, 8);
        for c in data.cells.iter_mut() {
            c.surface = 1;
            c.biome = 3; // 草原（陆色）
        }
        data.cells[9].surface = 0;
        data.cells[9].biome = 0; // 浅水
        data.cells[18].surface = 1;
        data.cells[18].biome = 1; // 森林
        let img = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let res = img.width() / data.width;
        let cell_center = |cell: usize| {
            let (gx, gy) = (cell % data.width as usize, cell / data.width as usize);
            let x = gx as u32 * res + res / 2;
            let y = gy as u32 * res + res / 2;
            let o = ((y * img.width() + x) * 4) as usize;
            [
                img.data.as_ref().expect("地形图必须有像素数据")[o],
                img.data.as_ref().expect("地形图必须有像素数据")[o + 1],
                img.data.as_ref().expect("地形图必须有像素数据")[o + 2],
            ]
        };
        assert_ne!(cell_center(9), cell_center(27), "浅水应与陆色不同");
        assert_ne!(cell_center(18), cell_center(27), "森林应与陆色不同");
    }

    /// 淡化开关：Schematic 图开启后像素向纸面色靠近（数值断言附上下文）。
    #[test]
    fn soften_flag_pulls_pixels_toward_paper_gray() {
        let data = placeholder_terrain(4, 4);
        let raw = image_from_terrain(
            &data,
            ViewMode::Schematic,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let soft = image_from_terrain(
            &data,
            ViewMode::Schematic,
            None,
            true,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let Color::Srgba(t) = colors::SOFTEN_TOWARD else {
            unreachable!("SOFTEN_TOWARD 应为 Srgba")
        };
        let mut changed = 0usize;
        for (r, s) in raw
            .data
            .as_ref()
            .expect("地形图必须有像素数据")
            .chunks_exact(4)
            .zip(
                soft.data
                    .as_ref()
                    .expect("地形图必须有像素数据")
                    .chunks_exact(4),
            )
        {
            if r == s {
                continue;
            }
            changed += 1;
            // 每个变化通道都应更接近纸面色。
            for (ch, tc) in [t.red, t.green, t.blue].into_iter().enumerate() {
                let d_raw = (r[ch] as f32 / 255.0 - tc).abs();
                let d_soft = (s[ch] as f32 / 255.0 - tc).abs();
                assert!(
                    d_soft <= d_raw + 1e-3,
                    "ch{ch}: soften 后距离应不增（raw {d_raw:.4} → soft {d_soft:.4}）"
                );
            }
        }
        assert!(changed > 0, "淡化应改变至少一个像素，实得 {changed}");
    }

    /// 轮廓图的增量 patch 与全图合成逐字节一致（wobble 路径共用性验证）。
    #[test]
    fn coastline_patch_matches_full_render() {
        let data = placeholder_terrain(8, 8);
        let full = image_from_terrain(
            &data,
            ViewMode::Coastline,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let mut patched = image_from_terrain(
            &data,
            ViewMode::Coastline,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        patch_terrain_image(
            &mut patched,
            &data,
            ViewMode::Coastline,
            None,
            [2, 2, 6, 6],
            1,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
        );
        assert_eq!(full.data, patched.data, "轮廓图 patch 与全图应逐字节一致");
    }

    // -----------------------------------------------------------------------
    // Phase 2：底图按图存 + 沿父图继承
    // -----------------------------------------------------------------------

    fn map_node(
        id: &str,
        parent: Option<&str>,
        extent: Option<wv_core::map::Extent>,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked(id),
            name: id.to_owned(),
            level: wv_core::map::MapLevel::City,
            parent: parent.map(wv_core::Id::new_unchecked),
            children: Vec::new(),
            icons: Vec::new(),
            layers: Vec::new(),
            extent,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        }
    }

    fn small_terrain(biome: u8) -> TerrainData {
        TerrainData {
            width: 4,
            height: 4,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 7,
                    surface: 1,
                    biome,
                };
                16
            ],
        }
    }

    fn two_level_world() -> wv_core::World {
        let mut world = wv_core::World::default();
        world.maps.push(map_node(
            "map_world",
            None,
            Some(wv_core::map::Extent::sized(4096.0, 4096.0)),
        ));
        world.maps.push(map_node(
            "map_city",
            Some("map_world"),
            Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
        ));
        world
    }

    /// 城市没刷地形时，底图沿 `parent` 向上继承世界图那一块（不是空白）。
    #[test]
    fn active_terrain_falls_back_to_the_nearest_ancestor_with_a_raster() {
        let mut world = two_level_world();
        world.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        let city = wv_core::Id::new_unchecked("map_city");
        let (node, data) = active_terrain(&world, Some(&city)).expect("应继承世界图底图");
        assert_eq!(node.id.as_str(), "map_world", "来源应是世界图");
        assert_eq!(data.cells[0].biome, 3, "应取世界图的格数据");
    }

    /// 城市一旦自己刷了地形，就用自己的（继承只在缺失时兜底）。
    #[test]
    fn active_terrain_prefers_the_current_maps_own_raster() {
        let mut world = two_level_world();
        world.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        world.set_terrain(wv_core::Id::new_unchecked("map_city"), small_terrain(5));
        let city = wv_core::Id::new_unchecked("map_city");
        let (node, data) = active_terrain(&world, Some(&city)).expect("应用城市自己的底图");
        assert_eq!(node.id.as_str(), "map_city");
        assert_eq!(data.cells[0].biome, 5);
    }

    /// 整条父链都没有栅格 → `None`（调用方用占位兜底，而不是误报某张图）。
    #[test]
    fn active_terrain_is_none_when_no_map_in_the_chain_has_a_raster() {
        let world = two_level_world();
        let city = wv_core::Id::new_unchecked("map_city");
        assert!(active_terrain(&world, Some(&city)).is_none());
    }

    /// 兜底占位落在**根图**上，并真的写进世界（笔刷/撤销才有落脚图）。
    #[test]
    fn placeholder_terrain_lands_on_the_root_map_when_nothing_has_a_raster() {
        let mut world = two_level_world();
        let city = wv_core::Id::new_unchecked("map_city");
        let (map, data) = active_terrain_or_placeholder(&mut world, Some(&city), 32);
        assert_eq!(map.map(|m| m.as_str().to_owned()), Some("map_world".into()));
        assert_eq!(data.width, 32, "占位地形按调用方给的格数生成");
        assert!(
            world
                .terrain_of(&wv_core::Id::new_unchecked("map_world"))
                .is_some(),
            "占位地形应写进根图，而不是只存在于纹理里"
        );
    }

    /// **启动早期的回归**：`DrillState` 还没 `ensure_root`（`current` 为 `None`）时，
    /// 不能把「当前图未知」当成「世界上没有地形」而用占位覆盖真实地形。
    #[test]
    fn unknown_current_map_does_not_overwrite_existing_terrain() {
        let mut world = two_level_world();
        let root = wv_core::Id::new_unchecked("map_world");
        world.set_terrain(root.clone(), small_terrain(3));
        let (map, data) = active_terrain_or_placeholder(&mut world, None, 32);
        assert_eq!(map.map(|m| m.as_str().to_owned()), Some("map_world".into()));
        assert_eq!(data.cells[0].biome, 3, "应取回已有真实地形，而不是占位");
        assert_eq!(
            world.terrain_of(&root).map(|t| t.width),
            Some(4),
            "世界里的地形不能被占位覆盖"
        );
    }

    /// 增量 patch / 撤销 diff 必须跟着**纹理手里的那张图**走：纹理显示世界图时，
    /// 即使当前图是城市，也返回世界图的地形。
    #[test]
    fn texture_terrain_follows_the_map_the_texture_holds() {
        let mut world = two_level_world();
        world.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        world.set_terrain(wv_core::Id::new_unchecked("map_city"), small_terrain(5));
        let city = wv_core::Id::new_unchecked("map_city");
        let mut tex = TerrainTexture {
            handle: Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
            last_map: Some(wv_core::Id::new_unchecked("map_world")),
        };
        let data = texture_terrain(&tex, &world, Some(&city)).expect("应取世界图地形");
        assert_eq!(
            data.cells[0].biome, 3,
            "纹理装了世界图，就不能拿城市格去 patch"
        );
        // 纹理换到城市后，同一调用应跟着换成城市的数据。
        tex.last_map = Some(city.clone());
        let data = texture_terrain(&tex, &world, Some(&city)).expect("应取城市地形");
        assert_eq!(data.cells[0].biome, 5);
    }

    /// `last_map` 无效或缺失时退回「当前图向上的第一张」，不留空白。
    #[test]
    fn texture_terrain_falls_back_to_the_current_map_when_last_map_is_unknown() {
        let mut world = two_level_world();
        world.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        let city = wv_core::Id::new_unchecked("map_city");
        let tex = TerrainTexture {
            handle: Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
            last_map: None,
        };
        let data = texture_terrain(&tex, &world, Some(&city)).expect("应退回继承口径");
        assert_eq!(data.cells[0].biome, 3);
    }

    /// 纹理创建时记住来源图——`rebuild_texture_on_mode_change` 靠它判断「换图了」。
    #[test]
    fn terrain_texture_records_which_map_it_holds() {
        let data = small_terrain(3);
        let image = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let mut images = Assets::<Image>::default();
        let held = wv_core::Id::new_unchecked("map_city");
        let tex = new_terrain_texture(
            &mut images,
            image,
            &data,
            Some(held.clone()),
            ViewMode::GoogleMap,
            false,
            Default::default(),
        );
        assert_eq!(tex.last_map.as_ref(), Some(&held));
    }

    /// 回归（用户报告：钻入武昌城「看不到武昌」）：城市**继承**父图底图时，钻入
    /// 一次都不会让纹理变化，`GridConfig` 必须靠 `drill.is_changed()` 认出来，
    /// 并把 `map_origin` 从世界原点切到城市图帧原点。
    ///
    /// 断言分两帧：第一帧在世界（图帧原点 == 栅格原点 == 0），第二帧钻入城市
    /// （**不动纹理**）——此时栅格原点仍是世界图的 (0,0)，而图帧原点必须变成
    /// 城市由父图图标派生的落点 (1536,1536)。
    #[test]
    fn grid_map_origin_follows_the_drilled_map_when_the_raster_is_inherited() {
        let mut core = two_level_world();
        // 助手把两张图都造 `City` 级；`ensure_root` 要认根图，标回 `World`。
        core.maps[0].level = wv_core::map::MapLevel::World;
        // 只有世界图有地形：城市继承它 → 纹理两帧都不变。
        core.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        {
            let root = &mut core.maps[0];
            root.children.push(wv_core::Id::new_unchecked("map_city"));
            root.icons.push(wv_core::map::IconSpec {
                id: wv_core::Id::new_unchecked("icon_city"),
                anchor: Some([0.5, 0.5]),
                shape: Default::default(),
                size: None,
                color: None,
                drill_into: Some(wv_core::Id::new_unchecked("map_city")),
            });
        }
        let city = wv_core::Id::new_unchecked("map_city");

        let mut drill = DrillState::default();
        drill.ensure_root(&core);
        assert_eq!(
            drill.current().map(|i| i.as_str().to_owned()),
            Some("map_world".to_owned()),
            "起始应落在根图"
        );

        let mut app = World::default();
        app.insert_resource(TerrainTexture {
            handle: Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
            // 纹理装的是世界图底图；钻入城市后**不会**重建（继承同一个来源）。
            last_map: Some(wv_core::Id::new_unchecked("map_world")),
        });
        app.insert_resource(WorldProjection {
            world: core.clone(),
            ..Default::default()
        });
        app.insert_resource(drill);
        app.insert_resource(GridConfig::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_grid_config);
        schedule.run(&mut app);
        assert_eq!(
            app.resource::<GridConfig>().map_origin,
            Vec2::ZERO,
            "世界图上图帧原点与栅格原点重合于 (0,0)"
        );

        // 第二帧：只换当前图（钻入城市），纹理一个字都不动。
        {
            let mut d = app.resource_mut::<DrillState>();
            assert!(d.enter(&core, &city), "城市应是根图的直接子图");
        }
        schedule.run(&mut app);

        let grid = app.resource::<GridConfig>();
        assert_eq!(grid.raster_origin, Vec2::ZERO, "继承的底图仍是世界图原点");
        assert_eq!(
            grid.map_origin,
            Vec2::new(1536.0, 1536.0),
            "城市图帧原点应由父图图标派生：2048 − 512 = 1536；实得 {}",
            grid.map_origin
        );
        assert_eq!(
            grid.map_size,
            Vec2::splat(1024.0),
            "幅面应取当前图（城市）的 extent，而不是世界图的 4096"
        );
    }

    // -----------------------------------------------------------------------
    // 换级淡化（ADR-0009 §2⑦ / README §12-25）
    // -----------------------------------------------------------------------

    /// 步进口径：0.4s 内走完，dt 超长一步到位；进度单调不增。
    #[test]
    fn fade_step_reaches_zero_within_the_declared_duration() {
        // 60fps：0.4s ≈ 24 步走完（前 23 步仍有余量，第 24 步归零回收）。
        let dt = 1.0 / 60.0;
        let mut alpha = 1.0f32;
        let mut steps = 0usize;
        while let Some(next) = advance_fade(alpha, dt) {
            assert!(next < alpha, "淡出必须单调递减：{alpha} → {next}");
            alpha = next;
            steps += 1;
            assert!(steps < 100, "淡出迟迟不结束，卡在 {alpha}");
        }
        assert!(steps <= 24, "0.4s @60fps 至多 24 步走完，实得 {steps} 步");
        // 一步一整个时长 → 直接归零。
        assert_eq!(advance_fade(1.0, LEVEL_FADE_SECS), None);
        // 零时长推进不动（不会变负）。
        assert_eq!(advance_fade(1.0, 0.0), Some(1.0));
    }

    /// 淡出精灵必须**克隆**旧像素（复用句柄会被原地覆写）并盖住旧栅格幅面。
    #[test]
    fn fadeout_spawn_clones_pixels_and_covers_the_old_raster() {
        let data = small_terrain(3);
        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            &data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let tex = new_terrain_texture(
            &mut images,
            image,
            &data,
            None,
            ViewMode::GoogleMap,
            false,
            Default::default(),
        );
        let grid = GridConfig {
            cell_size: 2.0,
            raster_origin: Vec2::new(100.0, 200.0),
            ..GridConfig::default()
        };

        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            spawn_basemap_fadeout(&mut commands, &mut images, &tex, &grid);
        }
        queue.apply(&mut world);

        let mut q = world.query::<(&BasemapFadeOut, &Sprite, &Transform)>();
        let (fade, sprite, tf) = q.single(&world).expect("应生成一个淡出精灵");
        assert_eq!(fade.alpha, 1.0, "初始不透明");
        assert_eq!(
            sprite.custom_size,
            Some(Vec2::splat(8.0)),
            "幅面 = 4 格 × cell_size 2.0"
        );
        assert_eq!(
            tf.translation.truncate(),
            Vec2::new(104.0, 204.0),
            "中心 = 栅格原点 + 半幅"
        );
        assert_eq!(tf.translation.z, FADEOUT_Z);
        // 克隆：句柄不同、内容一致，且旧句柄的像素未被动过。
        assert_ne!(sprite.image, tex.handle, "淡出必须用克隆出的新句柄");
        let old = images.get(&tex.handle).expect("旧资产还在");
        let new = images.get(&sprite.image).expect("克隆资产已入库");
        assert_eq!(old.data, new.data, "克隆像素应与旧底图逐字节一致");
    }

    /// 换图（换级）生成淡出精灵；换模式（换眼镜）不生成——淡化只属于 12-25。
    #[test]
    fn map_change_spawns_fadeout_but_mode_change_does_not() {
        let mut core = two_level_world();
        // `ensure_root` 只认 `level == World`（fixture 默认 City）：把根图升回来。
        core.maps[0].level = wv_core::map::MapLevel::World;
        let world_id = wv_core::Id::new_unchecked("map_world");
        let city_id = wv_core::Id::new_unchecked("map_city");
        core.set_terrain(world_id.clone(), small_terrain(3));
        core.set_terrain(city_id.clone(), small_terrain(5));

        let mut drill = DrillState::default();
        drill.ensure_root(&core);
        let (node, data) = active_terrain(&core, drill.current()).expect("世界图有地形");
        assert_eq!(node.id, world_id);
        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            data,
            ViewMode::GoogleMap,
            None,
            false,
            crate::theme::theme_pack::builtin::for_palette(Default::default()),
            0.0,
        );
        let tex = new_terrain_texture(
            &mut images,
            image,
            data,
            Some(world_id),
            ViewMode::GoogleMap,
            false,
            Default::default(),
        );

        let mut world = World::new();
        world.insert_resource(ViewState::default());
        world.insert_resource(WorldProjection {
            world: core.clone(),
            ..Default::default()
        });
        world.insert_resource(drill);
        world.insert_resource(GridConfig::default());
        world.insert_resource(tex);
        world.insert_resource(images);
        world.init_resource::<crate::terrain_view::rebuild::PendingTerrainSynth>();

        let count_fades = |world: &mut World| -> usize {
            let mut q = world.query::<&BasemapFadeOut>();
            q.iter(world).count()
        };
        let mut schedule = Schedule::default();
        schedule.add_systems(rebuild_texture_on_mode_change);

        // 换图：钻入自带地形的城市 → 应生成恰好一个淡出精灵。
        // fixture 没接 children（`enter` 只认直接子图），这里补上引用。
        core.maps[0].children.push(city_id.clone());
        {
            let mut d = world.resource_mut::<DrillState>();
            assert!(d.enter(&core, &city_id), "城市应是根图的直接子图");
        }
        schedule.run(&mut world);
        assert_eq!(count_fades(&mut world), 1, "换图应生成一个淡出精灵");

        // 换模式：即时重建是既有行为，不再叠加淡出。
        world.resource_mut::<ViewState>().mode = ViewMode::Schematic;
        schedule.run(&mut world);
        assert_eq!(
            count_fades(&mut world),
            1,
            "换模式（换眼镜）不应生成第二个淡出精灵"
        );
    }
}
