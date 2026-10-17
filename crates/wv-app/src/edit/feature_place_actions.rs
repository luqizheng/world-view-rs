//! 事实地点（地界）→ 提交 `wv_edit::command` 的动作层（theme overlay Task 7）。
//!
//! ## 职责
//! - [`add_feature_place_now`]：把选区意图环 + 作者输入的名称，
//!   包成一条 [`AddFeaturePlaceCommand`] 提交到 [`EditHistory`]。
//!
//! ## 设计依据
//!
//! 与 `region_panel_actions.rs` 同构：UI 只收集输入，提交集中在动作层；
//! 单条命令包成 `Transaction`（一次编辑 = 一次 Undo 单元）。入口是选择工具
//! M4 的「森林」意图（`FeaturePlaceDialog`），不是独立工具。
//!
//! ## 边界（不做什么）
//! - **不**直接改 `projection.world`——一律经 `History::commit`（AGENTS §3.3）；
//! - **不**做 UI——对话框在 `crate::ui::feature_place_dialog`；
//! - **不**做时间求值——`span.from` 直接取视图当前日（作者在「此刻」圈地，
//!   区间由 P6/P7 的既有模型表达，动作层不做任何比较）。

#![cfg(feature = "app")]

use crate::edit::editing::EditHistory;
use crate::state::{PlaybackState, WorldProjection};
use wv_core::distance::Place;
use wv_core::map::{GeometryData, GeometrySpan};
use wv_core::time::TimeSpan;
use wv_edit::command::AddFeaturePlaceCommand;
use wv_edit::history::Transaction;

/// 把一圈地界落成事实地点 + 初始地界。
///
/// ## 为什么地点与地界一条命令
///
/// 与 [`AddFeaturePlaceCommand`] 的文档同源：地理事实不是先有点、再补范围，
/// 拆成两条命令会让 Undo 出现「地点还在、地界没了」的中间态（P6 要求一次
/// 编辑整体回退）。
///
/// ## 调用方
/// 选择工具 M4 的「森林」意图：`FeaturePlaceDialog` 确认时调用。
///
/// ## 参数
/// - `name`：作者在命名对话框输入的名称（调用方已 trim / 非空校验）；
/// - `feature`：事实类型编号（工具条写 `FEATURE_FOREST`）；
/// - `map`：**捕获图 id**（意图按钮点击那一刻 `drill.current()` 钉住、由
///   `FeaturePlaceDialog` 自持的图）。环坐标只在捕获图的局部帧里合法，这里
///   **不用也不看** `view.current_map`：命名对话框开着时作者仍可下钻换图，
///   现取当前图会把环写进错误的图、带着越帧坐标静默落盘（final review
///   数据完整性修复）；
/// - `ring`：**捕获图局部 wu** 的闭合环（首尾不重复，≥3 点）。
///
/// ## 返回
/// 成功返回新地点 ID；环退化 / 命令被拒（如 ID 冲突）返回 `None`。
/// 失败只记 warn 不抛——与区域动作层的 V1 守约同口径（不阻止作者继续编辑）。
pub fn add_feature_place_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    playback: &PlaybackState,
    name: String,
    feature: u8,
    map: &wv_core::Id,
    ring: &[(f32, f32)],
) -> Option<wv_core::Id> {
    // 环退化守门；地图 id 由 `try_close` 从 `drill.current()` 钉来，
    // 天然非空（点击系统只在有当前图时才落点），无需再查空串。
    if ring.len() < 3 {
        return None;
    }
    let map = map.clone();
    let id = wv_core::Id::new_unchecked(format!("place_feature_{}", current_unix_ms()));
    // `x/y/w/h` 留空：`AddFeaturePlaceCommand::apply` 会按环重拟合锚点，
    // 动作层不复制拟合公式（唯一实现点在地点模型里）。
    let place = Place {
        id: id.clone(),
        name,
        feature: Some(feature),
        map: Some(map.clone()),
        x: None,
        y: None,
        w: None,
        h: None,
    };
    let flat: Vec<f32> = ring.iter().flat_map(|&(x, y)| [x, y]).collect();
    let territory = GeometrySpan {
        map,
        owner: id.clone(),
        span: TimeSpan {
            from: playback.current_day,
            to: None,
        },
        geometry: GeometryData::Polygon(flat),
        relation: None,
    };

    let mut tx = Transaction::new();
    tx.push(Box::new(AddFeaturePlaceCommand::new(place, territory)));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            Some(id)
        }
        Err(e) => {
            bevy::log::warn!("地界创建失败: {e}");
            None
        }
    }
}

/// 当前 Unix 毫秒——地点 ID 的时间后缀（`place_feature_<unix_ms>`）。
///
/// 与 `region_panel_actions::current_unix_ms` 同款；ID 只需可读 + 低冲突，
/// 不需要全局计数器（工程是单机单作者，§4.4）。
fn current_unix_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::GeometryData;
    use wv_render::theme::overlays::FEATURE_FOREST;

    /// 提交后：地点 1 条（feature=Some(4)）、地界 1 条（开放区间从当前日）、
    /// 锚点由命令按环拟合，返回新 ID。
    #[test]
    fn add_feature_place_now_writes_open_span_and_anchors() {
        let mut world = WorldProjection::default();
        let mut edit = EditHistory::default();
        let playback = PlaybackState {
            current_day: 42,
            ..PlaybackState::default()
        };
        let ring = [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)];
        let map = wv_core::Id::new_unchecked("world_main");

        let id = add_feature_place_now(
            &mut world,
            &mut edit,
            &playback,
            "黑森林".into(),
            FEATURE_FOREST,
            &map,
            &ring,
        )
        .expect("四点环应提交成功");

        assert_eq!(
            world.world.distances.places.len(),
            1,
            "应恰好新增 1 条事实地点"
        );
        let place = &world.world.distances.places[0];
        assert_eq!(place.id, id, "返回的 ID 必须是入库的地点 ID");
        assert_eq!(place.feature, Some(FEATURE_FOREST), "事实类型应为森林");
        assert_eq!(place.name, "黑森林");
        assert_eq!(
            place.map.as_ref().map(wv_core::Id::as_str),
            Some("world_main"),
            "地点应锚定到捕获图"
        );
        assert_eq!(place.x, Some(20.0), "x 应为环质心");
        assert_eq!(place.y, Some(20.0), "y 应为环质心");
        assert_eq!(place.w, Some(20.0), "w 应为包围盒宽");
        assert_eq!(place.h, Some(20.0), "h 应为包围盒高");

        assert_eq!(world.world.place_geometry.len(), 1, "应恰好新增 1 条地界");
        let geo = &world.world.place_geometry[0];
        assert_eq!(geo.owner, id, "地界 owner 应是新地点");
        assert_eq!(geo.map.as_str(), "world_main", "地界 map 应是捕获图");
        assert_eq!(geo.span.from, 42, "span.from 应取播放当前日");
        assert_eq!(geo.span.to, None, "新建地界必须是开放区间（至今）");
        assert_eq!(
            geo.geometry,
            GeometryData::Polygon(vec![10.0, 10.0, 30.0, 10.0, 30.0, 30.0, 10.0, 30.0]),
            "环应扁平化为 [x0,y0,x1,y1,...]"
        );

        assert!(world.dirty, "提交成功后工作副本应标记为脏");
        // 一次提交 = 一次 Undo 单元：撤销后地点与地界一起消失（P6）。
        assert!(edit.history.undo(&mut world.world).is_ok(), "应可撤销");
        assert!(world.world.distances.places.is_empty(), "撤销后地点应移除");
        assert!(
            world.world.place_geometry.is_empty(),
            "撤销后地界应一并移除"
        );
    }

    /// final review 数据完整性修复：提交**只认闭合时钉住的捕获图 id**。
    /// 命名对话框开着时作者钻进另一张图（`view.current_map` 已变）也不影响
    /// ——环坐标只在捕获图局部帧里合法，落错图等于带着越帧坐标静默写盘。
    /// 场景：环在 `city_zunyi` 里闭合，此刻视图已切回 `world_main`
    /// （对话框仍开着、地图背后仍可交互），提交必须落 `city_zunyi`。
    #[test]
    fn add_feature_place_now_lands_on_the_pinned_capture_map() {
        let mut world = WorldProjection::default();
        let mut edit = EditHistory::default();
        let playback = PlaybackState::default();
        let ring = [(0.0, 0.0), (64.0, 0.0), (64.0, 64.0), (0.0, 64.0)];
        // 捕获图 = 子图；对话框提交时视图可以停在别的图（默认 world_main）。
        let capture_map = wv_core::Id::new_unchecked("city_zunyi");

        let id = add_feature_place_now(
            &mut world,
            &mut edit,
            &playback,
            "城郊林地".into(),
            FEATURE_FOREST,
            &capture_map,
            &ring,
        )
        .expect("钉住的捕获图应提交成功");

        let place = &world.world.distances.places[0];
        assert_eq!(
            place.map.as_ref().map(wv_core::Id::as_str),
            Some("city_zunyi"),
            "地点必须落在闭合时钉住的捕获图，而不是提交时的当前图"
        );
        let geo = &world.world.place_geometry[0];
        assert_eq!(
            geo.map.as_str(),
            "city_zunyi",
            "地界必须与地点同图（同一捕获图 id）"
        );
        assert_eq!(geo.owner, id);
        assert!(
            edit.history.undo(&mut world.world).is_ok(),
            "钉图路径同样整体可撤销"
        );
    }
}
