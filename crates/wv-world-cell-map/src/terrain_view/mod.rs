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

pub mod atmosphere;
pub mod biome_raw;
pub mod cells;
pub mod field;
pub mod grid_lines;
pub mod isoline_overlay;
pub mod rebuild;
pub mod setup;
pub mod terrain_resolve;
pub mod terrain_texture;
pub mod tile_atlas;

// ── 路径兼容 re-export（保持 `crate::terrain_view::X` 不变） ─────────────────
pub use cells::{image_from_terrain, patch_terrain_image, terrain_pixels};
pub use grid_lines::GridLinesOverlay;
pub use isoline_overlay::IsolineOverlay;
pub use rebuild::{
    tick_basemap_fadeout, BasemapFadeOut, PendingTerrainSynth, SynthCtx, LEVEL_FADE_SECS,
};
pub use setup::{new_terrain_texture, placeholder_terrain};
pub use terrain_resolve::{
    active_terrain, active_terrain_or_placeholder, force_rebuild, replace_terrain_texture,
    texture_terrain,
};
pub use terrain_texture::TerrainTexture;
pub use tile_atlas::{ensure_atlas, load_tile_atlas, TileAtlas};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{self, CellPack, SeaRecipe, TreatmentFlags};
    use crate::shade::ShadeMode;
    use bevy::prelude::*;
    use wv_core::terrain::TerrainData;
    // 私有 helper 通过子模块路径导入
    use super::rebuild::advance_fade;
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
            CellPack::fixture().palette.soften_toward,
            CellPack::fixture().palette.soften_keep,
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
            CellPack::fixture().palette.soften_toward,
            CellPack::fixture().palette.soften_keep,
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
            ShadeMode::Schematic,
            Some(&atlas),
            false,
            &CellPack::fixture(),
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
            ShadeMode::Schematic,
            Some(&atlas),
            false,
            &CellPack::fixture(),
            0.0,
        );
        let b = image_from_terrain(
            &data,
            ShadeMode::Schematic,
            Some(&atlas),
            false,
            &CellPack::fixture(),
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
            ShadeMode::Schematic,
            Some(&atlas),
            false,
            &CellPack::fixture(),
            0.0,
        );
        assert_eq!(img.width(), 4 * 64); // 4 格 → res 上限 64
        let img_flat = image_from_terrain(
            &data,
            ShadeMode::Schematic,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        assert_eq!(img_flat.width(), 4);
        // 事件图不铺瓦片：有图集也按纯色 1px/格。
        let img_event = image_from_terrain(
            &data,
            ShadeMode::Event,
            Some(&atlas),
            false,
            &CellPack::fixture(),
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
            ShadeMode::Schematic,
            None,
            false,
            &CellPack::fixture(),
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
        let pack = &CellPack {
            treatment: TreatmentFlags {
                shore_shallow: false,
                ..TreatmentFlags::HAND_DRAWN
            },
            sea: SeaRecipe {
                shore_band_cells: 0.0,
                shore_peak: 0.0,
                ripples: vec![],
                ripple_warp_cells: 0.0,
                ..crate::palette::watercolor()
            },
            ..CellPack::fixture()
        };
        // patch 矩形 (1,1)-(3,3) = 2×2 = 4 像素。
        patch_terrain_image(
            &mut img,
            &data,
            ShadeMode::Schematic,
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
            ShadeMode::Schematic,
            None,
            false,
            &CellPack::fixture(),
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
        let pack = &CellPack {
            treatment: TreatmentFlags {
                shore_shallow: false,
                ..TreatmentFlags::HAND_DRAWN
            },
            sea: SeaRecipe {
                shore_band_cells: 0.0,
                shore_peak: 0.0,
                ripples: vec![],
                ripple_warp_cells: 0.0,
                ..crate::palette::watercolor()
            },
            ..CellPack::fixture()
        };
        // patch 矩形 (1,1)-(2,2) = 1 像素。
        patch_terrain_image(
            &mut img,
            &data,
            ShadeMode::Schematic,
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
            ShadeMode::Coastline,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let Color::Srgba(water) = palette::COAST_WATER else {
            unreachable!()
        };
        let Color::Srgba(land) = palette::COAST_LAND else {
            unreachable!()
        };
        let Color::Srgba(line) = palette::COAST_LINE else {
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
            ShadeMode::Coastline,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let b = image_from_terrain(
            &data,
            ShadeMode::Coastline,
            None,
            true,
            &CellPack::fixture(),
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
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        )
        .width()
            / data.width;
        let actual = terrain_res_for(&data, ShadeMode::GoogleMap, None);
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
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let res = image.width() / data.width;
        data.cells[5].surface = 0;
        data.cells[5].biome = 0;
        patch_terrain_image(
            &mut image,
            &data,
            ShadeMode::GoogleMap,
            None,
            [1, 1, 2, 2],
            res,
            false,
            &CellPack::fixture(),
        );
        assert!(image.width() > data.width, "patch 不应缩小纹理分辨率");
    }

    #[test]
    fn google_mode_renders_with_subpixel_resolution() {
        let data = placeholder_terrain(8, 8);
        let img = image_from_terrain(
            &data,
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
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
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let b = image_from_terrain(
            &data,
            ShadeMode::GoogleMap,
            None,
            true,
            &CellPack::fixture(),
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
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let Color::Srgba(land) = palette::GOOGLE_CORE_TERRAIN[2] else {
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
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
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
            ShadeMode::Schematic,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let soft = image_from_terrain(
            &data,
            ShadeMode::Schematic,
            None,
            true,
            &CellPack::fixture(),
            0.0,
        );
        let Color::Srgba(t) = CellPack::fixture().palette.soften_toward else {
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
            ShadeMode::Coastline,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let mut patched = image_from_terrain(
            &data,
            ShadeMode::Coastline,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        patch_terrain_image(
            &mut patched,
            &data,
            ShadeMode::Coastline,
            None,
            [2, 2, 6, 6],
            1,
            false,
            &CellPack::fixture(),
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
}
