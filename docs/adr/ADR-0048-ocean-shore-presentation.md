# ADR-0048: 海洋表现——离岸距离场、近岸浅滩带与浪花带

- 状态：Proposed（2026-10-21 草稿，S0/S1 已随 c74a175 并入 main，S2 已实现于 wip/ocean-shore-band，S3 未做）
- 关联：ADR-0035（soften_keep 值被本 ADR 修订）、ADR-0036（VEIL_MAX_ALPHA 值被修订）、
  ADR-0039（主题演绎规则——本 ADR 的岸线表现同属 cell 驱动演绎）、ADR-0044（水彩晕开——
  浅滩 tint 同样收在 base_color_of）、README §12-48

## 处境

work-view-test 实机验收（2026-10-20）：海占视口约 44%，其中 56% 的海色挤在两个颜色上；
整幅「发闷发灰」。逐层归因（`diag_sea_bake` 探针，烘焙 vs 截图直方图对表）：

1. **深度量程错配**：`water_depth_t` 线性量程 3500 m，而该图 1/4 水格深 < 400 m
   （线性 t ≤ 0.11）——近岸与远海在色板上几乎同色；
2. **双层洗白叠加**：烘焙期 soften（15% 向纸色）+ 默认浏览缩放吃满的 veil（18%），
   把海面主导色从烘焙基色 (68,134,168) 抬到实测 (121,160,180)；
3. **近岸缺几何事实**：用户要「近岸更浅 + 浪花」，而全仓库没有任何离岸距离场——
   水深与岸线位置无几何关系（DEM 近岸剖面陡缓不一，深度浅 ≠ 离岸近）。

## 决策

**S0（观感调参，已落地 c74a175）**

- `water_depth_t` 线性 → **平方曲线**：浅水段动态范围约 3×（400 m：0.11 → 0.34），
  近岸/远海一眼可辨；
- `JRPG_SOFTEN_KEEP` 0.85 → **0.92**（修订 ADR-0035 的锚值，方向不变：洗白更轻）；
- `VEIL_MAX_ALPHA` 0.18 → **0.12**（修订 ADR-0036 的锚值；保留远景纸面空气感）。

**S1（离岸距离场，已落地）**

- 新模块 `wv_terrain::shore`：多源 BFS（8 连通均匀代价 = **切比雪夫格距**），陆格为种子，
  截断在 `DEFAULT_MAX_DIST = 12` 格；**纯函数、有界支撑**（任一格的值只依赖半径内
  水陆分布——渲染增量 patch 按支撑半径膨胀脏区即可保持「局部 = 全图」逐字节一致）；
- 与主题无关（水彩 / 水墨共用）；归 wv-terrain（域规则层，`is_water_cell` 先例），
  不归 wv-render。

**S2（近岸浅滩带，已落地 wip/ocean-shore-band）**

- 新主题槽位 `PaletteColors.water_shore`（日间 `#A9D3D5` / 暮色 `#6C8090`）+
  旗标 `TreatmentFlags::shore_shallow`（HAND_DRAWN 开 / EMPTY 关）；
- tint 收在 `base_color_of`：离岸 ≤ `SHORE_BAND_CELLS = 6` 格的水格向 `water_shore`
  smoothstep 混合（水线峰值 `SHORE_TINT_MAX = 0.85`）——放在基色一步，
  「模糊输入 = 直铺输出」契约（ADR-0044）不破；
- 仅 VectorFlat / Schematic（平涂语法，与 bleed 同口径）；
- `patch_terrain_image` 内部把脏区按场支撑半径（12 格）膨胀——契约收在 patch 自身，
  调用方无感。

**S3（浪花带，未做，方向预登记）**

- **独立渲染 pass**（新 `PassId`），不烘进地形纹理——绕开局部脏区契约；
- 几何 ribbon：从距离场提取「离岸 X 格」等值线（`height_segments` 本就 level 参数化），
  对场做 fbm 扰动后 marching squares，浪花线自然破碎；顶点色 alpha 沿法向渐变；
- Z 落在海岸 halo（0.036–0.038）之上、主线（0.04）之下；新色板槽位 `surf_foam` +
  旗标 `shore_foam`。

## 理由

- **修曲线先于修色板**：色板两端（shallow/deep）本就区分度足够，是量程把区分度压没了；
  平方曲线一行改动、可随时回退。
- **洗白让位给覆盖层**：soften（烘焙期）与 veil（覆盖层）职能重叠（都洗向纸色）；
  把烘焙期洗白压轻、保留覆盖层职能，要素让位机制不动。
- **距离场必须截断**：真全局距离场与「局部脏区 patch」契约根本冲突（填掉一个湾，
  湾内所有格的距离都变）；截断在 12 格 = 有界支撑，是增量正确性的充要条件，
  且浪花/浅滩带用不到 12 格以外的距离。
- **tint 放基色而非采样末端**：水彩晕开以基色为模糊输入；tint 若在末端，
  晕开与直铺两条路径会漂移出拼接缝。

## 代价

- 洗白减淡是**全局观感变化**：修订 ADR-0035 / ADR-0036 两个已接受锚值（方向与两 ADR
  原意一致——都为「保色相」而调）；暮色主题共享 `VEIL_MAX_ALPHA` 常量，雾感随之略减
  （其自身 0.35 soften 主导，影响小）。
- 浅滩场每次合成重算（O(N) BFS，1024² ≈ 数 ms）：接受（同 Topographic 阈值口径，
  ADR-0039 §4.1.1）；后续若成热点，缓存口子 = `terrain_data_hash` 键控（isoline 先例）。
- patch 脏区膨胀 12 格：岸线附近笔刷的 patch 面积变大（约 +24 格环带），换增量正确性。
- 浅滩带只在平涂两模式生效：RealTerrain / GoogleMap 等不参与（语法不同源）。

## 被否决的备选

- **复活位图图集（tile atlas）**：4096² 纹理预算下逐格贴图只有 4 px/格（work-view-test）
  ——马赛克；且参考对象 civ-atlas 的质感实为**程序噪声**而非位图（fantasy.ts = 平涂 +
  boxBlur + hillshade；realistic.ts = 连续色板 + fBm）。「贴图」路线只在「世界空间平铺
  无缝纹理 + biome 掩膜」下才有意义，留待后续单独评估。
- **真全局距离场 + 全图重算**：正确性无懈可击但每次笔刷都全图重算，浪费；
  截断场 + 脏区膨胀以零感知成本达成同洁局。
- **浪花烘进地形纹理**：受 res（px/格）限制锯齿化，且把「随缩放变化的带宽」烧死在
  纹理里；独立 pass 可用亚格精度。

## 测试与验收锚点

- `wv-terrain`：shore 模块 11 测（直岸递增 / 切比雪夫 / 截断 / 全水全陆 / 湖 /
  支持有界性 / 双线性采样 / 退化输入）；
- `wv-render`：`shore_shallow_tint_lightens_waterline_and_spares_offshore`（带内提亮、
  带外与旗标关逐位一致）、`shore_tint_patch_with_dilated_rect_matches_full_bake`
  （膨胀后 patch = 全图重算逐字节）；
- 实机：`diag_sea_bake` 归因探针 + `shore_heatmap` 距离场热力图 + GUI 截图直方图
  （海明度 5%–95% 带宽 146–169 → 126–155，主导海色 #79A0B4 → #628BA5）。
