//! 跨度预警：**渲染/体验层**判定，与 i64 整数容量解耦。
//!
//! 架构判断（交付稿 §M5）：
//! - i64 对任何文学跨度都是过剩的（10 万年仅占 4e-12），**溢出不是真瓶颈**。
//! - 真正先崩的是 **UI 渲染/导航**：1 天 = 1 像素时，10 万年 = 3652 万格，
//!   是上一轮 18 万格痛点的 **202.9 倍**。
//! - 故预警由 **渲染层** 发出，**是降级（警告但继续）**，不是门禁。
//!
//! 与 `timeline_range.last_day`（§2.7.3 门禁类）的**关系：两回事**：
//! - `last_day` 门禁 = "作者声称的范围" vs "实际引用的实体" 的**静态数据合法性**，
//!   卡在加载期，由 `wv-io` 判定，不达标 = 拒绝加载。
//! - 本预警 = "范围本身虽然合法，但宽到画不出来" 的**体验问题**，
//!   卡在渲染期，由 `wv-app` 判定，不达标 = 显示骨架占位 + 横幅提示，**不拒绝**。

use crate::error::CoreError;

/// 建议的单轴跨度上限（天数）。= **1,000,000 天 ≈ 2738 年**。
///
/// 取值依据（见交付稿 §M5 算账表）：
/// - 上一轮 18 万格已判定为"头号交互痛点"；
/// - 30 万格 = 18 万格的 1.67×，仍在三层导航 + LOD 可承受区；
/// - **100 万格 = 5.56×**，1 天 1 像素下 1200px 屏需 833 屏横向滚动，
///   即使有 minimap 也超出"语义锚点跳转"能救的范围 ⇒ 取此为上限。
///
/// **这是产品阈值，不是物理阈值**；若要放宽，改此常量 + 改对应测试即可。
pub const RECOMMENDED_MAX_SPAN_DAYS: i64 = 1_000_000;

/// 跨度预警判定。**降级语义**：返回 `Some(Warning)` 时调用方**继续**，
/// 只是在 UI 上挂"跨度超出建议值"横幅并切到骨架占位渲染。
///
/// 与门禁的区别：本函数**从不**返回 `Err` 去阻断加载；`Err` 仅保留给
/// `days < 0`（调用方算错，属编程错误）。
pub fn check_span(first_day: i64, last_day: i64) -> Result<Option<SpanWarning>, CoreError> {
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
    /// 近似年数（仅用于文案展示，不参与任何运算）。
    pub approx_years: f64,
    pub limit: i64,
}

impl SpanWarning {
    /// 给作者看的一句话。
    pub fn message(&self) -> String {
        format!(
            "时间轴跨度约 {:.0} 年（{} 天），超过建议上限 {} 天；\
             渲染将切换为骨架模式，请用语义锚点导航。",
            self.approx_years, self.days, self.limit
        )
    }
}
