> **评审清单 · 未采纳 · 供制作人复算**

# 地图装饰三件套「规格 → 实现」符合性验收清单

| 项 | 内容 |
|---|---|
| 评审人 | 甄塑之 · 美术总监 |
| 日期 | 2026-11-13 |
| 版本 | **v1.2** · **本清单本身是「未采纳稿」**，只供制作人独立复算，不是验收结论、不是施工单（修订：① 第 4 问转 T01 + 新增 X01 ② L08 裁定「可替代 · 二选一」+ C03 α 合成 API 更正） |
| 规格基准 | `docs/reviews/2026-11-12-map-furniture-spec.md` **v1.0**（用户 2026-11-13 以「go」拍板采纳） |
| 实现基准 | `crates/wv-app/src/ui/map_furniture.rs`（主程按 v1.0 重写中；本清单所列「旧值」取自重写前骨架快照，行号见 §七） |
| 核验对象 | 只验「规格定值 → 实现证据」是否逐条对齐，**不出新设计、不改任何 `.rs` / WGSL / 冻结表 / `CellPalette` / 既有 GLOSSARY·ADR·README / 既有 docs 文件**；不改 2026-11-12 规格正文；不装依赖 / 不加资产 / 不提交 / 不用 `#[ignore]` |
| 沙箱限制 | 无法启动 GUI、无法看实际帧。故本清单把验收切成「可静态核验」与「必须实机看帧」两类，静态只判常量 / API / 几何数 / 色令牌名 / 布尔门控是否出现 |

---

## 0. 读法与计数口径（制作人先读这一节）

- 每行三列：**规格定值 | 期望实现证据 | 核验方式**。证据必须写到「应出现 / 不应出现的常量名、API 调用、色令牌名、几何数量、布尔字段」，**不写「看起来克制」「观感清爽」这类不可判表述**（唯一例外：§八 Y 项，明确标注静态判不了、转实机）。
- **核验方式三选一**：`grep`（源码字符串命中 / 零命中）/ `单测断言`（`FurnitureGate`、图元计数、逐通道 RGB 比对等可执行断言）/ `实机截图`（必须看帧）。一行只标一个主方式；需要实机补充的，在证据列注明「另见 §八 Y-n」。
- **色令牌名以本清单 §0.1 为准**；凡写「`#XXXXXX`」处，实现里应出现的是**令牌名或引用令牌的常量**，不是裸 hex 字面量（边界裁定见 §九）。
- **计数（全文自洽，可逐行复算）**：
  - 主表共 **57** 条验收行 = §3.1 罗盘 14 + §3.2 图廓 6 + §3.3 图例 10 + §3.0 共用 6 + §四 门控/负向 12 + §六 B1–B7 七条 7 + **X01（§2.1 纸色旧债回退）1** + **T01（§九 取色链路）1**。
  - 其中 **56 条主核验为 `grep` / `单测断言` → 可静态核验 56 项**（含 2026-11-13 追记新增 X01 / T01 两条）。
  - 余 **1 条（B02 · 50% 缩略图双向可辨）主核验为 `实机截图`**；与 §八另外 7 条「静态只能验到常量、像素/交互层面判不了」的补充项合并，**必须实机 8 项（Y1–Y8）**。
  - **原派单第 4 问（UI 层镜像令牌是否可接受）由 team-lead 独立复算解决前提**：分层无阻碍、令牌可直达 → **不作待裁、改写为静态验收项 T01，且不列入 §八 实机清单**。§八 Y1–Y8 逐条复核均为像素混合 / 缩略图 / 浮窗遮挡 / 输入命中 / 跨帧残留等观感项，与令牌可达性无关，一律保留，实机计数不虚减。
  - 一句话：**可静态核验 56 项 / 必须实机 8 项**。

### 0.1 唯一色板 → 上游令牌名（已亲读 `crates/wv-default-theme/src/colors_google.rs`）

| 规格角色 | hex | 应出现的上游令牌（行号） | 本轮核验状态 |
|---|---|---|---|
| 一级墨 | `#3C4043` | `GOOGLE_LABEL_FILL:150`、`GOOGLE_CONTROL_TEXT:228` | 已证实 |
| 二级墨 | `#9AA0A6` | `GOOGLE_ISOLINE_INK:160`、`GOOGLE_LINE_ON_MID:179` | 已证实 |
| 三级墨 | `#5F6368` | `GOOGLE_LINE_ON_DARK:180` | 已证实 |
| 极淡线 / 描边 | `#DADCE0` | `GOOGLE_CONTROL_BORDER:226`、`GOOGLE_LINE_ON_LIGHT:178`（另 `GOOGLE_GRID_LINE:162` / `GOOGLE_BUILDING_SIDE:166` / `GOOGLE_SNOW_SHADOW:170` 同值） | 已证实；规格 §1.2 另列 `GOOGLE_ROAD_CASING:68`，**本轮未亲读该行，制作人复算时补核** |
| 面板底 / 反白 | `#FFFFFF` | `GOOGLE_CONTROL_BG:224`、`GOOGLE_LABEL_STROKE:147`（另 `GOOGLE_SNOW_HIGHLIGHT:168` 同值） | 已证实 |

**取色链路（已亲读）**：`crates/wv-app/src/theme/mod.rs:34` `pub use wv_default_theme::{colors, colors_google, palette};`；`:43` `pub fn palette_to_color32(c: bevy::color::Color) -> bevy_egui::egui::Color32`。UI 层现成用例：`ui/hover_tooltip.rs:137,148,150`、`ui/list_panels_panel.rs:164`、`ui/situation_panel.rs:69`、`ui/inspector/situations.rs:49`。→ **`wv-app` 内取令牌不存在分层阻断**，详见 §九。

---

## 一、§3.1 罗盘 14 项（C01–C14）

> 规格 §3.1 表格 11 行 + 「图元总数 ≤14」的 14 件可数图元，逐件转成验收行。锚点、尺寸、色、α、朝向分别钉进对应行，不重复编号。

| ID | 规格定值 | 期望实现证据（应出现 / 不应出现） | 核验方式 |
|---|---|---|---|
| C01 | 位置锚点 = `central_area` **右上角**，距上 24px、距右 24px | 应出现 `central.max.x - 24.0 - COMPASS_R` 与 `central.min.y + 24.0 + COMPASS_R`（或等价 `right_top()` 系锚点）；**不应出现**骨架的 `central.min.x + COMPASS_OFFSET + COMPASS_R` / `central.min.y + ...`（左上锚）。常量 `COMPASS_OFFSET: f32 = 54.0` 应被 `24.0` 边距常量替换或删除。另见 §八 Y3（与 `panel_inspector` 遮挡） | grep |
| C02 | 底板 64×64px（`circle_filled` r=32），圆心内容直径 56px 恒定，**不随窗口缩放** | 应出现 `COMPASS_R = 32.0`（底板半径）与内容半径 `28.0`；绘制参数中**不应出现** `central.width()` / `viewport_rect()` 作为半径输入、不出现 camera / zoom / 世界→屏幕换算 API。骨架 `COMPASS_R = 22.0` 应改。另见 §八 Y1（50% 缩略图北向可辨） | grep |
| C03 | 底板 `circle_filled` r=32，`#FFFFFF` @ **α=0.92** | 应出现 `p.circle_filled(c, 32.0, egui::Color32::from_rgba_unmultiplied(r, g, b, 235))`，其中 R/G/B 由 `palette_to_color32(GOOGLE_CONTROL_BG).to_srgba_unmultiplied()` 取得；α 合成映射固定 **0.92→235、0.94→240、0.70→179、0.55→140**（后三者供 L05 / C05 / F05 复用，均为 `from_rgba_unmultiplied` 第四字节）；**不得**用顶栏的 `.gamma_multiply()` 近似拼 α（`ui/top_bar/status.rs:73` 明注「不学顶栏的 gamma_multiply 近似」；`wv-ui/src/ui_tokens.rs:239-250` 明注 Color32 为预乘格式、必须走 `from_rgba_unmultiplied`，直接改第四字节会过亮）；骨架**无** `circle_filled` 调用（只有 `circle_stroke`），须新增恰好 1 次。另见 §八 Y2（0.92 透出感） | grep |
| C04 | 底板 `circle_stroke` 1px `#DADCE0` @1.0 | 应出现 `egui::Stroke::new(1.0, ...GOOGLE_CONTROL_BORDER / GOOGLE_LINE_ON_LIGHT...)` 且 α=1.0；`circle_stroke` 在罗盘内总次数 = 3（1 描边 + 2 环），不多不少 | grep |
| C05 | 外环 r=28，1px `#9AA0A6` @**0.70** | 应出现 `p.circle_stroke(c, 28.0, Stroke::new(1.0, ...GOOGLE_ISOLINE_INK / GOOGLE_LINE_ON_MID... α=0.70))`；环半径不得为 22.0（骨架值） | grep |
| C06 | 内环 r=20，1px `#DADCE0` @1.0 | 应出现 `p.circle_stroke(c, 20.0, Stroke::new(1.0, ...GOOGLE_LINE_ON_LIGHT...))`；罗盘 `circle_stroke` 调用总数 = 3（C04+C05+C06），**不应出现**第 4 个环 | grep |
| C07 | 北瓣 = 等腰三角，底宽 8px / 高 18px，`#3C4043` **实心** | 应出现 1 次 `p.add(egui::Shape::convex_polygon(...))` 或 3 条 `line_segment` + `Shape::convex_polygon` 填 `GOOGLE_LABEL_FILL`；顶点坐标含 `8.0`（半底宽 4.0）与 `18.0`；**不应出现** 8 向星芒 / 三角旗 / fleur-de-lis 绘制 | grep |
| C08 | 南瓣 = 同形倒置，`#FFFFFF` **空心** + 1px `#9AA0A6` 描边 | 应出现倒置三角 1 个，填充 `GOOGLE_CONTROL_BG`、描边 `Stroke::new(1.0, ...GOOGLE_ISOLINE_INK...)`；针瓣总数 = 2（C07+C08），骨架 `8 条 line_segment` 射线应**全部消失** | grep |
| C09 | 十字基线 = 2 条 1px `#DADCE0`，水平 / 垂直各长 36px，过圆心，不出底板 | 应出现恰好 2 条 `p.line_segment` 长 `36.0`（半长 18.0）且以 `c` 为中点；`18.0 ≤ 32.0`（r=32）成立；颜色 `GOOGLE_LINE_ON_LIGHT`；**不应出现**骨架的 `long = COMPASS_R*0.82` / `short = COMPASS_R*0.5` 斜向射线 | grep |
| C10 | 刻度 = **4 个基点** N/E/S/W，各 4px 长、1px `#9AA0A6`，只在外环内侧 | 应出现 4 条长 `4.0` 的 `line_segment`（半径区间 `r<28.0`），颜色 `GOOGLE_ISOLINE_INK`；**不应出现** 8 / 16 / 32 分度循环（如 `for i in 0..8|16|32`）、`step` 角度分度盘 | 单测断言（图元计数） |
| C11 | 指北标记 = 外环正上方 2px 处 1 个 **「N」**，12px Regular `#3C4043` | 应出现恰好 1 次 `p.text(..., "N", proportional(12.0) 或等价 12px FontId, ...GOOGLE_LABEL_FILL...)`；文字位置 `c.y - 28.0 - 2.0` 系；**不应出现** `proportional(TEXT_PX)`（10.0）、`shell.t2()`、三角旗 / 星芒 / 装饰字母 / 多语言方位字 | grep |
| C12 | 圆心 = 1 个 r=2px `#3C4043` 实心点 | 应出现 1 次 `p.circle_filled(c, 2.0, ...GOOGLE_LABEL_FILL...)`；`circle_filled` 在罗盘内总数 = 2（C03 底板 + C12 圆心） | grep |
| C13 | 朝向 = N 恒定朝屏幕正上方，**不绑相机旋转** | 绘制代码**不应出现** `world_to_screen` / `camera` / `rotation` / `view.rotation` / 世界→屏幕反向换算调用；方位角应为编译期常量（0 = 正上）。骨架无此调用，重写后须保持零命中。另见 §八 Y8（转相机后 N 是否仍朝上） | grep |
| C14 | 图元总数 **≤ 14**（1 底板 + 1 描边 + 2 环 + 2 针瓣 + 2 基线 + 4 刻度 + 1「N」+ 1 圆心）；环 ≤2 / 针 ≤1 根双瓣 | 单测断言枚举罗盘绘制产出的 `egui::Shape` 数 = 14（或 ≤14），其中 `circle_filled` 2、`circle_stroke` 3、`line_segment` 6（2 基线 + 4 刻度）、凸多边形 / 三角 2、`text` 1；断言环数=2、针瓣=2、刻度=4。另见 §八 Y5（占屏 / 三测） | 单测断言 |

---

## 二、§3.2 图廓 6 项（F01–F06）

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| F01 | 锚点 = `central_area` 四边距边 **8px**；不跨入顶栏 38 / 状态栏 28 | 应出现 `central.shrink(8.0)` 或四边 `±8.0`；骨架 `FRAME_INSET: f32 = 12.0` 应改 8.0；**不应出现** `ctx.viewport_rect()` 直接作矩形 | grep |
| F02 | **单线 1px** `#9AA0A6` @**0.55**；零双线、零内外框、零刻度尺 | `rect_stroke` / `line_segment` 生成的闭合矩形**恰好 1 个（4 段）**；`Stroke::new(1.0, ...GOOGLE_ISOLINE_INK / GOOGLE_LINE_ON_MID...)`；骨架第 2 次 `p.rect_stroke(inner, 0.0, Stroke::new(INK_W * 0.5, shell.border_subtle()), Inside)` 与常量 `FRAME_GAP: f32 = 3.0` 应**完全消失**；`StrokeKind::Inside` 可用，`rounding=0.0` | 单测断言（几何段数 = 4，双线数 = 0） |
| F03 | 留白边 = 线外到 `central_area` 边缘 8px **保持透明**、不填色不铺纸纹；线内无任何填充 | **不应出现**覆盖 `central` 或 `outer` 的 `rect_filled` / `Shape::rect_filled` / 纹理 / `mesh`；零 `GOOGLE_PAPER` / 纸纹 / 噪点调用；`draw_frame` 内填充类调用数 = 0 | grep |
| F04 | 四角 = **直角直接相接、不收口**；不倒角、不 L 形加粗、不角标 / 角花 / 四角刻度 | `rect_stroke` 的 `rounding` 参数 = `0.0`；**不应出现**角部额外 `line_segment` / `circle_*` / 角标绘制循环；四角图元数 = 0（除矩形本身）。另见 §八 Y6（读成结构边而非画框） | grep |
| F05 | α ≤ **0.55** 硬上限 | 应出现 α 常量 `0.55`（或 `≈140/255`）且 `assert!(FRAME_ALPHA <= 0.55)` 成立；**不应出现** `shell.border_strong()` / `shell.border_subtle()` / α=1.0 的图廓描边。另见 §八 Y4（叠咸水 / 冷灰绿 ≤45 L、不改色相） | grep |
| F06 | 与世界空间 `MapFrame` **不是同一对象，不复活、不打开** | 全文**零命中** `MapFrame` / `PassId::MapFrame` / `map_frame.rs` / 「启用 MapFrame」；`draw_frame` 只走 `ui.painter()` 屏幕空间图元 | grep |

### 2.1 补充静态验收行 X01 · 纸色旧债回退（不计入 §3.2 的 6 项，单列）

> 2026-11-13 team-lead 追加：回退定为**精确恢复原始基线**、跟随界面外壳，**不是改成 `#FFFFFF`**。依据规格 §3.2「留白仍归窗口 chrome」「本轮明确不解决 D4」——把留白刷成白卡片等于把空白「做成纸」，回到古舆图，与 §一 1.1 / §5.2 相冲。

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| **X01** | 纸色旧债 `#F2EAD8` **精确恢复原始基线**：`resources.rs` 用 `ShellPalette::DARK.bg_app`、`shell.rs` 用 `cfg.palette().bg_app`（跟随界面外壳）；**不得**改成 `#FFFFFF` / `GOOGLE_CONTROL_BG` | 两处 diff 中 **出现次数必须为 0**：`#F2EAD8`、`242.0/255.0, 234.0/255.0, 216.0/255.0`（含无空格写法 `242.0/255.0,234.0/255.0,216.0/255.0`）；**应出现** `ShellPalette::DARK.bg_app`（`resources.rs`）与 `cfg.palette().bg_app`（`shell.rs`）；**不应出现**把该处替换为 `#FFFFFF` / `GOOGLE_CONTROL_BG` / `1.0, 1.0, 1.0` 的写法；绘制层（F03）亦零纸色 / 零纸纹命中 | grep |

---

## 三、§3.3 图例 10 项（L01–L10）

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| L01 | 按 `BIOME_LEGEND_GROUPS[8]` 分组折叠；**默认不列 21 项全表**；折叠态 = 8 族名 + 8 色块 | 应出现 `BIOME_LEGEND_GROUPS.iter()` 且行数 = 8；折叠态**不应出现**对 `BIOME_NAMES_ZH[21]` 的逐项遍历；单测断言折叠态行数 = 8（`BIOME_LEGEND_GROUPS.len() == 8` 骨架已有 `legend_group_data_is_read_without_mutating_the_frozen_table`，须扩到绘制层） | 单测断言 |
| L02 | 族色块 = **运行时直取 `BIOME_RAW_COLORS[group[0]]`**（首个非 255）；不复制第二份色表、不硬编码、不重映射、不提饱和、不改冻结表 | 应出现 `members.iter().copied().find(|&b| b != 255)` → `BIOME_RAW_COLORS[first]`（骨架已有，须保留）；**不应出现**第二份 `[u8;3]` 字面色表、`Color32::from_rgb` 常量色块、饱和度运算；单测断言水域族首色 = `#9DC3D8`、道路·无族首色 = `#9E9E9E`；`BIOME_RAW_COLORS` 只被索引、无 `mut` | 单测断言 |
| L03 | 色块 **12×12px** 正方形，**圆角 0**，1px `#DADCE0` 内描边 | 应出现 `egui::vec2(12.0, 12.0)`、`rect_filled(..., 0.0, ...)`（骨架 `SWATCH: f32 = 10.0` 应改 12.0；骨架 `rect_filled(sw, 1.0, ...)` 圆角 1.0 应改 0.0）、`Stroke::new(1.0, ...GOOGLE_CONTROL_BORDER...)`；色块 `rect_stroke` 数 = 8 | grep |
| L04 | 标题「群系图例」11px `#3C4043` Regular；族名 11px `#3C4043` Regular；展开项 10px `#5F6368` Regular + 缩进 12px；族名左 4px 竖条用族代表色（唯一允许颜色承载信息处，且有文字兜底） | 应出现 `proportional(11.0)` ×2、`proportional(10.0)` ×1、`GOOGLE_LABEL_FILL`、`GOOGLE_LINE_ON_DARK`、缩进常量 `12.0`、竖条宽 `4.0`；骨架 `TEXT_PX: f32 = 10.0` + `proportional(TEXT_PX - 1.0)`（9px）+ `shell.t2()` / `shell.t3()` 应消失；**不应出现** `.strong()` / `RichText::strong` / bold `FontId`（见 G03） | grep |
| L05 | 面板宽 **168px**；`#FFFFFF` @**0.94** + 1px `#DADCE0` + **圆角 2px** | 应出现 `LEGEND_W = 168.0`（骨架 148.0）、`rect_filled(panel, 2.0, ...GOOGLE_CONTROL_BG α=0.94...)`、`rect_stroke(panel, 2.0, Stroke::new(1.0, ...GOOGLE_CONTROL_BORDER...))`；骨架 `rect_filled(panel, 4.0, shell.float_fill())` / `rect_stroke(panel, 4.0, shell.border_strong())` 应替换；`shell.float_fill()` 零命中。另见 §八 Y2（0.94 透出感） | grep |
| L06 | 折叠态高 ≈**188px**（标题 28 + 8×20）；展开态上限 **340px**，超出用 `ScrollArea`，不占满画布 | 应出现 `LEGEND_ROW_H = 20.0`（骨架 16.0）、`TITLE_H = 28.0`、`LEGEND_MAX_H = 340.0`、`egui::ScrollArea`；单测断言 `28 + 8*20 == 188`、`min(展开内容高, 340.0)`；**不应出现** `rows * LEGEND_ROW_H + 10.0`（骨架 138px 算法）。另见 §八 Y7（展开是否占满画布） | 单测断言 |
| L07 | 位置锚点 = `central_area` **右下角**，距下 32px、距右 32px（含状态栏 28 后距屏幕边缘 60px ≥ 5%） | 应出现 `central.max.x - 32.0 - 168.0` 与 `central.max.y - 32.0 - h`；骨架 `LEGEND_EDGE: f32 = 22.0` 应改 32.0；`32.0 + 28.0 = 60.0 ≥ 54.0`（G05）成立 | grep |
| L08 | **美术域裁定：可替代 · 二选一**。**默认折叠**；标题栏右侧 12×12px 折叠指示符 = `ICON_CHEVRON_RIGHT`（折叠）/ `ICON_CHEVRON_DOWN`（展开）**或** 2 段 1px 折线三角（B4 规整几何），**二选一** | **应出现（二选一，不得两套并存）**：① `ICON_CHEVRON_RIGHT` / `ICON_CHEVRON_DOWN`（`ui/mod.rs:119-120` 现成）；或 ② 恰好 **2 段** `p.line_segment`、各 `Stroke::new(1.0, ...GOOGLE_LABEL_FILL / GOOGLE_LINE_ON_DARK...)`、5×5px 图形居中、两态仅换折线朝向（右 `>` / 下 `v`）。**两路共同**：`12.0×12.0` 命中区（不得因 5×5 图形缩小）、`collapsed: bool` 默认 `true`、**仅面板矩形内**的 `Sense::click()` widget（G06）、展开项 `BIOME_NAMES_ZH` 仅在 `collapsed == false` 分支绘制。**替代成立判据（缺一即失败）**：(a) 几何数 `line_segment == 2`，**零**第 3 段、零闭合 / 填充三角 `convex_polygon`、零实心箭头、零双 chevron、零圈 / 底框 / 分隔线；(b) **不引入第三族形状**——折线属「1px 细直线」族，全件仍只允许 {细直线, 正圆, 等腰三角, 直角矩形}，零圆角端点 / 阴影 / 右下投影 / 斜纹 / 抖动（B4）；(c) 色走 `GOOGLE_*` 中性墨、1px，**零** `shell.*` 暗壳令牌、零 `R>G>B` 暖序色、零族代表色（族色块是唯一允许颜色承载信息处，L04 / B01）；(d) 功能性控件指示符、朝向随折叠态变化，**不得**读作装饰花纹、不得加旋转动画，且不新增第 4 件装饰 / 不增图例行数 / 不计入罗盘 14 图元预算（B3 / B6 / B7）；(e) 走折线路时须在代码注明降级原因（纯 painter 背景层接不上纹理 / 字体图标链路）。骨架**零**折叠图标、零 `Sense` widget，须新增 | grep |
| L09 | `show_legend` 布尔 **默认开**（折叠态）；关掉后只剩罗盘 + 图廓 | 应出现 `show_legend: bool` 字段且 `Default` 为 `true`；单测断言 `show_legend == false` → `draw_plan()` 含 `Compass` + `Frame`、**不含** `Legend`（长度 2 或图例 part 被跳过）；骨架 `MapFurnitureState { enabled: bool }` 缺省 `false` 是总开关，与 `show_legend` 是**两个不同字段**，不得合并 | 单测断言 |
| L10 | 命名中文「**群系图例**」；禁「Legend」/ 衬线 / 字距 / 装饰横线 | 应出现字符串 `"群系图例"`；全文**零命中** `"Legend"`、衬线字体名、`letter_spacing` / 字距拉伸、标题下装饰横线 `line_segment` | grep |

---

## 四、§3.0 三件共用规则 6 项（G01–G06）

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| G01 | 锚点坐标系 = `central_area(ctx)`，**不是全屏** | 应出现 `crate::ui::top_bar::central_area(ctx)`（骨架 `:158` 已有，须保留）；三件绘制函数参数均接收 `central: egui::Rect`；**不应出现** `ctx.viewport_rect()` / `ctx.screen_rect()` 直接作锚点 | grep |
| G02 | 单位 = 屏幕 px，1080p 基准，**固定不随相机缩放 / 不随世界平移** | 绘制参数**不应出现** camera / zoom / `world_to_screen` / 平移向量 / `p.shape.y` 作为尺寸输入；`proportional()` 属 DPI 缩放不算违反（可保留） | grep |
| G03 | 字重一律 **Regular (400)**，不请求 bold | 全文**零命中** `.strong()`、`RichText::strong`、`FontId` 里 `family` 指向 bold / `Weight::BOLD`；层级只靠字号 + 缩进 + 位置（L04） | grep |
| G04 | 字号阶梯 **12 / 11 / 10 px**，对齐 `STATUS_FONT = 10.0` 与 `MICRO(11)`；装饰字号**不得大于**既有 chrome 读数 | 出现的 `proportional(...)` / 字体尺寸字面量集合 ⊆ `{12.0, 11.0, 10.0}`；**不应出现** >12.0 的字号；骨架 `proportional(TEXT_PX - 1.0)` = 9.0 应改 10.0 | grep |
| G05 | 安全区：折叠钮（唯一可交互元素）距屏幕边缘 ≥ 5%（1080p 54px）；`central_area` 内缩 32px + 状态栏 28px = 60px ✓；非交互装饰内缩 8–24px | 单测断言 `LEGEND_EDGE(32.0) + STATUS_BAR_H(28.0) == 60.0 >= 54.0`；罗盘边距 `24.0` ∈ [8,24]、图廓 `8.0` ∈ [8,24]；**不应出现** <8.0 的边缘内缩 | 单测断言 |
| G06 | 输入：`Sense::hover()` / `Sense::click()` **仅限面板矩形内**；不得吞地图点击 / 拖拽 / 笔刷 | 应保留骨架 `egui::Ui::new(... .layer_id(egui::LayerId::background()) .max_rect(central))` + 纯 `painter()` 形状（`:166-173`）；**新增**的交互 widget 必须 `Sense::click()` 且 `allocate_rect` 为图例面板矩形；**不应出现** `allocate_rect(central)` / 全视口 `Sense` / `interact_bg` 覆盖整块画布。另见 §八 Y7（拖拽 / 笔刷体感） | grep |

---

## 五、§四 可见性白名单与负向清单（W01–W03 + N01–N09，共 12 项）

> 含派单点名的 **3 条门控缺口**（W01 内 `!shows_loading_overlay()` ∧ `!playback.playing` ∧ `!is_stroking()`）。骨架 `FurnitureGate` 当前只有 `enabled / editor_visible / immersive / mode / branch` 五字段、`target_canvas()` 只判四项（`:99-104`），三条缺口**全部未判**。

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| W01 | 白名单唯一放行 = `ViewMode::VectorFlat` ∧ `view.terrain_visible = true` ∧ `view.field_render = true` ∧ `!screen.shows_loading_overlay()` ∧ `!playback.playing` ∧ `!paint.is_stroking()` ∧ 非欢迎页，**7 项全 AND** | `FurnitureGate` 应新增字段并全部 AND 进 `target_canvas()` / `draw_plan()`：`loading_overlay`、`playing`、`stroking`（并显式读 `view.terrain_visible` / `view.field_render`）；调用点应出现 `screen.shows_loading_overlay()`（`app_screen.rs:65`）、`playback.playing`、`is_stroking()`（`cell_paint.rs:295`）；单测断言 7 项中**任一项取反** → `draw_plan() == None`（逐项表驱动，7 条 case） | 单测断言 |
| W02 | 门控**不得**靠 `ViewBranch` / `p.shape.y` 推断 mode；须显式读 `view.mode` 写成独立布尔；且 `GoogleMap` 与 `VectorFlat` 同落 `Themed` | 应出现 `self.mode == ViewMode::VectorFlat` 显式比较；`ViewBranch` **只能作附加条件、不得作唯一 mode 判据**；全文**零命中** `p.shape.y`；保留骨架测试 `gate_branches_match_the_render_layers_single_choice_point`（`:396-424`，已钉住 GoogleMap 也是 Themed） | grep |
| W03 | 须处理「切模式不重绘 / 不重写状态」的旧值残留 | `FurnitureGate` 为纯数据快照、每帧由 system 重新构造；**不应出现**缓存上一帧 `visible` 的可变字段 / `static mut` / 跨帧残留布尔；单测断言连续 mode 序列 `[VectorFlat, GoogleMap, VectorFlat]` 求值结果 = `[Some, None, Some]` | 单测断言 |
| N01 | `ViewMode::GoogleMap`（Map 标签页）→ 一律不画 | `draw_plan() == None`，零图元；骨架 `google_map_professional_canvas_draws_nothing` 已有，须保留 | 单测断言 |
| N02 | `ViewMode::Topographic`（含 `CellFlat` / `BiomeRaw`）→ 一律不画；`biome_raw::pick` 输出逐位不变 | `draw_plan() == None`；单测断言图例**数据源**只读、绘制层零命中；图例面板本身不在此视图绘制 | 单测断言 |
| N03 | `ViewMode::Faction` / `ViewBranch::Flat`（政治素底页）→ 一律不画 | `draw_plan() == None`；骨架 `hidden_terrain_political_flat_base_draws_nothing` 已有 | 单测断言 |
| N04 | `view.terrain_visible = false` → 一律不画 | `draw_plan() == None`（须显式读该字段，见 W01） | 单测断言 |
| N05 | `view.field_render = false`（CPU 烘焙留守档）→ 一律不画 | `draw_plan() == None`（须显式读该字段，见 W01） | 单测断言 |
| N06 | `ViewMode::Event` / `ViewMode::Coastline` → 一律不画 | `draw_plan() == None`；骨架 `every_other_view_mode_draws_nothing` 已含此二项 | 单测断言 |
| N07 | `ViewMode::RealTerrain` → 一律不画（白名单外） | `draw_plan() == None`；日后放行须单独扩白名单、不得放宽分支判断 | 单测断言 |
| N08 | **加载遮罩 / 欢迎页 / 播放中 / 笔刷 stroke 期间** → 一律不画；三者任一为真即全隐 | `draw_plan() == None`；`shows_loading_overlay()` / `playback.playing` / `is_stroking()` 任一 `true` 的表驱动 case 全绿（与 W01 同一组断言，单列便于复算） | 单测断言 |
| N09 | 小地图浮窗 `panel_minimap` / **导出 PNG 路径 `export_png`** → 一律不画，三件套不进出图 | `map_furniture_system` 只挂在窗口 `EguiPrimaryContextPass`；**不应出现**在 `ui/export_png.rs` 离屏 `Camera2d` 链路中；导出图零装饰。另见 §八 Y8 | grep |

---

## 六、§六 B1–B7 七条可判边界（B01–B07）

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| B01 | 任一绘制色 `S=(max−min)/max ≤ 0.12` ∧ `\|R−B\| ≤ 12`；禁任何 `R>G>B` 暖序色；禁 `#F2EAD8` / `#D6B77B` / `#4A3F35` / `#6B5B49` / `#5A4E42`；配色只从 `colors_google.rs` 既有令牌取 | 单测断言枚举全部绘制色（五角色 hex）逐点算 `S` 与 `\|R−B\|`：`#3C4043`→S=0.104/7、`#5F6368`→0.087/9、`#9AA0A6`→0.072/12、`#DADCE0`→0.027/6、`#FFFFFF`→0/0 全部通过；禁色 hex 与 `UiShell` 暗壳色在绘制层**零命中**；`shell.t2()/t3()/float_fill()/border_*()` 零命中 | 单测断言 |
| B02 | **50% 缩略图双向**：(a) 北指向仍可辨（「N」+ 实心北瓣）；(b) 数不出刻度根数、读不出图例群系文字 | 静态**无法判定**（需真实帧 + 缩放 + 人工 / 图像二值化计数）→ **本条计入必须实机 Y1**，最短步骤见 §八。静态侧仅能验到 C02 / C10 / L04 的常量，不等于通过本条 | **实机截图** |
| B03 | 罗盘环 ≤2 / 针 ≤1 根双瓣 / 刻度 ≤4 / 图元 ≤14（禁 8·16·32 分度盘、禁星芒、禁百合花饰）；图廓**单线 1 条**、零双线、零角饰、零刻度尺；图例默认 ≤8 行、展开 ≤21 行且 ≤340px；三件套合计占屏 ≤5%、全幅新增着色像素 ≤2% | 单测断言 `环数==2 && 针瓣==2 && 刻度==4 && 罗盘 Shape<=14 && 图廓矩形数==1 && 折叠行==8 && 展开<=21 && 面板高<=340.0`；面积常量：`168*188 = 31584 px²`、`168*340 = 57120 px²`（1080p 基准 1.5% / 2.8%），`64*64 = 4096 px²`；21 行全列 ≈420px → 失败常量断言。新增着色像素 ≤2% 静态判不了 → §八 Y5 | 单测断言 |
| B04 | 形状语言唯一 = {1px 细直线、正圆、等腰三角、直角矩形}；禁圆角 >2px、斜纹、手绘抖动、±2px 抖动、右下投影、水彩 / 纸纹 / 噪点、衬线字、字距拉伸、装饰花纹 | `grep` 枚举图元 API 白名单 ⊆ `{circle_filled, circle_stroke, line_segment, rect_filled, rect_stroke, Shape::convex_polygon, text, add(Shape)}`；所有 `rounding` 字面量 ≤ `2.0`（面板 2.0、色块 0.0、图廓 0.0）；**零命中** `4.0` 圆角、噪声 / 抖动 / `shadow` / 斜纹 / `8 向射线` 循环。骨架 3 处 `4.0` / `1.0` 圆角须清理 | grep |
| B05 | **切到 Map（`GoogleMap`）三件套 100% 消失**；CellMap / 政治素底 / 隐藏地形 / 加载 / 播放中同样全消失 | §五 N01–N08 全部 `draw_plan() == None`；即 B05 = N01–N08 的合取，单测逐条跑通即通过 | 单测断言 |
| B06 | 不含比例尺数字 / 经纬度刻度 / 图名·制图者·年份注记 / 除「N」外方向文字；不新增第 4 件装饰；**不进出图**（导出 PNG 零装饰） | **零命中** `map_scale_bar` 新增调用、经纬度 / `°` / 年份 / 「制图」字符串、`p.text` 文本集合 ⊆ `{"N", "群系图例", 8 族名, 展开项中文名}`；`draw_plan` 数组长度 = 3（`FurniturePart` 变体 = 3）；导出链路零命中（N09） | grep |
| B07 | 零资产（无新图片 / 字体 / 音频 / 二进制）；罗盘全用 `ui.painter()` 现成图元；文字走既有系统栈；**规格 / 实现中不得出现「启用 / 打开 `MapFrame`」** | diff 中 `assets/` 新增文件数 = 0、`Cargo.toml` 依赖增量 = 0；**零命中** `include_bytes!` / `egui::TextureHandle` / 新字体加载；`painter()` 图元全覆盖；`MapFrame` / `PassId::MapFrame` 零命中（同 F06） | grep |

---

## 七、5 处方向相反项的替换判据（旧值 → 新值）

> 「旧值」= 派单给定的重写前骨架快照（`crates/wv-app/src/ui/map_furniture.rs` 行号已亲读）；「新值」= 规格 v1.0 定值。**每行给可验证对照，含色令牌名与几何数**。制作人对**重写后**文件跑 grep / 单测即可判「换没换干净」。

| # | 项 | 旧值（骨架，应消失） | 新值（规格，应出现） | 可验证判据 |
|---|---|---|---|---|
| 1 | **罗盘位置：左上 → 右上** | `COMPASS_OFFSET: f32 = 54.0`（:55）；圆心 `c = egui::pos2(central.min.x + COMPASS_OFFSET + COMPASS_R, central.min.y + COMPASS_OFFSET + COMPASS_R)`（:203-206）→ `central.min` = 左上；`COMPASS_R: f32 = 22.0`（:56）；几何 = 1 `circle_stroke` + 8 条 `line_segment`（4 正 `long=R*0.82` + 4 隅 `short=R*0.5`，:209-222）+ 1 `text "N"` @ `proportional(10.0)` / `shell.t2()`（:223-229）；**0** 个 `circle_filled`、**0** 底板、**0** 十字基线、**0** 圆心点、**0** 南瓣；环数 1、刻度 0、图元 10 | 锚点 `central.max.x - 24.0 - 32.0` / `central.min.y + 24.0 + 32.0`（右上，距上/右 24px）；`COMPASS_R = 32.0`、内容半径 28.0；几何 = **14** 件（1 底板 `circle_filled` + 1 `circle_stroke` 描边 + 2 环 + 2 针瓣 + 2 基线 + 4 刻度 + 1「N」+ 1 圆心点），环数 **2**、刻度 **4**、基线 **2**；色令牌 `GOOGLE_CONTROL_BG`(#FFFFFF@0.92) / `GOOGLE_CONTROL_BORDER`(#DADCE0) / `GOOGLE_LINE_ON_MID`(#9AA0A6@0.70) / `GOOGLE_LINE_ON_LIGHT`(#DADCE0) / `GOOGLE_LABEL_FILL`(#3C4043)；「N」12px | `grep`：`central.min.x + COMPASS_OFFSET` 零命中、`central.max.x - 24.0` 命中、`COMPASS_R = 32.0` 命中；单测：`circle_filled` 计数 2、`circle_stroke` 3、`line_segment` 6、`text` 1、总 Shape = 14；`shell.t2()` 零命中 |
| 2 | **图廓双线 → 单线（B3「零双线」）** | `FRAME_INSET: f32 = 12.0` / `FRAME_GAP: f32 = 3.0`（:52-53）；`draw_frame` **2 次** `p.rect_stroke`：外层 `outer = central.shrink(12.0)` + `Stroke::new(INK_W=1.0, shell.border_strong())`、内层 `inner = outer.shrink(3.0)` + `Stroke::new(INK_W*0.5=0.5, shell.border_subtle())`（:184-199）→ **内外双框 = 8 段线**；色令牌 `shell.border_strong()` / `shell.border_subtle()`（暗壳） | `central.shrink(8.0)`；**1 次**闭合矩形 = **4 段** 1px 线；`Stroke::new(1.0, ...GOOGLE_ISOLINE_INK / GOOGLE_LINE_ON_MID... α=0.55)`；`rounding = 0.0`；`FRAME_GAP` 常量**删除**；色令牌 `GOOGLE_LINE_ON_MID`(#9AA0A6@0.55) | `grep`：`FRAME_GAP` / 第 2 个 `rect_stroke` / `shell.border_subtle()` / `INK_W * 0.5` 全部零命中；`shrink(8.0)` 命中；单测：`draw_frame` 产出矩形数 == 1、线段数 == 4、双线数 == 0 |
| 3 | **圆角 4px → 禁 >2px（B4）** | `p.rect_filled(panel, 4.0, shell.float_fill())`（:246）、`p.rect_stroke(panel, 4.0, ...)`（:247-252）→ 面板圆角 **4.0**；`p.rect_filled(sw, 1.0, ...)`（:265）→ 色块圆角 **1.0**；`draw_frame` 两处 `0.0`（:189, :195） | 面板 `rounding = 2.0`；色块 `rounding = 0.0`；图廓 `rounding = 0.0`；罗盘为 `circle_*` 无圆角参数；**全文件 `rounding` 字面量集合 ⊆ {0.0, 2.0}**，最大值 2.0；色令牌 `GOOGLE_CONTROL_BG`(#FFFFFF@0.94) / `GOOGLE_CONTROL_BORDER`(#DADCE0) 取代 `shell.float_fill()` / `shell.border_strong()` | `grep`：字面量 `4.0` 作为 `rect_*` 第 2 参零命中、`1.0` 作为色块圆角零命中；`rect_filled(panel, 2.0, ...)` / `rect_stroke(panel, 2.0, ...)` 命中；单测：`max(rounding) <= 2.0`（B04） |
| 4 | **图例全展开 → 默认折叠** | `draw_legend` 无条件 `for (i, (group, members)) in BIOME_LEGEND_GROUPS.iter().enumerate()`（:253），每行额外 `p.text(BIOME_NAMES_ZH[first], proportional(TEXT_PX - 1.0), shell.t3())`（:273-279）→ **每行 3 列（族名 + 色块 + 首群系名）**、8 行常显、高 `h = 8*16 + 10 = 138px`；`LEGEND_W=148.0` / `LEGEND_ROW_H=16.0` / `LEGEND_EDGE=22.0` / `SWATCH=10.0` / `TEXT_PX=10.0`（:57-61）；**零** `collapsed` / `show_legend` 字段、**零** `ICON_CHEVRON_*`、**零** `ScrollArea`、**零** `Sense::click()` widget | `collapsed: bool` 默认 `true`、`show_legend: bool` 默认 `true`（**两个独立字段**）；折叠态仅 **8 族名 + 8 色块**（2 列，`BIOME_NAMES_ZH` 只在 `collapsed == false` 分支）；`ICON_CHEVRON_RIGHT` / `ICON_CHEVRON_DOWN` 12×12px；`LEGEND_W=168.0` / `LEGEND_ROW_H=20.0` / `LEGEND_EDGE=32.0` / `SWATCH=12.0`；折叠高 `28 + 8*20 = 188px`、展开上限 `340.0` + `egui::ScrollArea`；字号 11/11/10 | `grep`：`ICON_CHEVRON_RIGHT` / `ICON_CHEVRON_DOWN` / `ScrollArea` / `collapsed` / `show_legend` 全部命中；`proportional(TEXT_PX - 1.0)` 零命中；单测：`Default::collapsed == true`、`show_legend == true`、折叠态行数 == 8、`show_legend=false` → 绘制表不含 `Legend` |
| 5 | **取色：`UiShell` 暗壳令牌 → `colors_google` 白卡令牌**（规格 §1.2 明证「不走暗壳色」） | `use crate::ui::shell::UiShell;`（:45）；`let shell = UiShell::new(&shell_cfg);`（:162）；三个 `draw_*` 均带 `shell: &UiShell` 形参；实际用 `shell.t2()` / `shell.t3()` / `shell.float_fill()` / `shell.border_strong()` / `shell.border_subtle()`（:190,196,207,228,246,250,271,278）→ 暗壳 `bg_app @0.96` / `border_subtle #303B43` 系（`status.rs:40-41`）；色值数 **0** 个上游令牌 | 删除 `UiShell` 导入 / `UiShell::new` / 三个 `shell` 形参（或改为不取壳色）；改 `crate::theme::colors_google::GOOGLE_*`（`wv-app/src/theme/mod.rs:34` 已 re-export）+ `crate::theme::palette_to_color32(...)`（`:43`）；**恰好 5 个色值**：`GOOGLE_LABEL_FILL` #3C4043 / `GOOGLE_LINE_ON_MID`·`GOOGLE_ISOLINE_INK` #9AA0A6 / `GOOGLE_LINE_ON_DARK` #5F6368 / `GOOGLE_CONTROL_BORDER`·`GOOGLE_LINE_ON_LIGHT` #DADCE0 / `GOOGLE_CONTROL_BG` #FFFFFF；α 独立常量 `0.92 / 0.94 / 0.70 / 0.55`；依据 `colors_google.rs:223-230`「白卡片不跟随暗色软件壳」 | `grep`：`UiShell` / `shell.` / `UiShellConfig` 在 `map_furniture.rs` **零命中**；`colors_google::GOOGLE_` 与 `palette_to_color32` 命中；单测：五角色 hex 逐通道 `assert_eq!`（见 B01 / §九 P3） |

### 7.1 另 3 条门控缺口（同批替换，逐条可验）

| # | 旧值（骨架） | 新值（规格 §4.2 白名单） | 可验证判据 |
|---|---|---|---|
| M1 | `FurnitureGate` 无 `loading_overlay` 字段；`target_canvas()` 未读 `screen.shows_loading_overlay()`（`app_screen.rs:65`） | `gate` 新增 `loading_overlay = screen.shows_loading_overlay()` 并 AND 进判据；`true` → `draw_plan() == None` | `grep` 命中 `shows_loading_overlay()`；单测表驱动 case `loading_overlay=true → None` |
| M2 | 无 `playing` 字段；未读 `playback.playing` | `gate` 新增 `playing = playback.playing` 并 AND；`true` → `None` | `grep` 命中 `playback.playing`；单测 case `playing=true → None` |
| M3 | 无 `stroking` 字段；未读 `CellPaintSession::is_stroking()`（`cell_paint.rs:295`） | `gate` 新增 `stroking = paint.is_stroking()` 并 AND；`true` → `None` | `grep` 命中 `is_stroking()`；单测 case `stroking=true → None` |

> 注：骨架 `FurnitureGate` 现有 5 字段（`enabled / editor_visible / immersive / mode / branch`，:88-95）与 `target_canvas()` 四项合取（:99-104）**不含**上述三者，也不含 `view.terrain_visible` / `view.field_render` 的显式读取（当前由 `wv_render::cellmap_bridge::field_branch_of(...)` 间接推出 `:141-145`）。规格 §4.2 要求 7 项显式全 AND，制作人复算时须按 W01 / W02 核到「显式字段」而非「branch 推断」。

---

## 八、必须实机看帧的验收项（Y1–Y8，共 8 项）

> **静态核验的边界**：`grep` / 单测只能证明「常量、API、几何数、色令牌名、布尔门控」出现或零命中，**证明不了像素混合后的观感、缩放后的可读性、浮窗遮挡关系、输入命中体感、跨帧残留**。以下 8 项静态一律判不了，必须由用户在桌面看帧；Y1 即主表 B02（唯一一条主核验为实机的规格项），Y2–Y8 是主表标静态的行在像素 / 交互层面的补充验收。

### 8.1 用户桌面操作 · 最短步骤（一次走完 8 项，不重复截图）

1. 启动 World View，打开**任一工程**（须非欢迎页、非沉浸态）；若装饰未出现，按 **`Ctrl+Shift+D`** 翻开开关（重写前骨架 `MapFurnitureState` 缺省 `false`；按规格重写后应默认可见，可跳过此步）。
2. 顶栏点 **Browse**（`VectorFlat`）；确认：地形可见、未在播放、未在笔刷 stroke、无加载遮罩。
3. **整窗截图**存 PNG（系统截图 / `WV_SHOT` / HTTP `screenshot` 均可；注意导出 PNG 路径不含本层，不可用导出图代替）。
4. 把该 PNG **缩到 50%** 再看一遍。
5. 打开右上 `panel_inspector`（默认未开则手动开），看罗盘是否被压 / 重叠。
6. 点图例标题栏折叠钮**展开**，看高度与滚动；再在罗盘 / 图廓 / 图例区域按住左键拖地图、切笔刷涂一笔。
7. 取色器点图廓线内外两点，读 RGB 算 `L = 0.299R+0.587G+0.114B` 与 `|R−B|`。
8. 连续切 **Browse → Map → CellMap → Browse**；再走一次**导出 PNG**，打开导出图核对零装饰。

### 8.2 逐项判定表

| Y | 静态为什么覆盖不了 | 对应规格 / 主表 ID | 看第几步、判什么 |
|---|---|---|---|
| **Y1** | 「50% 缩略图双向可辨」需真实帧缩放 + 人工 / 图像二值化计数；静态只能验到 C02/C10/L04 的常量，不等于通过 | B02（主表唯一实机行）、C02、C10、L04、§八 2 | **第 3+4 步**：50% 图上 (a)「N」+ 实心北瓣仍能辨出北向 = 通过；(b) 数不出 4 根刻度、读不出图例群系文字 = 通过。反向任一成立即失败 |
| **Y2** | α=0.92 / 0.94 常量可 grep，但「保留一点底图透出、不成贴了一张纸」是像素混合观感，静态无判据 | C03（底板 @0.92）、L05（面板 @0.94）、§3.1 底板理由、§5.2-3 | **第 3 步**：白底上能隐约透出群系绿 / 水域蓝 = 通过；读成一块不透明白纸片 = 失败 |
| **Y3** | 规格 §3.1 位置锚点自标「须主程按 `ui-layout.yaml` 复核」、§七末行标「缺失 / 未证实」；`schedules.rs:579` 有 `ui::panel_inspector`，遮挡关系只能看帧 | C01、§3.1 位置锚点、§七末行 | **第 5 步**：罗盘（`central_area` 右上距 24px）与 `panel_inspector` 默认位不重叠、不互相压字 = 通过；被遮挡 / 压住 = 失败（退让规则按规格「罗盘下移」，图例左下不可选） |
| **Y4** | α≤0.55 可 grep，但「叠咸水 `#9DC3D8` / 冷灰绿 `#EEF1EF` 只压出 ≤45 L 极淡边、不改变底色色相归属」需取色器逐点算 | F05、§3.2 透明度、§5.1–5.2 | **第 7 步**：线内外 `ΔL ≤ 45` 且线本身 `S ≤ 0.12 ∧ |R−B| ≤ 12` = 通过；线被读成水岸线 / 行政边界（蓝相加深） = 失败 |
| **Y5** | 面板面积常量可静态算（168×188 / 168×340 / 64×64），但「合计占屏 ≤5%、全幅新增着色像素 ≤2%」+ 三测（剪影 / 灰度 / 形状语言）需看帧或截图差分 | C14、L06、B03、§二 色彩预算与三测、§八 1–2 | **第 3 步**：截图差分统计新增着色像素 ≤2%、三件套合计 ≤5%；去色后「墨 63 → 线 159 → 白底 255」三层明度单调 = 灰度测试通过；地物剪影未变 = 剪影测试通过；全帧只见规整几何一族 = 形状语言检查通过 |
| **Y6** | `rounding=0.0` / 零角饰可 grep，但「直角硬边读成结构边而非画框」是观感判据 | F04、§3.2 四角、B04 | **第 3 步**：四角直角直接相接、无倒角 / L 形加粗 / 角花，整体读作制图结构边 = 通过；读作装饰画框 = 失败 |
| **Y7** | `ScrollArea` / `Sense::click()` 是否出现可 grep，但「展开不占满画布」「折叠钮不吞地图拖拽 / 笔刷」需真实交互 | L06、L08、G06、§3.0 输入、`status.rs:31-34` 提层事故 | **第 6 步**：展开态面板 ≤340px、超出部分滚动、地图仍可见 = 通过；在装饰区域拖拽能平移地图、笔刷能落笔 = 输入未吞通过；拖不动 / 点不到地图 = 失败 |
| **Y8** | 门控布尔可单测，但「切模式不重绘 / 不重写状态」的旧值残留、导出图零装饰、沉浸态退场需看真实帧序列与导出产物 | W03、N08、N09、B05、B06、C13、§八 3 | **第 8 步**：切 Map / CellMap 时罗盘 + 图廓 + 图例 **100% 消失**、回 Browse 无上一帧残留 = 通过；导出 PNG 打开后零装饰 = 通过；转相机后「N」仍朝屏幕正上方（C13） = 通过 |

---

## 九、取色链路：静态验收项 T01（前提已复算解决 · 非待裁 · 不进实机清单）

> **2026-11-13 追记（team-lead 独立复算）**：原派单第 4 问「若主程因分层限制取不到该 crate 令牌、只能在 UI 层定义镜像常量，从美术域看是否可接受」——**前提已被推翻，本项不作裁定、不设前置条件、不列 §八 实机清单**。`crates/wv-app/src/theme/mod.rs:34` `pub use wv_default_theme::{colors, colors_google, palette}`、`:43` `pub fn palette_to_color32(bevy::color::Color) -> egui::Color32`；`colors_google.rs` 规格所用五个令牌全是 `bevy::color::Color` 常量（`GOOGLE_LABEL_FILL=0x3C4043:150`、`GOOGLE_ISOLINE_INK=0x9AA0A6:160`、`GOOGLE_LINE_ON_DARK=0x5F6368:180`、`GOOGLE_LINE_ON_LIGHT:178` / `GOOGLE_GRID_LINE:162` / `GOOGLE_CONTROL_BORDER:226` = `0xDADCE0`、`GOOGLE_CONTROL_BG=0xFFFFFF:224`）。**结论：分层无阻碍，UI 层可直达全部令牌，规格 §1.2「不写裸 hex」可完全满足；不存在「镜像常量」的适用情形，直接转为静态验收项 T01。**

| ID | 规格定值 | 期望实现证据 | 核验方式 |
|---|---|---|---|
| **T01** | 色值走 `colors_google.rs` 既有令牌，不写裸 hex 到 UI；不走 `UiShell` 暗壳色（§1.2 / §七 / B01） | **必须出现** `palette_to_color32(colors_google::GOOGLE_*)` 或全路径 `crate::theme::palette_to_color32(crate::theme::colors_google::GOOGLE_*)` 形式的调用，且覆盖 §0.1 五角色令牌；**禁止出现**裸 hex 常量（`#3C4043` / `0x3C4043` / `Color32::from_rgb(0x3C, 0x40, 0x43)` 等字面量、`const INK_...: Color32 = ...` 之类镜像定义）；**禁止出现** `shell.t2()` / `shell.t3()` / `shell.border_strong()` / `shell.border_subtle()` / `shell.float_fill()` / `UiShell::new(` / `use crate::ui::shell::UiShell` 等暗壳令牌与入口；α（`0.92 / 0.94 / 0.70 / 0.55`）为独立常量，不算裸 hex、不算镜像色值 | grep |

> **归类**：T01 计入「可静态核验 56 项」，**明确不列入 §八「必须实机」清单**（原派单第 4 问作为「待裁边界」的提法作废）。§八 Y1–Y8 逐条复核均为像素混合观感 / 缩略图可读性 / 浮窗遮挡 / 输入命中 / 跨帧残留，与「令牌能否取到」无关，故实机项仍为 8 项、不虚减。

### 9.1 事实核查：分层无阻碍（已亲读，供制作人复核）

| # | 证据 | 结论 |
|---|---|---|
| 1 | `crates/wv-app/src/theme/mod.rs:34` `pub use wv_default_theme::{colors, colors_google, palette};` | `wv-app` 已直接 re-export 整个 `colors_google` 模块 |
| 2 | `crates/wv-app/src/theme/mod.rs:43` `pub fn palette_to_color32(c: bevy::color::Color) -> bevy_egui::egui::Color32` | `Color` → `egui::Color32` 转换器就在 `wv-app` 本层 |
| 3 | `ui/hover_tooltip.rs:137,148,150`、`ui/list_panels_panel.rs:164`、`ui/situation_panel.rs:69`、`ui/inspector/situations.rs:49` | UI 层**现成**用法：`crate::theme::palette_to_color32(crate::theme::colors::ACCENT_EVENT)` |
| 4 | `wv-render/src/drill.rs:1412,1611,1765` | 连 `wv-render` 都在用 `crate::theme::colors_google::GOOGLE_LABEL_FILL` / `GOOGLE_CONTROL_TEXT` |
| 5 | `map_furniture.rs` 同属 `wv-app` crate，已 `use crate::theme::ui_theme::{proportional, UiShellConfig}`（:44） | 取 `crate::theme::colors_google::*` **不需新增依赖、不跨分层、不破坏 ADR-0054**（`wv-world-cell-map` 的「零 theme 依赖」约束管的是那个 crate，不是 `wv-app` 读它） |

→ **因此：不需要镜像常量。** 骨架当前用 `UiShell` 不是「取不到令牌」，而是**占位期随手取了暗壳色**（文件头 `:26-29` 自己写明「色值走 `UiShell` 令牌……最终视觉值以规格为准」、`#:31` 标 `PLACEHOLDER`）。这正是第 5 处方向相反项，按 §七 #5 直接改引用即可。

### 9.2 美术域收口（非裁定，仅说明 T01 为何是硬验收项）

1. **单一出处是基调的命根子**。规格 §1.2 / §七、B01 把「唯一出处 `colors_google.rs`、不写裸 hex 到 UI」钉为硬纪律；镜像常量就是**第二份色表**——一旦开口，后面每个人都能按自己喜好就地改值，基调就废了；风格打架比画得丑更致命，而第二份色表是风格打架最稳的制造机。前提既已证伪，镜像常量**无适用情形，不予讨论、不设「是否可接受」的备选判定、不列前置条件**。
2. **骨架用 `UiShell` 不是「取不到令牌」，是占位期随手取了暗壳色**（文件头 `:26-29`、`:31` `PLACEHOLDER` 自述）。它与 `colors_google.rs:223-230`「白卡片不跟随暗色软件壳」正面对撞，属 §七 #5 方向相反项，按 T01 直接改引用即可，不需要任何降级方案。
3. **α 不是色值**。`0.92 / 0.94 / 0.70 / 0.55` 允许以独立常量出现，但**不得写进任何色表**、不得作为「镜像色值」的替身；族色块仍须运行时直取 `BIOME_RAW_COLORS[group[0]]`（L02），不因 T01 改变。

> **一句话**：分层无阻碍 → 令牌直达 → 不镜像、不裁定；只留 T01 一条 grep 硬验收：**必须见 `palette_to_color32(colors_google::...)`，必须不见裸 hex 常量与暗壳令牌**。

---

## 十、核验对象与接线点对照（`规格项 | 资产文件 | 接线点`）

> 按交付纪律：**缺任一项标「缺失 / 未接线」**。本表只描述核验对象现状，不是已完成清单；截至本清单落盘，主程仍在按 v1.0 重写绘制层。

| 规格项 | 资产文件 | 接线点 |
|---|---|---|
| 五角色色值（§1.2） | `crates/wv-default-theme/src/colors_google.rs:147,150,160,178,179,180,224,226,228`；`crates/wv-app/src/theme/mod.rs:34,43` | **已证实（现成）**：`pub use colors_google` + `palette_to_color32`；UI 层 5+ 处现成用例 |
| 罗盘 / 图廓 / 图例绘制层（§3.1–3.3 全部定值） | `crates/wv-app/src/ui/map_furniture.rs`（重写中） | **缺失 / 未接线**：容器 `egui::Ui` + `LayerId::background()` + `painter()` 已挂 `EguiPrimaryContextPass`（骨架 `:158-180`），但**规格定值的绘制代码未落地**，C01–C14 / F01–F06 / L01–L10 待主程复算 |
| 白名单门控 7 条件（§4.2 / W01 / M1–M3） | 同上 `FurnitureGate` / `map_furniture_system` | **缺失 / 未接线**：`shows_loading_overlay()`（`app_screen.rs:65`）、`playback.playing`、`is_stroking()`（`cell_paint.rs:295`）、`view.terrain_visible`、`view.field_render` 五个显式字段未接入 |
| 图例数据 `BIOME_LEGEND_GROUPS[8]` / `BIOME_NAMES_ZH[21]` / `BIOME_RAW_COLORS[21]` | `crates/wv-world-cell-map/src/terrain_view/biome_raw.rs:106-140` | **已证实（现成）**：只读常量，骨架已有断言 `legend_group_data_is_read_without_mutating_the_frozen_table`（`:427-441`）；不改冻结表 |
| 折叠图标 `ICON_CHEVRON_DOWN` / `ICON_CHEVRON_RIGHT` | `crates/wv-app/src/ui/mod.rs:119-120` | **已证实（现成）**；零新增资产 |
| 字号 / 字重基准 `STATUS_FONT=10.0` / `MICRO(11)` / `proportional()` | `crates/wv-app/src/ui/top_bar/status.rs:10,234-235`；`crates/wv-app/src/theme/ui_theme.rs` | **已证实（现成）** |
| `central_area` 锚点矩形 | `crates/wv-app/src/ui/top_bar/mod.rs:140-150`；`crates/wv-app/src/ui_layout.rs:189,198` | **已证实（现成）**：骨架 `:158` 已在用 |
| `panel_inspector` 默认位 / `ui-layout.yaml` | `crates/wv-app/src/app_bootstrap/schedules.rs:579`；`ui-layout.yaml` / `ui_layout.rs` | **缺失 / 未证实**：罗盘右上是否冲突须实机复核 → §八 Y3 |
| 导出 PNG 零装饰（B06 / N09） | `crates/wv-app/src/ui/export_png.rs`（离屏 `Camera2d`） | **已证实（骨架注释 `:15-17`：egui pass 只挂窗口）**；导出图仍须实机核对 → §八 Y8 |
| 交互命中 / `ScrollArea` / `Sense::click()`（G06 / L08） | `map_furniture.rs`（重写中） | **缺失 / 未接线**：骨架为零 widget 纯 painter，折叠钮不可点，须新增仅面板矩形内 widget |
| 取色链路 T01 | `crates/wv-app/src/theme/mod.rs:34,43`；`crates/wv-default-theme/src/colors_google.rs:150,160,178,180,224,226` | **已证实（现成）**：`pub use colors_google` + `palette_to_color32`；验收 = 必须出现直引、零裸 hex、零暗壳令牌（§九，静态） |
| 纸色旧债回退 X01 | `resources.rs` / `shell.rs`（**本清单未亲读这两处，路径按 team-lead 转述标注**） | **待核验（静态）**：两处 diff 中 `#F2EAD8` 及浮点写法出现次数 = 0，且须为 `ShellPalette::DARK.bg_app` / `cfg.palette().bg_app` **精确恢复原始基线**，非 `#FFFFFF`（§2.1） |
| 本验收清单本身 | `docs/reviews/2026-11-13-map-furniture-acceptance.md`（本文件） | **评审清单 · 未采纳 · 供制作人复算**；零 `.rs` / 零资产 / 零既有 docs 改动 |

---

## 十一、计数汇总（供复算，勿改）

| 类别 | 项数 | 构成 |
|---|---|---|
| **可静态核验** | **56** | C01–C14 (14) + F01–F06 (6) + L01–L10 (10) + G01–G06 (6) + W01–W03 (3) + N01–N09 (9) + B01/B03/B04/B05/B06/B07 (6) + **X01 (1，§2.1 纸色旧债回退)** + **T01 (1，§九 取色链路)** = 56；主核验为 `grep` 或 `单测断言` |
| **必须实机** | **8** | Y1（= B02，主表唯一实机行）+ Y2 白底透出感 + Y3 `panel_inspector` 遮挡 + Y4 ≤45 L 极淡边 + Y5 占屏/三测 + Y6 直角观感 + Y7 `ScrollArea`/输入命中 + Y8 切帧残留/导出/朝向。**取色链路 / 令牌可达性不在本清单内**（已转静态 T01） |
| 验收行总计 | 57 + 7 补充 | 主表 57 行（56 静态 + 1 实机 B02）；§八 Y2–Y8 为 7 条静态覆盖不到的补充实机项 → 实机合计 8 项 |

> **复算结论口径**：制作人逐行跑 `grep` / 单测后，**56 项静态应全绿**；8 项实机须用户按 §8.1 八步走完再判。**任一「不应出现」项命中、或任一「应出现」项零命中，即判该行未符合规格 v1.0**。本清单不改任何实现文件。
