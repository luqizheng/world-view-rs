//! 「新建工程」对话框（2026-10-14 用户需求：参考 VS 的新建项目——名称 + 位置 + 路径预览）。
//!
//! ## 职责
//! - [`NewProjectDialog`] —— 对话框的跨帧 UI 状态（名称 / 位置 / 错误）。
//! - [`new_project_dialog`] —— 绘制 + 确认创建：在「位置\项目名称」下**落盘**
//!   一个新工程（含第一张世界地图），随后经 `load_project_into` 打开它。
//!
//! ## 边界（不做什么）
//! - **不**弹原生目录选择框——「浏览…」只排队意图（`DialogPurpose::PickNewProjectLocation`），
//!   发起与收口都在 `ui_shell::poll_open_project_dialog`（与打开 / 另存为同一条纪律：
//!   分发器与对话框保持纯状态，测试里不会弹真窗口）。
//! - **不**经 `wv-edit` 的命令链构造世界——这里造的是**全新世界**并整体落盘，
//!   成功后由 [`crate::ui_shell::load_project_into`] 整体替换工作副本。这与
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

use wv_core::map::{Extent, MapLevel, MapNode};
use wv_core::terrain::TerrainData;
use wv_core::{Id, TimeSpan};

use crate::edit::editing::EditHistory;
use crate::render::terrain_view::{TerrainTexture, TileAtlas};
use crate::state::{
    DrillState, PlaybackState, ProjectDiagnostics, RecentProjects, ViewState, WorldProjection,
};
use crate::ui_shell::{load_project_into, DialogPurpose, OpenProjectTask, ShellNotice};

// 与「新建地图」对话框同一口径的幅面 / 预填充默认值（首图 = 空世界的第一张图，
// 与 NewMapDialog::open_for 的空世界分支完全一致：世界级、最大幅面、陆地预填充）。
use super::new_map_dialog::{NewMapPrefill, EXTENT_CHOICES, GRID_CELL_SIZE};

/// 新工程首张地图（世界图）的 ID。
///
/// 必须与 `wv_io::Project::create` 写进 `world.yaml` 的 `root_map` 占位**同名**：
/// create 先立骨架（root_map 暂定 `world`），随后 `save_world` 用 `world.meta`
/// 全量覆写——两处口径一致，才不会出现「root_map 指向不存在的图」。
pub(crate) const FIRST_MAP_ID: &str = "world";

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
    /// 上次创建失败的错误（留在框内，作者改完再试；成功即清）。
    pub error: Option<String>,
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: "新工程".into(),
            location: String::new(),
            error: None,
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

/// 新工程的初始世界：工程元数据 + 一张世界级根图 + 陆地预填充栅格。
///
/// 首图参数与「新建地图」对话框对空世界的第一张图完全一致（世界级 / 最大幅面 /
/// 陆地预填充），保证「新建工程」与「新建工程后新建第一张地图」两条路径殊途同归。
fn initial_world(name: &str) -> wv_core::World {
    let side = ((EXTENT_CHOICES[0] / GRID_CELL_SIZE).round() as u32).max(1);
    let id = Id::new_unchecked(FIRST_MAP_ID);
    let mut world = wv_core::World {
        meta: Some(wv_core::WorldMeta {
            name: name.to_owned(),
            version: "0.1".to_owned(),
            schema_version: wv_io::CURRENT_SCHEMA_VERSION,
            root_map: id.clone(),
            // 与 `Project::create` 的占位同口径：作者随后可改。
            timeline_range: TimeSpan::new(0, 1).expect("0..1 是合法 TimeSpan（from < to 恒成立）"),
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
        extent: Some(Extent::sized(EXTENT_CHOICES[0], EXTENT_CHOICES[0])),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
    });
    world.set_terrain(
        id,
        TerrainData {
            width: side,
            height: side,
            cells: vec![NewMapPrefill::Land.cell(); (side * side) as usize],
        },
    );
    world
}

/// 「新建工程」对话框：绘制 + 确认创建。
#[allow(clippy::too_many_arguments)]
pub fn new_project_dialog(
    mut ctxs: EguiContexts,
    mut dialog: ResMut<NewProjectDialog>,
    mut open_task: ResMut<OpenProjectTask>,
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut recents: ResMut<RecentProjects>,
    mut notice: ResMut<ShellNotice>,
    mut diagnostics: ResMut<ProjectDiagnostics>,
    mut edit: ResMut<EditHistory>,
    mut playback: ResMut<PlaybackState>,
    mut view: ResMut<ViewState>,
    mut tex: ResMut<TerrainTexture>,
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    (time, current_loc): (Res<Time>, Res<crate::i18n::CurrentLocale>),
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let frame = (time.elapsed_secs() * 60.0) as u64;
    let mut open = dialog.open;
    let mut confirmed = false;
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
                if ui
                    .add_enabled(dialog.ready(), egui::Button::new("创建"))
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
        match create_project(
            &dialog,
            &mut projection,
            &mut drill,
            &mut recents,
            &mut notice,
            &mut diagnostics,
            &mut edit,
            &mut playback,
            &mut view,
            &mut tex,
            atlas.as_deref(),
            &mut images,
            &mut cameras,
            frame,
            &current_loc,
        ) {
            Ok(()) => dialog.open = false,
            Err(e) => dialog.error = Some(e),
        }
    }
}

/// 确认创建：校验 → 组初始世界 → 磁盘立骨架 + 全量落盘 → 从磁盘打开。
///
/// 落盘三步走（与「另存为到新目录」的 `save_world_to_dir` 同源，但**必然**造首图）：
/// 1. `Project::create`：写 `world.yaml` 骨架（拒绝覆盖已有工程）；
/// 2. `save_world(全量)`：首图的 `index.yaml` + `terrain.bin` + meta 覆写；
/// 3. `load_project_into`：从磁盘打开（最近工程 / 视图 / 纹理 / 播放全部接管，
///    顺带验证刚写下的东西真能读回来）。
#[allow(clippy::too_many_arguments)]
fn create_project(
    dialog: &NewProjectDialog,
    projection: &mut WorldProjection,
    drill: &mut DrillState,
    recents: &mut RecentProjects,
    notice: &mut ShellNotice,
    diagnostics: &mut ProjectDiagnostics,
    edit: &mut EditHistory,
    playback: &mut PlaybackState,
    view: &mut ViewState,
    tex: &mut TerrainTexture,
    atlas: Option<&TileAtlas>,
    images: &mut Assets<Image>,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
) -> Result<(), String> {
    let path = dialog.validated_path()?;
    let name = dialog.name.trim().to_owned();
    let world = initial_world(&name);

    wv_io::Project::create(&path, &name).map_err(|e| format!("创建工程失败：{e}"))?;
    let mut project = wv_io::Project::open(&path).map_err(|e| format!("写入工程文件失败：{e}"))?;
    project
        .save_world(&world, &[])
        .map_err(|e| format!("写入工程文件失败：{e}（目录已建，修好后可重试）"))?;

    let dir = path.display().to_string();
    load_project_into(
        &dir,
        projection,
        drill,
        recents,
        notice,
        diagnostics,
        edit,
        playback,
        view,
        tex,
        atlas,
        images,
        cameras,
        frame,
        current_loc,
    );
    if projection.project_dir.as_deref() != Some(dir.as_str()) {
        // load_project_into 失败分支已写 load_error + 状态栏回执；这里给对话框
        // 一份同样的原因（作者不用去找被欢迎页挡住的状态栏）。
        return Err(format!(
            "工程已落盘但打开失败：{}",
            projection.load_error.clone().unwrap_or_default()
        ));
    }
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

    /// 初始世界形状：meta 就位（root_map 与首图同名）、一张世界级根图、
    /// 陆地预填充栅格（幅面换算与「新建地图」同口径）。
    #[test]
    fn initial_world_has_meta_world_root_map_and_land_raster() {
        let w = initial_world("美洲大陆");
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
        let terrain = w.terrain_of(&map.id).expect("首图必须带预填充栅格");
        let side = (EXTENT_CHOICES[0] / GRID_CELL_SIZE).round() as u32;
        assert_eq!((terrain.width, terrain.height), (side, side));
        assert!(
            terrain.cells.iter().all(|c| c.height >= 0),
            "陆地预填充必须整片在海平面上"
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
        let world = initial_world("美洲大陆");
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
}
