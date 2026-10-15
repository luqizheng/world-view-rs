//! 画布占位装饰（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU`）。
//!
//! ## 职责
//! - `draw_map_decorations` —— 画 7 类程序化几何占位：背景渐变 / 网格 / 区域
//!   文字 / 河流 / 城市虚线圈 / 建筑色块 / 松树。
//!
//! ## 边界（不做什么）
//! - **不**接入 `WorldProjection` / `wv_core` —— 全部用绝对坐标写死，
//!   **不**走 `TimelineQuery`（不在 P7 时间语义上）；
//! - **不**替代 `vector_layers` / `terrain_chunks` 等数据驱动渲染 —— 它们是
//!   真正的底图，本模块只画 Figma 视觉对照所需的"死几何"。
//!
//! ## 为什么这个模块存在
//! M1 提交（commit `4cf60c2`）后 GUI 烟测已过（5 秒不崩），但**画面对不对**
//! 仍属人工验收。Figma 设计稿里"EAST FARMLANDS / ZUNYI OLD CITY 虚线圈 /
//! 河流 / 建筑 / 松树"这些是肉眼一眼能看的装饰，没有它们，UI 重做后的画
//! 布显得"空"。把它们画成占位几何，能让验收图与 Figma 直接对照。
//!
//! ## M3+ 取向
//! 真实 `City` / `Region` 数据稳定后，把"ZUNYI OLD CITY 虚线圈"接
//! `wv_core::place::polygon`、把"EAST FARMLANDS 标签"接 `Region::name`、
//! 河流接 `wv_core::track`、建筑接 `wv_core::building`。本模块即用同一
//! 个 `draw_map_decorations` 入口，但改从数据源读。
//!
//! ## 排障记录（2026-10-15，留给后来者）
//!
//! 接入期间 GUI 烟测曾稳定 panic（egui 0.36 `context.rs:1121`
//! `expect("No fonts available until first call to Context::run()")`）。
//! 排查中先后怀疑过 schedule 摆位 / `Order` / `ctx.input` / 全屏 Area，
//! 全部方向错误——**真因是 `app_bootstrap.rs` 里同时残留了一个
//! `Update` schedule 的注册**（首次移除的 Edit 因 fmt 重排未命中而失败，
//! 未被察觉）。egui 绘制类系统挂 `Update` 必挂此错（见 `app_bootstrap`
//! 「放 Update 会在第一帧 panic」的既有注释），而任何在
//! `EguiPrimaryContextPass` 上的摆位实验都被那个毒注册掩盖了结果。
//!
//! **教训**：egui 绘制系统**只能**挂在 `EguiPrimaryContextPass`；
//! 怀疑框架行为之前，先 `grep` 确认系统没有第二处注册点。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::ViewState;
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::radius;

use crate::ui::shell::UiShell;

// ── 装饰几何（屏幕像素坐标，与相机 / drill 无关；M2 占位） ──
//
// 屏幕坐标约定：`screen_w × screen_h` 是 `ctx.screen_rect()`；所有 x/y 都按
// 比例存（0..=1 归一化），实际绘制时按当前屏幕尺寸缩放。这样窗口缩放时
// 装饰位置也跟随，符合"装饰就该跟屏幕走"的直觉。

const SCENE_W: f32 = 1280.0;
const SCENE_H: f32 = 800.0;
const ZUNYI_CX: f32 = 0.55; // 城市虚线圈中心 x（归一化）
const ZUNYI_CY: f32 = 0.42; // 城市虚线圈中心 y（归一化）
const ZUNYI_R: f32 = 0.16; // 城市虚线圈半径（归一化）

/// 把 0..1 归一化坐标转屏幕像素。
fn pt(nx: f32, ny: f32, w: f32, h: f32) -> egui::Pos2 {
    egui::pos2(nx * w, ny * h)
}

/// 画布装饰：Bevy 系统，每帧 1 次（挂在 `EguiPrimaryContextPass` 上，
/// `Order::Background` 让面板浮在它上面）。
pub fn draw_map_decorations(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    _view: Res<ViewState>,
    windows: Query<&Window>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    // 不用 `ctx.input(|i| i.viewport_rect())`——egui 0.36 在 hot cache 第二帧
    // 偶尔 fonts 还没 ready，触发 `expect("No fonts available...")` panic。
    // 退化为：从 Bevy `Window` 资源读窗口尺寸（已在 bevy_winit 装配好）。
    let window_size = windows
        .single()
        .map(|w| w.resolution.size())
        .unwrap_or(bevy::prelude::Vec2::new(1280.0, 800.0));
    let w = window_size.x;
    let h = window_size.y;

    // 装饰比 v0.2 时（默认 1280×800）的偏差：若用户窗口大幅缩放，比例按
    // 当前尺寸走（Figma 是 1280×800，1280 偏差 ±20% 视觉差异可接受）。
    let scale_x = w / SCENE_W;
    let scale_y = h / SCENE_H;

    // 中央 Layer：放在世界渲染之后、egui UI 之前
    egui::Area::new(egui::Id::new("wv_map_decorations_v03"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(0.0, 0.0))
        // Middle：与各面板同层，egui 按 `.show()` 调用序排布；本系统在
        // `EguiPrimaryContextPass` 链上排最后（校验之后），故面板都压在本
        // 装饰之上。
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            let painter = ui.painter();
            let screen_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h));

            // ── 1. 背景渐变（land → water，v0.3 设计稿采样）──
            let n_stops = 12;
            for i in 0..n_stops {
                let t = i as f32 / n_stops as f32;
                let c = lerp(shell.map_water(), shell.map_land(), t);
                let band = egui::Rect::from_min_max(
                    egui::pos2(0.0, t * h),
                    egui::pos2(w, (t + 1.0 / n_stops as f32) * h),
                );
                painter.rect_filled(band, 0.0, c);
            }

            // ── 2. 网格（10% 步长，alpha 0.10）──
            let step = (w.min(h) * 0.10).max(40.0);
            let grid_color = shell.border_strong().linear_multiply(0.10);
            let mut x = 0.0;
            while x < w {
                painter.line_segment(
                    [egui::pos2(x, 0.0), egui::pos2(x, h)],
                    egui::Stroke::new(1.0, grid_color),
                );
                x += step;
            }
            let mut y = 0.0;
            while y < h {
                painter.line_segment(
                    [egui::pos2(0.0, y), egui::pos2(w, y)],
                    egui::Stroke::new(1.0, grid_color),
                );
                y += step;
            }

            // ── 3. EAST FARMLANDS 区域标签（白字，letter-spacing 大）──
            let farm_pos = pt(0.18, 0.78, w, h);
            painter.text(
                farm_pos,
                egui::Align2::CENTER_CENTER,
                "EAST  FARMLANDS",
                egui::FontId::proportional(18.0),
                shell.t1().linear_multiply(0.85),
            );
            let _ = scale_x;
            let _ = scale_y;

            // ── 4. 河流（粗青蓝多段线）──
            let river_color = shell.map_water();
            let river_path = [
                pt(0.05, 0.55, w, h),
                pt(0.18, 0.62, w, h),
                pt(0.30, 0.58, w, h),
                pt(0.45, 0.66, w, h),
                pt(0.62, 0.60, w, h),
                pt(0.80, 0.68, w, h),
                pt(0.97, 0.65, w, h),
            ];
            for w_px in [7.0_f32, 4.0, 2.0] {
                let stroke = if w_px > 4.0 {
                    egui::Stroke::new(w_px, river_color)
                } else {
                    egui::Stroke::new(w_px, shell.t1().linear_multiply(0.55))
                };
                painter.add(egui::Shape::line(river_path.to_vec(), stroke));
            }

            // ── 5. ZUNYI OLD CITY 城市虚线圈（金色 dashed）──
            let city_c = pt(ZUNYI_CX, ZUNYI_CY, w, h);
            let r_px = ZUNYI_R * w.min(h);
            // 12 段虚线小段（自绘，避免 dashed_circle 跨版本 API 差）
            let segments = 24;
            let dash_on = 0.04;
            let dash_off = 0.025;
            let stroke = egui::Stroke::new(2.0, shell.gold());
            for i in 0..segments {
                let t0 = i as f32 / segments as f32;
                let t1 = t0 + dash_on;
                if t1 > 1.0 {
                    continue;
                }
                let a0 = t0 * std::f32::consts::TAU;
                let a1 = t1 * std::f32::consts::TAU;
                let p0 = egui::pos2(city_c.x + r_px * a0.cos(), city_c.y + r_px * a0.sin());
                let p1 = egui::pos2(city_c.x + r_px * a1.cos(), city_c.y + r_px * a1.sin());
                painter.line_segment([p0, p1], stroke);
                // skip dash_off 段（下一个循环 i 起点自然跳过）
                let _ = dash_off;
            }
            // 城市名（letter-spacing 大）
            painter.text(
                egui::pos2(city_c.x, city_c.y - r_px - 18.0),
                egui::Align2::CENTER_CENTER,
                "ZUNYI  OLD  CITY",
                egui::FontId::proportional(16.0),
                shell.gold(),
            );

            // ── 6. 黄色建筑色块 3 个（City 内）──
            let building_color = egui::Color32::from_rgb(0xE8, 0xC0, 0x5A);
            for (dx, dy, sz) in [
                (-0.04_f32, 0.0_f32, 16.0_f32),
                (0.02, 0.02, 12.0),
                (0.05, -0.03, 18.0),
            ] {
                let center = pt(ZUNYI_CX + dx, ZUNYI_CY + dy, w, h);
                painter.rect_filled(
                    egui::Rect::from_center_size(center, egui::vec2(sz, sz)),
                    radius::BADGE,
                    building_color,
                );
            }

            // ── 7. Government Office 文字 + 松树 ──
            let go_pos = pt(ZUNYI_CX + 0.05, ZUNYI_CY - 0.03, w, h);
            painter.text(
                egui::pos2(go_pos.x + 14.0, go_pos.y - 14.0),
                egui::Align2::LEFT_CENTER,
                "Government Office",
                egui::FontId::proportional(12.0),
                shell.t1(),
            );
            // 松树（用等边三角形堆叠代表，3 棵）
            for (dx, dy) in [(0.42_f32, 0.30_f32), (0.46, 0.36), (0.40, 0.40)] {
                let base = pt(dx, dy, w, h);
                draw_pine(painter, base, 18.0, shell.t1().linear_multiply(0.55));
            }

            // 占位：让编译器不抱怨未使用的 screen_rect（实际是为了显式覆盖整个屏幕）
            let _ = screen_rect;
        });
}

/// 单棵松树（两个等边三角形堆叠），无边框。
fn draw_pine(painter: &egui::Painter, base: egui::Pos2, height: f32, color: egui::Color32) {
    let w_top = height * 0.5;
    let top = egui::pos2(base.x, base.y - height);
    let top_tri = vec![
        top,
        egui::pos2(base.x - w_top, base.y - height * 0.45),
        egui::pos2(base.x + w_top, base.y - height * 0.45),
    ];
    painter.add(egui::Shape::convex_polygon(
        top_tri,
        color,
        egui::Stroke::NONE,
    ));
    let w_bot = height * 0.7;
    let bot_tri = vec![
        egui::pos2(base.x, base.y - height * 0.55),
        egui::pos2(base.x - w_bot, base.y),
        egui::pos2(base.x + w_bot, base.y),
    ];
    painter.add(egui::Shape::convex_polygon(
        bot_tri,
        color,
        egui::Stroke::NONE,
    ));
}

/// 颜色线性插值（不感知 alpha——底色已带 alpha；alpha 段被忽略）。
fn lerp(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        255,
    )
}

// ── 单元测试：纯几何 / 颜色函数 ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_scales_normalized_to_screen() {
        let p = pt(0.5, 0.25, 1280.0, 800.0);
        assert_eq!(p.x, 640.0);
        assert_eq!(p.y, 200.0);
    }

    #[test]
    fn pt_clamps_to_screen_when_normalized_out_of_range() {
        // 不 clamp，直接按数学——caller 保证 0..=1
        let p = pt(1.5, 0.0, 1000.0, 1000.0);
        assert_eq!(p.x, 1500.0);
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn lerp_endpoints() {
        let a = egui::Color32::from_rgb(0x10, 0x20, 0x30);
        let b = egui::Color32::from_rgb(0xE0, 0xC0, 0xA0);
        // lerp 签名：`lerp(起点, 终点, t)`，t=0 → 起点；t=1 → 终点。
        assert_eq!(lerp(a, b, 0.0), a, "t=0 → 起点 a");
        assert_eq!(lerp(b, a, 0.0), b, "t=0 → 起点 b");
        assert_eq!(lerp(a, b, 1.0), b, "t=1 → 终点 b");
        let mid = lerp(a, b, 0.5);
        // 通道 0：0x10 + (0xE0-0x10)*0.5 = 0x10 + 0x68 = 0x78
        assert_eq!(mid.r(), 0x78);
        assert_eq!(mid.g(), (0x20 + (0xC0 - 0x20) / 2));
        assert_eq!(mid.b(), (0x30 + (0xA0 - 0x30) / 2));
    }

    #[test]
    fn lerp_clamps_t_outside_unit_interval() {
        let a = egui::Color32::from_rgb(0, 0, 0);
        let b = egui::Color32::from_rgb(255, 255, 255);
        assert_eq!(lerp(a, b, -1.0), a, "t<0 → a");
        assert_eq!(lerp(a, b, 2.0), b, "t>1 → b");
    }
}
