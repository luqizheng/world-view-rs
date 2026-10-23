# 三模式内容矩阵实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Browse/Edit/CellMap 各看各的——CellMap 接线既有 MODE_TABLE 关掉全部内容层并改用群系固定色块底图；Edit 追加隐藏事件与区域层；CellMap 下 Inspector 只留属性页签。

**Architecture:** 门控车辆 = ADR-0021 的 `RenderRecipe::MODE_TABLE`（`recipe.enabled(PassId)`）：已声明未消费的系统逐一接线；新语义（Schematic 藏事件、区域层与装饰分离）通过翻表 + 新 `PassId::RegionLayers` 进入同一张表。地形画法新增 `shade_biome_raw`（21 群系固定色板，theme 无关）。

**Tech Stack:** Rust / Bevy 0.19 / wv-render（recipe、drill 标签池、vector_layers、terrain_view）/ egui 0.36（wv-ui tabs）。

**Spec:** `docs/superpowers/specs/2026-10-22-mode-content-matrix-design.md`（v1.2；执行者须先读，§2 矩阵 + §6 三处裁定）

## Global Constraints

- 分支策略：**基于 `feat/unified-inspector` 叠分支**（U1 依赖新 Inspector）：`git branch feat/mode-content-matrix feat/unified-inspector` + worktree `.worktrees/feat/mode-content-matrix`。合并顺序：先 unified-inspector 后本支。
- 门禁六件套（AGENTS §7.1）+ `bash scripts/smoke-gui.sh`；每任务后跑（复用 gates.sh 模式：worktree 内独立 `CARGO_TARGET_DIR`；app 测试失败先重跑一次——`new_map_dialog` 有已知的负载抖动）。
- wv-render 依赖红线：`cargo tree -p wv-render | grep -E 'wv-edit|wv-app'` 每任务末跑，必须为空。
- 提交格式 `<scope>: <动作>`；worktree 内提交。
- 渲染层不做时间比较（AGENTS §3.2）——一切门只读 `view.mode` / recipe。

## Review Focus

1. **标签池冻结残留**：标签实体是**持久池**（隐藏非 despawn，drill.rs:971）。门控若只「跳过注册」会让旧标签冻结在屏上——必须让 false 走「wanted 置空」的既有隐藏尾部。→ Task 3 测试：Topographic 下跑系统后池实体全 `Hidden` 且 `LabelHits` 空。
2. **`hits.clear()` 归属**：`update_place_labels` 是标签链链首、独占清场；门控不得跳过它（否则别家追加的命中成 stale）。→ Task 3 测试断言。
3. **翻表的锚定测试**：MODE_TABLE 有「行数 = ViewMode::ALL」「行长 = PassId::COUNT」锚定测试；翻任何格 / 加 PassId 都要同步。→ Task 6/7 显式步骤。
4. **RegionLayers 拆分不破坏装饰**：region_outline/overlay 现与 terrain_decor 共享 PassId::TerrainDecor（Schematic=装饰 ✓ 区域 ✗ 的唯一障碍）。拆分后 Schematic 下装饰照画、区域消失。→ Task 7 测试。
5. **色板 theme 无关**：`BIOME_RAW_COLORS` 不得经 `PaletteColors` / `TreatmentFlags`。→ Task 2 测试：同一 terrain 数据在两套 palette 下 `shade_biome_raw` 输出逐位相同。
6. **候选枚举的代价**：全图连通区是 O(cells)，每帧跑会吃帧——必须走指纹缓存（Task 11 测试钉死）；Area 区/已命名 Nature 区覆盖的格不得出现在候选里。
7. **分界线的染色越界**：扫过格里第三种群系（水/山）被误染 = 数据破坏。→ Task 12 纯函数测试双向 + 第三方不动三种情形；预览必须与提交同一换算函数（预览说什么、松手改什么，不得两套代码）。

---

### Task 1: 分支 + worktree + 基线

**Files:** 无代码改动。

- [ ] Step 1: `git branch feat/mode-content-matrix feat/unified-inspector`；`git worktree add .worktrees/feat/mode-content-matrix feat/mode-content-matrix`；把本计划文件复制进 worktree 同路径并提交（`docs: 三模式内容矩阵实施计划`）。
- [ ] Step 2: 基线门禁（gates.sh 模式）全绿；磁盘余量确认（`df -h /d` < 90% 时先报告）。

### Task 2: 群系色块底图 `shade_biome_raw`（spec §2.5 / T1）

**Files:**
- Modify: `crates/wv-render/src/terrain_view/cells.rs`（`shade_topographic` 分支旁新增；Topographic 调用点 :126 改走新分支）
- Test: 同文件 `mod tests`

**Interfaces:**
- Produces: `pub const BIOME_RAW_COLORS: [egui::Color32; 21]`（索引 = `wv_terrain::biome` 冻结编号；同族同色相 + 序号明度阶梯；NONE 不在此表）+ `fn shade_biome_raw(data: &TerrainData, kind: u8) -> egui::Color32`——**签名无 `pal`**（编译期保证 theme 无关）。biome=0（NONE）按 surface 回退：水 → 固定水蓝、陆 → 固定灰米（两个常量）。
- 语义：Topographic 分支整体改走它——无浮雕、无晕开、无 TreatmentFlags、高度不参与。

- [ ] Step 1: 失败测试——`biome_raw_colors_cover_all_21_biomes`（长度锚定 + 无重复色）；`shade_biome_raw_is_theme_independent`（同一 data 在构造的两个不同 `PaletteColors` 下输出相同——经一个不吃 pal 的包装保证）；`shade_biome_raw_falls_back_by_surface_when_biome_none`。
- [ ] Step 2: 跑红（类型不存在）。
- [ ] Step 3: 实现（21 色定值写死 + 注释对照 `wv-terrain/src/biome.rs` 族表）。
- [ ] Step 4: 跑绿 + 全门禁。
- [ ] Step 5: 提交 `wv-render: Topographic 改群系固定色块底图（theme 无关 · spec 2026-10-22 §2.5）`

### Task 3: 五套标签系统接线 + SeaLabels 翻表（A2 / C1）

**Files:**
- Modify: `crates/wv-render/src/drill.rs`（`update_place_labels` / `update_faction_labels` / `update_event_labels` / `update_region_labels` / 海名系统）
- Modify: `crates/wv-render/src/streets.rs`（`update_street_labels`）
- Modify: `crates/wv-render/src/recipe.rs`（Topographic 行 SeaLabels false→**true**）
- Test: drill.rs / streets.rs 既有测试模块

**Interfaces:**
- 消费：`recipe.enabled(PassId::PlaceLabels / Faction / EventDots / RegionLayers·暂用 TerrainDecor / StreetLabels / SeaLabels)`。事件标签用 EventDots 格、区域标签暂挂 TerrainDecor（Task 7 迁 RegionLayers）。
- 语义：**disabled ⇒ wanted 置空**（走池的既有隐藏尾部），**不得**直接 return（池冻结残留）；`update_place_labels` 的 `hits.clear()` 照常执行。

- [ ] Step 1: 失败测试——`place_labels_pool_hidden_and_hits_cleared_under_topographic`（构造 Topographic recipe + 池实体 → 跑系统 → 池全 Hidden + `LabelHits` 空；照 drill.rs 既有 fixture 模式）。
- [ ] Step 2: 跑红。Step 3: 五系统逐一接线（头部查表置空 wanted；place 系统保持清场）。Step 4: 跑绿。
- [ ] Step 5: 翻 SeaLabels 格（Topographic true）+ 同步锚定测试 + 海名接线（true ⇒ 照常画）。
- [ ] Step 6: 全门禁 + 提交 `wv-render: 标签系统接线 MODE_TABLE，海名保留于 Topographic`

### Task 4: 街网 / POI 接线（A1）

**Files:**
- Modify: `crates/wv-render/src/vector_layers.rs`——`sync_street_layers`（Streets）、`sync_poi` + `sync_landmarks`（Poi）

**Interfaces:** disabled ⇒ 本帧不重建/隐藏既有 mesh（照 faction_overlays :122-125 的 despawn 先例）。

- [ ] Step 1: 失败测试（照 vector_layers 既有 fixture：Topographic 下不产 mesh / 既有 mesh 被清）。Step 2: 红。Step 3: 接线。Step 4: 绿 + 门禁。
- [ ] Step 5: 提交 `wv-render: 街网与 POI 接线 MODE_TABLE`

### Task 5: 事件点 gizmo + 钻入标记接线（A3 / A4）

**Files:**
- Modify: `crates/wv-render/src/world_projection.rs:684` 事件 gizmo——门 = `layers.event_point.visible ∧ recipe.enabled(PassId::EventDots)`
- Modify: `crates/wv-render/src/drill.rs`——`draw_drill_markers` 消费 DrillMarkers；`drill_pick`（:493）头部加 `!recipe.enabled(PassId::DrillMarkers)` 早退（标记不画 ⇒ 也不可钻）

- [ ] Step 1: 失败测试（可提取纯判定 `drill_marker_visible(layers, recipe)` / gizmo 同款；drill_pick 的早退走系统级，行为由 V2 实机验收兜底）。Step 2: 红。Step 3: 实现。Step 4: 绿 + 门禁。
- [ ] Step 5: 提交 `wv-render: 事件点与钻入标记接线 MODE_TABLE，Topographic 禁钻入`

### Task 6: EventDots Schematic 翻表（B1）

**Files:**
- Modify: `crates/wv-render/src/recipe.rs` Schematic 行 EventDots true→**false**（:210 第 18 格）+ 锚定/语义测试

- [ ] Step 1: 失败测试——`schematic_hides_event_layer`（断言 Schematic 行 EventDots=false；同时断言 VectorFlat 行仍 true）。Step 2: 红。Step 3: 翻格。Step 4: 绿 + 门禁。
- [ ] Step 5: 提交 `wv-render: Schematic 隐藏事件内容层（用户裁定：事件有时间性）`

### Task 7: 拆 `PassId::RegionLayers`（B2）

**Files:**
- Modify: `crates/wv-render/src/recipe.rs`——PassId 追加 `RegionLayers`（COUNT 24→25，MODE_TABLE 全行加格：VectorFlat/RealTerrain/GoogleMap/Faction/Event 等 = true；**Schematic = false**；**Topographic = false**；与 TerrainDecor 各行取值一致除上述两处）
- Modify: `crates/wv-render/src/region_outline.rs` / `region_overlay.rs`——TerrainDecor 格改读 RegionLayers
- Modify: `crates/wv-render/src/drill.rs` `update_region_labels`——从 TerrainDecor 改挂 RegionLayers
- Modify: `docs/MAP_LAYER_RENDERING_SPEC_v0.1.md`——PassId 表补条目（Z 槽位不变：轮廓 0.076 / 底色 0.064）

- [ ] Step 1: 失败测试——`region_layers_hidden_in_schematic_but_decor_visible`（Schematic 行：TerrainDecor=true ∧ RegionLayers=false；Topographic 行两者皆 false；VectorFlat 两者皆 true）+ 行长锚定自动生效。
- [ ] Step 2: 红。Step 3: 加 PassId + 全行补格（照每行现有 TerrainDecor 值复制后改 Schematic/Topographic）+ 三系统迁移。Step 4: 绿 + 门禁。
- [ ] Step 5: 提交 `wv-render: 区域层拆独立 PassId::RegionLayers，Schematic/Topographic 隐藏`

### Task 8: Inspector 页签禁用（U1）

**Files:**
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`——页签条 `disabled` 从 `&[]` 改 `inspector_disabled_tabs(panels.cell_workspace_backup.is_some())`

**Interfaces:** `fn inspector_disabled_tabs(workspace_active: bool) -> &'static [usize]`——true ⇒ `&[1, 2]`，false ⇒ `&[]`。

- [ ] Step 1: 失败测试 `inspector_tabs_disabled_only_in_cell_workspace`。Step 2: 红。Step 3: 实现（一行调用点）。Step 4: 绿 + 门禁（含冒烟：CellMap 进工作区页签变灰）。
- [ ] Step 5: 提交 `wv-app: CellMap 工作区下 Inspector 仅属性页签可点`

### Task 9: ADR-0051 + spec + GLOSSARY（D1）

**Files:**
- Create: `docs/adr/ADR-0051-mode-content-matrix.md`（处境/决策/理由/代价 + 被否决备选：散布 view.mode if / RegionLayers 不拆靠图层开关 / 色板进 ThemePack）；**修正 ADR-0039**（Topographic 视觉语义 = 群系原始数据视图，等高线降参照层）+ **翻 ADR-0021 两格**（SeaLabels-Topo、EventDots-Schematic）——头部标注被修正关系
- Modify: `docs/adr/README.md` 索引、`docs/adr/ADR-0021-*` 与 `ADR-0039-*` 头部标注（不改正文）、`GLOSSARY.md`（新词条：群系色块底图 / 原始数据视图 / 内容层门控）

- [ ] Step 1: 写 ADR + 更新三处文档。Step 2: 门禁（文档不入编译，跑快门禁即可）。
- [ ] Step 3: 提交 `docs: ADR-0051 三模式内容矩阵（修正 ADR-0021 两格 / ADR-0039 视觉语义）`

### Task 10: CellMap 自然区开关 + Nature 放行（N1）

**Files:**
- Modify: `crates/wv-render/src/state.rs`——`ViewState` 加 `pub nature_overlay: bool`（会话态，缺省 false）
- Modify: `crates/wv-app/src/ui/map_controls.rs`——grid 按钮（:488-493）旁新增 Nature 开关（Lucide 图标 + tooltip），点击翻转 `view.nature_overlay`
- Modify: `crates/wv-render/src/region_outline.rs` / `region_overlay.rs`（Task 7 的 RegionLayers 消费点）——Topographic 分支改判：`view.nature_overlay ∧ region.kind == Nature` 才画（Area 区在 CellMap 永远隐藏；其他模式不受开关影响）

**Interfaces:**
- Produces: `ViewState.nature_overlay: bool`；RegionLayers 渲染条件 = `recipe.enabled(RegionLayers) ∧ (mode ≠ Topographic ∨ (nature_overlay ∧ kind == Nature))`。
- 依据：spec §2.6-1/2（用户裁定：自然区属「自然」，CellMap 可见；Area 属内容，仍藏）。

- [ ] Step 1: 失败测试——`region_layers_in_topographic_only_nature_when_overlay_on`（Topographic + overlay on：Nature 区画、Area 区不画；overlay off：全不画；Schematic 不受开关影响照旧藏）。照 region_overlay 既有 fixture 模式。
- [ ] Step 2: 红。Step 3: 实现三处。Step 4: 绿 + 门禁。
- [ ] Step 5: 提交 `wv-render,wv-app: CellMap 自然区开关（map controls 与 grid 并列，仅放行 Nature 类）`

### Task 11: 全图候选枚举 + 候选命名表单（N2 / N3）

**Files:**
- Modify: `crates/wv-render/src/region_overlay.rs`——开关开启（仅 Topographic）时全图枚举未命名候选（复用 `isoline::kind_segments` + `assemble_chains`，ADR-0045 D1 同源），画细虚线金框（既有候选金同色系）；按地形指纹缓存（先例 `EditCellState.region_fingerprint`），地形编辑即失效；候选排除已被命名 Nature 区覆盖的格
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`——分派优先级插入「候选命名」：Cell 工具激活 > **候选命名**（`nature_overlay ∧ 点中候选`，上下文存既有 `NatureRegionUiState`）> 实体表单 > 空态
- Create: `crates/wv-app/src/ui/inspector/nature_candidate.rs`——候选命名表单（名字输入 + 物化 + 丢弃；物化走既有 `materialize_nature_region_now`，成功后清上下文 + 失效候选缓存）
- Test: 各文件 tests

**Interfaces:**
- Produces: `fn candidate_naming_form(ui, shell, ctx: &CandidateContext, projection, edit, view, grid, notice, current_loc)`；`CandidateContext { map: Id, cell: (i32,i32), biome: u8, depth: usize }`（照 `candidate_at` 返回值 + 定位）。
- 依据：spec §2.6-3/4（未命名不落盘 = ADR-0045 D4 既有；物化即落盘）。

- [ ] Step 1: 失败测试——`candidate_cache_invalidates_on_terrain_fingerprint_change`（纯函数：指纹变 → 重算）；`candidate_naming_form_materializes_via_existing_action`（stub EditHistory：物化按钮 → `materialize_nature_region_now` 被调 + 上下文清空）。
- [ ] Step 2: 红。Step 3: 实现（枚举放 region_overlay 渲染侧；表单动作走 `nature_region_actions`，UI 不直接改世界）。Step 4: 绿 + 门禁 + 冒烟。
- [ ] Step 5: 提交 `wv-render,wv-app: 全图候选高亮 + 候选命名表单（命名即物化，未命名不落盘）`

### Task 12: 群系分界线工具（F1 / spec §2.7）

**Files:**
- Modify: `crates/wv-app/src/state.rs`——`ToolKind::BiomeBoundary` 变体（顶层，照 ADR-0040 先例；`MapToolId` + 工具条图标 Lucide `slash` 落盘 `crates/wv-ui/assets/ui/tool_icons/`）
- Modify: `crates/wv-render/src/state.rs`——`BiomeBoundaryState`（会话态：`active: bool`、`line: Option<Line>`、`Line { a: (f32,f32), b: (f32,f32), side_a: u8, side_b: u8 }`、拖动中标志；`on_map_change` 清空，照 `EditCellState` 同款）
- Modify: `crates/wv-render/src/recipe.rs`——新会话轴 `PassId::BiomeBoundary`（COUNT +1，全行 true——照 CellSelection 同款：表全开、由 `tool_active` 控制实际画不画）
- Create: `crates/wv-app/src/edit/biome_boundary.rs`——输入系统（创建：工具激活 + 地图拖拽出新线段；拖线身平移 / 拖端点旋转伸缩；PointerOverUi gate 照 `commit_edit_cell_on_click`）+ **侧向换算纯函数** `swept_changes(old: &Line, new: &Line, data: &TerrainData) -> Vec<CellChange>`（扫掠多边形 = 新旧线段凸包，复用 `cell_selection::rasterize_polygon_cells` 栅格化；格子按旧线叉积定侧 → `biome == 退让侧` 才改；第三种群系不动）+ 提交（1 条 `BatchEditCellsCommand` 直提 + `mark_dirty` + `projection.dirty`，照 `push_cell_edit` 收尾）
- Modify: `crates/wv-render/src/`——预览渲染系统（新 pass：扫过格半透高亮 + 线段本体两侧各染侧色）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`——分派优先级：EditCell > **BiomeBoundary 表单**（两侧群系下拉 + 「清除此线」）> 候选命名 > 实体表单 > 空态
- Test: `biome_boundary.rs` 内测纯函数

**Interfaces:**
- `swept_changes` 契约：扫过格 = 新旧线段四端点凸包内的格；`old_side = side_of(old_line, cell)` 的群系 ≠ `new_side` 的群系且 `cell.biome == old_side 群系` ⇒ 改为 new_side 群系；其余（含第三种群系、边界恰在线上的格）不动。
- 一次拖动（press→release）= 恰 1 条命令；无扫过变更（线拖回原位）= 0 条。

- [ ] Step 1: 失败测试——`swept_changes_converts_only_receding_side_biome`（A|B 互转，第三种群系不动）；`swept_changes_symmetric_both_drag_directions`；`swept_changes_empty_when_line_returns_to_start`；`boundary_drag_commits_single_batch_command`（stub History 断言 1 条 + dirty 标记）。
- [ ] Step 2: 红。Step 3: 实现（顺序：状态与 pass → 纯函数 → 输入/提交 → 预览渲染 → Inspector 表单 → 工具条图标）。Step 4: 绿 + 全门禁 + 冒烟。
- [ ] Step 5: 实机：创建线 → 自动探测两侧群系（一个森林/草原边界上）→ 拖 3 次 → 撤销 3 次逐级回退、地图逐级重绘。
- [ ] Step 6: 提交 `wv-app,wv-render: 群系分界线工具（拖动推移边界，批改格子群系，可撤销）`

---

## 收尾

- [ ] 实机验收 V1–V7（spec §5）；V7 需播放时间轴（Browse 下事件标签仍随时间投影）。
- [ ] 整支 fresh review → 修 Critical/Important → 合并顺序：先 `feat/unified-inspector` 后本支（或按用户指示）。
