//! 分裂按钮（split button）——「主区 + 右下角小三角」的下拉工具（ADR-0043）。
//!
//! ## 职责
//! - [`SplitButtonMetrics`] —— 瓷砖尺寸、图标大小、小三角命中区边长。
//! - [`SplitMenuItem`] —— 弹层里的一项（图标 + 文案）。
//! - [`split_button`] —— 自绘一枚瓷砖：主区点按 = 立即选用当前项，
//!   右下角小三角点按 = 弹出菜单换项。
//!
//! ## 使用方
//! - `map_controls`（wv-app）—— 地图工具条的「选择组」与「测量组」两枚。
//!
//! ## 为什么是这个形态
//!
//! Figma 等工具把「同一族的一组工具」压成一枚按钮：主区是当前那件的图标
//! （点它 = 继续用），右下角一枚小三角收纳同族其余几件。作者在**多数帧**
//! 只关心「我现在用的是哪件」，所以主区必须最快；偶尔要换件时才付一次弹层。
//!
//! 拆成两枚并排的瓷砖会让工具条在窄屏上翻倍，也让「当前是哪件」这件事
//! 在视觉上散成两个高亮位——分裂按钮保住了这两个性质。
//!
//! ## 两个命中区为什么这样注册
//!
//! 主区**先**注册（`allocate_exact_size`）、小三角**后**注册（`ui.interact`）。
//! egui 的命中解析按注册顺序**后者居上**：若小三角先注册，主区会盖住它，小三角
//! 永远收不到点击；反之主区先注册、小三角后注册，则小三角吃掉自己那一角、
//! 主区接管其余，两块互不串扰。
//!
//! 这不是可以靠视觉推断的细节——写反了编译照过、单测照跑，只有真点击才暴露
//! （本模块的 `clicking_chevron_...` 就是为此存在的）。
//!
//! ## 边界（不做什么）
//!
//! - **不**持有任何领域状态：`active` / `selected` 由 caller 读资源算好传进来，
//!   点击结果只经 `on_main_click` / `on_pick` 两个回调出去。本控件不认识
//!   `SelectionShape` / `MeasureMode` 之类的东西（wv-ui 零兄弟依赖，AGENTS §2.1）。
//! - **不**做工具互斥：开 / 关哪个工具是 caller 分发器的事。
//! - **不**做 i18n：`label` 由 caller 传已本地化的字符串进来。
//!
//! ## 视觉规格
//! - 瓷砖底 / 描边 / 圆角沿用工具条既有规格（`icon_tile` 同款）。
//! - 活动态 `gold_soft` 底 + `gold` 描边与前景；hover 走 `hover_bg`。
//! - 小三角画在右下角命中区的正中，尺寸取命中区的 ~55%。

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::widgets::primitives::{icon_image, menu_item, MenuState};

/// 弹层菜单的固定宽度（px）。
///
/// **必须定宽**：`menu_item` 的行按 `available_width()` 铺满，而 Popup 的
/// 可用宽度默认是「剩余整个屏幕」——不定宽的菜单会横贯屏幕（2026-10-20
/// 实机反馈）。宽度按最长中文标签 + 行尾快捷键 + 图标 + 边距取值。
pub const MENU_WIDTH: f32 = 156.0;

/// 小三角在瓷砖上的方位（2026-10-20 用户裁定，两轮迭代后的终版）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChevronSide {
    /// 右缘垂直居中、三角朝下——横排工具条。**不**贴右下角：角落方位会把
    /// 图标往上挤（2026-10-20 实机反馈），居中方位让图标沿水平让位。
    #[default]
    RightCenter,
    /// 下缘水平居中、三角朝下——纵排工具条（小三角在图标正下方）。
    BottomCenter,
}

/// 分裂按钮的尺寸规格（caller 从自己的 `DockMetrics` 投影而来）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitButtonMetrics {
    /// 整枚瓷砖的外框尺寸。
    pub tile: egui::Vec2,
    /// 主区图标边长（正方形）。
    pub icon: f32,
    /// 小三角**命中区**的边长（正方形）。
    pub chevron_hit: f32,
    /// 瓷砖圆角。
    pub radius: f32,
    /// 小三角方位（横排右缘居中 / 纵排下缘居中）。
    pub chevron_side: ChevronSide,
}

impl SplitButtonMetrics {
    /// 小三角命中区的矩形，方位由 [`Self::chevron_side`] 决定：
    /// - [`ChevronSide::RightCenter`]：贴右缘、垂直居中；
    /// - [`ChevronSide::BottomCenter`]：贴下缘、水平居中。
    ///
    /// 两者都是 `chevron_hit` 见方。单测与 caller 若要预判命中区位置，都该
    /// 走这一个函数，避免几何在两处各写一遍后漂移。
    pub fn chevron_rect(&self, tile_rect: egui::Rect) -> egui::Rect {
        match self.chevron_side {
            ChevronSide::RightCenter => egui::Rect::from_min_max(
                egui::pos2(
                    tile_rect.right() - self.chevron_hit,
                    tile_rect.center().y - self.chevron_hit * 0.5,
                ),
                egui::pos2(
                    tile_rect.right(),
                    tile_rect.center().y + self.chevron_hit * 0.5,
                ),
            ),
            ChevronSide::BottomCenter => egui::Rect::from_min_max(
                egui::pos2(
                    tile_rect.center().x - self.chevron_hit * 0.5,
                    tile_rect.bottom() - self.chevron_hit,
                ),
                egui::pos2(
                    tile_rect.center().x + self.chevron_hit * 0.5,
                    tile_rect.bottom(),
                ),
            ),
        }
    }

    /// 主区图标的中心：小三角占了一侧，图标沿**另一条轴**让位——
    /// 右缘小三角让图标左移（垂直方向不再被挤），下缘小三角让图标上移。
    pub fn icon_center(&self, tile_rect: egui::Rect) -> egui::Pos2 {
        match self.chevron_side {
            // 图标中心 = 左缘 + (tile − hit) / 2，恰好在小三角左侧剩余区居中。
            ChevronSide::RightCenter => egui::pos2(
                tile_rect.left() + (self.tile.x - self.chevron_hit) * 0.5,
                tile_rect.center().y,
            ),
            ChevronSide::BottomCenter => egui::pos2(
                tile_rect.center().x,
                tile_rect.top() + (self.tile.y - self.chevron_hit) * 0.5,
            ),
        }
    }
}

/// 弹层里的一项。
#[derive(Debug, Clone)]
pub struct SplitMenuItem {
    /// 行首图标（`menu_item` 约定 12px）。
    pub icon: egui::ImageSource<'static>,
    /// 已本地化的显示名。
    pub label: String,
    /// 行尾补充提示（快捷键等），可为空串。
    pub hint: String,
}

impl SplitMenuItem {
    /// 便捷构造。
    pub fn new(icon: egui::ImageSource<'static>, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            hint: String::new(),
        }
    }

    /// 链式补行尾提示。
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }
}

/// 一枚分裂按钮。
///
/// **参数：**
/// - `id` —— 稳定 id。弹层的 open 状态存在 egui `Memory` 里按它寻址，**必须**
///   在同一位置每帧给同一个值，否则菜单会闪。
/// - `icon` —— 主区图标，caller 应传**当前项**的图标（换了项主区跟着换）。
/// - `active` —— 该工具组是否正在使用（决定 gold 高亮）。
/// - `main_hint` —— 主区悬停提示。
/// - `items` / `selected` —— 弹层条目与当前选中下标；`selected` 越界按 0 处理
///   （与顶栏 `unwrap_or(0)` 同口径，宁可指向第一项也不 panic）。
/// - `on_main_click` —— 主区被点：`FnOnce`，caller 据此排命令。
/// - `on_pick` —— 弹层选中第 i 项：`FnMut(usize)`，可能一次都不调。
///
/// **返回值**：主区的 [`egui::Response`]，供 caller 挂 `on_hover_text` 等。
#[allow(clippy::too_many_arguments)]
pub fn split_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: &str,
    metrics: SplitButtonMetrics,
    icon: egui::ImageSource<'static>,
    active: bool,
    main_hint: &str,
    items: &[SplitMenuItem],
    selected: usize,
    on_main_click: impl FnOnce(),
    mut on_pick: impl FnMut(usize),
) -> egui::Response {
    // 主区先注册、小三角后注册——egui 命中解析后者居上，顺序反了小三角会被
    // 主区整块盖住（见模块头「两个命中区为什么这样注册」）。
    let (tile_rect, main_resp) = ui.allocate_exact_size(metrics.tile, egui::Sense::click());

    // ── 绘制：底 / 描边 / 图标 / 小三角 ────────────────────────────────
    let chevron_rect = metrics.chevron_rect(tile_rect);
    let base_id = ui.id().with(id);
    let menu_id = base_id.with("menu");

    let chevron_resp = ui.interact(chevron_rect, menu_id.with("chev"), egui::Sense::click());

    let menu_open = egui::Popup::is_id_open(ui.ctx(), menu_id);
    let main_hovered = main_resp.hovered() && !menu_open;
    let chevron_hovered = chevron_resp.hovered();

    let fill = if active {
        shell.gold_soft()
    } else if main_hovered || chevron_hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if active {
        egui::Stroke::new(1.0, shell.gold())
    } else {
        egui::Stroke::NONE
    };
    ui.painter().rect(
        tile_rect,
        metrics.radius,
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );

    let tint = if active {
        shell.gold()
    } else if main_hovered || chevron_hovered || menu_open {
        shell.t1()
    } else {
        shell.t3()
    };
    // 图标让位：右缘小三角 → 图标左移（垂直方向不再被挤），下缘小三角 →
    // 图标上移（几何见 `SplitButtonMetrics::icon_center`，两处不各写一遍）。
    let icon_rect = egui::Rect::from_center_size(
        metrics.icon_center(tile_rect),
        egui::Vec2::splat(metrics.icon),
    );
    icon_image(icon, tint, metrics.icon).paint_at(ui, icon_rect);

    // 小三角：命中区的 ~55%，用 `convex_polygon` 画实心三角（与 `tree_row`
    // 的展开三角同款手法），比再嵌一张 chevron SVG 更省一层图像解码。
    // 两种方位都朝下——弹层都从瓷砖一侧展开。
    let c = chevron_rect.center();
    let half = metrics.chevron_hit * 0.27;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - half, c.y - half * 0.6),
            egui::pos2(c.x + half, c.y - half * 0.6),
            egui::pos2(c.x, c.y + half * 0.75),
        ],
        if active { shell.gold() } else { shell.t3() },
        egui::Stroke::NONE,
    ));

    // ── 弹层 ──────────────────────────────────────────────────────────
    // **必须** `from_toggle_button_response`（= memory 驱动的 Toggle）。egui
    // 0.36 的 `Popup::from_response` 默认 `OpenKind::Open`——文档原话「The
    // popup will be always open」，菜单会在第一帧就弹出来且再也关不掉。
    // 迁移踩坑记录见 wv-app `project_menu.rs` 同款注释。
    let mut picked: Option<usize> = None;
    egui::Popup::from_toggle_button_response(&chevron_resp)
        .id(menu_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            // 菜单**定宽**：`menu_item` 的行按 `available_width()` 铺满，而
            // Popup 的可用宽度默认是剩余屏幕——不定宽菜单会横贯整个屏幕。
            // 宽度取「最长中文标签 + 行尾快捷键 + 图标 + 边距」，长英文标签
            // 允许截断（行高 28 固定，截断比撑破工具条好）。
            ui.set_min_width(MENU_WIDTH);
            ui.set_max_width(MENU_WIDTH);
            egui::Frame::NONE
                .fill(shell.float_fill())
                .stroke(egui::Stroke::new(1.0, shell.border_strong()))
                .corner_radius(egui::CornerRadius::from(6.0))
                .inner_margin(egui::Margin::same(4))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(71),
                })
                .show(ui, |ui| {
                    let sel = selected.min(items.len().saturating_sub(1));
                    for (i, it) in items.iter().enumerate() {
                        let label = if it.hint.is_empty() {
                            it.label.clone()
                        } else {
                            format!("{}  {}", it.label, it.hint)
                        };
                        let state = if i == sel {
                            MenuState::Selected
                        } else {
                            MenuState::Default
                        };
                        let row = menu_item(ui, shell, Some(it.icon.clone()), &label, state);
                        // 行内文字过长时截到单行（定宽后可能发生），不给换行机会。
                        if row.clicked() {
                            picked = Some(i);
                        }
                    }
                });
        });
    if let Some(i) = picked {
        on_pick(i);
        egui::Popup::close_all(ui.ctx());
    }

    // ── 主区点击 ──────────────────────────────────────────────────────
    if main_resp.clicked() {
        on_main_click();
    }
    main_resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(main_hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::{make_ui, shell};

    fn metrics() -> SplitButtonMetrics {
        SplitButtonMetrics {
            tile: egui::vec2(40.0, 38.0),
            icon: 16.0,
            chevron_hit: 14.0,
            radius: 5.0,
            chevron_side: ChevronSide::RightCenter,
        }
    }

    fn items() -> Vec<SplitMenuItem> {
        vec![
            SplitMenuItem::new(crate::widgets::primitives::ICON_VECTOR_POLYGON, "多边形"),
            SplitMenuItem::new(crate::widgets::primitives::ICON_VECTOR_SQUARE, "矩形"),
            SplitMenuItem::new(crate::widgets::primitives::ICON_ELLIPSE, "椭圆"),
        ]
    }

    /// 右缘方位（`RightCenter`）：命中区贴**右缘**、垂直居中，图标中心
    /// 让到小三角左侧、**垂直方向仍在瓷砖中线**——右下角方位曾把图标往上挤
    /// （2026-10-20 实机反馈），这条测试钉住「不再挤」。
    #[test]
    fn right_center_side_hugs_the_right_edge_and_keeps_icon_vertically_centered() {
        let m = metrics(); // 默认即 RightCenter
        let tile = egui::Rect::from_min_size(egui::Pos2::ZERO, m.tile);
        let chev = m.chevron_rect(tile);
        assert!(tile.contains_rect(chev), "小三角区 {:?} 越出瓷砖", chev);
        assert!(
            (chev.right() - tile.right()).abs() < 1e-3,
            "横排小三角应贴右缘，chev={chev:?}"
        );
        assert!(
            (chev.center().y - tile.center().y).abs() < 1e-3,
            "横排小三角应垂直居中，chev={chev:?}"
        );
        let ic = m.icon_center(tile);
        assert!(
            (ic.y - tile.center().y).abs() < 1e-3,
            "图标垂直方向必须在中线（不往上挤），实得 {ic:?}"
        );
        assert!(
            ic.x < chev.left(),
            "图标中心 x={:.1} 应在小三角左缘 {:.1} 之左",
            ic.x,
            chev.left()
        );
        assert!(
            (ic.x - (m.tile.x - m.chevron_hit) * 0.5).abs() < 1e-3,
            "图标中心 = 左缘 + (tile−hit)/2，实得 {ic:?}"
        );
    }

    /// 纵排方位（`BottomCenter`）：命中区贴**下缘**、水平居中，图标中心
    /// 让到小三角上方、水平方向仍在瓷砖中线——「小三角在图标下方」。
    #[test]
    fn bottom_center_side_hugs_the_bottom_edge_and_keeps_icon_horizontally_centered() {
        let m = SplitButtonMetrics {
            chevron_side: ChevronSide::BottomCenter,
            ..metrics()
        };
        let tile = egui::Rect::from_min_size(egui::Pos2::ZERO, m.tile);
        let chev = m.chevron_rect(tile);
        assert!(tile.contains_rect(chev), "小三角区 {:?} 越出瓷砖", chev);
        assert!(
            (chev.bottom() - tile.bottom()).abs() < 1e-3,
            "纵排小三角应贴下缘，chev={chev:?}"
        );
        assert!(
            (chev.center().x - tile.center().x).abs() < 1e-3,
            "纵排小三角应水平居中，chev={chev:?}"
        );
        let ic = m.icon_center(tile);
        assert!(
            (ic.x - tile.center().x).abs() < 1e-3,
            "图标水平方向必须在中线，实得 {ic:?}"
        );
        assert!(
            ic.y < chev.top(),
            "图标中心 y={:.1} 应在小三角上缘 {:.1} 之上",
            ic.y,
            chev.top()
        );
    }

    /// 整枚按钮占位必须与 metrics 一致——工具条的尺寸推导式（两条 dock 尺寸
    /// 测试）依赖调用方按 metrics 精确铺排。
    #[test]
    fn split_button_allocates_exactly_one_tile() {
        let sh = shell();
        let m = metrics();
        let (_c, mut ui) = make_ui("split_geom", 200.0, 120.0);
        split_button(
            &mut ui,
            &sh,
            "sb_geom",
            m,
            crate::widgets::primitives::ICON_VECTOR_POLYGON,
            false,
            "提示",
            &items(),
            0,
            || {},
            |_| {},
        );
        let got = ui.min_rect();
        assert!(
            (got.width() - m.tile.x).abs() < 1e-3,
            "按钮宽应 = tile.x {}，实得 {}",
            m.tile.x,
            got.width()
        );
        assert!(
            (got.height() - m.tile.y).abs() < 1e-3,
            "按钮高应 = tile.y {}，实得 {}",
            m.tile.y,
            got.height()
        );
    }

    /// 真点击主区 → 只触发 `on_main_click`，绝不误触发 `on_pick`。
    ///
    /// 这是分裂按钮最容易写坏的一处：两块重叠的命中区若主区先注册，小三角
    /// 就永远收不到点击；反过来小三角会吞掉整枚按钮。
    #[test]
    fn clicking_main_area_does_not_pick_from_menu() {
        let sh = shell();
        let m = metrics();
        // 指针取瓷砖正中——离右下角小三角最远的点。
        let pos = egui::pos2(m.tile.x * 0.5, m.tile.y * 0.5);
        let (mut mains, mut picks) = (0usize, Vec::new());
        run_frames(&sh, "sb_main", m, pos, 3, |_| mains += 1, |i| picks.push(i));
        assert_eq!(mains, 1, "主区应恰好触发一次 on_main_click");
        assert!(picks.is_empty(), "主区点击不应触发 on_pick，got {picks:?}");
    }

    /// 点小三角 → **只**弹菜单，不触发主区回调；再在弹层上落点 → 回调
    /// `on_pick(下标)`。
    ///
    /// 落点用**扫描**而不是硬编码坐标：弹层的实际锚点由 egui 的
    /// `PopupAnchor::AlignToButton` 与帧内布局决定，写死 `+4+28+14` 之类的
    /// 偏移一旦弹层样式改动就假失败，而假失败会掩盖真回归。扫描一列点、
    /// 断言「三项按下标 0/1/2 依次可达」，验证的正是**行到下标的映射**这件
    /// 本来就该验的事，且对弹层偏移不敏感。
    #[test]
    fn clicking_chevron_opens_menu_without_firing_main_and_rows_map_to_indices() {
        let sh = shell();
        let m = metrics();
        let chev = m.chevron_rect(egui::Rect::from_min_size(egui::Pos2::ZERO, m.tile));
        let chevron_c = chev.center();

        // ① 只点小三角：开菜单，但主区回调一次都不能响。
        let (mut mains, mut picks) = (0usize, Vec::new());
        run_frames(
            &sh,
            "sb_chev_open",
            m,
            chevron_c,
            3,
            |_| mains += 1,
            |i| picks.push(i),
        );
        assert_eq!(mains, 0, "点小三角不应触发主区回调");
        assert!(
            picks.is_empty(),
            "仅点开菜单，不应已选中任何项，got {picks:?}"
        );

        // ② 弹层的**锚点方向**（按钮的左 / 右、上 / 下）是 egui `Popup` 的
        // 实现细节，不是本控件的契约；本控件承诺的是**行序 ↔ 下标**——
        // 菜单里第 i 行触发 `on_pick(i)`，且行随下标自上而下排列。于是扫
        // 按钮四周一片区域收集 (下标, 命中 y)，断言：三项全部可达 + y 严格
        // 按 0 < 1 < 2 递增。对锚点方向、定宽、行高改动都不敏感。
        let mut hits: Vec<(usize, f32)> = Vec::new();
        let mut x = chev.right() - 8.0;
        while x <= chev.right() + 150.0 {
            let mut y = chev.bottom();
            while y <= chev.bottom() + 110.0 {
                let mut got: Option<usize> = None;
                run_menu_then_click(&sh, m, chevron_c, egui::pos2(x, y), |i| got = Some(i));
                if let Some(i) = got {
                    hits.push((i, y));
                }
                y += 5.0;
            }
            x += 26.0;
        }
        let mut idxs: Vec<usize> = hits.iter().map(|(i, _)| *i).collect();
        idxs.sort_unstable();
        idxs.dedup();
        assert_eq!(idxs, vec![0, 1, 2], "弹层三项应全部可达，实得 {hits:?}");
        for i in 0..2usize {
            let y_max = hits
                .iter()
                .filter(|(gi, _)| *gi == i)
                .map(|(_, y)| *y)
                .fold(f32::NEG_INFINITY, f32::max);
            let y_min = hits
                .iter()
                .filter(|(gi, _)| *gi == i + 1)
                .map(|(_, y)| *y)
                .fold(f32::INFINITY, f32::min);
            assert!(
                y_max < y_min,
                "行序应随下标自上而下：第 {i} 项（最低命中 y={y_max}）应整体在第 {} 项（最高命中 y={y_min}）之上，全部命中 {hits:?}",
                i + 1
            );
        }
    }

    /// 先点开小三角（3 帧），再在 `row` 落点（3 帧），把选中的下标写进
    /// `on_pick`。两段共用一个 Context，模拟真实的两段交互。
    fn run_menu_then_click(
        sh: &UiShell,
        m: SplitButtonMetrics,
        chevron_c: egui::Pos2,
        row: egui::Pos2,
        mut on_pick: impl FnMut(usize),
    ) {
        let ctx = egui::Context::default();
        let its = items();
        let script: [egui::Pos2; 2] = [chevron_c, row];
        for pos in script {
            for frame in 0..3 {
                let pressed = frame == 1;
                let events = if frame == 0 {
                    vec![egui::Event::PointerMoved(pos)]
                } else {
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    }]
                };
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::default())
                        .show(ctx, |ui| {
                            // id 两段必须**相同**：弹层的开合状态按 id 存在 egui
                            // `Memory` 里，换 id 等于每帧开一个新弹层，永远打不开。
                            split_button(
                                ui,
                                sh,
                                "sb_scan",
                                m,
                                crate::widgets::primitives::ICON_VECTOR_POLYGON,
                                false,
                                "提示",
                                &its,
                                0,
                                || {},
                                &mut on_pick,
                            );
                        });
                });
                output.textures_delta.clear();
            }
        }
    }

    /// 驱动 `n` 帧（hover → 按下 → 抬起），把 `on_main` / `on_pick` 记进外部计数。
    fn run_frames(
        sh: &UiShell,
        id: &str,
        m: SplitButtonMetrics,
        pos: egui::Pos2,
        frames: usize,
        mut on_main: impl FnMut(usize),
        mut on_pick: impl FnMut(usize),
    ) {
        let ctx = egui::Context::default();
        let its = items();
        for frame in 0..frames {
            let events = match frame {
                0 => vec![egui::Event::PointerMoved(pos)],
                1 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                _ => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default())
                    .show(ctx, |ui| {
                        split_button(
                            ui,
                            sh,
                            id,
                            m,
                            crate::widgets::primitives::ICON_VECTOR_POLYGON,
                            false,
                            "提示",
                            &its,
                            0,
                            || on_main(1),
                            &mut on_pick,
                        );
                    });
            });
            // headless 无 GPU surface：丢弃未应用的纹理增量，避免 epaint
            // 「Dropped TexturesDelta with unapplied deltas」panic。
            output.textures_delta.clear();
        }
    }
}
