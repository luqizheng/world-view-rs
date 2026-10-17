//! # wv-ui —— 界面控件层（egui 控件 + 界面令牌）
//!
//! 本 crate 是 World View 的 **UI 控件叶子**（ADR-0026）：界面外壳「长什么样」
//! 的唯一载体——令牌（`ui_tokens`）、egui 视觉投影（`ui_theme`）、每帧样式
//! 快照（[`shell::UiShell`]）、全部公共控件（[`widgets`]）。
//!
//! ## 分层契约（AGENTS §2.1 / ADR-0026）
//!
//! - **只依赖** `bevy` / `bevy_egui` / `egui_extras`。禁止依赖 `wv-core` /
//!   `wv-terrain` / `wv-io` / `wv-edit` / `wv-render` / `wv-app`——控件物理上
//!   够不到世界数据与编辑入口，这是编译级保证，不是评审纪律。
//! - **禁止**文件系统访问（`ui-layout.yaml` 等持久化在 wv-app）、网络、
//!   i18n、世界色板（`crate::theme::colors` 是 wv-app 侧 re-export，不进本 crate）。
//! - **与 wv-render 的边界**：egui 控件与界面令牌 = 本 crate；地图渲染、
//!   GPU 主题、打包字体（Text2d 资源）= wv-render。二者互不依赖，由 wv-app 装配。
//!
//! ## 域件不住这里
//!
//! 依赖业务类型或世界色板的 UI 件留在 wv-app（ADR-0026 D3）：`float_window`
//! （`Panel` 枚举包装）、`terrain_grid` + 地形名/色块 helper。本 crate 只收
//! 「换个工程也成立」的纯控件。
//!
//! ## 测试纪律
//!
//! 控件单测**不依赖** `app` feature 门——本 crate 无 feature，标准
//! `cargo test --workspace` 天然全量编译（§7.1 盲区自本 crate 起消除）。

pub mod shell;
pub mod ui_theme;
pub mod ui_tokens;
pub mod widgets;
