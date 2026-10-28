# CellMap 真实地形晕渲 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** CellMap 高度差可读——双光源美术光照 + Horn 米制梯度 + 三尺度概括（远观见山脉、近观见支脉）+ 低地色带连续渐变，替换现行「逐格差分 + 钳制 + 远观关灯」路线。

**Architecture:** 公式真源收在新模块 `wv-world-cell-map/src/shading.rs`（纯函数 + 全套单测 = WGSL 的参照实现）；WGSL `terrain_field.wgsl` 同公式改写（shader 无法单测，靠 Rust 参照 + smoke + WV_SHOT 验证）。CPU 块烘焙路径（cells.rs）接单尺度双光源。色带连续渐变 CPU/GPU 同步。零数据层/存储改动。

**Tech Stack:** Rust 2021 / Bevy 0.19 / WGSL。无新第三方依赖。

**Spec:** `docs/superpowers/specs/2026-10-28-cellmap-relief-shading-design.md`（D1–D7 决策 + §8 裁量，本计划与之冲突时以 spec 为准）

## Global Constraints

- **分支纪律（AGENTS §13）**：Task 1 先建分支 `feat/cellmap-relief-shading` + worktree `.worktrees/feat/cellmap-relief-shading`（目录名=分支名）；构建用全局共享 `CARGO_TARGET_DIR`（不覆盖）。
- **测试环境**：worktree 内跑测试**不用** `scripts/cargo-test.sh`；先 `export TEMP=D:\kudi-projects\worldview-rs\.target\testtmp; export TMP=$TEMP` 再 `cargo test`。
- **分层红线**：改动全部落 `wv-world-cell-map`（允许依赖 wv-core / wv-terrain / bevy，均非 optional）与 `wv-render/src/cellmap_bridge.rs`（既有桥接点）；wv-core / wv-terrain / wv-io / wv-edit **零改动**；wv-world-cell-map 禁依赖任何 theme crate（固定色 crate 级禁令，ADR-0054 D3）。
- **uniform 双边同改**：`FieldParams`（Rust，`ShaderType` derive）与 WGSL `FieldParams` struct 必须同一提交内同步改——不同步编译期不报错，运行时上传 uniform 才炸（material.rs:270 注释警告的既知坑）。
- **公式唯一真源**：光照 / 梯度 / 权重常量只在 `shading.rs` 定义一次；material.rs / WGSL 引用或镜像（WGSL 侧 `const` 与 Rust 侧 `pub const` 数值必须一致，注释互指）。
- **P1–P7**：零新增存储字段、零格式变更；纯渲染常量 + shader + CPU 烘焙公式。
- **测试命名**：完整英文短句；数值断言附 `got {}` 上下文；公式类断言用手算期望值。
- **无 UI 文案改动**：本计划不含 i18n / 图标。
- **每任务收尾**：`cargo test -p wv-world-cell-map` 绿（涉及 wv-render 的任务加 `cargo app-test` / `cargo app-clippy`）+ `cargo fmt --all -- --check` 后才 commit；提交信息 `<scope>: <动作>` 中文。

## Review Focus

1. **无比例尺地图**（`MapNode.unit_per_km == None`）：晕渲必须照常工作——回落「1 wu = 1 m」假设并记待决，不得 panic 或除零 → T2 `meters_per_cell_without_scale_falls_back_to_one_wu_one_m`。
2. **全平地形**：整幅 I 恒等于平地值 `0.4 + 0.6·(√2/2) ≈ 0.8243`（不产生梯度噪点、不改色相）→ T4 `flat_ground_mul_equals_flat_intensity_everywhere`。
3. **极小栅格 / 全水**（1×1、2×2、全水）：Horn 邻域钳界取格，不 panic、不越界 → T2 `horn_gradient_clamps_at_grid_edges` + T4 沿用。
4. **极端陡崖**（邻格 ±9000 m）：法线有限、I 恒在 `[0.4, 1.0]`（新模型自带界内性，钳制已退役）→ T2 `extreme_cliff_stays_finite_and_in_bounds`。
5. **缩放 scrub 抖动**（px_per_cell 在 1–3 间来回）：三档权重和恒 1、detail 衰减单调，画面无闪跳 → T2 `scale_weights_sum_to_one_and_detail_fades_monotonically` + T9 实机两档 WV_SHOT。

---

### Task 1: 分支 + ADR-0077 + GLOSSARY + 待决登记

**Files:**
- Create: `docs/adr/ADR-0077-cellmap-relief-shading.md`
- Modify: `docs/adr/README.md`（索引；标注修正 ADR-0067 D2「专业制图浮雕调轻」与 ADR-0055「8 阶离散」两条关系）
- Modify: `GLOSSARY.md`（多尺度晕渲 multi-scale hillshading / 双光源 dual-light / Horn 算子 / 垂直夸张 vertical exaggeration / 连续色带 continuous hypsometric ramp）
- Modify: `README.md` §12（待决新增：无比例尺地图的米制假设——当前按 1 wu = 1 m 回落，标注 `TODO(阶段: 晕渲)` 由 T2 注释指向）

**Interfaces:** 无代码。后续任务的术语与常量名以 ADR/spec 为准。

- [ ] **Step 1: 建分支与 worktree**（`git branch feat/cellmap-relief-shading && git worktree add .worktrees/feat/cellmap-relief-shading feat/cellmap-relief-shading`）
- [ ] **Step 2: 写 ADR-0077**：处境（宽缓地形读不出 + 远观 T33 关灯 + 低地 200 m 一刀同色）/ 决策（D1–D6）/ 理由 / 代价（CPU 路径单尺度例外、uniform 槽位重载、Z_EXAG 需实机校准）+ 被否决备选（调轻钳制路线、R32Float、真实 mip 链、CPU 三尺度）
- [ ] **Step 3: GLOSSARY 五词条 + README §12 待决条目**
- [ ] **Step 4: Commit** `docs: ADR-0077 CellMap 真实地形晕渲 + 术语与待决登记`

### Task 2: shading.rs 公式真源（纯函数 + 全套单测）

**Files:**
- Create: `crates/wv-world-cell-map/src/shading.rs`（`mod shading;` 挂进 `crates/wv-world-cell-map/src/lib.rs`）
- Test: 同文件 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `wv_core::terrain::TerrainData`（`width/height/cells: Vec<TerrainCellData>`，`TerrainCellData.height: i16` 米）。
- Produces（T4/T5/T6/T7 全部依赖，签名钉死）:

```rust
pub const AMBIENT: f32 = 0.4;
pub const DIFFUSE_MAIN: f32 = 0.48;
pub const DIFFUSE_AUX: f32 = 0.12;
pub const Z_EXAG: f32 = 2.0;                    // 用户给定初始值；验收期可校准（唯一改动点）
pub const W_MACRO: f32 = 0.45;                  // 三尺度权重（和 = 1）
pub const W_MEDIUM: f32 = 0.40;
pub const W_DETAIL: f32 = 0.15;
pub const MEDIUM_RADIUS_CELLS: f32 = 3.0;       // 中观盒平均足迹（格）
pub const MACRO_RADIUS_CELLS: f32 = 12.0;       // 宏观盒平均足迹（格）
/// 主光 L₁：方位 315°（西北）、高度角 45°（与现行 LIGHT_DIR 同向）
pub const LIGHT_MAIN: [f32; 3] = [-FRAC_1_SQRT2, FRAC_1_SQRT2, FRAC_1_SQRT2];
/// 辅光 L₂：方位 45°（东北）、高度角 45°
pub const LIGHT_AUX: [f32; 3] = [FRAC_1_SQRT2, FRAC_1_SQRT2, FRAC_1_SQRT2];

pub fn meters_per_cell_of(cell_size_wu: f32, unit_per_km: Option<f32>) -> f32;
pub fn slope_gain_of(meters_per_cell: f32) -> f32;              // = Z_EXAG / mpc
pub fn lighting_intensity(n_dot_l1: f32, n_dot_l2: f32) -> f32; // 0.4 + 0.48·max(0,·) + 0.12·max(0,·)
pub fn horn_gradient(data: &TerrainData, x: i64, y: i64) -> (f32, f32); // (hx, hy) 米/格；+y=行号向下（南）
pub fn normal_world(hx: f32, hy: f32, slope_gain: f32) -> [f32; 3];     // normalize(−s·hx, +s·hy, 1)
pub fn detail_weight_fade(px_per_cell: f32) -> f32;             // smoothstep(1.0, 3.0, ppc)
pub fn scale_weights(px_per_cell: f32) -> [f32; 3];             // [macro, medium, detail]，和恒 1
```

- `meters_per_cell_of`：`Some(k) → cell_size_wu × 1000.0 / k`；`None → cell_size_wu`（1 wu = 1 m 假设，注释指向 README §12 待决 + `TODO(阶段: 晕渲)`）。`k <= 0` 按 `None` 处理。
- `horn_gradient`：Horn 3×3，`hx = (右列加权和 − 左列加权和) / 8`（[1,2,1]ᵀ·[1,2,1] 权重），`hy` 同理行向；邻域 `clamp` 在图内（0..width-1 / 0..height-1）；返回**米/格**（i16 直接作 f32 差分，不再乘 H_RANGE）。
- `normal_world` 的 **+s·hy**（不是 −s）：数据行号向下增大（南），世界 +y = 北，y 分量翻转一次——与现行 `cells.rs::relief_mul` 的 `lit = −(dx+dy)` 受光约定一致。**此符号是 CPU/GPU 对齐锚点，WGSL 侧梯度在纹理空间（+v 向南），法线写 `normalize(vec3(−zx, −zy, 1.0))` 不再翻**（纹理 v 与数据行同向、与世界 y 反向，翻转已隐含）——两侧注释互指此说明。

- [ ] **Step 1: 写失败测试**（全部手算锚点）：
  - `lighting_intensity_matches_hand_values_and_stays_in_bounds`（N·L₁=1, N·L₂=1 → 1.0；双 0 → 0.4；单 0.5/0 → 0.4+0.24=0.64；1e-6）
  - `lighting_coefficients_sum_to_one`（三常数和 = 1.0）
  - `horn_gradient_hand_fixture`（3×3：左列全 0、右列全 10 → hx=5.0 m/格，hy=0.0；再构造行向坡断言 hy）
  - `normal_world_north_facing_slope_is_lit_by_main_light`（高度向南增大 hy>0 → normal[1]>0 → dot(normal, LIGHT_MAIN)>0）
  - `same_physical_slope_yields_same_normal_across_maps`（100 m/格 + mpc=100 与 50 m/格 + mpc=50 → normal 逐分量相等 1e-6）
  - `extreme_cliff_stays_finite_and_in_bounds`（邻格 ±9000 → normal 全分量 finite，lighting ∈ [0.4, 1.0]）
  - `horn_gradient_clamps_at_grid_edges`（1×1 / 2×2 / 全水 fixture 不 panic 且返回有限值）
  - `meters_per_cell_without_scale_falls_back_to_one_wu_one_m`（None → cell_size_wu；Some(8) → ×125）
  - `scale_weights_sum_to_one_and_detail_fades_monotonically`（ppc=0.5 → detail=0；ppc≥3 → 0.15；ppc 0.5→4 采样单调；每处三权和 = 1±1e-6）
- [ ] **Step 2: 跑测试确认失败**（模块不存在，编译错误）：`cargo test -p wv-world-cell-map shading`
- [ ] **Step 3: 实现**全部函数与常量（文档注释写「为什么」：美术化光照非物理模拟、和=1 免钳制、+s·hy 的 y 翻转锚点说明）
- [ ] **Step 4: 跑测试确认通过**：`cargo test -p wv-world-cell-map`
- [ ] **Step 5: Commit** `wv-world-cell-map: shading 公式真源——双光源/Horn 米制梯度/三尺度权重`

### Task 3: 色带连续插值（CPU 侧）

**Files:**
- Modify: `crates/wv-world-cell-map/src/terrain_view/hypsometric.rs`
- Test: 同文件既有 `mod tests`

**Interfaces:**
- Consumes: 既有 `RAMP_SRGB` / `Cuts` / `band_of`（不动）。
- Produces（T4 消费）:
  - `pub fn segment_of(height: i16, cuts: &Cuts) -> (usize, f32)` —— `(段号 b, 段内分数 t)`：`b` = 既有 `band_of` 口径，`t ∈ [0,1)` = 在 `[cuts[b−1], cuts[b])` 内的位置（b=0 时下界按 cuts[0] 的等宽外推：`t = height / cuts[0]`，height<0 已被调用侧水域门挡住，负值钳 0）；t 钳 `[0, 1)`。
  - `pub fn tint_continuous(base: [f32; 4], height: i16, cuts: &Cuts, strength: f32) -> [f32; 4]` —— sRGB 空间（与既有 `tint_rgba` 同口径）：`ramp[b]` 与 `ramp[b+1]` 按 `t` lerp（b=7 时 t 恒 0，即纯 `ramp[7]`），再按 `strength` 混入 base。

- [ ] **Step 1: 写失败测试**：
  - `continuous_tint_matches_discrete_at_band_anchors`（高度恰在 cuts 各点上 → 与 `tint_rgba(base, b+1, strength)` 逐通道相等 1e-6；高度 0 → 与 `tint_rgba(base, 0, strength)` 相等）
  - `continuous_tint_interpolates_within_band`（DOMAIN_CUTS 下 height=350（band 1，200–500 段 t=0.5）→ 结果 = ramp[1] 与 ramp[2] 的中点色 1e-6）
  - `continuous_tint_keeps_lowland_green_not_washed`（公园绿 `[0xC9,0xE7,0xC7]` + height=50 + strength=1 → 结果 g 通道仍为最大分量（绿不被洗白））
  - `continuous_tint_strength_zero_returns_base`（沿用）
  - `segment_fraction_is_monotonic_within_band`（band 0 内 0/50/100/150/199 → t 严格递增）
- [ ] **Step 2: 跑测试确认失败**：`cargo test -p wv-world-cell-map tint_continuous`
- [ ] **Step 3: 实现**两函数（`segment_of` 内部复用 `band_of` 求段号；文档注释写明「8 锚点连续渐变」对 ADR-0055 的修正关系）
- [ ] **Step 4: 跑测试确认通过**
- [ ] **Step 5: Commit** `wv-world-cell-map: 海拔色带 8 锚点段内连续渐变（CPU）`

### Task 4: CPU 块烘焙路径切换（单尺度双光源 + 连续色带接线）

**Files:**
- Modify: `crates/wv-world-cell-map/src/terrain_view/cells.rs:206-250`（RELIEF_PER_METER / RELIEF_MIN / RELIEF_MAX / RELIEF_MIN_CARTOGRAPHIC / RELIEF_MAX_CARTOGRAPHIC 五常量退役；`relief_mul` / `relief_mul_clamped` 删除）
- Modify: `crates/wv-world-cell-map/src/terrain_view/cells.rs:1384-1407`（浮雕段换 `shading` 单尺度）
- Modify: `crates/wv-world-cell-map/src/terrain_view/cells.rs:1273-1274`（Topographic 色带换 `tint_continuous`）
- Modify: 烘焙输入结构（cells.rs:598-606 区域的参数结构体）增 `slope_gain: f32` 字段；构造点透传（构造点在 cells.rs `new`/装配区 ~:640-715；其上游调用侧若缺 map 上下文，沿调用链从 GridConfig.cell_size + 当前图 `MapNode.unit_per_km` 传入——检索 `unit_per_km` 在 wv-render/wv-app 的既有读取点复用同源）
- Test: cells.rs 既有 `mod tests`（改写 relief 三测）+ 新增

**Interfaces:**
- Consumes: T2 全部、T3 `tint_continuous`。
- Produces: CPU 烘焙路径的浮雕 = `shading::lighting_intensity` 单尺度（macro/medium/detail 三权重在此路径退化为 detail 全额 1.0——D4 例外）；Topographic 色带 = 连续渐变。

- [ ] **Step 1: 改写既有测试**（语义随模型更新，测试名照实改）：
  - `relief_is_flat_on_flat_ground` → `flat_ground_mul_equals_flat_intensity_everywhere`（平地 fixture 任意格 `mul == lighting_intensity(FRAC_1_SQRT2, FRAC_1_SQRT2) ± 1e-6`，即 ≈0.8243）
  - `relief_brightens_left_top_slopes_and_darkens_the_opposite` → `north_and_west_facing_slopes_are_lit_south_and_east_shaded`（沿用 fixture 方向断言，按新法线口径）
  - `relief_is_clamped_for_extreme_cliffs` → `extreme_cliff_mul_stays_finite`（钳制已退役，改断言有限性 + I 界内）
  - 新增 `relief_applies_single_scale_on_cpu_path`（slope_gain 透传：构造时给 `Some(8)` 与 `None` 两种单位比例 → mul 不同且均有限——钉住透传链通了）
  - 色带：`topographic_lowland_uses_continuous_tint`（含 50 m 陆格的块烘焙结果与 `tint_continuous` 直接调用一致）
- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**：浮雕段替换为 `let (hx, hy) = shading::horn_gradient(self.data, cx, gy); let n = shading::normal_world(hx, hy, self.slope_gain); let i = shading::lighting_intensity(dot(n, LIGHT_MAIN), dot(n, LIGHT_AUX)); color *= i`（land-only 门与三模式门 `VectorFlat|RealTerrain|GoogleMap` **原样保留**——三分支现在共走同一公式，GoogleMap 特判分支删除）；色带段 `band_of+tint_rgba` 换 `segment_of` 不需要——直接 `tint_continuous(rgba, cell.height, cuts, self.hypsometric)`；五常量与两函数删除
- [ ] **Step 4: 跑 `cargo test -p wv-world-cell-map` 全绿**
- [ ] **Step 5: Commit** `wv-world-cell-map: CPU 烘焙接双光源单尺度晕渲 + Topographic 连续色带`

### Task 5: GPU uniform 通道（行为零变化）

**Files:**
- Modify: `crates/wv-world-cell-map/src/terrain_view/field/material.rs`（FieldParams :275-324、填充点 :563-579、常量区 :101-121）
- Modify: `crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl:35-52`（WGSL FieldParams 镜像，**公式段本任务不动**）
- Modify: `crates/wv-render/src/cellmap_bridge.rs:1690-1880`（px_per_cell 填充区增 meters_per_cell / light_aux 写入）
- Test: material.rs / cellmap_bridge 既有测试区新增（Rust 侧可测部分）

**Interfaces:**
- Consumes: T2 常量与 `meters_per_cell_of`。
- Produces（T6/T7/T8 消费）——uniform 语义重载：
  - `light_dir.w`：`0.0`（预留）→ **meters_per_cell**（0 或非有限 = 无效，shader 回落 1.0）。
  - `relief`：`(RELIEF_LO, RELIEF_HI, 0, 0)` → **(AMBIENT, DIFFUSE_MAIN, DIFFUSE_AUX, 预留)**。WGSL 侧 `relief.z` 现为死槽（已核实仅 `.x/.y` 被读），重载安全。
  - **末尾追加 `light_aux: Vec4`**（xyz = LIGHT_AUX，w 预留）——追加在 `elev_ramp` 之后，双边同改（std140 布局只在尾部追加才是安全的）。
  - `shape.z`：Z_EXAG 来源改 `shading::Z_EXAG`（2.0），`shape.w`（grad_step 死槽）维持 0。
- 桥接侧：`cellmap_bridge.rs` 在现读 `cell_size` 处（:1697 附近）取当前图 `unit_per_km`（检索该文件/`WorldProjection` 的 map 上下文既有读取点；无则沿调用链透传），`shading::meters_per_cell_of(cell_size, unit_per_km)` → `light_dir.w`；`light_aux` 静态填 LIGHT_AUX。

- [ ] **Step 1: 写失败测试**（Rust 侧）：`field_params_default_carries_dual_light_and_meter_scale`（默认 FieldParams：relief = (0.4, 0.48, 0.12, 0.0)、light_aux.xyz == LIGHT_AUX 1e-6、shape.z == 2.0）；`bridge_fills_meters_per_cell_from_map_scale`（cellmap_bridge 可测纯函数段：unit_per_km Some(8) + cell_size=100 wu → 12500.0；None → 100.0）
- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**三文件（Rust struct + WGSL struct 同提交；WGSL 公式段一行不动——`p.relief.x/y` 旧语义字段名此步不改，仅注释标注「本步为槽位预载，下一任务切公式」；`material.rs` 的 `Z_EXAG/RELIEF_LO/RELIEF_HI/LIGHT_DIR` 常量区改为引用 `shading` 常量，`GRAD_STEP` 注释标注死槽）
- [ ] **Step 4: 验证**：`cargo test -p wv-world-cell-map && cargo app-test && cargo app-clippy` + `bash scripts/smoke-gui.sh`（uniform 尺寸变化的运行期验证靠 smoke；画面应零变化）
- [ ] **Step 5: Commit** `wv-world-cell-map,wv-render: 场管线 uniform 预载——双光源槽/米制比例/Z_EXAG 2.0（行为零变化）`

### Task 6: WGSL 光照与梯度切换（detail 档 + 双光源 + 卸掉远观关灯）

**Files:**
- Modify: `crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl:163-180`（采样与明暗段）
- Test: 无 WGSL 单测——验证靠 T2 参照实现 + smoke + T9 WV_SHOT

**Interfaces:**
- Consumes: T5 uniform 槽位、T2 公式（WGSL 内联同式，常量镜像注释互指 shading.rs）。
- Produces: detail 档明暗 = 双光源单尺度；`lod_t` 对 relief 的收敛（wgsl:180 `mix(..., 1.0, lod_t)`）**删除**——T33 远观关灯退役（ADR-0077 记档），`lod_t` 对 `h` 与 coast 的混合**原样保留**。

- [ ] **Step 1: 实现**：梯度段换 Horn 3×3——`height_at(pos + (i,j))`（i,j ∈ {−1,0,1}，8 tap 双线性）替代 `sample_height_bicubic_grad` 的解析偏导（高度取值 `hg.x` 仍走双三次，`sample_height_bicubic_grad` 保留但只取 `.x`；其文档注释同步改写）；梯度归一化值 × `H_RANGE_M` = 米/格，`n = normalize(vec3(−zx·s, −zy·s, 1.0))`，`s = p.shape.z / mpc`、`mpc = p.light_dir.w`（≤0 回落 1.0）；明暗 `relief = 0.4 + 0.48·max(0, dot(n, normalize(p.light_dir.xyz))) + 0.12·max(0, dot(n, normalize(p.light_aux.xyz)))`；**删除 wgsl:176-180 的钳制与 lod_t 收敛**；`land_col = base * relief` 的陆/水门原样保留
- [ ] **Step 2: 验证**：`cargo app-test && cargo app-clippy` + `bash scripts/smoke-gui.sh`
- [ ] **Step 3: Commit** `wv-world-cell-map: WGSL 切双光源 + Horn 米制梯度，T33 远观关灯退役`

### Task 7: WGSL 三尺度混合（macro/medium/detail + detail 距离衰减）

**Files:**
- Modify: `crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl`（明暗段 + 新 helper 函数）
- Test: 同 T6 口径

**Interfaces:**
- Consumes: T6 的 detail Horn；T2 `scale_weights` / `MEDIUM_RADIUS_CELLS` / `MACRO_RADIUS_CELLS`（WGSL 镜像常量）。
- Produces: `I_final = w_macro·I_macro + w_medium·I_medium + w_detail·I_detail`，权重随 px_per_cell 按 `scale_weights` 混合（detail 远观趋零——D3 对 T33 的替换）。

- [ ] **Step 1: 实现**：
  - 新 helper `fn height_blur_at(pos: vec2<f32>, r: f32) -> f32` —— `r <= 1.0` 时 `height_at(pos)`，否则 `box_avg_height(pos, r)`（复用既有 4 tap 盒平均；r=12 的欠采样在注释中记为可接受——低通目标场本身平滑，§8 tap 预算优化另裁）。
  - 新 helper `fn horn_grad_at(pos: vec2<f32>, r: f32) -> vec2<f32>` —— 8 邻域 Horn，每点经 `height_blur_at(·, r)`；返回归一化梯度。
  - 明暗段：三档半径 `r_detail = 1.0` / `r_medium = 3.0` / `r_macro = 12.0`，各自 `horn_grad_at` → 法线 → `lighting_intensity` 同式；`let (w_macro, w_medium, w_detail) = scale_weights(px_per_cell)`（WGSL 内联手写同式）；`relief = 加权和`；替换 T6 的单尺度 relief。
- [ ] **Step 2: 验证**：`cargo app-test && cargo app-clippy` + `bash scripts/smoke-gui.sh`
- [ ] **Step 3: Commit** `wv-world-cell-map: WGSL 三尺度晕渲混合——宏观/中观/细节随缩放过渡`

### Task 8: WGSL 海拔连续渐变 + 陈旧注释清理

**Files:**
- Modify: `crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl:251-255`（CellMap 分支取色）与 `:730-755`（elev 辅助函数）
- Modify: `crates/wv-world-cell-map/src/terrain_view/field/material.rs:590`（陈旧注释：「按逐图百分位写入（ADR-0055）」→ 绝对阶梯（ADR-0056）+ 连续渐变（ADR-0077））
- Test: 同 T6 口径；锚点连续性已由 T3 CPU 侧测试钉住（uniform 同源色表）

**Interfaces:**
- Consumes: T5 的 `elev_cuts`（归一化域，不动）/ `elev_ramp`（不动）。
- Produces: GPU 取色 = 段内 lerp，与 T3 `tint_continuous` 同式（锚点处与旧离散色一致——CPU/GPU 锚点连续性同源成立）。

- [ ] **Step 1: 实现**：新 fn `elev_ramp_continuous(h_norm: f32) -> vec3<f32>`——沿 7 条 cuts 找段 `b` 与段内 `t`（线性扫描 `elev_cuts` 两 vec4，与现 `elev_band` 同口径），`mix(p.elev_ramp[b].rgb, p.elev_ramp[b+1].rgb, t)`（b=7 → 纯 ramp[7]）；wgsl:253 `elev_ramp_color(elev_band(h))` 换之；`elev_band` / `elev_ramp_color` 若无其他消费点则删除（检索确认）
- [ ] **Step 2: 验证**：`cargo app-test && cargo app-clippy` + `bash scripts/smoke-gui.sh`
- [ ] **Step 3: Commit** `wv-world-cell-map: WGSL 海拔色带段内连续渐变 + ADR-0055 陈旧注释清理`

### Task 9: 实机验收 + 全门禁收尾

- [ ] **Step 1: 全门禁**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
cargo app-test && cargo app-clippy
bash scripts/smoke-gui.sh
```

- [ ] **Step 2: 实机走查（WV_SHOT 或用户手跑）**：
  - **远观**（px_per_cell ≈ 0.5）：大型山脉结构可见（对比改前：浮雕整体关灯）——G2 验收。
  - **近观**（≈ 4 px/格）：支脉与局部坡面呈现，受光/背光两面可辨——G1 验收。
  - **宽缓 fixture**（300 m / 20 格坡）：两面明暗可辨；不可辨则按 spec 校准 `shading::Z_EXAG`（唯一改动点，初始 2.0，预期可能上调），校准结果写回 ADR-0077 追记。
  - **低地**：0–200 m 渐变可读、公园绿不洗白（白洗回归截图对比）。
  - **笔刷实时**：拖画帧时间不回退（对改前基线）；倒放 scrub 无闪跳。
- [ ] **Step 3: 遗漏修补后逐项 commit**；分支收尾走 finishing-a-development-branch（合并等用户指令）

---

## Self-Review 记录

- **Spec 覆盖**：D1=T2/T4/T5/T6；D2=T2/T4/T5/T6；D3=T2/T7（T33 替换在 T6 步骤 1 显式删除）；D4=T4（单尺度例外）；D5=T3/T4/T8；D6=T2/T5（常量唯一真源）；D7=T1；G5 验收=T9；§8 裁量各归任务（mip 级偏移→T7 盒半径；衰减曲线→T2/T7 线性 smoothstep；tap 预算→T7 注释留项；色值→未动，低端区分度留验收期）。无缺。
- **类型一致**：`meters_per_cell_of / slope_gain_of / lighting_intensity / horn_gradient / normal_world / scale_weights` 在 T2 定义、T4/T5/T6/T7 同名引用；`tint_continuous / segment_of` T3 定义 T4 消费；uniform 三槽位语义（light_dir.w / relief.xyz / light_aux）T5 钉死、T6/T7/T8 只读。
- **Review Focus 5 条**：1→T2/T5；2→T4；3→T2/T4；4→T2；5→T2+T9。
- **比例**：计划 ≈ spec 体量，代码块仅签名/常量与测试名，无实现正文。
