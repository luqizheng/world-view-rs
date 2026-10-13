# ADR-0011：渲染引擎升级到 Bevy 0.19

**状态**：已接受
**日期**：2026-10-13
**决策者**：用户（授权「有必要可以升级 bevy」，并选定「现在升级 + 0.19.1」）；版本与路径由实现方提出
**关联**：ADR-0008（Google 风默认地图）、ADR-0009（分级缩放）、ADR-0010（街道矢量层）

---

## 1. 处境（Context）

- 本工程自 2024-11 起停在 **Bevy 0.15.3 / bevy_egui 0.32**，上游已发布 0.19.1（2026-08）与 0.20.0（2026-10-08）。
- 接下来的 **Phase 4（街道矢量）与 Phase 5（标签引擎）是新增 Bevy 代码最多的两个阶段**：自定义材质与批量网格、绘制工具交互、标签渲染。
- 本仓库可用的 Bevy 参考物（官方迁移指南、示例、协作方的知识口径）**全部按 0.19 描述**。在 0.15 上写新代码，等于每次都要把 0.19 的写法手工回译，出错与返工率都高。
- 迁移成本随代码量**单调增长**：越晚做，需要改动的行越多。
- 但升级会横跨整个 `wv-app`（28 个模块 + `Cargo.toml` / `Cargo.lock`），而当前工作树里**同时存在另一位协作者的在制改动**。

## 2. 决策（Decision）

1. 引擎升到 **Bevy 0.19.1**，UI 桥接升到 **bevy_egui 0.42.0**（该版本官方声明依赖 `bevy ^0.19`）。**不**直接跳 0.20。
2. 升级**在 Phase 4 之前**做，一次做完，不分散到后续阶段。
3. 升级前先建立**可回退点**（一次全量提交）。
4. 升级本身是**一个独立提交**，可整体 revert。
5. 迁移以**编译器驱动**（`cargo check -p wv-app --features app --all-targets`），并用官方迁移指南交叉核对「**能编译但行为变了**」的部分。
6. 升级后必须过 AGENTS §7.1 的**两套**门槛（含 `--features app` 的渲染层）+ GUI 冒烟目视复核。

## 3. 理由（Rationale）

- **时机**：Phase 4/5 之前是最后一个便宜窗口——此刻待迁移的代码量是后续任何时刻里最小的。
- **版本选择 0.19.1 而不是 0.20.0**：0.19.1 是成熟稳定版，且本仓库手头有 0.17→0.18、0.18→0.19 两段迁移参考；0.20.0 发布仅 5 天，无对应参考，等于自己摸索。跑一轮之后可以再单独开一份 ADR 追 0.20。
- **迁移面实测很小（决定性有利因素）**：全仓 **零 `*Bundle` 用法**——0.16 引入 required components 取代 Bundle、0.17 移除 Bundle，典型工程一半的迁移工作量在这里，本仓库直接免掉。其余要动的面只有：`EventReader` 3 处、`add_event` 2 处、`send_event` 2 处、`Text2d` 10 处、`Material2d` 15 处、`Shader` 10 处、`Msaa` 4 处、`Node` 16 处、`Camera2d` 27 处、`Sprite` 14 处。
- **工具链已满足**：本机 stable 1.97.1，无 rustup 升级动作。

## 4. 代价（Consequences）与被否决的备选

**代价**

1. 迁移**必然触碰另一位协作者在制的 `wv-app` 文件**（API 重命名无法局部回避）。属机械改动，但会与其 diff 重叠——需在提交信息中显式说明，且与其工作约定好先后次序。
2. `Cargo.lock` 大面积重写（Bevy 依赖树整体换代，wgpu 23 → 新版）。
3. 存在「**编译得过但行为变了**」的风险区：文本栈（0.19 把 `TextFont` 拆成 `FontSource` / `FontSize`）、颜色与色彩空间、2D 相机投影默认值、`Msaa` 语义、UI 布局细节。这些编译器抓不到，只能靠 GUI 冒烟目视复核。
4. **并发写同一工作树**：若协作者在迁移期间提交，可能把半迁移状态提交进去。缓解：迁移期间不做长任务、尽早提交、提交信息标注。

**被否决的备选**

| 备选 | 否决理由 |
|---|---|
| A. 先做 Phase 4，之后再升级 | 省下当前的协调成本，但 Phase 4/5 新增的每一行 Bevy 代码都要在迁移里再改一遍，总成本更高 |
| B. 直接上 0.20.0 | 最新但发布仅 5 天；且手头迁移参考不覆盖 0.19→0.20 这一段 |
| C. 长期停在 0.15.3 | 零成本，但新代码持续逆着参考口径写，且距上游越来越远，欠账只会更大 |
| D. 分段升级（0.16 → 0.17 → …） | 每一跳都要跑一遍构建 / 测试 / 冒烟，等于四个风险点；一次到位反而更稳 |
| E. 借机换掉 Bevy / 重写 UI 层 | 与 P1（单机可运行）的成本收益不符，也与 ADR-0008 既有结论冲突 |

## 5. 落地记录

- **（迁移进行中）**：2026-10-13 起，`Cargo.toml` 的 `bevy` 由 `0.15` 改为 `0.19`、`bevy_egui` 由 `0.32` 改为 `0.42`。完成后在此追加：实际改动的文件面、编译器驱动的修复清单、「能编译但行为变了」项目的逐项复核结果、以及两套门槛 + GUI 冒烟的验证结论。

### 5.1 迁移完成（2026-10-13 追加）

**版本落点**：`bevy 0.15.3 → 0.19.1`、`bevy_egui 0.32 → 0.42.0`（egui 0.36.2）、根 `Cargo.toml` `rust-version = "1.80" → "1.95"`（Bevy 0.19.1 的 MSRV 即 1.95.0；本机 stable 1.97.1 满足）。编译错误从 229 个归零（`cargo check -p wv-app --features app --all-targets` exit=0）。

**编译器驱动的修复清单（按类）**：

1. **egui 0.36 API**：`Frame::NONE`、`CornerRadius` 取代 `Rounding`、`Margin` 字段收窄为 `i8`、`Panel::top/bottom` 取代 `TopBottomPanel`/`BottomPanel`、`Popup` 改静态方法、`contexts.ctx_mut()` 返回 `Result` 需 `if let Ok`、`egui_wants_pointer_input` 取代 `ctx.is_pointer_over_area` 类判断。
2. **`Image.data` 变 `Option<Vec<u8>>`**（约 40 处）：`.as_ref().expect("...")` 取用；examples/render_map_modes.rs 与 build_seattle_terrain.rs 同步修。
3. **`Assets::get_mut` 返回 `AssetMut`**：借用存活到析构，mip 链连环借用处需先 `drop(mip0)` 再取下一个。
4. **系统参数 16 参上限**：`apply_pending_commands` 17 参 → 把窗口尺寸挪进 `ViewState.window_size: Vec2`（Default `(1280, 800)`），`camera.rs track_cursor_world` 每帧写入，`ui_shell.rs` 删掉 `Query<&Window>`。
5. **事件 → 消息**：`EventReader` → `MessageReader`（3 处）；`OrthographicProjection` 由结构体变 `Projection::Orthographic(ortho)` 枚举（新增 `camera.rs` `set_ortho_scale`，删掉只读的 `ortho_scale` 辅助函数）。
6. **其余**：`get_single()` → `single()`、`MouseWheel` 新增 `phase: TouchPhase` 字段、`Font::from_bytes` 不再返回 `Result`。

**「能编译但行为变了」三项（GUI 冒烟抓到并修复）**：

1. **egui 面板调度**：bevy_egui 0.42 把 UI 执行挪进独立调度 `EguiPrimaryContextPass`（在 `PostUpdate` 的 EndPass 内由 `run_egui_context_pass_loop_system` 的 `ctx.run_ui` 执行）；UI 系统留 `Update` 会在首帧字体初始化（`Context::run()` 首次调用）之前跑，触发 panic `No fonts available until first call to Context::run()`。修复：`gui.rs` 15 个面板块系统与 `ui_shell.rs` 的 command_palette / startup_load_overlay / load_error_card / startup_load 全部改挂 `EguiPrimaryContextPass`（startup_load 虽不碰 egui，但必须与加载遮罩同调度排其后）；`apply_pending_commands` / `sync_drill_mirror` / `persist_layout_on_change` 留 `Update`。
2. **ICU4X 断行告警刷屏**：每帧 `ICU4X data error: No segmentation model for complex script: Chinese/Japanese`（12 秒 3460 条）。根因：parley 的 `LineSegmenter` 走 `new_for_non_complex_scripts`，中文命中 `ComplexScript::ChineseOrJapanese` 回退且告警；而 icu_provider 2.3.1 在 **`logging` feature 关 + debug 构建**时把 `log::warn` 别名成 `std::eprintln`（`src/lib.rs:192-203`），裸格式绕过一切 `EnvFilter`。修复：`wv-app` 的 `app` feature 增加 `dep:icu_provider`（`features = ["logging"]`，版本 2.3 已在依赖树内，无新二进制），告警改走 `log` facade，再由 `LogPlugin` 的 `filter = "naga=warn,wgpu=error,icu_provider=off"` 静默。中文段落折行质量（回退为整串不断行）属上游数据缺口，不影响单行地名标签。

**验证**（2026-10-13，全部 exit=0）：`cargo test -p wv-app --features app`（lib 197 + bin 14）；`cargo test -p wv-core -p wv-io -p wv-edit -p wv-terrain`（117 / 14+34+3 / 25 / 8+9+20）；`cargo clippy` 双路（工作区无 feature 渲染盲区外，`-p wv-app --all-targets --features app`）；GUI 冒烟 `wv gui -m examples/xinhai-1911` 12 秒进程存活、stderr 仅 6 行 INFO、0 panic、0 ICU 刷屏。世界图 + 地名 + 事态时间轴 + 地形设计器面板渲染正常。

### 5.2 用户验收后的补修：egui 中文全变方块（2026-10-13 追加）

用户实机启动后报告「中文字符变为了方块」。截图定位：**画布地名（Bevy `Text2d`）正常，egui 面板全部豆腐块**——即 Bevy 侧字体资产没问题，只有 egui 侧 `ctx.set_fonts` 从未生效。

**根因**：`fonts::setup_egui_fonts` 注册在 `Startup`，而 bevy_egui **0.42 的主上下文要到 `PreUpdate` 的 `EguiPreUpdateSet::InitContexts` 才创建**；`Startup` 里 `ctx_mut()` 返回 `Err`，被原先的 `if let Ok(ctx) = contexts.ctx_mut()` **静默吞掉**，字体替换整段跳过。0.32 时代上下文在插件构建期即就绪，所以同一份代码在 0.15 下是对的——这是**迁移引入的静默回归**，不报错、不 panic，只有肉眼看界面才能发现（早期冒烟只核对 stderr 行数，漏掉了它）。

**修复**：`setup_egui_fonts` 改为**重试式**并移出 `Startup`——新增 `loaded: Option<Res<LoadedFonts>>` 首参用于成功后早退（零开销，不再反复读字体文件）、`let Ok(ctx) = contexts.ctx_mut() else { return; }` 未就绪就下帧再试；注册改为 `PreUpdate` 中与 `camera::poll_egui_wants_input` 同组 `.after(bevy_egui::EguiPreUpdateSet::InitContexts)`。

**验证**（全部 exit=0）：`cargo app-test`（lib **205** + bin **27**）、`cargo app-clippy`、`cargo clippy -p wv-core -p wv-io -p wv-edit -p wv-terrain --all-targets -- -D warnings`、`cargo test -p wv-core -p wv-io -p wv-edit -p wv-terrain`（120 / 20 / 34 / 3 / 25 / 8 / 9 / 4 / 21 / 5 / 20 全绿）、`rustfmt --check --config skip_children=true`（fonts.rs / gui.rs）；**GUI 实机截图复核**：菜单、最近工程、面板列表、时间轴事件名（铁路风潮 / 保路运动 / 武昌起义）、底部状态栏中文全部正常。

**遗留**：`wv fmt` 检查中 `terrain_chunks.rs:179` 等处 diff 属另一位协作者在制文件，未触碰。

### 5.3 用户验收后的补修：工程菜单常驻打开（2026-10-13 追加）

用户实机反馈：「最近工程 / 面板 这些应该是**点菜单才弹出来**，现在一启动就都出现了」。现象有三层：菜单浮层开机即在、Esc 关不掉、点击别处也关不掉；而它盖住了左上角的面包屑，于是**当前图名（如「中华大地 › 武昌城」）整段时间都看不见**——上一轮的钻取验收里把这个现象误判成「z-order 被浮层压住」，真因就是这条常驻浮层。

**根因**：egui 0.32 → 0.36 的 Popup API 更替时丢了「开合状态存放处」这层语义。

| | 旧（0.32） | 新（0.36，迁移时直接换名） |
|---|---|---|
| 入口 | `egui::popup::popup_below_widget(popup_id, close_behavior, …)` | `egui::Popup::from_response(&resp).id(popup_id)` |
| 开合判定 | **memory 驱动**（只有 memory 记录为打开才渲染） | `OpenKind::Open`——文档原话 *“The popup will be always open”* |
| 关闭路径 | 由 memory 关闭 | `should_close` 对 `OpenKind::Open` 是**空操作**（`egui-0.36.2/src/containers/popup.rs:632-633`），故 `Popup::close_all(ctx)` 也无效 |

即：同一段「`is_id_open` 判色 + 手写 `toggle_id` + `close_all`」的代码在旧 API 下逻辑自洽，换成新 API 后**整段开合控制全部落空**，只剩「永远展开」。与 §5.2 同类：不报错、不 panic，只有肉眼看界面才能发现。

**修复**（`crates/wv-app/src/ui.rs` 的 `project_menu`）：改用 memory 驱动的入口
`egui::Popup::from_toggle_button_response(&resp).id(popup_id).close_behavior(CloseOnClickOutside)`，
并**删掉手写的 `egui::Popup::toggle_id`**——`from_toggle_button_response` 内部即 `open_memory(clicked.then_some(Toggle))`，两处同时 toggle 会互相抵消、菜单永远打不开（代码里已留注释防止回退）。`ui.rs` 是**全工程唯一的 Popup 使用点**，已 grep 确认无同类残留。

**验证**（全部 exit=0）：`cargo app-test`（lib 207 + bin 27）、`cargo app-clippy`、`cargo fmt --all -- --check`；**GUI 实机复核**：开机菜单不出现、左上角面包屑「中华大地 中华大地 · 世界」可见 → 点工程名弹出菜单 → Esc 关闭 → 再点工程名关闭（开合对称）。

**教训（并入 §5.1 的「逐项复核」口径）**：迁移时凡「有默认值的 builder」都要回头看默认值语义；`from_response` 这种名字看着与旧函数等价，实际把「谁决定开合」换了主体。
