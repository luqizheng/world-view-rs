//! 命令面板（`Ctrl+K`）与命令注册表（设计规格 §6.2/§6.3）。
#![cfg(feature = "app")]

//! ## 职责
//!
//! - **零隐藏功能**（D5）：每个功能要么常驻，要么在命令面板里能被搜到。
//!   [`CommandRegistry::all`] 收录**全部**功能，不是只收常用项（§6.2）。
//! - **快捷键的可发现性文档**（§6.3）：每条命令显示自己的快捷键。
//! - **模糊匹配**：按命令文本 + 分组标题匹配，中文子串即命中。
//!
//! ## 为什么用「命令 id」而不是闭包
//!
//! 命令面板要能被搜到、被列出、被测试；闭包无法枚举。命令以
//! [`CommandId`] 注册，执行时由 [`crate::ui`] 的分发器 switch 到具体动作
//! ——注册表是**数据**，执行是**行为**，分开才可测（§9.3 验证项 8）。

use bevy::prelude::*;

use crate::state::{Panel, RecentProject};

/// 命令分组（§6.3，v0.2 §C 新增「项目」「导航」两组）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup {
    /// 工程级：新建 / 打开 / 另存为 / 关闭 / 设置 / 最近工程（v0.2 §A）。
    Project,
    /// 视图模式、缩放、沉浸。
    View,
    /// 撤销 / 重做 / 保存。
    Edit,
    /// 面板显隐。
    Panel,
    /// 时间轴播放与跳转。
    Time,
    /// 明暗、密度。
    Interface,
    /// 对象选择、查找。
    Object,
    /// 地图层级钻取（v0.2 §B）。
    Navigation,
}

impl CommandGroup {
    /// 分组标题（ADR-0018：locale-dependent 文案查表）。
    pub fn title(self, loc: crate::i18n::Locale) -> &'static str {
        use crate::i18n::{t, I18nKey};
        match self {
            CommandGroup::Project => t(loc, I18nKey::CommandGroupProject),
            CommandGroup::View => t(loc, I18nKey::CommandGroupView),
            CommandGroup::Edit => t(loc, I18nKey::CommandGroupEdit),
            CommandGroup::Panel => t(loc, I18nKey::CommandGroupPanel),
            CommandGroup::Time => t(loc, I18nKey::CommandGroupTime),
            CommandGroup::Interface => t(loc, I18nKey::CommandGroupInterface),
            CommandGroup::Object => t(loc, I18nKey::CommandGroupObject),
            CommandGroup::Navigation => t(loc, I18nKey::CommandGroupNavigation),
        }
    }

    /// 全部分组（面板按此顺序分节）。
    ///
    /// 「项目」放最前：它是**最外层的操作域**（决定后面所有操作作用在哪个工程上），
    /// 放在末尾会让人以为它是收尾动作。「导航」放最后：它是当前视图内的移动。
    pub const ALL: [CommandGroup; 8] = [
        CommandGroup::Project,
        CommandGroup::View,
        CommandGroup::Edit,
        CommandGroup::Panel,
        CommandGroup::Time,
        CommandGroup::Interface,
        CommandGroup::Object,
        CommandGroup::Navigation,
    ];
}

/// 命令标识。执行分发以它为键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    /// 切换视图模式（0..4 对应 [`crate::state::ViewMode`]）。
    SetViewMode(u8),
    /// 命令面板本身。
    OpenCommandPalette,
    /// 保存工程。
    Save,
    /// 撤销。
    Undo,
    /// 重做。
    Redo,
    /// 切换明暗主题。
    ToggleTheme,
    /// 切换密度档。
    ToggleDensity,
    /// 切换地图主题（ADR-0016：日间古舆图 ⇄ 暮色旧纸）。
    ToggleMapPalette,
    /// 播放 / 暂停。
    TogglePlay,
    /// 沉浸模式。
    ToggleImmersive,
    /// 全览地图（视野归位）。
    FitMap,
    /// 缩放（+1 放大 / -1 缩小）。
    Zoom(i8),
    /// 跳到时间轴首 / 尾。
    JumpToStart,
    /// 跳到时间轴尾。
    JumpToEnd,
    /// 笔刷半径增减（像素步长）。
    BrushRadius(i8),
    /// 显隐某个面板。
    TogglePanel(Panel),
    /// 重置所有面板位置。
    ResetPanelLayout,
    /// 打开最近工程列表里的第 n 条（索引对应 `RecentProjects::visible`）。
    OpenRecentProject(u8),
    /// 新建工程…（`Ctrl+Shift+N`）：弹「名称 + 位置 + 路径预览」对话框，
    /// 确认后在 `位置\名称` 落盘（含首张世界地图）并打开（2026-10-14 改版）。
    NewProject,
    /// 新建地图…（弹出参数对话框：名称 / 层级 / 幅面 / 海陆预填充）。
    NewMap,
    /// 打开工程…（`Ctrl+O`）。
    OpenProject,
    /// 另存为…（`Ctrl+Shift+S`）。
    SaveProjectAs,
    /// 关闭工程（`Ctrl+W`）。
    CloseProject,
    /// 导出（`Ctrl+E`）。§G 决议：v0.2 只加这一条工程级命令。
    ExportProject,
    /// 项目设置。
    ProjectSettings,
    /// 钻回上一级（`Backspace`）。
    DrillUp,
    /// 钻回世界地图（面包屑第一级）。
    DrillToRoot,
    /// 钻到面包屑的第 n 段（0 = 世界）。
    DrillToSegment(u8),
    /// 强制重建地形纹理（2026-10-14 用户需求：怀疑数据已改但画面没跟随时可用）。
    /// 把 `TerrainTexture.last_mode/last_map/soften` 一起重置为「未知」，
    /// 下一帧 `rebuild_texture_on_mode_change` 必然走全图重建路径。
    ForceRebuildTexture,
}

/// 一条命令：注册表的条目。
#[derive(Debug, Clone)]
pub struct Command {
    /// 命令标识。
    pub id: CommandId,
    /// 分组。
    pub group: CommandGroup,
    /// 显示名（可搜索文本）。
    ///
    /// 是 `String` 而不是 `&'static str`：最近工程的显示名来自工程自身
    /// （`world.yaml` 的 `meta.name`），运行时才知道。
    pub label: String,
    /// 快捷键（显示用，无则 None）。
    pub keys: Option<&'static str>,
}

impl Command {
    /// 是否命中查询（子串匹配，忽略大小写）。
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        self.label.to_lowercase().contains(&q)
            || self
                .group
                .title(crate::i18n::Locale::default())
                .contains(&q)
            || self.keys.unwrap_or("").to_lowercase().contains(&q)
    }
}

/// 命令注册表：全部功能的唯一清单。
pub struct CommandRegistry;

impl CommandRegistry {
    /// 全部命令，含动态的「最近工程」条目（§6.2：命令面板收录**全部**功能）。
    ///
    /// `recent` 由调用方传入而不是从全局读：注册表保持**纯函数**，
    /// 才能被单元测试直接戳（§9.3 验证项 8 靠它把关）。
    pub fn all_with(recent: &[RecentProject]) -> Vec<Command> {
        let mut v = Self::all();
        // 项目：最近工程（插在本组固定条目之前，按最近使用排序）。
        let mut recent_cmds: Vec<Command> = recent
            .iter()
            .take(8)
            .enumerate()
            .map(|(i, p)| Command {
                id: CommandId::OpenRecentProject(i as u8),
                group: CommandGroup::Project,
                label: format!("打开最近工程 · {}", p.name),
                keys: None,
            })
            .collect();
        let project_at = v
            .iter()
            .position(|c| c.group == CommandGroup::Project)
            .unwrap_or(v.len());
        recent_cmds.append(&mut v.split_off(project_at));
        v.extend(recent_cmds);
        v
    }

    /// 全部**静态**命令（不含运行时才知道名字的最近工程）。
    pub fn all() -> Vec<Command> {
        let mut v: Vec<Command> = Vec::new();
        let mut push =
            |id: CommandId, group: CommandGroup, label: &str, keys: Option<&'static str>| {
                v.push(Command {
                    id,
                    group,
                    label: label.to_owned(),
                    keys,
                })
            };

        // 项目（v0.2 §A）
        push(
            CommandId::NewProject,
            CommandGroup::Project,
            "新建工程…",
            Some("Ctrl+Shift+N"),
        );
        push(CommandId::NewMap, CommandGroup::Project, "新建地图…", None);
        push(
            CommandId::OpenProject,
            CommandGroup::Project,
            "打开工程…",
            Some("Ctrl+O"),
        );
        push(
            CommandId::SaveProjectAs,
            CommandGroup::Project,
            "另存为…",
            Some("Ctrl+Shift+S"),
        );
        push(
            CommandId::CloseProject,
            CommandGroup::Project,
            "关闭工程",
            Some("Ctrl+W"),
        );
        push(
            CommandId::ExportProject,
            CommandGroup::Project,
            "导出…",
            Some("Ctrl+E"),
        );
        push(
            CommandId::ProjectSettings,
            CommandGroup::Project,
            "项目设置",
            None,
        );

        // 视图（2026-10-14 起只列 BROWSING 两种，与顶栏分段控件同源）
        for (i, mode) in crate::state::ViewMode::BROWSING.iter().enumerate() {
            push(
                CommandId::SetViewMode(i as u8),
                CommandGroup::View,
                match mode {
                    crate::state::ViewMode::VectorFlat => "切换到矢量地形视图",
                    crate::state::ViewMode::Schematic => "切换到色块地形视图",
                    // BROWSING 之外的模式不再注册（枚举变体仍在，见 state.rs）。
                    _ => continue,
                },
                Some(["1", "2"][i]),
            );
        }
        push(
            CommandId::FitMap,
            CommandGroup::View,
            "全览地图（视野归位）",
            Some("F"),
        );
        push(
            CommandId::Zoom(1),
            CommandGroup::View,
            "放大地图",
            Some("+ / 滚轮"),
        );
        push(
            CommandId::Zoom(-1),
            CommandGroup::View,
            "缩小地图",
            Some("−"),
        );
        push(
            CommandId::ToggleImmersive,
            CommandGroup::View,
            "沉浸模式（只留地图与时间轴）",
            Some("Ctrl+Shift+I"),
        );

        // 编辑
        push(
            CommandId::OpenCommandPalette,
            CommandGroup::Edit,
            "打开命令面板",
            Some("Ctrl+K"),
        );
        push(
            CommandId::Save,
            CommandGroup::Edit,
            "保存工程",
            Some("Ctrl+S"),
        );
        push(CommandId::Undo, CommandGroup::Edit, "撤销", Some("Ctrl+Z"));
        push(
            CommandId::Redo,
            CommandGroup::Edit,
            "重做",
            Some("Ctrl+Shift+Z"),
        );

        // 面板
        for p in Panel::ALL {
            push(
                CommandId::TogglePanel(p),
                CommandGroup::Panel,
                match p {
                    Panel::MapTree => "显隐世界树面板",
                    Panel::Layers => "显隐图层面板",
                    Panel::Brush => "显隐地形笔刷面板",
                    Panel::Minimap => "显隐小地图面板",
                    Panel::Compass => "显隐指南针面板",
                    Panel::Timeline => "显隐时间轴",
                    Panel::Event => "显隐事件登记面板",
                    Panel::Situation => "显隐事态登记面板",
                    Panel::Character => "显隐人物登记面板",
                    Panel::Faction => "显隐势力层面板",
                    Panel::Region => "显隐区域面板",
                    Panel::Inspector => "显隐检视器面板",
                    Panel::Validation => "显隐校验面板",
                },
                None,
            );
        }
        push(
            CommandId::ResetPanelLayout,
            CommandGroup::Panel,
            "重置所有面板位置",
            None,
        );

        // 时间
        push(
            CommandId::TogglePlay,
            CommandGroup::Time,
            "播放 / 暂停时间轴",
            Some("Space"),
        );
        push(
            CommandId::JumpToStart,
            CommandGroup::Time,
            "跳到时间轴起点",
            Some("Home"),
        );
        push(
            CommandId::JumpToEnd,
            CommandGroup::Time,
            "跳到时间轴终点",
            Some("End"),
        );

        // 界面
        push(
            CommandId::ToggleTheme,
            CommandGroup::Interface,
            "切换明暗主题",
            Some("Ctrl+J"),
        );
        push(
            CommandId::ToggleDensity,
            CommandGroup::Interface,
            "切换紧凑 / 标准密度",
            None,
        );
        push(
            CommandId::ToggleMapPalette,
            CommandGroup::View,
            "切换地图主题（日间古舆图 / 暮色旧纸）",
            None,
        );
        // 2026-10-14 用户需求：怀疑数据已改、画面没跟随时手动重画底图。
        push(
            CommandId::ForceRebuildTexture,
            CommandGroup::Interface,
            "强制重画地形（怀疑没跟上时点一下）",
            Some("F5"),
        );

        // 对象
        push(
            CommandId::BrushRadius(1),
            CommandGroup::Object,
            "增大笔刷半径",
            Some("]"),
        );
        push(
            CommandId::BrushRadius(-1),
            CommandGroup::Object,
            "减小笔刷半径",
            Some("["),
        );

        // 导航（v0.2 §B）
        push(
            CommandId::DrillToRoot,
            CommandGroup::Navigation,
            "钻回世界地图",
            None,
        );
        push(
            CommandId::DrillUp,
            CommandGroup::Navigation,
            "钻回上一级",
            Some("Backspace"),
        );

        v
    }

    /// 按查询过滤（保持注册顺序）。
    pub fn search(query: &str, recent: &[RecentProject]) -> Vec<Command> {
        Self::all_with(recent)
            .into_iter()
            .filter(|c| c.matches(query))
            .collect()
    }
}

/// 命令面板的瞬时 UI 状态。
#[derive(Resource, Debug, Default)]
pub struct CommandPalette {
    /// 是否打开。
    pub open: bool,
    /// 查询串。
    pub query: String,
    /// 高亮项索引。
    pub selected: usize,
    /// 待执行命令（由 UI 系统消费）。
    pub pending: Option<CommandId>,
}

impl CommandPalette {
    /// 打开并聚焦。
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    /// 关闭。
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }

    /// 当前查询的命中项。
    pub fn results(&self, recent: &[RecentProject]) -> Vec<Command> {
        CommandRegistry::search(&self.query, recent)
    }

    /// 执行高亮项。
    pub fn run_selected(&mut self, recent: &[RecentProject]) {
        let results = self.results(recent);
        if let Some(cmd) = results.get(self.selected) {
            self.pending = Some(cmd.id);
        }
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_every_panel_and_view_mode() {
        let all = CommandRegistry::all();
        for p in Panel::ALL {
            assert!(
                all.iter().any(|c| c.id == CommandId::TogglePanel(p)),
                "面板 {} 未注册命令",
                p.title(crate::i18n::Locale::default())
            );
        }
        for i in 0..crate::state::ViewMode::BROWSING.len() as u8 {
            assert!(
                all.iter().any(|c| c.id == CommandId::SetViewMode(i)),
                "浏览视图模式 {i} 未注册命令"
            );
        }
        // 2026-10-14 收敛：BROWSING 之外的模式不占命令面板（两种 → 只有 0/1）。
        assert!(!all.iter().any(|c| c.id == CommandId::SetViewMode(2)));
    }

    #[test]
    fn search_finds_command_by_chinese_label_substring() {
        let hits = CommandRegistry::search("笔刷", &[]);
        assert!(hits.iter().any(|c| c.id == CommandId::BrushRadius(1)));
        assert!(hits
            .iter()
            .any(|c| c.id == CommandId::TogglePanel(Panel::Brush)));
    }

    #[test]
    fn search_finds_command_by_hotkey_text() {
        let hits = CommandRegistry::search("ctrl+k", &[]);
        assert!(
            hits.iter().any(|c| c.id == CommandId::OpenCommandPalette),
            "应能按快捷键文本搜到命令面板"
        );
    }

    #[test]
    fn every_command_has_a_label() {
        for c in CommandRegistry::all_with(&sample_recent()) {
            assert!(!c.label.is_empty(), "{:?} 缺少显示名", c.id);
        }
    }

    fn sample_recent() -> Vec<RecentProject> {
        vec![
            RecentProject {
                dir: "/w/a".into(),
                name: "四渡赤水 · 行军图".into(),
            },
            RecentProject {
                dir: "/w/b".into(),
                name: "封神演义 · 架空".into(),
            },
        ]
    }

    #[test]
    fn every_group_is_populated() {
        // 「零隐藏功能」D5 的另一面：分组不能在面板里显示成空标题。
        let all = CommandRegistry::all_with(&sample_recent());
        for g in CommandGroup::ALL {
            assert!(
                all.iter().any(|c| c.group == g),
                "分组「{}」没有任何命令",
                g.title(crate::i18n::Locale::default())
            );
        }
    }

    #[test]
    fn project_group_covers_the_v0_2_menu_entries() {
        let ids: Vec<CommandId> = CommandRegistry::all().iter().map(|c| c.id).collect();
        for want in [
            CommandId::NewProject,
            CommandId::OpenProject,
            CommandId::SaveProjectAs,
            CommandId::CloseProject,
            CommandId::ExportProject,
            CommandId::ProjectSettings,
        ] {
            assert!(ids.contains(&want), "{want:?} 未注册");
        }
        // 「保存」与「撤销」**不进**项目菜单（v0.2 §A：它们已有独立按钮）。
        let project_labels: Vec<String> = CommandRegistry::all()
            .into_iter()
            .filter(|c| c.group == CommandGroup::Project)
            .map(|c| c.label)
            .collect();
        assert!(
            !project_labels.iter().any(|l| l.contains("保存工程")),
            "保存工程不应出现在项目分组"
        );
    }

    #[test]
    fn navigation_group_covers_drill_commands() {
        let ids: Vec<CommandId> = CommandRegistry::all().iter().map(|c| c.id).collect();
        assert!(ids.contains(&CommandId::DrillUp));
        assert!(ids.contains(&CommandId::DrillToRoot));
    }

    #[test]
    fn recent_projects_become_searchable_commands_in_input_order() {
        let all = CommandRegistry::all_with(&sample_recent());
        let recents: Vec<&Command> = all
            .iter()
            .filter(|c| matches!(c.id, CommandId::OpenRecentProject(_)))
            .collect();
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[0].id, CommandId::OpenRecentProject(0));
        assert!(
            recents[0].label.contains("四渡赤水"),
            "got {}",
            recents[0].label
        );
        assert_eq!(recents[1].id, CommandId::OpenRecentProject(1));
        // 分组归属正确（否则面板会把它分错节）。
        assert!(recents.iter().all(|c| c.group == CommandGroup::Project));
        // 最近工程应排在项目组固定条目之前（列表顺序 = 视觉顺序）。
        let first_project = all
            .iter()
            .position(|c| c.group == CommandGroup::Project)
            .expect("项目组非空");
        assert!(matches!(
            all[first_project].id,
            CommandId::OpenRecentProject(_)
        ));
    }

    #[test]
    fn search_matches_a_recent_project_by_name() {
        let hits = CommandRegistry::search("封神", &sample_recent());
        assert_eq!(hits.len(), 1, "只应命中那一条最近工程");
        assert_eq!(hits[0].id, CommandId::OpenRecentProject(1));
    }

    #[test]
    fn command_count_stays_above_the_v0_1_baseline() {
        // v0.1 是 29 条，v0.2 加项目 6 条 + 导航 2 条 = 37 条（不含动态最近工程）。
        let n = CommandRegistry::all().len();
        assert!(n >= 37, "命令数 {n} 少于 v0.2 基线 37——有功能被漏注册");
    }

    #[test]
    fn run_selected_uses_the_same_result_list_that_was_displayed() {
        let mut p = CommandPalette::default();
        p.open();
        p.query = "钻回世界".into();
        let results = p.results(&sample_recent());
        assert!(!results.is_empty());
        p.selected = 0;
        p.run_selected(&sample_recent());
        assert_eq!(p.pending, Some(CommandId::DrillToRoot));
        assert!(!p.open, "执行后应关闭面板");
    }
}
