//! 合成输入（ADR-0096）：HTTP 工具面把「点击 / 抬起 / 按键」注入主帧，
//! 让 AI 能自己跑验收清单（点 POI、选群系、切工具……），不依赖人手。
//!
//! ## 设计
//! - HTTP 桥（`http_integration`）把事件**入队**（[`SyntheticInputQueue`]）；
//! - [`apply_synthetic_input`] 每帧**最多消费一个**事件：写
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

/// 待注入队列（HTTP 桥生产，[`apply_synthetic_input`] 消费）。
#[derive(Resource, Default)]
pub struct SyntheticInputQueue {
    pub events: VecDeque<SyntheticEvent>,
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

/// 消费合成输入：每帧至多一个事件（见模块注释「一帧一事件」）。
///
/// 排在 `track_cursor_world` **之后**、各点击/手势系统**之前**（同一条
/// chain）——光标覆盖必须在写侧之前，按下必须在其消费侧之前。
pub fn apply_synthetic_input(
    mut queue: ResMut<SyntheticInputQueue>,
    mut view: ResMut<crate::state::ViewState>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    // 合成点击独占指针（ADR-0096）：真实 OS 光标可能悬在面板上——那会让
    // 一切消费方的 `ui.wants_pointer` 让位门把合成点击吞掉。本资源在
    // PreUpdate 写入、本系统在 Update 消费——此处覆盖即管住当帧全部
    // Update 侧让位门（点选 / 圈选 / 手势 / drill_pick 同一套门）。
    mut pointer: ResMut<wv_render::camera::PointerOverUi>,
) {
    let Some(ev) = queue.events.pop_front() else {
        return;
    };
    bevy::log::debug!("[synth] 消费 {:?}", ev);
    pointer.wants_pointer = false;
    match ev {
        SyntheticEvent::PointerDown { world } => {
            view.cursor_world = Some(world);
            mouse.press(MouseButton::Left);
        }
        SyntheticEvent::PointerUp { world } => {
            view.cursor_world = Some(world);
            mouse.release(MouseButton::Left);
        }
        SyntheticEvent::KeyPress(code) => {
            keys.press(code);
        }
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
        schedule.add_systems(apply_synthetic_input);

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
        schedule.add_systems(apply_synthetic_input);
        schedule.run(&mut world);
        let keys = world.resource::<ButtonInput<KeyCode>>();
        assert!(keys.just_pressed(KeyCode::Escape), "Escape 应处于按下帧");
    }
}
