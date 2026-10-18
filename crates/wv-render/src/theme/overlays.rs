//! 主题覆盖物（theme overlay）的**纯数据核**：每主题的 feature 规则、镜头分带
//! 交叉淡化、近带确定性采样。
//!
//! ## 职责
//! - [`FeatureRule`] / [`FeatureOverlaySet`] —— 一个主题包对某类地理事实
//!   （v1 只有森林）的演绎参数：远带晕 + 近带符号；
//! - [`overlay_far_blend`] —— 同一张图内按缩放（世界单位 : 像素）求远 / 近带
//!   交叉淡化系数的纯函数；
//! - [`forest_sites`] —— 地界 ∩ 栅格网格的确定性采样（树冠落位）。
//!
//! ## 边界（不做什么）
//! - **不画任何东西**：本模块零 Bevy 类型、零 ECS 访问；渲染系统
//!   （`crate::feature_overlays`，Task 6）消费本模块的输出；
//! - **不做时间比较**：地界由 `wv_core::place_territory_at` 求值后传入
//!   （AGENTS §3）；本模块只看到「此刻的环」；
//! - **不引入 `rand`**：抖动是手写 splitmix64 哈希，跨进程 / 跨平台稳定
//!   （重开工程树冠不跳变）。

#![cfg(feature = "app")]

use wv_core::geometry::point_in_ring;
use wv_core::terrain::{TerrainCellData, TerrainData};

/// 森林的地理事实编号（`Place.feature` 的值）。
///
/// 与 `theme::colors::CORE_TERRAIN` 的索引 4（森林）同一张词汇表；默认主题的
/// 网格 kind 编号同源（`wv-terrain::intent` 文档：草原 3 / 丘陵 5 / 山地 6）。
/// `Place.feature` 在 `wv-core` 里是裸 `u8`（数据层不依赖渲染层），编号到
/// 演绎规则的映射由本模块的 [`FeatureOverlaySet::rule_for`] 承担。
pub const FEATURE_FOREST: u8 = 4;

/// 抖动幅度占采样间距的比例（`±0.35 × spacing_wu`）。
///
/// 上界必须 < 0.5：候选点按 `spacing_wu` 均布，抖动不到半格才能保证每个候选
/// 仍落在自己的采样格附近，不互相穿插、密度均匀。
const JITTER_FRACTION: f32 = 0.35;

/// 单个 feature 的演绎参数（远带晕 + 近带符号）。
///
/// **归属**：一个主题包对一类地理事实的「长相」；作者只声明事实（`Place.feature`），
/// 不直接写这些值（D1）。v1 为代码常量，YAML 化随 ADR-0023 M3 推迟。
///
/// **贴图字段**（D12 资产契约）：写主题资产根下的相对路径，由渲染侧映射到
/// 编译期嵌入的 PNG（`feature_overlays::embedded_texture`）。`None` = 纯色路径；
/// 键未知 / 解码失败同样回退纯色——缺资产不崩、不空帧。水墨包暂留 `None`，
/// 待美术校准后接线。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureRule {
    /// 远带地界晕的 RGB（sRGB 字节）。渲染色 = 本色 × alpha，不随 palette 走。
    pub halo_color: [u8; 3],
    /// 远带晕浓度（0..=1）；近带交叉淡化时由渲染系统乘 `0.4 + 0.6×blend`。
    pub halo_alpha: f32,
    /// 近带树冠符号的 RGB（sRGB 字节）。
    pub symbol_color: [u8; 3],
    /// 近带符号的屏幕尺寸（像素）；渲染时乘 `ViewState.scale` 换世界尺寸。
    pub symbol_size_px: f32,
    /// 近带采样间距（世界单位）——同时是抖动幅度的基准（见 [`JITTER_FRACTION`]）。
    pub symbol_spacing_wu: f32,
    /// 近带符号贴图（相对主题资产根的路径；3 格横向图集）。`None` = 纯色 quad。
    pub symbol_texture: Option<&'static str>,
    /// 远带晕贴图（按地界包围盒拉伸的单张图章）。`None` = 纯色多边形。
    pub halo_texture: Option<&'static str>,
}

/// 一套主题的 feature 词汇 → 演绎规则映射。
///
/// **缺省回退是硬要求**（设计稿 §6.4）：主题没给某 feature 规则时
/// [`Self::rule_for`] 返回 `None`，渲染侧仍画地界描边 + 名字——事实不因换主题
/// 而消失。v1 只登记森林一个条目。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureOverlaySet {
    /// 森林（[`FEATURE_FOREST`]）的规则；`None` = 本主题不演绎森林。
    pub forest: Option<FeatureRule>,
}

impl FeatureOverlaySet {
    /// 按 `Place.feature` 的词汇编号取规则。
    ///
    /// v1 仅识别 [`FEATURE_FOREST`]；其余编号（含未来山脉等）返回 `None`，
    /// 走「只画地界描边 + 名字」的回退，不静默消失。
    pub fn rule_for(&self, feature: u8) -> Option<&FeatureRule> {
        match feature {
            FEATURE_FOREST => self.forest.as_ref(),
            _ => None,
        }
    }
}

/// 远带交叉淡化系数：`scale` 越大（视野越远）返回值越接近 1。
///
/// `scale` 的口径与 `ViewState.scale` 一致（世界单位 / 像素，与等值线
/// `lod_bucket` 同源）。分带锚点：
///
/// | `scale` | 返回值 | 含义 |
/// |---|---|---|
/// | ≤ 0.75 | 0.0 | 近带（符号满强度，晕 40%） |
/// | 1.00 | 0.5 | 两带等权交叉 |
/// | ≥ 1.25 | 1.0 | 远带（晕满强度，符号退场） |
///
/// 曲线用 smoothstep（`t²(3−2t)`，中点 1.0），与 `lod_fade` 同族——两端斜率
/// 为零，缩放跨过阈值时不会出现可见的突变台阶。
pub fn overlay_far_blend(scale: f32) -> f32 {
    let t = ((scale - 0.75) / 0.5).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 地界内近带树冠落位（局部坐标，确定性）。
///
/// ## 口径
/// 1. 在 `ring_local` 的包围盒内按 `spacing_wu` 布候选网格（每格中心一个候选，
///    候选数 = `ceil(宽 / spacing) × ceil(高 / spacing)`）；
/// 2. 每个候选做 `±0.35 × spacing_wu` 的确定性抖动——哈希输入
///    `seed ^ (gx << 32) ^ gy`，经 splitmix64 展开；**不用 `rand`**，跨进程 /
///    跨平台同 seed 同布局（重开工程树冠不跳变）；
/// 3. 只保留 `point_in_ring` 内的候选；`holes`（带洞地界，ADR-0029 M2）内的
///    候选剔除——湖面 / 草地芯不落树；
/// 4. 局部点 + `raster_offset` 映射到地形格（`(p + offset) / cell_size`，向下取整），
///    越界候选丢弃；两遍规则——**有森林群系格就只收它们**
///    （作者刷过森林笔刷），否则收 `height > 0` 的陆地格（未刷 kind 的兜底）。
///
/// `raster_offset` = 本图局部原点相对栅格原点的偏移（渲染侧为
/// `map_origin - raster_origin`）——城市子图继承父图底图时，同一局部点必须
/// 换到父图栅格的正确格上。
///
/// 无地形（`None`）、退化环（顶点 < 3）、非正 `spacing_wu` / `cell_size`
/// 一律返回空——渲染侧据此回退为「远带晕 + 地界描边 + 名字」，不画树。
pub fn forest_sites(
    ring_local: &[f32],
    holes: &[&[f32]],
    terrain: Option<&TerrainData>,
    cell_size: f32,
    raster_offset: (f32, f32),
    seed: u64,
    spacing_wu: f32,
) -> Vec<(f32, f32)> {
    // 无栅格 = 无法判断水陆 / kind，近带不画树（设计稿 §6.4 空采样回退）。
    let Some(terrain) = terrain else {
        return Vec::new();
    };
    if ring_local.len() < 6 || spacing_wu <= 0.0 || cell_size <= 0.0 {
        return Vec::new();
    }

    let Some((min_x, min_y, max_x, max_y)) = ring_bounds(ring_local) else {
        return Vec::new();
    };

    // 候选数按包围盒跨度算；`ceil` 保证覆盖到远端（最后一个候选的中心可能
    // 超出 bbox 外沿半格，但抖动幅度不到半格，仍在环附近，由 `point_in_ring`
    // 统一裁决）。
    let cols = ((max_x - min_x) / spacing_wu).ceil() as i64;
    let rows = ((max_y - min_y) / spacing_wu).ceil() as i64;

    let jitter = JITTER_FRACTION * spacing_wu;
    let mut forest = Vec::new();
    let mut land = Vec::new();
    for gy in 0..rows {
        for gx in 0..cols {
            let center_x = min_x + (gx as f32 + 0.5) * spacing_wu;
            let center_y = min_y + (gy as f32 + 0.5) * spacing_wu;
            let hash = splitmix64(seed ^ ((gx as u64) << 32) ^ (gy as u64));
            let x = center_x + signed_unit(hash as u32) * jitter;
            let y = center_y + signed_unit((hash >> 32) as u32) * jitter;
            if !point_in_ring(ring_local, x, y) {
                continue;
            }
            // 带洞地界（ADR-0029 M2）：洞内（湖面 / 草地芯）不落树。
            if holes.iter().any(|h| point_in_ring(h, x, y)) {
                continue;
            }
            let Some(cell) = cell_at(terrain, cell_size, raster_offset, x, y) else {
                continue;
            };
            // 格子侧比较**森林群系**编号（biome 冻结表）；`FEATURE_FOREST`(4)
            // 是 Place.feature 的旧 kind 词汇表，两者经 v1 迁移表对应。
            if wv_terrain::biome::is_forest_family(cell.biome) {
                forest.push((x, y));
            }
            if cell.height > 0 {
                land.push((x, y));
            }
        }
    }

    // 两遍规则：刷过森林 kind 时只认森林格；一格都没有才回退到「陆地」。
    if forest.is_empty() {
        land
    } else {
        forest
    }
}

/// 环的包围盒 `(min_x, min_y, max_x, max_y)`；非有限坐标视为退化环。
fn ring_bounds(ring: &[f32]) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in ring.chunks_exact(2) {
        if !point[0].is_finite() || !point[1].is_finite() {
            return None;
        }
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    Some((min_x, min_y, max_x, max_y))
}

/// 局部点 + `raster_offset` → 地形格；越界返回 `None`。
fn cell_at(
    terrain: &TerrainData,
    cell_size: f32,
    raster_offset: (f32, f32),
    x: f32,
    y: f32,
) -> Option<&TerrainCellData> {
    let gx = ((x + raster_offset.0) / cell_size).floor();
    let gy = ((y + raster_offset.1) / cell_size).floor();
    if gx < 0.0 || gy < 0.0 {
        return None;
    }
    let gx = gx as usize;
    let gy = gy as usize;
    if gx >= terrain.width as usize || gy >= terrain.height as usize {
        return None;
    }
    terrain.cells.get(gy * terrain.width as usize + gx)
}

/// 32 位无符号整数 → `[-1, 1]` 的确定性有符号单位值（抖动分量）。
fn signed_unit(bits: u32) -> f32 {
    bits as f32 / u32::MAX as f32 * 2.0 - 1.0
}

/// splitmix64（Steele / Lea / Flood）—— 一次性雪崩哈希，作确定性抖动的唯一
/// 熵源。**不引入 `rand`**：跨进程 / 跨平台结果只由 `seed` 与格坐标决定，
/// 重开工程树冠布局不跳变。
fn splitmix64(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    /// 100×100 世界单位的正方形环（局部坐标，首尾不重复）。
    fn square_ring() -> Vec<f32> {
        vec![0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0]
    }

    /// `w`×`h` 格、每格 `cell_size` 世界单位、全图同 surface / biome / height。
    fn uniform_grid(w: u32, h: u32, surface: u8, biome: u8, height: i16) -> TerrainData {
        TerrainData {
            width: w,
            height: h,
            cells: vec![
                TerrainCellData {
                    height,
                    surface,
                    biome,
                };
                (w * h) as usize
            ],
        }
    }

    /// 森林格子的群系编号（biome 冻结表；feature 4 经 v1 迁移表映射到此）。
    const FOREST_BIOME: u8 = 1;

    /// 10×10 格（格边长 10）：左半列森林群系、右半列草原群系，
    /// 全图陆地（height=10）。
    fn half_forest_grid() -> TerrainData {
        let mut grid = uniform_grid(10, 10, 1, 3, 10);
        for (i, cell) in grid.cells.iter_mut().enumerate() {
            if i % 10 < 5 {
                cell.biome = FOREST_BIOME;
            }
        }
        grid
    }

    /// 把局部点按「局部 + raster_offset → 格心」口径映射回格子，读群系。
    fn kind_at(terrain: &TerrainData, cell_size: f32, offset: (f32, f32), x: f32, y: f32) -> u8 {
        let gx = ((x + offset.0) / cell_size).floor() as usize;
        let gy = ((y + offset.1) / cell_size).floor() as usize;
        terrain.cells[gy * terrain.width as usize + gx].biome
    }

    #[test]
    fn overlay_far_blend_anchors() {
        assert_eq!(overlay_far_blend(0.75), 0.0);
        assert_eq!(overlay_far_blend(1.25), 1.0);
        assert!(
            (overlay_far_blend(1.0) - 0.5).abs() < 1e-6,
            "中点 smoothstep=0.5，got {}",
            overlay_far_blend(1.0)
        );
    }

    #[test]
    fn overlay_far_blend_clamps_outside_the_band() {
        assert_eq!(overlay_far_blend(0.0), 0.0, "近端之外恒 0");
        assert_eq!(overlay_far_blend(2.0), 1.0, "远端之外恒 1");
    }

    #[test]
    fn forest_sites_are_inside_the_ring_and_deterministic() {
        let ring = square_ring();
        let terrain = uniform_grid(10, 10, 1, 3, 10);
        let a = forest_sites(
            &ring,
            &[],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        let b = forest_sites(
            &ring,
            &[],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        assert_eq!(a, b, "同 seed + 同网格两次调用必须逐元素相等");
        assert_eq!(
            a.len(),
            16,
            "100×100 环 / 25 间距 = 4×4 个候选，全部陆地全部保留，got {}",
            a.len()
        );
        for &(x, y) in &a {
            assert!(
                wv_core::geometry::point_in_ring(&ring, x, y),
                "采样点 ({x}, {y}) 必须落在地界内"
            );
        }
    }

    #[test]
    fn forest_sites_prefer_forest_kind_cells() {
        let terrain = half_forest_grid();
        let sites = forest_sites(
            &square_ring(),
            &[],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        assert_eq!(
            sites.len(),
            8,
            "左半 8 个候选落在 kind=4 格，got {}",
            sites.len()
        );
        for &(x, y) in &sites {
            assert_eq!(
                kind_at(&terrain, 10.0, (0.0, 0.0), x, y),
                FOREST_BIOME,
                "点 ({x}, {y}) 应落在森林格"
            );
        }
    }

    #[test]
    fn forest_sites_fall_back_to_land_when_no_cell_is_forest() {
        let terrain = uniform_grid(10, 10, 1, 3, 10);
        let sites = forest_sites(
            &square_ring(),
            &[],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        assert_eq!(
            sites.len(),
            16,
            "无森林格时收 height>0 的全部 16 个候选，got {}",
            sites.len()
        );
    }

    #[test]
    fn forest_sites_return_empty_when_all_water() {
        let terrain = uniform_grid(10, 10, 0, 0, -5);
        let sites = forest_sites(
            &square_ring(),
            &[],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        assert!(
            sites.is_empty(),
            "全水域 height=-5，无森林也无陆地，应得 0，got {}",
            sites.len()
        );
    }

    #[test]
    fn forest_sites_return_empty_without_terrain() {
        let sites = forest_sites(
            &square_ring(),
            &[],
            None,
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        assert!(
            sites.is_empty(),
            "无栅格地形时回退远带晕、不画树，got {}",
            sites.len()
        );
    }

    /// 带洞地界：洞内候选不落树（M2）。100×100 环、中央 30..70 洞、
    /// 间距 25 → 4×4 候选中去掉洞内 4 个，剩 12。
    #[test]
    fn forest_sites_skips_sites_inside_holes() {
        let ring = square_ring();
        let terrain = uniform_grid(10, 10, 1, 3, 10);
        let hole = vec![30.0, 30.0, 70.0, 30.0, 70.0, 70.0, 30.0, 70.0];
        let sites = forest_sites(
            &ring,
            &[&hole],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        assert_eq!(
            sites.len(),
            12,
            "4×4 候选应剔除洞内 4 个（候选 37.5/62.5 × 37.5/62.5），got {}",
            sites.len()
        );
        for &(x, y) in &sites {
            assert!(
                !wv_core::geometry::point_in_ring(&hole, x, y),
                "点 ({x}, {y}) 不得落在洞内"
            );
        }
    }

    #[test]
    fn forest_sites_honor_raster_offset() {
        let terrain = half_forest_grid();
        let ring = square_ring();
        let base = forest_sites(
            &ring,
            &[],
            Some(&terrain),
            10.0,
            (0.0, 0.0),
            0x5EED_1234,
            25.0,
        );
        let shifted = forest_sites(
            &ring,
            &[],
            Some(&terrain),
            10.0,
            (10.0, 10.0),
            0x5EED_1234,
            25.0,
        );
        assert!(
            !base.is_empty() && !shifted.is_empty(),
            "base / shifted 都应非空（got {} / {}）",
            base.len(),
            shifted.len()
        );
        assert_ne!(
            base, shifted,
            "同一批局部候选点 +10 偏移后落入不同格，筛选结果必须变化"
        );
        for &(x, y) in &shifted {
            assert_eq!(
                kind_at(&terrain, 10.0, (10.0, 10.0), x, y),
                FOREST_BIOME,
                "偏移后点 ({x}, {y}) 应落在森林格"
            );
        }
    }
}
