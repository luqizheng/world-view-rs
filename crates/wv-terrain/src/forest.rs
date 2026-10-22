//! 森林覆盖物生成（codex_forest_design.md §三～§六 的纯数据层实现）。
//!
//! ## 职责
//! 从已生成的归一化高度 / 温度 / 湿度三张场图（均为 `0..=1` 的 `Vec<f32>`，
//! 与 [`crate::generate`] 的输出同构），按 Codex 五段管线产出树木实例：
//!
//! ```text
//! 森林适宜度 → 森林密度场（聚集噪声） → 密度分类 → 带密度约束的泊松式采样
//! ```
//!
//! ## 边界（不做什么）
//! - **零第三方依赖**：聚集噪声复用 [`crate::generate::Perlin`]，采样用本模块
//!   内置的 splitmix64确定性 RNG——不引入 `noise` / `rand`
//!   （设计文档 §十一-7 的方案 B，待收敛前最易回退）。
//! - **不渲染**：`TreeInstance` 只存参数与位置，Sprite 化归 `wv-render` /
//!   `wv-theme`（ADR-0043）；本模块无 Bevy、无 IO。
//! - **不持久化**：设计文档 §十一-3，先从种子 + 参数确定性重算。
//!
//! ## 确定性契约
//! 同一 `(尺寸, 三张场图, 种子)` 必须产出逐树一致的结果（与 [`crate::climate`]
//! 的「逐格一致」纪律同源），单测覆盖。

use crate::biome::{
    BIOME_FOREST, BIOME_GRASSLAND, BIOME_HILLS, BIOME_JUNGLE, BIOME_OASIS, BIOME_PLAINS,
    BIOME_SAVANNA, BIOME_SWAMP, BIOME_TROPICAL_RAINFOREST, BIOME_WETLAND,
};
use crate::generate::Perlin;

// ============================================================================
// 数据结构（设计文档 §2.2；position 用 (f32, f32) 替代 Bevy Vec2，见模块注释）
// ============================================================================

/// 森林密度等级——**只用于选择视觉样式**（tint / 资产变体），
/// 树木间距随连续密度变化，避免分类阈值处出现突兀边界（文档 §五）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForestType {
    /// 疏林。
    Sparse,
    /// 常规森林。
    Normal,
    /// 密林。
    Dense,
}

/// 树种。
///
/// `Tropical` 对应资产表的棕榈行（设计文档 §0 图 B 已确认有资产）；
/// 干旱树种（仙人掌）暂不建模——待决 §十一-4。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeType {
    /// 阔叶。
    Broadleaf,
    /// 针叶。
    Conifer,
    /// 热带（棕榈）。
    Tropical,
}

/// 单棵树的实例：位置 + 大小 + 变体 + 类型。
///
/// **只存参数**：`variant` 是资产表中的列引用，`z_order` 由渲染层依
/// `position.1` 现算（文档 §2.2 仓库备注），不在此冗余。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeInstance {
    /// 地图像素坐标（左上原点；世界坐标换算归渲染层）。
    pub position: (f32, f32),
    /// 尺度抖动，`0.75..1.25`，避免机械感。
    pub scale: f32,
    /// 资产变体列号（同类第几个）。
    pub variant: usize,
    /// 树种。
    pub tree_type: TreeType,
    /// 所在林区的密度等级（视觉样式选择用）。
    pub forest_type: ForestType,
}

/// 森林生成结果：**密度场与树木实例分开保存**——密度决定哪里应该有森林，
/// 实例决定每棵树的位置、大小和类型（文档 §2.2 Codex 原则）。
#[derive(Clone, Debug, PartialEq)]
pub struct ForestMap {
    pub width: usize,
    pub height: usize,
    /// 连续密度场，`0..=1`，行优先 `index = y * width + x`。
    pub density: Vec<f32>,
    /// 采样产出的树木实例。
    pub trees: Vec<TreeInstance>,
}

// ============================================================================
// 平滑工具（文档 §3.1）
// ============================================================================

/// smoothstep：`edge0..edge1` 间 `0→1` 平滑上升，两端饱和。
///
/// 设计文档 §三：适宜度用平滑过渡而非硬阈值分类。
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ============================================================================
// 森林适宜度（文档 §3.1 Codex 原文移植）
// ============================================================================

/// 森林适宜度：五因子连乘，唯一的硬边界是海平面。
///
/// 入参均为归一化 `0..=1`。阈值含义：
/// - 海平面 `SEA_LEVEL = 0.45`：**示例值**，接入本仓库时以高度场实际海平面
///   替换（文档 §十一-9；`wv-terrain` 高度语义是「米、海平面 = 0」，
///   归一化在调用侧完成后此阈值随归一化口径修正）；
/// - 温度过冷（<0.12..0.35 渐变）不宜树，过热仅打 7 折；
/// - 高海拔 0.72..0.92 渐近林线；海岸 0.45..0.48 起坡限制滩涂。
pub const SEA_LEVEL: f32 = 0.45;

/// 森林适宜度（文档 §3.1 原式）。
pub fn forest_suitability(height: f32, temperature: f32, moisture: f32) -> f32 {
    // 海洋不能生成树木（唯一硬边界，语义明确）
    if height < SEA_LEVEL {
        return 0.0;
    }

    // 湿度越高越适合森林
    let wetness = smoothstep(0.25, 0.75, moisture);

    // 极冷地区不适合树木
    let cold_limit = smoothstep(0.12, 0.35, temperature);

    // 极热地区略微降低适宜度
    let heat_limit = 1.0 - 0.3 * smoothstep(0.85, 1.0, temperature);

    // 高海拔逐渐接近林线
    let altitude_limit = 1.0 - smoothstep(0.72, 0.92, height);

    // 海岸附近允许森林，但限制极低海拔
    let coast_limit = smoothstep(SEA_LEVEL, 0.48, height);

    (wetness * cold_limit * heat_limit * altitude_limit * coast_limit).clamp(0.0, 1.0)
}

// ============================================================================
// 森林密度场——聚集噪声（文档 §四；noise crate → 手写 Perlin，方案 B）
// ============================================================================

/// 大尺度聚集噪声频率：越小，森林斑块越大（参考图级大面积可降至 0.003～0.006）。
pub const LARGE_NOISE_FREQ: f64 = 0.008;
/// 小尺度边缘破碎噪声频率：越大，边缘越破碎。
pub const DETAIL_NOISE_FREQ: f64 = 0.035;

/// 林线下沿（米）：超过此海拔开始出现林线衰减。
pub const TREE_LINE_M: f32 = 2000.0;
/// 林线上沿（米）：超过此海拔森林密度归零。
pub const TREE_LINE_FULL_M: f32 = 2800.0;

/// 群系 → 森林基础密度（`0..=1`）。
///
/// **为什么用群系而不是气候**：温度/湿度是生成期派生量、不落盘
/// （见 [`crate::continent`]），渲染期只能拿到 `terrain.bin` 的三通道
/// （height / surface / biome）。群系已经把「温度 + 湿度 → 植被带」的推理
/// 固化下来，渲染期直接查表即可，不必重算气候。
///
/// 取值语义：热带雨林最密，森林次之，草原/丘陵稀，沼泽/湿地中等，
/// 荒漠/戈壁/雪原/苔原/水面为 0。
fn biome_forest_density(biome: u8) -> f32 {
    match biome {
        BIOME_TROPICAL_RAINFOREST | BIOME_JUNGLE => 0.95,
        BIOME_FOREST => 0.85,
        BIOME_SWAMP | BIOME_WETLAND => 0.45,
        BIOME_HILLS => 0.35,
        BIOME_OASIS => 0.30,
        BIOME_GRASSLAND | BIOME_PLAINS => 0.22,
        BIOME_SAVANNA => 0.12,
        _ => 0.0,
    }
}

/// 生成森林密度场（**群系驱动**，渲染期可用）。
///
/// 输入：`biomes`（`terrain.bin` 的 biome 通道）+ `heights_m`（海拔，米）。
/// 输出与 [`generate_forest_density_from_climate`] 同构：行优先
/// `index = y * width + x` 的 `0..=1` 连续密度场。
///
/// 组成：`群系基础密度 × 林线衰减 × 聚集噪声 patch`。
/// - **林线**：`TREE_LINE_M..TREE_LINE_FULL_M` 平滑衰减到 0；
/// - **聚集噪声**：与气候版同参（[`LARGE_NOISE_FREQ`] / [`DETAIL_NOISE_FREQ`]），
///   使森林成片、边缘破碎，并在同群系内部形成疏密变化（林间空地）。
///
/// 零基础密度格（荒漠 / 水面 / 雪原…）直接短路，不做噪声采样——噪声只在
/// 可能有树的格上计算，整图开销因此主要落在森林面积上。
pub fn generate_forest_density_from_biome(
    width: usize,
    height: usize,
    biomes: &[u8],
    heights_m: &[i16],
    seed: u64,
) -> Vec<f32> {
    assert_eq!(biomes.len(), width * height, "biomes 场尺寸不符");
    assert_eq!(heights_m.len(), width * height, "heights 场尺寸不符");

    let noise = Perlin::new(seed);
    let mut density = vec![0.0_f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;

            let base = biome_forest_density(biomes[i]);
            if base <= 0.0 {
                continue;
            }

            // 林线衰减（米）
            let h = heights_m[i] as f32;
            let altitude = 1.0 - smoothstep(TREE_LINE_M, TREE_LINE_FULL_M, h);
            if altitude <= 0.0 {
                continue;
            }

            // 大尺度噪声：森林整体分布
            let large =
                noise.noise2d(x as f64 * LARGE_NOISE_FREQ, y as f64 * LARGE_NOISE_FREQ) as f32;

            // 小尺度噪声：森林边缘破碎（+100 偏移与大尺度解相）
            let detail = noise.noise2d(
                x as f64 * DETAIL_NOISE_FREQ + 100.0,
                y as f64 * DETAIL_NOISE_FREQ + 100.0,
            ) as f32;

            let cluster = (0.5 + large * 0.38 + detail * 0.12).clamp(0.0, 1.0);
            let patch = smoothstep(0.25, 0.65, cluster);

            // **疏密调制而非砍半**：群系已判定「这里是森林」，聚集噪声只该
            // 制造林间空地与密林核心，不该把整片森林稀释成稀树草原。
            // 故取 `0.5 + 0.5 × patch`（0.5..=1.0），森林格永远保有基础密度。
            let patch_gain = 0.5 + 0.5 * patch;

            density[i] = base * altitude * patch_gain;
        }
    }

    density
}

/// 生成森林密度场（**气候驱动**）：`suitability × patch`，patch 由双层聚集噪声平滑得出。
///
/// 三张输入必须等长 `width * height`，行优先、坐标一一对应（文档 §八前提）。
///
/// 与文档原式的差异：`noise::OpenSimplex` → [`Perlin`]（复用现有手写噪声，
/// 保持 `wv-terrain` 零依赖）；噪声值域同为约 `[-1, 1]`，后续
/// `0.5 + large*0.38 + detail*0.12` 的抬升与 clamp 语义不变。
///
/// **适用边界**：温度/湿度是**生成期派生、不落盘**的量（见
/// [`crate::continent`]），渲染期拿不到——渲染期请用
/// [`generate_forest_density_from_biome`]。本函数供生成期/CLI/离线批处理使用。
pub fn generate_forest_density_from_climate(
    width: usize,
    height: usize,
    heights: &[f32],
    temperatures: &[f32],
    moistures: &[f32],
    seed: u64,
) -> Vec<f32> {
    assert_eq!(heights.len(), width * height, "heights 场尺寸不符");
    assert_eq!(
        temperatures.len(),
        width * height,
        "temperatures 场尺寸不符"
    );
    assert_eq!(moistures.len(), width * height, "moistures 场尺寸不符");

    let noise = Perlin::new(seed);
    let mut density = vec![0.0_f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;

            let suitability = forest_suitability(heights[i], temperatures[i], moistures[i]);

            // 大尺度噪声：森林整体分布
            let large =
                noise.noise2d(x as f64 * LARGE_NOISE_FREQ, y as f64 * LARGE_NOISE_FREQ) as f32;

            // 小尺度噪声：森林边缘破碎（+100 偏移与大尺度解相）
            let detail = noise.noise2d(
                x as f64 * DETAIL_NOISE_FREQ + 100.0,
                y as f64 * DETAIL_NOISE_FREQ + 100.0,
            ) as f32;

            // 抬回 0..=1：噪声约 [-1,1]，0.5 为中点
            let cluster = (0.5 + large * 0.38 + detail * 0.12).clamp(0.0, 1.0);

            // 平滑成片：连续林区，保留部分空地
            let patch = smoothstep(0.25, 0.65, cluster);

            density[i] = suitability * patch;
        }
    }

    density
}

// ============================================================================
// 密度分类（文档 §五）——只选视觉样式，间距另随连续密度变化
// ============================================================================

/// 密度分类阈值：`< 0.18` 无林；`0.18..0.40` 疏；`0.40..0.72` 常规；其余密林。
pub fn classify_forest(density: f32) -> Option<ForestType> {
    if density < 0.18 {
        None
    } else if density < 0.40 {
        Some(ForestType::Sparse)
    } else if density < 0.72 {
        Some(ForestType::Normal)
    } else {
        Some(ForestType::Dense)
    }
}

// ============================================================================
// 带密度约束的泊松式采样（文档 §六；rand crate → 内置 splitmix64，方案 B）
// ============================================================================

/// 最小树间距（地图像素单位；密度 1 处）。
pub const MIN_SPACING: f32 = 7.0;
/// 最大树间距（密度 0.18 处）。
pub const MAX_SPACING: f32 = 26.0;

/// splitmix64 确定性 RNG（零依赖替代 `rand::StdRng`）。
///
/// 输出质量满足本模块「随机投点 + 概率接受」的用途；与
/// `rand` 的流不兼容——确定性契约只对本模块内部自洽。
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0, 1)` 均匀 f32。
    fn next_f32(&mut self) -> f32 {
        // 取高 24 位，除以 2^24：足够均匀且无精度损失
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// `[lo, hi)` 均匀 f32。
    fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }

    /// `[0, n)` 均匀 usize。
    fn range_usize(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// 从密度场采样树木实例（文档 §六原式，`Vec2` → `(f32, f32)`）。
///
/// 随机候选点 + 空间网格检测：候选树与既有树需满足**候选树的间距要求**
/// （`spacing = max - (max - min) * d`）。注意这是**非严格对称**的可变半径
/// 泊松采样（文档 §六注意点 1）；严格对称版留待需要时升级。
///
/// 树种分流沿用文档原阈值：`temperature < 0.38 → Conifer`，
/// `> 0.75 → Tropical`，其余 Broadleaf（待决 §十一-4）。
pub fn generate_trees(
    width: usize,
    height: usize,
    density: &[f32],
    temperatures: &[f32],
    seed: u64,
) -> Vec<TreeInstance> {
    assert_eq!(density.len(), width * height, "density 场尺寸不符");
    assert_eq!(
        temperatures.len(),
        width * height,
        "temperatures 场尺寸不符"
    );
    assert!(width > 0 && height > 0, "网格不能为空");

    let mut rng = SplitMix64::new(seed);
    let mut trees: Vec<TreeInstance> = Vec::new();

    // 网格单元不大于最小间距 / √2，保证任一点的最邻域只需查 3×3 起步
    let cell_size = MIN_SPACING / std::f32::consts::SQRT_2;
    let grid_width = (width as f32 / cell_size).ceil() as usize;
    let grid_height = (height as f32 / cell_size).ceil() as usize;
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); grid_width * grid_height];

    // 随机候选数量：全图统一投点（Chunk 化为待决 §十一-10）
    let attempts = width * height / 25;

    for _ in 0..attempts {
        let x = rng.range_f32(0.0, width as f32);
        let y = rng.range_f32(0.0, height as f32);

        let ix = (x as usize).min(width - 1);
        let iy = (y as usize).min(height - 1);
        let index = iy * width + ix;

        let d = density[index];

        // 无林带直接跳过（与 classify_forest 的 None 阈值一致）
        if d < 0.18 {
            continue;
        }

        // 密度越高，树木越容易被接受
        if rng.next_f32() > d {
            continue;
        }

        // 密度越高，间距越小（连续变化，避免分类阈值处的突兀边界）
        let spacing = MAX_SPACING - (MAX_SPACING - MIN_SPACING) * d;

        let gx = (x / cell_size) as usize;
        let gy = (y / cell_size) as usize;

        let search_radius = (MAX_SPACING / cell_size).ceil() as usize;
        let x0 = gx.saturating_sub(search_radius);
        let y0 = gy.saturating_sub(search_radius);
        let x1 = (gx + search_radius).min(grid_width - 1);
        let y1 = (gy + search_radius).min(grid_height - 1);

        let mut valid = true;
        'neighbors: for cy in y0..=y1 {
            for cx in x0..=x1 {
                for &tree_index in &grid[cy * grid_width + cx] {
                    let other = &trees[tree_index];
                    let dx = other.position.0 - x;
                    let dy = other.position.1 - y;
                    if (dx * dx + dy * dy).sqrt() < spacing {
                        valid = false;
                        break 'neighbors;
                    }
                }
            }
        }

        if !valid {
            continue;
        }

        let forest_type = classify_forest(d).expect("d >= 0.18 必然有分类");
        let temperature = temperatures[index];

        let tree_type = if temperature < 0.38 {
            TreeType::Conifer
        } else if temperature > 0.75 {
            TreeType::Tropical
        } else {
            TreeType::Broadleaf
        };

        let tree = TreeInstance {
            position: (x, y),
            scale: rng.range_f32(0.75, 1.25),
            variant: rng.range_usize(8),
            tree_type,
            forest_type,
        };

        let tree_index = trees.len();
        trees.push(tree);
        grid[gy * grid_width + gx].push(tree_index);
    }

    trees
}

/// 一步到位：密度场 + 树木实例（文档 §八的串联入口，**气候驱动**）。
///
/// 两个种子分开：密度噪声一个、采样一个；均须由调用侧存 YAML（P3）。
///
/// **适用边界**：这是**离线整图**路径（生成期 / CLI / 批处理）。渲染期走
/// LOD 采样——密度场由 [`generate_forest_density_from_biome`] 给出，
/// 站点采样复用渲染层既有的 LOD 纪律（ADR-0031 D2/D3）。
pub fn generate_forest(
    width: usize,
    height: usize,
    heights: &[f32],
    temperatures: &[f32],
    moistures: &[f32],
    density_seed: u64,
    tree_seed: u64,
) -> ForestMap {
    let density = generate_forest_density_from_climate(
        width,
        height,
        heights,
        temperatures,
        moistures,
        density_seed,
    );
    let trees = generate_trees(width, height, &density, temperatures, tree_seed);
    ForestMap {
        width,
        height,
        density,
        trees,
    }
}

// ============================================================================
// 测试（§7.2：手算期望值断言；§7.3：完整英文短句命名；§7.4：附实际上下文）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- smoothstep ----

    #[test]
    fn smoothstep_is_half_at_midpoint() {
        // 手算：t=0.5 → 0.5²×(3-2×0.5)=0.25×2=0.5
        let s = smoothstep(0.0, 1.0, 0.5);
        assert!((s - 0.5).abs() < 1e-6, "got {s}");
    }

    #[test]
    fn smoothstep_saturates_outside_edges() {
        assert_eq!(smoothstep(0.25, 0.75, 0.0), 0.0, "低于下边界应为 0");
        assert_eq!(smoothstep(0.25, 0.75, 1.0), 1.0, "高于上边界应为 1");
    }

    // ---- forest_suitability ----

    #[test]
    fn ocean_height_yields_zero_suitability() {
        // 海平面硬边界：< 0.45 恒为 0，无论温湿度多理想
        let s = forest_suitability(0.44, 0.5, 1.0);
        assert_eq!(s, 0.0, "got {s}");
    }

    #[test]
    fn ideal_land_climate_yields_high_suitability() {
        // height=0.5（海岸刚过 coast_limit 顶点）、t=0.5、m=1.0：
        // wetness=1，cold=1，heat=1，altitude=1，coast=smoothstep(0.45,0.48,0.5)=1
        let s = forest_suitability(0.5, 0.5, 1.0);
        assert!(s > 0.99, "理想气候应接近 1，got {s}");
    }

    #[test]
    fn freezing_temperature_kills_suitability() {
        // t=0.0：cold_limit = smoothstep(0.12,0.35,0)=0 → 乘积为 0
        let s = forest_suitability(0.6, 0.0, 1.0);
        assert_eq!(s, 0.0, "got {s}");
    }

    #[test]
    fn extreme_altitude_kills_suitability() {
        // height=0.95 > 0.92：altitude_limit = 0 → 乘积为 0
        let s = forest_suitability(0.95, 0.5, 1.0);
        assert_eq!(s, 0.0, "got {s}");
    }

    #[test]
    fn hot_climate_only_reduces_suitability_partially() {
        // t=1.0：heat_limit = 1 - 0.3 = 0.7（原文只打折不归零）；
        // 其余因子取 1（height=0.5、m=1）
        let hot = forest_suitability(0.5, 1.0, 1.0);
        assert!((hot - 0.7).abs() < 1e-6, "极热应恰为 0.7，got {hot}");
    }

    // ---- classify_forest ----

    #[test]
    fn classification_boundaries_follow_document_thresholds() {
        // 文档 §五阈值：0.18 / 0.40 / 0.72
        assert_eq!(classify_forest(0.17), None, "0.17 应无林");
        assert_eq!(
            classify_forest(0.18),
            Some(ForestType::Sparse),
            "0.18 应疏林"
        );
        assert_eq!(
            classify_forest(0.40),
            Some(ForestType::Normal),
            "0.40 应常规"
        );
        assert_eq!(
            classify_forest(0.72),
            Some(ForestType::Dense),
            "0.72 应密林"
        );
    }

    // ---- generate_forest_density ----

    #[test]
    fn density_is_zero_where_suitability_is_zero() {
        // 全海图（height=0 < 0.45）：适宜度恒 0 → 密度恒 0
        let n = 8;
        let heights = vec![0.0_f32; n * n];
        let temps = vec![0.5_f32; n * n];
        let moist = vec![1.0_f32; n * n];
        let d = generate_forest_density_from_climate(n, n, &heights, &temps, &moist, 42);
        assert!(
            d.iter().all(|&v| v == 0.0),
            "全海图不应出现密度，got max {:?}",
            d.iter().cloned().fold(f32::MIN, f32::max)
        );
    }

    #[test]
    fn density_is_deterministic_for_same_seed() {
        // 确定性契约：同种子逐格一致（与 climate 模块同源纪律）
        let n = 16;
        let heights: Vec<f32> = (0..n * n).map(|i| 0.5 + (i % 7) as f32 * 0.01).collect();
        let temps: Vec<f32> = (0..n * n).map(|i| 0.4 + (i % 5) as f32 * 0.02).collect();
        let moist: Vec<f32> = (0..n * n).map(|i| 0.3 + (i % 11) as f32 * 0.05).collect();
        let a = generate_forest_density_from_climate(n, n, &heights, &temps, &moist, 7);
        let b = generate_forest_density_from_climate(n, n, &heights, &temps, &moist, 7);
        assert_eq!(a, b, "同种子密度场必须逐格一致");
    }

    #[test]
    fn density_is_bounded_in_unit_range() {
        let n = 32;
        let heights: Vec<f32> = (0..n * n).map(|i| (i % 13) as f32 / 13.0).collect();
        let temps: Vec<f32> = (0..n * n).map(|i| (i % 17) as f32 / 17.0).collect();
        let moist: Vec<f32> = (0..n * n).map(|i| (i % 19) as f32 / 19.0).collect();
        let d = generate_forest_density_from_climate(n, n, &heights, &temps, &moist, 1);
        assert!(
            d.iter().all(|&v| (0.0..=1.0).contains(&v)),
            "密度越界：{:?}",
            d.iter()
                .filter(|&&v| !(0.0..=1.0).contains(&v))
                .take(5)
                .collect::<Vec<_>>()
        );
    }

    // ---- generate_trees ----

    /// 构造一张均匀密林密度场（全部 1.0）。
    fn uniform_dense(n: usize) -> (Vec<f32>, Vec<f32>) {
        (
            vec![1.0_f32; n * n],
            vec![0.5_f32; n * n], // 温度 0.5 → 全 Broadleaf
        )
    }

    #[test]
    fn trees_are_deterministic_for_same_seed() {
        let n = 64;
        let (density, temps) = uniform_dense(n);
        let a = generate_trees(n, n, &density, &temps, 99);
        let b = generate_trees(n, n, &density, &temps, 99);
        assert_eq!(a, b, "同种子树木实例必须逐树一致");
    }

    #[test]
    fn trees_respect_minimum_spacing() {
        // 均匀密林中 spacing 恒为 MIN_SPACING=7；
        // 验证全局最近邻距离 >= 7 - ε（§六注意点 1 的「非严格对称」留裕量：
        // 理论上仍可能存在 pair 违反，此处用文档给出的保证口径——候选点满足
        // 自身 spacing——故均匀密度下等价于全局间距约束）
        let n = 128;
        let (density, temps) = uniform_dense(n);
        let trees = generate_trees(n, n, &density, &temps, 3);
        assert!(!trees.is_empty(), "密林应有树");

        let cell = MIN_SPACING / std::f32::consts::SQRT_2;
        let gw = (n as f32 / cell).ceil() as usize;
        let mut grid: Vec<Vec<usize>> = vec![Vec::new(); gw * gw];
        for (i, t) in trees.iter().enumerate() {
            let gx = ((t.position.0 / cell) as usize).min(gw - 1);
            let gy = ((t.position.1 / cell) as usize).min(gw - 1);
            grid[gy * gw + gx].push(i);
        }
        for (i, t) in trees.iter().enumerate() {
            let gx = ((t.position.0 / cell) as usize).min(gw - 1);
            let gy = ((t.position.1 / cell) as usize).min(gw - 1);
            let r = (MAX_SPACING / cell).ceil() as usize;
            for cy in gy.saturating_sub(r)..=(gy + r).min(gw - 1) {
                for cx in gx.saturating_sub(r)..=(gx + r).min(gw - 1) {
                    for &j in &grid[cy * gw + cx] {
                        if j <= i {
                            continue;
                        }
                        let o = &trees[j];
                        let dist = ((o.position.0 - t.position.0).powi(2)
                            + (o.position.1 - t.position.1).powi(2))
                        .sqrt();
                        // 均匀密度：两树间距要求相同（对称），不应小于 MIN
                        assert!(
                            dist >= MIN_SPACING - 1e-3,
                            "树 {i} 与树 {j} 间距 {dist} < {MIN_SPACING}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn species_partition_follows_temperature_thresholds() {
        // 三块温度带：0.2→Conifer，0.6→Broadleaf，0.9→Tropical
        let n = 32;
        let mut temps = vec![0.6_f32; n * n];
        for x in 0..n / 3 {
            for y in 0..n {
                temps[y * n + x] = 0.2;
            }
        }
        for x in 2 * n / 3..n {
            for y in 0..n {
                temps[y * n + x] = 0.9;
            }
        }
        let density = vec![1.0_f32; n * n];
        let trees = generate_trees(n, n, &density, &temps, 11);
        assert!(!trees.is_empty(), "密林应有树");
        for t in &trees {
            let expected = if t.position.0 < (n / 3) as f32 {
                TreeType::Conifer
            } else if t.position.0 >= (2 * n / 3) as f32 {
                TreeType::Tropical
            } else {
                TreeType::Broadleaf
            };
            assert_eq!(
                t.tree_type, expected,
                "树在 {:?} 处树种分流错误",
                t.position
            );
        }
    }

    #[test]
    fn forest_map_combines_density_and_trees() {
        // generate_forest 串联入口：密度与实例分开保存（§2.2 Codex 原则）
        let n = 32;
        let heights = vec![0.5_f32; n * n];
        let temps = vec![0.5_f32; n * n];
        let moist = vec![0.9_f32; n * n];
        let map = generate_forest(n, n, &heights, &temps, &moist, 5, 6);
        assert_eq!(map.density.len(), n * n);
        // 密度场应普遍为正（理想气候 + 平原）
        let positive = map.density.iter().filter(|&&v| v > 0.0).count();
        assert!(
            positive > n * n / 2,
            "理想气候下多数格子应有密度，got {positive}"
        );
        assert!(!map.trees.is_empty(), "理想气候下应采到树");
        for t in &map.trees {
            assert!(
                t.position.0 >= 0.0 && t.position.0 < n as f32,
                "x 越界：{:?}",
                t.position
            );
            assert!(
                t.position.1 >= 0.0 && t.position.1 < n as f32,
                "y 越界：{:?}",
                t.position
            );
            assert!((0.75..1.25).contains(&t.scale), "scale 越界：{}", t.scale);
            assert!(t.variant < 8, "variant 越界：{}", t.variant);
        }
    }

    #[test]
    fn barren_sea_map_yields_no_trees() {
        // 全海：密度 0 → 采样必为空
        let n = 16;
        let heights = vec![0.1_f32; n * n];
        let temps = vec![0.5_f32; n * n];
        let moist = vec![1.0_f32; n * n];
        let map = generate_forest(n, n, &heights, &temps, &moist, 1, 2);
        assert!(
            map.trees.is_empty(),
            "全海不应有树，got {}",
            map.trees.len()
        );
    }

    // ---- generate_forest_density_from_biome（渲染期路径）----

    #[test]
    fn biome_density_is_zero_for_non_forest_biomes() {
        // 沙漠 / 雪原 / 水面 / 戈壁：基础密度 0 → 整格密度 0（2×2 网格）
        let biomes = vec![
            crate::biome::BIOME_DESERT,
            crate::biome::BIOME_SNOWFIELD,
            crate::biome::BIOME_OCEAN,
            crate::biome::BIOME_GOBI,
        ];
        let heights = vec![100_i16; 4];
        let d = generate_forest_density_from_biome(2, 2, &biomes, &heights, 1);
        assert!(
            d.iter().all(|&v| v == 0.0),
            "非森林群系不应有密度，got {:?}",
            d
        );
    }

    #[test]
    fn biome_density_ranks_rainforest_above_grassland() {
        // 群系基础密度排序：热带雨林 > 森林 > 草原。
        // 用同一 seed 同一坐标比较，排除噪声差异干扰。
        let n = 1;
        let heights = vec![100_i16];
        let rain = generate_forest_density_from_biome(
            n,
            n,
            &[crate::biome::BIOME_TROPICAL_RAINFOREST],
            &heights,
            7,
        )[0];
        let forest =
            generate_forest_density_from_biome(n, n, &[crate::biome::BIOME_FOREST], &heights, 7)[0];
        let grass =
            generate_forest_density_from_biome(n, n, &[crate::biome::BIOME_GRASSLAND], &heights, 7)
                [0];
        assert!(
            rain > forest && forest > grass,
            "排序错误：rain={rain} forest={forest} grass={grass}"
        );
    }

    #[test]
    fn biome_density_vanishes_above_tree_line() {
        // 3000 m > TREE_LINE_FULL_M(2800) → 林线以上密度为 0
        let n = 1;
        let d =
            generate_forest_density_from_biome(n, n, &[crate::biome::BIOME_FOREST], &[3000_i16], 7);
        assert_eq!(d[0], 0.0, "林线以上应为 0，got {}", d[0]);
    }

    #[test]
    fn biome_density_is_deterministic_and_bounded() {
        let n = 32;
        let biomes: Vec<u8> = (0..n * n)
            .map(|i| {
                if i % 3 == 0 {
                    crate::biome::BIOME_FOREST
                } else {
                    crate::biome::BIOME_GRASSLAND
                }
            })
            .collect();
        let heights = vec![120_i16; n * n];
        let a = generate_forest_density_from_biome(n, n, &biomes, &heights, 21);
        let b = generate_forest_density_from_biome(n, n, &biomes, &heights, 21);
        assert_eq!(a, b, "同种子密度场必须逐格一致");
        assert!(
            a.iter().all(|&v| (0.0..=1.0).contains(&v)),
            "密度越界：{:?}",
            a.iter()
                .filter(|&&v| !(0.0..=1.0).contains(&v))
                .take(5)
                .collect::<Vec<_>>()
        );
    }
}
