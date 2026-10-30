//! 交互阈值单点常量（ADR-0089 D1）。
//!
//! 双击与点击/拖拽的判定位移、时长此前散落四处（`drill` / `selection` /
//! wv-app `pin_drag` / `region_select` 各自定义、数值两两不一），同一手势
//! 在不同工作区的「手抖余量」不同。本模块是**唯一**定义点：两个关注点
//! 各一个值，消费方按屏幕像素口径乘 `view.scale` 换算成世界单位。
//!
//! - **双击**（时间窗 + 同点容差）：「这两下是同一次双击」；
//! - **拖拽起手**（位移门槛）：「按住后光标移出多少算拖，否则是单击」
//!   （图钉挪钉等左键手势）；
//! - **点击上限**（位移上限）：「松手时位移不超过它算点击，超过是拖拽」
//!   （右键单击=取消选择 vs 右键拖拽=平移）。
//!
//! 起手与上限是两个关注点，数值允许不同（ADR-0089 D1）。

/// 双击判定的时间窗（秒）。
pub const DOUBLE_CLICK_SECS: f64 = 0.40;

/// 双击判定的同点容差（屏幕像素）——UX-024 口径：按屏幕像素定义、
/// 使用处以 `view.scale` 换算成世界单位，任何缩放档下手抖余量一致。
/// 原 wv-app `pin_drag` 的 6px 世界定长比较并入本值（既统一又修正口径）。
pub const DOUBLE_CLICK_SLOP_PX: f32 = 12.0;

/// 拖拽起手位移门槛（屏幕像素）：按下后光标移出此半径才算拖动。
pub const DRAG_START_SLOP_PX: f32 = 4.0;

/// 单击判定位移上限（屏幕像素）：松手时按住期间位移不超过它算「点击」，
/// 超过即拖拽（右键拖拽=平移，撤锚/取消选择的右键单击共用此判据）。
pub const CLICK_MAX_PX: f32 = 6.0;

#[cfg(test)]
mod tests {
    #[test]
    fn double_click_slop_is_forgiving_but_bounded() {
        // 双击容差必须宽于拖拽起手——「原地双击」不得被判成拖动起手。
        assert!(
            crate::input::DOUBLE_CLICK_SLOP_PX >= crate::input::DRAG_START_SLOP_PX,
            "double click slop {} < drag start {}",
            crate::input::DOUBLE_CLICK_SLOP_PX,
            crate::input::DRAG_START_SLOP_PX
        );
    }
}
