> **评审草稿 · 未采纳 · 待用户拍板**

# 近岸水纹（shore hatch）视觉规格：C 路线「轻量装饰」第一项

| 项 | 内容 |
|---|---|
| 评审人 | 甄塑之 · 美术总监 |
| 日期 | 2026-11-12 |
| 版本 | **v2.2** · ① 统一唯一墨色定稿 `#24496A`（按真实 GPU 基色 `WATER_SRGB #2A5D8F` L≈84 重算，撤销会提亮的 `#4A6B8A`）② 并入 3 处接线硬错（`pad0.x` 禁用 / std140 尾部追加 / 行坐标改未扭曲 `g.y`）、2 处事实矛盾（Faction 非 GPU 档、Browse 画 / Map 不画）与 `FieldKey`/mode 触发缺口 |
| 档位 | **评审草稿 · 候选素材，非已定实施方案**：用户尚未拍板本项落地。只出设计与规格，不改任何 `.rs` / WGSL、不改既有 GLOSSARY / ADR / README / docs 文件、不提交、不装依赖 |
| 触发 | 用户选 C 路线：保留「干净专业制图 + 群系真色主导」基底，加回轻量装饰；本轮只做近岸水纹。配色由 AI 定，不依赖外部参考附件 |
| 口径 | **GPU Themed 水面真实基色 = `WATER_SRGB #2A5D8F`（L≈84）**；冻结表契约水色 `#9DC3D8 / #7FB4CE / #6BA3C7` **不是本分支的取色源**。**唯一墨色定稿 = `#24496A`（L≈66）** |
| 交付物 | ① GPU 程序化档的定稿参数（含量纲）② 墨色不打架的判据 ③ 按 GPU 模式门控重写的「不该画的地方」 |
| 处置 | 未采纳前：`PROFESSIONAL.shore_hatch` 保持 `None`、`SeaRecipe::ink` 保持现值、`FieldPalette` / `FieldParams` / WGSL 一律不动 |

---

## 0. 前提更正：默认主画布是 GPU 场，不是 CPU 烘焙

**v1.0 已整段作废的结论**：`shore_hatch` 与 `SeaRecipe::ink` 在默认主画布上是**空转**——

- `shore_hatch` **只**被 CPU 烘焙路径消费：`wv-world-cell-map/src/terrain_view/cells.rs::water_detail` 第 ④ 段（`:889-910`）。
- 默认 Browse（VectorFlat）与 Map（GoogleMap）标签页 `field_render = true` 时，`wv-render/src/cellmap_bridge/chunks.rs:50-59` 在 `should_render_field(&view)` 为真时**整层早退**、despawn 全部 chunk、清脏矩形，CPU 烘焙纹理根本不上屏。判据同源：`field.rs:347-349 should_render_field = view.field_render && supports_field(view.mode)`；`supports_field`（`:305-313`）= { **VectorFlat, GoogleMap, RealTerrain, Topographic** }，这四档走 GPU 场 quad。
- 因此 CPU 留守档只有 { Faction, Event, Coastline }（`supports_field = false`）——而按用户约束，政治素底页也不该画排线。**给 CPU 档定值 = 给一份永远不会上屏的规格**，即「文件做了没接进游戏」。
- v1.0 误引的另一套斜纹机制（与 `SeaRecipe` 无关）已彻底删除，本稿不再提及、不作为负向或对照项。

**GPU 场现成的量（全部可直接接线，无需新造数据源）**：

| 量 | 出处 | 量纲 |
|---|---|---|
| `sd`（正=陆 / 负=水） | `terrain_field.wgsl:333` | **格** |
| `wd = max(-sd, 0.0)`（入水深度） | `terrain_field.wgsl:466` | **格** |
| `p.shore_edges = (1, 3, 9)` 滩/浅/深边界 | `wgsl:41`、`material.rs:79, 664` | **格** |
| `px_per_cell = max(p.shape.x, 1e-5)` | `wgsl:235` | **屏幕 px / 格** |
| `half_w = 0.5 * COAST_LINE_PX / px_per_cell` + `fwidth` | `wgsl:583`、`material.rs:84 COAST_LINE_PX = 1.2` | **屏幕 px** |
| `ink_gate = smoothstep(1.0, 3.0, px_per_cell)` | `wgsl:553` | 屏幕 px / 格 |
| `h_cell_norm` vs `SEA_LEVEL_NORM`（湖 / 海判别） | `wgsl:511-515`；`SEA_LEVEL_NORM = 0.4`（`:1139`）、`is_land = attr.r > 0.5`（`:318`） | 归一化高度 |
| **`g = uv0 * grid`（未扭曲格坐标，行坐标用它）** | `wgsl:222-233` | **格** |
| `gw = g + warp_offset(g)` → `pos`（已扭曲） | `wgsl:223, 233, 700-715` | 格；**排线不得用作行坐标** |
| **`WATER_SRGB = #2A5D8F`(L≈84) 水面真实基色** | `wgsl:148, 443, 475` / `shading.rs:171` | sRGB → 线性；**排线唯一混合对象** |
| `p.ink` / `p.river_ink` / `p.paper` | `material.rs:353-357`、WGSL `:45-46` | 线性色 |

**量纲结论（答复主程序问 ①）**：`HatchSpec::row_spacing_px`（`sea.rs:54-58`）是**烘焙纹理像素**，GPU 场是屏幕空间逐像素程序化求值，**不存在「烘焙纹理像素」这个量纲**，不能照搬。本稿明确：

- **行距 / 近岸带宽 → 用「格」（世界空间，锚定地图，平移不游动；行坐标取未扭曲的 `g.y`）**；
- **线宽 → 用「屏幕 px」（经 `px_per_cell` 折算 + `fwidth` AA，缩放恒定）**；
- 不采用屏幕 px 作行距（行会随平移游动），也不采用 `fract(wd/…)` 的等距轮廓线（那是「离岸波纹墨线」同族，会读成同心圆，回到插画）。

**目标选择（答复主程序问 ②）**：**只给甲（GPU 程序化档）**，不给乙（CPU 烘焙存档档）。理由见 §2.3。

---

## 一、风格定位

沿用既定基调：陆面由冻结表 `BIOME_RAW_COLORS[21]` 主导（`biome_tint = 0.82`），8 阶中性冷灰绿（`#EEF1EF → #6E776F`）只作极淡起伏底，`hillshade` 常开，无任何贴地符号 / 浪花 / 雪线 / 点点。

近岸水纹是**唯一被允许的线状装饰**，定位「水色加深一档的极淡岸影」，不是「画出来的浪」。三条硬约束：**冷色**、**无暖墨**、**不新增形状语言**（只允许世界水平等距直线；斜纹、圆角、尖刺一律不许混入）。GPU 档**不加** CPU 档那层 `0.55+0.45*n` 噪声调制——那是手绘感的来源，专业档要去掉。

旧暖褐墨（`SeaRecipe::ink #5A4E42`、GPU `p.ink ≈ #292421`）属古舆图 / 水墨语汇，**排线不得沿用**；但 `p.ink` 被岸线 / 道路共用，本轮保持现值不动（见 §2.2），只在新增的 `hatch_ink` 槽上定冷蓝灰。

---

## 二、GPU 程序化档：定稿参数

### 2.1 逐项定值（**全部是 GPU 侧新增参数，不是 `HatchSpec` 字段**）

| 参数（新增 / 未接线） | **量纲** | 定稿值 | 一句理由 |
|---|---|---|---|
| `hatch_ink` 墨色 | sRGB → 线性（uniform 色） | **`#24496A`**（36,73,106），**L≈66** | 由真实基水 `WATER_SRGB #2A5D8F`(L≈84) 同色相压暗一档派生（≈0.8×），满足 `ink_L ≤ water_L − 15`（66 ≤ 69）；α=0.20 后线色 `#295988`(L≈80.4) 是**加深 3.6 L**，不是提亮；冷蓝灰、无暖相，不引入第四种色相 |
| `hatch_band_cells` 近岸带宽 | **格**（`wd`） | **`3.0`**，外缘 `smoothstep(2.4, 3.0, wd)` 淡出 | 对齐 `p.shore_edges.y = 3`（`SHALLOW_EDGE`，`material.rs:79`）浅带外缘，不另起一套边界；软外缘避免 CPU 档 `d < dist` 的硬截断 |
| `hatch_row_spacing_cells` 行距 / 周期 | **格**（世界 Y，未扭曲 `g.y`） | **`0.75`** | 3 格带内落 4 道行（验收 2–4 道）；世界锚定、平移不游动；**必须取 `g.y`，不能取 `pos.y`**——`pos = gw = g + warp_offset(g)` 被 `p.pad0.x` 的 fbm 扰动（`wgsl:222-233, 700-715`），用它会画成波浪线、违反 §一；**不是纹理像素、不是屏幕 px** |
| `hatch_line_width_px` 线宽 | **屏幕 px** | **`1.0`** | 与 `COAST_LINE_PX = 1.2` 同族；`half_w = 0.5 * 1.0 / px_per_cell` + `fwidth(dd)` AA（`wgsl:583` 口径），近观不粗、远观不糊 |
| `hatch_alpha` 墨量 | 0..1 混合比（**非 alpha 通道**） | **`0.20`**（硬上限 `0.25`） | 峰值 20% 在真实基水 L≈84 上仅降 **3.6 L（≈4.3%）**，正好「水色加深一档」；上限 0.25 防止对比继续放大读成线框。用独立 `HATCH_ALPHA`，**不复用** `COAST_INK_ALPHA = 0.55`（`wgsl:90`） |
| 断续 / 密度规则 | 格 | **无人工断续**：线段由 `wd < 3.0` 的岸线形状自然裁成短段；密度只由 `hatch_row_spacing_cells` 定；**取消噪声调制**，alpha 恒定 | 短排线的「短」来自窄带裁剪，不靠抖动；不加 dash / break 参数，也不与离岸波纹墨线同族 |
| 远观门控 | 屏幕 px / 格 | 复用 **`ink_gate = smoothstep(1.0, 3.0, px_per_cell)`**（`wgsl:553`） | `< 1 px/格` 全关；原生满足 50% 缩略图判据并防远观闪点 |
| 落墨位置门 | — | `!is_land` ∧ `wd < 3.0` ∧ `h_cell_norm < SEA_LEVEL_NORM` | 只在水侧近岸、且**只海不湖**；湖 / 河判别是 WGSL 现成的（`:318, :513, :1139`），正好补上 CPU 档「窄水体被整片填满」的缺口 |

### 2.2 墨色按哪个源（答复主程序问 ② 的色值部分）

- **口径（本稿唯一）**：**GPU Themed 水面真实基色 = `WATER_SRGB = #2A5D8F`（42,93,143，L≈84）**，见 `wgsl:148, 443, 475` 与 `shading.rs:171`。冻结表契约水色 `#9DC3D8 / #7FB4CE / #6BA3C7`（`biome_raw.rs:92-94`）**不是本分支的取色源**——它们是群系表图例 / CellFlat 数据视图的契约色，GPU Themed 水面不读它们。
- **为什么可见档上基色就是 `#2A5D8F`**：近观 `band_gate`（`wgsl:485-496`）把三段海岸色带收敛回平色 `water_c`，白滩与浅带项同乘 `band_gate` 一并收掉；远观 `ink_gate`（`:553`）把排线关掉。故**排线真正可见的缩放档上，水面就是 `#2A5D8F`**，不存在「该按浅带还是深海算」的二义。
- **条件已确定成立**：`WATER_SRGB` 是 `wgsl:148` 的现行固定常量（`:475` 消费），本轮红线**不改任何冻结色值**，故「若换掉该常量」的前提不成立；实现值唯一为 `#24496A`，不存在第二候选。

> ⚠️ **待裁（主程序退出前复核提出，制作人独立核实属实；本稿未修、未验收）**：
> 上一条「可见档水面恒为 `#2A5D8F`」**不是普适成立**。`band_gate`（`wgsl:485-489`，
> 常量 `:95-96 COAST_BAND_CAP_MIN_PX=6.0 / MAX=24.0`）在 ppc<6 全开三段色带、
> **ppc≥24 才收敛回 `water_c`**；而 `ink_gate`（`:553 smoothstep(1,3,ppc)`）
> **ppc≥3 排线就全额可见**。故 **ppc ∈ [3,24) 排线全开、水面仍是
> `water_c→shallow_c→deep_c` 渐变**（`:478`），不是单一 `#2A5D8F`。
> §3.2 本稿用的 8 px/格 锚点正落在该区间 ⇒ §2.2 / §3.1 / §3.2 / §5.1 里
> 「L 84→80.4 / ΔL 3.6」与「`0 < water_L − line_L ≤ 0.10·water_L`」
> **只在 ppc≥24 成立**，不得当作全缩放区间的验收判据。
> 若日后按 (A) 采纳，实施前须先把亮度判据改成 **ppc 区间函数**，或把排线段
> 移到 `:478` 之前直接混进 `water_c`。
>
> **美术域口径裁定（甄塑之，2026-11-12 收口前，仅记口径、草稿盘上内容不动）**：
> **取 (a)** —— 排线段移到 `:478` 之前、直接混进 `water_c`，让三段色带再叠在
> 排线之上，使「排线锚在平色基水」语义成立；`#24496A`(L≈66) 与
> `ink_L ≤ water_L − 15` **单值判据不变**。不取 (b)（把墨色判据拆成色域、
> 丢掉唯一值）、不取 (c)（几乎压没可见档）。
> **用户未拍板前不实施**；本轮不落地、不改判 (B)。
> 详见 `.workbuddy/memory/2026-11-12.md`「最后一处规格缺陷」。
- **不按** `SeaRecipe::ink`（`sea.rs:97`，现值 `[0x5A,0x4E,0x42]`）——那是 CPU 烘焙路径的色源，GPU 场完全不读。
- **GPU 侧现成墨色是 `FieldParams::ink`（`material.rs:353`，WGSL `p.ink`）**，源 `FieldPalette::ink`（`material.rs:216`，`default()` 现值 `srgb(0.16, 0.14, 0.13) ≈ #292421` 暖近黑，由 `default_params` :687 装填）。
- **但 `p.ink` 被三处共用**：岸线墨线（`wgsl:592`）、道路主干 / 次级墨线（`wgsl:650 / :653`）。全局改成冷蓝灰会把**道路墨线染蓝**（不可接受），超出本轮范围。
- **故定稿（唯一值，无备选、无条件分支）**：排线用**独立新增槽 `hatch_ink`**，值 **`#24496A`（36,73,106，L≈66）**；该槽**按 std140 尾部追加**（见 §2.4），**不插在 `p.ink` 旁**；`p.ink` 现值**不动**，其暖褐问题另案处理。
- **定值规则（可复算，非拍脑袋）**：`ink_L ≤ water_L − 15` → `66 ≤ 84 − 15 = 69` ✓；混合后线色 `L = 84 + 0.20·(66 − 84) = 80.4`，即 `#295988`，**ΔL = −3.6（变暗 4.3%）** ✓ 满足 §5.2「必须变暗且 ≤10%」判据。v2.0 的 `#4A6B8A`(L≈101) 代入同式得 `L≈87`（**+3.4 L，提亮**），已作废；全稿不再保留任何第二取值。

### 2.3 为什么只给甲，不给乙（CPU 烘焙存档档）

1. `chunks.rs:50-59` 整层早退 → CPU 烘焙纹理在四档 GPU 模式下永不上屏；给乙的参数是空转。
2. CPU 留守档 { Faction, Event, Coastline } 按用户约束「政治素底页不画」，也不该有排线。
3. `HatchSpec` 的 `row_spacing_px` 是纹理像素量纲，与 GPU 的格 / 屏幕 px 无法互换，硬给两套值只会制造两套互相打架的口径。
4. **若日后确实要启用乙**：换算口径是「3.0 格 ↔ 16 纹理像素 @ 该烘焙的像素/格分辨率」，`alpha = 0.20`、墨色 `#24496A` 不变——但这**不是本稿交付参数**，`SeaRecipe` / `HatchSpec` 保持现值不动。

### 2.4 缺失 / 未接线清单（不许当成已完成）

- `hatch_ink` / `hatch_band_cells` / `hatch_row_spacing_cells` / `hatch_line_width_px` / `hatch_alpha` / `hatch_enable` — **全部新增 / 未接线**（`FieldParams` 现无这些槽）。
- **新槽必须尾部追加（布局硬约束）**：`FieldParams` 是 `ShaderType`/std140（`material.rs:311-401`），`:375-378` 与 `:388-391` 明确「**只有尾部追加**才不扰动既有字段偏移，往中间插字段编译期不报错、运行期上传 uniform 才炸」。新 `Vec4` 追加在 `biome_none_fallback`（`material.rs:400`）之后，WGSL `FieldParams` 镜像同步追加到末尾（`wgsl:54-62` 已写明同一纪律）；**不得**插在 `p.ink`（`:353`）附近。
- **可用预留槽**：`pad0.y/.w`、`pad1.z/.w`、`relief.w`（**`pad0.x` 已被 T19 域扭曲占用**：`material.rs:650`、`terrain_field.wgsl:707`、`field.rs:704`，不可作预留槽，主程序复核结论）。
- **`pad0.x` 不可占用（硬错）**：T19 域扭曲幅度 `warp_amp(px_per_cell)`（`material.rs:141-143`，钳在 0.05~0.45 格）写在 `pad0.x`（`:650-653`），WGSL `:707` `let amp = p.pad0.x` 由 `warp_offset()`（`:700-715`）消费，且 `field.rs:704` 每次缩放都重写 `material.params.pad0[0]`。占它会打散格网域扭曲，且值被每帧覆盖。**不得占用**的其余槽：`pad0.z`(biome_tint, `field.rs:593`)、`pad1.x`(WV_STRIP 掩码)、`pad1.y`(海岸编码)、`shape.w`(height_span)、`shore_edges.w`(wash_amt)。
- **mode 门控必须处理「切模式不重打、不重写 uniform」（缺口）**：`FieldKey`（`field.rs:22-34`）只有 map / terrain_hash / branch / palette_id，`structure_changed`（`:431-434`）不含 mode，uniform 重写条件（`:693`）只判 ppc / structure / hyps / strip；VectorFlat ↔ GoogleMap 同为 `Themed`、同 `palette_id` ⇒ **切模式既不重打也不重写 uniform**，`hatch_enable` 只在建材质写一次会留旧值（Map 漏画 / Browse 多画）。实现须给 `FieldKey` 加 mode（改 `:22-34` / `:431-434` / `:732` + 真值表测试）或加 `last_hatch_enable` 缓存并把 mode 变化并入 `:693`；**且重打路径（`:586-617`）与 uniform-only 路径（`:693-730`）两处都要写**。成本据此 **6 文件 + 1 shader / 1–1.5 人日**，`FieldKey` 改动触及 **ADR-0088 模式路由测试面**，须跑 `cargo test -p wv-render --features app` + `bash scripts/smoke-gui.sh`。详见 §四与 §5.1 `hatch_enable` 行。
- WGSL 排线段 — **未实现**。落点：`rgb = mix(water_col, land_col, land_t)`（`:518`）之后、岸线墨线（`:592`）之前；用 `mix(rgb, p.hatch_ink, row_mask * hatch_alpha * ink_gate * band_fade)`。岸线墨线 `:592` 在其后会覆盖贴岸那一列像素，不影响「只水侧近岸带」语义，但验收行数不计该列。
- **已核对可直接使用的常量**：`SHALLOW_EDGE = 3.0`（`material.rs:79`）、`COAST_LINE_PX = 1.2`（`:84`）、`ink_gate`（`wgsl:553`）、`SEA_LEVEL_NORM = 0.4`（`:1139`）、`is_land = attr.r > 0.5`（`:318`）、湖分支 `h_cell_norm >= SEA_LEVEL_NORM`（`:513`）。
- 模式门控 `hatch_enable` — **未接线**（见 §四实现约束）。

---

## 三、为什么压在「群系真色 + 冷灰绿起伏」上不打架（保留项）

### 3.1 墨色与真实基水 / 契约水色 / 冷灰绿的数值对照（sRGB，L = 0.299R+0.587G+0.114B）

| 色 | RGB | L | 与定稿墨色的关系 |
|---|---|---|---|
| **GPU Themed 水面真实基色 `WATER_SRGB`** | `#2A5D8F` 42,93,143 | **84** | 排线**唯一**混合对象 |
| 咸水（冻结表 14，契约色 / **非本分支取色源**） | `#9DC3D8` 157,195,216 | 186 | 仅图例 / 数据视图契约；墨色比它深 120 L |
| 淡水（15，同上） | `#7FB4CE` 127,180,206 | 167 | 同上 |
| 河流（16，同上） | `#6BA3C7` 107,163,199 | 150 | 同上 |
| 冷灰绿低阶 | `#EEF1EF` 238,241,239 | 240 | 中性近白，无蓝相 |
| 冷灰绿高阶 | `#6E776F` 110,119,111 | 115 | 中性灰绿，B−R = **1** |
| 旧暖墨（CPU 侧） | `#5A4E42` 90,78,66 | 80 | **R>G>B 暖褐**，排线不得沿用 |
| GPU `p.ink` 现值（岸线 / 道路共用，本轮不动） | `#292421` 41,36,33 | 37 | 暖近黑，另案处理 |
| v2.0 误用墨（**作废**） | `#4A6B8A` 74,107,138 | 101 | 比基水**亮 17 L** → α=0.20 后提亮，与定位相反 |
| **v2.1 定稿墨 `hatch_ink`** | **`#24496A` 36,73,106** | **66** | 比基水暗 18 L，B−R = **70**，冷蓝灰 |
| α=0.20 混合后线色 | `#295988` ≈41,89,136 | **80.4** | 比基水暗 **3.6 L（4.3%）**，「加深一档」 |

**三条不打架的理由（全部按 L=84 重算）：**

1. **同族加深，方向正确**。定稿墨与真实基水同为 B>G>R 的冷蓝序列，只是明度低一档、彩度略收（基水 B−R=101，墨 B−R=70）。按 `colors_google.rs:238-240` 已确立的制图口径「水岸线 = 水色加深一档，灰线会读成行政边界」，排线读成**水的阴影**而不是一条外来轮廓线。α=0.20 后线色 `#295988`（L≈80.4）相对 `#2A5D8F`（L≈84）**降 3.6 L**——是变暗不是提亮。冻结表三档契约浅蓝（L 150/167/186）虽非本分支取色源，但墨色 L=66 一律低于它们，作为图例对照仍是同族更深一档，不引入第四种色相。
2. **与冷灰绿陆地双重分开**。冷灰绿 8 阶 L=240→115，定稿墨 L=66 **落在整条色带之下**（比最深的板岩灰还低一档），明度已不重叠；色相上墨 B−R=70、冷灰绿高阶 B−R=1，更不会把「背光坡」误读成「水线」。且 `biome_tint = 0.82` 使冷灰绿只贡献约 18% 底色，陆地主色由群系真色承担，排线（只在水侧）与它**根本不在同一层**。
3. **暖墨不新增、既有墨线不误伤**。排线层只用 `#24496A` 冷蓝灰，不沿用 `#5A4E42` / `#292421` 的暖褐，因此新增装饰不会把画面拉回古舆图插画感；同时**不改动**被岸线 / 道路共用的 `p.ink`（现值不动），既有墨线逐位不变——暖墨的整体清理是另案，不在本轮范围。

### 3.2 色彩预算与三测

- **色彩预算**：主色（群系真色）60% / 辅色（冷灰绿 8 阶 + 水深渐变）30% / 点缀（排线）10%。GPU 档占空比 = 1 屏幕 px ÷ `(0.75 格 × px_per_cell)` ≈ **17% @ 8 px/格**，乘 α=0.20 → 近岸带内等效墨量 ≈ 3.3%，全幅 <1%，远低于点缀 15% 的「吵」阈值；主色占比不动。**峰值明度对比 = 0.20 × (84 − 66) = 3.6 L**（255 级的 1.4%），比 v2.0 按 L=186 误算的 17 L 弱一个量级。
- **剪影测试**：排线不改变任何地物 / 岸线剪影，只在近岸带叠一层微暗纹理。通过。
- **灰度测试**：去色后水面明度由 84 降到 80.4（**ΔL = −3.6，−4.3%**），方向为变暗；主次仍由群系真色与冷灰绿起伏的明度差撑住。**判据**：`0 < water_L − line_L ≤ 0.10·water_L`——既有上界（不超过 10%）又锁死方向（不得提亮）；v2.0 的 `#4A6B8A` 在此判据下为 +3.4 L，直接判失败。本稿通过。
- **形状语言检查**：只允许「世界水平等距直线」一种线语言，与专业制图的平行 / 直角体系一致；无斜纹、圆角、尖刺，无噪声抖动（行坐标取未扭曲 `g.y`，见 §2.1）。
- **50% 缩略图判据**：1 屏幕 px 线 + 0.75 格周期 + `ink_gate` 远观关闭，缩到 50% 后应并成一层**均匀淡岸影**、数不出单根线；本稿对比仅 3.6 L，若还能数出线，说明 `hatch_alpha` 或 `hatch_band_cells` 超了，判失败。

---

## 四、不该画的地方（按 GPU 模式门控重写）

**核心实现约束（必须写进规格）**：`field_branch_of`（`field.rs:276-292`）只按 `mode == Topographic` 与 `terrain_visible` 选分支——**`ViewMode::VectorFlat` 与 `ViewMode::GoogleMap` 都落到同一个 `ViewBranch::Themed`**；`field.rs:586-616` 装填 `FieldParams` 时只带 `branch`（编码在 `p.shape.y`：0=BiomeRaw / 1=Themed / >1.5=Flat），**不区分 `view.mode`**，`theme_conv.rs:232` 的 `shore_hatch = None` 也只管 CPU 侧 GoogleMap、与 GPU 场无关。因此 **「Map 不画」必须新增 mode 标志**：由 `field.rs` 按 `view.mode` 把 `hatch_enable` 写入 `FieldParams` 预留槽，WGSL 排线段开头 `if (hatch_enable < 0.5) { /* 跳过 */ }`。**不允许**靠 `p.shape.y` 或分支枚举推断 mode。

**切模式不重打 / 不重写 uniform 的缺口**（`FieldKey` 不含 mode、`structure_changed` 与 `:693` 重写条件均不含 mode）及其「加 mode 或加缓存、重打与 uniform-only 两处都写」的实现要求，见 §2.4 与 §5.1——这是本负向清单能成立的前提。

| # | 分支 / 位置 | 期望 | 负向断言建议 |
|---|---|---|---|
| 1 | **`ViewBranch::Themed` ∧ `ViewMode::VectorFlat` ∧ `field_render = true`**（**Browse 标签页**；`wv-app/src/state.rs:528-530 browse → VectorFlat / map → GoogleMap`） | **唯一放行：Browse 标签页画、Map 标签页不画** | `hatch_enable == 1`；水侧近岸带有 2–4 道行；其余参数按 §2.1 |
| 2 | **`ViewMode::GoogleMap`（此即 Map 标签页 / 普通地图，同样落 `Themed`；`state.rs:543-545 map → GoogleMap`）** | **不画（Map 标签页不画）** | `hatch_enable == 0`；与 #1 同分支同 `p.shape.y` 下，水面像素零排线（这条就是 mode 门控的回归锚点） |
| 3 | **`ViewMode::Topographic`（CellMap 方格数据视图 → `CellFlat`；`cell_raw` 探针 → `BiomeRaw`）** | **不画** | `hatch_enable == 0`；`biome_raw::pick` 输出逐位不变（数据视图要诚实，`pick` 不接收 palette，`biome_raw.rs` 文件头） |
| 4 | **`ViewBranch::Flat` 政治素底页**（地形层隐藏，`shape.y > 1.5`；Faction 属 CPU 留守档，见 §四 #6） | **不画** | `hatch_enable == 0`；素底 quad = 1、水面完全平色（`wgsl:493-495`），无行状纹理 |
| 5 | **`ViewMode::RealTerrain`（同样落 `Themed`）** | **不画**（白名单外，待用户拍板） | `hatch_enable == 0`；若用户日后要放行，须单独扩白名单而非放宽分支判断 |
| 6 | **CPU 留守档 { Faction, Event, Coastline }**（`supports_field = false` → `should_render_field = false` → `teardown_field` / chunk 路径，`field_branch_of` 根本不被调用） | **不画** | CPU 侧 `SeaRecipe.shore_hatch` 恒 `None`（`sea.rs:199`）；`chunks.rs:50-59` 未早退时也不得出现排线 |
| 7 | **陆面像素 / 内陆湖（biome 15）/ 河流（biome 16）/ 深水 `wd ≥ 3.0` 格** | **不画** | `!is_land` 门；`h_cell_norm >= SEA_LEVEL_NORM` 的湖 / 河零输出；带外零输出 |
| 8 | **远观 `px_per_cell < 1`** | **不画** | 复用 `ink_gate`（`wgsl:553`），排线强度乘它；远观零闪点 |

**白名单唯一**：`ViewBranch::Themed` ∧ `ViewMode::VectorFlat` —— **Browse 标签页画、Map 标签页不画**（两个标签页行为相反，不可笼统写作「Browse/Map 主画布」）。其余一律 `hatch_enable = 0`，CPU 侧一律 `shore_hatch = None`。

---

## 五、命名与接线点 / 验收

### 5.1 三列对照表（缺任一项即标「缺失 / 未接线」）

| 规格项 | 资产文件 | 接线点 |
|---|---|---|
| `hatch_ink = #24496A`(L≈66) | `crates/wv-world-cell-map/src/terrain_view/field/material.rs`（`FieldPalette` :206 / `FieldParams` :311 / `default_params` :687）**派生自** `shading.rs:171` / `wgsl:148 WATER_SRGB #2A5D8F`(L≈84) | **新增** `hatch_ink` 槽 → WGSL 排线段 `mix(rgb, p.hatch_ink, …)`；**不按** `SeaRecipe::ink`，**不复用** `p.ink`（岸线 :592 / 道路 :650,:653 共用）；定值判据 `ink_L ≤ water_L − 15`；`FieldParams` 按 **std140 只能尾部追加**（`material.rs:375-378, 388-391`），**不得插在 `p.ink` 旁**；新 `Vec4` 追加在 `biome_none_fallback`（`material.rs:400` / `wgsl:62`）之后，WGSL `FieldParams` 镜像同步末尾追加（`wgsl:54-62`） |
| 真实基色口径 | `wgsl:148, 443, 475, 485-496, 553` | 近观 `band_gate` 收敛回 `water_c`、远观 `ink_gate` 关排线 → 可见档水面恒为 `#2A5D8F`；冻结表三浅蓝非本分支取色源；本轮不改冻结色值，故实现值唯一 |
| `hatch_band_cells = 3.0` | 同上（预留槽） | `wgsl:466 wd = max(-sd,0)`、`p.shore_edges.y = 3`（`:41` / `material.rs:79, 664`）；外缘 `smoothstep(2.4,3.0,wd)` |
| `hatch_row_spacing_cells = 0.75` | 同上（预留槽） | WGSL 世界格坐标 **`g.y / 0.75`** 取行；**不得用 `pos.y`**——`wgsl:222-233` 与 `:700-715` 的 fbm 扰动会把它弯成波浪线，违反 §一『只允许世界水平等距直线』（`g` 口径见 `:229`）；**不接** `HatchSpec::row_spacing_px` |
| `hatch_line_width_px = 1.0` | 同上（`COAST_LINE_PX` 同族，`material.rs:84`） | `wgsl:583 half_w = 0.5 * COAST_LINE_PX / px_per_cell` + `fwidth` AA |
| `hatch_alpha = 0.20` | 同上（新增 `HATCH_ALPHA`） | 混合后 `L = 84 + 0.2·(66 − 84) = 80.4` → 变暗 3.6 L；**不复用** `COAST_INK_ALPHA = 0.55`（`wgsl:90`） |
| 远观门控 | `wgsl:553` | `ink_gate` 乘进排线强度 |
| 海 / 湖门 | `wgsl:318, 511-515, 1139` | `h_cell_norm < SEA_LEVEL_NORM`、`!is_land` |
| `hatch_enable`（mode 门控） | `crates/wv-render/src/cellmap_bridge/field.rs:586-616` | 按 `view.mode` 写 `FieldParams` 预留槽（**不得用 `pad0.x`**，见 §2.4）；WGSL 段首早退；**新增 / 未接线**；**切模式不重打 / 不重写 uniform 的缺口与「两处都写」要求见 §2.4 mode 门控条**（含 `FieldKey` 加 mode、`:732`、测试命令与成本） |
| **mode 变更触发（新增约束）** | `field.rs:22-34 FieldKey`、`:431-434 structure_changed`、`:693` uniform 重写条件 | VectorFlat ↔ GoogleMap 同为 `Themed`、同 `palette_id`，切模式既不重打也不重写 uniform ⇒ **给 `FieldKey` 加 mode，或加 `last_hatch_enable` 缓存并把 mode 变化并入 `:693`**；重打路径（`:586-617`）与 uniform-only 路径（`:693-730`）**两处都写**；`FieldKey` 改动触及 ADR-0088 模式路由真值表测试 |
| WGSL 排线段 | `crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl`（落点 `:518` 之后 / `:592` 之前） | **新增 / 未实现**；岸线墨线 `:592` 在其后覆盖贴岸那一列像素，验收「带内 2–4 道行」不计该列 |
| CPU 侧保持关 | `crates/wv-default-theme/src/sea.rs:199`、`theme_conv.rs:232` | `shore_hatch: None` 不动；不把 `conv_sea_flat` 当 GPU 护栏 |
| `HatchSpec` 三字段 | `crates/wv-default-theme/src/sea.rs:48-61` | **不在本稿参数集内 / 不接线**（GPU 无纹理像素量纲） |

**成本估计（供排期，非实施承诺）**：6 文件 + 1 shader / 1–1.5 人日；其中 `FieldKey` 改动触及 ADR-0088 模式路由测试面（真值表测试须同步）。本表所有接线项均为「缺失 / 未接线」，仅描述**若用户拍板后**的落点。

### 5.2 验收点（≤3 条，到什么程度算「方向对了」）

1. **量纲 / 灰度**：行距按格（取 `g.y`）、带宽按格、线宽按屏幕 px，`px_per_cell` 折算正确；**线色必须比水面暗**：`0 < water_L − line_L ≤ 0.10·water_L`，本稿 84 → 80.4（ΔL = 3.6，4.3%）；全幅等效墨量 <1%，`ink_gate` 远观关闭；50% 缩略图并成淡岸影、数不出单根线。
2. **不打架**：排线层冷蓝灰（`hatch_ink = #24496A`，L≈66 < 基水 L≈84）、不带暖相、零斜纹 / 圆角 / 尖刺、零噪声调制；`p.ink` / `p.river_ink` 与既有岸线 / 道路墨线逐位未动；剪影与形状语言两项检查通过。
3. **门控全绿**：`hatch_enable` mode 标志生效，`Themed(VectorFlat)` 唯一放行（Browse 画 / Map 不画），§四 #2–#8 全部零输出；**切 Browse ↔ Map 后 `hatch_enable` 无旧值残留**（§2.4 / §5.1 的 mode 触发约束已实现）；带内可见行数 2–4 道（**不计被 `wgsl:592` 岸线墨线覆盖的贴岸那一列**）；`ViewBranch::Themed` 下 VectorFlat 与 GoogleMap 的画面差异可复现（同一份 `FieldParams` 其余槽逐位相同）。

---

> 本稿为**待拍板草稿 / 候选素材，不是已定实施方案**：用户尚未拍板本项落地。采纳前：`PROFESSIONAL` 保持 `shore_hatch: None` 与 `ink: [0x5A,0x4E,0x42]`，`FieldPalette::ink` / `FieldParams` / WGSL 一律不改，所有既有 ADR / 测试 / 断言状态不变。
