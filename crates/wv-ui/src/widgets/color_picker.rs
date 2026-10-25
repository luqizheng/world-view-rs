//! 取色器（ColorPicker）—— 色块触发器 + Popup 浮层（SV 方块 / 色相条 /
//! alpha 条 / hex 输入）。
//!
//! 规格：`docs/UI_COLOR_PICKER_SPEC_v0.1.md`（2026-10-25 已接受）。
//! 本文件是规格 §3 参数表的代码投影，尺寸 / 令牌逐项对位，勿在此另立数字。
//!
//! ## 职责
//! - 「点色块 → 浮层选色 → 点外部关闭」的完整取色交互；
//! - hex 文本与色板**双向同步**（含非法输入的宽容处理，AGENTS §7.5 V1）；
//! - 对外只吃吐 [`egui::Rgba`]（**非预乘**）——`Color32` 是预乘格式，
//!   低 alpha 往返会丢精度（见 `ui_tokens::Rgb::to_color32_alpha` 的同款教训）。
//!
//! ## 边界（不做什么）
//! - **不依赖** `wv-core::color::Color`——那是 YAML 存储层类型，wv-ui 是
//!   叶子 crate 零兄弟依赖（ADR-0026）；调用方（wv-app）负责两边互转。
//! - **不做**预置色板 / RGB 数值输入 / 吸管（规格 §1.2 的否决项）。
//! - **不引入** Linear 色彩空间分支——全程 sRGB（egui 默认 GammaByte）。
//!
//! ## 关键实现选择（为什么）
//! - **SV 方块自绘而非复用 egui 内置**：egui 的 `color_picker` 弹层是它
//!   自己的视觉（灰底、英文标签），不受本工程令牌控制；而本工程控件纪律
//!   是「自绘 + 走 `UiShell` 令牌」（badge / tree_row / segmented 同款）。
//!   手法与 egui 内置相同——纯 CPU 顶点色网格（`Mesh::colored_vertex` +
//!   `add_triangle`），**无 shader、无新依赖**。
//! - **浮层用 `egui::Popup` 而非 [`crate::widgets::FloatWindow`]**：后者是
//!   带标题栏 + 可拖拽缩放的**常驻面板**外壳；取色器是锚定触发器旁、
//!   点外部即关的轻量弹出层。仓库先例见 `split_button.rs` 的下拉菜单。
//! - **hex 大写**：与 `wv_core::color::Color` 消费侧的
//!   `format!("#{:02X}{:02X}{:02X}", …)` 一致；解析宽容接受大小写
//!   （`u8::from_str_radix` 本就大小写不敏感）。

use bevy_egui::egui;
use bevy_egui::egui::ecolor::Hsva;

use crate::shell::UiShell;
use crate::ui_theme::mono;
use crate::ui_tokens::{font_size, radius, space};

// ── 规格参数（§3 的代码投影，改动须先改规格）───────────────────────────

/// 触发器色块边长（规格 §3.1：18px，与 faction 表单既有色块一致）。
const TRIGGER_SIZE: f32 = 18.0;
/// 触发器命中区边长（规格 §3.1：外扩到 24px，视觉块仍 18 居中）。
const TRIGGER_HIT: f32 = 24.0;
/// 浮层内容总宽（规格 §3.2：SV 208 + 左右 padding 12×2）。
const POPUP_W: f32 = 232.0;
/// 浮层内边距（规格 §3.2：`space::MD`，与菜单的 4px 分档——内容区有留白需求）。
const POPUP_PAD: f32 = space::MD;
/// SV 方块边长（规格 §3.3）。
const SV_SIZE: f32 = 208.0;
/// 渐变网格细分（规格 §3.3：与 egui 内置同档 N=36，(N+1)² 顶点开销可忽略）。
const MESH_N: u32 = 36;
/// 色相条 / alpha 条厚度（规格 §3.4 / §3.5：208×12）。
const BAR_H: f32 = 12.0;
/// 条与条之间的垂直间距（规格 §3.4：`space::SM`）。
const BAR_GAP: f32 = space::SM;
/// SV 方块选中指示器半径（规格 §3.3：空心圆环 r=7）。
const SV_CURSOR_R: f32 = 7.0;
/// 色相 / alpha 条指示器半宽（规格 §3.4：三角半宽 6 高 8）。
const BAR_CURSOR_HW: f32 = 6.0;
/// 色相 / alpha 条指示器高。
const BAR_CURSOR_H: f32 = 8.0;
/// hex 输入框高（规格 §3.6：24px）。
const HEX_H: f32 = 24.0;
/// hex 输入框左右内边距（规格 §3.6：6px）。
const HEX_PAD_X: f32 = 6.0;
/// 棋盘格格子边长（规格 §3.1：8px）。
const CHECKER_SIZE: f32 = 8.0;

// ── 纯函数：hex 编解码（规格 §6.2）──────────────────────────────────────

/// 颜色 → 大写 `#RRGGBB`（alpha < 1 时 `#RRGGBBAA`）。
///
/// 大写口径与 `wv_core::color::Color` 消费侧的 `format!("#{:02X}…")` 一致；
/// alpha 恰为 1 时不输出第四对（势力色等不透明场景拿到干净 6 位串）。
pub fn rgba_to_hex(c: egui::Rgba) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// 宽容解析 hex → 颜色。接受 `#RRGGBB` / `#RRGGBBAA`（6 或 8 位），
/// `#` 前缀可省、大小写不敏感；非法返回 `None`（**不 panic**，AGENTS §7.5 V1）。
///
/// 口径与 `wv_core::color::Color::to_rgb` 对齐：**不接受 3 位简写**
/// （太易与路径片段 / 代号混淆，wv-core 侧已裁定）。
pub fn hex_to_rgba(s: &str) -> Option<egui::Rgba> {
    let t = s.trim().trim_start_matches('#');
    match t.len() {
        6 => {
            let r = u8::from_str_radix(&t[0..2], 16).ok()?;
            let g = u8::from_str_radix(&t[2..4], 16).ok()?;
            let b = u8::from_str_radix(&t[4..6], 16).ok()?;
            Some(egui::Rgba::from_srgba_unmultiplied(r, g, b, 255))
        }
        8 => {
            let r = u8::from_str_radix(&t[0..2], 16).ok()?;
            let g = u8::from_str_radix(&t[2..4], 16).ok()?;
            let b = u8::from_str_radix(&t[4..6], 16).ok()?;
            let a = u8::from_str_radix(&t[6..8], 16).ok()?;
            Some(egui::Rgba::from_srgba_unmultiplied(r, g, b, a))
        }
        _ => None,
    }
}

// ── 组件（规格 §6.2 的 API 形态）───────────────────────────────────────

/// 取色器控件：色块触发器 + Popup 浮层。
///
/// ## 用法
/// ```no_run
/// # use wv_ui::shell::UiShell;
/// # use wv_ui::widgets::ColorPicker;
/// # fn demo(ui: &mut bevy_egui::egui::Ui, shell: &UiShell) {
/// let mut color = bevy_egui::egui::Rgba::from_rgba_unmultiplied(0.5, 0.3, 0.2, 1.0);
/// let resp = ColorPicker::new("my_picker", &mut color).show(ui, shell);
/// if resp.changed() {
///     // …提交编辑命令
/// }
/// # }
/// ```
///
/// ## 状态管理
/// HSV 编辑态跨帧存活在 egui memory（键 = 控件 id）——immediate-mode 控件
/// 没有别的地方放「拖色相条时保住 s/v」这类状态。egui 内置取色器用
/// `ctx.data` 缓存同款信息（保证灰色系拖 hue 时不跳变），此处同思路。
pub struct ColorPicker<'a> {
    id: egui::Id,
    color: &'a mut egui::Rgba,
    alpha: bool,
}

impl<'a> ColorPicker<'a> {
    /// 构造：控件 id（同一界面多个取色器用不同 salt 互不串扰）+ 颜色引用。
    pub fn new(id_salt: impl egui::AsId, color: &'a mut egui::Rgba) -> Self {
        Self {
            id: egui::Id::new(id_salt),
            color,
            alpha: true,
        }
    }

    /// 是否显示 alpha 条（默认 `true`）。不透明场景（如势力色 `#RRGGBB`）
    /// 传 `false`，浮层高度相应收 20px（规格 §3.5）。
    pub fn with_alpha(mut self, on: bool) -> Self {
        self.alpha = on;
        self
    }

    /// 装配并落地。返回触发器 [`egui::Response`]（`.changed()` = 颜色被改过）。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let Self { id, color, alpha } = self;

        // ── 触发器：18px 色块居中于 24px 命中区（规格 §3.1）──
        let (hit_rect, mut resp) =
            ui.allocate_exact_size(egui::vec2(TRIGGER_HIT, TRIGGER_HIT), egui::Sense::click());
        let block =
            egui::Rect::from_center_size(hit_rect.center(), egui::vec2(TRIGGER_SIZE, TRIGGER_SIZE));
        paint_color_block(ui.painter(), shell, block, *color);

        // hover / open 态：描边换 accent（底色是用户选的色，不能被污染）。
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let stroke = if resp.hovered() || open {
            egui::Stroke::new(1.0, shell.accent())
        } else {
            egui::Stroke::new(1.0, shell.border_strong())
        };
        ui.painter()
            .rect_stroke(block, radius::BADGE, stroke, egui::StrokeKind::Inside);

        // ── 浮层（规格 §3.2：Popup + 定宽 + 令牌化 Frame）──
        let color_ptr: *mut egui::Rgba = color;
        let mut inner_changed = false;
        egui::Popup::from_toggle_button_response(&resp)
            .id(id)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(POPUP_W);
                ui.set_max_width(POPUP_W);
                egui::Frame::NONE
                    .fill(shell.float_fill())
                    .stroke(egui::Stroke::new(1.0, shell.border_strong()))
                    .corner_radius(egui::CornerRadius::from(radius::PANEL))
                    .inner_margin(egui::Margin::same(POPUP_PAD as i8))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(71),
                    })
                    .show(ui, |ui| {
                        // 安全：闭包内只经 `color_ptr` 触碰颜色（借用检查的
                        // 常规出路，与 `split_button::split_button` 的
                        // `picked` 同款模式）。
                        inner_changed =
                            popup_body(ui, shell, id, unsafe { &mut *color_ptr }, alpha);
                    });
            });
        if inner_changed {
            resp.mark_changed();
        }
        resp
    }
}

/// 浮层内容：SV 方块 → 色相条 → alpha 条（可选）→ hex 输入。
///
/// 返回「颜色是否被改过」。HSV 编辑态存 egui memory：直接从 `Rgba` 反推
/// HSV 会在灰彩色上丢失 hue（s=0 时 hue 无定义），跨帧缓存才能让用户
/// 「停在白色往右拖立刻出红色」（egui 内置同款处理）。
fn popup_body(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: egui::Id,
    color: &mut egui::Rgba,
    alpha: bool,
) -> bool {
    // 区块间距全部由显式 `add_space(BAR_GAP)` 控制（规格 §3.4/3.5/3.6 的
    // 8px）：压掉 egui 默认 item_spacing.y 的叠加，否则实际间距 = 8+3，
    // 浮层高度会随默认样式漂移。
    ui.spacing_mut().item_spacing.y = 0.0;

    let hsva_id = id.with("hsva");
    let mut hsva = ui
        .ctx()
        .memory(|m| m.data.get_temp::<Hsva>(hsva_id))
        .unwrap_or_else(|| Hsva::from(*color));

    let mut changed = false;

    // ── SV 方块（规格 §3.3）──
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(SV_SIZE, SV_SIZE), egui::Sense::click_and_drag());
    if let Some(pos) = resp.interact_pointer_pos() {
        let s = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        // 屏幕 y 向下，SV 方块顶边是 v=1 —— 反向映射。
        let v = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        if (hsva.s - s).abs() > 1e-4 || (hsva.v - v).abs() > 1e-4 {
            hsva.s = s;
            hsva.v = v;
            changed = true;
        }
    }
    {
        let mut mesh = egui::Mesh::default();
        let n = MESH_N as i32;
        let hue = hsva.h;
        for yi in 0..=n {
            for xi in 0..=n {
                let xt = xi as f32 / n as f32;
                let vt = 1.0 - yi as f32 / n as f32;
                let c = Hsva {
                    h: hue,
                    s: xt,
                    v: vt,
                    a: 1.0,
                }
                .to_srgba_unmultiplied();
                let x = egui::lerp(rect.left()..=rect.right(), xt);
                let y = egui::lerp(rect.bottom()..=rect.top(), vt);
                mesh.colored_vertex(
                    egui::pos2(x, y),
                    egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 255),
                );
                if xi < n && yi < n {
                    let tl = (yi * (n + 1) + xi) as u32;
                    mesh.add_triangle(tl, tl + 1, tl + (n + 1) as u32);
                    mesh.add_triangle(tl + 1, tl + (n + 1) as u32, tl + (n + 2) as u32);
                }
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, shell.border_subtle()),
            egui::StrokeKind::Inside,
        );
        // 选中指示器：空心圆环，填充 = 选中色，描边 = 对比色（规格 §3.3）。
        let cx = egui::lerp(rect.left()..=rect.right(), hsva.s);
        let cy = egui::lerp(rect.bottom()..=rect.top(), hsva.v);
        let picked = egui::Color32::from_rgba_unmultiplied(
            hsva.to_srgba_unmultiplied()[0],
            hsva.to_srgba_unmultiplied()[1],
            hsva.to_srgba_unmultiplied()[2],
            255,
        );
        ui.painter().add(egui::Shape::circle_stroke(
            egui::pos2(cx, cy),
            SV_CURSOR_R,
            egui::Stroke::new(2.0, contrast_of(picked)),
        ));
        // 环内填充选中色，环本身用对比色描边（两层合成「空心环 + 色芯」）。
        ui.painter()
            .circle_filled(egui::pos2(cx, cy), SV_CURSOR_R - 2.0, picked);
    }
    ui.add_space(BAR_GAP);

    // ── 色相条（规格 §3.4）──
    {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(SV_SIZE, BAR_H), egui::Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos() {
            let h = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            if (hsva.h - h).abs() > 1e-4 {
                hsva.h = h;
                changed = true;
            }
        }
        let mut mesh = egui::Mesh::default();
        for i in 0..=MESH_N as i32 {
            let t = i as f32 / MESH_N as f32;
            let c = Hsva {
                h: t,
                s: 1.0,
                v: 1.0,
                a: 1.0,
            }
            .to_srgba_unmultiplied();
            let x = egui::lerp(rect.left()..=rect.right(), t);
            mesh.colored_vertex(
                egui::pos2(x, rect.top()),
                egui::Color32::from_rgb(c[0], c[1], c[2]),
            );
            mesh.colored_vertex(
                egui::pos2(x, rect.bottom()),
                egui::Color32::from_rgb(c[0], c[1], c[2]),
            );
            if i < MESH_N as i32 {
                let b = (2 * i) as u32;
                mesh.add_triangle(b, b + 1, b + 2);
                mesh.add_triangle(b + 1, b + 2, b + 3);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, shell.border_subtle()),
            egui::StrokeKind::Inside,
        );
        // 指示器：实心三角（规格 §3.4：半宽 6 高 8，顶点朝上贴条下沿）。
        let x = egui::lerp(rect.left()..=rect.right(), hsva.h);
        let pure = egui::Color32::from_rgb(
            hsva.to_srgba_unmultiplied()[0],
            hsva.to_srgba_unmultiplied()[1],
            hsva.to_srgba_unmultiplied()[2],
        );
        let tip = egui::pos2(x, rect.bottom() - BAR_CURSOR_H);
        let lb = egui::pos2(x - BAR_CURSOR_HW, rect.bottom());
        let rb = egui::pos2(x + BAR_CURSOR_HW, rect.bottom());
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tip, lb, rb],
            pure,
            egui::Stroke::new(1.0, contrast_of(pure)),
        ));
    }

    // ── alpha 条（规格 §3.5，可关）──
    if alpha {
        ui.add_space(BAR_GAP);
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(SV_SIZE, BAR_H), egui::Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos() {
            let a = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            if (hsva.a - a).abs() > 1e-4 {
                hsva.a = a;
                changed = true;
            }
        }
        paint_checkers(ui.painter(), shell, rect);
        // 渐变：左端不透明 → 右端全透明的当前色层（顶点色 alpha 插值）。
        let cur = Hsva { a: 1.0, ..hsva }.to_srgba_unmultiplied();
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(
            rect.left_top(),
            egui::Color32::from_rgba_unmultiplied(cur[0], cur[1], cur[2], 255),
        );
        mesh.colored_vertex(
            rect.left_bottom(),
            egui::Color32::from_rgba_unmultiplied(cur[0], cur[1], cur[2], 255),
        );
        mesh.colored_vertex(
            rect.right_top(),
            egui::Color32::from_rgba_unmultiplied(cur[0], cur[1], cur[2], 0),
        );
        mesh.colored_vertex(
            rect.right_bottom(),
            egui::Color32::from_rgba_unmultiplied(cur[0], cur[1], cur[2], 0),
        );
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 2, 3);
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, shell.border_subtle()),
            egui::StrokeKind::Inside,
        );
        let x = egui::lerp(rect.left()..=rect.right(), hsva.a);
        let with_a =
            egui::Color32::from_rgba_unmultiplied(cur[0], cur[1], cur[2], (hsva.a * 255.0) as u8);
        let tip = egui::pos2(x, rect.bottom() - BAR_CURSOR_H);
        let lb = egui::pos2(x - BAR_CURSOR_HW, rect.bottom());
        let rb = egui::pos2(x + BAR_CURSOR_HW, rect.bottom());
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tip, lb, rb],
            with_a,
            egui::Stroke::new(1.0, contrast_of(with_a)),
        ));
    }

    // 写回：hsva → color（仅在色板被拖动时；hex 输入直接改 color，不走这里）。
    if changed {
        let a = if alpha { hsva.a } else { 1.0 };
        *color = egui::Rgba::from(Hsva { a, ..hsva });
    }
    ui.ctx().memory_mut(|m| m.data.insert_temp(hsva_id, hsva));

    // ── hex 输入（规格 §3.6：与色板双向同步 + 非法宽容）──
    ui.add_space(BAR_GAP);
    let hex_id = id.with("hex");
    let hex_valid_id = id.with("hex_valid");
    // 上帧文本是否合法（合法才回写色板；非法保留文本不动色板）。
    let mut hex_text = ui
        .ctx()
        .memory(|m| m.data.get_temp::<String>(hex_id))
        .unwrap_or_else(|| rgba_to_hex(*color));
    // 色板侧刚变过 → 回写文本（覆盖手打内容；changed 只在拖动帧为真，
    // 与「文本侧改动」互斥，不会形成回写循环）。
    if changed {
        hex_text = rgba_to_hex(*color);
    }

    let (rect, _) = ui.allocate_exact_size(egui::vec2(SV_SIZE, HEX_H), egui::Sense::hover());
    let valid = hex_to_rgba(&hex_text).is_some();
    let stroke_color = if valid {
        shell.border_subtle()
    } else {
        shell.danger()
    };
    let p = ui.painter();
    p.rect_filled(rect, radius::FIELD, shell.input_bg());
    p.rect_stroke(
        rect,
        radius::FIELD,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    let _ = p;

    let text_color = if valid { shell.t1() } else { shell.danger() };
    let resp = ui.put(
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x + HEX_PAD_X, rect.min.y),
            egui::pos2(rect.max.x - HEX_PAD_X, rect.max.y),
        ),
        egui::TextEdit::singleline(&mut hex_text)
            .id(hex_id)
            .font(mono(font_size::MICRO))
            .desired_width(f32::INFINITY)
            .background_color(egui::Color32::TRANSPARENT)
            .margin(egui::Margin::same(0))
            .frame(egui::Frame::NONE)
            .text_color(text_color),
    );
    if resp.changed() {
        if let Some(c) = hex_to_rgba(&hex_text) {
            // 文本侧合法改动 → 写色板（alpha 关闭时钉死 1.0）。
            *color = if alpha {
                c
            } else {
                egui::Rgba::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 1.0)
            };
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(hsva_id, Hsva::from(*color)));
        }
        // 非法（打到一半）：不动色板、不 panic —— 只留危险色提示（§7.5 V1）。
    }
    ui.ctx()
        .memory_mut(|m| m.data.insert_temp(hex_id, hex_text));
    ui.ctx()
        .memory_mut(|m| m.data.insert_temp(hex_valid_id, valid));

    changed || resp.changed()
}

// ── 绘制 helper ─────────────────────────────────────────────────────────

/// 画一个色块：不透明直填；半透明先铺棋盘格再叠色（规格 §3.1）。
fn paint_color_block(
    painter: &egui::Painter,
    shell: &UiShell,
    rect: egui::Rect,
    color: egui::Rgba,
) {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 {
        painter.rect_filled(rect, radius::BADGE, egui::Color32::from_rgb(r, g, b));
    } else {
        paint_checkers(painter, shell, rect);
        painter.rect_filled(
            rect,
            radius::BADGE,
            egui::Color32::from_rgba_unmultiplied(r, g, b, a),
        );
    }
}

/// 棋盘格底（透明度指示）：两档灰走令牌（规格 §3.1 裁定）——
/// `input_bg`（暗档）与 `raised_bg`（亮档），明暗主题自动成立，
/// 不用 egui 内置的裸 `from_gray(32)/from_gray(128)`（亮色下几乎不可见）。
fn paint_checkers(painter: &egui::Painter, shell: &UiShell, rect: egui::Rect) {
    let dark = shell.input_bg();
    let bright = shell.raised_bg();
    let mut mesh = egui::Mesh::default();
    mesh.add_colored_rect(rect, dark);
    let mut top = true;
    let mut x = rect.left();
    let mut i = 0u32;
    while x < rect.right() {
        let r = egui::Rect::from_min_size(
            if top {
                egui::pos2(x, rect.top())
            } else {
                egui::pos2(x, rect.center().y)
            },
            egui::vec2(CHECKER_SIZE, CHECKER_SIZE),
        );
        mesh.add_colored_rect(r, bright);
        top = !top;
        i += 1;
        x = rect.left() + i as f32 * CHECKER_SIZE;
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// 取一色的对比描边色：亮度高用黑、低用白——指示器在同色系区域内
/// 仍可见（egui 内置 `contrast_color` 同思路，此处避免依赖其内部路径）。
fn contrast_of(c: egui::Color32) -> egui::Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    // 简化相对亮度（Rec.601 加权）：足够区分「该用黑还是白」。
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 150.0 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    }
}

// ── 测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! 判据四条对照（AGENTS §7 / 专家判据）：期望值来自手算常量与规格表，
    //! 不引用被测常量自证；数值断言附实际值；负向断言（非法输入不改色）。

    use super::*;
    use crate::widgets::test_support::{make_ui, shell};

    // ── hex 编解码（纯函数，期望值全部手算）────────────────────────────

    #[test]
    fn hex_round_trips_opaque_and_transparent() {
        // 手算：#7AB8FF = (122, 184, 255, 255)。
        // 不透明路径字节精确（Color32 from_rgb 直存，无预乘中转）。
        let c = egui::Rgba::from_srgba_unmultiplied(0x7A, 0xB8, 0xFF, 255);
        assert_eq!(rgba_to_hex(c), "#7AB8FF");
        assert_eq!(hex_to_rgba("#7AB8FF"), Some(c));
        // 不透明往返恒等（6 位 ↔ Rgba 严格保序）。
        assert_eq!(rgba_to_hex(hex_to_rgba("#00FF99").unwrap()), "#00FF99");

        // 带 alpha：egui 的 Rgba 在 alpha∉{0,255} 时经预乘 u8 中转
        //（`Color32::from_rgba_unmultiplied` 的 mul_frac_round），
        // 往返存在 ±1 量化（0xB8×0x80/255 → 0xB7）。这是 egui float
        // 色彩管线的固有行为，不是本控件的 bug——断言按「解析正确 +
        // 往返不漂超 1」验收（诚实记录优于假装精确）。
        let parsed = hex_to_rgba("#7AB8FF80");
        let [r, g, b, a] = parsed.expect("8 位串应解析成功").to_srgba_unmultiplied();
        assert_eq!((r, b, a), (0x7A, 0xFF, 0x80), "r/b/a 字节精确");
        assert!(
            (g as i32 - 0xB8).abs() <= 1,
            "g 在预乘往返 ±1 内，got {g:02X}"
        );
        let round = rgba_to_hex(parsed.unwrap());
        assert_eq!(round.len(), 9, "带 alpha 输出 8 位串，got {round}");
    }

    #[test]
    fn hex_parsing_is_lenient_about_case_and_prefix() {
        let upper = hex_to_rgba("#AABBCC");
        let lower = hex_to_rgba("#aabbcc");
        let no_hash = hex_to_rgba("AABBCC");
        assert_eq!(upper, lower, "大小写应等价");
        assert_eq!(upper, no_hash, "# 前缀应可省");
        assert_eq!(
            upper,
            Some(egui::Rgba::from_srgba_unmultiplied(0xAA, 0xBB, 0xCC, 255))
        );
    }

    #[test]
    fn invalid_hex_returns_none_without_panic() {
        // AGENTS §7.5 V1：解析失败只能 None，不得 panic。
        for bad in [
            "",
            " ",
            "#",
            "#0",
            "#12",
            "#12345",
            "#1234567",
            "#123456789",
            "#GGGGGG",
            "not-a-color",
            "#12 4567",
        ] {
            assert_eq!(hex_to_rgba(bad), None, "「{bad}」应判非法");
        }
    }

    #[test]
    fn three_digit_shorthand_is_rejected() {
        // 与 wv-core::Color::to_rgb 同口径：3 位简写不接受。
        assert_eq!(hex_to_rgba("#FFF"), None);
        assert_eq!(hex_to_rgba("#abc"), None);
    }

    #[test]
    fn opaque_color_formats_six_digits_not_eight() {
        // alpha=255 时输出 6 位（不透明场景拿到干净串）。
        let c = egui::Rgba::from_srgba_unmultiplied(0, 0, 0, 255);
        assert_eq!(rgba_to_hex(c), "#000000");
        assert_eq!(rgba_to_hex(c).len(), 7, "got {}", rgba_to_hex(c));
    }

    // ── 触发器几何 ──────────────────────────────────────────────────────

    #[test]
    fn trigger_occupies_hit_square_not_visual_block() {
        // 规格表：命中区 24（视觉块 18 居中）。期望值来自规格，非被测常量。
        let sh = shell();
        let (_c, mut ui) = make_ui("cp_geom", 200.0, 100.0);
        let mut c = egui::Rgba::from_rgba_unmultiplied(1.0, 0.0, 0.0, 1.0);
        let r = ColorPicker::new("cp", &mut c).show(&mut ui, &sh);
        assert!(
            (r.rect.width() - 24.0).abs() < 1e-3,
            "命中区 24，got {}",
            r.rect.width()
        );
        assert!(
            (r.rect.height() - 24.0).abs() < 1e-3,
            "got {}",
            r.rect.height()
        );
    }

    // ── 非法输入不改色（负向断言）──────────────────────────────────────

    #[test]
    fn invalid_hex_keeps_previous_legal_color() {
        // §7.5 V1：打到一半（如 "#0"）不得改色。直接驱动 popup_body 太重，
        // 这里测它的写值前提：hex_to_rgba 非法 → 不存在可写值。
        let mut color = egui::Rgba::from_srgba_unmultiplied(0x12, 0x34, 0x56, 255);
        let before = color;
        for bad in ["#0", "#12", "#12G"] {
            if let Some(c) = hex_to_rgba(bad) {
                color = c; // 不该走到这里
            }
        }
        assert_eq!(color, before, "非法串一律不得写回色板（负向：一次都不写）");
    }

    // ── 浮层：装配不崩 + alpha 关闭时高度差 20（规格 §3.5/§3.7）──────

    #[test]
    fn popup_body_renders_with_and_without_alpha_without_panic() {
        let sh = shell();
        let mut heights = Vec::new();
        for (name, alpha) in [("with_alpha", true), ("no_alpha", false)] {
            let (_c, mut ui) = make_ui(name, 320.0, 400.0);
            let mut c = egui::Rgba::from_rgba_unmultiplied(0.2, 0.4, 0.6, 0.8);
            let id = egui::Id::new(name);
            let _ = popup_body(&mut ui, &sh, id, &mut c, alpha);
            heights.push(ui.min_rect().height());
        }
        let [h_on, h_off] = [heights[0], heights[1]];
        // 规格真正钉死的量（§3.5/§3.7）：关闭 alpha 条，内容高恰减
        // 12（条）+ 8（条上间距）= 20。绝对高度受 egui item_spacing
        // 影响，差值才是规格约束（独立复算：12 + 8 = 20）。
        assert!(
            (h_on - h_off - 20.0).abs() < 1.5,
            "alpha 开关高度差应 = 20（12 条 + 8 间距），got {} - {} = {}",
            h_on,
            h_off,
            h_on - h_off
        );
    }

    /// 真点击链路（三帧驱动，范式同 segmented_control）：拖 SV 方块
    /// 左上角（s≈0, v≈1 → 白）后颜色确实改变。
    #[test]
    fn sv_drag_changes_color() {
        let sh = shell();
        // popup 内容画在 (0,0) 起的 Ui 里（Frame 内边距 12 → SV 方块从
        // (12,12) 起）。点 (14, 14) = 方块左上角 → s≈0 / v≈1（近白）。
        let pos = egui::pos2(14.0, 14.0);
        let mut c = egui::Rgba::from_rgba_unmultiplied(1.0, 0.0, 0.0, 1.0);
        let ctx = egui::Context::default();
        let id = egui::Id::new("sv_drag");
        // 先跑一帧建立状态，再驱动拖拽（按下 + 移动 + 抬起）。
        for frame in 0..4 {
            let events = match frame {
                0 => vec![egui::Event::PointerMoved(pos)],
                1 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                2 => vec![egui::Event::PointerMoved(pos)],
                _ => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 400.0),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default())
                    .show(ctx, |ui| {
                        let _ = popup_body(ui, &sh, id, &mut c, false);
                    });
            });
            output.textures_delta.clear();
        }
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        // 红色 (255,0,0) → 拖到左上角应变成近白（s≈0 ⇒ RGB 三通道接近）。
        assert!(
            r > 200 && g > 200 && b > 200,
            "SV 左上角应近白，got ({r},{g},{b})"
        );
    }
}
