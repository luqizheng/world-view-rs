//! 合成输入（ADR-0096）：HTTP 工具面把「点击 / 抬起 / 按键」注入主帧，
//! 让 AI 能自己跑验收清单（点 POI、选群系、切工具……），不依赖人手。
//!
//! ## 设计
//! - HTTP 桥（`http_integration`）把事件**入队**（[`SyntheticInputQueue`]）；
//! - [`apply_synthetic_input_press`]（PreUpdate）+ [`apply_synthetic_input_cursor`]（Update）每帧**最多消费一个**事件：写
//!   `ViewState::cursor_world`（覆盖 `track_cursor_world` 的 OS 光标换算）
//!   并按下去——链序保证本帧晚于它跑的选择/笔触系统看到 `just_pressed`；
//! - 一帧一事件是**双击语义的前提**：双击判定按 `just_pressed` 事件计，
//!   同帧两按只会记一次；两帧各一次（HTTP 两次调用天然隔帧）即成双击。
//! - `ButtonInput::clear` 由 Bevy 输入插件在下帧 PreUpdate 执行，合成按下
//!   不会污染后续帧；抬起的显式 `release` 让 `just_released` 消费方
//!   （切刀松手提交等）可用。

#![cfg(feature = "app")]

use bevy::prelude::*;
use std::collections::VecDeque;

/// 一个待注入的合成事件（HTTP 词 → 此处强类型）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyntheticEvent {
    /// 左键按下（世界坐标 = 根图帧）。
    PointerDown { world: Vec2 },
    /// 左键抬起（世界坐标）。
    PointerUp { world: Vec2 },
    /// 按键（当前只接 Escape——验收清单的「Esc 清选区」）。
    KeyPress(KeyCode),
}

/// 待注入队列（HTTP 桥生产，两个消费系统协作）。
#[derive(Resource, Default)]
pub struct SyntheticInputQueue {
    pub events: VecDeque<SyntheticEvent>,
    /// 本帧已按压事件的等待光标：按压系统（PreUpdate）写入，光标系统
    /// （Update 链上 `track_cursor_world` 之后）消费——拆两半的原因见
    /// [`apply_synthetic_input_press`] 文档。
    pub pending_cursor: Option<Vec2>,
}

impl SyntheticInputQueue {
    /// 入队（HTTP 侧调用；无上限——验收脚本是短脉冲，不设背压）。
    pub fn push(&mut self, ev: SyntheticEvent) {
        self.events.push_back(ev);
    }

    /// 队列是否为空（回执 / 测试用）。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// HTTP DTO → 强类型事件（词法错误以 Err 字符串回执，V1 只提示不阻断）。
#[cfg(feature = "http")]
impl TryFrom<crate::http::bridge::SyntheticEventDto> for SyntheticEvent {
    type Error = String;

    fn try_from(dto: crate::http::bridge::SyntheticEventDto) -> Result<Self, String> {
        match dto.op.as_str() {
            "down" => Ok(Self::PointerDown {
                world: Vec2::new(dto.x, dto.y),
            }),
            "up" => Ok(Self::PointerUp {
                world: Vec2::new(dto.x, dto.y),
            }),
            "key" => match dto.key.as_str() {
                "escape" | "esc" => Ok(Self::KeyPress(KeyCode::Escape)),
                other => Err(format!("未知键 `{other}`；当前词表：escape")),
            },
            other => Err(format!("未知输入操作 `{other}`；合法词表：down / up / key")),
        }
    }
}

/// 按压语义注入（PreUpdate）：每帧至多消费一个事件。
///
/// **为什么在 PreUpdate**：`just_pressed` 只在按压帧为真，下一帧被
/// bevy_input 的清场系统抹掉。本仓库的点击消费方分散在两条互不排序的
/// 注册链上——wv-app 的编辑链（点选/圈选/手势，排在本仓库光标系统之后，
/// Update 内可见）与 wv-render 自注册链（`drill_pick` 等，与编辑链无
/// 约束，帧内可能跑在注入之前 ⇒ 永远看不到）。放 PreUpdate、排在
/// `bevy::input::InputSystems`（清场）之后，所有 Update 侧消费者无论
/// 内部顺序一律可见——一处排序治全部。
///
/// 同帧还压掉 [`wv_render::camera::PointerOverUi`]（排在
/// `poll_egui_wants_input` 之后写入）：真实 OS 光标可能悬在面板上让
/// 「UI 让位」门吞掉合成点击；PreUpdate 覆盖管住当帧全部让位门。
///
/// 光标不在这里写（`track_cursor_world` 会在 Update 里用真实鼠标覆写它）——
/// 存进 [`SyntheticInputQueue::pending_cursor`]，由 Update 链上的
/// [`apply_synthetic_input_cursor`] 消费。
pub fn apply_synthetic_input_press(
    mut queue: ResMut<SyntheticInputQueue>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut pointer: ResMut<wv_render::camera::PointerOverUi>,
) {
    let Some(ev) = queue.events.pop_front() else {
        return;
    };
    bevy::log::debug!("[synth] 消费 {:?}", ev);
    pointer.wants_pointer = false;
    match ev {
        SyntheticEvent::PointerDown { world } => {
            mouse.press(MouseButton::Left);
            queue.pending_cursor = Some(world);
        }
        SyntheticEvent::PointerUp { world } => {
            mouse.release(MouseButton::Left);
            queue.pending_cursor = Some(world);
        }
        SyntheticEvent::KeyPress(code) => {
            keys.press(code);
        }
    }
}

/// 光标覆盖（Update）：消费本帧按压事件的世界坐标。
///
/// 必须排在 `track_cursor_world` **之后**（覆盖它）、各点击/手势系统
/// **之前**（同一条 chain）——位置与旧单系统实现一致。
pub fn apply_synthetic_input_cursor(
    mut queue: ResMut<SyntheticInputQueue>,
    mut view: ResMut<crate::state::ViewState>,
) {
    if let Some(world) = queue.pending_cursor.take() {
        view.cursor_world = Some(world);
    }
}

#[cfg(test)]
mod tests {
    //! 每帧一事件 + 光标覆盖 + 按下语义（ADR-0096 的两个回归锚点）。

    use super::*;
    use bevy::ecs::schedule::Schedule;

    fn world_with_queue(events: Vec<SyntheticEvent>) -> World {
        let mut world = World::new();
        world.init_resource::<crate::state::ViewState>();
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        // 系统签名含 PointerOverUi（UI 抢占门）——夹具缺席即参数校验失败。
        world.init_resource::<wv_render::camera::PointerOverUi>();
        let mut q = SyntheticInputQueue::default();
        for ev in events {
            q.push(ev);
        }
        world.insert_resource(q);
        world
    }

    /// 两个 PointerDown 分两帧消费（双击语义前提），光标被逐帧覆盖。
    #[test]
    fn queue_consumes_one_event_per_frame() {
        let mut world = world_with_queue(vec![
            SyntheticEvent::PointerDown {
                world: Vec2::new(1.0, 2.0),
            },
            SyntheticEvent::PointerDown {
                world: Vec2::new(3.0, 4.0),
            },
        ]);
        let mut schedule = Schedule::default();
        // 生产里两系统分处 PreUpdate/Update（帧内必此序）；裸 Schedule 无
        // 帧界语义，显式 chain 复刻同一顺序。
        schedule.add_systems((apply_synthetic_input_press, apply_synthetic_input_cursor).chain());

        schedule.run(&mut world);
        {
            let view = world.resource::<crate::state::ViewState>();
            assert_eq!(view.cursor_world, Some(Vec2::new(1.0, 2.0)));
            let mouse = world.resource::<ButtonInput<MouseButton>>();
            assert!(mouse.just_pressed(MouseButton::Left), "首帧应按下");
        }

        schedule.run(&mut world);
        {
            let view = world.resource::<crate::state::ViewState>();
            assert_eq!(view.cursor_world, Some(Vec2::new(3.0, 4.0)));
            // ButtonInput 自身不跨帧清 just_pressed——清场由 Bevy 输入插件
            // 的下帧 PreUpdate 负责（生产调度）；本裸 Schedule 里两帧连跑
            // 会同时看到两次按下，属测试环境差异，非系统缺陷。这里只断言
            // 「第二帧光标已被第二个事件覆盖」= 每帧恰消费一个。
            // （并行会话 clippy 协作修：原 `let mouse` 绑定未使用，删绑定
            // 而非加下划线——注释即文档。）
        }
        let queue = world.resource::<SyntheticInputQueue>();
        assert!(queue.is_empty(), "两帧后队列应清空");
    }

    /// KeyPress 注入 Escape 的按下语义。
    #[test]
    fn key_press_injects_just_pressed() {
        let mut world = world_with_queue(vec![SyntheticEvent::KeyPress(KeyCode::Escape)]);
        let mut schedule = Schedule::default();
        // 生产里两系统分处 PreUpdate/Update（帧内必此序）；裸 Schedule 无
        // 帧界语义，显式 chain 复刻同一顺序。
        schedule.add_systems((apply_synthetic_input_press, apply_synthetic_input_cursor).chain());
        schedule.run(&mut world);
        let keys = world.resource::<ButtonInput<KeyCode>>();
        assert!(keys.just_pressed(KeyCode::Escape), "Escape 应处于按下帧");
    }
}
