场管线收尾：模式路由 + 分块指纹 + Themed 观感核查 Implementation Plan
For agentic workers: REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

Goal: 补齐场管线转正后的三笔欠账——按模式路由（修 Coastline/Event/Faction 错画成 Themed 与 oblique 被吃两个活缺陷）、编辑帧分块指纹（3ms→<0.5ms）、Themed 分支浮雕观感剥离核查（用户实报「Browse 看不出来」的正主），并留核查报告与 ADR。

Architecture: 路由判据收在桥接侧单一纯函数 should_render_field，场/chunk 两系统同消费（互相排斥上屏红线不破）；指纹改 256² 分块缓存 + 序贯折叠，与既有 field_dirty 脏矩形同源；Themed 观感走 WV_STRIP 环境探针在 shader 内逐层剥离出对比帧，调参待用户目验后另行落参。零数据层改动。

Tech Stack: Rust 2021 / Bevy 0.19 / WGSL。无新第三方依赖。

Spec: docs/superpowers/specs/2026-10-28-field-incremental-and-default-on-design.md（v2，含修订记录；冲突时以 spec 为准）

Global Constraints
分支纪律（AGENTS §13）：Task 1 建分支 feat/field-mode-routing + worktree .worktrees/feat/field-mode-routing（目录名=分支名）；构建用全局共享 CARGO_TARGET_DIR=D:\compil_world_view_cache\target（每条命令显式 export，正斜杠——反斜杠会被 shell 吃掉在 cwd 造出垃圾目录）。
测试环境：worktree 内不用 cargo-test.sh；先 export TEMP=D:\kudi-projects\worldview-rs\.target\testtmp; export TMP=$TEMP 再 cargo test。
分层红线：改动落 wv-world-cell-map + wv-render（cellmap_bridge / terrain_view shim / state）+ wv-app（debug_probe 探针）；wv-core / wv-terrain / wv-io / wv-edit 零改动；禁依赖 theme crate。
双底图红线：场 quad 与 chunk 纹理同时只能有一条上屏（ADR-0053 回退开关注释）；should_render_field 是唯一判据，两系统必须消费同一函数，不许各写各的布尔式。
N1 教训：重打/重算判据只比内容（指纹），不比变更戳；edit_signal 只许进 needs_hash。
P7/红线：零新增存储字段、零格式变更；field_render 会话态不落盘。
测试命名：完整英文短句；数值断言附 got {} 上下文；无 i18n / 图标改动。
每任务收尾：cargo test -p <改动 crate> 绿 + cargo fmt --all -- --check 过才 commit；提交信息 <scope>: <动作> 中文。渲染/UI 相关任务加 cargo app-test / cargo app-clippy。
Review Focus
路由翻转瞬间的双底图：切进场支持集档时 chunk 实体必须全退场、切出时场 quad 必须完全 teardown——任何一侧残留 = 双重底图叠加。→ T2 routing_flip_leaves_exactly_one_base_layer（沿 field_render_on_chunk_entities 既有统计口径断言帧末真值）。
oblique 判据只在 Topographic 生效：topographic_compression > 0 与非 Topographic 模式组合时不得路由回 CPU（compression 是全局字段，判据必须与 mode 合取）。→ T2 真值表含该行。
指纹缓存的跨图/跨分支污染：换图、换 branch（Topographic↔其余）、换 palette 必须整体重建块缓存，禁止复用旧块哈希（structure_changed 路径）。→ T3 fingerprint_cache_rebuilds_on_structure_change。
退化输入：空图 / 1×1 / 全水栅格上分块指纹不 panic、块数为 0 时 total 与全量一致。→ T3 fingerprint_handles_degenerate_grids。
探针缺省零行为变化：WV_STRIP 未设置（或 =0）时 WGSL 走完整 Themed 管线，生产路径逐位不变。→ T4 strip_mask_zero_is_identity + smoke。
Task 1: 分支 + ADR-0080 + GLOSSARY + SPEC LOCK 追记登记
Files:

Create: docs/adr/ADR-0080-field-mode-routing-fingerprint.md（编号现场核实：ls docs/adr/ 取下一空位；0079 已被势力图占用则 0080，若又有并行占用顺延并在索引注明）
Modify: docs/adr/README.md（索引追加；标注与 ADR-0053 / SPEC LOCK 追记十四的承接关系）
Modify: docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md（文末追加「追记十六：模式路由 + 分块指纹 + Themed 观感核查立项」，按 §6.3 追写）
Modify: GLOSSARY.md（场支持集 field-supported modes / 模式路由 mode routing / 分块指纹 block fingerprint / 剥离对照 layered strip comparison）
Interfaces: 无代码。术语与决策编号（D1–D6）以 spec v2 为准。

 Step 1: 建分支与 worktree（主 checkout 执行：git branch feat/field-mode-routing && git worktree add .worktrees/feat/field-mode-routing feat/field-mode-routing）
 Step 2: 写 ADR-0080：处境（转正后三笔欠账 + 两个活缺陷）/ 决策（路由判据单一函数、分块指纹自身等价判据、观感剥离探针、预算表）/ 理由 / 代价 + 被否决备选（v1 重做式增量、高质量模式开关 B、无路由维持现状）
 Step 3: GLOSSARY 四词条 + SPEC LOCK 追记十六（简要立项记录，指向 ADR 与 spec）
 Step 4: Commit docs: ADR-0080 场管线模式路由+分块指纹+观感核查立项
Task 2: should_render_field 路由（修两个活缺陷）
Files:

Modify: crates/wv-render/src/cellmap_bridge.rs（新纯函数 + 两处 gate 替换：:1082 if view.field_render、:1653 gate 注释处）
Test: 同文件 #[cfg(test)]（bridge 已有测试模块）
Interfaces:

Consumes: ViewState（field_render: bool、mode: ViewMode、topographic_compression: f32，均在 wv_render::state）。
Produces:
pub fn should_render_field(view: &ViewState) -> bool —— view.field_render && supports_field(view.mode) && !oblique_active(view)；supports_field(m) = matches!(m, VectorFlat | GoogleMap | RealTerrain | Topographic)；oblique_active(v) = v.mode == ViewMode::Topographic && v.topographic_compression > 0.0。
pub fn supports_field(mode: ViewMode) -> bool（独立 pub，供真值表与调试面板复用）。
消费方改造：sync_terrain_field 的 gate 与 sync_terrain_chunks 的整层早退分支（:1082）都改调 should_render_field(view)；命令面板 ToggleFieldRender（wv-app dispatch.rs:381）不动（意愿开关语义保留）。
 Step 1: 写失败测试（真值表全覆盖，7 模式 × field_render × compression 组合的关键行）：
field_supported_modes_route_to_field（Topographic/VectorFlat/RealTerrain/GoogleMap + field_render=true + compression=0.3 时：Topographic → false（oblique 生效，compression>0），其余三档 → true）
cpu_reserved_modes_never_route_to_field（Faction/Event/Coastline 任意 compression → false）
oblique_only_pulls_topographic_back_to_cpu（VectorFlat + compression=0.3 → true（判据必须与 mode 合取）；Topographic + compression=0.0 → true）
field_render_wish_gates_everything（field_render=false 全表 false）
routing_flip_leaves_exactly_one_base_layer（沿用 bridge 既有系统级测试形态：同帧先 field 后 chunk 或按注册序，断言 field_render_on_chunk_entities 帧末口径与场 quad 存在性互斥）
 Step 2: 跑测试确认失败（函数不存在）
 Step 3: 实现两函数并替换两处 gate（sync_terrain_chunks 早退分支与 sync_terrain_field 的 gate：should_render_field 为假 → field 侧走既有 teardown_field；为真 → chunk 侧既有整层退场分支照旧）
 Step 4: 验证：cargo test -p wv-render --features app + cargo app-test + cargo app-clippy（基底债务 map_tree ×2 / os-error-5 类照旧只注明）+ bash scripts/smoke-gui.sh
 Step 5: Commit wv-render: 场管线按模式路由——CPU 留守档不再错画 Themed，oblique 回落 CPU
Task 3: 分块指纹（编辑帧 3ms→<0.5ms）
Files:

Create: crates/wv-world-cell-map/src/terrain_view/field/fingerprint.rs（mod fingerprint; 挂进 field/mod.rs，并经 terrain_view.rs shim 可达）
Modify: crates/wv-render/src/cellmap_bridge.rs（TerrainFieldRender 缓存字段 + sync_terrain_field 指纹段替换，约 :1717-1745）
Interfaces:

Consumes: TerrainData（width/height/cells）、pack_content_hash（仅作对拍基准，保留 pub）、既有 field_dirty 矩形（take_field_dirty() 产物，与补丁共用同一矩形——先取先用，补丁与指纹同帧同源）。
Produces:
pub const FP_BLOCK: u32 = 256;
pub struct BlockFingerprint（私有 block_hashes: Vec<u64> + blocks_x/blocks_y: u32）:
pub fn new_full(data: &TerrainData) -> Self（全量建）
pub fn update_region(&mut self, data: &TerrainData, rect: [u32; 4])（重算矩形覆盖的块；rect 为格坐标半开区间，超界钳制）
pub fn total(&self) -> u64（按块行序 FNV-1a 序贯折叠：h = (h ^ block_hash) * 0x100000001b3，起始 offset 0xcbf29ce48422325）
语义：块 hash = 块内 cells 的 height/surface/biome 三字段字节序贯 FNV（与 pack_content_hash 同字段口径、同序）。
集成：TerrainFieldRender 增 fingerprint: Option<BlockFingerprint>；sync_terrain_field 中 needs_hash 命中时——有缓存且有脏矩形 → update_region；否则 new_full。structure_changed（map/branch/palette 变）→ 丢弃缓存强制 new_full。总 hash 进既有 FieldKey.terrain_hash 槽，重打判据不动（N1）。
 Step 1: 写失败测试（fingerprint.rs 内）：
incremental_total_equals_full_recompute_bitwise（随机编辑序列 property 测试：32×24 小栅格 × 50 轮随机改格，每轮 update_region(编辑格) 后 total() 与 new_full(...).total() 逐位相等）
fingerprint_distinguishes_any_single_cell_edit（任一格改动 → total 变；同格改回 → total 恢复）
fingerprint_handles_degenerate_grids（0×0 / 1×1 / 全水 5×5 不 panic，total 与全量一致）
sensitivity_matches_legacy_content_hash（对拍：随机编辑序列下「新 total 是否变化」与「旧 pack_content_hash 是否变化」逐轮一致——敏感性等价，非值等价）
fingerprint_cache_rebuilds_on_structure_change（new_full 后改 map 语义场景由桥接侧测：缓存被丢弃重建——桥接侧最小测试或以 update_region 前置条件文档 + T3 单元覆盖丢弃逻辑的纯函数部分）
 Step 2: 跑测试确认失败
 Step 3: 实现 fingerprint.rs + 桥接集成（TerrainFieldRender 字段、needs_hash 分支、structure_changed 丢缓存；旧 pack_content_hash 保留 pub 并加「对账基准，消费面仅指纹对拍」文档注记）
 Step 4: 验证：cargo test -p wv-world-cell-map + cargo app-test + bash scripts/smoke-gui.sh；带工程启动日志确认编辑帧无回归（perf_timed 既有口径）
 Step 5: Commit wv-world-cell-map,wv-render: 编辑帧分块指纹——3ms 全量扫改脏块增量
Task 4: WV_STRIP 剥离探针 + Themed 浮雕对比帧
Files:

Modify: crates/wv-app/src/debug_probe.rs（WV_STRIP=<0-31> 环境探针：读入 ViewState 新会话字段）
Modify: crates/wv-render/src/state.rs（ViewState.strip_mask: u32，默认 0，会话态不落盘——先例 field_render）
Modify: crates/wv-render/src/cellmap_bridge.rs（strip_mask 写进 FieldParams.pad1.x，:287 注释的未用槽）
Modify: crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl（Themed 分支读 p.pad1.x 位域：bit0=色板 themed_land、bit1=soften、bit2=wash、bit3=veil、bit4=浮雕 relief——mask 位清零 = 该层退回裸底色；浮雕 bit 特例：清零 = relief 恒 1.0）
Test: mask 解码纯函数真值表（bridge 侧）+ strip_mask_zero_is_identity（mask=0 时 FieldParams 填充与现行为逐字段一致）
Interfaces:

Consumes: 既有 Themed 加工链 uniform（theme/pad0.z/pad1/shore_edges.w/relief）。
Produces: 对比帧组（WV_SHOT）：jrpg-world map_1 同机位 × {mask 0（全开）, 1（裸浮雕）, 2, 4, 8, 16（逐层）} × {近观, 远观}——落 .target/strip-shots/。
 Step 1: 写失败测试：strip_mask_zero_is_identity（mask=0 → pad1.x==0 且默认 params 其余字段不变）；strip_mask_decode_truth_table（WGSL 侧语义的 Rust 镜像 fn：每 bit 单独关掉对应层）
 Step 2: 跑测试确认失败
 Step 3: 实现探针链（env → ViewState → params → WGSL）；mask 位语义注释写进 pad1 槽位文档
 Step 4: 采集对比帧：WV_STRIP × 机位矩阵跑 WV_SHOT，帧组落盘；结论写进任务报告（浮雕对比度被哪一层吃掉：色板相近 / soften / wash / veil / 浮雕系数），给出调参候选与建议值——不落参（spec D4：观感由用户目验拍板）
 Step 5: 验证：cargo app-test + cargo app-clippy + bash scripts/smoke-gui.sh（探针缺省零行为变化）
 Step 6: Commit wv-render,wv-app: WV_STRIP 剥离探针——Themed 加工链逐层对比帧
Task 5: 核查报告 + 全门禁收尾
Files:

Modify: docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md（追加「追记十七：转正后核查报告」——预算表逐项数字 + 路由支持集定稿 + Themed 观感结论与调参提案）
Interfaces: 无代码（报告 + 采集）。

 Step 1: 预算实测（对照 spec D5 表）：
编辑单帧（1024²，jrpg-world）：笔刷连拖 perf_timed 数值，预算 ≤8ms
4096² 合成图：用既有生成器造图（参数入报告），单笔 patch 全链 ≤16ms
VRAM：guangdong 实图打开后场纹理集占用（bevy 诊断/任务管理器 GPU 专用内存前后差），预算 ≤1.5GB
支持集逐档 WV_SHOT：{Topographic, VectorFlat, RealTerrain, GoogleMap} 走场 + {Faction, Event, Coastline} 走 CPU，两两与转正前语义比对
 Step 2: 全门禁

bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
cargo app-test && cargo app-clippy
bash scripts/smoke-gui.sh
（基底债务照旧注明：app-clippy map_tree ×2；os-error-5 类若复现按 TEMP 口径重跑）

 Step 3: 追记十七落盘（数字 + 支持集定稿 + 观感结论 + 超标处置建议）；手跑清单（用户目验项：Themed 对比帧组挑层、四 CPU 留守档恢复语义确认）
 Step 4: Commit docs: 场管线转正后核查报告——预算数字/支持集定稿/观感结论
Self-Review 记录
Spec 覆盖：D1 顺序=T2→T3→T4→T5；D2 路由=T2；D3 指纹=T3（判据修正进测试设计）；D4 观感=T4（剥离+结论，落参待用户）；D5 预算=T5；D6 留痕=T1/T5；G1–G5 对应 T2/T3/T4/T5/T1。非目标四条无任务（正确）。无缺。
类型一致：should_render_field/supports_field（T2 定义 T5 复用）；BlockFingerprint::new_full/update_region/total/FP_BLOCK（T3 定义，T4/T5 不直接消费）；strip_mask（T4 定义，state.rs→params→WGSL 单向）。
Review Focus 5 条：1→T2；2→T2；3→T3；4→T3；5→T4。
比例：与 spec v2 同量级，无实现正文。