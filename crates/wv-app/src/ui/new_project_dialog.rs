//! 「新建工程」对话框（2026-10-14 用户需求：参考 VS 的新建项目——名称 + 位置 + 路径预览）。
//!
//! ## 职责
//! - [`NewProjectDialog`] —— 对话框的跨帧 UI 状态（名称 / 位置 / 错误）。
//! - [`new_project_dialog`] —— 绘制 + 确认创建：在「位置\项目名称」下**落盘**
//!   一个新工程（含第一张世界地图），随后**排队**由后台线程打开它。
//!
//! ## 边界（不做什么）
//! - **不**弹原生目录选择框——「浏览…」只排队意图（`DialogPurpose::PickNewProjectLocation`），
//!   发起与收口都在 `ui_shell::poll_open_project_dialog`（与打开 / 另存为同一条纪律：
//!   分发器与对话框保持纯状态，测试里不会弹真窗口）。
//! - **不**经 `wv-edit` 的命令链构造世界——这里造的是**全新世界**并整体落盘，
//!   成功后由后台加载任务（[`crate::project_loader`]）整体替换工作副本。这与
//!   「加载 / 关闭工程」同类，是 shell 级换世界操作，不是 System 编辑世界
//!   （§3.3 的约束对象）；首图栅格也由 `save_world` 写 `terrain.bin`，不绕过
//!   任何落盘语义。
//!
//! ## 主要导出
//! - `NewProjectDialog` / `new_project_dialog` —— Bevy 资源与系统。

#![cfg(feature = "app")]

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use wv_core::map::{Extent, MapBase, MapLevel, MapNode};
use wv_core::terrain::TerrainData;
use wv_core::{Id, TimeSpan, WorldTemplate, WorldTemplateKind};
use wv_terrain::climate::{generate_cold_north, pick_city_sites};

use crate::project_loader::PendingProjectLoad;
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};
use crate::ui_shell::{DialogPurpose, OpenProjectTask, ShellNotice};

// 与「新建地图」对话框同一口径的幅面 / 预填充默认值（首图 = 空世界的第一张图，
// 与 NewMapDialog::open_for 的空世界分支完全一致：世界级、最大幅面、海洋底图）。
// 2026-10-16 用户裁定：底图默认 = 海洋（与 NewMapDialog 默认同源）。
use super::new_map_dialog::{DEFAULT_ASPECT, EXTENT_CHOICES, GRID_CELL_SIZE};
use super::shell::UiShell;

/// 新工程首张地图（世界图）的 ID。
///
/// 必须与 `wv_io::Project::create` 写进 `world.yaml` 的 `root_map` 占位**同名**：
/// create 先立骨架（root_map 暂定 `world`），随后 `save_world` 用 `world.meta`
/// 全量覆写——两处口径一致，才不会出现「root_map 指向不存在的图」。
pub(crate) const FIRST_MAP_ID: &str = "world";

/// 「新建工程」待消费的"创建意图"——UI 系统按下「创建」时打包进去，
/// apply 系统下一帧读走并落盘 / 打开。
///
/// 拆 UI 系统与 apply 系统的目的是让 UI 系统保持在 Bevy 16 系统参数上限内
/// （原本 UI 系统一次性拿全部 ResMut 已经踩线到 16，加 shell_cfg 17 直接
/// 触发 `the trait bound IntoSystem is not satisfied`，详见 2026-10-15 拆分 commit）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectIntent {
    /// 用户敲定的工程目录（`位置\名称` 拼出来的 PathBuf）。
    pub path: std::path::PathBuf,
    /// 项目显示名（同 [`NewProjectDialog::name`]）。
    pub name: String,
    /// `time.elapsed_secs() * 60.0` 计算出的帧号，给 `ShellNotice` 用。
    pub frame: u64,
    /// 新建世界采用的模板（ADR-0037 D1/D2）。
    pub template: WorldTemplateKind,
}

/// 「新建工程」对话框的跨帧状态。
///
/// **为什么是资源而不是 egui memory**：打开它的可以是命令分发器也可以是欢迎页
/// （都拿不到 egui 窗口记忆），且创建失败 / 位置选择回调都要跨帧写状态。
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct NewProjectDialog {
    /// 是否打开。
    pub open: bool,
    /// 项目名称（可中文；将作为**目录名**出现在 `位置\名称` 落点里）。
    pub name: String,
    /// 位置（父目录的绝对路径；「浏览…」由原生目录对话框回填）。
    pub location: String,
    /// 新建世界采用的模板（ADR-0037 D1/D2，缺省空海洋）。
    pub template: WorldTemplateKind,
    /// 上次创建失败的错误（留在框内，作者改完再试；成功即清）。
    pub error: Option<String>,
    /// UI 系统按下「创建」时把已校验的输入打包放这里，下一帧 apply 系统取走
    /// 执行落盘 + 打开。`take()` 后下次创建前保持 `None`。
    pub pending_apply: Option<NewProjectIntent>,
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: "新工程".into(),
            location: String::new(),
            template: WorldTemplateKind::Ocean,
            error: None,
            pending_apply: None,
        }
    }
}

impl NewProjectDialog {
    /// 打开对话框并重置默认值。
    ///
    /// **保留上一次的位置**（若有）：作者连建几个工程时通常落在同一父目录下，
    /// 每次重选一遍是纯摩擦（VS 的新建项目同款记忆行为）。
    pub fn open(&mut self) {
        let keep_location = self.location.trim().to_owned();
        *self = Self::default();
        self.open = true;
        if !keep_location.is_empty() {
            self.location = keep_location;
        }
    }

    /// 工程落点：`位置 \ 项目名称`。
    ///
    /// 用户示例：位置 `d:\demo\game` + 名称「美洲大陆」→ `d:\demo\game\美洲大陆`。
    /// 用 [`Path::join`] 而不是字符串拼接——分隔符跟随平台，尾部的 `\` 也自然消化。
    pub fn project_path(&self) -> PathBuf {
        Path::new(self.location.trim()).join(self.name.trim())
    }

    /// 输入校验（纯内存，不碰磁盘）：名称非空且不含目录名非法字符，
    /// 位置非空且为绝对路径。磁盘侧的「已是工程」检查交给
    /// `wv_io::Project::create` 的 `AlreadyAProject`（磁盘 IO 收口在 wv-io）。
    fn validated_path(&self) -> Result<PathBuf, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("请输入项目名称".into());
        }
        // Windows 目录名非法字符（`\` 之外的都在此处拦截；`\` 同列）。
        const ILLEGAL: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
        if let Some(c) = name.chars().find(|c| ILLEGAL.contains(c)) {
            return Err(format!("项目名称不能包含「{c}」（目录名非法字符）"));
        }
        let location = self.location.trim();
        if location.is_empty() {
            return Err("请选择位置（项目目录将创建在它下面）".into());
        }
        if !Path::new(location).is_absolute() {
            return Err("位置需要是绝对路径，例如 D:\\projects，或点「浏览…」选择".into());
        }
        Ok(Path::new(location).join(name))
    }

    /// 名称与位置是否都已填（「创建」按钮的启用条件；细节校验在确认时做）。
    fn ready(&self) -> bool {
        !self.name.trim().is_empty() && !self.location.trim().is_empty()
    }
}

/// 新工程的初始世界：工程元数据 + 一张世界级根图 + 模板地形。
///
/// 首图参数与「新建地图」对话框对空世界的第一张图完全一致（世界级 / 最大幅面 /
/// 16:9，2026-10-16 / 2026-10-17 用户裁定），保证「新建工程」与「新建工程后
/// 新建第一张地图」两条路径殊途同归。`Ocean` 模板明确写 `MapNode.base` →
/// 「重置到底图」按钮可用；生成模板不写 `base`（重置会毁掉模板，按钮自动禁用）。
///
/// 生成模板（ADR-0037）用 `seed` 决定海岸线 / 山脉；同 seed 同结果，
/// 重生成命令（M2-4）靠它复现。
fn initial_world(name: &str, template: WorldTemplateKind, seed: u64) -> wv_core::World {
    // 2026-10-17 用户裁定：新建图缺省 16:9（与 NewMapDialog 同源）。
    let extent_w = EXTENT_CHOICES[0];
    let extent_h = extent_w / DEFAULT_ASPECT.ratio();
    let width = ((extent_w / GRID_CELL_SIZE).round() as u32).max(1);
    let height = ((extent_h / GRID_CELL_SIZE).round() as u32).max(1);
    let id = Id::new_unchecked(FIRST_MAP_ID);
    let mut world = wv_core::World {
        meta: Some(wv_core::WorldMeta {
            name: name.to_owned(),
            version: "0.1".to_owned(),
            schema_version: wv_io::CURRENT_SCHEMA_VERSION,
            root_map: id.clone(),
            // 与 `Project::create` 的占位同口径：作者随后可改。
            timeline_range: TimeSpan::new(0, 1).expect("0..1 是合法 TimeSpan（from < to 恒成立）"),
            template: (template != WorldTemplateKind::Ocean).then_some(WorldTemplate {
                kind: template,
                seed,
                rivers_seed: seed ^ 0x9E37_79B9_7F4A_7C15,
            }),
        }),
        ..Default::default()
    };
    world.maps.push(MapNode {
        id: id.clone(),
        name: "世界".into(),
        level: MapLevel::World,
        parent: None,
        children: Vec::new(),
        layers: Vec::new(),
        icons: Vec::new(),
        extent: Some(Extent::sized(extent_w, extent_h)),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        // 2026-10-16：空海洋模板写 base（重置到底图可用）；生成模板不写 base。
        base: (template == WorldTemplateKind::Ocean).then_some(MapBase::Ocean),
        outline_polygon: None,
    });
    let mut city_sites: Option<(Vec<[f32; 2]>, u32, u32)> = None;
    let terrain = match template {
        WorldTemplateKind::Ocean => TerrainData {
            width,
            height,
            cells: vec![MapBase::Ocean.cell(); (width * height) as usize],
        },
        WorldTemplateKind::ColdNorth => {
            // 模板按 16:9 的栅格尺寸生成（与首图幅面同源），不强制 1024²。
            let generated = generate_cold_north(width, height, seed);
            // 城市初布点（ADR-0037 D6）：宜居群系散 8 城（前 2 城 rank 3）。
            // 与河流同一「派生」精神，但城市是**作者实体**（可改名 / 拖动 /
            // 删除，V2），所以落盘为 Place 而不是运行时派生。
            let heights: Vec<i16> = generated.cells.iter().map(|c| c.height).collect();
            let biomes: Vec<u8> = generated.cells.iter().map(|c| c.biome).collect();
            let sites = pick_city_sites(
                generated.width,
                generated.height,
                &heights,
                &biomes,
                seed ^ 0x9E37_79B9_7F4A_7C15,
                8,
            );
            city_sites = Some((sites, generated.width, generated.height));
            TerrainData {
                width: generated.width,
                height: generated.height,
                cells: generated
                    .cells
                    .into_iter()
                    .map(|c| wv_core::terrain::TerrainCellData {
                        height: c.height,
                        surface: c.surface,
                        biome: c.biome,
                    })
                    .collect(),
            }
        }
        WorldTemplateKind::AridBelt => TerrainData {
            // 预留模板：M2 未实现，按空海洋兜底（V1：不崩、不阻止创建）。
            width,
            height,
            cells: vec![MapBase::Ocean.cell(); (width * height) as usize],
        },
    };
    // 生成器河实体（ADR-0070 D3 生成器统一 · 计划 Task 12）：种子河在
    // **创建期即实体**（渲染期 seed 派生已删，T8）——与下面的城市初布点
    // 同款「创建期即 Place」：工程首保存即落盘，之后河是作者的实体
    // （可改名 / 拖端点 / 删除，V2）。无下切、宽度不设；Ocean 无模板
    // 记录 → 无河（种子口径与 RegenWorld 同源，见 `edit::river_gen`）。
    let river_places = match world.meta.as_ref().and_then(|m| m.template) {
        Some(tpl) => crate::edit::river_gen::generator_river_places(
            &id,
            &terrain,
            tpl.rivers_seed,
            (extent_w, extent_h),
        ),
        None => Vec::new(),
    };
    world.set_terrain(id.clone(), terrain);
    world.distances.places.extend(river_places);

    // 城市初布点（ADR-0037 D6）：归一化落点 -> 图局部坐标（左上原点、y 向下，
    // 与 icon anchor 同一口径）。名字走「新城 N」占位，作者随便改（V2）。
    if let Some((sites, gw, gh)) = city_sites {
        let extent = world
            .maps
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.extent)
            .unwrap_or_else(|| Extent::sized(gw as f32, gh as f32));
        for (i, [nx, ny]) in sites.into_iter().enumerate() {
            let rank = if i < 2 { 3 } else { 2 };
            world.distances.places.push(wv_core::map::distance::Place {
                kind: None,
                line: None,
                id: Id::new_unchecked(format!("city_new_{}", i + 1)),
                name: format!("新城 {}", i + 1),
                feature: None,
                map: Some(id.clone()),
                x: Some(nx * extent.width),
                y: Some(ny * extent.height),
                w: None,
                h: None,
                biome: None,
                envelope: None,
                rank: Some(rank),
                line_kind: None,
                line_width_m: None,
            });
        }
    }
    world
}

/// 「新建工程」对话框 UI 渲染（5 个系统参数）。
///
/// 该函数只负责画窗口 + 收集输入 + 在「创建」被按下时把已校验输入打包放进
/// [`NewProjectDialog::pending_apply`]，由 [`new_project_apply_create`] 系统下
/// 一帧读走并执行落盘 / 打开。
///
/// **为什么拆成两个系统**：原 `new_project_dialog` 一个系统拿了 17 个 ResMut，
/// 超过 Bevy 0.19 系统参数上限 16（`impl_system_function, 0, 16, F`），拆 UI /
//  apply 两个系统后两边均落在上限内。Apply 系统那 14 个参数（投影 / 钻取 /
/// 资源 / 纹理 / 摄像机 + 时间 + 对话框）刚好压线下。
#[allow(clippy::too_many_arguments)]
pub fn new_project_dialog(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<NewProjectDialog>,
    mut open_task: ResMut<OpenProjectTask>,
    time: Res<Time>,
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
    let mut confirmed_intent: Option<NewProjectIntent> = None;
    let mut cancelled = false;

    egui::Window::new("新建工程")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        // 模态置顶（2026-10-14 用户要求）：与 new_map_dialog 同一口径——
        // Foreground 压过浮动面板（Middle），Tooltip 仍在其上。
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("项目名称")
                    .on_hover_text("可中文；将与位置拼成项目目录");
                let mut name = dialog.name.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .desired_width(320.0)
                            .hint_text("例如：美洲大陆"),
                    )
                    .changed()
                {
                    dialog.name = name.trim().to_owned();
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("位置");
                let mut loc = dialog.location.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut loc)
                            .desired_width(286.0)
                            .hint_text("项目将创建在这个目录下"),
                    )
                    .changed()
                {
                    // 不 trim：路径里的空格是合法的，尾部斜杠交给 join 消化。
                    dialog.location = loc;
                }
                if ui.button("浏览…").clicked() {
                    // 只排队意图；原生窗口由 poll_open_project_dialog 统一发起，
                    // 选完的路径也由它回填 `dialog.location`（与本模块文档一致）。
                    open_task.request(DialogPurpose::PickNewProjectLocation);
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("世界模板")
                    .on_hover_text("冷北世界会生成双大陆 + 气候带；空海洋是空白起点");
                egui::ComboBox::from_id_salt("new_project_template")
                    .selected_text(template_label(dialog.template))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut dialog.template,
                            WorldTemplateKind::Ocean,
                            "空海洋（空白起点）",
                        );
                        ui.selectable_value(
                            &mut dialog.template,
                            WorldTemplateKind::ColdNorth,
                            "冷北世界（大陆 + 气候带）",
                        );
                    });
            });

            ui.add_space(6.0);
            // 路径预览（用户需求原文：显示「项目将会在 [位置]\项目名称 创建目录」）。
            let preview = if dialog.ready() {
                format!("项目将在 {} 创建目录", dialog.project_path().display())
            } else {
                "项目将在 位置\\项目名称 创建目录（先填名称、选位置）".to_owned()
            };
            ui.label(
                egui::RichText::new(preview)
                    .text_style(egui::TextStyle::Small)
                    .color(ui.style().visuals.weak_text_color()),
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
                    dialog.ready(),
                )
                .clicked()
                {
                    // 在 UI 线程完成输入校验，避免 apply 系统再依赖 UI 内的变量；
                    // 校验失败把错误塞回对话框而不是塞进 intent——让作者就在框
                    // 内修。
                    match dialog.validated_path() {
                        Ok(path) => {
                            confirmed_intent = Some(NewProjectIntent {
                                path,
                                name: dialog.name.trim().to_owned(),
                                frame,
                                template: dialog.template,
                            });
                        }
                        Err(e) => dialog.error = Some(e),
                    }
                }
            });
        });

    // X（`open(&mut open)` 写回）与「取消」都走关；「创建」失败时留在框内。
    dialog.open = open && !cancelled;
    // 把确认的意图交给 apply 系统；apply 系统下一帧才会拿到，本帧这段
    // `pending_apply` 不为空时对话框应保持关闭——成功的归 apply，失败的归
    // `dialog.error`，后者仍要求对话框打开以让作者改。
    if let Some(intent) = confirmed_intent {
        dialog.open = false;
        dialog.pending_apply = Some(intent);
        dialog.error = None;
    }
}

/// 消费 [`NewProjectDialog::pending_apply`]：落盘 + 打开 + 回执。
///
/// 系统参数 14 个，刚好压在 Bevy 0.19 `impl_system_function, 0, 16` 上限内。
/// 若以后需要再往里塞资源，先考虑抽一个中间 struct 而不是突破上限。
#[allow(clippy::too_many_arguments)]
pub fn new_project_apply_create(
    mut dialog: ResMut<NewProjectDialog>,
    mut pending: ResMut<PendingProjectLoad>,
    mut notice: ResMut<ShellNotice>,
) {
    let Some(intent) = dialog.pending_apply.take() else {
        return;
    };
    if let Err(e) = create_project(&intent, &mut pending, &mut notice) {
        // 失败时让对话框再次打开让作者改。
        dialog.error = Some(e);
        dialog.open = true;
    }
}

/// 模板下拉的显示名。
fn template_label(template: WorldTemplateKind) -> &'static str {
    match template {
        WorldTemplateKind::Ocean => "空海洋（空白起点）",
        WorldTemplateKind::ColdNorth => "冷北世界（大陆 + 气候带）",
        WorldTemplateKind::AridBelt => "干旱世界（预留）",
    }
}

/// 墙钟种子：纳秒低位足够区分连续创建的工程。
pub(crate) fn seed_from_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x0D0E_5EED_2026)
}

/// 确认创建：校验 → 组初始世界 → 磁盘立骨架 + 全量落盘 → 从磁盘打开。
///
/// 落盘三步走（与「另存为到新目录」的 `save_world_to_dir` 同源，但**必然**造首图）：
/// 1. `Project::create`：写 `world.yaml` 骨架（拒绝覆盖已有工程）；
/// 2. `save_world(全量)`：首图的 `index.yaml` + `terrain.bin` + meta 覆写；
/// 3. **排队**由后台加载任务打开（最近工程 / 视图 / 纹理 / 播放全部接管，
///    顺带验证刚写下的东西真能读回来）——2026-10-22 起打开不再阻塞窗口。
///
/// 输入校验已在 UI 系统打包 [`NewProjectIntent`] 时完成（`validated_path()`），
/// 此处不再重复。
fn create_project(
    intent: &NewProjectIntent,
    pending: &mut PendingProjectLoad,
    notice: &mut ShellNotice,
) -> Result<(), String> {
    let path = intent.path.clone();
    let name = intent.name.clone();
    let frame = intent.frame;
    // 墙钟种子：每次新建拿到不同的海岸线；种子随 meta 落盘，重生成可复现。
    let seed = seed_from_clock();
    let world = initial_world(&name, intent.template, seed);

    wv_io::Project::create(&path, &name).map_err(|e| format!("创建工程失败：{e}"))?;
    let mut project = wv_io::Project::open(&path).map_err(|e| format!("写入工程文件失败：{e}"))?;
    project
        .save_world(&world, &[])
        .map_err(|e| format!("写入工程文件失败：{e}（目录已建，修好后可重试）"))?;

    let dir = path.display().to_string();
    // 打开**排队**即可：真正的读取在后台线程（`project_loader`），主线程
    // 轮询结果后接管工作副本。打开失败的原因由加载错误卡片展示（窗口内
    // 可见，V1），不再由本函数同步判定。
    pending.0 = Some(dir.clone());
    notice.info(format!("已创建工程「{name}」：{dir}"), frame);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 落点拼接：位置 + 名称 → 完整项目目录（用户示例的口径）。
    #[test]
    fn project_path_joins_location_and_name() {
        let dialog = NewProjectDialog {
            location: r"d:\demo\game".into(),
            name: "美洲大陆".into(),
            ..Default::default()
        };
        assert_eq!(
            dialog.project_path(),
            PathBuf::from(r"d:\demo\game").join("美洲大陆"),
            "落点必须是 位置\\名称"
        );
        // 尾部分隔符 / 首尾空格都应消化，不产生双斜杠。
        let dialog = NewProjectDialog {
            location: r"d:\demo\game\ ".into(),
            name: " 美洲大陆 ".into(),
            ..Default::default()
        };
        assert_eq!(
            dialog.project_path(),
            PathBuf::from(r"d:\demo\game").join("美洲大陆")
        );
    }

    /// 校验：空名称 / 非法字符 / 空位置 / 相对路径逐条给出人话错误。
    #[test]
    fn validated_path_rejects_bad_input_with_readable_errors() {
        let base = NewProjectDialog {
            location: r"d:\demo".into(),
            name: "美洲大陆".into(),
            ..Default::default()
        };
        assert!(base.validated_path().is_ok());

        let mut d = base.clone();
        d.name = "  ".into();
        assert!(d.validated_path().unwrap_err().contains("项目名称"));

        let mut d = base.clone();
        d.name = "a:b".into();
        assert!(
            d.validated_path().unwrap_err().contains(':'),
            "目录名非法字符必须拦截"
        );

        let mut d = base.clone();
        d.location = " ".into();
        assert!(d.validated_path().unwrap_err().contains("位置"));

        let mut d = base;
        d.location = r"demo\game".into();
        assert!(
            d.validated_path().unwrap_err().contains("绝对路径"),
            "相对路径会让工程落到不确定的工作目录，必须拦"
        );
    }

    /// 默认海洋锚定（2026-10-17 ocean-default-16-9 Task 1）：新建工程首图
    /// 必须是 Ocean 底图，且 Ocean 单格高度在海平面以下——「全零格」在 Q1
    /// 高度契约下是歧义态（0 = 海平面），预填负高度消歧。
    #[test]
    fn world_map_prefill_is_ocean_below_sea_level() {
        let w = initial_world("海洋锚定", WorldTemplateKind::Ocean, 0);
        let map = &w.maps[0];
        assert_eq!(map.base, Some(MapBase::Ocean), "首图底图必须是 Ocean");
        assert!(
            MapBase::Ocean.cell().height < 0,
            "Ocean 单格高度必须 < 0（海平面 = 0），得 {}",
            MapBase::Ocean.cell().height
        );
        let terrain = w.terrain_of(&map.id).expect("首图必须带预填充栅格");
        assert!(
            terrain.cells.iter().all(|c| c.height < 0 && c.surface == 0),
            "首图预填充必须整图深水负高度"
        );
    }

    /// 初始世界形状：meta 就位（root_map 与首图同名）、一张世界级根图、
    /// 海洋底图预填充栅格（2026-10-16 默认 = 海洋；幅面换算与「新建地图」同口径）。
    /// `MapNode.base` 也应明确为 Ocean，让「重置到底图」按钮可用。
    #[test]
    fn initial_world_has_meta_world_root_map_and_ocean_raster() {
        let w = initial_world("美洲大陆", WorldTemplateKind::Ocean, 0);
        let meta = w
            .meta
            .clone()
            .expect("初始世界必须带 meta（否则保存时 world.yaml 退回占位）");
        assert_eq!(meta.name, "美洲大陆");
        assert_eq!(meta.root_map.as_str(), FIRST_MAP_ID);
        assert_eq!(w.maps.len(), 1, "初始世界只带一张图");
        let map = &w.maps[0];
        assert_eq!(map.id.as_str(), FIRST_MAP_ID);
        assert_eq!(map.level, MapLevel::World);
        assert!(map.parent.is_none(), "首图是钻取树的根，无父图");
        assert_eq!(
            map.base,
            Some(MapBase::Ocean),
            "首图必须声明 base = Ocean，让「重置到底图」按钮可用"
        );
        let terrain = w.terrain_of(&map.id).expect("首图必须带预填充栅格");
        // 16:9：4096×2304 wu / 8 = 512×288 格（无余数）。
        assert_eq!((terrain.width, terrain.height), (512, 288));
        assert!(
            terrain.cells.iter().all(|c| c.height < 0),
            "海洋底图必须整片在海平面下"
        );
        assert!(
            terrain.cells.iter().all(|c| c.surface == 0),
            "海洋底图地表必须全部为水"
        );
    }

    /// 磁盘 round-trip：initial_world → create + 全量保存 → 重新打开，
    /// 世界形状原样读回（首图 / 栅格 / meta）。这是「新建工程即落盘」的主链路。
    #[test]
    fn initial_world_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "wv_new_project_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let world = initial_world("美洲大陆", WorldTemplateKind::Ocean, 0);
        wv_io::Project::create(&dir, "美洲大陆").expect("create 骨架");
        let mut project = wv_io::Project::open(&dir).expect("open 骨架");
        project
            .save_world(&world, &[])
            .expect("全量落盘（含 terrain.bin）");

        let loaded = project.load_world().expect("读回");
        assert_eq!(loaded.maps.len(), 1);
        assert_eq!(loaded.maps[0].id.as_str(), FIRST_MAP_ID);
        assert!(
            loaded.maps[0].raster.is_some(),
            "save_world 必须把 RasterRef 写回 index.yaml，否则重开后首图无栅格"
        );
        assert!(loaded.terrain_of(&loaded.maps[0].id).is_some());
        assert_eq!(
            loaded.meta.as_ref().map(|m| m.name.as_str()),
            Some("美洲大陆")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 冷北模板：生成双大陆栅格（有海有陆）+ 模板种子写进 meta + 不写 base。
    #[test]
    fn cold_north_template_generates_land_and_records_seed() {
        let w = initial_world("冷北", WorldTemplateKind::ColdNorth, 42);
        let map = &w.maps[0];
        assert_eq!(map.base, None, "生成模板不能写 base——重置到底图会毁掉模板");
        let terrain = w.terrain_of(&map.id).expect("模板必须带栅格");
        assert_eq!(
            (terrain.width, terrain.height),
            (512, 288),
            "首图 16:9 / cell 8 的栅格尺寸"
        );
        assert!(
            terrain.cells.iter().any(|c| c.surface == 1),
            "冷北模板必须生成陆地"
        );
        assert!(
            terrain.cells.iter().any(|c| c.surface == 0),
            "冷北模板必须生成海"
        );
        let meta = w.meta.expect("初始世界必须带 meta");
        let t = meta.template.expect("模板记录必须写入 meta");
        assert_eq!(t.kind, WorldTemplateKind::ColdNorth);
        assert_eq!(t.seed, 42, "种子必须原样记录，重生成才可复现");
        assert_ne!(t.rivers_seed, t.seed, "河流种子必须独立");
        // 城市初布点（ADR-0037 D6）：模板世界自带占位城市，全部锚定首图。
        // （T12 起生成器河也是 Place——它们的断言归
        // `template_new_project_has_river_entities`，这里只看城市。）
        let cities: Vec<_> = w
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with("city_"))
            .collect();
        assert!(!cities.is_empty(), "冷北模板必须自带城市");
        for p in &cities {
            assert_eq!(p.map.as_ref(), Some(&map.id), "城市必须锚定首图");
            assert!(p.x.is_some() && p.y.is_some(), "城市必须有坐标");
            assert!(p.effective_rank() >= 2, "模板城市 rank >= 2");
        }
        assert!(
            meta.template.is_some() && map.base.is_none(),
            "模板记录与 base 的互斥口径"
        );
    }

    /// 冷北模板的 meta（含 seed）必须能过磁盘 round-trip。
    #[test]
    fn cold_north_template_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "wv_cold_north_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let world = initial_world("冷北", WorldTemplateKind::ColdNorth, 777);
        wv_io::Project::create(&dir, "冷北").expect("create 骨架");
        let mut project = wv_io::Project::open(&dir).expect("open 骨架");
        project.save_world(&world, &[]).expect("全量落盘");

        let loaded = project.load_world().expect("读回");
        let t = loaded
            .meta
            .as_ref()
            .expect("meta 必须写回 world.yaml")
            .template
            .expect("template 字段必须写回 world.yaml");
        assert_eq!(t.kind, WorldTemplateKind::ColdNorth);
        assert_eq!(t.seed, 777);
        assert!(loaded.terrain_of(&loaded.maps[0].id).is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 「创建」按钮只在名称与位置都非空时可用（细节校验留给确认时）。
    #[test]
    fn ready_requires_both_name_and_location() {
        let mut d = NewProjectDialog::default();
        assert!(!d.ready());
        d.name = "美洲大陆".into();
        assert!(!d.ready(), "只有名称不够");
        d.location = r"d:\demo".into();
        assert!(d.ready());
    }

    /// 重开对话框保留上次的位置（连建几个工程时少一次目录选择）。
    #[test]
    fn reopen_preserves_the_last_location() {
        let mut d = NewProjectDialog {
            location: r"d:\demo\game".into(),
            error: Some("旧错误".into()),
            ..Default::default()
        };
        d.open();
        assert!(d.open);
        assert_eq!(d.location, r"d:\demo\game", "位置应跨打开保留");
        assert!(d.error.is_none(), "错误必须清掉（旧失败不再成立）");
    }

    /// 生成器河实体（计划 Task 12 点名）：生成模板新建工程在世界创建期即落
    /// 河线 POI（与城市初布点同款「创建期即实体」），数量与 `rivers_seed`
    /// 口径一致、实体字段合规、同种子确定性可重放；Ocean 无模板 → 无河。
    #[test]
    fn template_new_project_has_river_entities() {
        use crate::edit::river_gen::{GENERATOR_RIVER_PREFIX, MAX_GENERATOR_RIVERS};
        use wv_terrain::climate::generate_rivers;

        let seed = 7_u64;
        let w = initial_world("冷北", WorldTemplateKind::ColdNorth, seed);
        let map_id = Id::new_unchecked(FIRST_MAP_ID);
        let rivers: Vec<&wv_core::map::distance::Place> = w
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with(GENERATOR_RIVER_PREFIX))
            .collect();
        assert!(
            !rivers.is_empty(),
            "生成模板新建工程必须带生成器河实体，实得 {} 条（总 {} 条）",
            rivers.len(),
            w.distances.places.len()
        );

        // 数量与种子口径一致：= generate_rivers(首图地形, rivers_seed) 折线数。
        let terrain = w.terrain_of(&map_id).expect("首图带地形");
        let tpl = w
            .meta
            .as_ref()
            .and_then(|m| m.template)
            .expect("生成模板带 template 记录");
        let heights: Vec<i16> = terrain.cells.iter().map(|c| c.height).collect();
        let expected = generate_rivers(
            terrain.width,
            terrain.height,
            &heights,
            tpl.rivers_seed,
            MAX_GENERATOR_RIVERS,
        )
        .len();
        assert_eq!(rivers.len(), expected, "生成器河实体数应与种子折线数一致");

        // 字段合规：锚定首图、河语义显式、宽度不设、折线在图幅内。
        let extent = w
            .maps
            .iter()
            .find(|m| m.id == map_id)
            .and_then(|m| m.extent)
            .expect("首图带幅面");
        for p in &rivers {
            assert_eq!(p.map.as_ref(), Some(&map_id), "必须锚定首图");
            assert_eq!(
                p.line_kind,
                Some(wv_core::map::distance::LineKind::River),
                "河语义显式"
            );
            assert_eq!(p.line_width_m, None, "宽度不设");
            let l = p.line.as_ref().expect("有折线");
            assert!(
                l.chunks(2).all(|c| {
                    c[0] >= 0.0 && c[0] <= extent.width && c[1] >= 0.0 && c[1] <= extent.height
                }),
                "折线必须换算到图幅内，实得 {l:?}"
            );
        }

        // 确定性：同种子再建 → 同一批折线（重生成可复现的创建期镜像）。
        let again = initial_world("冷北", WorldTemplateKind::ColdNorth, seed);
        let lines_of = |world: &wv_core::World| {
            let mut v: Vec<Vec<f32>> = world
                .distances
                .places
                .iter()
                .filter(|p| p.id.as_str().starts_with(GENERATOR_RIVER_PREFIX))
                .filter_map(|p| p.line.clone())
                .collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        assert_eq!(lines_of(&w), lines_of(&again), "同种子应产同一批河");

        // Ocean：无 template 记录 → 无生成器河。
        let ocean = initial_world("海洋", WorldTemplateKind::Ocean, seed);
        assert!(
            !ocean
                .distances
                .places
                .iter()
                .any(|p| p.id.as_str().starts_with(GENERATOR_RIVER_PREFIX)),
            "海洋模板不得产生成器河"
        );
    }
}
