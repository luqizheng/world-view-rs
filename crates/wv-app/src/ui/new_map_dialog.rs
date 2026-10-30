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
//! - `NewMapDialog` / `new_map_dialog` —— Bevy 资源与系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use wv_core::map::{Extent, MapBase, MapLevel, MapNode};
use wv_core::terrain::TerrainData;
use wv_core::Id;

use crate::edit::editing::EditHistory;
use crate::i18n::CurrentLocale;
use crate::state::{map_level_label, DrillState, ProjectDiagnostics, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};
use crate::ui_shell::ShellNotice;
use wv_render::terrain_view::TerrainTexture;

use super::shell::UiShell;

/// 幅面候选（世界单位，作用于**宽**）。栅格宽 = 幅面 / [`GRID_CELL_SIZE`]。
///
/// `pub(crate)`：`new_project_dialog` 的首图默认值与本列表同源（最大幅面），
/// 开放引用而不是复制数字，两处口径才不会漂移。
pub(crate) const EXTENT_CHOICES: [f32; 4] = [4096.0, 2048.0, 1024.0, 512.0];

/// 每格世界边长——与 `GridConfig::default().cell_size` 同一口径（8 wu/格）。
pub(crate) const GRID_CELL_SIZE: f32 = 8.0;

/// 缺省宽高比（2026-10-17 用户裁定：新建图默认 16:9，可改）。
pub(crate) const DEFAULT_ASPECT: MapAspect = MapAspect::Wide16x9;

/// 幅面宽高比档。**缺省不是约束**——作者可换回 4:3 / 1:1（V1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapAspect {
    /// 16:9（缺省，适配主流窗口 / 视频帧）。
    #[default]
    Wide16x9,
    /// 4:3（旧绘图习惯）。
    Standard4x3,
    /// 1:1（正方形，旧版默认）。
    Square1x1,
}

impl MapAspect {
    /// 宽 / 高。
    pub fn ratio(self) -> f32 {
        match self {
            Self::Wide16x9 => 16.0 / 9.0,
            Self::Standard4x3 => 4.0 / 3.0,
            Self::Square1x1 => 1.0,
        }
    }

    /// 面板档位顺序。
    pub const ALL: [MapAspect; 3] = [
        MapAspect::Wide16x9,
        MapAspect::Standard4x3,
        MapAspect::Square1x1,
    ];
}

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
    /// 幅面宽（世界单位）。
    pub extent_w: f32,
    /// 幅面高（世界单位）——随宽 × 宽高比换算。
    pub extent_h: f32,
    /// 宽高比档（同一次打开内切换不丢）。
    pub aspect: MapAspect,
    /// 底图（2026-10-16）。原 `NewMapPrefill` 已并入 [`MapBase`]——
    /// 字段语义「整图铺满的初始态」与「重置按钮回退目标」共享同一份枚举。
    pub prefill: MapBase,
    /// 上次创建失败的错误（留在框内，作者改完再试；成功即清）。
    pub error: Option<String>,
    /// UX-026：作者显式选择的父图 Id（下拉）。`None` = 沿用当前钻取图
    /// （旧行为）；选中的图被删/进垃圾桶时创建路径回退当前图。
    pub parent_override: Option<String>,
}

impl Default for NewMapDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: "新地图".into(),
            level: MapLevel::City,
            extent_w: EXTENT_CHOICES[0],
            extent_h: EXTENT_CHOICES[0] / DEFAULT_ASPECT.ratio(),
            aspect: DEFAULT_ASPECT,
            // 2026-10-16 用户裁定：默认铺海（新建即满图海洋，便于「先画大陆」场景）。
            prefill: MapBase::Ocean,
            error: None,
            parent_override: None,
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
            extent_w: EXTENT_CHOICES[0],
            extent_h: EXTENT_CHOICES[0] / DEFAULT_ASPECT.ratio(),
            aspect: DEFAULT_ASPECT,
            prefill: MapBase::Ocean,
            error: None,
            parent_override: None,
        };
    }

    /// 切换宽高比：**只改高**，宽保持作者当前选择（review focus 3）。
    pub fn apply_aspect(&mut self, aspect: MapAspect) {
        self.aspect = aspect;
        self.extent_h = self.extent_w / aspect.ratio();
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

    /// 幅面宽 → 栅格宽（格数）。
    fn grid_cells_w(&self) -> u32 {
        ((self.extent_w / GRID_CELL_SIZE).round() as u32).max(1)
    }

    /// 幅面高 → 栅格高（格数）。
    fn grid_cells_h(&self) -> u32 {
        ((self.extent_h / GRID_CELL_SIZE).round() as u32).max(1)
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
    // 建好即取景到新图（作者下一步就是在这张图上工作）。
    mut view: ResMut<crate::state::ViewState>,
    grid: Res<crate::state::GridConfig>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
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
        // 与 welcome_screen / drive_project_load 的先例同层。
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
                    if ui
                        .selectable_value(&mut dialog.extent_w, e, format!("{e:.0}"))
                        .clicked()
                    {
                        // 宽变了：高按当前比例重算（比例是持久选择）。
                        dialog.extent_h = e / dialog.aspect.ratio();
                    }
                }
                ui.label("wu");
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(crate::i18n::t_key(
                    &current_loc,
                    crate::i18n::I18nKey::NewMapAspect,
                ));
                for a in MapAspect::ALL {
                    let key = match a {
                        MapAspect::Wide16x9 => crate::i18n::I18nKey::Aspect16x9,
                        MapAspect::Standard4x3 => crate::i18n::I18nKey::Aspect4x3,
                        MapAspect::Square1x1 => crate::i18n::I18nKey::Aspect1x1,
                    };
                    let label = crate::i18n::t_key(&current_loc, key);
                    if ui.selectable_value(&mut dialog.aspect, a, label).clicked() {
                        dialog.apply_aspect(a);
                    }
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("预填充").on_hover_text(
                    "整图先铺满海洋或陆地，再用地形笔刷修形——\
                     矢量地形视图下「陆」由高度决定，预填充同时写高度与类型；\
                     此处选项即写入 `MapNode.base`，供「重置到底图」按钮回退",
                );
                for p in [MapBase::Ocean, MapBase::Land] {
                    ui.selectable_value(&mut dialog.prefill, p, p.zh_label());
                }
            });

            ui.add_space(4.0);
            // 落点：世界层级永远独立成根；其余层级挂当前图（空世界时它就是根）。
            if first_map {
                ui.label("将成为世界地图（钻取树的根）");
            } else if dialog.level == MapLevel::World {
                ui.label("将成为独立的世界图（不挂在任何图之下；与现有世界并列）")
                    .on_hover_text(
                        "独立世界图没有上级：zoom-out 到最大就停在本图，\
                         不会钻回之前打开的地图",
                    );
            } else {
                // UX-026：父图可改选（旧版只显示当前图，想挂别处必须先钻过去）。
                // 候选 = 全部存活图（含各层级），默认当前钻取图；垃圾桶里的
                // 图不出现在下拉里。
                let candidates: Vec<&wv_core::map::MapNode> = world
                    .maps
                    .iter()
                    .filter(|m| world.map_alive(&m.id))
                    .collect();
                if candidates.is_empty() {
                    ui.label("（当前没有选中地图，将作为独立根图创建）");
                } else {
                    let current = drill.current().map(|i| i.as_str().to_owned());
                    let selected = dialog
                        .parent_override
                        .clone()
                        .or_else(|| current.clone())
                        .unwrap_or_else(|| candidates[0].id.as_str().to_owned());
                    let selected_name = candidates
                        .iter()
                        .find(|m| m.id.as_str() == selected)
                        .map(|m| {
                            format!(
                                "{}（{}）",
                                m.name,
                                crate::state::map_level_label(m.level, crate::i18n::Locale::Zh)
                            )
                        })
                        .unwrap_or_else(|| "（无效，将回退当前图）".into());
                    ui.label("挂在");
                    egui::ComboBox::from_id_salt("wv_new_map_parent")
                        .selected_text(selected_name)
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for m in &candidates {
                                let label = format!(
                                    "{}（{}）",
                                    m.name,
                                    crate::state::map_level_label(m.level, crate::i18n::Locale::Zh)
                                );
                                if ui
                                    .selectable_label(m.id.as_str() == selected, label)
                                    .clicked()
                                {
                                    dialog.parent_override = Some(m.id.as_str().to_owned());
                                }
                            }
                        });
                    ui.label("之下");
                }
            }
            ui.label(
                egui::RichText::new(format!(
                    "栅格 {}×{}（每格 {GRID_CELL_SIZE:.0} wu）",
                    dialog.grid_cells_w(),
                    dialog.grid_cells_h()
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
        let first_map = projection.world.maps.is_empty();
        match create_map(&mut dialog, &mut projection, &drill, &mut edit) {
            Ok((id, dirty)) => {
                // 新栅格 / 新图都要让底图重走一遍：mark_all_dirty 触发整图重建，
                // 钻入后 `rebuild_texture_on_mode_change` 认 last_map 变化再换图。
                tex.mark_all_dirty();
                diagnostics.needs_diagnose = true;
                // 建好就切到新图。世界层级图是**独立根图**——导航上下文整体换新
                // （settle_on：trail 只含新图、访问痕迹清空），绝不能 `enter`：
                // enter 会留下旧图面包屑，zoom-out 撞外界就弹回旧图
                // （2026-10-17 用户报告「新建世界地图 zoom out 加载前一张地图」）。
                // 首图同理（没有导航上下文可沿）。其余层级沿用 enter：
                // 子图钻出回父图是 P4 设计行为。
                if first_map || dialog.level == MapLevel::World {
                    drill.settle_on(id.clone());
                } else if drill.node(&projection.world, &id).is_some()
                    && drill.current().map(|c| c != &id).unwrap_or(false)
                {
                    drill.enter(&projection.world, &id);
                }
                // 取景到新图：切换后旧相机的中心/缩放属于上一张图，
                // 新图更小时会停在角落。窗口尺寸优先取真实窗口，取不到用视图镜像。
                let window_size = windows
                    .single()
                    .map(|w| w.resolution.size())
                    .unwrap_or(view.window_size);
                if let Some((center, scale)) =
                    fit_view_to_map(&id, &projection.world, &grid, &mut view, window_size)
                {
                    wv_render::camera::apply_view_to_camera(center, scale, &mut cameras);
                }
                // 创建即落盘（2026-10-17 用户建议：新建地图 = 创建新地图文件）：
                // 新图 YAML + terrain.bin 立即写入，不等下一次手动保存。
                // 失败不打断创建——dirty 集合仍持有这些路径，下次保存补写。
                if let Some(dir) = projection.project_dir.clone() {
                    match wv_io::Project::open(&dir)
                        .and_then(|mut p| p.save_world(&projection.world, &dirty))
                    {
                        Ok(()) => {}
                        Err(e) => notice.error(
                            format!("新图落盘失败：{e}（已留脏标记，下次保存补写）"),
                            frame,
                        ),
                    }
                }
                notice.info(format!("已创建地图「{}」", dialog.name), frame);
                dialog.open = false;
            }
            Err(e) => dialog.error = Some(e),
        }
    }
}

/// 把视图取景到指定地图（中心 = 图帧原点 + 幅面一半；缩放 = fit 两轴较紧者）。
///
/// 返回 `(center, scale)`，同时写回 [`ViewState`]；调用方再把它应用到相机。
/// 与「适应全图」按钮（`ui_shell` 的 FitMap）同一套 `fit_scale` 口径。
pub(crate) fn fit_view_to_map(
    id: &Id,
    world: &wv_core::World,
    grid: &crate::state::GridConfig,
    view: &mut crate::state::ViewState,
    window_size: Vec2,
) -> Option<(Vec2, f32)> {
    let node = world.maps.iter().find(|m| &m.id == id)?;
    let size = node
        .extent
        .as_ref()
        .filter(|e| e.width > 0.0 && e.height > 0.0 && e.width.is_finite() && e.height.is_finite())
        .map(|e| Vec2::new(e.width, e.height))
        .unwrap_or(grid.map_size);
    let origin = wv_core::resolve_frame_in(&world.maps, id)
        .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
        .unwrap_or(grid.raster_origin);
    let center = origin + size * 0.5;
    let scale = wv_render::zoom_level::fit_scale(size, window_size);
    view.camera_center = center;
    view.scale = scale;
    Some((center, scale))
}

/// 确认创建：组装 `AddMapCommand` 并提交 `History`。失败原因留在 `dialog.error`。
///
/// 成功返回 `(新图 id, 本次提交的 dirty 路径)`——调用方拿 dirty 路径做
/// 「创建即落盘」（2026-10-17 用户建议：新建地图 = 创建新地图文件）。
fn create_map(
    dialog: &mut NewMapDialog,
    projection: &mut WorldProjection,
    drill: &DrillState,
    edit: &mut EditHistory,
) -> Result<(Id, Vec<String>), String> {
    let name = if dialog.name.trim().is_empty() {
        "新地图".to_owned()
    } else {
        dialog.name.trim().to_owned()
    };
    let id = dialog.next_free_id(&projection.world);
    // 空世界的第一张图固定为世界图（父 = None）；世界层级图**永远**独立成根
    // （2026-10-17 用户报告修复：世界挂在当前图下，zoom-out 撞外界就钻回旧图）。
    // 其余层级挂在当前图下——子图钻出回父图是 P4 的设计行为。
    let first_map = projection.world.maps.is_empty();
    let parent = if first_map || dialog.level == MapLevel::World {
        None
    } else {
        // UX-026：作者显式选择的父图优先；选中图已不存在（被删/进垃圾桶）
        // 时回退当前钻取图（V1：只提示不阻止的口径）。
        dialog
            .parent_override
            .as_deref()
            .and_then(|id| {
                let id = wv_core::Id::new_unchecked(id);
                projection.world.map_alive(&id).then_some(id)
            })
            .or_else(|| drill.current().cloned())
    };
    let level = if first_map {
        MapLevel::World
    } else {
        dialog.level
    };
    // **世界图预填海洋，非世界图不预填**（场景编辑器 M1，2026-10-29）。
    //
    // 「默认海洋」是 2026-10-16 用户对**世界图**的裁定——世界图该是海洋，
    // 作者再自己生成大陆。对城市 / 地下城 / 区域图预填海洋是另一回事：
    // ① 场景图的城市分区要刷在**自己的**基底网格上，预填的 -3500 m 深水
    //    会让「刷分区」把格子从海底抬起来（虽然分区笔刷不改高度，但底图
    //    本身是深海，视觉与语义都不对）；
    // ② 场景图首次刷色时由 `CreateSceneBaseGridCommand` 自动铺基底，
    //    作者不必先做一遍隐藏的技术准备；
    // ③ 城市图「沿父链继承父图底图」是既有契约（`active_terrain`）——
    //    不预填栅格，钻进去看到的是父图的地形，与父图一致。
    let is_world = level == MapLevel::World;
    let node = MapNode {
        id: id.clone(),
        name,
        level,
        parent: parent.clone(),
        children: Vec::new(),
        layers: Vec::new(),
        icons: Vec::new(),
        extent: Some(Extent::sized(dialog.extent_w, dialog.extent_h)),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: is_world.then_some(dialog.prefill),
        outline_polygon: None,
        ..Default::default()
    };
    // 世界图预填栅格（海洋 / 陆地的底图态）；非世界图不预填（None = 沿用父图底图）。
    let terrain = is_world.then(|| {
        let w = dialog.grid_cells_w();
        let h = dialog.grid_cells_h();
        TerrainData {
            width: w,
            height: h,
            cells: vec![dialog.prefill.cell(); (w * h) as usize],
        }
    });

    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(wv_edit::AddMapCommand::new(node, terrain)));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty.clone());
            projection.dirty = true;
            Ok((id, dirty))
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
                base: None,
                outline_polygon: None,
                ..Default::default()
            });
        }
        let dialog = NewMapDialog::default();
        assert_eq!(
            dialog.next_free_id(&world).as_str(),
            "map_3",
            "1、2 已被占用，应从 3 继续"
        );
    }

    /// 默认海洋锚定（2026-10-17 ocean-default-16-9 Task 1）：
    /// `NewMapDialog::default()` 的预填充必须是 Ocean，且 Ocean 高度 < 0。
    #[test]
    fn new_map_dialog_default_prefill_is_ocean() {
        let dialog = NewMapDialog::default();
        assert_eq!(dialog.prefill, MapBase::Ocean, "默认预填充必须是海洋");
        assert!(
            MapBase::Ocean.cell().height < 0,
            "Ocean 单格高度必须 < 0（海平面 = 0），得 {}",
            MapBase::Ocean.cell().height
        );
    }

    /// 场景编辑器 M1：**非世界图不预填栅格**（城市 / 地下城 / 区域图）。
    ///
    /// 「默认海洋」是 2026-10-16 用户对**世界图**的裁定，不是对场景图的。
    /// 场景图首次刷分区时由 `CreateSceneBaseGridCommand` 自动铺基底，
    /// 不该让作者先做一遍隐藏的技术准备。
    #[test]
    fn non_world_dialog_omits_terrain_prefill() {
        let mut projection = WorldProjection::default();
        let root = Id::new_unchecked("map_root");
        projection.world.maps.push(MapNode {
            id: root.clone(),
            name: "世界".into(),
            level: MapLevel::World,
            extent: Some(Extent::sized(512.0, 512.0)),
            base: Some(MapBase::Ocean),
            ..Default::default()
        });
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&projection.world);
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::Interior,
            ..Default::default()
        };
        let new_id = create_map(&mut dialog, &mut projection, &drill, &mut edit)
            .expect("创建")
            .0;
        assert!(
            projection.world.terrain_of(&new_id).is_none(),
            "非世界图不该预填栅格（首次刷色时自动铺基底）"
        );
        let node = projection
            .world
            .maps
            .iter()
            .find(|m| m.id == new_id)
            .expect("新图");
        assert!(
            node.base.is_none(),
            "非世界图不该设底图锚点（否则橡皮会擦成海洋）"
        );
    }

    /// 世界图**仍**预填海洋——2026-10-16 用户裁定不得回归。
    #[test]
    fn world_dialog_keeps_ocean_prefill_unchanged() {
        let mut projection = WorldProjection::default();
        let drill = crate::state::DrillState::default();
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::World,
            ..Default::default()
        };
        let new_id = create_map(&mut dialog, &mut projection, &drill, &mut edit)
            .expect("创建")
            .0;
        let g = projection
            .world
            .terrain_of(&new_id)
            .expect("世界图应预填栅格");
        assert!(
            g.cells
                .iter()
                .all(|c| c.surface == wv_terrain::SURFACE_WATER && c.height == -3500),
            "默认海洋裁定不得回归"
        );
        let node = projection
            .world
            .maps
            .iter()
            .find(|m| m.id == new_id)
            .expect("新图");
        assert_eq!(node.base, Some(MapBase::Ocean), "世界图保留底图锚点");
    }

    /// 新建地图并钻入后，底图纹理必须换成新图（旧图不得留在主画面）。
    #[test]
    fn creating_a_new_map_swaps_the_basemap_texture() {
        let mut projection = WorldProjection::default();
        let root = Id::new_unchecked("map_root");
        projection.world.maps.push(MapNode {
            id: root.clone(),
            name: "旧世界".into(),
            level: MapLevel::World,
            extent: Some(Extent::sized(512.0, 512.0)),
            base: Some(MapBase::Ocean),
            ..Default::default()
        });
        let old_terrain = TerrainData {
            width: 64,
            height: 64,
            cells: vec![MapBase::Ocean.cell(); 64 * 64],
        };
        projection
            .world
            .set_terrain(root.clone(), old_terrain.clone());
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&projection.world);
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::World,
            ..Default::default()
        };
        let new_id = create_map(&mut dialog, &mut projection, &drill, &mut edit)
            .expect("创建")
            .0;
        // 世界层级新图是独立根图，导航切换走 settle_on（不能 enter——它不是旧图子图）。
        drill.settle_on(new_id.clone());

        // 模拟一帧 UI 之后的渲染侧：旧纹理 + 新 current。
        let mut images = Assets::<Image>::default();
        let image = wv_render::terrain_view::image_from_terrain(
            &old_terrain,
            wv_world_cell_map::shade::ShadeMode::VectorFlat,
            None,
            false,
            &wv_world_cell_map::palette::CellPack::fixture(),
            0.0,
        );
        let tex = wv_render::terrain_view::new_terrain_texture(
            &mut images,
            image,
            &old_terrain,
            Some(root.clone()),
            wv_world_cell_map::shade::ShadeMode::VectorFlat,
            false,
            wv_render::theme::theme_pack::builtin::DEFAULT.id,
            0.0,
        );
        let mut bevy_world = bevy::prelude::World::new();
        bevy_world.insert_resource(images);
        bevy_world.insert_resource(tex);
        bevy_world.insert_resource(crate::state::ViewState::default());
        bevy_world.insert_resource(projection);
        bevy_world.insert_resource(drill);
        bevy_world.insert_resource(crate::state::GridConfig::default());
        bevy_world
            .insert_resource(wv_render::terrain_view::rebuild::PendingTerrainSynth::default());
        let mut sched = Schedule::default();
        sched.add_systems((
            wv_render::terrain_view::rebuild_texture_on_mode_change,
            wv_render::terrain_view::land_pending_terrain_synth,
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            sched.run(&mut bevy_world);
            let t = bevy_world.resource::<wv_render::terrain_view::TerrainTexture>();
            if t.last_map.as_ref() == Some(&new_id) {
                assert_eq!(t.size, 512, "新图栅格宽");
                assert_eq!(t.height, 288, "新图栅格高");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "3s 内底图未换成新图（last_map={:?}）",
                t.last_map
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// 新建地图后视图取景到新图（中心 = 新图中心，缩放 = fit 两轴较紧者）。
    #[test]
    fn creating_a_map_fits_the_view_to_it() {
        let mut projection = WorldProjection::default();
        projection.world.maps.push(MapNode {
            id: Id::new_unchecked("map_root"),
            name: "旧世界".into(),
            level: MapLevel::World,
            extent: Some(Extent::sized(512.0, 512.0)),
            base: Some(MapBase::Ocean),
            ..Default::default()
        });
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&projection.world);
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::World,
            ..Default::default()
        };
        let (id, _) = create_map(&mut dialog, &mut projection, &drill, &mut edit).expect("创建");
        let mut view = crate::state::ViewState {
            camera_center: Vec2::splat(9999.0),
            scale: 0.01,
            ..Default::default()
        };
        let window = Vec2::new(1280.0, 800.0);
        let (center, scale) = fit_view_to_map(
            &id,
            &projection.world,
            &crate::state::GridConfig::default(),
            &mut view,
            window,
        )
        .expect("新图必有 extent");
        // 默认 4096×2304、父图无图标 → 图帧原点 (0,0)、中心 (2048, 1152)。
        assert_eq!(center, Vec2::new(2048.0, 1152.0), "取景中心应是新图中心");
        let want = (4096.0_f32 / 1280.0).max(2304.0 / 800.0);
        assert!(
            (scale - want).abs() < 1e-6,
            "fit 缩放应取较紧一轴 {want}，得 {scale}"
        );
        assert_eq!(view.camera_center, center, "应写回 ViewState 中心");
        assert_eq!(view.scale, scale, "应写回 ViewState 缩放");
    }

    /// 第二张「世界」级地图必须是**独立根图**（2026-10-17 用户报告修复）：
    /// 不挂旧图之下（否则 zoom-out 撞外界会钻回旧图），导航 settle 到新图上，
    /// 旧世界原样保留、children 不被改动。
    #[test]
    fn creating_a_world_level_map_is_an_independent_root() {
        let mut projection = WorldProjection::default();
        let root = Id::new_unchecked("map_root");
        projection.world.maps.push(MapNode {
            id: root.clone(),
            name: "世界".into(),
            level: MapLevel::World,
            extent: Some(Extent::sized(512.0, 512.0)),
            base: Some(MapBase::Ocean),
            ..Default::default()
        });
        projection.world.set_terrain(
            root.clone(),
            TerrainData {
                width: 64,
                height: 64,
                cells: vec![MapBase::Ocean.cell(); 64 * 64],
            },
        );
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&projection.world);
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::World,
            ..Default::default()
        };
        let (id, dirty) =
            create_map(&mut dialog, &mut projection, &drill, &mut edit).expect("创建应成功");
        let node = projection
            .world
            .maps
            .iter()
            .find(|m| m.id == id)
            .expect("新图应在世界里");
        assert_eq!(node.parent, None, "世界层级新图不得有父图");
        assert!(
            !root_children_of(&projection.world, &root).contains(&id),
            "旧世界 children 不得收录新图"
        );
        assert!(
            dirty.iter().any(|p| p == "maps"),
            "dirty 路径应含 maps 目录（创建即落盘的路由依据）"
        );
        // 对话框层的导航分流：settle_on 后 zoom-out（up）原地不动——
        // 这是「zoom out 到最大加载前一张地图」的直接回归断言。
        drill.settle_on(id.clone());
        assert_eq!(drill.current(), Some(&id), "应落在新图上");
        assert!(!drill.up(), "新图上 zoom-out 不得弹回旧图");
        let (node, _) = wv_render::terrain_view::active_terrain(&projection.world, drill.current())
            .expect("新图应带地形");
        assert_eq!(node.id, id);
    }

    /// 世界之下的城市图仍走 P4 挂接（父引用 + 钻出回父图）——独立根图只限世界层级。
    #[test]
    fn creating_a_city_level_map_still_hangs_under_current() {
        let mut projection = WorldProjection::default();
        let root = Id::new_unchecked("map_root");
        projection.world.maps.push(MapNode {
            id: root.clone(),
            name: "世界".into(),
            level: MapLevel::World,
            extent: Some(Extent::sized(512.0, 512.0)),
            base: Some(MapBase::Ocean),
            ..Default::default()
        });
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&projection.world);
        let mut edit = crate::edit::editing::EditHistory::default();
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::City,
            ..Default::default()
        };
        let (id, _) =
            create_map(&mut dialog, &mut projection, &drill, &mut edit).expect("创建应成功");
        let node = projection
            .world
            .maps
            .iter()
            .find(|m| m.id == id)
            .expect("新图应在世界里");
        assert_eq!(
            node.parent.as_ref(),
            Some(&root),
            "城市图应挂在当前图之下（P4）"
        );
        assert!(
            drill.enter(&projection.world, &id),
            "城市图应能从当前图钻入"
        );
        assert!(
            drill.up(),
            "钻出应回到父图（子图 zoom-out 回父图是设计行为）"
        );
    }

    /// UX-026：`parent_override` 让新图挂到**作者选的**图下（不必先钻过去）；
    /// 选中的图失效（如进了垃圾桶）时回退当前钻取图。
    #[test]
    fn parent_override_mounts_under_chosen_map_and_falls_back_when_invalid() {
        // 世界(root) + 一张区域图(region_a)：当前钻在 root。
        let mut projection = WorldProjection::default();
        let root = Id::new_unchecked("map_root");
        let region = Id::new_unchecked("region_a");
        projection.world.maps.push(MapNode {
            id: root.clone(),
            name: "世界".into(),
            level: MapLevel::World,
            extent: Some(Extent::sized(512.0, 512.0)),
            ..Default::default()
        });
        projection.world.maps.push(MapNode {
            id: region.clone(),
            name: "东区".into(),
            level: MapLevel::Region,
            parent: Some(root.clone()),
            extent: Some(Extent::sized(512.0, 512.0)),
            ..Default::default()
        });
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&projection.world);
        let mut edit = crate::edit::editing::EditHistory::default();

        // ① override = region_a → 新图挂在 region_a 下（而非当前的 root）。
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::City,
            parent_override: Some("region_a".into()),
            ..Default::default()
        };
        let new_id = create_map(&mut dialog, &mut projection, &drill, &mut edit)
            .expect("创建")
            .0;
        let parent = projection
            .world
            .maps
            .iter()
            .find(|m| m.id == new_id)
            .and_then(|m| m.parent.clone())
            .expect("新图应有父图");
        assert_eq!(parent, region, "override 应挂到作者选的 region_a");
        assert!(
            root_children_of(&projection.world, &region).contains(&new_id),
            "region_a 的 children 应含新图"
        );

        // ② override 指向垃圾桶里的图 → 回退当前钻取图（root）。
        let mut dialog = NewMapDialog {
            open: true,
            level: MapLevel::City,
            parent_override: Some("ghost_map".into()),
            ..Default::default()
        };
        let new_id2 = create_map(&mut dialog, &mut projection, &drill, &mut edit)
            .expect("创建")
            .0;
        let parent2 = projection
            .world
            .maps
            .iter()
            .find(|m| m.id == new_id2)
            .and_then(|m| m.parent.clone())
            .expect("新图应有父图");
        assert_eq!(parent2, root, "失效 override 应回退当前钻取图");
    }

    /// 从 `world.maps` 里取指定图的 children 克隆（测试辅助）。
    fn root_children_of(world: &wv_core::World, id: &Id) -> Vec<Id> {
        world
            .maps
            .iter()
            .find(|m| &m.id == id)
            .map(|m| m.children.clone())
            .unwrap_or_default()
    }

    /// 幅面 → 栅格换算：4096×2304 wu / 8 wu每格 = 512×288（16:9 整数格）。
    #[test]
    fn grid_cells_derive_from_extent_divided_by_cell_size() {
        let dialog = NewMapDialog {
            extent_w: 4096.0,
            extent_h: 2304.0,
            ..Default::default()
        };
        assert_eq!(dialog.grid_cells_w(), 512, "4096 wu / 8 = 512 格");
        assert_eq!(dialog.grid_cells_h(), 288, "2304 wu / 8 = 288 格");
        let small = NewMapDialog {
            extent_w: 512.0,
            extent_h: 512.0,
            ..Default::default()
        };
        assert_eq!(small.grid_cells_w(), 64);
        assert_eq!(small.grid_cells_h(), 64);
    }

    /// 缺省 16:9（2026-10-17 用户裁定）：4096×2304，栅格 512×288 无余数。
    #[test]
    fn default_new_map_extent_is_16_9_with_whole_cells() {
        let dialog = NewMapDialog::default();
        assert_eq!(dialog.aspect, MapAspect::Wide16x9, "缺省比例应为 16:9");
        assert!(
            (dialog.extent_w - 4096.0).abs() < 1e-3,
            "缺省宽应为 4096，得 {}",
            dialog.extent_w
        );
        assert!(
            (dialog.extent_h - 2304.0).abs() < 1e-3,
            "缺省高应为 2304，得 {}",
            dialog.extent_h
        );
        assert_eq!(dialog.grid_cells_w(), 512, "宽 512 格");
        assert_eq!(dialog.grid_cells_h(), 288, "高 288 格（无余数）");
    }

    /// 切比例只改高、不改宽；选择持久在同一会话里。
    #[test]
    fn aspect_switch_updates_height_only() {
        let mut dialog = NewMapDialog::default();
        dialog.apply_aspect(MapAspect::Standard4x3);
        assert_eq!(dialog.grid_cells_w(), 512, "宽不应被比例切换改动");
        assert!(
            (dialog.extent_h - 3072.0).abs() < 1e-3,
            "4:3 → 高 = 4096×3/4 = 3072，得 {}",
            dialog.extent_h
        );
        dialog.apply_aspect(MapAspect::Square1x1);
        assert!((dialog.extent_h - 4096.0).abs() < 1e-3, "1:1 → 高 = 宽");
        assert_eq!(dialog.aspect, MapAspect::Square1x1, "选择应持久");
    }

    /// 底图语义：海洋必须在海平面下（高度 < 0）、陆地在海平面上（≥ 0）——
    /// 这是 VectorFlat 填充「高度赢过类型」口径下的硬约束。
    #[test]
    fn prefill_ocean_is_below_sea_level_and_land_is_above() {
        let ocean = MapBase::Ocean.cell();
        assert!(
            ocean.height < 0,
            "海洋底图高度应为负，实得 {}",
            ocean.height
        );
        assert!(ocean.surface == 0, "海洋底图地表应为水");
        let land = MapBase::Land.cell();
        assert!(land.height >= 0, "陆地底图高度应非负，实得 {}", land.height);
        assert!(land.surface == 1, "陆地底图地表应为陆");
    }

    /// 2026-10-16 用户裁定：新建对话框的默认底图 = 海洋。
    #[test]
    fn default_prefill_is_ocean_per_user_ruling() {
        let d = NewMapDialog::default();
        assert_eq!(
            d.prefill,
            MapBase::Ocean,
            "默认铺海（便于「先画大陆」场景）"
        );
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
            base: None,
            outline_polygon: None,
            ..Default::default()
        });
        let mut dialog = NewMapDialog::default();
        dialog.open_for(&world);
        assert_eq!(dialog.level, MapLevel::City, "非空世界默认建城市图");
    }
}
