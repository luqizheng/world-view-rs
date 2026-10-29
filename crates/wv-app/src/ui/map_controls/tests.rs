//! `map_controls` 的单测（原文件尾部 `mod tests` 整段平移）。

use super::widgets::line_poi_tile_visible;
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

/// 横向内容宽度：**13 个等宽槽**（5 瓷砖 + 4 图标钮 + 3 分隔 + 1 线段 POI
/// 分裂按钮，槽位统一后全部 = `tile.x`）+ 12 间距 + 左右内边距。必须在
/// 1280 窗口内留出两侧空白——否则贴底居中的工具条会被窗口裁掉。**元素数
/// 与 `slots` 闭包的最坏排布一一对应**（线段 POI 仅 Edit 档渲染，此处按
/// 显示时计），改布局必须同步改这条。
///
/// 2026-10-23：「清除全部测量」图标钮插在测量组右侧（槽 6.5），
/// 图标钮 3 → 4、槽位 11 → 12。
/// 2026-10-26（ADR-0070 D4）：线段 POI 分裂按钮入列（槽 14），槽位 12 → 13。
/// 2026-10-29（ADR-0086）：泛洪瓷砖与海拔滑条出列——13 是收紧前的最坏
/// 排布口径，实际更少，两条断言只会更宽裕，不改数。
#[test]
fn horizontal_dock_fits_a_1280_window() {
    let m = DockMetrics::for_orientation(false);
    let content = 13.0 * m.tile.x + 12.0 * m.gap;
    let framed = content + 2.0 * m.pad_x;
    assert!(framed < 1280.0, "横向工具条 {framed}px 超出 1280 窗口宽度");
}

/// 纵向紧凑形态必须在时间轴之上收口：800 高窗口 − 顶栏 38 − 时间轴
/// 约 152 − 边距，可用高度约 648；13 个等高槽纵排必须留出余量。
/// **元素数与 `slots` 闭包的最坏排布一一对应**（线段 POI 仅 Edit 档渲染，
/// 此处按显示时计），改布局必须同步改这条。
#[test]
fn vertical_dock_fits_above_the_timeline() {
    let m = DockMetrics::for_orientation(true);
    let height = 13.0 * m.tile.y + 12.0 * m.gap + 2.0 * m.pad_y;
    let available = 800.0 - 152.0;
    assert!(
        DOCK_TOP_CLEARANCE + height < available,
        "纵向工具条到 {:.0}px，与时间轴（顶约 {:.0}px）重叠",
        DOCK_TOP_CLEARANCE + height,
        available
    );
}

// （「竖排滑条收进一个瓷砖列」守点已随槽 10.75 挪 topOptionBar 退役
// （ADR-0086）——滑条不再是本工具条的槽位，宽度约束由顶条单行布局自担。）

/// 线段 POI 瓷砖只在 Edit 档渲染（ADR-0070 D4）：Nation / Region 档作者
/// 看不到入口——「线」是放大到可编辑级别才画的东西。
///
/// 纯 egui 渲染不好直接断言，门控提成纯函数 [`line_poi_tile_visible`]
/// 后按 ADR-0043 的状态断言手法测：三档直接戳 + `lod_tier(scale)` 求值
/// 同源走一遍（阈值 1.0 / 3.0，见 `wv_render::state` 常量）。
#[test]
fn line_poi_tile_hidden_outside_edit_tier() {
    use wv_render::state::{lod_tier, LodTier};

    assert!(
        line_poi_tile_visible(LodTier::Edit),
        "Edit 档应显示线段 POI 入口"
    );
    assert!(
        !line_poi_tile_visible(LodTier::Region),
        "Region 档不得显示线段 POI 入口"
    );
    assert!(
        !line_poi_tile_visible(LodTier::Nation),
        "Nation 档不得显示线段 POI 入口"
    );

    // 与真实视野求值同源：scale 0.5（近）= Edit，1.0 = Region 下界，3.0 = Nation 下界。
    assert!(
        line_poi_tile_visible(lod_tier(0.5)),
        "scale 0.5 应视为 Edit 档"
    );
    assert!(
        !line_poi_tile_visible(lod_tier(1.0)),
        "scale 1.0 应视为 Region 档（≥ LOD_REGION_SCALE）"
    );
    assert!(
        !line_poi_tile_visible(lod_tier(3.0)),
        "scale 3.0 应视为 Nation 档（≥ LOD_NATION_SCALE）"
    );
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
