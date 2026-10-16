# ADR-0023：主题包（Theme Pack）注入契约 —— 地图渲染可复用与游戏风格注入的统一接入点

**状态**：提议（Proposed，待用户拍板）
**日期**：2026-10-16
**决策者**：待定（源自 2026-10-16「地图各种表达方式调研」+ 「wv-render 复用到游戏」讨论）
**关联**：P3（YAML 为主格式）/ P5（图层化编辑）、AGENTS §2（分层）/ §3（数据与渲染分离）/ §6.4、ADR-0012（VectorFlat 浏览态默认底图）/ 0013（LayerFrame kind 轴单点）/ 0016（古舆图默认主题 + MapPalette 调色板轴）/ 0020（wv-render 独立 crate，§2.8 留项）/ **0021（RenderRecipe + Treatment，D2 的 TreatmentFlags 是本 ADR 配色包的一个字段）**、`docs/STYLE_SWAPPABLE_ARCHITECTURE.md`（2026 Q1 设计稿，契约骨架来源）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

### 1.1 复用动机（用户原话，2026-10-16）

> "我是想把 wv render 复用到游戏的，但是游戏风格会有所不同。所以我希望有个地方可以注入不同风格的地图。"

> "我认为这些不同风格地图应该都是一个 crate，如 chinese_ink_theme, jrpg_theme, cybepunk。还应该包括覆盖的贴图。"

两条动机：
1. **跨工程复用** —— `wv-render` 不仅是本编辑器的渲染层，**也应能被游戏直接接入**渲染同一份世界数据（P1：单机；同一份 `wv-core` 世界）。
2. **风格可注入** —— 加一个新游戏风格 = 加一个 crate（**不**改 `wv-render` 源码），含色板 + 装饰旗标 + 贴图。

### 1.2 当前 wv-render 的 4 个差距

| # | 差距 | 现状证据 | 后果 |
|---|---|---|---|
| 1 | **`MapPalette` 是封闭枚举** | `crates/wv-render/src/theme/palette.rs:33` `enum MapPalette { Daylight, Dusk }` | chinese_ink / cyberpunk 加进来必须**改 wv-render 源码**——游戏方动不了你的 crate，注入点不存在 |
| 2 | **色板是编译期静态分发** | `palette.rs:176` `for_palette() -> &'static PaletteColors` | 与第 1 条联动：`ViewMode` 7 个 + `MapPalette` 2 个 = 14 个 if 分支散在 `drill.rs` / `streets.rs` / `vector_layers.rs` 等 8 个模块 20+ 处（`grep "view.map_palette"` 实证）|
| 3 | **贴图钩子已存在但被锁死在 wv-render 内部** | `crates/wv-render/src/terrain_view/tile_atlas.rs:68` `atlas_base_dir()` 硬编码 `env!("CARGO_MANIFEST_DIR").join("assets")` | 主题 crate 的贴图进不来。注意 `TileAtlas` 是 `pub Resource` 且 `tile_atlas.rs:13` 注释「JRPG 图集已弃用，`load_tile_atlas` 保留供未来接入其他图集包」——**这个钩子就是给贴图包预留的**，只是没接到主题体系上 |
| 4 | **编辑器专属系统混在 wv-render 里** | `brush_cursor` / `measurement` / `editing_bridge` / `grid_lines` 都是编辑器专属 | 游戏接入时无法只挑"地图本体"系统——必须拖全部，或手工逐系统挑着注册（`crates/wv-app/src/app_bootstrap.rs:149-350` 一条长链） |

### 1.3 已有的契约骨架

`docs/STYLE_SWAPPABLE_ARCHITECTURE.md`（2026 Q1，674 行）已把可替换架构的**目标形态**画完：分层（L1 地图 / L2 桥 / L3 壳）、Theme Pack 契约（manifest schema / 校验规则 V1–V9 / 装载流程 / 运行时热切换双缓冲）。**但它面向 Tauri v2 + TypeScript 前端**，**未对接到当前 Rust + Bevy 现状**：
- 它假设从 JS 侧 `import` 主题，**未考虑 Rust 编译期静态分发**；
- 它假设 `compat.backend ∈ {svg, canvas2d, webgl2}`，**未考虑 Bevy 的 PBR 渲染**；
- 它的"加风格 = 加目录 + 一行 yaml"是 Web 资源加载模型，**移植到 Rust 需用 cargo feature 或独立 crate**（取舍见 §5）。

**本 ADR 把 STYLE 的契约骨架移植到 Rust 现状**，保留其"加风格不动 wv-render 源码"的核心承诺，对 Rust 侧不成立的假设（资源热加载粒度 / 后端选型）显式替换。

### 1.4 与 ADR-0021 的协同点（重要不变量）

ADR-0021 提议的 **`TreatmentFlags`**（D2）是表达三轴里的**装饰族**。本 ADR 把 `TreatmentFlags` **作为 `ThemePack` 的字段之一**而非独立轴分发——理由：装饰旗标与配色同属"静态外观定义"，运行时切换的失效面一致（同帧生效、同帧失效），分开发只会增加注册点。**这条不变量由 ADR-0021 §2 D2 §4「特性归轴」表 + 本 ADR §2 D4 共同维持**。

### 1.5 边界声明（不在本 ADR）

- **YAML 化主题加载**（README §12-30 延期项）：Style doc §2.3 的 V1–V9 校验规则本 ADR 推迟到 M3，**不阻塞 M1/M2**——M1/M2 主题仍是 Rust const 实例（即用户提出的"每风格一个 crate"形态 A），schema 形态 C 是更远的事。
- **`ViewMode` 拆分 / 缩并**：本 ADR 不动 ViewMode 7 个成员；GoogleMap 仍保留为可切换技法（与 `colors_google` 的去留是 ADR-0016 的产品决策，已独立讨论）。
- **跨进程主题分发**（游戏作为独立进程加载 wv-render）：本 ADR 只定契约，跨进程打包链另开 ADR（与 `wv-overture` 平行叶子 crate 同档工作量）。

---

## 2. 决策（Decision）

四项决定（D1–D4）**可独立接受或否决**，但 D1 是其他三条的语法前提。

### D1 主题包类型 `ThemePack`（契约的根类型）

1. 在 `wv-render` 内新增 `pub struct ThemePack`，聚合一个完整视觉风格所需的全部静态外观数据：

   ```rust
   // crates/wv-render/src/theme/theme_pack.rs (新文件)
   pub struct ThemePack {
       pub id: &'static str,                              // "antique_default" / "chinese_ink" / ...
       pub display_name: &'static str,                    // "日间古舆图" / "水墨" / ...
       pub palette: PaletteColors,                        // 现有 36 槽位（色板轴）
       pub treatment: TreatmentFlags,                     // 装饰旗标（= ADR-0021 D2 载荷）
       pub atlas: Option<TileAtlasSpec>,                  // 贴图（None = 纯色平涂，当前行为）
   }
   ```

2. `TileAtlasSpec` 复用现有 `TileAtlas`（`tile_atlas.rs:42`）——已包含 `tiles: [Option<[u32; 2]>; 10]`，**与 `TerrainKind` 编号 0..10 严格对齐**（古舆图 spec 决定了）。**不**新建第二套贴图抽象。
3. `PaletteColors` 与 `TreatmentFlags` 全部字段 `Copy` + `Serialize`（前者已 Copy；后者在 ADR-0021 M2 落地时声明 `#[derive(Serialize)]`），**为 M3 YAML 化留接口位、M1/M2 不实现**。
4. 渲染系统消费 `ActiveThemePack(pub &'static ThemePack)` Resource（**新增**），**替换**当前 `MapPalette` 在 `ViewState` 里的位置——但**保留** `ViewState.map_palette: MapPalette` 字段作"持久化键"（详见 D3）。

### D2 开放注册点（核心注入接口）

1. **不**把 `ThemePack` 集合放进 `wv-render` 内（避免"加风格改 wv-render 源码"反模式）。改用 **cargo feature 闭包 + 静态切片**：
   - `wv-render` 提供 `pub trait ThemePackRegistry { fn packs() -> &'static [(&'static str, &'static ThemePack)]; }`（**新**文件 `theme/registry.rs`）；
   - `wv-render` 默认实现 `BuiltinPacks` 给出"日间古舆图"一个 const 实例（即现有 `colors.rs` 的封装），保证 **零回归**；
   - 主题 crate（`wv-theme-chinese-ink` 等）依赖 `wv-render`，实现 `ThemePackRegistry` 并 `pub const PACK: ThemePack = ...`；
   - 游戏 app `depend wv-render + 自己想要的主题 crate`，把多个 registry 合并成 `AppThemePacks`（同 trait 多 impl，**无需 dyn 调度**：`&'static ThemePack` 直接访问）。
2. **不**用 `Box<dyn ThemePack>`（vtable 开销 + 类型擦除 + 难以单测）：全部 `&'static` 切片，靠**编译期单态化**取得性能。
3. wv-render 自身**不知道**任何具体主题 crate 存在——符合 AGENTS §2.1 依赖方向红线（用户对所有依赖项跑 `cargo tree -p wv-render | grep -E 'wv-theme'` 应**输出空**）。

### D3 持久化与运行时切换

1. `ui-layout.yaml` 的 `map_theme: Option<String>` 字段**保留**（不动 schema），但语义从 "MapPalette key" 升格为 "ThemePack id"：
   - 内置主题的 id 与 `MapPalette` key 故意取同名（`daylight` / `dusk`），保证存量工程的 YAML 不用改；
   - **未知 id 不报错**（V1 纪律），回退到 `BuiltinPacks::DEFAULT` + 在状态栏一次性提示「主题 `xxx` 未装载，已用默认」——AGENTS §7.5 V1 推广到主题切换。
2. 切换流程（M1）：
   - 用户在 UI 选主题 → 写 `view.map_palette_key: String`（新字段，**覆盖** `view.map_palette: MapPalette`）→ 落 `ui-layout.yaml`；
   - 启动时由 `ui_shell::register` 读 YAML → 解析为 `&'static ThemePack` → 塞 `ActiveThemePack` Resource；
   - 切换时**不重建系统**，只换 Resource + 触发一次 `terrain_view::rebuild_texture_on_mode_change` 同口径的脏标（M1 沿用现有路径；Recipe 落地的 M2 再做精细化）。
3. 顶栏 "日间古舆图 / 暮色旧纸" 切换按钮**保留**作为内置主题的快速入口（不暴露所有 ThemePack，符合"壳层只装自己点过名的"原则）。
4. **作用域口径（2026-10-16 用户澄清，双词分立）**：
   - **运行时跟着软件**——主题是软件级全局观感：单一 `ActiveThemePack` Resource，切换 = 整软件一个开关，所有地图 / 所有要素同帧换肤；**不是**地图属性、**不**进 `wv-core` 数据层、**不**按元素混搭（见 §4 负 3）；
   - **持久化跟着工程**——`map_theme` 记在工程根的 `ui-layout.yaml`，与 `theme` / `density` / `language` 同一纪律（`ui_layout.rs` "只写 UI 偏好"）：打开 A 工程恢复 A 上次选的主题，打开 B 工程恢复 B 的。这是**作者偏好随工程走**，不是"主题绑定地图数据"。

### D4 与 ADR-0021 协同

1. **接受** ADR-0021 D2 的 `Treatment { HandDrawn, Skyline }`，但**重新定位**：不再通过 `view.treatment` 单独持有，而是**作为 `ThemePack` 字段**承载（`pub treatment: TreatmentFlags`）。理由：
   - 风格包是"色板 + 装饰 + 贴图"的原子组合，分开发会让"Daylight × Skyline"和"古舆图 × HandDrawn"配出 N×M 组合反而复杂；
   - 用户原诉求是"加一个水墨 crate 进去就能用"——原子包最自然。
2. **内置古舆图 crate**（无论留在 `wv-render` 还是拆出为 `wv-theme-antique`）必须把 `treatment = HandDrawn` 作为唯一 `const` 装载——**零回归是接受本决策的硬前提**。
3. ADR-0021 的 Recipe 骨架（D1 收口 20+ 散点）**继续独立**——它是"判定写在哪"的内务，与本 ADR 正交不冲突。**先后顺序**：本 ADR 落地后 ADR-0021 更容易做（`map_palette` 字段消失，20+ 散点里有一半自然消除）。

### D5 Plugin 拆分（与 1.2 差距 4 对应）

1. `wv-render` 内部拆出三个 `Plugin` trait（`bevy::Plugin` 即可，不引入第三方）：
   - `MapRenderPlugin` —— 地形 / 矢量层 / 标注 / 势力多边形 / 钻取（**游戏可用**）；
   - `EditorOverlayPlugin` —— 笔刷光标 / 测量 / 编辑桥 / 编辑态网格（**编辑器专属**）；
   - `MapShellPlugin` —— 纸面氛围 / 等值线叠加 / 雾霭 / 海冰 / 雪调（受 `TreatmentFlags` 控制；游戏想开可装，不想开可不装）。
2. `wv-app::app_bootstrap` 三个都装；游戏 app **只装前两个**（按需加第三个）。
3. **当前 `app_bootstrap.rs:149-350` 的长系统链不立即重写**——M1 沿用裸 `add_systems`，M2 验收后再合并到 Plugin 内。**回退粒度**：M1/M2 独立可 revert。

---

## 3. 理由（Rationale）

- **为什么 ThemePack 是聚合而非分散 trait**：三件（色板 + 装饰 + 贴图）共享同一生命周期（切换 = 整组失效）、同一持久化键（一个 id）、同一注册点（一个 const）。**分三个 trait 注册 = 多三个出错点**，且让"开箱即用"变三步。
- **为什么用 cargo feature / 多 crate 而非"加目录 = 加风格"**：Rust 没有 JS `import.meta.glob`。**多 crate 是 Rust 语境下"加资源不动核心"的最自然映射**（与 `wv-overture` / `wv-terrain` 平行叶子 crate 同模式）。代价是每个风格要写 `Cargo.toml` + `lib.rs` + 一次 `pub const PACK`——约 30 行 boilerplate，**单人可接受**（与 STYLE §5.3 优化项「风格 >3 个时再做」不矛盾）。
- **为什么不引入 `Box<dyn ThemePack>` 动态分发**：渲染层热路径（每帧 60 次）消费主题色值，vtable 跳转会回退 Bevy 0.19 ECS 调度优化；**`&'static` 单态化在编译期确定**，与现有 `palette::colors(MapPalette) -> &'static PaletteColors` 同口径。
- **为什么 `MapPalette` 字段保留**：`ui-layout.yaml` 的 schema 兼容是 1.3 提到的"存量工程不动"硬约束，**字段换名 = 强制迁移**。保留字段名 + 升格语义 = 兼容 + 演进同时达成。
- **为什么 Plugin 拆分等 M1/M2 验收后**：先把契约立住并验证"内置古舆图经 ThemePack 渲染与现状逐位一致"，再做系统重组——**测试金字塔从底往上盖**，避免在契约还没稳定时做 350 行系统链大改。
- **为什么 Treatment 并入 ThemePack 不另开轴**：ADR-0021 §5 否决了 C（并入 ViewMode），但**没说不能并入 ThemePack**。本 ADR 提议的是 D2 的修订（与 D1 独立可接受/否决），修订理由见 D4 §1。

---

## 4. 代价（Consequences）

- **正**：
  1. 加新游戏风格 = `cargo new wv-theme-xxx` + 实现 `ThemePack` + 接入 = **零 wv-render 源码改动**；
  2. 主题切换走 `&'static` 切片 + 单 Resource，运行时零分配（与现状同性能档）；
  3. 复用 wv-render 到游戏 = `depend wv-render + wv-theme-xxx` 即可，Plugin 拆分后只装"地图本体"；
  4. `TreatmentFlags` 归入 ThemePack 减少 ADR-0021 D2 的注册点（1 个而非 2 个）；
  5. `MapPalette` 字段保留 → `ui-layout.yaml` 旧工程**零迁移**。
- **负**：
  1. **每风格一个 crate ≈ 30 行 boilerplate**（`Cargo.toml` / `lib.rs` / `pub const PACK`），单人可接受但**不像 Web 那样零代码**；
  2. **`MapPalette` 字段名与实际语义错位**（字段叫 palette，承载的是 ThemePack id）——新增团队成员可能困惑；靠文档 + `view.map_palette_key` 字段名（M2 落地）消除；
  3. **主题是软件级全局观感，不按元素混搭**（2026-10-16 用户澄清后显式化）：运行时 = 单一 `ActiveThemePack` Resource，切换 = 整软件一个开关（所有地图 / 所有要素同帧换肤）；**不存在**也不预留"一张图同时用古舆图底 + 水墨 POI"的 per-element 覆盖——那是假想需求，真要做属于新 ADR 的 scope（pass / shader / draw 链全要按元素 tag），本契约**不为它预留逃生舱**；
  4. `AppThemePacks`（多 registry 合并）需要**合并顺序**约束（"后注册覆盖先注册"）——文档化、不强制；
  5. 与 `STYLE_SWAPPABLE_ARCHITECTURE.md`（面向 Web）的契约**部分**会偏离（资源热加载粒度、JS 字体卸载、gl.deleteTexture）——**显式记录在 §5 被否决的备选**。
- **待裁（落地前收敛）**：
  | 子点 | 选项 |
  |---|---|
  | 内置古舆图主题是否拆出为 `wv-theme-antique` | A. 留在 wv-render（推荐，零依赖增量）/ B. 拆出（与未来游戏主题同档） |
  | 内置主题的 crate 命名 | A. `wv-theme-antique`（与游戏主题命名一致）/ B. `wv-theme-jrpg`（沿用 ADR-0016 名）/ C. 留在 wv-render 不命名 |
  | ThemePack 的持久化键字段名 | A. 沿用 `map_theme`（推荐，零迁移）/ B. 改为 `theme_pack_id`（语义更准）|
  | 多个 registry 合并时的优先级 | A. 后注册覆盖（推荐，简单）/ B. 显式 `priority: u8` 字段 |
  | `MapPalette` 字段何时彻底移除 | A. 等 1-2 个版本观察 → 移除（推荐，AGENTS §12 最易回退）/ B. 永远保留作为 fallback key |
  | 风格热切换 vs 重启 | A. 当前 A1 = 运行时（推荐，与 STYLE §4.2 一致）/ B. 切重启（更稳但 UX 差） |
  | `Skyline` 在内置古舆图 crate 里是否提供 | A. 不提供（v1 只一个 HandDrawn；想做 Skyline 走新 crate `wv-theme-jrpg-clean`，推荐）/ B. 提供（同 crate 多 const） |

---

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **A. 动态加载（`include_str!` + `unsafe` 反射）** | Rust 编译期不允许多 crate 动态注入；尝试用 `inventory` crate 跨 crate 注册会污染 build script、与 Bevy 0.19 ECS 装配不友好。 |
| **B. `Box<dyn ThemePack>` 动态分发** | 渲染热路径 vtable 跳转；类型擦除让单测难做；与既有 `&'static PaletteColors` 设计语言冲突。 |
| **C. 把 `STYLE_SWAPPABLE_ARCHITECTURE.md` 当 Rust ADR 直接照搬** | 文档面向 Tauri v2 + TS，§2.3 V4 `compat.backend` 等字段在 Bevy 0.19 语境下无意义；其"加目录 = 加风格"模型在 Rust 没有等价物。**承认 STYLE 是契约骨架的来源，不当实现目标**。 |
| **D. 把 ADR-0021 D2 的 Treatment 留作独立轴** | 风格切换是原子的，分两个轴会出现"Daylight × Skyline"这类组合 + 2 个注册点；与本 ADR D4 §1 论证同源。 |
| **E. 不动 `MapPalette` 枚举，新风格用魔法字符串逃逸** | 双轨分发 = 测试必须两个路径都覆盖 = 漏改概率上升；与"加风格零改 wv-render 源码"目标正相反。 |
| **F. ThemePack 字段全部 Option + 默认值逃生舱** | "Optional 一切" = "什么都没有"。渲染层每个消费者都得 `unwrap_or`，等于把强类型契约弱化回 JS 风格。**违反 AGENTS §8「类型即文档」**。 |
| **G. 拒绝做 Plugin 拆分** | 复用游戏 = 拖着编辑器专属系统；用户要的是"只装想要的"。 |
| **H. 立即把 Recipe + Treatment + ThemePack 一次性做** | 三个决策互相影响但正交，单 ADR 内做三件事 = 风险失控。**三期各自可 revert**（参见 ADR-0021 M1/M2 拆分逻辑）。 |
| **I. ThemePack 用单一 trait + 多 impl 注册** | Rust 多 impl 同一 trait 没问题，但需要在 wv-render 暴露 trait——本 ADR D2 §1 改用更轻的"静态切片 + 实现 trait 即可"模型，**无需暴露 trait 给 wv-render 消费者**。 |

---

## 6. 分期与落地闸门（若接受）

### 6.1 M1（契约立柱 + 零回归验收）

**目标**：`ThemePack` 类型落地；内置古舆图经 `ThemePack` 渲染与现状**逐位一致**（像素 hash）。

任务：
1. `wv-render/src/theme/theme_pack.rs` 新增 `ThemePack` / `ActiveThemePack` / `BuiltinPacks`；
2. `view.map_palette` 字段保留；新增私有 `view.theme_pack_id: Option<String>` 字段（M1 写、`map_palette` 仍为权威 source）；
3. 渲染层消费 `ActiveThemePack`，`palette::colors(MapPalette)` 调用点逐步替换（**M1 不全替**——让存量调用走 `BuiltinPacks::DEFAULT` 兜底，**保证零回归**）；
4. 顶栏 / 命令面板切换主题入口**M1 不动**（仍是 `MapPalette::toggled`），M2 再切到 ThemePack id 列表；
5. 验收：`cargo test -p wv-render --features app` + `bash scripts/smoke-gui.sh` + 截图 hash 与 `main` 一致。

### 6.2 M2（注入 + 持久化 + Plugin 拆分）

**目标**：新主题 crate 可被游戏 app 接入；持久化键从 `MapPalette` 升格为 `ThemePack` id；Plugin 拆分落地。

任务：
1. `theme/registry.rs` 新增 `ThemePackRegistry` trait；
2. `crates/wv-theme-chinese-ink/` 新建示例 crate（占位 `pub const PACK: ThemePack` + `theme.yaml` 注释），**作为契约的第一个真实消费者**——但**色值留空 / 标 `// TODO: 美术提供`**，避免提交未校准的色值进 Git；
3. `ui-layout.yaml::map_theme` 升格为 `theme_pack_id` 语义（**字段名沿用 `map_theme`**——见 §4 待裁 A）；
4. `app_bootstrap.rs` 三个 Plugin 注册；游戏 app 接入示例（**不**实装游戏 crate，写一份 `examples/game_consumer.rs` 验证 Plugin 拆分正确）；
5. 验收：内置古舆图主题在 `theme.yaml` 形式下渲染与 M1 像素一致；`wv-theme-chinese-ink` 装载后报"色值未校准"告警 + 兜底到内置默认。

### 6.3 M3（YAML 化 + 校验 + 热切换）—— 推迟

与 README §12-30、STYLE §2.3 V1–V9 对齐。**M3 启动前提**：M1 + M2 已在 `main` 跑 1-2 个版本未发现契约缺陷；M3 启动时单开 ADR 追记。**本 ADR 显式推迟 M3**（AGENTS §12 最易回退）。

### 6.4 闸门（与 AGENTS §7.1 一致）

- `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`；
- `cargo test -p wv-render --features app` + `cargo clippy -p wv-render --all-targets --features app -- -D warnings`；
- `bash scripts/smoke-gui.sh`；
- 渲染层回归：`examples/layer-showcase` 启动 5s + 截图 hash（M1 与 `main` 一致；M2 提示手工验收色差）。

---

## 7. 闸门（与 ADR-0021 §6 同步）

1. M1：内置古舆图经 `ThemePack` 渲染与 `main` 像素 hash 一致；
2. M2：`wv-theme-chinese-ink` 示例 crate 装载流程跑通，告警/兜底/无 panic；
3. 三阶段各自独立 commit、独立可 revert（与 ADR-0021 M1/M2 拆分纪律同档）。

---

**相关文档**：
- `docs/STYLE_SWAPPABLE_ARCHITECTURE.md`（契约骨架的来源；本 ADR 显式声明偏离项见 §5 C）
- `docs/adr/ADR-0016-jrpg-antique-map-default-theme.md`（MapPalette 调色板轴的现行约定，本 ADR 在其上层叠加 ThemePack 抽象）
- `docs/adr/ADR-0020-wv-render-crate.md`（wv-render 独立 crate，本 ADR 是其 §2.8「Recipe/Treatment 留项」的接续——但走 ThemePack 路线而非 Recipe 单点）
- `docs/adr/ADR-0021-render-recipe-and-treatment.md`（D1 Recipe 仍独立；D2 Treatment 归入本 ADR D4）
- `docs/adr/ADR-0022-city-submap-spatial-form.md`（同批次兄弟 ADR，独立可接受）
- `README.md §12-30`（palette.yaml Theme Pack 加载机制延期项 —— **本 ADR 闭环 M1+M2 后即可在 M3 推进**）
