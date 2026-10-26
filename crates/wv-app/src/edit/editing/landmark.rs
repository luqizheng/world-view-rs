//! 地标放置（ADR-0071）：Landmark 工具激活时单击地图创建点状 `Place`（POI）。
//!
//! ## 与既有 Place 入口的分工
//!
//! 全应用此前没有「单击地图放置 POI」的入口：圈选意图菜单的「标记…」产出
//! 的是**无锚点**的群系标记（`x/y = None`，锚点由包络派生），作者小河
//! （`place_line`）的锚点由折线中点派生。本模块补上最直接的一条——点哪
//! 建哪，名称默认「新地标」，随后在统一 Inspector 的地点表单改名 / 改类别
//! / 调级别（ADR-0058 属性段唯一，本模块**不**重复属性表单）。
//!
//! ## 坐标纪律（两套原点陷阱）
//!
//! - Edit 浏览档：`local = cursor_world − drill::map_origin(...)`——与
//!   `place_line` / `selection` 同一口径，**不**碰 `raster_origin`；
//!   不要求地形（纯矢量图也能放）。
//! - CellMap 工作区（`ViewMode::Topographic`）：先 `cell_pick::pick_at`
//!   反查格子，命中则取**格中心**（`cell_world_rect`）再减 map_origin——
//!   格子工作台对齐格子语义（放出来的点与格网对齐）；点在图外不创建。
//!
//! ## 提交路径
//!
//! 与 Inspector 表单通道（`update_place_now`）同款：直接组 `Transaction`
//! 提交 `AddPlaceCommand`（AGENTS §3.3：改世界必经 `wv-edit`）。创建后把
//! `projection.selection` 指向新点——Inspector 立即可改名，闭环不落地。
//!
//! 互斥（放置期间选择 / 点选 / EditCell 让位）由 `dispatch` 的
//! `SelectMapTool(Landmark)` 分支统一保证，本系统不再自设闸门。

use super::*;

/// 地标放置的左键处理器（ADR-0071）。
///
/// 门控与 [`super::cell::commit_point_select_on_click`] 同口径：
/// 左键按下沿 / 非加载态 / 工具激活 / 指针不在 egui 上 / 非播放 /
/// 极低缩放禁放（与街道手绘 `view.scale < 0.15` 同值）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn commit_landmark_click(
    buttons: Res<ButtonInput<MouseButton>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    screen: Res<crate::app_screen::AppScreen>,
    landmark: Res<crate::state::LandmarkToolState>,
    current_loc: Res<crate::i18n::CurrentLocale>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // 加载期间不放置：落点会写进旧世界，对不上新世界。
    if screen.shows_loading_overlay() {
        return;
    }
    if !landmark.placing {
        return;
    }
    // 指针压在 egui 面板 / 顶栏上：这是面板交互，不是放地标（与点选 /
    // 笔刷的 PointerOverUi gate 同口径）。
    if ui.wants_pointer {
        return;
    }
    // 时间游标在动 → 当前 world 是投影 → 不能改（基线 §1.8 / V37）。
    if playback.playing {
        return;
    }
    // 极低缩放禁放：一格屏幕几千 wu，落点毫无意义还容易误触（与
    // place_line / 街道手绘同值 0.15）。
    if view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let origin = wv_render::drill::map_origin(&projection.world, node, &grid);
    let snap_to_cells = view.mode == wv_render::state::ViewMode::Topographic;
    let terrain = projection.world.terrain_of(&node.id);
    let Some(local) = landmark_anchor(cursor, &grid, terrain, origin, snap_to_cells) else {
        return;
    };

    let place_id = wv_core::Id::new_unchecked(format!(
        "place_poi_{}",
        crate::edit::intent_translator::now_millis()
    ));
    let place = wv_core::distance::Place {
        id: place_id.clone(),
        name: crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::LandmarkDefaultName)
            .to_string(),
        feature: None,
        map: Some(node.id.clone()),
        x: Some(local.x),
        y: Some(local.y),
        w: None,
        h: None,
        biome: None,
        envelope: None,
        rank: Some(landmark.default_rank),
        line: None,
        kind: landmark.default_kind,
    };
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::command::AddPlaceCommand::new(place)));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // 创建即选中：Inspector 立刻显示该点的属性表单，改名零跳转。
            projection.selection = Some(wv_core::SelectedEntity::new(
                wv_core::EntityKind::Place,
                place_id,
            ));
        }
        Err(e) => {
            bevy::log::warn!("地标放置失败: {e}");
        }
    }
}

/// 点击点 → 本图局部坐标（ADR-0071 的落点换算唯一实现点）。
///
/// - `snap_to_cells = false`（Edit 浏览档）：`cursor − origin`，不吸附；
/// - `snap_to_cells = true`（CellMap 工作区）：先反查格子，命中取**格中心**
///   再减 origin；点在图外 / 无地形 → `None`（不创建）。
///
/// 纯函数——坐标链（两套原点）的回归守点，测试钉死格吸附不走 raster_origin。
pub fn landmark_anchor(
    cursor: bevy::math::Vec2,
    grid: &GridConfig,
    terrain: Option<&wv_core::terrain::TerrainData>,
    origin: bevy::math::Vec2,
    snap_to_cells: bool,
) -> Option<bevy::math::Vec2> {
    if snap_to_cells {
        let terrain = terrain?;
        let pick = crate::ui::cell_pick::pick_at((cursor.x, cursor.y), grid, terrain)?;
        // 格中心是世界坐标（基于 raster_origin）；减 map_origin（不是
        // raster_origin）才是本图局部——两套原点只在图自带底图时相等。
        let (center, _) =
            wv_world_cell_map::cell_selection::cell_world_rect(grid, pick.gx, pick.gy);
        Some(center - origin)
    } else {
        Some(cursor - origin)
    }
}

/// 当前图可进地标列表的点状地点（ADR-0071）。
///
/// 过滤口径：锚定在本图（`map == Some(map_id)`）+ 有锚点（`x/y` 均有值，
/// 顺带排除无锚点的群系标记）+ 非折线（小河是线状地物，归 ADR-0046 / 0070
/// 的线段 POI 线，不进地标列表）。**不**按 `kind` 过滤：kind 缺省（旧数据）
/// 的点状地点同样是作者摆的 POI，列表该列。
pub fn landmark_candidates<'a>(
    world: &'a wv_core::World,
    map_id: &wv_core::Id,
) -> Vec<&'a wv_core::distance::Place> {
    world
        .distances
        .places
        .iter()
        .filter(|p| p.map.as_ref() == Some(map_id))
        .filter(|p| p.x.is_some() && p.y.is_some())
        .filter(|p| p.line.is_none())
        .collect()
}

// ── 测试 ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    fn grid() -> GridConfig {
        GridConfig::default()
    }

    fn terrain(w: u32, h: u32) -> wv_core::terrain::TerrainData {
        wv_core::terrain::TerrainData {
            width: w,
            height: h,
            // pick_at 只读 width/height 做边界判断，cells 内容无关。
            cells: Vec::new(),
        }
    }

    #[test]
    fn anchor_in_browse_mode_is_cursor_minus_origin_without_snapping() {
        let g = grid();
        let origin = Vec2::new(100.0, 50.0);
        let a = landmark_anchor(
            Vec2::new(123.4, 77.7),
            &g,
            None, // 无地形：浏览档照放（纯矢量图）
            origin,
            false,
        );
        let a = a.expect("浏览档只要给光标就应返回锚点");
        assert!((a - Vec2::new(23.4, 27.7)).length() < 1e-3, "got {a:?}");
    }

    #[test]
    fn anchor_in_cellmap_mode_snaps_to_cell_center() {
        let g = grid(); // cell_size 缺省 8.0，raster_origin 缺省 0
        let t = terrain(16, 16);
        let origin = Vec2::ZERO;
        // 点击落在格 (2,1) 内部偏角（格范围 x∈[16,24) y∈[8,16)），
        // 吸附后必须是格中心 (20.0, 12.0)。
        let a = landmark_anchor(Vec2::new(17.5, 9.25), &g, Some(&t), origin, true);
        assert_eq!(a, Some(Vec2::new(20.0, 12.0)), "got {a:?}");
    }

    #[test]
    fn anchor_in_cellmap_mode_rejects_outside_and_missing_terrain() {
        let g = grid();
        let t = terrain(4, 4);
        // 图外：不吸附不创建。
        assert_eq!(
            landmark_anchor(Vec2::new(-5.0, 3.0), &g, Some(&t), Vec2::ZERO, true),
            None
        );
        // 无地形 + 吸附模式：CellMap 工作区没有格可落，不创建。
        assert_eq!(
            landmark_anchor(Vec2::new(3.0, 3.0), &g, None, Vec2::ZERO, true),
            None
        );
    }

    #[test]
    fn candidates_keep_only_anchored_point_places_of_current_map() {
        let m1 = wv_core::Id::new_unchecked("map_1");
        let m2 = wv_core::Id::new_unchecked("map_2");
        let mut w = wv_core::World::default();
        let mk = |id: &str, map: Option<wv_core::Id>, x: Option<f32>, line: Option<Vec<f32>>| {
            wv_core::distance::Place {
                id: wv_core::Id::new_unchecked(id),
                name: id.into(),
                feature: None,
                map,
                x,
                y: x,
                w: None,
                h: None,
                biome: None,
                envelope: None,
                rank: None,
                line,
                kind: None,
            }
        };
        w.distances.places = vec![
            mk("ok", Some(m1.clone()), Some(1.0), None),
            // 其他图：不列。
            mk("other_map", Some(m2), Some(2.0), None),
            // 无锚点（群系标记形态）：不列。
            mk("no_anchor", Some(m1.clone()), None, None),
            // 折线（小河）：不列。
            mk(
                "river",
                Some(m1.clone()),
                Some(3.0),
                Some(vec![0.0, 0.0, 9.0, 9.0]),
            ),
        ];
        let got: Vec<_> = landmark_candidates(&w, &m1)
            .into_iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(got, vec!["ok"], "got {got:?}");
    }
}
