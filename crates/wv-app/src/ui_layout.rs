//! 面板布局持久化（设计规格 §9.1 `ui_layout.rs`、§10 Q5）。
#![cfg(feature = "app")]

//! ## 职责
//!
//! 把每个面板的**位置 / 尺寸 / 折叠 / 可见性**落盘到工程目录下的
//! `ui-layout.yaml`，让布局成为长期手感（§10 Q5：落盘）。
//!
//! ## 纪律
//!
//! - **只写 UI 布局，不写世界数据**：`ui-layout.yaml` 不是 world.yaml 的一部分，
//!   也不参与 `wv-core` 的序列化（P2「目录即工程」：布局随工程走，不进注册表）。
//! - **纳入版本控制**（2026-10-12 用户裁定）：`ui-layout.yaml` **不进 `.gitignore`**——
//!   布局是作者调好的手感，作者可以 `git add` 让它随工程一起提交；不想提交就不提交，
//!   文件缺失时静默回退默认布局，两种选择都不破坏任何东西。
//! - **可回退**：文件损坏 / 缺失时静默用 [`default_layout`]
//!   （§4 面板默认位置互不重叠），不阻塞启动（V1：校验只提示不阻止）。
//! - **手写友好**：YAML 字段名就是面板 id，作者可直接改。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::state::{Panel, PanelState};

/// 单个面板的布局记忆。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelLayout {
    /// 左上角 x（逻辑像素）。
    pub x: f32,
    /// 左上角 y（逻辑像素）。
    pub y: f32,
    /// 宽。
    pub width: f32,
    /// 高。
    pub height: f32,
    /// 是否折叠（仅剩标题栏）。
    #[serde(default)]
    pub collapsed: bool,
    /// 是否可见。
    #[serde(default = "default_true")]
    pub visible: bool,
}

fn default_true() -> bool {
    true
}

/// 整份布局（`ui-layout.yaml` 的根）。
///
/// 作为 Bevy [`Resource`] 常驻内存：启动时读盘一次，运行中由
/// [`crate::ui_shell::persist_layout_on_change`] 按 egui 记忆的**真实**窗口矩形回写。
///
/// ## 持久化字段说明（AGENTS §7.5 / P2 兼容）
/// `panels` / `theme` / `density` / `map_theme` / `language` 都**只**写 UI 偏好，
/// 不写世界数据——与本文件顶上的「纪律」一致。新增字段一律 `Option<String>` +
/// `#[serde(default)]`（缺字段 / 字段无法解析静默忽略，V1 守约）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Resource)]
pub struct LayoutFile {
    /// 各面板布局，键为 [`Panel::key`]。
    #[serde(default)]
    pub panels: BTreeMap<String, PanelLayout>,
    /// 明暗主题（持久化，Q5 的一部分）。
    #[serde(default)]
    pub theme: Option<String>,
    /// 密度档。
    #[serde(default)]
    pub density: Option<String>,
    /// 地图主题（ADR-0016：`daylight` / `dusk`，`MapPalette::key`）。
    #[serde(default)]
    pub map_theme: Option<String>,
    /// UI 语言（ADR-0018：`zh` / `en`）。
    ///
    /// 缺省 `None` = 使用默认 `Locale::Zh`；
    /// 写入不可识别字符串（`ParseError` 等情况）由 `i18n::parse` 返回 `None`
    /// 静默忽略，**不**阻塞启动（V1 守约）。
    #[serde(default)]
    pub language: Option<String>,
}

/// 布局文件路径：工程目录下的 `ui-layout.yaml`（P2：目录即工程）。
pub fn layout_path(project_dir: Option<&str>) -> Option<PathBuf> {
    project_dir.map(|d| Path::new(d).join("ui-layout.yaml"))
}

impl LayoutFile {
    /// 读取布局。文件缺失/损坏时返回默认值，**不报错**（V1：只提示不阻止）。
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_yaml::from_str(&text).unwrap_or_default()
    }

    /// 写回布局。失败只打印警告，不影响编辑（V1）。
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_yaml::to_string(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    eprintln!("[wv] 布局写入失败 `{}`: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[wv] 布局序列化失败: {e}"),
        }
    }

    /// 取某面板布局，缺失时给默认值。
    pub fn get(&self, panel: Panel) -> PanelLayout {
        self.panels
            .get(panel.key())
            .cloned()
            .unwrap_or_else(|| default_layout(panel))
    }

    /// 写入某面板布局。
    pub fn set(&mut self, panel: Panel, layout: PanelLayout) {
        self.panels.insert(panel.key().to_string(), layout);
    }
}

/// 面板默认布局（§3.3：默认位置互不重叠）。
pub fn default_layout(panel: Panel) -> PanelLayout {
    let [width, height] = panel.default_size();
    let [x, y] = panel.default_pos();
    PanelLayout {
        x,
        y,
        width,
        height,
        collapsed: false,
        visible: panel.default_visible(),
    }
}

/// 取某面板**这一帧**该用的布局。
///
/// 为什么两个来源都要传进来：
/// - `layouts` 是**跨会话**的记忆（磁盘上的 `ui-layout.yaml`）；
/// - `panels` 是**本会话**的记忆（用户刚改了显隐）。
///
/// 单一事实源仍是 `ui-layout.yaml`；本函数只做「落盘值 + 当前显隐」的合并。
/// 「重置所有面板位置」不在这里生效——它必须调用
/// `egui::Memory::reset_areas()` 抹掉 egui 自己的窗口记忆，见 [`crate::ui_shell`]。
pub fn current_layout(layouts: &LayoutFile, panels: &PanelState, panel: Panel) -> PanelLayout {
    let mut l = layouts.get(panel);
    l.visible = panels.is_visible(panel);
    l
}

/// 把某面板的**真实**几何记录回布局表（供落盘）。
///
/// `rect` 来自 `egui::Memory::area_rect`——那才是用户拖动后的真相，
/// 而不是 [`default_layout`]。`rect` 为 `None`（窗口本帧未出现）时保留旧值。
pub fn record_layout(
    layouts: &mut LayoutFile,
    panel: Panel,
    rect: Option<[f32; 4]>,
    collapsed: bool,
    visible: bool,
) {
    let mut l = layouts.get(panel);
    if let Some([x, y, w, h]) = rect {
        l.x = x;
        l.y = y;
        l.width = w;
        l.height = h;
    }
    l.collapsed = collapsed;
    l.visible = visible;
    layouts.set(panel, l);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_layout_file_yields_defaults_without_panic() {
        let f = LayoutFile::load(Path::new("definitely-not-here/ui-layout.yaml"));
        assert!(f.panels.is_empty());
        assert_eq!(f.get(Panel::Brush), default_layout(Panel::Brush));
    }

    #[test]
    fn layout_round_trips_through_yaml() {
        let mut f = LayoutFile::default();
        let mut l = default_layout(Panel::MapTree);
        l.x = 123.0;
        l.collapsed = true;
        f.set(Panel::MapTree, l.clone());
        let text = serde_yaml::to_string(&f).expect("应能序列化");
        let back: LayoutFile = serde_yaml::from_str(&text).expect("应能反序列化");
        assert_eq!(back.get(Panel::MapTree), l);
    }

    #[test]
    fn default_panel_rects_never_overlap() {
        // §9.3 验证项 1：默认位置两两不重叠（含折叠态）。
        let panels = Panel::ALL;
        for (i, a) in panels.iter().enumerate() {
            let la = default_layout(*a);
            for b in panels.iter().skip(i + 1) {
                let lb = default_layout(*b);
                let overlap = la.x < lb.x + lb.width
                    && lb.x < la.x + la.width
                    && la.y < lb.y + lb.height
                    && lb.y < la.y + la.height;
                assert!(
                    !overlap,
                    "{} 与 {} 默认矩形重叠: ({:.0},{:.0},{:.0},{:.0}) vs ({:.0},{:.0},{:.0},{:.0})",
                    a.title(crate::i18n::Locale::default()),
                    b.title(crate::i18n::Locale::default()),
                    la.x,
                    la.y,
                    la.width,
                    la.height,
                    lb.x,
                    lb.y,
                    lb.width,
                    lb.height
                );
            }
        }
    }

    #[test]
    fn persisted_rect_wins_over_default() {
        // 跨会话恢复：落盘值必须压过默认值，否则拖动结果白丢。
        let mut f = LayoutFile::default();
        let mut l = default_layout(Panel::Brush);
        l.x = 640.0;
        l.y = 320.0;
        f.set(Panel::Brush, l);

        let got = current_layout(&f, &PanelState::default(), Panel::Brush);
        assert_eq!((got.x, got.y), (640.0, 320.0), "落盘位置未被采用");
    }

    #[test]
    fn current_layout_mirrors_session_visibility() {
        // 本会话改了显隐、还没落盘时，合并结果必须立刻反映它。
        let f = LayoutFile::default();
        let mut panels = PanelState::default();
        panels.set_visible(Panel::Inspector, true);
        assert!(
            current_layout(&f, &panels, Panel::Inspector).visible,
            "会话内显隐未生效"
        );
        panels.set_visible(Panel::Inspector, false);
        assert!(!current_layout(&f, &panels, Panel::Inspector).visible);
    }

    #[test]
    fn record_layout_keeps_previous_rect_when_area_absent() {
        // 面板本帧没画（被隐藏 / 折叠到底）时，不该把已知位置抹成 0。
        let mut f = LayoutFile::default();
        record_layout(
            &mut f,
            Panel::Event,
            Some([100.0, 200.0, 300.0, 400.0]),
            false,
            true,
        );
        record_layout(&mut f, Panel::Event, None, true, false);

        let got = f.get(Panel::Event);
        assert_eq!(
            (got.x, got.y, got.width, got.height),
            (100.0, 200.0, 300.0, 400.0)
        );
        assert!(got.collapsed, "折叠态未被记录");
        assert!(!got.visible, "显隐未被记录");
    }

    // ── language 持久化（ADR-0018 §2.2） ────────────────────────────

    #[test]
    fn language_round_trips_through_yaml() {
        let f = LayoutFile {
            language: Some("en".into()),
            ..Default::default()
        };
        let text = serde_yaml::to_string(&f).expect("序列化应成功");
        assert!(
            text.contains("language: en"),
            "yaml 应有 language 字段: {text}"
        );
        let back: LayoutFile = serde_yaml::from_str(&text).expect("反序列化应成功");
        assert_eq!(back.language, Some("en".into()));
    }

    #[test]
    fn missing_language_field_loads_as_none_without_error() {
        let yaml = "panels: {}\n"; // 完全无 language 字段（与现存 examples/xinhai-1911/ui-layout.yaml 同口径）
        let f: LayoutFile = serde_yaml::from_str(yaml).expect("缺字段应被 serde(default) 兜底");
        assert_eq!(f.language, None);
    }

    #[test]
    fn garbage_language_value_loads_as_raw_string_without_panic() {
        // `Locale::parse` 处理在 ui_shell 层；本文件只保证**不做**字段级解析。
        let yaml = "language: \"xx-XX-not-real\"\n";
        let f: LayoutFile = serde_yaml::from_str(yaml).expect("字段保留为字符串");
        assert_eq!(f.language.as_deref(), Some("xx-XX-not-real"));
    }

    #[test]
    fn newer_yaml_with_extra_unknown_fields_loads_into_older_struct() {
        // 兼容旧的 4 字段布局 yaml 加载到新版 LayoutFile（带 language）—— 不报错。
        let yaml = "panels: {}\ntheme: dark\ndensity: cozy\n";
        let f: LayoutFile = serde_yaml::from_str(yaml).expect("旧 yaml 加载应成功");
        assert_eq!(f.theme.as_deref(), Some("dark"));
        assert_eq!(f.density.as_deref(), Some("cozy"));
        assert_eq!(f.language, None);
    }
}
