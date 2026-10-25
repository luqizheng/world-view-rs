# UI 取色器规格 v0.1（ColorPicker）

> **状态**：已接受（2026-10-25 · 制作人代判）
>
> **代判说明**：本规格由制作人代为落盘。视觉总监在两次催告后仍未产出终态结论，
> 且持续扩散只读调研（累计 50+ 次 Read/Grep 未落盘）。按协作协议「验收超时升级
> 三级」条款，由制作人依据下列已核实事实代判：
> - **用户拍板**：带 alpha 条且可开关；hex 输入框做进浮层且与色板双向同步；
>   本轮交付 = 组件 + 单测 + gallery 展示 + 势力表单迁移。
> - **技术事实**：egui 0.36.2 源码逐项核实（见 §2）。
> - **仓库惯例**：`crates/wv-ui/src/widgets/` 既有控件（badge / tree_row /
>   segmented_control / layer_panel / split_button / value_box）的写法。
>
> 若视觉总监后续回传，本规格按 §7 的「变更规则」追加修订，不静默覆盖。

---

## 1. 范围与边界

### 1.1 做什么

在 `wv-ui` 增加一个**取色器控件**：色块触发器 + 轻量浮层，浮层内含

1. SV 方块（饱和度 × 明度二维取色）
2. 色相条（横向）
3. alpha 条（横向，可开关）
4. hex 文本输入框（与色板双向同步）

### 1.2 明确不做什么

| 不做 | 理由 |
|---|---|
| 不做预置色板 / 主题色快捷区 | 当前无消费者需要；且预置色板会引入「哪些色是合法世界色」的世界语义判断，属 `wv-core` / `wv-theme` 域 |
| 不做 RGB / HSL 数值输入 | hex 已覆盖精确取色需求；再加一组数值输入会让浮层高度翻倍 |
| 不做颜色渐变 / 吸管 / 屏幕取色 | 超出工程工具定位（离线单机、无跨应用权限） |
| 不依赖 `wv-core::color::Color` | `wv-ui` 是叶子 crate，**零兄弟依赖**是硬红线（AGENTS §2.1 / ADR-0026） |
| 不改世界数据模型 | 颜色在数据层已是 `#RRGGBB` 字符串，本控件只是编辑入口 |

### 1.3 形态裁定

**裁定：色块触发器 + `egui::Popup` 浮层（方案 A）。**

被否决的备选：

| 方案 | 否决理由 |
|---|---|
| **B. 仅色板，无 alpha 条** | 用户已拍板「带 alpha 且可开关」。控件层应做成通用件：主题包取色、图层透明度将来可复用同一控件，届时不必改组件重出规格 |
| **C. 内联展开（非 Popup）** | 取色器是**瞬时**操作控件（选一个色就收），不是常驻面板。内联展开会把检视器的属性行撑成几十行高，且不点开时白占地方 |
| **D. 复用 `FloatWindow` 浮窗外壳** | `FloatWindow`（`crates/wv-ui/src/widgets/floater.rs`）是**带标题栏 + 可拖拽 + 可缩放角标**的常驻面板外壳，服务于地图上的 Inspector / Layers 面板。取色器是锚定在控件旁、点外部即关的轻量弹出层，用 `FloatWindow` 过重（多一条标题栏、可拖动语义与「选完就关」冲突）。`egui::Popup` 正是为这种场景设计的，且**仓库已有先例**（`split_button.rs` 的下拉菜单） |

---

## 2. 技术事实（egui 0.36.2 源码核实）

> 实现者只凭本文即可施工，无需再翻 egui 源码。

### 2.1 SV 方块自绘：纯 CPU 网格，无 shader

egui 内置取色器**没有用 GPU shader**。`egui::widgets::color_picker::color_slider_2d` 的实现是顶点色网格：

```rust
let mut mesh = egui::Mesh::default();
for xi in 0..=N {                       // N = 36
    for yi in 0..=N {
        mesh.colored_vertex(egui::pos2(x, y), color32);   // 逐顶点着色
        if xi < N && yi < N {            // 拼两个三角形
            mesh.add_triangle(tl, tl + 1, tl + N + 1);
            mesh.add_triangle(tl + 1, tl + N + 1, tl + N + 2);
        }
    }
}
ui.painter().add(egui::Shape::mesh(mesh));
```

可用 API（`epaint::Mesh`，egui 顶层 `egui::Mesh` 即可）：

- `colored_vertex(Pos2, Color32)` —— 加一个带色顶点
- `add_triangle(a, b, c)` —— 索引三顶点成面
- `add_colored_rect(Rect, Color32)` —— 平铺一块矩形（棋盘格用）

**结论：自绘 SV 方块 / 色相条 / 棋盘格都不需要新依赖、不需要 shader。** 这与
wv-ui 既有控件（全部自绘走 `UiShell` 令牌）的纪律一致。

### 2.2 浮层机制：`egui::Popup`

仓库既有范本见 `crates/wv-ui/src/widgets/split_button.rs`（约 254–285 行）：

```rust
egui::Popup::from_toggle_button_response(&resp)   // ⚠️ 必须是 toggle 版
    .id(menu_id)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .show(|ui| {
        ui.set_min_width(W);                      // ⚠️ 必须定宽
        ui.set_max_width(W);
        egui::Frame::NONE
            .fill(shell.float_fill())
            .stroke(egui::Stroke::new(1.0, shell.border_strong()))
            .corner_radius(egui::CornerRadius::from(6.0))
            .inner_margin(egui::Margin::same(4))
            .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0,
                                   color: egui::Color32::from_black_alpha(71) })
            .show(ui, |ui| { /* … */ });
    });
```

**两个必须遵守的坑（仓库已踩过，注释留痕）**：

1. **必须 `from_toggle_button_response`**。egui 0.36 的 `Popup::from_response` 默认
   `OpenKind::Open` —— 官方文档原话「The popup will be always open」，后果是
   **第一帧就弹出且再也关不掉**（`split_button.rs` 与 wv-app `project_menu.rs`
   两处均踩过）。
2. **必须定宽**。Popup 可用宽度默认是剩余屏幕，不定宽会横贯整个屏幕。

`Popup::frame(egui::Frame)` 可完全覆盖默认 frame（默认 `Frame::popup(ui.style())`），
故浮层外观**能全部走项目令牌**。

### 2.3 颜色类型：`egui::Rgba`（非预乘）

egui 顶层 `pub use ecolor::{Color32, Rgba};`

| 类型 | 格式 | 取舍 |
|---|---|---|
| `egui::Color32` | **预乘** u8 | ❌ 取色器要编辑 alpha，预乘会把 RGB 乘掉，低 alpha 往返丢精度 |
| `egui::Rgba` | **非预乘** f32[4] | ✅ 对外 API 用它 |
| `egui::Hsva` | HSV + A f32 | ✅ 内部状态用它（HSV 是 SV 方块 / 色相条的天然坐标） |

转换 API（均已核实存在）：

- `Rgba::from_srgba_unmultiplied([u8; 4])` / `Rgba::from_rgba_unmultiplied(f32×4)`
- `Rgba::to_srgba_unmultiplied() -> [u8; 4]` / `Rgba::to_rgba_unmultiplied() -> [f32; 4]`
- `Hsva::from_srgba_unmultiplied([u8; 4])` / `Hsva::from_rgba_unmultiplied(f32×4)`
- `Hsva::to_srgba_unmultiplied() -> [u8; 4]` / `Hsva::to_rgba_unmultiplied() -> [f32; 4]`

**色彩空间**：全程 **sRGB**（egui 默认 `NumericColorSpace::GammaByte`），**不引入
Linear 变体**。规格与实现均不得出现 Linear 分支。

### 2.4 仓库既有范式

| 需要什么 | 抄谁 |
|---|---|
| 浮层 + 菜单 | `widgets/split_button.rs`（Popup 用法范本） |
| 自绘值框 | `widgets/controls.rs::value_box`（底 `input_bg` / 描边 `border_subtle` / mono 字） |
| 自绘行 / 色块 | `widgets/primitives.rs`（`row_rect` / `rect_stroke` / `icon_image`） |
| 令牌解析 | `shell::UiShell`（`crates/wv-ui/src/shell.rs`） |
| 尺寸阶梯 | `ui_tokens::{space, radius, font_size}` |
| 测试脚手架 | `widgets/test_support.rs`（`make_ui` / `shell`）；真点击测试范本见 `segmented_control.rs`（三帧驱动 `PointerMoved` → 按下 → 抬起） |

---

## 3. 核心参数表

> 实现者照抄本表数字即可，无需再问。

### 3.1 触发器（色块按钮）

| 项 | 值 | 说明 |
|---|---|---|
| 色块边长 | **18.0 px** | 与 `faction.rs` 现有色块（18×18）一致，迁移时视觉不跳变 |
| 圆角 | `radius::BADGE`（4.0） | 与 `list_item` 的类型色块同档 |
| 描边 | 1.0 px `shell.border_strong()`，`StrokeKind::Inside` | 内描边，不被邻块裁切 |
| 命中区 | 色块矩形本体 | 18px 偏小，故**外扩到 24×24 命中区**（`shell.hit` 24 紧凑档口径），视觉块仍 18px 居中 |
| 焦点环 | 2.0 px `shell.focus()`，`StrokeKind::Outside` | 走令牌：亮色下焦点环是深琥珀而非金色（`ui_tokens::ShellPalette::focus` 的标定理由） |
| 悬停 | 描边由 `border_strong()` 换 `shell.accent()` | 不改底色 —— 底色是**用户选的颜色**，不能被 hover 污染 |

**半透明色的画法**（色块与 SV/alpha 条通用）：先铺棋盘格，再叠一层该颜色。
棋盘格两档灰**走令牌**：`shell.input_bg()`（暗档深）与 `shell.raised_bg()`（亮档浅）。
选它们的理由：这两枚是「输入区底」与「抬升块底」的既有语义色，恰好构成
「凹陷底 + 抬升格」的经典棋盘格明暗关系，且在明暗两套主题下都自动成立 ——
不必像 egui 内置那样写死 `from_gray(32)` / `from_gray(128)` 裸值（那会在亮色
主题下变成几乎不可见的深灰格）。

格子边长 **8.0 px**。

### 3.2 浮层几何

| 项 | 值 | 说明 |
|---|---|---|
| 内容总宽 | **232.0 px** | SV 方块 208 + 左右内边距 12×2 = 232。SV 方块边长取 208 是为了与「色相条 / alpha 条 / hex 框」共用同一列宽 |
| 内边距 | `space::MD`（12.0），四边 | **与菜单分档**：菜单是紧排文字行（4px 够），取色器是有留白的内容区（12px）。这是与 `split_button` 菜单浮层的刻意差异 |
| 圆角 | `radius::PANEL`（10.0） | 内容区比菜单更「有块面感」，用面板档圆角 |
| 描边 | 1.0 px `shell.border_strong()` | 同菜单 |
| 底色 | `shell.float_fill()` | 同菜单（含标定 alpha，明暗分别标定过） |
| 阴影 | `offset [0,8] / blur 24 / spread 0 / black_alpha(71)` | 同菜单。阴影是遮罩不是界面色，沿用既有裸值不做令牌化 |
| 层序 | `egui::Order::Middle` | 取色器浮在面板之上、地图之上（与 `FloatWindow` 默认层序一致） |

### 3.3 SV 方块

| 项 | 值 |
|---|---|
| 边长 | **208.0 × 208.0 px**（正方形） |
| 圆角 | 0.0（方角）—— 渐变网格的圆角裁切在 egui 里需额外 mask，收益不抵复杂度 |
| 描边 | 1.0 px `shell.border_subtle()`，`StrokeKind::Inside` |
| 网格细分 | N = 36（与 egui 内置同档；`(N+1)²` = 1369 顶点，开销可忽略） |
| 选中指示器 | 半径 **7.0 px** 的空心圆环：填充 = 当前选中色，描边 2.0 px **对比色**（选中色的反色，保证在同色系区域内仍可见） |
| 指示器对齐 | 圆心落在方块内，**半径内缩 1.0 px** 防被描边裁掉 |
| 悬停反馈 | 无额外反馈（指示器已在该位置） |

### 3.4 色相条

| 项 | 值 |
|---|---|
| 方向 | 横向 |
| 长度 | **208.0 px**（与 SV 方块同宽） |
| 厚度 | **12.0 px** |
| 位置 | SV 方块下方，间距 `space::SM`（8.0） |
| 网格细分 | N = 36（1D：每格上下两个顶点） |
| 指示器 | 实心三角：顶点朝上，底边贴条下沿，半宽 **6.0 px**，高 **8.0 px**；填充 = 当前色相的纯色，描边 1.0 px 对比色 |
| 描边 | 1.0 px `shell.border_subtle()`，`StrokeKind::Inside` |

### 3.5 alpha 条

| 项 | 值 |
|---|---|
| 方向 | 横向 |
| 长度 | **208.0 px** |
| 厚度 | **12.0 px** |
| 位置 | 色相条下方，间距 `space::SM`（8.0） |
| 画法 | 棋盘格打底（8px 格）+ 从左到右由**不透明**渐变到**全透明**的该色层 |
| 指示器 | 同色相条：实心三角，顶点朝上，半宽 6.0 px，高 8.0 px，填充 = 当前色（含 alpha），描边 1.0 px 对比色 |
| 描边 | 1.0 px `shell.border_subtle()`，`StrokeKind::Inside` |
| **关闭时** | 该条整体不渲染，浮层内容总高相应减少 `12.0 + 8.0` = **20.0 px** |

### 3.6 hex 输入框

| 项 | 值 |
|---|---|
| 位置 | alpha 条下方，间距 `space::SM`（8.0） |
| 高度 | **24.0 px** |
| 宽度 | **208.0 px**（与上方各条同宽，整列对齐） |
| 底色 | `shell.input_bg()` |
| 描边 | 1.0 px `shell.border_subtle()`；**非法态**改为 `shell.danger()` |
| 圆角 | `radius::FIELD`（4.0） |
| 字体 | `crate::ui_theme::mono(font_size::MICRO)`（11px 等宽） |
| 文字色 | 合法 `shell.t1()`；**非法态 `shell.danger()`** |
| 前缀 | 显示 `#`（`#RRGGBB` 形式），大写十六进制 |
| 内边距 | 左右 6.0 px |
| 控件实现 | 用 `egui::TextEdit::singleline` 承载输入（egui 原生支持光标 / 选区 / 粘贴），**外观由 `Frame` 自绘**（`TextEdit` 自身的框用 `visuals` 压掉：底透明、描边 NONE） |

**hex 格式**：显示与产出统一为**大写** `#RRGGBB`（与 `faction.rs` 现有
`format!("#{:02X}{:02X}{:02X}", …)` 一致）。带 alpha 时显示 `#RRGGBBAA`
（8 位）。解析**宽容接受大小写**（`u8::from_str_radix` 本就大小写不敏感）。

### 3.7 浮层内容总高（推导）

| 组成 | 高度 |
|---|---|
| SV 方块 | 208.0 |
| 间距 | 8.0 |
| 色相条 | 12.0 |
| 间距 | 8.0 |
| alpha 条 | 12.0（关闭时 0） |
| 间距 | 8.0 |
| hex 输入框 | 24.0 |
| **内容合计** | **280.0**（alpha 关闭时 **260.0**） |
| 上下内边距 | 12.0 × 2 = 24.0 |
| **浮层总高** | **304.0**（alpha 关闭时 **284.0**） |

---

## 4. 状态表

> 除「用户选的颜色本身」外，一切颜色走令牌。

| 子区块 | 状态 | 表现 |
|---|---|---|
| **触发器色块** | 正常 | 底 = 当前色（半透明则棋盘格打底）；描边 1px `border_strong()` |
| | 悬停 | 描边换 `accent()`；底色不变 |
| | 按下 | 描边换 `accent()`，底色不变 |
| | 聚焦 | 外侧 2px `focus()` 焦点环 |
| | 禁用 | 底色叠 40% 透明；描边换 `border_subtle()`；不响应点击 |
| **SV 方块** | 正常 | 渐变网格 + 1px `border_subtle()` 描边 |
| | 指示器 | 圆环，填充 = 选中色，描边 2px 对比色 |
| **色相条 / alpha 条** | 正常 | 渐变 + 1px `border_subtle()` 描边 |
| | 指示器 | 三角，填充 = 当前值，描边 1px 对比色 |
| **hex 输入框** | 正常 | 底 `input_bg()`，描边 `border_subtle()`，文字 `t1()` |
| | 聚焦 | 描边换 `accent()`（复用 egui `TextEdit` 的焦点态，但**框是自己画的**，故自行处理） |
| | **非法** | 描边 `danger()`，文字 `danger()`；**色板保持上一个合法值不变** |
| **浮层容器** | 正常 | 见 §3.2 |

---

## 5. 明暗双主题差异

`ShellPalette::DARK` / `ShellPalette::LIGHT` 已分别标定，本控件**不引入任何
明暗分支代码** —— 全部走令牌，主题切换自动成立。需注意的三点：

1. **焦点环**：`shell.focus()` 在亮色主题下是**深琥珀**而非金色（`#F2C14E` 在白底
   仅 1.62:1，不能当亮色焦点环）。故焦点环必须走 `focus()` 令牌，**不得**用
   `shell.gold()` 硬编码。
2. **棋盘格**：走 `input_bg()` / `raised_bg()`，明暗自动反相（暗色下深底浅格，
   亮色下浅底深格），无需分支。
3. **浮层底色**：`shell.float_fill()` 已含明暗分别标定的 alpha（暗 .94 / 亮 .93），
   直接用，不要另设。

**浮层内 egui 原生控件的样式**：`Popup` 的默认 frame 来自 `ui.style()`，而
`ui_theme::apply_ui_theme` 已对 Dark / Light **两套主题**都调过
`set_visuals_of`，故 egui 原生控件（`TextEdit` 的光标 / 选区）自动跟随主题，
无需额外处理。**但 `TextEdit` 自身的框必须压掉**（`visuals` 底透明 + 描边 NONE），
否则会与自绘的框叠成两层。

---

## 6. 接线说明

### 6.1 新文件

`crates/wv-ui/src/widgets/color_picker.rs` —— 与 `layer_panel.rs` /
`segmented_control.rs` 的「一职责一文件」惯例一致。

### 6.2 公开 API（建议形态）

```rust
// crates/wv-ui/src/widgets/color_picker.rs

/// 取色器控件（色块触发器 + Popup 浮层）。
pub struct ColorPicker<'a> {
    color: &'a mut egui::Rgba,   // 非预乘，进出唯一载体
    id_salt: egui::Id,
    alpha: bool,                 // 是否显示 alpha 条
    label: Option<&'a str>,      // 可选的无障碍标签
}

impl<'a> ColorPicker<'a> {
    /// 构造：颜色引用 + 控件 id（同一 id 的多个取色器互不串扰）。
    pub fn new(id_salt: impl egui::AsId, color: &'a mut egui::Rgba) -> Self;

    /// 是否显示 alpha 条（默认 true）。
    pub fn with_alpha(mut self, on: bool) -> Self;

    /// 装配并落地。返回触发器的 [`egui::Response`]（`.changed()` 表示颜色被改过）。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response;
}
```

配套导出一个**纯函数**（便于调用方自建 hex 字段 / 便于单测）：

```rust
/// 颜色 → 大写 `#RRGGBB`（或 `#RRGGBBAA`）。
pub fn rgba_to_hex(c: egui::Rgba) -> String;

/// 宽容解析 hex → 颜色。非法返回 `None`（不 panic）。
pub fn hex_to_rgba(s: &str) -> Option<egui::Rgba>;
```

### 6.3 `widgets.rs` 导出

在 `crates/wv-ui/src/widgets.rs`：

```rust
pub mod color_picker;                       // 与其它模块同一区块
pub use color_picker::{hex_to_rgba, rgba_to_hex, ColorPicker};
```

保持 `crate::ui::X` 路径兼容（既有 `pub use` 集中重导出惯例）。

### 6.4 `wv gallery` 展示

- 位置：既有「**04 数值与颜色**」分区（该节现有 Slider 三变体 / ValueUnit /
  Progress）内追加，**不新开分区** —— 取色器就是数值与颜色这一组的成员。
- 布局：色块触发器放在该分区末尾；**浮层默认不展开**（截图看不到）。
  为让 `WV_SHOT` 截图能同时看到浮层，建议在该分区加一个「常驻展开的取色器
  演示」—— 直接用 `ColorPicker` 的内部浮层内容以 `egui::Window` 常驻方式渲染，
  或在 gallery 启动时用 `egui::Popup::open_memory` 预开一个取色器浮层。
  **具体做法由实现者定，只要 `WV_SHOT` 截图中能看到 SV 方块 + 色相条 + hex 框即可。**

### 6.5 势力表单迁移

`crates/wv-app/src/ui/inspector/faction.rs` 现有代码（约 60–90 行）是手搭的
「色板按钮 + 旁边一个 hex `TextEdit`」两个控件，且解析失败时画一块暗红占位
（`egui::Color32::from_rgb(90, 40, 40)` 裸值）。

**替换后目标形态**：

```rust
// 原：egui::color_picker::color_edit_button_srgb(ui, &mut rgb) + 旁边手搭 TextEdit
// 新：
let mut rgba = <从 color_buf 解析出的颜色，或默认色>;
let resp = ColorPicker::new(("faction_color", id.as_str()), &mut rgba)
    .with_alpha(false)                    // 势力色是 #RRGGBB 不透明
    .show(ui, &shell);
if resp.changed() {
    color_buf = rgba_to_hex(rgba);        // 大写 #RRGGBB
    changed = true;
}
```

**行为保留裁定**：

| 原有行为 | 处置 |
|---|---|
| 色板改动回写 hex 文本 | ✅ 保留 —— 由组件内部双向同步承担，调用方只收 `changed()` |
| 手打 hex 回写色板 | ✅ 保留 —— 组件内部实现，调用方不再需要手搭 `TextEdit` |
| **hex 非法时显示暗红占位块** | ⚠️ **改为组件内部行为**：非法 hex 在浮层内的输入框上以 `danger()` 描边 + 危险色文字呈现，**触发器色块仍显示上一个合法色**（不清空、不变红）。原「整块变暗红」的占位视觉并入组件统一处理，调用方不再重复实现 |

**注意**：调用方仍需保留 `color_buf`（字符串）作为提交给 `EditCommand` 的载荷 ——
`wv_core::color::Color` 的类型不变，本控件不改变数据层。

---

## 7. 变更规则

本规格已接受。按 AGENTS §6.3「文档追写不改机制」：后续如需修正，**追加**修订
小节并写明「原结论 X 被修正为 Y，原因 Z」，**不删改**既有结论。

## 8. 待裁项

| # | 待裁 | 暂按 | 收敛点 |
|---|---|---|---|
| 1 | 预置色板（常用世界色快捷区）是否需要 | **不做** | 若将来势力/事件/地点各有常用色板，需先定「色板归 `wv-ui` 还是 `wv-app`」——它依赖世界语义，不属叶子控件层 |
| 2 | 触发器是否需要下拉箭头指示「可点开」 | **不做**（纯色块，靠 hover 描边变化提示） | 若用户反馈「不知道能点」，改为加 `chevron-down` 图标（Lucide） |
| 3 | 方向键能否调 SV 方块取值 | **不支持**（只能鼠标拖拽） | 若需键盘可达，得引入焦点管理 + 方向键映射，属无障碍增强项 |

---

## 9. 相关文档

- `AGENTS.md` —— 七条设计原则、分层纪律（§2.1）、测试要求（§7）、校验不阻止（§7.5 V1）
- `docs/adr/ADR-0026-wv-ui-crate.md` —— wv-ui 叶子 crate 契约
- `assets/ui-design/UI_DESIGN_SPEC_v0.1.md` —— 令牌表与明暗标定
- `crates/wv-ui/src/ui_tokens.rs` —— 令牌唯一代码载体
- `crates/wv-ui/src/widgets/split_button.rs` —— Popup 范本
- `crates/wv-ui/src/widgets/controls.rs::value_box` —— 自绘值框范本
