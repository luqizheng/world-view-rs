# work-view-test：策展 JRPG 演示世界

后续所有 demo / 视觉验收 / 新模块试跑都复用这一个工程，**不再新建一次性工程**。

## 立场

- **底座**：Natural Earth 110m（公共领域）→ wv-overture 管线 → 简化到 ≤ 2k 顶点的 chunk
- **装饰**：JRPG A 调色板 + M1 装饰（Nation 山形/沙漠/浮雕）+ M2 河流/城市
- **内容**（2026-10-19）：33 地点 / 5 势力（含 5 块领地多边形）/ 6 人物 / 4 事件 / 4 轨迹 / 5 条关系
- **风格自洽**：底图风格由 wv-render 决定，**不被 Natural Earth 风格牵走**

## 数据准备（一次性）

> **完整步骤见 [DATA.md](DATA.md)**——下载直链、Python 环境、常见问题都在那里。下为本速查。

数据是公开数据集，**不入仓**——只在本机下载一次。

```bash
mkdir -p examples/work-view-test/data/ne
# 三件套（公共领域，引用 1-1-1）：
# 1. 海岸线 / 国界   ne_110m_admin_0_countries.shp
# 2. 主要河流       ne_110m_rivers_lake_centerlines.shp
# 3. 海域多边形     ne_110m_geography_marine_polys.shp
# 下载地址：https://www.naturalearthdata.com/downloads/110m-cultural-vectors/
# 落 examples/work-view-test/data/ne/ 下
```

## 工程操作

生成器是**独立 CLI**（无 GUI / 无 app feature）：

```bash
# 1) 生成底座（只重写 maps/ + world.yaml；不碰内容层）
cargo build -p wv-app --bin wv-gen-demo
./target/debug/wv-gen-demo examples/work-view-test
#   --peaks 10000    RPG 幻想海拔尺度（缺省 3400 m）
#   --dump-png x.png 顺带导出一张高度场预览

# 2) 校验（目录完整性）
cargo run -p wv-app --features app -- validate examples/work-view-test

# 3) 跑 GUI 看效果
cargo run -p wv-app --features app -- gui -m examples/work-view-test
```

## 内容层约定（2026-10-19 实机验收后固化）

### 坐标：extent 空间、y 向上

`maps/world_main/index.yaml` 的 `extent: 4096×3072`，而 `terrain.bin` 是
`1024×768` 栅格。**内容层（places / tracks / faction geometry）一律写 extent 坐标**，
不是栅格像素：

```
x_extent = x_raster * 4
y_extent = 3072 - y_raster * 4      # ADR-0034：北 = y 大，上北下南
```

（栅格行 0 = 北，图内坐标 y 向上；两套 y 方向相反，漏掉翻转就会「北非跑到地图上方」。）

### places：`distances/places.yaml`

- `rank`：3 = 国家级 / 2 = 市镇 / 1 = 普通点（渲染按缩放档过滤，Nation ≥ 3）；
- `biome` 可缺省（按生成器气候轴派生）；不存在 `facilities` 字段。

### factions：每实体一文件（§4.5）

`factions/<id>.yaml` 是**单个 mapping**，不是列表。必填 `id` / `name` / `color`；
`rank` 是**字符串**渲染提示（`kingdom` / `order` / `tribe` / `free_city` …），不是数字。
领土不在势力文件里，而在 `geometry/faction/<file>.yaml`：

```yaml
map: world_main
owner: crown_of_seville
from: 1
geometry: [x0, y0, x1, y1, ...]   # extent 坐标，扁平外环
```

### events：`participants` 是人物，不是势力

`participants` 引用 `characters/<id>.yaml`；势力关系写进 `relations/graph.yaml`。
`effects` 只有四个变体：`move_character` / `set_title` / `claim_region` / `custom`
（声明式，见 `wv-core/src/model/event.rs`）。写别的变体工程会**打不开**（YAML 解析失败）。

### tracks：`place` 只做引用校验，渲染靠 `pos`

`waypoints[].place` 用于悬空引用诊断；渲染层的 `sample_at` / `grown_polyline`
只读 `waypoints[].pos`。**两者都要写**，`pos` 与 place 的中心坐标一致。

## 选区

**地中海 + 北非**——伊比利亚 / 意大利 / 希腊半岛 + 北非沙漠 + 爱琴海群岛。JRPG 经典蓝本，海岸线戏剧性最强（半岛 / 岛屿 / 海湾密集），跨气候轴（南岸暖沙漠 / 北岸温带森林），与现成 ColdNorth 调色板一拍即合。

## 暂存区

`data/ne/` 落原始 Natural Earth shapefile（gitignore）。生成器会就地解析，**不**写中间格式。
