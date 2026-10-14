//! 图层帧（LayerFrame）：`MapNode.layers[].z / visible / opacity` 的每帧投影，
//! 是「作者 YAML 驱动渲染」（原则五 P5）的**唯一求值点**（ADR-0013）。
//!
//! ## 职责
//! - 从 `WorldProjection.world` 取**当前图**（`DrillState` 所指）的 `layers`，
//!   按 [`LayerKind`] 求值出每类渲染组的 [`LayerBinding`]（可见 / 透明度 / Z 偏移）。
//! - 只读世界、只写本资源——**不**改世界（AGENTS §3.3），**不**做时间比较
//!   （layers 无时间区间，不涉 P7）。
//!
//! ## 边界（不做什么）
//! - **不**持有「改图层」的写路径——图层面板写回 YAML（EditCommand）是后续任务。
//! - **不**吸收编辑器脚手架的开关——地图外框（`map_frame`）、笔刷光标、街道
//!   橡皮筋预览属于编辑会话，不属于作者声明的图层。
//! - **不**管轨迹（`tracks_visible`）——`LayerKind` 没有轨迹变体（ADR-0013 待裁）。
//!
//! ## kind → 渲染组的映射（ADR-0013 决策）
//!
//! | `LayerKind` | 渲染内容 | 会话开关（AND） |
//! |---|---|---|
//! | `Terrain` | 地形分块底图 + 等值线 + 编辑网格 | `terrain_visible` |
//! | `Road` | 道路 casing/fill + 街道 + 街道名标注 | `roads_visible` |
//! | `Place` | POI 点标 + 地名标注 | `places_visible` |
//! | `Influence` | 势力多边形叠加 | `factions_visible` |
//! | `EventPoint` | 事件高亮（Gizmos） | `events_visible` |
//! | `Building` | 2.5D 建筑挤出（阴影 / 侧面 / 顶面，ADR-0016 §6） | `buildings_visible` |
//! | `Border` / `Custom` | **无消费者**（预留声明位） | — |
//!
//! ## z 的语义：槽位偏移（ADR-0013 v1）
//!
//! 作者 `z`（i32）按 **每格 [`SLOT_STEP`] = 0.05 世界 Z** 平移该 kind 渲染组的
//! 内建槽位，clamp 到 `±[`Z_CLAMP`]` 格。各渲染组**保留自己的内建槽位常量**
//! （道路 casing 0.09、POI 0.21、地名 0.4……），渲染时叠加 `z_offset`——
//! 因此 `z` 全缺省（=0）时渲染与历史版本**逐位一致**。被否决的「按 z 全量
//! 分带重排」及其理由见 ADR-0013。
//!
//! ## 多个同 kind 图层
//!
//! 取**第一个声明**的生效（与 `streets::target_road_layer` 的「find 第一个
//! Road 图层」口径一致）；其余同 kind 声明位保留在数据里，不做合并。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{DrillState, ViewState, WorldProjection};
use wv_core::map::{LayerKind, MapNode};

/// 作者 `z` 每一格对应的世界 Z 偏移。
///
/// 取 0.05 的理由：小于相邻渲染组的最小槽位差（道路 0.09 → POI 0.21 差 0.12），
/// 作者「+1 格」做的是微调；要越过一组 typically +2~+3 格。上限见 [`Z_CLAMP`]。
pub const SLOT_STEP: f32 = 0.05;

/// 作者 `z` 的钳制半宽：`z ∈ [-4, 4]` → 偏移 `±0.2` 世界 Z。
///
/// 保证任何组合下渲染组都不会越过笔刷光标（0.9）或跌破地形块（0.0）太深，
/// 防呆而不拒收（AGENTS §7.5 V1：数据层不校验渲染层钳）。
pub const Z_CLAMP: i32 = 4;

/// 一个渲染组从作者图层声明求出的绑定。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerBinding {
    /// 会话开关（图层面板布尔）**与**数据层 `visible` 的合取。
    ///
    /// 面板是「本会话临时隐藏」（不落盘），YAML 是作者的持久声明——
    /// 任何一方说不可见即不可见。
    pub visible: bool,
    /// 数据层 `opacity`（clamp 到 `0.0..=1.0`）。未声明 = 1.0。
    pub opacity: f32,
    /// 数据层 `z` 换算的槽位偏移（世界 Z 单位）。未声明 = 0.0。
    pub z_offset: f32,
}

/// 当前图各渲染组的图层绑定（每帧由 [`resolve_layer_frame`] 重算）。
///
/// 命名字段（而非 `HashMap<LayerKind, _>`）让「七类一一在场」由类型保证，
/// 渲染侧 `frame.road` 直读，无查找、无缺键分支。
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct LayerFrame {
    pub terrain: LayerBinding,
    pub border: LayerBinding,
    pub influence: LayerBinding,
    pub road: LayerBinding,
    pub event_point: LayerBinding,
    pub place: LayerBinding,
    pub building: LayerBinding,
    pub custom: LayerBinding,
}

impl Default for LayerFrame {
    /// 启动期缺省（`init_resource`）：按缺省会话开关全可见。
    ///
    /// **不得** derive `Default`——`bool::default()` 是 `false`，会让首帧
    /// （世界尚未求值）所有图层整体消失。
    fn default() -> Self {
        Self::resolve(None, &ViewState::default())
    }
}

impl LayerFrame {
    /// 从当前图（可 `None` = 尚未加载 / 图不在世界）与会话开关求值。
    ///
    /// 纯函数：同样的 (node, view) 输入永远得到同样的输出，方便手算期望值测试。
    pub fn resolve(node: Option<&MapNode>, view: &ViewState) -> Self {
        // 缺省绑定：未声明的 kind 沿用会话开关、不透明、不偏移——
        // 这正是「layers 全缺省的既有地图渲染不变」的落点。
        let mut frame = Self {
            terrain: LayerBinding {
                visible: view.terrain_visible,
                ..LayerBinding::NEUTRAL
            },
            border: LayerBinding::NEUTRAL,
            influence: LayerBinding {
                visible: view.factions_visible,
                ..LayerBinding::NEUTRAL
            },
            road: LayerBinding {
                visible: view.roads_visible,
                ..LayerBinding::NEUTRAL
            },
            event_point: LayerBinding {
                visible: view.events_visible,
                ..LayerBinding::NEUTRAL
            },
            place: LayerBinding {
                visible: view.places_visible,
                ..LayerBinding::NEUTRAL
            },
            building: LayerBinding {
                visible: view.buildings_visible,
                ..LayerBinding::NEUTRAL
            },
            custom: LayerBinding::NEUTRAL,
        };
        let Some(node) = node else {
            return frame;
        };
        // 同 kind 只认第一个声明（模块文档「多个同 kind 图层」）。
        let mut claimed = [false; 8];
        for layer in &node.layers {
            let idx = kind_index(&layer.kind);
            if claimed[idx] {
                continue;
            }
            claimed[idx] = true;
            let view_visible = slot_visible_default(&frame, &layer.kind);
            *slot_mut(&mut frame, &layer.kind) = LayerBinding {
                visible: view_visible && layer.visible,
                opacity: layer.opacity.clamp(0.0, 1.0),
                z_offset: (layer.z.clamp(-Z_CLAMP, Z_CLAMP) as f32) * SLOT_STEP,
            };
        }
        frame
    }
}

impl LayerBinding {
    /// 中性绑定：可见、不透明、不偏移。
    const NEUTRAL: LayerBinding = LayerBinding {
        visible: true,
        opacity: 1.0,
        z_offset: 0.0,
    };
}

/// kind → 槽位序号（穷尽匹配：`LayerKind` 加变体时此处编译报错，防漏登记）。
fn kind_index(kind: &LayerKind) -> usize {
    match kind {
        LayerKind::Terrain => 0,
        LayerKind::Border => 1,
        LayerKind::Influence => 2,
        LayerKind::Road => 3,
        LayerKind::EventPoint => 4,
        LayerKind::Place => 5,
        LayerKind::Building => 6,
        LayerKind::Custom => 7,
    }
}

/// 取某 kind 槽位的**会话开关**缺省值（声明合取用）。
fn slot_visible_default(frame: &LayerFrame, kind: &LayerKind) -> bool {
    match kind {
        LayerKind::Terrain => frame.terrain.visible,
        LayerKind::Border => frame.border.visible,
        LayerKind::Influence => frame.influence.visible,
        LayerKind::Road => frame.road.visible,
        LayerKind::EventPoint => frame.event_point.visible,
        LayerKind::Place => frame.place.visible,
        LayerKind::Building => frame.building.visible,
        LayerKind::Custom => frame.custom.visible,
    }
}

/// 取某 kind 槽位的可变引用（与 [`kind_index`] 同序）。
fn slot_mut<'f>(frame: &'f mut LayerFrame, kind: &LayerKind) -> &'f mut LayerBinding {
    match kind {
        LayerKind::Terrain => &mut frame.terrain,
        LayerKind::Border => &mut frame.border,
        LayerKind::Influence => &mut frame.influence,
        LayerKind::Road => &mut frame.road,
        LayerKind::EventPoint => &mut frame.event_point,
        LayerKind::Place => &mut frame.place,
        LayerKind::Building => &mut frame.building,
        LayerKind::Custom => &mut frame.custom,
    }
}

/// 每帧重算图层帧。放在 `compute_projection` 之前跑（bootstrap 主链）。
///
/// 手动相等比较（而非直接赋值）：绑定没变时不触发 Bevy change detection，
/// 下游 `is_changed()` 的渲染系统（分块 / 等值线 / 网格的材质与 Z 补丁）保持静默。
pub fn resolve_layer_frame(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
    mut frame: ResMut<LayerFrame>,
) {
    let candidate = LayerFrame::resolve(drill.current_node(&projection.world), &view);
    if *frame != candidate {
        *frame = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::{Layer, MapLevel};
    use wv_core::{Id, World};

    fn layer(id: &str, kind: LayerKind) -> Layer {
        Layer {
            id: Id::new_unchecked(id),
            name: id.into(),
            kind,
            visible: true,
            locked: false,
            opacity: 1.0,
            z: 0,
            source: None,
        }
    }

    fn map(id: &str, layers: Vec<Layer>) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: id.into(),
            level: MapLevel::World,
            parent: None,
            children: Vec::new(),
            icons: Vec::new(),
            layers,
            extent: None,
            raster: None,
            geographic_source: None,
            unit_per_km: None,
            orientation: 0.0,
        }
    }

    #[test]
    fn undeclared_layers_fall_back_to_view_switches_with_neutral_binding() {
        let view = ViewState::default();
        let frame = LayerFrame::resolve(None, &view);
        assert!(frame.terrain.visible && frame.road.visible && frame.place.visible);
        assert!(
            (frame.road.opacity - 1.0).abs() < 1e-6,
            "got {}",
            frame.road.opacity
        );
        assert!(
            frame.road.z_offset.abs() < 1e-6,
            "got {}",
            frame.road.z_offset
        );
        // Border / Custom 无会话开关：缺省恒可见。
        assert!(frame.border.visible && frame.custom.visible);
    }

    #[test]
    fn declared_invisible_layer_hides_its_kind_but_leaves_others() {
        let mut world = World::default();
        let mut road = layer("l_road", LayerKind::Road);
        road.visible = false;
        world
            .maps
            .push(map("map_a", vec![layer("l_t", LayerKind::Terrain), road]));
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let frame = LayerFrame::resolve(drill.current_node(&world), &ViewState::default());
        assert!(!frame.road.visible, "声明 visible:false 的 Road 应隐藏");
        assert!(frame.terrain.visible, "未声明的 kind 不受影响");
        assert!(frame.place.visible);
    }

    #[test]
    fn session_switch_and_layer_visible_conjunctively_gate() {
        let mut world = World::default();
        world
            .maps
            .push(map("map_a", vec![layer("l_r", LayerKind::Road)]));
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let view = ViewState {
            roads_visible: false,
            ..ViewState::default()
        };
        let frame = LayerFrame::resolve(drill.current_node(&world), &view);
        assert!(!frame.road.visible, "会话关 + 声明开 = 不可见");
    }

    #[test]
    fn z_shifts_slot_by_step_per_unit_with_clamp() {
        let mut world = World::default();
        let mut road = layer("l_r", LayerKind::Road);
        road.z = 2;
        let mut place = layer("l_p", LayerKind::Place);
        place.z = 9; // 越界：钳到 Z_CLAMP = 4。
        world.maps.push(map("map_a", vec![road, place]));
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let frame = LayerFrame::resolve(drill.current_node(&world), &ViewState::default());
        assert!(
            (frame.road.z_offset - 2.0 * SLOT_STEP).abs() < 1e-6,
            "got {}",
            frame.road.z_offset
        );
        assert!(
            (frame.place.z_offset - 4.0 * SLOT_STEP).abs() < 1e-6,
            "z=9 应钳到 +4 格：got {}",
            frame.place.z_offset
        );
    }

    #[test]
    fn negative_z_shifts_group_below_its_built_in_slot() {
        let mut world = World::default();
        let mut road = layer("l_r", LayerKind::Road);
        road.z = -4;
        world.maps.push(map("map_a", vec![road]));
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let frame = LayerFrame::resolve(drill.current_node(&world), &ViewState::default());
        assert!(
            (frame.road.z_offset + 0.2).abs() < 1e-6,
            "got {}",
            frame.road.z_offset
        );
    }

    #[test]
    fn opacity_clamps_into_unit_range() {
        let mut world = World::default();
        let mut a = layer("l_a", LayerKind::Road);
        a.opacity = 1.5;
        let mut b = layer("l_b", LayerKind::Place);
        b.opacity = -0.5;
        world.maps.push(map("map_a", vec![a, b]));
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let frame = LayerFrame::resolve(drill.current_node(&world), &ViewState::default());
        assert!(
            (frame.road.opacity - 1.0).abs() < 1e-6,
            "got {}",
            frame.road.opacity
        );
        assert!(
            (frame.place.opacity - 0.0).abs() < 1e-6,
            "got {}",
            frame.place.opacity
        );
    }

    #[test]
    fn first_declared_layer_of_a_kind_wins() {
        let mut world = World::default();
        let mut first = layer("l_first", LayerKind::Road);
        first.opacity = 0.5;
        let mut second = layer("l_second", LayerKind::Road);
        second.opacity = 0.9;
        second.z = 3;
        world.maps.push(map("map_a", vec![first, second]));
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        let frame = LayerFrame::resolve(drill.current_node(&world), &ViewState::default());
        assert!(
            (frame.road.opacity - 0.5).abs() < 1e-6,
            "got {}",
            frame.road.opacity
        );
        assert!(
            frame.road.z_offset.abs() < 1e-6,
            "第二个声明不得覆盖：got {}",
            frame.road.z_offset
        );
    }

    #[test]
    fn frame_of_child_map_uses_child_layers_not_parents() {
        let mut world = World::default();
        let mut parent_road = layer("l_parent_road", LayerKind::Road);
        parent_road.visible = false;
        let mut parent = map("map_parent", vec![parent_road]);
        parent.children.push(Id::new_unchecked("map_child"));
        let mut child = map("map_child", vec![layer("l_child_road", LayerKind::Road)]);
        child.parent = Some(Id::new_unchecked("map_parent"));
        child.level = MapLevel::City;
        world.maps.push(parent);
        world.maps.push(child);
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        assert!(
            drill.enter(&world, &Id::new_unchecked("map_child")),
            "应能钻入子图"
        );
        let frame = LayerFrame::resolve(drill.current_node(&world), &ViewState::default());
        assert!(frame.road.visible, "子图层接管后父图声明不再生效");
    }
}
