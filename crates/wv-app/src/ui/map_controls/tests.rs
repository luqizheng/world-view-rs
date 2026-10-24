//! `map_controls` 的单测（原文件尾部 `mod tests` 整段平移）。

use super::*;
use bevy::prelude::Vec2;

/// `＋/－` 每次 1.6×（`view.scale / 1.6`、`* 1.6`），经 `set_map_scale` 夹在
/// **当前级**区间内：连按到底应停在内界/外界，而不是穿过去。
#[test]
fn zoom_button_steps_stop_at_the_levels_band() {
    let zoom =
        wv_render::zoom_level::LevelZoom::for_map(Vec2::splat(1024.0), Vec2::new(1280.0, 800.0));
    let mut s = zoom.fit;
    for _ in 0..40 {
        s = zoom.clamp(s / 1.6);
    }
    assert!((s - zoom.min).abs() < 1e-6, "got {s}, min {}", zoom.min);
    let mut s = zoom.fit;
    for _ in 0..40 {
        s = zoom.clamp(s * 1.6);
    }
    assert!((s - zoom.max).abs() < 1e-6, "got {s}, max {}", zoom.max);
}

/// 朝向持久化键 round-trip；不可识别值静默回退（V1）。
#[test]
fn dock_orientation_round_trips_through_key() {
    for o in [MapDockOrientation::Vertical, MapDockOrientation::Horizontal] {
        assert_eq!(MapDockOrientation::from_key(o.key()), Some(o));
    }
    assert_eq!(MapDockOrientation::from_key("diagonal"), None);
    assert_eq!(
        MapDockOrientation::Vertical.toggled(),
        MapDockOrientation::Horizontal
    );
    assert_eq!(
        MapDockOrientation::Horizontal.toggled(),
        MapDockOrientation::Vertical
    );
}

/// 横向内容宽度：**12 个等宽槽**（5 瓷砖 + 4 图标钮 + 3 分隔，槽位统一
/// 后全部 = `tile.x`）+ 11 间距 + 左右内边距。必须在 1280 窗口内留出两侧
/// 空白——否则贴底居中的工具条会被窗口裁掉。**元素数与 `slots` 闭包的
/// 排布一一对应**，改布局必须同步改这条。
///
/// 2026-10-23：「清除全部测量」图标钮插在测量组右侧（槽 6.5），
/// 图标钮 3 → 4、槽位 11 → 12。
#[test]
fn horizontal_dock_fits_a_1280_window() {
    let m = DockMetrics::for_orientation(false);
    let content = 12.0 * m.tile.x + 11.0 * m.gap;
    let framed = content + 2.0 * m.pad_x;
    assert!(framed < 1280.0, "横向工具条 {framed}px 超出 1280 窗口宽度");
}

/// 纵向紧凑形态必须在时间轴之上收口：800 高窗口 − 顶栏 38 − 时间轴
/// 约 152 − 边距，可用高度约 648；12 个等高槽纵排必须留出余量。
/// **元素数与 `slots` 闭包一一对应**，改布局必须同步改这条。
#[test]
fn vertical_dock_fits_above_the_timeline() {
    let m = DockMetrics::for_orientation(true);
    let height = 12.0 * m.tile.y + 11.0 * m.gap + 2.0 * m.pad_y;
    let available = 800.0 - 152.0;
    assert!(
        DOCK_TOP_CLEARANCE + height < available,
        "纵向工具条到 {:.0}px，与时间轴（顶约 {:.0}px）重叠",
        DOCK_TOP_CLEARANCE + height,
        available
    );
}

/// 纵向工具条的宽度由**最宽槽位**决定：海拔分层设色滑条在竖排里必须收进
/// 一个瓷砖列。横条的 3 槽宽会把整条从 34px 撑到约 116px，右侧留一片空底
/// （2026-10-24 用户反馈）。
#[test]
fn vertical_tint_slider_stays_within_one_tile_column() {
    let vertical = DockMetrics::for_orientation(true);
    let slot = vertical.tint_slider_slot(true);
    assert!(
        slot.x <= vertical.tile.x,
        "竖排滑条槽宽 {} 超过瓷砖宽 {}，会把工具条撑宽",
        slot.x,
        vertical.tile.x
    );
    assert_eq!(slot.y, vertical.tile.y * 2.0, "竖排滑条占 2 个槽高");

    let horizontal = DockMetrics::for_orientation(false);
    let slot = horizontal.tint_slider_slot(false);
    assert_eq!(slot.y, horizontal.tile.y, "横排滑条不应改变工具条高度");
    assert!(slot.x > horizontal.tile.x, "横排滑条维持 3 槽长条");
}

/// 分裂按钮的几何必须从 `DockMetrics` 单点投影——两处各写一遍尺寸表，
/// 早晚漂移成「弹层锚点歪了」这类难查的视觉 bug。
#[test]
fn split_metrics_project_from_dock_metrics() {
    for vertical in [false, true] {
        let m = DockMetrics::for_orientation(vertical);
        let s = m.split_metrics(vertical);
        assert_eq!(s.tile, m.tile);
        assert_eq!(s.icon, m.tile_icon);
        assert_eq!(s.chevron_hit, m.chevron_hit);
        assert_eq!(s.radius, TILE_RADIUS);
        assert_eq!(
            s.chevron_side,
            if vertical {
                wv_ui::widgets::ChevronSide::BottomCenter
            } else {
                wv_ui::widgets::ChevronSide::RightCenter
            },
            "小三角方位应随朝向"
        );
        // 小三角命中区必须完全在瓷砖内（wv-ui 侧测试再验几何，这里验的是
        // 我们喂进去的尺寸本身合法）。
        assert!(m.chevron_hit < m.tile.x.min(m.tile.y));
    }
}
