> **评审草稿 · 未采纳 · 待用户拍板**

# 地图视觉方向裂缝对账：V1/V2 古舆图未提交改动 × C 路线专业色终局裁定

| 项 | 内容 |
|---|---|
| 评审人 | 制作人（架构级反向审视，非美术规格） |
| 日期 | 2026-11-13 |
| 版本 | v1.0 · 首稿 |
| 档位 | **评审草稿 · 未采纳 · 待用户拍板**。只做「裂缝 / 反设计 / 待裁」对账，**不改任何 `.rs` / WGSL / 冻结表 / `CellPalette` / 既有 GLOSSARY·ADR·README / 既有 docs 文件**；不提交、不装依赖、不加资产、不用 `#[ignore]` |
| 触发 | 用户 `go`（C 路线 (c) 完整落地后的继续推进）。按纪律开工前先读项目现状，发现工作树里**两个相反方向的未提交改动混在一起** |
| 核验方式 | `git status` / `git diff` 逐文件实读 + 消费点 grep + 单测复跑；**不采信任何回报** |
| 沙箱限制 | GUI/GPU 被企业策略拒绝，**未看实际帧**；以下全部为代码级静态事实 |

---

## 0. 结论先行

**发现 1 条会造成白干的方向性裂缝（高）**：工作树里仍压着 V1/V2「方向 A · 古舆图」的一整批**未提交**改动，与用户后续翻转的终局裁定「古舆图 / Google 样式全部去除，全部改为专业色 + 群系真色主导」**正面冲突**。

**且这批改动不是空转**（与 11-12 判定的 (a) 近岸水纹 `shore_hatch` 空转相反，务必区分）：`PaletteColors::DEFAULT` 经 `conv_palette` 被矢量层 / 等值线 / 网格 / 氛围罩 / 区域轮廓 / 势力覆盖真实消费。默认 Browse（`VectorFlat`）画布上的注记色、晕边色、字体，目前是**古舆图褐墨 `#4A3F35` + 纸色晕圈 `#F2EAD8` + 霞鹜文楷书法衬线体**；而同帧 (c) 三件套是**冷中性灰五角色 + Regular 无衬线**。即 `2026-11-11-worldmap-visual-direction.md:99` 诊断的 D2「两套互斥令牌拼在同一帧」依然存在，只是方向反了。

**这不是 (c) 实现的问题**：22 条单测全绿、56 项静态验收口径成立、X01 两处清屏 diff 相对 HEAD 只剩注释增量。问题在 (c) 之前的**基底没清干净**——X01「纸色旧债回退」只覆盖了两个清屏文件，未覆盖 `palette.rs` 这个真正的纸色源。

| 严重度 | 项 | 状态 |
|---|---|---|
| 🔴 高 | 默认色板 / 注记 / 字体仍是古舆图，与终局裁定反向 | 转待裁 |
| 🔴 高 | 待办 #3「V3 收边」逐条与 (c) 规格 / X01 冲突，执行即推翻 22 单测与 56 项验收 | 转待裁 |
| 🟡 中 | `theme_pack.rs` 注释自称「默认风格包 = 古舆图 · 有墨（V2）」，`display_name` 仍是「专业制图」，名实不符 | 转待裁 |
| 🟡 中 | X01 验收口径只 grep 两个清屏文件 diff，未覆盖 palette 层（口径覆盖不足，不是当时做错） | 转待裁 |
| ✅ | (c) 三件套 / 群系真色 `biome_tint=0.82` / 去点点 / 去雪线 / 去白浪花 / 冷灰绿海拔带 / WGSL 精确群系表 | 与终局裁定一致，保留 |

---

## 1. 时间线：三轮裁定翻转（裂缝成因）

| 阶段 | 裁定 | 落盘产物 |
|---|---|---|
| ① 11-11 方向 A | 归位古舆图：暖纸底 `#F2EAD8`、褐墨线、衬线地名、图廓/罗盘/比例尺收边；V1 同纸 → V2 有墨 → V3 收边 | 工作树里 `palette.rs` / `colors.rs` / `theme_conv.rs` / `drill.rs` / `streets.rs` / `region_outline.rs` / `cellmap_bridge/mod.rs` 一批改动；`docs/reviews/2026-11-11-worldmap-visual-direction.md`；任务 #3 pending |
| ② 用户翻转 | **「古舆图平 google 样式全部去除，全部改为专业色」**；选「重新设计一套干净专业配色」+「群系真色主导」；后续「贴图/雪线/白浪花/点点全部不要」 | `colors_google.rs` 冷灰绿 8 阶、`theme_pack.rs biome_tint 0.82`、`terrain_field.wgsl` 精确 `biome_table`、`decor.rs` 七类符号全关 + `wave_band` 归零、`terrain_decor.rs` 早退、`sea.rs` |
| ③ C 路线 | 看过精美古舆图后选 C：保留「干净专业制图 + 群系真色主导」基底，加回轻量装饰；配色由 AI 定 | (a) 近岸水纹判空转零改动；(c) `map_furniture.rs` 41.7KB / 22 单测；X01 纸色清屏回退；规格 v1.0 + 验收清单 v1.2 |

**关键**：② 只回退了 ① 的**窗口清屏色**一处（X01），以及 GPU Themed 主画布的取色链；① 留在矢量层 / 色板 / 字体 / 轮廓 / 测试断言里的部分**整批未动**。三阶段改动混在同一工作树、全部未提交，导致「哪版算对」没有唯一判据——正是 D1 的老病灶。

---

## 2. 裂缝清单（逐条可验证，按严重度排序）

> 「是否上屏」列已逐条核消费点；**空转与否必须与 (a) 近岸水纹区分**：`shore_hatch` 是 CPU 烘焙路径、`chunks.rs:50` 早退；本表条目全部经 `conv_palette` / `view.active_theme.palette` 直达上屏层。

| # | 文件:行（当前工作树） | 现值（古舆图） | 终局裁定应值 | 是否上屏 · 消费点 |
|---|---|---|---|---|
| F1 | `wv-default-theme/src/palette.rs:228` `DEFAULT.core` | `colors::ANTIQUE_TERRAIN`（灰蓝海 / 灰橄榄陆 / 暖沙，雪原按裸地褐） | `colors_google::GOOGLE_CORE_TERRAIN`（专业制图近白陆 / 淡水 / 公园绿） | ✅ `theme_conv.rs:43 conv_palette` → `CellPalette.core` → `atmosphere.rs:105` / `isoline.rs:94,254,273` / `grid_lines.rs:60` |
| F2 | `palette.rs:231-268` 30+ 令牌 | 全切 `colors::*` 古舆图褐墨族：`label_fill #4A3F35`、`soften_toward #F2EAD8`、`street_major #A67C52`、`road_casing #F2EAD8`、`halo_fill #F2EAD8` | 对应 `GOOGLE_*` 冷中性族：`#3C4043` / `#FFFFFF` / `#DADCE0` | ✅ `drill.rs:1414,1420` / `streets.rs:283,289` / `vector_layers.rs:1002,1052-1053` / `faction_overlays.rs:289` / `atmosphere.rs:113` |
| F3 | `drill.rs:1399-1404`（×5 处）`streets.rs:274-279` | `_ => fonts.calligraphy_handle()` —— **霞鹜文楷书法衬线体**，且 `VectorFlat`（Browse 默认）落在此分支 | `fonts.label_handle()`（Noto Sans SC 无衬线，ADR-0065 用户裁定） | ✅ 六处标签直出；`theme/fonts.rs:36-40` 明写「文楷不再是默认，仅主题显式选用」；`fonts/README.md:14,19` 同 |
| F4 | `drill.rs:1411-1422` `streets.rs:280-291` | `VectorFlat` 取 `palette.label_place` / `label_fill`（褐墨）+ `halo_fill`（纸色晕圈） | `GOOGLE_LABEL_FILL` + 白晕 | ✅ 仅 `GoogleMap` 分支走冷灰，其余全走古舆图 |
| F5 | `wv-render/src/region_outline.rs:406-411` | `≈ #4A3F35` 褐墨描边（注释自述「V2·有墨」） | 原基线 `srgba(0.21,0.35,0.48,0.85)` 深蓝 | ✅ `sync_region_outlines` 材质色，Browse/Map 均走 else 分支 |
| F6 | `wv-render/src/cellmap_bridge/theme_conv.rs:22-43` + `mod.rs:658-690` 测试 | 移除 `GoogleMap` 的 `GOOGLE_CORE_TERRAIN` 硬覆盖，改为「V1 同纸」；测试**反转为断言** `GoogleMap != GOOGLE_CORE_TERRAIN` | 恢复硬覆盖与正向断言（专业制图） | ✅ `conv_pack_with:189` 每帧构造 `CellPack.palette`；测试把反向口径钉死 |
| F7 | `wv-default-theme/src/palette.rs:283-320` 测试 | `antique_palette_user_pinned_hexes_are_stable` **断言** `soften_toward == #F2EAD8`、`label_fill == #4A3F35`、`street_major == #A67C52` | 专业制图锚点值 | ✅ 测试即契约，改动会红；X01 口径下这条必然命中失败 |
| F8 | `wv-default-theme/src/theme_pack.rs:299-323` 注释 | 「**默认风格包 = 古舆图 · 有墨（V2）**」+「展示名暂保留专业制图字面值，UI 重命名留待 V3」；`display_name` 实为 `"专业制图"` | 若终局是专业色，注释应改回 | 🟡 文档层名实不符，不改渲染 |

**X01 覆盖缺口（口径，不是实现错）**：`2026-11-13-map-furniture-acceptance.md:83` X01 只要求「`resources.rs` / `shell.rs` 两处 diff 中 `#F2EAD8` 出现次数为 0」。实测这两处确为 0 ✅，但 `#F2EAD8` 的**真正源头** `colors.rs:301 SOFTEN_TOWARD` 经 `palette.rs:232` → `atmosphere.rs:113` / `faction_overlays.rs:289` / `theme_conv.rs:51` 仍在链上。当时的判断「纸色旧债已精确回退」只在这两文件范围内成立，不能推广到全仓。

---

## 3. 反设计：待办 #3「V3 收边」执行即白干

任务 #3（pending）描述逐条对照已落地的 (c) 规格 v1.0 / X01：

| #3 要求 | 已落地裁定 | 冲突 |
|---|---|---|
| 画布四边各 1 道**细褐墨图廓 `#6B5B49` 系** | 规格 §1.2 明列 `#6B5B49` 为「用户已明确弃用」语汇；F02 定稿**单线 1px `#9AA0A6`@0.55**、零双线零褐墨 | 直接相反，`FRAME_PLAN` / 4 段单测全推翻 |
| 罗盘复用 `assets/symbols/指南针_*` **PNG 贴图** | B07 零资产（禁 `include_bytes!` / 新字体 / 贴图）；C07–C14 定**程序化 14 图元** | 贴图资产实测存在（32/64/96 三档），照做即破坏 B07 |
| **比例尺 1 个** | B06「不含比例尺数字 / 不新增第 4 件装饰」；规格 §0.1 #7 明写「比例尺驻状态栏，本轮不新增、不搬回画布」 | 直接相反 |
| 欢迎页拉回**浅色壳**，顶栏/浮窗底色取**纸色一族** | X01「精确恢复原始基线、跟随界面外壳 `bg_app`，**不得**改 `#F2EAD8` / 白卡片」；规格 §3.2「留白仍归窗口 chrome」 | 直接相反 |
| 25 枚白工具图标按底色反色 / 加描边 | (c) 零 UI 图标改动，色值只走 `colors_google` 五令牌 | 越出 C 路线范围 |

**结论**：#3 是 ① 阶段的路线 A 收尾任务，已被 ②③ 两次裁定**整体作废**。若照它实施，会推翻 `map_furniture.rs` 41.7KB / 22 单测 / 56 项静态验收 / X01 回退，属确定性白干。建议标为「作废 / 需按终局裁定重写」，但**本稿不擅自改任务状态**，待用户拍板。

---

## 4. 待裁项（制作人不代答）

| # | 问题 | 为什么必须用户拍板 |
|---|---|---|
| D1 | ① 阶段那批古舆图未提交改动，是**作废**还是**仍有效**？ | 涉及推翻一整套已接线改动，触及 `conv_palette_terrain` / `CellPalette` 输入链 / 6 处字体 / 测试断言；且用户原话「古舆图全部去除」与工作树现状互斥，AI 不能替用户解释自己的裁定 |
| D2 | 若作废：是**整批精确回退到 HEAD 基线**，还是**人工挑拣保留**（例如 `ANTIQUE_TERRAIN` 作为可选变体常量、`decor.rs` 新增的 5 个 `*_style` 字段须保留）？ | 一刀切 `git checkout` 会连带回退 ② 阶段合规的去点点 / 去雪线 / 去白浪花改动，属于误伤 |
| D3 | `PaletteColors::DEFAULT` 与 (c) 三件套的取色是否要**统一到一个出处**（`colors_google`）？ | 规格 §1.2「单一出处是基调命根子」只约束了 UI 层；矢量层仍走 `colors::`，口径要不要扩展到全仓属架构决策 |
| D4 | 任务 #3 是标作废、还是按终局裁定重写成新收边任务？ | 属范围裁定 |

**明确不待裁、AI 已可判定**：`decor.rs` 七类符号全关 / `wave_band` 归零 / `terrain_decor.rs` 早退 / `colors_google.rs` 冷灰绿 8 阶 / `biome_tint=0.82` / WGSL 精确 `biome_table` / (c) 三件套——这七项与 ②③ 裁定一致，**保留**，任何回退方案都不得连带它们。

---

## 5. 三个处置方案（先给推荐项及理由）

### 方案 A【推荐】· 精确回退古舆图三件套，保留全部合规改动

只把 F1–F8 里属于 ① 阶段的部分恢复到 HEAD 基线：`palette.rs` 的 `core` + 30 令牌 + 测试锚点、`theme_conv.rs` 硬覆盖 + `mod.rs` 测试、`drill.rs`/`streets.rs` 六处 `label_handle()`、`region_outline.rs` 深蓝、`theme_pack.rs` 注释。**逐行挑拣、不用 `git checkout` 整文件回退**，因为 `decor.rs` / `theme_pack.rs` / `colors_google.rs` 混着 ② 阶段的合规改动。

- **理由**：与用户原话「古舆图 / Google 样式全部去除，全部改为专业色」及 (c) 规格 §1.2 枚举的弃用色值逐条对齐；X01 已开「精确恢复原始基线」先例，口径一致；回退后矢量层与三件套同走 `colors_google` 冷中性，D2 两套契约打架从根上消失。
- **代价 / 风险**：触及 `conv_palette_terrain`（ADR-0088 模式路由测试面邻近）、`CellPalette` 输入链、6 处字体分支；须重跑 `cargo test -p wv-default-theme`（54）、`-p wv-app --features app`（22+）、`-p wv-render --features app` 并补 `smoke-gui.sh`（若字体/材质改动触发 AGENTS §7.1）。约 8 文件，纯回退无新设计。
- **红线**：冻结群系表 / `CellPalette` 冻结值 / `conv_sea_flat` / `TreatmentFlags::EMPTY` / `terrain.bin` / `wv-io` / `PassId::MapFrame` / ADR-0066 / `export_png.rs` / 既有文档一律不动。

### 方案 B · 保留现状，登记为「三层混搭」既定契约

代码零改动；把「GPU 主画布群系真色 + 矢量层古舆图褐墨衬线注记 + UI 冷中性装饰」写成既有设计，并追溯修订 X01 覆盖面与 (c) 规格 §1.2 的弃用色清单。

- **理由**：零风险、零回归；沙箱看不了帧，避免盲改。
- **代价**：与用户原话正面冲突，且 (c) 规格 / 验收清单 B01「禁 `#4A3F35` / `#F2EAD8` / `#6B5B49`」要整段改写；等于承认「古舆图没去除干净」。**不推荐**。

### 方案 C · 最小止血：只回退纸色源，其余等实机

仅把 `soften_toward` / `halo_fill` / `road_casing` 从 `#F2EAD8` 拉回专业白，褐墨注记与书法体留到用户桌面看帧再定；同时把本对账稿与 #3 作废建议落盘。

- **理由**：改动面最小（3 令牌 + 测试），先在 X01 口径内把纸色清干净；沙箱无帧，不盲动字体。
- **代价**：D2 混搭仍在，#4A3F35 与衬线字继续与三件套同帧；下一轮还得再裁一次。适合「先不动大结构、等实机」的节奏。

> **共同边界**：三方案本轮都**只产出文档与留痕，不改任何代码、不提交**；实施须用户拍板后另起一轮。按 AGENTS.md T5，既有文档只 append 追记、不整行覆盖。

---

## 6. 验证状态（本稿亲手复跑，非采信回报）

- `TEMP/TMP=.target/testtmp cargo test -p wv-app --features app --lib map_furniture` → **22 passed / 0 failed**（8.72s；三条 `os error 5` 是沙箱增量锁文件删除告警，非 Rust 代码级告警）
- `git diff --stat` → 19 文件 / 378 insertions / 146 deletions，另有 8 个未跟踪文件
- `git diff -- resources.rs shell.rs` → 纸色禁用串 `F2EAD8` / `GOOGLE_CONTROL_BG` / `242.0/255.0` 命中 **0**；基线 `ShellPalette::DARK.bg_app` / `cfg.palette().bg_app` 各保留 —— X01 在**其自身范围内**成立 ✅
- `grep '#[test]' map_furniture.rs` → **22**，与复跑数一致
- 消费点 grep：`conv_palette(&view.active_theme.palette)` 在 `atmosphere.rs:105` / `grid_lines.rs:60` / `isoline.rs:94,254,273` 命中；`palette.label_fill` / `halo_fill` 在 `drill.rs` / `streets.rs` / `vector_layers.rs` 命中 → **非空转已证**
- 沙箱 GUI/GPU 被企业策略拒绝，**8 项实机验收（Y1–Y8）仍未做**；本稿不含任何观感结论

## 7. 红线与落盘清单

冻结群系表 / `CellPalette` / `conv_sea_flat` / `TreatmentFlags::EMPTY` / `terrain.bin` / `wv-io` / `PassId::MapFrame` / ADR-0066 / `recipe.rs:663-675` / `export_png.rs` / 既有 GLOSSARY·ADR·README / 既有 docs 全未动；零新依赖、零新资产、无 `#[ignore]`、**未提交**。

本轮唯一写盘：`docs/reviews/2026-11-13-visual-direction-conflict-reconciliation.md`（本文件，评审草稿 · 未采纳）与 `.workbuddy/memory/2026-11-13.md` 的 append-only 追记。

---

# 追记 · 2026-11-13 · 用户裁决：三方案均不取，改走「重新设计配色」

> 按 AGENTS.md T5 只 append、不改上文。上文 §5 的 A/B/C 三方案与「停在待拍板」状态是**裁决前快照**，以本节为准。

## 裁决过程

用户先选了 **A 精确挑拣回退（推荐）**；AI 正要取 HEAD 基线时，用户给出纠正：

> 「我觉得你可以重新设计过正儿配色」

AI 立即停止并说明「回退到 HEAD 等于回到用户要去掉的 Google 旧色板（`#AADAFF` / `#C9E7C7` / `#EA4335`），目前尚未改动任何 `.rs`」。随后用户就范围再拍板：

> **「A 只重建矢量层色板（推荐）」**

→ 最终路线**不是**上文三方案中的任何一个：既不回退到 Google 旧色，也不保留古舆图褐墨，而是**新建项目自有第三套 `WV_*` 色板**。规格见 `docs/reviews/2026-11-13-wv-palette-spec.md`。

## D1–D4 待裁项的落地结果

| 项 | 上文状态 | 裁决后处置 |
|---|---|---|
| D1 古舆图批次作废与否 | 待裁 | ✅ **剔除但不是回退**：`ANTIQUE_TERRAIN` 整段删除、褐墨 / 纸色 / 书法衬线体 / 褐墨轮廓 / 锚点测试反转全部替换为 `WV_*`。`colors.rs` / `sea.rs` 已回到 HEAD 零差异 |
| D2 整批回退 vs 挑拣保留 | 待裁 | ✅ **逐行挑拣实施完成**，全程未用 `git checkout` 整文件回退；② 阶段七项合规改动（`decor.rs` 七符号全关 / 冷灰绿 8 阶 / `biome_tint=0.82` / WGSL 精确 `biome_table` / 去浪花 / 去雪线 / (c) 三件套）与 X01 清屏色回退、启动 panic 修复**全部保留**；「移除 `GoogleMap` 硬覆盖」结构保留（回退它等于把 Google 地形色请回来） |
| D3 单一出处口径是否扩到矢量层 | 待裁 | ✅ **用户拍板 A 已扩**：`colors_wv.rs` 成为矢量层唯一出处，36 槽全量重绑；五枚中性墨别名重导出 (c) 三件套令牌，同帧同源 |
| D4 任务 #3「V3 收边」 | 待裁 | ✅ **保持作废登记**，未实施、未擅自改任务状态（褐墨图廓 / 指南针贴图 / 比例尺 / 浅色纸壳与 (c) 规格逐条冲突） |

## 上文 §6 验证状态的更新

上文 §6 是**实施前快照**（19 文件 / +378 / −146、8 未跟踪文件、22 条 `map_furniture`）。实施后亲手复跑：

- `cargo check -p wv-app --features app` ✅
- `wv-default-theme` **60 passed**、`wv-render --features app` **495 passed**、`wv-app --lib` **495 passed**、`map_furniture` **22 passed**、`wv-world-cell-map` **7 passed / check ✅**
- 首轮 `wv-app` 全量曾 **1 条 FAILED**（`hover_tooltip::kind_color_per_kind_distinct`，规格 §2.4 POI 四族只剩三色），已按「用足第三档中性明度」裁决修复并复跑全绿；未放宽断言
- `git diff --stat` → 19 文件 / **+410 / −157**；新增未跟踪 `colors_wv.rs`

**代码已改**（上文「代码零改动」不再成立）；冻结红线与 (c) 三件套全未动；零新依赖 / 资产；**未提交**。沙箱 GUI/GPU 仍被拒，**8 项实机验收 Y1–Y8 仍未做**，所有结论仍为代码级 / 数值级，不含观感判断。
