# 场管线收尾：模式路由 + 增量指纹 + Themed 浮雕观感（ADR 编号现场取）

- **日期**：2026-10-28
- **状态**：提议（v2，待用户批准）
- **上游裁定**：用户选 A 案（场管线收尾两步走），否决 B「高质量模式开关」与 C「蛮翻默认」

## 修订记录（§6.3）

- **v1 作废**：v1 草稿建立在「`field_render` 默认 false、增量补丁不存在」的前提上，builder check 对照 trunk 证伪——增量补丁（`field_dirty` + `FieldPack::patch_region`，coast ±32 / biome ±8 裕量、`patch_region_matches_full_repack_*` 逐位等价 property 测试）已在 commit `92302ce`（2026-10-24）落地；默认翻转（`field_render: true`）已在 commit `9dbd1e0`（2026-10-26，SPEC 追记十四）落地。v1 的 D2/D3/G5 重做已有工作，作废。
- **连带诊断勘误**：会话中「Browse 走 CPU 回退路径所以晕渲弱」的诊断**错误**（依据 `debug_probe.rs` 过时注释）；Browse 自转正起就是场管线 Themed 分支。当日落地的 CPU 烘焙比例接线（`bake_meters_per_cell` 等）对回退路径仍正确且保留，但非用户实报问题的解药。本 spec 以「用户实报：同图小地图（BiomeRaw 分支）浮雕明显、Browse（Themed 分支）看不出」为出发点重定标。

## 1. 处境（v2，对齐 trunk 现状）

场管线已转正（默认渲染器），增量补丁已就位。真实剩余缺口四项：

1. **无模式路由（活缺陷，最急）**：转正后无任何按模式路由——切 Coastline / Event / Faction 画的是 Themed 场（`ViewBranch` 只有 BiomeRaw/Themed 二分，`cellmap_bridge.rs:1694`），语义错误；Topographic + 2.5D oblique（`topographic_compression > 0`）时平放的场 quad 直接吃掉斜切（oblique 只在 CPU `cells.rs` 烘焙侧存在）。这两个是**当前默认路径上的用户可见缺陷**，不是翻默认时的假想风险。
2. **编辑帧全量指纹**：`pack_content_hash` 每编辑帧全量扫 1M 格 ≈3ms（`pack.rs`）。
3. **Themed 分支浮雕观感（用户实报问题的头号嫌疑）**：同图同角度，BiomeRaw 分支（裸群系色 + 海拔色带）浮雕明显，Themed 分支（主题色板 + soften/wash/veil 加工链）看不出——转正后 SPEC LOCK 文档已记录同方向实机反馈。需要定位冲淡源并调参裁定。
4. **无核查数字、无 ADR**：转正与增量补丁都没有留下预算数字与决策记录。

## 2. 目标 / 非目标

**目标**

- G1 路由上线：`should_render_field` 单一判据 + 真值表测试，错画与 oblique 被吃两个缺陷当场消失。
- G2 分块指纹：编辑帧指纹只算脏块（<0.5ms@1M 格），且**新方案自身**增量 ≡ 全量（逐位）；与旧全量口径做敏感性对拍（见 D3，值等价不可能）。
- G3 Themed 浮雕观感：WV_SHOT 同图同角度对照 BiomeRaw/Themed，定位冲淡源（veil / soften / wash / 浮雕混合系数），调参方案交用户目验裁定。
- G4 核查报告：支持集逐档 + guangdong 实图 + 4096² 合成图，编辑延迟 / 上传 / VRAM / 帧时间数字入档。
- G5 ADR + GLOSSARY 留痕。

**非目标**

- CPU 块烘焙路径退役（泡稳后另立一轮）。
- Coastline / Event / Faction 三档的场等价移植（路由表把它们安全留在 CPU；移植另行立项）。
- 2.5D oblique 的场等价（oblique 生效时路由回 CPU 即可）。
- GPU 子资源局部上传（Bevy 整传语义下先实测，超标再立专项）。

## 3. 既有地基（全部复用）

- 增量补丁链：`tex.field_dirty` 专用旗标（`terrain_texture.rs:53`，`mark_dirty` 双标，无系统顺序耦合）→ `take_field_dirty()` → `FieldPack::patch_region`（窗口读全量、只写区域；裕量 coast ±32 / biome ±8 / dash ±8 / height·attr 0）→ `patch_material_images` 桥接贴片；`sync_terrain_field` 已有「先补丁后全量」分支（约 :1779）。
- 指纹 gate：`pack_content_hash` + `FieldKey`（map/branch/palette_id）+ N1 教训（重打判据只比内容不比通知）。
- 大小编辑分流现状：代码已有阈值（约 1/4 幅面，与 v1 提案 1/8 不同，取代码现值为准）。
- Themed 分支加工链：主题色板（`themed_land`）、水彩 wash、纸雾 veil、soften——观感核查的对象。
- oblique：仅 CPU 烘焙（`image_from_terrain` 的 compression 参数路径）。

## 4. 决策

### D1 顺序：路由先行（活缺陷 > 性能 > 观感 > 留痕）

路由（D2）→ 分块指纹（D3）→ Themed 观感核查调参（D4）→ 核查报告 + ADR（D5）。路由提前的理由：它是默认路径上的现行语义错误，每多活一轮用户就多撞一次。

### D2 按模式路由

- 判据单一函数 `should_render_field(view: &ViewState, grid: &GridConfig) -> bool`，收桥接侧；`sync_terrain_field` 与 `sync_terrain_chunks` 同消费（互相排斥上屏的既有红线不破）。
- 判据 = `field_render ∧ mode ∈ 场支持集 ∧ ¬(oblique 生效)`。
- **场支持集（初始提案，P2 核查后定稿）**：`{Topographic, VectorFlat, RealTerrain, GoogleMap, Schematic}`；CPU 留守 `{Coastline, Event, Faction}`。
- 真值表测试钉全集（7 模式 × oblique 开关 × field_render 开关）。

### D3 分块指纹（验收口径修正）

- 256×256 格一块，块 hash（FNV-1a）就地缓存，总 hash 按块序确定性折叠；编辑只重算脏块（脏块集合取自 D2 同源的 `field_dirty` 矩形）。
- **验收口径（builder Q2 裁定）**：FNV 链式下「增量总 hash ≡ 旧全量 hash」数学上不可达（块缓存与链式传播互斥）。判据改为两条：① 新方案自身「增量重算 ≡ 全量重算」逐位等价（property 测试，随机编辑序列）；② 新旧口径**敏感性**对拍（同一编辑集合下两者都翻转、未编辑都不翻转）——迁移期双跑一轮，钉死后旧全量口径是否退役由实施按消费面（仅 cellmap_bridge 一处）裁定。
- 代码现值的大编辑阈值（约 1/4 幅面直接全量重打包）维持不动。

### D4 Themed 分支浮雕观感（用户实报问题的正主）

- 核查法：同一张图（jrpg-world map_1）同机位 WV_SHOT × {BiomeRaw, Themed} × {近观, 远观}，外加把 Themed 加工链逐层剥离出对比帧（裸浮雕 → +色板 → +soften → +wash → +veil），定位浮雕对比度被吃掉的是哪一层。
- 产出：冲淡源结论 + 调参方案（候选：Themed 分支浮雕混合系数上调 / soften 对浮雕通道豁免 / veil 强度分档），**数字不裁定观感**——对比帧交用户目验拍板后再落参。

### D5 核查预算（本会话裁定，可改）

| 项 | 预算 | 超标处置 |
|---|---|---|
| 编辑路径单帧（1024²，含指纹+补丁+上传） | ≤8ms | 超标→D3 指纹/补丁再剖 |
| 4096² 合成图单笔 patch 全链 | ≤16ms | 超标→GPU 子资源上传专项（另 ADR） |
| VRAM（场纹理集，guangdong 实图） | ≤1.5GB | 超标→按图幅条件路由（`should_render_field` 加幅面参数）作为后备案 |
| 默认翻转形态 | **无条件默认开**（回退开关已存在，不落盘） | —— |

- 4096² fixture：无现成工程——用既有生成器合成一张（生成参数入核查报告）；guangdong（最大实图）必测。

### D6 留痕

- ADR（编号现场取；ADR-0079 已被势力图切蛋糕占用，从 0080 起核查）：处境（转正后的三笔欠账：路由缺失/全量指纹/Themed 观感）/ 决策（D2–D4）/ 理由 / 代价 + 被否决备选（v1 的重做式增量、B 开关、C 蛮翻已翻后的不作为）；索引标注与 ADR-0053 / SPEC LOCK 追记十四的承接关系。
- GLOSSARY：场支持集（field-supported modes）、模式路由（mode routing）、分块指纹（block fingerprint）、Themed 观感剥离对照（layered strip comparison）。

## 5. 验收

- 路由真值表测试绿；切 Coastline/Event/Faction 恢复 CPU 语义（与转正前截图对齐）；Topographic+oblique 恢复斜切观感。
- 指纹 property 测试绿（自身增量≡全量）+ 敏感性对拍报告。
- Themed 观感对比帧组 + 用户目验结论入档；若落参，落参后 WV_SHOT 复验。
- 核查报告：预算表逐项数字；guangdong + 4096² 合成图实测。
- 全门禁（test / clippy / fmt / app-test / app-clippy / smoke-gui）。

## 6. 留给实施计划的裁量

- 块大小 / 折叠算子 / 块缓存的数据结构（`Vec<u64>` vs HashMap）。
- `should_render_field` 的参数取材方式（直读 ViewState 字段 vs 传枚举）。
- Themed 剥离对照的实现形态（调试探针 uniform 通道 vs 临时分支编译）。
- 旧全量 `pack_content_hash` 的去留。
- 观感调参的候选系数初始值。
