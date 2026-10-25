//! `top_bar` / `status_bar` 的单测（原文件尾部 `mod tests` 整段平移）。

use bevy_egui::egui;

use super::status::{
    format_fixed2, format_lat, format_lon, format_thousands, project_display_name,
};
use super::{central_area_of, STATUS_BAR_H, TOP_BAR_H};

/// 中央工作区 = 视口纵向挖去顶栏 38 / 状态栏 28，横向不收（2026-10-25）。
#[test]
fn central_area_carves_top_and_status_bars_from_viewport() {
    let vp = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
    let area = central_area_of(vp);
    assert!((area.min.y - TOP_BAR_H).abs() < 1e-3, "got {}", area.min.y);
    assert!(
        (area.max.y - (800.0 - STATUS_BAR_H)).abs() < 1e-3,
        "got {}",
        area.max.y
    );
    assert_eq!(area.min.x, vp.min.x);
    assert_eq!(area.max.x, vp.max.x, "横向不收：左右缘无 chrome");
}

#[test]
fn thousands_groups_every_three_digits() {
    assert_eq!(format_thousands(0), "0");
    assert_eq!(format_thousands(480), "480");
    assert_eq!(format_thousands(12_480), "12,480");
    assert_eq!(format_thousands(1_234_567), "1,234,567");
}

#[test]
fn fixed2_pads_two_decimals_and_groups() {
    // 设计稿示例：X 1,284.50。
    assert_eq!(format_fixed2(1284.5), "1,284.50");
    assert_eq!(format_fixed2(0.0), "0.00");
    assert_eq!(format_fixed2(60.0), "60.00");
    // 0.125 在 f64 里精确，×100 = 12.5，round 半值远离零 → 13。
    assert_eq!(format_fixed2(0.125), "0.13");
}

#[test]
fn fixed2_keeps_sign_for_negative_coordinates() {
    // 真实地理图（西雅图）x 常为负。
    assert_eq!(format_fixed2(-122.5), "-122.50");
    assert_eq!(format_fixed2(-1_284.25), "-1,284.25");
}

#[test]
fn lat_lon_hemisphere_suffixes_follow_sign() {
    assert_eq!(format_lat(28.034), "28.03° N");
    assert_eq!(format_lat(-33.9), "33.90° S");
    assert_eq!(format_lon(116.4), "116.40° E");
    assert_eq!(format_lon(-122.5), "122.50° W");
}

#[test]
fn project_display_name_takes_the_last_segment() {
    assert_eq!(
        project_display_name(r"D:\projects\guizhou-atlas.world"),
        "guizhou-atlas.world"
    );
    assert_eq!(project_display_name("foo/atlas"), "atlas");
    // 无可拆分段时退回原串（显示完整路径好过空白）。
    assert_eq!(project_display_name(""), "");
}
