# ADR-0060: 山地表达与山峰 POI——区域归地形，地标归 POI

- 状态：**草案**（待主理人审批；本 ADR 取代 ADR-0044 D3「山 = 符号，不是颜色」）
- 关联：ADR-0044 D3（被本 ADR 取代）、ADR-0031（程序化装饰）、ADR-0032（群系族键）、
  ADR-0045（区域覆盖物 / 开放键）、ADR-0048（DecorRecipe 配方）、ADR-0059
  （`SymbolStyle`）、ADR-0055 / 0056（CellMap 海拔色带与晕渲，本 ADR 参考其算法而非复用其配色）
- 背景材料：`codex_forest_design.md` 追记三（山地表达的诊断与预览验证）

## 1. 处境（Context）

**主理人裁定（2026-10-25 19:33 原话）**：

> 我觉得山可以不贴图了。因为山脉一段一段的。山峰可以用来标 poi，如喜马拉雅山脉用
> 程序生成，珠穆朗玛峰是 poi。

现状与该裁定的三处差距：

1. **连续地貌被当成散点装饰**。`terrain_decor` 按采样步距逐块扫峰、每块一座山符号
   ——密度由 `stride` 决定而非山体尺度，山脉的连续脊线无从表达，读起来是栅栏。
   追记三的预览已量化该差异。
2. **贴图用错了层**。ADR-0059 为 `mountain_style` / `forest_style` 开了
   `SymbolStyle::Tile`，但地形装饰符号只有 5~29 屏幕像素——96px 贴图缩到这个尺寸
   等于一个色斑。裁定「山不贴图」等于承认该分支**没有使用场景**。
3. **贴图资产找不到入口**。已产出的七类手绘符号（山峰 / 雪峰 / 城堡 / 城镇 / 帆船 /
   罗盘 / 树冠）全是「有名字、尺寸大、数量少」的地物，属 POI 层；而 POI 当前只有
   红点白环（`vector_layers::sync_poi` 四材质桶），`PlaceDot { pos, is_city, rank,
   name_len }` 没有贴图字段。

**数据模型侧的好消息**：裁定所需的两类地物**已经能表达**，无需新增模型——
`wv_core::Place` 有 `biome: BiomeFamily`（ADR-0032 唯一词汇）+ `x/y` 锚点 +
`w/h`（`Some` = 范围地物，`None` = 点 icon）。即：喜马拉雅山脉 = `biome: Highland`
且带 `w/h` 的 Place；珠穆朗玛峰 = 同样的 `biome: Highland` 但 `w/h: None` 的 Place。

## 2. 决策（Decision）

**D1 · 山地按「区域 / 地标」两层归位，地形装饰不再画山**

| 层 | 表达对象 | 载体 | 归属 |
|---|---|---|---|
| 区域 | 山地的**高度与坡度**（无名山脉、整片高地） | 海拔色带 + 晕渲 + 等高线（逐像素烘焙） | `wv-render` 地形纹理管线 |
| 区域 | 具名山脉的范围（如「喜马拉雅山脉」的辖域） | `Place{biome: Highland, w/h: Some}` + `FeatureRule.highland` | `wv-core` 数据 / `wv-theme` 规则 |
| 地标 | 具名山峰（如「珠穆朗玛峰」） | `Place{biome: Highland, w/h: None}` + POI 符号 | `wv-core` 数据 / 渲染 POI |

`DecorKind::Mountain` 的**地形装饰符号退场**：目标态下 `DecorRecipe.mountain_style`
应为 `None`（与水墨包的现状同向）。山体起伏由 D1 第一行承担。

**D2 · 迁移纪律：先有底图手段，再关符号**

**这是硬约束，不可颠倒**：若在底图尚无海拔色带与晕渲时就把 `mountain_style` 设为
`None`，山地会**彻底失语**——既没有符号，也没有色阶与阴影，只剩一块同色高原。
顺序必须是：

1. `terrain_texture` 烘焙期加入海拔色带（跟主题的褐阶 / 墨阶）；
2. 同管线加入晕渲（法线光照，强度上限≈1.12，避免 3D 感）；
3. 等高线（`wv_terrain::isoline` 扩展到高度等值线）；
4. **之后**才允许内置主题把 `mountain_style` 切到 `None`。

**D3 · `SymbolStyle::Tile` 在地形装饰层标记为「无使用场景」**

不删除枚举变体（契约对称、未来 `forest_style` 仍可能用），但在
`DecorRecipe::mountain_style` 的文档中明确：山不贴图（本 ADR 的裁定），
`Tile` 分支对 mountain 只是「无效配置」而非「待实现功能」。
`terrain_decor` 的 `Tile` 一次性 warn 文案相应改写（从「装载通道待 v2」改为
「地形装饰层不加载贴图，请改用 POI 符号」）。

**D4 · POI 符号支持贴图（新增能力）**

`PlaceDot` 增 `symbol: Option<&'static str>`（贴图键，与
`FeatureRule::symbol_texture` 同一口径）；渲染侧从 `EMBEDDED_TEXTURES` 取图，
**未知键回退红点白环**（「缺资产不崩」，与覆盖物层一致）。`w/h: Some` 与
`w/h: None` 两类地物都可挂。

键的来源：`FeatureRule` 已有 `symbol_texture`，但 POI 不读 `FeatureRule`
（那是区域覆盖物的规则）。v1 采用**按 `BiomeFamily` 取主题级 POI 贴图表**
（新增 `ThemePack::poi_symbols`，8 族键），因为裁定只要求「山峰能标 POI」。

**D5 · 资产归位**

| 资产 | 归属层 | 机制 |
|---|---|---|
| 山峰 / 雪峰 | POI（地标峰） | `poi_symbols.highland` → `PlaceDot.symbol` |
| 城堡 / 城镇 | POI（聚落） | 同上，按 `biome` 或 rank 分派 |
| 帆船 | POI（海上地物） | 需 `water` 族点地物 |
| 罗盘 | UI（`wv-ui`） | 不在渲染层 |
| 树冠 | 地点覆盖物（**已通**） | `FeatureRule.symbol_texture` |

## 3. 理由（Why This）

- **符合用户的语言模型**：山脉是「一段一段的」连续体，山峰是「一个点」。用同一套
  等距点阵表达两者，等于强行把连续量离散化——这是栅栏感的根源。
- **符合既有架构**：`Place` 的 `biome` + `w/h` 二态（范围 / 点）本来就是为「区域地物
  与点地物」设计的（ADR-0032 / ADR-0045），本裁定只是把「山峰」放进这个既有容器，
  零模型变更。
- **贴图终于落在尺寸合适的地方**：POI 符号 28~64px，96px 资产有 1.5~3.4x 超采样；
  地形装饰 5~29px，同一资产只剩色斑。同一批图换个层就从「糊」变「清晰」。
- **解决了 ADR-0044 D3 的历史包袱**：当年把山从色带改成符号，是为了修「色带笔直」；
  正解应是「色带 + 晕渲」（跟着高度场走，天然跟随脊线），而非换成散点。

## 4. 代价与风险（Cost）

- **前置工程量在底图**：色带 + 晕渲 + 等高线三者都在 `terrain_texture` 烘焙管线，
  是本次裁定的真正成本（估量级：色带小、晕渲中、等高线中偏大）。
- **`PlaceDot` +1 字段**：内部分投影结构，非主题 API，外部无感。
- **`ThemePack` +1 字段**：`poi_symbols`，两套内置包各一份字面量；外部主题 crate
  用 `..WATERCOLOR` 更新语法可继承（与 ADR-0059 D4 同一手法）。
- **风险**：若色带 / 晕渲做得过重，地图会从「手绘」滑向「3D 渲染」。预览期已确认
  晕渲系数需压在 0.90~1.12，且不作用于水域。
- **风险**：把山峰做成 Place 后，作者需要手动放置每一座峰。若某工程没有 Place 数据，
  山地将只有底色与晕渲（没有峰）——这是可接受的降级（真实手绘地图也常只画山形）。

## 5. 被否决的备选（Alternatives）

- **给地形装饰层实现 `Tile` 装载（原 ADR-0059 D3 的遗留项）**：用户明确「山不贴图」；
  且 5~29px 的符号尺寸下贴图无收益。否决。
- **用 `DecorRecipe` 保留稀疏的山峰点阵（比如每 3 个步距一座）**：只是把栅栏变稀，
  仍未表达连续山体，且与「山峰 = 地标」的语义冲突。否决。
- **为山峰新增 `Peak` 数据模型**：`Place` 的 `w/h: None` 已经是点地物，新增模型会
  制造两套「地物」概念。否决。
- **一步到位：先把 `mountain_style` 全设 `None` 再补底图**：会导致山地失语期间
  地图不可用（D2 的顺序约束即为此）。否决。

## 6. 实施计划（Implementation）

| # | 阶段 | 文件 / crate | 内容 | 前置 |
|---|---|---|---|---|
| 1 | 底图·色带 | `wv-world-cell-map` 或 `wv-render` 地形纹理 | 海拔分档着色，配色**跟主题**（不复用 `hypsometric` 固定 8 阶） | — |
| 2 | 底图·晕渲 | 同上 | 法线光照，系数 0.90~1.12，水域不施加 | 1 |
| 3 | 底图·等高线 | `wv_terrain::isoline` + 渲染叠加 | 高度等值线（现只有海岸线与类型边界） | — |
| 4 | POI 贴图 | `wv-render::world_projection::PlaceDot` / `vector_layers::sync_poi` | `symbol` 字段 + 贴图批次 + 缺键回退 | — |
| 5 | 主题配置 | `wv-theme::theme_pack::ThemePack` | `poi_symbols` 8 族键；内置包填 highland = 山峰图集 | 4 |
| 6 | 关符号 | `wv-theme` 两套内置 `DecorRecipe` | `mountain_style: None` | 1、2 完成且验收通过 |

阶段 1~3 与 4~5 可并行；阶段 6 **必须最后**。

## 7. 验收标准（Acceptance）

1. 切到内置水彩主题，山地区域**没有**等距山符号点阵，但起伏可读（色阶 + 晕渲）。
2. 放一座 `Place{name:"珠穆朗玛峰", biome:Highland, w:None, h:None}` 在地形高点上，
   地图显示手绘山峰贴图 + 名称标签；切换主题时贴图与色带同步换。
3. 删除该 Place 后，山峰贴图消失，地形其余部分不受影响（数据驱动，非烘焙）。
4. 内置主题的截图对比：山地「无符号但有形」，且不与水墨包的等值线表达冲突
   （水墨 `mountain_style` 已是 `None`，本 ADR 只是把水彩也拉齐）。
5. 缺贴图键时 POI 回退红点白环，不 panic（单测覆盖）。
6. `cargo test --workspace` 全绿；`cargo tree` 依赖断言不变（本 ADR 不新增跨层依赖）。

## 8. 遗留

- 具名山脉的 `FeatureRule.highland` 演绎（山脉纹理 / 山脉名）本 ADR 未展开，
  待 D5 落地后另裁。
- 帆船需要「水域族点地物」的建模（`Place{biome: Water, w/h: None}` 是否成立）
  待裁。
- 罗盘属 `wv-ui` 覆盖件，与本 ADR 无关。
