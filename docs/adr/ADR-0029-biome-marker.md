# ADR-0029: 生物群系标记（命名段）的存储与生成语义

> 状态：**已接受（Accepted）**（2026-10-18 起草并实现；2026-10-18 用户确认接受）
> 关联：spec `2026-10-18-world-from-zero-map-design.md` D9/D12/D13/D14/D16/D18 + 追记一；
> ADR-0028（势力划分，共用掩膜/环提取管线）；GLOSSARY「生物群系 / 生物群系标记 / 湿判定」

## 处境

世界生成第三段是**命名**：作者圈一块地 → 放标记（名字 + 生物群系类型）→ 系统在包络内
「识别或生成」该群系并给出边界。现有 geo-fact（theme overlay v1）是前身：`Place.feature`
用旧 kind 词汇、`place_geometry` 存手画边界、不碰格子。D18 已裁定它演进为标记模型。
侦察确认三处缺口：`Place` 没有包络字段；`place_territory_at` 只接受单环 `Polygon`；
地形侧没有任何「连通分量 / 多边形填充群系」的公共原语（`LakeRegion` 仍是挖碗派，非 D14）。

## 决策

**D1 · 存储 = Place 扩展，不新建实体集合**。`Place` 增加两个 serde-default 字段：
`biome: Option<u8>`（生物群系编号，冻结表在 `wv_terrain::biome`）与
`envelope: Option<Vec<f32>>`（意图包络，扁平点列，图内局部 wu）。
`place_geometry` 继续存**生成后的边界**（单环，`Polygon`）。理由：名字/锚点/标签/面板
全部复用既有 Place 管线；新开 `biome_markers/` 集合要动 World/io/命令/诊断/面板五层，
而 Place 恰好已经是「名字 + 锚点 + 可选范围」的实体。代价：Place 多两个可选字段
（旧 YAML 零迁移）。

**D2 · 边界 = 生成快照（生成即存储），不逐帧派生**。创建/重生成时一次算出边界存入
`place_geometry`；渲染仍读 span（与 ADR-0028 D1 同构）。builder-check 曾建议按格子
连通派生，但 wv-terrain 无洪泛原语、逐帧派生要按 `terrain_data_hash` 缓存整图，
M1 成本过高；笔刷改动不自动改标记边界，**重圈重生成**（spec D13）是唯一的调整路径，
M2 再评估派生渲染。代价：笔刷后标记光晕与格子群系可能短暂不一致（名字与格子色仍正确）。

**D3 · 「识别或生成」= 自动二选一（D9 的两面）**。内核在包络内统计目标群系格数：
- 占比 ≥ 8%（且 ≥ 4 格）→ **识别**：边界 = 这些格子的连通边界（取最大外环 + 其洞），
  **不写格子**；
- 否则 → **生成**：把包络内的格子写成目标群系（水群系写水 + 就地海拔，陆群系跳过水格），
  边界 = 写入后掩膜的边界（同一条环提取管线）。
两个分支都不阻止作者（V1）：圈错地方最多得到一个空/怪区域，可重圈重生成。

**D4 · 湖泊 = D14 语义派（就地水面）**。目标群系为 `LAKE` 时：包络内**陆格**设为
`surface = WATER`、`biome = LAKE`、`height = 包络内中位海拔`（湖面平，不再挖碗）。
`RIVER` 群系不进 M1（线状标记需要走线段流程而非包络，另立 M2）。既有 `LakeRegion`
意图（挖碗派）保留不动——它是地形工具，标记湖是另一条路，两者不互相覆盖。

**D5 · 群系 → 主题覆盖物词汇的过渡映射**。主题 pack 的 overlay 规则仍以旧
`FEATURE_FOREST = 4` 为键（D18 的词汇统一延后）。渲染取 `place.biome` 映射：
森林/丛林/热带雨林 → 4，其余 → None；`place.feature` 继续作回退（旧工程）。诊断同理：
有 `biome` 时直接比较 `cell.biome`，无时走旧 `feature → migrate_v1_kind` 路径。

**D6 · 时间 = 与 Place/span 既有口径一致**。`from = 创建时的当前日`，`to = None`；
时间轴回拨自动不可见（既有 `place_territory_at` 过滤），不需要新机制。

## 代价

- `Place` 两个新字段（全工作区 struct literal 大多用 `..Default?`——Place 无 Default，
  构造函数调用点需补字段；recon 显示只有 3-4 处字面量构造）；
- 内核新增 `marker.rs`（掩膜环提取要做成 `pub(crate)` 复用）与「写群系」分支；
- 标签/光晕与格子短暂不一致（D2）；
- 主题词汇仍未统一（D5 是桥，不是收口）。

## 被否决的备选

新实体集合 `biome_markers/`（五层联动、与 Place 职责重叠）；逐帧派生边界（无洪泛原语 +
缓存成本）；复用 `place_geometry` 的 `relation` 同时存包络（place 几何文件名无 relation
后缀，同 from 两 span 会互相覆盖）；湖沿用挖碗派（违背 D14「湖面=当地海拔」）。
