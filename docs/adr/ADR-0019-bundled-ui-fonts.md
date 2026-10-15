# ADR-0019：界面字体打包——拉丁 Inter + 中文 Noto Sans SC（编译期嵌入）

**状态**：提议（Proposed）——2026-10-16 用户已下达字体指令（「英文字符采用 Inter，中文用 Open Sans，如果没有请你下载」），实现已按本决策落地；待用户确认后转「已接受」
**日期**：2026-10-16
**决策者**：用户（2026-10-16 字体指令）；「中文用 Open Sans」的 CJK 落地口径（Noto Sans SC）由实现侧起草并显式记入本 ADR §2
**关联**：`docs/ART_STYLE_SPEC_v0.1.md` §4.2 硬约束 2（被本 ADR 修正，文内已追记）、ADR-0016（古舆图主题；字体属 LOCKED SHELL，与主题正交）、ADR-0018（i18n；依赖「Latin 主 + CJK 兜底」回退结构，本 ADR 不改结构）、`crates/wv-app/src/theme/fonts.rs`、`crates/wv-app/assets/fonts/`、AGENTS §6.3 / §6.4 / §9 / §1 P1/P2
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；反向修正须新开 ADR。

---

## 1. 处境（Context）

**现状**：`theme/fonts.rs` 只用**系统字体栈**——Windows 探测微软雅黑 / 黑体，
macOS 探测 PingFang，Linux 探测 Noto Sans CJK；**不打包任何字体文件**
（`ART_STYLE_SPEC_v0.1` §4.2 硬约束 2 原文：「必须使用系统字体栈，不引入未授权
字体文件」）。

**两个驱动力**：

1. **设计稿锁定 Inter**。Figma 重做的精确依据是 web 原型
   （`~/Downloads/World-View UI Prototype`），其 `font-family: Inter, ui-sans-serif,
   system-ui, sans-serif`（400–700 四个字重）。Windows 系统中不存在 Inter——
   系统栈下设计稿的拉丁字形**不可能复现**。
2. **中文字形跨平台漂移**。微软雅黑 / PingFang / SimHei 的汉字字形差异明显，
   同一界面在三平台是三张脸。本次实机核查还发现：**微软雅黑缺
   ◈▤◧◉⬡▣✓ 等图标字形**（`state::Panel::icon` 的面板头图标），egui 以替换符
   ◻(U+25FB) 兜底——跨平台「字形可预期」是硬需求。

**用户裁定（2026-10-16）**：英文字符采用 **Inter**，中文用 **Open Sans**，
没有就下载。

**事实约束**：Open Sans **本身无 CJK 字形**（Google Fonts 官方只覆盖拉丁 / 希腊 /
西里尔）。中文字面的「Open Sans」在工程上不存在——取其配套物 **Noto Sans SC**
（Google 的 CJK 字体，拉丁部分与 Open Sans 同源设计），即「中文的 Open Sans」。
本取舍是本 ADR 的核心显式化点，请用户复核。

---

## 2. 决策（Decision）

1. **打包两份字体，编译期嵌入**（`include_bytes!`，与 `ui/widgets.rs` 图标 SVG 的
   `egui::include_image!` 同口径——不依赖运行时工作目录，二进制自带）。
   文件入 `crates/wv-app/assets/fonts/`：
   - **拉丁**：`Inter-Regular.ttf`（Inter 4.x 静态实例，SIL OFL-1.1）；
   - **中文**：`NotoSansSC-Regular.ttf`（Noto Sans SC 2.004，SIL OFL-1.1）。
2. **egui 族装配（逐字形回退）**：
   - `Proportional` = `[Inter, Noto Sans SC, …egui 内置]`——拉丁走 Inter，
     中文（含全角标点）回退 Noto Sans SC；
   - `Monospace` = `[系统等宽, Noto Sans SC, Inter, …内置]`——**数字等宽最优**
     （§4.2 约束 1：年份数字不得跳动），中文注释回退可读。
3. **Bevy `Text2d`（画布地名标注）**：Noto Sans SC **单一字体句柄**
   （`LoadedFonts::cjk_font`）。`TextFont` 只吃一个句柄，parley 逐字形回退顺序
   不受本模块控制——用「拉丁 + CJK 全覆盖」字体避免回退不确定导致的方块。
4. **系统字体探测范围收窄**：CJK 探测**删除**（打包字体覆盖 30 890 码点，
   超过微软雅黑 29 905）；**等宽（数字）仍走系统探测**——等宽是几何约束
   （数字不跳动）而非设计身份，各平台 Consolas / SF Mono / DejaVu 均可满足，
   且中文等宽字重体积巨大，不值得打包。
5. **只用 Regular 一档**。egui 无合成粗体，`.strong()` 只改颜色——注册的全部是
   Regular。设计稿 600 字重（§4.2 `/font.title/` 等）接线需要第二份文件 + 命名族
   + 位点替换，列为**独立后续任务**（文件获取方法已写进 `assets/fonts/README.md`）。
6. **体积口径**：Inter ≈ 1.0 MB + Noto Sans SC ≈ 10.1 MB → 仓库 / 二进制
   约 +11 MB；egui 侧借用编译期字节（零拷贝），Bevy 侧 Noto 一份内存拷贝。

---

## 3. 理由（Rationale）

1. **设计可复现**：Inter 是设计稿字体，打包后任何机器 1:1——这是 Figma
   还原路线的前置条件。
2. **跨平台一致**：中文字形不再随平台漂移；连带把「微软雅黑缺图标字形」这一
   实机问题部分修复（▤◉▣✓ 由 Noto Sans SC 提供，之前是 ◻ 替换符）。
3. **不破 P1 / P2**：字体随二进制走——不写注册表 / AppData，不依赖网络下载，
   单机可运行、目录即工程不受影响。
4. **不破 AGENTS §9（依赖纪律）**：零第三方 Rust 依赖新增（字体属资产，不是 crate
   依赖）；与 YAML round-trip 无关。
5. **与 i18n（ADR-0018）兼容**：ADR-0018 依赖的是「Latin 主 + CJK 兜底」这一
   回退**结构**，打包后结构保持；EN 用户无副作用。

---

## 4. 代价（Consequences）与被否决的备选

### 4.1 代价（必须承认）

1. 仓库与二进制各约 **+11 MB**（Noto Sans SC 单文件 10.1 MB 是大头）。
2. 无字体裁剪链路（无 fontTools）：Noto Sans SC 全量嵌入，未来可做 subset 缩减。
3. **◈(U+25C8) ◧(U+25E7) ⬡(U+2B21)** 三个面板头图标字形在 Inter / Noto Sans SC /
   egui 内置字体中**都**缺失——egui 仍以替换符 ◻ 呈现。**与打包前同表现**
   （微软雅黑同样缺这三个，非回归）；真修复走 SVG 图标化（Figma 原型即 Lucide），
   列为独立任务。
4. 600 字重未接线：设计稿 600 字重（面板标题等）当前渲染为 400（`.strong()`
   只改颜色）。独立任务。

### 4.2 被否决的备选（按推荐度倒序）

| 备选 | 内容 | 否决理由 |
|---|---|---|
| **A. 系统栈 + 分发时安装字体** | 保持现探测逻辑，安装包把字体装进系统字体目录 | 破 P2（写 OS 全局位置）；部署不可控（用户可能拒装）；macOS/Linux 附加步骤更多 |
| **B. 运行时从 `assets/` 目录读** | 不嵌入，启动时 `std::fs::read` 相对路径 | 分发需与目录同步；嵌入与图标 SVG 同口径更稳；且项目已有「编译期嵌入」先例 |
| **C. 只打包 Inter，中文继续系统栈** | 拉丁复现设计稿，中文沿用雅黑/PingFang | 中文字形漂移问题不解决；已实测的图标缺字修复也不会发生；「中英一致的设计观感」半途而废 |
| **D. 拉丁也用 Noto Sans SC** | 只打一份字体，拉丁走 Noto 的拉丁部分 | 违背用户指令（英文字符用 Inter）；拉丁字形与设计稿不符 |
| **E. 拉丁打包真正的 Open Sans** | 把用户口语「中文用 Open Sans」读成「打一份 Open Sans 供拉丁兜底」 | 用户已明确英文字符用 Inter；Open Sans 的 CJK 空缺依旧无解 |

---

## 5. 与既有 ADR / 文档的关系

| 文档 | 关系 |
|---|---|
| `ART_STYLE_SPEC_v0.1` §4.2 硬约束 2 | **被本 ADR 修正**（「必须系统字体栈」→「打包授权字体」）；原结论保留在文内，修正以追记块说明（§6.3 追写不改机制） |
| ADR-0016（古舆图主题） | **不触碰**。字体属 LOCKED SHELL 层，与地图主题（色板 + 技法）正交 |
| ADR-0018（i18n） | 不冲突：其「不变更字体装配」前提针对的是回退结构，本 ADR 保持该结构 |
| ADR-0008 / 0012（地图视觉） | 无关 |

---

## 6. 落地路径（已发生 / 待办）

**已随本 ADR 同批完成**（2026-10-16）：

- 字体文件落 `crates/wv-app/assets/fonts/`（来源 / 许可 / 追加字重方法见其 README）；
- `theme/fonts.rs` 重写：打包字体装配 + 族顺序 + 系统探测收窄至等宽；
- 追记：`ART_STYLE_SPEC` §4.2（本 ADR 引用）、`state.rs` 面板图标注释、
  `ui/project_menu.rs` 面包屑偏离记录、`lib.rs` / `theme/mod.rs` 模块表；
- 门禁：`cargo test --workspace` / `-p wv-app --features app`、
  `cargo clippy`（含 app）、`cargo fmt --check` 全绿。

**待办（不属本 ADR 范围）**：

1. 600 字重接线（第二份文件 + egui 命名族 + 面板标题位点）；
2. ◈◧⬡ 面板头图标 SVG 化（清掉最后的替换符 ◻）；
3. 字重 / 字形视觉验收（与 web 原型逐屏比对）。
