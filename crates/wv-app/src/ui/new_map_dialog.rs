//! 「新建地图」对话框（2026-10-14 用户需求：补上地图级的新建入口）。
//!
//! ## 职责
//! - `NewMapDialog` —— 对话框的跨帧 UI 状态（打开标志 + 名称 / 层级 / 幅面 / 预填充）。
//! - `new_map_dialog` —— 对话框绘制 + 确认落笔（`AddMapCommand` 经 `History` 提交）。
//!
//! ## 边界（不做什么）
//! - **不**直接改 `projection.world` —— 走 `wv_edit::AddMapCommand`（§3.3）。
//! - **不**管入口 —— 菜单 / 命令面板只排队 `CommandId::NewMap`，由唯一分发器
//!   调 [`NewMapDialog::open_for`]；本模块只管对话框自身。
//!
//! ## 主要导出
//! - `NewMapDialog` / `NewMapPrefill` / `new_map_dialog` —— Bevy 资源与系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use wv_core::map::{Extent, MapLevel, MapNode};
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::Id;

use crate::edit::editing::EditHistory;
use crate::i18n::CurrentLocale;
use crate::render::terrain_view::TerrainTexture;
use crate::state::{map_level_label, DrillState, ProjectDiagnostics, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};
use crate::ui_shell::ShellNotice;

use super::shell::UiShell;

/// 新地图的**预填充**（2026-10-14 用户需求：新建时可选铺海还是铺陆）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NewMapPrefill {
    /// 全海：高度压在海平面下、类型深水——适合先画大陆的作者。
    Ocean,
    /// 全陆：高度抬在海平面上、类型草原——适合先画内海的作者。
    #[default]
    Land,
}

impl NewMapPrefill {
    fn label(self) -> &'static str {
        match self {
            NewMapPrefill::Ocean => "海洋",
            NewMapPrefill::Land => "陆地",
        }
    }

    /// 预填充格：`(height, kind)`。
    ///
    /// 海洋取 `-120`：VectorFlat 双线性填充下是足够深的纯水，又不至于与
    /// 周边拉出夸张等值线；陆地取 `+80`：稳居海平面之上、留出向下挖河的余量。
    ///
    /// `pub(crate)`：`new_project_dialog` 的首图预填充与本方法同源。
    pub(crate) fn cell(self) -> TerrainCellData {
        match self {
            NewMapPrefill::Ocean => TerrainCellData {
                height: -120,
                kind: 0,
            },
            NewMapPrefill::Land => TerrainCellData {
                height: 80,
                kind: 3,
            },
        }
    }
}

/// 幅面候选（世界单位，正方形）。栅格边长 = 幅面 / [`GRID_CELL_SIZE`]。
///
/// `pub(crate)`：`new_project_dialog` 的首图默认值与本列表同源（最大幅面），
/// 开放引用而不是复制数字，两处口径才不会漂移。
pub(crate) const EXTENT_CHOICES: [f32; 4] = [4096.0, 2048.0, 1024.0, 512.0];

/// 每格世界边长——与 `GridConfig::default().cell_size` 同一口径（8 wu/格）。
pub(crate) const GRID_CELL_SIZE: f32 = 8.0;

/// 「新建地图」对话框的跨帧状态。
///
/// **为什么是资源而不是 egui memory**：打开它的是命令分发器（拿不到 egui
/// 上下文里的窗口记忆），且创建失败时要把错误留在框里跨帧展示。
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct NewMapDialog {
    /// 是否打开。
    pub open: bool,
    /// 地图名（可中文；地图 ID 由 [`Self::next_free_id`] 自动生成）。
    pub name: String,
    /// 地图层级。
    pub level: MapLevel,
    /// 幅面（世界单位，正方形边长）。
    pub extent: f32,
    /// 预填充。
    pub prefill: NewMapPrefill,
    /// 上次创建失败的错误（留在框内，作者改完再试；成功即清）。
    pub error: Option<String>,
}

impl Default for NewMapDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: "新地图".into(),
            level: MapLevel::City,
            extent: 4096.0,
            prefill: NewMapPrefill::Land,
            error: None,
        }
    }
}

impl NewMapDialog {
    /// 由分发器调用：打开对话框并按世界现状预填默认值。
    pub fn open_for(&mut self, world: &wv_core::World) {
        *self = Self {
            open: true,
            // 空世界的第一张图就是世界图，名字顺势默认「世界」。
            name: if world.maps.is_empty() {
                "世界".into()
            } else {
                "新地图".into()
            },
            level: if world.maps.is_empty() {
                MapLevel::World
            } else {
                MapLevel::City
            },
            extent: 4096.0,
            prefill: NewMapPrefill::Land,
            error: None,
        };
    }

    /// 生成下一个空闲的 `map_N` 形式 ID（§4.4：ID 可读字符集）。
    fn next_free_id(&self, world: &wv_core::World) -> Id {
        for n in 1..u32::MAX {
            let candidate = Id::new_unchecked(format!("map_{n}"));
            if !world.maps.iter().any(|m| m.id == candidate) {
                return candidate;
            }
        }
        unreachable!("map_N 形式的 ID 不可能用尽 u32")
    }

    /// 幅面 → 栅格边长（格数）。
    fn grid_cells(&self) -> u32 {
        ((self.extent / GRID_CELL_SIZE).round() as u32).max(1)
    }
}

/// 「新建地图」对话框：绘制 + 确认提交。
#[allow(clippy::too_many_arguments)]
pub fn new_map_dialog(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<NewMapDialog>,
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut edit: ResMut<EditHistory>,
    mut tex: ResMut<TerrainTexture>,
    mut diagnostics: ResMut<ProjectDiagnostics>,
    mut notice: ResMut<ShellNotice>,
    time: Res<Time>,
    current_loc: Res<CurrentLocale>,
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let frame = (time.elapsed_secs() * 60.0) as u64;
    let mut open = dialog.open;
    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new("新建地图")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        // 模态置顶（2026-10-14 用户要求）：egui 绘制序 Background < Middle <
        // Foreground < Tooltip——浮动面板是 Middle，弹窗必须在 Foreground 才
        // 压得住面板；Tooltip 在 Foreground 之上，窗内 on_hover_text 不受影响。
        // 与 welcome_screen / startup_load 的先例同层。
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let world = &projection.world;
            let first_map = world.maps.is_empty();

            ui.horizontal(|ui| {
                ui.label("名称").on_hover_text(
                    "显示名，可中文；地图 ID（YAML/Git 里用的）会按 map_1、map_2… 自动生成",
                );
                let mut name = dialog.name.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .desired_width(220.0)
                            .hint_text("新地图"),
                    )
                    .changed()
                {
                    dialog.name = name.trim().to_owned();
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("层级");
                // 空世界的第一张图必须是世界图——它是钻取树的根（ensure_root
                // 只认 level == World），别的层级在树下无处可挂。
                if first_map {
                    ui.label(map_level_label(MapLevel::World, current_loc.0))
                        .on_hover_text("空世界的第一张地图固定为世界图");
                } else {
                    for l in [
                        MapLevel::World,
                        MapLevel::Region,
                        MapLevel::City,
                        MapLevel::Interior,
                    ] {
                        ui.selectable_value(
                            &mut dialog.level,
                            l,
                            map_level_label(l, current_loc.0),
                        );
                    }
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("幅面");
                for e in EXTENT_CHOICES {
                    ui.selectable_value(&mut dialog.extent, e, format!("{e:.0}"));
                }
                ui.label("wu");
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("预填充").on_hover_text(
                    "整图先铺满海洋或陆地，再用地形笔刷修形——\
                     矢量地形视图下「陆」由高度决定，预填充同时写高度与类型",
                );
                for p in [NewMapPrefill::Ocean, NewMapPrefill::Land] {
                    ui.selectable_value(&mut dialog.prefill, p, p.label());
                }
            });

            ui.add_space(4.0);
            // 落点：当前图是父图；空世界时它就是根。
            if first_map {
                ui.label("将成为世界地图（钻取树的根）");
            } else {
                let parent = drill
                    .current()
                    .and_then(|id| drill.node(world, id).cloned());
                match parent {
                    Some(p) => ui.label(format!("挂在「{}」之下", p.name)),
                    None => ui.label("（当前没有选中地图，将作为独立根图创建）"),
                };
            }
            ui.label(
                egui::RichText::new(format!(
                    "栅格 {}×{}（每格 {GRID_CELL_SIZE:.0} wu）",
                    dialog.grid_cells(),
                    dialog.grid_cells()
                ))
                .text_style(egui::TextStyle::Small),
            );

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
                    ButtonContent::Text("创建"),
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

    // X（`open(&mut open)` 写回）与「取消」都走关；「创建」失败时留在框内。
    dialog.open = open && !cancelled;
    if !dialog.open {
        return;
    }
    if confirmed {
        match create_map(&mut dialog, &mut projection, &drill, &mut edit) {
            Ok(id) => {
                // 新栅格 / 新图都要让底图重走一遍：mark_all_dirty 触发整图重建，
                // 钻入后 `rebuild_texture_on_mode_change` 认 last_map 变化再换图。
                tex.mark_all_dirty();
                diagnostics.needs_diagnose = true;
                // 建好就钻进去：作者接下来八成是在这张图上修地形。
                if drill.node(&projection.world, &id).is_some()
                    && drill.current().map(|c| c != &id).unwrap_or(false)
                {
                    drill.enter(&projection.world, &id);
                }
                notice.info(format!("已创建地图「{}」", dialog.name), frame);
                dialog.open = false;
            }
            Err(e) => dialog.error = Some(e),
        }
    }
}

/// 确认创建：组装 `AddMapCommand` 并提交 `History`。失败原因留在 `dialog.error`。
fn create_map(
    dialog: &mut NewMapDialog,
    projection: &mut WorldProjection,
    drill: &DrillState,
    edit: &mut EditHistory,
) -> Result<Id, String> {
    let name = if dialog.name.trim().is_empty() {
        "新地图".to_owned()
    } else {
        dialog.name.trim().to_owned()
    };
    let id = dialog.next_free_id(&projection.world);
    // 空世界的第一张图固定为世界图（父 = None）；否则挂在当前图下。
    let first_map = projection.world.maps.is_empty();
    let parent = if first_map {
        None
    } else {
        drill.current().cloned()
    };
    let level = if first_map {
        MapLevel::World
    } else {
        dialog.level
    };
    let node = MapNode {
        id: id.clone(),
        name,
        level,
        parent: parent.clone(),
        children: Vec::new(),
        layers: Vec::new(),
        icons: Vec::new(),
        extent: Some(Extent::sized(dialog.extent, dialog.extent)),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
    };
    let n = dialog.grid_cells();
    let terrain = TerrainData {
        width: n,
        height: n,
        cells: vec![dialog.prefill.cell(); (n * n) as usize],
    };

    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(wv_edit::AddMapCommand::new(node, Some(terrain))));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            Ok(id)
        }
        Err(e) => Err(format!("创建失败：{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `next_free_id` 跳过已占用的编号，从 1 起找最小空位。
    #[test]
    fn next_free_id_skips_occupied_numbers() {
        let mut world = wv_core::World::default();
        for id in ["map_1", "map_2"] {
            world.maps.push(MapNode {
                id: Id::new_unchecked(id),
                name: id.into(),
                level: MapLevel::City,
                parent: None,
                children: Vec::new(),
                layers: Vec::new(),
                icons: Vec::new(),
                extent: None,
                raster: None,
                geographic_source: None,
                time_span: None,
                unit_per_km: None,
                orientation: 0.0,
            });
        }
        let dialog = NewMapDialog::default();
        assert_eq!(
            dialog.next_free_id(&world).as_str(),
            "map_3",
            "1、2 已被占用，应从 3 继续"
        );
    }

    /// 幅面 → 栅格换算：4096 wu / 8 wu每格 = 512²（与启动占位地形同量级）。
    #[test]
    fn grid_cells_derive_from_extent_divided_by_cell_size() {
        let dialog = NewMapDialog {
            extent: 4096.0,
            ..Default::default()
        };
        assert_eq!(dialog.grid_cells(), 512);
        let small = NewMapDialog {
            extent: 512.0,
            ..Default::default()
        };
        assert_eq!(small.grid_cells(), 64);
    }

    /// 预填充语义：海洋必须在海平面下（高度 < 0）、陆地在海平面上（≥ 0）——
    /// 这是 VectorFlat 填充「高度赢过类型」口径下的硬约束。
    #[test]
    fn prefill_ocean_is_below_sea_level_and_land_is_above() {
        let ocean = NewMapPrefill::Ocean.cell();
        assert!(
            ocean.height < 0,
            "海洋预填充高度应为负，实得 {}",
            ocean.height
        );
        assert!(ocean.kind <= 1, "海洋预填充类型应为水系");
        let land = NewMapPrefill::Land.cell();
        assert!(
            land.height >= 0,
            "陆地预填充高度应非负，实得 {}",
            land.height
        );
        assert!(land.kind > 1, "陆地预填充类型应为陆系");
    }

    /// `open_for` 的默认值随世界现状切换：空世界 → 世界图；非空 → 当前图下的城市图。
    #[test]
    fn open_for_picks_defaults_by_world_shape() {
        let mut dialog = NewMapDialog::default();
        dialog.open_for(&wv_core::World::default());
        assert!(dialog.open);
        assert_eq!(dialog.level, MapLevel::World, "空世界的首图应为世界图");

        let mut world = wv_core::World::default();
        world.maps.push(MapNode {
            id: Id::new_unchecked("world_main"),
            name: "世界".into(),
            level: MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
        });
        let mut dialog = NewMapDialog::default();
        dialog.open_for(&world);
        assert_eq!(dialog.level, MapLevel::City, "非空世界默认建城市图");
    }
}
