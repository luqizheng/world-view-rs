# 城市街区工具与缩放语法 v1 设计（2026-11-06）

> 状态：设计定稿，待排期实施。
> 来源：2026-11-05/06 brainstorm（用户 = luqizheng），专家路线 #2 的落地。
> 关联：ADR-0038（进城手势）、ADR-0070（line-POI）、ADR-0091（rank 呈现）、
> ADR-0103（LOD 比例阈值）、README §12-31（建筑工具挂账）。

## 1. 意图与为什么

城市图（`MapNode` 子图）此前只有「钻进去看」的语义，没有「钻进去**做**」的
工具——作者把同一座城市画两遍（世界图上摆 POI、城市图里手摆建筑）负担
极重，世界图的 32× 深放大又没有内容支撑（插值色块）。本设计把城市图变成
**模拟城市 2000 式的编辑场景**：框选一块地 → 自动长出随机建筑群；用道路 /
河流工具画线；用 POI 工具点出「警察局 / 市政厅 / 主角的家 / 学校」。世界图
回归「看」的职责，深缩放收浅。

## 2. 用户拍板的决策（变更需回炉）

| # | 决策 | 内容 |
|---|---|---|
| D1 | 子图保留 | 城市图是真实编辑场景，不退役钻入管线 |
| D2 | **B 留种** | 区域记录生成参数（密度/风格/seed），可整区重摇、调参重摇 |
| D3 | **① 重摇清全区** | 重摇 = 清空该 ring 内**全部建筑（含作者手摆的）**后重新长，**一个事务一条 undo**；不做出身标记、不做预览确认、不追问——误重摇靠 undo 兜底（2026-11-06 用户确认接受误删推论） |
| D4 | **A 新实体** | `District`（街区）独立实体，**遵循 BuildingLayer 同款图层文件架构**（见 §3） |
| D5 | 世界图 8× | 世界图 `LEVEL_ZOOM_SPAN` 32→**8**；城市图保留 32×；双击图标进城不变（ADR-0038 语义不动） |
| D6 | 词表补二 | PoiKind 追加 `Police` / `House`；**市政厅用既有 `Government`**（评审发现撞车，用户裁定不并存）；其余（Medical/教育/Shop/Shrine/Lodging/Government…）词表已覆盖，不重复添加 |

## 3. 数据模型

### 3.1 District（新，wv-core）

**架构对齐 `BuildingLayer` 先例**（P5 图层化 + 几何不进领域 YAML）：

```rust
// wv-core/src/source/district.rs（新文件）
pub struct District {
    pub id: Id,
    pub name: Option<String>,
    /// 区域多边形（本图局部坐标，与 Building.ring / StreetSegment.points 同口径）。
    pub ring: Vec<Point>,
    /// 生成参数。
    pub params: DistrictParams,
    /// 生成种子：同参数同 seed ⇒ 同建筑群（确定性，跨机一致）。
    pub seed: u64,
}
pub struct DistrictParams {
    /// 目标密度（0..1，占地率）；缺省 0.35。
    pub density: f32,
    /// 楼层带（米），缺省 6..=18（2~6 层）。
    pub height_min_m: f32,
    pub height_max_m: f32,
    /// 街区肌理：格网对齐尺寸（米）带；缺省 12..=30。
    pub block_min_m: f32,
    pub block_max_m: f32,
}
// World 增字段（与 buildings 同款 BTreeMap + 图层文件）：
pub city_blocks: BTreeMap<Id, DistrictLayer>,   // DistrictLayer { districts: Vec<District> }
```

- `Layer`（kind）扩 `district`；`Layer.source` 指图层文件（YAML，P3）。
- **无图层自动补建**：目标图没有 Building 图层时，生成命令自动补建
  `LayerKind::Building` 图层并挂 `MapNode.layers`（评审 Minor 定夺；
  district 图层同理）。保存脏过滤走 project.rs 既有虚拟命名空间先例
  （`buildings/<layer_id>` 同款 `city_blocks/<layer_id>`）。
- **保存纪律**：`save_world` 须把 `city_blocks` 纳入脏过滤（与 `write_terrains`
  / building 文件同款），否则重摇后不落盘。

### 3.2 街区可见形态（2026-11-06 用户裁定）

ring 本体**要画**：常态 = 淡描边（isoline_ink 同族，alpha ≈0.35）+ 极低
透明填充（≈0.08，可辨认「这里圈过」）；选中态 = 金色高亮描边 + 蒙版脉动
（与统一选区蒙版语法同族）。渲染走独立 overlay pass（新面，约 200 行量级）
——不是「渲染零新面」；§8 对应句修正。点选命中 = 射线法点在环内。

### 3.3 Building 派生（不改 Building 本体）

- 生成建筑就是普通 `Building`（ring + height_m + 可选 name），落进所属图的
  building 图层文件——渲染/拾取/导出零特判。
- **重摇事务**：`RegenerateDistrict(id)` 单命令 = 删除该 ring 内全部现有
  Building + 按参数重新生成 + 替换，**一次 History 记录**（先例：角色等级
  整表替换命令）。
- 生成建筑与 ring 的包含判定用「中心点在环内」（射线法，ring 简单多边形假设）。

### 3.4 PoiKind 扩词表（D6）

`Police`（治安/警署）、`House`（住宅/主角的家）两个新值；市政厅语义用既有
`Government`（行政/政府机构，distance.rs:181）。serde 按名序列化，尾部追加
（现有枚举值不动，旧工程零迁移）；图标走 Lucide 裁定（§8），随图标轮补
两枚 SVG。

## 4. 工具交互（城市图工具条）

1. **街区框选**：城市图是矢量图，**不能复用**统一选区的手势（那是地形格
   工作台 wv-app/edit/editing/cell.rs 的格选；ADR-0057/0043 语义绑栅格）——
   新写**屏幕多边形圈选**（点列入环，回退矩形双击快选）→ 松手弹出
   `DistrictForm`（密度/楼层带/肌理带滑条 + seed 显示 + 「生成」）→ 生成。
   已有 district 上重框 = 选中后走同一表单（调参重摇 / 「清空重摇」按钮）。
2. **道路 / 河流笔**：数据面零新增（StreetSegment / line-POI 已有，ADR-0070）；
   工具面板把既有线工具在城市图模式下的入口点亮即可。
3. **POI 工具**：既有 place 工具 + 新词表三类的下拉项。

## 5. 生成算法（要点；实现自由度在 builder）

- **确定性**：`splitmix64(seed)` 家族（与 biome 装饰 ADR-0031 同源），逐建筑
  派生子种子；同 (参数, seed) 跨机逐位一致，进测试。
- **肌理**：格网对齐 = 先把 ring 裁进 block 网格（block_min..max 抖动），
  街道侧留 setback；每个 block 内按占地率摆 1..n 栋矩形/切角矩形，
  短边贴街（沿街对齐），高度在楼层带内采样。
- **顺街优先**：有 StreetSegment 的 block 边缘生成率上调（沿街商铺肌理）；
  v1 允许退化（无街道时纯格网）。
- **规模守卫**：单区建筑数上限（建议 500）+ 总顶点上限，防手滑框全图。

## 6. 缩放语法（D5）

- `LEVEL_ZOOM_SPAN` 拆两档：**世界图 8×，其余级 32×**（判据 = `MapLevel`
  或当前图是否含有 Buildings 图层；实现取「按图判」——`MapLevel::World`
  为 8，其余 32，最易回退）。
- ADR-0038 的边界语义（撞内界夹停、双击进城）**全部不动**；只改内界深度。
- 深度测试锚点：世界图 4096/800px，fit 5.12 → 内界 0.64（旧 0.16）。
  `zoom_level.rs` 的既有测试按新值改锚。
- 城市图建筑在 Edit 档全可见（rank 门槛不适用于 Building——建筑跟随图层，
  与现状一致）。

## 7. 非目标 / 反目标

- 非目标：道路自动沿街对齐建筑（v2）、建筑逐栋拾取编辑 UI（README §12-31(c)
  另轮）、3D/透视、街区经济模拟。
- 反目标：生成参数进主题包（这是数据不是风格）；建筑数据二进制化（P3）；
  重摇弹确认框（D3 裁定：undo 兜底，不追问——§7.5 V2 同哲学）。

## 8. 约束与惯例（实施须遵守）

- AGENTS 分层：District 定义在 wv-core（零 bevy）；**生成算法纯函数放
  wv-core**（矢量几何与 building.rs 同居；wv-terrain 是栅格/笔刷域，评审
  定夺），wv-app 的 EditCommand 调用；渲染**两处**：建筑复用 buildings.rs
  已覆盖，街区 overlay（§3.2）是新面（独立 pass）。
- i18n：全部 UI 文案双语入表（两处同源纪律）。
- 图标：Lucide，`stroke="#ffffff"` 烘焙，kebab-case 落盘。
- YAML round-trip：District / DistrictLayer 必须过 `load(save(w)) == w`。
- 测试：生成确定性（同 seed 同输出）、重摇事务原子性（undo 后逐位还原）、
  保存纪律（city_blocks 落盘）、包含判定（凹环）、词表 serde 兼容。

## 9. 留给实施者的自由度

- 框选手势与统一选区的具体复用方式（矩形/多边形/自由圈）。
- 生成算法细节（block 切分策略、setback 数值、沿街提升的具体公式）。
- DistrictForm 的控件布局（wv-ui 既有 slider/表单件拼装）。
- `MapLevel` 判据与「含 Buildings 图层判据」的取舍（§6 建议前者）。
