//! 初始取景：按世界内容算 (中心, 缩放)。

use bevy::prelude::*;

/// 按世界内容计算初始取景（中心 + 缩放）。
///
/// - 优先**框住根图内容**（icons 包围盒 + 25% 边距）：大图幅上内容只占
///   一小块时，按整图 fit 会把内容挤成屏幕中心的一撮点、四周全是空黑；
/// - 无内容/空世界时退回根图 extent（缺省 4096²）的中心。
///
/// Startup（`run` 装配）与运行期加载（`ui_shell::apply_loaded_world`）共用，
/// 保证「加载工程后看到的取景」与「启动即加载」完全一致。
pub(crate) fn fit_view_for_world(world: &wv_core::World) -> (Vec2, f32) {
    let root_id = world
        .meta
        .as_ref()
        .map(|m| m.root_map.clone())
        .unwrap_or_else(|| wv_core::Id::new_unchecked("world_main"));
    let root_extent = world
        .maps
        .iter()
        .find(|m| m.id == root_id)
        .and_then(|m| m.extent.as_ref())
        .map(|e| (e.width, e.height))
        .unwrap_or((4096.0f32, 4096.0));
    // 口径：marker/Place 的世界坐标 = anchor 直接乘 extent（不做 y 翻转，
    // 与 drill 标记一致）；这里只做取景，别自作聪明翻转。
    let content_box: Option<(Vec2, Vec2)> =
        world.maps.iter().find(|m| m.id == root_id).and_then(|m| {
            let (w, h) = m
                .extent
                .as_ref()
                .map(|e| (e.width, e.height))
                .unwrap_or((4096.0, 4096.0));
            let pts: Vec<Vec2> = m
                .icons
                .iter()
                .filter_map(|ic| ic.anchor.map(|a| Vec2::new(a[0] * w, a[1] * h)))
                .collect();
            (pts.len() >= 2).then(|| {
                let lo = pts.iter().copied().fold(Vec2::MAX, Vec2::min);
                let hi = pts.iter().copied().fold(Vec2::MIN, Vec2::max);
                (lo, hi)
            })
        });
    fit_for_content(root_extent, content_box)
}

/// 纯几何部分：给定根图 extent 与内容包围盒，算 (center, scale)。
///
/// `ViewState.scale` 的口径是「世界单位 / 像素」（screen_to_world 乘它），
/// 所以 fit = 图宽 / 视口宽，取两轴较紧的一个；缩放不小于 1（避免比像素还细）。
fn fit_for_content(root_extent: (f32, f32), content_box: Option<(Vec2, Vec2)>) -> (Vec2, f32) {
    match content_box {
        Some((lo, hi)) => {
            let size = hi - lo;
            let margin = size.max_element() * 0.25 + 64.0;
            let span = size + Vec2::splat(margin * 2.0);
            (
                (lo + hi) * 0.5,
                (span.x / 1280.0).max(span.y / 800.0).max(1.0),
            )
        }
        None => (
            Vec2::new(root_extent.0 * 0.5, root_extent.1 * 0.5),
            (root_extent.0 / 1280.0).max(root_extent.1 / 800.0).max(1.0),
        ),
    }
}

#[cfg(test)]
mod fit_view_tests {
    use super::fit_for_content;
    use bevy::math::Vec2;

    #[test]
    fn empty_world_falls_back_to_centered_4096_view() {
        let (center, scale) = fit_for_content((4096.0, 4096.0), None);
        assert_eq!(center, Vec2::splat(2048.0), "center {center:?}");
        // 4096 / 800 = 5.12（取两轴较紧的一轴）。
        assert!((scale - 5.12).abs() < 1e-4, "got {scale}");
    }

    #[test]
    fn content_box_centers_the_view_with_25_percent_margin() {
        // 内容 640×400 位于 (0,0)-(640,400)：
        // margin = 640*0.25 + 64 = 224；span = (1088, 848)；
        // scale = max(1088/1280, 848/800) = 1.06；center = (320, 200)。
        let (center, scale) = fit_for_content(
            (4096.0, 4096.0),
            Some((Vec2::ZERO, Vec2::new(640.0, 400.0))),
        );
        assert_eq!(center, Vec2::new(320.0, 200.0), "center {center:?}");
        assert!((scale - 1.06).abs() < 1e-4, "got {scale}");
    }

    #[test]
    fn tiny_content_is_not_zoomed_past_one_to_one() {
        // 内容只有 10×10：span ≈ 10 + 2*(2.5+64) = 143；两轴 fit 都 < 1，
        // 缩放下限 1.0 兜底（世界单位 ≈ 像素时不再放大）。
        let (_, scale) = fit_for_content((4096.0, 4096.0), Some((Vec2::ZERO, Vec2::splat(10.0))));
        assert!((scale - 1.0).abs() < 1e-6, "got {scale}");
    }
}
