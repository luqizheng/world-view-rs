//! Place 表单（统一 Inspector 的「属性」页签 · 地点分支）。
//!
//! ## 锚点的可写性规则（本文件的核心）
//!
//! `Place.x/y/w/h` **不是一律可写的**：
//!
//! - **有地界环**的地点：锚点与范围由 `Place::fit_anchor_to_ring` 从环
//!   **派生**（质心 + 包围盒）。`UpdatePlaceTerritoryCommand` 与
//!   `UpdatePlaceLineCommand` 每次都会重算。故 Inspector 里改它们等于
//!   造出「两个真相」——改完的 x/y 会在下次重生成边界时被覆盖。
//! - **折线**地点（ADR-0046 小河）：锚点由折线中点派生（`anchor_from_line`）。
//! - **无环的点状**地点：位置就是它的真值（作者手摆的 icon），**可编辑**。
//!
//! 所以 [`place_anchor_editable`] 按「有无地界环 + 是否折线」分流，
//! 表单据此决定给可编辑输入框还是只读展示。
//!
//! ## 群系是只读的
//!
//! `Place.biome` 的真值是「标记包络」——`PlaceBiomeMarkerCommand` 会同时
//! 改地形格，是它唯一的写入口。自由下拉改 biome 会与地形格脱钩，故本表单
//! 只读展示 + 给一个「重生成标记」按钮（走 `regenerate_marker_now`）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::state::{ViewState, WorldProjection};
use crate::ui::shell::UiShell;
use crate::ui::widgets::{numeric_input_bounded, value_box};

use wv_core::distance::{Place, PoiKind};
use wv_core::Id;

/// 该地点是否**有地界环**（`world.place_geometry` 里有它的 span）。
///
/// 有环 → 锚点/范围是派生量 → Inspector 只读。
pub fn place_has_territory(world: &wv_core::World, place_id: &Id) -> bool {
    world
        .place_geometry
        .iter()
        .any(|span| &span.owner == place_id)
}

/// 锚点/范围在 Inspector 里**是否可编辑**。
///
/// 不可编辑的三种情形（有环 / 折线 / 两者皆无但未锚定）都需要区分：
/// - 有地界环 → 派生量，环说了算；
/// - 折线地点 → 锚点由中点派生；
/// - 其余（无环、非折线）→ 位置是作者手摆的真值，可编辑。
pub fn place_anchor_editable(place: &Place, has_territory: bool) -> bool {
    if has_territory {
        return false;
    }
    if place.line.is_some() {
        return false;
    }
    true
}

/// 地点字段的一次变更。
///
/// 表单每次只改**一个**字段，但提交时要带上**全部**字段的当前值
/// （`UpdatePlaceCommand` 是整实体命令）。用结构体把这组值捆在一起，
/// 比 8 个平铺参数清楚，也避开了 clippy 的参数上限。
#[derive(Debug, Clone)]
pub struct PlaceFieldPatch {
    /// 目标地点 ID。
    pub id: Id,
    /// 新名称。
    pub name: String,
    /// 新呈现级别（`None` = 不改）。
    pub rank: Option<u8>,
    /// 新锚点 X（`None` = 不改）。
    pub x: Option<f32>,
    /// 新锚点 Y（`None` = 不改）。
    pub y: Option<f32>,
}

impl PlaceFieldPatch {
    /// 只改名称（锚点与级别保持原样）。
    pub fn name_only(id: Id, name: String) -> Self {
        Self {
            id,
            name,
            rank: None,
            x: None,
            y: None,
        }
    }

    /// 只改呈现级别。
    pub fn rank_only(id: Id, name: String, rank: u8) -> Self {
        Self {
            id,
            name,
            rank: Some(rank),
            x: None,
            y: None,
        }
    }

    /// 只改锚点。
    pub fn anchor_only(id: Id, name: String, x: f32, y: f32) -> Self {
        Self {
            id,
            name,
            rank: None,
            x: Some(x),
            y: Some(y),
        }
    }
}

/// 提交地点属性变更。
///
/// 走 `wv-edit` 的 `UpdatePlaceCommand`（AGENTS §3.3：System 不直接改世界）。
/// patch 里的 `x` / `y` 为 `None` = 不改锚点（只读态下就走这条路）。
pub fn update_place_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    patch: PlaceFieldPatch,
) {
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::command::UpdatePlaceCommand::new(
        patch.id, patch.name, patch.rank, patch.x, patch.y,
    )));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            let _ = view;
        }
        Err(e) => {
            bevy::log::warn!("地点编辑失败: {e}");
        }
    }
}

/// 提交地点类别变更（ADR-0064）。
///
/// 走 `wv-edit` 的 `SetPlaceKindCommand`（AGENTS §3.3：System 不直接改世界）。
/// `kind = None` = 清空（YAML 不再写 `kind` 行），与 `Other` 的界面语义一致。
pub fn set_place_kind_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    id: Id,
    kind: Option<PoiKind>,
) {
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::command::SetPlaceKindCommand::new(
        id, kind,
    )));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
        }
        Err(e) => {
            bevy::log::warn!("地点类别编辑失败: {e}");
        }
    }
}

/// 类别 → 表单文案键。穷尽 match：新增 `PoiKind` 变体会在编译期逼出文案。
fn poi_kind_label_key(kind: PoiKind) -> crate::i18n::I18nKey {
    use crate::i18n::I18nKey as K;
    match kind {
        PoiKind::Other => K::PoiKindOther,
        PoiKind::Settlement => K::PoiKindSettlement,
        PoiKind::Peak => K::PoiKindPeak,
        PoiKind::Port => K::PoiKindPort,
        PoiKind::Station => K::PoiKindStation,
        PoiKind::Scenic => K::PoiKindScenic,
        PoiKind::Food => K::PoiKindFood,
        PoiKind::Lodging => K::PoiKindLodging,
        PoiKind::Medical => K::PoiKindMedical,
        PoiKind::Education => K::PoiKindEducation,
        PoiKind::Government => K::PoiKindGovernment,
        PoiKind::Shop => K::PoiKindShop,
    }
}

/// 只读值框的**固定宽槽位**（220px）。///
/// `value_box` 吃 `available_width()`——在 egui Window 的无界测量 pass 里
/// 那是屏幕级宽，会把窗口撑到全屏且缩不回来（Resize 只扩不缩，
/// 2026-10-22 实机 bug）。一切 Inspector 表单里的 `value_box` /
/// `numeric_input_bounded` 都必须先包固定宽的 `allocate_ui`
/// （cell_editor 的既有先例）。
fn value_slot(ui: &mut egui::Ui, shell: &UiShell, text: &str) {
    ui.allocate_ui(egui::vec2(220.0, 28.0), |ui| {
        value_box(ui, shell, text, None, false);
    });
}

/// 可编辑数值输入的**固定宽槽位**（110px）。
fn numeric_slot(ui: &mut egui::Ui, shell: &UiShell, value: &mut f32) -> egui::Response {
    let mut resp = None;
    ui.allocate_ui(egui::vec2(110.0, 28.0), |ui| {
        resp = Some(numeric_input_bounded(
            ui, shell, value, "", 1.0, -100_000.0, 100_000.0,
        ));
    });
    resp.expect("allocate_ui 闭包必然执行一次")
}

/// 地点属性表单。
#[allow(clippy::too_many_arguments)]
pub fn place_form(
    ui: &mut egui::Ui,
    shell: &UiShell,
    place: &Place,
    has_territory: bool,
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    notice: &mut crate::ui_shell::ShellNotice,
    time: &Time,
    playback: &crate::state::PlaybackState,
    current_loc: &crate::i18n::CurrentLocale,
) {
    use crate::i18n::{t_key, I18nKey};
    let id = place.id.clone();
    let anchor_editable = place_anchor_editable(place, has_territory);

    // ── 名称 ────────────────────────────────────────────────────────
    let mut name_buf = place.name.clone();
    ui.horizontal(|ui| {
        ui.label("名称：");
        if ui.text_edit_singleline(&mut name_buf).changed() {
            update_place_now(
                projection,
                edit,
                view,
                PlaceFieldPatch::name_only(id.clone(), name_buf.clone()),
            );
        }
    });

    // ── 呈现级别 ─────────────────────────────────────────────────────
    let mut rank_buf = place.rank.unwrap_or(1);
    ui.horizontal(|ui| {
        ui.label(format!(
            "{}：",
            t_key(current_loc, I18nKey::InspectorPlaceRank)
        ));
        egui::ComboBox::from_id_salt("place_rank")
            .selected_text(format!("{rank_buf}"))
            .show_ui(ui, |ui| {
                for r in 0u8..=3 {
                    ui.selectable_value(&mut rank_buf, r, r.to_string());
                }
            });
    });
    if rank_buf != place.rank.unwrap_or(1) {
        update_place_now(
            projection,
            edit,
            view,
            PlaceFieldPatch::rank_only(id.clone(), place.name.clone(), rank_buf),
        );
    }

    // ── POI 类别（ADR-0064）──────────────────────────────────────────
    // 选「未分类」= 清空（命令传 None），YAML 不落 `kind` 行。
    // 与级别下拉同款：选完立即提交一条可撤销命令，没有确定按钮。
    let mut kind_buf = place.kind.unwrap_or(PoiKind::Other);
    ui.horizontal(|ui| {
        ui.label(format!(
            "{}：",
            t_key(current_loc, I18nKey::InspectorPlaceKind)
        ));
        egui::ComboBox::from_id_salt("place_kind")
            .selected_text(t_key(current_loc, poi_kind_label_key(kind_buf)))
            .show_ui(ui, |ui| {
                for k in PoiKind::ALL {
                    ui.selectable_value(
                        &mut kind_buf,
                        k,
                        t_key(current_loc, poi_kind_label_key(k)),
                    );
                }
            });
    });
    if kind_buf != place.kind.unwrap_or(PoiKind::Other) {
        set_place_kind_now(
            projection,
            edit,
            id.clone(),
            if kind_buf == PoiKind::Other {
                None
            } else {
                Some(kind_buf)
            },
        );
    }

    // ── 锚点 / 范围 ──────────────────────────────────────────────────
    // 派生量（有环 / 折线）→ 只读；否则可编辑。
    ui.horizontal(|ui| {
        ui.label("锚点：");
        let mut x_buf = place.x.unwrap_or(0.0);
        let mut y_buf = place.y.unwrap_or(0.0);
        if anchor_editable {
            // 固定宽槽位——**必须**包 allocate_ui：value_box / numeric 吃
            // `available_width()`，而 egui Window 偶发做无界测量（表单切换 /
            // 重开），那一刻 available 是屏幕级宽 → 内容期望宽撑到全屏 →
            // Resize 只扩不缩，窗口永久变成全屏宽（2026-10-22 实机 bug）。
            // cell_editor 的既有用法（allocate_ui(96×28 / 168×28)）就是这层。
            let x_resp = numeric_slot(ui, shell, &mut x_buf);
            let y_resp = numeric_slot(ui, shell, &mut y_buf);
            if x_resp.changed() || y_resp.changed() {
                update_place_now(
                    projection,
                    edit,
                    view,
                    PlaceFieldPatch::anchor_only(id.clone(), place.name.clone(), x_buf, y_buf),
                );
            }
        } else {
            value_slot(
                ui,
                shell,
                &format!(
                    "{:.2} / {:.2}",
                    place.x.unwrap_or(0.0),
                    place.y.unwrap_or(0.0)
                ),
            );
        }
    });
    ui.horizontal(|ui| {
        ui.label("范围：");
        // 范围是包围盒的推论——与锚点同源，故始终只读。
        value_slot(
            ui,
            shell,
            &format!(
                "{:.2} × {:.2}",
                place.w.unwrap_or(0.0),
                place.h.unwrap_or(0.0)
            ),
        );
    });
    if !anchor_editable {
        ui.label(
            egui::RichText::new(t_key(current_loc, I18nKey::InspectorAnchorDerived))
                .small()
                .color(shell.t3()),
        );
    }

    // ── 群系（只读 + 重生成标记）──────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("群系：");
        value_slot(ui, shell, &biome_name(place.biome));
    });
    if ui
        .button(t_key(current_loc, I18nKey::InspectorPlaceRegenMarker))
        .clicked()
    {
        match crate::edit::feature_place_actions::regenerate_marker_now(
            projection,
            edit,
            playback,
            tex,
            id.clone(),
        ) {
            Ok(()) => {
                notice.info(
                    format!("已重生成地点「{}」的标记", place.name),
                    (time.elapsed_secs() * 60.0) as u64,
                );
            }
            Err(msg) => {
                notice.error(msg, (time.elapsed_secs() * 60.0) as u64);
            }
        }
    }

    // ── 所属地图 / 稳定 ID（只读）─────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("所属地图：");
        value_slot(
            ui,
            shell,
            place.map.as_ref().map(|m| m.as_str()).unwrap_or("—"),
        );
    });
    ui.horizontal(|ui| {
        ui.label("ID：");
        value_slot(ui, shell, place.id.as_str());
    });
}

/// 群系编号的显示名（`wv_terrain::biome` 的冻结表）。
fn biome_name(biome: Option<u8>) -> String {
    match biome {
        Some(b) => format!("{b}"),
        None => "—".to_string(),
    }
}

// ── 测试 ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn place_with_line(line: Option<Vec<f32>>) -> Place {
        Place {
            kind: None,
            line,
            id: Id::new_unchecked("p1"),
            name: "P".into(),
            feature: None,
            biome: None,
            envelope: None,
            map: None,
            x: Some(1.0),
            y: Some(2.0),
            w: Some(3.0),
            h: Some(4.0),
            rank: None,
        }
    }

    fn world_with_territory(owner: &str) -> wv_core::World {
        let mut w = wv_core::World::default();
        w.place_geometry.push(wv_core::map::GeometrySpan {
            map: Id::new_unchecked("m1"),
            owner: Id::new_unchecked(owner),
            span: wv_core::time::TimeSpan { from: 0, to: None },
            geometry: wv_core::map::GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
            relation: None,
        });
        w
    }

    #[test]
    fn place_has_territory_true_only_for_owning_place() {
        let w = world_with_territory("p1");
        assert!(place_has_territory(&w, &Id::new_unchecked("p1")));
        assert!(!place_has_territory(&w, &Id::new_unchecked("other")));
    }

    #[test]
    fn place_has_territory_false_on_empty_world() {
        let w = wv_core::World::default();
        assert!(!place_has_territory(&w, &Id::new_unchecked("p1")));
    }

    #[test]
    fn place_anchor_editable_false_when_territory_exists() {
        // 有地界环 → 锚点由环派生 → 不可编辑。
        let p = place_with_line(None);
        assert!(!place_anchor_editable(&p, true));
    }

    #[test]
    fn place_anchor_editable_false_for_line_place() {
        // 折线地点（ADR-0046）→ 锚点由折线中点派生 → 不可编辑。
        let p = place_with_line(Some(vec![0.0, 0.0, 10.0, 10.0]));
        assert!(!place_anchor_editable(&p, false));
    }

    #[test]
    fn place_anchor_editable_true_for_plain_point() {
        // 无环、非折线 → 位置是作者手摆的真值 → 可编辑。
        let p = place_with_line(None);
        assert!(place_anchor_editable(&p, false));
    }

    #[test]
    fn place_patch_constructors_leave_other_fields_untouched() {
        // 「只改一个字段」的构造器必须把其余字段留成 `None`（= 不改），
        // 否则会把「没动过的字段」写成 None，把地点的锚点/级别清空。
        let id = Id::new_unchecked("p1");

        let name_only = PlaceFieldPatch::name_only(id.clone(), "新名".into());
        assert_eq!(name_only.name, "新名");
        assert_eq!(name_only.rank, None);
        assert_eq!(name_only.x, None);
        assert_eq!(name_only.y, None);

        let rank_only = PlaceFieldPatch::rank_only(id.clone(), "原名".into(), 3);
        assert_eq!(rank_only.rank, Some(3));
        assert_eq!(rank_only.x, None);
        assert_eq!(rank_only.y, None);

        let anchor_only = PlaceFieldPatch::anchor_only(id.clone(), "原名".into(), 9.0, 8.0);
        assert_eq!(anchor_only.x, Some(9.0));
        assert_eq!(anchor_only.y, Some(8.0));
        assert_eq!(anchor_only.rank, None);
    }
}
