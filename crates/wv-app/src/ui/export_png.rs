//! PNG 导出（UX-003 / round-2 审计：创作此前无出口，Ctrl+E 只弹「尚未实现」）。
//!
//! ## 干净出图的实现
//!
//! `Screenshot` 没有「只拍某台相机」的变体（bevy 0.19 只有窗口 / Image 目标），
//! 窗口截图必然带上 egui 面板。本模块 spawn **第二台 `Camera2d`**：
//! 同 Transform / 同 Orthographic、渲染目标 = 离屏 `Image`——egui 的渲染
//! pass 只挂窗口，不经这台相机，故成像**只有地图**（审计验收标准「无面板
//! 入镜」）。两拍时序：第 N 帧生成导出相机，第 N+1 帧挂 `Screenshot`
//! （保证相机先完成一次渲染；捕获完成事件里自毁相机）。
//!
//! ## 路径约定
//!
//! v1 无对话框：`<工程目录>/export/export-<unix_ts>.png`（一键即出，审计
//! 首选）；失败（无工程 / 相机缺失）弹回执。导出相机的分辨率 = 窗口物理
//! 分辨率。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::ui_shell::ShellNotice;

/// 导出状态机（一次性请求 → 生成相机 → 挂截图 → 收尾）。
#[derive(bevy::prelude::Resource, Debug, Default)]
pub struct ExportPng {
    state: ExportState,
}

#[derive(Debug, Default)]
enum ExportState {
    #[default]
    Idle,
    /// 已请求：本帧生成导出相机（帧 N）。
    SpawnCamera { path: std::path::PathBuf },
    /// 相机已在场：本帧挂 `Screenshot`（帧 N+1）。
    Capture {
        path: std::path::PathBuf,
        camera: Entity,
        image: Handle<Image>,
    },
}

impl ExportPng {
    /// 请求一次导出（`path` 的父目录须已存在；调用方 `create_dir_all`）。
    pub fn request(&mut self, path: std::path::PathBuf) {
        self.state = ExportState::SpawnCamera { path };
    }

    /// 是否有导出在途（重复 Ctrl+E 时提示/忽略）。
    pub fn busy(&self) -> bool {
        !matches!(self.state, ExportState::Idle)
    }
}

/// 导出相机标记（防止 `Without` 查询抓到自己）。
#[derive(bevy::prelude::Component)]
pub struct ExportCamera;

/// 两拍导出系统（Update；render 侧自会在后续帧消费挂上的 `Screenshot`）。
/// 主相机查询类型抽别名（clippy type_complexity）。
type MainCamQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Transform, &'static Projection),
    (With<Camera2d>, Without<ExportCamera>),
>;

#[allow(clippy::too_many_arguments)]
pub fn export_png_system(
    mut export: ResMut<ExportPng>,
    mut commands: Commands,
    windows: Query<&Window>,
    main_cam: MainCamQuery,
    mut images: ResMut<Assets<Image>>,
    mut notice: ResMut<ShellNotice>,
    current_loc: Res<CurrentLocale>,
    time: Res<Time>,
) {
    let frame = (time.elapsed_secs_f64() * 60.0) as u64;
    let loc = current_loc.0;
    match std::mem::take(&mut export.state) {
        ExportState::Idle => {}
        ExportState::SpawnCamera { path } => {
            let Ok(window) = windows.single() else {
                notice.error(t(loc, I18nKey::NoticeExportFailedPrefix), frame);
                return;
            };
            let size = window.resolution.physical_size();
            let Ok((tf, proj)) = main_cam.single() else {
                notice.error(t(loc, I18nKey::NoticeExportFailedPrefix), frame);
                return;
            };
            let image = images.add(bevy::image::Image::new_target_texture(
                size.x.max(1),
                size.y.max(1),
                TextureFormat::Bgra8UnormSrgb,
                None,
            ));
            let camera = commands
                .spawn((
                    Camera2d,
                    // bevy 0.19：RenderTarget 是 Camera 的 require 组件（非字段）。
                    bevy::camera::RenderTarget::Image(bevy::camera::ImageRenderTarget {
                        handle: image.clone(),
                        scale_factor: 1.0,
                    }),
                    // 窗口相机 order 0；离屏相机独立 render target，order 无关序。
                    Camera {
                        order: 10,
                        ..Default::default()
                    },
                    *tf,
                    proj.clone(),
                    ExportCamera,
                ))
                .id();
            export.state = ExportState::Capture {
                path,
                camera,
                image,
            };
        }
        ExportState::Capture {
            path,
            camera,
            image,
        } => {
            // 目标 image 的句柄由相机持有（RenderTarget::Image），此处
            // `Screenshot::image` 引用同一份——截图管理器渲染一帧后触发
            // `ScreenshotCaptured`，两个观察者分别落盘与自毁相机。
            let saved_path = path.clone();
            commands
                .entity(camera)
                .insert(Screenshot::image(image))
                .observe(save_to_disk(saved_path.clone()))
                .observe(
                    move |on: On<bevy::render::view::screenshot::ScreenshotCaptured>,
                          mut commands: Commands,
                          mut notice: ResMut<ShellNotice>,
                          loc: Res<CurrentLocale>| {
                        let _ = on;
                        commands.entity(camera).despawn();
                        notice.info(
                            format!(
                                "{} {}",
                                t(loc.0, I18nKey::NoticeExportSavedPrefix),
                                saved_path.display()
                            ),
                            frame,
                        );
                    },
                );
        }
    }
}

/// 默认导出路径：`<工程目录>/export/export-<unix_ts>.png`（父目录会创建）。
pub fn default_export_path(project_dir: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(project_dir).join("export");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(format!("export-{ts}.png"))
}

/// `default_export_path` 的纯几何部分：文件名带时间戳、落在 export/ 下。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_export_path_lands_in_export_dir_with_timestamp() {
        let p = default_export_path("D:/tmp/proj");
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.contains("proj/export/export-"), "got {s}");
        let name = p.file_name().unwrap().to_string_lossy();
        assert!(
            name.starts_with("export-") && name.ends_with(".png"),
            "got {name}"
        );
    }
}
