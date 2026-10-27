# ADR-0071: 地标面板——Edit/CellMap 两模式单击放置 POI

- 状态：**已接受**（2026-10-26，用户三项裁定：即点即建 / 删除功能 / 面板内开关）
- 关联：ADR-0058（统一属性表单——属性段唯一，本面板不重复）、ADR-0043
  （工具条槽位——本决策**不动**它）、ADR-0057（CellMap 统一选区——放置与
  点选的互斥走同一条 dispatch 机制）、ADR-0070（线段 POI——同域后续，
  本 ADR 的列表过滤已为它留位：折线地点不进地标列表）

## 1. 处境（Context）

POI 的数据层早已齐备（`Place` + `kind: Option<PoiKind>` 12 类别 + rank 呈现
级别，`AddPlaceCommand` / `SetPlaceKindCommand` 都在），但**作者没有任何
「在地图上摆一个 POI」的 UI 入口**——既有三条路径全部间接：

1. 圈选意图菜单「标记…」产出的是**无锚点**群系标记（`x/y = None`）；
2. 作者小河（ADR-0046）锚点由折线中点派生；
3. HTTP skill `create_place`（LLM 通道，非人工）。

地名录式的属性编辑（Inspector 地点表单）与地图放置之间断了一环：作者想
「在这座山上放个山峰 POI」只能手填坐标数字。

## 2. 决策（Decision）

1. **新增 `Panel::Landmark` 地标面板**（`landmark_panel.rs`），两段：
   - **放置段**：类别下拉（12 类 PoiKind，「未分类」= `None`）+ 级别下拉
     （0–3）+ 「放置地标」toggle；
   - **列表段**：当前图的点状地点（`map == 当前` ∧ 有 `x/y` ∧ 非折线），
     点击行 = 选中 → Inspector 地点表单编辑（属性段唯一）；行尾 trash
     按钮 = 删除。
2. **即点即建**（用户裁定 A）：激活放置后单击地图立即创建，默认名
   「新地标」（i18n `LandmarkDefaultName`），随后在 Inspector 改名。不弹
   对话框确认（被否的备选 B——多一步、防误点的收益被 Ctrl+Z 覆盖）。
   - 默认 rank = **3** 而非 1：rank 门（Nation ≥ 3 / Region ≥ 2）下低级别
     新点在远档位看不见，「点了没反应」比「级别偏高」更伤放置反馈。
3. **新增 `wv-edit::DeletePlaceCommand`**（用户裁定：要删除）：删
   `Place` 并**级联删除**以它为端点的 `PlaceEdge`；undo 按原索引恢复实体
   与边（YAML 顺序不变，diff 干净）。级联范围仅止于边——地界环 / 折线
   归属的悬空引用交校验面板列出（V1：只提示不阻止）。
4. **放置模式 = `MapToolId::Landmark` 变体，但无工具条瓷砖、无快捷键**
   （用户裁定 C：面板内开关）。保留 `MapToolId` 身份是为了**白拿 dispatch
   的 `SelectMapTool` 互斥机制**——激活时自动关闭选择 / 点选 / EditCell 等
   一切左键消费者；反之进 CellMap 工作区时也会被 `SelectMapTool(EditCell)`
   关掉。不另写让位逻辑（避免 wv-app 面板去写 wv-render 的
   `SelectionState.enabled` 之类跨层脏写）。
5. **落点换算**（`landmark_anchor` 纯函数，测试钉死）：
   - Edit 浏览档：`cursor_world − drill::map_origin`，不吸附、不要求地形
     （纯矢量图也能放）；
   - CellMap 工作区（`Topographic`）：先 `cell_pick::pick_at` 反查格子，
     命中取**格中心**（`cell_world_rect`）再减 map_origin——格子工作台对齐
     格子语义；点图外不创建。
   - 两处都只走 `map_origin`，**不碰 `raster_origin`**（两套原点陷阱，
     实机错位前科）。
6. **CellMap 工作区随行开启地标面板**（`bar.rs` 进工作区时
   `set_visible(Landmark, true)`，随 `Brush` 同款）；退出随快照还原。

## 3. 理由（Rationale）

- **面板而非纯工具**：放置需要「先名词后动词」的默认值载体（类别/级别），
  面板是这些默认值的自然住址；列表管理（找到已有的点、删掉放错的点）与
  放置同屏，闭环不跳窗。
- **复用而非重建**：属性编辑不进本面板（ADR-0058 属性段唯一）；渲染反馈
  不加预览层（`projection.dirty` → 重投影 → POI 桶重画，放置后立即见点，
  wv-render 零改动）。
- **命令直提而非意图桥**：点击系统住 wv-app（与 `commit_point_select_on_click`
  同域），提交走 Inspector 表单通道同款（直接组 `Transaction` →
  `EditHistory::commit`）——不经过 `EditIntents` 桥（那是 wv-render 输入
  系统的通道），省一个意图变体。

## 4. 代价（Consequences）

- `Panel::ALL` 10→11，默认位 `[652,700]`（Event 列下方、时间轴带之上），
  `ui-layout.yaml` 存量工程首次出现 `landmark` 键（新键追加，零迁移）。
- `DeletePlaceCommand` 让「删点」与「删边」可以分开发生（先删点产生级联，
  再 undo 只回点不回边是不可能的——同一条命令原子回退）；想「只删边」
  走既有路径（距离边编辑，暂无 UI）。
- 5 条 temp 写盘类测试按用户裁定（2026-10-26）暂时 `#[ignore]`——
  AGENTS §2.2 记录的受限环境假失败，非本 ADR 引入；桌面终端
  `--ignored` 可复核。

## 5. 被否决的备选（Alternatives）

- **B 对话框确认式放置**（面板点「添加」→ 填名称/类别 → 确认 → 点击）：
  防误点，但与 V2「作者随便改」精神相悖，且误点成本 = 一次 Ctrl+Z。否。
- **工具条加瓷砖**（`MapToolId::Landmark` 上 ADR-0043 工具条）：入口更显眼，
  但工具条已 14 槽、上轮槽位对齐是踩坑重灾区；地标放置是低频动作，面板
  toggle 足够。否（保留枚举变体不保留瓷砖）。
- **放置走 `EditIntents` 意图桥**（仿 place_line）：与渲染层输入系统对称，
  但本工具的点击住 wv-app 不住 wv-render，绕桥徒增一层翻译。否。
- **列表重复一份属性表单**：违反 ADR-0058 属性段唯一。否——点行选中，
  Inspector 编辑。
