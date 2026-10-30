# ADR-0091: 地名标签显隐门槛对齐符号在场（rank ≥ 2 全档有名字）

日期：2026-10-30 ／ 状态：已接受（用户在「Browse POI 没有文字」诊断报告上拍板选 A）

## 处境

实机反馈（2026-10-30，fix/label-atlas-blur 发糊验收时）：世界图远观（Nation 档，
scale ≥ 3）下 POI 图标在场而名字缺席，观感如「Browse 没有文字」。WV_SHOT 同机位
A/B 实证与 Browse/Map 模式无关，根因是**标记与标签的档位门不对称**：

| 元素 | 门槛（改动前） | 出处 |
|---|---|---|
| 城市符号 `landmark_dots` | rank ≥ 2 **全档都画** | ADR-0047 D2（「全球档可见城市符号」） |
| 微点点标 `place_dots` | `poi_visible_at_rank`（Nation ≥ 3） | ADR-0030 D2 |
| 拾取门 | `rank ≥ 2 ‖ poi_visible_at_rank` | UX-032 单真源审计 |
| **标签门** | `poi_visible_at_rank`（Nation ≥ 3） | ADR-0030 D6 沿用 |

即 rank ≥ 2 的市镇「有图标无名字」。jrpg-world 8 锚定城中 5 个 rank2 全部中招，
3 个 rank3 又因视口外/浮窗遮挡只剩 1 个有字，放大了观感。拾取门早已是并集口径
（可拾取=渲染上可见），唯独标签掉队。

## 决策

1. 新增单真源 [`wv_render::state::place_visible_at_rank(rank, tier)`] =
   `rank ≥ 2 ‖ rank_visible_at_tier(rank, tier)`——语义 = 「符号在场」
   （城市符号 ∪ 点标的并集）。**标签门与拾取门都收口到它**。
2. `poi_visible_at_rank` / `rank_visible_at_tier` 语义不变，继续供
   `select_pois`（微点点标）、道路 / 街道 / 河流使用。
3. 碰撞预算不变（Nation 档 Place=12，远观 6 城余量充足）。

## 理由

- 项目定位（AGENTS §7.5）：工具帮创作者**整理与查看**自己的世界——自己起的
  城名在世界图上应当与它的图标同生死，图标在场而名字缺席读起来像缺陷。
- 拾取门（UX-032）已经裁定「可拾取 = 渲染上可见」；标签是同一符号的伴生
  信息，跟随同一并集是**消除第二份真值**而非引入新政策。
- 远观 6-8 个标签远低于 ADR-0073 密度预算，无性能与可读性代价。

## 代价 + 被否决的备选

- 代价：Nation 档标签密度上升（本演示 1→6 个）；远观只标国家级的 Google Maps
  式惯例被放弃——对以「世界设定整理」为本位的产品，可读性优先于制图惯例。
- 备选 B（维持现状，滚近见字）：被否——图标/名字分离的观感即本次反馈本身。
- 备选 C（只把演示数据升 rank3）：被否——治标，产品问题原样保留。
- 触及原则：不触 P1–P7（纯呈现层门槛语义）；修正 ADR-0047 D2 的标签随行解读，
  按 §6.3 以本 ADR 记录，不回改旧文。

## 实施

- `crates/wv-render/src/state.rs`：`place_visible_at_rank` + 单测
  `place_symbol_gate_admits_rank2_at_every_tier`。
- `crates/wv-render/src/drill.rs`：`drill_pick` 拾取门与 `update_place_labels`
  标签门改引单真源；门槛行为测试补 rank2@Nation=true、rank1@Nation=false 断言。
