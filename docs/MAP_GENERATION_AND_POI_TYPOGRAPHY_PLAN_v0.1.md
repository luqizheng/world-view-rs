# 地图生成与 POI 字体完整方案 v0.1

> 项目：World View（worldview-rs）——单机优先、目录即工程、YAML 驱动的世界观构建工具
> 主题：① 现有地图生成技术分析　② POI 字体系统分析　③ 「文字 → JRPG 水彩地图」端到端方案
> 状态：**提议态（Proposed）**——本文不含已落地的机制变更，涉及决策处均标「转待裁」
> 关联：`ART_STYLE_SPEC_v0.1` / `ADR-0042` / `ADR-0043` / `ADR-0044` / `JRPG_MAP_CHARACTERISTICS_RESEARCH.md`

---

## 〇、一句话结论

**现有地图是「CPU 程序化合成位图 + 主题包色板 + 矢量描边层」的三层管线，没有 GPU 着色器、也没有位图图集**（图集已弃用）；POI 字体是**单份霞鹜文楷**、屏幕像素恒定、8 向纸色晕圈。

因此「文字生成地图」的**最短正确路径不是新写渲染器，而是补上「文字 → 世界蓝图 → 现有资产」的上游生成器**——渲染层与主题包**零改动**即可复用全部 JRPG 水彩观感。

但复用前必须先堵三处裂缝：**主题包消费路径实际未接通**、**POI 碰撞优先级语义反了**、**标签密度上限超行业基准 13–16 倍**。

---

## 一、现有地图生成技术分析

### 1.1 管线全景

地图从 YAML / 二进制到画布，走**四层**：

| 层 | 技术 | 关键实现 | 证据 |
|---|---|---|---|
| **L1 底图** | **CPU 逐格合成 RGBA8 `Image`** → 按 256 格切 chunk → 上传 GPU | `terrain_view/cells.rs::terrain_pixels`（行带并行）、`terrain_chunks.rs` | `cells.rs:1092`、`terrain_chunks.rs:70` |
| **L2 矢量描边** | marching squares 等值线 + 类型场边界 → Chaikin 平滑 → Douglas-Peucker 抽稀 | `wv-terrain/src/isoline.rs`、`terrain_view/isoline_overlay.rs` | `isoline.rs:48`、`isoline_overlay.rs:307` |
| **L3 程序化装饰** | 符号**程序化三角化**（Circle/HalfDisc/Triangle/Arc）→ 8 桶合批 mesh | `terrain_decor.rs` | `terrain_decor.rs:526`、`terrain_decor.rs:650` |
| **L4 标签层** | Bevy `Text2d` + 8 向纸色晕子实体 + AABB 碰撞过滤 | `drill.rs`、`label_halo.rs`、`label_collision.rs` | `drill.rs:912`、`label_halo.rs:33` |

**着色不是 shader。** 全仓无 WGSL/GLSL 着色器路径，着色是 Rust 函数指针：

```rust
type Shader = fn(&TerrainData, u8, &PaletteColors) -> Color;   // cells.rs:91
```

八种视图模式（`ViewMode`）各有独立 shade 函数，由单张 `MODE_TABLE` 真值表分发（`recipe.rs:171`）。**这是"地图模式"与"视图模式"统一后的单管线结构**——不存在"Google 地图"和"JRPG 地图"两条渲染路径，只有同一管线的 8 个取值。

### 1.2 关键技术决策与理由

| 决策 | 现状 | 理由 | 状态 |
|---|---|---|---|
| **位图图集** | **已弃用**，`ensure_atlas` 恒返回 `None` | 饱和纹理碎的 RPG 瓦片会淹没路线与地标 | `tile_atlas.rs:126` |
| **GPU 着色器** | **不用** | 全 Rust 函数指针，可单元测试、无 shader 编译链 | `cells.rs:91` |
| **CPU 合成** | **用**，行带并行 | 1024² 整图实测 317ms → 多核接近带宽封顶 | `cells.rs:1086` |
| **chunk 化** | 256 格/chunk，3 级 mip（CPU 盒式降采样） | 稳态**零像素拷贝**，只有脏矩形相交块被 patch | `terrain_chunks.rs:70`、`terrain_chunks.rs:36` |
| **LOD 选级** | 按**屏幕 texel 尺寸**非线性选级（`scale=世界单位/像素`） | 早期按常数阈值选级方向搞反，2026-10-15 修正 | `terrain_chunks.rs:153` |
| **装饰落位** | splitmix64 哈希，站点级 ±0.45 步距抖动 | 防棋盘感；**脊点吸附**（山符号落到峰上） | `terrain_decor.rs:187`、`terrain_decor.rs:776` |
| **合批** | 按 `(材质, 色, 尺寸档)` 分桶，单桶单 mesh，指纹不变则原地改写顶点 | ART_STYLE_SPEC §3.6「禁止逐块独立材质」 | `terrain_decor.rs:664` |

**结论：现有管线是「零外部资源的程序化绘制」路线**（ART_STYLE_SPEC §3.6 的等价规格），且已在 2026-10-14 / 10-20 两轮实机校准中把「填充色退后、图标层向前」做成了既定取舍。**新做地图生成器应当复用它，而不是另起炉灶。**

### 1.3 手绘水彩观感：11 项处理点

JRPG 水彩感**不是独立渲染路径**，而是 `TreatmentFlags`（11 个 `bool`）对同一条管线的差异化 gate：

| 处理 | 实现要点 | 落点 |
|---|---|---|
| 陆格颗粒 | ±4.5% 确定性明度抖动 | `cells.rs:403` |
| 水彩晕开 | 5×5 盒式模糊基色（ADR-0044 D1） | `cells.rs:361` |
| 边界揉边 | vnoise 扰动，幅度 0.45px | `cells.rs:468` |
| 海色渐变 | 按深度平方曲线 lerp + 深水冷偏 | `cells.rs:213` |
| 雪格双调 | 高海拔叠冷蓝灰 | `cells.rs:419` |
| 坡度浮雕 | 左上 45° 光源，钳 `[0.84, 1.16]` | `cells.rs:233` |
| 墨线亚线性 | 墨线半宽 ∝ `scale^0.78` | `isoline_overlay.rs:50` |
| 纸面雾霭 | 随 scale 插值，峰值 α 0.12，**编辑态免雾** | `atmosphere.rs:68` |
| 纸纹斑驳 + 晕影 | 两张 256² 程序化贴图 | `paper_overlay.rs:76` |
| 山体两面化 | 受光面 + 背光面（压暗右侧 0.85 α） | `terrain_decor.rs:335` |
| 波浪提亮 | 海面高光提亮 | ADR-0039 |

**这 11 项全部在 `TreatmentFlags::HAND_DRAWN` 下开启**，内置两包（`antique_default` / `antique_dusk`）都装载它。

### 1.4 ⚠️ 架构裂缝：主题包消费路径**实际未接通**

这是复用前**必须先堵**的一处。表面契约与实际行为不一致：

| 项 | 契约声称 | 实际 |
|---|---|---|
| `ActiveThemePack` | 「渲染系统统一取色入口」 | **无同步系统**——`init_resource` 后永远停在 `DEFAULT`（`app_bootstrap.rs:190`） |
| `theme_pack_id` | 决定风格 | 渲染消费点一律 `builtin::for_palette(view.map_palette)`——**风格实际由 `MapPalette`（日间/暮色）决定** |
| `wv-theme-chinese-ink` | 演示「加风格零改核心」 | **未被 app 消费**（唯一引用在 `examples/game_consumer.rs:40`）；其 `treatment` 还是 `EMPTY`（素图） |
| `TreatmentFlags.sea_ice` | 11 项之一 | **渲染从未接线**（`cells.rs` 无读取点） |

**影响**：ADR-0043 承诺的「加新风格 = 加新 crate、零改渲染层源码」目前只在**契约层**成立，运行时不可选。外部主题包是**演示件**，不是可用功能。

**分级**：🟡 中——不影响当前观感（内置两包行为正确），但**任何「换主题包生成新风格」的需求都会撞墙**。若文字生成地图方案要支持多风格输出，此项须先解。

> 附带发现：`tile_atlas.rs:126` 注释写「JRPG 图集已弃用（**2026-11** 用户裁定）」，而当前日期为 2026-10——**日期笔误**（应为 2026-10）。已列入 §七 待修。

---

## 二、POI 字体系统分析

### 2.1 三份字体，七个消费方

全部 `include_bytes!` 编译期嵌入，实际位于 **`crates/wv-render/assets/fonts/`**（ADR-0019 与 ART_STYLE_SPEC 仍写 `crates/wv-app/assets/fonts/`，路径已随 ADR-0020 拆 crate 变更而文档未追记）：

| 字体 | 体积 | 用途 | 消费方 |
|---|---|---|---|
| **霞鹜文楷** LXGW WenKai v1.530 | 24.7 MB | **画布标签唯一字体** | `drill.rs`×5（地名/势力/事件/区域/海名）、`streets.rs`×1、`measurement.rs`×1 |
| Noto Sans SC v2.004 | 10.1 MB | egui 中文 + **Bevy 回落字体** | 无直接消费方（仅 `label_handle()` 回落读一次） |
| Inter v4.001 | 334 KB | egui 拉丁 | egui `Proportional` 首位 |

**关键事实**：
- 7 个 `Text2d` 消费方**全部**走 `LoadedFonts::label_handle()`（文楷优先，回落 Noto），无一例外。
- 文楷字符集实测覆盖 **46926 码位**：CJK 统一表意 20992 + 扩展 A 6592 + 扩展 B+ 2615，**简/繁/日（汉字+平假名+片假名）全覆盖**。作为通用中文地图字体绰绰有余。
- 文楷在 egui 侧注册了命名族 `wv_map_label`（`fonts.rs:167`），但**全仓零消费方**——是给未来 egui 侧标签预留的。
- UI 面板**不用**文楷（楷体做界面正文可读性差），`Proportional = [Inter, Noto, 内置]` 顺序未变。

### 2.2 标签规格现状

| 标签类型 | 屏幕字号 | 字色令牌 | 色值 | 晕色 | 可拾取 |
|---|---|---|---|---|---|
| 地名 Place | 12 px | `LABEL_PLACE` | `#4A3F35` | `HALO_FILL` `#F2EAD8` | ✅ |
| 势力 Faction | 16.8 px（12×1.4） | `LABEL_REGION` | `#382F27` | ⚠️ 硬编码 Google 陆地色 | ✅ |
| 事件 Event | 12 px | — | 硬编码 `GOOGLE_CONTROL_TEXT` | ⚠️ 同上 | ✅ |
| 区域 Region | 16.8 px | `LABEL_REGION` | `#382F27` | ⚠️ 同上 | ✅ |
| 海名 Sea | 16.8 px | `LABEL_WATER` | `#3F5B6A` | ✅ | ❌ 有意 |
| 街名 Street | 9 px | `LABEL_FILL` | `#4A3F35` | ✅ | ❌ 无稳定 id |
| 测量 Measure | 13 px | 硬编码白 | `#FFFFFF` | 4 向纯黑描边 + 暗底板 | ✅ |

**共性机制**：
- **屏幕像素恒定**：`font_size = LABEL_PX × view.scale`（世界单位）⇒ 屏幕上恒为 `LABEL_PX`，**不随相机缩放**。分级只切**显隐**（`rank_visible_at_tier` 硬门：Nation ≥3 / Region ≥2 / Edit 全见），**不切字号**。
- **8 向纸色晕**：每标签 spawn 8 个 `Text2d` 子实体，偏移 1.1 屏幕像素，偏移写在**父局部系**（地图旋转不影响晕的屏幕观感）。晕色 = 主题纸面色。
- **屏幕对齐**：地图旋转时文字取相机旋转的**逆**，屏幕角恒为零（GLOSSARY「屏幕对齐文字」）。
- **三级色梯度**承担层级，**不是字号本身**；数据层已有 `font_size_ladder`（Nation 7 / Region 10 / Edit 13），但**渲染层零消费**。

### 2.3 已知缺陷（按严重度排序）

| # | 缺陷 | 位置 | 严重度 |
|---|---|---|---|
| 1 | **事件标签 spawn 错组件**——贴 `FactionLabel` 而非 `EventLabel`，事件标签被势力系统回收/复用 | `drill.rs:1324` | 🔴 高 |
| 2 | **字号两分支不一致**——faction/region/event 池复用分支写 `font_size`(12px)、新建分支写 `region_font_size`(16.8px)；**首帧大字、之后每帧被改回小字** | `drill.rs:1126` vs `1148`；`1490` vs `1512`；`1305` vs `1327` | 🔴 高 |
| 3 | **碰撞优先级语义与注释相反**——注释说「长标签优先保留」，`sort_by_key` 升序实际是**短标签优先** | `drill.rs:838` + `label_collision.rs:96` | 🟡 中 |
| 4 | **`PlaceKind` 权重从未实现**——`label_collision.rs` 文档声称的三级优先级，代码只有文本长度 | `label_collision.rs:9` | 🟡 中（文档漂移） |
| 5 | **碰撞盒原点偏移**——`Aabb` 按左下角构造，`Text2d` 以中心为原点，碰撞盒偏左上半字 | `label_collision.rs:47` vs `drill.rs:861` | 🟡 中 |
| 6 | **字符数口径不一**——`select_pois` 用 `chars().count()`，`label_collision` 用 `len()`（字节），中文差 3 倍 | `world_projection.rs:833` vs `drill.rs:839` | 🟡 中 |
| 7 | **密度上限 200 条**，JRPG 研究报告基准为 12–15 条同屏非重叠标签，**超 13–16 倍** | `drill.rs:856` vs `JRPG_MAP:152` | 🟡 中（设计待裁） |
| 8 | **街名 9 px 低于 ART_STYLE_SPEC §4.2 硬约束 3**「中文字号 ≥ 12px」 | `streets.rs:148` | 🟡 中（设计待裁） |
| 9 | **无字重**——全 Regular；`×1.4` 是字号不是字重 | `fonts.rs:39` | 🟢 低 |
| 10 | **无行高概念**——代码零命中，唯一"行高"是碰撞估算写死 `字号×1.2` | `label_collision.rs:79` | 🟢 低 |
| 11 | **晕色无视 ViewMode**——faction/event/region 硬编码 Google 陆地色 | `drill.rs:1115` / `1294` / `1479` | 🟢 低 |
| 12 | **`max_labels = 200` 散落 5 处**，无共享常量 | `drill.rs:856`/`1075`/`1263`/`1439`/`1621` | 🟢 低 |
| 13 | **字体资产路径文档过期**（三处写 `wv-app/assets/fonts/`） | ADR-0019:121、ART_STYLE_SPEC:436、fonts/README.md:1 | 🟢 低 |
| 14 | **`cjk_font` 零直接消费方**，README / ADR 对 Bevy 字体的描述已过时 | `fonts.rs:97`、README.md:243 | 🟢 低 |

---

## 三、完整方案：文字 → JRPG 水彩地图

### 3.1 方案总览

**核心取舍**：不写新渲染器，**复用现有 `wv-theme` + `wv-render` 管线**；新建的是**上游生成器**——把一段自然语言描述转成「世界蓝图」，再从蓝图生成标准资产（YAML 实体 + 地形栅格），由现有管线渲染。

```
自然语言描述
   │
   ▼  阶段 1：语义解析（LLM / 规则模板）
┌─────────────────────────────────┐
│  世界蓝图 WorldBlueprint (JSON) │  ← 唯一中间表示
│  · 地形参数（大陆形状/山脊/水系）│
│  · 生物群系分布规则            │
│  · 聚落清单（名/类型/坐标/层级）│
│  · 区域与疆界                  │
│  · 命名风格（音译/意译/后缀）   │
└─────────────────────────────────┘
   │
   ▼  阶段 2-3：蓝图 → 资产（写入 YAML / bin）
   examples/<world>/places/*.yaml、regions/*.yaml、terrain.bin
   │
   ▼  阶段 4：现有渲染管线（零改动）
   wv-theme 色板 + TreatmentFlags + wv-render 四层管线
   │
   ▼  阶段 5：POI 标注（新增规格，本方案 §四）
```

**为什么必须经过「蓝图」这一层**：
1. **P3（YAML 为主格式）**——蓝图只是生成期的中间表示，落盘产物仍是标准 YAML / 二进制，作者可手改。
2. **P2（目录即工程）**——生成器写进工程目录，不引入云端/注册表。
3. **P6 / P7（非破坏 + 时间轴）**——生成器只写**初始态**资产，历史与时间轴仍由 `wv-edit` 管辖。
4. **可复现**——蓝图是确定性中间层，同一蓝图 → 同一世界，可 diff、可回溯。

### 3.2 阶段 1：世界蓝图 Schema

蓝图是**生成器的私有契约**，不进工程目录（避免与 P2 冲突），但建议随生成器版本化。

```jsonc
{
  "schema_version": 1,
  "meta": {
    "seed": 20261021,              // 决定性：同 seed → 同世界
    "extent_km": 320,              // 图幅物理宽度（对齐 Map.extent.width_km）
    "grid": 1024,                  // 栅格边长（格）
    "name_style": { "language": "zh", "register": "古雅", "suffix": ["州","郡","渡"] }
  },
  "terrain": {
    "continent": { "shape": "coastal", "coverage": 0.42 },
    "relief":    { "peak_count": 7, "peak_height_m": 3200, "ridge_gaussian": 10 },
    "water":     { "rivers": 4, "lakes": 2, "sea_level_m": 0 },
    "biome_rules": [
      { "when": "height_m > 2750", "biome": "Snow" },
      { "when": "height_m > 1500", "biome": "Mountain" },
      { "when": "latitude > 0.72", "biome": "ConiferForest" }
    ]
  },
  "settlements": [
    { "name": "临渊", "kind": "Capital", "level": 3, "at": { "coast": "north" }, "near": { "water": true } }
  ],
  "regions": [
    { "name": "北境", "kind": "Nation", "from_settlements": ["临渊", "霜港"] }
  ]
}
```

**约束**：
- `seed` 是**唯一决定性输入**——所有随机（海岸线、山脊噪声、气候带边界、树冠落位、POI 落位）均从 `seed` 派生，与现有 `rivers_seed` / `hash01` 同一手法。
- `settlements.level` 直接对应 `Place.rank`（3 国家级 / 2 市镇 / 1 普通 / 0 微点）——**与 ADR-0047 统一门槛对接，不新造词表**。
- `kind` 目前是**语义输入**（决定命名与符号选择），落地时映射到 `Place.w/h` 或后续 `PlaceKind`（该字段 ADR-0017 Q5 标注为待裁）。

### 3.3 阶段 2：蓝图 → 地形栅格

**复用现有生成端能力，不新写噪声算法。** 工程内已有 `wv-gen-demo`（ADR-0042 生成端：山脊高斯 22→10 格、边缘噪声化、雪线 2250→2750、五档深海渐变至 -3200），蓝图只是把**硬编码参数换成蓝图驱动**：

| 蓝图字段 | 落到 | 复用 |
|---|---|---|
| `relief.ridge_gaussian` | 山脊高斯半径（格） | gen-demo 现有参数 |
| `relief.peak_height_m` | 峰高 | 同上 |
| `terrain.biome_rules` | 高度/纬度 → biome 阈值 | 同上 + `wv-terrain::biome::display_kind` |
| `water.rivers` | 派生河（`rivers_seed`） | `world_projection` 现有派生链 |

**产出**：`terrain.bin`（高度场 + 地形类型）+ 索引 YAML —— 走**现有**二进制分块纪律（AGENTS §5：须 `RasterRef` 显式声明 + `format_version` + 校验和）。

### 3.4 阶段 3：蓝图 → POI 实体

**POI 落位规则**（与现有渲染零冲突）：

| 蓝图约束 | 落位算法 | 依赖 |
|---|---|---|
| `at.coast: "north"` | 沿海岸线等距取点（`isoline::assemble_chains` 的 level=0.0 链） | 复用 `wv-terrain::isoline` |
| `near.water: true` | 在水系缓冲带内取点 | 复用派生河折线 |
| `level: 3` | `Place.w/h` 置 `Some`（→ `effective_rank` 折 3） | 现有折算 |
| 名字 | 由 `name_style` + 音节表生成，可被作者覆盖 | 新增（生成器侧） |

**产出**：`places/<id>.yaml`（**每实体一文件**，AGENTS §4.5），ID 为可读字符串（`city_linyuan` 式，§4.4）。

### 3.5 阶段 4：渲染复用（**零改动**）

| 资产 | 消费方 | 是否需改渲染层 |
|---|---|---|
| `terrain.bin` | `terrain_chunks` | ❌ |
| `places/*.yaml` | `drill.rs` 标签链 | ❌ |
| `regions/*.yaml` | `region_outline` / `region_overlay` | ❌ |
| 主题包 | `wv-theme`（需先解 §1.4 裂缝） | ⚠️ 需先接通消费路径 |
| POI 字体规格 | `drill.rs` 字号/色/晕 | ⚠️ 按 §四 调参（不新增依赖） |

**结论：文字生成地图的主体工作量在「生成器」，不在渲染层。** 这与 ADR-0044 D5「数据零迁移，新观感是纯渲染层」的思路一致。

### 3.6 阶段 5：POI 标注规格

见下节。**核心改动只有三处调参 + 一处优先级修正**，不需要新字体资产。

---

## 四、POI 字体完整规格

### 4.1 字体选型（**维持现状，不加字重**）

| 场景 | 字体 | 理由 |
|---|---|---|
| 画布地名 / 街名 / 海名 / 势力 / 事件 / 区域 / 测量 | **霞鹜文楷 Regular** | 已覆盖简繁日全覆盖；手绘参考图的书法体身份 |
| egui 面板 | Inter + Noto Sans SC | 楷体做界面正文可读性差 |

**不加 SemiBold 的理由**：文楷每字重 ≈ 25 MB，加粗 = 资产 +25 MB；而 12–17 px 下 Regular 与 SemiBold 的视觉差有限（`assets/fonts/README.md` 已记此顾虑）。**层级由「字号 + 色梯度」承担**——这与 ART_STYLE_SPEC §4.2 硬约束 4「字重只用 2 档」精神一致。

> 转待裁：若实机确认国名/大区名在 Nation 档不够醒目，替代方案是**给国名加深色 + 加 1px 纸色底衬**（零资产成本），而非加字重文件。

### 4.2 字号阶梯（修订建议）

**现状**：12 / 16.8 / 9 px 三档 + 屏幕像素恒定。**层级不随缩放变，只切显隐**——这是正确的（缩放时改字号会导致文字忽大忽小）。

建议调整为：

| 标签类型 | 现状 | 建议 | 依据 |
|---|---|---|---|
| 区域 / 势力 | 16.8 px | **17 px** | 12×1.4=16.8 是小数，与像素栅格不对齐；17 ≈ 12×1.42，视觉等效且落整数 |
| 地名 | 12 px | **12 px**（维持） | 满足 SPEC §4.2 硬约束 3「中文 ≥12px」 |
| 事件 | 12 px | **12 px**（维持） | |
| 海名 | 16.8 px | **17 px** | 同区域 |
| 街名 | 9 px | **11 px** | ⚠️ 现值违反 SPEC §4.2 硬约束 3（中文 ≥12px 的下限放宽口径：地图内嵌文字不同于 UI 正文，但 9px 中文在 1x 屏已不可读）；**11px 是「改为 12」与「保持小」之间的折中，建议实机二选一** |
| 测量 | 13 px | **13 px**（维持） | |

> **转待裁（字号）**：街名 9 → 11 还是 12。11px 让全国视野不炸、编辑级又勉强可读；12px 严格合规但街名会与地名争夺视觉权重。**需实机截图对比裁定。**

### 4.3 颜色梯度（**维持现状**）

| 令牌 | 色值 | 语义 | 状态 |
|---|---|---|---|
| `LABEL_PLACE` | `#4A3F35` | 城 / POI | ✅ 维持（= SPEC `label.fill`） |
| `LABEL_REGION` | `#382F27` | 国 / 大区（最深墨） | ✅ 维持 |
| `LABEL_WATER` | `#3F5B6A` | 海名（青蓝） | ✅ 维持 |
| `LABEL_FILL` | `#4A3F35` | 街名 | ✅ 维持 |
| `HALO_FILL` | `#F2EAD8` | 纸色晕 | ✅ 维持 |

**唯一修正是缺陷 #11**：faction / event / region 的晕色应走 `palette::colors(view.map_palette)` 的纸面色，**不得硬编码 `GOOGLE_COAST_LAND`**（`drill.rs:1115` / `1294` / `1479`）。否则在暮色主题下，褐墨字配近白晕 = 白纸上的深墨，晕圈反而比字更亮。

### 4.4 晕圈规格（**维持现状，修正一处**）

| 项 | 值 | 说明 |
|---|---|---|
| 方向数 | 8（4 正交 + 4 对角，对角 1/√2 归一） | 正交留对角缝，8 向才闭合 |
| 偏移 | 1.1 屏幕像素 | `HALO_PX` |
| 坐标系 | 父局部系 | 地图旋转不影响晕的屏幕观感 |
| Z | 父标注后撤 0.005 | 保证晕在下、字在上 |
| 补强建议 | 测量标注已有 4 向纯黑描边 + 暗底板，**地名/街名无描边** | ⚠️ 见下 |

> **转待裁（描边）**：`GOOGLE_LABEL_STROKE #FFFFFF` 已定义但从未被消费。JRPG 参考图的纸面地名的可读性主要靠**纸色晕 + 深墨字**，当前 8 向晕已足够；**不建议**给地名加描边（会让水彩纸面变"游戏 UI 感"）。**但必须在 Nation 档实测**：远景纸雾 α 0.12 + 晕圈 1.1px 可能不足以托住 17px 墨字。

### 4.5 碰撞优先级（**必须修正**）

**现状缺陷 #3/#4**：优先级 = `name.len()`，且语义与注释相反。

**建议口径**（对齐 ADR-0047 统一门槛）：

| 优先级 | 规则 | 理由 |
|---|---|---|
| 1 | **呈现级别 `rank` 降序**（3 > 2 > 1 > 0） | 与点标预算 `select_pois` 同序（ADR-0030 D3），点标与标签才不打架 |
| 2 | `is_city` 降序 | 城市优先 |
| 3 | **名称长度降序**（长名信息量大） | 修正现状的升降序颠倒 |
| 4 | 词典序 | 稳定排序，保证确定性 |

**附带修正**：
- 缺陷 #5：`Aabb` 改为**中心原点**（`Text2d` 语义），消除半字偏移。
- 缺陷 #6：字符数统一用 `chars().count()`，与 `select_pois` 对齐。
- 缺陷 #4：`label_collision.rs` 模块文档同步改为"rank → is_city → 名称长度 → 词典序"。

### 4.6 密度预算（**转待裁**）

| 项 | 现状 | JRPG 研究基准 | 建议 |
|---|---|---|---|
| POI 点标 | 400 | — | ✅ 维持（ADR-0030 D3） |
| POI 文字标签 | 200 | 12–15 条同屏非重叠 | **建议 60** |

**依据**：AABB 过滤后剩下的即"不重叠标签"，200 条意味着屏幕上最多 200 个不重叠文字块——在 1200×800 视口内即"字海"，与 Google 地图城市级视野（数十条）差两个数量级。**60 条**在 Nation 档可容纳约 20 个区域名 + 40 个 POI 名，Region 档因 rank 硬门已自动收敛，Edit 档由碰撞过滤兜底。

> **转待裁（密度）**：60 是工程估值，需实机在三档视野下截图核对。**同时须确认**：密度下调后，是否需要在低档位**淡出**被硬门放行的次要标签（而非直接不画）——这涉及 ADR-0030 D2「硬门取代淡出带」的原决策，**不得静默推翻**。

---

## 五、实施路线

| 阶段 | 内容 | 触及原则 | 预估 |
|---|---|---|---|
| **M0 修缺陷** | 缺陷 #1 / #2（高危）+ #11（晕色） | 无（纯修） | 小 |
| **M1 标签规格对齐** | #3 / #4 / #5 / #6 / #12 + §4.2 字号 + §4.6 密度 | P7 | 中 |
| **M2 主题消费路径** | §1.4 裂缝：`ActiveThemePack` 同步系统 + 消费点改读 | P3 / P4 | 中（需 ADR） |
| **M3 生成器** | 蓝图 Schema → 地形/POI/区域资产 | P2 / P3 / P6 / P7 | 大（新增 crate） |
| **M4 文档追写** | 字体资产路径、图集弃用日期、README/ADR 描述 | §6.3 | 小 |

**M0/M1 属渲染层缺陷修复，不改机制**；M2 触碰主题契约，**须开 ADR**（建议编号 ADR-0048，前 0044 已撞号、0045–0047 已占）；M3 触及 P2/P3/P6/P7，**须开 ADR 并在 README §12 登记**。

**新增 crate 建议**：`wv-gen`（与 `wv-core` 平行，纯数据叶子，零 Bevy 依赖）——生成器必须能独立跑 CLI、独立测试（AGENTS §2.3 同一原则）。

---

## 六、待裁问题清单

| # | 问题 | 选项 | 建议 |
|---|---|---|---|
| Q1 | 主题包消费路径是否本轮接通 | A 本轮解 / B 推迟到生成器需要多风格时 | **A**——生成器若要输出多风格，必须先解 |
| Q2 | 街名字号 | A 9px 维持 / B 11px / C 12px | **B**，实机二选一 |
| Q3 | 标签密度上限 | A 200 维持 / B 60 / C 随档位分级 | **B**，并评估是否需淡出 |
| Q4 | 是否加文楷 SemiBold | A 不加（用色梯度） / B 加（+25MB） | **A** |
| Q5 | 地名是否加描边 | A 不加（现有晕足够） / B 加 | **A**，Nation 档实测后复评 |
| Q6 | 生成器蓝图是否落盘进工程 | A 不落盘（生成期中间态） / B 落 `blueprint.yaml` | **A**——落盘会与 P2「目录即工程」的「世界数据」语义混淆 |
| Q7 | 蓝图是否引入 LLM 依赖 | A 引入 / B 规则模板先行 | **B 先行**——LLM 属 P1 单机可运行的边界外；可作为可选外部输入，核心生成仍本地确定 |

---

## 七、待修文档漂移（不涉机制，可随手做）

| # | 位置 | 现状 | 应改为 |
|---|---|---|---|
| 1 | `tile_atlas.rs:126` | 「2026-11 用户裁定」 | **2026-10** |
| 2 | ADR-0019:121 / ART_STYLE_SPEC:436 / fonts/README.md:1 | `crates/wv-app/assets/fonts/` | `crates/wv-render/assets/fonts/` |
| 3 | README.md:243 / docs/adr/README.md:82 | 「Bevy Text2d 用 NotoSC 单句柄」 | 文楷优先 + Noto 回落 |
| 4 | `label_collision.rs:9` | 「PlaceKind 权重 → 文本长度 → 词典序」 | 「rank → is_city → 名称长度降序 → 词典序」 |
| 5 | `streets.rs:153-155` | 函数文档说「只在编辑级视野」 | 限��已删（`:205` 注释为准） |
| 6 | `assets/fonts/README.md:5` | 「只放正文字重（Regular）——只消费这两个文件」 | 三个文件 |

---

## 附录：关键证据索引

```
crates/wv-theme/src/theme_pack.rs          ThemePack 契约 · TreatmentFlags 11 项 · builtin 两包
crates/wv-theme/src/colors.rs              36 槽色板 · LABEL_* / HALO_FILL 令牌
crates/wv-theme/src/registry.rs            ThemePackRegistry / MergedThemeRegistry
crates/wv-theme-chinese-ink/src/lib.rs     外部主题包（占位，未被 app 消费）
crates/wv-render/src/terrain_view/cells.rs CPU 合成 · shade 分派 · 11 项处理点
crates/wv-render/src/terrain_chunks.rs     256 格 chunk · 3 级 mip · 脏矩形 patch
crates/wv-render/src/terrain_decor.rs      8 桶合批 · 程序化符号 · 脊点吸附
crates/wv-render/src/terrain_view/isoline_overlay.rs  等值线管线 · 亚线性墨线
crates/wv-render/src/terrain_view/atmosphere.rs      纸面雾霭 · 纸张噪点
crates/wv-render/src/paper_overlay.rs      纸纹斑驳 · 四角晕影
crates/wv-render/src/drill.rs              标签主战场（5 类标签 · 2466 行）
crates/wv-render/src/streets.rs            街名标签
crates/wv-render/src/measurement.rs        测量标注（唯一有描边+底板的）
crates/wv-render/src/label_collision.rs    AABB 碰撞 · 优先级
crates/wv-render/src/label_halo.rs         8 向纸色晕
crates/wv-render/src/label_pick.rs         拾取注册表
crates/wv-render/src/state.rs              LodTier · 统一门槛 · map_to_view
crates/wv-render/src/theme/fonts.rs        字体装配（3 份 · 文楷命名族）
crates/wv-render/assets/fonts/             实际资产目录（36.8 MB）
crates/wv-terrain/src/isoline.rs           marching squares · Chaikin · DP
crates/wv-terrain/src/biome.rs             display_kind（10 槽映射）
docs/ART_STYLE_SPEC_v0.1.md               §1.4.5 色板 · §1.5 光影 · §3.6 程序化 · §4.2 字体
docs/JRPG_MAP_CHARACTERISTICS_RESEARCH.md 标签量化阈值（≥10px / 12–15 条）
docs/adr/ADR-0042-hand-drawn-nation-style.md  手绘 Nation · 追记三 = ADR-0044
docs/adr/ADR-0043-theme-crate-extraction.md    主题层抽 crate
docs/adr/ADR-0047-uniform-presentation-rank.md 统一呈现级别
```
