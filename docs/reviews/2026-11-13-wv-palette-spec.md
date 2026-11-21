> **规格草稿 · 用户 2026-11-13 拍板 A 范围 · 待复算**

# World View 自有色板 WV（群系真色主导 · 冷中性矢量层）

| 项 | 内容 |
|---|---|
| 日期 | 2026-11-13 |
| 版本 | v1.0 · 首稿 |
| 触发 | 用户「我觉得你可以重新设计过正儿配色」→ 选 **A：只重建矢量层色板**；配色值由 AI 定 |
| 档位 | 规格草稿 → 逐行挑拣实施 → 全量测试 → 留痕。**不提交** |
| 唯一视觉出处 | 新建 `crates/wv-default-theme/src/colors_wv.rs`（`WV_*`），供 `PaletteColors::DEFAULT` 全量重绑 |
| 不改 | 冻结群系表 `BIOME_RAW_COLORS` / `DISTRICT_RAW_COLORS`、`CellPalette`、`conv_sea_flat`、`TreatmentFlags::EMPTY`、`terrain.bin`、`wv-io`、`PassId::MapFrame`、ADR-0066 / `recipe.rs:663-675`、`export_png.rs`、**C 路线 (c) 三件套 `map_furniture.rs` 与规格 v1.0 / 56 项验收** |
| 沙箱限制 | GUI/GPU 被企业策略拒绝，**未看实际帧**；本稿只有代码级与数值级结论 |

---

## 0. 结论先行

**不回退到 HEAD**：HEAD 的 `GOOGLE_CORE_TERRAIN`（`#AADAFF` 淡水 / `#C9E7C7` 公园绿 / `#F7F5F0` 暖白陆）正是用户要去掉的「google 样式」；工作树里的 `ANTIQUE_TERRAIN`（灰蓝海 / 暖沙 / 褐墨）是用户已弃用的古舆图。**两头都不能取** → 新建第三套项目自有色板。

| 严重度 | 判定 | 状态 |
|---|---|---|
| 🔴 | 主画布 GPU Themed 走 `biome_table[21]` + `biome_tint=0.82` + 冷灰绿 8 阶，**群系真色是唯一彩色来源**，本轮不动 | ✅ 冻结 |
| 🔴 | 矢量层必须**零彩度**，只靠明度分级承载层级，否则与 21 色群系底抢戏（D2 复发） | ✅ 本稿核心 |
| 🔴 | 「深墨 + 白晕」是硬规格：一级墨压热带雨林仅 **1.68:1**，白晕 **6.23:1**；单墨通道不可判 | ✅ 数值已验 |
| 🟡 | 中性墨阶沿用 (c) 三件套五枚令牌（别名重导出，非第二份色表） | ✅ 刻意一致性约束 |
| 🟡 | GoogleMap 页 15 枚品牌 POI 色 / 道路三档 / `district` 12 槽暖褐 | 转待裁 D5–D6，本轮不动 |

---

## 1. 设计语言（三条可判边界，不写「克制」这类虚词）

1. **群系真色主导**：画面上唯一的彩色来自冻结群系表 21 色（含 `#9DC3D8` 咸水 / `#F2F5F7` 雪原 / `#4A7C59` 森林）。矢量层（注记 / 道路 / POI / 等值线 / 网格 / 氛围罩 / 区域轮廓）一律零彩度。
2. **单一出处、同帧同源**：矢量层五枚中性墨**别名重导出** `(c)` 三件套已拍板的 `GOOGLE_LABEL_FILL` 等常量，值逐位一致。同一帧出现两套灰 = D2「拼的」复发，故这是刻意约束、不是省事。新增单测逐位钉死，防漂移。
3. **唯一强调色 = 石板蓝 `#2A5D8F`**：值直接取主画布 `terrain_field.wgsl:148 WATER_SRGB`（ADR-0101），**不引入新色相**；只用于「事件族 POI / 海名 / 深水位」这类需跳出的极小面积（全幅 <0.1%），从根上消掉 11-12 日志记的「两套水色口径」坑。

**禁用色（全表可 grep）**：`#F2EAD8` 纸色、`#4A3F35` / `#6B5B49` / `#5A4E42` 褐墨、`#A67C52` 棕褐、`#EA4335` 红、`#1A73E8` 蓝、`#E8710A` 橙、`#9334E6` 紫、`#F9C25C` / `#FBD366` 橙黄干线、`#AADAFF` / `#C9E7C7` / `#F7F5F0` Google 地形色。

---

## 2. 完整色卡（`WV_*` 全量，36 槽 + 10 槽 core）

### 2.1 中性墨阶 5 档 —— **别名重导出，零新字面量**

| WV 令牌 | 上游常量（唯一出处） | hex | S | \|R−B\| | 用途 |
|---|---|---|---|---|---|
| `WV_INK_PRIMARY` | `GOOGLE_LABEL_FILL` | `#3C4043` | 0.104 | 7 | 一级墨：地名 / 街名 / POI 点 / 区域轮廓 |
| `WV_INK_SECONDARY` | `GOOGLE_LINE_ON_DARK` | `#5F6368` | 0.087 | 9 | 三级墨：势力名 / 中等线 / 势力 POI |
| `WV_INK_MUTED` | `GOOGLE_ISOLINE_INK` | `#9AA0A6` | 0.072 | 12 | 二级墨：等值线 / 冰缘 / 次级线 |
| `WV_LINE_FAINT` | `GOOGLE_CONTROL_BORDER` | `#DADCE0` | 0.027 | 6 | 极淡线：网格 / 路缘 / 建筑侧 / 雪阴影 / 图例描边 |
| `WV_PAPER` | `GOOGLE_CONTROL_BG` | `#FFFFFF` | 0.000 | 0 | 白晕（关键）/ 道路白填充 / POI 白环 / 雪高光 |

五枚全为 `R<G<B` 冷序、零 `R>G>B` 暖序、`S ≤ 0.12`、`|R−B| ≤ 12` —— 与 `(c)` 规格 §5.1 / B1 判据**同一张表**。

### 2.2 新增语义色（本轮真正的新设计）

| WV 令牌 | hex | S | \|R−B\| | 来源 / 理由 |
|---|---|---|---|---|
| `WV_SURFACE` | `#EEF1EF` | 0.012 | 1 | 冷近白底：`soften_toward` / `event_backdrop` / `ice_paper` / `building_top`。取 ② 阶段冷灰绿海拔带第 0 阶，与山势底同族；替代暖纸 `#F2EAD8` 与 Google 暖白 `#F7F5F0` |
| `WV_SURFACE_DEEP` | `#E8EBE8` | 0.013 | 0 | 淡化次级面：`core[5]` 丘陵底 |
| `WV_ACCENT` | `#2A5D8F` | 0.706 | 101 | 石板蓝，= `WATER_SRGB`。**唯一豁免 B1 的色**，因它不属矢量墨阶而属水体语义；仅 3 处极小面积 |
| `WV_WATER_SHORE` | `#9DC3D8` | 0.301 | 59 | = 冻结群系表 `[14]` 咸水。`water_shore` 槽，与图例 / 数据视图同值 |
| `WV_WATER_DEEP` | `#2A5D8F` | — | — | = `WV_ACCENT`。`deep_ocean_blue` 槽，与 GPU 收敛水色同值 |
| `WV_SNOW` | `#F2F5F7` | 0.021 | 5 | = 冻结群系表 `[9]` 雪原。`core[7]` |
| `WV_ICE_SHADOW` | `#CFD6D3` | 0.032 | 4 | 冰背光冷灰 |
| `WV_ROAD` | `#FFFFFF` | — | — | = `WV_PAPER`（别名） |

### 2.3 `core[10]`（TerrainKind 0..10，CPU 留守档 / CellMap 折叠语义）

主画布 GPU Themed **不读 `core`**（走 `biome_table`），此表只服务 CPU 留守档与等值线 / 网格取色，故按「低信息量折叠」设计，不与群系真色竞争：

| 槽 | 语义 | 值 | 槽 | 语义 | 值 |
|---|---|---|---|---|---|
| 0 | 深水 | `WV_ACCENT` `#2A5D8F` | 5 | 丘陵 | `WV_SURFACE_DEEP` `#E8EBE8` |
| 1 | 浅水 | `WV_WATER_SHORE` `#9DC3D8` | 6 | 山地 | `#DDE3DD` |
| 2 | 沙 / 基础陆 | `WV_SURFACE` `#EEF1EF` | 7 | 雪原 | `WV_SNOW` `#F2F5F7` |
| 3 | 草原 | `#F1F3F0` | 8 | 沼泽 | `#E2EAE3` |
| 4 | 林地 | `#E6EBE6` | 9 | 道路 | `WV_ROAD` `#FFFFFF` |

`style_core_and_kind_core_are_distinct`：`core[3]/[4]` 与 `CORE_TERRAIN` 高饱和十色逐位不同 ✅。`district_palette_has_one_slot_per_district`：12 槽互异且均 ≠ `core[8] = #E2EAE3` ✅（暖褐系全场无此值）。

### 2.4 36 槽接线表（`PaletteColors::DEFAULT`）

| 字段 | WV 令牌 | 字段 | WV 令牌 |
|---|---|---|---|
| `core` | `WV_CORE_TERRAIN` (§2.3) | `kind_core` | `colors::CORE_TERRAIN`（**不动**，逐格技术色） |
| `district` | `colors::DISTRICT_COLORS`（**不动**，见 D5） | `soften_keep` | `WV_SOFTEN_KEEP = 0.96` |
| `soften_toward` | `WV_SURFACE` | `poi_dot` | `WV_INK_PRIMARY` |
| `poi_ring` | `WV_PAPER` | `road_stroke` | `WV_ROAD` |
| `road_casing` | `WV_LINE_FAINT` | `street_major` / `street_minor` | `WV_ROAD` |
| `label_fill` / `label_place` | `WV_INK_PRIMARY` | `label_region` | `WV_INK_PRIMARY`（靠字号 20px 分级，不另立色） |
| `label_water` | `WV_ACCENT` | `dot_place` / `dot_region` / `text_place` / `text_region` | `WV_INK_PRIMARY` |
| `dot_faction` / `text_faction` | `WV_INK_SECONDARY` | `dot_event` / `text_event` | `WV_ACCENT` |
| `snow_highlight` | `WV_PAPER` | `snow_shadow` | `WV_LINE_FAINT` |
| `ice_paper` | `WV_SURFACE` | `ice_ink` | `WV_INK_MUTED` |
| `ice_shadow` | `WV_ICE_SHADOW` | **`halo_fill`** | **`WV_PAPER`**（双通道关键） |
| `isoline_ink` | `WV_INK_MUTED` | `grid_line` | `WV_LINE_FAINT` |
| `building_top` | `WV_SURFACE` | `building_side` | `WV_LINE_FAINT` |
| `event_backdrop` | `WV_SURFACE` | `line_on_light` | `WV_LINE_FAINT` |
| `line_on_mid` | `WV_INK_MUTED` | `line_on_dark` | `WV_INK_SECONDARY` |
| `deep_ocean_blue` | `WV_WATER_DEEP` | `water_shore` | `WV_WATER_SHORE` |
| `elevation_ramp` | `Some(GOOGLE_ELEVATION_RAMP)`（**不动**，② 阶段冷灰绿 8 阶） | `elevation_strength` | `GOOGLE_ELEVATION_STRENGTH`（**不动**） |

**POI 4 族策略变更**：`dot_*` / `text_*` 原为 Google 品牌四色（红 / 蓝 / 橙 / 紫），现改为「3 档中性明度 + 事件族唯一石板蓝」，分类信息由工程既有 21 枚 POI 图形（ADR-0105）+ 文字标签承载，**颜色不再承载分类**（沿用 `(c)` L04 / B01 纪律）。

---

## 3. 数值验算（独立算，不引用实现常量）

### 3.1 21 色群系底 × 墨阶：WCAG 对比度

| 墨 | 最差 | 最好 |
|---|---|---|
| 一级墨 `#3C4043` | **1.68:1**（热带雨林 `#3A6B45`） | 9.56:1（雪原 `#F2F5F7`） |
| 三级墨 `#5F6368` | 1.03:1（热带雨林） | 5.53:1（雪原） |
| 白晕 `#FFFFFF` | 1.09:1（雪原） | **6.23:1**（热带雨林） |

→ **深墨在暗群系上不可读、白晕在亮群系上不可读，二者恰好互补**。故规格硬约束：所有画布文字走「`WV_INK_PRIMARY` 字身 + `WV_PAPER` 8 向晕圈」（工程 `sync_halo` / ADR-0016 M2 已落地，本轮只把 `halo_fill` 从纸色 `#F2EAD8` 改回纯白）。这不是审美选择，是可判的读数前提。

### 3.2 B1 中性判据（新增色逐项）

`WV_SURFACE 0.012/1`、`WV_SURFACE_DEEP 0.013/0`、`WV_ICE_SHADOW 0.032/4`、`WV_SNOW 0.021/5`、`#F1F3F0 0.013/0`、`#E6EBE6 0.032/0`、`#DDE3DD 0.015/0`、`#E2EAE3 0.044/1` —— 全部 `S ≤ 0.12`、`|R−B| ≤ 12`、零暖序 ✅。
豁免项仅 `WV_ACCENT` / `WV_WATER_SHORE` / `WV_WATER_DEEP` / `WV_SNOW` 中取自**冻结群系表或 GPU 真值**的水体色（语义是水，不是墨）。

---

## 4. 接线表（逐文件，逐行挑拣；**禁用 `git checkout` 整文件回退**）

| # | 文件 | 动作 |
|---|---|---|
| 1 | `wv-default-theme/src/colors_wv.rs` | **新建**：§2 全量 `WV_*` + `WV_CORE_TERRAIN` + 5 条一致性 / 判据单测 |
| 2 | `wv-default-theme/src/lib.rs` | 加 `pub mod colors_wv;`；职责段「色板取百度 / Google 普通地图那套」改为 WV 口径 |
| 3 | `wv-default-theme/src/palette.rs` | `DEFAULT` 36 槽按 §2.4 重绑；`antique_palette_user_pinned_hexes_are_stable` → `wv_palette_user_pinned_hexes_are_stable`（期望值手抄 §2 色卡）；`style_core_and_kind_core_are_distinct` / `district_palette_has_one_slot_per_district` / `out_of_range_kind_falls_back_without_panic` 三条**逻辑不动、值自动跟随**；`MapPalette::Daylight` 注释改 WV 口径 |
| 4 | `wv-default-theme/src/colors.rs` | **删除** `ANTIQUE_TERRAIN` 整段（全仓 6 处引用中 5 处是注释、1 处是 `palette.rs`，无测试引用）；其余古舆图历史令牌与其 14 条既有测试**不动**（红线） |
| 5 | `wv-default-theme/src/decor.rs` | 仅改 `:125` 注释去掉 `ANTIQUE_TERRAIN` 引用 + 清 2 处空白噪音；**七类符号全关 / `wave_band` 归零 / 5 个 `*_style` 字段全部保留**（② 合规） |
| 6 | `wv-default-theme/src/sea.rs` | 仅清 1 处空白噪音；`PROFESSIONAL.shore_hatch = None` **不动** |
| 7 | `wv-default-theme/src/theme_pack.rs` | `builtin::DEFAULT` 注释回「专业制图 · WV 自有色板」并去掉「UI 重命名留待 V3」；**`biome_tint: 0.82` 与群系真色注释保留**（② 合规）；`display_name: "专业制图"` 本就名实相符，不动 |
| 8 | `wv-render/src/cellmap_bridge/theme_conv.rs` | **保留「移除 GoogleMap 硬覆盖」的结构**（它正是「不再用 Google 地形色」），只把注释从「V1 同纸古舆图」改为「WV 单一色板，Map / Browse 同源」；`conv_palette` 36 行 1:1 映射不动 |
| 9 | `wv-render/src/cellmap_bridge/mod.rs` | 测试 `google_mode_terrain_core_is_the_google_palette` → 改名 `wv_palette_is_shared_across_map_and_browse`；**两条断言全部保留**（`core` 相等 + `≠ GOOGLE_CORE_TERRAIN` 正是本轮目标），只改注释口径 |
| 10 | `wv-render/src/drill.rs` ×5 / `streets.rs` ×1 | 六处 `match view.mode { GoogleMap\|Topographic => label_handle(), _ => calligraphy_handle() }` → 统一 `fonts.label_handle()`（ADR-0065 Noto 无衬线）；衬线书法体是古舆图语汇，必须随色板一起退场 |
| 11 | `wv-render/src/region_outline.rs` | `else` 分支褐墨 `#4A3F35` → `WV_INK_PRIMARY @0.85`；`in_cellmap` 白分支不动 |
| 12 | `wv-app/src/theme/mod.rs` | 若需 `colors_wv` re-export 则加一行（实施时按 `crate::theme` 结构定） |

---

## 5. 挑拣矩阵：不是「一刀切回退」

| ① 阶段（方向 A 古舆图） | 处置 |
|---|---|
| `ANTIQUE_TERRAIN` / 30+ 令牌切褐墨 / 书法衬线体 / 褐墨轮廓 / 纸色晕 / 锚点测试反转 | ❌ 剔除（本轮替换为 WV） |
| **移除 `GoogleMap` 硬覆盖（两 tab 同纸）** | ✅ **保留结构，只改注释** —— 回退它等于把 `#AADAFF` Google 地形色请回来，与本轮目标相反 |
| `examples/*/ui-layout.yaml` 布局快照 | ✅ 不动（运行期窗口坐标 / brush 参数，与配色无关） |
| 任务 #3「V3 收边」（褐墨图廓 / 指南针贴图 / 比例尺 / 浅色纸壳） | ✅ 保持作废登记，不实施（与 `(c)` 规格逐条冲突） |

| ② 阶段（用户翻转后） | 处置 |
|---|---|
| `decor.rs` 七类符号全关 / `terrain_decor.rs` 早退 / `sea.rs` / `colors_google.rs` 冷灰绿 8 阶 / `biome_tint=0.82` / WGSL 精确 `biome_table` / `(c)` 三件套 | ✅ **七项全部保留**，任何回退不得连带 |
| `resources.rs` / `shell.rs` X01 清屏色精确回退 + `SelectedDistrict` 修复 | ✅ 保留 |

---

## 6. 范围边界（A 不含，如实登记不隐瞒）

| ID | 残留 | 为什么不动 |
|---|---|---|
| D5 | `colors::DISTRICT_COLORS` 12 槽仍是暖褐（`palette.district`） | 分区色是 12 槽语义编码，灰阶化需单独做可辨性设计 + 实机验证；且只在城市/街区视图消费，不在主画布上屏。塞进本轮会重演「没看帧就改色」 |
| D6 | `vector_layers.rs` 15 枚 `GOOGLE_POI_*` 品牌色 + `GOOGLE_ROAD_GRADES` 橙黄干线 | 已证实在 `GoogleMap` 分支门控（`:1541` `if mode == GoogleMap`）；Browse 默认画布走 `pal.poi_ring/poi_dot`，本轮已转 WV 中性。Map 页按 `(c)` 规格 §6.1 裁定「维持原样、零装饰」，动它等于改规格 |
| D7 | `GOOGLE_ELEVATION_RAMP` / `GOOGLE_*` 令牌**命名**遗留 | 值已是 ② 阶段重定的冷灰绿 8 阶与中性灰，改名波及 6 条测试 + ADR-0067；`WV_*` 用别名指向它们，值合规，仅名字带历史 |
| D8 | `docs/` 既有文件 | 只 append 追记，禁止整行覆盖（AGENTS T5） |
| D9 | 8 项实机验收（Y1–Y8） | 沙箱 GUI/GPU 被拒，仍须用户桌面按 `2026-11-13-map-furniture-acceptance.md §8.1` 走 |

---

## 7. 测试计划（可判，不写「看起来对」）

| # | 断言 | 期望 |
|---|---|---|
| T1 | `WV_*` 五枚中性墨逐位 == `colors_google::GOOGLE_*` 对应常量 | 单一出处不漂移 |
| T2 | 全部 WV 墨色 `S ≤ 0.12 ∧ |R−B| ≤ 12` ∧ 零 `R>G>B` 暖序 | B1 判据 |
| T3 | `halo_fill == WV_PAPER`（纯白） | 双通道硬前提 |
| T4 | `PaletteColors::DEFAULT.core != ANTIQUE_TERRAIN` 且 `!= GOOGLE_CORE_TERRAIN` | 两头都不取 |
| T5 | `WV_WATER_DEEP == WATER_SRGB #2A5D8F`；`WV_WATER_SHORE == BIOME_RAW_COLORS[14]` | 消两套水色口径 |
| T6 | `deep_ocean_blue == WV_ACCENT` ∧ `water_shore == WV_WATER_SHORE` | 接线到位 |
| T7 | 禁用色 grep 全表零命中（§1 列表） | 去品牌 / 去古舆图 |
| T8 | `Map` 与 `Browse` 的 `CellPalette.core` 逐位相等，且 `≠ GOOGLE_CORE_TERRAIN` | 保留 ① 正确结构 |
| T9 | `drill.rs` / `streets.rs` 全仓 `calligraphy_handle()` 零命中（除 `fonts.rs` 定义） | 衬线退场 |
| T10 | 复跑 `wv-default-theme` 54 → 新增后全绿；`wv-app --features app map_furniture` 22；`wv-render --features app map_frame_is_retired_in_every_mode` 1；`wv-render --features app` 全量 | 零回归 |

---

## 8. 红线与状态

冻结群系表 / `CellPalette` 冻结值 / `conv_sea_flat` / `TreatmentFlags::EMPTY` / `terrain.bin` / `wv-io` / `PassId::MapFrame` / ADR-0066 / `recipe.rs:663-675` / `export_png.rs` / 既有 GLOSSARY·ADR·README / `(c)` 三件套与规格 v1.0 / 56 项验收 —— **全未动**。零新依赖、零新资产、无 `#[ignore]`、**未提交**。

本轮写盘：`docs/reviews/2026-11-13-wv-palette-spec.md`（本文件）、`docs/reviews/2026-11-13-visual-direction-conflict-reconciliation.md` 末尾 append 追记、`colors_wv.rs`（新）、§4 表列 12 处代码改动、`.workbuddy/memory/2026-11-13.md` append-only 追记。

---

# 追记 · 2026-11-13 · A 范围实施完成（复算已收口）

> 顶部「规格草稿 · 待复算」标记按 AGENTS.md T5 **不整行覆盖**；本节为权威的后续状态。**代码已改、未提交、未看实际帧**。

## A. 实施期三处发现与裁决（规格未预先覆盖，如实登记）

### F1 · 依赖边界：T5 无法在 `wv-default-theme` 单 crate 内闭合 🔴

`wv-default-theme/Cargo.toml` 只依赖 `bevy` + `wv-terrain`（ADR-0054 单向依赖红线），**不能**引用 `wv_world_cell_map::terrain_view::biome_raw::BIOME_RAW_COLORS`。故：

- `WV_WATER_SHORE` / `WV_SNOW` 只能按冻结表值**镜像抄录**，在 `colors_wv.rs` 加注释写明来源与限制；
- 真正的**跨 crate 逐位一致**断言改落 `wv-render/src/cellmap_bridge/mod.rs::wv_water_and_snow_mirror_the_frozen_biome_table`（`app` feature 下 `wv-render` 同时依赖两个 crate）：断言 `rgb(WV_WATER_SHORE) == BIOME_RAW_COLORS[14]`、`rgb(WV_SNOW) == BIOME_RAW_COLORS[9]`；
- `WATER_SRGB #2A5D8F` 是 WGSL 字符串常量，**Rust 侧无编译期引用**，`WV_ACCENT` 只能手抄 + 全仓 grep 纪律保证，无自动断言。这一条是规格 T5 的**不可自动验证部分**，不隐瞒。

### F2 · §2.4 接线表内在矛盾：POI 四族只剩三色 🔴

§2.4 文字写「**3 档中性明度 + 事件族唯一石板蓝**」（= 4 值），但接线表把 `dot_place` 与 `dot_region` 都写成 `WV_INK_PRIMARY`，`WV_INK_MUTED` 未分给任何 POI 族 → 实际只有 3 值。

实施后 `cargo test -p wv-app --features app --lib` 立刻暴露真实回归：

```
ui::hover_tooltip::tests::kind_color_per_kind_distinct
assertion `left != right` failed: kind 0 与 kind 1 同色
  left: #3C_40_43_FF   right: #3C_40_43_FF
```

`hover_tooltip.rs:131-134` 读 `pal.dot_*`，其测试的产品契约是「四色是浮窗里区分实体种类的唯一线索（同色 = 用户分不清在看什么）」，与 `colors_google.rs` 的四色注释同源。

**裁决：按文字描述用足第三档，不改测试、不改方向、不引入新色值。**

| 族 | 实施前（规格表） | 实施后（裁决） | hex |
|---|---|---|---|
| Place | `WV_INK_PRIMARY` | `WV_INK_PRIMARY` | `#3C4043` |
| Region | `WV_INK_PRIMARY` ❌ | **`WV_INK_MUTED`** | `#9AA0A6` |
| Faction | `WV_INK_SECONDARY` | `WV_INK_SECONDARY` | `#5F6368` |
| Event | `WV_ACCENT` | `WV_ACCENT` | `#2A5D8F` |

`dot_*` / `text_*` 同步。四值互异、除事件族外零彩度、明度分级承载层级，完全落在用户拍板方向内。新增 T11 断言 `palette.rs::wv_palette_user_pinned_hexes_are_stable` 逐对钉死，防再并档。**未采用「放宽断言适配新设计」**——那等于承认产品退化。

### F3 · §3.2 B1 数值复算（规格「待复算」标记收口）🟡

独立按 `S = (max−min)/max`（8-bit sRGB）复算，**判据 `S ≤ 0.12 ∧ |R−B| ≤ 12 ∧ 零 R>G>B 暖序` 全部通过**，但规格 §3.2 手记的 4 个 S 值不准：

| 色 | §3.2 手记 | 实测 | 结论 |
|---|---|---|---|
| `#F1F3F0` core[3] | 0.013 / 0 | **0.0123 / 1** | |R−B| 记错，判据过 |
| `#E6EBE6` core[4] | 0.032 / 0 | **0.0213 / 0** | S 偏高，判据过 |
| `#DDE3DD` core[6] | 0.015 / 0 | **0.0264 / 0** | S 偏低，判据过 |
| `#E2EAE3` core[8] | 0.044 / 1 | **0.0342 / 1** | S 偏高，判据过 |
| `WV_WATER_SHORE` | S 0.301 | **0.2731** | 豁免项，不影响 |

五枚中性墨 `0.1045 / 0.0865 / 0.0723 / 0.0268 / 0.0000` 与 §2.1 一致 ✅。**原表不覆盖**，此表为更正口径。`colors_wv.rs::wv_neutral_inks_satisfy_b1` 用同一公式在 15 个中性色上逐项运行，是自动化的权威判据。

## B. 实际接线（§4 表逐行核对）

| # | 文件 | 结果 |
|---|---|---|
| 1 | `colors_wv.rs` | ✅ 新建：5 枚别名 + 9 枚语义色 + `WV_SOFTEN_KEEP` + `WV_CORE_TERRAIN[10]` + **6 条单测**（T1/T2/T3/T4/T5 镜像自证/T7） |
| 2 | `lib.rs` | ✅ `pub mod colors_wv;` + 定位段 / 职责段改 WV 口径 |
| 3 | `palette.rs` | ✅ 36 槽全量重绑；锚点测试改名 `wv_palette_user_pinned_hexes_are_stable` 并手抄 §2 色卡；另加 T3/T6/T11 断言；`MapPalette::Daylight` 注释改 WV |
| 4 | `colors.rs` | ✅ `ANTIQUE_TERRAIN` 整段删除 → **该文件相对 HEAD 已零差异**；其余历史令牌与 14 条测试未动 |
| 5 | `decor.rs` | ✅ 仅改 `:125` 注释 + 清 2 处空白噪音；七类符号全关 / `wave_band` 归零 / 5 个 `*_style` 全保留 |
| 6 | `sea.rs` | ✅ 仅清 2 处空白噪音 → **该文件相对 HEAD 已零差异**；`shore_hatch = None` 未动 |
| 7 | `theme_pack.rs` | ✅ 默认包注释改「专业制图 · WV 自有色板」；`display_name` / `biome_tint: 0.82` / `TreatmentFlags::EMPTY` 全保留 |
| 8 | `theme_conv.rs` | ✅ **保留「移除 GoogleMap 硬覆盖」结构**，只改两段注释口径；36 行 1:1 映射未动 |
| 9 | `cellmap_bridge/mod.rs` | ✅ 测试改名 `wv_palette_is_shared_across_map_and_browse`（两条断言全保留）+ 新增 T5 跨表测试 |
| 10 | `drill.rs` ×5 / `streets.rs` ×1 | ✅ 六处统一 `fonts.label_handle()`；全仓 `calligraphy_handle()` 消费点归零（仅 `fonts.rs` 定义与字体 README 残留） |
| 11 | `region_outline.rs` | ✅ 新增 `wv_outline_ink()` 读 `WV_INK_PRIMARY @0.85`，渲染层零裸 RGB；`in_cellmap` 白分支未动 |
| 12 | `wv-app/src/theme/mod.rs` | ✅ 加 `colors_wv` re-export（UI 侧路径可达，无第二份色值） |

**逐行挑拣，全程未用 `git checkout` 整文件回退**；`examples/*/ui-layout.yaml` 布局快照未动。

## C. 亲手复跑的验证（非采信回报）

| 命令 | 结果 |
|---|---|
| `cargo check -p wv-app --features app` | ✅ Finished（仅沙箱 `os error 5` 增量锁告警，非 Rust 代码告警） |
| `cargo test -p wv-default-theme` | **60 passed / 0 failed**（原 54 + 新增 6） |
| `cargo test -p wv-render --features app` | **452 lib + 15 + 11 + 17 = 495 passed / 0 failed**（+1 ignored doc-test） |
| `cargo test -p wv-app --features app --lib` | **495 passed / 0 failed**（+1 ignored） |
| `cargo test -p wv-app --features app --lib map_furniture` | **22 passed / 0 failed** |
| `cargo check -p wv-world-cell-map` / `cargo test -p wv-world-cell-map` | ✅ / **7 passed** |
| `recipe.rs:668 map_frame_is_retired_in_every_mode` | 仍在 452 条 lib 测试内，全绿（ADR-0066 防复活锚点） |
| Grep | `palette.rs` 内 `super::colors::*` / `#F2EAD8` / `#4A3F35` / `#A67C52` / `#EA4335` / `#AADAFF` **零命中**；全仓 `ANTIQUE_TERRAIN` 仅剩 `colors_wv.rs` 一句「不是古舆图」的测试注释 |

首轮 `wv-app` 全量的 **1 条 FAILED**（F2）已按裁决修复并复跑全绿，未以放宽断言掩盖。

## D. 范围外残留（含实施期新增 D10）

D5 `DISTRICT_COLORS` 12 槽暖褐、D6 GoogleMap 页品牌 POI / 橙黄道路、D7 `GOOGLE_*` 命名遗留、D9 8 项实机验收 —— 均按 §6 原样保留。

新增 **D10**：`drill.rs` / `streets.rs` 内若干 **HEAD 既有**注释仍以「古舆图 / 暮色 / 纸色晕」描述取色分支（如 `drill.rs:2040` 写 `label_water` 是 `#3F5B6A`，现值已是 `#2A5D8F`）。这些是历史注释口径残留、**非本轮引入**，本轮严格遵守 A 范围未改；登记在此，待下一轮统一收口，不影响任何取色结果。

## E. 状态

冻结群系表 / `CellPalette` 冻结值 / `conv_sea_flat` / `TreatmentFlags::EMPTY` / `terrain.bin` / `wv-io` / `PassId::MapFrame` / ADR-0066 / `recipe.rs:663-675` / `export_png.rs` / 既有 GLOSSARY·ADR·README / `(c)` 三件套与规格 v1.0 / 56 项验收 —— **全未动**。零新依赖、零新资产、无新增 `#[ignore]`、**未提交**。

`git diff --stat`：19 文件 / +410 / −157（相对上一轮快照 +32 / −11 为本轮 A 范围净改动）。未跟踪新增 `crates/wv-default-theme/src/colors_wv.rs`。

沙箱 GUI/GPU 被企业策略拒绝 → **仍未看实际帧**，以上全部为代码级 / 数值级结论；Y1–Y8 须用户桌面按 `2026-11-13-map-furniture-acceptance.md §8.1` 走。
