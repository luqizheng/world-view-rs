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

/// 单格 → T1 高度 texel。**全量与增量共用**，否则两条路径的字节会漂。
fn encode_height(cell: &TerrainCellData) -> u16 {
    let h_norm = (cell.height as f32 - H_MIN) / (H_MAX - H_MIN);
    encode_unorm16(h_norm)
}

/// 单格 → T3 海岸 texel（编码随 [`CoastEncoding`]）。
///
/// `d` 是**无符号**的「到最近异类格距离」，符号由 `cell` 自身类别给出
/// ——与全量重打同一口径（spec 锁 T5：不用带符号 SDF 作为场本身）。
fn encode_coast(d: f32, cell: &TerrainCellData, encoding: CoastEncoding) -> u16 {
    let norm = match encoding {
        CoastEncoding::Unsigned => d / COAST_MAX_CELLS,
        CoastEncoding::Signed => {
            // `d ≥ 1`（岸线两侧的格心到对面都是 1），故有符号距离在岸线两侧
            // 分别是 `+1` / `−1`——**双线性插值穿过 0 的位置正是两格之间的
            // 交界**，这就是亚格连续性的来源。需先把 [−32, 32] 映射到 [0, 1]。
            let sd = if is_wet(cell) { -d } else { d };
            (sd.clamp(-COAST_MAX_CELLS, COAST_MAX_CELLS) + COAST_MAX_CELLS)
                / (2.0 * COAST_MAX_CELLS)
        }
    };
    encode_unorm16(norm)
}

/// 单格 → T2 属性 texel（`R,G,B,A`）。
///
/// R = 水陆（湿判定口径：surface 为水 **或** 海拔 < 0）；
/// G = 裸 biome 字节（biome 是 ID，shader 侧必须 Nearest 采样，
/// 线性插值会插出不存在的 biome ID——自研地形最常见的翻车点）；
/// B/A 预留为 0。
///
/// R 用 **255 = 陆 / 0 = 水**，不是 1——`Rgba8Unorm` 采样后归一化成
/// `1/255 ≈ 0.0039`，shader 里 `r > 0.5` 会恒为假、整张图被当成水渲染。
/// 实测踩过这个「1 不等于 1.0」的坑。
fn encode_attr(cell: &TerrainCellData) -> [u8; 4] {
    let wet = is_wet(cell);
    [if wet { 0 } else { 255 }, cell.biome, 0, 0]
}

/// 海岸距离纹理的编码方式。
///
/// 两种编码**占用同样的存储**（都是一张 `R16Unorm`），差别只在语义：
///
/// | | 0 的含义 | 岸线在哪里 | 交界的连续性 |
/// |---|---|---|---|
/// | `Unsigned`（现行，T3 原值） | 岸上（距离 0） | 两侧格心**都是 1**，中间平顶 | 符号来自 `attr.r`（Nearest）→ **逐格硬切** |
/// | `Signed`（实验） | 岸线本身 | 两侧格心分别是 `+1` / `−1` | 插值穿过 0 → **亚格连续** |
///
/// `Signed` 不是「更好的编码」这种口味问题，它解决两个可测的缺陷：
/// 1. 水陆交界在放大后是 1 格宽的正交台阶（`Unsigned` 的符号是 per-texel）；
/// 2. 岸线墨线**无法用距离场定位**——平顶使 `|d − 0.5|` 的最小值是 0.5，
///    永远大于线宽阈值（实测零像素）。
///
/// 代价：偏离已锁定的 T3/T5（T5 明写「不用带符号 SDF」）。故默认仍是
/// `Unsigned`，`Signed` 需显式选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoastEncoding {
    /// T3 原值：存无符号距离 `d / 32`。
    #[default]
    Unsigned,
    /// 实验：存有符号距离 `(sd + 32) / 64`，`sd > 0` = 陆。
    Signed,
}

/// 逻辑格坐标的**半开**矩形 `[x0, x1) × [y0, y1)`——增量补丁的作用域。
///
/// ## 为什么作用域是矩形
///
/// 三张纹理都是连续缓冲（`R16` / `Rgba8`），矩形能**按行**做连续写；逐格列表
/// 会退化成随机访问，且让 coast 的「窗口 EDT」没有明确输入。
///
/// 矩形合并只会**多算**，不会算错——多算的格同样从**当前**数据重算，结果与
/// 全量重打逐字节相同（见 [`FieldPack::patch_region`] 的一致性证明）。故上游
/// 给出「略大」的矩形是安全的，给出「略小」的矩形才会留下陈旧字节。
///
/// ## 半开约定
///
/// `TerrainTexture::mark_dirty` 的上游生产者一律按 `gx + 1` 给上界
/// （`stroke_bbox_in_cells` / `cells_diff_rect` / inspector 单格 / 群系分界线），
/// 本类型沿用该约定，不做「闭区间 + 1」的猜测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    /// 左边界（含）。
    pub x0: u32,
    /// 上边界（含）——行 0 = `TerrainData.cells` 的第 0 行。
    pub y0: u32,
    /// 右边界（**不含**）。
    pub x1: u32,
    /// 下边界（**不含**）。
    pub y1: u32,
}

impl CellRect {
    /// 直接构造（不做规范化，越界交给 [`Self::intersect`]）。
    pub fn new(x0: u32, y0: u32, x1: u32, y1: u32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// 从 `TerrainTexture::mark_dirty` 的 `[x0, y0, x1, y1]` 格式构造。
    pub fn from_bounds(bounds: [u32; 4]) -> Self {
        Self::new(bounds[0], bounds[1], bounds[2], bounds[3])
    }

    /// 空矩形（含反向输入 `x1 < x0`）。
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// 列数（空矩形为 0；反向输入不会下溢）。
    pub fn width(&self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    /// 行数（空矩形为 0；反向输入不会下溢）。
    pub fn height(&self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }

    /// 格数（空矩形为 0；反向输入不会下溢）。
    pub fn area(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            u64::from(self.x1 - self.x0) * u64::from(self.y1 - self.y0)
        }
    }

    /// 与 `w × h` 幅面求交（越界部分丢弃）。
    pub fn intersect(&self, w: u32, h: u32) -> Self {
        let r = Self::new(
            self.x0.min(w),
            self.y0.min(h),
            self.x1.min(w),
            self.y1.min(h),
        );
        if r.is_empty() {
            // 归一成 (0,0,0,0)：让 `is_empty` 之外的调用方也拿到无歧义的空盒。
            Self::new(0, 0, 0, 0)
        } else {
            r
        }
    }

    /// 各向外扩 `pad` 格后再与幅面求交（**EDT 窗口**用，见 [`FieldPack::patch_region`]）。
    pub fn expand(&self, pad: u32, w: u32, h: u32) -> Self {
        Self::new(
            self.x0.saturating_sub(pad),
            self.y0.saturating_sub(pad),
            self.x1.saturating_add(pad),
            self.y1.saturating_add(pad),
        )
        .intersect(w, h)
    }
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
    /// 海岸纹理用的编码——shader 侧必须按同一约定解码。
    pub encoding: CoastEncoding,
    /// T1：height 场，`R16Unorm` 字节序（小端 `u16`），线性采样。
    /// 归一化 `h_norm = (h − H_MIN) / (H_MAX − H_MIN)`。
    pub height_tex: Vec<u8>,
    /// T3：coast_dist 场，`R16Unorm` 字节序（小端 `u16`），线性采样。
    /// 编码随 [`Self::encoding`]：`d / 32` 或 `(sd + 32) / 64`。
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
        Self::from_terrain_with(data, CoastEncoding::default())
    }

    /// 同 [`Self::from_terrain`]，可指定海岸编码（见 [`CoastEncoding`]）。
    pub fn from_terrain_with(data: &TerrainData, encoding: CoastEncoding) -> Self {
        let w = data.width as usize;
        let h = data.height as usize;

        // 空地形：明确返回全 0 尺寸的 pack，不做任何分配、不 panic。
        if w == 0 || h == 0 {
            return Self {
                width: data.width,
                height: data.height,
                encoding,
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
            height_tex.extend_from_slice(&encode_height(&cell).to_le_bytes());

            // T3：距离 → 归一化（`d / 32` 或带符号映射）→ 钳位 → 四舍五入。
            //
            // 两种编码（见 [`CoastEncoding`]）：
            // - **Unsigned（现行，T3 原值）**：存「到最近异类格的距离」，0 = 岸上。
            // - **Signed（实验）**：存**有符号**距离，0 = 岸线本身，正 = 陆、负 = 水。
            let d = coast_d.get(i).copied().unwrap_or(coast_sdf::FAR);
            coast_tex.extend_from_slice(&encode_coast(d, &cell, encoding).to_le_bytes());

            // T2：R = 水陆（湿判定口径），G = 裸 biome 字节，B/A 预留为 0。
            attr_tex.extend_from_slice(&encode_attr(&cell));
        }

        Self {
            width: data.width,
            height: data.height,
            encoding,
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

    /// **增量补丁**：按 `data` 重算本次编辑影响到的 texel，结果与
    /// [`Self::from_terrain_with`] 全量重打**逐字节一致**（整张缓冲，不只是脏区）。
    ///
    /// 返回 `false` = 本 pack 不能用于增量（幅面不符 / 缓冲长度不符），
    /// 调用方**必须**改走全量重打。
    ///
    /// ## 三张纹理的传播半径不一样（本函数唯一的难点）
    ///
    /// | 纹理 | 回写范围 | 理由 |
    /// |---|---|---|
    /// | `height` | `rect` | 每 texel 只由自己的 `height` 决定 |
    /// | `attr` | `rect` | 每 texel 只由自己的 `surface` / `biome` 决定 |
    /// | `coast` | **`rect ⊕ 32`** | EDT 把「岸线搬家」传播出去，但编码在 32 格饱和 |
    ///
    /// 只回写 `rect` 是**错的**（第一版这么写，被单测抓住）：改一格地表，
    /// 周围 32 格内的海岸距离全变；只补 rect 会让岸线色带（T7–T9）与岸线墨线
    /// （T11）在编辑点附近残留旧值，直到下一次全量重打。
    ///
    /// ## 为什么回写半径 32、求值窗口半径 64
    ///
    /// **回写 32 格足够**：某格的编码字节会变 ⇒ 它的新旧最近异类格至少有一个
    /// 落在 `rect` 内（否则这次编辑没碰它的种子集），且只有 `d < 32` 才可能改变
    /// 字节（`d ≥ 32` 两种情形都饱和到 `0xFFFF`）⇒ 该格必在 `rect ⊕ 32` 内。
    /// 故 `rect ⊕ 32` 之外**没有任何 texel 会变**，跳过它们不影响与全量的等价。
    ///
    /// **窗口 64 格是回写精确性的前提**：回写区内某格的候选种子在它 32 格内，
    /// 而该格又在 `rect ⊕ 32` 内 ⇒ 种子必在 `rect ⊕ 64` 内。窗口取 64 才保证
    /// 「窗口 EDT = 全图 EDT」，不会在窗口边缘出现「种子被裁掉导致距离偏大」
    /// 的假值（这正是「窗口半径 = 回写半径」那个版本翻车的地方）。
    ///
    /// 代价：窗口格数 ≈ `(w + 128) × (h + 128)`。一笔 brush（半径 5 格、脏区
    /// ~12²）约 140×138 ≈ 19k 格，**亚毫秒**；全量重打是 1024² ≈ 1M 格
    /// （实测 48.4 ms，其中 ~40 ms 是 EDT）。
    pub fn patch_region(&mut self, data: &TerrainData, rect: CellRect) -> bool {
        let (w, h) = (self.width, self.height);
        if w != data.width || h != data.height {
            return false;
        }
        let n = (w as usize) * (h as usize);
        if data.cells.len() != n
            || self.height_tex.len() != n * 2
            || self.coast_tex.len() != n * 2
            || self.attr_tex.len() != n * 4
        {
            return false;
        }
        let r = rect.intersect(w, h);
        // 空矩形：无事可做，但这是「合法且已完成」而不是「不能增量」。
        if r.is_empty() {
            return true;
        }

        // ── height / attr：严格局部，按行连续写 ──
        //
        // 每行是一段连续字节，`copy_from_slice` 一次搬 `(x1−x0) × bpp`；
        // 逐格 `extend` 在这里既慢又容易写错步长。
        let cols = (r.x1 - r.x0) as usize;
        for y in r.y0..r.y1 {
            let row0 = (y as usize) * (w as usize);
            let mut h_row = Vec::with_capacity(cols * 2);
            let mut a_row = Vec::with_capacity(cols * 4);
            for x in r.x0..r.x1 {
                let cell = data.cells[row0 + x as usize];
                h_row.extend_from_slice(&encode_height(&cell).to_le_bytes());
                a_row.extend_from_slice(&encode_attr(&cell));
            }
            let h_at = (row0 + r.x0 as usize) * 2;
            self.height_tex[h_at..h_at + cols * 2].copy_from_slice(&h_row);
            let a_at = (row0 + r.x0 as usize) * 4;
            self.attr_tex[a_at..a_at + cols * 4].copy_from_slice(&a_row);
        }

        // ── coast：窗口 EDT（回写 `rect ⊕ 32`，窗口 `rect ⊕ 64`，见 doc 的证明）──
        let sat = COAST_MAX_CELLS as u32;
        let out = r.expand(sat, w, h); // 回写区 = 一切会变的 texel 的超集
        let win = r.expand(sat * 2, w, h); // 求值窗口 = 保证回写区精确
        let (ww, wh) = (win.x1 - win.x0, win.y1 - win.y0);
        let mut cells = Vec::with_capacity((ww as usize) * (wh as usize));
        for y in win.y0..win.y1 {
            let row0 = (y as usize) * (w as usize);
            cells.extend_from_slice(&data.cells[row0 + win.x0 as usize..row0 + win.x1 as usize]);
        }
        let window_data = TerrainData {
            width: ww,
            height: wh,
            cells,
        };
        let coast_d = coast_sdf::distance_to_coast(&window_data);
        let encoding = self.encoding;
        let out_cols = (out.x1 - out.x0) as usize;
        for y in out.y0..out.y1 {
            let mut c_row = Vec::with_capacity(out_cols * 2);
            for x in out.x0..out.x1 {
                let wi = ((y - win.y0) as usize) * (ww as usize) + (x - win.x0) as usize;
                let cell = data.cells[(y as usize) * (w as usize) + x as usize];
                c_row.extend_from_slice(&encode_coast(coast_d[wi], &cell, encoding).to_le_bytes());
            }
            let row0 = (y as usize) * (w as usize);
            let at = (row0 + out.x0 as usize) * 2;
            self.coast_tex[at..at + out_cols * 2].copy_from_slice(&c_row);
        }

        true
    }
}

/// 数据纹理的**内容指纹**（FNV-1a，进程内比较用，不落盘）。
///
/// ## 为什么不能复用 `isoline_overlay::terrain_data_hash`
/// 那个指纹折叠的是 `biome::display_kind(...)`（三通道 → 0..9 的**显示槽位**），
/// 因为等值线只关心「视觉上是不是同一类」。但 [`FieldPack`] 的 `attr` 纹理
/// **G 通道存的是裸 `biome` 字节**（shader 侧 Nearest 采样，见
/// [`FieldPack::from_terrain_with`] 的 T2 注释）——两个不同的 biome 完全可能
/// 落在同一个 display_kind 里。若拿显示槽位指纹当「数据纹理要不要重打」的判据，
/// 就会出现**指纹相同但打包结果不同**：地形改了，画面不更新，且没有任何报错。
///
/// 所以这里按**打包真正消费的字段**逐个折叠：`height` / `surface` / `biome`
/// 原值 + 幅面。判据与产物同源，这是唯一可靠的做法。
///
/// `surface` 参与折叠的理由：`is_wet` 用 `surface == 水 ∨ height < 0`，改
/// `surface` 会改变 R 通道与海岸距离场，即使 `height` 与 `biome` 都没动。
pub fn pack_content_hash(data: &TerrainData) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = FNV_OFFSET;
    for c in &data.cells {
        h ^= (c.height as u16) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= c.surface as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= c.biome as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    // 幅面折进尾部：同序列不同形状不共享指纹。
    h ^= data.width as u64;
    h = h.wrapping_mul(FNV_PRIME);
    h ^= (data.height as u64) << 32;
    h.wrapping_mul(FNV_PRIME)
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

    // ── 有符号海岸编码（CoastEncoding::Signed）────────────────────────────

    /// 有符号编码在岸线两侧必须是 `+1` / `−1`——这是「双线性插值穿过 0」
    /// 的前提。若写成 `±(d−0.5)`，两侧会变成 `±0.5`，插值零点落在格边上，
    /// 又退回逐格硬切。
    #[test]
    fn signed_encoding_puts_opposite_signs_on_both_sides() {
        // 2 格一行：0 = 陆，1 = 水。两格到对面都是 1。
        let data = row_data(&[true, false]);
        let pack = FieldPack::from_terrain_with(&data, CoastEncoding::Signed);
        let land = decode_coast(&pack, 0) as f64 / 65535.0;
        let water = decode_coast(&pack, 1) as f64 / 65535.0;

        // 解码：(raw − 0.5) × 64
        let sd_land = (land - 0.5) * 64.0;
        let sd_water = (water - 0.5) * 64.0;
        assert!(
            (sd_land - 1.0).abs() < 1e-3,
            "陆地侧应为 +1 格，实得 {sd_land}"
        );
        assert!(
            (sd_water + 1.0).abs() < 1e-3,
            "水域侧应为 −1 格，实得 {sd_water}"
        );

        // 关键性质：两格之间线性插值必然穿过 0（符号相反即可保证）。
        assert!(sd_land > 0.0 && sd_water < 0.0);
    }

    /// 有符号编码下，全水地图解码后应饱和在 `−32`（而不是 0）。
    #[test]
    fn signed_encoding_saturates_homogeneous_water() {
        let data = row_data(&[false, false, false]);
        let pack = FieldPack::from_terrain_with(&data, CoastEncoding::Signed);
        for i in 0..3 {
            let raw = decode_coast(&pack, i) as f64 / 65535.0;
            let sd = (raw - 0.5) * 64.0;
            assert!((sd + 32.0).abs() < 1e-2, "全水应饱和到 −32，实得 {sd}");
        }
    }

    /// 两种编码的存储开销完全一致（都是 `w×h×2` 字节）——signed 不是
    /// 「更贵的编码」，这是它可以被考虑的前提。
    #[test]
    fn both_encodings_use_identical_bytes() {
        let data = row_data(&[true, false, true, false]);
        let u = FieldPack::from_terrain_with(&data, CoastEncoding::Unsigned);
        let s = FieldPack::from_terrain_with(&data, CoastEncoding::Signed);
        assert_eq!(u.coast_tex.len(), s.coast_tex.len());
        assert_eq!(
            u.coast_tex.len(),
            data.width as usize * data.height as usize * 2
        );
        assert_eq!(u.encoding, CoastEncoding::Unsigned);
        assert_eq!(s.encoding, CoastEncoding::Signed);
    }

    /// `from_terrain` 必须等价于 `from_terrain_with(.., Unsigned)`——
    /// 默认路径不能因为加了变体而悄悄改变。
    #[test]
    fn from_terrain_defaults_to_unsigned() {
        let data = row_data(&[true, false]);
        let a = FieldPack::from_terrain(&data);
        let b = FieldPack::from_terrain_with(&data, CoastEncoding::Unsigned);
        assert_eq!(a.encoding, CoastEncoding::Unsigned);
        assert_eq!(a.coast_tex, b.coast_tex);
    }

    // ── 测试辅助 ─────────────────────────────────────────────────────────

    /// 造一行地形（`land[i]` 为真 = 陆）。高度随水陆给固定值。
    fn row_data(land: &[bool]) -> TerrainData {
        let cells = land
            .iter()
            .map(|&l| TerrainCellData {
                height: if l { 200 } else { -800 },
                surface: u8::from(l),
                biome: if l { 2 } else { 15 },
            })
            .collect();
        TerrainData {
            width: land.len() as u32,
            height: 1,
            cells,
        }
    }

    /// 取第 `i` 格的海岸纹理 u16 原始值。
    fn decode_coast(pack: &FieldPack, i: usize) -> u16 {
        let k = i * 2;
        u16::from_le_bytes([pack.coast_tex[k], pack.coast_tex[k + 1]])
    }

    /// 内容指纹必须与**打包真正消费的字段**同源（`height` / `surface` / `biome` 原值）。
    ///
    /// 这条测试的存在理由不是「函数能跑」，而是钉住一个**已经踩过的坑**：
    /// `isoline_overlay::terrain_data_hash` 折叠的是 `display_kind`（显示槽位），
    /// 而 attr 纹理 G 通道存裸 biome——若拿显示槽位指纹当「要不要重打」的判据，
    /// 就会出现「地形改了、画面不更新、且没有任何报错」。
    #[test]
    fn pack_content_hash_tracks_the_channels_the_pack_actually_consumes() {
        let base = TerrainData {
            width: 2,
            height: 1,
            cells: vec![
                TerrainCellData {
                    height: 100,
                    surface: 1,
                    biome: 3,
                },
                TerrainCellData {
                    height: -5,
                    surface: 0,
                    biome: 1,
                },
            ],
        };
        let same = TerrainData {
            cells: base.cells.clone(),
            ..base.clone()
        };
        assert_eq!(
            pack_content_hash(&base),
            pack_content_hash(&same),
            "同内容必须同指纹（否则每次稳态都重打）"
        );

        let mut biome_only = base.clone();
        biome_only.cells[0].biome = 4; // 只改裸 biome 字节
        assert_ne!(
            pack_content_hash(&base),
            pack_content_hash(&biome_only),
            "biome 改 → 指纹必须变（attr 纹理 G 通道会变）"
        );

        let mut surface_only = base.clone();
        surface_only.cells[0].surface = 0; // 只改 surface（wet 判定与海岸场都会变）
        assert_ne!(
            pack_content_hash(&base),
            pack_content_hash(&surface_only),
            "surface 改 → 指纹必须变（R 通道与海岸距离场会变）"
        );

        let mut height_only = base.clone();
        height_only.cells[1].height = -6;
        assert_ne!(
            pack_content_hash(&base),
            pack_content_hash(&height_only),
            "height 改 → 指纹必须变（height/coast 两张纹理都会变）"
        );

        // 幅面折进指纹：同序列不同形状不得共享。
        let reshaped = TerrainData {
            width: 1,
            height: 2,
            cells: base.cells.clone(),
        };
        assert_ne!(
            pack_content_hash(&base),
            pack_content_hash(&reshaped),
            "幅面改 → 指纹必须变"
        );
    }

    // ══════════ 增量补丁（T6）：一致性 = 与全量重打逐字节相同 ══════════

    /// 造一张**带海岸线**的地形：中心一块实心岛 + 一条斜向的陆脊。
    ///
    /// 必须同时存在「大片同质区」（距离 > 32、编码饱和）与「密集岸线」
    /// （距离 1–3 格），否则窗口 EDT 的两种失效模式各测不到一半。
    fn island(w: u32, h: u32) -> TerrainData {
        let mut cells = vec![
            TerrainCellData {
                height: -100,
                surface: SURFACE_WATER,
                biome: 0,
            };
            (w * h) as usize
        ];
        for y in 0..h {
            for x in 0..w {
                let inner = x >= w / 4 && x < w * 3 / 4 && y >= h / 4 && y < h * 3 / 4;
                let ridge = (x as i64 - y as i64).rem_euclid(11) < 2 && x > 2 && y > 2;
                if inner || ridge {
                    let i = (y * w + x) as usize;
                    cells[i].surface = SURFACE_LAND;
                    cells[i].height = 100 + (x % 7) as i16;
                    cells[i].biome = (x % 5) as u8;
                }
            }
        }
        TerrainData {
            width: w,
            height: h,
            cells,
        }
    }

    /// 补丁 vs 全量：三张缓冲**整张**逐字节比对。
    ///
    /// 这是 `patch_region` 唯一的验收口径，也是它比「只比脏区」强得多的地方：
    /// 整张相等意味着「补丁路径与全量路径视觉上不可区分」。任何窗口开小了
    /// 的实现（第一版把回写半径取成 0）都会被海岸距离场当场抓住。
    fn assert_patch_matches_full(encoding: CoastEncoding, w: u32, h: u32, rect: CellRect) {
        // 越界矩形先裁一次：改动点必须落在**会真的被補丁覆盖**的地方。
        let r = rect.intersect(w, h);
        for seed in 0..6u32 {
            let base = island(w, h);
            let before = FieldPack::from_terrain_with(&base, encoding);

            let mut after = base.clone();
            // 在矩形内改一格：高度 / 地表 / 群系三件都动，覆盖三张纹理各自的
            // 输入（`surface` 改 = 岸线搬家，是 coast 的最强扰动）。
            if !r.is_empty() {
                let cx = r.x0 + (seed * 3) % r.width();
                let cy = r.y0 + (seed * 5) % r.height();
                let i = (cy * w + cx) as usize;
                after.cells[i].height = if seed % 2 == 0 { 2500 } else { -3000 };
                after.cells[i].surface = if after.cells[i].surface == SURFACE_WATER {
                    SURFACE_LAND
                } else {
                    SURFACE_WATER
                };
                after.cells[i].biome = (seed + 3) as u8;
            }

            let full = FieldPack::from_terrain_with(&after, encoding);
            let mut patched = before.clone();
            assert!(
                patched.patch_region(&after, rect),
                "{encoding:?} 矩形 {rect:?} 应可增量"
            );

            let ctx = format!("{encoding:?} {w}×{h} rect={rect:?} seed={seed}");
            assert_eq!(patched.height_tex, full.height_tex, "height 不一致：{ctx}");
            assert_eq!(patched.attr_tex, full.attr_tex, "attr 不一致：{ctx}");
            assert_eq!(patched.coast_tex, full.coast_tex, "coast 不一致：{ctx}");
        }
    }

    #[test]
    fn patch_region_matches_full_repack_on_interior_rect() {
        assert_patch_matches_full(
            CoastEncoding::Unsigned,
            64,
            48,
            CellRect::new(20, 14, 26, 19),
        );
        assert_patch_matches_full(CoastEncoding::Signed, 64, 48, CellRect::new(20, 14, 26, 19));
    }

    #[test]
    fn patch_region_matches_full_repack_when_rect_touches_borders() {
        for rect in [
            CellRect::new(0, 0, 5, 4),     // 左上角（窗口被幅面裁掉一半）
            CellRect::new(58, 0, 64, 6),   // 右上角
            CellRect::new(0, 40, 7, 48),   // 左下角
            CellRect::new(50, 36, 64, 48), // 右下角
            CellRect::new(0, 20, 64, 24),  // 横贯整幅的一行带
            CellRect::new(30, 0, 34, 48),  // 纵贯整幅的一列带
        ] {
            assert_patch_matches_full(CoastEncoding::Unsigned, 64, 48, rect);
        }
    }

    #[test]
    fn patch_region_matches_full_repack_for_the_whole_map() {
        // 窗口 = 幅面 ⇒ 退化成全量 EDT，任何编码都必须一致。
        assert_patch_matches_full(CoastEncoding::Unsigned, 40, 30, CellRect::new(0, 0, 40, 30));
        assert_patch_matches_full(CoastEncoding::Signed, 40, 30, CellRect::new(0, 0, 40, 30));
    }

    #[test]
    fn patch_region_clamps_out_of_bounds_rects() {
        // 越界矩形：裁到幅面后依然逐字节正确（不能 panic、不能少写）。
        assert_patch_matches_full(
            CoastEncoding::Unsigned,
            32,
            24,
            CellRect::new(28, 20, 999, 999),
        );
    }

    #[test]
    fn patch_region_on_empty_rect_touches_nothing() {
        let base = island(32, 24);
        let pack = FieldPack::from_terrain(&base);
        let snapshot = (
            pack.height_tex.clone(),
            pack.coast_tex.clone(),
            pack.attr_tex.clone(),
        );

        let mut patched = pack.clone();
        assert!(
            patched.patch_region(&base, CellRect::new(5, 5, 5, 9)),
            "空矩形是合法输入，返回 true"
        );
        assert_eq!(
            (
                patched.height_tex.clone(),
                patched.coast_tex.clone(),
                patched.attr_tex.clone()
            ),
            snapshot
        );
    }

    #[test]
    fn patch_region_refuses_mismatched_dimensions() {
        let base = island(32, 24);
        let mut pack = FieldPack::from_terrain(&base);
        let other = island(16, 12);
        assert!(
            !pack.patch_region(&other, CellRect::new(0, 0, 4, 4)),
            "幅面不符必须拒绝，调用方改走全量重打"
        );
    }

    #[test]
    fn patch_region_refuses_ragged_cells() {
        // 畸形输入（cells 长度与幅面不符）：全量路径会补齐，增量路径**不能**
        // 猜——直接拒绝，由调用方走全量。
        let base = island(16, 12);
        let mut pack = FieldPack::from_terrain(&base);
        let mut ragged = base.clone();
        ragged.cells.truncate(10);
        assert!(!pack.patch_region(&ragged, CellRect::new(0, 0, 4, 4)));
    }

    #[test]
    fn patch_region_is_far_cheaper_than_full_repack() {
        // 计数器口径（不用墙钟）：增量只付「回写区 + 求值窗口」这两块格数。
        // 用格数这个**确定性**量对比，跨机器同值。
        let (w, h) = (1024u32, 1024u32);
        let data = island(w, h);
        // 一笔半径 5 格的 brush 的典型脏区。
        let rect = CellRect::new(500, 500, 512, 510);
        let sat = COAST_MAX_CELLS as u32;
        let out = rect.expand(sat, w, h);
        let win = rect.expand(sat * 2, w, h);
        assert_eq!(out.area(), (12 + 64) as u64 * (10 + 64) as u64);
        assert_eq!(win.area(), (12 + 128) as u64 * (10 + 128) as u64);
        let full = u64::from(w) * u64::from(h);
        assert!(
            win.area() * 50 < full,
            "一笔 brush 的求值窗口必须远小于全场（{}/{} = {:.2}%）",
            win.area(),
            full,
            win.area() as f64 / full as f64 * 100.0
        );
        // 一致性仍然成立（这条不是成本断言，是正确性断言）。
        let mut patched = FieldPack::from_terrain(&data);
        let mut after = data.clone();
        after.cells[505 * 1024 + 506].height = 900;
        assert!(patched.patch_region(&after, rect));
        assert_eq!(
            patched.coast_tex,
            FieldPack::from_terrain(&after).coast_tex,
            "1024² 大图上补丁仍须与全量重打逐字节一致"
        );
    }

    #[test]
    fn cell_rect_intersect_and_expand_are_half_open() {
        let r = CellRect::new(4, 6, 10, 12);
        assert_eq!(r.area(), 6 * 6);
        assert!(!r.is_empty());
        assert_eq!(r.intersect(8, 8), CellRect::new(4, 6, 8, 8));
        assert!(r.intersect(4, 4).is_empty(), "完全在外 → 空");
        // 反向输入不算「负面积」。
        assert_eq!(CellRect::new(9, 9, 3, 3).area(), 0);
        assert!(CellRect::new(9, 9, 3, 3).is_empty());
        // 外扩后裁到幅面。
        assert_eq!(
            CellRect::new(1, 1, 3, 3).expand(2, 10, 10),
            CellRect::new(0, 0, 5, 5)
        );
        assert_eq!(
            CellRect::new(1, 1, 3, 3).expand(2, 4, 4),
            CellRect::new(0, 0, 4, 4)
        );
    }
}
