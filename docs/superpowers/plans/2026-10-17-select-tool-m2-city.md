# 选择工具 M2 实施计划（生成城市 + 自动钻入）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 意图菜单「生成城市」——闭合选区 → 对话框（名字 / ID / 底图 / 面积预填可改）→ 确认建 16:9 城市子图并**自动钻入**编辑器；钻入图标锚点显式落在质心。

**Architecture:** 新 wv-app 模块 `ui/city_intent.rs`（对话框资源 + 预填纯函数 + 确认系统， continent_dialog 的 UI/apply 拆分同款）；几何纯函数（质心 / 包围盒面积 / 面积→16:9）同模块可测；wv-render 只顺带两件小事（FALLBACK_EXTENT 导出、双击容差 px 化）。选区在确认后由切图自动清空（`sync_selection_map_change`），无需手动消费。

**Tech Stack:** 同 M1（Rust 2021 / Bevy 0.19 / egui 0.36），无新依赖。

**Spec:** `docs/superpowers/specs/2026-10-17-select-tool-pen-intent-design.md` §5（生成城市流程）+ §12 裁定；ADR-0025 D4。

## Global Constraints

- 同 M1 六门禁（workspace 三条 + app 两条 + smoke）；wv-render 红线不变。
- 手算断言：`A = 14400 → 160×90`（16:9）；方形环质心 = 中心。
- ID 校验沿用旧城市弹窗口径：`[A-Za-z0-9_]+` 非空 + `world.maps` 重名检测；缺省 `city_{maps.len()}` /「新城 {n}」。
- 面积输入沿 seed 文本框口径：parse 失败保持旧值；下限 `MIN_CITY_AREA = 100.0` wu²。
- anchor 归一化 = 质心 ÷ 父图 extent（无 extent 用 `FALLBACK_EXTENT` 4096²，与 `icon_local` 同款），clamp 后交给 `AddCityCommand.icon_anchor`（其 apply 落 `IconSpec.anchor = Some`）。
- `extent.origin = 质心 − (w/2, h/2)`；`outline_polygon` = 局部矩形（旧口径）；`base` 缺省 Ocean。

## Review Focus

1. **对话框开着时选区被 Esc 清掉** → 确认必须仍可（质心 / 面积 / anchor 已预填进对话框，不读活选区）。
2. **自动钻入失败**（`drill.enter` false，如父图 children 异常）→ 建图成功但不能静默：notice 提示 + 不崩。
3. **父图无 extent**（世界图常态）→ anchor 用 FALLBACK_EXTENT 归一化，与 `icon_local` 渲染口径一致，图标不得漂到哈希位。
4. **双击容差 px 化**（延后 minor 清偿）→ `handle_click` 签名变化不得静默改变既有测试语义。

---

### Task 1: 几何纯函数（`ui/city_intent.rs` 骨架）

**Files:**
- Create: `crates/wv-app/src/ui/city_intent.rs`（模块头：职责/边界——几何 + 对话框 + 确认系统，不直接碰 History 之外的渲染态）
- Modify: `crates/wv-app/src/ui/mod.rs`（`pub mod city_intent;`）

**Interfaces（Task 3 逐字消费）:**

```rust
/// 多边形质心（面积加权）；退化（零面积）回退包围盒中心。
pub fn ring_centroid(ring: &[(f32, f32)]) -> (f32, f32);
/// 包络包围盒面积（wu²）。
pub fn ring_bbox_area(ring: &[(f32, f32)]) -> f32;
/// 面积 → 16:9 画布 (w, h)：w=√(A·16/9)，h=√(A·9/16)。
pub fn area_to_16_9(area: f32) -> (f32, f32);
```

- [ ] **Step 1: 失败测试**（同文件 tests）：

```rust
#[test]
fn square_ring_centroid_is_its_center()                  // (0,0)(10,0)(10,10)(0,10) → (5,5)
#[test]
fn degenerate_ring_centroid_falls_back_to_bbox_center()  // 共线三点 → bbox 中心
#[test]
fn ring_bbox_area_measures_envelope()                    // 上环 → 100
#[test]
fn area_maps_to_16_9_extent()                            // 14400 → (160, 90)；手算 √25600 / √8100
```

- [ ] **Step 2:** `cargo test -p wv-app --features app ring_ area_maps` → FAIL（函数不存在）。
- [ ] **Step 3:** 实现（鞋带公式质心；`f32` 精度足够）。
- [ ] **Step 4:** PASS → Commit：`wv-app: 城市意图几何纯函数（质心/包围盒面积/16:9 画布）`。

---

### Task 2: wv-render 顺手两件（FALLBACK_EXTENT 导出 + 双击容差 px 化）

**Files:**
- Modify: `crates/wv-render/src/drill.rs:63`（`const FALLBACK_EXTENT` → `pub const FALLBACK_EXTENT`）
- Modify: `crates/wv-render/src/selection.rs`（`handle_click` 加 `dbl_tol: f32` 参数；`DOUBLE_CLICK_WU` → `pub const DOUBLE_CLICK_PX`；`commit_selection_click` 传 `DOUBLE_CLICK_PX * view.scale`）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（无签名变化，不需改——Bevy 系统按 fn 推导）

**Interfaces:**

```rust
// selection.rs
pub const DOUBLE_CLICK_PX: f32 = 12.0;   // 双击位移容差（屏幕像素）
pub fn handle_click(sel: &mut SelectionState, p: (f32, f32), now: f64,
                    tol: f32, snap: f32, dbl_tol: f32);
```

- [ ] **Step 1:** 既有 selection 测试全部调用点补 `12.0` 实参 → 编译失败即 RED（签名变更）。
- [ ] **Step 2:** 改签名 + 调用点；删 `DOUBLE_CLICK_WU`（全库 rg 清零）。
- [ ] **Step 3:** 全套件 PASS（语义等价：旧测试传 12.0 = 原 12 wu）→ Commit：`wv-render: 双击容差 px 化 + FALLBACK_EXTENT 导出`。

---

### Task 3: `CityIntentDialog` 资源 + 预填 + 对话框 + 确认系统

**Files:**
- Modify: `crates/wv-app/src/ui/city_intent.rs`

**Interfaces:**

```rust
pub const MIN_CITY_AREA: f32 = 100.0;   // wu²，10×10 起步

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CityIntentDialog {
    pub open: bool,
    pub parent_map: Option<wv_core::Id>,
    /// 父图局部质心（确认时算 extent.origin 用）。
    pub centroid: (f32, f32),
    /// 钻入图标锚点（父图 extent 归一化，预填时算好并 clamp）。
    pub anchor: [f32; 2],
    pub id: String,
    pub name: String,
    pub base: wv_core::map::MapBase,
    /// 画布面积（wu²，可改；下限 MIN_CITY_AREA）。
    pub area: f32,
}

/// 从闭合选区预填（spec §5：质心落点 + 包围盒面积 + anchor 归一化）。
/// `parent_extent` = 父图 extent（None → FALLBACK_EXTENT 4096²，与 icon_local 同款）。
pub fn prefill(dialog: &mut CityIntentDialog, selection: &SelectionState,
               parent_map: wv_core::Id, parent_extent: Option<wv_core::map::Extent>,
               map_count: usize);

/// 确认系统：ID/重名校验 → extent/outline → AddCityCommand（icon_anchor 显式）
/// → commit → drill.enter → notice；失败 notice 且**窗保持开**（作者输入不丢）。
pub fn city_intent_dialog_system(/* EguiContexts, UiShellConfig, CurrentLocale, dialog,
    projection, edit, mut drill, grid, notice, time, startup_req */);
```

对话框字段沿用旧城市弹窗 + continent_dialog 口径：ID 文本框 + charset/重名校验、名字、底图 `selectable_value`（海洋/陆地）、面积文本框（parse 失败保持旧值，参照 `parse_seed_text`）。

- [ ] **Step 1: 失败测试**：

```rust
#[test]
fn prefill_fills_centroid_area_and_default_ids()      // 方形环 → 质心 (5,5)、area 100、id city_3、名「新城 3」
#[test]
fn prefill_normalizes_anchor_against_parent_extent()  // extent 100×200、质心 (50,50) → anchor [0.5, 0.25]
#[test]
fn prefill_uses_fallback_extent_when_parent_has_none()// 无 extent → ÷4096
#[test]
fn area_input_parse_failure_keeps_previous_value()    // parse_area_text 同 parse_seed_text 口径
```

- [ ] **Step 2:** FAIL → **Step 3:** 实现（确认路径按 Interfaces 注释建 `MapNode`：`level=City`、`parent`、`extent{w,h,origin=质心−(w/2,h/2)}`、`outline_polygon=局部矩形`、`base`；`AddCityCommand::new` 后覆写 `icon_anchor`）。
- [ ] **Step 4:** PASS → Commit：`wv-app: 生成城市对话框（面积预填 + 16:9 画布 + 显式锚点）`。

---

### Task 4: 意图菜单接线 + i18n

**Files:**
- Modify: `crates/wv-app/src/ui/intent_menu.rs`（目录 → 2；「生成城市…」按钮 → `prefill` + `dialog.open = true`；参数加 `city_dialog: ResMut<CityIntentDialog>`）
- Modify: `crates/wv-app/src/i18n/keys.rs` + `table.rs`（`IntentMenuGenerateCity`：zh「生成城市…」/ en "Generate city…"）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（`.init_resource::<CityIntentDialog>()` + `ui::city_intent::city_intent_dialog_system` 挂 `EguiPrimaryContextPass`，`.after(ui::intent_menu)`——链上限 20，单独注册）

- [ ] **Step 1:** 更新既有断言 `intent_catalog_nonempty_for_world_and_region`：World/Region → **2**（FAIL）。
- [ ] **Step 2:** 实现按钮 + i18n + 注册。
- [ ] **Step 3:** 六门禁 + `smoke-gui.sh` 全绿。
- [ ] **Step 4:** Commit：`wv-app: 意图菜单生成城市入口（选区预填 + 自动钻入）`。

---

### Task 5: spec 追记 + 收尾

- [ ] spec §M1 追记后追加 **M2 完成注记**（验收 4 对应项：自动钻入 / 质心锚点 / 16:9）。
- [ ] 六门禁复跑 → Commit：`docs: 选择工具 M2 完成追记`。
- [ ] 台账收口 + 工作区清理。

## Self-Review 记录

- spec §5 六步 → Task 3（对话框/确认）/ Task 4（入口）；验收 4 → Task 3/4；Review Focus 1-3 → Task 3 预填与确认设计、4 → Task 2。
- 签名一致：`prefill`/`area_to_16_9`/`handle_click` 新参在定义与消费处逐字核对。
- 比例：约为 M1 计划的一半，代码块仅签名 + 测试断言。
