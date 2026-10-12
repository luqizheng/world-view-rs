# ADR-0006：历法支持真公历（Calendar 双历制）

**状态**：已接受（Accepted）（2026-01 用户裁定，见「处境」）
**日期**：2026-01
**关联**：基线 §2.16（时间维度）、`crates/wv-core/src/time.rs`、样例 `examples/xinhai-1911/`、P3/P7

---

## 处境（Context）

辛亥样例（`examples/xinhai-1911/`）需要表达**真实历史日期**：1911-05-09 铁路国有、1911-09-07 成都血案、1911-10-10 武昌起义、1912-02-12 清帝退位。用户裁定「公历，这不是幻想世界」。

现行 `Calendar`（基线 §2.16）只有两个常量 `days_per_year`（默认 360）与 `days_per_month`（默认 30）：

- 无法表达真实月长（28/30/31）；
- 无闰年（1912 年有 2 月 29 日）；
- 若强行用 360/30 近似，「12 月 31 日」会溢出成「13 月 1 日」，且动画时间跨度与史实差约 2%。

而 `TimePoint` 是**纯天数计数器**（1-based，半开区间），这一核心形态**不动**——历法只是显示层派生。

## 决策（Decision）

`Calendar` 增加判别字段 `kind`（`#[serde(default)]`，缺省 `simple`，旧 YAML 完全兼容）：

```yaml
# 旧（继续有效）
calendar: { prefix: 无仙, days_per_year: 360, days_per_month: 30 }

# 新：真公历
calendar: { kind: gregorian, prefix: 公元, start_year: 1900 }
```

- `CalendarKind::Simple`：现行行为，`days_per_year` × `days_per_month` 整除历法。
- `CalendarKind::Gregorian`：**proleptic Gregorian**（外推公历，支持任意年份，含 1912 闰年）。
  - 月长表 `[31,28,31,30,31,30,31,31,30,31,30,31]`；
  - 闰年规则：`y%4==0 && (y%100!=0 || y%400==0)`，闰年 2 月 29 天；
  - 计数器锚点：**第 1 天 = `start_year` 年 1 月 1 日**（与 Simple 的 `start_year` 语义一致）；
  - 换算用 Howard Hinnant 的 `days_from_civil` / `civil_from_days` 纯整数算法，**不引入 chrono**（见「被否决的备选」）。
- `to_ymd` / `to_day` / `format` 按 `kind` 分派；`days_per_year` / `days_per_month` 在 Gregorian 下被忽略（serde 仍接受，便于同字段集往返）。

## 理由（Rationale）

1. **计数器形态不变**（P7）：数据只有一个真相（天数计数器），公历只是另一种显示换算纯函数——这正是基线 §2.16「历法不进数据模型」的自然延伸，不是推翻。
2. **零破坏**：`kind` 缺省 `simple`，minimal-world 等既有工程 YAML 与测试一字不改。
3. **日期自洽可逆**：`to_day(to_ymd(d)) == d` 必须成立，边界测试覆盖闰年 2/29、年界 12/31→1/1。
4. **依赖纪律**（AGENTS.md §9）：`wv-core` 只允许 `serde`/`thiserror`；Hinnant 算法约 20 行纯整数运算，胜过引入 chrono（会把日期语义、时区、格式化一整坨拖进数据层）。

## 代价 + 被否决的备选（Consequences）

| 备选 | 否决理由 |
|---|---|
| 360/30 近似 + 登记待决（原方案 C） | 用户明确「这不是幻想世界」；12/31 溢出成 13 月是硬伤 |
| 引入 `chrono`/`time` crate | 违反 wv-core 依赖白名单（§2.1）；仅需要两个纯函数不值得 |
| 给 `Event`/`Anchor` 直接加 `ymd` 字段 | 制造「两个真相」，违反基线 §2.16 第 1 条（P7） |

**负面代价**：
- `time.rs` 增加约 120 行（枚举 + 换算 + 边界测试）；
- 作者在 Gregorian 工程里写 `days_per_year` 会被静默忽略——由 `wv-io` 校验面板以 `Hint` 提示（不阻止，§7.5 V1）。
