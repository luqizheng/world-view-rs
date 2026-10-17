//! 事实地点（地界）→ 提交 `wv_edit::command` 的动作层（theme overlay Task 7）。
//!
//! ## 职责
//! - [`add_feature_place_now`]：把圈地界工具闭合的环 + 作者输入的名称，
//!   包成一条 [`AddFeaturePlaceCommand`] 提交到 [`EditHistory`]。
//!
//! ## 设计依据
//!
//! 与 `region_panel_actions.rs` 同构：UI 只收集输入，提交集中在动作层；
//! 单条命令包成 `Transaction`（一次编辑 = 一次 Undo 单元）。
//!
//! ## 边界（不做什么）
//! - **不**直接改 `projection.world`——一律经 `History::commit`（AGENTS §3.3）；
//! - **不**做 UI——对话框在 `crate::ui::feature_place_dialog`；
//! - **不**做时间求值——`span.from` 直接取视图当前日（作者在「此刻」圈地，
//!   区间由 P6/P7 的既有模型表达，动作层不做任何比较）。

#![cfg(feature = "app")]

use crate::edit::editing::EditHistory;
use crate::state::{PlaybackState, ViewState, WorldProjection};
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
/// ## 参数
/// - `name`：作者在命名对话框输入的名称（调用方已 trim / 非空校验）；
/// - `feature`：事实类型编号（工具条写 `FEATURE_FOREST`）；
/// - `ring`：**当前图局部 wu** 的闭合环（首尾不重复，≥3 点）。
///
/// ## 返回
/// 成功返回新地点 ID；无当前图 / 环退化 / 命令被拒（如 ID 冲突）返回 `None`。
/// 失败只记 warn 不抛——与区域动作层的 V1 守约同口径（不阻止作者继续编辑）。
pub fn add_feature_place_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    playback: &PlaybackState,
    name: String,
    feature: u8,
    ring: &[(f32, f32)],
) -> Option<wv_core::Id> {
    // 无当前图 = 没有可落点的工作区（对话框理论上只在有工程时打开；
    // 这里仍守一道，避免空 id 进 `Id::new_unchecked`）。
    if view.current_map.is_empty() || ring.len() < 3 {
        return None;
    }
    let map = wv_core::Id::new_unchecked(&view.current_map);
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
        let view = ViewState::default();
        let playback = PlaybackState {
            current_day: 42,
            ..PlaybackState::default()
        };
        let ring = [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)];

        let id = add_feature_place_now(
            &mut world,
            &mut edit,
            &view,
            &playback,
            "黑森林".into(),
            FEATURE_FOREST,
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
            "地点应锚定到当前图"
        );
        assert_eq!(place.x, Some(20.0), "x 应为环质心");
        assert_eq!(place.y, Some(20.0), "y 应为环质心");
        assert_eq!(place.w, Some(20.0), "w 应为包围盒宽");
        assert_eq!(place.h, Some(20.0), "h 应为包围盒高");

        assert_eq!(world.world.place_geometry.len(), 1, "应恰好新增 1 条地界");
        let geo = &world.world.place_geometry[0];
        assert_eq!(geo.owner, id, "地界 owner 应是新地点");
        assert_eq!(geo.map.as_str(), view.current_map, "地界 map 应是当前图");
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
}
