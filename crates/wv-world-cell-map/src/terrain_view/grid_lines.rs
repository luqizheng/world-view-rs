//! 逐格编辑网格（plan v0.1 D4）：笔刷开启或手动勾选时显示的格线叠加层。
//!
//! ## 职责
//! - 在**栅格幅面**（`tex.size × grid.cell_size`，`grid.raster_origin` 起）上
//!   画格线。可见性 = `view.grid_lines` ∧ Terrain 图层可见 ∧
//!   `RenderRecipe.enabled(GridLines)`（后两条见 wv-render 的 `sync_grid_lines`）。
//! - 提供**纯函数规格**（[`grid_coarse_stride`] / [`grid_line_alpha`] …）与
//!   **材质 + WGSL**（[`GridLinesMaterial`]），系统本体在 wv-render 的
//!   `cellmap_bridge`（要读 `ViewState`，ADR-0054 D5）。
//!
//! ## 2026-10-23 重写（迭代档缺陷修复 · 用户症状「开了网格看不到」）
//!
//! ### 旧实现错在哪（三重）
//! 1. **整层退场**：格距 `< 2px` 时 fade = 0 → 实体 despawn。而世界级「全图
//!    适配」`scale = 5.12`（`zoom_level::fit`）× `cell_size = 4.0`（lod-demo：
//!    extent 4096 / raster 1024）→ `cell_px = 0.78` → **fade 恒 0**。也就是说
//!    「打开 CellMap 就看不到网格」，要看到得先放大 2.6 倍。
//! 2. **逐档重建**：`GridKey` 含 `scale_bits`（`view.scale` 精确位比较）→
//!    滚轮每推一档（×1.12 / ×0.88）都整层 despawn + `grid_chains(1024,1024)`
//!    （2050 条链）+ `build_stroke_mesh` + `meshes.add` + `Assets::remove`。
//! 3. **线宽靠 CPU**：1px 屏幕线宽要换算成世界宽度，天然与 scale 绑定，
//!    这正是第 2 条的病根。
//!
//! ### 现在的模型：一个 quad + 屏幕空间 shader，双档网格
//! - mesh = **1 个**覆盖栅格幅面的矩形，只在 `size / height / cell_size /
//!   raster_origin` 变时重建；**缩放只改 uniform，而本设计连 uniform 都不用改**
//!   （格距/线宽全部由 shader 的 `fwidth` 从世界坐标导数推出，见 WGSL）。
//! - 双档：格距 ≥ [`GRID_FINE_FULL_PX`] 画每格细线；格子更密时**不再退场**，
//!   改画「每 `stride` 格一条」粗线，`stride` 取最小 2 的幂使屏上定位格
//!   ≥ [`GRID_COARSE_MIN_PX`]（[`grid_coarse_stride`]）—— 任意缩放下屏上都
//!   有 ≥ 8px 的定位格。细则淡出、粗则恒在（[`grid_line_alpha`]）。
//! - 线宽 1px 由 `fwidth` 保证（[`GRID_LINE_PX`] 进 uniform `tune.x`）。
//!
//! ## 边界（不做什么）
//! - **不画参考网格**——1/4 幅面参考网格是 `map_frame` 的（浏览态也在）；
//! - **不再让 `grid_chains` 进渲染热路径**——它是 CPU 版格线展开器，
//!   保留为**探针锚点与规格参照**（`perf_probe::perf_probe_grid_lines_rebuild`
//!   量的就是「旧路径一档多少钱」）。若下轮确认无消费者，可删。
//!
//! [ADR-0054]: https://…ADR-0054-world-cell-map-crate

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

// ── 可见性常量（全部有数字来源，禁裸数字）──────────────────────────────────

/// 网格线本色的 alpha（作者未给 Terrain 图层设 opacity 时的值）。
///
/// 2026-10-23：`0.35 → 0.48`。旧值在 1px 线宽 + 远观淡出的双重削弱下，
/// 叠在 `#C9B58A` 一带的中等亮度地形上几乎读不出来（用户症状的直接成因之一）。
/// 0.48 落在验收要求的 `0.45–0.5` 区间中部：再高就与等值线墨色
/// （`ISOLINE_BASE_ALPHA = 1.0`）抢视线，再低则又回到「看不见」。
pub const GRID_LINE_BASE_ALPHA: f32 = 0.48;

/// 粗档（降采样档）相对基色的 alpha 系数。
///
/// 粗档屏上格距恒 ≥ [`GRID_COARSE_MIN_PX`]，本身不需要淡出；0.85 是
/// 「骨架比细档轻一档」的表态：远观时画面只剩地形 + 粗格，格子不压地形。
pub const GRID_COARSE_ALPHA_K: f32 = 0.85;

/// Z 序：底图块（0.0）之上、等值线描边（0.04）之下。
pub const GRID_Z: f32 = 0.03;

/// 网格线屏幕宽度（px）。`fwidth` 的分辨率下限就是 1px，再粗才会看出双像素边。
pub const GRID_LINE_PX: f32 = 1.0;

/// 细档（每格一条）**开始**淡出的格距（px）：格距 ≥ 此值全显。
///
/// 4px 沿用 2026-10-15 的实测判断（「还能数清格子」的下限）；真值来源是从
/// 「1px 线 + 1px 间隙 + 1px 线 + 1px 间隙」的最小可辨周期 = 4px。
pub const GRID_FINE_FULL_PX: f32 = 4.0;

/// 细档**完全退出**的格距（px）：≤ 此值不画细线（粗档接手，不再整层退场）。
///
/// 2px = 1px 线宽的 2 倍：此时每格被线占去一半面积，细线只剩摩尔纹。
pub const GRID_FINE_MIN_PX: f32 = 2.0;

/// 粗档定位格的**最小**屏上边长（px）。
///
/// 2026-10-23 二次定标：`8.0 → 4.0`，与 QA/制作人统一的可达参数一致
/// （`m = min{2^n : 2^n × cell_px ≥ 4}`，`n ≤ 5`）。取 4 而不是 8 的理由比原来更硬：
/// **4px 正是细档的「全显」锚点**（[`GRID_FINE_FULL_PX`]）——细档在格距 4px 时
/// 刚好读得最清，再密就淡出；粗档接过它，把「读得清的最小间距」作为下限，
/// 两档在同一格线可读性口径上交接，不会出现「细档已淡、粗档还稀」的空档。
/// 手算：fit 下 `cell_px = 0.78125` → `m = 8` → 定位格 6.25px（≥4px ✓）。
pub const GRID_COARSE_MIN_PX: f32 = 4.0;

/// 粗档倍率上限的指数（`stride ≤ 2^5 = 32`）。
///
/// 需要多大的倍率由 `cell_px` 决定：4096² 栅格（`cell_size = 1.0`）在
/// `scale = 8.0` 时细档 0.125px，要顶到 4px 需 `stride = 32`——所以上限必须
/// ≥ 32（QA 的 A4 复算）。再大意味着「一格已经小到看不见」，粗档也失去定位意义。
pub const GRID_COARSE_MAX_EXP: u32 = 5;

/// 粗档倍率的硬上限（`2^GRID_COARSE_MAX_EXP`）。WGSL 侧从 uniform `mix.y` 读同一个值
/// ——上限只在这一处定义，shader 不得自带字面量。
pub const GRID_COARSE_MAX_MULT: u32 = 1 << GRID_COARSE_MAX_EXP;

// ── 纯函数规格（Rust 是规格，WGSL 是镜像实现；两侧必须逐值一致）─────────────

/// 每格的屏上边长（px）。`scale` 口径 = 世界单位/屏幕像素（大 = 远）。
///
/// `scale` 取 `max(1e-6)` 只为防 0/负（相机钳制下限远大于 0，正常路径不会触发）。
pub fn grid_cell_px(cell_size: f32, scale: f32) -> f32 {
    cell_size / scale.max(1e-6)
}

/// 细档淡出系数：格距 `cell_px` 在 `[MIN, FULL]` 间线性，端点外钳制。
///
/// 纯函数锚点：`4px→1`、`3px→0.5`、`2px→0`。**=0 只表示「细档不画」**——
/// 2026-10-23 起它不再意味着「整层退场」（旧语义见文件头）。
pub fn grid_line_fade(cell_px: f32) -> f32 {
    ((cell_px - GRID_FINE_MIN_PX) / (GRID_FINE_FULL_PX - GRID_FINE_MIN_PX)).clamp(0.0, 1.0)
}

/// 粗档倍率：最小 2 的幂 `s` 使 `cell_px × s ≥ GRID_COARSE_MIN_PX`，且 `s ≤ 32`。
///
/// 为什么必须是 2 的幂：`s` 倍的格线必须**落在格缘上**（`k·s` 格处），
/// 否则粗线与细线错拍，看着像「网格歪了」。2 的幂还让倍率在缩放时按整档跳变，
/// 不会出现「粗线慢慢爬」的漂移观感。
pub fn grid_coarse_stride(cell_px: f32) -> u32 {
    // `cell_px <= 0.0` 对 NaN 为假，故必须另判有限性（NaN 走 return 1）。
    if cell_px <= 0.0 || !cell_px.is_finite() {
        return 1;
    }
    let mut s = 1u32;
    while (cell_px * s as f32) < GRID_COARSE_MIN_PX && s < GRID_COARSE_MAX_MULT {
        s *= 2;
    }
    s
}

/// QA 口径的粗档倍率（`coarse_multiplier`）：`min{2^n : 2^n × cell_px ≥ 4}`，`n ≤ 5`。
///
/// 与 [`grid_coarse_stride`] 同值，`u8` 是 QA 验收单要求的签名（倍率最大 32，
/// `u8` 装得下）。两个名字并存是刻意的：`grid_coarse_stride` 是本 crate 的
/// 表述（强调「整格步长」），`coarse_multiplier` 是验收单的表述（强调「倍率」）——
/// 单测钉住两者恒等，改一个忘了改另一个会红。
pub fn coarse_multiplier(cell_px: f32) -> u8 {
    grid_coarse_stride(cell_px) as u8
}

/// 第 `index` 格上的格线是否属于粗档（`index ≡ 0 (mod stride)`）。
///
/// 这是「作者按粗线定位不会偏半格」的**唯一判据**：粗线必须落在格索引整数倍上，
/// 而不是落在某个与 scale 相关的浮点偏移上。WGSL 侧的等价写法是
/// `fract(cell / stride)` 在整数处为 0（`cell / stride` 取整即 `index / stride`）。
pub fn coarse_line_hits(index: i64, stride: u32) -> bool {
    let s = stride.max(1) as i64;
    index.rem_euclid(s) == 0
}

/// 粗档定位格的屏上边长（px）= `cell_px × stride`，恒 ≥ `GRID_COARSE_MIN_PX`
/// （除非撞上 [`GRID_COARSE_MAX_EXP`] 上限）。
pub fn grid_coarse_px(cell_px: f32) -> f32 {
    cell_px * grid_coarse_stride(cell_px) as f32
}

/// 屏上一像素实际落到的网格 alpha（不含图层 opacity）。
///
/// = `GRID_LINE_BASE_ALPHA × max(细档淡出, 粗档系数)`——与 WGSL 的
/// `max(m_fine × fine, m_coarse × mix.x) × ink.a` 同构：某像素要么被细线覆盖、
/// 要么被粗线覆盖（粗线是细线的子集），取较强者。
///
/// **恒 > 0**：这是本轮修的语义——「开关开 = 一定看得见」。旧实现的
/// `fade = 0 ⇒ 整层退场` 不再存在。
pub fn grid_line_alpha(cell_px: f32) -> f32 {
    GRID_LINE_BASE_ALPHA * grid_line_fade(cell_px).max(GRID_COARSE_ALPHA_K)
}

/// 同上，再叠图层 opacity（消费侧 `layers.terrain.opacity`）。
pub fn grid_line_alpha_with_opacity(cell_px: f32, opacity: f32) -> f32 {
    grid_line_alpha(cell_px) * opacity.clamp(0.0, 1.0)
}

/// 网格线的**屏上宽度**（px）——可断言的「1px 屏幕常量」口径。
///
/// 返回恒为 [`GRID_LINE_PX`]，**与 `scale` 无关**。这不是「算出来恰好相等」，
/// 而是结构决定的：线宽由 WGSL 的 `fwidth(cell)` 归一化，`fwidth` 的单位就是
/// 「每像素」，所以线宽天然是屏幕量。`scale` 参数保留只为让调用方（验收测试 /
/// 诊断面板）能按「给什么 scale、要什么宽度」提问，并顺手断言「问什么都不变」。
pub fn grid_screen_width_px(_cell_size: f32, _scale: f32) -> f32 {
    GRID_LINE_PX
}

// ── 材质 ────────────────────────────────────────────────────────────────────

/// 网格材质的内建 shader（`load_internal_asset!` 注入，不走 AssetServer）。
///
/// **为什么不用文件路径**：`Material2d::fragment_shader()` 返回的
/// `ShaderRef::Path` 由 AssetServer 相对 **asset 根**解析，而 asset 根随
/// 运行目录变化（example 与 app 不同，见 `terrain_field_demo` 的踩坑注释）。
/// 网格是 CellMap 的常驻层，不能因为「资产根没配好」就静默不画——
/// 编译期内联是唯一无环境依赖的接法。UUID 一旦发布不得修改（句柄身份）。
pub const GRID_LINES_SHADER: Handle<Shader> = uuid_handle!("e40b1d7a-93c2-4f58-8a16-2d7c54be9081");

/// 网格材质。双档判定与线宽全在 WGSL 里（见 `grid_lines.wgsl`）。
#[derive(AsBindGroup, Debug, Clone, Asset, TypePath)]
pub struct GridLinesMaterial {
    #[uniform(0)]
    pub params: GridParams,
}

impl Material2d for GridLinesMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(GRID_LINES_SHADER)
    }

    /// 网格是叠加线：必须混合。`Opaque`（默认）会让格线把底图整片盖成纯墨色。
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// 与 WGSL `GridParams` 逐字段对应的 uniform 块。
///
/// **全部用 `Vec4`，不用 `[f32; 4]`**：encase 把 `[f32; 4]` 当 `array<f32, 4>`
/// （stride 4），而 WGSL uniform 要求数组 stride 是 16 的倍数 → 运行期 panic
/// `array stride must be a multiple of 16`。此坑在 `field/material.rs` 已有
/// 血书记录，这里按同一纪律写。
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GridParams {
    /// xy = 每格世界边长（`GridConfig.cell_size`）；zw = 栅格原点（世界坐标）。
    pub geo: Vec4,
    /// rgb = 墨色（**线性空间**，与 shader 输出同空间）；a = 本帧基色 alpha
    /// （已含 `GRID_LINE_BASE_ALPHA × layers.terrain.opacity`）。
    pub ink: Vec4,
    /// x = [`GRID_LINE_PX`]；y = [`GRID_FINE_MIN_PX`]；z = [`GRID_FINE_FULL_PX`]；
    /// w = [`GRID_COARSE_MIN_PX`]。四个阈值以 uniform 下发——**改常量即改 shader
    /// 行为，不用重编 shader**（也让两侧不会各写一份数字而漂移）。
    pub tune: Vec4,
    /// x = [`GRID_COARSE_ALPHA_K`]；y = [`GRID_COARSE_MAX_MULT`]（倍率硬上限，
    /// shader 的 `exp2(ceil(log2(need)))` 必须与 CPU 的 `min(..., 上限)` 一致，
    /// 否则两端在极远观会给出不同倍率）；zw 预留。
    pub mix: Vec4,
}

/// 组装 uniform。`ink` 用 `LinearRgba`（`Color::to_linear()` 得来），
/// 避免 sRGB 值直接进 uniform 后被再亮一次（`field/material.rs` 踩过的坑）。
pub fn grid_params(
    cell_size: f32,
    raster_origin: Vec2,
    ink: LinearRgba,
    base_alpha: f32,
) -> GridParams {
    GridParams {
        geo: Vec4::new(cell_size, cell_size, raster_origin.x, raster_origin.y),
        ink: Vec4::new(ink.red, ink.green, ink.blue, base_alpha),
        tune: Vec4::new(
            GRID_LINE_PX,
            GRID_FINE_MIN_PX,
            GRID_FINE_FULL_PX,
            GRID_COARSE_MIN_PX,
        ),
        mix: Vec4::new(GRID_COARSE_ALPHA_K, GRID_COARSE_MAX_MULT as f32, 0.0, 0.0),
    }
}

/// 材质 + shader 的注册插件。
///
/// 组合根（wv-app）装一次即可；`wv-render` 的桥接系统只做「建 mesh / 建材质 /
/// 挂实体」，不注册管线。
pub struct GridLinesMaterialPlugin;

impl Plugin for GridLinesMaterialPlugin {
    fn build(&self, app: &mut App) {
        // shader 走**编译期内联**（见 [`GRID_LINES_SHADER`] 的理由）。
        bevy::asset::load_internal_asset!(
            app,
            GRID_LINES_SHADER,
            "grid_lines.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(Material2dPlugin::<GridLinesMaterial>::default());
    }
}

// ── 重算键与资源 ────────────────────────────────────────────────────────────

/// 重算键：**只含结构性输入**——`view.scale` 刻意不在其中。
///
/// 旧键含 `scale_bits`（精确位比较），滚轮每档都整层重建（1024² 栅格
/// 2050 条链 + 全量网格上传）。现在线宽/格距全由 shader 的 `fwidth` 从
/// 世界坐标导数推出，**缩放对 CPU 侧零工作**：不重建、不写 uniform。
#[derive(Debug, Clone, PartialEq)]
pub struct GridKey {
    /// 逻辑格列数。
    pub size: u32,
    /// 逻辑格行数（非方形地图下 != `size`）。
    pub height: u32,
    /// 每格世界边长（位比较，精确）。
    pub cell_bits: u64,
    /// 栅格原点（位比较）。
    pub origin: (u64, u64),
    /// 主题包 id——换主题要换墨色（走 uniform 快路径就够，但幅面几何也随之
    /// 重新校验一次，成本是一次矩形 mesh，可忽略）。
    pub palette: &'static str,
}

/// 资源：网格实体 + 资产句柄 + 上次构建键 + **重建计数**。
#[derive(Resource, Default)]
pub struct GridLinesOverlay {
    pub entity: Option<Entity>,
    pub mesh: Option<Handle<Mesh>>,
    pub material: Option<Handle<GridLinesMaterial>>,
    pub key: Option<GridKey>,
    /// mesh 重建次数（**公开计数字段**，2026-10-23 验收断言 2 用：
    /// 缩放 12 档触发的重建次数必须为 0）。只统计「结构重建」，
    /// uniform 写入不计——那才是「每帧可承受」与「每档不可承受」的分界。
    pub rebuilds: u32,
    /// 最近一次写进 uniform 的 `[r, g, b, a]`（线性色 + 基色 alpha）。
    /// 用于幂等短路：本帧算出的值与上次相同就不碰 `Assets::get_mut`
    /// （`get_mut` 会置 `Modified` 事件，白让资产管线跑一遍）。
    pub ink_applied: Option<[f32; 4]>,
}

impl GridLinesOverlay {
    /// 退场：实体 + **两个资产**一并回收（不留泄漏；验收断言 3）。
    ///
    /// 材质也回收是刻意的：材质只持有一个 uniform 块，随手重建的成本是
    /// 「一次 `Assets::add`」，而留着不删会让「开关网格 100 次」积累 100 份
    /// 死资产（旧实现正是这样漏的——只删 mesh 不删 material）。
    pub fn despawn(&mut self, commands: &mut Commands) {
        if let Some(entity) = self.entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(mesh) = self.mesh.take() {
            commands.queue(move |world: &mut World| {
                if let Some(mut assets) = world.get_resource_mut::<Assets<Mesh>>() {
                    assets.remove(&mesh);
                }
            });
        }
        if let Some(material) = self.material.take() {
            commands.queue(move |world: &mut World| {
                if let Some(mut assets) = world.get_resource_mut::<Assets<GridLinesMaterial>>() {
                    assets.remove(&material);
                }
            });
        }
        self.key = None;
        self.ink_applied = None;
    }

    /// 就地刷新 uniform（墨色 / 透明度）——**不重建 mesh**。
    ///
    /// 幂等：与上次写入的值相同即返回。这是「缩放不花钱」的另一半：
    /// 缩放根本不调它；只有换主题 / 拖图层透明度才走到这里，一次 f32 比较。
    pub fn refresh_ink(
        &mut self,
        materials: &mut Assets<GridLinesMaterial>,
        ink: LinearRgba,
        base_alpha: f32,
    ) {
        let target = [ink.red, ink.green, ink.blue, base_alpha];
        if self.ink_applied == Some(target) {
            return;
        }
        if let Some(handle) = &self.material {
            if let Some(mut mat) = materials.get_mut(handle) {
                mat.params.ink = Vec4::from_array(target);
            }
        }
        self.ink_applied = Some(target);
    }
}

/// 幅面 `width × height` 格的格线链：`(width+1)` 竖 + `(height+1)` 横，格坐标。
///
/// **渲染已不走这里**（2026-10-23 改 shader）。保留原因：
/// 1. `perf_probe` 用它量「旧路径一档多少钱」——构建档位决策的判据；
/// 2. 它是「细档/粗档链数」的唯一可复算口径（`tests/acceptance_grid_visibility.rs`
///    A5 断言 2050 / 258 条链）；
/// 3. 未来若要在 CPU 侧预览网格（导出图片等），这是现成的展开器。
pub fn grid_chains(
    width: u32,
    height: u32,
    half_w: f32,
) -> Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> {
    grid_chains_strided(width, height, 1, half_w)
}

/// 粗档格线链（`stride` 格一条）：`width/stride + 1` 竖 + 同法横。
///
/// 供 QA 的「粗档对齐」断言直接消费（`stride = 8` 的 1024² 幅面 = 129 竖 +
/// 129 横 = 258 条链，手算可复算）。`half_w` 是**常量**入参——本函数不含
/// `view.scale`，这是与旧实现的分界：几何只认格单位。
pub fn grid_chains_strided(
    width: u32,
    height: u32,
    stride: u32,
    half_w: f32,
) -> Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> {
    use wv_terrain::isoline::Pt;
    let s = stride.max(1);
    let mut chains = Vec::new();
    let pt = |x: f32, y: f32| Pt { x, y };
    let (w, h) = (width as f32, height as f32);
    let mut i = 0u32;
    while i <= width {
        let f = i as f32;
        chains.push((vec![pt(f, 0.0), pt(f, h)], false, half_w));
        i = match i.checked_add(s) {
            Some(n) => n,
            None => break,
        };
    }
    let mut j = 0u32;
    while j <= height {
        let f = j as f32;
        chains.push((vec![pt(0.0, f), pt(w, f)], false, half_w));
        j = match j.checked_add(s) {
            Some(n) => n,
            None => break,
        };
    }
    chains
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lod-demo 世界图口径：extent 4096 / raster 1024 → `cell_size = 4.0`。
    const CELL: f32 = 4.0;
    /// 世界级全图适配：`max(4096/1280, 4096/800) = 5.12`。
    const FIT: f32 = 5.12;

    /// 双档核心语义：`fit` 下细档已退出（fade = 0），但**整体 alpha 仍 > 0**。
    #[test]
    fn grid_stays_visible_at_world_fit() {
        let px = grid_cell_px(CELL, FIT);
        assert!((px - 0.78125).abs() < 1e-9, "4.0/5.12 = 0.78125，实得 {px}");
        assert_eq!(grid_line_fade(px), 0.0, "细档在 0.78px 格距下应退出");
        assert!(
            grid_line_alpha(px) > 0.0,
            "细档退出不等于整层退场：粗档必须还在（旧实现的 bug 正是这里）"
        );
        assert!(
            (grid_line_alpha(px) - GRID_LINE_BASE_ALPHA * GRID_COARSE_ALPHA_K).abs() < 1e-6,
            "只有粗档时 alpha = 基色 × 粗档系数"
        );
    }

    /// 粗档倍率：屏上定位格恒 ≥ 4px（可读下限），且倍率是 2 的幂、不超过 32。
    #[test]
    fn coarse_stride_keeps_locator_cells_readable() {
        for scale in [0.16_f32, 0.5, 1.0, 2.0, FIT, 8.0, 50.0] {
            let px = grid_cell_px(CELL, scale);
            let stride = grid_coarse_stride(px);
            assert!(
                stride.is_power_of_two(),
                "scale {scale}：倍率 {stride} 必须是 2 的幂（粗线要落格缘）"
            );
            assert!(stride <= GRID_COARSE_MAX_MULT, "倍率不得超过上限 32");
            let coarse = px * stride as f32;
            assert!(
                coarse >= GRID_COARSE_MIN_PX - 1e-4 || stride == GRID_COARSE_MAX_MULT,
                "scale {scale}：粗档格距 {coarse}px 应 ≥ {GRID_COARSE_MIN_PX}px"
            );
            // 最小性：再降一档就不够 4px（撞上限时豁免）
            if stride > 1 && stride < GRID_COARSE_MAX_MULT {
                assert!(
                    px * ((stride / 2) as f32) < GRID_COARSE_MIN_PX,
                    "scale {scale}：倍率 {stride} 不是最小可用值"
                );
            }
        }
    }

    /// `fit` 下的具体倍率：0.78px → 4 不够（3.125px）→ 8（6.25px）。
    #[test]
    fn fit_scale_lands_on_stride_eight() {
        let px = grid_cell_px(CELL, FIT);
        assert_eq!(grid_coarse_stride(px), 8, "0.78125 × 8 = 6.25px ≥ 4px");
        assert!((grid_coarse_px(px) - 6.25).abs() < 1e-4);
        assert_eq!(
            coarse_multiplier(px),
            8,
            "QA 口径的 coarse_multiplier 与 grid_coarse_stride 必须同值"
        );
    }

    /// 粗档倍率上限 32：4096² 栅格（`cell_size 1.0`）最远端 scale 8.0 也要够。
    #[test]
    fn coarse_multiplier_caps_at_thirty_two() {
        // 1024² 栅格最远端：cell_size 4.0 / scale 8.0 = 0.5px → 4px 需 m=8
        assert_eq!(coarse_multiplier(0.5), 8);
        // 4096² 栅格最远端：cell_size 1.0 / scale 8.0 = 0.125px → 4px 需 m=32
        assert_eq!(coarse_multiplier(0.125), 32);
        // 再小就不再加倍（上限），避免「一格小到看不见还很密」
        assert_eq!(coarse_multiplier(0.01), GRID_COARSE_MAX_MULT as u8);
        assert_eq!(GRID_COARSE_MAX_MULT, 32);
    }

    /// 粗线落点：第 `i` 格属于粗档 ⇔ `i ≡ 0 (mod m)`（作者按粗线定位不偏半格）。
    #[test]
    fn coarse_lines_land_on_multiples_of_the_multiplier() {
        let m = coarse_multiplier(grid_cell_px(CELL, FIT)) as u32;
        assert_eq!(m, 8);
        let hits: Vec<i64> = (0..=24).filter(|i| coarse_line_hits(*i, m)).collect();
        assert_eq!(hits, vec![0, 8, 16, 24], "粗线只落在 8 的整数倍格上");
        assert!(
            coarse_line_hits(-8, m) && !coarse_line_hits(-1, m),
            "负格号同样按 mod 对齐（rem_euclid 不含 0 的偏向）"
        );
        // CPU 的落点口径与 WGSL（fract(cell/m) 在整数处为 0）同构：两者都在 index/m ∈ ℤ 处落线。
        let straddled: Vec<i64> = (0..=16).filter(|i| coarse_line_hits(*i, 8)).collect();
        assert_eq!(straddled.len(), 3, "0/8/16 三条");
    }

    /// 粗档链数（QA 的 A5 口径）：1024² 幅面 stride 8 → 129 竖 + 129 横 = 258。
    #[test]
    fn strided_chains_match_the_hand_computed_count() {
        let chains = grid_chains_strided(1024, 1024, 8, 0.5);
        assert_eq!(chains.len(), 2 * (1024 / 8 + 1), "258 条链");
        assert_eq!(chains.len(), 258);
        // 落点全在 8 的整数倍格上
        for (pts, _, _) in &chains {
            let x = pts[0].x as i64;
            let y = pts[0].y as i64;
            assert!(
                coarse_line_hits(x, 8) || coarse_line_hits(y, 8),
                "链首 ({x},{y}) 不在 stride 8 的格缘上"
            );
        }
        // stride 1 等价于细档（旧 grid_chains 口径不变）
        assert_eq!(
            grid_chains(8, 8, 0.5).len(),
            grid_chains_strided(8, 8, 1, 0.5).len()
        );
    }

    /// 线宽是屏幕常量：任意 `cell_size` / `scale` 下都答 1px（结构决定，非巧合）。
    #[test]
    fn screen_width_is_scale_invariant() {
        let widths: Vec<f32> = [0.16_f32, 0.5, 1.0, 2.0, 5.12, 8.0, 50.0]
            .iter()
            .map(|s| grid_screen_width_px(CELL, *s))
            .collect();
        assert!(
            widths.iter().all(|w| (*w - GRID_LINE_PX).abs() < 1e-9),
            "50× 缩放跨度内线宽必须恒为 {GRID_LINE_PX}px，实得 {widths:?}"
        );
        assert!(widths.iter().all(|w| (*w - 1.0).abs() < 1e-9), "手算 1px");
    }

    /// 细档全显区的 alpha = 基色（粗档更淡，被 max 吃掉）。
    #[test]
    fn fine_tier_full_alpha_at_four_px_and_above() {
        assert_eq!(grid_line_fade(GRID_FINE_FULL_PX), 1.0);
        assert!((grid_line_alpha(GRID_FINE_FULL_PX) - GRID_LINE_BASE_ALPHA).abs() < 1e-6);
        assert!(
            (grid_line_alpha(48.0) - GRID_LINE_BASE_ALPHA).abs() < 1e-6,
            "近观不更亮"
        );
    }

    /// 单调性：格子越密 alpha 不增（细档淡出 → 粗档接管），且永不为 0。
    #[test]
    fn alpha_never_drops_to_zero_as_cells_get_denser() {
        let mut prev = grid_line_alpha(0.01);
        for px in [0.01_f32, 0.5, 1.0, 2.0, 3.0, 4.0, 8.0, 32.0] {
            let a = grid_line_alpha(px);
            assert!(
                a > 0.0,
                "cell_px {px} 的 alpha 必须 > 0（开关开就一定看得见）"
            );
            assert!(a >= prev - 1e-6, "cell_px {px} 的 alpha 不应高于更密的档");
            prev = a;
        }
    }

    /// 极端输入不 panic、不出 NaN（相机钳制失效 / 除零的兜底）。
    #[test]
    fn degenerate_inputs_are_safe() {
        for scale in [0.0_f32, -1.0, f32::MIN_POSITIVE] {
            let px = grid_cell_px(CELL, scale);
            assert!(px.is_finite() && px >= 0.0, "scale {scale} → cell_px {px}");
            assert!(grid_coarse_stride(px) >= 1);
            assert!(grid_line_alpha(px) > 0.0);
        }
        assert_eq!(grid_coarse_stride(f32::NAN), 1, "NaN 退化为倍率 1");
    }

    /// uniform 布局：每个字段都是有限值（NaN 会让整个 uniform 静默失效），
    /// 且四个阈值与常量同源。
    #[test]
    fn grid_params_mirror_constants() {
        let p = grid_params(CELL, Vec2::new(100.0, 200.0), LinearRgba::WHITE, 0.48);
        assert_eq!(p.geo, Vec4::new(CELL, CELL, 100.0, 200.0));
        assert_eq!(
            p.tune,
            Vec4::new(
                GRID_LINE_PX,
                GRID_FINE_MIN_PX,
                GRID_FINE_FULL_PX,
                GRID_COARSE_MIN_PX
            )
        );
        assert_eq!(p.mix.x, GRID_COARSE_ALPHA_K);
        for v in [p.geo, p.ink, p.tune, p.mix] {
            assert!(v.is_finite(), "uniform 出现非有限值: {v:?}");
        }
    }

    /// CPU 规格与 WGSL 的接线必须对得上：shader 自己**不持有任何阈值字面量**，
    /// 四个阈值与粗档系数只能来自 uniform（`tune` / `mix`）。改了 Rust 常量
    /// 却漏接 uniform，会在这里立刻红。
    #[test]
    fn wgsl_reads_every_threshold_from_uniform() {
        let src = include_str!("grid_lines.wgsl");
        for needle in [
            "p.tune.x", // 线宽 px
            "p.tune.y", // 细档退出 px
            "p.tune.z", // 细档全显 px
            "p.tune.w", // 粗档最小 px
            "p.mix.x",  // 粗档 alpha 系数
            "p.mix.y",  // 粗档倍率上限（CPU/GPU 必须同值）
            "fwidth(cell.x)",
            "exp2(ceil(log2(need)))",
        ] {
            assert!(
                src.contains(needle),
                "grid_lines.wgsl 缺 `{needle}`——Rust 常量与 shader 的接线断了"
            );
        }
        // shader 不得用 Rust 常量名（那是「同一事实两个载体」的入口）
        for forbidden in [
            "GRID_LINE_PX",
            "GRID_FINE_MIN_PX",
            "GRID_FINE_FULL_PX",
            "GRID_COARSE_MIN_PX",
            "GRID_COARSE_ALPHA_K",
        ] {
            assert!(
                !src.contains(forbidden),
                "grid_lines.wgsl 出现了 Rust 常量名 `{forbidden}`——阈值只能走 uniform"
            );
        }
    }

    /// 退场清空全部状态（含 uniform 缓存），重进时不会误判「已写过」。
    #[test]
    fn despawn_resets_key_and_ink() {
        let mut overlay = GridLinesOverlay {
            key: Some(GridKey {
                size: 8,
                height: 8,
                cell_bits: 4.0f32.to_bits() as u64,
                origin: (0, 0),
                palette: "daylight",
            }),
            ink_applied: Some([1.0, 1.0, 1.0, 1.0]),
            rebuilds: 3,
            ..Default::default()
        };
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            overlay.despawn(&mut commands);
        }
        queue.apply(&mut world);
        assert!(overlay.key.is_none() && overlay.ink_applied.is_none());
        assert!(overlay.entity.is_none() && overlay.mesh.is_none());
        assert_eq!(overlay.rebuilds, 3, "历史重建计数不该被 despawn 抹掉");
    }
}
