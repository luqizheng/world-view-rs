//! Field 管线三张数据纹理的**纯数据打包**（P0 规格锁 T1/T2/T3）。
//!
//! ## 做什么
//!
//! 把 [`wv_core::terrain::TerrainData`]（中性格数据）压成三张可直接上传 GPU 的
//! 原始像素缓冲：
//!
//! | 纹理 | 格式 | 采样 | 内容 | 规格 |
//! |---|---|---|---|---|
//! | `height_tex` | `R16Unorm` | Linear | 归一化高度 | T1 |
//! | `coast_tex` | `R16Unorm` | Linear | 到最近异类格的欧氏距离 / 32 格 | T3 |
//! | `attr_tex` | `Rgba8Unorm` | Nearest | R=水陆（**255 陆 / 0 水**）、G=biome、B/A 预留 | T2 |
//!
//! ## 为什么不转成 Bevy `Image`
//!
//! 本模块**不引入任何 Bevy 类型**（连 `bevy::prelude::Color` 也不引）——
//! AGENTS.md §2.1 分层红线：`wv-render` 虽可依赖 Bevy，但「数据打包」这一步
//! 与渲染 API 无关，绑定 Bevy 会让这条逻辑只能在 `app` feature 下被测试，
//! 且把「编码口径」与「GPU 资源类型」耦在一起。纹理落地（`Image` /
//! `TextureFormat::R16Unorm` 等）由调用方（`material.rs` 侧）按本模块
//! 给出的字节序与分辨率自行上传。
//!
//! ## 与谁的口径一致
//!
//! - **水陆（湿判定）**：与全仓一致——`surface == SURFACE_WATER`
//!   （`wv_terrain::SURFACE_WATER`）**或** `height < 0` 视为水
//!   （`wv_terrain::edit::is_water_cell`，唯一入口）。
//!   现行 CPU 烘焙路径 `terrain_view::cells` 也走同一口径，否则同一张图会
//!   被 CPU 路径与 GPU 路径渲出两种颜色。
//! - **海岸距离**：复用邻居模块 [`super::coast_sdf`] 的精确欧氏距离变换
//!   （Felzenszwalb EDT），**不重写 EDT**。T3 规定「用 surface 翻转处为
//!   海岸权威」（authoring 真值），不用海平面等深线。
//!
//! ## 已知口径差（刻意保留，记录在此）
//!
//! `attr_tex` 的 R 通道用**湿判定**（含 `height < 0`），而 `coast_tex` 的
//! 距离场按 **surface 翻转**分两侧（T3 硬约束）。二者对「surface=陆但
//! height<0」的格判定不同：attr 说它是水，coast 说它是陆。这是规格自身的
//! 分层（湿判定服务于「海平面以下都是湿的」这条全域规则，海岸线服务于
//! 「岸线由作者标注」这条作者真值），不是本模块的实现偏差。
//!
//! ## 编码口径（T1/T3）
//!
//! - T1：`h_norm = (h − H_MIN) / (H_MAX − H_MIN)`，`clamp` 到 `[0,1]`，
//!   `× 65535` 四舍五入成 `u16`。`i16` 实际取值范围 `[-32768, 32767]`
//!   超出 `H_MIN/H_MAX` 的部分**钳位**（不 panic）。
//! - T3：`enc = clamp(d / COAST_MAX_CELLS, 0, 1) × 65535`。
//!   `coast_sdf::FAR`（`1e6`，全图同质的饱和值）被钳到 `1.0` → `0xFFFF`，
//!   不允许溢出成 NaN/inf。
//! - **字节序**：`R16Unorm` 在 Bevy/WGPU 里是原生 `u16` **小端**存放，
//!   故第 `2i`、`2i+1` 字节 = `u16::to_le_bytes()`。
//! - **分辨率**：三张纹理分辨率**完全一致**，都等于地形格数
//!   （`data.width × data.height`），一格一 texel。**不**按现行 CPU 路径的
//!   `tile_res_for()` 放大——field 路径靠 shader 侧双三次插值取亚格精度
//!   （这正是 ADR-0053 选 GPU 逐像素求值的理由）。
//! - **行主序**：`index = y * width + x`，`y` 自上而下（= 屏幕向下）。
//!
//! 规格出处：`docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md` §1.1（T1/T2/T3）。

use wv_core::terrain::{TerrainCellData, TerrainData};

use super::coast_sdf;

/// 高度归一化下界（米）。T1 硬编码常量。
pub const H_MIN: f32 = -4000.0;

/// 高度归一化上界（米）。T1 硬编码常量。
pub const H_MAX: f32 = 6000.0;

/// `coast_dist` 编码覆盖的格数上限（T3：编码 `d/32`，覆盖 0–32 格）。
pub const COAST_MAX_CELLS: f32 = 32.0;

/// Unorm16 的最大值（`65535`）。T1/T3 共用。
const UNORM16_MAX: f32 = 65535.0;

/// 把 `[0,1]` 的归一化值编码成 `u16`（四舍五入）。
///
/// 输入先钳到 `[0,1]`，因此 NaN 也会被钳成 0（`f32::clamp` 对 NaN 返回
/// 下界）——永不 panic、永不产生越界 `u16`。
fn encode_unorm16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * UNORM16_MAX).round() as u16
}

/// 三张数据纹理打包好的原始像素缓冲（**未**转成 Bevy `Image`）。
///
/// 三张纹理的分辨率一致：宽 = `TerrainData::width`，高 = `TerrainData::height`。
#[derive(Debug, Clone)]
pub struct FieldPack {
    /// 纹理宽（= 地形格宽）。
    pub width: u32,
    /// 纹理高（= 地形格高）。
    pub height: u32,
    /// T1：height 场，`R16Unorm` 字节序（小端 `u16`），线性采样。
    /// 归一化 `h_norm = (h − H_MIN) / (H_MAX − H_MIN)`。
    pub height_tex: Vec<u8>,
    /// T3：coast_dist 场，`R16Unorm` 字节序（小端 `u16`），线性采样。
    /// 编码 `d / COAST_MAX_CELLS`，饱和在 `1.0`。
    pub coast_tex: Vec<u8>,
    /// T2：attr 通道，`Rgba8Unorm`。
    /// R = 水陆（1 陆 / 0 水，**湿判定**口径）、G = biome（裸字节）、
    /// B = 湿度预留（0）、A = 覆盖物键预留（0）。
    pub attr_tex: Vec<u8>,
}

impl FieldPack {
    /// 主入口：把一份地形数据压成三张数据纹理的原始像素缓冲。
    ///
    /// 契约：
    /// - 三张纹理分辨率**完全一致**，都等于 `data.width × data.height`；
    /// - 空地形（`width == 0` 或 `height == 0`）返回三个空缓冲、
    ///   尺寸为原样的 pack（不 panic）；
    /// - 同一份输入压两次**逐字节一致**（无随机、无时间依赖）；
    /// - `height` / `coast` 字节序为小端 `u16`；`attr` 字节序为
    ///   `R,G,B,A` 逐字节。
    ///
    /// 验收口径：T1 / T2 / T3（见模块文档）。
    pub fn from_terrain(data: &TerrainData) -> Self {
        let w = data.width as usize;
        let h = data.height as usize;

        // 空地形：明确返回全 0 尺寸的 pack，不做任何分配、不 panic。
        if w == 0 || h == 0 {
            return Self {
                width: data.width,
                height: data.height,
                height_tex: Vec::new(),
                coast_tex: Vec::new(),
                attr_tex: Vec::new(),
            };
        }

        let n = w * h;

        // 防御：`cells` 长度与 `width × height` 不符的畸形输入（只可能来自
        // 外部构造的 TerrainData）。补齐到 `n` 格（缺省格 = 水），
        // 保证后续索引不越界、coast_sdf 内部索引不 panic。
        let padded: Option<Vec<TerrainCellData>> = if data.cells.len() == n {
            None
        } else {
            let mut c = data.cells.clone();
            c.resize(n, TerrainCellData::default());
            Some(c)
        };
        let cell_at = |i: usize| -> TerrainCellData {
            match &padded {
                Some(p) => p[i],
                None => data.cells[i],
            }
        };

        // T3：复用邻居的精确 EDT（不重写距离变换）。全图同质时返回
        // `coast_sdf::FAR`，下面编码时钳到 1.0。
        let coast_d: Vec<f32> = match &padded {
            Some(p) => coast_sdf::distance_to_coast(&TerrainData {
                width: data.width,
                height: data.height,
                cells: p.clone(),
            }),
            None => coast_sdf::distance_to_coast(data),
        };

        let mut height_tex = Vec::with_capacity(n * 2);
        let mut coast_tex = Vec::with_capacity(n * 2);
        let mut attr_tex = Vec::with_capacity(n * 4);

        for i in 0..n {
            let cell = cell_at(i);

            // T1：高度归一化 + 钳位 + 四舍五入。
            let h_norm = (cell.height as f32 - H_MIN) / (H_MAX - H_MIN);
            height_tex.extend_from_slice(&encode_unorm16(h_norm).to_le_bytes());

            // T3：距离 → 归一化（d / 32）→ 钳位 → 四舍五入。
            let d = coast_d.get(i).copied().unwrap_or(coast_sdf::FAR);
            let coast_norm = d / COAST_MAX_CELLS;
            coast_tex.extend_from_slice(&encode_unorm16(coast_norm).to_le_bytes());

            // T2：R = 水陆（湿判定口径：surface 为水 **或** 海拔 < 0），
            // G = 裸 biome 字节（biome 是 ID，shader 侧必须 Nearest 采样，
            // 线性插值会插出不存在的 biome ID——自研地形最常见的翻车点），
            // B/A 预留为 0。
            let wet = is_wet(&cell);
            // R：**255 = 陆 / 0 = 水**（不是 1——Rgba8Unorm 采样后
            // 归一化成 `1/255 ≈ 0.0039`，shader 里 `r > 0.5` 会恒为假，
            // 于是整张图被当成水渲染。实测踩过这个「1 不等于 1.0」的坑）。
            attr_tex.push(if wet { 0 } else { 255 });
            attr_tex.push(cell.biome); // G：裸 biome
            attr_tex.push(0); // B：湿度预留
            attr_tex.push(0); // A：覆盖物键预留
        }

        Self {
            width: data.width,
            height: data.height,
            height_tex,
            coast_tex,
            attr_tex,
        }
    }

    /// T1 高度纹理的字节缓冲（小端 `u16`，行主序）。
    pub fn height_bytes(&self) -> &[u8] {
        &self.height_tex
    }

    /// T3 海岸距离纹理的字节缓冲（小端 `u16`，行主序）。
    pub fn coast_bytes(&self) -> &[u8] {
        &self.coast_tex
    }

    /// T2 属性纹理的字节缓冲（`R,G,B,A` 逐字节，行主序）。
    pub fn attr_bytes(&self) -> &[u8] {
        &self.attr_tex
    }
}

/// 湿判定（**全仓唯一入口**的等价封装）：`surface == SURFACE_WATER`
/// **或** `height < 0` 视为水。
///
/// 与 `wv_terrain::edit::is_water_cell` 逐条等价（这里直接调它，避免
/// 两份判定漂移）。
fn is_wet(cell: &TerrainCellData) -> bool {
    wv_terrain::edit::is_water_cell(wv_terrain::TerrainCell {
        height: cell.height,
        surface: cell.surface,
        biome: cell.biome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::{SURFACE_LAND, SURFACE_WATER};

    /// 造一张地形：先按 `land` 列表摆陆格（高度 +100），其余为水（高度 −100）。
    fn terrain(w: u32, h: u32, land: &[(u32, u32)]) -> TerrainData {
        let mut cells = vec![
            TerrainCellData {
                height: -100,
                surface: SURFACE_WATER,
                biome: 0,
            };
            (w * h) as usize
        ];
        for &(x, y) in land {
            cells[(y * w + x) as usize].surface = SURFACE_LAND;
            cells[(y * w + x) as usize].height = 100;
        }
        TerrainData {
            width: w,
            height: h,
            cells,
        }
    }

    /// 取某格的 T1 编码值（小端 `u16` 复原）。
    fn height_u16(pack: &FieldPack, x: u32, y: u32) -> u16 {
        let i = (y * pack.width + x) as usize * 2;
        u16::from_le_bytes([pack.height_tex[i], pack.height_tex[i + 1]])
    }

    /// 取某格的 T3 编码值（小端 `u16` 复原）。
    fn coast_u16(pack: &FieldPack, x: u32, y: u32) -> u16 {
        let i = (y * pack.width + x) as usize * 2;
        u16::from_le_bytes([pack.coast_tex[i], pack.coast_tex[i + 1]])
    }

    /// 取某格 attr 的某通道。
    fn attr_ch(pack: &FieldPack, x: u32, y: u32, ch: usize) -> u8 {
        let i = (y * pack.width + x) as usize * 4 + ch;
        pack.attr_tex[i]
    }

    // ── 1. 高度 round-trip：H_MIN → 0，H_MAX → 65535，0 → 期望值 ──────────
    #[test]
    fn height_encoding_round_trips_at_range_bounds() {
        let mut data = terrain(3, 1, &[]);
        data.cells[0].height = H_MIN as i16;
        data.cells[1].height = 0;
        data.cells[2].height = H_MAX as i16;
        let pack = FieldPack::from_terrain(&data);

        assert_eq!(height_u16(&pack, 0, 0), 0, "H_MIN 应编码为 0");
        assert_eq!(height_u16(&pack, 2, 0), 65535, "H_MAX 应编码为 65535");
        // 0 m：(0 − (−4000)) / (6000 − (−4000)) × 65535 = 0.4 × 65535 = 26214
        let want = (0.4f32 * 65535.0).round() as u16;
        assert_eq!(height_u16(&pack, 1, 0), want, "0 m 应编码为 26214");
    }

    /// 小端字节序断言：0.5 归一化 → `0x8000` → 字节 `[0x00, 0x80]`。
    #[test]
    fn height_encoding_uses_little_endian_byte_order() {
        // 造一个恰好 0.5 归一化的高度：h_norm = (h+4000)/10000 = 0.5 → h = 1000
        let mut data = terrain(1, 1, &[]);
        data.cells[0].height = 1000;
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(
            pack.height_tex,
            vec![0x00, 0x80],
            "0.5 归一化应为 0x8000，小端字节 [0x00, 0x80]"
        );
    }

    // ── 2. 超出量程被 clamp（不 panic）─────────────────────────────────────
    #[test]
    fn height_out_of_range_is_clamped_without_panic() {
        let mut data = terrain(2, 1, &[]);
        data.cells[0].height = 32767; // > H_MAX
        data.cells[1].height = -32768; // < H_MIN
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(height_u16(&pack, 0, 0), 65535, "32767 应钳到 65535");
        assert_eq!(height_u16(&pack, 1, 0), 0, "-32768 应钳到 0");
    }

    // ── 3. 全水图 → coast 全饱和（FAR 钳到 1.0 → 0xFFFF）──────────────────
    #[test]
    fn coast_far_value_is_clamped_to_full_scale() {
        let data = terrain(3, 3, &[]); // 全水
        let pack = FieldPack::from_terrain(&data);
        assert!(
            pack.coast_tex.iter().all(|&b| b == 0xFF),
            "全水图 coast 应全 0xFF"
        );
        assert!(pack.coast_tex.len().is_multiple_of(2));
        // 小端 0xFFFF → [0xFF, 0xFF]
        assert_eq!(&pack.coast_tex[0..2], &[0xFF, 0xFF]);
        for y in 0..3u32 {
            for x in 0..3u32 {
                assert_eq!(coast_u16(&pack, x, y), 65535);
            }
        }
    }

    // ── 4. coast 编码正确：d / 32 × 65535 ─────────────────────────────────
    #[test]
    fn coast_encoding_matches_distance_to_coast_result() {
        // 5×5：左 2 列水，右 3 列陆。
        let land: Vec<(u32, u32)> = (2..5).map(|x| (x, 0)).collect();
        let data = terrain(5, 5, &land);
        let d = coast_sdf::distance_to_coast(&data);
        let pack = FieldPack::from_terrain(&data);
        for y in 0..5u32 {
            for x in 0..5u32 {
                let i = (y * 5 + x) as usize;
                let want = (d[i] / COAST_MAX_CELLS).clamp(0.0, 1.0) * 65535.0;
                let want = want.round() as u16;
                assert_eq!(
                    coast_u16(&pack, x, y),
                    want,
                    "({x},{y}): d={} 编码不符",
                    d[i]
                );
            }
        }
        // 手算对照：水格 (1,0) 到最近陆格 (2,0) 距离 1 → 1/32×65535 ≈ 2048
        assert_eq!(
            coast_u16(&pack, 1, 0),
            (65535.0f32 / 32.0f32).round() as u16,
            "距离 1 格 → 1/32 × 65535"
        );
    }

    // ── 5. attr 的 R 通道是湿判定，而非裸 surface ─────────────────────────
    #[test]
    fn attr_red_channel_uses_wet_judgement_not_raw_surface() {
        let mut data = terrain(2, 1, &[]);
        // surface = 陆，但海拔 −50 → 湿判定为水。
        data.cells[0].surface = SURFACE_LAND;
        data.cells[0].height = -50;
        // surface = 水且海拔为正（异常但可能）→ 仍是水。
        data.cells[1].surface = SURFACE_WATER;
        data.cells[1].height = 200;
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(attr_ch(&pack, 0, 0, 0), 0, "陆但海拔<0 → R 应为 0（水）");
        assert_eq!(attr_ch(&pack, 1, 0, 0), 0, "surface=水 → R 应为 0（水）");
    }

    /// 对照：真正的陆格 R = 1。
    #[test]
    fn attr_red_channel_is_one_for_land() {
        let data = terrain(2, 1, &[(0, 0), (1, 0)]);
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(attr_ch(&pack, 0, 0, 0), 255, "陆地 → R=255");
        assert_eq!(attr_ch(&pack, 1, 0, 0), 255, "陆地 → R=255");
    }

    // ── 6. attr 的 G 通道存裸 biome 字节 ──────────────────────────────────
    #[test]
    fn attr_green_channel_stores_raw_biome_byte() {
        let mut data = terrain(2, 1, &[(0, 0), (1, 0)]);
        data.cells[0].biome = 7;
        data.cells[1].biome = 255;
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(attr_ch(&pack, 0, 0, 1), 7);
        assert_eq!(attr_ch(&pack, 1, 0, 1), 255);
    }

    /// B/A 预留通道为 0。
    #[test]
    fn attr_blue_and_alpha_channels_are_reserved_zero() {
        let data = terrain(2, 2, &[(0, 0)]);
        let pack = FieldPack::from_terrain(&data);
        for y in 0..2u32 {
            for x in 0..2u32 {
                assert_eq!(attr_ch(&pack, x, y, 2), 0, "B 应预留为 0");
                assert_eq!(attr_ch(&pack, x, y, 3), 0, "A 应预留为 0");
            }
        }
    }

    // ── 7. 三张纹理长度一致且分辨率 = 格数 ────────────────────────────────
    #[test]
    fn texture_lengths_match_cell_count() {
        let w = 7u32;
        let h = 5u32;
        let data = terrain(w, h, &[(1, 1), (3, 2)]);
        let pack = FieldPack::from_terrain(&data);
        let n = (w * h) as usize;
        assert_eq!(pack.width, w);
        assert_eq!(pack.height, h);
        assert_eq!(pack.height_tex.len(), n * 2);
        assert_eq!(pack.coast_tex.len(), n * 2);
        assert_eq!(pack.attr_tex.len(), n * 4);
        assert_eq!(pack.height_bytes().len(), n * 2);
        assert_eq!(pack.coast_bytes().len(), n * 2);
        assert_eq!(pack.attr_bytes().len(), n * 4);
    }

    // ── 8. 确定性：同一份数据压两次逐字节一致 ────────────────────────────
    #[test]
    fn packing_is_deterministic() {
        let data = terrain(6, 6, &[(0, 0), (2, 3), (5, 5)]);
        let a = FieldPack::from_terrain(&data);
        let b = FieldPack::from_terrain(&data);
        assert_eq!(a.height_tex, b.height_tex);
        assert_eq!(a.coast_tex, b.coast_tex);
        assert_eq!(a.attr_tex, b.attr_tex);
    }

    // ── 9. 行主序：2×2 逐格按 (x, y) 定位 ─────────────────────────────────
    #[test]
    fn attr_pixels_are_row_major() {
        let w = 2u32;
        let h = 2u32;
        let mut data = terrain(w, h, &[]);
        // 四格各自不同的 biome + 高度，便于定位。
        data.cells[0].biome = 10; // (0,0)
        data.cells[1].biome = 11; // (1,0)
        data.cells[2].biome = 20; // (0,1)
        data.cells[3].biome = 21; // (1,1)
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(attr_ch(&pack, 0, 0, 1), 10);
        assert_eq!(attr_ch(&pack, 1, 0, 1), 11);
        assert_eq!(attr_ch(&pack, 0, 1, 1), 20);
        assert_eq!(attr_ch(&pack, 1, 1, 1), 21);
        // 行主序直查：index = y * width + x，attr 每格 4 字节。
        // 用 `attr_ch` 助手（同一断言的语义写法）——直写下标会被 clippy 的
        // identity_op 判为「无效果的运算」，而 y * 4 的形式才看得懂意图。
        assert_eq!(attr_ch(&pack, 0, 0, 1), 10);
        assert_eq!(attr_ch(&pack, 1, 0, 1), 11);
        assert_eq!(attr_ch(&pack, 0, 1, 1), 20);
        assert_eq!(attr_ch(&pack, 1, 1, 1), 21);
    }

    // ── 10. 空地形 / width = 0：明确行为，不 panic ────────────────────────
    #[test]
    fn empty_terrain_yields_empty_pack() {
        let data = TerrainData {
            width: 0,
            height: 0,
            cells: Vec::new(),
        };
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(pack.width, 0);
        assert_eq!(pack.height, 0);
        assert!(pack.height_tex.is_empty());
        assert!(pack.coast_tex.is_empty());
        assert!(pack.attr_tex.is_empty());
    }

    /// `width = 0` 但 `height > 0`（退化输入）同样不 panic。
    #[test]
    fn zero_width_terrain_does_not_panic() {
        let data = TerrainData {
            width: 0,
            height: 4,
            cells: Vec::new(),
        };
        let pack = FieldPack::from_terrain(&data);
        assert_eq!(pack.width, 0);
        assert_eq!(pack.height, 4);
        assert!(pack.height_tex.is_empty());
        assert!(pack.coast_tex.is_empty());
        assert!(pack.attr_tex.is_empty());
    }
}
