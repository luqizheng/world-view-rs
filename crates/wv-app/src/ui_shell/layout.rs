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
/// - **面板显隐 + 折叠态**（UX-008 / round-2 审计：此前 `visible` 是
///   write-only 死数据、折叠 id `"head"` vs `"collapsing"` 不一致——重启后
///   显隐回默认三项、折叠全开。现在两者都真实回灌）
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_layout_on_project_open(
    mut contexts: EguiContexts,
    project: Res<WorldProjection>,
    mut layout: ResMut<LayoutFile>,
    mut view: ResMut<ViewState>,
    mut shell_cfg: ResMut<UiShellConfig>,
    mut current_loc: ResMut<CurrentLocale>,
    mut dock: ResMut<crate::ui::map_controls::MapDockState>,
    // UX-008：面板显隐回灌的落点（此前只写不读，重启回默认三项）。
    mut panels: ResMut<PanelState>,
    // UX-016：笔刷参数回灌落点。
    mut brush: ResMut<wv_render::state::BrushState>,
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
    // 播放条折叠态回灌（2026-10-27 停靠化）：`collapsed` 的跨会话真相在
    // ui-layout.yaml，egui 侧的会话缓存（`timeline_collapse_id`）在工程打开
    // 的这一刻强制对齐——若不回灌，时间轴首帧会以「缓存缺失→落盘值（还没
    // 装进资源）→默认展开」抢跑，且随后 persist 会把缓存里的 false 写回盘，
    // 折叠记忆就丢了（实测回归）。
    if let Ok(ctx) = contexts.ctx_mut() {
        let collapsed = file.get(Panel::Timeline).collapsed;
        ctx.data_mut(|d| {
            d.insert_persisted(crate::ui::timeline::timeline_collapse_id(), collapsed)
        });
        // UX-008：其余面板显隐 + 折叠态回灌。折叠真相在 `id.with("head")`
        // （`wv_ui::FloatWindow` 自绘头的 CollapsingState 键——persist 曾读
        // `"collapsing"` 位，两处 Id 不一致导致折叠永不恢复）。Timeline 例外：
        // 上面已按播放条专用位回灌。
        for p in Panel::ALL {
            let pl = file.get(p);
            panels.set_visible(p, pl.visible);
            if p != Panel::Timeline {
                use bevy_egui::egui::collapsing_header::CollapsingState;
                let state = CollapsingState::load_with_default_open(
                    ctx,
                    bevy_egui::egui::Id::new(("wv_panel", p)).with("head"),
                    !pl.collapsed,
                );
                state.store(ctx);
            }
        }
    }
    if let Some(theme) = file.theme.as_deref().and_then(UiTheme::from_key) {
        shell_cfg.theme = theme;
    }
    // 地图主题（ADR-0066）：单风格时代**只有一种**风格，存量 `map_theme` 值
    // （"daylight" / "dusk" / 外部包 id）一律映射回默认档——零迁移，且旧工程
    // 打开后观感统一到专业制图。
    if let Some(t) = file.map_theme.as_deref() {
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
    // UX-016：笔刷参数回灌（缺字段保留当前值 = BrushState::default）。
    if let Some(prefs) = file.brush {
        use wv_render::state::BrushTool;
        brush.radius = prefs.radius.max(1.0);
        brush.softness = prefs.softness.clamp(0.0, 1.0);
        brush.strength = prefs.strength.clamp(0.2, 2.0);
        brush.kind = prefs.kind;
        brush.tool = BrushTool::ALL.get(prefs.tool).copied().unwrap_or_default();
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
    // UX-016：笔刷参数落盘来源。
    brush: Res<wv_render::state::BrushState>,
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
    // 风格恒为默认档，持久化键写它的 key（存量格式，零迁移）。
    file.map_theme = Some(view.map_palette.key().to_string());
    file.language = Some(current_loc.0.as_str().to_string());
    file.map_dock = Some(dock.orientation.key().to_string());
    file.map_snap = Some(dock.snapping);
    file.map_dock_pos = dock.pos.map(|p| [p.x, p.y]);
    file.map_zoom_strip_pos = dock.zoom_strip_pos.map(|p| [p.x, p.y]);
    // UX-016：笔刷参数（enabled 不持久化——打开工程默认收笔）。
    file.brush = Some(crate::ui_layout::BrushPrefs {
        radius: brush.radius,
        softness: brush.softness,
        strength: brush.strength,
        kind: brush.kind,
        tool: wv_render::state::BrushTool::ALL
            .iter()
            .position(|t| *t == brush.tool)
            .unwrap_or(0),
    });

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
        || file.brush != layout.brush
    {
        changed = true;
    }
    for p in Panel::ALL {
        let id = bevy_egui::egui::Id::new(("wv_panel", p));
        let rect = ctx
            .memory(|m| m.area_rect(id))
            .map(|r| [r.min.x, r.min.y, r.width(), r.height()]);
        let before = file.get(p);
        // UX-008 修正：折叠真相在 `id.with("head")`（`wv_ui::FloatWindow`
        // 自绘头的 CollapsingState 键）——旧代码读 `"collapsing"` 子位，
        // 该位恒缺失 → 折叠态持久化恒 false、恢复恒展开。
        // 例外（2026-10-27 播放器停靠化）：Timeline 的折叠是「播放条」开关，
        // 会话缓存在独立位（`timeline_collapse_id`）。
        let collapsed = match p {
            Panel::Timeline => {
                crate::ui::timeline::timeline_collapsed(ctx).unwrap_or(before.collapsed)
            }
            _ => bevy_egui::egui::collapsing_header::CollapsingState::load(ctx, id.with("head"))
                .map(|s| !s.is_open())
                .unwrap_or(false),
        };
        // UX-008 修正：工作区内可见性**冻结在落盘值**——此前守卫只豁免隐藏
        // 面板，工作区内仍可见的笔刷/地标窗会把 `visible: true` 泄漏进盘，
        // 与守卫注释「临时态不得记盘」的意图相悖。几何照常记录（隐藏面板
        // 无 area_rect，rect=None 几何自然不变）。
        let visible = if panels.cell_workspace_backup.is_some() {
            before.visible
        } else {
            panels.is_visible(p)
        };
        crate::ui_layout::record_layout(&mut file, p, rect, collapsed, visible);
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
