# TV-01 Follow-up — Toolbar Text Corruption 根因验证报告

Date: 2026-11-21
Scope: TV-01 技术验证原型（`WorldView.PresentationPrototype/`），非生产渲染器。
结论状态：**Strongly Indicated**（原型内触发条件已确认；是否为 Avalonia 框架缺陷仍未确认）。

---

## 0. 结论速览（对应交付问题 1–8）

| # | 问题 | 回答 | 依据 |
|---|---|---|---|
| 1 | 手工构造 GlyphRun 是否问题触发条件？ | **是（仅限本原型内）**：不画它即干净，画它就复现 | Experiment A / B；bisect D |
| 2 | TextLayout 生成的 GlyphRun 能否正常绘制？ | **能**，同样走 `DrawGlyphRun`，工具栏保持干净（2/2 次） | Experiment A；`console-exp-a-textlayout-run1/2.png` |
| 3 | 问题在构造阶段还是消费阶段？ | **消费阶段**：只做 shaping + GlyphRun 构造、不绘制 → 干净 | Experiment B；`console-exp-b-construct-only.png` |
| 4 | 能否在独立最小工程复现？ | **不能**（见 §6，已如实记录） | `console-exp-c-*.png` |
| 5 | 移除 `DrawGlyphRunSamples` 是否彻底解决原型问题？ | **是**：Debug / Release 实机截图均干净 | `console-after-fix-*-toolbar.png` |
| 6 | 是否需要向 Avalonia 提交 Issue？ | **暂不提交**：缺少独立复现，先补齐再报 | §6、§11 |
| 7 | 对 Production Renderer Architecture V0.2 是否有影响？ | **无架构性影响**，新增一条实现约束 | §12 |
| 8 | Text & Label Engine 能否继续进入生产架构设计？ | **可以** | §12 |

---

## 1. 问题描述

主原型工具栏前两个按钮（`Load 1K` / `Load 10K`）的文字渲染成多层叠印，混入项目中
本不存在的多语言城市名字符串；按钮 3–8 正常。运行时 bounds dump 证明布局无重叠
（`Load1KButton 104x25@(0,0)`、`Load10KButton @(110,0)`），因此是文本绘制层问题，不是布局问题。

进一步定位：`MapViewport.Render` 中的诊断代码 `DrawGlyphRunSamples` 把
`TextShapingService.Shape` 手工构造的 `GlyphRun`（HarfBuzz shaping 输出 + `new GlyphRun(...)`）
通过 `DrawingContext.DrawGlyphRun` 绘制；移除该调用后工具栏恢复干净。

本报告验证两件事：

1. 触发条件是"手工构造的 GlyphRun"，还是"`DrawGlyphRun` 这一消费方式"；
2. 是否属于 Avalonia 框架缺陷（需要独立最小工程复现才可下此结论）。

---

## 2. 基线版本与 commit

| 项 | 值 |
|---|---|
| 初始 TV-01 基线 | `7b40861`（Avalonia 12 + Skia 技术验证原型） |
| 前置 follow-up 实验 | `c2e2bfa`（Fluent / 软件渲染 / 最小复现工程） |
| 调查起点（main 合并点） | `f9b3697` |
| 上一轮 console 对照 | `dc387b7`（TV-01_FOLLOWUP_FINDINGS.md console 一节） |
| 本轮修复提交 | 本文件所在 commit（`git log` 可见） |
| 分支 | `main` |
| 目标框架 | `net10.0` |
| .NET SDK | 11.0.100 |
| Avalonia | 12.1.6（Skia 3.119.4 / HarfBuzz / Themes.Simple / Fonts.Inter / Desktop） |
| OS | Windows 11 Enterprise build 26300 |
| 会话 | 真实控制台（`query session` = `>console ... Active`，非 RDP） |
| DPI | 1.25 |
| 渲染后端 | `OpenGL`（默认 GPU）/ `CPU raster`（`--software`） |

说明：macOS 仍未验证，本报告只陈述 Windows 结论。

---

## 3. 完整实验表

| # | 条件 | 结果 | 证据 |
|---|---|---|---|
| 1 | SimpleTheme 基线 | 叠印（前 2 按钮） | `test-results/tv01-scene-1k-v7.png` |
| 2 | FluentTheme | 仍叠印 | `test-results/tv01-fluent-toolbar.png` |
| 3 | 移除 Inter 字体 | 仍叠印 | 早期会话记录 |
| 4 | 全局 shaping lock | 仍叠印 | 早期会话记录 |
| 5 | XAML / code-behind Content | 均叠印 | 早期会话记录 |
| 6 | GPU OpenGL / CPU Raster | 均叠印 | `tv01-software-toolbar.png`、`console-software-toolbar.png` |
| 7 | RDP / 真实 Console | 均叠印 | `tv01-realscreen-toolbar3.png`、`console-gpu-toolbar.png` |
| 8 | 最小复现工程（无 Skia、无 DrawGlyphRun） | 干净 | `tv01-repro-harfbuzz.png`、`console-repro.png` |
| 9 | 移除后台 Snapshot 发布（UI 线程同步发布） | 仍叠印 | `console-bisect-a-no-background.png` |
| 10 | 移除 `context.Custom(MapDrawOperation)` | 仍叠印（Skia 排除） | `console-bisect-b-no-customdraw.png` |
| 11 | 不托管 `MapViewport`（仅工具栏） | 干净 | `console-bisect-c-toolbar-only.png` |
| 12 | 移除 `DrawGlyphRunSamples` | 干净 | `console-bisect-d-no-drawglyphrun.png` |
| 13 | 只绘制拉丁标签的 `DrawGlyphRun` | 仍叠印（非跨字体冲突） | `console-bisect-e-latin-only-drawglyphrun.png` |
| 14 | **A1** 手工构造 GlyphRun + DrawGlyphRun（第 1 次） | 叠印 | `console-exp-a-manual-run1.png` |
| 15 | **A1** 手工构造 GlyphRun + DrawGlyphRun（第 2 次） | 叠印 | `console-exp-a-manual-run2.png` |
| 16 | **A2** TextLayout 生成的 GlyphRun + 同一 DrawGlyphRun（第 1 次） | **干净** | `console-exp-a-textlayout-run1.png` |
| 17 | **A2** TextLayout 生成的 GlyphRun + 同一 DrawGlyphRun（第 2 次） | **干净** | `console-exp-a-textlayout-run2.png` |
| 18 | **B1** 仅 shaping + 构造，不调用 DrawGlyphRun | **干净** | `console-exp-b-construct-only.png` |
| 19 | **B0** 完全不构造也不绘制（off） | 干净 | `console-exp-b-off.png` |
| 20 | **C** 独立工程，20 个手工 GlyphRun，立即绘制 | **干净（未复现）** | `console-exp-c-manual-run1/2.png` |
| 21 | **C** 独立工程，TextLayout GlyphRun | 干净 | `console-exp-c-textlayout.png` |
| 22 | **C** 独立工程，构造不绘制 / off | 干净 | `console-exp-c-construct.png`、`console-exp-c-off.png` |
| 23 | **C** 独立工程，延迟 1.5s 后再绘制（模拟工具栏先渲染） | **干净（未复现）** | `console-exp-c-manual-delayed.png` |
| 24 | **C** 独立工程，先跑原型同款 startup shaping suite，再绘制 | **干净（未复现）** | `console-exp-c-suite-manual.png`、`console-exp-c-suite-manual-delayed.png` |
| 25 | **修复后** Debug 实机 | 干净 | `console-after-fix-debug-toolbar.png` |
| 26 | **修复后** Release 实机 | 干净 | `console-after-fix-release-toolbar.png` |

所有截图均在真实控制台会话、DPI 1.25 下用 `CopyFromScreen` 抓取（非离屏合成）。

---

## 4. Experiment A — TextLayout GlyphRun 对照

做法（原型内，其余条件完全不变：Toolbar / MapViewport / 字体 / DPI / 渲染环境）：

- A1：`label.Shaped.GlyphRun`（`TextShapingService.Shape` 手工构造）→ `DrawGlyphRun`；
- A2：用 `new TextLayout(text, typeface, size, brush, ...)` 生成布局，取
  `ShapedTextRun.GlyphRun` → 同一个 `DrawGlyphRun`；
- A0：不调用（对照组）。

结果：

| 变体 | 第 1 次 | 第 2 次 |
|---|---|---|
| A1 手工 GlyphRun | 叠印 | 叠印 |
| A2 TextLayout GlyphRun | 干净 | 干净 |
| A0 不绘制 | 干净 | 干净 |

A2 截图中视口底部标签（含 CJK）正常绘出，工具栏文字正常，说明
**`DrawingContext.DrawGlyphRun` 本身可用**；差异只来自传入的 `GlyphRun` 对象。

---

## 5. Experiment B — 构造阶段 vs 消费阶段

- B0：`off`，不构造新的绘制入口，不绘制 → 干净。
- B1：`construct`，完整执行 HarfBuzz shaping 并构造 `GlyphRun`（与生产路径同一构造代码），
  但不调用 `DrawingContext.DrawGlyphRun` → **干净**。
- B2：`manual`，同一批对象交给 `DrawGlyphRun` → 叠印。

结论：**问题不在 shaping / GlyphRun 构造阶段，而在消费该手工 GlyphRun 的绘制阶段。**

补充：场景构建过程中本来就会为每个标签构造 GlyphRun；B1 说明"构造了但不画"无害，
这与 §3 实验 12（仅移除绘制调用即干净）互相印证。

---

## 6. Experiment C — 独立最小复现工程（结果：未能复现）

工程：`WorldView.PresentationPrototype/repro/ToolbarTextRepro`（独立 Avalonia 12.1.6 项目，
不依赖 World View Domain、不依赖生产 Renderer、无后台线程、无 Snapshot）。

为尽量贴近原型，工程内包含：

- 6 个普通 `Button`；前两个按钮的 `Content` 与原型一致，在 code-behind 中赋值；
- 一个最小自定义 `Control`（`GlyphRunProbe`）在 `Render` 中绘制 GlyphRun；
- 与原型**完全相同**的 20 条标签文本与 Typeface 选择（Segoe UI / Microsoft YaHei UI）；
- 与原型**完全相同**的手工构造（`TextShaper.Current.ShapeText` + `new GlyphRun(...)`）；
- 可选 `--suite`（复刻原型 startup shaping suite）与 `--delay`（先渲染工具栏，1.5s 后
  再开始绘制 GlyphRun，模拟原型首帧无快照、后续帧才绘制的时序）。

模式：`off` / `construct` / `manual` / `textlayout`。

结果：**所有模式、所有组合下，6 个按钮文字均保持干净，未复现叠印**。

| 组合 | 结果 |
|---|---|
| `--mode=manual`（20 标签，立即绘制） | 干净 |
| `--mode=manual`（重复第二次） | 干净 |
| `--mode=manual --delay` | 干净 |
| `--mode=manual --suite` | 干净 |
| `--mode=manual --suite --delay` | 干净 |
| `--mode=textlayout` | 干净 |
| `--mode=off` / `--mode=construct` | 干净 |

按任务要求如实记录：**当前独立最小工程无法复现，因此不能据此宣称 Avalonia Framework 存在缺陷。**
这说明触发还需要原型进程内的某些额外状态，尚未隔离。

---

## 7. 临时修复内容

按"诊断代码不得进入正常渲染路径"的要求：

1. 从 `MapViewport.Render` 中移除 `DrawGlyphRunSamples(context)` 调用；
2. 删除 `DrawGlyphRunSamples` 方法及仅供其使用的 `ToColor` helper；
3. 移除本轮实验用的临时开关（`--glyphrun=` / `MapViewport.GlyphRunMode`）；
4. `ShapedLabel.GlyphRun` 保留：headless 测试仍用它做 `Bounds` 与 layout advance 的度量一致性校验；
5. HarfBuzz shaping、CJK/RTL/mixed shaping 测试全部保留，未删除任何验证逻辑；
6. 未来如需 GlyphRun 可视化诊断，使用 `repro/ToolbarTextRepro`（`--mode=manual|textlayout`），
   不回到生产渲染路径。

未改动：`ShapedLabel` 的 shaping 数据（GlyphInfos / Typeface / advance / baseline）、
`TextShapingService`（唯一 shaping 权威）、Skia 绘制路径。

---

## 8. 修复前后截图

| 状态 | 截图 |
|---|---|
| 修复前（Debug，GPU） | `test-results/console-gpu-toolbar.png` |
| 修复前（软件渲染） | `test-results/console-software-toolbar.png` |
| 修复前（仅构造不绘制，对照） | `test-results/console-exp-b-construct-only.png` |
| 修复前（TextLayout 对照，干净） | `test-results/console-exp-a-textlayout-run1.png` |
| 修复后（Debug） | `test-results/console-after-fix-debug-toolbar.png` |
| 修复后（Release） | `test-results/console-after-fix-release-toolbar.png` |

原始证据截图均保留，未覆盖。

---

## 9. 测试结果

| 命令 | 结果 |
|---|---|
| `dotnet build WorldView.PresentationPrototype.sln -c Debug` | 成功（0 error，3 个既有 warning） |
| `dotnet build WorldView.PresentationPrototype.sln -c Release` | 成功（0 error，3 个既有 warning） |
| `dotnet test WorldView.PresentationPrototype.sln` | **33/33 通过** |
| 真实 Console 实机运行（Debug / Release） | 工具栏文字正常 |

Shaping 回归覆盖（均在 33 项内）：

- `HarfBuzz_Shaper_Is_Active`
- `Latin_Text_Shapes_With_Correct_Advance_Sum`
- `Cjk_Text_Shapes_With_Plausible_Glyph_Count`
- `Mixed_Latin_Cjk_Shapes_All_Characters`
- `Arabic_Rtl_Shapes_Rtl_With_Joined_Forms`
- `Combining_Marks_Produce_Metrics`
- `GlyphRun_Bounds_Agree_With_Layout_Advance`

TextLayout 与手工 GlyphRun 的绘制对照：见 Experiment A（A1 叠印 / A2 干净）。

---

## 10. 根因判断及置信度

**总体状态：Strongly Indicated。**

| 主张 | 置信度 | 依据 |
|---|---|---|
| 原型中，绘制 `TextShapingService` 手工构造的 `GlyphRun` 会破坏同窗口其他文本的渲染 | **Confirmed（原型范围内）** | 移除即干净；A1 两次复现、A2 两次干净；B1 证明构造无害 |
| 问题发生在"消费"阶段（`DrawGlyphRun`），不在"构造"阶段 | **Confirmed** | Experiment B |
| 这是 Avalonia Framework 缺陷 | **Unresolved** | Experiment C 独立工程无法复现 |
| "手工构造的 GlyphRun 违反了 Avalonia 预期的不变量（未经过 TextLayout/内部 shaping 管线的规范化）" | **Strongly Indicated** | A1 vs A2 的唯一差异就是 GlyphRun 来源 |

对差异的说明（诚实标注）：手工构造路径把 HarfBuzz 输出的 `GlyphInfo` 直接塞进
`new GlyphRun(glyphTypeface, size, text.AsMemory(), glyphInfos, baselineOrigin, bidiLevel)`，
绕过了 Avalonia `TextLayout` 的文本运行划分、fallback 分支与字形定位规范化。A2 使用
Avalonia 自己产出的 `GlyphRun` 完全正常，说明 `DrawGlyphRun` API 可用，问题集中在
"这个对象是否以受支持的方式产生"。**但这只是高置信推断，不是已证实的框架缺陷。**

---

## 11. 尚未解决的问题

1. 独立最小工程无法复现：原型进程内还缺少哪一项状态（窗口尺寸、控件组合、
   第二次布局/渲染时序、字体管理器预热方式等）尚未隔离。
2. 手工 `GlyphRun` 具体违反了哪条不变量（GlyphOffset/GlyphAdvance 单位、cluster、
   `Text` 内存生命周期、fallback 分支缺失）尚未确认。
3. 是否需要（以及能否）向 Avalonia 上游提交 Issue：当前**不建议**提交，
   因为缺少独立复现；先按 §6 的 harness 继续缩小，能独立复现后再报。
4. macOS 未验证；本报告所有结论仅限 Windows 11 + Avalonia 12.1.6。

---

## 12. 对 Production Renderer Architecture 的影响

架构边界不变：

- HarfBuzz 仍是唯一文字 shaping 权威；
- Text & Label Engine 仍独立于 Renderer，负责 shaping / 候选位置 / 碰撞避让 / 标签布局；
- Core Domain 仍只提供文本内容与历史事实；
- Renderer 仍只绘制最终结果；
- 未引入第二套 shaping authority；
- 未通过忽略 CJK/RTL 规避问题；
- 字体 fallback 仍保留实际使用的 Typeface 信息；Glyph ID / Typeface / advance / offset /
  baseline 数据不变。

新增一条实现约束（应写入 Renderer Architecture V0.2）：

> **不得为 Avalonia 绘制路径手工构造 `GlyphRun`。** 生产渲染要么走 Skia glyph-ID 路径
> （现有 `SKTextBlob` 方案），要么用 Avalonia `TextLayout` 产出 GlyphRun。
> `TextShapingService.Shape` 的手工 GlyphRun 仅可用于度量校验（headless 测试），
> 不得进入任何 `DrawingContext` 绘制调用。

结论：**Text & Label Engine 可以继续进入生产架构设计**；本次问题不构成架构调整理由，
只增加上述实现约束。

---

## 附：复现/对照工具

```bash
# 原型（修复后：正常渲染，不再绘制诊断 GlyphRun）
cd WorldView.PresentationPrototype
dotnet build WorldView.PresentationPrototype.sln -c Debug
./src/WorldView.PresentationPrototype/bin/Debug/net10.0/WorldView.PresentationPrototype.exe

# 独立对照工程（Experiment C）
cd repro/ToolbarTextRepro
dotnet run -- --mode=manual        # 手工构造 GlyphRun + DrawGlyphRun
dotnet run -- --mode=textlayout    # TextLayout 产出 GlyphRun + DrawGlyphRun
dotnet run -- --mode=construct     # 只构造，不绘制
dotnet run -- --mode=off           # 对照
dotnet run -- --mode=manual --suite            # 追加 startup shaping suite
dotnet run -- --mode=manual --delay            # 延迟 1.5s 再绘制
```
