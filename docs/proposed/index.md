# 提议区（未获决策者确认）

> [!CAUTION] 快照 2026-10-21
> 本区收录状态为**「提议（Proposed）」**的 ADR——**已写出、尚未获得决策者确认**。
> 按 `docs/adr/README.md` §3 纪律：**此时不得据此实现代码**（个别条目实现已先行落地，见各条说明）。
> 提议一旦获确认即转入 [现行口径](/current/)；被否决则转入 [演化史](/evolution) 并新开 ADR。

## 当前提议（3 份）

### 1. 🔶 界面字体打包 —— 拉丁 Inter + 中文 Noto Sans SC（编译期嵌入）

**提议内容**：`include_bytes!` 编译期嵌入两份 OFL 字体；egui `Proportional` 逐字形回退（`[Inter, NotoSC, 内置]`）；`Monospace` 数字仍走系统等宽；Bevy `Text2d` 用 NotoSC 单句柄；系统 CJK 探测删除、等宽探测保留。追记（2026-10-20）：追加霞鹜文楷 v1.530 作画布标签书法体（UI 不用）。

- **状态依据**：ADR 原文写明「用户已下达字体指令……**待用户确认后转「已接受」**」——**实现已在代码中落地**，但决策状态仍是提议。
- **原文**：[ADR-0019](/adr/adr-0019-bundled-ui-fonts) · **主题域**：[平台与基础设施](/current/platform)

### 2. 🔶 POI 呈现级别（rank）与屏幕密度预算

**提议内容**：`Place.rank` 可选字段（0 网络节点 / 1 普通 / 2 市镇 / 3 国家，`None` 按旧两档折算，零迁移）；呈现门槛 = 档位 × 最低级别硬门；密度预算 `POI_MAX_DOTS = 400` 按 `(rank, is_city, 名字长)` 裁剪；POI 材质合批 ≤4 mesh + 资产原地复用；道路段列缓存 + 线宽量化 4%。

- **状态依据**：ADR 原文「2026-10-18 起草；实现按本 ADR 进行，**用户可否决后重开**」。
- **与现行部分的关系**：其中的「呈现级别词表」已被 [ADR-0047](/adr/adr-0047-uniform-presentation-rank) **扩表接入**（非取代）——`rank_visible_at_tier` 统一门槛属**现行**；密度预算部分仍属提议。
- **原文**：[ADR-0030](/adr/adr-0030-poi-rank-and-density-budget) · **主题域**：[呈现与 LOD](/current/presentation-lod)

### 3. 🔶 主题演绎规则 —— cell 属性驱动的覆盖物变体

**提议内容**：`DecorKind::OceanWaves`（浅海带白弧散点）；森林按 y 轴气候变体（针叶 / 阔叶 / 绿洲）；山装饰 ≥3400 m 雪岩双色。规则先落 Rust 常量表，签名稳定后随 ADR-0023 M3 迁 ThemePack YAML。

- **状态依据**：ADR 文件头部**「状态：提议（Proposed，2026-10-20）」**。
- **⚠️ 索引表冲突**：`docs/adr/README.md` 索引表把此条记为「已接受」——与其文件头部冲突。按 §3 纪律（不一致时**以 ADR 头部为准**），本站按提议处理。冲突已记入生成报告。
- **原文**：[ADR-0039](/adr/adr-0039-theme-derivation-rules)（`ADR-0039-theme-derivation-rules.md`，注意与 `ADR-0039-topographic-viewmode.md` 撞号）· **主题域**：[地形与装饰](/current/terrain-decor)

---

## 提议态的行为约束

| 约束 | 说明 |
|---|---|
| **不得据此实现代码** | `docs/adr/README.md` §3：提议态「此时不得据此实现代码」 |
| **不阻断编辑** | 但按 §7.5 V1，校验只提示不阻止——提议相关的不一致同样只提示 |
| **转正路径** | 决策者确认 → 状态改「已接受」 → 索引表更新 → 本站下次生成时转入现行 |

> 关联：[现行口径](/current/)（已接受 47 份）· [待决问题](/pending)（README §12 开放问题）· [演化史](/evolution)（被推翻的决策）