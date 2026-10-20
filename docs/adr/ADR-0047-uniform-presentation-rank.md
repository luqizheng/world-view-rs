# ADR-0047: 全要素统一呈现级别——道路与河流接入「rank × 档位」体系

**状态**：已接受（Accepted，2026-10-20，用户裁定「按照你的推荐把」——推荐即本文的最小改动路径：`PlaceEdge` 与河流补可选 `rank`、三条散落的硬编码阈值统一成一张真值表）

---

## 1. 处境（Context）

用户问：「道路、POI、河流应该由级别，在不同的 LOD 决定是否显示吧。现在有什么机制做这件事？」

逐要素盘点（2026-10-20 实况）：

| 要素 | 级别数据 | LOD 筛选 | 密度预算 |
|---|---|---|---|
| POI（`Place`） | ✅ `Place.rank`（ADR-0030 D1） | ✅ `poi_visible_at_rank` 硬门（ADR-0030 D2） | ✅ `POI_MAX_DOTS` + 视口裁剪 |
| 城市街道（`StreetSegment`） | ⚠️ 有但是派生值：`RoadClass::lod_rank` 0–7 | ✅ `street_kept_at_tier`（阈值 **硬编码散落**在 wv-render） | ❌ |
| 世界道路（`PlaceEdge`，即地点边连线） | ❌ **无**（只有 distance_km / terrain / bidirectional） | ❌ 无筛选，任何缩放档全量进 mesh（guangdong 87k 边的性能隐患） | ❌ |
| 河流（`rivers_seed` 派生 ∪ `Place.line` 作者河） | ❌ **无** | ❌ 无（gate 只有模式轴 + terrain 图层） | ❌ |

已有的两根轴：缩放档 `lod_tier`（Edit / Region / Nation，ADR-0009 同构）与模式轴 `RenderRecipe::MODE_TABLE`（ADR-0021）。缺的是**第三根轴**——要素自身的呈现级别——以及把已有但不统一的街道阈值收口。

## 2. 决策（Decision）

### D1 · 数据模型 = `PlaceEdge.rank: Option<u8>`，缺省按距离折算

与 `Place.rank`（ADR-0030 D1）同一张呈现级别词表：`3` 国家级干线 / `2` 区域级 / `1` 局部道路 / `0` 仅编辑级。serde `default + skip_serializing_if`，旧 YAML 零迁移。

`None` 时 `PlaceEdge::effective_rank` 按 `distance_km` 折算：**≥ 100 km → 3、≥ 30 km → 2、否则 → 1**。折算规则是冻结契约（测试 `place_edge_effective_rank_folds_by_distance` 钉死）；显式 `rank` 永远压过折算。

### D2 · 统一门槛 = `rank_visible_at_tier`（扩表，不是新表）

Nation 档放行 rank ≥ 3、Region 档 ≥ 2、Edit 档全见——阈值只存在于 wv-render `state.rs` 这一个 match。`poi_visible_at_rank` 降级为别名。**与 ADR-0030 的关系是扩表**：同一张呈现级别词表、同一道门槛，消费方从 POI 一类扩到四类；不是另立体系。

### D3 · 街道接入 = lod 折算表 + 委托，行为零变化

`RoadClass::presentation_rank_from_lod`：lod 0–2（高速/干道/主干）→ 3、3–5（次干/支路/住宅/未定级）→ 2、6–7（服务/土路/小径类）→ 1。`street_kept_at_tier` 改为「折算 + `rank_visible_at_tier`」的委托；等价性测试（`street_tier_gate_equals_legacy_thresholds`）在全部 8 个 lod 档 × 3 视野档上与旧硬编码阈值逐位对表——**街道呈现行为零变化**，只是并入统一口径，旧阈值不再散落。

### D4 · 世界道路渲染 = 降序前缀切片 + 分档指纹

`compute_projection` 按 `effective_rank` **降序**收集段列，一次算齐三档的前缀切割位（`road_rank_cuts`）与各自内容指纹（`road_tier_fingerprints`）；绘制侧按当前档位取 O(1) 前缀切片。段列缓存门（`road_key`，ADR-0030 D5a）**不含 scale**——缩放不重算段列，跨档只是换 mesh（指纹判变），与线宽量化（ADR-0030 D5b）叠加。

### D5 · 河流级别 = 折线长度占图幅宽的比例折算

**≥ 0.40 → 3（横贯级主河）、≥ 0.15 → 2（区域河）、否则 → 1（溪流）**。占比口径与图幅尺寸无关（4096² 世界图与 512² 城市图同一把尺）。派生河（`rivers_seed`）与作者河（`Place.line`，ADR-0046 双供给）**共用同一折算**——「双供给同一呈现语义」；图幅未知的作者河归 rank 1（亚格数据本质是局部细节，宁可少显示）。绘制侧过 D2 统一门槛。

## 3. 理由（Rationale）

- **为什么级别落到边，而不是从端点地点推**：边的端点 rank 是「地点重要性」，不是「这条路线的重要性」——同一对城市间可以同时有官道（rank 3）与山道（rank 1，ADR-0030 时代就并存多边）。用端点推边会一刀切。
- **为什么距离是缺省折算**：长边是城际干线的天然代理；Overture 导入工程（数万边）不改一行 YAML 即获得「Nation 档只画干线」的主干优先呈现。作者对折算不满时显式 `rank` 覆盖（显式永远赢，与 `Place.rank` 同纪律）。
- **为什么河流用占比不用绝对长度**：河流折线来自归一化 `[0,1]²` 高度场派生，绝对长度随图幅缩放；占比在任意图幅下语义一致（「横贯图幅的河」在任何尺寸的图上都该远观可见）。
- **为什么不把级别门槛下沉进 Recipe MODE_TABLE**：MODE_TABLE 是「模式 × pass」轴（ADR-0021），级别 × 档位是正交的第三维；混进真值表会让表从 `8 × 24` 膨胀到 `8 × 24 × rank`，且档位门槛已被证明该是纯函数（ADR-0030 D2 先例）。
- 回溯到 P7（当前值 = 档位求值，不是存结论）与 Google 地图式「每级只画该出现的要素」（ADR-0008 的观感承诺、ADR-0030 的未竟后半）。

## 4. 代价（Consequences）

- **表现层魔法数增加**：100/30 km、0.40/0.15、lod 三段——全部集中单点（wv-core 折算入口 / wv-render 门槛）+ 冻结契约测试 + 本 ADR 登记；调整只动一处。
- **距离折算是代理不是真级别**：一条 120 km 的山道会被当干线画到 Nation 档。作者可显式降级；工具不替作者做决定（V1）。
- **道路线宽不随 rank 分档**（deferred）：需要按 rank 分池分材质，等实机观感反馈再立项；河流同理（现仍统一 2.2px）。
- **跨档瞬间 mesh 重建**：级别门槛切换内容 → 指纹变 → 换 mesh。已有线宽量化 + 空列表快路径缓解；河流 ≤ 12 条代价可忽略。
- **wv-render 测试不在标准门禁编译范围的历史缺口再次咬人**：本次顺手修复了两例带病测试（`cells.rs` 填充边测试没关 ADR-0044 水彩晕开、`terrain_decor.rs` 测试没初始化 ADR-0045 的 `RegionOverlayIndex`）与一例合并回归（`world_projection.rs` 测试夹具丢 `rank: None`）。**教训照 §7.1 旧账：凡动 wv-render，`cargo app-test` 必跑。**

## 5. 被否决的备选

| 备选 | 否决理由 |
|---|---|
| per-rank 淡出带（每级一条 fade 曲线） | ADR-0030 D2 已否决（档位硬门 + 缩放运动掩盖 pop-in）；本 ADR 沿用 |
| `PlaceEdge` 加 `RoadClass` 式枚举 `class` | 重演 `wv-core::source::street::RoadClass`；u8 与 `Place.rank` 同词表，跨实体一致、作者零学习成本 |
| 河流按绝对长度定级 | 与图幅尺寸耦合：世界图上的「长河」阈值套到城市图会把每条沟都定成主河 |
| 作者河固定 rank 1（按亚格语义） | 作者在世界图上手绘的长河会被 Nation 档藏掉；长度占比折算让双供给自然各得其所 |
| 级别门槛进 Recipe MODE_TABLE | 见 §3；两轴正交，合并即表爆炸 |
| 世界道路复用街道 LOD（每帧按 tier 过滤全量段列） | 每帧 O(E) 过滤 + 指纹重算，guangdong 规模下正是要消灭的每帧成本；降序前缀 + 切割位把过滤变成 O(1) 切片且缩放零重算 |

## 6. 原则触表

- **P3**（YAML 为主）：`rank` 可选字段，serde 缺省 + 不写键，零迁移。
- **P7**（时间轴驱动）：rank 是静态属性非时变量，不入 span；若未来需要「某时期级别不同」，按 §4.1 区间化（当前不做，登记为扩展方向）。
- **AGENTS §4.2**：rank 不是行程时间结论（时间仍只由 `distance_km + terrain + Vehicle` 计算），呈现属性不参与路径搜索。
- **V1**（校验不闸门）：分级裁剪纯渲染侧，不阻止编辑、不阻止保存。
- **P5**（图层化）：不动 `LayerFrame` 的 kind 轴开关；级别轴与图层轴正交叠加。

## 7. 关联

- ADR-0030（呈现级别与密度预算——本 ADR 是其「扩表到全要素」的续篇，不取代）
- ADR-0010（街道矢量层——`street_kept_at_tier` 的阈值来源，现收口进统一门槛）
- ADR-0021（RenderRecipe——模式轴，与级别轴正交）
- ADR-0037 D5（河流派生）/ ADR-0046（作者河双供给——本 ADR D5 的级别语义覆盖两路供给）
- ADR-0009 / ADR-0012（LodTier 三档体系）
- README §12-48（登记缺口，道路半边由本 ADR 关闭；海洋半边另案）
