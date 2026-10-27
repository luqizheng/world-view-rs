# 势力图切蛋糕分区 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 势力图层（国界/贵族/宗教……）支持零和「切蛋糕」分区：图层面板每层眼睛、自然图关 = 素底政治图集页、pen→弹框（新增填生效时间/修改选段跳转）→ 图上切刀/拖边界/整块易主，全部落 span 随时间轴求值。

**Architecture:** 快照式分区数据（每段历史 = 一份完整拓扑快照：共享顶点表 + 脸环引用，零和由共享顶点构造保证）落 wv-core，YAML 经 wv-io 读写，编辑经 wv-edit 五个命令（Begin/Cut/Move/Assign/层可见性复用既有），wv-render 每帧按 `PlaybackState.current_day` 求值渲染（活动层填色、其余层画边），wv-app 出 pen 弹框 + 编辑 HUD（地图属性含当前时间选择器）+ 三手势。

**Tech Stack:** Rust 2021 / Bevy 0.19 + egui 0.36（wv-ui 控件）/ serde_yaml。无新第三方依赖。

**Spec:** `docs/superpowers/specs/2026-10-27-faction-map-partition-design.md`（D1–D9 裁定 + 设计默认，执行者必读）

## Global Constraints

- 分支 `feat/faction-partition-map`，worktree `.worktrees/feat/faction-partition-map`（AGENTS §13）；**共享全局 `CARGO_TARGET_DIR`**（2026-10-25 裁定，勿在 worktree 覆盖）。
- 依赖红线：wv-core 零 Bevy/网络/fs；wv-edit 只依赖 wv-core+wv-terrain；wv-render 不依赖 wv-edit/wv-app；System 不直改世界（一律 EditCommand→History→dirty）。
- P3：分区拓扑走 YAML（flat f32 坐标），禁二进制；P4：`owner` 只存 Faction `Id` 引用（可空 = 无主，D-无主裁定）；P6/P7：只存 span 化状态，禁「当前划分」字段；V1：校验只提示不阻止；V2：改旧不质问。
- 坐标口径：map-local 平面坐标、flat f32 `[x0,y0,x1,y1,...]`（与 `GeometryData::Polygon` 同款）；⚠️ 本仓库两套原点陷阱（map_origin vs raster_origin），分区数据一律 map-local。
- 时间：天数 `i64`，span 半开 `[from, to)`，`to==None` 至今（`wv_core::time::TimeSpan` 同口径）。
- 门禁：`cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check`；渲染/UI 改动追加 `cargo app-test` / `cargo app-clippy` / `bash scripts/smoke-gui.sh`。受限环境跑测试：worktree 内勿用 `cargo-test.sh`，先 `export TEMP=<主checkout>/.target/testtmp` 再 `cargo test`。
- 图标一律 Lucide（`crates/wv-ui/assets/ui/tool_icons/` kebab-case，描边烘焙 `stroke="#ffffff"`）；新 UI 文案一律 I18nKey（zh/en 双语，注意 Key 计数常量）。
- 测试名完整英文短句；数值断言附实际值；提交信息 `<scope>: <动作>` 中文。

## Review Focus

1. **改旧段传染后段**：修改历史段必须只改选中 span 的快照，后段求值不变 → T7/T9 测试「三段历史改中段，前后段 day 求值不变」。
2. **切刀未边到边**：笔画端点不在面边界上必须报错不落盘，否则产出破脸 → T8 测试「端点在面内部 → Err」。
3. **拖非共享顶点破坏零和**：被移动顶点必须被 ≥2 张脸引用（或显式单脸模式），否则单侧拉出缝 → T9 测试「moves 含仅 1 脸引用的顶点 → Err」。
4. **day 落在首段之前 / 段间隙**：`partition_span_at` 返回 `None` 不 panic（渲染画空）→ T2 测试。
5. **海岸线变动后快照错配静默**：改地形后校验面板必须列出海陆错配（不阻止保存）→ T13 测试。

---

### Task 1: ADR-0074 + GLOSSARY 术语

**Files:**
- Create: `docs/adr/ADR-0074-faction-partition-cake-cutting.md`
- Modify: `docs/adr/README.md`（索引；标注 ADR-0028 的划分势力入口被本 ADR 取代）
- Modify: `GLOSSARY.md`

**Interfaces:** 无代码。后续任务的术语以本 ADR 为准。

- [ ] **Step 1: 写 ADR-0074**：处境（ADR-0027 层已有、无画图工具、独立多边形无零和语义）/ 决策（快照式分区 = 共享顶点表+脸环引用；五命令；切后点选归属；整块易主=关旧 span 开新 span；claim 不再可新建；ADR-0028 入口退役）/ 理由 / 代价 + 被否决备选（操作日志式 span、独立多边形+几何吸附、显式边表 DCEL、编辑态第二时钟）。落 spec 的 D1–D9 与设计默认。
- [ ] **Step 2: GLOSSARY 登记**：分区（partition）、脸（face）、边界网络（共享顶点）、切蛋糕、政治图集页、整块易主、生效时间。
- [ ] **Step 3: Commit** `git add docs && git commit -m "docs: ADR-0074 势力图切蛋糕分区 + 术语登记"`

### Task 2: wv-core 分区数据模型 + 求值查询

**Files:**
- Create: `crates/wv-core/src/model/faction_partition.rs`
- Modify: `crates/wv-core/src/lib.rs`（`World.partitions` 字段，`#[serde(default)]`，位置挨着 `faction_geometry`；mod 声明 + re-export）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces（后续任务全部依赖）:
  - `pub struct FactionPartition { pub map: Id, pub layer: Id, pub spans: Vec<PartitionSpan> }`
  - `pub struct PartitionSpan { pub from: i64, pub to: Option<i64>, pub vertices: Vec<PartitionVertex>, pub faces: Vec<PartitionFace> }`
  - `pub struct PartitionVertex { pub id: u32, pub xy: [f32; 2] }`
  - `pub struct PartitionFace { pub id: u32, pub owner: Option<Id>, pub ring: Vec<u32>, #[serde(default, skip_serializing_if)] pub holes: Vec<Vec<u32>> }`（ring/holes 存顶点 id，首尾不重复）
  - `pub fn partition_span_at<'a>(world: &'a World, map: &Id, layer: &Id, day: i64) -> Option<&'a PartitionSpan>`（`from <= day < to`；`to==None` 至今；无命中/无分区 → None）

- [ ] **Step 1: 写失败测试**：`span_query_returns_none_before_first_span_and_in_gaps`（构造两段 `[0,10)` `[20,None)`，断言 day=-1/15 → None、day=0/9 → 第一段、day=20 → 第二段）；`partition_yaml_round_trips_with_skip_defaults`（owner None / holes 空时字段 skip，`serde_yaml` 往返相等）；`face_ring_vertex_ids_must_exist` 构造辅助校验 fn `PartitionSpan::validate_topology() -> Result<(), String>`（ring 引用的顶点 id 存在、ring ≥3 点、span 区间不重叠且升序）。
- [ ] **Step 2: 跑测试确认失败**（类型未定义）。
- [ ] **Step 3: 实现模型 + 查询 + `validate_topology`**；`World` 加字段。ring 顶点 id 语义写进类型文档注释（共享 = 多脸引用同一 id，零和的构造基础）。
- [ ] **Step 4: 跑 `cargo test -p wv-core`** 全绿。
- [ ] **Step 5: Commit** `wv-core: 分区数据模型——共享顶点表+脸环引用+span 求值查询`

### Task 3: wv-io 分区落盘/读盘

**Files:**
- Modify: `crates/wv-io/src/project.rs`（save 分支 + load 分支）
- Test: project.rs 既有测试文件旁新增

**Interfaces:**
- Consumes: T2 类型。落盘位置：`maps/<map_id>/partitions/<layer_id>.yaml`（每分区一文件，P2 目录即工程）。
- Produces: dirty 键 `"partitions"`（命令返回此 dirty 值，saver `n("partitions")` 命中时全量重写该图 partitions 目录）。

- [ ] **Step 1: 失败测试**：`partition_save_load_round_trips`（World 塞 1 图 1 层 2 段 → save → 重 load → `world.partitions` 相等）；`missing_partition_files_silently_default`（无 partitions 目录 → 空Vec 不报错）。
- [ ] **Step 2-4: 实现 + 验证**（save/load 配方抄 `geometry/faction` 分支：删除多余文件 + 逐分区写 YAML）。
- [ ] **Step 5: Commit** `wv-io: partitions/ 逐层落盘与读回`

### Task 4: 图层面板——势力图层组眼睛 + 自然图声明化

**Files:**
- Modify: `crates/wv-edit/src/command/overlay.rs`（`UpdateLayerCommand::ensure_terrain_visibility(map, visible)` 构造器：图无 terrain 声明层时插入 `layer_terrain` 声明，有则改 visible）
- Modify: `crates/wv-app/src/ui/layers_panel.rs`
- Test: overlay.rs 命令测试 + layers_panel 逻辑测试（feature=app）

**Interfaces:**
- Consumes: `UpdateFactionLayerCommand`（wv-edit/src/command/faction.rs，全字段快照命令——调用侧先读现值再改 `new_visible`，read-modify-write，同 `UpdateLayerCommand::visibility` 配方）；`wv_core::resolved_faction_layers(&world)`。
- Produces: 面板新增「势力图层」组（声明轴与会话轴之间）：每层一行 `名字 + 眼睛`，眼睛 → `UpdateFactionLayerCommand`；「自然图」行 → `ensure_terrain_visibility`；**会话轴 8 开关里的 `Terrain` 行退役**（`terrain_soften` 保留）。

- [ ] **Step 1: 失败测试**（wv-edit）：`ensure_terrain_visibility_inserts_declaration_when_absent` / `..._updates_existing_row`。
- [ ] **Step 2: 实现 ensure 构造器**，测试绿。
- [ ] **Step 3: 面板改造**：势力层组数据 = `resolved_faction_layers`；眼睛 toggle 走 `commit_layer` 同款事务配方；自然图行替换会话 Terrain 行；新增 i18n Key（组标题/自然图）。
- [ ] **Step 4: 验证**：`cargo app-test` + 手验：眼睛落盘到 `ui`无关的 YAML（faction 层 visible / map layers），重开工程保留。
- [ ] **Step 5: Commit** `wv-app,wv-edit: 图层面板势力层组眼睛 + 自然图声明化落盘`

### Task 5: wv-render 政治图集页素底

**Files:**
- Modify: `crates/wv-render/src/map_frame.rs`（terrain 隐藏分支）+ 相关 recipe/pass
- Test: app-test 下渲染单测（现有 map_frame 测试模式）

**Interfaces:**
- Consumes: `ViewState.terrain_visible`（map_frame.rs:97 现有门）。
- Produces: `terrain_visible == false` 时地形 chunk 不 despawn 而是画**平面海陆两色**（海色/陆色走 CellPalette/MapPalette 既有令牌），海岸线沿用既有海岸绘制；其余内容层不动（设计默认 5）。

- [ ] **Step 1: 失败测试**：`hidden_terrain_renders_flat_land_sea_base`（terrain_visible=false 时 chunk 材质为双色平面配方而非 despawn）。
- [ ] **Step 2: 实现**（最小改法：隐藏时把 chunk 着色换成 sea/land 平色——高度/群系纹理跳过）。
- [ ] **Step 3: `cargo app-test` 绿 + smoke-gui**。
- [ ] **Step 4: Commit** `wv-render: 自然图关=素底政治页——平面海陆底+海岸线保留`

### Task 6: wv-terrain 陆地外环追踪

**Files:**
- Create: `crates/wv-terrain/src/land_outline.rs`（mod 声明进 lib.rs）
- Test: 同文件

**Interfaces:**
- Produces: `pub fn land_outlines(grid: &TerrainGrid) -> Vec<Vec<[f32; 2]>>` —— 海陆掩膜 → 陆地多边形外环列表（含洞：湖=洞，格点级精度、不做简化）；坐标 **map-local**（cell 尺寸换算 + y 翻转在函数内完成，常量与 raster 元数据同源；⚠️ 两套原点陷阱，禁用 raster_origin 口径输出）。判定：高度 ≥ 海平面（与 wv-terrain 既有海平面常数同源，禁第二份硬编码）。

- [ ] **Step 1: 失败测试**（手算期望）：`single_land_cell_produces_unit_square_ring`（1 格陆地 → 正方环 4 点，坐标值手算写入断言）；`island_with_lake_emits_hole`；`all_sea_yields_empty`。
- [ ] **Step 2-4: 实现（cell 边界追踪/marching-squares 均可，测试定标准）+ `cargo test -p wv-terrain` 绿**。
- [ ] **Step 5: Commit** `wv-terrain: land_outlines 海陆掩膜→map-local 陆地外环（格点级）`

### Task 7: wv-edit BeginPartitionSpanCommand

**Files:**
- Create: `crates/wv-edit/src/command/partition.rs`（mod 进 command.rs；re-export）
- Test: partition.rs 内 + command_tests.rs 注册

**Interfaces:**
- Consumes: T2 类型、T6 `land_outlines`（wv-edit 可依赖 wv-terrain ✓）。
- Produces: `pub struct BeginPartitionSpanCommand { pub map: Id, pub layer: Id, pub from: i64, pub initial_owner: Option<Id> }` — 两分支：①该层无任何 span → 用 `world.terrain_of(map)` 的 `land_outlines` 造首段（最大外环为初始脸，湖为洞；`initial_owner` 可空=无主）；②已有 span → 在 `from` 处 fork：命中段 `[a,b)` 拆成 `[a,from)` + 新段 `[from,b)`（快照复制，顶点/脸 id 保持）。dirty = `"partitions"`。

- [ ] **Step 1: 失败测试**：`first_span_is_whole_land_with_lake_hole`（fixture 栅格手算期望环数/洞数）；`fork_closes_old_span_and_copies_snapshot`（两段历史 fork 中间日，断言旧段 to=from、新段顶点 id 与原段相等）；`fork_day_before_first_span_creates_empty_partition_errors`（→ Err，不落盘）。
- [ ] **Step 2-4: 实现 + `cargo test -p wv-edit` 绿**。
- [ ] **Step 5: Commit** `wv-edit: BeginPartitionSpan——首段整块地/fork 快照`

### Task 8: wv-edit CutPartitionFaceCommand

**Files:**
- Modify: `crates/wv-edit/src/command/partition.rs`
- Test: 同文件

**Interfaces:**
- Consumes: T2。Produces: `pub struct CutPartitionFaceCommand { pub map: Id, pub layer: Id, pub span_from: i64, pub face: u32, pub stroke: Vec<f32>, pub assign_at: [f32; 2], pub new_owner: Option<Id> }` — **就地改选中 span**（改旧语义，V2）：笔画两端须落在该脸 ring 边上（吸附容差 4.0 map-local px；命中已有顶点则复用 id）；沿笔画把 ring 分成两环（新脸分配新 id）；`assign_at` 点落在哪半，那半 owner = `new_owner`，另一半保持原 owner。任一校验失败 → Err 不落盘。dirty = `"partitions"`。

- [ ] **Step 1: 失败测试**（手算坐标）：`vertical_cut_splits_square_into_two_rings`（方脸竖切，断言两环顶点序列与共享 2 顶点 id）；`stroke_endpoint_inside_face_errors`（端点距边 >4px → Err）；`assign_at_decides_owner_side`（两种 assign_at 各断言归属）；`cut_reuses_existing_vertex_id_when_snapped`。
- [ ] **Step 2-4: 实现（线段求交 + 环重织）+ 绿**。Review Focus 2 在此钉死。
- [ ] **Step 5: Commit** `wv-edit: CutPartitionFace——边到边切分+点选归属侧`

### Task 9: wv-edit Move/Assign 命令

**Files:**
- Modify: `crates/wv-edit/src/command/partition.rs`
- Test: 同文件

**Interfaces:**
- Produces:
  - `pub struct MovePartitionVerticesCommand { pub map: Id, pub layer: Id, pub span_from: i64, pub moves: Vec<(u32, [f32; 2])> }` — 就地改选中 span；**每个被移动顶点必须被 ≥2 张脸引用**（共享顶点=零和构造保证），否则 Err；其余顶点/脸不动。
  - `pub struct AssignPartitionFaceCommand { pub map: Id, pub layer: Id, pub from: i64, pub face: u32, pub owner: Option<Id> }` — **fork 型**（整块易主带生效时间，D8）：在 `from` fork（同 T7 ②配方），fork 段中该脸 owner 换新值，几何零改动。

- [ ] **Step 1: 失败测试**：`moving_shared_vertex_moves_both_faces_rings`（断言两环引用同一 id 后单次 move 双侧生效、第三脸字节不变）；`moving_single_face_vertex_errors`（Review Focus 3）；`assign_forks_span_and_changes_owner_only`（fork 前段求值 owner 不变、fork 段变）；`assign_on_nonexistent_span_errors`。
- [ ] **Step 2-4: 实现 + 绿**。Review Focus 1 在 fork 测试里钉死（三段历史改中段）。
- [ ] **Step 5: Commit** `wv-edit: Move/AssignPartition——零和拖顶点 + 整块易主 fork`

### Task 10: wv-render 分区渲染

**Files:**
- Create: `crates/wv-render/src/faction_partition.rs`（Plugin/system 注册进既有装配点）
- Test: app-test 模式单测（对齐 faction_overlays 测试形态）

**Interfaces:**
- Consumes: `partition_span_at`（T2）、`resolved_faction_layers`、`ViewState.factions_visible` + 层 `visible`、`PlaybackState.current_day`、活动层 id（复用 faction_overlays 的 `active_layer_id` 口径）。
- Produces: 每帧对**每个可见层**求值 `partition_span_at(current_day)`：活动层脸填势力色（color_mode 沿用 ADR-0027 D6/D7 口径）、非活动层只描边；无命中 span → 该层不画。世界数据只读（`RenderInputs` 同款），禁自比时间（AGENTS §3.2）。

- [ ] **Step 1: 失败测试**：`active_layer_fills_and_other_layers_stroke_only`；`day_before_first_span_draws_nothing`（Review Focus 4 的渲染侧）。
- [ ] **Step 2: 实现**（脸环→mesh 走既有 Polygon/PolygonWithHoles 填充配方，earcut 同源）。
- [ ] **Step 3: `cargo app-test` + smoke-gui 绿**。
- [ ] **Step 4: Commit** `wv-render: 分区渲染——活动层填色其余描边，按当前日求值`

### Task 11: wv-app pen 按钮 + PartitionPenDialog

**Files:**
- Create: `crates/wv-app/src/ui/partition_pen.rs`（对话框 + `PartitionEditState` 资源）
- Modify: `crates/wv-app/src/ui/inspector/layers.rs`（层行 pen 按钮）、`crates/wv-app/src/ui/mod.rs`、装配 schedules、i18n Key
- 图标：`crates/wv-ui/assets/ui/tool_icons/pencil.svg`（Lucide pencil，#ffffff；若 wv-ui 已有则复用）
- Test: app-test（对话框逻辑：`partition_pen::decide(...)` 纯函数分测，UI 壳薄）

**Interfaces:**
- Consumes: T7 `BeginPartitionSpanCommand`、`PlaybackState.current_day`、`FactionDivisionDialog` 退役后的弹窗槽位模式（float_window/Panel 配方照 brush_panel）。
- Produces: `pub struct PartitionEditState { pub layer: Id, pub span_from: i64, pub tool: PartitionTool }`、`pub enum PartitionTool { Cut, DragBoundary, Assign }`；对话框流程：pen → 「新增（生效时间 i64 预填 current_day + 起始归属下拉 Option）/ 修改（span 列表 `[from,to)` 行）」；新增确认 → 事务提交 Begin；修改确认 → `PlaybackState.current_day = span.from`（**全局时间轴单时钟**，设计默认 3）+ 进入编辑态。**pen 门控**：当前图 World/Region ∧ `world.terrain_of(map).is_some()`（设计默认 2）。

- [ ] **Step 1: 失败测试**：`pen_gated_off_without_own_raster`；`modify_jumps_global_timeline_to_span_from`；`create_prefills_current_day`。
- [ ] **Step 2: 实现对话框 + pen 按钮 + i18n（zh/en）**。
- [ ] **Step 3: `cargo app-test` 绿**。
- [ ] **Step 4: Commit** `wv-app: 势力层 pen + 新增/修改弹框——生效时间预填、修改跳全局时间轴`

### Task 12: wv-app 编辑 HUD + 切刀手势

**Files:**
- Create: `crates/wv-app/src/ui/partition_edit.rs`（HUD + 手势；mod 装配）
- 图标：`scissors.svg` / `move.svg` / `arrow-left-right.svg`（Lucide，#ffffff）
- Test: app-test（手势数学纯函数分测）

**Interfaces:**
- Consumes: T11 `PartitionEditState`、T8 `CutPartitionFaceCommand`、`FloatWindow`（wv-ui）。
- Produces: 编辑态常驻 HUD（顶部居中浮条）：层名 + 当前 span `[from,to)` + **当前时间选择器**（i64 输入/drag → 写 `PlaybackState.current_day`，即「地图属性·当前时间」）+ 三子工具瓷砖 + 退出。切刀手势：编辑态下指针按下→折线采样→抬起：`stroke_crosses_face` 校验（端点吸附 4px）→ 两半高亮 → 点选一半 → 归属小弹窗（既有势力列表 + 新建 + 无主）→ 事务提交 Cut。编辑态期间意图菜单/播放门控沿用（非播放）。

- [ ] **Step 1: 失败测试**：`hud_shows_layer_span_and_day_selector`；`stroke_capture_rejects_open_stroke`（端点不落边 → 不出高亮、不提交）；`owner_pick_commits_cut_with_assign_at`（断言命令字段含 assign_at=点击位）。
- [ ] **Step 2: 实现 HUD + 手势**（指针→map-local 换算沿用选区/笔刷既有管线；`PointerOverUi` 门）。
- [ ] **Step 3: `cargo app-test` + smoke-gui 绿**。
- [ ] **Step 4: Commit** `wv-app: 分区编辑 HUD（地图属性·当前时间）+ 切刀点选归属手势`

### Task 13: 拖边界/易主手势 + ADR-0028 退役 + 校验 + 收口

**Files:**
- Modify: `crates/wv-app/src/ui/partition_edit.rs`（DragBoundary/Assign 两手势）
- Modify: `crates/wv-app/src/ui/intent_menu.rs`（删「划分势力」瓷砖与门控函数）、删 `crates/wv-app/src/ui/faction_divide_dialog.rs` 及其装配/注册/测试、i18n 清理
- Modify: wv-core 校验模块（新增条目）+ `validation_panel` 呈现
- Test: 各处

**Interfaces:**
- Consumes: T9 两命令、T2 `validate_topology`。
- Produces:
  - 拖边界手势：命中**共享边**（点-线距 ≤6px 且该边两环引用同顶点对）→ 拖动为刚体平移（链上全部顶点）→ 抬起提交 `MovePartitionVerticesCommand`；
  - 易主手势：Assign 工具下点脸 → 小弹窗（势力 + 生效时间预填 current_day）→ `AssignPartitionFaceCommand`；
  - 校验新条目（只提示，V1）：悬空 owner / ring 引用缺失顶点 / span 区间重叠（`validate_topology` 接线）/ **海陆错配**（脸与 `land_outlines` 当前掩膜求交面积 < 50% → 列条目）；
  - ADR-0028 入口退役（数据存量 claim 保留渲染兼容，D9）。

- [ ] **Step 1: 失败测试**：`drag_grabs_shared_edge_chain_not_single_vertex`；`assign_dialog_commits_fork_with_effective_day`；`coastline_drift_lists_mismatch_hint_without_blocking`（Review Focus 5：改栅格后校验有条目、保存不阻止）；`intent_menu_no_longer_lists_divide`。
- [ ] **Step 2: 实现全部**；意图菜单常量/目录数同步改（`intent_count_for` 8→7）。
- [ ] **Step 3: 全门禁**：test/clippy/fmt + app-test + app-clippy + smoke-gui 全绿；spec 验收清单逐条手验并在计划勾选。
- [ ] **Step 4: Commit** `wv-app,wv-core: 拖边界/整块易主 + 划分势力入口退役 + 分区校验条目`

---

## Self-Review 记录

- Spec 覆盖：G1=T4，G2=T4+T5，G3=T2/3/6/7/8/9，G4=T11/12/13，G5=T10（+T11 修改跳段），D7=T8/12，D8=T9/13，D9=T13，ADR=T1，验收清单条目分布 T4/T5/T10/T13。无缺。
- 类型一致：`PartitionSpan/PartitionVertex/PartitionFace`（T2 定义，T3/7/8/9/10/13 同名引用）；命令名 Begin/Cut/Move/Assign 四处一致；`span_from: i64` 定位段、`from: i64` 开段，语义在 T7/T8 Interfaces 里钉死。
- Review Focus 5 条均已绑测试任务。
