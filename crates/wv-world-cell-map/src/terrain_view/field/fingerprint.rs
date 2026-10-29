//! 编辑帧分块指纹（ADR-0088 D3）：把场路径的内容指纹从「每编辑帧全量扫
//! 1M 格 ≈3 ms」改成「256² 分块缓存 + 脏块增量重算」（<0.5 ms）。
//!
//! ## 做什么
//!
//! [`super::pack::pack_content_hash`] 每次都对整幅地形逐格折叠 FNV-1a——
//! 增量补丁把**重打包**省到亚毫秒后，指纹成了编辑帧剩下的最大头（1024²
//! 实测 ≈3 ms）。本模块把指纹拆成 [`FP_BLOCK]² 格一块的**块 hash 缓存**
//! （[`BlockFingerprint`]）：
//!
//! - [`BlockFingerprint::new_full`]：全量建（与旧全量指纹同价，一次性）；
//! - [`BlockFingerprint::update_region`]：编辑帧只重算 `field_dirty` 矩形
//!   覆盖到的块——一笔 brush 的脏区 ~12² 格，重算量从 1M 格跌到千格级；
//! - [`BlockFingerprint::total`]：按块行序把块 hash 序贯折叠成一个 `u64`，
//!   填进 wv-render 侧 `FieldKey.terrain_hash` 槽（重打判据**一行不动**，
//!   N1 教训：只比内容，不比通知）。
//!
//! ## 等价判据（为什么不是「与旧 hash 值相等」）
//!
//! FNV 是**链式**的：全量 hash 的每个中间位都依赖前面所有格。要「增量值
//! ≡ 旧全量值」就得保留全链依赖，就得放弃块缓存——两者数学上互斥（ADR-0088
//! 「验收口径修正」）。故判据钉两条，**敏感性等价**而非值等价：
//!
//! 1. **自身一致**：随机编辑序列下「增量 `update_region` 后的 `total()`」
//!    与「`new_full` 全量重建的 `total()`」逐位相等（property 测试）；
//! 2. **敏感性对拍**：同一编辑序列下「新 total 是否翻转」与旧
//!    `pack_content_hash` 是否翻转逐轮一致——编辑必被看见、未编辑必不误报。
//!
//! 值域切换的一次性后果只是迁移瞬间 `terrain_hash` 对不上、当帧触发一次
//! 全量重打包（用户无感）。
//!
//! ## 与 `pack_content_hash` 的口径关系
//!
//! 块 hash 折叠的字段与顺序**照抄** [`super::pack::pack_content_hash`]：
//! 每格 `height`（u16 零扩展）/ `surface` / `biome` 三字段序贯 FNV-1a——
//! 打包真正消费的字段（attr 的 R 湿判定读 surface、G 读裸 biome；height/coast
//! 读 height），判据必须与产物同源（该函数文档里记着 `display_kind` 那次漏打）。
//! `pack_content_hash` 保留 pub，职责收窄为**对账基准**（见其文档注记）。
//!
//! ## `update_region` 的前置条件（结构一致性）
//!
//! 缓存的格–块映射必须与传入数据同幅面。不同幅面 = 结构已变（换图 / 同图
//! 换栅格），旧块账本对新结构没有意义——就地补一个矩形只会留下「半新半旧」
//! 的静默错账，故**自愈为等价的全量重建**（本函数无返回值可拒，与
//! `FieldPack::patch_region`「幅面不符必须拒绝、调用方改走全量」同一纪律的
//! 无返回值变体）。调用方（wv-render `sync_terrain_field`）另在
//! `structure_changed`（换图 / 换分支 / 换色板）时**整份丢弃**缓存。
//!
//! ## 脏矩形的来源纪律
//!
//! 与 GPU 补丁共用 `TerrainTexture::field_dirty` 的**同一个**矩形（一次取用、
//! 两处消费，ADR-0088 D3）——不引入第二份脏区追踪。该矩形自带编码裕量
//! （coast ±32 / biome ±8 的外扩由上游写入时决定），指纹重算「多算不错算」：
//! 矩形盖到的块一律从**当前**数据重算，与全量重建逐位一致。

use wv_core::terrain::{TerrainCellData, TerrainData};

/// 分块边长（格）。256² = 65536 格/块；1024² 图 = 4×4 = 16 块、4096² 图 =
/// 16×16 = 256 块——块数远小于格数，`total()` 的折叠成本可忽略。
///
/// 取 256 的理由：一笔 brush 的典型脏区（~12² 格）永远落在 1–4 个块内，
/// 增量重算量与块粒度的权衡在 256 处最划算（更小 → 块多、total 折叠贵、
/// 缓存条目碎；更大 → 单块重算贵）。
pub const FP_BLOCK: u32 = 256;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 编辑帧地形内容指纹的**分块缓存**（ADR-0088 D3）。
///
/// 会话态运行时结构（不落盘、不进任何序列化格式——P3/P6/P7 原样），
/// 消费方只有 wv-render 的 `sync_terrain_field`：`total()` 折叠值填
/// `FieldKey.terrain_hash`。
///
/// 块 hash 按块**行序**存（`index = by * blocks_x + bx`，与地形数据的
/// 行主序同构）；`grid_w/grid_h` 记录建缓存时的幅面，供 [`Self::update_region`]
/// 做结构一致性守卫（见模块文档「前置条件」）。
pub struct BlockFingerprint {
    /// 各块的 FNV-1a hash（行序）。
    block_hashes: Vec<u64>,
    /// 横向块数（`ceil(grid_w / FP_BLOCK)`）。
    blocks_x: u32,
    /// 纵向块数（`ceil(grid_h / FP_BLOCK)`）。
    blocks_y: u32,
    /// 建缓存时的地形幅面（宽）——结构一致性的守卫依据。
    grid_w: u32,
    /// 建缓存时的地形幅面（高）——结构一致性的守卫依据。
    grid_h: u32,
}

impl BlockFingerprint {
    /// 全量建：对整幅地形逐块算 hash（成本与旧全量指纹同价，≈3 ms @1M 格，
    /// 每个编辑帧只该付一次的那个「首次 / 结构变更 / 无可信脏区」价）。
    ///
    /// 畸形输入（`cells` 长度与幅面不符）按缺省格补齐再建——与
    /// `FieldPack::from_terrain` 的 padding 同款防御，保证索引不越界。
    pub fn new_full(data: &TerrainData) -> Self {
        let (w, h) = (data.width, data.height);
        let n = w as usize * h as usize;
        if data.cells.len() != n {
            let mut cells = data.cells.clone();
            cells.resize(n, TerrainCellData::default());
            return Self::new_full(&TerrainData {
                width: w,
                height: h,
                cells,
            });
        }
        let blocks_x = w.div_ceil(FP_BLOCK);
        let blocks_y = h.div_ceil(FP_BLOCK);
        let mut block_hashes = Vec::with_capacity((blocks_x * blocks_y) as usize);
        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                block_hashes.push(block_hash(data, bx, by));
            }
        }
        Self {
            block_hashes,
            blocks_x,
            blocks_y,
            grid_w: w,
            grid_h: h,
        }
    }

    /// 增量重算：只更新 `rect`（格坐标**半开**区间 `[x0,y0,x1,y1)`，与
    /// `TerrainTexture::field_dirty` 同约定）覆盖到的块；越界部分钳制，
    /// 空矩形 = 合法无事。
    ///
    /// ## 前置条件：结构一致（见模块文档）
    ///
    /// 缓存幅面与 `data` 幅面不符时**自愈为全量重建**——绝不就地补丁
    /// （半新半旧的错账比慢更糟：指纹漏翻 → 画面陈旧且无报错）。
    ///
    /// 块覆盖判定：格 → 块 = 坐标 / [`FP_BLOCK`]；半开上界的最后一个格是
    /// `x1−1`，其所在块即需重算的最后一块。
    pub fn update_region(&mut self, data: &TerrainData, rect: [u32; 4]) {
        let blocks_x = data.width.div_ceil(FP_BLOCK);
        let blocks_y = data.height.div_ceil(FP_BLOCK);
        if self.grid_w != data.width
            || self.grid_h != data.height
            || self.blocks_x != blocks_x
            || self.blocks_y != blocks_y
        {
            *self = Self::new_full(data);
            return;
        }
        let x0 = rect[0].min(data.width);
        let y0 = rect[1].min(data.height);
        let x1 = rect[2].min(data.width);
        let y1 = rect[3].min(data.height);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let bx0 = x0 / FP_BLOCK;
        let bx1 = (x1 - 1) / FP_BLOCK + 1;
        let by0 = y0 / FP_BLOCK;
        let by1 = (y1 - 1) / FP_BLOCK + 1;
        for by in by0..by1 {
            for bx in bx0..bx1 {
                let idx = (by * self.blocks_x + bx) as usize;
                self.block_hashes[idx] = block_hash(data, bx, by);
            }
        }
    }

    /// 总 hash：按块行序把块 hash 序贯 FNV-1a 折叠（`h = (h ^ bh) * PRIME`，
    /// 起始 [`FNV_OFFSET`]），尾部再折入建缓存时的幅面（`grid_w` / `grid_h`，
    /// 与 [`super::pack::pack_content_hash`] 同款收尾）。
    ///
    /// 幅面必须参与折叠：≤256² 的**单块图**换幅面（同图换栅格 / RasterRef
    /// 尺寸元数据改了而格子字节序列不变）时，行主序字节逐格相同 → 只折块
    /// hash 会把「同字节不同形状」折成同一个 total——换幅面不重打、旧布局
    /// 材质按新幅面拉伸上屏（task-3-review I-1；旧口径的形状敏感性由
    /// `total_distinguishes_same_bytes_different_shapes` 钉住）。
    ///
    /// **值域与旧 `pack_content_hash` 仍不可通约**（分块折叠 ≠ 全链折叠）——
    /// 敏感性等价由模块文档的判据 ② 钉住；重打判据消费的是「变没变」，
    /// 不是具体值。
    pub fn total(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for &bh in &self.block_hashes {
            h ^= bh;
            h = h.wrapping_mul(FNV_PRIME);
        }
        // 幅面折进尾部（同 `pack_content_hash` 的收尾形状）。
        h ^= self.grid_w as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= (self.grid_h as u64) << 32;
        h.wrapping_mul(FNV_PRIME)
    }
}

/// 单块 hash：块内 cells 的 `height`（u16 零扩展）/ `surface` / `biome`
/// 三字段按 [`super::pack::pack_content_hash`] 的同一顺序序贯 FNV-1a
/// （同字段口径、同序——对账基准就是它）。
fn block_hash(data: &TerrainData, bx: u32, by: u32) -> u64 {
    let x0 = bx * FP_BLOCK;
    let y0 = by * FP_BLOCK;
    let x1 = (x0 + FP_BLOCK).min(data.width);
    let y1 = (y0 + FP_BLOCK).min(data.height);
    let w = data.width as usize;
    let mut h = FNV_OFFSET;
    for y in y0..y1 {
        let row = y as usize * w;
        for x in x0..x1 {
            let c = &data.cells[row + x as usize];
            h ^= (c.height as u16) as u64;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= c.surface as u64;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= c.biome as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::super::pack::pack_content_hash;
    use super::*;
    use std::time::Instant;
    use wv_terrain::{SURFACE_LAND, SURFACE_WATER};

    /// 确定性 PRNG（xorshift64*）：property 测试的「随机」必须**可复现**——
    /// 固定种子逐位复现同一序列，失败时可重放定位。不引入 `rand` 依赖
    /// （AGENTS §9：默认拒绝新依赖）。
    struct Xorshift(u64);

    impl Xorshift {
        fn new(seed: u64) -> Self {
            Self(seed | 1) // 种子非零化
        }
        fn next_u32(&mut self) -> u32 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
        }
        /// `[0, n)` 内的均匀值（测试用，n 远小于 2¹⁶，模偏差可忽略）。
        fn below(&mut self, n: u32) -> u32 {
            self.next_u32() % n
        }
    }

    /// 夹具地形：水为底（−100 m），按确定性花纹铺约 40% 陆格
    /// （100–106 m，biome 0–4）——同时有同质大片与水陆交界，
    /// 让高度 / surface / biome 三类编辑都有落点。
    fn fixture(w: u32, h: u32) -> TerrainData {
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
                if (x * 7 + y * 13 + x * y) % 5 < 2 {
                    let i = (y * w + x) as usize;
                    cells[i] = TerrainCellData {
                        height: 100 + (x % 7) as i16,
                        surface: SURFACE_LAND,
                        biome: (x % 5) as u8,
                    };
                }
            }
        }
        TerrainData {
            width: w,
            height: h,
            cells,
        }
    }

    fn cell(height: i16, surface: u8, biome: u8) -> TerrainCellData {
        TerrainCellData {
            height,
            surface,
            biome,
        }
    }

    /// 一次「编辑」：随机挑一个字段改（三类字段都覆盖）；~1/10 概率把原值
    /// 写回（无操作编辑）——钉「未编辑必不误报」的另一半敏感性口径。
    fn mutate_cell(data: &mut TerrainData, x: u32, y: u32, rng: &mut Xorshift) {
        let i = (y * data.width + x) as usize;
        if rng.below(10) == 0 {
            return; // 原值写回 = 无操作编辑
        }
        let c = &mut data.cells[i];
        match rng.below(3) {
            // 夹具高度 ∈ [−100, 106]，saturating_add(1..=200) 必然改值。
            0 => c.height = c.height.saturating_add(1 + rng.below(200) as i16),
            1 => {
                c.surface = if c.surface == SURFACE_WATER {
                    SURFACE_LAND
                } else {
                    SURFACE_WATER
                };
            }
            _ => c.biome = c.biome.wrapping_add(1),
        }
    }

    // ══════ 判据 ①：自身一致——增量 ≡ 全量（逐位） ══════

    /// 随机编辑序列 property 测试（brief 指定）：32×24 小栅格 × 50 轮随机改格，
    /// 每轮 `update_region(编辑格)` 后 `total()` 与 `new_full(...).total()`
    /// 逐位相等。
    ///
    /// 随机序列设计：xorshift64* 固定种子 `0x5EED_2026_1028`——「随机」覆盖
    /// 三类字段 × 有操作 / 无操作 × 矩形贴边（x=w−1 时 `x+1` 越界，练钳制），
    /// 「确定」保证失败可重放。50 轮覆盖同一格被反复编辑、编辑后改回等轨迹。
    #[test]
    fn incremental_total_equals_full_recompute_bitwise() {
        let mut rng = Xorshift::new(0x5EED_2026_1028);
        let (w, h) = (32u32, 24u32);
        let mut data = fixture(w, h);
        let mut fp = BlockFingerprint::new_full(&data);
        for round in 0..50u32 {
            let (x, y) = (rng.below(w), rng.below(h));
            mutate_cell(&mut data, x, y, &mut rng);
            fp.update_region(&data, [x, y, x + 1, y + 1]);
            let want = BlockFingerprint::new_full(&data).total();
            assert_eq!(
                fp.total(),
                want,
                "第 {round} 轮编辑 ({x},{y}) 后：增量 total 与全量重建不等"
            );
        }
    }

    /// 跨块边界版（判据 ① 的多块覆盖）：32×24 全落在一个块里，这里的
    /// 600×300 = 3×2 = 6 块，编辑落在块角（列 254–257 跨 255/256 边界）与
    /// 带多格裕量的矩形也要与全量逐位一致。
    #[test]
    fn incremental_total_matches_full_across_block_boundaries() {
        let mut rng = Xorshift::new(0xFEED_C0DE);
        let (w, h) = (600u32, 300u32);
        let mut data = fixture(w, h);
        let mut fp = BlockFingerprint::new_full(&data);
        assert_eq!(fp.total(), BlockFingerprint::new_full(&data).total());
        for round in 0..40u32 {
            let (x, y) = (rng.below(w), rng.below(h));
            mutate_cell(&mut data, x, y, &mut rng);
            // 带 2 格裕量的矩形（上游 mark_dirty 会并出这种形状）。
            let rect = [
                x.saturating_sub(2),
                y.saturating_sub(2),
                (x + 3).min(w),
                (y + 3).min(h),
            ];
            fp.update_region(&data, rect);
            assert_eq!(
                fp.total(),
                BlockFingerprint::new_full(&data).total(),
                "第 {round} 轮编辑 ({x},{y}) rect={rect:?} 后：增量与全量不等"
            );
        }
        // 块边界角落：列 255/256 是块 0 与块 1 的分界，任一侧的单格编辑
        // 都必须只动所在块也对上全量。
        for x in [254u32, 255, 256, 257] {
            let i = (100 * w + x) as usize;
            data.cells[i].biome = data.cells[i].biome.wrapping_add(3);
            fp.update_region(&data, [x, 100, x + 1, 101]);
            assert_eq!(
                fp.total(),
                BlockFingerprint::new_full(&data).total(),
                "列 {x}（块分界附近）单格编辑后：增量与全量不等"
            );
        }
    }

    // ══════ 单格敏感性 + 可逆性 ══════

    /// 任一格的三字段任一改动 → total 变；同格改回 → total 逐位恢复。
    /// 「改回恢复」是分块折叠下「编辑必被看见」的反向保证（改回 = 内容
    /// 还原 = 指纹还原），顺带钉住 total 的确定性。
    #[test]
    fn fingerprint_distinguishes_any_single_cell_edit() {
        let (w, h) = (5u32, 4u32);
        let data = fixture(w, h);
        let baseline = BlockFingerprint::new_full(&data).total();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                // ── height：全量路径 ──
                let mut edited = data.clone();
                edited.cells[i].height += 7;
                assert_ne!(
                    BlockFingerprint::new_full(&edited).total(),
                    baseline,
                    "({x},{y}) height 改 → 指纹必须变"
                );
                // ── surface：增量路径（先改 → 变，再改回 → 恢复）──
                let mut surface_flipped = data.clone();
                surface_flipped.cells[i].surface = if data.cells[i].surface == SURFACE_WATER {
                    SURFACE_LAND
                } else {
                    SURFACE_WATER
                };
                let mut fp = BlockFingerprint::new_full(&data);
                fp.update_region(&surface_flipped, [x, y, x + 1, y + 1]);
                assert_ne!(
                    fp.total(),
                    baseline,
                    "({x},{y}) surface 翻转 → 指纹必须变（湿判定与海岸场都会变）"
                );
                fp.update_region(&data, [x, y, x + 1, y + 1]);
                assert_eq!(
                    fp.total(),
                    baseline,
                    "({x},{y}) surface 改回 → 指纹必须逐位恢复"
                );
                // ── biome：增量路径 ──
                let mut biome_bumped = data.clone();
                biome_bumped.cells[i].biome = biome_bumped.cells[i].biome.wrapping_add(1);
                let mut fp = BlockFingerprint::new_full(&data);
                fp.update_region(&biome_bumped, [x, y, x + 1, y + 1]);
                assert_ne!(
                    fp.total(),
                    baseline,
                    "({x},{y}) biome 改 → 指纹必须变（attr G 通道会变）"
                );
                fp.update_region(&data, [x, y, x + 1, y + 1]);
                assert_eq!(
                    fp.total(),
                    baseline,
                    "({x},{y}) biome 改回 → 指纹必须逐位恢复"
                );
            }
        }
    }

    // ══════ 退化输入 ══════

    /// 0×0 / 1×1 / 全水 5×5：不 panic，`total()` 与全量重建一致；越界矩形
    /// 在退化幅面上也只是钳制后无事。
    #[test]
    fn fingerprint_handles_degenerate_grids() {
        // 0×0：无块，total = 纯 offset 基值；越界矩形不动任何东西。
        let empty = TerrainData {
            width: 0,
            height: 0,
            cells: Vec::new(),
        };
        let mut fp = BlockFingerprint::new_full(&empty);
        let before = fp.total();
        fp.update_region(&empty, [0, 0, 10, 10]);
        assert_eq!(fp.total(), before, "0×0 上 update_region 不该有任何效果");
        assert_eq!(fp.total(), BlockFingerprint::new_full(&empty).total());

        // 1×1：单格单块，改一格即翻指纹。
        let mut one = TerrainData {
            width: 1,
            height: 1,
            cells: vec![cell(5, SURFACE_LAND, 3)],
        };
        let mut fp = BlockFingerprint::new_full(&one);
        let before = fp.total();
        one.cells[0].height = 99;
        fp.update_region(&one, [0, 0, 1, 1]);
        assert_eq!(fp.total(), BlockFingerprint::new_full(&one).total());
        assert_ne!(fp.total(), before, "1×1 的唯一格被改 → 指纹必须变");

        // 全水 5×5：同质图不 panic、确定性成立。
        let sea = TerrainData {
            width: 5,
            height: 5,
            cells: vec![cell(-100, SURFACE_WATER, 0); 25],
        };
        let fp = BlockFingerprint::new_full(&sea);
        assert_eq!(fp.total(), BlockFingerprint::new_full(&sea).total());
    }

    // ══════ 判据 ②：敏感性对拍（与旧 pack_content_hash 等价） ══════

    /// 同一随机编辑序列下，「新 total 是否翻转」与「旧 `pack_content_hash`
    /// 是否翻转」逐轮一致（**敏感性等价，非值等价**——两个口径的值域数学上
    /// 不可通约，见模块文档）。无操作编辑轮两边都必须**不**翻转。
    #[test]
    fn sensitivity_matches_legacy_content_hash() {
        let mut rng = Xorshift::new(0xABCD_1234);
        let (w, h) = (32u32, 24u32);
        let mut data = fixture(w, h);
        let mut fp = BlockFingerprint::new_full(&data);
        let mut fp_prev = fp.total();
        let mut legacy_prev = pack_content_hash(&data);
        for round in 0..60u32 {
            let (x, y) = (rng.below(w), rng.below(h));
            mutate_cell(&mut data, x, y, &mut rng);
            fp.update_region(&data, [x, y, x + 1, y + 1]);
            let fp_now = fp.total();
            let legacy_now = pack_content_hash(&data);
            assert_eq!(
                fp_now != fp_prev,
                legacy_now != legacy_prev,
                "第 {round} 轮编辑 ({x},{y})：新指纹{}而旧 hash{}——敏感性等价破坏",
                if fp_now != fp_prev {
                    "翻转"
                } else {
                    "未翻转"
                },
                if legacy_now != legacy_prev {
                    "翻转"
                } else {
                    "未翻转"
                }
            );
            fp_prev = fp_now;
            legacy_prev = legacy_now;
        }
    }

    // ══════ 结构变更：缓存整份重建（纯函数部分） ══════

    /// 结构不一致的前置条件（换图 / 同图换栅格）：缓存与传入数据幅面不符时，
    /// `update_region` 必须**整份重建**而不是拿旧块账就地补——只补一小块矩形
    /// 会留下「半新半旧」的错账。桥接侧（wv-render `sync_terrain_field`）的
    /// `structure_changed` 丢弃逻辑由该 crate 的集成测试覆盖。
    #[test]
    fn fingerprint_cache_rebuilds_on_structure_change() {
        let a = fixture(32, 24);
        let mut fp = BlockFingerprint::new_full(&a);
        // 幅面不同 = 格–块结构已变（32×24 → 1 块；16×48 → 1 块但格映射不同）。
        let b = fixture(16, 48);
        fp.update_region(&b, [0, 0, 4, 4]); // 只盖一角
        assert_eq!(
            fp.total(),
            BlockFingerprint::new_full(&b).total(),
            "结构不一致时必须整份重建：total 应等于新数据的全量，而非旧账补一块"
        );
        // 更彻底的形状差（块数都变）：同样必须重建。
        let c = fixture(600, 300);
        fp.update_region(&c, [0, 0, 4, 4]);
        assert_eq!(fp.total(), BlockFingerprint::new_full(&c).total());
    }

    // ══════ 形状敏感性（task-3-review I-1） ══════

    /// 同一格字节序列、不同幅面 → total 必不相等（对照 `pack.rs` 既有测试
    /// 「幅面折进指纹：同序列不同形状不得共享」的形态）。
    ///
    /// 触发面：≤256² 的**单块图**换幅面时行主序字节逐格相同（6×4 与 4×6 与
    /// 8×3 读的是同一份 cells 序列），块 hash 逐位相同——`total()` 若不折幅面，
    /// 三者共享指纹 ⇒ 换幅面不重打、旧布局材质拉伸上屏。幅面折尾后：
    /// 形状不同 ⇒ total 必不同；同形状同数据 ⇒ total 逐位相等（确定性）；
    /// 增量路径自愈重建后的 total 携带**新**幅面（与 new_full 一致）。
    #[test]
    fn total_distinguishes_same_bytes_different_shapes() {
        let cells: Vec<TerrainCellData> = (0..24)
            .map(|i| cell(100 + (i % 7) as i16, SURFACE_LAND, (i % 5) as u8))
            .collect();
        let wide = TerrainData {
            width: 6,
            height: 4,
            cells: cells.clone(),
        };
        let tall = TerrainData {
            width: 4,
            height: 6,
            cells: cells.clone(),
        };
        let squat = TerrainData {
            width: 8,
            height: 3,
            cells,
        };
        // 三种幅面各自单块（≤256²），块 hash 逐位相同——区分只能来自幅面折尾。
        let wide_total = BlockFingerprint::new_full(&wide).total();
        assert_ne!(
            wide_total,
            BlockFingerprint::new_full(&tall).total(),
            "6×4 与 4×6（同字节序列）不得共享指纹"
        );
        assert_ne!(
            wide_total,
            BlockFingerprint::new_full(&squat).total(),
            "6×4 与 8×3（同字节序列）不得共享指纹"
        );
        // 同形状同数据 → 逐位相等（确定性）。
        assert_eq!(
            wide_total,
            BlockFingerprint::new_full(&wide).total(),
            "同形状同数据的两次全量建必须逐位相等"
        );
        // 增量路径：幅面不符自愈重建后，total 携带新幅面（与 new_full 一致）。
        let mut fp = BlockFingerprint::new_full(&wide);
        fp.update_region(&tall, [0, 0, 1, 1]);
        assert_eq!(
            fp.total(),
            BlockFingerprint::new_full(&tall).total(),
            "自愈重建后的 total 必须携带新幅面（与 new_full(tall) 一致）"
        );
    }

    // ══════ 手动性能探针（不进门禁） ══════

    /// 手动跑：`cargo test -p wv-world-cell-map fingerprint -- --ignored --nocapture`
    ///
    /// 1024² 全量建 vs 一笔 brush 脏区的增量重算（debug 构建的墙钟，量级对比用
    /// ——验收口径 <0.5 ms 指 release 编辑帧；debug 下比值仍应有 2–3 个数量级差）。
    #[test]
    #[ignore = "手动性能探针：--ignored --nocapture"]
    fn perf_full_vs_dirty_region_on_1024_grid() {
        let data = fixture(1024, 1024);
        let t0 = Instant::now();
        let mut fp = BlockFingerprint::new_full(&data);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;

        let mut edited = data.clone();
        let rect = [500u32, 500, 512, 510]; // 一笔 brush 的典型脏区（12×10 格）
        for y in rect[1]..rect[3] {
            for x in rect[0]..rect[2] {
                edited.cells[(y * 1024 + x) as usize].height = edited.cells
                    [(y * 1024 + x) as usize]
                    .height
                    .saturating_add(5);
            }
        }
        let t1 = Instant::now();
        fp.update_region(&edited, rect);
        let region_ms = t1.elapsed().as_secs_f32() * 1000.0;

        let t2 = Instant::now();
        let _ = BlockFingerprint::new_full(&edited);
        let full_again_ms = t2.elapsed().as_secs_f32() * 1000.0;

        assert_eq!(
            fp.total(),
            BlockFingerprint::new_full(&edited).total(),
            "探针同时复核一致性"
        );
        println!(
            "1024²：new_full = {full_ms:.2} ms；update_region(12×10) = {region_ms:.4} ms；全量对照 = {full_again_ms:.2} ms"
        );
    }
}
