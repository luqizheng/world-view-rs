//! 「生成城市」意图（2026-10-17 选择工具 spec §5；M2）。
//!
//! ## 职责
//! - 几何纯函数：[`ring_centroid`]（质心落点）/ [`ring_bbox_area`]（面积预填）/
//!   [`area_to_16_9`]（面积 → 16:9 画布）。
//! - （Task 3）[`CityIntentDialog`] 对话框资源 + 预填 + 确认系统：
//!   确认 → 16:9 extent + 局部矩形轮廓 → `AddCityCommand`（锚点显式落质心）
//!   → 自动 `drill.enter` 进城市编辑器。
//!
//! ## 边界（不做什么）
//! - **不选底图地形**——城市图画布走既有继承/预填口径，本模块只建 `MapNode`。
//! - **不碰选区**——确认后钻入新图，选区由 `sync_selection_map_change` 随切图清空。
//! - 坐标口径：质心 / 包围盒为**父图局部 wu**（与选区同源链）。

#![cfg(feature = "app")]

/// 多边形质心（鞋带公式，面积加权）；退化（零面积）回退包围盒中心。
///
/// 作者圈的环不保证凸也不保证不自交——鞋带公式对两类都给出确定值，
/// 零面积（共线 / 一字形）时加权分母为零，回退包围盒中心最不意外。
pub fn ring_centroid(ring: &[(f32, f32)]) -> (f32, f32) {
    let n = ring.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut a2 = 0.0f32; // 2×有向面积
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        a2 += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if a2.abs() < 1e-6 {
        // 退化：包围盒中心。
        let (min_x, max_x, min_y, max_y) = bbox(ring);
        return ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
    }
    (cx / (3.0 * a2), cy / (3.0 * a2))
}

/// 包络包围盒面积（wu²）。
pub fn ring_bbox_area(ring: &[(f32, f32)]) -> f32 {
    let (min_x, max_x, min_y, max_y) = bbox(ring);
    (max_x - min_x) * (max_y - min_y)
}

/// 面积 → 16:9 画布 (w, h)：`w = √(A·16/9)`、`h = √(A·9/16)`（spec §5）。
pub fn area_to_16_9(area: f32) -> (f32, f32) {
    let w = (area * 16.0f32 / 9.0).sqrt();
    let h = (area * 9.0f32 / 16.0).sqrt();
    (w, h)
}

/// 环的包围盒（空环返回零盒）。
fn bbox(ring: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in ring {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if !min_x.is_finite() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (min_x, max_x, min_y, max_y)
}

#[cfg(test)]
mod tests {
    use super::{area_to_16_9, ring_bbox_area, ring_centroid};

    fn square() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
    }

    #[test]
    fn square_ring_centroid_is_its_center() {
        let c = ring_centroid(&square());
        assert!((c.0 - 5.0).abs() < 1e-4 && (c.1 - 5.0).abs() < 1e-4, "got {c:?}");
    }

    /// 共线三点面积为零 → 回退包围盒中心（不产生 NaN 质心）。
    #[test]
    fn degenerate_ring_centroid_falls_back_to_bbox_center() {
        let collinear = vec![(0.0, 0.0), (5.0, 5.0), (10.0, 10.0)];
        let c = ring_centroid(&collinear);
        assert!((c.0 - 5.0).abs() < 1e-4 && (c.1 - 5.0).abs() < 1e-4, "got {c:?}");
    }

    #[test]
    fn ring_bbox_area_measures_envelope() {
        assert!((ring_bbox_area(&square()) - 100.0).abs() < 1e-4, "10×10 方环面积 100");
    }

    /// 手算：A=14400 → w=√(14400·16/9)=√25600=160，h=√(14400·9/16)=√8100=90。
    #[test]
    fn area_maps_to_16_9_extent() {
        let (w, h) = area_to_16_9(14400.0);
        assert!((w - 160.0).abs() < 1e-3, "got w={w}");
        assert!((h - 90.0).abs() < 1e-3, "got h={h}");
        assert!((w * h - 14400.0).abs() < 1e-2, "面积守恒 w·h={}", w * h);
    }
}
