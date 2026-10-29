# ADR-0088：场管线收尾——按模式路由 + 分块指纹 + Themed 浮雕观感核查（重做版）

- 状态：**已接受**（2026-10-29，用户上游裁定 A 案「场管线收尾两步走」、否决 B「高质量模式开关」与 C「蛮翻默认」后，按实施计划 `docs/superpowers/plans/2026-10-28-field-mode-routing-fingerprint.md`（Task 1–5）立项；本文即该计划 Task 1 的留痕件）
- 日期：2026-10-29
- 决策者：用户（A/B/C 三案裁定 + spec v2 决策 D1–D6；分块指纹验收口径修正经 planning builder check 裁定）
- 设计稿：`docs/superpowers/specs/2026-10-28-field-incremental-and-default-on-design.md`（v2，含修订记录；冲突时以 spec 为准）
- 编号说明：本 ADR 的 v1（2026-10-28）拟编 **0082**，随其所在分支一并放弃（见下「修订记录」）；重做时现场核实 0082 已被「自然层单例与 LLM 命名」占用（0080–0087 均已被并行轮次占满），按「`ls docs/adr/` 现场核实取下一空位、顺延并在索引注明」规则改取 **0088**
- 关联：[ADR-0053](ADR-0053-terrain-gpu-field-rendering.md)（GPU 场管线地基与转正——本 ADR 是其转正后的收尾轮）、[ADR-0054](ADR-0054-world-cell-map-crate.md)（改动落点 crate 与固定色禁令）、[ADR-0069](ADR-0069-retire-schematic-mode.md)（Schematic 退役——场支持集据此为七模式口径）、[ADR-0079](ADR-0079-faction-partition-cake-cutting.md)（`ViewBranch` 三分与 `field_branch_of` 的来源）、`docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md` 追记十四（转正）/ 追记十八（本 ADR 立项登记）

---

## 修订记录（重做说明，2026-10-29）

本 ADR 的 **v1 写于 2026-10-28**，落盘在其专用分支上；该分支因 **main 结构漂移**被放弃，未合入。漂移两处：

- 旧单体 `wv-render/src/cellmap_bridge.rs` 已拆分为 `wv-render/src/cellmap_bridge/` 子模块（field / chunks / terrain_texture / overlays 等）；
- `ViewBranch` 随 ADR-0054 的 crate 抽取迁至 `wv-world-cell-map`（`terrain_view/field/material.rs`），且 ADR-0079 T5 追加了第三分支 **Flat**（政治图集页素底）——v1「`ViewBranch` 只有 BiomeRaw/Themed 二分」的表述已过时。

本版为**重做**：决策内容沿 v1 不变，仅①编号顺延、②按 trunk 现状修正处境中的路径与结构描述（行号引用一律删除，只留文件级指向）、③按七模式现值注明 spec v2 §4 D2 误列 Schematic 的笔误。v1 文本未进入任何共享分支，无「只增不改」的既成对象。

---

## 1. 处境（Context）

场管线已转正（`field_render` 默认 `true`，SPEC LOCK 追记十四，2026-10-26），增量补丁已就位（`field_dirty` + `FieldPack::patch_region`，coast ±32 / biome ±8 裕量、`patch_region_matches_full_repack_*` 逐位等价 property 测试，commit `92302ce`）。转正只回答了「默认画不画场」，没有回答「**哪些模式该画场**」——真实剩余缺口四项：

| # | 缺口 | 性质 |
|---|---|---|
| 1 | **无模式路由（两个活缺陷，最急）** | 转正后无**模式级**路由。分支级取色路由 `field_branch_of`（`wv-render/src/cellmap_bridge/field.rs`：Topographic→BiomeRaw；其余模式地形层可见→Themed、不可见→Flat 素底，ADR-0079 T5）只回答「场用哪个色分支画」，不回答「该不该画场」——切 Coastline / Event / Faction 仍画 Themed 场，这三档的语义（海岸线 / 事件 / 势力填色）是 CPU 侧呈现，吃主题色板是**语义错误**（`ViewBranch` 本体：BiomeRaw/Themed/Flat 三分，在 `wv-world-cell-map` 的 `terrain_view/field/material.rs`）；场路径亦无 oblique 判据（`cellmap_bridge/field.rs` 零 `topographic_compression` 引用），Topographic + 2.5D oblique（`topographic_compression > 0`）时平放的场 quad 直接吃掉斜切——oblique 只在 CPU 烘焙侧（`wv-world-cell-map` 的 `terrain_view/cells.rs` `image_from_terrain`）存在。两者都是**当前默认路径上的用户可见缺陷**，不是假想风险 |
| 2 | **编辑帧全量指纹** | `pack_content_hash` 每编辑帧全量扫 1M 格 ≈3ms（`wv-world-cell-map` 的 `terrain_view/field/pack.rs`）——增量补丁把重打包省了，指纹还在全量扫 |
| 3 | **Themed 分支浮雕观感（用户实报问题的头号嫌疑）** | 同图同角度，BiomeRaw 分支（裸群系色 + 海拔色带）浮雕明显，Themed 分支（主题色板 + soften/wash/veil 加工链）看不出。需定位冲淡源并调参裁定 |
| 4 | **无核查数字、无 ADR** | 转正与增量补丁都没有留下预算数字与决策记录 |

**v1 草稿作废勘误（spec 修订记录）**：spec v1 建立在「`field_render` 默认 false、增量补丁不存在」的前提上，builder check 对照 trunk 证伪——增量补丁与默认翻转（`9dbd1e0`）均已落地；连带诊断勘误：「Browse 走 CPU 回退路径所以晕渲弱」错误（依据 `crates/wv-app/src/debug_probe.rs` 过时注释），Browse 自转正起就是场管线 Themed 分支。本 ADR 以「用户实报：同图小地图（BiomeRaw 分支）浮雕明显、Browse（Themed 分支）看不出」为出发点。

---

## 2. 决策（Decision）

### D1 顺序：路由先行（活缺陷 > 性能 > 观感 > 留痕）

路由（D2）→ 分块指纹（D3）→ Themed 观感核查调参（D4）→ 核查报告 + 留痕（D5）。路由提前的理由：它是默认路径上的现行语义错误，每多活一轮用户就多撞一次；性能与观感是「更好」，路由错画是「不对」。

### D2 按模式路由（修两个活缺陷）

- **判据单一函数** `should_render_field(view: &ViewState) -> bool` = `field_render ∧ mode ∈ 场支持集 ∧ ¬(oblique 生效)`，收在桥接侧；`sync_terrain_field`（`cellmap_bridge/field.rs`）与 `sync_terrain_chunks`（`cellmap_bridge/chunks.rs`）**同消费**——场 quad 与 chunk 纹理互相排斥上屏的既有红线（ADR-0053 回退开关注释）不破，不许两系统各写各的布尔式。
- **场支持集（初始提案，P2 核查后定稿）**：`{Topographic, VectorFlat, RealTerrain, GoogleMap}`；**CPU 留守** `{Coastline, Event, Faction}`。**spec v2 §4 D2 的初始提案误列 Schematic（按其退役前的 8 模式旧口径），本文按七模式现值修正**：`ViewMode` 现为 VectorFlat / GoogleMap / RealTerrain / Faction / Event / Coastline / Topographic 七档（ADR-0069，8→7），Schematic 不存在，七模式下实为四场三 CPU。
- **oblique 判据与模式合取**：`oblique_active(v) = v.mode == Topographic ∧ v.topographic_compression > 0`——compression 是全局字段，不合取会让非 Topographic 档被误拉回 CPU。
- **真值表测试钉全集**（7 模式 × oblique 开关 × field_render 开关），外加系统级「翻转瞬间恰一条底图上屏」互斥测试。

### D3 分块指纹（验收口径修正）

- 256×256 格一块，块 hash（FNV-1a，与旧 `pack_content_hash` 同字段同序：height/surface/biome）就地缓存，总 hash 按块序确定性折叠；编辑只重算脏块（脏块集合取自与补丁同源的 `field_dirty` 矩形——一次取用两处消费，不引入第二份脏区追踪）。目标：编辑帧指纹 3ms → <0.5ms@1M 格。
- **验收口径（builder Q2 裁定）**：FNV 链式折叠下「增量总 hash ≡ 旧全量 hash」**数学上不可达**（块缓存与链式传播互斥——缓存的前提是旧块哈希不因他块编辑而变，链式折叠恰恰要求总 hash 依赖全部字节序）。判据改为两条：
  1. **新方案自身**「增量重算 ≡ 全量重算」逐位等价（property 测试，随机编辑序列）；
  2. 新旧口径**敏感性**对拍——同一编辑集合下两者都翻转、未编辑都不翻转。
  迁移期双跑一轮；钉死后旧全量 `pack_content_hash` 是否退役由实施按消费面（仅 cellmap_bridge 一处）裁定。
- 代码现值的大编辑阈值（约 1/4 幅面直接全量重打包）维持不动。

### D4 Themed 分支浮雕观感核查（剥离对照）

- **核查法**：同一张图（jrpg-world map_1）同机位 WV_SHOT × {BiomeRaw, Themed} × {近观, 远观}，外加把 Themed 加工链**逐层剥离**出对比帧（裸浮雕 → +色板 → +soften → +wash → +veil），定位浮雕对比度被吃掉的是哪一层。
- **实现**：`WV_STRIP` 环境探针 → `ViewState.strip_mask: u32`（会话态不落盘，沿 `field_render` 先例）→ `FieldParams` 预留槽 → WGSL 位域逐层关闭（mask 缺省 0 = 生产路径逐位不变）。
- **产出**：冲淡源结论 + 调参候选（Themed 分支浮雕混合系数上调 / soften 对浮雕通道豁免 / veil 强度分档）。**数字不裁定观感**——对比帧交用户目验拍板后再落参，落参后 WV_SHOT 复验。

### D5 核查预算（本轮裁定，可改）

| 项 | 预算 | 超标处置 |
|---|---|---|
| 编辑路径单帧（1024²，含指纹+补丁+上传） | ≤8ms | 超标→D3 指纹/补丁再剖 |
| 4096² 合成图单笔 patch 全链 | ≤16ms | 超标→GPU 子资源上传专项（另 ADR） |
| VRAM（场纹理集，guangdong 实图） | ≤1.5GB | 超标→按图幅条件路由（`should_render_field` 加幅面参数）作为后备案 |
| 默认翻转形态 | **无条件默认开**（回退开关已存在，不落盘） | —— |

- 4096² fixture：无现成工程——用既有生成器合成一张（生成参数入核查报告）；guangdong（最大实图）必测。
- 非目标（另行立项）：CPU 块烘焙路径退役；Coastline / Event / Faction 三档的场等价移植；2.5D oblique 的场等价；GPU 子资源局部上传。

---

## 3. 理由（Rationale）

- **单一判据函数是双底图红线的客观兜底**：场 quad 与 chunk 纹理同时只能有一条上屏是 ADR-0053 以来的红线；两个系统若各写布尔式，判断漂移只是时间问题。收成桥接侧单一纯函数 + 真值表测试，错一处即红。
- **路由先行是缺陷分级**：错画不是性能损耗，是**语义错误**——Coastline/Event/Faction 每切一次错一次，oblique 斜切（作者明确设置的视角语言）被静默吃掉。修复成本极低（一个纯函数 + 两处 gate 替换），收益立现。
- **分块指纹的等价判据让步是数学必然，不是放松要求**：要「增量值 ≡ 旧全量值」就得保留全链式依赖，就得放弃块缓存——两者互斥。改钉「自身一致 + 敏感性等价」两条，保住的恰是指纹的**语义**（编辑必被看见、未编辑必不误报），且 property 测试可自动化；值域变化的一次性后果只是迁移瞬间触发一次全量重打包（当帧完成，用户无感）。
- **脏块与补丁同源**：`field_dirty` 矩形已带编码裕量（coast ±32 / biome ±8），指纹重算与 GPU 补丁共用同一矩形——增量正确性的证明（窗口读全量、只写区域）对两者同构，不新增正确性论证负担。
- **观感不落参**：「冲淡源是哪层」可由剥离帧组客观定位；「调到多少算好看」是审美裁定。把客观部分做穷（定位 + 候选），把主观部分交还用户——与 V1「工具让作者看见问题」同一分工哲学。
- **预算表先行**：转正欠下的「数字账」（缺口 4）用显式预算 + 超标处置闭环；超标路径全部预登记（再剖 / 专项另 ADR / 幅面路由后备案），实施期不临场扩 scope。

---

## 4. 代价与风险（Consequences）

- **路由翻转瞬间的双底图风险**：切进场支持集档时 chunk 实体必须全退场、切出时场 quad 必须完全 teardown——任何一侧残留即双重底图叠加。真值表之外加系统级互斥测试钉住（沿 `field_render_on_chunk_entities` 既有统计口径）。
- **迁移期双跑**：新旧指纹口径并行一轮，编辑帧多一次旧全量计算（≈3ms），属一次性过渡成本；迁移瞬间 `FieldKey.terrain_hash` 值域切换触发一次全量重打包。
- **WV_STRIP 探针的零行为要求**：mask=0 时 WGSL 必须走完整 Themed 管线逐位不变——探针若在缺省态泄漏任何差异，等于给所有用户加了隐形开关。以 `strip_mask_zero_is_identity` 逐字段断言 + smoke 双保险。
- **预算可能超标**：三条超标处置都是预登记路径，不构成本 ADR 决策的推翻；VRAM 超标的后备案（按图幅条件路由）会扩大 `should_render_field` 签名，届时另议。
- **场支持集定稿悬置**：初始提案到 P2 核查定稿之间，实现按初始提案走；若核查推翻某档归属，属参数修订而非决策修订。

---

## 5. 被否决的备选（Alternatives）

| # | 备选 | 否决理由 |
|---|---|---|
| 1 | **v1 重做式增量**（推翻 `92302ce` 补丁链，按 spec v1 草稿重做增量方案） | **前提证伪**：builder check 对照 trunk 证实增量补丁（`field_dirty` + `patch_region` + 逐位等价 property 测试）与默认翻转（`9dbd1e0`）均已落地——重做是无中生有；spec v1 的 D2/D3/G5 作废（spec 修订记录）。真实剩余的只是「指纹还在全量扫」这一段，由 D3 分块指纹承接 |
| 2 | **高质量模式开关（B 案）**：场管线只在高画质档启用，低档回 CPU 烘焙 | 治标不治本：两套管线的**语义边界**（哪些模式该画什么）依旧不存在，开关只把错画推迟到高画质档；且与转正裁定直接冲突——场管线已是默认渲染器（追记十四），再加一档「低质回退」等于给已定案的默认态配一个永不再调的旋钮 |
| 3 | **无路由维持现状**：转正即终态，错画与 oblique 被吃不修 | 两个活缺陷都在**默认路径**上：Coastline/Event/Faction 语义错误每切一次错一次；oblique 是作者显式设置的视角语言，被平放场 quad 静默吃掉。「维持现状」= 让语义错误常驻默认路径，与 D1「路由先行」裁定直接对立 |

---

## 6. 涉及与不涉及（AGENTS §10 自检）

**触及**：§6.4（本文即留痕；与 ADR-0053 / SPEC LOCK 追记十四的承接关系入索引）；§7.1 渲染层门禁（改动落渲染 crate，Task 2–5 各自跑 `cargo app-test` / `app-clippy` + `smoke-gui.sh`）。

**未触及**：P1–P7（零新增存储字段、零格式变更——`strip_mask` 与指纹块缓存均为**会话态**运行时结构，`field_render` 保持不落盘，P3/P6/P7 原样）；分层（改动落 `wv-world-cell-map`（fingerprint 模块 / `assets/shaders/terrain_field.wgsl`）+ `wv-render`（cellmap_bridge 子模块 / state）+ `wv-app`（debug_probe 探针）；wv-core / wv-terrain / wv-io / wv-edit 零改动；固定色 crate 级禁令不破——剥离探针控的是既有加工链开关，不引入色值）；§3.3（零新命令）；§7.5 V1/V2（无交叉）。

---

## 7. 验收（Acceptance）

1. 路由真值表测试绿；切 Coastline / Event / Faction 恢复 CPU 语义（与转正前对齐）；Topographic + oblique 恢复斜切观感；翻转瞬间恰一条底图上屏（系统级互斥测试）。
2. 指纹 property 测试绿（自身增量 ≡ 全量逐位）+ 敏感性对拍报告；退化输入（0×0 / 1×1 / 全水）不 panic。
3. Themed 观感对比帧组 + 用户目验结论入档；若落参，落参后 WV_SHOT 复验。
4. 核查报告：预算表逐项数字（编辑单帧 / 4096² patch / VRAM）；guangdong + 4096² 合成图实测；支持集逐档 WV_SHOT 与转正前语义比对。
5. 全门禁（test / clippy / fmt / app-test / app-clippy / smoke-gui）。

---

## 8. 遗留（不构成本 ADR 决策）

- 场支持集定稿：P2 核查后回填（spec D2 初始提案按本文七模式口径为 {Topographic, VectorFlat, RealTerrain, GoogleMap}）。
- 旧全量 `pack_content_hash` 的去留：由实施按消费面（仅 cellmap_bridge 一处）裁定。
- Themed 观感调参候选系数初始值：待 Task 4 剥离帧结论。
- CPU 块烘焙路径退役、三 CPU 留守档的场等价移植、2.5D oblique 场等价、GPU 子资源局部上传：各自另行立项。

---

## 追记（勘误，2026-10-29）：D2 的 oblique 判据项作废

D2 原文 `should_render_field = field_render ∧ mode ∈ 场支持集 ∧ ¬(oblique 生效)` 的第三合取项**作废**：`ViewState.topographic_compression` 已被 `cdb4e17`（ViewState 死字段清理）删除——该字段自 `02dbedf` 引入起全历史零消费、全部烘焙点恒传字面量 `0.0`，2.5D 斜切（`oblique_skew_pixels`）在主 App 任何路径都不渲染，缺陷②「平放场 quad 吃掉斜切」的前提在 trunk 上不成立。现行实现为 `should_render_field = field_render ∧ supports_field(mode)`（commit `faeeaf9`）；待 oblique 机制（§8 非目标）另行立项复活时，再补第三合取项与真值表的 compression 轴（salvage 存档 `0002-wv-render-CPU-Themed-oblique-CPU.patch` 备着完整形态）。
