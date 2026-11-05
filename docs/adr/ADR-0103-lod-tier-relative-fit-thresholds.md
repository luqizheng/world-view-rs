# ADR-0103: LOD 档位阈值解绑绝对 scale——比例口径

- 状态：已接受（2026-11-05，用户拍板排期「1-3-2」中的 #3；实施当日完成）
- 关联：ADR-0009（分级缩放模型，`fit` 的来源）、ADR-0030（POI 分档硬门，
  阈值消费方）、ADR-0047（统一呈现门槛）、ADR-0091（档位单真源）、
  ADR-0102（等高线档门，迁移到比例口径）、README §12（待裁收敛）

## 处境

`lod_tier(view_scale)` 用两个**绝对**阈值（3.0 / 1.0）切 Nation/Region/Edit
三档。数值锚在「4096² 世界图 / 800px 窗口」的 `fit = 5.12` 上（3.0 ≈ 全图
60% 可见）。分级缩放模型（ADR-0009）下每张子图有自己的 `[fit/32, fit×1.56]`
区间：城市图 1024 在 800px 窗口的 `fit = 1.28`、**外界只有 2.0**——
`scale ≥ 3.0` 永远不可达，即**城市图上 Nation 档整套语义（含 its 消费方：
装饰带、标签预算、建筑淡出、POI 硬门、等高线门）全部死锁**；街图更甚。
换窗口尺寸同样让阈值漂移（4K 屏 fit 更大，Nation 更难到达）。

## 决策

1. **阈值改比例口径**：`scale ≥ LOD_NATION_RATIO × fit`（0.5859375）/
   `scale ≥ LOD_REGION_RATIO × fit`（0.1953125）。比值 = 3.0/5.12、1.0/5.12
   ——4096 图 / 800px 窗口下与旧绝对常量**逐位重合**（演示工程零行为变化）。
2. **`ViewState.lod_fit: f32`**（缺省 5.12 = 历史锚点）由
   `wv-app::seed_drill_state` 每帧维护：`fit_scale(GridConfig.map_size,
   view.window_size)`——`GridConfig.map_size` 就是当前图 `extent` 镜像，
   与 `current_map` 同点同步。
3. **`lod_tier(view_scale)` → `lod_tier_at(view_scale, fit)`**：全部
   ~13 个消费点迁移（wv-render：drill / vector_layers ×4 helper / 
   terrain_decor / label_collision / buildings / cellmap_bridge/isoline；
   wv-app：edit/cell、pin_drag、dispatch、toolbar）。旧常量
   `LOD_NATION_SCALE` / `LOD_REGION_SCALE` 保留为**历史锚点**（测试与
   缺省值引用），不再参与活跃判定。
4. `fit` 退化（NaN / ≤0）回落锚点 5.12——档位永不死锁。

## 理由

- 档位语义是「看到全图的几成」，天然是**相对量**；绑绝对 scale 是 4096
  单图时代的遗产。
- 零行为变化锚点让本重构可独立验收：demo 上逐位一致，小图上才出现新行为
  （城市图 0.75 即 Nation、0.25 即 Region——修正死锁）。

## 代价 + 被否决的备选

**代价**

- 同一 scale 值在不同图幅下档位不同：比例尺读数/截图对比要注明当前图。
  这是语义修正的本意，不是缺陷。
- 探针（`WV_SCALE` 等）写 `view.scale` 但不写 `lod_fit`：缺省 5.12 保持
  旧行为——探针本来只对 4096 演示图有意义。

**被否决的备选**

- **每图幅存三档绝对阈值**：数据冗余，`fit` 本身已唯一确定三档。
- **阈值进 ThemePack / 配置资产**：档位是渲染层口径（ADR-0047 收口在
  `rank_visible_at_tier` 一个 match），不是作者可调项。
- **维持绝对值 + 给小图单独常量**：第二套特判比比例公式更难维护。

## 验收

- 单测（wv-render/state.rs）：`lod_tier_at_scales_thresholds_with_the_
  current_level_fit`（城市图 2.0→Nation、0.25→Region；4096 图逐位重合）、
  `lod_tier_at_falls_back_on_degenerate_fit`（NaN/0/负/∞ 回落锚点）。
- 全量门禁（wv-render + wv-app + cellmap + terrain）绿。
- 演示工程实机：无可见变化（零行为变化的推论，抽查确认）。
