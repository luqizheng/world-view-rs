# ADR-0069: Schematic 模式整建制退役——7 模式、顶栏三段、键位 1/2/3

- 状态：**已接受**（2026-10-26，用户裁定「A2 更彻底，一次性解决」）
- 关联：**取代 ADR-0068 D2**（「Schematic 降级色块画布」——本 ADR 走到更彻底的
  A2：模式本身退役；0068 的 D1 顶栏三段化不变）；修正 ADR-0051 内容矩阵的
  「编辑态 Schematic 隐藏 EventDots / Region」规则（载体模式消失）；修订
  ADR-0021 矩阵（Schematic 行删除）
- 取代：`ViewMode::Schematic`、`BROWSING` 四项制、快捷键 `1/2/3/4`

## 1. 处境（Context）

ADR-0068 先把 Edit 段移出顶栏、`Schematic` 降级「色块画布」（`kind_core`
十色对照）。落地后复核：为一个**笔刷时的辅助视图**在 `ViewMode` 枚举、
`MODE_TABLE`、`BROWSING`、i18n、命令面板里维持一整个正式模式，成本与它的
出场频率不成比例——主理人裁定走更彻底的 A2：**模式整建制退役**（8 → 7）。

笔刷 kind 反馈缺口（A2 的已知代价）的承载评估：
- **CellMap 统一属性表单**（ADR-0058）显示选中格的群系（含 swatch）——
  精确反馈的正式入口；
- **笔刷面板**的群系 swatch 显示将要涂的 kind——落笔前反馈；
- 专业色板下「山/雪」类涂改仍有海拔色带 + 晕渲可见（ADR-0067）。
高度反馈（涂山必推高，ADR-0035 跨域规则）在 Browse/Map 的浮雕上直接可读。

## 2. 决策（Decision）

1. **`ViewMode::Schematic` 从枚举删除**：`ALL` 8→7、`BROWSING` 4→3。
2. **`MODE_TABLE` 删 Schematic 行**；其专属规则（EventDots=false /
   RegionLayers=false / 装饰开）随之废止——ADR-0051 内容矩阵的对应行由
   本 ADR 修正。
3. **快捷键重排**：`BROWSING` = [VectorFlat, GoogleMap, Faction]，键
   **1=Browse、2=Map、3=势力**——与顶栏前两段 Browse/Map 对齐；键位绑定
   改为按 `BROWSING.len()` 截断（键数不再硬编码 4，防「按了没效果」）。
4. **i18n 删键**：`ViewModeSchematic` / `CmdSwitchSchematicView` 两键连
   enum 变体一并移除（`ALL` 520→518）。
5. **wv-world-cell-map 的 `ShadeMode::Schematic` 技法名保留**：它是数据
   视图 crate 内部的平涂技法（`shade_flat`，VectorFlat 同用；十余处测试
   消费），不是产品模式——产品侧退役不溯及内部技法枚举。
6. **conv_palette_terrain 的 Schematic 换色分支删除**（ADR-0068 D2 的
   实现随之撤销；`kind_core` 重新成为纯数据字段）。

## 3. 理由（Why This）

- **枚举是契约，不是仓库**：每留一个模式，`MODE_TABLE` 一行、穷尽 match
  一臂、i18n 两键、测试一批全部永久跟随。一个只有笔刷场景才召唤的视图
  不值得这个常驻成本。
- **反馈缺口已有正式承载**（§1）：CellMap 表单 + 笔刷 swatch 覆盖
  「落笔前 / 落笔后」两端，十色画布是第三条冗余路径。
- **键位与顶栏对齐**是重排的唯一原则：1=Browse、2=Map 就是顶栏从左到右
  的前两段，不需要记两套顺序。

## 4. 代价与风险（Cost）

- **键位二次迁移**：势力图 2→3、普通地图 4→2（ADR-0068 刚把用户从
  「4=Map」迁到顶栏点选）。存量习惯受损一次，换来长期「顶栏序 = 键序」。
- **「编辑态」概念消失**：ADR-0051 的「编辑态隐藏事件」规则失去载体——
  事件层现在在所有浏览模式显示（Topographic 本就全关）。若未来需要
  「演示模式」（隐藏交互内容），应做成会话开关而非模式。
- `WV_MODE=edit` 探针失效（debug_probe 删臂，报未知并忽略）。

## 5. 被否决的备选（Alternatives）

- **维持 ADR-0068 D2（色块画布留模式）**：常驻成本 > 出场收益（§1），
  用户裁定不保留。否决。
- **删模式但保键位（GoogleMap 留在键 4）**：`BROWSING` 三项填不出键 4，
  除非塞占位项——比迁移更糟。否决。
- **连 `ShadeMode::Schematic` 一起删**：那是 cellmap 数据视图的内部技法
  名，与产品模式无关，动它要重写十余个无关测试。否决（见 D5）。

## 6. 验收（Acceptance）

1. `ViewMode::ALL.len() == 7`、`BROWSING == [VectorFlat, GoogleMap,
   Faction]`（测试 `browsing_list_offers_vector_flat_google_and_faction`）；
2. 键 `1/2/3` 切 Browse/Map/势力，键 `4` 无效果；命令面板视图组三项、
   无「色块画布」；
3. `MODE_TABLE` 无 Schematic 行（`region_layers_hidden_in_cellmap_but_visible_in_browse`
   等改写后全绿）；`wv-render` 380 测全绿；
4. `WV_MODE=edit` 启动日志报「未知，忽略」且正常进 Browse；
5. 门禁：三套 clippy `-D warnings` / `fmt --check` / `smoke-gui.sh` PASS。
