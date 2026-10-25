# ADR-0063: 专业制图的边界线层——势力层退化为「国境线」

- 状态：**已接受**（2026-10-25，随 `feat/professional-cartography` 落地）
- 关联：ADR-0027（势力图层：分层填色 + 边界 + 声称/实控）、ADR-0061（专业制图范式）、
  ADR-0062（群系矢量面）、专业地图样张 `preview/professional_map_preview.html`

## 1. 处境（Context）

专业地图样张的图例有一项「**边界 · 虚线 #C4C4C4**」，而 ADR-0061 完全没提边界层。
核对其渲染路径后发现三处不匹配：

| 现状 | 位置 | 为什么不合用 |
|---|---|---|
| `PassId::Faction` 只在 Faction 模式开 | `recipe.rs` MODE_TABLE | 专业制图下边界根本不画 |
| 主 system **恒给当前层填色** 0.30 alpha | `faction_overlays.rs` `FACTION_ALPHA` | 普通地图不按政治染色 |
| 边界是 casing（深墨）+ inner（势力色）**双线**，Z = 0.3 | 同上 `draw_border_ring` / `FACTION_Z` | 双线比道路还重；Z 0.3 盖住 POI（0.21） |

另有一处语义冲突要先解决：虚线在 Faction 模式里**专表 `relation = claim`**
（声称 / 实控二分）。若专业模式也用虚线，两种含义会混。

## 2. 决策（Decision）

**D1 · 复用 `PassId::Faction`，不新建 pass**

GoogleMap 行由 `false` 改 `true`。pass 开只是让 `sync_faction_overlays` 跑起来；
填色是否发生由样式函数决定。`PassId::COUNT` 是编译期常量，加一条要动全部模式行
与测试，收益不抵。

**D2 · 新增 `cartographic_style(is_active, is_claim)` 纯函数**

| 输入 | 输出 |
|---|---|
| 当前层 + 实控 | `fill: None, border: Some(1.0), dashed: true` |
| 其他层 | `None`（不画） |
| 当前层 + 声称 | `None`（不画） |

三条判据都是「一眼看懂」而不是「看懂谁的领土」：不填势力色、不叠多层边界、
不表达司法争议。

**D3 · 单线 + 固定细线 + 主题灰**

- 跳过 casing 层（`draw_border_ring` 增 `single_line` 参数）；
- 线宽固定 `CARTO_BORDER_PX = 1.2` 屏幕像素（不按势力 rank 变粗）；
- 颜色走 `wv_theme::colors_google::GOOGLE_BORDER = #C4C4C4`（样张图例值），
  **不用**势力图的 `BORDER_INK`（深褐手绘墨线）。

**D4 · Z = 0.086**

压在河流（0.082 / 0.084）之上、道路（0.09）之下。不能沿用 `FACTION_Z = 0.3`
——那是「填色 + 墨线」的槽位，会盖住 POI。普通地图里行政界要给道路与注记让路。

**D5 · 虚线语义：本模式不画声称，故不冲突**

`cartographic_style` 直接排除 `is_claim`，所以专业模式里「虚线 = 边界」是唯一
解释。Faction 模式维持「实线 = 实控 / 虚线 = 声称」的既有二分，两模式各自自洽。

## 3. 理由（Why This）

- **零新数据**：边界几何来自既有 `faction_territories_at`（时间求值在 wv-core），
  洞环描边已支持（国中国的洞沿就是内国边界）。
- **不新建模式 / pass**：专业制图已有专属入口（ADR-0061），边界只是该范式下的
  一个图层语义；新 pass 会加重 `PassId::COUNT` 的连锁维护。
- **样式判据集中**：只在本模块一个纯函数里，符合 ADR-0061 追记一提出的
  「mode → style 判据要可点数」的方向。

## 4. 代价与风险（Cost）

- **只暴露当前层**：作者若有多层势力（国 / 省），专业地图只画 `active_layer`
  的边界。可接受——普通地图通常只画一条国界；要省市界需按 layer rank 分层
  （见遗留）。
- **虚线密度固定 6px 段 / 4px 隙**（`DASH_PX` / `GAP_PX`，屏幕像素）：远观会
  退化成点线，与样张在深缩放下的观感可能有差；实机确认后可做 LOD 变体。
- **与 Faction 模式共享几何管线**：同一份 `faction_territories_at` 每帧重建；
  势力数量小（典型 < 20），可忽略。

## 5. 被否决的备选（Alternatives）

- **新建 `PassId::FactionBorders`**：`PassId::COUNT` 连锁改动；语义上仍是同一层。
- **在 GoogleMap 下沿用势力色**：普通地图不按政治染色（ADR-0061 的核心裁定）。
- **沿用 casing + inner 双线**：在近白底图上过重，边界会压过道路。
- **用实线区分国界、虚线留给声称**：样张明确要虚线边界，且本模式不画声称。

## 6. 验收（Acceptance）

1. `cartographic_style_is_border_only_for_active_control`：三态样式判定。
2. `google_mode_disables_atmosphere_veil_buildings_and_keeps_faction`：模式表语义
   （原 `google_mode_disables_atmosphere_faction_buildings` 已按新语义更名改断言）。
3. `wv-render` 373 测全绿；`smoke-gui.sh` PASS。

## 7. 遗留

- **国界 / 省界分级线型**：需要势力层的 rank 语义（现只有 faction rank），
  另裁。
- **边界文字（国名沿边界排布）**：标注层未支持沿路径排版。
- **与自然区边界的关系**：`Region` 轮廓仍走 `RegionLayers`（GoogleMap 行关），
  普通地图是否需要显示作者划的自然区，待裁。
