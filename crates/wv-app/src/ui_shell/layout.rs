//! 布局持久化：`ui-layout.yaml` 的读回与写盘。

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::i18n::{parse as parse_locale, CurrentLocale};
use crate::state::{Panel, PanelState, ViewState, WorldProjection};
use crate::theme::palette::MapPalette;
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::UiTheme;
use crate::ui_layout::LayoutFile;

/// 布局持久化：把面板的**真实几何**回写 `ui-layout.yaml`（Q5）。
///
/// 位置的真相在 egui 的窗口记忆里（`Memory::area_rect`），不在 [`crate::ui_layout::default_layout`]。
/// 拖动过程中不写盘（`is_using_pointer`），松手后的那一帧才落盘——避免 60 次/秒的文件写。
/// 打开工程时读回 `ui-layout.yaml` 里**当前已落盘的 layout 偏好**。
///
/// 当前覆盖：
/// - `theme`（2026-10-18：界面日夜模式，`dark` / `light`）
/// - `map_theme`（ADR-0016：日间 / 暮色）
/// - `language`（ADR-0018：zh / en，注入 `CurrentLocale`）
/// - `map_dock` / `map_snap`（2026-10-19：地图工具条朝向与吸附开关）
///
/// 界面 theme 与地图 map_theme 是两条独立的线（D2）：前者只影响外壳与 egui
/// 系统控件，后者只影响地图配色。不可识别值静默保留当前值（V1，不阻塞加载）。
///
/// **追加说明（2026-10-18）**：此前本系统刻意不读回软件壳 `theme` / `density`，
/// 理由是「避免修一个字段扩成激活整套壳配置」。日夜模式落地后，`theme` 必须随
/// 工程恢复（否则按钮切完下次打开又回到默认）；`density` 仍不读回，保持原状，
/// 待单独裁定。
///
/// 面板几何由 egui 自带的窗口记忆按 panel id 接管，不属于本函数职责。
pub(crate) fn load_layout_on_project_open(
    project: Res<WorldProjection>,
    mut layout: ResMut<LayoutFile>,
    mut view: ResMut<ViewState>,
    mut shell_cfg: ResMut<UiShellConfig>,
    mut current_loc: ResMut<CurrentLocale>,
    mut dock: ResMut<crate::ui::map_controls::MapDockState>,
    mut loaded_dir: Local<Option<String>>,
) {
    let Some(dir) = project.project_dir.clone() else {
        return;
    };
    if loaded_dir.as_ref() == Some(&dir) {
        return;
    }
    let Some(path) = crate::ui_layout::layout_path(Some(&dir)) else {
        return;
    };
    let file = LayoutFile::load(&path);
    if let Some(theme) = file.theme.as_deref().and_then(UiTheme::from_key) {
        shell_cfg.theme = theme;
    }
    // 地图主题（ADR-0023 M2）：`map_theme` 存**主题包 id**。
    // 存量工程存的是 `MapPalette::key()`（"daylight" / "dusk"）——由
    // `switch::resolve_pack` 的兼容回退解析（内置包 id 与之不同名），零迁移。
    if let Some(t) = file.map_theme.as_deref() {
        view.theme_pack_id = Some(t.to_string());
        // 兼容：存量键映射回 `map_palette`（内置包的快捷选择器）。
        if let Some(pal) = MapPalette::from_key(t) {
            view.map_palette = pal;
        }
    }
    if let Some(lang) = file.language.as_deref() {
        if let Some(loc) = parse_locale(lang) {
            // V1：language 缺字段 / 不可解析 → 保留 CurrentLocale 当前值
            // （project 没换就不该重置；project 第一次打开保持默认 Zh）。
            current_loc.0 = loc;
        }
    }
    // 地图工具条偏好（2026-10-19）：缺字段 / 不可识别 → 保留当前值（V1）。
    if let Some(orient) = file
        .map_dock
        .as_deref()
        .and_then(crate::ui::map_controls::MapDockOrientation::from_key)
    {
        dock.orientation = orient;
    }
    if let Some(snap) = file.map_snap {
        dock.snapping = snap;
    }
    if let Some([x, y]) = file.map_dock_pos {
        dock.pos = Some(crate::ui::map_controls::MapDockPos { x, y });
    }
    if let Some([x, y]) = file.map_zoom_strip_pos {
        dock.zoom_strip_pos = Some(crate::ui::map_controls::MapDockPos { x, y });
    }
    *layout = file;
    *loaded_dir = Some(dir);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn persist_layout_on_change(
    mut contexts: EguiContexts,
    panels: Res<PanelState>,
    cfg: Res<UiShellConfig>,
    mut layout: ResMut<LayoutFile>,
    project: Res<WorldProjection>,
    view: Res<ViewState>,
    current_loc: Res<CurrentLocale>,
    dock: Res<crate::ui::map_controls::MapDockState>,
) {
    let Some(path) = crate::ui_layout::layout_path(project.project_dir.as_deref()) else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut file = (*layout).clone();
    file.theme = Some(cfg.theme.key().into());
    file.density = Some(cfg.density.key().into());
    // 主题包 id 优先；未设时回落到内置包的 key（存量格式，零迁移）。
    file.map_theme = Some(
        view.theme_pack_id
            .clone()
            .unwrap_or_else(|| view.map_palette.key().to_string()),
    );
    file.language = Some(current_loc.0.as_str().to_string());
    file.map_dock = Some(dock.orientation.key().to_string());
    file.map_snap = Some(dock.snapping);
    file.map_dock_pos = dock.pos.map(|p| [p.x, p.y]);
    file.map_zoom_strip_pos = dock.zoom_strip_pos.map(|p| [p.x, p.y]);

    let mut changed = false;
    // 语言切换本身就让 layout 跟着变；与面板几何检测并入同一个 `changed` 累加器。
    if file.language != layout.language
        || file.theme != layout.theme
        || file.density != layout.density
        || file.map_theme != layout.map_theme
        || file.map_dock != layout.map_dock
        || file.map_snap != layout.map_snap
        || file.map_dock_pos != layout.map_dock_pos
        || file.map_zoom_strip_pos != layout.map_zoom_strip_pos
    {
        changed = true;
    }
    for p in Panel::ALL {
        let id = bevy_egui::egui::Id::new(("wv_panel", p));
        let rect = ctx
            .memory(|m| m.area_rect(id))
            .map(|r| [r.min.x, r.min.y, r.width(), r.height()]);
        // egui 的窗口折叠态与 `Window` 同 Id，另挂一个 "collapsing" 子 Id。
        let collapsed =
            bevy_egui::egui::collapsing_header::CollapsingState::load(ctx, id.with("collapsing"))
                .map(|s| !s.is_open())
                .unwrap_or(false);
        let before = file.get(p);
        crate::ui_layout::record_layout(&mut file, p, rect, collapsed, panels.is_visible(p));
        if file.get(p) != before {
            changed = true;
        }
    }
    if !changed {
        return;
    }
    // 拖动中不落盘：松手后的帧自然还是 changed，届时再写（只写一次）。
    if ctx.egui_is_using_pointer() {
        return;
    }
    file.save(&path);
    *layout = file;
}
