//! # game_consumer —— 演示"游戏 app 接入 wv-render"的最小路径
//!
//! **目的**（ADR-0023 M2 验收点）：证明一个游戏 app 可以**只装想要的部分**——
//! `MapRenderPlugin` + `MapShellPlugin`（游戏可用）而**不装** `EditorOverlayPlugin`（编辑器专属）。
//! 同时演示风格层接入：默认风格包来自 `wv-default-theme`（ADR-0066 单一风格）。
//!
//! ## 运行方式
//!
//! ```bash
//! cargo run --example game_consumer
//! ```
//!
//! ## 与 `wv-app` 编辑器入口的对比
//!
//! | 装配项 | wv-app 编辑器 | 本示例（游戏）|
//! |---|---|---|
//! | `MapRenderPlugin` | ✅ | ✅ |
//! | `MapShellPlugin` | ✅ | ✅（可选，游戏可省）|
//! | `EditorOverlayPlugin` | ✅ | ❌ 不装 |
//! | 顶栏 / 浮动面板 / 命令面板 | ✅ | ❌ 不装 |
//! | MCP / HTTP 服务 | ✅（`http` feature） | ❌ 不装 |
//! | 风格包 | 默认风格 | 默认风格 |
//!
//! ## 边界
//! - **不**打开窗口（不调 `app.run()`——纯装配演示）；
//! - **不**做实际渲染（无 World 数据 + 无相机装配——属于真游戏 app 的事）；
//! - **不**是生产代码模板——只是验收点。
//!
//! ## 期望输出
//!
//! ```text
//! [game_consumer] default style pack: 专业制图
//! [game_consumer] MapRenderPlugin + MapShellPlugin installed (no EditorOverlayPlugin)
//! ```

use bevy::prelude::*;
use wv_default_theme::theme_pack::builtin;
use wv_render::plugins::{EditorOverlayPlugin, MapRenderPlugin, MapShellPlugin};

fn main() {
    // 1. 风格包：单一默认包（ADR-0066 退役多主题机制后的唯一入口）。
    let default_pack = &builtin::DEFAULT;

    println!(
        "[game_consumer] default style pack: {} (id={})",
        default_pack.display_name, default_pack.id,
    );

    // 2. Bevy App 装配：只装 MapRenderPlugin + MapShellPlugin，**不装** EditorOverlayPlugin
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(MapRenderPlugin)
        .add_plugins(MapShellPlugin);
    // 故意不调：app.add_plugins(EditorOverlayPlugin);

    // 3. 验证 EditorOverlayPlugin **存在但未装**：能构造、能 install，但本 App 不调它
    let _editor_plugin = EditorOverlayPlugin; // 仅构造，证明类型可达
    println!(
        "[game_consumer] MapRenderPlugin + MapShellPlugin installed; \
         EditorOverlayPlugin type-reachable but not added (game app skip)"
    );

    // 4. 不调 app.run()——本示例是装配演示，非可运行游戏
    drop(app);
    println!("[game_consumer] done (no window opened)");
}
