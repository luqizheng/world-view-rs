//! # wv-app::edit —— UI 动作 → `EditCommand` 链路
//!
//! 「面板发生了什么操作」与「世界数据怎么被改」之间的桥。
//! 桥的这一端只关心**命令**；Undo/Redo 在 `wv-edit` 里，渲染在 `wv-render`（ADR-0020）里。
//!
//! ## 职责
//! - 把笔刷操作转成 `wv_edit::EditCommand`（`editing`）。
//! - 把面板的「应用/拒绝」按钮转成 `EditCommand`（`faction_panel_actions`）。
//! - 把意图菜单闭合的环 + 名称/群系转成 `PlaceBiomeMarkerCommand`
//!   （`feature_place_actions`；ADR-0032 D6 后旧词汇的动作层与命令
//!   `AddFeaturePlaceCommand` 均已删除）。
//! - 把校验面板点击「跳到位置」转成相机/选择变更（`validation_actions`）。
//! - 把内存 `World` 派生 `Diagnostics` 给校验面板（`diagnostics_bridge`）。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界。一切修改经 `wv-edit` 的命令栈。
//! - **不**渲染。这些模块不注册 Bevy `System` 函数（除非显式需要）。
//! - **不**持有 UI 外壳/面板（那是 `crate::ui::*` 的事）。
//!
//! ## 主要导出
//! - `translate_edit_intents`（渲染编辑意图 → `EditCommand` 提交，ADR-0020 §2.5）—— 来自 `intent_translator`
//! - `apply_brush_at`（笔刷提交）—— 来自 `editing`
//! - `apply_faction_edit`（势力面板提交）—— 来自 `faction_panel_actions`
//! - `place_biome_marker_now` / `regenerate_marker_now`（标记提交/重生成）—— 来自 `feature_place_actions`
//! - `jump_to_diagnostic`（校验面板跳转）—— 来自 `validation_actions`

#[cfg(feature = "app")]
pub mod diagnostics_bridge;
#[cfg(feature = "app")]
pub mod editing;
#[cfg(feature = "app")]
pub mod faction_panel_actions;
#[cfg(feature = "app")]
pub mod feature_place_actions;
#[cfg(feature = "app")]
pub mod intent_translator;
#[cfg(feature = "app")]
pub mod region_panel_actions;
#[cfg(feature = "app")]
pub mod validation_actions;
