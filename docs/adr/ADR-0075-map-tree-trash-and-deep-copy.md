# ADR-0075：地图树垃圾桶与深拷贝复制粘贴

- 状态：已接受（2026-10-27）
- 编号追记：分支内原编 0074，与并行会话山系笔（改取 0076）撞号；按 main 索引撞号备注（2026-10-27）改取缓冲号 **0075**。
- 关联：`docs/superpowers/specs/2026-10-27-map-tree-enhancement-design.md`（含 §10 追记）、`docs/superpowers/plans/2026-10-27-map-tree-enhancement.md`
- 修正关系：无（不推翻既有 ADR；与 ADR-0022 城市子图空间形态正交）

## 处境

地图树面板长期是 4 行硬编码占位，零交互；命令层只有增/移（`AddMapCommand` / `AddCityCommand` / `MoveCityCommand`），**没有任何删除地图的命令**，wv-io 保存也从不删 `maps/<id>/` 目录（v0.1 自认简化）。作者的实际工作流是「世界图存在多个版本，版本之间搬运城市图 / 建筑图」——为此曾提议「城市图↔世界图多对多 + Ctrl 拖拽挂关系」，2026-10-27 用户**否决**多对多，改走复制粘贴。

## 决策

1. **D1 关系模型维持严格单亲树**（多对多被否决）。跨世界复用 = 深拷贝副本（`PasteMapCommand`）：整棵子树 + POI / 区域 / 海域 / 双几何 / 逐图数据（terrain / streets / buildings / measurements）全量克隆，**ID 全量重生成**（地图、图层、实体），名字兄弟内补号（青州城 → 青州城 2 → 3），副本与源零共享。
2. **D2 软删**：删除 = `TrashMapCommand` 只在**被删子树根**上打 `trashed` + `trashed_at`；children 引用与父图钻取图标**原样保留**，显隐由渲染层按 `World::map_alive` **派生**（`resolve_markers` 单点门控：图标不画、回退入口不补、双击不可钻）。恢复 = `RestoreMapCommand` 翻回标志（父链存活守卫：父图在垃圾桶则拒绝并提示先恢复父图）。
3. **D3 真删**：`PurgeMapCommand`（仅接受已软删的图）物理移除节点/实体/悬空图标；wv-io `save_maps` 补**目录删除传播**（keep-set = 内存全部地图含垃圾桶条目，purge 后保存即 `remove_dir_all`）。**首条改动 `meta` 的命令**：删除的恰是 `root_map` 时顺带改写为幸存首根——否则下次启动 `ensure_root` 把轨迹钉在隐形图上。
4. **D4 彻底删除豁免撤销**：purge 本体实现完整快照 revert（误入栈不崩），但 wv-app 编辑包装层走 `apply_detached`（apply + 脏路径，**不 commit 进 Undo 栈**）——用户确认过的永久动作不该被 Ctrl+Z 复活。撤销「删除」与垃圾桶「恢复」是两条等效回家路（Ctrl+Z = 恢复）。
5. **D5 粘贴规则**：层级门控（City|Region→World、Interior→City、World→独立新根——多版本世界的生成路径）；锚点 = 目标图视野中心（clamp 进 extent）；粘贴只对**当前打开的图**生效（视野中心才有定义，树上其他节点置灰）。
6. **D6 trashed_at 形态**：epoch 秒字符串 + 显示层相对时间（实施期裁定；std 无时区/日历，UTC 时刻串会差 8 小时）。

## 理由

- **多对多被否决**：会破坏 P4 单亲树纪律与全部既有树形消费方（钻取、校验、坐标系派生）；「版本间搬运」的真实需求用深拷贝满足，成本远低于把树改成图。
- **软删只打标、显隐派生**（spec §10 追记 1 的由来）：Undo 栈不跨会话持久，若删除时物理摘除 children / 图标，保存重开后图标永久丢失、恢复残缺；派生显隐让「恢复」跨重启完整，也让 `revert` 退化为翻标志。
- **root_map 必须改写**：`ensure_root` 每帧钉 `meta.root_map`，删根图不改写 = 启动落进隐形图。

## 代价

- `trashed` 字段进 `MapNode`（老 YAML 零迁移：serde 缺省 false、未删不序列化）；全仓 40+ 处穷举构造补字段。
- 派生显隐要求**所有**子图消费方过 `map_alive`——本轮覆盖树/图标/钻取；城市轮廓（outline_polygon）暂无生产渲染消费者（见台账裁定），将来落地时必须走同一判据。
- purge 豁免撤销 = 用户必须靠垃圾桶 UI 找回（故有两步确认 + 清空确认）；深拷贝大子树的内存/磁盘成本与子树规模成正比。

## 被否决的备选

| 备选 | 否决理由 |
|---|---|
| 多对多挂关系（Ctrl 拖拽） | 破坏 P4 单亲树；树形消费方全要改成 DAG；用户裁定不要 |
| 硬删（无垃圾桶） | 误删不可挽回；与「作者要能随便改」的项目定位（AGENTS §7.5）冲突 |
| 独立 `trash.yaml` 清单 | 状态与节点分离，两处真源易漂移；打标进 index.yaml 更简单且 round-trip 干净 |
| 删除时物理摘除 children / 图标 | 跨重启恢复残缺（Undo 栈不持久）；见 spec §10 追记 1 |
| purge 实现 `revert = Err` | 违反 `EditCommand` trait 契约；快照兜底更稳 |
| `trashed_at` 存 UTC 时刻串 | std 无时区换算，显示差 8 小时 |
