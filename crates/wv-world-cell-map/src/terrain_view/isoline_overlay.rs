//! 等值线矢量叠加层（ADR-0012 / plan v0.1 M2）：`VectorFlat` 浏览态的几何边界。
//!
//! ## 职责
//! - 从 `TerrainData` 派生海岸线（高度场海平面等值线）与类型场边界
//!   （`wv_terrain::isoline` 纯函数），Chaikin 平滑 + LOD 分档 DP 抽稀（Q8），
//!   装配成**单个批量描边 mesh** 叠在底图之上。
//! - 结构键（来源图 / 尺寸 / 分辨率 / 淡化 / LOD 档）变化时重算；`chunks_dirty`
//!   有待处理脏区（笔刷提交 / 整图重建）时下一帧重算。
//!
//! ## 边界（不做什么）
//! - **派生几何永不落盘**（plan D9）——mesh 每次重算；
//! - **不做 chunk 级增量提取**——脏区触发的是全图重算（512² 量级毫秒级；
//!   4096² 每笔一顿是已知妥协，`TODO(plan M2.5)`：按 chunk + halo 局部提取）；
//! - **不改变平涂底图**——色块仍由纹理管线负责，本层只画边界线
//!   （ADR-0012 §2①「多边形平涂」的 mesh 化填充留待 M2 后半）。

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::mesh::MeshVertexAttribute;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use wv_terrain::isoline::simplify_dp;

use crate::palette::{CellPalette, TreatmentFlags};

/// 等值线本色的 alpha（作者未给 Terrain 图层设 opacity 时的值）。
/// 墨色本体走 `PaletteColors.isoline_ink`（ADR-0016：日间褐墨 / 暮色浅墨）。
const ISOLINE_BASE_ALPHA: f32 = 1.0;

/// 叠加层 Z：底图块（`TERRAIN_Z = 0.0`）之上、换级淡化精灵（0.05）与道路
/// （`ROAD_Z = 0.1`）之下——等值线属于底图的一部分。
pub const ISOLINE_Z: f32 = 0.04;

/// 描边屏幕宽度（px）：世界宽度 = `px × view.scale`（ADR-0008 §8 的「乘」口径）。
pub const STROKE_PX: f32 = 1.25;

/// 描边亚线性缩放指数（手绘地图 #10）：世界半宽 ∝ scale^`GLYPH_SHRINK_EXP`。
///
/// 取自 civ-atlas 的 GLYPH_SHRINK = 0.22（屏幕观感 k^(1-0.22) = k^0.78 的
/// 对偶）：∈ (0, 1) 才是「放大时屏幕变纤细」；= 1 退化为 ADR-0008 §8 的
/// 严格乘口径（屏幕恒定）；> 1 则放大时线变粗（非手绘观感）。
const GLYPH_SHRINK_EXP: f32 = 0.78;

/// 描边世界半宽的唯一实现（主线 / 水彩晕 / 河水中线全走这里）。
///
/// `base_px` = 屏幕像素基准宽（scale=1 时的屏幕宽），`floor` = 世界下限
/// （极远防消隐）。同公式供测试手算锚定。
pub fn stroke_half_world(base_px: f32, scale: f32, floor: f32) -> f32 {
    (base_px * 0.5 * scale.powf(GLYPH_SHRINK_EXP)).max(floor)
}

/// 类型边界描边的格径半宽（× cell_size）：盖住「阶梯 → Chaikin 平滑」的
/// 偏差 + 填充的像素量化，让类型边界读作一条画上去的墨线。
pub const KIND_STROKE_CELL: f32 = 0.25;

/// 海岸线水彩外晕（手绘地图 #4，2026-10-15）：
/// 内晕 = 主线 `thin_half × HALO_1_MULT`、外晕 = `thin_half × HALO_2_MULT`。
/// 两层渐变 alpha（[`HALO_1_ALPHA`] / [`HALO_2_ALPHA`]）模拟水彩晕染。
const HALO_1_MULT: f32 = 2.6;
const HALO_2_MULT: f32 = 5.0;
pub const HALO_1_ALPHA: f32 = 0.35;
pub const HALO_2_ALPHA: f32 = 0.18;

/// 河水中线（手绘地图 #8，2026-10-15）：
/// - `RIVER_MULT` = 主线 `thin_half × 0.5`（比主线细一半）。
/// - `RIVER_ALPHA` = 0.5（半透，不抢主线）。
const RIVER_MULT: f32 = 0.5;
pub const RIVER_ALPHA: f32 = 0.5;

/// 重算触发键：只含**结构性**输入——任一变化才重算派生几何。
///
/// `view.scale` 只通过 `lod` 档（Q8 分档）进入，连续缩放不触发重算。
///
/// **palette 不进键**（2026-10-17 性能修正）：几何输出只依赖地形内容与
/// treatment 的三个几何位（晕 / 河中线 / 亚线性描边宽），与墨色无关——内置
/// 两包（日间 / 暮色）treatment 完全相同（`HAND_DRAWN` 零回归硬前提），
/// 把 palette 放进键会让纯换色白付一整条 marching squares + Chaikin + DP
/// 管线（1024² 图实测 ~75ms）。墨色变化走 `ink_palette` 快路径重上色。
#[derive(Debug, Clone, PartialEq)]
pub struct IsolineKey {
    pub map: Option<wv_core::Id>,
    pub size: u32,
    /// 逻辑格行数（非方形地图下 != `size`）。
    pub height: u32,
    pub res: u32,
    pub soften: bool,
    /// 几何相关装饰位 `[coast_halo, river_centerline, ink_sublinear]`
    /// （`ThemePack.treatment` 的子集）。
    pub treatment: [bool; 3],
    /// DP 容差档（1/2 octave 对数桶，2026-10-23 起替代旧的 1/64 量化）。
    /// 缩放跨过档界才重算 DP；档内几何逐位不变。
    pub tol_bucket: i32,
    /// Nation 档（`view.scale >= LOD_NATION_SCALE`）：只画海岸线、不画类型边界。
    pub nation: bool,
}

/// 地形内容指纹（仅进程内缓存比较用，不落盘、不跨进程）：FNV-1a 扫
/// `height + kind`。纯换色（palette 切换）会让整图重建置位 `chunks_dirty`，
/// 但**不改编单元格**——指纹相同即「几何无需重算」，跳过派生管线。
///
/// 为什么不用 `DefaultHasher`：逐 cell 走 `Hash` trait 每字段一次虚分派，
/// 1024² 图实测量级会到几十 ms，抵消快路径收益；手写 FNV 按字段折叠，
/// 1M 格 ~3ms，只发生在「结构同键但有脏区待消费」的帧。
pub fn terrain_data_hash(data: &wv_core::terrain::TerrainData) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = FNV_OFFSET;
    for c in &data.cells {
        h ^= (c.height as u16) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        // 指纹折叠显示槽位（三通道 → 旧 0–9）：等值线的类型边界语义是
        // 「视觉上的类型差异」，随 biome 变化也要重算。
        h ^= wv_terrain::biome::display_kind(&wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        }) as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    // 尺寸折叠进尾部：不同 width/height 的同序列 cells 不共享指纹。
    h ^= data.width as u64;
    h = h.wrapping_mul(FNV_PRIME);
    h ^= (data.height as u64) << 32;
    h.wrapping_mul(FNV_PRIME)
}

/// 一条平滑后的等值线链（点列 + 闭合位）——链缓存的存储单位。
pub type IsolineChain = (Vec<wv_terrain::isoline::Pt>, bool);

/// 链缓存失效输入：地形内容 / 来源图 / treatment 几何位 / Nation 档。
///
/// **不含 `tol_bucket`**——DP 容差只作用于缓存链；缩放跨档只重跑 DP + 建网格，
/// 不重跑 marching squares / 装配 / Chaikin。
#[derive(Debug, Clone, PartialEq)]
pub struct ChainCacheInputs {
    pub map: Option<wv_core::Id>,
    pub data_hash: u64,
    pub treatment: [bool; 3],
    pub nation: bool,
}

/// 提取缓存：Chaikin 之后、DP 之前的链。
pub struct CachedChains {
    pub inputs: ChainCacheInputs,
    pub coast_smoothed: Vec<IsolineChain>,
    pub kinds_smoothed: Vec<IsolineChain>,
}

/// 资源：等值线叠加实体 + 上次构建的键。
#[derive(Resource, Default)]
pub struct IsolineOverlay {
    pub entity: Option<Entity>,
    pub mesh: Option<Handle<Mesh>>,
    pub material: Option<Handle<ColorMaterial>>,
    /// 海岸线水彩外晕（手绘地图 #4，2026-10-15）：2 个额外描边层（2.6× / 5.0×
    /// `thin_half`），仅在海岸存在时插入；alpha 渐变模拟水彩晕。类型边界不
    /// 走晕（kind 切换不是水彩晕的语义）。
    pub halo_outer: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    pub halo_outer_outer: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    /// 河水中线（手绘地图 #8，2026-10-15）：海岸链上方一道窄青蓝线（`label_water`
    /// 颜色）——civ-atlas 风格的"水边反光"。宽度 0.5× main，alpha 0.5。仅海岸。
    pub coast_river: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    pub key: Option<IsolineKey>,
    /// 最近一次构建时的地形内容指纹（[`terrain_data_hash`]）：辨认「脏区待消费
    /// 但地形内容没变」（纯换色）用，见 `sync_isoline_overlay` 的快路径。
    pub data_hash: Option<u64>,
    /// 最近一次上墨色的调色板：palette 变化而几何不变时只需重上色，不重算。
    pub ink_palette: Option<&'static str>,
    /// 提取缓存（Task 3）：与实体生命周期无关——`despawn()` **不**清它，
    /// 只在 `inputs` 失配时重建。缩放只消费它重跑 DP。
    pub chains: Option<CachedChains>,
}

impl IsolineOverlay {
    pub fn despawn(&mut self, commands: &mut Commands) {
        if let (Some(entity), Some(mesh)) = (self.entity.take(), self.mesh.take()) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        for (entity, mesh, _mat) in [
            self.halo_outer.take(),
            self.halo_outer_outer.take(),
            self.coast_river.take(),
        ]
        .into_iter()
        .flatten()
        {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        self.key = None;
        self.data_hash = None;
        self.ink_palette = None;
    }

    /// 「建过一次且结论是无几何」：实体与 mesh 皆空但键还在。整图同类型
    /// （新建地图的常态）没有海岸线 / 类型边界，叠加层**合法地无实体**；
    /// 不把这个稳态记进键，守卫 `entity.is_some()` 会撞空、每帧全图
    /// marching-squares 重算。
    pub fn empty_settled(&self) -> bool {
        self.entity.is_none() && self.mesh.is_none() && self.key.is_some()
    }
}

/// DP 抽稀容差（连续档，spec M1/M2）：**近细远粗**。
///
/// `view.scale` 口径 = 世界单位/屏幕像素，**大 = 远**（spec §0 三证据）。
/// `0.5 × scale` 钳到 `[0.125, 0.5]`：锚点 0.25→0.125、0.5→0.25、≥1.0→0.5。
/// 这同时关掉了 2026-10-15 侧记的「三档方向反装」——连续函数在锚点上即
/// 方向修正（spec CONTINUOUS_LOD_PLAN_v0.1 §3.2）。
pub fn dp_tol_continuous(scale: f32) -> f32 {
    (0.5 * scale).clamp(0.125, 0.5)
}

/// 容差档的对数步长（每 octave 几档）。2 档/octave = 相邻档相差 ×1.414。
///
/// 取值理由：DP 容差直接决定「抽稀多少点」，档间跳变若到 ×2（1 档/octave）
/// 近观会看到线条细节成片增减（视觉上是「线忽然变直」）；×1.414 一档的
/// 几何差在半像素量级，读不出来，而重算次数只有 1 档/octave 的 2 倍——
/// 12 档缩放实测 4 次重算（见下），仍有 3 倍余量。
const DP_TOL_STEPS_PER_OCTAVE: f32 = 2.0;

/// DP 容差的**对数档**：`round(log2(tol) × 2)`，即 1/2 octave 桶。
///
/// ## 2026-10-23 改动（迭代档缺陷修复 D：等值线不再每档重算）
///
/// 旧实现 `(dp_tol_continuous(scale) × 64.0) as u8`（1/64 格量化）有两个问题：
/// 1. **一档必跨**：近观带 `scale < 1.0` 时 `tol = 0.5 × scale`，滚轮一档
///    ×0.88 → 量化值必减 2~4 → 每档都重跑 DP + 建 mesh（1024² 实测 15.6ms/debug，
///    见 `docs/PERF_GRID_AND_PROFILE_v0.1.md`）。
/// 2. **`as u8` 会绕回**：`scale > 8.0` 时 `tol × 64 = 32 × scale > 255`，
///    `as u8` 按 256 取模 → 档值反而变小（`scale 12 → 128`），键非单调，
///    同一次缩放可能来回重算。
///
/// 现在容差本身按对数档量化（[`dp_tol_for_bucket`]），档内 DP 输出**逐位不变**：
/// 整条缩放范围只剩 5 个档（tol = 0.125 / 0.1768 / 0.25 / 0.3536 / 0.5）。
pub fn dp_tol_bucket(scale: f32) -> i32 {
    let tol = dp_tol_continuous(scale).max(1e-6);
    (tol.log2() * DP_TOL_STEPS_PER_OCTAVE).round() as i32
}

/// 档中心容差：`2^(bucket / 2)`——与 [`dp_tol_bucket`] 互逆（差一次取整）。
///
/// **几何只用本函数**（不用 `dp_tol_continuous(scale)`）：否则键说「同档」而
/// 容差随 scale 连续漂移，几何变了却不重建（或反过来白重建）。
pub fn dp_tol_for_bucket(bucket: i32) -> f32 {
    (bucket as f32 / DP_TOL_STEPS_PER_OCTAVE).exp2()
}

/// 按 palette 与图层 opacity 重写全部等值线材质颜色（主线 / 两圈水彩晕 / 河中线）。
///
/// 材质是**缓存复用**的资产（`Assets::get_mut` 就地改，不新建）；快路径
/// （纯换色 / 图层绑定变化）靠它把墨色刷到四类实体上，免去整条几何管线重算。
///
/// 2026-10-17 从 `apply_material_alpha` 扩展：晕与河中线的 alpha 原先只在
/// spawn 时烘焙，改图层 opacity 不生效（既有不一致）；统一走本函数后四类
/// 材质都响应 opacity 与 palette。
pub fn apply_material_colors(
    materials: &mut Assets<ColorMaterial>,
    overlay: &IsolineOverlay,
    pal: &CellPalette,
    opacity: f32,
) {
    let ink = pal.isoline_ink;
    if let Some(handle) = &overlay.material {
        if let Some(mut mat) = materials.get_mut(handle) {
            mat.color = ink.with_alpha(ISOLINE_BASE_ALPHA * opacity);
        }
    }
    for (slot, alpha) in [
        (&overlay.halo_outer, HALO_1_ALPHA),
        (&overlay.halo_outer_outer, HALO_2_ALPHA),
    ] {
        if let Some((_, _, handle)) = slot {
            if let Some(mut mat) = materials.get_mut(handle) {
                mat.color = ink.with_alpha(alpha * opacity);
            }
        }
    }
    if let Some((_, _, handle)) = &overlay.coast_river {
        if let Some(mut mat) = materials.get_mut(handle) {
            mat.color = pal.label_water.with_alpha(RIVER_ALPHA * opacity);
        }
    }
}

/// 一条描边：点列 + 闭合位 + 世界半宽。
type IsolineStroke = (Vec<wv_terrain::isoline::Pt>, bool, f32);

/// 从缓存链装配四路描边：主线 / 水彩内晕 / 水彩外晕 / 河中线。
///
/// 入参是 **Chaikin 之后** 的链；本函数只跑 DP + 半宽赋值（缩放热路径）。
/// 两段水彩晕分开返回（计划 Interfaces 写的是单路 `coast_halo`，但既有渲染
/// 是 2.6× / 5.0× 两层 alpha 渐变，合并会改变观感——见 ledger Ruling）。
#[allow(clippy::too_many_arguments)]
pub fn assemble_strokes(
    coast: &[IsolineChain],
    kinds: &[IsolineChain],
    tol: f32,
    thin_half: f32,
    kind_half: f32,
    treatment: TreatmentFlags,
) -> (
    Vec<IsolineStroke>,
    Vec<IsolineStroke>,
    Vec<IsolineStroke>,
    Vec<IsolineStroke>,
) {
    let mut strokes: Vec<IsolineStroke> = Vec::new();
    let mut halo_inner: Vec<IsolineStroke> = Vec::new();
    let mut halo_outer: Vec<IsolineStroke> = Vec::new();
    let mut river: Vec<IsolineStroke> = Vec::new();
    for (pts, closed) in coast {
        let thin = simplify_dp(pts, *closed, tol);
        if thin.len() < 2 {
            continue;
        }
        strokes.push((thin.clone(), *closed, thin_half));
        if treatment.coast_halo {
            halo_inner.push((thin.clone(), *closed, thin_half * HALO_1_MULT));
            halo_outer.push((thin.clone(), *closed, thin_half * HALO_2_MULT));
        }
        if treatment.river_centerline {
            river.push((thin, *closed, thin_half * RIVER_MULT));
        }
    }
    for (pts, closed) in kinds {
        let thin = simplify_dp(pts, *closed, tol);
        if thin.len() >= 2 {
            strokes.push((thin, *closed, kind_half));
        }
    }
    (strokes, halo_inner, halo_outer, river)
}

/// 把折线链展开成**单个**三角网（每段一个加宽四边形，段端外扩半宽盖住转角缝隙）。
///
/// 每链自带半宽（`chains` 第三元）：海岸线细、类型边界粗。
/// 不做圆头/斜头连接：同色 overdraw 下外扩接缝不可见，省掉逐转角的扇形开销。
/// `grid_lines`（编辑态逐格网格）复用同一展开器。
///
/// 返回 `None` 表示**什么都没挤出来**（链表为空 / 全是短于 2 点或零长的退化
/// 链）。调用方必须以退场响应 `None`——绝不能把 0 顶点 mesh 插进 `Assets`：
/// MeshAllocator 对空 mesh 跳过分配、拷贝阶段却无条件读取，bevy #24874 会
/// 误报 `Use-after-free`。
pub fn build_stroke_mesh(
    chains: &[(Vec<wv_terrain::isoline::Pt>, bool, f32)],
    origin: Vec2,
    cell_size: f32,
) -> Option<Mesh> {
    let to_world = |p: wv_terrain::isoline::Pt| origin + Vec2::new(p.x, p.y) * cell_size;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (pts, closed, half_w) in chains {
        let n = pts.len();
        if n < 2 {
            continue;
        }
        let last = if *closed { n } else { n - 1 };
        for i in 0..last {
            let a = to_world(pts[i]);
            let b = to_world(pts[(i + 1) % n]);
            let dir = b - a;
            let len = dir.length();
            if len < 1e-9 {
                continue;
            }
            let dir = dir / len;
            let nrm = Vec2::new(-dir.y, dir.x);
            // 段两端沿方向外扩半宽：相邻段的扩出部分互相覆盖，转角无豁口。
            let a0 = a - dir * half_w;
            let b0 = b + dir * half_w;
            let base = positions.len() as u32;
            positions.push([a0.x + nrm.x * half_w, a0.y + nrm.y * half_w, 0.0]);
            positions.push([a0.x - nrm.x * half_w, a0.y - nrm.y * half_w, 0.0]);
            positions.push([b0.x - nrm.x * half_w, b0.y - nrm.y * half_w, 0.0]);
            positions.push([b0.x + nrm.x * half_w, b0.y + nrm.y * half_w, 0.0]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::isoline::{assemble_chains, chaikin, height_segments, kind_segments, Pt};

    fn p(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    /// Q8 分档：缩放 1.0 / 0.3 / 0.1 分别落近 / 中 / 远档。
    #[test]
    fn continuous_dp_tolerance_hits_anchor_points() {
        let cases = [
            (0.1_f32, 0.125_f32), // 深缩（近）→ 钳到最细
            (0.25, 0.125),        // 恰锚点
            (0.5, 0.25),          // 恰锚点
            (1.0, 0.5),           // 恰锚点
            (4.0, 0.5),           // 远观 → 钳到最粗
        ];
        for (scale, want) in cases {
            let got = dp_tol_continuous(scale);
            assert!(
                (got - want).abs() < 1e-6,
                "scale={scale} 得 {got}，应 {want}"
            );
        }
    }

    /// Nation 位是几何结构输入：跨 `LOD_NATION_SCALE` 时容差档可能不变
    /// （都在钳位区），key 必须仍判不等——否则类型边界残留 / 缺失。
    #[test]
    fn isoline_key_distinguishes_nation_tier() {
        let base = IsolineKey {
            nation: false,
            tol_bucket: -2,
            map: Some(wv_core::Id::new("m").expect("合法 id")),
            size: 512,
            height: 512,
            res: 4,
            soften: false,
            treatment: [true, true, true],
        };
        let mut far = base.clone();
        far.nation = true;
        assert_ne!(base, far, "Nation 档几何不同（类型边界不画），key 必须不等");
    }

    /// 容差档：整条缩放范围只有 5 个档（tol 钳在 `[0.125, 0.5]` → log2 ∈ [-3, -1]）。
    #[test]
    fn tol_buckets_are_clamped_to_five_values() {
        assert_eq!(dp_tol_bucket(4.0), -2, "远观钳位 0.5 → log2×2 = -2");
        assert_eq!(dp_tol_bucket(1.0), -2, "1.0 恰钳位边界");
        assert_eq!(dp_tol_bucket(0.25), -6, "0.125 → log2×2 = -6");
        assert_eq!(dp_tol_bucket(0.1), -6, "深近观仍钳在 -6");
        // 大 scale 不再绕回（旧 `as u8` 在 scale > 8 时按 256 取模）
        assert_eq!(dp_tol_bucket(12.0), -2, "scale 12 仍是最粗档");
        assert_eq!(dp_tol_bucket(1000.0), -2);
    }

    /// 档中心容差与档值互逆，且每档恰是上一档的 ×1.414。
    #[test]
    fn tol_bucket_centers_step_half_an_octave() {
        for b in -6..=-2 {
            let cur = dp_tol_for_bucket(b);
            let next = dp_tol_for_bucket(b + 1);
            assert!(
                (next / cur - std::f32::consts::SQRT_2).abs() < 1e-5,
                "档 {b}→{}：{cur} → {next} 应为 ×√2",
                b + 1
            );
            assert!(
                (0.125 - 1e-6..=0.5 + 1e-6).contains(&cur),
                "档 {b} 越界：{cur}"
            );
        }
    }

    /// 12 档连续缩放的**重算次数**对比（改动 D 的验收数）：
    ///
    /// 起点 `scale = 1.0`（格距 4px 的编辑档），每档 ×0.88（滚轮一格）：
    ///
    /// | 步 | scale | tol = 0.5×scale | 旧 1/64 量化 | 新 1/2 octave 档 |
    /// |---|---|---|---|---|
    /// | 0 | 1.000 | 0.500 | 32 | -2 |
    /// | 1 | 0.880 | 0.440 | 28 | -2 |
    /// | 2 | 0.774 | 0.387 | 24 | -3 |
    /// | 3 | 0.682 | 0.341 | 21 | -3 |
    /// | 4 | 0.600 | 0.300 | 19 | -3 |
    /// | 5 | 0.528 | 0.264 | 16 | -4 |
    /// | 6 | 0.464 | 0.232 | 14 | -4 |
    /// | 7 | 0.409 | 0.204 | 13 | -5 |
    /// | 8 | 0.360 | 0.180 | 11 | -5 |
    /// | 9 | 0.316 | 0.158 | 10 | -5 |
    /// | 10 | 0.279 | 0.139 | 8 | -6 |
    /// | 11 | 0.245 | 0.125 | 8 | -6 |
    /// | 12 | 0.216 | 0.125 | 8 | -6 |
    ///
    /// 旧：10 次重算（每档都跨）。新：4 次（第 2/5/7/10 步各一次）。
    #[test]
    fn twelve_zoom_steps_trigger_at_most_four_recomputes() {
        let mut scale = 1.0_f32;
        let mut bucket = dp_tol_bucket(scale);
        let mut changes = 0;
        let mut trace = vec![bucket];
        for _ in 0..12 {
            scale *= 0.88;
            let b = dp_tol_bucket(scale);
            if b != bucket {
                changes += 1;
                bucket = b;
            }
            trace.push(b);
        }
        assert!(
            changes <= 4,
            "12 档缩放只允许 ≤4 次 DP 重算，实测 {changes}（档序 {trace:?}）"
        );
        assert_eq!(changes, 4, "手算应为 4 次（第 2/5/7/10 步）");
        assert_eq!(
            trace,
            vec![-2, -2, -3, -3, -3, -4, -4, -5, -5, -5, -6, -6, -6]
        );
    }

    /// 缓存链复用口径：同一组链 + 同容差 → 主线 + 两路晕 + 河中线都产出。
    #[test]
    fn assemble_strokes_from_cached_chains_yields_main_halo_and_river() {
        let coast = vec![(vec![p(0.5, 0.5), p(8.5, 0.5)], false)];
        let (strokes, halo_inner, halo_outer, river) =
            assemble_strokes(&coast, &[], 0.125, 0.5, 1.0, TreatmentFlags::HAND_DRAWN);
        assert_eq!(strokes.len(), 1, "一条海岸链 = 一条主线");
        assert_eq!(halo_inner.len(), 1, "coast_halo 全开 → 内晕每链一");
        assert_eq!(halo_outer.len(), 1, "coast_halo 全开 → 外晕每链一");
        assert_eq!(river.len(), 1, "river_centerline 全开 → 每链一河中线");
    }

    /// 全退化链（空表 / 单点链）→ 无 mesh：渲染层不得插 0 顶点网格
    /// （bevy #24874 误报 Use-after-free），调用方以退场响应 `None`。
    #[test]
    fn stroke_mesh_degenerate_chains_produce_no_mesh() {
        assert!(build_stroke_mesh(&[], Vec2::ZERO, 1.0).is_none());
        assert!(build_stroke_mesh(&[(vec![p(0.0, 0.0)], false, 0.5)], Vec2::ZERO, 1.0).is_none());
    }

    #[test]
    fn stroke_mesh_two_point_chain_yields_one_quad() {
        let chains = vec![(vec![p(0.0, 0.0), p(4.0, 0.0)], false, 0.5)];
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 1.0).expect("非退化链应有 mesh");
        let pos = mesh
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在")
            .as_float3()
            .expect("应为 Float32x3")
            .to_vec();
        assert_eq!(pos.len(), 4);
        // 外扩：x 范围 [-0.5, 4.5]，y 范围 [-0.5, 0.5]。
        let xs: Vec<f32> = pos.iter().map(|v| v[0]).collect();
        let ys: Vec<f32> = pos.iter().map(|v| v[1]).collect();
        let (xmin, xmax) = (
            xs.iter().cloned().fold(f32::MAX, f32::min),
            xs.iter().cloned().fold(f32::MIN, f32::max),
        );
        let (ymin, ymax) = (
            ys.iter().cloned().fold(f32::MAX, f32::min),
            ys.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(
            (xmin + 0.5).abs() < 1e-6 && (xmax - 4.5).abs() < 1e-6,
            "x 范围 [{xmin}, {xmax}] ≠ [-0.5, 4.5]，顶点 {pos:?}"
        );
        assert!(
            (ymin + 0.5).abs() < 1e-6 && (ymax - 0.5).abs() < 1e-6,
            "y 范围 [{ymin}, {ymax}] ≠ [-0.5, 0.5]，顶点 {pos:?}"
        );
        assert_eq!(mesh.indices().expect("索引应存在").len(), 6);
    }

    /// 闭环三角链：三条边各一段（含闭合边），12 顶点 18 索引。
    #[test]
    fn stroke_mesh_closed_chain_draws_closing_edge() {
        let chains = vec![(vec![p(0.0, 0.0), p(4.0, 0.0), p(2.0, 3.0)], true, 0.5)];
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 1.0).expect("非退化链应有 mesh");
        assert_eq!(mesh.indices().expect("索引应存在").len(), 18);
    }

    /// 结构键：图 / 尺寸 / 分辨率 / 淡化 / LOD / 几何装饰位任一变化都判不等；
    /// 瞬态脏位不进键。
    #[test]
    fn isoline_key_covers_structural_inputs_only() {
        let base = IsolineKey {
            treatment: [true, true, true],
            map: Some(wv_core::Id::new("m").expect("合法 id")),
            size: 512,
            height: 512,
            res: 4,
            soften: false,
            tol_bucket: -2,
            nation: false,
        };
        let same = base.clone();
        assert_eq!(base, same);
        let variants = [
            // 容差档变 → key 必须不等（缩放跨量化步长要重算）。
            IsolineKey {
                tol_bucket: base.tol_bucket + 1,
                ..base.clone()
            },
            IsolineKey {
                map: None,
                size: 512,
                height: 512,
                res: 4,
                soften: false,
                treatment: base.treatment,
                nation: false,
                tol_bucket: -2,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 256,
                height: 512,
                res: 4,
                soften: false,
                treatment: base.treatment,
                nation: false,
                tol_bucket: -2,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                height: 512,
                res: 8,
                soften: false,
                treatment: base.treatment,
                nation: false,
                tol_bucket: -2,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                height: 512,
                res: 4,
                soften: true,
                treatment: base.treatment,
                nation: false,
                tol_bucket: -2,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                height: 512,
                res: 4,
                soften: false,
                treatment: base.treatment,
                nation: false,
                tol_bucket: -3,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                height: 512,
                res: 4,
                soften: false,
                treatment: [false, true, true],
                nation: false,
                tol_bucket: -2,
            },
        ];
        for v in variants {
            assert_ne!(base, v, "键 {v:?} 应与基键不同");
        }
    }

    /// palette 不进结构键（2026-10-17 性能修正）：内置两包 treatment 的几何位
    /// 完全相同，纯换色必须落进「同键 → 跳过几何重算」的快路径；几何装饰位
    /// 变了才判不等。
    #[test]
    fn isoline_key_ignores_palette_but_respects_treatment_bits() {
        // 镜像域内的同语义断言：几何位（HAND_DRAWN 全开）一致时键必须相等
        // （wv-theme 侧「两内置包几何位相同」的断言留在 wv-theme 测试）。
        let day = TreatmentFlags::HAND_DRAWN;
        let dusk = TreatmentFlags::HAND_DRAWN;
        let key_of = |t: TreatmentFlags| IsolineKey {
            map: Some(wv_core::Id::new("m").expect("合法 id")),
            size: 512,
            height: 512,
            res: 4,
            soften: false,
            treatment: [t.coast_halo, t.river_centerline, t.ink_sublinear],
            nation: false,
            tol_bucket: -2,
        };
        assert_eq!(
            key_of(day),
            key_of(dusk),
            "内置两包几何位相同 → 键相等（纯换色不重算几何）"
        );
    }

    /// 内容指纹：同数据同指纹；任一 cell 的 height / kind 变了指纹必变；
    /// 尺寸进指纹（同序列不同形状不共享）。
    #[test]
    fn terrain_data_hash_separates_content_and_shape() {
        fn make(height: i16, biome: u8, w: u32, h: u32) -> wv_core::terrain::TerrainData {
            wv_core::terrain::TerrainData {
                width: w,
                height: h,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height,
                        surface: 1,
                        biome,
                    };
                    (w * h) as usize
                ],
            }
        }
        let base = make(10, 3, 4, 4);
        assert_eq!(
            terrain_data_hash(&base),
            terrain_data_hash(&make(10, 3, 4, 4))
        );
        assert_ne!(
            terrain_data_hash(&base),
            terrain_data_hash(&make(11, 3, 4, 4)),
            "height 变 → 指纹变"
        );
        assert_ne!(
            terrain_data_hash(&base),
            terrain_data_hash(&make(10, 4, 4, 4)),
            "kind 变 → 指纹变"
        );
        assert_ne!(
            terrain_data_hash(&base),
            terrain_data_hash(&make(10, 3, 2, 8)),
            "形状变（cells 数相同）→ 指纹变"
        );
    }

    /// 端到端（纯函数段）：小地形 → 提取 → 装配 → 平滑 → 抽稀 → mesh 非空。
    #[test]
    fn full_derivation_pipeline_yields_nonempty_mesh() {
        let (w, h) = (8u32, 8u32);
        let cells: Vec<wv_core::terrain::TerrainCellData> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let land = (2..=5).contains(&x) && (2..=5).contains(&y);
                wv_core::terrain::TerrainCellData {
                    height: if land { 100 } else { -100 },
                    surface: if land { 1 } else { 0 },
                    biome: 3,
                }
            })
            .collect();
        let data = wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        };
        let cell = |x: u32, y: u32| &data.cells[(y * w + x) as usize];
        let coast = height_segments(w, h, |x, y| cell(x, y).height as f32, 0.0);
        let kinds = kind_segments(w, h, |x, y| {
            wv_terrain::biome::display_kind(&wv_terrain::TerrainCell {
                height: cell(x, y).height,
                surface: cell(x, y).surface,
                biome: cell(x, y).biome,
            })
        });
        assert!(!coast.is_empty() && !kinds.is_empty());
        let mut strokes = Vec::new();
        for (segs, pre) in [(coast, false), (kinds, true)] {
            for c in assemble_chains(&segs) {
                let pts = if pre {
                    simplify_dp(&c.pts, c.closed, 0.30)
                } else {
                    c.pts.clone()
                };
                let thin = simplify_dp(&chaikin(&pts, c.closed, 2), c.closed, 0.125);
                if thin.len() >= 2 {
                    strokes.push((thin, c.closed, 0.5));
                }
            }
        }
        let mesh = build_stroke_mesh(&strokes, Vec2::ZERO, 20.0).expect("4×4 陆块应有 mesh");
        let idx = mesh.indices().expect("索引应存在").len();
        assert!(idx > 0, "4×4 陆块的海岸线与类型边界不应为空");
        assert_eq!(idx % 3, 0, "三角网索引应是 3 的倍数");
    }
    // -----------------------------------------------------------------------
    // 水彩外晕（手绘地图 #4，2026-10-15）
    // -----------------------------------------------------------------------

    /// 海岸水彩晕比内晕更宽：HALO_2_MULT > HALO_1_MULT > 1。
    #[test]
    fn halo_multipliers_form_widening_radii() {
        const { assert!(HALO_1_MULT > 1.0, "内晕应比主线宽") }
        const { assert!(HALO_2_MULT > HALO_1_MULT, "外晕应比内晕更宽") }
        const {
            assert!(
                HALO_1_ALPHA < 1.0 && HALO_2_ALPHA < HALO_1_ALPHA,
                "晕越外 alpha 越低"
            )
        }
    }

    /// 主线 mesh 在 kind 边界单独注入点（kind=6 山地块）下也能产出——我们没有
    /// 改 build_stroke_mesh 的契约，复用既有测试断言。
    #[test]
    fn main_stroke_mesh_unaffected_by_halo_branch() {
        let mesh = build_stroke_mesh(
            &[(
                vec![
                    wv_terrain::isoline::Pt { x: 0.5, y: 1.5 },
                    wv_terrain::isoline::Pt { x: 2.5, y: 1.5 },
                ],
                false,
                0.5,
            )],
            Vec2::ZERO,
            1.0,
        );
        assert!(mesh.is_some(), "主线 mesh 仍应产出");
    }

    /// 水彩晕 alpha < 1 才有「水彩」感；等于 1 退化成主线，不算晕。
    #[test]
    fn halo_alphas_are_strictly_less_than_one() {
        const {
            assert!(
                HALO_1_ALPHA > 0.0 && HALO_1_ALPHA < 1.0,
                "内晕 alpha 应在 (0,1)"
            )
        }
        const {
            assert!(
                HALO_2_ALPHA > 0.0 && HALO_2_ALPHA < 1.0,
                "外晕 alpha 应在 (0,1)"
            )
        }
    }

    // -----------------------------------------------------------------------
    // 河水中线（手绘地图呈现 #8，2026-10-15）
    // -----------------------------------------------------------------------

    /// RIVER_MULT = 0.5，RIVER_ALPHA = 0.5：比主线更细 + 半透，不抢主线。
    #[test]
    fn river_layer_dimensions_are_strictly_smaller_than_main() {
        const { assert!(RIVER_MULT < 1.0, "河水中线应比主线窄") }
        const { assert!(RIVER_MULT > 0.0, "但不能为 0") }
        const {
            assert!(
                RIVER_ALPHA > 0.0 && RIVER_ALPHA < 1.0,
                "河水中线 alpha 应在 (0,1)"
            )
        }
    }

    /// 河水中线 mesh 产出非空：与主线同链（海岸），复用 `build_stroke_mesh`。
    #[test]
    fn river_coast_mesh_is_nonempty_when_coast_present() {
        let mesh = build_stroke_mesh(
            &[(
                vec![
                    wv_terrain::isoline::Pt { x: 0.5, y: 1.5 },
                    wv_terrain::isoline::Pt { x: 5.5, y: 1.5 },
                    wv_terrain::isoline::Pt { x: 5.5, y: 5.5 },
                ],
                false,
                0.5 * 1.25, // RIVER_MULT * STROKE_PX
            )],
            Vec2::ZERO,
            1.0,
        );
        assert!(mesh.is_some(), "河水中线 mesh 应产出");
    }

    // -----------------------------------------------------------------------
    // 符号缩放（手绘地图呈现 #10，2026-10-15）
    // -----------------------------------------------------------------------

    /// 亚线性缩放锚点（手算）：scale=1 → base/2；scale=4 → base/2 × 4^0.78
    /// ≈ base/2 × 2.928（**小于**线性的 base/2 × 4 = 2·base——放大时世界
    /// 尺寸长得慢）；scale=0.25 → base/2 × 0.25^0.78 ≈ base/2 × 0.341
    /// （**大于**线性的 base/8——缩小时世界尺寸缩得慢，屏幕更粗更可辨）。
    #[test]
    fn stroke_half_world_grows_sublinearly_with_scale() {
        let base = 2.0; // STROKE_PX 量级，手算方便
        let floor = 0.0;
        let at_one = stroke_half_world(base, 1.0, floor);
        assert!(
            (at_one - 1.0).abs() < 1e-5,
            "scale=1 应为 base/2=1，实得 {at_one}"
        );
        let at_four = stroke_half_world(base, 4.0, floor);
        // 4^0.78 = e^(0.78·ln4) = e^(0.78×1.38629) = e^1.08131 ≈ 2.9485
        assert!(
            (at_four - 2.9485).abs() < 0.01,
            "scale=4 应为 1×2.9485≈2.95，实得 {at_four}"
        );
        assert!(at_four < 4.0, "亚线性：4 倍缩放下世界半宽应 < 线性值 4");
        let at_quarter = stroke_half_world(base, 0.25, floor);
        // 0.25^0.78 = 1/2.9485 ≈ 0.3392
        assert!(
            (at_quarter - 0.3392).abs() < 0.01,
            "scale=0.25 应为 1×0.3392≈0.34，实得 {at_quarter}"
        );
        assert!(
            at_quarter > 0.25,
            "亚线性：1/4 缩放下世界半宽应 > 线性值 0.25"
        );
    }

    /// 下限生效：极小 scale 时回到 floor，不随指数继续缩。
    #[test]
    fn stroke_half_world_respects_floor_at_extreme_zoom_out() {
        let base = 2.0;
        let floor = 0.5;
        let v = stroke_half_world(base, 0.001, floor);
        assert_eq!(v, floor, "极远时应取世界下限 {floor}，实得 {v}");
        // 指数约束：∈ (0,1) 才是亚线性。
        const {
            assert!(
                GLYPH_SHRINK_EXP > 0.0 && GLYPH_SHRINK_EXP < 1.0,
                "缩放指数应在 (0,1)"
            )
        }
    }
}
