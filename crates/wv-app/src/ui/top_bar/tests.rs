//! `top_bar` / `status_bar` 的单测（原文件尾部 `mod tests` 整段平移）。

use super::status::{
    format_fixed2, format_lat, format_lon, format_thousands, project_display_name,
};

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
