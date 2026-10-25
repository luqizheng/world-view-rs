# ADR-0058: 装饰符号画法主题化契约（SymbolStyle）

- 状态：**已批准**（2026-10-25 用户批准落地并合入 main；§6 五步全部实施完毕，
  门禁见提交说明。§14 色板定稿值的落地另开 commit）
- 关联：ADR-0048（`SeaRecipe` / `DecorRecipe` 配方）、ADR-0043（主题层抽 crate /
  加新风格零改渲染层）、ADR-0023（ThemePack 契约 · M1/M3）、ADR-0045（区域覆盖物
  开放键哲学）、ADR-0032（群系族键）、水墨校准方案 §7 / §12
  （docs/design/chinese-ink-theme-calibration.md）

## 1. 处境（Context）

**用户裁定（2026-10-25，原话）**：「地图上有一个三角形的山体贴图，应该去除，
这个山体应该由 theme 决定……我觉得可以内置山体等默认的样式，但是要提供方式给
theme，改写它们，毕竟 theme，水彩和水墨可能是完全不同的画法，甚至可能它会
被瓦片的方式替代。」

现状与该裁定的差距：

1. **符号几何是渲染层私藏的美术常量**：`terrain_decor.rs` 的八类装饰符号
   （山三角 + 背光面 + 簇生次峰 + 雪帽 + 露岩、林团、半圆丘、沼泽苇点、沙漠
   砾石、白波弧……）的**形状与生成逻辑全部硬编码**。`DecorRecipe` 只提供了
   墨色（`mountain_ink` 等）、阈值（`mountain_min_h` / `snow_cap_h` /
   `rock_h`）、间距（`spacing_px_*`）与尺寸倍率（`size_mul_*`）——**theme
   能调色、调疏密，不能定形**。水彩与水墨对「山」的画法可能完全不同
   （三角符号 vs 等值线 + 皴法），当前机制表达不了该差异。
2. **露岩色硬编码 bug**：`terrain_decor.rs:377` 的极高山露岩三角写死
   `[0.79, 0.73, 0.54, 0.95]`（`#C9B98A` 亮赭），**绕过 theme 的 `rock_ink`**。
   内置水彩基准 `rock_ink` 为 `(0.788, 0.725, 0.541, 1.0)`（decor.rs:169）
   ——RGB 极接近但 **alpha 为 1.0 ≠ 0.95**，故修复后内置主题有一处
   alpha +0.05 的微调（露岩三角略实），需截图比对确认可接受；水墨包已把
   `rock_ink` 改为岩墨灰 `(0.38, 0.37, 0.34, 0.82)` 却不生效，是本 bug 的
   直接受害面。
3. **水墨实机截图的直接触发**：宣纸底上那一排排具象黑三角与方案 §7 自己的
   highland 留白裁定（「山由 DecorRecipe 皴法 + 等值线承担，再画地界符号会
   双料」）自相矛盾——根因是「不画」这个选项在契约里不存在。
4. **与待裁 ③ 同族**：外部主题贴图装载通道（ADR-0023 M1 登记的差距）迟早要
   建；「瓦片方式替代符号」的演进路径应在本次契约设计时一次预留，避免二改。

## 2. 决策（Decision）

**D1 · 新增 `SymbolStyle` 枚举（wv-theme 契约层，纯数据）**

```rust
/// 装饰符号的画法（ADR-0058）：渲染层内置默认为保底，theme 可改写。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SymbolStyle {
    /// 内置默认程序化形状（现状：山三角/林团/半圆丘……）。向后兼容的保底。
    #[default]
    Default,
    /// 不画该符号——表达交给其他层（等值线 / 皴法 / 海配方 / 地形色）。
    None,
    /// 贴图瓦片：键 → 渲染侧装载表。未知键 warn 后回退 `Default`
    /// （「缺资产不崩」口径，与 FeatureRule::symbol_texture 一致）。
    Tile(&'static str),
}
```

**为什么是 enum 不是 bool**：bool 只能「开/关」，表达不了「换画法」——用户明确
画出的演进空间（水彩与水墨画法不同、未来瓦片化）要求契约第一版就覆盖三态；
后续若加「程序化形状变体」（如点皴），是同 enum 加变体，不破结构。

**D2 · `DecorRecipe` 新增 `mountain_style: SymbolStyle`（v1 只加山）**

痛点驱动：山是当前唯一被截图证实的画法冲突。字段模式（style per kind）为其余
七类预留——后续同类追加是重复劳动而非新设计，与 `RegionOverlaySet` 开放键
「数据承载开放集」同哲学。

**D3 · `terrain_decor.rs` 消费分派**

`site_primitives()` 的 Mountain 几何臂（:325）按 `recipe.mountain_style`
分派：`Default` → 现逻辑（三角/背光/次峰/雪帽/露岩）；`None` → 返回空
（该站点不产任何图元）；`Tile` → v1 一次性 warn「装载通道待 v2」后走
`Default`。分派点选在几何臂而非高度判定臂（:950）：同时覆盖高度触发与
biome 笔刷触发两条路径，作者手涂的山地也不画符号（biome 是域事实，
符号是画法——AGENTS §3 数据渲染分离）。

**为什么 Tile v1 只 warn 回退不实现**：给 terrain_decor 建贴图 mesh 路径
（batch UV + 句柄缓存）是把契约变更拖成渲染特性；契约第一版定型
（`Tile(&'static str)` 键在），装载实现挂 ADR-0023 M3 / 水墨待裁 ③，
届时**零契约变更**接通。

**D4 · 内置包 `WATERCOLOR` / `WATERCOLOR_DUSK` 设 `Default`**

观感零变化（除 D6 的 alpha 微调）、存量主题零迁移。水墨包构造点
（`..wv_theme::decor::WATERCOLOR` 更新语法，lib.rs:190）自动继承新字段，
外部主题 crate 构造面不破。

**D5 · 水墨包设 `SymbolStyle::None`**

山体表达收敛到：等值线（`isoline_ink` 重墨）+ 海岸顿笔积墨 + 海配方皴线 +
地形色阶。与方案 §7 的 highland 留白裁定合并叙事（从「接受暖绿回退晕」到
「连符号也不画」的彻底化——后者已于 2026-10-25 经显式零规则先行落地）。

**D6 · 修露岩硬编码色**

`terrain_decor.rs:377` 的 `[0.79, 0.73, 0.54, 0.95]` → `recipe.rock_ink.to_rgba()`。
内置主题仅 alpha 0.95→1.0 的微调（见 §1.2），需截图比对；生效面：任何改了
`rock_ink` 的主题（水墨包岩墨灰自此生效）。

## 3. 理由（Why This）

- **用户理念的契约化**：「内置默认 + theme 可改写」正是 `Default` +
  (`None` / `Tile`) 的结构；渲染层从「画法的垄断者」退为「默认的提供者」，
  选择权归 theme——这是 ADR-0043 精神的自然延伸（该 ADR 禁的是「加风格要
  改渲染源码」，不禁「渲染层提供可替换的默认」）。
- **为什么先只做 Mountain**：唯一被实机截图证实的画法冲突；其余七类没有
  证据表明水彩/水墨需要不同画法（苔点/半圆丘/苇点在宣纸上并无违和）。
- **为什么保留渲染层默认三角**：删默认 = 内置主题重画全部符号 + 破坏存量
  观感；留默认 + 可覆盖 = 零成本兼容。用户明言「可以内置山体等默认的样式」。

## 4. 代价与风险（Cost）

- `wv-theme` 公开 API +1 类型（`SymbolStyle`）、`DecorRecipe` +1 字段
  （基准值在 `WATERCOLOR` / `WATERCOLOR_DUSK` 两处字面量，构造点零改）。
- `terrain_decor.rs` Mountain 几何臂 +约 6 行分派；露岩 1 行修复。
- **风险 1**：水墨关掉山符号后，若某演示世界没有等值线数据，山体表达可能
  「消失过度」——验收时需在 work-view-test 上确认等值线层可见。
- **风险 2**：D6 的 alpha 微调使内置主题非逐像素一致——以 snapshots/ 比对
   + 人眼确认为准，若不可接受则回退 D6（单独修）。
- **非风险**：`None` 不影响地形数据本身（高度场/群系是域事实）。

## 5. 被否决的备选（Alternatives）

- **`mountain_symbol: bool`**：不可演进到瓦片/形状变体，与用户「甚至可能它
  会被瓦片的方式替代」的演进判断冲突；且 bool 语义上把「不画」当成异常态，
  而水墨里「不画」是正常画法（山交给等值线）。否决。
- **`decor_enabled: [bool; 8]` 八类一次全可控**：改动面大一圈且无证据支持——
  当前唯一被截图证实的画法冲突是山；其余七类在宣纸上并无违和，苔点反而是
  水墨正解。采纳「模式预留、按证据追加」，否决「现在全做」。
- **trait object / 闭包定制画法**（`Box<dyn Fn(..) -> Mesh>`）：最灵活但把
  主题契约从「纯数据」拖成「代码插件」——违反 P3（YAML 为主）与 ADR-0023
  「主题包是数据不是代码」的底线，且外部主题 crate 依赖渲染类型会破坏
  wv-theme 叶子纯洁性（ADR-0043 D1）。否决。
- **删掉渲染层默认三角、全部改主题声明**：内置主题需重画全部符号、存量观感
  破坏、演示世界回归风险大。用户明言保留内置默认。否决。

## 6. 实施计划（Implementation）

| # | 文件 | 改动 | 门禁 |
|---|---|---|---|
| 1 | `crates/wv-theme/src/decor.rs` | 新增 `SymbolStyle`；`DecorRecipe` 增 `mountain_style` 字段；`WATERCOLOR`(:159) / `WATERCOLOR_DUSK`(:193) 设 `Default`；随文件 re-export 惯例导出 | `cargo test -p wv-theme` |
| 2 | `crates/wv-render/src/terrain_decor.rs` | Mountain 几何臂(:325)按 style 分派（None→空 / Tile→一次性 warn+Default / Default→现逻辑）；:377 露岩硬编码 → `recipe.rock_ink.to_rgba()` | `cargo test -p wv-render` |
| 3 | `crates/wv-theme-chinese-ink/src/lib.rs` | decor 块显式 `mountain_style: SymbolStyle::None`；补锚点测试（钉住 None + 露岩走 rock_ink 的传播路径） | `cargo test -p wv-theme-chinese-ink` |
| 4 | `docs/adr/ADR-0058-*.md` | 本 ADR 状态 草案 → 已批准（用户确认后由主理人改） | — |
| 5 | 全工作区 | fmt + clippy + 相关 crate 测试；Mountain 既有单测（:1265 雪帽 / :1375 露岩 / :1203 recipe 助手）过一遍，形状不变应全绿 | `cargo test --workspace` |

实施由 engineering-lead 执行；主理人汇编前复核：零改 wv-theme 既有公开 API
语义（仅新增）、水墨包 diff 仅数据层、工作树无越界文件。

## 7. 验收标准（Acceptance）

1. `cargo test --workspace` 全绿（重点：wv-render 的 terrain_decor Mountain
   既有测试、wv-theme-chinese-ink 新锚点）。
2. 内置 daylight 主题截图与改动前一致（允许 D6 露岩 alpha 0.95→1.0 微调，
   snapshots/ 比对 + 人眼确认）。
3. 水墨主题 GUI：提尔城上方那列具象黑三角**消失**；山体表达收敛到等值线
   重墨 + 海岸顿笔；地形起伏仍可读（等值线层可见）。
4. `SymbolStyle::Tile` 未知键路径有 warn 日志且不 panic（v1 无使用者，
   以单测覆盖分派逻辑）。

## 8. 后果（Consequences）

- **正面**：主题对装饰画法有完整三态表达；渲染层从「画法垄断者」退为
  「默认提供者」；待裁 ③（贴图装载通道）的契约半壁先行落地（Tile 键在，
  装载实现零契约变更可接）。
- **负面**：`DecorRecipe` +1 字段——仓库内构造点共 4 处（两个基准字面量 +
  测试助手 + 水墨包更新语法），其中测试助手(:1203)若为全字段字面量需同步
  加字段；仓库外无已知第三方主题包。
- **遗留**：Tile 的装载实现（batch UV mesh + 句柄缓存 + 外部 crate 资产
  注入）挂 ADR-0023 M3 / 水墨待裁 ③；bamboo 等水墨专属区域变体维持待裁 ④
  （双包同步碰 ADR-0043 红线的问题不变）；其余七类符号的 style 字段按证据
  追加（本 ADR 的 style-per-kind 模式即为此预留）。
