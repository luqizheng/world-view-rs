# CellMap 真实地形晕渲设计（双光源 + Horn 米制梯度 + 多尺度概括 + 低地色带修订）

- **日期**：2026-10-28
- **状态**：提议（brainstorm 收敛稿，待用户批准）
- **上游讨论**：CellMap 高度差呈现不足 → 专业制图地势表达三件套对齐 → 用户给出 Heightmap + 多方向晕渲 + Hypsometric + GPU Shader + 分块 LOD 架构与参数
- **后续**：批准后走 superpowers:writing-plans 出实施计划

## 1. 处境与问题

CellMap 已有三条「读高度」通道，但高度差读不出：

| 通道 | 实现 | 问题 |
|---|---|---|
| 坡度浮雕 | CPU `cells.rs::relief_mul` / GPU `field/material.rs` | **逐相邻格高差算明度**（0.0012/m，钳 ±12%/±16%）：宽缓地形（百米高差铺十几格）每格明度变化 1–3%，肉眼不可见；只有陡崖边缘亮——「山形有了、体量没有」 |
| 分层设色 | `hypsometric.rs` 8 阶 | 绝对阶梯首刀 200 m：0–200 m 整段**同色**，手绘世界主体恰在此段 → 低地一整片平色；窄域兜底（<300 m 跨度）覆盖不到典型图 |
| 等值线 | `isoline_overlay.rs` | 有，但密了难读、疏了无用，非本轮对象 |

根因两条（与参数调轻无关的结构性问题）：

1. **梯度无概括**：单尺度逐格差分只响应局部台阶，不响应宏观形态。专业界的答案是晕渲从**概括后的地形**算（Swisstopo/Google 先低通/多尺度概括再打光），我们缺这层。
2. **低段无分辨**：色带 8 阶在低端一刀 200 m，低地内部高度差完全不参与颜色。

用户期望（原话归纳）：山体连续而非孤立贴图；山脊受光/背光分明；缩放到整个州仍见大型山脉结构；放大后逐渐呈现局部坡面、支脉与细部；编辑实时（笔刷→脏块→上传→重算）。

## 2. 目标 / 非目标

**目标**

- G1 双光源美术光照替换现有单光源 + 明度钳制路线，宽缓地形（≥300 m 高差铺 15+ 格）受光/背光两面肉眼可辨。
- G2 多尺度概括：远观（px_per_cell≈0.5）见山脉宏观结构，近观（≈4 px/格）见支脉细节，随缩放平滑过渡。
- G3 梯度算子米制化：法线由真实米制高度 + cell 尺寸 + 垂直夸张算出，不同格距的图同物理坡得到同一法线。
- G4 低地色带分辨：0–200 m 段内部高度差可读，且不复活 2026-10-26 的「低地洗白」缺陷。
- G5 编辑实时性维持：笔刷 patch 路径性能不回退（现 <1ms 增量上传量级）。

**非目标**

- 3D 网格地形 / 视角旋转（2.5D texture-bake 已另有实现，不碰）。
- 高度纹理改 R32Float：现 R16Unorm 域编码步长 ≈0.4 m，晕渲精度足够，带宽省一半（已裁，不采纳）。
- 光照参数进主题包（ThemePack）：本轮全部 crate 常量；主题化留未来 ADR。
- 计曲线加粗 / 等高线样式改造：另一轮。
- 派生数据（坡度/河流/植被）异步更新：本项目维持手动按钮触发口径。

## 3. 既有地基（全部复用，零数据层改动）

- 高度场：`TerrainCellData.height: i16`（米）；GPU 侧 R16Unorm 域编码（`field/pack.rs`，H_MIN=-4000/H_MAX=6000，步长 ≈0.15 m；仿射编码 ⇒ mip 盒滤在编码域与米域等价）。
- GPU 场管线 `field/`：自定义 WGSL 材质（`assets/shaders/terrain_field.wgsl`，776 行，`load_internal_asset!` 编译期内联）：Catmull-Rom 双三次高度采样 → 双三次解析梯度（`sample_height_bicubic_grad`，`grad_step` 槽位 GPU 侧已停用）→ 单光源 lambert → 海岸三段色带 / 墨线 / 海拔设色（`apply_hypsometric` 消费 `normalized_cuts`，**离散取色**：`elev_ramp_color(elev_band(h))`）；uniform `shape.z = z_exag`、`relief.xy = 明暗钳制`、`relief.z = lod_blend`。
- **⚠️ 现行远观行为与本目标相反**：`lod_t = 1 − smoothstep(1, 3, px_per_cell)`，远观时高度采样退化为盒平均的同时 **relief 被整体收敛到 1.0（浮雕完全关闭，T33 噪声抑制口径，wgsl:180）**——这正是「缩放到整个州看不到山脉结构」的直接原因之一。本设计将**替换**该行为（见 D3）。
- 分块 LOD：256 格 chunk + 3 级 mip + 视口拣选 + 脏块重烘（`terrain_chunks.rs`）。
- 笔刷实时链路（用户 §8 的流程**已实现**）：笔划包围盒脏标记（`edit/editing/stroke.rs:139`）→ `patch_terrain_image` 增量上传（整图 80ms → 局部 <1ms，`edit/editing/terrain.rs:14`）→ shader 每帧按纹理重算法线 → 脏 chunk 重烘 + mip。本轮零改动。
- 色带：`hypsometric.rs`（DOMAIN_CUTS 绝对阶梯 / RAMP_SRGB / 窄域兜底 / `normalized_cuts` 已备 GPU uniform 用途）+ 工具条槽 10.75 强度滑条（会话态）。
- CPU 块状烘焙路径：`cells.rs`（Topographic 块视图 + 笔刷 patch 烘焙），`relief_mul` 光源左上 45°。

## 4. 决策

### D1 双光源美术光照（替换钳制路线）

```
I = 0.4 + 0.48 · max(0, N·L₁) + 0.12 · max(0, N·L₂)
```

| 参数 | 初始值 | 说明 |
|---|---|---|
| 主光 L₁ | 方位 315°（西北），高度角 45° | 与现行光源方向约定一致（左上受光） |
| 辅光 L₂ | 方位 45°（东北），高度角 45° | 弱辅助光，治「山脉恰与光照平行时山脊山谷消失」 |
| 环境光 | 0.4 | 三系数和 = 1.0，I 天然界内 [0.4, 1.0]，**不再需要 RELIEF_LO/HI 钳制** |

这是美术化光照而非物理太阳模拟（用户明示）。`I` 作为明度乘子作用于最终色（只压明度、不动色相——Imhof 纪律）。

**ADR 关系**：新开 ADR（编号现场取）修正 ADR-0067 D2「专业制图档浮雕调轻 0.90–1.12」——该裁定用调轻解决「过重像 3D」，本轮改用「概括化 + 界内光照模型」解决，索引标注修正关系。

### D2 Horn 3×3 梯度 + 米制量纲

```
N = normalize(−s·hₓ, −s·h_y, 1)，s = Z_EXAG / meters_per_cell
```

- hₓ/h_y 用 Horn 算子（[1,2,1]ᵀ·[1,2,1]/8 加权隔格差分），步长 1 格；比中心差分抗噪（手绘数据有台阶噪声）。
- **梯度与高度插值分工**：高度**取值**仍走双三次插值（海岸线/取色依赖其平滑）；**梯度**三档尺度一律从 mip 采样面上取 8 邻域 Horn（GPU 侧现用的双三次解析梯度退役——多尺度下它只服务单一尺度，两套梯度并存会让档间混合出现语义断层）。CPU 侧中心差分同步退役。
- `meters_per_cell` 由地图栅格元数据提供（现有换算，禁止第二份硬编码）。
- **Z_EXAG 初始 2.0**（用户给定）；现行 `shape.z` 的 0.55 退役。
- y 方向约定：数据行号向下增大、坡面朝左上受光——与现行 `LIGHT_DIR` 注释同款，WGSL 与 CPU 同约定。

### D3 多尺度概括（mip 三级混合，替换 T33 远观关灯）

```
I_final = 0.45·I_macro + 0.40·I_medium + 0.15·I_detail
```

- 三尺度 = 同一张高度纹理的**三个 mip 级**（mip 即盒式低通；仿射编码下编码域滤波 ≡ 米域滤波）。每档尺度独立按 D1/D2 算 I，再加权混合。
- **detail 权重随 px_per_cell 增大衰减**（远观细纹理成噪点），衰减量回流 macro/medium——复用现有 `lod_blend` uniform 通道，公式由实现校准（近观 detail 全额、远观趋零）。
- **替换关系（builder check 钉死）**：现行 WGSL `relief = mix(钳制对, 1.0, lod_t)` 在远观把浮雕**整体关闭**（T33 噪声抑制口径）——本设计以「detail 档衰减、macro/medium 持续」**取代**「整灯关闭」；T33 的噪声顾虑由 detail 衰减承接，宏观山脉结构在远观反而首次可见。这是对 T33 意图的有意推翻，入 ADR。
- GPU 成本：三次 LOD 纹理采样 + 三组 8 邻域 Horn（24 tap）/像素，比现状（16 tap 双三次 + 解析梯度）多，但 chunk 按视口拣选 + mip 已就位；若实测超预算，medium 档可复用 detail 档 tap 跨步采样降回 16 tap（实现期裁量）。

### D4 CPU/GPU 同公式（带声明的例外）

- **GPU 场管线**（VectorFlat / 主题浏览态）：完整 D1+D2+D3。
- **CPU 块状烘焙路径**（`cells.rs`，Topographic 块视图 + 笔刷 patch）：D1+D2 **单尺度**（全分辨率 = 中观档）。三尺度卷积在 CPU patch 路径上成本不成比例，且块视图本就是示意块美学。
- 这是「同格任何渲染路径同乘子」旧纪律的**显式放宽**：两路径在单尺度段逐格一致，多尺度段差异记录为视图模式语义。ADR 里写明。

### D5 低地色带分辨（修订 ADR-0055「8 阶」语义，不碰阶梯）

问题精确定位：不是「低地不参与」（CellMap 的 `tint_rgba` 本就全强度混入），而是 **0–200 m 一刀同色**，段内高度差零分辨。

方案：

1. **CellMap 色带改连续插值**：8 个锚点色不变，阶内按高度在两锚点色间线性过渡（`band 0` 内部 0–200 m 即有渐变）。DOMAIN_CUTS 绝对阶梯与窄域兜底**原样不动**（ADR-0056 语义、阈值不随数据漂移的性质保留）。
2. **GPU 侧同步**（builder check 核实）：现 shader `elev_ramp_color(elev_band(h))` 是离散取色——改为「段内按高度 lerp 相邻锚点色」，uniform 布局（`elev_cuts` 2×vec4 + `elev_ramp` 8×vec4）不变，仅 shader 取色逻辑改动。顺带清理 `material.rs:590` 的陈旧注释（仍写「ADR-0055 百分位」，ADR-0056 已改绝对阶梯）。
3. **低端锚点色可顺带加深区分度**（`RAMP_SRGB[0..2]` 微调，保持明度单调上升的既有测试约束）——具体色值实现期校准。
4. **专业制图 `ElevationBand` 与 `BAND_WEIGHT` 原样不动**（wv-render 侧阶梯语义 + 2026-10-26 白洗修复不回退）；连续插值只落 CellMap 数据视图路径。

ADR-0055「8 阶（用户裁定）」在 CellMap 路径修正为「8 锚点连续渐变」，修正关系入索引。

### D6 参数落点

全部为 `wv-world-cell-map` crate 常量（`mod` 内 `pub const` 表，禁散落字面量）：光源方位/高度表、三光系数、Z_EXAG、meters_per_cell 换算入口、三尺度权重表、detail 衰减参数。依据 ADR-0054 D4「数据视图不随主题」；主题化（日系手绘包想换光）留未来 ADR 走桥接层。

### D7 留痕

- 新 ADR（处境/决策/理由/代价 + 被否决备选：调轻钳制路线、R32Float、CPU 三尺度）+ 索引登记修正 ADR-0067 D2 / ADR-0055 两条关系。
- GLOSSARY 登记：多尺度晕渲（multi-scale hillshading）、双光源（dual-light）、Horn 算子、垂直夸张（vertical exaggeration）、连续色带（continuous hypsometric ramp）。

## 5. 纪律声明

- **P1–P7**：零新增存储字段、零格式变更、零数据层改动；纯渲染常量 + WGSL + CPU 烘焙公式。
- **分层**：改动全部落 `wv-world-cell-map`（固定色 crate 级禁令不破——光照常量属数据视图语义，不引 theme）；wv-core / wv-terrain / wv-io 零改动；wv-render 若有 uniform 透传仅在桥接层跟随。
- **测试纪律**：手算期望值锚点；GPU 公式在 Rust 侧做参照实现逐格对比（WGSL 无法直接单测，用同公式移植 + 固定 fixture 锚点钉住）。

## 6. 验收

- fixture 宽缓高原（300 m / 20 格坡）在 GPU 场管线下受光/背光两面肉眼可辨（WV_SHOT 前后对比）。
- 远观（px_per_cell≈0.5）见山脉宏观结构、近观见细节层，随缩放平滑过渡（两档 WV_SHOT）。
- 0–200 m 低地内部渐变可读，公园绿等群系色不被洗白（白洗回归测试通过）。
- 笔刷拖画帧时间不回退；全门禁 + `cargo app-test` / `app-clippy` / `smoke-gui.sh` 绿。

## 7. 测试策略

- 光照纯函数：N·L=0 半兰伯特边界、I ∈ [0.4,1.0] 界内性、三系数和 = 1。
- Horn 梯度：3×3 fixture 手算法线；同物理坡不同 meters_per_cell 同法线（量纲测试）。
- 多尺度：三档权重和 = 1；detail 衰减随 px_per_cell 单调；混合公式手算锚点。
- CPU/GPU 同值：固定高度阵，CPU 实现与 Rust 参照实现（= WGSL 公式移植）逐格相等（1e-6）。
- 色带：连续插值在锚点处与旧阶梯色一致（锚点连续性）；明度单调测试沿用；白洗回归（公园绿 + 低段混入 → 色差有界）。

## 8. 留给实施计划的裁量

- macro/medium/detail 对应的具体 mip 级偏移（相对当前 LOD 基准 +0/+1/+2 附近校准）。
- detail 衰减曲线形状（线性/平滑阶梯）与 `lod_blend` 的合并方式。
- `relief.xy`（现明暗钳制槽位）改载三光系数的方式与 `relief.zw` 的用途分配。
- D3 的 tap 预算：全 24 tap 或 medium 复用 detail 跨步的 16 tap 降档。
- `RAMP_SRGB[0..2]` 具体色值。
- CPU 单尺度路径的 patch 成本复核（Horn 比中心差分多 4 次采样/格）。

## 9. Builder Check 记录（2026-10-28）

审查方式：并发限额降级为本会话以 builder 视角对照源码核查（材料侧 `field/material.rs` + `assets/shaders/terrain_field.wgsl` + `field/pack.rs` 逐条对照）。结论按三堆分类：

- **已在对话中回答、已并入正文**：① 远观 relief 整体关闭（T33）与 G2 相反——并入 D3 替换关系；② GPU 海拔设色为离散取色且接线已在（`apply_hypsometric` 消费 `normalized_cuts`）——并入 D5-2；③ pack 步长实为 ≈0.15 m——§3 已修正；④ GPU 梯度现用双三次解析、`grad_step` 已停用——并入 D2 分工。
- **builder 自行裁量、已在 §8 列出**：uniform 槽位重载、tap 预算、衰减曲线、mip 偏移、色值校准、patch 成本复核。
- **须用户回答的**：无。
