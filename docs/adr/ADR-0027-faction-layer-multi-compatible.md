# ADR-0027：势力层多兼容实现（势力层 / 归属边 / 声称实控）

- **状态**：已接受（Accepted）
- **日期**：2026-10-18
- **决策者**：用户对话拍板（2026-10-18：层=维度 / 贵族自持多边形 / `relation` 字段 / `Membership.to`→`target` / 一次只填一个层）
- **关联**：[ADR-0001](ADR-0001-faction-layer-model.md)（本 ADR 是其**实现补充**，不修改 ADR-0001 正文）、[ADR-0013](ADR-0013-layer-frame-binding.md)（图层帧）、[ADR-0021](ADR-0021-render-recipe-and-treatment.md)（渲染配方）、[ADR-0025](ADR-0025-selection-first-tooling.md)（选择优先）、`AGENTS.md` §2.1 / §4.1 / §6.4 / §7.5、`GLOSSARY.md`（势力层 / 势力 / 势力归属 / 声称·实控）
- **前置事实**：ADR-0001 已接受「势力层 / 势力 / 归属」三层模型；但 S12 v0.1 代码只实现了**扁平版**——`World.factions: Vec<Faction>` + `World.faction_geometry`，`Faction` 只有 `id / name / color`，代码中**没有** `FactionLayer`、没有 `Membership`、没有 `rank`、没有声称/实控。

---

## 1. 背景与问题

2026-10-18 用户再次明确需求：

> 「省界应该是势力图的一种，开始定义势力图的时候，是多种兼容的。假设是省，王国，贵族控制多个省份或者城市。」

这与 ADR-0001 的原始裁定（2026-10-11）逐字同源：

> 「假设欧洲法国，有贵族控制多个省或者一个省，几个贵族属于一个雅各宾派。那么贵族领地可能不够分。我的想法是：国界、省份、贵族等等，都叫做势力层。可以命名，然后绘图。」

当前扁平模型无法表达以下任一场景：

| 场景 | 扁平模型为什么装不下 |
|---|---|
| 北境省属于大陈王国 | 没有「上级」字段；只能在名字里写死 |
| 镇北侯实控北境省与河东省 | 没有多对多关系；一个势力只能有一份几何，无法表达「控多块」 |
| 一省同时有两位贵族 | 几何按 owner 存，同一块地画两遍只能靠作者手动复制 |
| 贵族加盟雅各宾派（跨层） | 没有层概念，也没有跨层边 |
| 一块地同时是「省 + 贵族领 + 派系据点」 | 扁平列表可以画重叠多边形，但没有语义区分与层开关 |
| 1789 年前属 A、之后属 B | 几何有 `TimeSpan`，但「归属关系」没有时间区间 |

因此本 ADR 的目标不是重新设计，而是把 ADR-0001 的模型**落到当前代码**，并记录落地时必须补充的细节与对 ADR-0001 的显式偏离。

---

## 2. 决策

### D1. 势力层 = 政治划分**维度**，不是等级；不设 `FactionLevel` 枚举

- 一个 `FactionLayer` 是作者命名的一个划分维度：`国界` / `行政区` / `贵族领` / `派系` / `教团` / `行会`……
- 所有政治实体都是 `Faction`，差异由 `layer` + 可选 `rank` + `memberships` 图表示。
- **等级不是数据模型的字段**：王国与省的区别由「谁隶属谁」的边决定；`rank` 只是渲染提示（见 D2）。
- 同一层内允许混合等级（作者可以把王国与省都放进 `行政区` 层），跨层关系靠 `Membership`。

**理由**：ADR-0001 D2 已裁定「用有限枚举表达无限世界是模型设计错误」。本 ADR 不重新讨论该结论。

### D2. `Faction` 扩展字段

```rust
pub struct Faction {
    pub id: Id,
    pub name: String,
    /// 势力色。省级势力可沿用父级色，由 `FactionLayer.color_mode` 决定。
    pub color: Color,
    /// 所属势力层。`None` = 默认层（兼容 ADR-0001 之前的旧 YAML）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Id>,
    /// 渲染提示（自由字符串，**不是层级**）："kingdom" / "province" / "noble_house" /
    /// "free_city" / "tribe" / "order" …… 未知值照收，仅影响边框粗细与面板徽标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    /// 可选：政治实体的驻地/首府（引用 `Place.id`，P4 只存引用）。
    /// 城市国家 / 自由市用它把「政权」与「地点」接起来；纯地方势力可留空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<Id>,
    /// 本势力对其他势力的归属边（有向、多对多、带时间）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memberships: Vec<Membership>,
}
```

- `rank` 是**开放字符串**，不设枚举。默认边框粗细映射见 D6；未知 rank 走默认档。
- `seat` 只存 `Id`，不复制 `Place` 的名字/坐标（P4）。
- `Faction::new(id, name, color)` 的新字段全部取缺省；既有调用点不改语义。

### D3. `Membership`：有向边，字段名 `target`（修正 ADR-0001 的 serde 冲突）

```rust
pub struct Membership {
    /// 上级 / 目标势力的 ID。ADR-0001 原文叫 `to`——与扁平化的
    /// `TimeSpan.to` 同名，serde 会把结束日静默吞掉，故改名 `target`。
    pub target: Id,
    /// 关系类型（自由字符串，不设枚举）："行政隶属" / "实控" / "名义归属" /
    /// "效忠" / "加盟" / "纳贡" ……
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 关系有效期。半开区间 [from, to)。
    #[serde(flatten)]
    pub span: TimeSpan,
}
```

**字段修正（必须）**：ADR-0001 写的是 `to: Id` + `#[serde(flatten)] span: TimeSpan { from, to: Option<i64> }`。两个 `to` 键冲突——序列化只输出一个 `to`，结束日丢失。本 ADR 把上级字段改名为 `target`，保留扁平 `from/to`：

```yaml
memberships:
  - target: faction_kingdom_chen
    kind: 行政隶属
    from: 1000
    to: 1200        # ← 现在能正确落盘
```

- 边存放在**下级势力**的 `memberships` 中（与 ADR-0001 一致）；`target` 指上级/目标。
- 一个势力可以有多条边；一个目标可以被多个势力指向（多对多）。
- **范围限定**：v1 只实现 **Faction → Faction** 的边。ADR-0001 提到的「人也可以隶属势力层」留给 `Character.memberships` 的未来扩展；本 ADR 不实现，避免把人物模型一起拖进来。

### D4. `FactionLayer` 结构与落盘

```rust
pub struct FactionLayer {
    pub id: Id,
    /// 作者命名："国界" / "行政区" / "贵族领" / "派系" ……
    pub name: String,
    /// 绘制顺序：数值越大越靠上。同 z 按 id 字典序稳定排序。
    #[serde(default)]
    pub z: i32,
    /// 配色策略：Own = 每个势力用自己的 color；Inherit = 优先继承上级势力色。
    #[serde(default)]
    pub color_mode: FactionColorMode,
    /// 数据级可见性（与会话总开关 `ViewState.factions_visible` 合取）。
    #[serde(default = "default_true")]
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactionColorMode {
    #[default]
    Own,
    Inherit,
}
```

- `FactionColorMode` 是**闭合的渲染策略**（不是世界内容），因此允许枚举——与 `LayerKind` 同理：枚举只回答「怎么画」。
- 落盘：`faction_layers/<id>.yaml`，每层一个文件（AGENTS §4.5）。目录可缺失。
- **默认层**：`id = faction_layer_default`、`name = "势力"`、`z = 0`、`color_mode = Own`、`visible = true`。
  - 旧工程没有 `faction_layers/` 时，加载后**在内存中合成**默认层；**不写盘**（V1：作者没动就别替他造文件）。
  - `Faction.layer == None` 或指向不存在的层 → 归入默认层；后者同时产生诊断（D9）。
- **删除一个层**：该层下的势力 `layer` 置 `None`（回落默认层），不删除势力本体（可撤销）。

### D5. 领土：保留 `World.faction_geometry`，新增 `relation`

- **不做** ADR-0001 原文的 `Faction.territory` 内联：几何文件体积大，`geometry/faction/` 的「一 span 一文件」对 Git diff 更友好。`World.faction_geometry` + `owner` 引用在语义上等价，**这是对 ADR-0001 Rust 结构的显式偏离**（见 §3）。
- `GeometrySpan` 新增：

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub relation: Option<String>,   // None = control（实控）；"claim" = 声称；其余自定义
```

- 一个势力可以拥有**多个** span（飞地、据点网、分散领地）；span 之间可以不相连。
- 不同势力 / 不同层之间的多边形**重叠合法**（ADR-0001 R1′）。
- **贵族领地（D2=A）**：贵族自己也持有一组多边形（省/城市/据点），与省份多边形重叠是有意的——它们是不同的政治声称，不是重复数据。代价：同一块地可能被画两遍；由 D6 的「只填当前层」规则消化。
- **未来选项**：`territory = 自有 ∪ 下属领地` 的派生（不重复几何）留作待裁（§5），v1 不做。

### D6. 势力图呈现：一次一个层 + 其他层只画边界

- `ViewMode::Faction` 重新加入 `ViewMode::BROWSING`（第三段）。这是对 2026-10-14「只暴露两种浏览模式」裁定的**单点例外**，不是放开全部 7 种。
- 新增会话资源 `FactionDisplay`（wv-render）：

```rust
pub struct FactionDisplay {
    /// 当前填色的层；None = 第一个可见层。
    pub active_layer: Option<Id>,
    /// 其他可见层只画边界（不填色）。
    pub show_other_borders: bool,
    /// 是否画 relation = claim 的声称层。
    pub show_claims: bool,
    /// 选中势力：自身 + 直接下属高亮，其余淡化。
    pub focus: Option<Id>,
}
```

- **合成规则**：
  1. 可见层按 `(z, id)` 升序绘制；层 `visible` ∧ 会话 `factions_visible`。
  2. **当前层**：填充 + 边界。填充 alpha 基数 0.30（沿用现状），可被 `LayerBinding.opacity` 相乘。
  3. **其他可见层**：只画边界（细线、alpha ≈ 0.55），不填色。避免多层颜色糊在一起。
  4. `focus = Some(id)`：该势力与其**直接下级**（`subordinates_of`）保持满强度；其余层/势力填充降到 0.06、边界降到 0.20。选中本身由选择工具/面板驱动（ADR-0025 同族）。
  5. **声称/实控**：`relation = None` 或 `"control"` → 实填 + 实线；`"claim"` → 低 alpha（0.12）+ 虚线。其他自定义 relation 默认按实控画，但保留字段供未来样式扩展。
  6. **无主地**：没有任何层覆盖的陆地 → 灰显，不报错（ADR-0001 R4′）。
- **边框粗细**（屏幕像素，经 `stroke_world_thickness(px, view.scale)` 换算，保证缩放时屏幕宽度恒定）：

| `rank` | 边框 px | 说明 |
|---|---|---|
| `empire` / `kingdom` / `sovereign` | 3.0 | 国界 |
| `duchy` / `principality` | 2.4 | 公国/亲王国 |
| `province` / `county` / `prefecture` | 1.8 | 省/县 |
| `noble_house` / `free_city` | 2.0 / 1.6 | 贵族领 / 自由市 |
| `None` | 由归属深度派生：根 = 2.6，子 = 1.8 | 作者没写 rank 时的兜底 |

- `rank` 映射是**渲染默认值**，不是数据语义；实机视觉校准后可调，不改数据。
- **`color_mode = Inherit`**：优先取第一条有效上级边的 `target` 势力的 color，与该主题纸面色混合（v1：`mix(parent, paper, 0.35)`）；无上级或上级无色 → 回落自身 color。精确比例待视觉校准。

### D7. 城市：两种形态都允许

| 形态 | 数据 | 适用 |
|---|---|---|
| **地点 + 领地** | `Place`（POI）+ 某个 Faction 的一小块多边形 | 普通城市/贵族控制的城市 |
| **城市政权** | 一个 `Faction { rank: free_city, seat: Some(place_id) }` + 城市多边形 | 自由市 / 城邦 / 城市国家 |

- 两者都不复制 `Place` 的名字/坐标（P4）；`seat` 只是引用。
- v1 渲染对两种形态一视同仁（都是多边形）；`seat` 只在面板显示，不做特殊渲染。

### D8. 迁移与兼容

- 旧 `factions/<id>.yaml`（无 `layer / rank / seat / memberships`）：serde 缺省 → `None / None / None / 空`，全部归入默认层。
- 旧 `geometry/faction/*.yaml`（无 `relation`）：`None` = 实控。
- 旧工程无 `faction_layers/`：合成默认层，不写盘。
- **不升 `schema_version`**：全是可选字段 + 可缺失目录，旧数据零迁移；新数据被旧二进制读取时未知字段被 serde 忽略（前向兼容风险记录在 §5）。
- 必须通过 `load(save(w)) == w` 的旧/新夹具 round-trip（AGENTS §7.2）。

### D9. 查询 API（wv-core）

| API | 输出 | 用途 |
|---|---|---|
| `faction_territories_at(world, map, day)` | `Vec<FactionTerritory { layer: Id, faction: &Faction, relation: Option<&str>, vertices: &[f32] }>` | 渲染主入口；比 `factions_at` 多带层与关系 |
| `factions_at(world, map, day)` | 现有 `Vec<(&Faction, &[f32])>` | **保留为兼容包装**，内部委托新 API |
| `memberships_of(world, faction_id, day)` | `Vec<&Membership>` | 面板 / 关系查询 |
| `subordinates_of(world, faction_id, day)` | `Vec<&Faction>` | focus / 层级聚合 |
| `query_at_point(world, map, point, day)` | `Vec<(Id /*layer*/, Id /*faction*/, Option<String> /*relation*/)>` | 点选 / 未来编辑器 |

- v1 全部是**直接扫描**（势力与边数量小），不建 `MembershipIndex`；ADR-0001 的反向索引留到性能需要时（TODO 标注）。
- 所有查询先按 `TimeSpan::contains(day)` 过滤（P7）。

### D10. 校验：只提示，不阻止（V1）

以下问题进入 `diagnose()` 的结构化诊断，**不阻止保存/编辑**：

- `Faction.layer` / `memberships[].target` / `Faction.seat` 悬空；
- `Membership.span.to < from`；
- 归属边成环（A→B→A）；
- 重复的 `FactionLayer.id`（文件名不同但 id 相同）；
- 势力有领土但 `layer` 指向不存在的层（回落默认层 + Hint）。

`rank` 的未知值**不产生诊断**——它是开放集。

---

## 3. 与 ADR-0001 的显式偏离（只增不改；ADR-0001 正文不动）

| # | ADR-0001 原文 | 本 ADR 落地 | 理由 |
|---|---|---|---|
| 1 | `Membership.to: Id` | `Membership.target: Id` | `to` + 扁平 `TimeSpan.to` 的 serde 冲突会静默丢结束日 |
| 2 | `Faction.territory: Vec<GeometrySpan>` 内联 | `World.faction_geometry` + `owner` 引用 | 几何文件大、一 span 一文件更适合 diff；语义等价 |
| 3 | 每个 `FactionLayer` 生成一个 `Layer` 条目 | v1 用 `Influence` 总开关 + 面板层选择器 | 避免改动 ADR-0013 的 9 槽位与图层树；后续可升级 |
| 4 | 「声称 / 实控」模型上分开 | `GeometrySpan.relation: Option<String>` | 最小字段；自由字符串避免枚举化 |
| 5 | 无 `rank` 概念 | 可选 `rank: Option<String>`，仅渲染提示 | 边框粗细需要作者意图；不参与层级判定 |
| 6 | `Membership.to` 可为人或势力层 | v1 只做 Faction → Faction | 缩小范围；人物归属留待 `Character.memberships` |

---

## 4. 硬约束

| 约束 | 落地 |
|---|---|
| **P4** 只存引用 | `layer` / `target` / `seat` 全是 `Id`；不复制被引用实体的名字/几何 |
| **P6/P7** 历史即区间 | 领土 `GeometrySpan.span`、归属 `Membership.span` 全部带 `TimeSpan`；不存「当前归属」字段 |
| **V1** 校验不是闸门 | 悬空/成环只进诊断；无主地灰显；未知 rank 照收 |
| **V2** 历史可重写 | 作者改任何势力的层/等级/归属都直接可改；不弹窗阻止 |
| **AGENTS §2.1** | `wv-core` 不依赖 `wv-terrain` / Bevy；`Faction` 的 kind 仍是裸字符串，不引入渲染类型 |
| **AGENTS §6.4** | 本 ADR 记录对 ADR-0001 的补充与偏离；ADR-0001 正文不改 |
| **零新依赖** | 仅 serde 派生；不引入 uuid / petgraph / geo |

---

## 5. 后果

### 正面

- 王国 / 省 / 贵族领 / 派系 / 城邦全部可表达，新增政治实体零代码改动。
- 一贵族控多省、一省多贵族、跨层加盟、重叠声称、时间演化全部成立。
- 旧 YAML 零迁移；旧工程打开后自动落入默认层。
- 势力图一次看一个层，视觉可读性可控；其他层只画边界，仍能看见层级关系。
- 声称/实控、rank 边框、focus 都是渲染层规则，不改数据。

### 负面 / 代价

- 概念门槛提高：作者需要理解「势力层 / 势力 / 归属」三概念；面板必须给引导。
- 贵族领地（D2=A）可能重复绘制同一块地；由「只填当前层」与作者自控层选择消化。
- 默认层是内存合成，不在 YAML 里；作者若手改 `factions/` 会看到 `layer` 缺失，需文档说明。
- `Membership` 的 v1 直接扫描是 O(N×M)；N 超 1 万时需索引（ADR-0001 已记待裁）。
- 旧二进制读取新工程会忽略新字段并在保存时丢失——**不要用旧版本打开新工程**（记录在此）。

### 待裁（记录，不在本 ADR 收敛）

| # | 待裁 | v1 选择 | 收敛点 |
|---|---|---|---|
| T1 | 反向索引失效策略 | 每次查询重建 / 直接扫描 | `MembershipIndex` 的 build 时机 |
| T2 | 贵族领地是否从归属派生（不重复几何） | 不派生，贵族自持多边形 | `territory = own ∪ controlled` 的规则 |
| T3 | 每个势力层是否生成独立 `Layer` 条目 | 面板选择器 + `Influence` 总开关 | ADR-0013 的图层树扩展 |
| T4 | `color_mode = Inherit` 的精确混色 | 与纸面色 35% 混合 | 实机视觉校准 |
| T5 | rank → 边框粗细的最终档位 | 见 D6 表 | 实机视觉校准 |
| T6 | 人物是否也能有 `memberships` | 不实现 | `Character.memberships` 扩展 |
| T7 | 同一 `(map, owner, from)` 多条 relation 的文件名 | 文件名追加 relation 稳定哈希 | 若作者频繁同日起算 |
| T8 | 带洞多边形（飞地「回」字） | 不支持，拆多 span | ADR-0001 同名待裁 |

---

## 6. 备选方案（被否决的）

| 方案 | 否决理由 |
|---|---|
| **A. 层级枚举** `enum FactionLevel { Kingdom, Province, Noble }` | ADR-0001 已否决；装不下「省属于王国 + 贵族实控 + 加盟派系」的多重关系 |
| **B. 独立 `AdminDivision` 集合** | 语义更干净，但复制一套几何 + 时间 + 编辑命令；且「省」与「国」在数据上本质都是政治实体，拆成两套模型会让「省升级为国」变成跨模型迁移 |
| **C. 把省界做成 `Border` 图层的作者笔画** | 省界不是作者随手画的线，而是省多边形推导出的边界；笔画无法随时间/归属变化自动更新 |
| **D. 所有层同时填色叠加** | 3 层以上颜色糊成泥；违反「负空间 / 色调明确」的可读性要求 |
| **E. `Faction.territory` 内联几何** | 几何文件大、diff 噪音大；当前 `geometry/faction/` 已被证明可用 |
| **F. 声称/实控合并为单字段** | ADR-0001 已否决；二者常不一致，合并即丢作者意图 |
| **G. `Membership.to` 不改名，嵌套 `span`** | 可行但 YAML 更啰嗦（`span: { from, to }`）；改名 `target` 更短且一次性解决 |

---

## 7. 参考

- `docs/adr/ADR-0001-faction-layer-model.md`（三层模型、重叠规则、三个查询 API、法国例子）
- `docs/MAP_EDITOR_BASELINE_v1.md` §2.5 / §2.6 / §2.10 / §5.1
- `docs/MAP_LAYER_RENDERING_SPEC_v0.1.md`（`Influence` / `Border` 通道、Z 槽位）
- `GLOSSARY.md`（势力层 / 势力 / 势力归属 / 无主地 / 声称·实控）
- 代码：`crates/wv-core/src/model/faction.rs`、`crates/wv-core/src/map/mod.rs`、`crates/wv-io/src/project.rs`、`crates/wv-edit/src/command.rs`、`crates/wv-render/src/faction_overlays.rs`
- 实施计划：`docs/superpowers/plans/2026-10-18-faction-layer-multi-compatible.md`
