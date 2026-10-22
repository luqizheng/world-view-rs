//! 「划分势力」对话框（ADR-0028 / 势力交互 M1 Task 6）。
//!
//! ## 职责
//! - [`FactionDivisionDialog`] —— 跨帧 UI 状态：打开标志、名称、模式、宿主、
//!   以及**捕获那一刻钉住的意图环 + 图 id**。
//! - [`resolve_host`] —— 由意图环质心解析宿主势力（面积最小的含入实控）。
//! - [`faction_divide_dialog`] —— 绘制 + 确认提交：经
//!   [`crate::edit::faction_panel_actions::divide_faction_now`] 落成
//!   `DivideFactionCommand`；成功后清空选区。
//!
//! ## 打开 / 关闭的单一来源
//!
//! 与「新建地理事实」对话框同构：只由选区意图菜单的「划分势力」按钮打开
//! （[`super::intent_menu`] 调 [`FactionDivisionDialog::open_with`]）。
//! 环坐标只在捕获图的局部帧里合法，钉住的图 id 永远正确。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界——提交走动作层的 `EditCommand`（AGENTS §3.3）；
//! - **不**画选区几何——那是 `wv_render::selection::draw_selection`；
//! - **不**硬编码文案——标题 / 标签 / 按钮全走 `I18nKey`。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing::EditHistory;
use crate::edit::faction_panel_actions::divide_faction_now;
use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::WorldProjection;
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};
use wv_edit::command::DivisionMode;

use super::shell::UiShell;

/// 「划分势力」对话框的跨帧状态。
///
/// 环与地图 id **自持**（不读渲染层会话态）：意图按钮点击那一刻把
/// `SelectionState.closed` 与 `drill.current()` 拷进来，之后选区怎么变、
/// 地图怎么下钻都与这次提交无关。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct FactionDivisionDialog {
    /// 是否打开。
    pub open: bool,
    /// 作者输入的名称（提交前 trim；空名禁用「创建」）。
    pub name: String,
    /// 闭合那一刻钉住的图 id；`None` = 无待提交环。
    pub map: Option<wv_core::Id>,
    /// 待划分的意图环（**捕获图局部 wu**，首尾不重复，≥3 点）。
    pub ring: Vec<(f32, f32)>,
    /// 边界模式（缺省贴地）。
    pub mode: DivisionMode,
    /// 宿主势力；`None` = 空势力（全图陆地）。
    pub host: Option<wv_core::Id>,
    /// 捕获那一刻钉住的格尺寸（wu/格）——打开后作者可能下钻换图，
    /// 提交时必须用捕获图的口径（M6）。
    pub cell_size: f32,
    /// 捕获那一刻钉住的时间轴日（宿主解析与 span.from 同一天）。
    pub from: i64,
}

impl FactionDivisionDialog {
    /// 用选区环 + 当前图 id + 解析好的宿主打开（意图菜单点击时调用）。
    ///
    /// `snap` 来自地图工具条的 magnet（[`crate::ui::map_controls::MapDockState`]）：
    /// 开启 = 默认「贴地」，关闭 = 默认保留栅格平滑边框。作者仍可在对话框里改。
    pub fn open_with(
        &mut self,
        map: wv_core::Id,
        ring: Vec<(f32, f32)>,
        host: Option<wv_core::Id>,
        cell_size: f32,
        from: i64,
        snap: bool,
    ) {
        self.open = true;
        self.name = "新势力".into();
        self.map = Some(map);
        self.ring = ring;
        self.mode = if snap {
            DivisionMode::Snap
        } else {
            DivisionMode::Straight
        };
        self.host = host;
        self.cell_size = cell_size;
        self.from = from;
    }

    /// 关闭并清空一切（取消 / X / 无工程 / 提交成功）。
    pub fn close(&mut self) {
        self.open = false;
        self.name.clear();
        self.map = None;
        self.ring.clear();
        self.host = None;
        self.cell_size = 0.0;
        self.from = 0;
    }
}

fn ring_area(ring: &[f32]) -> f32 {
    let n = ring.len() / 2;
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0f32;
    for i in 0..n {
        let (x0, y0) = (ring[2 * i], ring[2 * i + 1]);
        let (x1, y1) = (ring[2 * ((i + 1) % n)], ring[2 * ((i + 1) % n) + 1]);
        a += x0 * y1 - x1 * y0;
    }
    (a / 2.0).abs()
}

fn centroid(ring: &[(f32, f32)]) -> Option<(f32, f32)> {
    if ring.len() < 3 {
        return None;
    }
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for &(x, y) in ring {
        sx += x;
        sy += y;
    }
    Some((sx / ring.len() as f32, sy / ring.len() as f32))
}

/// 由意图环质心解析宿主：**面积最小的实控含入势力**（D5「在某个势力中间画」）。
///
/// 只认实控 `span`（`relation == None`）；声称（claim）不算宿主。
/// 质心不在任何势力内 → `None`（= 空势力陆地）。
pub fn resolve_host(
    world: &wv_core::World,
    map: &wv_core::Id,
    ring: &[(f32, f32)],
    day: i64,
) -> Option<wv_core::Id> {
    let (cx, cy) = centroid(ring)?;
    let t = wv_core::time::TimePoint::from_day(day);
    let mut best: Option<(wv_core::Id, f32)> = None;
    for (_, faction_id, relation) in wv_core::faction::query_at_point(world, map, cx, cy, day) {
        // **只认实控命中**（终审 I1）：声称命中不能当宿主——混合场景下
        // （点在声称 span 里、实控 span 在别处）必须跳过，否则会划到
        // 一个并不覆盖该点的实控疆界上。
        if relation.is_some() {
            continue;
        }
        let Some(span) = world.faction_geometry.iter().find(|g| {
            &g.map == map && g.owner == faction_id && g.relation.is_none() && g.span.contains(&t)
        }) else {
            continue;
        };
        // 双重保险：该 span 必须真的覆盖质心（洞内不算）。
        if !wv_core::geometry::point_in_geometry(&span.geometry, cx, cy) {
            continue;
        }
        let area = match &span.geometry {
            wv_core::map::GeometryData::Polygon(v) => ring_area(v),
            wv_core::map::GeometryData::PolygonWithHoles { outer, .. } => ring_area(outer),
            _ => continue,
        };
        if best.as_ref().map(|(_, a)| area < *a).unwrap_or(true) {
            best = Some((faction_id, area));
        }
    }
    best.map(|(id, _)| id)
}

/// 空势力路径的重叠提示（终审 I6）：意图环与既有实控疆界是否明显重叠。
///
/// 7×7 采样（确定性、便宜）；≥5% 采样点落在既有势力内 → true。
/// V1 只提示不阻止：作者可能正是要画出交叠的争议区。
pub fn ring_overlaps_existing(
    world: &wv_core::World,
    map: &wv_core::Id,
    ring: &[(f32, f32)],
    day: i64,
) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for &(x, y) in ring {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let flat: Vec<f32> = ring.iter().flat_map(|p| [p.0, p.1]).collect();
    let t = wv_core::time::TimePoint::from_day(day);
    let mut inside = 0usize;
    let mut hits = 0usize;
    for i in 0..7 {
        for j in 0..7 {
            let x = min_x + (max_x - min_x) * i as f32 / 6.0;
            let y = min_y + (max_y - min_y) * j as f32 / 6.0;
            if !wv_core::geometry::point_in_ring(&flat, x, y) {
                continue;
            }
            inside += 1;
            let covered = world.faction_geometry.iter().any(|g| {
                &g.map == map
                    && g.relation.is_none()
                    && g.span.contains(&t)
                    && wv_core::geometry::point_in_geometry(&g.geometry, x, y)
            });
            if covered {
                hits += 1;
            }
        }
    }
    inside > 0 && hits * 100 >= inside * 5
}

/// 划分对话框：绘制 + 确认提交。
///
/// 提交成功后**不清空**选择工具选区（ADR-0033：意图生效不再消费选区）。
#[allow(clippy::too_many_arguments)]
pub fn faction_divide_dialog(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<FactionDivisionDialog>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    current_loc: Res<CurrentLocale>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
    screen: Res<crate::app_screen::AppScreen>,
) {
    if !screen.editor_visible() {
        if dialog.open {
            dialog.close();
        }
        return;
    }
    if !dialog.open {
        return;
    }
    if dialog.ring.len() < 3 || dialog.map.is_none() {
        dialog.close();
        return;
    }

    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let loc = current_loc.0;
    let shell = UiShell::new(&shell_cfg);
    let mut open = dialog.open;
    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new(t(loc, I18nKey::FactionDivisionDialogTitle))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::DivisionNameLabel));
                let mut name = dialog.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(220.0))
                    .changed()
                {
                    dialog.name = name;
                }
            });
            ui.add_space(6.0);
            // 宿主（只读展示）：有名字就显示名字，无则显式「无宿主（空势力）」。
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::DivisionHostLabel));
                let host_text = match &dialog.host {
                    Some(id) => projection
                        .world
                        .factions
                        .iter()
                        .find(|f| &f.id == id)
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| id.to_string()),
                    None => t(loc, I18nKey::DivisionHostNone).to_owned(),
                };
                ui.label(egui::RichText::new(host_text).color(shell.t1()));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::DivisionModeLabel));
                if ui
                    .selectable_label(
                        dialog.mode == DivisionMode::Snap,
                        t(loc, I18nKey::DivisionModeSnap),
                    )
                    .clicked()
                {
                    dialog.mode = DivisionMode::Snap;
                }
                if ui
                    .selectable_label(
                        dialog.mode == DivisionMode::Straight,
                        t(loc, I18nKey::DivisionModeStraight),
                    )
                    .clicked()
                {
                    dialog.mode = DivisionMode::Straight;
                }
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                    cancelled = true;
                }
                let can_create = !dialog.name.trim().is_empty();
                if widget_button(
                    ui,
                    &shell,
                    ButtonContent::Text(t(loc, I18nKey::DivisionCreate)),
                    ButtonStyle::Accent,
                    None,
                    can_create,
                )
                .clicked()
                {
                    confirmed = true;
                }
            });
        });

    if cancelled || !open {
        dialog.close();
        return;
    }

    if confirmed {
        let name = dialog.name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let Some(map) = dialog.map.clone() else {
            dialog.close();
            return;
        };
        let frame = (time.elapsed_secs() * 60.0) as u64;
        let loc = current_loc.0;
        let overlap = dialog.host.is_none()
            && ring_overlaps_existing(&projection.world, &map, &dialog.ring, dialog.from);
        let mode = dialog.mode;
        let host = dialog.host.clone();
        let cell_size = dialog.cell_size;
        let from = dialog.from;
        match divide_faction_now(
            &mut projection,
            &mut edit,
            map,
            host,
            dialog.ring.clone(),
            mode,
            name.clone(),
            cell_size,
            from,
        ) {
            Ok(created) => {
                let host_name = dialog
                    .host
                    .as_ref()
                    .and_then(|h| projection.world.factions.iter().find(|f| &f.id == h))
                    .map(|f| f.name.clone());
                let text = match host_name {
                    Some(host) => t(loc, I18nKey::DivisionNoticeCreatedFromHost)
                        .replace("{host}", &host)
                        .replace("{name}", &name),
                    None => t(loc, I18nKey::DivisionNoticeCreated).replace("{name}", &name),
                };
                let mut text = text;
                // Q2 退化回执：贴地没吸到任何地物 → 明确告诉作者用的是直线。
                if mode == DivisionMode::Snap && !created.snapped {
                    text.push('\n');
                    text.push_str(t(loc, I18nKey::DivisionNoticeSnapDegraded));
                }
                if overlap {
                    text.push('\n');
                    text.push_str(t(loc, I18nKey::DivisionNoticeOverlap));
                }
                let _ = created.id;
                notice.info(text, frame);
                // 选区保持不动（ADR-0033）。
                dialog.close();
            }
            Err(msg) => {
                // 失败留在框内（作者可改名/改模式重试）；回执给人话。
                notice.error(msg, frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn faction(id: &str) -> wv_core::Faction {
        wv_core::Faction::new(
            wv_core::Id::new_unchecked(id),
            id,
            wv_core::color::Color("#112233".into()),
        )
    }

    fn span(owner: &str, ring: Vec<f32>) -> wv_core::map::GeometrySpan {
        wv_core::map::GeometrySpan {
            map: wv_core::Id::new_unchecked("map_world"),
            owner: wv_core::Id::new_unchecked(owner),
            span: wv_core::time::TimeSpan { from: 0, to: None },
            geometry: wv_core::map::GeometryData::Polygon(ring),
            relation: None,
        }
    }

    #[test]
    fn host_resolution_skips_claim_hit_when_control_is_elsewhere() {
        // 终审 I1：同一势力在质心处只有声称、实控在地图另一角 → 不能当宿主。
        let mut w = wv_core::World::default();
        w.factions.push(faction("faction_mixed"));
        let mut claim = span(
            "faction_mixed",
            vec![0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0],
        );
        claim.relation = Some("claim".into());
        w.faction_geometry.push(claim);
        w.faction_geometry.push(span(
            "faction_mixed",
            vec![200.0, 200.0, 300.0, 200.0, 300.0, 300.0, 200.0, 300.0],
        ));
        let map = wv_core::Id::new_unchecked("map_world");
        let ring = vec![(40.0, 40.0), (60.0, 40.0), (60.0, 60.0), (40.0, 60.0)];
        assert_eq!(
            resolve_host(&w, &map, &ring, 0),
            None,
            "声称命中 + 实控不覆盖 = 无宿主"
        );
    }

    #[test]
    fn dialog_prefills_name_and_defaults_to_snap_mode() {
        let mut d = FactionDivisionDialog::default();
        d.open_with(
            wv_core::Id::new_unchecked("map_world"),
            vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
            None,
            8.0,
            0,
            true,
        );
        assert!(d.open);
        assert_eq!(d.name, "新势力", "默认名应预填");
        assert_eq!(d.mode, DivisionMode::Snap, "吸附开启时应默认贴地");
        d.close();
        assert!(!d.open);
        assert!(d.name.is_empty());
        assert!(d.ring.is_empty());
    }

    /// 吸附关闭（magnet off）时，划分对话框默认保留栅格平滑边框。
    #[test]
    fn dialog_defaults_to_straight_when_snapping_is_off() {
        let mut d = FactionDivisionDialog::default();
        d.open_with(
            wv_core::Id::new_unchecked("map_world"),
            vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
            None,
            8.0,
            0,
            false,
        );
        assert_eq!(d.mode, DivisionMode::Straight, "吸附关闭时应默认直线");
    }

    #[test]
    fn host_resolution_picks_smallest_containing_faction() {
        let mut w = wv_core::World::default();
        w.factions.push(faction("faction_outer"));
        w.factions.push(faction("faction_inner"));
        // 外环 0..100；内环 20..80（嵌套，重叠合法）。
        w.faction_geometry.push(span(
            "faction_outer",
            vec![0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0],
        ));
        w.faction_geometry.push(span(
            "faction_inner",
            vec![20.0, 20.0, 80.0, 20.0, 80.0, 80.0, 20.0, 80.0],
        ));
        let map = wv_core::Id::new_unchecked("map_world");
        let ring = vec![(40.0, 40.0), (60.0, 40.0), (60.0, 60.0), (40.0, 60.0)];
        assert_eq!(
            resolve_host(&w, &map, &ring, 0),
            Some(wv_core::Id::new_unchecked("faction_inner")),
            "质心在两国内 → 取面积更小的内国"
        );
        // 质心落在外环内、内环外 → 外环。
        let ring2 = vec![(5.0, 5.0), (10.0, 5.0), (10.0, 10.0), (5.0, 10.0)];
        assert_eq!(
            resolve_host(&w, &map, &ring2, 0),
            Some(wv_core::Id::new_unchecked("faction_outer"))
        );
        // 质心不在任何势力内 → None（空势力陆地）。
        let ring3 = vec![(200.0, 200.0), (210.0, 200.0), (210.0, 210.0)];
        assert_eq!(resolve_host(&w, &map, &ring3, 0), None);
    }

    #[test]
    fn host_resolution_ignores_claim_only_territory() {
        let mut w = wv_core::World::default();
        w.factions.push(faction("faction_claimant"));
        let mut s = span(
            "faction_claimant",
            vec![0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0],
        );
        s.relation = Some("claim".into());
        w.faction_geometry.push(s);
        let map = wv_core::Id::new_unchecked("map_world");
        let ring = vec![(40.0, 40.0), (60.0, 40.0), (60.0, 60.0), (40.0, 60.0)];
        assert_eq!(
            resolve_host(&w, &map, &ring, 0),
            None,
            "只有声称的势力不能当宿主"
        );
        let _ = Mutex::new(());
    }
}
