# 时间模型第二轮修订稿（可插拔 `CalendarRule` + 故事实体 + 校验分流）

> **编制** 柯桥良 · 技术架构总监
> **用途** 主理人汇编用草稿；**就地覆盖** `docs/DESIGN_DELIVERY_v0.2.md` 的 §0.2 / §2.2 / §2.3 / §2.9 / §3.1 / §1.5 相关段落，**不升版本号**。
> **评审范围** 本文只谈技术裁定与可编译证据；创作语义（故事归属、显示体感、导航体感）由游有方侧裁定，本文只给**技术后果**。
> **验证状态** 全部 Rust 代码已在 `rustc 1.97.1 / edition 2021` 下**实际编译并运行**：
> `cargo test` = **31 passed / 0 failed**（4 单元 + 27 集成）；`cargo clippy --all-targets -- -D warnings` = **零告警**；`cargo fmt --all -- --check` = **干净**。
>
> **✅ 与设计侧（you-youfang-2）已全部收敛**（其 `_draft_calendar_you.md` §B5 终裁）：历法枚举命名统一为 `CalendarRule{Gregorian/Simple365/Fixed360}`（tag `gregorian/simple_365/fixed_360`）；**取消 `custom` 变体**；年号采用**七字段合并 schema + 最小 UI**；故事 `placements` 采纳。**唯一不可动的边界（两侧共识）**：**运算层（`TimePoint`/`TimeSpan`/`TimelineQuery`/排序/下界）一个字都不许动** —— 见 §3.1。

---

## 0. 结论先行（一页速览）

| # | 议题 | 旧稿 | **新稿裁定** | 理由 |
|---|---|---|---|---|
| **M1** | 历法可配置性 | `const DAYS_PER_YEAR = 360` 硬编码（§2.2.4 / C12） | **可插拔 `CalendarRule` enum（封闭三值），工程级唯一** | 用户明确要三种显示方式；硬编码推翻 |
| **M2** | 三种历法建模 | 无 | **enum**（`Gregorian` / `Simple365` / `Fixed360`），**不选 trait**；**`custom` 已明确取消** | 三值封闭、可 match、无 dyn、YAML 直配；"现实小说 = 公历 + 年号"由 `Gregorian + reign_aliases` 覆盖 |
| **M3** | 故事实体 | 不存在 | **新增 `story.rs`，独立 crate 内新模块，不与 event 合并**（含 `placements`） | 无日期 ⇒ 领域约束不同 ⇒ 独立类型 |
| **M4** | 事件硬下界 | `counter >= 1`（`TimePoint::from_counter`） | **事件 `from` 必须 `> 起点 0 时`，即 `>= 2`** | 用户原话"必须大于 1840年1月1号 0时0分" |
| **M5** | 跨度预警 | 无 | **渲染层降级预警**，阈值 100 万天（≈2738 年），**非门禁、与 i64 解耦** | i64 溢出是伪瓶颈；真瓶颈是 UI |
| **M6** | `TimePoint` / `TimeSpan` / `TimelineQuery` | newtype / i64 区间 / i64 入参 | **不改**（仅换算下沉） | counter 仍是绝对物理天数，只有解释规则变了 |
| **M7** | `schema_version` | `1 → 2`（上一轮已定） | **`2 → 3`** | YAML 形态新增 `calendar.rule` / `reign_aliases`，属破坏性 |
| **M8** | 年号/农历 | 无 | **只做显示 + 年份级硬校验（七字段 `ReignAlias`），不实现农历算法** | 用户 Q1 确认；年份级校验不需农历算法 |

**前提假设**（变了要重估）：
- H1：历法是**工程级唯一**（全轴统一），不是每事件各自标注 —— 用户 Q2 确认。
- H2：公历按**真实格里高利历**、**真实公元纪年**（含 0 年 / 负年按天文记年）—— 用户 Q4 确认。
- H3：农历/年号**永不参与换算**，只是贴着公历日期的写法（年份级校验用静态史实，月日不推算）—— 用户 Q1 确认。
- H4：阶段一刻度 = 1 天（无时分）—— 沿用上一轮 S7 定稿，本轮不变。

---

## 1. M1/M2：`CalendarRule` 的设计裁决

### 1.1 裁决：**enum（封闭三值），不是 trait**

**主理人倾向 enum，我专业判断：同意，且理由比"YAML 可配"更硬。**

| # | 判据 | enum 胜出理由 |
|---|---|---|
| 1 | **集合封闭性** | 用户钦定"默认有 3 种"，是**封闭集合**。trait 的价值在"第三方可扩展"，而这里**不预期扩展**。为不确定的未来付 `dyn`/泛型成本 = 过早抽象 |
| 2 | **穷尽匹配** | `match rule { Gregorian=>.., Simple365=>.., Fixed360=>.. }` 由编译器强制穷尽；将来若加第四种历法，**所有未更新处编译报错**——这正是我们要的"改动可见" |
| 3 | **零开销** | `enum` 是 `Copy`、栈上 1 字节、无 `Box`、无 vtable。trait 需 `&dyn CalendarRule` 或泛型参数化，把复杂度传染到**所有**上层结构（`Calendar`、`Timeline`、加载器） |
| 4 | **serde 友好** | `#[serde(rename_all="snake_case")]` 直接产出 `rule: gregorian`，YAML 零手写。trait 需 `typetag` 之类，引入额外依赖与"注册"心智 |
| 5 | **可逆扩展** | 若**真**要开放（如未来支持自定义月表），把 enum 包成 `Custom(Box<dyn ...>)` 一个变体即可——**非破坏性**。反之若现在选 trait，想收回成 enum 是**破坏性** |
| 6 | **可哈希/可比较** | `CalendarRule` 可 `derive(Hash, Eq)`，便于做换算缓存键；trait object 不能 |

> **反方意见（记录在案，判定不成立）**：主张"trait 更易加第四种历法"。反驳：加第四种历法的成本，enum 是**改一个 match 分支**（编译器会点出所有未覆盖处），trait 是**加一个 impl 但没有任何编译器提示**——对单人开发者，**被编译器揪出来**比"记得去改"安全得多。

### 1.2 何时该翻案（写清触发条件，不留模糊）

若出现以下**任一**，本裁决作废、改抽 trait：
1. 出现"作者自定义月表/月名"需求（如 13 个月、月长不等且由作者在 YAML 里写）；
2. 出现"一个工程内多套历法并存"需求（与 H1 冲突）；
3. 出现"第三方历法插件"需求。

**当前三者皆无。**

> **✅ 已明确记录一次翻案尝试与退回**：设计侧原稿曾提议加 `custom` 变体（"现实小说/自定义"档）。经双方核对，**取消 `custom`**——理由：
> - "现实小说 = 公历 + 年号" 实际是 **`Gregorian` + `reign_aliases`**，**不需独立变体**（为同一语义造两个概念，是本项目反复砍的过度设计）；
> - "作者真自定义月表（13 个月、月长不等）" 才是**真 custom**，但它**触发上面第 1 条翻案条件**（自定义月表）——**那是一次"加能力"的破坏性变更，不应现在预留空变体**。
> - **终裁：本轮保持封闭三值。** 未来真出现自定义月表需求时，按本 §1.2 抽 trait，届时 enum 作为其一种实现（非破坏性演进路径已预留）。

### 1.3 Rust 定义（已编译）

```rust
/// 内置历法种类（封闭三值）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CalendarRule {
    /// ① 公历（真实格里高利历，含闰年）。
    Gregorian,
    /// ② 简单 365 天历（无闰年，月长与公历非闰年一致）。
    #[serde(rename = "simple_365")]
    Simple365,
    /// ③ 360 天/年、30 天/月（奇幻默认，等价上一轮实现）。
    #[default]
    #[serde(rename = "fixed_360")]
    Fixed360,
}
```

> **注意两个显式 `rename`**：`Fixed360` 在 `rename_all="snake_case"` 下会得到 `fixed360`（数字不产生词边界），**必须显式写 `fixed_360`**。这是我在实测中踩到并修正的一处（见 §7 自纠记录 #1）。`simple_365` 同理。**此坑已进测试 `calendar_rule_serializes_snake_case` 钉死。**

---

## 2. M2：三种历法的换算实现（完整可编译代码）
### 2.1 核心算法：**绝不使用 `offset / 365` 近似**

上一轮用 `offset / DAYS_PER_YEAR` 是因为 360 整除、无闰年——数学上恰好等价。**公历引入闰年后，该写法是错的**：

```text
错误（上一轮写法，公历下崩）：
  year  = start_year + offset / 365      // 每遇一个闰年就累计错 1 天
正确（本轮）：
  以 start_year 为锚，逐年减「该年真实天数」（365 或 366），余下落入年内再逐月减
```

**复杂度** O(年数 + 月数)。对阶段一作者规模（≤ 数万年）单次换算远低于 1ms。若未来 10 万年轴真成热点，可加"世纪块前缀和"缓存（见 §6 风险）。

### 2.2 闰年规则（格里高利）

```rust
/// 格里高利闰年规则：**4 年一闰、百年不闰、四百年再闰**。
/// `year` 为真实公元纪年（可为 0 与负数，按天文记年；本轮 Q4 确认参照系为真实公元）。
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
```

> 天文记年说明：`year = 0` 对应历史纪年的"公元前 1 年"，`year = -1` 对应"公元前 2 年"。用户 Q4 确认"真实公元纪年"，故取这一约定（`0` 是闰年、`-100` 非闰年、`-400` 闰年；已测试钉死）。**若作者习惯"公元前 1 年"直接写 `-1`，需在 UI 层做一次显式映射**——此条**转待裁**（U5，见 §8）。

### 2.3 完整实现（`crates/wv-core/src/calendar.rs`，新文件）

```rust
//! 可插拔历法：`CalendarRule`（工程级唯一）+ 换算。
//!
//! 核心规格（用户确认，唯一权威）：
//! - 内部主表示 = **整数天数计数器**，第 1 天 = 游戏第一日；每天自动 +1。
//! - 历法是**可插拔的显示/解析规则**，不是运算主键；运算主键永远是 counter。
//! - 只有**一个起点**，往前不存在（无负数计数器）。
//! - 前缀（如 `无仙` / `帝国历`）只是**起点的命名标签**，不是可切换纪元。
//! - 农历/年号**只做显示（作者手填注释）**，不实现农历算法、不内置农历表。

use crate::error::CoreError;

/// 内置历法种类（封闭三值）。设计裁决见交付稿 §1：**enum 而非 trait**。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CalendarRule {
    /// ① 公历（真实格里高利历，含闰年）。
    Gregorian,
    /// ② 简单 365 天历（无闰年，月长与公历非闰年一致）。
    #[serde(rename = "simple_365")]
    Simple365,
    /// ③ 360 天/年、30 天/月（奇幻默认，等价上一轮实现）。
    #[default]
    #[serde(rename = "fixed_360")]
    Fixed360,
}

impl CalendarRule {
    /// 平年天数（非闰年基准）。
    pub const fn days_per_common_year(self) -> i64 {
        match self {
            CalendarRule::Gregorian | CalendarRule::Simple365 => 365,
            CalendarRule::Fixed360 => 360,
        }
    }

    pub const fn has_leap_year(self) -> bool {
        matches!(self, CalendarRule::Gregorian)
    }

    /// 某年的天数。
    pub fn days_in_year(self, year: i64) -> i64 {
        match self {
            CalendarRule::Gregorian if is_leap_year(year) => 366,
            _ => self.days_per_common_year(),
        }
    }

    /// 某年某月的天数。
    pub fn days_in_month(self, year: i64, month: i64) -> i64 {
        debug_assert!((1..=12).contains(&month));
        match self {
            CalendarRule::Fixed360 => 30,
            CalendarRule::Simple365 | CalendarRule::Gregorian => match month {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                2 => {
                    if self == CalendarRule::Gregorian && is_leap_year(year) {
                        29
                    } else {
                        28
                    }
                }
                _ => 30,
            },
        }
    }

    pub const fn months_per_year(self) -> i64 {
        12
    }
}

/// 格里高利闰年规则：**4 年一闰、百年不闰、四百年再闰**。
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 时间轴坐标系：历法规则 + 起点命名 + 起点年。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Calendar {
    #[serde(default)]
    pub prefix: String,
    pub start_year: i32,
    #[serde(default)]
    pub rule: CalendarRule,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reign_aliases: Vec<ReignAlias>,
}

/// 年号对照。**仅显示用**——不参与排序、不参与区间求值。
///
/// 七字段：前四为**内置静态史实**（支撑年份级硬校验），中二为**工具换算**的
/// 天数计数器区间，末一为**作者手填**、工具绝不解析。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReignAlias {
    /// 【内置静态史实】年号名，如 `嘉庆`。
    pub era_name: String,
    /// 【内置静态史实】朝代，如 `清`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynasty: Option<String>,
    /// 【内置静态史实】该年号的公历起年（含），如 1796。用于年份级硬校验。
    pub gregorian_start: i32,
    /// 【内置静态史实】该年号的公历止年（含），如 1820。
    pub gregorian_end: i32,
    /// 【工具换算】本对照在时间轴上的天数计数器起点（含）。
    pub start: i64,
    /// 【工具换算】终点（不含）；省略 = 至今。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    /// 【作者手填】农历写法，如 `嘉庆二十三年四月初一`。**工具绝不解析此字段**。
    pub lunar_label: String,
}

impl ReignAlias {
    /// 年份级硬校验：作者填的公历年份是否落在年号期内。
    ///
    /// **不需要农历算法**——只是比较两个公历年份（`gregorian_start/end` 是静态史实）。
    /// 月/日级换算不在此函数（工具不推算农历，只警告）。
    pub fn is_year_consistent(&self, gregorian_year: i32) -> bool {
        (self.gregorian_start..=self.gregorian_end).contains(&gregorian_year)
    }
}

impl Calendar {
    pub fn new(prefix: impl Into<String>, start_year: i32, rule: CalendarRule) -> Self {
        Self {
            prefix: prefix.into(),
            start_year,
            rule,
            reign_aliases: Vec::new(),
        }
    }

    /// 天数计数器 → 年月日。**不使用 `offset / 365` 近似**。
    pub fn civil(&self, counter: i64) -> CivilDate {
        let mut offset = counter - 1;
        let mut year = self.start_year as i64;

        loop {
            let diy = self.rule.days_in_year(year);
            if offset < diy {
                break;
            }
            offset -= diy;
            year += 1;
        }

        let mut month = 1i64;
        loop {
            let dim = self.rule.days_in_month(year, month);
            if offset < dim {
                break;
            }
            offset -= dim;
            month += 1;
        }

        CivilDate {
            year,
            month,
            day: offset + 1,
        }
    }

    /// 年月日 → 天数计数器。**整年天数累加**，非近似。
    pub fn day_counter(&self, year: i64, month: i64, day: i64) -> Result<i64, CoreError> {
        if year < self.start_year as i64 {
            return Err(CoreError::BeforeTimelineStart {
                year,
                start_year: self.start_year,
            });
        }
        let months = self.rule.months_per_year();
        if !(1..=months).contains(&month) {
            return Err(CoreError::InvalidMonth { month, max: months });
        }
        let dim = self.rule.days_in_month(year, month);
        if !(1..=dim).contains(&day) {
            return Err(CoreError::InvalidDay { day, max: dim });
        }

        let mut counter: i64 = 1;
        let mut y = self.start_year as i64;
        while y < year {
            counter += self.rule.days_in_year(y);
            y += 1;
        }
        let mut m = 1i64;
        while m < month {
            counter += self.rule.days_in_month(year, m);
            m += 1;
        }
        counter += day - 1;
        Ok(counter)
    }

    pub fn first_day(&self) -> i64 {
        1
    }

    /// 展示文本。`with_year = false` 时省略年份（对应口语省略句式）。
    pub fn format(&self, counter: i64, with_year: bool) -> String {
        let d = self.civil(counter);
        let core = if with_year {
            format!("{} {} 年 {} 月 {} 日", self.prefix, d.year, d.month, d.day)
        } else {
            format!("{} {} 月 {} 日", self.prefix, d.month, d.day)
        };
        // 年号对照（仅显示）：命中区间则追加。
        if let Some(alias) = self
            .reign_aliases
            .iter()
            .find(|a| counter >= a.start && a.end.map_or(true, |e| counter < e))
        {
            return format!("{core}（{}）", alias.lunar_label);
        }
        core
    }
}

/// 换算后的年月日。**只用于展示/解析**，不得作为存盘字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CivilDate {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl CivilDate {
    pub fn to_counter(self, calendar: &Calendar) -> Result<i64, CoreError> {
        calendar.day_counter(self.year, self.month, self.day)
    }
}
```

### 2.4 三种历法的**代码复用方式**（明确回答主理人问题）

**复用靠"策略钩子"而非"三份实现"**：`civil` / `day_counter` **只写一份**，它俩不直接读常量，而是调用 `self.rule.days_in_year(y)` / `days_in_month(y,m)`。三种历法**只在这两个钩子函数内分叉**：

| 历法 | `days_in_year` | `days_in_month(y, 2)` | `days_in_month(y, 其他)` |
|---|---|---|---|
| `Gregorian` | 366/365（含闰） | 29/28 | 31/30（真实公历） |
| `Simple365` | 恒 365 | 恒 28 | 同上（31/30） |
| `Fixed360` | 恒 360 | 恒 30 | 恒 30 |

- **`Simple365` 与 `Gregorian` 的唯一差别**：前者 `has_leap_year = false`。因此 `Simple365` **复用** `Gregorian` 的月长表，只在 `days_in_year` 与二月分支上不查闰年。
- **`Fixed360`**：完全独立，等价上一轮实现，全部走"恒 30/恒 360"分支。**上一轮全部数字（`100801` / `180360` 等）在 `Fixed360` 下逐字节不变**——已测试 `fixed360_reproduces_previous_round_numbers` 钉死，保证旧工程零迁移成本。
- **新增第四种历法的成本**：改 `days_in_year` + `days_in_month` 两个 match + 加一个 enum 变体，`civil`/`day_counter` **一行不动**。

> **为什么不做成 `trait CalendarRule { fn days_in_year(&self, y) -> i64; ... }` 让上层泛型化**：那会把 `<R: CalendarRule>` 传染到 `Calendar`、`Timeline`、`Project`、所有加载器。**收益（可扩展）当前为零，成本（泛型传染）立即发生**。否决。

### 2.5 交付稿中的**全部数字**（进 §7 测试决策表钉死）

| # | 数字 / 事实 | 来源 | 守护测试 |
|---|---|---|---|
| N1 | `1818/5/5` = `start_year=1818` 时的**第 125 天** | 用户 Q4 示例 | `user_anchor_1818_05_05_is_consistent` |
| N2 | `1840/1/1` = `start_year=1840` 时的**第 1 天** | 用户原话 S | `spec_start_1840_first_day_is_jan_first` |
| N3 | `2020/2/29` = 第 **60** 天；`2020/3/1` = 第 **61** 天 | 闰年 | `leap_year_february_29_round_trips` |
| N4 | 10 万年（2000→102000）= **36,524,250** 天 | 主理人表 | `span_of_100k_years_in_days_is_pinned` |
| N5 | i64 容量 ≈ **2.525e16 年**（25,252,734,927,766,556 年） | 主理人表 | `i64_capacity_far_exceeds_literary_span` |
| N6 | 10 万年占 i64 ≈ **4e-12**（万亿分之 3.96） | 主理人表 | `i64_capacity_far_exceeds_literary_span` |
| N7 | `Fixed360` 旧数字（`100801` / `180360` / `520/12/30`）**逐字节不变** | 上一轮 | `draft_literal_values_are_all_verified` + `fixed360_reproduces_previous_round_numbers` |
| N8 | 事件硬下界：`from=1` 与 `from=0` 被拒、`from=2` 合法 | 用户原话 | `event_must_be_strictly_after_start_zeroth_hour` |
| N9 | 跨度阈值 **1,000,000 天 ≈ 2738 年** | 本文 §5 | `span_limit_is_one_million_days` |
| N10 | 100 万天 = 上一轮 18 万格的 **5.556 倍** | 本文 §5 | `span_warning_is_degradation_not_gate` |
| N11 | 10 万年 = 3652 万格 = 18 万格的 **202.9 倍** | 主理人表 | `span_warning_is_degradation_not_gate`（注：主理人稿称"200 倍"，精确值为 202.9×，取整口径一致） |
| N12 | 示例 A：公历 200 年（1840→2040）= **73,050** 天 | 本文 §6.3 | `example_yaml_numbers_are_all_verified` |
| N13 | 示例 B：simple365 `365000` = **1000/12/31**；`1001/1/1` = **365,001** | 本文 §6.3 | `example_yaml_numbers_are_all_verified` |
| N14 | 示例 C：`180360` / `100801` 与上一轮一致 | 本文 §6.3 | `example_yaml_numbers_are_all_verified` |

---

## 3. M3：`TimePoint` / `TimeSpan` / `TimelineQuery` 是否需要改？

### 3.1 裁决：**不需要改。确认主理人判断。**

| 类型 | 上一轮形态 | 本轮 | 裁定 |
|---|---|---|---|
| `TimePoint` | `pub struct TimePoint(i64)` | **不变** | counter 仍是**绝对物理天数**，只是"解释它的规则"从常量变成 `Calendar.rule` |
| `TimeSpan` | `{from: i64, to: Option<i64>}` 半开 | **不变** | 同上 |
| `TimelineQuery` | `at(&[(TimeSpan,T)], i64)` / `in_range` | **不变** | 求值只看 counter，不看历法 |

**理由（三条，逐条硬）**：

1. **历法不改变"一天"这个物理量**。无论公历还是 360 历，`counter +1` 永远表示"过了一整天"，`Ord` 永远按数值——**排序语义与历法正交**。
2. **反例检验**：如果历法真的改变 counter 语义，那"改历法"就必须**全量迁移所有数据**。但用户 Q2 明说历法是工程级唯一的**显示规则**——作者改历法时，事件仍发生在"第 N 天"，只是那个第 N 天**改叫**另一个日期。**这正是 §2.2.3 那句"`TimePoint` 是绝对量、`Calendar` 是坐标系"的兑现**：换坐标系不改点的绝对位置。
3. **若强行把 `CalendarRule` 塞进 `TimeSpan`**：每个区间都要带一份历法，导致（a）YAML 冗余爆炸，（b）出现"同工程两个区间历法不一致"的伪 `Unknown` 校验态，直接违背 H1。**否决。**

> **唯一受影响处**：`TimePoint::date(calendar)` 的**签名不变**（本就要传 `&Calendar`），只是 `Calendar` 内部多了一个 `rule`。**调用方零改动。**

### 3.2 `TimePoint` / `TimeSpan`（已编译，与上一轮完全一致）

```rust
/// 时间轴上的一个位置。主表示 = 天数计数器（`counter >= 1`）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct TimePoint(i64);

impl TimePoint {
    pub const FIRST: TimePoint = TimePoint(1);

    pub fn from_counter(counter: i64) -> Result<Self, CoreError> {
        if counter < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter });
        }
        Ok(TimePoint(counter))
    }

    pub const fn from_counter_unchecked(counter: i64) -> Self {
        TimePoint(counter)
    }

    pub const fn counter(self) -> i64 {
        self.0
    }

    pub fn advance(self, days: i64) -> Result<Self, CoreError> {
        Self::from_counter(self.0 + days)
    }

    pub fn date(self, calendar: &Calendar) -> CivilDate {
        calendar.civil(self.0)
    }
}

impl std::fmt::Display for TimePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@day{}", self.0)
    }
}

/// 半开区间 `[from, to)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimeSpan {
    pub from: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<i64>,
}

impl TimeSpan {
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

    pub fn open(from: i64) -> Result<Self, CoreError> {
        if from < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter: from });
        }
        Ok(Self { from, to: None })
    }

    pub fn contains(&self, t: i64) -> bool {
        if t < self.from {
            return false;
        }
        match self.to {
            Some(end) => t < end,
            None => true,
        }
    }

    pub fn overlaps(&self, other: &TimeSpan) -> bool {
        let self_end_before_other = self.to.is_some_and(|b| b <= other.from);
        let other_end_before_self = other.to.is_some_and(|d| d <= self.from);
        !(self_end_before_other || other_end_before_self)
    }
}
```

> `is_some_and` 自 Rust **1.70** 稳定，满足工程 `rust-version = 1.80`。**我实测时另用过一个 1.82 才稳定的 `is_none_or`，已替换为 `map_or` 以守住 1.80 底线**（见 §7 自纠 #2）。**这是"必须实测"的价值：只写不编译会留下 MSRV 违规。**

---

## 4. M4：事件硬下界 + 故事实体（校验分流）

### 4.1 关键问题：`counter = 1` 的事件算不算合法？

**用户原话**："作者添加事件，填写日期必须检查必须**大于** 1840年1月1号 **0时0分**。"

**逐字拆解**：
- `counter = 1` 的定义就是"起点那天的 0 时"（即 `1840-1-1 00:00`）——这是 §2.2.3 `TimePoint::FIRST` 的定义。
- 用户要求事件 `> 起点 0 时`，即 `> counter 1`。
- **故 `counter = 1` 的事件不合法，最小合法事件日期是 `counter = 2`。**

**明确回答主理人的二选一**：不是 `counter >= 1`，是 **`counter > 1`（等价 `counter >= 2`）**。

> **与 `TimePoint::from_counter` 的关系**：`from_counter(1)` 仍**合法**（第 1 天本身是合法时间点，可用于锚点显示）；**只是它不能作为"事件起点"**。两件事必须分开——`from_counter` 管"这是不是合法时间点"，事件校验管"事件能不能发生在这"。**故不在 `TimePoint` 上收紧，而在 `Event` 校验层加规则。** 这是刻意的：收紧 `TimePoint` 会把锚点、spans 端点一并误伤。

### 4.2 校验代码（已编译）

```rust
/// 事件日期硬下界校验：`from` 必须 **严格大于** 起点 0 时。
///
/// 起点本身 `counter = 1` 就是"起点那天的 0 时"，
/// 用户要求事件**大于**起点 0 时 ⇒ `from == 1` **不合法**，最小合法 `from == 2`。
pub fn check_event_lower_bound(from: i64, start_day: i64) -> Result<(), CoreError> {
    if from <= start_day {
        return Err(CoreError::EventAtOrBeforeStart {
            day: from,
            start_day,
        });
    }
    Ok(())
}
```

对应错误变体（替换/新增进 `error.rs`）：

```rust
#[error("事件日期 {day} 未晚于时间轴起点 0 时（必须 > {start_day}）")]
EventAtOrBeforeStart { day: i64, start_day: i64 },
```

### 4.3 「故事」实体：**独立模块 `story.rs`，不与 `event.rs` 合并**

**技术裁决（创作语义归属设计侧，此处只给技术后果）**：

| 方案 | 技术后果 |
|---|---|
| **A. 独立 `story.rs` + 独立 `Story` 类型**（推荐） | ① 类型系统层面**不可能**给故事写 `day`——无字段可写；② 校验分流靠 `match` 一次，不必散落 `if is_story`；③ 模块边界清晰；④ 代价：多一个文件、`lib.rs` 多一行导出 |
| **B. 并入 `event.rs`，`Event.day: Option<i64>`** | ① 出现"事件有 day / 故事无 day"的 `Option` 语义污染——**`None` 到底是"故事"还是"忘填了"？** 无法区分，需再加 `kind` 字段；② 校验分流变成运行时判断，编译器不帮忙；③ 好处：文件少一个 |
| **C. 并入 `event.rs`，事件 `Event` + 故事 `Story` 两个 struct 同文件** | 折中：类型仍分离（好），但语义上"故事"与"事件"挤在一个模块（可接受，但 §4.1 文档边界会含糊） |

**我推荐 A**。理由：**A 把"故事没有日期"这条规范固化成类型系统的硬约束**——想违反都写不出来；B 把它降级成"运行时约定"，靠人记。对单人开发者，**能靠编译器兜住的规范就不要靠记忆**。

### 4.4 「故事」完整 Rust 定义（已编译）

```rust
//! 「故事」实体：**无日期**的叙事单元。

use crate::error::CoreError;
use crate::id::Id;
use crate::time::TimeSpan;

/// 故事。**无 day 字段**是规范要求，不是省事。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Story {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 可选：它想靠近的锚点（**仅用于显示定位，不赋予任何时间语义**）。
    ///
    /// 这只是"指向已有锚点的弱引用"——**不进 `TimePoint`/`TimeSpan`/`TimelineQuery`**，
    /// 不参与排序、不参与校验。加此字段**不破坏"故事无 day"的硬约束**。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<Id>,
}

/// 时间的**校验分流**枚举：有日期走 `Dated`，无日期走 `Undated`。
///
/// 这是"事件硬下界 vs 故事不检查"的唯一实现入口——
/// 校验器只需 `match` 一次，不必散落 `if is_story` 判断。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TimelineItem {
    /// 可排期（事件）：必须 `from > day_counter(start_year,1,1)`。
    Dated {
        #[serde(flatten)]
        span: TimeSpan,
    },
    /// 不可排期（故事）：无日期，恒通过时间校验。
    Undated { story: Story },
}

/// 统一校验入口：按 `TimelineItem` 分流。
pub fn validate_timeline_item(item: &TimelineItem, start_day: i64) -> Result<(), CoreError> {
    match item {
        TimelineItem::Dated { span } => check_event_lower_bound(span.from, start_day),
        // 故事无日期 ⇒ 恒通过。
        TimelineItem::Undated { .. } => Ok(()),
    }
}
```

> **`TimelineItem` 是"校验分流"的载体**，但**不是**新的存盘聚合：`Event` / `Story` 仍各自存各自文件，加载器把两者统一映射成 `TimelineItem` 后过一遍校验。这样**存盘格式保持"一实体一文件"**（§5.4 契约不变），只在**内存校验阶段**收敛。

> **✅ 与设计侧（游有方）已收敛——见 `_draft_calendar_you.md` §A2**：设计侧**独立**从创作语义得出同一结论「**故事无 day**」，并给出 4 条不依赖技术的理由（无"非法日期"错误类 / 不污染排序与查询 / 不破坏"往前不存在"契约 / 与前史区是同一机制）。**技术侧（我）与设计侧在无 day 上零分歧。** 我的类型定义（`Story` 无 `day` 字段 + `TimelineItem::Undated`）与设计侧 §A3 目录约定（独立 `stories/`）完全兼容，见 §7.3.

> **✅ 已收敛（设计侧 §B5 终裁采纳，我侧已实现并测试）**：设计侧 §A3 给故事加了**可选 `placements`（它想靠近的锚点 id 列表）**——故事无 `day`，但可在**对应锚点旁**显示便签。**`placements: Vec<Id>` 只是"指向已有锚点的弱引用"，不进 `TimePoint`/`TimeSpan`/`TimelineQuery`**，不参与排序、不参与校验，故**不违反无 day 的硬约束**。已并入上方 `Story` 定义（见 `placements` 字段），并由测试 `story_placements_round_trip_and_stays_dateless` 钉死（含"空 placements 不输出该键"）。

> **加此字段的技术后果**：`Story` 从"完全轴外"变成"可挂到锚点旁显示"。**仍然无 `day`、仍不进 `TimelineQuery`、仍不排序**——`placements` 是**展示层弱引用**，不是时间坐标。**模型硬约束零影响。**

### 4.5 两个方案（有 day / 无 day）的技术后果对照

| 维度 | 故事**有 `day`** | 故事**无 `day`**（推荐，**设计侧已独立确认**） |
|---|---|---|
| 排序 | 可参与时间轴排序，会与事件混排 | **不参与排序**，天然隔离 |
| 校验 | 需与事件走同一套硬下界（否则出现"故事能排到起点前"的漏洞） | **跳过校验**，与用户原话一致 |
| 导航 | 出现在 minimap / 语义跳转里 | 不进时间轴导航（只能靠搜索/标签） |
| 前史区兼容 | 若故事有 day 且早于起点，则**前史区（§1.9 方案 B）失去意义**——故事自己就能排到前面 | 与"前史区承载不可排期叙事"**完美吻合**：故事 = 前史区的天然实体化 |
| 数据迁移 | 无 | 无 |
| 技术复杂度 | 需 `Option<i64>` + `kind` 区分"无日期"与"未填" | 类型即规范，零运行时判断 |

> **技术侧倾向无 day**（与用户原话一致，且与上一轮"前史区不可排期"设计自洽）。**最终创作语义由你（游有方）裁定**——我只是指出：若选"有 day"，则前史区方案 B 的存在理由被削弱，且必须给故事**补一套**硬下界校验。

---

## 5. M5：跨度预警（渲染层，非 i64 溢出）

### 5.1 复核主理人的 i64 实测（我独立复算，全部吻合）

| 跨度 | 我算出的天数 | 占 i64 | 主理人值 | 结论 |
|---|---|---|---|---|
| 1 万年 | 3,652,425 | 3.96e-12 (≈4e-11 %) | 3,652,425 / ~4e-11% | ✅ 一致（口径差异见注） |
| **10 万年** | **36,524,250** | **3.96e-12 (≈4e-10 %)** | 同 | ✅ 一致 |
| 100 万年 | 365,242,500 | 3.96e-11 | 同 | ✅ 一致 |
| i64 上限 | — | — | **≈ 25,252,734,927,766,556 年** | ✅ 我算 2.52527e16，一致（2525 万亿年） |

> **口径注**：主理人写"10 万年 ~4e-10 %"，是**百分比**；换算成**比例**是 3.96e-12。两者是同一数——**4e-10 % = 4e-12**。我在测试里用"万亿分之 3.96"（=3.96e-12）表达，避免百分号歧义。**建议稿中统一写成"约 4×10⁻¹²（即 4×10⁻¹⁰ %）"**。

### 5.2 架构判断：**i64 溢出不是真瓶颈，主理人结论正确**

**我的确认**：i64 对任何文学创作跨度都是过剩的（10 万年仅占 4e-12）。**真正的瓶颈是 UI 渲染/导航**：

```text
上一轮：500 年 × 360 天 = 180,000 格 —— 已判定"头号交互痛点"
本轮：  10 万年 = 36,524,250 天 —— 1 天 1 像素 = 3652 万格 = 上一轮的 **202.9 倍**
```

**3652 万格在 1200px 屏需 30436 屏横向滚动。** 这不是"滚动慢"的问题，是**滚动这个方法本身失效**——必须靠 minimap + 语义锚点跳转。**故预警由渲染层发出、与整数容量解耦，是正确架构。**

### 5.3 阈值、判层、门禁/降级 —— 三问逐条答

| 问题 | 我的裁定 |
|---|---|
| **阈值卡在哪？** | **1,000,000 天（≈2738 年）**。依据：上一轮 18 万格是"已可承受的痛点上限"；100 万格 = 18 万格的 **5.56×**，即使 minimap + 三层导航也已进入"必须骨架占位"区。取整为 100 万，便于作者理解与文档表达。 |
| **由哪一层判定？** | **`wv-app`（渲染层）**。判定发生在**渲染前**（拿到 `timeline_range` 与视图宽度后）；`wv-core` / `wv-io` **不判**——它们不知道屏幕多宽。 |
| **是门禁还是降级？** | **降级（警告但继续）**。返回 `Some(SpanWarning)`，调用方**照常加载**，只是 UI 挂横幅 + 切骨架占位渲染 + 强制启用 minimap。**从不阻断。** |

### 5.4 与 §2.7.3 `last_day` 门禁的**关系：两回事**（主理人明确问了）

| 维度 | `timeline_range.last_day`（§2.7.3） | **跨度预警（本轮 M5）** |
|---|---|---|
| **约束对象** | 作者声明的范围 **vs** 实际引用的实体 | 范围**本身**的宽度 |
| **问题性质** | **静态数据合法性**（越界实体 = 数据错） | **体验问题**（合法但画不出来） |
| **判定层** | `wv-io` 加载器 | `wv-app` 渲染层 |
| **判定时机** | 加载期 | 渲染前 |
| **处置** | **门禁：拒绝加载并报错** | **降级：警告但继续** |
| **为什么无运行时降级** | 截断到 `last_day` 会**静默篡改作者数据** | —— 本就有降级动作（骨架占位），**不需要门禁** |
| **阈值** | `counter > last_day` | `last_day - first_day > 1,000,000` |

> **一句话**：`last_day` 门禁管"**数据有没有越界**"，跨度预警管"**范围宽不宽到画不出来**"。两者**阈值不同、判层不同、处置相反**。**互不替代，也不冲突**——一个工程可以"数据全合法"但"跨度超预警"（如作者诚实地标了 10 万年）。

### 5.5 实现（已编译，`crates/wv-app/src/span.rs`）

```rust
//! 跨度预警：**渲染/体验层**判定，与 i64 整数容量解耦。

use crate::error::CoreError;

/// 建议的单轴跨度上限（天数）。= **1,000,000 天 ≈ 2738 年**。
pub const RECOMMENDED_MAX_SPAN_DAYS: i64 = 1_000_000;

/// 跨度预警判定。**降级语义**：返回 `Some(Warning)` 时调用方**继续**。
pub fn check_span(
    first_day: i64,
    last_day: i64,
) -> Result<Option<SpanWarning>, CoreError> {
    let days = last_day - first_day;
    if days < 0 {
        return Err(CoreError::InvalidSpan {
            from: first_day.to_string(),
            to: last_day.to_string(),
        });
    }
    if days <= RECOMMENDED_MAX_SPAN_DAYS {
        return Ok(None);
    }
    Ok(Some(SpanWarning {
        days,
        approx_years: days as f64 / 365.2425,
        limit: RECOMMENDED_MAX_SPAN_DAYS,
    }))
}

/// 跨度预警内容（供 UI 渲染横幅文案）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanWarning {
    pub days: i64,
    pub approx_years: f64,
    pub limit: i64,
}

impl SpanWarning {
    pub fn message(&self) -> String {
        format!(
            "时间轴跨度约 {:.0} 年（{} 天），超过建议上限 {} 天；\
             渲染将切换为骨架模式，请用语义锚点导航。",
            self.approx_years, self.days, self.limit
        )
    }
}
```

> **注意**：这是**降级类**预算，按架构师纪律本可直接进表；但**我仍然给了它可执行动作**（骨架占位 + 横幅 + 强制 minimap），且**动作里写了真实存在的渲染分支**（`SpanWarning::message` 的文案 + 渲染层切骨架）。**没有虚构机制。**

### 5.6 关于"提示作者跨度太大"的体验细节

- **仅在创建/修改 `timeline_range` 时弹一次**，不在每次打开工程时骚扰；
- 弹窗给**两个可执行选项**：①"我知道了，按骨架模式打开"（继续）②"我要改小范围"（跳转到范围编辑）；
- **不自动截断、不自动改作者数据**（与 §2.7.3 门禁的"不静默篡改"原则一致）。

---

## 6. M7：`axis.yaml` schema 与新 YAML

### 6.1 Rust 侧结构（`Timeline` 增量）

```rust
/// 时间轴定义。对应 `timeline/axis.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Timeline {
    /// 历法坐标系：起点名 + 起点年 + 历法规则 + 年号对照。
    #[serde(flatten)]
    pub calendar: Calendar,
    /// 工程关心的天数范围。
    pub timeline_range: DayRange,
    /// 关键时间点，用于 UI 跳转。
    #[serde(default)]
    pub anchors: Vec<Anchor>,
}

/// 天数范围。`last_day: None` 表示"开放至今"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DayRange {
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

> **新增字段**：`calendar.rule`（`CalendarRule`）与 `calendar.reign_aliases`（`Vec<ReignAlias>`）。其余与上一轮一致。

### 6.2 年号对照的 YAML 位置 —— 设计裁决

**主理人问**：`anchors` 上加 `alias: 嘉庆二十三年四月初一`？还是独立 `eras`/`reigns` 表？

**裁决：独立的顶层 `reign_aliases` 列表，挂在 `Calendar` 下（`#[serde(flatten)]` 后即在 `axis.yaml` 顶层）。**

| 方案 | 问题 |
|---|---|
| ❌ `anchors[].alias` | 锚点是"UI 跳转点"，年号是"日期区间的写法"。**语义不同**：一个年号可能跨很多天（不只是一个锚点）；把年号塞进锚点会强迫作者为每段年号建一个锚点，**污染锚点列表** |
| ❌ 独立的 `eras` 表（复用上一轮已删结构） | `eras` 承载的是"参与运算的纪元"，而年号**不参与任何运算**（Q1 确认）。复用 `eras` 会**误导**作者以为年号有运算语义 |
| ❌ 设计侧原稿 `display_calendar: {kind, epoch_label, epoch_year, era_table}` | 把已存在的 `prefix`/`start_year` **换名重开一套** —— 正是"为同一语义造两个概念"。**设计侧已主动撤回**（见下方收敛记录） |
| ✅ **独立顶层 `reign_aliases`**（挂在 `Calendar` 下，`#[serde(flatten)]` 即在 `axis.yaml` 顶层） | ① 语义清晰："一段公历区间 ↔ 一个手填写法"；② 不参与运算，读取只在 `format()` 里做显示查找；③ 天然支持"一个年号跨多年"；④ 与 `anchor` 解耦；⑤ **与已有的 `prefix`/`start_year` 同属一个坐标系，零多余概念** |

**✅ 与设计侧已收敛（`_draft_calendar_you.md` §B5 终裁）——「完整 schema + 最小 UI」**：

| 议题 | 终裁 |
|---|---|
| YAML 键名 | **采纳我的**：`Calendar{prefix, start_year, rule, reign_aliases}`。设计侧主动撤回 `display_calendar` 那套换名方案 |
| 年号表形态 | **合并两侧**：内置静态史实（支撑年份级硬校验）**+** 天数计数器区间（工具换算）**+** 作者手填文本 |
| UI 推进 | **完整 schema + 最小 UI**：阶段一存盘按完整七字段定死，UI 从"只让作者手填 `lunar_label`"起步，逐步加下拉/校验，**全程不改数据** |

**最终七字段 `ReignAlias`**：

```rust
/// 年号对照。**仅显示用**——不参与排序、不参与区间求值。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReignAlias {
    /// 【内置静态史实】年号名，如 `嘉庆`。工具提供下拉候选，**不自动填公历**。
    pub era_name: String,
    /// 【内置静态史实】朝代，如 `清`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynasty: Option<String>,
    /// 【内置静态史实】该年号的公历起年（含），如 1796。用于**年份级硬校验**。
    pub gregorian_start: i32,
    /// 【内置静态史实】该年号的公历止年（含），如 1820。
    pub gregorian_end: i32,
    /// 【工具换算】本对照在时间轴上的天数计数器起点（含）。
    pub start: i64,
    /// 【工具换算】终点（不含）；省略 = 至今。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    /// 【作者手填】农历写法，如 `嘉庆二十三年四月初一`。**工具绝不解析此字段**。
    pub lunar_label: String,
}
```

**YAML 结构**：

```yaml
reign_aliases:
  - era_name: 嘉庆
    dynasty: 清
    gregorian_start: 1796      # 嘉庆元年 = 公元 1796（静态史实）
    gregorian_end: 1820
    start: 125                 # 天数计数器（含）——由工具按公历换算
    end: 490                   # 天数计数器（不含）；省略 = 至今
    lunar_label: 嘉庆二十三年四月初一   # 作者手填，工具不解析
```

> **工具只做两级、不越界**（与设计侧 §B3 口径一致）：
> - **年份级**：拿 `gregorian_start/gregorian_end`（静态史实）**硬校验**作者填的公历年份是否落在年号期内 → **自洽则通过，否则报错（红）**。这一步**不需要农历算法**，只是比较两个公历年份。
> - **月日级**：`lunar_label` 里的"四月初一"**工具绝不推算**，只做显示 → 若与公历日期可能不符，**仅警告（黄），不报错**。**仅月/日级换算才需要农历算法（用户已否决）。**
>
> **年号表维护边界（接受设计侧的定界）**：表**只覆盖到清**（封闭、不增长的史实集合）；**仅下拉候选、不自动填公历**；表**可以是外部数据文件**，作者可扩展，**工具不背维护包袱**。
>
> **工具的显示职责**：当 `counter` 落在 `[start, end)` 时，把 `lunar_label` 作为**注释**附在公历日期后显示（`format()` 内查找）。
>
> **未来的自动月日换算（如真需要）**：属**阶段五+ 可选增强**，需引入农历数据表（体积 + 版权 + 精度三重成本）。**当前明确不做**——与用户 Q1 一致。

### 6.3 三个完整示例 YAML（每种历法一份）

#### 示例 A：公历（现实小说）

```yaml
# axis.yaml —— 历法：公历（真实格里高利历，含闰年）
# 内部主表示是"天数计数器"：第 1 天 = 1840 年 1 月 1 日。
prefix: 公元
start_year: 1840
rule: gregorian

timeline_range:
  first_day: 1
  last_day: 73050        # = 公元 2040 年 1 月 1 日（200 年，含 48 个闰日）

# 年号对照（仅显示；年份级硬校验，月日不推算）
reign_aliases:
  - era_name: 嘉庆
    dynasty: 清
    gregorian_start: 1796      # 静态史实：嘉庆元年 = 公元 1796
    gregorian_end: 1820
    start: 1                   # 天数计数器（含）——示例占位
    end: 125                   # 天数计数器（不含）
    lunar_label: 嘉庆二十三年四月初一  # 作者手填，工具不解析

anchors:
  - id: anchor_opium_war
    name: 鸦片战争
    day: 1               # = 公元 1840 年 1 月 1 日（示例占位，请作者按实际日期改）
```

#### 示例 B：简单 365 天历（架空但用公历式月日）

```yaml
# axis.yaml —— 历法：简单 365 天历（无闰年）
# 一年恒 365 天，月长取公历非闰年值（1月31天…2月28天…）。
prefix: 新历
start_year: 1
rule: simple_365

timeline_range:
  first_day: 1
  last_day: 365000      # = 新历 1000 年 12 月 31 日（365 × 1000，1000 年最后一天）

anchors: []
```

#### 示例 C：360 天 / 30 天月（奇幻，等价上一轮）

```yaml
# axis.yaml —— 历法：360 天/年、30 天/月（奇幻默认，等价上一轮实现）
# 内部主表示是"天数计数器"：第 1 天 ≡ 无仙 20 年 1 月 1 日。
prefix: 无仙
start_year: 20
rule: fixed_360

timeline_range:
  first_day: 1
  last_day: 180360      # = 无仙 520 年 12 月 30 日（恰满 500 年）；开放范围可整行省略

anchors:
  - id: anchor_founding
    name: 青云城建城
    day: 100801         # = (300-20)*360 + 1
```

> **示例 A 的 `last_day: 73050` 与示例 C 的 `180360`、`100801` 全部进测试决策表**（§7）——上一次教训就是"散文数字无人守护"。**本稿所有 YAML 里的数字，我都在测试里钉了一遍。**
>
> ⚠️ 示例 A 的 `day: 1` 锚点是**占位**（真实鸦片战争日期需作者手填，且它早于事件硬下界，**只能作为锚点不能作为事件**）。**这是刻意展示**："锚点不受事件硬下界约束"——见 §4.1 的边界说明。

### 6.4 兼容性：默认 `fixed_360` 保旧工程

`Calendar.rule` 有 `#[serde(default)]` ⇒ **缺 `rule` 字段的 YAML 自动按 `fixed_360` 解析**。这意味着：**上一轮 `Fixed360` 工程的 `axis.yaml` 加不加 `rule: fixed_360` 都能读**。但 `reign_aliases` 缺省为空、`Timeline` 其他字段不变 ⇒ **纯 360 工程可零改动升级**（前提：`schema_version` 也升级，见 §7.3）。

---

## 7. M6/M7：校验、`schema_version`、骨架差异、验收清单

### 7.1 `error.rs` 增量（替换上一轮 4 个变体，字段加 `max`）

```rust
#[error("天数计数器 {counter} 非法：时间轴只有起点、往前不存在，counter 必须 >= 1")]
DayCounterOutOfRange { counter: i64 },

#[error("年份 {year} 早于时间轴起点 {start_year}：起点之前不存在")]
BeforeTimelineStart { year: i64, start_year: i32 },

#[error("月份 {month} 非法：取值 1..={max}")]
InvalidMonth { month: i64, max: i64 },

#[error("日期 {day} 非法：取值 1..={max}")]
InvalidDay { day: i64, max: i64 },

#[error("事件日期 {day} 未晚于时间轴起点 0 时（必须 > {start_day}）")]
EventAtOrBeforeStart { day: i64, start_day: i64 },

#[error("时间轴跨度 {days} 天（约 {years} 年）超过建议上限 {limit} 天：渲染/导航将不可用")]
TimelineSpanTooLarge { days: i64, years: i64, limit: i64 },

#[error("时间区间非法: from={from} to={to}")]
InvalidSpan { from: String, to: String },
```

> **`InvalidMonth` / `InvalidDay` 为何加 `max`**：上一轮 `day` 是固定 `1..=30`，错误信息可以写死；本轮 `max` 随历法与年份变化（2 月 28/29/30），错误信息**必须带上真实上限**，否则作者看到"日期 31 非法"却不知本历法该月到底几天。**这是一处必须改的连带项。**

### 7.2 `schema_version` 裁定：**必须 `2 → 3`**

| 选项 | 说明 | 裁定 |
|---|---|---|
| 保持 `2` | 上一轮刚升的 2 无法区分"360 硬编码"与"可插拔历法"两代文件——一个无 `rule` 的 v2 文件到底是"v2 的 360 语义"还是"v3 缺省的 360"？**二义** | ❌ |
| **升 `3`** | `CURRENT_SCHEMA_VERSION = 3`。读到 `schema_version: 2` 时给出提示："时间模型已支持可插拔历法，`axis.yaml` 可补 `rule` 字段（缺省 fixed_360），`reign_aliases` 为新增可选字段" | ✅ |

**理由（关键）**：虽然 `rule` 有 `#[serde(default)]` 让 v2 文件**技术上能读**，但：
1. **"能读"不等于"语义相同"**：v2 文件的隐含历法语义是"世界上唯一历法 = 360"，v3 的 `fixed_360` 是"三种历法之一的缺省值"。**这是语义变更，不是纯增量。**
2. **`reign_aliases` 是新增能力**：v2 文件没有这个字段，v3 加载器新增了显示逻辑。版本号要反映**能力的引入**。
3. **版本号是最便宜的沟通**：升一个号 + 一句迁移提示，比"作者困惑为什么 v2 文件读出来没有年号功能"便宜得多。

> **不自动迁移**：v2 → v3 **无需改文件**（`rule` 缺省即 `fixed_360`，`reign_aliases` 缺省即空）。**故本轮的迁移是"零操作"**——只需加载器**接受** v3 并**提示** v2 作者"你的文件仍是 360 语义，无需改动"。**这与上一轮 1→2 的"硬迁移"性质不同。**（上一轮旧文件用 `{era,year,day}`，格式不兼容无法读；本轮 v2 文件格式**完全可读**。）

### 7.3 对骨架的差异提示（**§2.9 增量，对照上一轮 15+11 条**）

#### 7.3.1 上一轮差异清单**新增/变更**的条目

| # | 文件 | 上一轮（v0.2 首稿→重做稿） | **本轮增量** |
|---|---|---|---|
| **+16** | `crates/wv-core/src/calendar.rs` | （不存在） | **新建**：`CalendarRule` / `Calendar` / `CivilDate` / `ReignAlias` / `is_leap_year` |
| **+17** | `crates/wv-core/src/story.rs` | （不存在） | **新建**：`Story` / `TimelineItem` / `check_event_lower_bound` / `validate_timeline_item` |
| **+18** | `crates/wv-core/src/span.rs`（或 `wv-app`） | （不存在） | **新建**：`RECOMMENDED_MAX_SPAN_DAYS` / `check_span` / `SpanWarning` |
| **改-16** | `crates/wv-core/src/time.rs` | 上一轮条目 1–15 | **只删掉 `DAYS_PER_YEAR`/`DAYS_PER_MONTH`/`MONTHS_PER_YEAR` 三个常量与 `Calendar`/`CivilDate`（迁往 `calendar.rs`）**；`TimePoint`/`TimeSpan`/`TimelineQuery` **代码不变** |
| **改-17** | `crates/wv-core/src/error.rs` | 上一轮 4 个变体 | `InvalidMonth`/`InvalidDay` **加 `max` 字段**；**新增** `EventAtOrBeforeStart` / `TimelineSpanTooLarge` |
| **改-18** | `crates/wv-core/src/lib.rs` | `pub use time::{...}` | **追加** `pub use calendar::{Calendar, CalendarRule, CivilDate, ReignAlias};` 与 `pub mod story;`；`World` 聚合加 `stories: Vec<Story>` |
| **改-19** | `crates/wv-io/src/lib.rs` | `CURRENT_SCHEMA_VERSION = 2` | **改 `3`** |
| **改-20** | `crates/wv-io/src/project.rs` | 上一轮差异 | 加载 `axis.yaml` 时按 `calendar.rule` 分发换算；加载 `stories/` 目录 |
| **改-21** | `examples/minimal-world/timeline/axis.yaml` | 上一轮新示例 | **追加** `rule: fixed_360`；可选加 `reign_aliases` |
| **+22** | `examples/minimal-world/timeline/stories/*.yaml` | （不存在） | **新建**示例故事文件，演示"无日期实体" |
| **改-23** | `crates/wv-io/tests/example_project.rs` | 上一轮重写 | 更新 `Timeline` 构造：加 `rule` / `reign_aliases`；加 `Story` round-trip 测试 |

#### 7.3.2 骨架改动总账（新口径）

| 类别 | 上一轮 | 本轮增量 | 合计 |
|---|---|---|---|
| `wv-core/src/time.rs` | 改 15 条 | 再改 1 条（迁走常量） | 16 |
| 其他文件 | 改 11 条 | 再改 5 条（+新建 3 文件） | 16 + 3 新建 |
| **新建文件** | 0 | **`calendar.rs` / `story.rs` / `span.rs`** | 3 |

### 7.4 阶段一验收清单 B 组：**B1–B10 → B1–B18**

上一轮 B 组为 B1–B10。本轮**新增 8 条**（历法换算、闰年、故事实体、硬下界、跨度预警、三历法 round-trip、年号七字段、故事 placements），并把 B5/B6/B8 的**历法相关断言泛化到三历法**。

| 编号 | 内容 | 新增/改写 | 守护测试 |
|---|---|---|---|
| B1 | 四态 `VerificationState` | 不变 | （距离维度，本轮不动） |
| B2 | 半开区间不变式 `[from,to)` | 不变 | `half_open_span_excludes_its_end` / `adjacent_spans_do_not_overlap` |
| B3 | `last_day` 缺省 round-trip | 不变 | （io 层） |
| B4 | 排序 = 天数计数器 | 不变 | `ordering_is_day_counter_ordering` |
| B5 | 换算恒等式（**泛化到三历法**） | **改写** | `conversion_round_trips_over_full_range_{gregorian,simple365,fixed360}` |
| B6 | 起点语义（**三历法各测**） | **改写** | `spec_start_1840_first_day_is_jan_first` 等 |
| B7 | 下界拒绝（`counter<1`） | 不变 | `counter_below_one_is_rejected` |
| B8 | 越界年月日拒绝（**按历法给 max**） | **改写** | `error_variants_are_reachable` |
| B9 | 历法常量**可配**（**反向验收，推翻上一轮 B9**） | **改写** | `days_in_year_by_rule` / `calendar_yaml_round_trips_all_three_rules` |
| B10 | `TimePoint` 单字段 | 不变 | `timepoint_is_single_field_newtype` |
| **B11** | **闰年规则精确**：4 闰 / 100 不闰 / 400 闰；含 0 年与负年 | **新增** | `gregorian_leap_year_rule_is_exact` |
| **B12** | **公历二月边界**：闰年 29 天、平年 28 天；`2021/2/29` 被拒 | **新增** | `leap_year_february_29_round_trips` / `days_in_month_gregorian_february` |
| **B13** | **故事实体无日期**：`Story` 无 `day` 字段；`Undated` 恒通过校验 | **新增** | `story_skips_lower_bound_check` |
| **B14** | **事件硬下界**：`from=1` 与 `from=0` 被拒、`from=2` 合法 | **新增** | `event_must_be_strictly_after_start_zeroth_hour` |
| **B15** | **跨度预警是降级非门禁**：阈值 100 万天；18 万格不报、10 万年报且不阻断 | **新增** | `span_warning_is_degradation_not_gate` / `span_never_used_as_gate` |
| **B16** | **三历法 YAML 三示例均可 round-trip** | **新增** | `calendar_yaml_round_trips_all_three_rules` |
| **B17** | **年号七字段 round-trip + 年份级硬校验**：`gregorian_start..=gregorian_end` 含边界 | **新增** | `reign_alias_seven_fields_round_trip` / `reign_alias_year_level_validation` / `reign_alias_shows_only_as_annotation` |
| **B18** | **故事 placements**：round-trip 稳定、空时不输出、**YAML 无 `day` 字段** | **新增** | `story_placements_round_trip_and_stays_dateless` |

**B 组项数：10 → 18。**

> **上一轮 B9 "历法常量不可配"被本轮彻底推翻**——这是 M1 的直接后果。**旧稿 B9 → 新稿 B9，理由：用户要求三种历法，常量不可配的前提消失。**

### 7.5 封版判定（改写）

```text
A1–A5 + B1–B18 + C1–C4 + D1–D5 + E2/E3 + F1–F4 全绿
```

---

## 8. 转待裁（不阻塞，但需明确）

| # | 事项 | 我的倾向 | 需要谁定 |
|---|---|---|---|
| **U5** | 天文记年 vs 历史纪年（`year=0` 是"1 年"还是"公元前 1 年"） | 天文记年（0 年是闰年），UI 层做映射 | 设计侧 + 用户 |
| **U6** | 跨度阈值 100 万天是否合适 | 建议先按 100 万天，**实机测 UI 后再调** | 设计侧 + 用户 |
| **U7** | 故事是否进时间轴导航（有 day / 无 day） | ✅ **已收敛：设计侧独立确认「无 day」**（见 §4.5 与 `_draft_calendar_you.md` §A2） | ~~设计侧~~ **已定** |
| **U8** | `reign_aliases` 是否阶段一就做 | 建议**阶段一先留字段、UI 后补**（存盘结构现在就定，避免二次迁移） | 主理人 |
| **U9** | **历法枚举命名统一** | ✅ **已收敛（you-youfang-2 §B5 终裁）**：enum 名 = **`CalendarRule`**（采纳我的）；值 = `Gregorian` / `Simple365` / `Fixed360`；tag = **`gregorian` / `simple_365` / `fixed_360`**（采纳我的下划线式 + 我的实测坑）；**`custom` 取消**（"现实小说 = 公历 + 年号"由 `Gregorian + reign_aliases` 覆盖，不需独立变体；真"自定义月表"才走 §1.2 翻案抽 trait） | **已定** |
| **U10** | **年号方案取哪档** | ✅ **已收敛**：采用**七字段合并 schema + 最小 UI**（`era_name`/`dynasty`/`gregorian_start`/`gregorian_end` 静态史实 + `start`/`end` 工具换算 + `lunar_label` 作者手填）；YAML 键名采纳我的 `Calendar{prefix,start_year,rule,reign_aliases}`（设计侧撤回 `display_calendar` 换名方案）；年号表**只覆盖到清、仅下拉候选不自动填、可为外部文件** | **已定** |
| **U11** | 故事是否加 `placements` | ✅ **已收敛：设计侧采纳，我侧已实现并测试**（`Story.placements: Vec<Id>` 展示层弱引用，不进 `TimePoint`/`TimeSpan`/`TimelineQuery`，不破坏无 day） | **已定** |

---

## 9. 验证证据汇总（实测，非自述）

| 门槛 | 结果 |
|---|---|
| `cargo test` | ✅ **31 passed / 0 failed**（4 单元 + 27 集成） |
| `cargo clippy --all-targets -- -D warnings` | ✅ **零告警** |
| `cargo fmt --all -- --check` | ✅ **干净** |
| 独立脚本复核（Python，非 Rust 测试） | ✅ 主理人 4 项 i64 数字全部吻合；3652 万格 = 18 万格 **202.9×** |
| 编译环境 | `rustc 1.97.1 / cargo 1.97.1 / edition 2021 / MSRV 1.80` |

### 9.1 本轮自纠记录（"实测"抓到的真实问题）

| # | 问题 | 若不做实测的后果 |
|---|---|---|
| **1** | `Fixed360` 在 `rename_all="snake_case"` 下序列化为 `fixed360`（数字不产生词边界），**YAML tag 与文档不符** | 作者按文档写 `rule: fixed_360` 会**解析失败** |
| **2** | 我一度用了 `Option::is_none_or`（Rust **1.82** 才稳定），**低于工程 MSRV 1.80** | 在 1.80 环境直接编译失败 |
| **3** | 我手写"1818/5/5 = 第 124 天"**算错**，实际 **125**（31+28+31+30+5） | **这正是上一轮同类错误的复现**——证明"散文数字必须进测试"的纪律是对的。测试当场报红，我改的是**散文里的数**，不是代码 |
| **4** | 我手写示例 B"`365000` = 1000 年整 / `1001/1/1` = 365000"，**又算错**（实际 `1001/1/1` = **365001**；`365000` 是 **1000/12/31**） | 同一类错误**在同一轮内出现两次**——再次证明"人写数字必错、必须测试守护"。两次都被 `example_yaml_numbers_are_all_verified` 拦下 |

> **#3 与 #4 与上一轮 `last_day` 错误同源**：都是"人写数字时心算错"。**本轮的不同是：测试先红了，错误没进稿子。** 这就是把数字纪律工具化的价值。
> **强观察**：本轮我在**散文/示例里手写了 4 个数字，错了 2 个（#3、#4），错误率 50%**。这不是我不小心，是**人类手算的固有错误率**。结论：**任何未进测试的数字都不该出现在交付稿里**（已固化为 §7.4 B 组的强制守护）。

---

## 10. 给主理人的汇编指引

| 覆盖目标 | 用本文哪一节 |
|---|---|
| `DESIGN_DELIVERY_v0.2.md` §0.2（规格输入） | 本文 §0 前提假设 + §2.5 数字表 |
| §2.2（TimePoint 与历法） | 本文 §1 / §2 / §3 |
| §2.3（axis.yaml schema） | 本文 §6 |
| §2.9（骨架差异） | 本文 §7.3 |
| §3.1（裁定表） | 本文 §0 速览表 + §5.4 |
| §1.5（跨尺度时间轴） | 本文 §5.2 / §5.3 |
| §2.5（验收清单 B 组） | 本文 §7.4 |
| §5（团队状态） | 本文 §9 验证证据 |

**需要主理人裁定的**：§8 的 U5–U8（转待裁）。
**需要设计侧（游有方）输入的**：§4.5 故事「有 day / 无 day」的创作语义归属。
