---
name: wv-worldmap
description: 把一份游戏/小说设定文档（Markdown 章节：势力、地貌、城市要塞村落清单、地理关系）变成 wv（World View）里的一张世界地图——读文档 → 抽结构化清单 → 经 wv HTTP 工具面（127.0.0.1:8732）生成大陆/山河/国界/节点 → 截图复核 → 落盘。当用户说「用这份设定做一张地图」「把设定画成世界地图」「重建/更新世界地图」时使用。
---

# 设定文档 → wv 世界地图

## 前置

- wv GUI 在跑（HTTP 服务随 GUI 自启，默认 `127.0.0.1:8732`）。
- 工具面与调用方式见 `wv-http` skill——本 skill 只讲**怎么把设定做成地图**，
  不重复 HTTP 基础用法。**开工前先读 wv-http 的「故障表」**。
- 设定文档可以是多份 Markdown（章节形式），不需要固定 schema。

## 铁律（先读，违反会毁工程）

1. **不静默覆盖。** 工程里已有世界图时必须停下来问用户三选一：
   新建一张 / 在现有图上增量更新 / 放弃。**永远不要**默认「覆盖」。
2. **动手前把清单过用户眼睛。** 大陆轮廓（包络多边形）、势力名单、
   节点清单这三样先以摘要形式展示，用户确认后再写入。
3. **`cell_size` 一经建图不可变。** 后续每次地形/势力调用都必须给同一个值
   ——把它写进你的工作清单里，别每次重算。
4. **坐标只有一个坐标系**：图内局部世界单位（wu），与 `Place.x/y` 同源。
   不许混入屏幕像素、根图帧或经纬度（除非文档给了经纬度换算，那要单独换算
   成 wu 并记录依据）。
   **⚠️ y 轴向上为正**：`(0,0)` 在图的**左下角**，`y = extent_h` 在北。
   写「北境」就是**大 y**。按屏幕直觉（y 向下）画会把整个格局上下颠倒
   —— 2026-11-01 M0 实测踩过，返工 11 条命令。
5. **文档没写的就不硬造。** 缺坐标就按文档叙述的相对方位排布，并在汇报里
   说明「这些位置是推出来的」；缺字段就留空说清楚。
6. **切国界在前，画山河在后。** `divide_faction` 会检查贴地结果是否连通，
   **被一条河 / 一道山脊分成两块的意图会被直接拒绝**
   （报「这次划分会把领地切成多块」）。所以要么让多边形避开已知水系，
   要么先切完所有势力再画河。

## 流程

### 0. 摸清现状

```
get_ui_state        → 看当前图 / 视图模式
list_maps           → 工程里已有哪些图（找 level=world 的根图）
list_factions       → 已有势力（增量更新的 diff 前提）
list_places(limit=…) → 已有节点（同上）
```

- 有世界图 → **停**，把「已有：<名字>（<节点数> 个节点、<势力数> 个势力）」
  告诉用户，请其三选一。
- 没有 → 提案「建一张 <尺寸> 的世界图」，等确认。

### 1. 读文档，抽清单

从设定文档里抽出五样东西（缺哪样说哪样）：

| 抽取物 | 文档里的典型位置 |
|---|---|
| 地图幅面 | 「大陆尺寸」「距离与刻度」表 |
| 大陆轮廓 | 「地图关系」示意图 + 各国「地貌与气候」 |
| 山河 | 「地貌与气候」里的山脉 / 高原 / 大河 / 湖泊 |
| 势力 | 各国档案章节（名字 / 特色 / 无王或王城） |
| 节点 | 每国的「城市 / 要塞 / 村落 / 营地」清单表 |

抽完把**摘要**给用户看（地图幅面 + 势力数 + 各类节点数），等确认。

### 2. 建图（world 级 + 栅格）

```
create_map { id, name, level: "world", parent: null,
             size_w, size_h, cell_size }
```

- `cell_size` 决定栅格分辨率：`size_w / cell_size` 就是栅格宽。
  世界级的合理区间是**幅面/几十到几百格**——幅面 1200×1500 用
  `cell_size: 8` 得 150×188 格，够画山河、跑得动。
- 回执里的 `grid: {width, height, cell_size}` 记下来，全流程都要用。
- **注意**：不带 `cell_size` 建的图没有栅格，地形与切蛋糕工具都会失败。

### 3. 地形

```
generate_continent {
  map_id, cell_size, seed,
  envelopes: [[[x,y], …], …],      # 主大陆一圈；群岛就多圈
  params: { coast_wiggle, relief, forest_cover, desert_cover, islands, … }
}
```

- `envelopes` 是**闭合多边形顶点**（≥3 点），圈出「陆地大致长这样」。
- `params.relief`：`hills` / `mountains` / `high_mountains`。
  「北境高原 + 大陆最重的一道山脉」这类设定 → `mountains` 或 `high_mountains`。
- `seed` 记下来（换 seed = 换一份岸线细节；想原样重跑就复用）。

然后逐条画山河：

```
paint_terrain_intent { map_id, cell_size, intent: { kind, … } }
```

| 设定里的东西 | kind | 字段 |
|---|---|---|
| 山脉 / 山系 | `mountain_range` | `pts`（脊线折点）、`radius`（半宽）、`strength` |
| 高原 / 台地 | `plateau` | `pts`、`radius`、`strength` |
| 丘陵 | `hills` | `pts`、`radius`、`strength` |
| 火山 | `volcano` | `center`、`radius`、`strength` |
| 湖 | `lake` | `center`、`radius`、`strength` |
| 大河 | `river` | `pts`（源头→河口）、`strength` |
| 大片山群 | `mountain_massif` | `polygon` |
| 大片丘陵 | `hills_region` | `polygon` |
| 平原（主动压平） | `plains_region` | `polygon` |
| 湖区 | `lake_region` | `polygon` |

- 每条 intent 是**一条命令、一次 undo**。画错了就 `undo` 重来。
- **加法语义**：同一条山脊跑两次会继续增高——不要重复下发。
- 地形改完立刻 `screenshot` 看一眼，再往下走。

### 4. 势力

```
create_faction { faction: { id, name, color: "#RRGGBB", rank, seat } }
divide_faction { map_id, host, intent: [[x,y], …], mode: "snap"|"straight",
                 faction: {…}, from: 0, cell_size }
```

- `divide_faction` 一条命令同时建势力 + 切领土，**通常用它就不用先
  `create_faction`**（除非势力暂时没有领土）。
- `host`：从哪个势力身上切；`null` = 从空白地切。
- `mode: "snap"` 让边界吸附水陆交界 / 河链 / 山脊——用户若说「国界沿着
  山脉/大河」就用它；回执的 `snapped` 为 `false` 说明没吸附到地物，
  要在汇报里说明。
- **切的顺序有影响**（后切的面会吃掉先切的）。按「大 → 小」切：
  先切最大的国家，最后切被包围的小块或入侵区。
- `rank` 是自由串（`kingdom` / `tribe` / `free_city` …），只影响边框粗细与
  面板徽标，不影响逻辑。
- `seat` 指向首府 Place 的 ID——如果首府还没建，先建节点再回来 `update`，
  或干脆留空。

### 5. 节点

```
create_place { id, name, map, x, y, kind, rank }
```

`kind` 词表（19 类，全部 `snake_case`）：`other` / `settlement` / `peak` /
`port` / `station` / `scenic` / `food` / `lodging` / `medical` / `education` /
`government` / `shop` / `ruins` / `arena` / `shrine` / `portal` / `danger` /
`fortress` / `camp`。

`rank`（呈现级别）：`3` = 国家级（王城）/ `2` = 市镇级（城市、要塞）/
`1` = 普通（村落、营地）/ `0` = 微点。

**节点分类映射**（设定术语 → wv 词表）：

| 设定里的 | kind | rank |
|---|---|---|
| 王城 / 各国中心 | `settlement` | 3 |
| 城市 / 港市 / 集市 | `settlement`（有出海口的可 `port`） | 2 |
| 要塞 / 堡垒 / 关口 | `fortress` | 2 |
| 村落 / 聚落 | `settlement` | 1 |
| 流动营地 / 游牧驻地 | `camp` | 1 |
| 驿站 / 坑口 / 渡口 | `station` | 1 |
| 神殿 / 祭坛 / 教廷 | `shrine` | 2 |
| 传送门 / 魔法节点 | `portal` | 2 |
| 遗迹 / 废墟 | `ruins` | 1 |
| 险地 / 禁区 | `danger` | 1 |
| 山峰 / 地标 | `peak` | 1 |

- **ID 规则**：`place_<拼音>`（如 `place_tiezhacheng`）、`faction_<拼音>`
  （如 `faction_shuangguan`）。**同一份文档重读必须产出同一 ID**——这是增量
  更新的全部基础。拼音取全拼、去声调、小写、`_` 分隔。
- 世界图 v1 一律用点图标：**不要**给 `w`/`h`（那是矩形范围，留给城市子图）。
- 一次一条 `create_place`，不要幻想有批量接口；几百个点会慢，如实告诉用户。

### 6. 复核与落盘

```
screenshot（管道落盘，别让 base64 进上下文）→ 看画面
get_ui_state / list_places → 抽样核对
save_project
```

汇报格式固定三段：
1. **建了什么**：图 / 网格尺寸 / 势力数 / 各类节点数 / 地形意图条数；
2. **推出来的部分**：哪些坐标是照文档方位估的、哪些字段文档没写给空了；
3. **没做的**：文档里提到但本轮没落地的（子图、迁徙路线、距离参数…）。

## 增量更新（已有图）

v1 只覆盖**节点集**：

1. `list_factions` + `list_places` 拿现有 ID；
2. 与本次清单求差：新 ID → `create_place`；字段变了 → `update_place`
   （name / rank / x / y）或 `set_place_kind`（kind）；
3. **工程里有、本次清单没有的 → 不动**，列进汇报的「未触及」段；
4. 势力与地形的变化 **v1 做不了增量**（无删除工具、地形是加法语义）——
   如实告诉用户「要改国界/山川，得新建一张图重跑」。

## 三则特例（用户已裁定，别自作聪明）

| 设定情况 | v1 做法 |
|---|---|
| 「悬在云上的高塔链」（浮空） | 当普通节点落在地图上（`settlement`/`education`），不要给它们造地形 |
| 「地底国」（无地面领土） | 地表只落坑口（`station`）与地表营地（`camp`），国土面照常参与切蛋糕 |
| 「会迁徙的营地」 | 落成静态 `camp` 点，**不要**假装做时间轴位移 |

## 故障与坑

| 现象 | 原因与处置 |
|---|---|
| `generate_continent` 报「无栅格」类错误 | 建图时没给 `cell_size`。删图重建（或 GO 里手动铺底）后重来 |
| `divide_faction` 报「这次划分会把领地切成多块」 | 意图被河/山脊割成两块（贴地后不连通）。**缩小多边形避开那道地貌**，或先切势力后画河（见铁律 6） |
| `divide_faction` 报「势力已存在」 | 该 ID 已建。改用 `host` 指定宿主，或换个 ID |
| `snapped: false` | 意图多边形没压到水陆/河/山脊。放大多边形或改 `mode: "straight"` |
| 势力切错了想重切 | **v1 没有删除工具**，只能 `undo` 回到切之前重来（README §12-57 待裁）。势力数少时先规划好再切 |
| 想给势力补首府 `seat` | `seat` 只能在 `create_faction` / `divide_faction` 时给；**事后没有工具能改**。建势力时就一起给 |
| `list_places` 报 `limit 上限 100` | 分页：带 `cursor` 续取，直到 `next_cursor` 为空 |
| 地形画了但截图没变 | 已修（ADR-0099 排水失效贴图）；若仍旧，`set_view_mode` 往返一次或 GUI「强制重画地形」 |
| 截图超时 / `os error 5` | ① 窗口最小化/后台挂起 —— 恢复前台；② 若 GUI 由 AI agent 的沙箱进程拉起，写盘与截图会被拒——用非沙箱方式启动，或把工程放进可写目录 |
| 点太多、一条条建很慢 | 如实汇报进度；不要为了快而合并 ID 或省略节点 |
