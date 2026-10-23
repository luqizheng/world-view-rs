# AGENTS.md

> 本文件是 **World View 项目的协作规约**，面向所有参与本仓库工作的 AI 助手与人类协作者。
> 它不是建议，是约束。**违反这里任一条的改动不应被合入。**

---

## 0. 阅读顺序

任何人在改动代码前，按此顺序建立上下文：

1. `README.md` —— 项目定位、五维度、七原则、架构分层
2. 本文件 —— 架构纪律、模块边界、术语与留痕规则
3. 目标 crate 的 `src/lib.rs` 顶部文档注释 —— 该层的契约

> 若本文件与代码注释冲突，**以本文件为准**，并应提 issue 修正注释。

---

## 1. 最高约束：七条设计原则

`README.md` §3 的七条原则**不可动摇**。任何提案在评审时先过这一关：

| 编号 | 原则 | 违反时的典型表现 |
|---|---|---|
| P1 | 单机可运行 | 引入服务端接口、账号系统、云端同步 |
| P2 | 目录即工程 | 把状态写进注册表 / AppData / 全局配置 |
| P3 | YAML 为主格式 | 把矢量数据内联成二进制、把配置藏进代码常量 |
| P4 | 多地图层级 | 子图内容被复制进父图，而非 ID 引用 |
| P5 | 图层化编辑 | 图层开关被写成全局状态，无法逐层控制 |
| P6 | 非破坏编辑 | 直接覆盖历史值，而非追加区间（**适用边界见 §7.5**，勿据此禁止作者改历史） |
| P7 | 时间轴驱动 | 存「当前值」而非「带区间的状态片段」 |

**评审动作：** 每个 PR 描述中必须显式声明「本改动触及/未触及哪条原则」。触及的，需说明如何满足。

> ⚠️ **P6 的适用范围**：P6 约束的是**存储层实现**（区间不覆盖、可回溯），不是**作者操作权限**。
> 作者**可以**修改已发生的事件（参与者 / 时间 / 地点）——见 §7.5 V2「历史可重写」。

---

## 2. 架构纪律：分层与依赖方向

依赖只能**向下**，禁止反向或跨层：

```
wv-app  →  wv-edit  ───┐
   │                    │
   ├────→ wv-render ────┤
   │                    ├─→  wv-core
   ├────→ wv-ui ────────┤            （wv-core 是唯一根）
   └────→ wv-io ────────┘
                                wv-terrain（与 wv-core 平行，纯数据层叶子）
                                wv-overture（与 wv-core 平行，CLI 数据导入辅助）
                                wv-ui（与 wv-render 平行，egui 控件 + 界面令牌叶子，ADR-0026）
                                wv-theme（与 wv-render 平行，色板 + 主题包契约叶子，ADR-0043）
                                wv-world-cell-map（wv-render 下游叶子：世界级地形工作台，wv-render 单向依赖之，ADR-0054）
```

> `wv-edit` 与 `wv-render` 是 **双子 crate**——前者写、后者读，**互不依赖**，由 `wv-app` 同时装配。

### 2.1 硬性约束

| Crate | 允许依赖 | **禁止** |
|---|---|---|
| `wv-core` | `serde`, `thiserror` | ❌ Bevy / egui / 任何渲染库<br>❌ 任何文件系统访问（`std::fs`）<br>❌ 任何网络库 |
| `wv-io` | `wv-core`, `serde`, `serde_yaml` | ❌ Bevy / 渲染库<br>❌ 反向依赖 `wv-edit` / `wv-render` / `wv-app` |
| `wv-edit` | `wv-core`, `wv-terrain` | ❌ 直接读写文件（那是 `wv-io` 的职责）<br>❌ 渲染库<br>❌ 反向依赖 `wv-render` |
| `wv-render` | `wv-core`, `wv-terrain`, `bevy`, `bevy_egui`, `egui_extras`, `wv-theme`, `wv-world-cell-map`（均 `optional = true`，随 `app` feature） | ❌ 直接读写文件<br>❌ 反向依赖 `wv-edit` / `wv-app`<br>❌ 直接持有 `wv-edit::EditHistory`（intent→dispatcher 翻译必走 wv-app）<br>❌ 直接构造 `wv_edit::EditCommand`（同上）|
| `wv-ui` | `bevy`, `bevy_egui`, `egui_extras`（均**非 optional**，ADR-0026） | ❌ `wv-core` / `wv-terrain` / `wv-io` / `wv-edit` / `wv-render` / `wv-app` 任何依赖<br>❌ 文件系统 / 网络 / i18n / 世界色板 |
| `wv-theme` | `bevy`（**非 optional**，ADR-0043 D1）、`wv-terrain`（纯数据叶子，取 `BiomeFamily`） | ❌ `wv-core` / `wv-io` / `wv-edit` / `wv-render` / `wv-app` 任何依赖<br>❌ `bevy_egui`（egui 是界面关注点，桥接在 wv-app 侧）<br>❌ 文件系统 / 网络 |
| `wv-world-cell-map` | `wv-core`, `wv-terrain`, `bevy`（均**非 optional**，ADR-0054 D3） | ❌ `wv-theme`（固定色 crate 级禁令）/ `wv-io` / `wv-edit` / `wv-render` / `wv-app` 任何依赖<br>❌ 文件系统 / 网络<br>❌ 世界数据写入（编辑必经 wv-app 的 `EditCommand` 链） |
| `wv-app` | `wv-edit`, `wv-render`, `wv-io`, `wv-core`, `wv-terrain`, `wv-overture`, `wv-ui` | ❌ 作为世界的**权威存储**（source of truth 是磁盘 YAML）<br>❌ 绕过 `wv-edit` 直接改世界（必须走 `EditCommand` / `History`）<br>❌ 在 `System`（包括 wv-render 注册的）中 `ResMut<wv-edit::EditHistory>` 或 `ResMut<wv-app state>` 的世界数据<br>❌ 持有「当前值」形态的世界状态（应经 `TimelineQuery` 求值） |

> **`wv-world-cell-map` 补记**（2026-10-23，ADR-0054）：世界级地形工作台
> （`terrain_view/` 含 GPU field 管线 / `terrain_chunks` / `cell_selection` /
> `region_overlay` / `biome_boundary` / `EditCellState` / `GridConfig`）从
> `wv-render` 整建制独立成叶子 crate。**固定色 crate 级禁令**：不依赖任何
> theme crate，色值走平面数据（`CellPalette`），默认源 = 随迁的冻结群系表；
> `ThemePack → CellPalette` 换算与 `ViewMode → ShadeMode` 映射在 wv-render 侧。
> wv-render 单向依赖它（主题浏览态消费同一条 GPU 场管线）；旧路径经
> wv-render re-export 兼容。族后续成员（wv-city-map / wv-building-map）立项各开 ADR。
> bevy 非 optional，标准门禁天然覆盖——为「独立被 AI 优化」服务。

> **`wv-theme` 补记**（2026-10-20，ADR-0043）：主题层（色板 / 装饰旗标 /
> 覆盖物规则 / 主题包契约）从 `wv-render` 整建制独立成平行叶子 crate
> `wv-theme`。`wv-render` 侧只留**重导出**（`crate::theme::colors::X` 等
> 既有路径零改动）与**渲染运行时**（字体装配、树冠采样、镜头分带）。
> 外部主题 crate（如 `wv-theme-chinese-ink`）只依赖 `wv-theme`，
> **不再依赖 `wv-render`**——「加新风格 = 加新 crate、零改渲染层源码」
> 至此成立。`wv-render` 对 `wv-theme` 的依赖是 **optional**，随 `app`
> feature 打开（不带 feature 仍零 Bevy 依赖树）。

> **`wv-ui` 补记**（2026-10-17，ADR-0026）：由 `ui/widgets` + `theme/{ui_tokens,ui_theme}` +
> `ui/shell.rs::UiShell` 整建制独立而成的平行 leaf crate（先于 wv-render 补记阅读亦可）。
> 封装 egui 控件与界面令牌，**不**依赖 wv-core、不做文件 IO、不含 i18n / 世界色板；
> `float_window`（`Panel` 包装）与 `terrain_grid` 地形三件留在 wv-app（shim 保持
> `crate::ui::widgets::X` 等旧路径）；tool_icons 图标资产随迁至
> `crates/wv-ui/assets/ui/tool_icons/`。

> **`wv-terrain` 补记**：它是与 `wv-core` **平行**的纯数据层叶子 crate（零第三方依赖，无 Bevy），承载地形网格（高度场 + 地形类型）与笔刷算法。`wv-edit` / `wv-render` / `wv-app` 均可直接依赖它。

> **`wv-overture` 补记**：与 `wv-core` **平行**的 CLI 数据导入辅助 crate（仅被 `wv-app/src/overture_cli.rs` 引用，负责 Parquet 切片下载与几何转换）；渲染层零引用。详见 ADR-0007 / ADR-0010。

> **`wv-render` 补记**（2026-10-16 用户裁定 + ADR-0020 提议）：由 `wv-app` 内 `render/` + `theme/{palette,colors,colors_google,colors_dusk}`（15359 + 922 行）整建制独立而成的平行 leaf crate。封装 Bevy 渲染与渲染配色，**不**持有编辑入口。render 模块对应用工作区状态的访问一律走 `wv_render::RenderInputs` trait。渲染相关 CJK 字体等 UI 资源（`fonts / ui_tokens / ui_theme`）保留在 `wv-app`。

> **`wv-app` 持有可变 `World` 工作副本是允许的**（2026-10-11 用户裁定）：编辑器必须有可变工作区，`WorldProjection.world` 即为该工作副本，真正的 source of truth 仍是磁盘 YAML。约束的是**改动的路径**——一律经 `wv-edit` 的 `EditCommand` / `History`，不得由 System 直接改世界（§3.3）。`wv-render` 对 `World` 的访问仅以只读引用形式（`*const World` 或 `&World` 接口），所有权留在 `wv-app`。

### 2.2 验证方式

```bash
# 确认 wv-core / wv-terrain 零 Bevy 依赖（应输出空；两条都跑）
cargo tree -p wv-core   | grep -i bevy
cargo tree -p wv-terrain | grep -i bevy

# 确认 wv-core / wv-terrain / wv-edit 零网络栈依赖（应输出空）
cargo tree -p wv-core   | grep -E 'tokio|axum|hyper|reqwest'
cargo tree -p wv-terrain | grep -E 'tokio|axum|hyper|reqwest'
cargo tree -p wv-edit   | grep -E 'tokio|axum|hyper|reqwest'

# 确认 wv-render 反向不依赖 wv-edit / wv-app（应输出空；ADR-0020）
cargo tree -p wv-render | grep -E 'wv-edit|wv-app'

# 确认 wv-ui 不依赖任何兄弟 / 根 crate（应输出空；ADR-0026）
cargo tree -p wv-ui | grep -E 'wv-core|wv-terrain|wv-io|wv-edit|wv-render|wv-app'

# 确认 wv-world-cell-map 不依赖 theme / 兄弟 crate（应输出空；ADR-0054）
cargo tree -p wv-world-cell-map | grep -E 'wv-theme|wv-io|wv-edit|wv-render|wv-app'

# 确认无循环依赖
cargo tree --workspace --duplicates
```

> **2026-10-16 增列**（ADR-0020 提议态）：新 5 条断言对应"render 抽 crate 后的依赖红线"。一旦发现 `wv-render` 引用 `wv-edit` 或 `wv-app`、或 `wv-core/wv-terrain/wv-edit` 沾染 bevy/网络，**立即拒收 PR**——这是分层纪律与 §3.3「不直接改世界」的客观兜底，不必依赖 review 者的记忆。

### 2.3 为什么这条最重要

`wv-core` 必须能独立测试、独立跑 CLI、独立做校验器。`wv-render` 必须能独立测试、独立 headless 跑（未来 wv-render-cli）。**一旦 Bevy 渗进数据层、wv-render 反向依赖 wv-edit，这两件事全部失效**——这是本项目最容易犯、代价最大的错误。

---

## 3. 数据与渲染分离

**YAML 是 source of truth，Bevy ECS 只是投影。**

具体规则：

1. **禁止**把领域模型直接作为 ECS Component。ECS 中只放「本帧渲染所需」的投影数据。
2. **禁止**在渲染层做时间比较。任何「某时刻的状态」必须经 `wv_core::time::TimelineQuery::at` 求出。
   - 理由：把时间语义散落在渲染代码里，等于把 P7 变成口头承诺。
3. **禁止**在 `System` 中修改世界数据。System 只能生成 `EditCommand`，交给 `wv-edit` 执行。
4. 视图刷新是**拉动式**的：时间轴变化 → 重新求值 → 更新渲染资源。不要写「数据变了自动通知渲染」的反向耦合。

---

## 4. 数据模型约定

### 4.1 历史即区间

**永远不要**为随时间变化的量存「当前值」字段。

```rust
// ❌ 错误：这等于把历史覆盖掉，P7 与 P6 同时失效
pub struct Character {
    pub current_place: Id,
}

// ✅ 正确：区间化，当前地点是某时刻的投影
pub struct Character {
    pub state_spans: Vec<CharacterStateSpan>,  // 每个 span 带 TimeSpan
}
```

判断标准：**如果这个字段的值会随时间改变，它就必须是区间化的。**

### 4.2 距离只存参数

**禁止**在数据中出现任何「行程时间结论」字段。

```rust
// ❌ 错误：把计算结果固化成数据，参数一改就过期
pub struct PlaceEdge {
    pub travel_days: f32,
}

// ✅ 正确：只存参数，时间由 DistanceGraph::segment_days 计算
pub struct PlaceEdge {
    pub distance_km: f32,
    pub terrain: String,
}
```

唯一的速度换算入口是 `Vehicle::effective_daily_speed` 与 `DistanceGraph::segment_days`。**禁止在任何其他位置重复实现该公式。**

### 4.3 只存引用，不重复存储

层级关系（地图父子、事件参与者、角色所在地点）一律用 `Id` 引用。P4。

### 4.4 ID 是可读字符串

ID 字符集限制为 `[A-Za-z0-9_]`，如 `city_qingyun`、`kingdom_a`。

- 理由：ID 会出现在 YAML、Git diff 和作者手写内容中，必须可读、可手工引用。
- **禁止**改用自增整数或 UUID。UUID 让 diff 不可读，自增整数让跨文件引用易错。

### 4.5 每实体一文件

角色、事件等实体各自一个文件，对应其 ID。P2 + Git 友好。

---

## 5. 二进制分块纪律

二进制是 P3 的**唯一例外**，因此门槛最高。新增二进制格式前必须同时满足：

1. 确实属于「超大地图网格数组」——**其他任何数据类型都不得使用二进制**；
2. 在索引 YAML 中通过 `RasterRef` 显式声明；
3. 声明 `format_version`，并在 `docs/` 中补充《二进制格式规格》对应条目；
4. 元数据只存引用（路径 + 尺寸 + 编码 + 校验和），**不得内联数据**。

---

## 6. 留痕纪律

### 6.1 术语即时登记

讨论中一旦出现**字典未收录的新术语**，立即登记到 `GLOSSARY.md`（若尚未创建则在首次使用时创建），不允许「先聊着，回头补」。

登记内容包括：术语名、英文/拼音形式、定义、首次出现位置、所属维度。

### 6.2 待决问题显式化

设计中没有定论的问题，**必须**写进 `README.md` §12 或对应的 `docs/` 文件，标注为「待裁」。

- **禁止**用「临时这样实现」的方式绕过待决问题而不记录。
- 代码中出现未定论的实现时，用 `TODO(阶段X):` 标注，并在注释里指向对应待决条目。

### 6.3 文档追写不改机制

补充说明历史设计时，**追加**内容，不修改已有结论。若必须修正，应新开一节说明「原结论 X 被修正为 Y，原因 Z」，保留可追溯性。

### 6.4 重大决策必须开 ADR

架构 / 设计决策（**尤其会推翻既有文档的**）必须留下一份 `docs/adr/ADR-NNNN-*.md`，
写清四件事：**处境 / 决策 / 理由 / 代价 + 被否决的备选**。索引与流程见 `docs/adr/README.md`。

| 原则 | 说明 |
|---|---|
| **分工** | 基线回答「**是什么**」，ADR 回答「**为什么是这个，而不是别的**」 |
| **只增不改** | ADR 一旦「已接受」，**正文不再修改**；决策被推翻时**新开一份**，并在索引里标注修正关系，**不得删改旧 ADR** |
| **不许沉默推翻** | 任何推翻既有 ADR / 基线结论的改动，若没有对应 ADR，视为**未完成** |

---

## 7. 测试要求

### 7.1 强制门槛

任何合入前必须通过：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

> **渲染层追加命令（2026-10-13 用户裁定）**：`wv-app` 的渲染/UI 模块在
> `#[cfg(feature = "app")]` 下，**上面三条命令都不编译它们**（对 wv-app 跑出
> 0 个测试目标，§12-23 的发现）。下方两条渲染层命令已在 `.cargo/config.toml`
> 配了同名别名 `cargo app-test` / `cargo app-clippy`，好敲好记。

**渲染层必须单独跑一遍**（2026-10-13 追加）：`wv-app` 的渲染 / UI 模块全部挂在 `#[cfg(feature = "app")]` 之下，而该 feature **不在默认 feature 里**，所以上面三条命令**根本不编译它们**——实测 `cargo test -p wv-app`（不带 feature）报 `0 passed`，改坏 `terrain_view.rs` / `vector_layers.rs` 之类的文件可以一路绿灯。因此：

```bash
cargo test -p wv-app --features app
cargo clippy -p wv-app --all-targets --features app -- -D warnings
```

> **ADR-0026 后口径（2026-10-17）**：控件层已抽为 `wv-ui`（无 feature 门），
> 标准 `cargo test --workspace` **天然编译并测试控件层全部单测**——上文
> 「三条命令不编译它们」的盲区对 `wv-ui` 不再成立。`app-test` / `app-clippy`
> 继续覆盖仍留在 feature 门后的 wv-app panel / dialog 层（panel 层脱离
> feature 门为待裁项，README §12-39）。

**GUI 启动烟测必须过一遍**（2026-10-15 追加，ADR-0016 followup）：上面所有闸门都在
**测试层**——单元测试不实例化 Bevy 系统集，B0001（query 互斥证据缺失）、资源初始化
顺序、Plugin 互依赖这类**运行期 panic** 只有真正启动 GUI 才暴露。ADR-0016 M2 文字晕边
（halo query 缺 `Without<Camera2d>`）就是全闸门绿灯、首次 `wv gui` 即崩的实例。因此：

```bash
bash scripts/smoke-gui.sh        # 启动 wv gui 观察 5s，进程存活 + 日志无 panic/B000x
```

判定口径：**进程 5 秒存活 + 日志干净 = PASS**；渲染正确性（画面好不好看）不在此范围，
仍属人工验收。涉及渲染系统签名 / Plugin 装配 / 新增 Resource 的改动，提交前必跑。

### 7.2 必须写测试的场景

| 场景 | 要求 |
|---|---|
| 时间区间逻辑（`contains` / `overlaps` / `at`） | 必须覆盖边界：区间开闭、零长度、开放区间 |
| 行程时间计算 | 必须用**手算的期望值**做断言，不接受「跑出来的值就是对的」 |
| 路径搜索 | 必须覆盖：无路径、单向边、换乘、瞬时交通工具 |
| YAML round-trip | `load(save(w)) == w`，每个领域模型都要过 |
| 引用完整性 | 悬空引用必须被**检出并列出**（校验面板），不能静默忽略。**但不得阻止保存**——见 §7.5 |

### 7.3 测试命名

测试名用**完整英文短句**描述行为，不用 `test1` / `test_ok`：

```rust
// ✅
fn instant_vehicle_costs_zero_days()
fn closed_span_rejects_reversed_bounds()

// ❌
fn test_span()
fn it_works()
```

### 7.4 断言要给上下文

比较数值时附上实际值，便于定位：

```rust
assert!((j.total_days - 224.0).abs() < 1e-3, "got {}", j.total_days);
```

### 7.5 校验不是闸门（项目定位的直接推论 · 2026-10-11 用户裁定）

**项目定位**：World View 是**协助创作者整理世界设定的工具**——整理关键人物、地理、事件、关系。作者要能**随便改**。这是定位，不是宽容。

由此推出两条**不可动摇**的规则：

| # | 规则 | 含义 |
|---|---|---|
| **V1** | **校验只提示，不阻止** | 悬空引用、逻辑矛盾等问题**必须被检出并清晰列出**（校验面板），但**不得阻止保存、不得阻止编辑**。作者可以先这么放着，回头再补。工具的职责是「让作者看见问题」，不是「代替作者做决定」。 |
| **V2** | **历史可重写** | 作者可以回头改**已发生**事件的参与者、时间、地点，工具**不阻止、不追问**。改稿是创作常态。 |

**与原则六（P6 非破坏编辑）的关系——必须分清两个层次**：

| 层次 | 规则 | 说明 |
|---|---|---|
| **存储层**（P6 原文） | 用**带时间区间的 span** 增量存储，**不存「当前值」字段** | 这是**实现手段**，目的是让「某时刻的状态」可被求值（P7），并保留可追溯性（撤销/快照） |
| **作者操作层**（V2） | 作者**想改就改**，工具配合，不设阻力 | 这是**用户体验**，作者是主人 |

**P6 约束的是数据结构，不是作者的手。** 把 P6 读成「禁止作者修改历史」是**误读**——它说的是「不要用覆盖式存储把旧值弄丢」，而不是「不许作者改主意」。

> **典型场景**：作者写完第 30 章，发现第 12 章那个角色不该在场。V2 要求工具**允许**他改掉第 12 章的事件参与者，**不弹窗质问他「这与第 30 章矛盾」**。至于数据怎么存（覆盖 or 区间历史），由存储层按 P6 决定，作者无感。

---

## 8. 代码规范

- **Rust edition 2021**，`rust-version = 1.80`。
- 工作区统一依赖版本写在根 `Cargo.toml` 的 `[workspace.dependencies]`，成员 crate 用 `workspace = true` 引用。**禁止**在成员 crate 里写死版本号。
- 公开 API 必须有文档注释（`///`），说明**为什么**这么设计，而不只是「这是什么」。
- 错误类型用 `thiserror`，不用 `anyhow` 作为库的公开错误类型（应用层可例外）。
- 中文注释与文档是**预期**的（本项目以中文为工作语言），但标识符、测试名、类型名一律英文。
- 提交信息格式：`<scope>: <动作>`，如 `wv-core: 为 TimeSpan 增加 overlaps`。
- **UI 图标一律取自 [Lucide](https://lucide.dev/icons/)**（2026-10-17 用户裁定；ADR-0026 后归 wv-ui），不引入其他图标库、不手绘图标。新增图标：在 Lucide 站内选好 → 下载 SVG → 落盘到 `crates/wv-ui/assets/ui/tool_icons/<lucide-name>.svg`（kebab-case，与 Lucide 同名）→ 代码里经 `egui::include_image!` 引用（现有用法见 `crates/wv-ui/src/widgets/primitives.rs`）。描边色必须烘焙为 `stroke="#ffffff"`，不得保留 `currentColor`。

---

## 9. 依赖新增纪律

新增任何第三方依赖前，必须回答：

1. 它是否有引入违反**分层约束**的风险？（尤其：会不会把渲染/网络/异步运行时拖进 `wv-core`）
2. 是否已有等价的标准库或现有依赖能完成？
3. **YAML round-trip 是否仍稳定？** 对序列化相关依赖尤其关键——`bevy_reflect` 之所以不用，就是这个原因。

**默认立场是拒绝。** 依赖是不可逆的成本。

---

## 10. 变更检查清单

提交前逐条自检：

- [ ] 触及了哪条设计原则？如何满足？
- [ ] 依赖方向是否正确？`wv-core` / `wv-terrain` / `wv-edit` 是否仍零 Bevy 与零网络依赖？`wv-render` 是否仍**未**反向依赖 `wv-edit` / `wv-app`？（§2.2 五条 `cargo tree` 都跑过）
- [ ] 有无新增「当前值」字段或「时间结论」字段？（应为区间 / 参数）
- [ ] 新术语是否已登记？
- [ ] 新出现的未定论实现是否已标注待决？
- [ ] `cargo test` / `clippy` / `fmt` 是否全绿？
- [ ] 渲染层改动（`wv-render` / `wv-app/theme/palette`）是否额外跑过 `cargo app-test` 与 `cargo app-clippy` 与 `bash scripts/smoke-gui.sh`（§7.1）？
- [ ] `wv-ui` 是否仍零兄弟/根依赖？§2.2 的 `cargo tree -p wv-ui` 断言是否跑过（ADR-0026）？
- [ ] 数值断言是否附了上下文？

---

## 11. 常见反模式速查

| 反模式 | 违反 | 正确做法 |
|---|---|---|
| 在 `wv-core` 里 `use bevy::prelude::*` | 分层 | 投影放 `wv-app` |
| `character.current_location` | P6/P7 | `state_spans` 区间列表 |
| `edge.travel_time_days` | 数据决策 | 只存 `distance_km` + `terrain`，运行时计算 |
| 地图 `children` 里内联子图数据 | P4 | 只存子图 `Id` |
| 把矢量数据塞进 `.bin` | P3 | 只有超大地图网格数组可用二进制 |
| 渲染代码里写 `if time > event.time` | P7 | 走 `TimelineQuery::at` |
| 直接把 `World` 当 ECS Resource 改 | P6 | 生成 `EditCommand` 交给 `wv-edit` |
| 在成员 crate 写死依赖版本 | 规范 | 根 `[workspace.dependencies]` |

---

## 12. 待决问题的处置

`README.md` §12 列出的问题在收敛前，**不得**在代码中做出不可回退的选择。

- 若实现必须依赖某个待决问题，选择**最容易回退**的方案，并在注释中标注：
  ```rust
  // 待决 §12-1：路径是否允许中途换乘
  // 当前按「允许」建模（状态扩展为 (PlaceId, VehicleId)）；
  // 若最终决定不允许，收敛点是 SearchState，改动面可控。
  ```
- 每个待决问题收敛后，应更新 `README.md`，并在 `docs/` 中补一份决议记录。

---

## 13. 与人协作的约定

- **先分支，再 worktree，后写码。**（2026-10-20 用户裁定）任何涉及代码改动的任务，动工顺序固定为：① 创建分支 `git branch <feat|wip>/<主题>`；② 在该分支上落 worktree：`git worktree add .worktrees/<分支名> <分支名>`——**worktree 目录名 = 分支名**，统一收在仓库根 `.worktrees/` 下（仓库既有约定，已入 `.gitignore`）；③ 之后才在 worktree 内编写代码，主 checkout 不直接改码。worktree 须使用**独立构建目录**（勿跨 worktree 共享 `CARGO_TARGET_DIR`），避免并行会话互相污染出 stale rlib（2026-10-18 实测教训）。
- **不越权提交。** 涉及文档落盘、架构决议的动作，先列出方案，等明确确认后再执行。
- **结论先行。** 回复先给结论，再给依据；用表格承载对比，按严重度排序。
- **多方案并列。** 存在多种合理路径时，给出 A/B 选项与各自代价，而非单方面推荐。
- **智识诚实。** 未经验证不宣称「找不到」；判断错误直接承认并修正。
- **scope 纪律。** 严格遵守当前任务范围，扩张 scope 前先询问。
