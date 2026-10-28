//! 「地图属性」对话框（2026-10-27 用户需求：没有地方看 / 改地图属性）。
//!
//! ## 职责
//! - [`MapPropertiesDialog`] —— 跨帧 UI 状态（打开标志 + 目标图 + 名称 /
//!   尺度声明草稿）。
//! - [`map_properties_dialog`] —— 绘制 + 确认提交（[`wv_edit::UpdateMapPropsCommand`]
//!   经 `History` 提交，同帧即时落盘）。
//!
//! ## 可改什么（与命令的边界一致）
//! 只有**名称**与**比例声明 `unit_per_km`**——前者是显示名（ID 不改，那是
//! YAML/Git 引用锚），后者是视口比例尺 / 陆地尺度提示的唯一换算入参。
//! 其余属性（ID / 层级 / 幅面 / 栅格 / 每格尺寸）**只读展示**：它们是结构
//! 数据，改等于换一张图（各有专属命令或重导入路径）。
//!
//! ## 边界（不做什么）
//! - **不**直接改 `projection.world` —— 走 `wv_edit` 命令（§3.3）。
//! - **不**管入口 —— 工程菜单 / 命令面板只排队 `CommandId::MapProperties`，
//!   由唯一分发器调 [`MapPropertiesDialog::open_for_node`]。
//! - 对话框体内文案沿用「新建地图」对话框的中文直写先例（i18n 全量收口
//!   是另一笔既有债，不在本轮 scope）。
//!
//! ## 主要导出
//! - `MapPropertiesDialog` / `map_properties_dialog` —— Bevy 资源与系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use wv_core::map::MapNode;
use wv_core::Id;

use crate::edit::editing::EditHistory;
use crate::i18n::CurrentLocale;
use crate::state::{map_level_label, ProjectDiagnostics, WorldProjection};
use crate::theme::ui_theme::{mono, UiShellConfig};
use crate::ui::map_scale_bar::format_km_flexible;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};
use crate::ui_shell::ShellNotice;

use super::shell::UiShell;

/// 「地图属性」对话框的跨帧状态（资源：分发器拿不到 egui memory，见
/// `NewMapDialog` 同款理由）。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct MapPropertiesDialog {
    /// 是否打开。
    pub open: bool,
    /// 目标图（打开时钉住；运行中图被删则对话框自动失效关閉）。
    pub map_id: Option<Id>,
    /// 名称草稿。
    pub name: String,
    /// 比例声明草稿（每公里多少世界单位；**空串 = 未声明**）。
    pub unit_per_km_text: String,
    /// 地图备注草稿（ADR-0082 LLM 命名风格来源；多行长文本）。
    pub comment: String,
    /// 上次保存失败的错误（留在框内，作者改完再试；成功即清）。
    pub error: Option<String>,
}

impl MapPropertiesDialog {
    /// 由分发器调用：按目标图现状预填。
    pub fn open_for_node(&mut self, node: &MapNode) {
        self.open = true;
        self.map_id = Some(node.id.clone());
        self.name = node.name.clone();
        self.unit_per_km_text = node.unit_per_km.map(|v| v.to_string()).unwrap_or_default();
        self.comment = node.comment.clone().unwrap_or_default();
        self.error = None;
    }

    /// 解析比例声明输入：空串 = `None`（未声明）；其余必须是非正有限数之外
    /// 的正数——0 / 负数 / 乱码都报错留在框内，不静默清声明。
    pub fn parsed_unit_per_km(&self) -> Result<Option<f32>, String> {
        let s = self.unit_per_km_text.trim();
        if s.is_empty() {
            return Ok(None);
        }
        match s.parse::<f32>() {
            Ok(v) if v.is_finite() && v > 0.0 => Ok(Some(v)),
            _ => Err("比例必须是正数（每公里多少世界单位）；留空表示未声明".into()),
        }
    }
}

/// 「地图属性」对话框：绘制 + 确认提交。
#[allow(clippy::too_many_arguments)]
pub fn map_properties_dialog(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<MapPropertiesDialog>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut diagnostics: ResMut<ProjectDiagnostics>,
    mut notice: ResMut<ShellNotice>,
    grid: Res<crate::state::GridConfig>,
    current_loc: Res<CurrentLocale>,
    time: Res<Time>,
) {
    if !dialog.open {
        return;
    }
    let Some(map_id) = dialog.map_id.clone() else {
        dialog.open = false;
        return;
    };
    // 目标图运行中被删（并发编辑 / 撤销）：静默关框，不渲染悬空属性。
    let Some(node) = projection.world.maps.iter().find(|m| m.id == map_id) else {
        dialog.open = false;
        return;
    };
    let node = node.clone();
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let frame = (time.elapsed_secs() * 60.0) as u64;
    let mut open = dialog.open;
    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new("地图属性")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        // 模态置顶（同 new_map_dialog：Foreground 压得住 Middle 浮面板）。
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // ── 只读区：结构属性（ID / 层级 / 幅面 / 栅格 / 每格尺寸）──
            ui.label(egui::RichText::new("ID").size(11.0).color(shell.t3()));
            ui.label(
                egui::RichText::new(node.id.as_str())
                    .font(mono(11.0))
                    .color(shell.t2()),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("层级").size(11.0).color(shell.t3()));
                ui.label(map_level_label(node.level, current_loc.0));
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("幅面").size(11.0).color(shell.t3()));
                match &node.extent {
                    Some(ext) => ui.label(format!("{:.0} × {:.0} wu", ext.width, ext.height)),
                    None => ui.label("未设置"),
                };
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("栅格").size(11.0).color(shell.t3()));
                match projection.world.terrain_of(&node.id) {
                    Some(t) if t.width > 0 => {
                        ui.label(format!("{} × {} 格", t.width, t.height))
                            .on_hover_text(format!("每格 {:.1} wu（会话推导）", grid.cell_size));
                    }
                    _ => {
                        ui.label("无");
                    }
                };
            });
            // 每格真实尺寸 = 数据分辨率（作者最常追问的数；比例尺 tooltip 同源）。
            if let Some((kx, _)) = node.km_per_wu() {
                ui.label(
                    egui::RichText::new(format!(
                        "每格 ≈ {}（数据分辨率）",
                        format_km_flexible(f64::from(grid.cell_size) * kx)
                    ))
                    .size(11.0)
                    .color(shell.t3()),
                );
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // ── 可改区：名称 / 比例声明 ──
            ui.horizontal(|ui| {
                ui.label("名称")
                    .on_hover_text("显示名，可中文；ID 不随改名变化");
                let mut name = dialog.name.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .desired_width(220.0)
                            .hint_text("地图名"),
                    )
                    .changed()
                {
                    dialog.name = name.trim().to_owned();
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("比例").on_hover_text(
                    "每公里 = 多少世界单位（作者自报）。视口比例尺、陆地尺度提示\
                     都依此换算；留空 = 未声明，比例尺不显示。示例：4 = 4 wu/km",
                );
                let mut text = dialog.unit_per_km_text.clone();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(90.0)
                        .hint_text("未声明"),
                );
                if resp.changed() {
                    dialog.unit_per_km_text = text.trim().to_owned();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirmed = true;
                }
            });
            ui.add_space(4.0);
            // 备注（ADR-0082）：多行长文本——LLM 给本图自然区域命名的风格
            // 来源之一（与工程 meta.comment 并读）；存进 maps/<id>/index.yaml。
            ui.label(egui::RichText::new("备注").size(11.0).color(shell.t3()))
                .on_hover_text("这张图的基调 / 风格 / 语境自由描述——AI 命名自然区域时会参考它");
            let mut comment = dialog.comment.clone();
            ui.add(
                egui::TextEdit::multiline(&mut comment)
                    .desired_width(f32::INFINITY)
                    .desired_rows(3)
                    .hint_text("例：东亚风，多山，河谷文明……"),
            );
            dialog.comment = comment;

            if let Some(err) = &dialog.error {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(err).color(egui::Color32::LIGHT_RED));
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("取消").clicked() {
                    cancelled = true;
                }
                if widget_button(
                    ui,
                    &shell,
                    ButtonContent::Text("保存"),
                    ButtonStyle::Accent,
                    None,
                    true,
                )
                .clicked()
                {
                    confirmed = true;
                }
            });
        });

    // X（`open(&mut open)` 写回）与「取消」都走关；保存失败时留在框内。
    dialog.open = open && !cancelled;
    if !dialog.open {
        return;
    }
    if confirmed {
        let name = if dialog.name.trim().is_empty() {
            node.name.clone() // 空名不收——退回原名（改名不是清名）
        } else {
            dialog.name.trim().to_owned()
        };
        let mut commit = || -> Result<Vec<String>, String> {
            let unit = dialog.parsed_unit_per_km()?;
            let mut tx = wv_edit::Transaction::new();
            tx.push(Box::new(wv_edit::UpdateMapPropsCommand::with_comment(
                map_id.clone(),
                name.clone(),
                unit,
                Some(dialog.comment.trim().to_owned()).filter(|c| !c.is_empty()),
            )));
            edit.history
                .commit(&mut projection.world, tx)
                .map_err(|e| format!("保存失败：{e}"))
        };
        match commit() {
            Ok(dirty) => {
                edit.record_dirty(dirty.clone());
                projection.dirty = true;
                diagnostics.needs_diagnose = true;
                // 即时落盘（同 create_map：改属性 = 改文件）；失败留脏标记补写。
                if let Some(dir) = projection.project_dir.clone() {
                    if let Err(e) = wv_io::Project::open(&dir)
                        .and_then(|mut p| p.save_world(&projection.world, &dirty))
                    {
                        notice.error(format!("落盘失败：{e}（已留脏标记，下次保存补写）"), frame);
                    }
                }
                notice.info(format!("已更新地图「{name}」"), frame);
                dialog.open = false;
            }
            Err(e) => dialog.error = Some(e),
        }
    }
}
