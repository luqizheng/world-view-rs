//! CellMap 真实地形晕渲的**公式唯一真源**（ADR-0077 D6）。
//!
//! 双光源美术光照 + Horn 米制梯度 + 三尺度概括权重的全部常量与纯函数
//! 都收在本模块——后续 CPU 块烘焙、GPU `FieldParams` uniform、WGSL
//! 场着色器三处**只准消费这里的签名与常量，禁散落字面量**（ADR-0077 D2/D6）。
//!
//! ## 为什么是「美术化光照」而非物理太阳模拟
//!
//! 用户明示：晕渲的目标是**让山体可读**（山脊受光 / 背光分明、宽缓地形
//! 有体量），不是渲染真实日照。因此三光系数（[`AMBIENT`] /
//! [`DIFFUSE_MAIN`] / [`DIFFUSE_AUX`]）是**美术调参**，和恒为 1：
//! `I = 0.33 + 0.52·max(0, N·L₁) + 0.15·max(0, N·L₂)`，入参 = 法线与
//! 光向量的点积。[`LIGHT_MAIN`] / [`LIGHT_AUX`] 存**单位向量**（`dot(n,
//! LIGHT_*)` 即合法入参，无需归一化）。单位光向下 `I ∈ [0.33, ≈0.935]`
//! （三系数和为 1，`max(0,·)` 非负）**界内性免费获得**——环境光从
//! 0.4 降下来是 ADR-0101 的核心目标之一：旧 0.4 保底把背光面抬平，山体
//! 明暗比只有 2.5:1；新口径 ≈2.8:1，背光面沉下去且不过重（若拿非单位
//! 光向量直接点积则可越界，见 [`lighting_intensity`] 文档的历史注记）。
//! 辅光 L₂ 治「山脉走向恰与主光平行时受光/背光面同时消失」的盲区。
//!
//! ## 2026-11-05 修订（ADR-0101，推翻 ADR-0077 D1 的参数面）
//!
//! 外部制图评审（2026-11-05，结论已裁定采纳）推翻 ADR-0077 D1 的**数值**：
//! ① 主光高度角 45° → **38°**、辅光 45° → **50°**（仰角越低坡面对比越锐，
//! 45° 偏「无影」）；② 能量配比 0.4/0.48/0.12 → **0.33/0.52/0.15**（评审
//! 方案 B：保底下调 = 阴影下沉；首轮 0.28/0.55/0.17 实机暗侧过重，微抬）。
//! 机制不变：三系数和 = 1、单位光向量、免钳制。
//! ③ 合成由 RGB 直接相乘改为 **LCH(ab) 明度调制**（[`lch_shade`]）——
//! 只缩 L、色相严格不动、阴影区色度小幅回补，防「乘法变灰变脏」；
//! ④ 新增**山脊高光**装饰层（[`RIDGE_DOT_THRESH`] 家族，仅 Themed 分支）。
//! 依据与被否决备选（米制 Z、SSAO、AO）见 ADR-0101。
//!
//! ## 量纲纪律（2026-10-30 起：**归一化高度域**，修正 ADR-0077 D2 的米制口径）
//!
//! [`horn_gradient`] 返回**作者高度单位/格**（i16 差分）；[`slope_gain_of`]
//! 用**本图陆地高度实际跨度**把它归一化（`s = Z_EXAG / 陆地跨度`，2026-11-05
//! ADR-0101 起 [`height_span_of`] 只统计 `height > 0` 的格——海底深洼会把
//! 分母灌大、稀释陆地浮雕，实测 jrpg-world 全域 span 5246 vs 陆地 3548，
//! 改后浮雕 +1.48×）。语义 =「把本图**陆地**最高差按 [`Z_EXAG`] 格起伏
//! 打光」——同**相对**高差 → 同法线，与作者的高度单位（米 / 抽象格值）、
//! 图的格距、有无比例尺全部无关。
//!
//! **为什么废弃米制口径**（ADR-0092 §1 证据链）：米制 `s = Z_EXAG / 米每格`
//! 与 GPU 场管线的**纹理归一化梯度**（h_norm ∈ [0,1]，固定域 −4000..6000）
//! 相乘，虚构图上有效夸张比可读带小数百倍（法线恒近平，浮雕输出恒定），
//! CPU raw 域又落在饱和端——同一旋钮在两管线双双失效；且作者高度本就
//! 不必然是米（README §12-53 的老问题在晕渲侧的显影）。米制机器
//! （`meters_per_cell_of` / `DEFAULT_METERS_PER_CELL`）随本修正退役。
//! **ADR-0101 再次确认归一化域**（否决外部评审「Z_EXAG=2.5 米制」提案——
//! 那是误读本常量：200 是「全图跨度按 200 格起伏」的归一化域系数，等效
//! 米制夸张 ≈38×，填 2.5 会把山脉压成 6° 缓坡）。
//!
//! ## 三尺度概括
//!
//! 三尺度各求**法线**，按权重混合成单一法线后**统一打光**（2026-11-05
//! ADR-0101 起；原 ADR-0077 D3 为「各档打光再加权明暗」——法线混合避免
//! 多档明暗互相冲淡，且混合法线正是山脊高光的判定输入）。三权重和 = 1。
//! 远观时细纹理成噪点，detail 权重随 px_per_cell 平滑衰减、衰减量按
//! 宏观：中观比例回流（[`scale_weights`]）。
//!
//! [ADR-0077]: ../docs/adr/ADR-0077-cellmap-relief-shading.md

use wv_core::terrain::TerrainData;

// ---------------------------------------------------------------------------
// 常量（ADR-0077 D1/D2/D3；值锁死于设计稿，验收期唯一校准点 = Z_EXAG）
// ---------------------------------------------------------------------------

/// 环境光系数：背光面保底亮度。与两漫反射系数和恒为 1（见模块文档）。
///
/// **0.33（ADR-0101）**：旧 0.4 把背光面抬得太高（明暗比 2.5:1，山体
/// 「平」的直接原因之一）。首轮落参 0.28（明暗比 ≈3.3:1），实机验收
/// 觉得中景山体暗侧过重（「脏棕」观感），微抬到 0.33——即评审方案 B
/// 配比 0.33/0.52/0.15（和仍 = 1，免钳制纪律不变）。
pub const AMBIENT: f32 = 0.33;
/// 主光漫反射系数。
pub const DIFFUSE_MAIN: f32 = 0.52;
/// 辅光漫反射系数。
pub const DIFFUSE_AUX: f32 = 0.15;

/// 垂直夸张系数（**归一化域，无量纲**）：`s·梯度 = Z_EXAG × 梯度/跨度`，
/// 语义 =「把本图最高差按 [`Z_EXAG`] 格起伏打光」。**2026-10-30 实机落参
/// 200.0**——用户逐档目验 {12, 25, 50, 100, 200, 400}（jrpg-world 远近两
/// 机位）：25 及以下内陆不可读（「跨度÷中位宏观坡度」反推值 ~26 被**海岸
/// 悬崖**拉高的全图中位失真 ~8×，内陆实际宏观坡度平缓得多），200 起山脉链
/// 远近皆可读且近距无噪斑，400 过浓。结论追记 ADR-0092 §8。历史沿革：
/// 初值 2.0（米制口径，ADR-0077 D2）→ 6.0（米制无效校准）→ 25.0（归一化
/// 域反推，仍欠）→ 200.0（实机目验落参）。运行时探针 `WV_Z_EXAG` 可覆盖
/// （[`z_exag()`]），复校准不重编译。
pub const Z_EXAG: f32 = 200.0;

/// 运行时纵向夸张：[`Z_EXAG`] 的探针覆盖版——ADR-0077 §4 验收期校准的
/// 对比帧采集件。环境变量 `WV_Z_EXAG=<正数>` 在进程内首次调用时读定、
/// 之后恒定；未设置 / 非法值回落 [`Z_EXAG`]。与 WV_MODE / WV_SCALE /
/// WV_STRIP / WV_DRILL 同族（家族清单见 wv-app `debug_probe` 模块文档）。
/// GPU 侧经 `FieldParams.shape.z`、CPU 侧经 [`slope_gain_of`] 同走本函数，
/// 两管线不出现不同夸张。
pub fn z_exag() -> f32 {
    static OVERRIDE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *OVERRIDE.get_or_init(|| {
        let raw = std::env::var("WV_Z_EXAG").ok();
        let parsed = raw
            .as_deref()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v > 0.0);
        // 探针激活时打一条 stderr 追踪（纯 crate 无 bevy 日志面；缺省零输出）。
        if let Some(v) = parsed {
            eprintln!("[wv] WV_Z_EXAG → z_exag = {v}（缺省 {}）", Z_EXAG);
        }
        parsed.unwrap_or(Z_EXAG)
    })
}

/// 宏观档权重（远观山脉骨架）。
/// 三尺度权重 [`W_MACRO`] + [`W_MEDIUM`] + [`W_DETAIL`] 和 = 1，
/// 混合结果不需要再钳制——与光照系数同一「和=1 免钳制」纪律。
///
/// **0.35（ADR-0101）**：旧 0.45 让最大尺度的盒平均主导整幅，山脊骨架
/// 被平滑掉；评审建议把权重重心移到中观（山坡褶皱），宏观只负责骨架。
pub const W_MACRO: f32 = 0.35;
/// 中观档权重（支脉 / 山体）。
pub const W_MEDIUM: f32 = 0.45;
/// 细节档权重（局部坡面），随缩放衰减（[`detail_weight_fade`]）。
pub const W_DETAIL: f32 = 0.20;

/// 中观盒平均足迹（格）：中观档 ≈ 以此为半径的盒式低通后的地形打光。
/// GPU 侧映射到 mip 级偏移 / 跨步采样（mip 即盒式低通；高度为仿射编码
/// ⇒ 编码域滤波 ≡ 米域滤波，ADR-0077 D3）。
pub const MEDIUM_RADIUS_CELLS: f32 = 3.0;
/// 宏观盒平均足迹（格）：宏观档的盒式低通半径。
///
/// **8（ADR-0101）**：旧 12 格 = 12 km 的低通把山脉链整体抹平（骨架反而
/// 软）；8 格保住山链走向的同时仍滤掉单格噪声。
pub const MACRO_RADIUS_CELLS: f32 = 8.0;

/// 主光 L₁：方位 315°（西北）、**高度角 38°**（ADR-0101，2026-11-05 外部
/// 制图评审落参；旧 ADR-0077 D1 值 45°——仰角越低坡面明暗对比越锐，
/// 45° 接近「无影照明」。「左上受光」与全部地物符号/阴影同向）。**单位
/// 向量**：水平模 cos38° ≈ 0.788011 均分到西北两轴（各 ≈0.557208），
/// z = sin38° ≈ 0.615661——纯 2D hillshade 的 z=0 会让所有法线与光垂直、
/// 整幅图恒定亮度。**消费侧（T4 CPU / T6 WGSL）直接 dot 即可，无需再
/// normalize**（多 normalize 也无害，单位向量不动）。
pub const LIGHT_MAIN: [f32; 3] = [-0.5572078, 0.5572078, 0.6156615];
/// 辅光 L₂：方位 45°（东北）、**高度角 50°**（ADR-0101：辅光只补层次，
/// 仰角抬高、避免与主光抢对比）。与主光**只差 x 符号与仰角**——
/// 正交的受光盲区互补：任一走向的山脉，两面至少被一盏灯看见。
pub const LIGHT_AUX: [f32; 3] = [0.4545195, 0.4545195, 0.7660444];

/// 山脊高光：主光点积门槛（ADR-0101）。混合法线与主光点积**超过**此值
/// （强受光坡面）才参与判定——高光只沿「被照亮的陡坡」出一条边。
pub const RIDGE_DOT_THRESH: f32 = 0.80;
/// 山脊高光：坡度门槛（度，ADR-0101）。混合法线 z 分量换算的坡角超过
/// 此值（陡坡）才参与判定——平缓台地不提亮。
pub const RIDGE_SLOPE_DEG: f32 = 18.0;
/// 山脊高光：命中后的**线性明度增量上限**（ADR-0101）。加在 LCH 调制结果
/// 上（linear RGB 域），且按 `1 − 目标明度` 自削弱——首版 0.12 定值加在
/// 近纸白的色带高阶上直接饱和成白斑（2026-11-05 实机验收退回）；现值
/// 0.06：中调坡面 ≈+0.04，高光坡面自削弱，斑点退回细线。
pub const RIDGE_BOOST: f32 = 0.06;

/// LCH 阴影色度补偿上限（ADR-0101）：[`lch_shade`] 在最深处（shade→0）
/// 给色度 C 追加的最大 Lab 色度量。0.12 ≈ 肉眼「阴影不发灰」的舒适带
/// （0.10–0.15，评审建议区间取中）；中性色（C < 1e-4）不补偿——无 hue
/// 可守，注入色相反而染脏纸面色。
pub const SHADE_CHROMA_BOOST: f32 = 0.12;

/// 固定海水基色（**sRGB 口径**，`#2A5D8F`；ADR-0101：地形渲染与 ThemePack
/// 解耦后的固定常量，主题整合推迟 2.0）。浅滩 / 深海色带在 GPU 侧由此
/// 派生（浅 = 向色带低阶色混、深 = ×0.82），不再读 `p.theme[0]`。
/// **首版标定值，实机验收后可调**（改这里 = 改 Browse/Map 的海色）。
pub const WATER_SRGB: [f32; 3] = [0.1647059, 0.3647059, 0.5607843];

/// detail 权重衰减曲线起点（px/格）：低于此分辨率细纹理全是噪点，
/// detail 权重归零。
pub const DETAIL_FADE_MIN_PX_PER_CELL: f32 = 1.0;
/// detail 权重衰减曲线终点（px/格）：高于此分辨率细部可信，detail 全额。
pub const DETAIL_FADE_MAX_PX_PER_CELL: f32 = 3.0;

// ---------------------------------------------------------------------------
// 纯函数（消费方：CPU 块烘焙 / GPU uniform / WGSL 三处只认这里）
// ---------------------------------------------------------------------------

/// 本图**陆地**高度实际跨度（作者单位）：`max(h) − min(h)` 仅统计
/// `height > 0` 的格，下限 1 防平图/全海图除零。
///
/// **2026-11-05 ADR-0101 修正**：原口径是全域 max−min——海底深洼
/// （demo 实测全域 span 5246 vs 陆地 3548）会把归一化分母灌大 ≈1.48×，
/// 陆地浮雕被稀释成缓坡。晕渲管的是**陆面**（水面不进明暗），分母
/// 自然也只管陆面。全海图（无 `h > 0` 格）回落下限 1：梯度本来就全零，
/// 下限只保证 gain 有限、不产生 NaN。
///
/// 归一化夸张的分母——**每次烘焙现场算一遍**（O(N) 一次线性扫），不做缓存：
/// 笔刷改高度后跨度会变，缓存会让编辑前后的浮雕口径漂移。平图（跨度 0）
/// 梯度本来就全零，下限只保证 gain 有限、不产生 NaN。
pub fn height_span_of(data: &TerrainData) -> f32 {
    let mut lo = i16::MAX;
    let mut hi = i16::MIN;
    for c in &data.cells {
        if c.height <= 0 {
            continue;
        }
        lo = lo.min(c.height);
        hi = hi.max(c.height);
    }
    if hi < lo {
        // 全海图（无 h > 0 格）：回落下限——i16 直减还会溢出（MIN−MAX），
        // 必须在减法前拦截。
        return 1.0;
    }
    ((hi as i32) - (lo as i32)).max(1) as f32
}

/// 梯度归一化增益：`s = z_exag() / 本图陆地高度跨度`。
///
/// 梯度是「作者高度单位/格」，s 把「每格高差 ÷ 陆地跨度」缩成无纲相对坡度
/// 再进法线公式；归一化后「同**相对**高差 → 同法线」跨图成立——作者的
/// 高度单位、格距、有无比例尺都不影响观感（量纲纪律见模块文档）。
pub fn slope_gain_of(height_span: f32) -> f32 {
    z_exag() / height_span
}

/// CPU 块烘焙旧签名（`terrain_pixels` / `patch_terrain_image`）委托
/// `_with_scale` 变体时传入的**过渡缺省跨度**（作者单位）。
///
/// **纯过渡缺省**：接线方一律现场传 [`height_span_of`]（wv-render 桥接
/// `bake_height_span` 封装），未接线的消费方统一按纹理固定域跨度
/// （10000，= `field/pack.rs` 的 H_MAX−H_MIN）打光——该值下 gain 与
/// 归一化纹理域的梯度天然同量级，旧签名路径不致失真。
pub const DEFAULT_HEIGHT_SPAN: f32 = 10000.0;

/// 光照明度乘子：`AMBIENT + DIFFUSE_MAIN·max(0, N·L₁) + DIFFUSE_AUX·max(0, N·L₂)`。
///
/// ## 接口契约：入参 = `dot(n, LIGHT_*)`
///
/// [`LIGHT_MAIN`] / [`LIGHT_AUX`] 是**单位向量**（方位 315°/45°、高度角
/// 38°/50°，ADR-0101），`n_dot_l1 / n_dot_l2` 直接取
/// `dot(n, LIGHT_MAIN)` / `dot(n, LIGHT_AUX)` 即可，无需归一化
///（2026-10-28 前常量存模长 √(3/2) 方向、归一化是调用方义务——契约随
/// 常量单位化取消；本函数始终锁死为纯公式，不做隐式归一化）。
///
/// 对单位光向量点积入参，`I ∈ [0.33, ≈0.935]`（三系数和为 1，`max(0,·)`
/// 非负）——**界内性免费获得，调用方不许再钳制**。上界不紧：全定义域
/// 实际 max = `0.33 + |0.52·L₁ + 0.15·L₂|`，其中 `L₁·L₂ = 0.4716`（两灯
/// 只差 x 符号与仰角，水平项相消），故 `|0.52L₁ + 0.15L₂| =
/// √(0.52² + 0.15² + 2·0.52·0.15·0.4716) ≈ 0.6054`，即 max ≈ 0.9354
///（取得于 `n ∝ 0.52L₁ + 0.15L₂`）；下界 0.33 可达（n 与两灯点积同时
/// ≤ 0，如 n = (0, −1, 0)）。
///
/// ## ⚠️ 历史注记：非单位光向量入参可越界
///
/// 若调用方拿**非单位**光向量直接点积（2026-10-28 前的常量口径），
/// `I` 可超 1——越界即亮部溢出，事后钳制救不回层次。现常量已单位化，
/// 此坑只在未来改光向时需要警惕：**改常量必须保持模长 1**（锚点测试
/// `lighting_constants_are_unit_vectors_at_declared_elevation`）。
pub fn lighting_intensity(n_dot_l1: f32, n_dot_l2: f32) -> f32 {
    AMBIENT + DIFFUSE_MAIN * n_dot_l1.max(0.0) + DIFFUSE_AUX * n_dot_l2.max(0.0)
}

/// LCH(ab) 明暗调制（ADR-0101）：**只缩 L（明度）、色相 H 严格不动、
/// 阴影区色度 C 小幅回补**——替代旧的 RGB 直接相乘（乘法把暗部饱和度
/// 打掉，颜色发灰发脏，山体越暗越「糊」）。
///
/// `linear RGB → XYZ → Lab → LCH → 缩放/回补 → Lab → XYZ → linear RGB`，
/// 白点 D65，全内联纯函数（GPU 侧 `terrain_field.wgsl::lch_hillshade`
/// 与本函数**逐式镜像**，注释互指——改任何一侧必须同步另一侧）。
///
/// ## 参数
///
/// - `rgb`：**线性 sRGB**（不是伽马编码值——本管线色板一律先
///   `srgb_to_linear` 再进着色）。
/// - `shade`：晕渲明暗因子（[`lighting_intensity`] 的输出，0.33–0.94）。
/// - `chroma_boost_max`：阴影最深处追加的最大色度（[`SHADE_CHROMA_BOOST`]）；
///   按阴影深度线性缩放（`shade=1` → 零补偿）。
///
/// ## 边界行为
///
/// - `shade = 1`：恒等（±1e-4 浮点回路误差，测试钉住）。
/// - 中性色（C < 1e-4）**跳过色度补偿**：无 hue 可守，注入色相会把纸面
///   灰染脏；atan2(0,0) 在部分后端未定义，一并规避。
/// - 出界钳回 [0, 1]：高饱和深阴影在 Lab→RGB 反解时可出线性域，
///   clamp 是最后防线（极边缘处色相有微小损失，可接受）。
pub fn lch_shade(rgb: [f32; 3], shade: f32, chroma_boost_max: f32) -> [f32; 3] {
    // linear sRGB → XYZ（D65）。行 × 列展开书写，避免 mat 语义歧义。
    let x = 0.4124564 * rgb[0] + 0.3575761 * rgb[1] + 0.1804375 * rgb[2];
    let y = 0.2126729 * rgb[0] + 0.7151522 * rgb[1] + 0.0721750 * rgb[2];
    let z = 0.0193339 * rgb[0] + 0.119192 * rgb[1] + 0.950304 * rgb[2];

    const D65: [f32; 3] = [0.95047, 1.0, 1.08883];
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let fx = f(x / D65[0]);
    let fy = f(y / D65[1]);
    let fz = f(z / D65[2]);
    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b = 200.0 * (fy - fz);

    // LCH：C = 模、H = atan2(b, a)。调制只动 L 与 C，H 原样带回。
    let c = (a * a + b * b).sqrt();
    let hue = b.atan2(a);
    let boost = if c > 1e-4 {
        (1.0 - shade) * chroma_boost_max
    } else {
        0.0
    };
    let c_new = (c + boost).clamp(0.0, 130.0);
    let l_new = (l * shade).clamp(0.0, 100.0);
    let a_new = c_new * hue.cos();
    let b_new = c_new * hue.sin();

    // Lab → XYZ → linear sRGB。
    let fy2 = (l_new + 16.0) / 116.0;
    let fx2 = a_new / 500.0 + fy2;
    let fz2 = fy2 - b_new / 200.0;
    let finv = |t: f32| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            (t - 16.0 / 116.0) / 7.787
        }
    };
    let x2 = D65[0] * finv(fx2);
    let y2 = D65[1] * finv(fy2);
    let z2 = D65[2] * finv(fz2);
    let r = 3.2404542 * x2 - 1.5371385 * y2 - 0.4985314 * z2;
    let g = -0.969266 * x2 + 1.8760108 * y2 + 0.041556 * z2;
    let bl = 0.0556434 * x2 - 0.2040259 * y2 + 1.057225 * z2;
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), bl.clamp(0.0, 1.0)]
}

/// Horn 3×3 梯度：`[1,2,1]ᵀ·[1,2,1]/8` 加权隔格差分，返回 `(hx, hy)` **米/格**。
///
/// 为什么 Horn 而不是中心差分：手绘/生成数据有台阶毛刺，中心权重 4、
/// 直邻 2、对角 1 的加权平滑能抑制单格噪声（ADR-0077 D2）——Horn 起是
/// 晕渲法线的**唯一梯度来源**（GPU 双三次解析梯度与 CPU 中心差分退役；
/// 高度**取值**仍走双三次插值，两者分工不变）。
///
/// 坐标约定：`x` 列向右、`y` 行号**向下**增大（南）；`hx > 0` = 高度向右
/// 增大、`hy > 0` = 高度**向南**增大。邻域 `clamp` 在图内
///（`0..width-1 / 0..height-1`），边界格不 panic、结果有限——边缘处
/// 梯度退化为中心半格差，晕渲可接受（外推假想坡比切断渲染更不可取）。
/// `y` 分量翻转的世界坐标含义在 [`normal_world`]（CPU/GPU 对齐锚点）。
pub fn horn_gradient(data: &TerrainData, x: i64, y: i64) -> (f32, f32) {
    let w = data.width as i64;
    let h = data.height as i64;
    if w == 0 || h == 0 || data.cells.is_empty() {
        // 空图没有坡可言；防止下方 clamp(0, -1) panic。
        return (0.0, 0.0);
    }
    let at = |dx: i64, dy: i64| -> f32 {
        let xi = (x + dx).clamp(0, w - 1) as usize;
        let yi = (y + dy).clamp(0, h - 1) as usize;
        data.cells[yi * w as usize + xi].height as f32
    };
    // 可分离核：列向 = (右列 [1,2,1] 和 − 左列 [1,2,1] 和)/8，行向同理。
    let left = at(-1, -1) + 2.0 * at(-1, 0) + at(-1, 1);
    let right = at(1, -1) + 2.0 * at(1, 0) + at(1, 1);
    let top = at(-1, -1) + 2.0 * at(0, -1) + at(1, -1);
    let bottom = at(-1, 1) + 2.0 * at(0, 1) + at(1, 1);
    ((right - left) / 8.0, (bottom - top) / 8.0)
}

/// 梯度 → 世界坐标法线：`normalize(−s·hx, +s·hy, 1)`。
///
/// ## ⚠️ `+s·hy`（不是 −s）——CPU/GPU 对齐锚点，两侧不许各改各的
///
/// 数据行号向下增大（南），世界坐标 **+y = 北**——法线的 y 分量在
/// 此**翻转一次**。这样「高度向南增大」的坡（`hy > 0`）法线朝北
///（`normal[1] > 0`），被东北—西北方向的主/辅光（`L.y = +0.5`）照亮：
/// **山影倒向南方、坡面朝左上受光**。本公式的受光方向锚点 =
/// `field/material.rs` 的 `LIGHT_DIR` + 纹理空间法线（GPU 路径，制图学
/// 西北受光惯例）。注意：`cells.rs::relief_mul` 的 `lit = −(dx + dy)`
/// 与本公式**逐轴相反**——其最亮分支（dx < 0 且 dy < 0）恰是本公式的
/// 背光分支（本公式最亮条件 hx > 0 且 hy > 0），属该旧 CPU 路径的既有
/// 缺陷；Task 4 切换后 CPU/GPU 方向统一到本公式（= material.rs）口径，
/// relief_mul 随路径退役。此翻转是全局约定：换光源方位必须两侧同步，
/// 不允许单侧改。
///
/// WGSL 侧对应约定（与本函数互指，两侧不许各改各的）：梯度同在纹理/行向
/// 取（`+v` 向南），法线写 `normalize(vec3(-zx, +zy, 1.0))`——**WGSL 也翻，
/// 翻在法线构造处**（`+s·zy` 与本函数 `+s·hy` 逐字同形：zy/hy 同为行向
/// 梯度），之后与世界系 `LIGHT_*` 直接点积。
///
/// ## 追记（2026-10-28，Task 6 自审发现 + 控制器裁定修正）
///
/// 本注记此前称 WGSL 写 `-zy`「不再翻、翻转已隐含在纹理空间 → 世界坐标
/// 的变换里」——那是锚点笔误：变换只对**法线**成立，对**光源**不成立
///（世界系光源的 y 分量未经变换就进了纹理系点积，y 项符号反挂）。旧
/// GPU 路线（≤T33）受光因此等效西南而非西北。ADR-0077 起两管线统一为
/// **西北受光**（spec D2「左上 45° 受光 + CPU/GPU 同公式」为最高权威）。
/// 两侧若同时翻或不翻，南北光照镜像，视觉验收立刻可察。
///
/// 健壮性契约：任何输入（含 `slope_gain = inf` 这类退化）都返回**有限的
/// 单位法线**——退化时回落平地法线 `[0, 0, 1]`。这是给 CPU/GPU 双侧
/// 消费的公式真源，不允许一侧 nan 一侧正常的分叉。
pub fn normal_world(hx: f32, hy: f32, slope_gain: f32) -> [f32; 3] {
    let nx = -slope_gain * hx;
    // + 号即上文翻转锚点：数据行向下（南）→ 世界 +y（北）。
    let ny = slope_gain * hy;
    let nz = 1.0;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    if !len.is_finite() || len == 0.0 {
        return [0.0, 0.0, 1.0];
    }
    [nx / len, ny / len, nz / len]
}

/// detail 档权重衰减：`smoothstep(1.0, 3.0, px_per_cell)`。
///
/// 远观（< 1 px/格）细纹理在屏幕上是噪点 → detail 归零；近观
///（≥ 3 px/格）细部可信 → 全额。曲线必须随 px_per_cell **单调**
///（ADR-0077 D3 的验收口径），smoothstep 保证之。
pub fn detail_weight_fade(px_per_cell: f32) -> f32 {
    let t = ((px_per_cell - DETAIL_FADE_MIN_PX_PER_CELL)
        / (DETAIL_FADE_MAX_PX_PER_CELL - DETAIL_FADE_MIN_PX_PER_CELL))
        .clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 三尺度权重 `[macro, medium, detail]`，**和恒为 1**（不再钳制混合结果）。
///
/// detail 份额 = [`W_DETAIL`] × [`detail_weight_fade`]；衰减掉的部分按
/// 宏观：中观的既有比例（0.45 : 0.40）回流两档——衰减是「把噪声预算
/// 换给宏观结构」，不是整体减光（ADR-0077 D3 取代 T33 关灯的理由）。
pub fn scale_weights(px_per_cell: f32) -> [f32; 3] {
    let detail = W_DETAIL * detail_weight_fade(px_per_cell);
    let back = W_DETAIL - detail; // 衰减量，按既有比例回流
    let base = W_MACRO + W_MEDIUM;
    let macro_w = W_MACRO + back * (W_MACRO / base);
    let medium_w = W_MEDIUM + back * (W_MEDIUM / base);
    [macro_w, medium_w, detail]
}

// ---------------------------------------------------------------------------
// 测试（期望值全部手算锚点——「跑出来的值就是对的」不接受，AGENTS §7.2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_1_SQRT_2;
    use wv_core::terrain::TerrainCellData;

    const EPS: f32 = 1e-6;

    /// 3×3 定高 fixture：行优先 `heights`，surface/biome 填 0。
    fn grid3(width: u32, height: u32, heights: &[i16]) -> TerrainData {
        assert_eq!(heights.len(), (width * height) as usize);
        TerrainData {
            width,
            height,
            cells: heights
                .iter()
                .map(|&h| TerrainCellData {
                    height: h,
                    surface: 0,
                    biome: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn lighting_intensity_matches_hand_values_and_stays_in_bounds() {
        // 全受光（两盏灯正对）：0.33 + 0.52 + 0.15 = 1.0
        let full = lighting_intensity(1.0, 1.0);
        assert!((full - 1.0).abs() < EPS, "got {}", full);
        // 纯背光：只吃环境光 0.33（ADR-0101：阴影下沉，山脉背光面在这）
        let dark = lighting_intensity(0.0, 0.0);
        assert!((dark - AMBIENT).abs() < EPS, "got {}", dark);
        // 单灯半受光：0.33 + 0.52·0.5 = 0.59
        let half = lighting_intensity(0.5, 0.0);
        assert!((half - 0.59).abs() < EPS, "got {}", half);
        // 负点积截断：0.33 + 0 + 0.15·0.5 = 0.405（max(0,·) 的手算证据）
        let clamped = lighting_intensity(-1.0, 0.5);
        assert!((clamped - 0.405).abs() < EPS, "got {}", clamped);
        // 平地（法线 z=1，两灯各得自身 z 分量）：0.33 + 0.52·0.6156615
        //   + 0.15·0.7660444 = 0.7650512（手算锚点）
        let flat = lighting_intensity(LIGHT_MAIN[2], LIGHT_AUX[2]);
        assert!(
            (flat - 0.7650512).abs() < 1e-4,
            "got {flat}（期望 0.33 + 0.52·{} + 0.15·{}）",
            LIGHT_MAIN[2],
            LIGHT_AUX[2]
        );
        // 界内性对抽样点成立。任意 [0,1] 点积入参下 I ∈ [0.33, 1.0]
        //（系数和 = 1）；(1,1) 这种「双灯正对」在真实法线下几何不可达，
        // 真实可达上限单独验（下一条断言）。
        for &(n1, n2) in &[
            (0.0, 0.0),
            (1.0, 1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
            (0.5, 0.5),
            (LIGHT_MAIN[2], LIGHT_AUX[2]),
            (1e-6, 1e-6),
        ] {
            let i = lighting_intensity(n1, n2);
            assert!(
                (AMBIENT - EPS..=1.0 + EPS).contains(&i),
                "N·L=({n1},{n2}) -> I={} 越界",
                i
            );
        }
        // 几何可达上限：n ∝ 0.52·L₁ + 0.15·L₂（函数文档的手算），
        // I_max = 0.33 + |0.52L₁ + 0.15L₂| ≈ 0.9354 —— ADR-0101 明暗比
        // ≈2.8:1 的来源。
        let vx = 0.52 * LIGHT_MAIN[0] + 0.15 * LIGHT_AUX[0];
        let vy = 0.52 * LIGHT_MAIN[1] + 0.15 * LIGHT_AUX[1];
        let vz = 0.52 * LIGHT_MAIN[2] + 0.15 * LIGHT_AUX[2];
        let len = (vx * vx + vy * vy + vz * vz).sqrt();
        let (vx, vy, vz) = (vx / len, vy / len, vz / len);
        let dot1 = vx * LIGHT_MAIN[0] + vy * LIGHT_MAIN[1] + vz * LIGHT_MAIN[2];
        let dot2 = vx * LIGHT_AUX[0] + vy * LIGHT_AUX[1] + vz * LIGHT_AUX[2];
        let i_max = lighting_intensity(dot1, dot2);
        assert!(
            (i_max - 0.9354).abs() < 1e-3,
            "几何可达上限应 ≈0.9354，得 {i_max}"
        );
    }

    #[test]
    fn lighting_coefficients_sum_to_one() {
        // 和 = 1 是「免钳制」的前提（模块文档）；漂移即破坏界内性。
        let sum = AMBIENT + DIFFUSE_MAIN + DIFFUSE_AUX;
        assert!((sum - 1.0).abs() < EPS, "got {}", sum);
    }

    #[test]
    fn horn_gradient_hand_fixture() {
        // 左列全 0、右列全 10（中列 0）：
        // hx = (10 + 2·10 + 10 − 0)/8 = 5.0；行向对称 → hy = 0.0
        let cols = grid3(3, 3, &[0, 0, 10, 0, 0, 10, 0, 0, 10]);
        let (hx, hy) = horn_gradient(&cols, 1, 1);
        assert!((hx - 5.0).abs() < EPS, "got {}", hx);
        assert!((hy - 0.0).abs() < EPS, "got {}", hy);
        // 行向坡：顶行全 0、底行全 10 → hy = 40/8 = 5.0，hx = 0.0
        let rows = grid3(3, 3, &[0, 0, 0, 0, 0, 0, 10, 10, 10]);
        let (hx, hy) = horn_gradient(&rows, 1, 1);
        assert!((hx - 0.0).abs() < EPS, "got {}", hx);
        assert!((hy - 5.0).abs() < EPS, "got {}", hy);
    }

    #[test]
    fn lighting_constants_are_unit_vectors_at_declared_elevation() {
        // ADR-0101：方位 315°/45°、高度角 38°/50° ⇒ 单位向量
        // (±cos(el)/√2, cos(el)/√2, sin(el))，模长 1——`lighting_intensity`
        // 的「直接 dot」契约以此为前提，模长漂移即界内性失效（见该函数
        // 历史注记）。锚点：cos38°/√2 = 0.5572078、sin38° = 0.6156615；
        // cos50°/√2 = 0.4545195、sin50° = 0.7660444（手算）。
        for (l, y_ref, z_ref) in [
            (LIGHT_MAIN, 0.5572078, 0.6156615),
            (LIGHT_AUX, 0.4545195, 0.7660444),
        ] {
            let len2 = l[0] * l[0] + l[1] * l[1] + l[2] * l[2];
            assert!((len2 - 1.0).abs() < 1e-6, "模长² = {len2} 应为 1");
            assert!(
                (l[1] - y_ref).abs() < 1e-6,
                "y = {:?} 应为 cos(el)/√2",
                l[1]
            );
            assert!((l[2] - z_ref).abs() < 1e-6, "z = {:?} 应为 sin(el)", l[2]);
        }
        assert!(
            (LIGHT_MAIN[0] + LIGHT_MAIN[1]).abs() < EPS,
            "主光从西北（x = −y）"
        );
        assert!(
            (LIGHT_AUX[0] - LIGHT_AUX[1]).abs() < EPS,
            "辅光从东北（x = +y）"
        );
    }

    #[test]
    fn normal_world_north_facing_slope_is_lit_by_main_light() {
        // 高度向南增大（hy > 0）→ 坡面朝北 → normal[1] > 0（y 翻转锚点）。
        // 45° 坡：normalize(0, 1, 1) = (0, 1/√2, 1/√2)。
        let n = normal_world(0.0, 1.0, 1.0);
        assert!(n[1] > 0.0, "got {}", n[1]);
        assert!((n[1] - FRAC_1_SQRT_2).abs() < EPS, "got {}", n[1]);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((len - 1.0).abs() < EPS, "got {}", len);
        // 单位光向量的原始点积即合法入参。45° 坡两分量各 1/√2：
        // dot₁ = (1/√2)·(0.5572078 + 0.6156615) ≈ 0.8293438
        // dot₂ = (1/√2)·(0.4545195 + 0.7660444) ≈ 0.8630689
        let dot = n[0] * LIGHT_MAIN[0] + n[1] * LIGHT_MAIN[1] + n[2] * LIGHT_MAIN[2];
        assert!(dot > 0.0, "got {}", dot);
        assert!(
            (dot - FRAC_1_SQRT_2 * (LIGHT_MAIN[1] + LIGHT_MAIN[2])).abs() < EPS,
            "got {}",
            dot
        );
        let i = lighting_intensity(
            dot,
            n[0] * LIGHT_AUX[0] + n[1] * LIGHT_AUX[1] + n[2] * LIGHT_AUX[2],
        );
        // 手算绝对锚点：0.33 + 0.52·0.8293438 + 0.15·0.8630689 ≈ 0.890_716。
        assert!((i - 0.890_716).abs() < 1e-4, "got {i}");
    }

    #[test]
    fn same_relative_slope_yields_same_normal_across_maps() {
        // 同一**相对**坡（高差占全图跨度同比例）：甲图跨度 10000、hx=250；
        // 乙图跨度 100、hx=2.5——梯度/跨度同为 0.025。
        // （Z = Z_EXAG，校准落参不影响等式）——归一化的全部意义：法线与
        // 作者高度单位、图幅大小无关。
        let a = normal_world(250.0, 0.0, slope_gain_of(10000.0));
        let b = normal_world(2.5, 0.0, slope_gain_of(100.0));
        for k in 0..3 {
            assert!(
                (a[k] - b[k]).abs() < EPS,
                "分量 {} 不等: a={} b={}",
                k,
                a[k],
                b[k]
            );
        }
        // 手算锚点：s·hx = Z/40 → normalize(−Z/40, 0, 1)；
        // 分母 = √((Z/40)² + 1)，随校准落参自动跟随 Z_EXAG。
        let g = Z_EXAG / 40.0;
        let denom = (g * g + 1.0).sqrt();
        assert!((a[0] - (-g / denom)).abs() < EPS, "got {}", a[0]);
        assert!((a[2] - (1.0 / denom)).abs() < EPS, "got {}", a[2]);
    }

    #[test]
    fn height_span_of_covers_land_range_with_floor() {
        // ADR-0101：跨度 = 陆地（h > 0）max−min（作者单位）；
        // [100, -50, 250] 的海底 −50 **不参与**分母 → 250 − 100 = 150。
        let data = grid3(3, 1, &[100, -50, 250]);
        assert!(
            (height_span_of(&data) - 150.0).abs() < EPS,
            "got {}",
            height_span_of(&data)
        );
        let flat = grid3(3, 1, &[7, 7, 7]);
        assert!(
            (height_span_of(&flat) - 1.0).abs() < EPS,
            "got {}",
            height_span_of(&flat)
        );
        // 全海图：无 h > 0 格 → 回落下限 1（gain 有限、梯度全零，无害）。
        let sea = grid3(3, 1, &[-5, -9, -2]);
        assert!(
            (height_span_of(&sea) - 1.0).abs() < EPS,
            "got {}",
            height_span_of(&sea)
        );
    }

    #[test]
    fn extreme_cliff_stays_finite_and_in_bounds() {
        // 邻格 ±9000 断崖：hx = (9000·4 − (−9000)·4)/8 = 9000 作者单位/格，
        // 过渡缺省跨度 10000（s = 25/10000）→ s·hx = 22.5，法线有限且界内。
        let cliff = grid3(3, 3, &[-9000, 0, 9000, -9000, 0, 9000, -9000, 0, 9000]);
        let (hx, hy) = horn_gradient(&cliff, 1, 1);
        assert!((hx - 9000.0).abs() < 1.0, "got {}", hx);
        assert!(hy.is_finite(), "got {}", hy);
        let n = normal_world(hx, hy, slope_gain_of(DEFAULT_HEIGHT_SPAN));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(len.is_finite(), "got {}", len);
        assert!((len - 1.0).abs() < 1e-4, "got {}", len);
        let i = lighting_intensity(
            n[0] * LIGHT_MAIN[0] + n[1] * LIGHT_MAIN[1] + n[2] * LIGHT_MAIN[2],
            n[0] * LIGHT_AUX[0] + n[1] * LIGHT_AUX[1] + n[2] * LIGHT_AUX[2],
        );
        assert!(
            (AMBIENT - 1e-4..=0.928 + 1e-4).contains(&i),
            "断崖明度越界: got {}",
            i
        );
    }

    #[test]
    fn horn_gradient_clamps_at_grid_edges() {
        // 1×1：九邻域全 clamp 到同一格 → 梯度必为 0，不 panic。
        let one = grid3(1, 1, &[42]);
        let (hx, hy) = horn_gradient(&one, 0, 0);
        assert_eq!((hx, hy), (0.0, 0.0));
        // 2×2（全格都是边界格），角落 (0,0) 邻域 clamp 手算：
        // 右下格 30、右上 10、左下 20 → hx = (10+20+30 − 0−0−20)/8 = 5.0
        // hy = (20+40+30 − 0−0−10)/8 = 10.0
        let two = grid3(2, 2, &[0, 10, 20, 30]);
        let (hx, hy) = horn_gradient(&two, 0, 0);
        assert!((hx - 5.0).abs() < EPS, "got {}", hx);
        assert!((hy - 10.0).abs() < EPS, "got {}", hy);
        // 越界坐标也要 clamp 而非 panic（调用方是逐格烘焙，防御在函数内）。
        let (hx, hy) = horn_gradient(&two, -5, 99);
        assert!(hx.is_finite() && hy.is_finite(), "got ({}, {})", hx, hy);
        // 全水 fixture：surface=0 不参与梯度，平高场 → 零梯度。
        let water = grid3(3, 3, &[120; 9]);
        let (hx, hy) = horn_gradient(&water, 1, 1);
        assert_eq!((hx, hy), (0.0, 0.0));
    }

    #[test]
    fn radius_and_weight_constants_are_pinned() {
        // 三尺度常量锚点（ADR-0077 D3/D6 建制；ADR-0101 改值 2026-11-05）：
        // GPU 侧 `terrain_field.wgsl` 以**字面量**镜像三档盒半径与三权重
        //（Rust 真源 → WGSL 镜像是人工同步，无编译期检查），本测试钉死
        // 真源字面值——任何人改常量都会先在这里撞红，从而有意识地同步
        // WGSL 镜像，不会静默漂移。
        assert!(
            (MEDIUM_RADIUS_CELLS - 3.0).abs() < EPS,
            "got {}",
            MEDIUM_RADIUS_CELLS
        );
        assert!(
            (MACRO_RADIUS_CELLS - 8.0).abs() < EPS,
            "got {}",
            MACRO_RADIUS_CELLS
        );
        // DETAIL_RADIUS_CELLS（= 1.0）没有 Rust 常量（只活在 WGSL），其
        // 概念锚 = detail 权重衰减边界的 smoothstep(1, 3)——两个边界常量
        // 钉死即钉住「detail 档 r=1、缩放过渡 1–3 px/格」的整套口径。
        assert!(
            (DETAIL_FADE_MIN_PX_PER_CELL - 1.0).abs() < EPS,
            "got {}",
            DETAIL_FADE_MIN_PX_PER_CELL
        );
        assert!(
            (DETAIL_FADE_MAX_PX_PER_CELL - 3.0).abs() < EPS,
            "got {}",
            DETAIL_FADE_MAX_PX_PER_CELL
        );
        // 三尺度权重字面值（和 = 1 由权重混合测试另行保证）。
        assert!((W_MACRO - 0.35).abs() < EPS, "got {}", W_MACRO);
        assert!((W_MEDIUM - 0.45).abs() < EPS, "got {}", W_MEDIUM);
        assert!((W_DETAIL - 0.20).abs() < EPS, "got {}", W_DETAIL);
        // ADR-0101 新家族：山脊高光 / LCH 色度补偿 / 固定水色（WGSL 镜像
        // 同一人工同步纪律）。
        assert!(
            (RIDGE_DOT_THRESH - 0.80).abs() < EPS,
            "got {}",
            RIDGE_DOT_THRESH
        );
        assert!(
            (RIDGE_SLOPE_DEG - 18.0).abs() < EPS,
            "got {}",
            RIDGE_SLOPE_DEG
        );
        assert!((RIDGE_BOOST - 0.06).abs() < EPS, "got {}", RIDGE_BOOST);
        assert!(
            (SHADE_CHROMA_BOOST - 0.12).abs() < EPS,
            "got {}",
            SHADE_CHROMA_BOOST
        );
        assert!(
            (WATER_SRGB[0] - 0.1647059).abs() < 1e-6
                && (WATER_SRGB[1] - 0.3647059).abs() < 1e-6
                && (WATER_SRGB[2] - 0.5607843).abs() < 1e-6,
            "WATER_SRGB = {:?} 应为 #2A5D8F",
            WATER_SRGB
        );
    }

    #[test]
    fn scale_weights_sum_to_one_and_detail_fades_monotonically() {
        // 手算 smoothstep(1, 3, ·)：t = (ppc−1)/2，s = t²(3−2t)。
        // ppc=2.0 → t=0.5 → fade=0.5 → detail = 0.20·0.5 = 0.10，
        // 回流 0.10 按 35:45 分：macro = 0.35+0.10·(35/80) = 0.39375，
        // medium = 0.45+0.10·(45/80) = 0.50625（ADR-0101 权重）。
        let mid = scale_weights(2.0);
        assert!((mid[2] - 0.10).abs() < EPS, "got {}", mid[2]);
        assert!((mid[0] - 0.39375).abs() < EPS, "got {}", mid[0]);
        assert!((mid[1] - 0.50625).abs() < EPS, "got {}", mid[1]);
        // 远观 0.5 px/格：fade=0 → detail=0，全额回流。
        let far = scale_weights(0.5);
        assert!((far[2] - 0.0).abs() < EPS, "got {}", far[2]);
        // 近观 ≥ 3 px/格：detail 全额 0.20，回到基线 (0.35, 0.45, 0.20)。
        for ppc in [3.0, 4.0] {
            let near = scale_weights(ppc);
            assert!(
                (near[2] - W_DETAIL).abs() < EPS,
                "ppc={} got {}",
                ppc,
                near[2]
            );
            assert!(
                (near[0] - W_MACRO).abs() < EPS,
                "ppc={} got {}",
                ppc,
                near[0]
            );
            assert!(
                (near[1] - W_MEDIUM).abs() < EPS,
                "ppc={} got {}",
                ppc,
                near[1]
            );
        }
        // 每个采样点三权和 = 1 ± 1e-6，detail 单调不减（ADR-0077 D3 口径）。
        let mut prev_detail = -1.0;
        let ppc_samples = [0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0];
        for &ppc in &ppc_samples {
            let w = scale_weights(ppc);
            let sum = w[0] + w[1] + w[2];
            assert!((sum - 1.0).abs() < EPS, "ppc={} sum={}", ppc, sum);
            assert!(
                w[2] >= prev_detail,
                "ppc={} detail={} < prev={}",
                ppc,
                w[2],
                prev_detail
            );
            prev_detail = w[2];
        }
    }

    #[test]
    fn lch_shade_is_identity_at_full_shade() {
        // shade=1：无色度补偿（shadow_factor=0）+ L 不缩 → 回路恒等
        //（±1e-4 是 XYZ↔Lab 往返的浮点残差）。
        for rgb in [
            [0.5, 0.5, 0.5],
            [0.2, 0.7, 0.3],
            [0.1118, 0.2120, 0.0908], // ramp 低阶 #5E7F55 的线性值（手算）
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
        ] {
            let out = lch_shade(rgb, 1.0, SHADE_CHROMA_BOOST);
            for k in 0..3 {
                assert!(
                    (out[k] - rgb[k]).abs() < 1e-4,
                    "通道 {k}: in {:?} out {:?}",
                    rgb,
                    out
                );
            }
        }
    }

    #[test]
    fn lch_shade_white_halves_to_l50_gray() {
        // 白（linear 1,1,1）→ L=100、C=0；shade=0.5 → L=50 灰。
        // 手算锚点：L50 灰的线性值 = ((50+16)/116)³ = 0.184187（三通道同值）。
        let out = lch_shade([1.0, 1.0, 1.0], 0.5, SHADE_CHROMA_BOOST);
        for (k, v) in out.iter().enumerate() {
            assert!((v - 0.184187).abs() < 1e-4, "通道 {k} = {v}，期望 0.184187");
        }
    }

    #[test]
    fn lch_shade_keeps_hue_order_and_neutral_never_tints() {
        // 色相保持：绿色在深阴影下仍是「G 最大」，只是变暗。
        let green = lch_shade([0.2, 0.7, 0.3], 0.3, SHADE_CHROMA_BOOST);
        assert!(
            green[1] > green[0] && green[1] > green[2],
            "got {:?}",
            green
        );
        assert!(green[1] < 0.7, "明度应下降: got {:?}", green);
        // 中性灰不染色：C < 1e-4 跳过补偿，三通道严格相等（发灰但不发彩）。
        let gray = lch_shade([0.5, 0.5, 0.5], 0.2, SHADE_CHROMA_BOOST);
        assert!(
            (gray[0] - gray[1]).abs() < 1e-6 && (gray[1] - gray[2]).abs() < 1e-6,
            "灰被染色: got {:?}",
            gray
        );
        // 输出界内。
        let deep = lch_shade([0.9, 0.1, 0.1], 0.05, SHADE_CHROMA_BOOST);
        assert!(
            deep.iter().all(|c| (0.0..=1.0).contains(c)),
            "got {:?}",
            deep
        );
    }
}
