# ADR-0032：Place 地理事实词汇统一（feature → biome 收口，群系族规则键）

> 状态：**已接受（Accepted，2026-10-18，用户对话拍板）**
> 关联：spec `2026-10-18-world-from-zero-map-design.md` D18 + 追记三/四；**ADR-0029 D5（过渡桥，被本 ADR 取代——D5 自注「词汇统一延后」，本 ADR 即该延后的收口，ADR-0029 其余决定不动摇）**；ADR-0023（主题包契约，外部主题 crate 兼容性）；
> ADR-0030（rank，同扩 Place 零冲突）；ADR-0031（terrain decor，同以冻结群系表为数据源）；GLOSSARY「群系族」

## 处境

`Place` 同时携带两套地理事实词汇：`feature: Option<u8>`（旧 TerrainKind 0–9 词汇，主题
覆盖物按它取规则）与 `biome: Option<u8>`（冻结群系表 0–20，ADR-0029 D1，标记管线用）。
D18 早裁定统一，ADR-0029 D5 只搭了过渡桥。收口前的五个读点各自为政：

| 读点 | 收口前口径 |
|---|---|
| 渲染桥 `overlay_feature_for_place`（wv-render `feature_overlays.rs`） | 三臂 match：biome 森林族→4；其他群系→None；无 biome 回退 `place.feature` |
| 主题规则键 `FeatureOverlaySet::rule_for(u8)` + `FEATURE_FOREST=4` | 数字键，与冻结表无类型关联 |
| POI 抑制 `place_dot_for`（world_projection.rs） | `feature.is_some() \|\| biome.is_some()` |
| 诊断 `detect_feature_territory_hints`（wv-io `diagnostics.rs`） | `biome.or(feature)`，feature 经 `migrate_v1_kind(f).1` 换算比较 |
| serde | `feature`/`biome` 双键读写，写回仍写 feature |

旧词汇的**最后一个生产写入者 `add_feature_place_now` 已是死代码**（M4 起 UI 全走
`place_biome_marker_now`，全仓零调用点）。仓内 example 工程（xinhai-1911）不用
feature——迁移的实际受益者是仓外旧工程。wv-io 已有同构先例：terrain.bin v1→v2 的
`migrate_v1_kind` 表 + dev-dep 测试钉一致性（`migration_table_matches_wv_terrain_frozen_biome_ids`）。

## 决策

**D1 · 唯一词汇 = biome（冻结表）**。`Place.feature` 降级为 serde 只读迁移脚手架：
`#[serde(default, skip_serializing)]`——旧 YAML 照常读入，写回一律不再出现。
迁移窗口一个版本周期：**本版本**读 + 加载迁移 + 诊断提示；**下版本**删除字段（届时旧
YAML 里的 feature 键被 serde 静默忽略，但信息已在本版本首次加载时迁入 biome，无损）。
约 30 处 `feature: None` 构造点本版本不动（字段还在，字面量照常编译），下版本机械清扫。

**D2 · 主题规则键 = 群系族（BiomeFamily）**。枚举落 `wv_terrain::biome`（与
`is_forest_family` 同居——权威零复制、零新钉测；wv-render 本就依赖 wv-terrain，经主题
API 再导出给主题 crate，外部主题 crate 不直接摸 wv-terrain）。冻结表 21 编号全覆盖划分：

| 族 | 成员（冻结编号） | 对应 `display_kind` 陆面槽位 |
|---|---|---|
| Forest 森林族 | 1 森林、10 丛林、11 热带雨林 | 4 |
| Grassland 草原族 | 2 平原、3 草原、17 绿洲、18 稀树草原 | 3 |
| Desert 干旱族 | 4 戈壁、5 沙漠、19 盐碱地 | 2 |
| Wetland 湿地族 | 6 沼泽、7 湿地 | 8 |
| Cold 寒带族 | 8 苔原、9 雪原 | 7 |
| Highland 高地族 | 12 丘陵、13 山地 | 5 / 6 |
| Water 水域族 | 14 海洋、15 湖泊、16 河流 | 0 / 1 |
| Road 道路族 | 20 道路 | 9 |
| （无族） | 0 NONE、未登记编号 | 高度回退 |

族界与既有 `display_kind` 的陆面槽位分带对齐（绿洲入草原族即从此先例）。
钉测固化的不变式是**槽位划分细分族划分**：同槽位的陆面群系必同族；族**可以**
跨多个槽位（高地族横跨丘陵 5 / 山地 6 两槽位；水域族 0/1 按深度分档而非按群系）。`FeatureOverlaySet` 改为**全族字段**（v1 内置
主题仅 forest 有值），`rule_for(family: BiomeFamily)`。一次性定型全集字段：族已对冻结
表穷尽，未来不给外部主题 crate 制造第二次破坏性变更。

**D3 · 加载迁移，写回只写 biome**。wv-io 载入 places 时：`biome` 空且 `feature` 有 →
按表（D4）写入 `biome`；`biome` 已在 → biome 胜，不做迁移。**内存中保留 feature 原值**
作为提示的状态证据：`derive_hints` 看见 `feature` 在场即发迁移提示（指纹
`feature-migrated|{place_id}`，文案以 biome 字段为准）——作者下次保存**地点数据**（`save_world` 的
`distances/places` 脏区被写入）后 feature 从磁盘消失，重载后提示自然消散（指纹
可认领），零新管道。注意脏区路由：只保存地形等其他子树不会重写 places，提示
会保留到下一次地点回写或重开工程——提示文案已按此口径措辞。磁盘旧文件在作者保存前一字不动
（P6 非破坏）；词汇表外垃圾值发 `feature-unmapped|{place_id}`，按普通地点处理。

**D4 · place 级迁移表（wv-io `place_feature_to_biome`，紧邻 `migrate_v1_kind`）**：

| 旧 feature | 0 深水 | 1 浅水 | 2 沙 | 3 草原 | 4 森林 | 5 丘陵 | 6 山地 | 7 雪 | 8 沼泽 | 9 道路 | 其他 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| → biome | 14 海洋 | 14 海洋 | 5 沙漠 | 3 草原 | 1 森林 | 12 丘陵 | 13 山地 | 9 雪原 | 6 沼泽 | 20 道路 | None+提示 |

与 `migrate_v1_kind(f).1` 在 **{2,3,4,8,9} 完全同值**——这些值的诊断比较逐位不变；
**{0,1,5,6,7} 从「映射到 biome 0 的假阴性」升级为冻结表真实群系**（旧表把它们归 0 是
格子侧「高度派生带 / 水」的取舍，place 侧不继承——声明了丘陵的地点在新词汇下就该是
丘陵标记，可进区域面板、可重生成）。新表独立钉测；wv-core 零 wv-terrain 依赖不变。

**D5 · 渲染收口**。`overlay_feature_for_place` 三臂桥塌缩为
`place.biome.and_then(family_of)`；`FEATURE_FOREST` 常量删除；feature 兜底臂删除（凡经
加载器的世界不再有 feature-only 地点，渲染层不再知道旧词汇）。`forest_sites` 已按群系
比较，零改动。

**D6 · 死代码清除**。删 `add_feature_place_now` 及其 2 个测试、`edit/mod.rs` 与
`feature_place_dialog.rs` 的过时文档引用——旧词汇的最后一个写入者退场，词汇收口在
写入侧也闭合。

**D7 · 诊断指纹稳定**。`feature-no-cells|` / `feature-orphan-territory|` 字符串**不改**
（UI 已读/忽略状态锚定，换指纹等于重置认领记录）；新增 `feature-migrated|`、
`feature-unmapped|` 两条。

**D8 · round-trip 契约的显式修订（§6.3 追写）**。迁移窗口内 `load(save(w)) == w` 对
「携带旧 feature 的世界」不再字面成立（save 掉 feature）——契约改为两条：① 对不含旧
字段的世界严格等价不变（现有测试主体）；② 迁移收敛性 `load(save(save(w))) ==
load(save(w))`（首次**地点数据**保存即物理完成迁移，二次保存恒等）。旧 fixture（wv-core
`distance.rs` 的「旧 feature 字段保留」、wv-io `place_territory_yaml.rs` 的
`feature: 4`）按新契约翻转。下版本删字段后契约回归严格等价。

## 理由

以 biome 收口而非反向：冻结表是生成管线的**活词汇**（十七群系对话框、标记管线、
ADR-0031 装饰采样全挂在它上），feature 只有森林一条规则在消费；反向统一等于让 21 编号
世界迁就 0–9。族键解决「写 1 漏 10/11」的森林族碎裂；族界对齐 `display_kind` 槽位让
「族 = 同一演绎长相」有仓库先例可依、有不变式可测。加载迁移 + 内存驻留让「提示」与
「非破坏」同时成立，是唯一不需要新状态管道的方案。外部主题 crate（ADR-0023 的
受众）对全族字段一次定型：今后加规则内容不破 API。

## 代价

- round-trip 契约修订一次（D8，显式留痕，§7.2 的口径修正）；
- M-A 使 0/1/5/6/7 声明的旧地点可能**新出现** feature-no-cells 提示——旧口径把
  5..7 归 biome 0，只有在格心群系为 NONE 的格上才「命中而静默」（假阴性）；
  新口径升级为真实群系后，这些地界内若没有对应群系格就会提示（噪音变化，非视觉回退）；
- 外部主题 crate **破坏性 API 变更**（chinese-ink 同 PR 修复，作兼容样例）；
- **非森林族标记现在会画回退晕**（有意）：旧口径只给森林族开覆盖物门（其余 biome
  无覆盖物——名字/标签仍可见）；族键化后 17 群系标记都会进入渲染，无规则的族得到中性
  回退晕（`FALLBACK_HALO_ALPHA` 0.12）+ 地界 + 名字——这是「事实不因换主题消失」
  原则的延伸，也让「有标记却没有任何边界」的旧观感统一为可见边界。若将来要恢复
  「无规则族不画」，收敛点 = `sync_feature_overlays` 在进回退分支前按
  `rule.is_some()` 门控（单行改动）。
- 迁移窗口内 Place 双字段并存，POI 抑制暂保持 `feature||biome`（对窗口内非法 feature
  值仍抑制点标），下版本收敛为 biome 单条件并机械清扫 ~30 处 `feature: None` 字面量。

## 被否决的备选

- **以 feature 收口**：冻结表是活词汇而 feature 仅剩一条规则在消费，成本倒挂。
- **D5 桥永续（双词汇长期双读）**：词汇漂移永久化，每个新消费者都要双臂实现。
- **主题规则键直接用群系编号（无族）**：森林族碎裂（写 1 漏 10/11）。
- **加载即清 feature（保严格 round-trip）**：迁移提示失去状态来源，需新开加载报告管道。
- **迁移表复制进 wv-core**：wv-io 已是唯一运行时换算点，wv-core 无消费者，复制 + 钉测
  是纯成本（wv-core 零 wv-terrain 依赖不变）。
- **FeatureOverlaySet 只留 forest 字段**：最小 diff，但外部主题 crate 在下一个族上线时
  会再破一次。
- **绿洲入干旱族**（草案原分法）：与 `display_kind` 槽位先例冲突（绿洲按草原槽取色），
  改入草原族并对齐槽位不变式。
