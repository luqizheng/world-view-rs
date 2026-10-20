# 风格可替换架构方案

> **⚠️ 历史设计稿（2026 Q1）· 不作为现行规格**：本文面向 **Tauri v2 + Vite + TypeScript** 前端，**未对接到当前 Rust + Bevy 现状**（技术栈、渲染后端 `svg/canvas2d/webgl2`、`theme.yaml`/`view.yaml` 资源加载模型均已落后）。其「地图层可替换 / 软件壳不替换」的核心原则与契约骨架，已被 [ADR-0016](adr/ADR-0016-jrpg-antique-map-default-theme.md)（常量层前身）与 [ADR-0023：主题包注入契约](adr/ADR-0023-theme-pack-injection-contract.md)（Rust 移植，其 §5 C 显式记录本稿偏离项）承接。
> **本文保留用途**：作为「契约骨架来源」的历史出处与溯源依据，被 ADR-0008 / 0016 / 0023、`crates/wv-render/src/drill.rs`、README §12-12/§12-33 引用。正文按 AGENTS §6.3 不改，仅此状态头为追记。
>
> 项目：world-view-rs（《暗流涅槃》sim 层「世界观视图层」前端子项目）
> 技术栈基线：Tauri v2 + Vite + TypeScript；渲染以 Web 技术（SVG / Canvas / WebGL 视具体）为主
> 项目惯例：配置严格走 YAML，禁止硬编码
>
> **用户诉求（逐字）**：「美术风格。JRPG风格，但是要留出可以替换风格的接口。1. 地图风格可以替换 2. 软件风格偏向于工具类型。不做替换」

---

## 0. 一句话结论

把前端拆成**两层**：

- **表现层内的「地图渲染层」= 可替换**：其外观**完全由 Theme Pack（风格包）驱动**，代码只认契约（manifest schema + 资源寻址规则），不认具体美术。
- **软件壳 UI 层 = 固定**：工具型交互外壳（面板、工具栏、时间轴、Inspector、诊断）**不做主题化**，只允许一个「设计令牌层」（间距/圆角/字号/暗色模式）供内部一致性使用，**不对外开放替换**。

两层通过**一个单向数据流接口**通信：壳层持有权威状态（时间游标、选中实体、视口变换、校验结果)，地图层只**订阅**这些状态并渲染像素；地图层只向上抛**语义事件**（点击了哪个 PlaceId / 哪个地图节点），**永远不向上抛绘制细节**。

> **前提假设（假设变了结论要重估）**：
> A1. 只做**单人 / 极小团队**开发 → 优先「低维护成本 + 少工具链」，凡是可以靠「约定 + manifest」解决的，不引入构建插件或运行时框架。
> A2. 地图为**静态**（《DESIGN_DELIVERY_v0.2》§1.6/V6 已裁定「地形静态 + 预留全区间」，历法只改显示不动地形）→ 本文按静态地图设计；若未来地图需要时间区间化，需重估第 4 节的缓存失效粒度。
> A3. 「地图风格」= **视觉表现**（图块外观 / 色板 / 字体 / 图标 / 滤镜），**不包含地图语义数据**（节点、连线、Place 位置）。语义数据归 `wv-core` / `wv-io`，与风格解耦。这是本次最关键的边界假设。

---

## 1. 分层设计

### 1.1 分层图

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Tauri v2 Shell (Rust)                           │
│   wv-core / wv-io / wv-edit / wv-app  →  IPC (commands + events)       │
└───────────────────────────────┬────────────────────────────────────────┘
                                 │  invoke() / listen()
┌───────────────────────────────▼────────────────────────────────────────┐
│  前端 (Vite + TS)                                                       │
│                                                                         │
│  ┌───────────────────────────────────────────────────────────────────┐ │
│  │  L3  【固定壳层】 Tool Shell  —— 不做主题替换                       │ │
│  │  ├ Toolbar / MenuBar / Command Palette                             │ │
│  │  ├ Inspector（属性面板）                                           │ │
│  │  ├ Timeline Axis（时间轴，L1–L4 四层）                              │ │
│  │  ├ Sidebar（Stories / Events / Layers 列表）                        │ │
│  │  ├ Validation Panel（四态校验输出）                                 │ │
│  │  └ Design Tokens（间距/圆角/字号/暗色模式）— 仅内部一致性，不开放    │ │
│  └───────────────────────────────┬───────────────────────────────────┘ │
│                                   │                                     │
│  ┌────────────────────────────────▼──────────────────────────────────┐ │
│  │  L2  【桥接层】 View Bridge —— 固定，双向但职责分明                 │ │
│  │  ├ 下行：ViewState 快照（Store 只读订阅）                           │ │
│  │  └ 上行：语义事件 SemanticEvent（只含 id，不含绘制细节）            │ │
│  └───────────────────────────────┬───────────────────────────────────┘ │
│                                   │                                     │
│  ┌────────────────────────────────▼──────────────────────────────────┐ │
│  │  L1  【可替换层】 Map Renderer —— 外观 100% 由 Theme Pack 驱动      │ │
│  │  ├ Style Resolver（把 manifest 解析成运行时 StyleTokens）           │ │
│  │  ├ ThemeRegistry（已装载风格包索引）                               │ │
│  │  ├ AssetResolver（按风格包 + 逻辑名 → URL/纹理）                    │ │
│  │  └ RenderBackend（SVG / Canvas / WebGL 之一，按风格包能力声明选）   │ │
│  └───────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                 ▲
                                 │ 静态资源（构建期打包 or 运行期加载）
                        ┌────────┴────────┐
                        │  Theme Packs    │  ← 「只加资源 + manifest」
                        │  jrpg_classic/  │
                        │  ink_wash/      │
                        └─────────────────┘
```

### 1.2 边界定义（谁可替换 / 谁固定）

| 层 | 名称 | 可替换性 | 内容 | 判定依据 |
|---|---|---|---|---|
| L1 | Map Renderer | ✅ **主题可替换层** | 地图节点/连线/图块/地形填充/地图内图标/地图色板/地图字体 | 用户原话「地图风格可以替换」 |
| L2 | View Bridge | ⛔ **固定** | 状态订阅协议 + 语义事件协议 | 替换它等于改架构，不是改风格 |
| L3 | Tool Shell | ⛔ **固定** | 工具型 UI 外壳全部 | 用户原话「软件风格偏向于工具类型。不做替换」 |
| — | Design Tokens（壳层内部） | ⛔ **固定** | 壳层自己的间距/圆角/字号/暗色模式 | 属于壳层内部实现，不对外暴露为「风格」 |

**边界一句话**：**「地图画出来的东西」可换；「地图之外的一切」不可换。**

### 1.3 两层通信接口（L2 Bridge 契约）

**下行（壳 → 地图）：只传状态快照，不传命令。**

```ts
// 壳层拥有的权威状态，地图层只能读
interface MapViewState {
  readonly viewport: { x: number; y: number; zoom: number };  // 视口变换
  readonly cursorTime: number;        // 当前时间游标（天数计数器 i64 → number 时注意精度，见风险 R3）
  readonly selectedIds: readonly string[];   // 选中实体（PlaceId / MapNodeId）
  readonly visibleLayers: readonly string[]; // 逻辑图层名（来自 manifest 声明的 layer 槽位）
  readonly highlightIds: readonly string[];  // 校验高亮（如 Unreachable 链）
  readonly revision: number;          // 语义数据版本号，递增即触发地图重算
}
```

**上行（地图 → 壳）：只抛语义事件，禁止抛绘制细节。**

```ts
// ⚠️ 契约红线：事件里只允许出现「逻辑 id」与「交互语义」，
//    绝不允许出现像素坐标、颜色、纹理名等表现细节——否则风格层会反向泄漏进壳层。
type SemanticEvent =
  | { type: 'node.clicked';      nodeId: string; modifiers: Modifiers }
  | { type: 'node.hovered';      nodeId: string | null }
  | { type: 'node.doubleClicked';nodeId: string; modifiers: Modifiers }
  | { type: 'edge.clicked';      edgeId: string; modifiers: Modifiers }
  | { type: 'canvas.contextMenu'; nodeId: string | null }   // 不含 x/y，壳层自己去取
  | { type: 'viewport.changed';  viewport: MapViewState['viewport'] }  // 地图内部手势回调
  | { type: 'renderer.ready';    packId: string; packVersion: string }
  | { type: 'renderer.error';    code: RendererErrorCode; detail: string };
```

> **为什么这样切**：壳层是工具（面板、按钮、数字），地图是画布。工具不关心画布用什么笔刷；画布不关心工具栏怎么排版。二者共享的是**语义**（哪个节点被选中），不是**外观**。

---

## 2. 主题包（Theme Pack）契约

一个风格包 = **一个目录 + 一份 `theme.yaml` + 一组资源**。**加风格 = 加目录，不改代码**（目标，见第 5 节）。

### 2.1 目录结构

```
themes/                                  # 所有风格包根目录（构建期扫描）
└── jrpg_classic/                        # ← 风格包 id（= 目录名，须匹配 [a-z0-9_]+）
    ├── theme.yaml                       # ★ 必需：manifest
    ├── palette.yaml                     # 必填（或内联进 theme.yaml，二选一）
    ├── atlas/                           # 图集/图块（切图产物）
    │   ├── terrain.png                  #   ← 由美术侧按 §6 规格切好
    │   ├── terrain@2x.png
    │   ├── entities.png
    │   ├── entities@2x.png
    │   └── icons.png
    ├── fonts/                           # 地图内字体（壳层字体不在此处）
    │   ├── map-label.woff2
    │   └── map-label-bold.woff2
    ├── filters/                         # 可选：滤镜/叠加参数（YAML，不写死着色器）
    │   └── paper_grain.yaml
    └── README.md                        # 建议：风格说明 + 授权信息
```

> **资源目录里禁止出现代码**。风格包是纯数据 + 图片 + 字体。任何需要「逻辑」才能表现的效果，都必须抽象成 manifest 里的**声明式字段**（见 2.2 的 `filters` / `tilesets`）。

### 2.2 `theme.yaml` manifest schema（字段级、可校验）

> 类型记法：`string` / `int` / `float` / `bool` / `enum` / `[T]` 数组 / `{K:V}` 映射。
> 「必填」= schema 校验不通过则整包拒绝装载。

```yaml
# ─────────────────────────────────────────────────────────────
# 主题包 manifest —— 版本、兼容性、能力、资源清单
# ─────────────────────────────────────────────────────────────
schema: theme-pack            # 必填 string，固定字面量，用于识别文件类型
schema_version: 1             # 必填 int，本 manifest 遵循的 schema 版本（当前 = 1）

id: jrpg_classic              # 必填 string，须 = 所在目录名，须匹配 ^[a-z][a-z0-9_]*$
display_name: "JRPG 经典"      # 必填 string，UI 展示名（可含中文）
version: "1.2.0"              # 必填 string，语义化版本，本风格包自身版本
description: "16-bit 质感、暖色调、图块化地形的回合制 JRPG 地图风格。"  # 可选 string
author: "art-team"            # 可选 string
license: "CC-BY-4.0"          # 可选 string（分发合规用；缺失仅告警不阻断）
preview: "atlas/preview.png"  # 可选 string，风格选择器缩略图（相对包根）

# ── 兼容性 ──────────────────────────────────────────────────────
compat:
  min_viewer: "0.1.0"         # 必填 string，本包要求的最低前端版本（semver）
  backend: [webgl2, canvas2d] # 必填 [enum]，本包能在哪些渲染后端工作
                              #   enum ∈ { svg, canvas2d, webgl2 }
  dpr: [1, 2]                 # 可选 [int]，提供 @Nx 的资源倍率档（缺省 [1]）

# ── 调色板 ──────────────────────────────────────────────────────
palette:                      # 必填（可 inline，也可 palette_ref 指向 palette.yaml）
  bg:          "#1a1614"      # 画布底色（地图区，非壳层）
  grid:        "#2e2823"      # 参考网格
  terrain_land:"#6b7f4a"
  terrain_sea: "#2b4a63"
  terrain_coast:"#d8c9a3"
  node_default:"#e8dcc0"
  node_selected:"#f2c14e"
  node_disabled:"#6a6155"
  edge_default:"#8a7f6a"
  edge_highlight:"#f2c14e"
  label:       "#efe6d2"
  label_muted: "#a89a80"
# 取值须为 #RGB / #RRGGBB / #RRGGBBAA；颜色语义键由本 schema 固定（见下表），
# 风格包不得自造新键（否则壳层/渲染层无法对应）。

# ── 图集与图块 ──────────────────────────────────────────────────
atlas:
  terrain:                    # 逻辑名（被渲染层按语义引用，不是按文件名）
    chunks: "atlas/terrain.png"       # 必填 string
    chunks_2x: "atlas/terrain@2x.png" # 可选（dpr:2 时用）
    tile: 32                  # 必填 int，单块像素边长（@1x）
    columns: 8                # 必填 int，图集列数
    rows: 6                   # 必填 int，图集行数
    tiles:                    # 必填 {string:int}，逻辑地形类型 → 图集索引
      land: 0
      sea: 1
      coast: 2
      forest: 3
      mountain: 4
      desert: 5
  # 逻辑类型键须来自下方 terrain_types 白名单，不得自造
  entities:
    chunks: "atlas/entities.png"
    tile: 32
    columns: 4
    rows: 2
    tiles:
      city: 0
      village: 1
      fort: 2
      port: 3
  icons:
    chunks: "atlas/icons.png"
    tile: 16
    columns: 4
    rows: 1
    tiles:
      anchor: 0
      story: 1
      event: 2
      warned: 3

# ── 字体（仅地图内文本；壳层字体不受此影响）──────────────────────
fonts:
  map_label:
    regular: "fonts/map-label.woff2"     # 必填 string
    bold: "fonts/map-label-bold.woff2"   # 可选 string
    family_name: "MapLabel"              # 必填 string，字体族名（渲染层用它注册）
    fallback: ["system-ui", "sans-serif"]# 可选 [string]

# ── 渲染管线参数（声明式，不写代码）─────────────────────────────
render:
  min_zoom: 0.25              # 可选 float，本风格可读的最小缩放
  max_zoom: 8.0               # 可选 float
  label_min_zoom: 0.75        # 可选 float，低于此缩放隐藏地图标签（LOD 由风格决定而非写死）
  anti_alias: true            # 可选 bool
  image_smoothing: nearest    # 可选 enum ∈ {nearest, linear} —— 像素风通常 nearest

filters:                      # 可选 [FilterSpec]，按顺序叠加。无则不加滤镜
  - id: paper_grain
    kind: overlay             # enum ∈ {overlay, tint, vignette, scanlines}
    opacity: 0.06             # float 0..1
    texture: "atlas/paper.png"# kind=overlay 时必填
```

**允许的调色板语义键（白名单，schema 固定，风格包不得增删）**：

| 键 | 用途 | 必填 |
|---|---|---|
| `bg` | 地图画布底色 | ✅ |
| `grid` | 参考网格线 | 可选 |
| `terrain_land` / `terrain_sea` / `terrain_coast` | 地形基底色（未被图块覆盖时兜底） | 可选 |
| `node_default` / `node_selected` / `node_disabled` | 地图节点三态 | ✅ |
| `edge_default` / `edge_highlight` | 连线两态 | ✅ |
| `label` / `label_muted` | 地图内文字两态 | ✅ |

**允许的地形逻辑类型白名单**（`atlas.terrain.tiles` 的键）：
`land, sea, coast, forest, mountain, desert, tundra, river`。

### 2.3 校验规则（装载前必须全过，否则整包拒绝）

| # | 规则 | 不通过后果 |
|---|---|---|
| V1 | `schema == "theme-pack"` 且 `schema_version` 为已知值 | 拒绝装载，报 `THEME_SCHEMA_UNSUPPORTED` |
| V2 | `id` == 目录名，且匹配 `^[a-z][a-z0-9_]*$` | 拒绝装载 |
| V3 | `compat.min_viewer <= 当前前端版本` | 拒绝装载并提示升级软件 |
| V4 | `compat.backend` 与可用后端**有交集** | 拒绝装载，提示该风格需哪个后端 |
| V5 | manifest 中**所有**引用的资源路径**存在** | 拒绝装载（**不做静默降级**，缺图必须暴露） |
| V6 | `atlas.*.tile * columns/rows` 与实际图片尺寸**自洽** | 拒绝装载，报尺寸不符（防止美术没按切图规格交） |
| V7 | `palette` 键 ∈ 白名单，值为合法颜色 | 拒绝装载 |
| V8 | `atlas.*.tiles` 的键 ∈ 对应白名单 | 拒绝装载 |
| V9 | `dpr` 中声明的倍率，对应 `_2x` 等资源存在 | 该倍率降级使用 @1x + 告警（**唯一允许降级的项**，仅影响清晰度不影响正确性） |

> **V5/V6 存在意义**：把「美术没按规格交」的问题**挡在装载期**，而不是留到运行时画出一个破图。这是本契约对美术侧的硬约束（呼应第 6 节）。

### 2.4 装载产出的运行时对象

manifest 是**静态数据**；渲染层不直接读它，而是先经 Style Resolver 转成运行时 `StyleTokens`：

```ts
interface StyleTokens {
  packId: string;
  packVersion: string;
  palette: Record<PaletteKey, string>;          // 键已校验合法
  atlas: Record<string, ResolvedAtlas>;         // 逻辑名 → { url, tile, columns, rows, tiles }
  fonts: Record<string, { family: string; regUrl: string; boldUrl?: string }>;
  render: ResolvedRenderParams;                 // 已填默认值
  filters: ResolvedFilter[];
}
```

渲染层**只依赖 `StyleTokens`**，不依赖 manifest 文件路径。这样切换风格 = 换一份 `StyleTokens`，渲染代码零改动。

---

## 3. 配置入口（YAML 驱动，禁止硬编码）

### 3.1 风格配置文件位置

延续项目「配置走 YAML」惯例，在**工程根**放一份前端可见的视图配置（与 `world.yaml` 同级或放在 `view/` 子目录）：

```
<project>/
├── world.yaml
├── view/
│   └── view.yaml            # ★ 新增：视图/风格配置
└── themes/                  # 见第 2 节（可放工程内，也可放全局）
```

> **为何放工程内而非全局**：风格选择是「这个工程长什么样」的属性，应随工程走、可进 git、可被 round-trip（呼应 `wv-io` 的 YAML round-trip 原则）。全局主题库另论（见 5.3）。

### 3.2 `view.yaml` schema

```yaml
schema: view-config
schema_version: 1

style:
  # 默认风格 id（必填；须在 available 内或存在于 themes/ 目录）
  default: jrpg_classic

  # 可切换风格列表（必填 [string]；UI 风格选择器就渲染这个列表）
  available:
    - jrpg_classic
    - ink_wash
    - blueprint

  # 切换策略：runtime（默认，运行时可切）| build（构建期烤入）
  switch_mode: runtime

  # 找不到 default 时的兜底（可选；缺省则报错）
  fallback: jrpg_classic

  # 资源寻址根（可选；缺省 = 工程内 themes/）
  # 允许 [工程内相对路径, 全局主题库绝对路径]，按序查找
  search_paths:
    - "themes/"
    - "%APP_DATA%/worldview/themes/"
```

### 3.3 加载流程（顺序固定）

```
1. Tauri 启动 → wv-io 读 world.yaml（既有）
2. 读 view/view.yaml
   ├─ 不存在 → 使用内置默认（见 3.4），并提示「尚未配置视图风格」
   └─ 存在 → 按 schema 校验
3. 扫 search_paths 下所有 themes/*/theme.yaml → 建 ThemeRegistry 索引（只读 manifest 头部，不加载图）
4. 过滤 available：取出 default 与 available 中「在 registry 里存在 且 compat 通过」的包
   ├─ default 不存在/不兼容 → 用 fallback；fallback 也不行 → 报错并降级到「无风格占位」（见 3.5）
5. 装载 default 包：V1–V9 全过 → 产出 StyleTokens → 注入 Map Renderer
6. 其余 available 包：仅登记，**懒加载**（用户切到时才真正加载资源）
```

### 3.4 无配置时的内置默认（拒绝硬编码美术，允许内置「空风格」）

**不内置任何具体风格皮肤**（否则又是硬编码）。无 `view.yaml` 时，前端使用一份**程序生成的 `__placeholder` 风格**：单色块 + 系统字体 + 简单几何，仅供开发者看到「有东西」，并明确标注「未配置风格」。它**不能**出现在 `available` 里被用户选择。

### 3.5 加载失败的用户可见行为（不允许静默）

| 情况 | 行为 |
|---|---|
| `view.yaml` schema 校验失败 | **阻断**：进入「配置错误」界面，指出第几行第几个字段，不进入编辑器 |
| `default` 与 `fallback` 均不可用 | **进入占位风格 + 顶部红条告警**，允许用户进设置改风格；**不崩溃** |
| 某个 `available` 包损坏 | 从选择器**隐藏该包** + 日志记录；不影响其他包与当前风格 |
| 运行时切风格时新包加载失败 | **保持当前风格不变**，弹非模态错误，**绝不白屏** |

> 全部走 YAML = 换风格不需要改任何 `.ts`。壳层的「设置 → 地图风格」下拉就是读 `available`。

---

## 4. 运行时切换机制

### 4.1 可行性结论

**可行，且推荐 `runtime` 为默认模式**，但需满足三个前提：

1. **风格只作用于 L1 地图层** → 切换范围有界，不需要重建壳层、不需要重挂事件系统。
2. **地图是静态的**（假设 A2）→ 没有「时间相关的风格状态」需要保存，切换的失效面小。
3. **manifest 契约把资源寻址完全参数化** → 渲染代码不持有任何写死的资源引用。

### 4.2 推荐方案：**双缓冲 StyleTokens 热交换**（非重建式）

**核心思路**：渲染层对外是「消费 `StyleTokens` 的纯函数」，切换 = 换一个 tokens 对象 + 触发一次重绘，**不销毁、不重建渲染器实例**。

```
切换风格(packId) 流程：

 ① 从 ThemeRegistry 取 manifest → 校验 → 产出新 StyleTokens（未生效，处于缓冲）
       │  （若已缓存过该包，直接用缓存 tokens）
 ② 预热：预加载新包所有图片/字体到内存（Promise.all）
       │  失败 → 中止切换，保持旧风格（见 3.5）
 ③ 原子提交：renderer.setTokens(newTokens)
       │  - 旧 tokens 暂存为 prevTokens
       │  - 触发 renderer.redraw()
 ④ 提交后：
       │  - 释放 prevTokens 引用；旧包资源按 LRU 缓存策略保留（见 4.4）
       │  - 视口变换 / 选中态 / 时间游标「按引用复用」，不重置
 ⑤ dispatch CSS 变量（若地图区有 CSS 参与的边框/阴影）→ 一次性替换
```

**关键设计点**：

- **视口与选中态跨越切换保持**：因为它们存在**壳层**（`MapViewState`），不在渲染器内部。渲染器是**无状态**的（只读 tokens + 只读 state 快照 → 画）。所以「切风格不丢视口」是结构自带的，不需要额外保存/恢复逻辑。
- **资源热替换靠 URL 变化**：新 tokens 的 `atlas.*.url` 指向新包路径，浏览器/CSS 自动重新取图；旧图由缓存策略回收，**无需手动 unload 纹理**（除非用 WebGL，见 4.3）。
- **切换是原子的**：先全量预热成功再 commit，避免「切换过程中半新半旧」。

### 4.3 按后端区分的实现要点

| 后端 | 资源热替换做法 | 注意事项 |
|---|---|---|
| **SVG** | 直接换 `<use href>` / `<image href>` / CSS 变量 | 最易热替换；地图若走 SVG，用 CSS 变量承载 palette，切风格 = 换一组变量 |
| **Canvas2D** | 换 image 对象引用后 `redraw()`；`imageSmoothingEnabled` 等按 tokens 重设 | 需在 commit 时把 `render` 参数重新应用 |
| **WebGL2** | 需**显式**删除旧纹理 `gl.deleteTexture()` 再上传新纹理；图集换新须重建 texture atlas | 唯一有「必须显式回收」的后端；建议用**纹理池**管理，切换时按包维度整体释放 |

> **建议**：阶段一优先 **Canvas2D**（单人工具，维护成本最低，热替换最简）；若 JRPG 风格需要大量图块/滤镜再评估 WebGL2。**后端选择由 tokens 的 `compat.backend` 表达，渲染层按可用后端择一**，不改业务代码。

### 4.4 缓存失效与资源生命周期

```
ThemeRegistry 内部维护：
  loadedPacks: LRU<packId, { tokens, assets, lastUsed }>   // 内存上限：按图集字节数计
  策略：
   - 当前激活包：pin，永不淘汰
   - 最近切换过的包：LRU 保留（默认保留 3 个包）—— 单人切来切去很常见，避免反复加载
   - 超出上限：淘汰最久未用包的 assets（图片/字体的金丝雀引用置 null）
   - 字体：切换后**不卸载**（字体卸载会闪烁 + 影响已排版文本）；字体体积小，常驻
   - 图片：按包整体淘汰；Canvas2D 下让 GC 处理；WebGL2 下显式 deleteTexture
```

### 4.5 状态保持对照表

| 状态 | 归属层 | 切风格是否保持 | 说明 |
|---|---|---|---|
| 视口（x/y/zoom） | 壳层 | ✅ 保持 | 结构性保持，无需额外逻辑 |
| 选中实体 id | 壳层 | ✅ 保持 | 高亮会用新风格重绘，但选中不变 |
| 时间游标 | 壳层 | ✅ 保持 | 与风格无关 |
| 图层可见性 | 壳层（逻辑名） | ✅ 保持 | 逻辑名与风格解耦；若新包缺少某逻辑层 → 该层用兜底色/隐藏 + 告警 |
| 悬停 tooltip 内容 | 壳层 | ✅ 保持 | 文本内容与风格无关 |
| 地图内部滚动手势状态 | 渲染层 | ⚠️ 重置 | 原子的，用户感知不到 |
| 已烘焙的纹理图集 | 渲染层 | ❌ 重建 | 属资源，随包换 |

### 4.6 何时应选 `build` 模式

若某风格包**极大**（如几十 MB 图集）或**需要额外着色器编译**，运行期切换体验差。此时 `switch_mode: build`：Vite 构建时按 `default` 只打包一个风格，切换需重新构建。**单人项目默认 runtime**；`build` 作为逃生舱保留。

---

## 5. 扩展点与风险

### 5.1 理想目标：「只加资源 + 加一份 manifest，不改代码」

新增一个风格包 `ink_wash` 的标准动作：

```
1. mkdir themes/ink_wash
2. 按 §6 规格产出 atlas / fonts
3. 写 themes/ink_wash/theme.yaml
4. 在 view/view.yaml 的 available 里加一行 "ink_wash"
   └─ 若希望设为默认，改 default: ink_wash
```

**需要改的代码文件数：0。**
**需要改的配置行数：1（available 列表）。**

> 严格说 `available` 那行也是配置不是代码。若希望连这行都免，可让 `available` 缺省 = 「扫描到的所有通过校验的包」（见 5.3 优化项）。

### 5.2 这个目标会被什么破坏（风险清单）

| # | 破坏因素 | 具体表现 | 对策 |
|---|---|---|---|
| **R1** | **风格差异超出契约表达能力** | 新风格需要「契约里没声明的效果」（如动态天气、粒子、视差），只能改渲染代码 | 契约只覆盖**静态外观**。差异过大的风格应视为**新渲染后端/新插件**，走显式扩展点（5.4），**不硬塞进 manifest** |
| **R2** | **美术不按切图规格交** | 图块边长不统一 / 图集行列数对不上 / 缺 @2x | 靠 §2.3 的 V5/V6 **装载期拒绝**，把问题上抛到美术侧（第 6 节交接条件） |
| **R3** | **时间游标精度** | `cursorTime` 是 i64 天数（设计侧支持到 20 万年），JS number 安全整数上限约 9×10¹⁵，**20 万天花不完**，但若未来到天文尺度需注意 | 阶段一 `number` 够用（20 万年 ≈ 7.3×10⁷ 天，远小于 2⁵³）。若未来扩展，用 `bigint` 或字符串传输。**此项与风格无关，但会经 Bridge 传递，记录于此** |
| **R4** | **壳层误偷地图表现** | 壳层代码直接引用某风格的色值/图片，造成隐式耦合 | **契约红线**：壳层只允许用 `Design Tokens` 自己的值；code review 检查壳层不得 import `themes/*`。可加 lint 规则禁止 |
| **R5** | **逻辑层名与风格不一致** | 风格包没有某逻辑图层（如 `sea`），但工程数据里有海 | 渲染层对「tokens 里缺失的逻辑类型」走**兜底**（用 `palette.terrain_*` 纯色填充）+ 告警，**不报错**（保证鲁棒） |
| **R6** | **构建期把主题写死** | 用 `import` 直接引入某风格资源 → 打包进主包，切换失效 | 主题资源**一律走动态路径/动态 import**，禁止静态 import 到 `themes/`；Vite 侧用 `import.meta.glob` 扫描（见 5.3） |
| **R7** | **字体授权** | 分发的字体带 license 限制 | manifest 有 `license` 字段（可选）+ 装载期不强制；发布流程人工核对（见第 6 节） |

### 5.3 两个低成本优化项（可选，单人项目按需）

1. **`available` 自动扫描**：用 Vite 的 `import.meta.glob('themes/*/theme.yaml')` 在构建期建索引，`available` 缺省 = 「所有通过 schema 校验的包」。省掉手动维护列表。
2. **全局主题库**：`search_paths` 已预留全局路径。用户可在应用数据目录放共享主题，多个工程复用。

> 单人项目**不必现在做**这两项。第一项等风格超过 3 个时再做，第二项等有跨工程复用需求时再做。

### 5.4 显式扩展点（当 manifest 表达不了时）

若出现 R1 的情况，扩展点**只有一个**，且必须显式注册：

```ts
// 一个渲染插件 = 一组自定义绘制钩子 + 它读哪些 manifest 字段
interface RenderPlugin {
  id: string;
  manifestKeys: string[];                              // 本插件消费的 manifest 字段
  onSetup(tokens: StyleTokens): void;
  onDrawLayer(ctx: DrawContext, state: MapViewState): void;
  onTeardown(): void;
}
```

manifest 里用 `plugins: [id]` 声明启用。**新增插件属于「改代码」，必须走评审**——它是逃生舱，不是常规路径。**阶段一不实现，仅预留接口位置**。

---

## 6. 与美术侧的接口（交接条件）

> 以下规格是**美术侧必须提供**的东西；不满足则风格包**无法通过装载校验**（§2.3 V5/V6），会直接报错而非画破图。
> **标注「依赖美术规格确认」的条目**需要美术侧先给结论，我这边才能定死 schema 默认值。

### 6.1 命名约定（美术侧必须遵守）

| 项 | 规则 | 示例 |
|---|---|---|
| 风格包目录名 | `^[a-z][a-z0-9_]*$`，全小写 | `jrpg_classic` ✅ / `JRPG-Classic` ❌ |
| 图集文件名 | 全小写 + 下划线；倍率后缀 `@2x` | `terrain.png` / `terrain@2x.png` |
| 逻辑图块名 | 用小写英文；**必须**落在白名单内（land/sea/coast/forest/mountain/desert/tundra/river） | `mountain` ✅ / `山` ❌ |
| 字体文件 | `woff2` 优先；family 名用英文 | `map-label.woff2` |

### 6.2 尺寸与切图规则（**依赖美术规格确认**）

| 项 | 要求 | 待确认点 |
|---|---|---|
| 图块基准尺寸 | **候选：32×32（@1x）** | ⚠️ 「依赖美术规格确认」：JRPG 常用 16/32/48。**请美术侧拍板基准 tile 尺寸**，我据此定 manifest 默认与校验 |
| 图集行列 | 规则网格、无间隙、无 padding，或统一 padding | ⚠️ 「依赖美术规格确认」：是否需要 tile 间 padding（防 bleeding）。若需要，manifest 增加 `padding: int` 字段 |
| @2x 规则 | @2x = @1x 的整数倍（2×），行列数一致，仅单块边长翻倍 | 确认是否提供 @2x |
| 锚点 | 图块原点统一为**左上角** | 无需确认，直接钉 |
| 透明 | 图集背景必须透明（PNG-24 + alpha） | 直接钉 |

### 6.3 色板交付（**依赖美术规格确认**）

- 美术侧提供 **palette 键值表**（对应 §2.2 白名单的每个键），值为 HEX。
- ⚠️ 「依赖美术规格确认」：**是否所有白名单键都必须给值**？我的建议：`bg` / `node_*` / `edge_*` / `label*` **必须**；`terrain_*` 可给兜底色。请美术侧确认能覆盖。
- 色板以 `palette.yaml` 或内联 manifest 形式交付，**不写成图片**（图片取色不可校验）。

### 6.4 图标槽位（**依赖美术规格确认**）

§2.2 的 `icons` 声明了 `anchor / story / event / warned` 四个槽位（对应设计侧 §1.12 的便签图标 📜、事件卡、告警态）。
- ⚠️ 「依赖美术规格确认」：**这四个槽位是否够用**？是否需要选中态、hover 态、禁用态各一张？
- 若需要多态，manifest 的 `tiles` 键扩展为 `event_selected` 等，请美术侧给全。

### 6.5 交付清单（美术侧 → 技术侧的交接包）

```
themes/<pack_id>/
├── theme.yaml            ← 由技术侧模板 + 美术侧填色板/尺寸
├── atlas/
│   ├── terrain.png  (+ @2x)   ← 按 §6.2 规格
│   ├── entities.png (+ @2x)
│   └── icons.png
├── fonts/
│   └── map-label.woff2 (+ bold)
├── preview.png           ← 240×135 风格缩略图（可选但强烈建议）
└── LICENSE-fonts.txt     ← 字体授权说明（可选但发布必需）
```

**交接验收标准**：`theme.yaml` 能被 schema 校验通过 + V5（资源存在）+ V6（尺寸自洽）全过，即为合格交接。

---

## 7. 「现在就要做」vs「到那一步再说」

| 项 | 阶段 | 理由 |
|---|---|---|
| L1/L2/L3 三层拆分 + Bridge 契约 | **现在** | 结构地基，后补成本极高 |
| `theme.yaml` schema v1 + 校验器（V1–V9） | **现在** | 契约是「不改代码」目标的前提 |
| `view.yaml` + 加载流程 | **现在** | 配置驱动是项目惯例 |
| 1 个 JRPG 参考风格包（jrpg_classic） | **现在** | 至少一个真实包来验证契约（**依赖美术侧尺寸确认**） |
| 运行时切换（双缓冲热交换） | **现在** | 用户明确要「留出可替换接口」，且切换机制决定渲染层写法，晚做要返工 |
| Canvas2D 后端 | **现在** | 单人工具，最低维护成本 |
| WebGL2 后端 | **到那一步再说** | 图块/滤镜量确实吃紧时再上 |
| `available` 自动扫描 | **到那一步再说** | 风格 >3 个时再做 |
| 全局主题库 | **到那一步再说** | 有跨工程复用需求时再做 |
| RenderPlugin 扩展点 | **现在只留接口位，不实现** | 避免过度工程，但留好逃生舱位置 |
| 多平台分发（Steam/主机/移动） | **不在本方案范围** | 本子项目是工具前端，非游戏本体；打包话题另立方案 |

---

## 8. 待确认 / 依赖项汇总

| # | 事项 | 依赖方 | 影响 |
|---|---|---|---|
| D1 | 基准 tile 尺寸（16/32/48） | **美术侧** | manifest 默认值 + V6 校验阈值 |
| D2 | 图块是否需要 padding | **美术侧** | manifest 是否加 `padding` 字段 |
| D3 | 是否提供 @2x | **美术侧** | `compat.dpr` 取值 + 资源清单 |
| D4 | icons 是否需多态 | **美术侧** | `icons.tiles` 键集 |
| D5 | palette 白名单是否全覆盖 | **美术侧** | 校验严格度 |
| D6 | 渲染后端最终选型（Canvas2D 优先？） | 技术侧（我）/ 待原型验证 | 热替换实现细节 |
| D7 | i64 时间游标是否需 bigint 传输 | 技术侧（我）/ 待与 design 侧确认最大跨度 | Bridge 契约字段类型 |

---

## 9. 风险与验证方法（先最小验证，再铺开）

| 风险 | 最小验证方式 | 通过标准 |
|---|---|---|
| R1 契约表达力不足 | 用 1 个 JRPG 包 + 1 个截然不同风格（如水墨 ink_wash）**各做一版** | 若水墨无法用同一 manifest 表达 → 提前暴露，决定是扩 schema 还是走插件 |
| R6 构建期写死 | 打包后检查主 bundle 是否含 `themes/` 资源 | 主 bundle 不含主题图片 = 通过 |
| 运行时切换白屏/闪烁 | 手写测试：切风格 20 次，观察 | 无白屏、无残留旧图、视口/选中不丢 |
| R4 壳层偷表现 | lint 规则：`src/shell/**` 禁止 import `themes/**` | lint 报错即拦截 |
| V6 尺寸校验有效性 | 故意交一张尺寸不符的图 | 装载期报错，不进编辑器 |

---

## 附录 A：最小可用 `view.yaml` 与 `theme.yaml` 样例

**view/view.yaml**
```yaml
schema: view-config
schema_version: 1
style:
  default: jrpg_classic
  available: [jrpg_classic, ink_wash]
  switch_mode: runtime
  fallback: jrpg_classic
  search_paths: ["themes/"]
```

**themes/jrpg_classic/theme.yaml（骨架）**
```yaml
schema: theme-pack
schema_version: 1
id: jrpg_classic
display_name: "JRPG 经典"
version: "1.0.0"
compat:
  min_viewer: "0.1.0"
  backend: [canvas2d]
  dpr: [1]
palette:
  bg: "#1a1614"
  node_default: "#e8dcc0"
  node_selected: "#f2c14e"
  node_disabled: "#6a6155"
  edge_default: "#8a7f6a"
  edge_highlight: "#f2c14e"
  label: "#efe6d2"
  label_muted: "#a89a80"
atlas:
  terrain:
    chunks: "atlas/terrain.png"
    tile: 32            # ← 待 D1 确认
    columns: 8
    rows: 6
    tiles: { land: 0, sea: 1, coast: 2, forest: 3, mountain: 4, desert: 5 }
  entities:
    chunks: "atlas/entities.png"
    tile: 32
    columns: 4
    rows: 2
    tiles: { city: 0, village: 1, fort: 2, port: 3 }
  icons:
    chunks: "atlas/icons.png"
    tile: 16
    columns: 4
    rows: 1
    tiles: { anchor: 0, story: 1, event: 2, warned: 3 }
fonts:
  map_label:
    regular: "fonts/map-label.woff2"
    family_name: "MapLabel"
render:
  image_smoothing: nearest
  label_min_zoom: 0.75
```

---

*本文档引擎中立，聚焦 Web/Tauri 技术栈下的资源抽象与配置驱动。所有「依赖美术规格确认」项以 D1–D5 汇总。前提假设变更（尤其 A2 地图静态、A3 风格≠语义）时，第 4 节缓存策略与第 2 节契约需重估。*
