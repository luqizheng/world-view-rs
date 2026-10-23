# 地图渲染实施方案 v0.1（世界地图整体 + 地形地貌）

> 项目：World View（worldview-rs）——单机优先、目录即工程、YAML 驱动的世界观构建工具
> 定位：`docs/ART_STYLE_SPEC_v0.1.md`（下称 SPEC）的**落地实施方案**——回答"怎么从现状走到 SPEC 要求的画面"，不是重写 SPEC
> 依据：SPEC 原文 + 代码实读（wv-render / wv-theme / wv-terrain / wv-app，2026-10-22 HEAD）+ ADR-0008/0009/0012/0016/0020/0023/0026/0030/0031/0032/0035/0038/0039/0040/0041/0042/0043/0044/0047/0048/0049
> 署名：技术架构章 = 柯桥良（技术架构总监）；美术规格章 = 甄塑之（美术总监）；汇编与待裁对账 = 主理人
> 范围：**世界地图整体渲染 + 地形地貌渲染**。疆界 / 影响力 / 道路 / 事件点图层只留接口位（§3.1.3），不做细案。
> 状态：**方案已获用户「按建议」基线裁定（2026-10-22）**；各待裁项的 ADR 化列入 M0。

---

## 〇、结论先行

1. **现行渲染栈已走"程序化水彩"路线，与 SPEC §3.6（零外部资源模式）高度吻合，而非 SPEC §3.2 的位图图集路线**——这不是差距，是 2026-11 用户裁定"JRPG 图集弃用、全面改纯色平涂 + Theme Pack 色板"（`tile_atlas.rs` 模块头注释）后的既定方向。位图 pack（terrain_atlas 40 块 + transition_mask）是 SPEC 要求的第二通道，机制骨架（`TileAtlas` / `load_tile_atlas`）仍在但需扩建。**双通道并存是本方案的核心架构决策**，位图通道是否本期投资由 T1 基线（双轨保留）执行。
2. **斜投影俯瞰（SPEC §1.1）已有真实先例**（ADR-0041 `oblique_skew_pixels`，Topographic 专用 compression 0.3；`buildings.rs` 右下挤出方向常量），扩到 JRPG 主题是工程量问题不是可行性问题。难点在矢量层与 shear 变换的坐标联动。
3. **SPEC 与 ADR-0035/0042/0048 无色值冲突**；冲突全部是"SPEC 未追记代码已生效修订"的文档债，共四处（28→36 槽、soften 0.85→0.92、缩放白名单、place_kind 词表），M-A 追记清偿。
4. **底图管线存在路线之争（T0）**：同日早间的 `docs/TERRAIN_VISUAL_OVERHAUL_PLAN_v1.0.md`（GPU 逐像素求值，D1–D9 待裁）与本方案柯章"维持 CPU 烘焙"互斥。**用户已裁定按建议基线**：T0 先裁 overhaul D1–D9；M2/M-A（色板 + 文档追记）与底图路线无关，先行不阻塞。

---

## 第一部分 技术架构章（柯桥良）

### 一、现状盘点表（逐模块，客观）

#### 1.1 渲染主干（地形底图管线）

| 模块 | 已有能力 | 与 SPEC 的对应 |
|---|---|---|
| `terrain_view/cells.rs`（~1650 行） | `CellRenderer`：全图合成与增量 patch 共用；7 个着色 shader（flat/google/real/faded/event/coast/topographic）；边界扰动 `wobble_px`（±0.45 格确定性噪声）+ 2×2 旋转网格 4 样本超采样；水彩晕开 `blended_base`（5×5 盒式模糊，ADR-0044）；陆格颗粒 `land_grain`（±4.5%）；水深渐变 `water_shade_at`；雪双调 `snow_shade_at`；坡度浮雕 `relief_mul`（光源固定左上 45°，系数 0.0012/m，钳 0.84–1.16）；水面纸纹 + 离岸墨线 + 泡沫 + 排线 + 积墨环（ADR-0048 五件套）；行带并行（任意带数逐字节一致，1024² ≈ 317ms）；`patch_terrain_image` 路径依赖型脏矩形增量 | §3.3 地貌笔触的程序化等价物已大量落地；§1.5 光源方向已对齐左上 45°；§3.6 程序化承诺基本满足 |
| `terrain_view/terrain_texture.rs` | `TerrainTexture`：句柄 + 逻辑格尺寸 + `res`（每格像素）+ 双脏区 + 四指纹（mode/soften/theme_id/map）；`terrain_res_for` 分辨率唯一口径 | §3.1 尺寸档位的分辨率侧实现（无 tile_size 概念） |
| `terrain_view/rebuild.rs` | 换条件重建（模式/淡化/主题/来源图任一变才触发）；异步合成（`AsyncComputeTaskPool`，主线程零阻塞）+ `SynthCtx` 指纹校验；换级淡出 `BasemapFadeOut` 0.4s | §3.5 图层化的"底图属 pack"侧运行时；切换原子性（§六 B13）机制基础 |
| `terrain_view/terrain_resolve.rs` | `active_terrain` 沿 parent 向上继承底图；`texture_terrain` 增量 patch 跟纹理实际持有的图走 | P4 多地图层级的渲染侧投影 |
| `terrain_view/tile_atlas.rs` | `TileAtlas`（RGBA + cols/rows/inset + `tiles[Option<[col,row]>;10]` 显式映射）；`load_tile_atlas`（YAML+PNG，std::fs，当前恒不被调用——JRPG 图集 2026-11 弃用）；`tile_res_for` 纹理预算钳制；`TileTint` Q8 定点着色 | §3.2 terrain_atlas 的机制骨架（显式索引 ✅ 对应 C4；仅 10 槽单变体，无 40 块/4 变体/mask） |
| `terrain_chunks.rs` | 分块 256 逻辑格 + 3 级 CPU mip 链 + 视口拣选（despawn 视口外 ±1 块）+ 脏矩形驱动（稳态零像素拷贝）；`select_mip` 按 texel 屏幕尺寸选级 | 性能预算主要执行者 |
| `terrain_view/isoline_overlay.rs` | marching squares 等值线 + Chaikin 平滑 + DP 抽稀分档；批量单 mesh 描边；屏幕像素线宽 + 亚线性缩放（1.25px × scale^0.78）；海岸水彩晕双层；河中线；`IsolineKey` 结构缓存 | §1.1 描线维度的矢量等价物（口径是屏幕像素非"边长 1/16"）；§3.3 海岸线笔触几何路线 |
| `terrain_view/atmosphere.rs` + `paper_overlay.rs` | 雾霭（α≤0.12，编辑态免雾）+ 纸张噪点（256² 可平铺三阶噪声）+ 纸纹斑驳 + 四角晕影 | §1.6 材质语言边界之外的纸面氛围层（SPEC 未定义，ADR-0016/0042 演进产物） |
| `terrain_view/grid_lines.rs` / `grid_sync.rs` | 编辑网格（1px，4px→2px 屏幕淡出）；栅格原点/图帧原点双轨对齐 | §3.3"格子是工具提示不是画面主体"的落实 |

#### 1.2 装饰与地物符号

| 模块 | 已有能力 | 对应 |
|---|---|---|
| `terrain_decor.rs` | 八类程序化符号（森林团簇/草点/丘/三角山+背光面/雪/沼/荒漠/浅海波）；三档 LOD 步距（Nation/Region/Edit）；splitmix64 确定性落点 + 站点抖动 + 簇生 + 尺寸起伏（ADR-0042 D8–D10/追记二 D13）；八桶合批（每类一 mesh 一实体，顶点原地改写）；`DecorVariant`（conifer/broadleaf/sparse/boulder）；`MAX_SITES_PER_KIND=2200` 兜底 | §3.3 山体走向/森林团簇的程序化演绎；§3.4 地物符号的非位图路线；§3.6 合批承诺执行者 |
| `landmark_symbols.rs` | PNG 地标符号（Z=0.24） | §3.4 props_atlas 雏形（固定资产，无 pack 契约/anchor/place_kind 映射） |

#### 1.3 主题层（wv-theme，ADR-0043 独立 crate）

| 模块 | 已有能力 | 对应 |
|---|---|---|
| `theme_pack.rs` | `ThemePack`（id/display_name/palette/treatment/atlas/decor/sea/overlays/region_overlays，全字段 Copy，`&'static` 单态化）；`TreatmentFlags` 12 项（HAND_DRAWN/EMPTY 两极，缺省=手绘全开）；`TileAtlasSpec` 占位；`builtin::DEFAULT` | §六 A1 ✅；A2 ❌ 无字段；A3 🟡 结构在 |
| `palette.rs` | `MapPalette{Daylight,Dusk}` + `PaletteColors` 36 槽（core 10/soften×2/poi/road/street/label×4/dot×4/text×4/snow×2/ice×3/halo/isoline/grid/building×2/water_shore/deep_ocean/event_backdrop） | §1.4.1 的 28 令牌结构子集：core 10 ✅、line 3 ✅、accent 4 ✅；faction 8 与 ui_link 3 未落 |
| `colors.rs` 系列 | 日间 JRPG 中高饱和（ADR-0035 定稿 + ADR-0042 D7 修正 mountain #9E9070）+ 暮色古舆图 + Google 三套；`SOFTEN_KEEP` 日 0.92（ADR-0048 S0 修订）/暮 0.35；锚点测试锁值 | §1.4.5 现行值已落地 ✅ |
| `sea.rs` / `decor.rs` | `SeaRecipe`（深度量程/曲线指数/浅滩带宽/纸纹振幅/波纹墨线/泡沫/排线/墨色）、`DecorRecipe`（阈值/气候带/墨色/LOD 步距/尺寸）——数值配方住主题层，执行器住渲染层 | §六"同一接口消费两种 pack"的配方化基础 |
| `registry.rs` + `theme/switch.rs` | `ThemePackRegistry`/`MergedThemeRegistry`；`ViewState.theme_pack_id` 权威 → `sync_active_theme_pack`（PreUpdate 最前，id 变才写）→ 13 个渲染文件读 `view.active_theme`（ADR-0049 端到端） | §六 B13 切换原子性 ✅ |

#### 1.4 觇图与缩放体系

| 模块 | 已有能力 | 对应 |
|---|---指|---|
| `state.rs` | `ViewMode` 8 态（默认 VectorFlat，ADR-0012；BROWSING 三态 = VectorFlat/Schematic/Faction）；`ViewState`；`LodTier` 三档 + `lod_fade` smoothstep | §3.7 缩放档的 LOD 侧（连续缩放 + 三档语义分级） |
| `zoom_level.rs` | `LevelZoom`（fit 锚定/内界 fit÷32/外界 fit×1.5625）；`fly_in_scale` 双击钻入；`ZoomCross::Out` 撞外界回父级 | 分级缩放（ADR-0009/0038），与 SPEC §3.1 白名单是两套模型（G9） |
| `camera.rs` | Camera2d 正交 + 滚轮锚指针缩放（零惯性）+ 旋转 + `screen_to_world` 唯一变换链；线宽 = 屏幕像素 × scale（ADR-0008 §8） | §1.1 视角载体（当前纯正俯瞰 2D，斜投影未接） |
| `cells.rs::oblique_skew_pixels` | 按行 x 向 shear（`(y−h/2)×compression`，边缘复制填充），仅 Topographic 接线；`buildings.rs` `EXTRUDE_DIR`（右下 45°）/`SHADOW_LENGTH_FACTOR 0.5`/`SHADOW_ALPHA 0.35` | §1.1 斜投影的已有先例：机制在、默认视图未启用、compression 0.3 非 0.5 |

#### 1.5 装配与治理

| 模块 | 已有能力 | 对应 |
|---|---|---|
| `plugins.rs` | 三 Plugin 拆分：`MapRenderPlugin`/`EditorOverlayPlugin`/`MapShellPlugin`（ADR-0023 D5） | §3.5 图层化 + 复用边界 |
| `recipe.rs` | `RenderRecipe` + `PassId` 24 项模式×缩放真值表 | 图层开关治理面 |
| `layer_frame.rs` | `LayerFrame`：作者 YAML `layers[].z/visible/opacity` 唯一求值点（ADR-0013） | P5 图层化编辑的渲染绑定 |
| `world_projection.rs` | `TimelineQuery::at` 时间投影 + 轨迹生长 + 道路分级前缀切片（ADR-0047） | P7 纪律执行样本 |

#### 1.6 Z 序全景（代码实取，图层合成基线）

```
0.000 地形 chunk │ 0.030 编辑网格 │ 0.040 等值线 │ 0.050 换级淡出/地图外框
0.060 雾霭/参考网格 │ 0.064 区域填充 │ 0.065 特征晕 │ 0.070 特征符号
0.075 地形装饰(桶间步 0.0004) │ 0.076 区域轮廓 │ 0.078 缺失标记/连线预览
0.080 纸纹斑驳 │ 0.0805 晕影 │ 0.082 河流 │ 0.084 作者河
0.090 道路casing │ 0.100 道路fill │ 0.108/0.110 主干casing/fill
0.113/0.115 支路casing/fill │ 0.120 街道预览 │ 0.125/0.135/0.145 建筑影/体/顶
0.210/0.220 POI外环/内点 │ 0.240 地标符号 │ 0.300 势力叠加
0.350 街道名 │ 0.400 地名 │ 0.450 测量 │ 0.500 纸张噪点 │ 0.900 选中/笔刷光标
```

（作者 `z` 声明叠加 `z_offset`，ADR-0013；`vector_layers.rs` 有编译期 Z 序断言。）

### 二、差距分析表（SPEC vs 现状，G1–G22）

> 判定口径：✅ 已满足 / 🟡 部分满足（括注缺什么）/ ❌ 缺失。SPEC v0.1 的"JRPG 默认地图"结论已被 ADR-0008→0012→0016→0035→0042→0044 演进链多次修正，现行默认 = VectorFlat 技法 × JRPG 中高饱和色板 × 手绘 Nation 装饰。差距表按 SPEC 原文逐条对标，不因 ADR 演进减免标注，但标注冲突来源。

| # | SPEC 条目（节号） | 现状 | 判定 |
|---|---|---|---|
| G1 | 斜投影俯瞰 2.5D，垂直 0.5× 压缩（§1.1） | `oblique_skew_pixels` 仅 Topographic（0.3 ≠ 0.5）；主浏览模式纯正俯瞰 2D；矢量层无 shear 联动 | 🟡 机制有先例，默认视图未接 |
| G2 | 外轮廓硬边描线，线宽=边长 1/16，深色不纯黑（§1.1） | 等值线 = 屏幕像素 1.25px × 亚线性（非比例口径）；色 `#6B5B49` 非纯黑 ✅；装饰符号自带描线语义 | 🟡 描线存在但口径不同（比例口径仅在位图 pack 下有意义） |
| G3 | 平涂 + 单层软阴影，光源左上 45°，阴影同色相压暗（§1.1/§1.5） | 平涂 ✅；`relief_mul` 左上 45° ✅；建筑右下投影 ✅；**山/树装饰的地面投影未显式落** | 🟡 地物级投影缺装饰类 |
| G4 | 饱和度区间与 28 令牌色板（§1.4） | 36 槽 PaletteColors；**faction a–f 六档 ❌、ui_link 三色 ❌** 未落 wv-theme | 🟡 结构缺 9 令牌 |
| G5 | 色板硬约束可自动校验（§1.4.3：ui_link 锁相 ΔH≤8°、派系 ΔE≥25/30） | 无任何色度校验代码 | ❌ |
| G6 | Theme Pack = 目录 + pack.yaml + palette.yaml + 5 类资产（§2.B.1） | 现行 = Rust const crate 注入（ADR-0023/0043/0049）；无目录/manifest | ❌ 形态不同（机制等价物已在，T2 裁分层共存） |
| G7 | terrain_atlas：10 地形 × 4 变体 = 40 块（§3.2.1） | `TileAtlas` 仅 10 槽单变体；JRPG 图集 2026-11 用户裁定弃用；变体哈希选择 ❌ | 🟡 骨架在，变体 ❌（T1 双轨基线） |
| G8 | transition_mask 8 向灰度遮罩（§3.2.3） | 无位图遮罩；程序化等价物已超配（wobble + 超采样 + 5×5 晕开） | ❌ 位图通道 / ✅ 程序化通道（按通道分别判定） |
| G9 | 缩放白名单 {0.5,1,1.5,2,3}（§3.1/§六 B9） | 连续缩放 + LevelZoom 区间 + LodTier 三档 | ❌（动机"位图描线断裂"在矢量路线不成立，T4 条件化） |
| G10 | @2x 倍率资产 + 缺省整数倍降级（§3.1/B10） | 无倍率档机制 | ❌（依附位图通道） |
| G11 | 地貌笔触：海岸±2px/山脊线/森林团簇/河宽 tile÷4/路宽 tile÷8（§3.3） | 海岸 wobble ±0.45 格；山脊线 ✅；森林团簇 ✅；河流折线连续 ✅（河宽口径未核实，M1 补）；道路宽 = 屏幕像素口径 | 🟡 语义全覆盖，数值口径为屏幕像素系 |
| G12 | props_atlas：11 类符号、1×1/2×2/3×3 档、anchor、place_kind 映射（§3.4） | `landmark_symbols` 固定 PNG（citadel/town 两张）；无尺寸档/anchor/映射表 | 🟡 有雏形无契约 |
| G13 | 图层归属：地形/疆界/影响力/道路属 pack，事件点/便签属壳（§3.5） | `LayerFrame` + `LayerKind` ✅；但势力色未入 pack、事件点色在壳常量 | 🟡 框架在，归属切分未对表 |
| G14 | 程序化绘制承诺（§3.6）+ 同色同尺寸合批、禁逐块材质 | 程序化路线为现行默认；八桶合批 ✅；地形整图烘焙（0 逐块实体） | ✅（超额） |
| G15 | 命名规范 TILE_/ATLAS_/PROP_ 与图集零文字（§5.1） | 无位图资产可命名 | ➖ 随位图通道落地 |
| G16 | assets/shell 与 assets/theme_packs 物理隔离（§5.2/B7） | 壳资产在 `wv-ui/assets/ui/` ✅；theme_packs 根 ❌ | 🟡 半边成立 |
| G17 | ui_link 三色单向穿透到壳（§六 B8） | wv-ui 零兄弟依赖（ADR-0026 红线），无穿透通道 | ❌ T5 装配层注入方案已裁基线 |
| G18 | pack 切换原子性、不动 YAML、不进 Undo（§六 B13） | `sync_active_theme_pack` 纯渲染层 ✅ | ✅ |
| G19 | 校验器 C1–C10 逐条 pass/fail（§六 B14） | 无 CLI 校验器 | ❌ M3 |
| G20 | 单屏 ≤48 色 / 调色板 ≤64 色（§1.3） | 无计数校验（36 槽 < 64 ✅ 结构满足，未验证单屏） | 🟡 T6 只做诊断 |
| G21 | 最小可见格数 ≥36×20（§1.3/§3.1） | 相机无格数下限钳制 | 🟡 只防误缩，不做硬闸 |
| G22 | schema_version 解耦、读旧 pack 警告（§六 B12） | `TileAtlasSpec.format_version` 占位 | ❌ 随 T2 |

### 三、技术实施方案

#### 3.1 世界地图渲染管线

**结论**：维持"CPU 整图烘焙 → 分块上传 → 视口拣选"主管线（**T0 基线：先裁 overhaul D1–D9，若 GPU 路线胜出则本节主管线让位**）；斜投影作为主题属性（TreatmentFlags 新位）接入；缩放白名单条件化执行（仅位图 pack 吸附档位）。

##### 3.1.1 斜投影俯瞰的落地方式

- **路线**：复用 ADR-0041 已验证的 texture-bake 方案（按行 shear，O(w×h) 一次性烘焙，1024² ≈ 10ms），从 Topographic 推广为 `TreatmentFlags` 级开关：
  - 新增 `TreatmentFlags::oblique_projection` + `ThemePack.oblique_compression: f32`（JRPG pack 按 §1.1 取 0.5；Topographic 沿用 0.3）；
  - `image_from_terrain` 的 shear 触发条件从 `mode == Topographic` 扩为 `pack.treatment.oblique_projection || (mode == Topographic && compression > 0)`；
  - **矢量层联动（主要工程量，约 6 处）**：等值线、河流/作者河、道路 casing/fill、街道、势力多边形、POI/地标的世界→屏幕坐标变换必须过同一条 shear：`GridConfig` 增加只读字段 `oblique: Mat2`，所有 `map_to_view` 类换算单点接入；`camera.rs::screen_to_world` 反向乘逆矩阵。禁止逐层各自实现（加编译期 `OBlique` 断言，与 Z 序断言同款纪律）；
  - `buildings.rs` 右下挤出方向与斜投影天然同构，不动。
- **明确不做**：Camera3d/透视（ADR-0041 §4.3 已否决）；逐格顶点 z 挤出推广到浏览模式。
- **风险内置**：shear 后视口拣选 AABB 需按 shear 量扩轴（R1）；`BasemapFadeOut`/`map_frame` 幅面按平行四边形扩（R2）。

##### 3.1.2 缩放档与 zoom_level / camera 的关系

| 体系 | 方案 |
|---|---|
| 连续缩放 + LevelZoom 区间 | **保留为唯一相机模型**（编辑器"指哪打哪"零惯性已裁定） |
| LodTier 三档 + lod_fade | 保留（SPEC §3.7 无 LOD 条目，此为演进产物） |
| SPEC 白名单 {0.5,1,1.5,2,3} | **条件化**：仅 `pack.atlas.is_some()`（位图 pack）时吸附档位（`snap_to_zoom_steps` 纯函数，档位常量入 wv-theme 供校验器复用）；程序化/矢量 pack 保持连续 |
| 最小可见格数 ≥36×20 | `LevelZoom` 增内界修正（纯函数 + 单测）；只防误缩不硬闸 |

##### 3.1.3 图层合成顺序（SPEC §3.5 归属 × 现状 Z 表）

| SPEC 图层 | 归属 | 现状落点（Z） | pack 化程度 |
|---|---|---|---|
| 地形层 | pack | chunks 0.0 / isoline 0.04 / decor 0.075 | ✅ 色板+treatment+sea+decor 配方已随 pack 走 |
| 疆界/影响力层 | pack | faction_overlays 0.3（**接口位**） | 🟡 faction 色未入 pack（G4） |
| 道路层 | pack | vector_layers 0.09–0.12（**接口位**） | 🟡 street 色已入 PaletteColors ✅ |
| 事件点/便签 | 壳 | POI dots 0.21（ProjectionFrame） | 现状即壳常量 ✅ |
| 纸面氛围 | 演进产物 | veil 0.06 / paper 0.080/0.0805 / grain 0.5 | 已随 treatment 走 ✅ |

**规则钉死（图层准入三件套）**：新增任何图层必须同时声明（a）Z 槽位常量 + 编译期序断言；（b）`LayerKind` 绑定或会话轴声明（recipe.rs PassId）；（c）pack 归属（视觉参数从 `view.active_theme` 取，禁止直引常量——ADR-0049 D5 口径）。

#### 3.2 地形地貌渲染

##### 3.2.1 terrain_atlas 图集与过渡遮罩的技术路径（位图通道，条件 T1）

- **数据结构扩展**（`tile_atlas.rs`）：`tiles: [Option<[u32;2]>; 10]` → `[[Option<[u32;2]>; 4]; 10]`（图集变体表，40 块）；变体选择 = 坐标哈希（复用 `hash01(cx, cy, seed)`，`v = hash01 % 4`）——满足"引擎按坐标哈希选变体，pack 不控制概率"与既有确定性契约；图集尺寸校验 = 2 的幂 + `tile_size × N` 整除（加载期断言，进校验器 C3）。
- **transition_mask**（§3.2.3）：只用于位图通道的边界格混合——`write_boundary_cell` 现行"4 样本超采样混色"替换为"8 向 mask 采样"；带宽 `tile/4` 由 pack 声明。**不画 47 块 autotile**（SPEC 明示不要求）。
- **加载路径改造**：废除 `atlas_base_dir()` 硬编码 `CARGO_MANIFEST_DIR/assets`（ADR-0023 §1.2 已点名）——图集路径改由 pack 声明（相对 pack 根），文件 IO 上移至 **wv-app 装配层**（AGENTS §2.1 红线修复），wv-render 只收已解码像素。
- **@2x**：`TileAtlas` 增 `mip_of(2x)` 可选句柄；缺省时按整数倍降级，警告进状态栏。

##### 3.2.2 硬边描线 + 软阴影的着色路径

- **双口径并存**：
  - **程序化 pack（现行默认）**：描线 = `isoline_overlay` 几何描边（1.25px × scale^0.78 + 水彩晕双层）。保留现口径，理由：矢量描边任意缩放连续无断裂；SPEC"1/16 边长"比例口径在程序化路线无锚点（格的世界尺寸由作者 extent 定）；
  - **位图 pack**：描线**烘焙在图块内**（SPEC §2.B.2）——渲染层零额外描线 pass，线宽 = tile/16 由 pack 资产自保证，渲染侧只做 C 级抽检。
- **软阴影/明暗 4 阶（§1.5）补齐**：`relief_mul` 已覆盖地形面；补**装饰地物投影**（G3）：`terrain_decor` 八桶各增投影子 mesh——山/树团按 `EXTRUDE_DIR` 右下偏移 `0.5×符号高`，色 = 符号墨色 α0.35 暗化版（禁纯黑，明度 −20%），与 `buildings.rs` 同公式同常量；桶内合批不变（每类 2 mesh：影+符号）；阴影偏冷 +8°~12° 进 `DecorRecipe`（主题配方非渲染常量）。
- **明确不做**：法线贴图/PBR/噪声纹理（§1.6 禁令）；多层高光；AO 烘焙。

##### 3.2.3 程序化绘制（§3.6）的取舍

**判定：程序化路线是资产，不是欠账。** 取舍三条：
1. **保留**：程序化 pack 为 `_default` 基准（SPEC §六 16 → `builtin::DEFAULT` 即其 Rust 形态）；
2. **补齐**：水面"2 条横向高光条"未实现，作为 `SeaRecipe.highlight_bars` 可选项补（M2 小项）；
3. **不做**：程序化"城市符号 = 方块+三角"——地物符号走 `landmark_symbols` PNG 通道，程序化版仅在零资源 pack 作降级绘制（M3 尾部，可裁）。

#### 3.3 Theme Pack 契约落地（SPEC §六 14 条逐条）

| # | SPEC 要求 | 现状 | 落位方案 |
|---|---|---|---|
| 1 | pack_id 正则 + == 目录名 | `ThemePack.id` ✅ | 校验器 C1；目录名约束随 T2 |
| 2 | tile_size ∈ {16,24,32,48,64} | ❌ 无字段 | `ThemePack.tile_size: Option<u8>`（None = 程序化）；随位图通道（M3） |
| 3 | atlas_layout 显式声明 | `TileAtlas.tiles` ✅ | 扩 4 变体 + schema 进 pack 声明 |
| 4 | palette 28 令牌全集 | 36 槽（编译期，缺键=编译错，更严） | 补 faction 8 + ui_link 3（M2）；"fallback+警告"仅 YAML 形态需要 |
| 5 | asset_paths 相对 pack 根 | ❌ 硬编码 | M3 路径声明 + IO 上移 wv-app |
| 6 | props[].place_kind 映射、缺符号降级圆点 | ❌ | M3：`ThemePack.landmarks`，未命中回退现有 POI 点标（`PlaceDot` 圆点系，降级天然成立） |
| 7 | shell/pack 物理隔离 | 壳在 wv-ui/assets ✅；pack 无目录 | T2 落地时建 `assets/theme_packs/`；**Rust crate 形态下隔离由 cargo 依赖图天然保证**（优于 SPEC） |
| 8 | ui_link 三色单向穿透 | ❌（零依赖更严） | **T5 已裁基线：装配层注入**——三常量落 `wv-theme::colors::UI_LINK_*`，wv-app 启动时写进 wv-ui 的 UiTokens（setter API，不加依赖），方向单向 pack→app→ui |
| 9 | 缩放白名单 | ❌ 连续 | 条件化（§3.1.2）；白名单常量落 wv-theme |
| 10 | @2x 降级+警告 | ❌ | M3（§3.2.1） |
| 11 | 零资源模式，同一接口 | ✅ `atlas = None` 即程序化 | 现状即 SPEC 愿景的 Rust 形态 |
| 12 | schema_version 解耦 | 🟡 占位 | YAML 形态落地时进 pack 头（T2） |
| 13 | 切换原子性 | ✅ | 已达 |
| 14 | C1–C10 校验器 | ❌ | M3：`wv validate-pack` CLI（wv-app 子命令）；C7/C8 色度计算（CIEDE2000）纯函数进 wv-theme（手写 ~80 行，可测，无第三方依赖） |

**美术侧反向承诺（§六 15/16）**：命名/目录/档位变更须先改 SPEC 再进契约——对应 Rust 形态 = 先改 `wv-theme` 契约再改渲染消费点；`wv-theme-chinese-ink` 示范 crate 是符合性样本。

#### 3.4 性能预算与合批策略

| 指标 | 数值目标 | 测量方式 | 超阈处置 |
|---|---|---|---|
| 整图纹理预算 | ≤ 4096²（RGBA8 ≈ 67MB） | `tile_res_for` 单测已锁（512→8/2048→2/8192→1） | 自动降级：`(4096/cells).clamp(1,64)` 逐格分辨率下调 |
| 全图合成（1024²） | ≤ 350ms 且主线程零阻塞 | tracing span + `PendingTerrainSynth` 单测 | 自动降级：异步线程池 + `SynthCtx` 指纹丢弃重排 |
| 笔刷增量 patch | ≤ 16ms/笔触 | `patch_terrain_image` 计时单测 | 门禁卡点：`cargo app-test` 回归即拒收 |
| 稳态每帧 CPU | chunk 层零像素拷贝零 mesh 重建 | `chunks_dirty=None` 帧早退单测 + smoke-gui | 自动降级：脏矩形 + 视口拣选 despawn |
| 缩小摩尔纹 | texel <1px 必降 mip | `select_mip` 纯函数单测 | 自动降级：CPU 盒式 mip |
| 装饰总量 | 每类 ≤2200 组 | 常量 + 计数日志 | 自动降级：LOD 步距稀疏化 + 截断兜底 |
| Draw Call | 视口内 chunk ≤~12 + 装饰 8×2 + 矢量层每层 1 批量 | Bevy 诊断 + smoke-gui | 门禁卡点：合批纪律 = PR review 卡点（架构师判定） |
| 换级淡出 | 0.4s 单调递减 | `advance_fade` 单测 | 常量已锁 |
| 编辑网格退场 | ≤2px 整层消失 | `grid_line_fade` 单测 | 自动降级：线性衰减 |
| 渲染烟测 | GUI 5s 存活无 panic | `scripts/smoke-gui.sh` | 门禁卡点：渲染改动提交前必跑 |
| 依赖红线 | wv-render 不依赖 wv-edit/wv-app | `cargo tree` 断言 ×2 | 门禁卡点：输出非空即拒收 |

**合批总则**（SPEC §3.6"禁逐块独立材质"）：地形 = 单张（≤16 chunk）纹理 quad，0 逐格实体；装饰 = 类级单 mesh 桶；矢量层 = 层级批量 mesh；任何"逐元素 spawn Sprite"的新增代码 review 一票否决。

#### 3.5 与分层红线（AGENTS.md）的对齐声明

| 约束 | 触及情况 |
|---|---|
| wv-render 禁直接读写文件 | **触及（正向修复）**：位图通道图集 IO 上移 wv-app，偿还 `load_tile_atlas` std::fs 历史豁免 |
| wv-theme 禁文件/网络/兄弟依赖 | 不触：CIEDE2000 纯函数；UI_LINK 常量纯数据 |
| wv-ui 零依赖 | **潜在触及**：T5 装配层注入方案下 wv-ui 只加 setter API 不加依赖——红线保持 |
| 数据渲染分离 / System 不改世界 | 不触：shear 矩阵是 `GridConfig` 派生缓存（拉动式） |
| §6.4 重大决策开 ADR | **触及**：斜投影推广（修正 ADR-0041 范围）、位图 pack 复活（与 2026-11 弃用裁定对话）、ui_link 穿透（修正 ADR-0026 边界）均需新 ADR |
| P1–P7 | 只动渲染层，零数据 schema 变更；P5 由 LayerFrame/PassId 承接 |
| §7.1 测试闸门 | 全部里程碑出口含 `cargo test --workspace` + `app-test`/`app-clippy` + `smoke-gui.sh` |

### 四、分阶段里程碑（技术线）

| 期 | 内容 | 出口判据 | 依赖 |
|---|---|---|---|
| **M0 契约收敛** | T0–T5 ADR 化；SPEC 追记条目定稿 | 4–5 份 ADR「已接受」；SPEC 按 §6.3 追写 | 无 |
| **M1 斜投影+光影补全**（纯程序化通道） | oblique treatment 位 + compression 进 ThemePack；shear 矩阵进 GridConfig + 6 处矢量层联动 + 编译期断言；装饰八桶投影子 mesh；`SeaRecipe.highlight_bars`；河宽口径核实 | ① JRPG 主题实机截图：斜投影、投影、矢量层零错位；② `screen_to_world` round-trip 单测；③ 全闸门绿 + smoke-gui | M0 T3 |
| **M2 色板契约补全**（与 M1 并行） | faction 8 色 + ui_link 3 色落 wv-theme（数值甄塑之给）；Dusk 镜像完整性测试扩槽；装配层注入；CIEDE2000 纯函数 + 校验单测 | ① Dusk 镜像测试绿；② ΔE 校验单测（SPEC §1.4.3 手算样例断言）；③ 换主题除画布外像素不变 | 甄塑之出 faction 六色数值 |
| **M3 位图 pack 通道**（条件 T1） | 图集变体表 + 坐标哈希；transition_mask 8 向混合；图集 IO 上移 + pack 相对路径；tile_size 档位；@2x；landmarks.place_kind 映射；`wv validate-pack` CLI | ① 40 块测试图集加载 + 变体确定性单测；② mask 边界混合单测；③ validate-pack 对坏 pack 逐条 fail；④ 缩放吸附仅位图模式生效单测 | M0 T1/T2；美术侧测试图集 |
| **M4 性能与验收收口** | 预算表全行实测归档；draw call 基线；单屏 ≤48 色诊断；最小格数钳制 | ① 实测值 ≤ 目标入档；② 换 pack §2.B.2 验收；③ 全闸门 + smoke | M1–M3 |

**并行性**：M1 与 M2 无依赖可并行；M3 独立分支（feature flag 保护，未裁定不合并）；M4 收尾。总口径：**先程序化通道完善（M1/M2），位图通道按需投资（M3）**。

---

## 第二部分 美术规格落地章（甄塑之）

### 一、规格 → 渲染特性映射总表

#### A. 风格锚点（SPEC §一）

| 规格条目 | 现状（实读） | 差距 | 验收口径 |
|---|---|---|---|
| §1.1 斜投影俯瞰、方形格 | 地形是 2D 栅格俯视；立体感由山符号两面体+背光面与 RealTerrain 高度明度调制承担 | 「垂直 0.5× 压缩」未实现，视觉目标由符号达成（T3 建议实装 shear，美术联署） | 截图核对：山符号有受光/背光两面、无等距菱形；T3 实装后含斜投影 |
| §1.1 描线 1/16 格宽、深色不纯黑 | 等值线屏宽 1.25px × scale^0.78，色 #6B5B49 褐墨 ✓ | 口径从「格比例」变为「屏幕像素」（口径翻译，SPEC 追记） | 海岸线/类型边界 Nation 档可辨、Edit 档不糊线；墨色取 line.*/isoline_ink 族，无 #000 |
| §1.1 平涂+单层软阴影、左上光 | 平涂 ✓；山背光面在右侧（受光左上）✓、建筑 side α0.35 ✓ | 无全局光照系统（SPEC 本也不要求） | 山/树/建筑阴影方向全部右下或右侧；无 AO、无多点光 |
| §1.1 中高饱和 S45–75%、大地色呼吸位 | CORE_TERRAIN = ADR-0035 A 方案 ✓ 与 §1.2 呼吸位一致 | 无 | 色板锚点测试（已存在）+ 截图肉眼核对草/林 1 秒可辨 |
| §1.3 32px TILE_BASE 锚点、40×23 格最小画面 | TILE_BASE 程序化路径无消费；尺寸锚点由屏幕像素常量承担（符号步距 26/46/72px、线宽 1.25px、树冠 2.6px） | 口径翻译：格像素锚点 → 屏幕像素锚点（SPEC 追记） | 各主题配方的屏幕像素常量成套存在；换主题不写死渲染层 |
| §1.3 单屏 ≤48 色、无帧动画 | 程序化着色无调色板数量校验；无位图帧动画 ✓ | 色数校验缺失（弱项，T6 只做诊断） | 无帧动画 ✓ 保持；色数诊断列入 M4 |

#### B. 色彩 / 光影 / 材质（SPEC §1.4–§1.6）

| 规格条目 | 现状 | 差距 | 验收口径 |
|---|---|---|---|
| §1.4.1 28 令牌结构 | 代码为 36 槽 PaletteColors | SPEC 结构表落后于代码（文档债①） | 追记后：palette.rs 字段集 = SPEC 追记表；新增槽位先 SPEC 后代码 |
| §1.4.3① ui_link 三色锁相 | colors.rs 常量与 SPEC 逐位一致，无替换路径 ✓ | 无（穿透通道 T5 装配层注入） | 主题包不得覆盖此三色——评审项 |
| §1.4.5 日间十色现行值 | CORE_TERRAIN 与 SPEC §1.4.5 + ADR-0042 D7 逐位一致 ✓ | 无 | 锚点测试 + 换主题不污染日间值 |
| §1.4.5 soften.keep 0.85 | 代码 `JRPG_SOFTEN_KEEP = 0.92`（ADR-0048 S0 修订） | 文档偏差（待裁②） | 追记后三处一致（SPEC/ADR-0048/colors.rs） |
| §1.5 阴影同色相压暗偏冷、禁纯黑 | 山背光 #474033 墨褐 ✓；雪冷阴影冷蓝灰 ✓ | 无 | 截图抽查阴影色相 |
| §1.6 Water 横向高光条 2 条 | 现行海面 = 等深渐变+浅滩带+泡沫带+离岸墨线圈+白波弧（SeaRecipe），不是横向高光 | 实现语言不同、目标一致且更强；SPEC 表述过时（M2 补 highlight_bars 可选项） | 海面 Nation 档读得出「越远越深+海在动」；岸线→泡沫→墨线圈三层可辨（截图） |
| §1.6 禁噪声纹理/法线/PBR | 全部程序化平涂+值噪声颗粒（±4.5%）——颗粒是明度抖动非贴图纹理 | 无（精神一致） | 不引入任何外部贴图做地形材质；颗粒幅度超 ±5% 回美术审定 |

#### C. 地图绘制规格（SPEC §三）

| 规格条目 | 现状 | 差距 | 验收口径 |
|---|---|---|---|
| §3.1 缩放白名单 | 连续缩放+分级区间 clamp + LOD 三带 | 实质冲突、已被 ADR-0008/0009/0038 事实上取代（待裁③→T4 条件化） | LOD 切换带（scale 1.0/3.0 附近）无符号跳变（decor 步距/尺寸平滑、veil 连续） |
| §3.2 terrain_atlas 40 块 | tile_atlas.rs：JRPG 图集已弃用（2026-11 裁定），ensure_atlas 恒 None | 位图通路整体让位程序化（T1 双轨保留：M3 条件重启） | 现行验收：纯色平涂+watercolor_bleed 下 10 类地形 1 秒可辨（截图）；位图契约保留 M3 |
| §3.2.3 transition_mask 8 向 | 无资产无消费点；过渡由 watercolor_bleed 5×5 + 等值线承担 | 同上 | 群系边界 2 格宽柔和渐变、无硬切格边（截图放大）；位图遮罩契约保留 M3 |
| §3.3 海岸线 ±2px 格内抖动不跨格 | wobble_px 像素级确定性扰动 + Chaikin 平滑 ✓ | 口径翻译（格比例→纹理像素） | 海岸线有机不规则、无直线段感；同工程重开逐字节一致（已有测试） |
| §3.3 山沿隐式脊线 | 脊点吸附 + 生成端脊线噪声化 ✓ 超规格：±35% 尺寸变体+30% 簇生次峰 | 无 | 山链沿蜿蜒脊线成串、无栅栏直线排布、无同尺寸复印感 |
| §3.3 森林团簇、树冠直径 tile/4 | 针叶 3 棵细三角成排/阔叶 3 颗圆团/稀树 1 团，直径 ≈2.6–5.2px；密度由森林密度场×主题增益调制 | 数量 3 而非 3–5（视觉等价）；口径为屏幕像素 | 三气候变体形状可辨（北针叶/中阔叶/南稀树）；密度场让林区有疏密呼吸 |
| §3.3 河宽 tile/4、不显示格界 | 河流由水系槽位+river_centerline 河中线呈现 | 河宽口径未核实（M1 补核实） | 待核实后补：河流视觉连续、无格界断裂 |
| §3.3 道路 tile/8+路肩 | 矢量双线：fill 3.5px + casing 6px（屏幕像素口径） | 口径翻译（casing 语义替代路肩） | 路缘任意缩放下可读 |
| §3.4 props_atlas 11 类 | citadel.png/town.png 两种（include_bytes 嵌入）+ 程序化山/树符号；LandmarkKind 仅 Citadel/Town | **缺口最大项**：11 类 place_kind 映射表无代码载体 | 见资产缺口 + M-B |
| §3.5 图层归属 | LayerFrame ✓；decor/sea/overlays 配方已 ThemePack 化 ✓ | 换主题时地形族配方整套跟随 ✓ 已成立 | 切 chinese_ink 后海面/装饰全套变化（ADR-0049 已验证） |
| §3.6 程序化等价物 | 已实现且超集：平涂+晕开、森林程序化、两面体山、合批 ✓；纸纹/雾霭/晕影为增项 | 无 | 既有单测+截图；新增程序化效果须配确定性测试 |
| §3.7 C1–C10 校验 | C1 ✓ 约定存在；C5 以编译期静态构造形式强于 schema；C7/C8 色差计算、独立校验器 CLI 不存在 | 校验器缺口（M3） | M3 交付 `wv validate-pack`，C1–C10 逐条 pass/fail |

#### D. Theme Pack 契约（SPEC §六，仅列相关）

| 契约条目 | 现状 | 差距 |
|---|---|---|
| A1 pack_id | ThemePack.id + Registry 拒绝重复 ✓ | 无 |
| A2 tile_size | 无消费点 | 随 M3 |
| A3 atlas_layout | YAML 配置支持显式 (col,row)，但无图集资产 | 随 M3 |
| A4 palette 缺失报错 | 静态构造缺失=编译错误（更强）；YAML 包 fallback 不存在 | M3（README §12-30 挂账） |
| A5 asset_paths | include_bytes! 编译期嵌入 | 外部资产通路 = M3 技术决策 |
| A6 props place_kind 映射 | LandmarkKind 仅 2 种，无词表；降级未实现 | 待裁 D④：M-B 定词表 |
| B7 两层隔离 | crate 隔离 ✓ 优于目录隔离；仓库根 assets/ 非 pack 结构 | 目录式 pack = T2 |
| B8 ui_link 穿透 | 常量层锁相 ✓；穿透机制未建 | T5 装配层注入（M2） |
| B9 缩放白名单 | 被 ADR-0009 连续分级取代 | T4 条件化 + SPEC 追记 |
| B11 零资源模式 | **已成立**：atlas=None 即程序化 ✓ | 无 |
| B12 schema_version | 无 | 随 T2 |
| B13 切换原子性 | 纯渲染层替换 ✓ | 无 |
| B14 可校验性 | C1–C10 校验器不存在 | M3 |

### 二、资产缺口清单

> 前提：现行默认路径 = 程序化（零外部资产）。建议产出方式按此分档：程序化已覆盖的不补位图；位图仅用于「程序化表达不了的手绘细节」。

| 资产 | SPEC 契约 | 现状 | 缺口 | 产出方式 |
|---|---|---|---|---|
| terrain_atlas（40 块） | §3.2.1 | 无资产无消费 | 整套（条件 T1） | 暂不产出；M3 重启时：程序化导出占位图集，外包手绘逐块替换 |
| transition_mask（8 向） | §3.2.3 | 无 | 整套（条件 T1） | 程序化生成（距离场→渐变遮罩，一条脚本）；无手绘价值 |
| props 符号族（11 类） | §3.4 | 仅 citadel/town 两张手绘水彩 | **9 类**（镇/村/关口/港口/桥/渡口/矿/神社寺/战场遗迹/驿站） | 手绘为主（延续 citadel/town 风格），程序化圆点兜底。前置：place_kind 词表（D④） |
| mountain_a.png（日/暮） | 无 SPEC 契约 | 资产存在但**无代码消费点** | 接线决策 | M-B 裁：接入 DecorRecipe 作山的位图变体，或明确废弃——不建议悬置 |
| 森林图集（3 变体×日/暮） | 无（ThemeOverlay 稿） | tree_atlas_day/dusk + forest_fill 已内嵌消费 ✓ | 无缺口；@2x 无 | 保持程序化消费；@2x 待高清屏验收后决定 |
| faction_marks | §2.B.1/§3.5 | 无 | 整套 | **范围外**（疆界层细案），仅登记 |
| decor：frame/compass/legend | §2.B.1 | 无（地图壳走极简控件） | 整套，低优先 | 手绘；先出指北针+图例底板各 1 张试风格，边框待用户表态 |
| 纸张纹理 | 无 SPEC 契约 | 程序化已覆盖（256² 可平铺噪声） | 无位图缺口 | **维持程序化**。真实纸扫描质感会破坏确定性重开一致，慎重 |
| 字体 | §4.2 + ADR-0042 D4 | Inter/NotoSansSC（壳）+ LXGWWenKai（画布）已嵌入 ✓ | 无 | 保持 |

### 三、色板与现行 ADR 的关系

**结论：SPEC 与 ADR-0035/0042/0048 无色值冲突；冲突全部是文档债，共四处。** 管辖划分清晰无争议：

| 关系项 | 结论 |
|---|---|
| 日间十色现行值 | SPEC §1.4.5 + ADR-0042 D7 修正与 CORE_TERRAIN **逐位一致** ✓ |
| ADR-0035 管辖 | 日间地形十色 + 淡化机制 + 雪高光联动；暮色/Google/POI/标签明文不动 |
| ADR-0042 管辖 | 手绘表现层；仅修正 mountain 一项，已闭环 |
| ADR-0048 管辖 | SeaRecipe；修订 soften 锚值 0.85→0.92——代码与 ADR 一致，SPEC 未追记 |
| ui_link 锁相 | 三方（SPEC/ADR/代码）无冲突 ✓ |

**文档债清偿（M-A，全部走 SPEC「只增不改」追记）：**

| # | 待裁项 | 建议（已按用户「按建议」基线记录） |
|---|---|---|
| D① | SPEC §1.4.1 仍是 28 令牌，代码已是 36 槽 | SPEC 追记 §1.4.6「现行槽位全集表」，声明扩容原则：新增槽位先 SPEC 后代码 |
| D② | SPEC 表内 soften.keep=0.85，代码=0.92 | SPEC §1.4.5 追记修正条（同 mountain 追记格式） |
| D③ | SPEC §3.1 白名单 vs 连续分级 | SPEC 追记：白名单条款限定于位图图集通路；程序化通路按 ADR-0009 连续分级验收 |
| D④ | props 11 类词表无载体；mountain_a 悬置 | M-B 开项：定词表归属 + 裁 mountain_a |

另重申 ADR-0035 遗留开放待裁：**暮色是否跟进 JRPG 色系**——若跟进需暮色十色校准依据，列美术 M-B。

### 四、美术侧里程碑

| 期 | 美术交付物 | 验收 |
|---|---|---|
| **M-A 规格收口**（可立即启动，零代码依赖） | ① SPEC 四处追记；② 基准图清单（每主题×三档 LOD×日/暮标准机位与拍摄口径）；③ props 11 类词表草案 | 追记合入（只增不改）；清单入库；词表过创意+主程序联审 |
| **M-B 配方深化**（主题配方迭代窗口） | ① 暮色跟进 JRPG 校准提案（十色+soften+装饰墨色一套，含实机截图）或明确不跟进；② DecorRecipe/SeaRecipe 逐主题校准记录；③ props 扩充：先补 3 类高频（镇/村/关口）+ place_kind 映射接线方案 | 切主题后「除画布外像素不变」；每主题基准图 ≥2 项可见差异；新符号与 citadel/town 同描线同投影同底座 |
| **M-C 位图通路**（若 T1 重启） | ① 占位图集 40 块 + 外包替换计划；② mask 生成脚本；③ 命名规范落地；④ C1–C10 校验用例 | 位图 pack 与程序化 pack 同接口消费；风格等价（截图评审）；校验器逐条 pass/fail |
| **M-D 外包资产**（M-C 后） | props 剩余 8 类 + decor（指北针/图例/边框）手绘交付，含 LICENSE/署名 | C1–C10 全过；符号在 1×1 格档下剪影可辨 |

### 五、风格守护清单（每期验收必核对——防「跑偏丢味」）

> 精神承自 SPEC §5.5。每条都是「曾经真的跑偏过」或「最容易跑偏」的点。

| # | 守护项 | 跑偏形态（历史实据） | 验收动作 |
|---|---|---|---|
| 1 | 纸纹与颗粒的克制上限 | 斑驳 α>0.13 / 晕影 >0.16 / 颗粒 >±5% 会盖住笔迹（ADR-0042 D11 定上限的原因） | 改动任一纹理参数截图 200% 放大对比「墨线仍清晰」；上限不得上调除非实机评审 |
| 2 | veil 与 soften 的叠加洗白 | 双重洗色曾把海面抬亮 30%（atmosphere.rs 注释实据） | 动任一参数全局档截图核对海色/草色不退灰白；两参数不许同向调大 |
| 3 | 山是符号不是颜色 | 填充色带曾画出「白色巨墙缎带」（ADR-0042 处境①） | 山地/雪填充色保持水彩过渡角色；地貌身份只能由装饰符号承担；新增效果先问「这是符号还是色带」 |
| 4 | 符号布点的自然感 | 步距格心直铺曾排出「完美棋盘栅栏」（ADR-0042 追记二实据） | 新增装饰类别必须带确定性抖动+尺寸变体；zoom out 截图查「无行列感」 |
| 5 | 密度的屏幕量级控制 | 符号过密会淹没路线/地标（JRPG 图集弃用的直接原因） | 每类符号有 MAX_SITES 兜底；新增类别同步给定上限与三档步距 |
| 6 | Google 模式的干净例外 | 陆格颗粒/纸纹曾考虑全模式铺（ADR-0042 D6 刻意排除） | 任何新纹理效果接入核对：GoogleMap/Coastline/Event 模式不受影响 |
| 7 | 色值改动的锚点纪律 | 色板漂移无感知（ADR-0042 D7 修 mountain 时 SPEC 与 ADR 出现偏差） | 用户定稿色必须挂锚点测试；任何 hex 改动三问：SPEC 追记了吗 / ADR 修正关系标了吗 / 测试锚了吗 |
| 8 | 确定性重开一致 | 纸纹/装饰/波纹若引入时间或随机源，重开工程树会跳位 | 所有程序化视觉走纯函数哈希；新效果必须有「同输入两次运行逐字节一致」测试 |
| 9 | 主题配方的整包性 | 配方若散落渲染层，换主题就会「陆不动海动」（ADR-0048 追记二立论） | 新画法参数一律进 wv-theme 配方；渲染层只留执行器；评审 grep 渲染层不得新增美术常量 |
| 10 | 暮色/外部主题的成套交付 | 只换一两个值会得到「脏了的日间」而非新主题 | 每主题交付 = 色板+treatment+sea+decor+overlays 整套 + ≥2 项可见差异截图证据 |
| 11 | LOD 带边界的连续性 | 三档步距/尺寸切换处屏幕跳变 | scale 1.0 与 3.0 附近连续缩放录屏：符号密度/尺寸渐变、无突跳、veil 连续 |
| 12 | 基准图回归 | 无基准则「好不好看」无对错（ADR-0035 代价栏） | 每期收尾按 M-A 清单重拍全套入 `snapshots/theme-eval/<期号>/`，评审对比后再合入 |

### 附：本轮明确不做 / 未核实登记

- **不做**：疆界/影响力/道路/事件点图层细案（范围外，接口位见 §3.1.3）；渲染管线/合批/性能细案（柯章已含）；交互逻辑。
- **未核实**：① snapshots/theme-eval 截图内容（工具无法读图，仅核实文件存在与命名）；② 河宽 tile/4 实现现状（M1 核实）；③ mountain_a.png 产出意图（无代码引用无文档登记）；④ 外部主题 crate（wv-theme-chinese-ink）配方观感质量（契约已验证，美术校准自述「待校准」）。

---

## 第三部分 汇编：统一待裁清单与裁定记录（主理人）

> 2026-10-22 用户裁定「按建议」，以下基线已记录；**其中 T0 的具体裁定仍需对 overhaul D1–D9 完成后生效**。

| # | 议题 | 裁定基线 |
|---|---|---|
| **T0** | 底图管线路线：GPU 逐像素（`docs/TERRAIN_VISUAL_OVERHAUL_PLAN_v1.0.md` D1–D9）vs CPU 烘焙维持+补全 | **已裁定（2026-10-22 22:22）：甲 = GPU 逐像素路线胜出**。ADR-0051 已立（提议态，P0 规格锁后转已接受）；柯章 §3.1 主管线让位（其余部分存活）；D3 按 T1 同款双轨修正（程序化 `_default` 保留，图集为位图 pack 能力）；overhaul 头部「ADR-0050」编号过时，实落 ADR-0051。M1 中装饰投影/河宽核实与 GPU 路线兼容；斜投影（T3）在 GPU 路线下改 shader 内变换，实现路径待 P1 后重估 |
| T1 | 位图图集通道去留 | 双轨：程序化默认，位图为可选 pack 能力（M3 条件展开） |
| T2 | pack 形态收敛 | crate 管配方 + YAML 管数据资产（palette/图集路径/place_kind 映射），需新 ADR |
| T3 | 斜投影：真 shear vs 符号化立体 | TreatmentFlags 化（JRPG 系开、Google 系关），0.3/0.5 实机对比后拍终值，需美术联署 |
| T4 | 缩放白名单 | 条件化：位图 pack 吸附、程序化连续 + SPEC 追记 |
| T5 | ui_link 三色穿透 | 装配层注入（wv-ui 只加 setter，零依赖红线不破） |
| T6 | 单屏 ≤48 色校验 | 只做诊断计数不做闸门 |
| D①② | SPEC 28→36 槽追记；soften 0.85→0.92 追记 | M-A 文档追记（只增不改） |
| D④ | place_kind 词表归属 | M-B 定词表，映射住 ThemePack |

**小项登记**：mountain_a.png 悬置资产（M-B 裁接线或废弃）；河宽口径核实（M1）。

### 与既有方案的交接关系

- **T0 已裁定（2026-10-22）：甲 = GPU 逐像素路线胜出，ADR-0053 已立（提议态）**。柯章 §3.1 主管线描述让位给 `TerrainFieldMaterial`（3 张数据纹理 + WGSL 逐像素求值），但本方案以下部分**不受影响、继续有效**：§3.1.3 图层合成与图层准入三件套、§3.2.2 双口径描线/软阴影（装饰投影子 mesh 与 GPU 底图管线正交）、§3.3 契约 14 条、第二部分（美术章全套）、风格守护清单 12 条、M2/M-A/M-B/M-D。斜投影（T3）在 GPU 路线下改为 shader 内变换，texture-bake 先例降级为参考实现。
- 与 ADR-0052（CellMap 工作台，提议态）无冲突：工作台底图与本方案地形渲染共享数据层，D10 已声明「纯色不加 theme」；GPU 路线下其群系可见底图由 shader 着色分支实现（接口不变）。

### 里程碑总览（合轨）

| 期 | 技术线 | 美术线 | 出口门 |
|---|---|---|---|
| M0 / M-A | T0–T5 ADR 化 + SPEC 追记定稿 | SPEC 四处追记 + 基准图清单 + props 词表草案 | ADR 已接受；追记合入 |
| M1（视 T0/T3 范围） | oblique treatment + 矢量层联动 + 装饰投影 + highlight_bars + 河宽核实 | 投影色值/偏冷量/高光条参数 | 截图 diff + round-trip 单测 + 全闸门 |
| M2（与 M1 并行） | faction 8 色 + ui_link 3 色 + ΔE2000 + 装配层注入 | faction 六色数值 + 暮色跟进提案 | Dusk 镜像测试 + 换主题像素不变 |
| M3（条件 T1） | 图集变体表 + mask + IO 上移 + validate-pack | 占位图集 + mask 脚本 + 命名规范用例 | 变体确定性 + 坏 pack 逐条 fail |
| M4 / M-D | 性能实测归档 + draw call 基线 | 外包 props 9 类 + 指北针/图例 | 实测 ≤ 目标入档 + 基准图回归 |

---

## 附录：纪律与验证

- 本方案遵循 AGENTS.md §6.3「文档追写不改机制」：SPEC 修正一律走追记条目，不改正文。
- 需新开 ADR 的项（M0 清单）：斜投影范围（修正 ADR-0041）、位图 pack 通道（与 2026-11 弃用裁定对话）、pack 形态分层（T2）、ui_link 穿透（修正 ADR-0026 边界）。
- 验证基线：每期出口含 `cargo test --workspace` + `cargo app-test`/`app-clippy` + `bash scripts/smoke-gui.sh` + `cargo tree` 依赖红线断言（AGENTS §2.2/§7.1）。

（完）
