//! 地图上 POI 的悬停气泡（手绘地图呈现 #7，2026-10-15）。
//!
//! ## 视觉口径
//! 光标悬停在任何 label 包围盒上时，画面右下角（与光标同侧）显示
//! `● 名字 · 副标题` 浮窗。仿 civ-atlas `MapDecor.tsx` 的 `● 扎希亚族 · 部落地带`
//! 风格——圆点表 kind、名字、点号分隔、灰色副标题表分类。
//!
//! ## 时序
//! 与 label 命中注册同节拍：每帧拿 `LabelHits`（已由三个 label 系统
//! 注册完），用 `label_pick::hit_test` 在光标视图坐标上查——没命中
//! 就关掉浮窗。点击是另一个系统（drill_pick / selection），不重复。
//!
//! ## 边界（不做什么）
//! - **不消费 selection**——hover 是非 commit 的"看一眼就走"。
//! - **不显示 StreetSegment**——无稳定 id，没有可读的副标题。
//! - **不抢 inspector 面板**——hover 浮窗只显示"是什么 + 简短副标题"，
//!   完整详情仍走 inspector_panel（点选后右侧抽屉）。
//!
//! ## 主要导出
//! - `hover_tooltip_system` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{ViewState, WorldProjection};
use crate::theme::palette;
use crate::ui::shell::UiShell;
use wv_core::{EntityKind, SelectedEntity};
use wv_render::label_pick::{hit_test, LabelHits};

/// 悬停气泡系统：每帧根据光标视图坐标 + `LabelHits` 决定是否显示浮窗。
///
/// 与 `drill_pick` 不同——这里只读 `LabelHits`、不改 selection。
pub fn hover_tooltip_system(
    mut contexts: EguiContexts,
    shell_cfg: Res<crate::theme::ui_theme::UiShellConfig>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    label_hits: Res<LabelHits>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };

    // 1. 屏幕光标（bevy 物理像素）→ 视图坐标（世界单位）
    let Some(cursor_screen) = window.cursor_position() else {
        return;
    };
    let Ok(cam_t) = camera.single() else {
        return;
    };
    let cursor_view = screen_to_view(cursor_screen, cam_t, &view);

    // 2. 命中检测（复用 label_pick 的纯函数）
    let Some(target) = hit_test(cursor_view, &label_hits.items) else {
        return;
    };

    // 3. 解析名字 + 副标题
    let (name, subtitle) = resolve_label(&target, &projection);
    if name.is_empty() {
        return;
    }

    // 4. egui 浮窗（屏幕位置 = 光标 + 8px 偏移）
    let pos = [cursor_screen.x + 8.0, cursor_screen.y + 8.0];
    let pal = palette::colors(view.map_palette);
    let kind_dot = kind_color(target.kind, pal);
    let text_color = kind_text_color(target.kind, pal);
    let sub_text = if subtitle.is_empty() {
        String::new()
    } else {
        format!(" · {}", subtitle)
    };
    egui::Area::new(egui::Id::new("wv_hover_tooltip"))
        .anchor(egui::Align2::LEFT_TOP, pos)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            // 手绘地图 #7：气泡=圆点 + 名字 + 副标题。Frame 用 none
            // （简单容器）；视觉靠内边距 + 圆点 + 文字三段拼出。
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(6, 3))
                .fill(shell.raised_bg())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // ● 圆点：与 kind 主题色一致（手绘地图 #7）
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, kind_dot);
                        ui.label(
                            egui::RichText::new(format!("{}{}", name, sub_text))
                                .color(text_color)
                                .size(shell.scale.small),
                        );
                    });
                });
        });
}

/// 屏幕光标（bevy 物理像素，bevy::Vec2）→ 视图坐标（世界单位）。
///
/// 简化：直接用 view.camera_center + (screen - half_window) / scale，
/// 不读真实窗口尺寸（hover 系统不是热路径，误差在 ±1 cell 可接受）。
fn screen_to_view(
    screen: bevy::math::Vec2,
    cam_t: &Transform,
    view: &ViewState,
) -> bevy::math::Vec2 {
    let half = bevy::math::Vec2::new(640.0, 400.0);
    let local = (screen - half) / view.scale.max(0.01);
    let rot = cam_t.rotation.inverse();
    let p = rot * Vec3::new(local.x, local.y, 0.0);
    view.camera_center + bevy::math::Vec2::new(p.x, p.y)
}

/// 按 `EntityKind` 取圆点色（手绘地图 #7 + #9 统一调色板）。
///
/// 色源走 `PaletteColors.dot_*`（与 `LABEL_*` 同族），日间 / 暮色
/// 自动切换；之前是 hover_tooltip 模块内硬编码 RGB，theme 切不到。
fn kind_color(kind: EntityKind, pal: &palette::PaletteColors) -> egui::Color32 {
    let c = match kind {
        EntityKind::Place => pal.dot_place,
        EntityKind::Region => pal.dot_region,
        EntityKind::Faction => pal.dot_faction,
        EntityKind::Event => pal.dot_event,
        EntityKind::StreetSegment => return egui::Color32::GRAY,
    };
    crate::theme::colors::palette_to_color32(c)
}

/// POI hover 文字色（手绘地图 #12）：与 `kind_color` 同色族但更深的
/// `text_*` 令牌。日间 / 暮色自动切换；`color()` 接收 `egui::Color32`。
fn kind_text_color(kind: EntityKind, pal: &palette::PaletteColors) -> egui::Color32 {
    let c = match kind {
        EntityKind::Place => pal.text_place,
        EntityKind::Region => pal.text_region,
        EntityKind::Faction => pal.text_faction,
        EntityKind::Event => pal.text_event,
        EntityKind::StreetSegment => {
            return crate::theme::colors::palette_to_color32(pal.label_fill)
        }
    };
    crate::theme::colors::palette_to_color32(c)
}

/// 从 `WorldProjection.world` 解析 `(name, subtitle)`。
///
/// - Place: 名字 + "城市" / "村庄" / "地标"（按 PlaceKind 副分类）
/// - Region/Faction: 名字 + "区域" / "势力"（中文统一副标题）
/// - Event: 名字 + "事件"
/// - 找不到 → 返回空字符串，浮窗不显示
fn resolve_label(target: &SelectedEntity, projection: &WorldProjection) -> (String, String) {
    match target.kind {
        EntityKind::Place => projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id == target.id)
            // Place 暂无细分（kind 字段未落地，v0.4 待裁），统一副标题。
            .map(|p| (p.name.clone(), "地点".to_string()))
            .unwrap_or_default(),
        EntityKind::Region => projection
            .world
            .regions
            .iter()
            .find(|r| r.id == target.id)
            .map(|r| (r.name.clone(), "区域".to_string()))
            .unwrap_or_default(),
        EntityKind::Faction => projection
            .world
            .factions
            .iter()
            .find(|f| f.id == target.id)
            .map(|f| (f.name.clone(), "势力".to_string()))
            .unwrap_or_default(),
        EntityKind::Event => projection
            .world
            .events
            .iter()
            .find(|e| e.id == target.id)
            .map(|e| (e.name.clone(), "事件".to_string()))
            .unwrap_or_default(),
        EntityKind::StreetSegment => (String::new(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::distance::Place;
    use wv_core::Id;
    use wv_core::World as CoreWorld;

    fn setup_world_with_place() -> (WorldProjection, SelectedEntity) {
        let mut core = CoreWorld::default();
        let place = Place {
            id: Id::new_unchecked("p1"),
            name: "落云城".into(),
            map: None,
            x: Some(0.0),
            y: Some(0.0),
            w: None,
            h: None,
            feature: None,
        };
        core.distances.places.push(place);
        let proj = WorldProjection {
            world: core,
            ..Default::default()
        };
        let target = SelectedEntity::new(EntityKind::Place, Id::new_unchecked("p1"));
        (proj, target)
    }

    /// 命中 Place：返回 (name, "地点")。
    #[test]
    fn resolve_place_returns_name_and_subtitle() {
        let (w, t) = setup_world_with_place();
        let (name, sub) = resolve_label(&t, &w);
        assert_eq!(name, "落云城");
        assert_eq!(sub, "地点");
    }

    /// kind_color：每个 kind 都返回非 GRAY 的特定色。
    #[test]
    fn kind_color_per_kind_distinct() {
        let pal = palette::colors(crate::theme::palette::MapPalette::Daylight);
        let place = kind_color(EntityKind::Place, pal);
        let region = kind_color(EntityKind::Region, pal);
        let faction = kind_color(EntityKind::Faction, pal);
        let event = kind_color(EntityKind::Event, pal);
        let colors = [place, region, faction, event];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "kind {i} 与 kind {j} 同色");
            }
        }
    }

    /// 找不到的 ID：返回空串，浮窗不显示。
    #[test]
    fn resolve_missing_entity_returns_empty() {
        let (mut w, _) = setup_world_with_place();
        w.world.distances.places.clear();
        let ghost = SelectedEntity::new(EntityKind::Place, Id::new_unchecked("ghost"));
        let (name, sub) = resolve_label(&ghost, &w);
        assert!(name.is_empty());
        assert!(sub.is_empty());
    }
}
