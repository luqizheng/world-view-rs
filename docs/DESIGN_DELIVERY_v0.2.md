# World View 设计交付说明

> **版本** v0.2 设计交付稿（**时间模型重做版**，就地覆盖，不升版本号）
> **编制** 主理人（范可控）汇编，各专业章节均标注来源成员
> **状态** 时间模型章节已按用户 2026-10-11 拍板重做；§2.2 / §2.5 / §3.1 / §5 / §1.5 / §1.7 均为**新稿**
> **权威依据** 用户最终确认的时间模型规格（§0），与本文其他章节冲突处**一律以 §0 为准**
> **待用户拍板** 见 §4

---

## 0. 本轮全景与规格输入

### 0.1 本轮做了什么

用户推翻了 v0.2 首稿的 `TimePoint` 设计（5 元组 + 纪元 + 负年份），给出**新的时间模型规格**，并要求**直接覆盖文档、不升版本**。本轮调度成员 **2 名**（游有方 · 创意总监、柯桥良 · 技术架构总监），主理人只做拆解、调度、独立复核、仲裁与汇编。

| 成员 | 交付物 | 篇幅要点 |
|---|---|---|
| 柯桥良 · 技术架构总监 | 《时间模型重做稿》 | `TimePoint` 单字段 newtype、`Calendar`、`TimeSpan`、`axis.yaml` schema、B 组验收改写、骨架差异 15+11 条 |
| 游有方 · 创意总监 | 《创意裁定：前缀语义与 UI 呈现》 | 前缀双维度裁定、显示规则 S1–S4、18 万格轴导航、前史区方案、V 组修订、C 组冲击 |

### 0.2 规格输入（用户原话形式化，唯一权威）

| # | 用户原话 | 形式化 |
|---|---|---|
| S1 | "系统就是要给计算器，一开始内部记录就是 1 天。然后每天自动+1" | 内部主表示 = **整数天数计数器**，第 1 天 = 游戏第一日 |
| S2 | "作者设置启动需要设置 前序 如 帝国历，无仙，后面写一个数值 如 1000，20，10" | `axis.yaml` 有 `prefix: String` + `start_year: i32`（**只能填一个数值**） |
| S3 | "一年360天，固定每月30天。不做处理" | 360 天/年、30 天/月、12 月/年、**无闰年、无例外** |
| S4 | "无仙+20，那么现实就是 无仙20年1月1号。就是游戏第一日的开始" | 第 1 天 ≡ `{prefix} {start_year} 年 1 月 1 日` |
| S5 | "所有运算都基于这个年开始算起" | 一切换算以 `start_year` 为锚 |
| S6 | "系统日子计算是2，那么显示就是 无仙1月2日" | 天数计数器 → 年月日 = 纯加法除法换算（口语省略年，见 §4.1 N2） |
| S7 | 精度追问 | **天数为主，年月日是换算显示** |
| S8 | 起点追问 | **只有一个起点，往前不存在** |
| S9 | 时间轴追问 | **单一时间轴，前缀只是命名** |

**换算公式**（令 `offset = counter - 1`）：

```text
正算（counter → 年月日）:
    year  = start_year + offset / 360
    month = (offset % 360) / 30 + 1
    day   = (offset % 30)  + 1

反算（年月日 → counter）:
    counter = (year - start_year) * 360 + (month - 1) * 30 + day
```

**关键推论（必须内化）**：`year` / `month` / `day` 全部由 `counter` + `start_year` **唯一决定**，**它们不是独立自由度**。因此它们**不能**作为存盘字段、也不能作为 DTO 字段——否则会造出"counter 说 10、YAML 说 12"的二义态。这是本轮全部技术裁定的共同根因。

### 0.3 独立复核（主理人执行，非成员自述）

主理人用独立脚本（非成员代码）对**全部换算数字**逐项复算：

| 复核项 | 结果 |
|---|---|
| 沈砚生辰 `280/1/15` → 93615 | ✅ |
| 建城 `300/1/1` → 100801 | ✅ |
| 事件 `to 300/1/10` → 100810 | ✅ |
| span 切换 `305/1/1` → 102601 / `305/1/2` → 102602 | ✅ |
| 轴右端 `520/12/30` → 180360 | ✅（**初稿误作 180030，已拦截修正，见 §3.2**） |
| `1..200000` 天全覆盖恒等式 | ✅ **零反例** |
| 设计稿算例「第 75 天」→ 20年3月15日 / 「无仙22年4月12日」→ 第 822 天 | ✅ |

---

## 1. 设计篇（来源：游有方 · 创意总监）

### 1.1 核心立场

> **"把地理、时间、人物、事件放进同一个工程，让它们互相校验——你再也不会写出'三天从北境到南疆'。"**

**体验北极星**：从"作者意识到矛盾"到"作者完成修改"的**路径长度**。任何让作者需要离开工具、打开另一个文件、手算时间的设计，都是体验失败。

### 1.2 核心创作循环（螺旋上升，非线性）

```
建工程 → 建世界 → 圈地理 → 立时间 → 摆事件 → 看校验 → 改设定 → 回到任一环
```

**硬性体验要求**：允许**任意环节进入、任意环节返回**，未完成维度**不阻塞**其他维度（允许"空地图上先摆事件"）。

### 1.3 四维校验的体验设计

| 状态 | 颜色 | 形状/线型 | 面板文案要点 |
|---|---|---|---|
| Reachable 可达 | 绿 | 实心圆点 / 实线 | 到达时间 + 耗时（人话："耗时 1 天 6 小时 20 分"） |
| ArrivesLate 迟到 | 黄 | 三角警告 / 虚线 + 迟到徽标 | **必须反算"最早出发时间"**，让作者一次点击即可采纳 |
| Unreachable 不可达 | 红 | 叉 / 断裂线 | 给出具体缺失原因（缺哪个工具、哪个前置条件） |
| **Unknown 信息不足**（第 4 态） | 灰 | 问号 / 点线 | 「未声明路径，无法计算」 |

**无障碍硬要求**：三态不能仅靠色相区分，必须叠加形状与线型差异（色盲友好）。

### 1.4 对 §5.4「移动由事件声明」的立场

> **强烈支持。** 它是"作者主权"哲学的核心，且实质**减轻**作者心理负担。

净负担变轻的理由：作者在实践中**本来就必须知道角色走哪条路**（这是小说作者的职业本能）。工具只是把"本来就在脑中的信息"落到纸面。真正的负担来自**声明动作的摩擦**——必须用三条体验要求兜住：边推荐列表、复制上一条、**未声明 ≠ 不可达**。

**唯一翻车点**：若不区分"不可达"与"未声明"，作者会困惑"我明明说了从 A 到 B，为什么说不可达？"

### 1.5 跨尺度时间轴（头号交互痛点，**已按新时间模型重评**）

**跨度比 ≈ 2.6 亿 : 1**（500 年 vs 1 分钟），单条线性轨道不可能同时表达。

**新模型改变了什么**：

| 旧模型（v0.2 首稿） | 新模型（本轮） |
|---|---|
| 多纪元，`era` 参与排序 | **单轴，无纪元** |
| 纪元切换 = 复杂换算 | **纪元切换概念消失** |
| 5 元组字典序排序 | **单键天数计数器升序** |
| 可前溯（负年份） | **只有起点，往前不存在** |

**净效果**：原「跨尺度 + 跨纪元」**双难点**，只剩「跨尺度」**单难点**。但该单难点**尺度极大且无天然切分点**——旧模型靠「纪元」天然切分，新模型连这个都没了。

**五方案逐条重评（修订后优先级）**：

| 原方案 | 优先级变化 | 裁定 | 理由 |
|---|---|---|---|
| ① minimap 全局条 | 不变（**必做**） | ✅ 保留 | 18 万格轴比旧模型**更需要**全局条 |
| ② 时间书签/锚点 | **上升** | ✅ **升级为必做** | 新模型下是唯一可用的粗粒度定位 |
| ③ 语义跳转 | **上升** | ✅ **升级为必做** | 补上「纪元没了」留下的定位空缺 |
| ④ 缩放范围记忆 | **可删** | 🟡 降级为「若免费则做」 | 单轴下作者不会「忘记自己在哪个纪元」 |
| ⑤ 双轨分屏 | **转待裁** | 🟡 转待裁 | 双轨原动机是「对比两纪元」，纪元没了唯一强用例消失 |

**新增痛点（新模型引入的唯一新痛点）：18 万格连续轴怎么导航？**

算账：`500 年 × 360 天 = 180,000 天`。1 天 = 1 像素 → 1200px 屏需 **150 倍横向滚动**。

**裁定方案（三层导航，替代原「下钻/缩放」二分法）**：

| 层 | 手段 | 适用尺度 | 解决什么 |
|---|---|---|---|
| L1 全局条 | minimap + 滑动窗口 | 1 年 ~ 500 年 | 一眼看全貌 |
| L2 语义锚点 | 书签 + 关键事件标记 + 「跳到下一事件」 | 跨任意尺度 | **对抗 18 万格的唯一实用手段**：不靠滚动，靠「跳到下一个有意思的点」 |
| L3 年月日跳转 | 输入框「跳到 → 40年3月」+ 微调箭头 | 1 天 ~ 1 月 | 精确定位，绕开连续轴 |

**关键设计判断（保留并强化）**：**"下钻"与"缩放"不该是同一件事**。新模型下下钻对象变了（旧模型下钻 = 换纪元；新模型下钻 = 从年到月到日的 LOD），但**编辑语境（城市 vs 区域）的时间轴下钻仍必须独立于时间轴缩放**——此判断不受模型变更影响。

### 1.6 地形双模式（调参数 vs 画线条）

> **作者应该是"先调参数得到 80 分，再用画笔修补最后 20 分"。**

**最高优先级体验要求**：**改参数不丢覆盖**。若调海平面会清空手改的海岸线，作者会**不敢调参数**，生成模式的价值归零。

### 1.7 阶段一设计验收（V1–V14，**已按新时间模型修订**）

| 编号 | 必须已有答案的设计问题 | 处置 |
|---|---|---|
| V1 | 「未声明路径」与「不可达」是两种状态（→ 第 4 态） | ✅ 保留不变 |
| V2 | 「迟到」能否反算最早/最晚出发时间 | ✅ 保留不变 |
| V6 | 地形/海岸线是静态还是时间区间化 | 🟡 简化：单轴下校验语义更简单，结论「静态 + 预留全区间」不变 |
| V7 | 时间区间开闭语义（→ 半开 `[from, to)`） | 🟡 语义微调：端点类型从 TimePoint → **天数计数器单值** |
| V9 | 地点边的开通/关闭是否随时间变化 | 🟡 **降级**至阶段五+（新模型下作者最关心「事件能否接上」，边时变是长尾需求） |
| **V11**（新） | 前缀为空时如何显示 | ✅ 新增，阶段一必答 → 显示「第 N 天」，不显示年月日（规则 S2） |
| **V12**（新） | `start_year` 是否允许 0 / 负数 | ✅ 新增，阶段一必答 → **允许任意 `i32`**，`counter` 恒 ≥ 1（前史方案 A 的支撑点） |
| **V13**（新） | 18 万格连续轴最小导航方案 | 🟡 新增，阶段五必答 → 三层导航（见 §1.5） |
| **V14**（新） | 前史区是否存在 | 🟡 新增，**转待裁** → 推荐方案 B（见下文） |

### 1.8 「前缀」的创作语义（**新增章节**）

**作者写「无仙」时想的是什么？**
→ 他在给时间**命名**，不是**分段**。写「帝国历」时同理。在创作心理上，「无仙」（创世界名）与「帝国历」（王朝名）**是同一层级的东西**，区别只是世界观味道；**在引擎里，二者区别为零**。

**核心裁定：前缀 = 可承载世界观语义的第一个锚点，但运算上是纯标签。**

| 维度 | 前缀地位 | 说明 |
|---|---|---|
| **语义维度**（作者所见） | **重** | 世界观的第一个锚点，建立「时间质感」的入口 |
| **运算维度**（引擎所见） | **轻** | 纯字符串，不参与换算/排序/区间求值 |

> 大白话：前缀不是历法系统，是你给时间**起的名字**。像给主角取名——名字很重要，但不改身高体重。以后改叫「太初历」，所有事件时间点一个都不动，只是标签变了。

**前缀硬性契约**：工程级唯一（一工程一个 `prefix`）；自由字符串（建议 ≤16 字符）；**不参与运算**；可改且不触碰任何计数器；默认空 → 显示「第 N 天」兜底；与 `start_year` **平级独立**（`prefix` 管「叫什么」，`start_year` 管「从哪年数」）。

**「多层纪年」（创世 → 帝国 → 联邦）由事件文案消化，工具不提供多前缀。** 这是主动砍范围（见 K1）：多前缀 = 多起点 = 多条轴 = 用户刚亲手砍掉的复杂度又长回来。

### 1.9 「起点不可前溯」的体验评估（**新增章节**）

**结论：是解放，但工具必须兜住，否则变创作事故。**

**为什么是解放**：旧模型「可前溯」看似给自由，实际给三种心理负担——① 每次写时间要先问正负；② 负年份 + 多纪元 → 排序规则复杂 → 作者不敢信任工具排序；③ 「纪元前」要求先定义纪元，但作者可能只想写个事件。新模型下所有时间正、递增、可心算，**作者对工具排序的信任度大幅提升**。

**关键问题：作者想写「建国前 300 年的传说」怎么办？→ 工具层正面解法**

- **方案 A（首选，成本 ≈ 0）：`start_year` 前移**
  作者不需负年份，只需把 `start_year` 设小。想写「建国前 300 年」→ 把 `start_year` 从 20 改成 -280（或 0 / 1）→ 第 1 天 = 无仙 -280 年 1 月 1 日 → 「建国前 300 年」≈ 第 21 天（在轴的**开头**）。
  代价：作者要「往上顶一格」重新理解起点。收益：**轴永远正、递增、一条线**。
  → 这解释了**为何 `start_year` 允许任意 `i32`，而 `counter` 恒 ≥ 1**（对应 V12）。

- **方案 B（推荐与 A 组合）：前史区**
  起点**左侧**留一条视觉不同的「前史区」（虚线/灰色/不可排期，仅承载叙事文本）：

```
┌─ 前史区（虚线 / 灰色 / 不可排期，仅承载叙事）───────────────┐
│  🌌 创世 · 上古神话 · 失落文明     ┊  正常时间轴（实线）      │
│  （自由文本，无 day_counter）      ┊  无仙20年1月1日 = 第1天  │
└────────────────────────────────────┊──────────────────────────┘
                                     ↑ 起点锚点（不可左移）
```

  规则：只承载叙事文本、不参与四维校验、不可排期（条目是纯文本卡不是时间点）、视觉明确区分避免误拖。

**为什么组合是对的**：A 让「可排期时间」永远正递增（保持模型纯洁）；B 让「不可排期前史」有个家（创作欲不被扼杀）。边界清晰——**能算的进主轴，不能算的进前史区**。

**与用户原话一致性**：✅「只有一个起点」= 主轴只有第 1 天不可左移；✅「往前不存在」= **可排期事件**往前不存在，**叙事文本**由前史区承载；✅「所有运算基于这个年开始算起」= 方案 A 的 `start_year` 正是这个「年」。**零冲突，且补上用户没说的那块（前史）。**

### 1.10 UI 呈现规则（**新增章节**）

**核心裁定：以年月日为主显，天数计数器为次显。** 用户原话「系统日子计算是 2，那么显示就是无仙1月2日」是**硬约束**。

| 显示位置 | 主显 | 次显 |
|---|---|---|
| 时间轴刻度 | `无仙 20 年 1 月 1 日`（可缩写 `20年1月1日`） | 无 |
| 事件卡 | `无仙 20 年 3 月 15 日` | 悬停显示 `第 75 天` |
| Inspector / 属性面板 | `20 年 3 月 15 日` | 小字 `内部序号：第 75 天`（永不占主位） |
| 校验输出 | `无仙 20 年 1 月 3 日` | 耗时 `1 天 6 小时 20 分` |
| 开发者/诊断模式 | `#75` | 完整年月日（普通作者不可见） |

**显示规则（可执行规约）**：

- **S1 有前缀**：`<prefix> <Y> 年 <M> 月 <D> 日`
- **S2 前缀为空**：显示 `第 <counter> 天`，**不显示年月日**——无前缀时年月日是凭空冒出的数字，反而困惑。**唯一允许天数做唯一显示的场景**。
- **S3 紧凑语境自适应缩写**：`20年1月1日` → `20/1/1` → `20年`（LOD，不改变主显语义）
- **S4 每日 +1 进位**（对应用户原话，作者唯一能心算验证的东西）：

```
第 1 天   → 无仙 20 年 1 月 1 日
第 2 天   → 无仙 20 年 1 月 2 日
第 30 天  → 无仙 20 年 1 月 30 日
第 31 天  → 无仙 20 年 2 月 1 日    ← 月进位
第 360 天 → 无仙 20 年 12 月 30 日
第 361 天 → 无仙 21 年 1 月 1 日    ← 年进位
```

### 1.11 风险与可砍项（设计侧）

**风险（按严重度）**：

| 严重度 | 风险 | 兜底 |
|---|---|---|
| 🔴 高 | 作者把「前缀」误解为可随时切换的历法 | 文档 + tooltip 明说「前缀是命名不是历法切换」；多纪年靠事件文案 |
| 🔴 高 | 作者想写前史却无处安放 | 方案 A（调 `start_year`）+ 方案 B（前史区） |
| 🟡 中 | 18 万格轴定位困难，滚动到崩溃 | 三层导航 + 年月日跳转；语义跳转升为必做 |
| 🟡 中 | 作者心算不出「第 361 天 = 21 年 1 月 1 日」 | UI 提供「天数 ⇄ 年月日」双向输入，不让心算（K2） |
| 🟢 低 | 前缀过长破坏时间轴排版 | 缩写 LOD + 悬停看全称 |

**可砍项（一人开发，范围优先）**：

| # | 可砍项 | 代价 | 何时砍 |
|---|---|---|---|
| K1 | 多前缀/多纪年系统 | 无法用工具表达换纪年，靠文案代偿 | **现在就砍**（用户已明确单轴单前缀） |
| K2 | 天数 ⇄ 年月日双向换算输入框 | 作者需心算，概率低 | 阶段一先做年月日输入，天数输入后补 |
| K3 | 双轨分屏 | 无法左右对比两远点 | **转待裁** |
| K4 | 缩放范围记忆 | 切走再切回丢失缩放层级 | 阶段五排期紧则优先砍 |
| K5 | 前史区（方案 B） | 退化为方案 A 单独成立 | **转待裁** |

---

## 2. 架构篇（来源：柯桥良 · 技术架构总监）

### 2.1 crate 分层与依赖方向

```
wv-app（应用层，Bevy）
   ↓
wv-edit ─────→ wv-io
   ↓             ↓
   └──→ wv-core ←┘        wv-terrain（与 wv-core 平行，互不依赖）
```

**依赖矩阵（行依赖列）**：

| ↓依赖→ | wv-core | wv-terrain | wv-io | wv-edit | wv-app |
|---|---|---|---|---|---|
| wv-core | — | ✗ | ✗ | ✗ | ✗ |
| wv-terrain | **✗** | — | ✗ | ✗ | ✗ |
| wv-io | ✓ | ✓ | — | ✗ | ✗ |
| wv-edit | ✓ | ✗ | ✓ | — | ✗ |
| wv-app | ✓ | ✓ | ✓ | ✓ | — |

**关键裁决**：**wv-terrain 不依赖 wv-core**。理由：地形生成是纯几何/信号处理，输入数值参数、输出几何数据，不需要引用领域模型。若未来需共享类型，**抽 `wv-types` 而非反向依赖**。

**新增不变式**：**Place 独立于 Map**。Place 归属 `distances/places.yaml`，MapMarker 单向引用 PlaceId，Place 不知道 Map 存在。

### 2.2 TimePoint 与历法（**重做，替换 v0.2 首稿**）

> **本节为就地覆盖**。旧稿的 5 元组 `{era, year, day, hour, minute}` + 缓存 + 纪元 + 负年份**全部作废**。

#### 2.2.1 裁定总表

| # | 议题 | 旧稿 | **新稿裁定** | 理由 |
|---|---|---|---|---|
| **R1** | `TimePoint` 主表示 | 5 元组 + `total_minutes` 缓存 | **newtype `pub struct TimePoint(i64)`** | S1「内部记录就是 1 天」；newtype 使 `Ord` 天然按天数、杜绝多字段打架 |
| **R2** | 「纪元」实体 | `era: i16` + `Vec<Era>` + `order` | **全删** | S9「单一时间轴，前缀只是命名」 |
| **R3** | 负年份 / 纪元前 | 刚需，`i32` 支持负数年 | **不存在**，`counter >= 1` 硬约束 | S8「只有一个起点，往前不存在」 |
| **R4** | 时分精度 | `hour: u8` / `minute: u8` 必填 | **阶段一删除**，刻度 = 1 天 | S1/S7；保留会引入"不足一天如何进位"的未定义行为 |
| **R5** | 排序语义 | `(era,year,day,hour,minute)` 字典序 + "不依赖 total_minutes" | **天数计数器数值序**（`derive Ord`）；旧约束**整条作废** | R1 的直接后果；旧约束前提（多字段/缓存）已消失 |
| **R6** | 历法常量 | `days_per_year` / `months` 可配 | **`const` 常量** | S3「不做处理」；可配置 = 伪需求 |
| **R7** | 时间轴范围 | `TimeSpan{from:TimePoint, to:Option<TimePoint>}` | `DayRange{first_day, last_day: Option<i64>}` | 纯整数下套 `TimePoint` 是纯噪音 |
| **R8** | 起步语义 | 未提 | `counter = 1` 是**逻辑起点**，`start_year` 只影响**显示** | S4/S6；**推论：改 `start_year` 不需迁移任何数据** |

#### 2.2.2 为什么 `TimePoint` 退化为单字段 newtype

| # | 理由 | 说明 |
|---|---|---|
| 1 | **规格强制** | "内部记录就是 1 天"——缓存 `year/month/day` 是冗余存储 |
| 2 | **不变式免维护** | 多字段结构有 N 个字段就有 N-1 个可能不一致；newtype 令不一致**无法表达** |
| 3 | **`Ord` 天然正确** | `#[derive(Ord)]` 直接按 `i64` 比较，无需任何"不得依赖缓存"的口头约束 |
| 4 | **换算成本可忽略** | 一次除法 + 两次取模，纳秒级；**没有优化必要** |

> **反方意见（记录在案，判定不成立）**：主张"缓存避免重复换算"。反驳：① 换算成本如上一行；② 缓存是**过早优化**；③ 若要缓存，应在**展示层**缓存字符串（那个才贵），而非在领域模型缓存数值。

#### 2.2.3 最终 Rust 定义（`crates/wv-core/src/time.rs`）

> **验证状态**：以下代码已在 `rustc 1.97.1 / edition 2021` 下**实际编译通过**，
> `cargo clippy --all-targets -- -D warnings` **零告警**，`cargo fmt --check` **干净**，
> 附带 **26 项单元测试全绿**（见 §2.5.3）。

```rust
//! 时间维度：单一时间轴、天数计数器、无纪元、无闰年。
//!
//! 核心规格（用户确认，唯一权威）：
//! - 内部主表示 = **整数天数计数器**，第 1 天 = 游戏第一日；每天自动 +1。
//! - 历法固定：360 天/年、30 天/月、12 月/年，**无闰年、无例外**。
//! - 只有**一个起点**，往前不存在（无负数计数器）。
//! - 前缀（如 `无仙` / `帝国历`）只是**起点的命名标签**，不是可切换纪元。

use crate::error::CoreError;

/// 每月的天数。规格固定：S3「固定每月30天。不做处理」。
pub const DAYS_PER_MONTH: i64 = 30;
/// 每年的月数。
pub const MONTHS_PER_YEAR: i64 = 12;
/// 每年的天数。规格固定：S3「一年360天」。
pub const DAYS_PER_YEAR: i64 = 360;

/// 时间轴上的一个位置。
///
/// **主表示是天数计数器**（`counter >= 1`），年月日是按 `Calendar` 换算出的**展示**。
/// 之所以用 newtype 而非 struct：年月日不是独立自由度，缓存它们只会制造
/// "两个字段打架"的可能，而 newtype 使这种不一致**无法表达**。
///
/// 排序语义：`Ord` 按 `counter` 数值——这**就是**时间先后，无需其他规则。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct TimePoint(i64);

impl TimePoint {
    /// 游戏第一日。等价于 `Calendar::day_counter(calendar.start_year, 1, 1)`。
    pub const FIRST: TimePoint = TimePoint(1);

    /// 由天数计数器构造。`counter` 必须 `>= 1`。
    ///
    /// 之所以把下界检查放在构造函数而非 `date()`：`TimePoint` 一经构造即须合法，
    /// 否则 `Ord` 与后续全部换算都会在下游才炸，定位成本高。
    pub fn from_counter(counter: i64) -> Result<Self, CoreError> {
        if counter < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter });
        }
        Ok(TimePoint(counter))
    }

    /// 由计数器直接构造，不经校验。仅供**已校验来源**（YAML 加载后、内部累加）
    /// 与测试使用；公开 API 优先用 `from_counter`。
    pub const fn from_counter_unchecked(counter: i64) -> Self {
        TimePoint(counter)
    }

    /// 天数计数器原值。
    pub const fn counter(self) -> i64 {
        self.0
    }

    /// 推进 `days` 天。用于 S1「每天自动+1」的推进器与编辑器。
    ///
    /// 越过下界（< 1）报错：规格 S8 明确"往前不存在"，
    /// 故不允许通过减法把时间点推到第一日之前。
    pub fn advance(self, days: i64) -> Result<Self, CoreError> {
        Self::from_counter(self.0 + days)
    }

    /// 换算为年月日（展示用）。
    ///
    /// 注意：**不**在 `TimePoint` 上直接暴露 `year()` / `month()` / `day()`，
    /// 因为换算依赖 `Calendar.start_year`；时间点自己不知道起点。
    /// 这个边界是刻意的：`TimePoint` 是**绝对量**，`Calendar` 是**坐标系**。
    pub fn date(self, calendar: &Calendar) -> CivilDate {
        calendar.civil(self.0)
    }
}

impl std::fmt::Display for TimePoint {
    /// Debug 展示（无起点信息时）。带作者前缀的展示请走 [`Calendar::format`]。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@day{}", self.0)
    }
}
```

#### 2.2.4 `Calendar` 与 `CivilDate`

```rust
/// 历法坐标系。对应 `timeline/axis.yaml` 的 `prefix` + `start_year`。
///
/// 刻意**不**提供 `days_per_year` 之类的字段：规格 S3 说"不做处理"，
/// 做成可配置就是伪需求。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Calendar {
    /// 起点的命名标签，如 `无仙` / `帝国历` / `开元`。
    /// 只是名字——不是纪元实体，不参与任何运算（规格 S9）。
    pub prefix: String,
    /// 起点年份。第 1 天 ≡ `{prefix} {start_year} 年 1 月 1 日`（规格 S4）。
    pub start_year: i32,
}

impl Calendar {
    pub fn new(prefix: impl Into<String>, start_year: i32) -> Self {
        Self {
            prefix: prefix.into(),
            start_year,
        }
    }

    /// 天数计数器 → 年月日。`offset = counter - 1`。
    ///
    /// 纯整数运算，无浮点、无舍入误差、无闰年分支。
    pub fn civil(&self, counter: i64) -> CivilDate {
        let offset = counter - 1;
        CivilDate {
            year: self.start_year as i64 + offset / DAYS_PER_YEAR,
            month: (offset % DAYS_PER_YEAR) / DAYS_PER_MONTH + 1,
            day: (offset % DAYS_PER_MONTH) + 1,
        }
    }

    /// 年月日 → 天数计数器。
    ///
    /// 错误条件（全部硬校验）：
    /// - `year  < start_year`：起点之前不存在（规格 S8）
    /// - `month ∉ 1..=12`
    /// - `day   ∉ 1..=30`
    pub fn day_counter(&self, year: i64, month: i64, day: i64) -> Result<i64, CoreError> {
        if year < self.start_year as i64 {
            return Err(CoreError::BeforeTimelineStart {
                year,
                start_year: self.start_year,
            });
        }
        if !(1..=MONTHS_PER_YEAR).contains(&month) {
            return Err(CoreError::InvalidMonth { month });
        }
        if !(1..=DAYS_PER_MONTH).contains(&day) {
            return Err(CoreError::InvalidDay { day });
        }
        Ok((year - self.start_year as i64) * DAYS_PER_YEAR + (month - 1) * DAYS_PER_MONTH + day)
    }

    /// 展示文本。`with_year = false` 时省略年份（对应 S6 的口语句式）。
    pub fn format(&self, point: TimePoint, with_year: bool) -> String {
        let d = self.civil(point.counter());
        if with_year {
            format!("{} {} 年 {} 月 {} 日", self.prefix, d.year, d.month, d.day)
        } else {
            format!("{} {} 月 {} 日", self.prefix, d.month, d.day)
        }
    }
}

/// 换算后的年月日。**只用于展示/解析**，不得作为存盘字段（见 §0.2 关键推论）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CivilDate {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl CivilDate {
    /// 便利：解析后一步到天数。
    pub fn to_counter(self, calendar: &Calendar) -> Result<i64, CoreError> {
        calendar.day_counter(self.year, self.month, self.day)
    }
}
```

#### 2.2.5 排序语义的明确废弃声明

| 项 | 旧稿 | **新稿** |
|---|---|---|
| 排序定义 | `(era, year, day, hour, minute)` **字典序** | **天数计数器数值序** |
| 实现方式 | 手写多字段比较 / 派生 `Ord` 于多字段 struct | `#[derive(Ord)]` 于 `TimePoint(i64)` |
| 附加约束 | "排序恒按字典序，**不依赖** `total_minutes`" | **该约束整条作废**——不复存在 `total_minutes` 缓存 |
| 跨纪元排序 | 需 `era` 参与比较 | `era` 已删除；`prefix` 是名字，**不参与**排序 |
| 前缀切换排序 | （旧稿未定义） | S9 单一时间轴 ⇒ **前缀不产生"跨前缀比较"问题** |

> **一句话**：旧稿"字典序 + 不依赖缓存"是一条**为了绕开缓存不一致而设的口头约束**；新稿删掉了缓存与多字段，那条约束连同它的风险一起消失。

### 2.3 `axis.yaml` schema 与扁平化落地

#### 2.3.1 Rust 侧结构

```rust
/// 时间轴定义。对应 `timeline/axis.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Timeline {
    /// 历法坐标系：起点名 + 起点年。
    #[serde(flatten)]
    pub calendar: Calendar,
    /// 工程关心的天数范围。字段名刻意用 `day` 而非 `from/to`，
    /// 以防被误读为"日期"——这里全是**天数计数器**。
    pub timeline_range: DayRange,
    /// 关键时间点，用于 UI 跳转。
    #[serde(default)]
    pub anchors: Vec<Anchor>,
}

/// 天数范围。`last_day: None` 表示"开放至今"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DayRange {
    /// 第 1 天 = 游戏第一日。
    pub first_day: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_day: Option<i64>,
}

/// 时间轴锚点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    pub id: crate::id::Id,
    pub name: String,
    /// **天数计数器**，非 `{year,month,day}`。
    pub day: i64,
}
```

> **`serde(flatten)` 的取舍说明**：`flatten` 会关掉该结构的"未知字段即报错"能力（反序列化宽容度上升）。若希望**严格拒绝**拼错的键（如 `startyears`），则改为显式嵌套 `calendar:`。**当前采用 flatten**：面向单人作者，扁平可读优先；拼错键的代价只是"没生效"而非崩坏，且 `start_year` 无 `#[serde(default)]`，缺失即 error 兜住。

#### 2.3.2 新 `axis.yaml` 示例（替换 `examples/minimal-world/timeline/axis.yaml`）

```yaml
# 时间轴定义。
# 内部主表示是"天数计数器"：第 1 天 = 游戏第一日。
# 年月日是换算显示：第 1 天 ≡ 无仙 20 年 1 月 1 日。
# 历法固定：360 天/年、30 天/月，无闰年。

prefix: 无仙            # 起点的命名标签（不是纪元实体）
start_year: 20          # 只能填一个数值，规格 S2

timeline_range:
  first_day: 1          # 必须为 1
  last_day: 180360      # = 无仙 520 年 12 月 30 日（恰好满 500 年：520-20=500）；开放范围可整行省略（= 至今）

anchors:
  - id: anchor_founding
    name: 青云城建城
    day: 100801         # = (300-20)*360 + 1 → 严格对应旧稿"开元300年1月1日"
```

#### 2.3.3 YAML 表达选择：**存天数计数器为权威整数**

**裁定**：YAML 存 `day: 100801`（裸整数），**不存** `{year, month, day}`。理由：

| # | 理由 |
|---|---|
| ① | 与内部主表示 **1:1 无损**、天然可比较、可 diff |
| ② | 若 YAML 写 `{year,month,day}`，则加载期要换算、保存期要反算，**两处都可能出错**，且引入"YAML 与内部不一致"的窗口 |
| ③ | 自然语言"无仙1月2日"**无法静态校验**且需解析器 |

**可读性用 CLI 双表示兜底**（见 §2.7.1）：`@day1 (无仙 20 年 1 月 1 日) .. @day180360 (无仙 520 年 12 月 30 日)`。YAML 里写天数是为了**机器可验、可比较**；作者评审时需要人话，双表示在 CLI 一行内解决。

### 2.4 `TimeSpan` 与 `TimelineQuery`（**重做**）

#### 2.4.1 `TimeSpan` 退化为 `i64` 区间

```rust
/// 时间区间，**半开** `[from, to)`。
///
/// 半开语义的三条不变式（阶段一验收 B2）：
/// 1. `to` 时刻**不**属于本区间；
/// 2. `to == from`（零长度）恒为空区间，不包含任何时刻；
/// 3. 相邻区间 `[a,b)` 与 `[b,c)` **不重叠**——正是"状态切换零重叠"所需。
///
/// `to: None` 表示"至今有效"（开放区间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimeSpan {
    /// 天数计数器，闭端（含）。必须 >= 1。
    pub from: i64,
    /// 天数计数器，开端（不含）。`None` = 至今。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<i64>,
}

impl TimeSpan {
    /// 构造半开区间，前后校验顺序与下界。
    pub fn new(from: i64, to: i64) -> Result<Self, CoreError> {
        if from < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter: from });
        }
        if to < from {
            return Err(CoreError::InvalidSpan {
                from: from.to_string(),
                to: to.to_string(),
            });
        }
        Ok(Self { from, to: Some(to) })
    }

    /// 从 `from` 起至今（开放区间）。
    pub fn open(from: i64) -> Result<Self, CoreError> {
        if from < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter: from });
        }
        Ok(Self { from, to: None })
    }

    /// `t` 是否落在本区间内（半开）。
    pub fn contains(&self, t: i64) -> bool {
        if t < self.from {
            return false;
        }
        match self.to {
            Some(end) => t < end, // 严格小于：`to` 不属于本区间
            None => true,
        }
    }

    /// 与另一区间是否重叠。半开区间下可直接用端点比较。
    pub fn overlaps(&self, other: &TimeSpan) -> bool {
        // [a, b) 与 [c, d) 不交 ⟺ b <= c 或 d <= a
        let self_end_before_other = self.to.is_some_and(|b| b <= other.from);
        let other_end_before_self = other.to.is_some_and(|d| d <= self.from);
        !(self_end_before_other || other_end_before_self)
    }
}
```

> **为什么 `contains` 参数用 `i64`**：`TimePoint` 是 newtype `i64`，传 `i64` 免去调用方 `t.counter()` 的仪式。若团队偏好类型安全，可改签名为 `contains(&self, t: TimePoint)`。**两种都成立**，倾向 `i64`：`TimeSpan` 本身就是计数器区间，与 `TimePoint` 混用反而要在脑子里做一次"这是计数器还是时间点"的翻译。**此条留作团队一致性决议（U2），不阻塞封版。**

#### 2.4.2 `TimeSpan` 与 `i64` 的边界原则（**新增**）

| 原则 | 内容 |
|---|---|
| **计数器 vs 时间点** | `TimeSpan` 两端是**计数器**（`i64`）而非 `TimePoint`——区间端点本身不需要"已校验的时间点"语义 |
| **下界检查位置** | 在构造函数（`TimeSpan::new` / `open`）而非求值处 |
| **`TimePoint` 的门槛** | 只在"这是世界中的一个时刻"语义成立处使用；区间端点不属于该范畴 |

#### 2.4.3 `TimelineQuery` 重做

```rust
/// 时间轴求值入口。原则七的落地点。
///
/// 任何「某时刻的世界状态」都必须经过这里，禁止在渲染层自行比较时间点。
pub struct TimelineQuery;

impl TimelineQuery {
    /// 求 `t` 时刻的投影：返回所有**包含** `t` 的片段。
    pub fn at<T>(spans: &[(TimeSpan, T)], t: i64) -> Vec<&(TimeSpan, T)> {
        spans.iter().filter(|(s, _)| s.contains(t)).collect()
    }

    /// 求 `[start, end)` 区间内**有交集**的片段。
    /// 用于"这一年内发生了什么"这类范围查询。
    pub fn in_range<T>(spans: &[(TimeSpan, T)], start: i64, end: i64) -> Vec<&(TimeSpan, T)> {
        let probe = TimeSpan {
            from: start,
            to: Some(end),
        };
        spans.iter().filter(|(s, _)| s.overlaps(&probe)).collect()
    }
}
```

### 2.5 阶段一验收清单（**重做**）

#### 2.5.1 B 组改写对照

| 旧编号 | 旧内容 | **新编号** | **新内容** | 变更性质 |
|---|---|---|---|---|
| B1 | 四态 `VerificationState` | **B1** | 四态 `VerificationState`（不变，仍在距离/校验维度） | 保留 |
| B8 | 半开区间不变式 | **B2** | **半开区间不变式** `[from, to)`：`to` 时刻**不**属于本区间；零长度区间 `to == from` 恒为空 | 改写来源 |
| B9 | `to:null` round-trip | **B3** | **`last_day` 缺省 round-trip**：`last_day: None` 序列化时**不输出**该键，反序列化回 `None` | 改写来源 |
| B10 | 字典序不依赖 `total_minutes` | **B4** | **排序 = 天数计数器**：`TimePoint::from_counter(a) < from_counter(b) ⟺ a < b`；**删除**"不依赖 total_minutes"表述 | 废除旧约束 |
| B11 | 起始序号与换算 | **B5** | **换算恒等式**：对 `counter ∈ [1, 200000]` 全覆盖，`day_counter(civil(c)) == c` | 强化 |
| — | — | **B6** | **起点语义**：`Calendar::new("无仙", 20)` 时 `civil(1) == {20,1,1}`，且 `day_counter(20,1,1) == 1` | **新增** |
| — | — | **B7** | **下界拒绝**：`from_counter(0)` / `from_counter(-1)` 均返回 `DayCounterOutOfRange`；`advance(-1)` 在 `counter==1` 上被拒 | **新增** |
| — | — | **B8** | **越界年月日拒绝**：`month` ∉ 1..=12、`day` ∉ 1..=30、`year < start_year` 各返回对应错误，且**不 panic** | **新增** |
| — | — | **B9** | **历法常量不可配**：`DAYS_PER_YEAR == 360`、`DAYS_PER_MONTH == 30` 为 `const`；`Timeline` 不再有 `days_per_year` / `months` 字段 | **新增（反向验收）** |
| — | — | **B10** | **`TimePoint` 单字段**：`mem::size_of::<TimePoint>() == 8`；`#[serde(transparent)]` 使其 YAML 形态为裸整数 | **新增（反向验收）** |

**B 组项数：11 → 10**。

#### 2.5.2 封版判定（改写）

```text
A1–A5 + B1–B10 + C1–C4 + D1–D5 + E2/E3 + F1–F3 全绿
```

**A/C/D/E/F 组本轮未改**；E 组建议增补 `E5`：`civil()` / `day_counter()` 的属性测试式全覆盖。

**F 组新增**：`F4 时间模型 ADR`——必须写明"未来引入时分 = 一次破坏性 schema 迁移"（见 §2.8.2）。

#### 2.5.3 测试清单（26 项，含本轮新增的 3 条护栏）

```rust
    /// 轴右端边界。**本测试是为堵住"手写字面量未过计算器"这类错而补**。
    #[test]
    fn timeline_range_right_edge_is_exactly_500_years() {
        let c = cal();
        // 选定口径：轴右端 = 恰好满 500 年 = 无仙 520 年 12 月 30 日
        assert_eq!(c.civil(180_360), CivilDate { year: 520, month: 12, day: 30 });
        assert_eq!(c.day_counter(520, 12, 30).unwrap(), 180_360);
        // 交叉验证"满 500 年"语义：500*360 + 11*30 + 30
        assert_eq!(500 * DAYS_PER_YEAR + 11 * DAYS_PER_MONTH + 30, 180_360);
        // 右端的下一天必须已跨入 521 年
        assert_eq!(c.civil(180_361), CivilDate { year: 521, month: 1, day: 1 });
    }

    /// 把 §2.3.2 的 axis.yaml 示例当作**契约**钉死：三值必须自洽。
    #[test]
    fn example_axis_yaml_values_are_mutually_consistent() {
        let c = cal();
        let first_day: i64 = 1;
        let last_day: i64 = 180_360;
        let anchor_day: i64 = 100_801;
        assert_eq!(c.civil(first_day),  CivilDate { year: 20,  month: 1,  day: 1 });
        assert_eq!(c.civil(last_day),   CivilDate { year: 520, month: 12, day: 30 });
        assert_eq!(c.civil(anchor_day), CivilDate { year: 300, month: 1,  day: 1 });
        assert!(first_day <= anchor_day && anchor_day <= last_day, "锚点必须落在 timeline_range 内");
    }

    /// 决策表：把稿中所有出现过的字面量钉成一张表，防止再出现"手写数不过计算器"。
    #[test]
    fn draft_literal_values_are_all_verified() {
        let c = cal();
        let table: &[(i64, i64, i64, i64)] = &[
            // (counter, year, month, day)
            (1, 20, 1, 1), (2, 20, 1, 2), (30, 20, 1, 30), (31, 20, 2, 1),
            (360, 20, 12, 30), (361, 21, 1, 1),
            (93_615, 280, 1, 15), (100_801, 300, 1, 1), (100_810, 300, 1, 10),
            (102_601, 305, 1, 1), (102_602, 305, 1, 2),
            (180_360, 520, 12, 30),
        ];
        for &(counter, y, m, d) in table {
            assert_eq!(c.civil(counter), CivilDate { year: y, month: m, day: d }, "counter {counter} 换算不符");
            assert_eq!(c.day_counter(y, m, d).unwrap(), counter, "{y}/{m}/{d} 反算不符");
        }
    }
```

**其余 23 项测试与规格/验收项逐条映射**（节选）：`first_day_maps_to_start_year_month_one_day_one`（S4·B6）、`advancing_one_day_lands_on_month_one_day_two`（S1/S6）、`day_thirty_rolls_into_month_two`（S3 月进位）、`day_360_rolls_into_next_year`（S3 年进位）、`conversion_round_trips_over_full_range`（B5）、`counter_below_one_is_rejected`（S8·B7）、`ordering_is_day_counter_ordering`（R5·B4）、`timepoint_is_single_field_newtype`（R1·B10）、`half_open_span_excludes_its_end` / `zero_length_span_is_empty` / `adjacent_spans_do_not_overlap`（B2）、`axis_yaml_parses_flat`（§2.3.2）、`timepoint_serializes_as_bare_integer`（R1）。

#### 2.5.4 数字纪律（**新增，来自本轮两次同类错误**）

**纪律建议**：今后凡在文档/示例 YAML 里写具体数字，**必须同时进 `draft_literal_values_are_all_verified` 的表**（或对应局部测试）。否则数字只存在于散文里、**无人复核**。

理由：本轮技术稿 23 项测试全绿，但覆盖的全是"函数行为"，**文档/示例里的具体数字无人守护**——这正是 `last_day` 错漏网的原因。见 §3.2。

### 2.6 二进制地形格式

| 决策 | 裁定 |
|---|---|
| 存盘格式 | **bincode + zstd**（不用 rkyv：跨版本脆弱，与"长期存档"定位冲突） |
| 分块 | 256×256，索引 YAML |
| 时间语义 | **永不进 `terrain.bin`**（放 YAML 层） |
| 必须预留 | 8 字节文件头（魔数 `WVT1` + version）、索引 `format_version`、`chunk_size`/`dtype`、`schema_version` 与 `binary_format_version` 分离 |

### 2.7 错误处理、CLI 与性能

#### 2.7.1 新增 `CoreError` 变体（`crates/wv-core/src/error.rs`）

```rust
    #[error("天数计数器 {counter} 非法：时间轴只有起点、往前不存在，counter 必须 >= 1")]
    DayCounterOutOfRange { counter: i64 },

    #[error("年份 {year} 早于时间轴起点 {start_year}：起点之前不存在")]
    BeforeTimelineStart { year: i64, start_year: i32 },

    #[error("月份 {month} 非法：本历法固定 12 月/年，取值 1..=12")]
    InvalidMonth { month: i64 },

    #[error("日期 {day} 非法：本历法固定 30 天/月，取值 1..=30")]
    InvalidDay { day: i64 },
```

> **`InvalidSpan` 的处置**：保留但**仅由 `TimeSpan::new` 使用**——`TimeSpan` 现在存 `i64`，其 `from > to` 仍是非法区间。

**CLI 双表示兜底**：`wv-io` 的 `Project::summary()` 输出：

```text
时间轴:   @day1 (无仙 20 年 1 月 1 日) .. @day180360 (无仙 520 年 12 月 30 日)
```

#### 2.7.2 时间区间求值性能门槛

| 区间总数 N | 策略 | 阶段 |
|---|---|---|
| N < 5 万 | 朴素 O(n) 遍历 | 阶段五默认 |
| 5 万 ≤ N < 50 万 | 按 begin 排序 + 二分 + 后缀扫描 | 性能门禁触发时 |
| N ≥ 50 万 | 区间树 | 仅在真出现超大工程 |

> **注意（本轮修订）**：新模型端点是**单值 `i64`**，排序键从 5 元组塌缩为单键，比较成本更低 ⇒ 上述门槛**可上调**（具体数值待实测）。

**拖动优化**：**边界跨越检测**——若新 t 与旧 t 之间无任何区间边界，投影不变，零重算。

#### 2.7.3 `timeline_range.last_day` 的处置（门禁类）

| 项 | 内容 |
|---|---|
| **性质** | **门禁类**（无运行时降级） |
| **归类依据** | 约束的是**加载期的静态数据合法性**（作者声明的范围 vs 实际引用）。运行期不存在"把范围改小"的降级动作——改小会**丢掉数据语义** |
| **阈值与触发点** | `TimePoint.counter > last_day`（当 `last_day.is_some()`） |
| **卡在哪 / 谁判定 / 后果** | 卡在 **YAML 加载**环节，由 `wv-io` 加载器判定；不达标 = **拒绝加载并报错**，指出越界实体与越界值 |
| **为何不用运行时降级** | 为它造"自动截断到 last_day"会**静默篡改作者数据**，是过度工程 + 数据危险 |

### 2.8 风险与可逆性

| # | 风险 | 严重度 | 验证方法 | 触发点 |
|---|---|---|---|---|
| **T1** | 作者把 `prefix` 当纪元用 | 低 | `axis.yaml` 注释明示"只是名字"；schema 只有单 `prefix` 字段，**结构上无法表达多纪元** | 加载时无多纪元字段可读 |
| **T2** | YAML 天数计数器不可读（作者看到 `100801` 不知是几号） | **中** | `wv load` 摘要中同时打印天数与换算后日期（§2.7.1） | 阶段一 CLI |
| **T3** | 旧文件静默解析失败 | 中 | §2.9.3：升 `schema_version` 并给明确错误 | `Project::open` |
| **T4** | `i64` 计数器溢出 | **极低** | `i64::MAX / 360 ≈ 2.5e16` 年，远超任何工程 | 不需处理，仅记录 |
| **T5** | `hour/minute` 删除后未来要加回 | 中 | 见 §2.8.2 的可逆性说明 | 阶段五+ |
| **T6** | `contains` 签名 `i64` vs `TimePoint` 团队不一致 | 低 | 团队内部一致性决议（U2） | 阶段一 PR review |

#### 2.8.1 可读性兜底（T2）
见 §2.7.1 的 CLI 双表示。

#### 2.8.2 精度的可逆性（T5，**必须写进 ADR**）

**重新引入时分时的路径**：

1. `TimePoint` 当前 = `i64` 天。要加时分，**最小侵入**方案是再包一层 `struct TimePoint { day: i64, minute_of_day: u16 }`——但会破坏 `#[serde(transparent)]`，且 YAML 形态从裸整数变成 `{day, minute_of_day}`，**又是一次全量 YAML 迁移**。
2. 替代方案（**推荐**）：把刻度降为**分钟**：`TimePoint(i64)` 语义改为"第 N 分钟"，加常量 `MINUTES_PER_DAY = 1440`。YAML 形态仍是裸整数，但**含义变了**（旧文件的 `100801` 会变成第 100801 分钟 ≈ 第 70 天）——**仍是破坏性迁移**。
3. **结论**：时分若一定有，**早知道比晚知道便宜**。当前按 S7 删除是正确取舍，但**必须在 ADR 里写明"未来引入 = 一次 schema 迁移"**，不要留"以后加就行了"的错觉。

### 2.9 对骨架的差异提示（**§5 重做**）

#### 2.9.1 `crates/wv-core/src/time.rs`：必须修改的 15 条

| # | 现骨架条目（v0.1） | 处置 | 说明 |
|---|---|---|---|
| 1 | `struct TimePoint { era, year, day, hour, minute }` | **删并改** | 替换为 `pub struct TimePoint(i64)` |
| 2 | `TimePoint::new(era, year, day)` | **删** | 替换为 `TimePoint::from_counter` / `Calendar::day_counter` |
| 3 | `impl Display for TimePoint`（`E{era}.{year}.{day}`） | **改** | 改为 `@day{n}`；带前缀展示走 `Calendar::format` |
| 4 | `struct TimeSpan { from: TimePoint, to: Option<TimePoint> }` | **改** | 字段类型 `TimePoint` → `i64` |
| 5 | `TimeSpan::new`（返回 `InvalidSpan`） | **改** | 入参改 `(i64, i64)`，并补下界检查 |
| 6 | `TimeSpan::open(from)` | **改** | 入参改 `i64`，返回 `Result`（下界可错） |
| 7 | `TimeSpan::contains(&TimePoint)`（闭区间 `t <= end`） | **改** | 改 `contains(i64)`，**`t < end`（半开）** |
| 8 | `TimeSpan::overlaps(&TimeSpan)` | **改** | 半开端点比较实现 |
| 9 | `struct Timeline { eras, days_per_year, months, anchors }` | **改** | 替换为 `{ calendar, timeline_range, anchors }` |
| 10 | `struct Era { id, name, order, start }` | **删** | 无纪元概念 |
| 11 | `struct MonthDef { name, days }` | **删** | 历法固定 30 天/月 |
| 12 | `struct Anchor { id, name, at: TimePoint }` | **改** | `at` → `day: i64`（推荐，与 YAML 一致；见 U3） |
| 13 | `TimelineQuery::at(&[(TimeSpan,T)], &TimePoint)` | **改** | 参数改 `i64`；新增 `in_range` |
| 14 | 模块文档注释"至少到纪元+年+日" | **改** | 改为"主表示是天数计数器" |
| 15 | 全部 5 个 `#[cfg(test)]` 测试 | **重写** | 用 §2.5.3 的测试集替换 |
| — | `default_days_per_year()` | **删** | 常量替代 |

#### 2.9.2 受牵连的其他文件（11 个）

| 文件 | 现状引用 | 处置 |
|---|---|---|
| `crates/wv-core/src/lib.rs` | `pub use time::{TimePoint, TimeSpan}` | 追加导出 `Calendar` / `CivilDate` / `Timeline` / `DayRange`；`WorldMeta.timeline_range: TimeSpan` 改为 `DayRange` |
| `crates/wv-core/src/error.rs` | 无时间换算错误 | 追加 §2.7.1 的 4 个变体 |
| `crates/wv-core/src/event.rs` | `span: TimeSpan` | **类型不变**，但 YAML 形态变：`from: 100801` / `to: 100810` |
| `crates/wv-core/src/character.rs` | `born_at: Option<TimePoint>`、`CharacterStateSpan.span: TimeSpan` | 类型不变；YAML 形态变 |
| `crates/wv-core/src/map.rs` | `GeometrySpan.span: TimeSpan` | 类型不变；YAML 形态变 |
| `crates/wv-core/src/distance.rs` | `Feasibility` 用 `TimePoint` | 类型不变；**构造处**需走 `TimePoint::from_counter` 并处理 `Result` |
| `crates/wv-io/src/project.rs` | `summary()` 打印 `timeline_range.from/to` | 改打印 `first_day` / `last_day` |
| `crates/wv-io/src/lib.rs` | `CURRENT_SCHEMA_VERSION = 1` | **升 2**（见 §2.9.3） |
| `crates/wv-io/tests/example_project.rs` | `timeline_yaml_round_trips` 构造 `Era`/`MonthDef` | **重写**该测试 |
| `examples/minimal-world/timeline/axis.yaml` | `eras: [...]` + `days_per_year` | **替换**为 §2.3.2 的 YAML |
| `examples/minimal-world/timeline/events/evt_qingyun_founding.yaml` | `from: {era: 1, year: 300, day: 1}` | 改为 `from: 100801` / `to: 100810` |
| `examples/minimal-world/world.yaml` | `timeline_range.from/to` 为 `{era,year,day}` | 改为 `first_day` / `last_day` |
| `examples/minimal-world/characters/char_shenyan.yaml` | `born_at` 与 `state_spans` 用 `{era,year,day}` | 全量换算为天数计数器 |

**旧 YAML → 新天数计数器 换算表**（`start_year = 20`，已逐项复核）：

| 旧稿（era/year/day） | 新 `day` | 算式 |
|---|---|---|
| 280 / 1 / 15（沈砚生辰） | **93,615** | `(280-20)*360 + 0*30 + 15` |
| 300 / 1 / 1（建城 / 事件 from） | **100,801** | `(300-20)*360 + 1` |
| 300 / 1 / 10（事件 to） | **100,810** | `(300-20)*360 + 10` |
| 305 / 1 / 1（state_span 切换） | **102,601** | `(305-20)*360 + 1` |
| 305 / 1 / 2（第二条 span from） | **102,602** | `(305-20)*360 + 2` |
| 520 / 12 / 30（轴右端 `last_day`） | **180,360** | `(520-20)*360 + 11*30 + 30`（= 满 500 年） |

> **轴右端口径**：`180360` / `无仙 520 年 12 月 30 日`，三处（`axis.yaml` / CLI 示例 / 本表）已对齐。**变更须三处同改 + 两条护栏测试同改。**

> **注**：旧稿 `char_shenyan.yaml` 的 `to: 305/1/1` 与第二条 span 的 `from: 305/1/2` 在旧模型下是**1 天空隙的闭区间**；换成半开 `[100801, 102601)` / `[102602, ∞)` 后**间隙仍在**（`102601` 未被覆盖）。这不是本稿引入的缺陷，是旧稿既有数据问题——**建议示例工程改为 `to: 102602`**，使两段严丝合缝。

#### 2.9.3 `schema_version` 处置

| 选项 | 说明 | 裁定 |
|---|---|---|
| 保持 `1`，静默接受旧文件 | 旧文件的 `{era,year,day}` 会**解析失败**（新 `TimePoint` 期望裸整数），报错是"期望整数"——作者不知道发生了什么 | ❌ 不可接受 |
| **升 `2` + 拒绝旧文件并给出迁移提示** | `CURRENT_SCHEMA_VERSION = 2`；读到 `schema_version: 1` 时给出明确错误"时间模型已从纪元/年月日改为天数计数器，请迁移" | ✅ **采纳** |
| 升 `2` + 自动迁移 | 需要读旧 `{era,year,day}` 并配 `start_year` 才能换算——但旧文件**没有 `start_year`**（只有 `eras[].order`），**无法自动迁移** | ❌ 不可行 |

> **结论**：升 `2`，**不做自动迁移**（信息不足），但给出**可操作的**错误指引，并在 `examples/` 里直接把示例工程改到位，作为人工迁移的参考样板。**此项改 v0.2 已确认内容，列入 §4 待拍板。**

#### 2.9.4 建议清理项（非时间模型，但同批触碰）

| 文件 | 项 | 理由 |
|---|---|---|
| `crates/wv-core/src/distance.rs` | `DistanceGraph::shortest_journey`（Dijkstra） | v0.2 §5 已裁定"不做路径搜索"；本轮顺带确认仍应删除 |
| `crates/wv-core/src/error.rs` | `CoreError::PathSearch` | 随上条一并删除 |
| `crates/wv-core/src/lib.rs` | 模块文档"历史状态以时间区间存储" | 与新时间模型自洽，**保留** |

### 2.10 一页速览

```text
TimePoint  = struct TimePoint(i64)           // 天数计数器，>= 1，serde(transparent)
Calendar   = { prefix: String, start_year: i32 }
CivilDate  = { year: i64, month: i64, day: i64 }   // 仅展示/解析
TimeSpan   = { from: i64, to: Option<i64> }        // 半开 [from, to)
Timeline   = { prefix, start_year, timeline_range{first_day, last_day?}, anchors[] }

常量: DAYS_PER_YEAR = 360, DAYS_PER_MONTH = 30, MONTHS_PER_YEAR = 12
换算: year = start_year + (c-1)/360;  month = ((c-1)%360)/30 + 1;  day = (c-1)%30 + 1
反向: c = (year - start_year)*360 + (month-1)*30 + day
下界: c >= 1，无负数、无纪元前、无 era 字段
排序: 天数计数器数值序（derive Ord）
删除: era / Era / MonthDef / days_per_year / hour / minute / total_minutes / 负年份
```

---

## 3. 裁定篇（双方交叉核对后的收敛结果）

### 3.1 时间模型相关裁定（**本轮重做**）

| # | 议题 | 裁定 | 裁定者 |
|---|---|---|---|
| **C1** | 时间区间开闭语义 | **半开 `[from, to)`**；`to: null` 表无限 | 设计提出，技术采纳 |
| **C2** | 校验第 4 态 | `Unknown { missing: Vec<String> }` | 两人一致 |
| **C3** | 负年份 | **作废**：时间主键无负数（`counter >= 1`）；替代机制为 `start_year: i32` 允许负数（前向显示年份） | 用户拍板（S8） |
| **C4** | 地形时间语义 | **静态**，但数据模型预留区间化 | 设计裁定 + 技术确认 |
| **C5** | 地图级可见区间 | 可行，成本 ≈ 0，**只做可见性不自动切 active map**；端点类型随之简化为 `i64` | 技术评估，设计接受 |
| **C6** | 校验不依赖地图 | **签字确认**，固化为不变式 + 硬验收 D5 | 设计签字，技术固化 |
| **C7** | 覆盖层相对偏移 | **架构强行保证**"改参数不清空手改" | 技术裁定 |
| **C8** | 区间求值性能 | <5 万朴素，>5 万索引；**端点塌缩为单键后门槛可上调** | 两人独立收敛 |
| **C9** | 结论新鲜度 | 脏结论必须灰化标注"待刷新" | 两人独立收敛 |
| **C10** | Q1 纪元长度 | **作废**：纪元概念消失；`year_length_days` 可选字段**删除**，代之以硬约束 360/30 | 用户拍板（S3/S9） |
| **C11** | 书签归属 | `timeline/bookmarks.yaml`（世界语义，非 UI 偏好）；**优先级升为「必做, 阶段五」** | 设计裁定（本轮升级） |
| **C12** | 历法常量 | 360 天/年 + 30 天/月 是**唯一历法（硬约束）**；旧「非 365」讨论彻底关闭 | 技术最终裁定（本轮升级） |

**冲击汇总**：作废 2（C3 时间主键、C10）、修订 5（C1/C5/C8/C11 + C3 替代机制）、升级 1（C12）、不变 5（C2/C4/C6/C7/C9）。

### 3.2 本轮值得记录的协作质量证据

**技术侧的真实自我纠错（两次同类错误）**：

1. **柯桥良第二轮推翻自己的 v0.2 定稿**——v0.2 刚把 `TimePoint` 定为"5 元组 + `i16/i32` 支持负年份"，用户新规格一到，**主动降级为单字段 newtype 并删除 `era`**。这不是修补，是**退回去重做**。
2. **柯桥良自查出一个真实算术错误**——初版把锚点天数写成 `107461`（应为 `100801`），测试 #17 断言失败暴露。若不做编译+测试验证，此错会直接进交付稿并让示例工程全部锚点偏移。
3. **主理人独立复核出第二个同类错误（未被任何测试覆盖）**——技术稿 23 项测试全绿，但 `180000` / `180030` / "520年12月30日" 三个数字在同一份稿里互不相容：`§3.2 axis.yaml` 写 `last_day: 180000`、`§10.1 CLI` 写 `@day180000 (无仙 520 年 12 月 30 日)`、`§9.2 换算表` 写 `520/12/30 → 180,030`。实际 `180360` 才是正确值。**根因：数字写在散文/示例里，无测试守护。**
4. **柯桥良的响应不是"改一个字"，而是补结构性护栏**——新增 3 条测试（轴右端边界 / `axis.yaml` 三值自洽 / **决策表钉死全部字面量**），把这个错误类从"靠细心"变成"靠测试"。
5. **一处口径冲突被识别而非掩盖**——用户 S6 算例"无仙1月2日"与 S4 严格推导"无仙20年1月2日"存在口语省略差异。**没有硬凑**，而是标注为待确认项（U4）并给出"数据模型不受影响"的隔离结论。
6. **拒绝过度工程**——面对"是否保留时分"，没有为"以后可能要用"而保留 `hour/minute`，而是按 S3/S7 直接删除，并**写明重新引入的代价与路径**（§2.8.2）。

**协作质量结论**：两次算术错误，一次由成员自查发现，一次由主理人复核发现。这说明**单一角色自审不足以覆盖"散文里的数字"**，需要在流程上补一道（已固化为 §2.5.4 数字纪律）。

### 3.3 主理人（汇编者）在本轮发现并拦截的问题

| # | 拦截的问题 | 若放过的后果 |
|---|---|---|
| 1 | 三处 `last_day` 数字互不相容（`180000`/`180030`/`520年12月30日`） | 示例工程轴右端错误；所有"满 500 年"表述失准 |
| 2 | 技术稿测试集**无 `timeline_range` 边界断言** | 上述错误类无守护，未来同类错仍会漏网 |
| 3 | 设计稿"前缀"易被误读为可切换历法 | 作者期待「帝国历→联邦历」切换，用不了 |
| 4 | 设计稿"往前不存在"未处理前史需求 | 作者第一次写「上古传说」时撞墙（已由设计侧给出前史区方案） |

---

## 4. 需用户拍板事项

### 4.1 时间模型相关（本轮新增）

| # | 事项 | 团队结论 | 触及 v0.2 已确认项 | 晚改代价 |
|---|---|---|---|---|
| **N1** | `schema_version` 1 → 2 + 拒绝旧文件 | 建议升 2，不做自动迁移（旧文件无 `start_year`，信息不足）；给可操作错误指引 | ✅ 改 §9 数据版本 | 低（示例工程同步改即可） |
| **N2** | **S6 算例口径**：显示是否恒带年 | 技术按严格推导**带年**；设计建议**小跨度省略年、大跨度强制带年**。**数据模型不受影响** | 展示层策略 | 低（纯展示） |
| **N3** | **前史区**（方案 B）是否存在 | 设计侧**建议要**——否则「建国前传说」无处安放 | 新增能力 | 中 |
| **N4** | 双轨分屏（原 ⑤）是否保留 | 纪元消失后唯一强用例消失，设计侧**建议砍** | §1.5 机制 | 低 |
| **N5** | V9（地点边时变）降级是否可接受 | 设计侧**建议降级**至阶段五+ | 验收范围 | 低 |

### 4.2 沿用上轮的四项决策（团队意见已一致，2 项触及 v0.2）

| # | 事项 | 团队结论 | 触及 v0.2 已确认项 | 晚改代价 |
|---|---|---|---|---|
| **D1** | 校验增设第 4 态 `Unknown` | 两人一致同意 | ✅ 改 §10.4 三态契约 | 高（UI 返工） |
| **D2** | 「迟到」反算最早出发时间 | 沿已声明事件链反推，不做全局搜索 | §10.4 新增能力 | 中 |
| ~~D3~~ | 负年份 → `i16/i32` | **已被本轮作废**（见 C3） | — | — |
| **D4** | 多地图各带可见时间区间 | 可行，成本 ≈ 0 | §3.1 机制扩展（落阶段五） | 低 |

### 4.3 三项团队内部一致性决议（不阻塞封版）

| # | 项 | 推荐 | 性质 |
|---|---|---|---|
| **U1** | `calendar:` 是否嵌套在 YAML 里 | 扁平（`flatten`），见 §2.3.1 取舍说明 | 一致性 |
| **U2** | `TimeSpan::contains` 入参 `i64` vs `TimePoint` | `i64`（见 §2.4.1 注） | 一致性 |
| **U3** | `Anchor.day` 用 `i64` 还是 `TimePoint` | `i64`（与 YAML 一致） | 一致性 |

### 4.4 一项小偏好（不阻塞封版）

| 项 | 默认 | 备选 |
|---|---|---|
| `year: 0` / `start_year` 为负的处理 | **允许任意 `i32`**（支撑前史方案 A，见 V12） | 硬禁 |

---

## 5. 团队状态

| 成员 | 状态 |
|---|---|
| 游有方 · 创意总监 | ✅ 已完成，设计侧零遗留 |
| 柯桥良 · 技术架构总监 | ✅ 已完成，26 项测试全绿 / clippy 零告警 / fmt 干净；三处 `last_day` 口径已对齐 |
| 阻塞项 | **无** |

**验证证据汇总**：

| 门槛 | 结果 |
|---|---|
| `cargo test` | ✅ **26 passed / 0 failed** |
| `cargo clippy --all-targets -- -D warnings` | ✅ **零告警** |
| `cargo fmt --all -- --check` | ✅ **干净** |
| 草稿 markdown 代码块**程序化抽取**编译 | ✅ 同样 26 passed（证明"草稿文字本身"可编译） |
| 主理人独立数字复核（§0.3） | ✅ 全部通过，含 `1..200000` 全覆盖恒等式零反例 |

**下一步**：用户对 §4 拍板 → 按 §2.9 差异清单修正骨架 → 进入阶段一实现。
