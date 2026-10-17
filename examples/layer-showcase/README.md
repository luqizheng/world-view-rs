# 图层演示世界（layer-showcase）

一个「打开即看全图层体系」的演示工程：**世界图 9 种 LayerKind 全声明、每层一个
肉眼可见的演示点**，外加城市子图（声明全量接管）、时间区间化疆域、段级街道窗口、
2.5D 建筑、测量、轨迹。数据由 `cargo run -p wv-app --example gen_layer_showcase`
生成（走 `wv_io::Project::save_world` 标准链路，schema 与加载器同源）。

## 打开方式

```bash
cargo run -p wv-app --features app --bin wv -- gui -m examples/layer-showcase
```

## 30 秒预备知识：三层概念

1. **数据层**：你在 `maps/<id>/index.yaml` 里写的 `layers:` 数组（作者声明，
   落 YAML、进 Git）；
2. **求值层**：程序每帧把声明翻译成「这个渲染组画不画 / 多透明 / 抬多少」
   （ADR-0013 LayerFrame）；
3. **渲染层**：各渲染组在 Z 栈上的**内建槽位**（地形 0.0 → 道路 0.1 → POI 0.21
   → 势力 0.3 → 标注 0.4 → …… → 笔刷光标 0.9）。

作者的 `z` 不是「绝对高度」，是**槽位偏移**：1 格 = 0.05 世界 Z，钳制 ±4 格。
全 0 时渲染与历史版本逐位一致。

## 演示点清单

启动默认在 day 1（时间轴最左端）。

| # | 演示点 | 操作 | 应看到 |
|---|---|---|---|
| 1 | 9 种 LayerKind 全在场 | 打开即得 | 地形 / 官道 / 城邑 / 势力 / 事件 / 标注渲染中；`border`（疆界·预留）与 `custom`（私注·隐藏）**不画也不报错** |
| 2 | `visible: false` | 看 `layer_notes`（custom，声明隐藏） | 没有任何渲染；图层面板也没有它——它是作者声明，不是会话开关 |
| 3 | 半透明 `opacity` | `layer_roads` opacity=0.65 | 城际连线半透明，底图透出来 |
| 4 | `z` 抬升 | `layer_roads` z=+4 | 官道连线**盖在**城邑红点之上（默认 z=0 时在点之下）。想对比：把 z 改回 0、保存重开 |
| 5 | `z` 下沉 | `layer_influence` z=-4 | 势力疆域垫在道路之下、不再盖住城邑点标（默认 0.3 会盖住 0.21 的点标） |
| 6 | 越界 `z` 不拒收 | `layer_border_reserve` z=10 | 照常加载——数据层不校验渲染层的钳（钳到 +4 是渲染层的事，AGENTS §7.5 V1） |
| 7 | 同 kind 双声明 | `layer_roads_backup`（第二个 road 层） | **第一个声明生效**：改 backup 的 opacity/z 无任何渲染变化 |
| 8 | 会话开关 ∧ 作者声明 | 图层面板关掉 Road | 官道消失（本会话）；重开后恢复——面板开关**不写回 YAML**（面板写回是已列的后续任务） |
| 9 | LOD 三档 | 拉远 / 拉近 | 远观只剩 4 座有范围的城邑点标；拉近点标渐次补齐（`lod-demo` 的同款验收） |
| 10 | 时间区间化疆域（P6/P7） | 拖时间轴：day 1 → 80 → 120 | 北府大版图 → 南盟出现 → 北府收缩（疆域是带区间的 span，不是「当前值」） |
| 11 | 事件时间窗 | 拖到 day 1 / 120 / 200 附近 | 西陵建城 / 落雁隘之战 / 东港开埠 的事件高亮只在各自窗口内出现 |
| 12 | 轨迹（Trail） | 拖到 day 100–112 | 驿使队从西陵经青石渡走向北原。注意：轨迹**不在 LayerKind**，只有会话开关（面板 Trail toggle，ADR-0013 待裁项） |
| 13 | 子图声明全量接管 | **双击**东港城菱形图标钻入；**Backspace** 返回 | 子图生效的是它自己声明的 5 层；世界图声明的 9 层**全部失效**。子图故意不声明 EventPoint / Influence——未声明的 kind 回落会话开关缺省，**不是继承父图** |
| 14 | 子图自己的地形 | 钻入东港城 | 256² 滨海平原（东侧入海），与世界图 1024² 栅格各自独立（`World.terrains` 按图索引） |
| 15 | 街道层（`Layer.source`） | 钻入东港城 | 主干 / 次干 / 支路粗细分明、有名街道带标注——几何在 `roads.yaml`，不在 index.yaml |
| 16 | 段级时间窗（ADR-0015） | 钻入东港城，时间轴拖过 day 150（「老墙拆除」锚点） | 环城老墙街整条消失——街道段自带 `[1,150)` 的 time_span |
| 17 | 2.5D 建筑（ADR-0016 §6） | 钻入东港城 | 知港府 / 望海塔（42 m）/ 市仓 / 民居按高度挤出，阴影在体块之下 |
| 18 | 测量（工具产物） | 世界图上看黄色标注 | 折线（西陵→北原路测）+ 矩形（东南角）。测量**不走 LayerKind**，每图一份 `measurements.yaml` |

## 数据文件 ↔ 演示点对照

| 文件 | 内容 |
|---|---|
| `maps/map_world/index.yaml` | 演示点 1–8 的全部图层声明（含注释） |
| `geometry/faction/*.yaml` | 演示点 10 的三段疆域 span |
| `timeline/events/*.yaml` | 演示点 11 |
| `tracks/` + `units/` | 演示点 12 |
| `maps/map_city_dongang/index.yaml` | 演示点 13 的子图 5 层声明 |
| `maps/map_city_dongang/roads.yaml` | 演示点 15/16（`time_span` 在段上） |
| `maps/map_city_dongang/buildings.yaml` | 演示点 17 |
| `maps/map_world/measurements.yaml` | 演示点 18 |
| `maps/*/labels.yaml` | 标签层配置（见下方「已知边界」） |

## 已知边界（读演示时别被误导）

- **`Border` / `Custom` 零消费者**：声明了不渲染（演示点 1/2 正是演示这个事实）。
- **`labels.yaml` 配置 App 端未消费**：标签**层**的可见 / 透明 / z 偏移已接通
  （标注渲染随 `label` 槽位走），但白名单 / 字号阶梯 / 优先级这套**配置文件**
  的消费管线按 ADR-0017 仍待落地——文件先按 schema 落盘占位。
- **`Region`（沃野郡）**：数据就位（标签投影源之一），面渲染与标签派生同属
  ADR-0017 后续。
- **官道的几何来自 `distance.edges`**（城际连线），不是街道路网；世界级
  `Layer.source` 目前不消费（街道 `source` 在子图级消费，见 ADR-0010）。
- **图层面板不写回 YAML**：8 个 toggle 全是会话开关（含 Trail / Terrain Smooth），
  作者声明只能手改 YAML——「面板写回（EditCommand）」是已列的后续任务。

## 再生成 / 改数据

```bash
cargo run -p wv-app --example gen_layer_showcase   # 重新落盘（README 不会被覆盖）
cargo run -p wv-app --features app --bin wv -- validate examples/layer-showcase
```

想体验「YAML 是 source of truth」：直接改 `maps/map_world/index.yaml` 里任一层的
`visible / opacity / z`，保存后重新打开工程即可见效。
