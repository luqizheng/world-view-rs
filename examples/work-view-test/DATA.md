# 参考数据生成指南（work-view-test 底座）

> 从零复现 `work-view-test` 的地理底座：下载 Natural Earth 公共领域数据 →
> 裁剪转换成 GeoJSON → 用 `wv-gen-demo` 生成 `terrain.bin`。
> 全程约 10 分钟（不含下载时间），只需 Python + Rust 工具链，无 GDAL、无网络依赖进 wv 本体。

---

## 1. 数据是什么、为什么是它

| 文件 | 内容 | 用途 | 许可 |
|---|---|---|---|
| `ne_110m_admin_0_countries.geojson` | 国界多边形（110m 比例尺） | **海岸线**——国界多边形的外边界就是海岸 + 陆地轮廓 | 公共领域（PD） |
| `ne_110m_rivers_lake_centerlines.geojson` | 主要河流中心线 | 河流 → BIOME_RIVER | 公共领域（PD） |
| `ne_110m_geography_marine_polys.geojson` | 海域多边形 | 海洋兜底（可选，无它也能跑） | 公共领域（PD） |

**为什么 110m**：最小比例尺 = 顶点最少（全球海岸线约 5k 点），裁剪 + 简化后
约 1k 顶点，`wv-gen-demo` 秒级出图。要更细的海岸 later 可换 50m（顶点 ×5）。

**为什么 Natural Earth**：公共领域、无署名要求、无清算风险——
对比 Overture（CDLA-Per 下游义务）和 Google/OSM（署名/_shareAlike），
是唯一能「拿来做 demo 再发行」零负担的数据源。

---

## 2. 下载（一次性）

NE 官网的 zip 直链已失效（WordPress 报错页）；**官方 GitHub 仓库的 `geojson/` 目录**
有现成 GeoJSON（shp 的官方转换，随仓库更新）：

```bash
cd examples/work-view-test/data/ne
export https_proxy=http://localhost:10808   # 需要代理时
base="https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson"
curl -sSL -o ne_countries.geojson "$base/ne_110m_admin_0_countries.geojson"
curl -sSL -o ne_rivers.geojson      "$base/ne_110m_rivers_lake_centerlines.geojson"
curl -sSL -o ne_marine.geojson      "$base/ne_110m_geography_marine_polys.geojson"
```

完成后 `data/ne/` 下应有三个 `ne_*.geojson`（合计 ~1.4MB）。

> 下载物已在 `.gitignore`（数据不入仓）；`convert.py` 是唯一入仓的文件。

---

## 3. Python 环境

**零第三方依赖**——`convert.py` v2 只用标准库（json / math / pathlib）。
任何 Python 3.10+ 开箱即用。

---

## 4. 裁剪转换（GeoJSON → 裁剪 + 简化）

`convert.py` 已内置「地中海 + 北非」bbox（lon -12–37、lat 30–48）与
Douglas-Peucker 简化容差（0.02° ≈ 2km）。直接跑：

```bash
cd examples/work-view-test/data/ne
python convert.py . ../
# 输出三行 [ok]，产物落在 data/ 下（海岸线约 1068 顶点）：
#   data/coastlines.geojson   国界多边形（海岸 + 陆地）
#   data/rivers.geojson       河流折线
#   data/seas.geojson         海域多边形
```

产物三个 `.geojson` 也已 gitignore——它们能从上面两步确定性重建。

**换裁剪区域**：改 `convert.py` 顶部的 `BBOX` 与 `TOL`。
建议初次跑通后保持地中海选区（与内容层的地名/势力手作配套）。

---

## 5. 生成底座（GeoJSON → terrain.bin）

```bash
# 在仓库根目录
cargo build -p wv-app --bin wv-gen-demo
./target/debug/wv-gen-demo.exe examples/work-view-test
```

预期输出：

```
工程目录：examples/work-view-test
[natural-earth] terrain.bin (1024x768) checksum Some("....")
[ok] examples/work-view-test\maps\world_main\index.yaml
完成。用 `wv gui -m examples/work-view-test` 查看效果。
```

**分支逻辑**（自动）：
- `data/coastlines.geojson` + `rivers.geojson` 存在 → NE 真底座；
- 缺任一 → fallback 到程序冷北生成（与 `examples/jrpg-world` 同源），
  输出标 `[program]`。**工程在任何情况下都能打开**——所以随时可以删掉
  `data/*.geojson` 回到程序世界做对比。

生成器做什么（`crates/wv-app/src/bin/gen_demo/`）：

| 步骤 | 内容 |
|---|---|
| 栅格化 | 1024×768 格，海岸多边形扫描线填充 surface 0/1 |
| 高度 | 陆格按「距最近海岸线」距离分带（30/150/700/1500/2300m），海格按距离估深 |
| 群系 | 纬度带（北 18% 雪 / 28% 森林 / 中部平原 / 55% 草原 / 75% 沙漠）+ 高度修正（山/丘） |
| 河流 | 折线刷 BIOME_RIVER + 河床 -2m |
| 落盘 | `terrain.bin` + `maps/world_main/index.yaml` + `world.yaml` 的 `template:` 段 |

**内容层安全**：生成器只动地形与 template 段；`distances/`、`factions/`、
`characters/`、`timeline/` 等手作内容永不被覆盖（重跑安全）。

---

## 6. 验证

```bash
cargo run -p wv-app --features app --bin wv -- validate examples/work-view-test
cargo run -p wv-app --features app --bin wv -- gui -m examples/work-view-test
```

GUI 里检查（Browse 视图、拉到全局档）：

- [ ] 海陆轮廓与真实地中海可对照（伊比利亚 / 意大利 / 希腊 / 北非可辨）
- [ ] 河流蓝线从内陆入海
- [ ] M1 装饰在位（山脉三角 + 雪帽 / 森林团簇 / 沙漠扇形）
- [ ] 日间主题色与 `examples/jrpg-world` 一致（同一 JRPG A 色板）

---

## 7. 常见问题

| 症状 | 原因 | 处理 |
|---|---|---|
| NE 官网 zip 直链返回 HTML | 官网 WordPress 直链已失效 | 改用第 2 节的 GitHub raw 镜像（GeoJSON 现成，无需 shp） |
| 下载 0 字节 | media.githubusercontent（LFS）不可达 | 用 raw.githubusercontent.com 路径（本指南第 2 节即此路径） |
| GUI 打开后欢迎页报错 | index.yaml 缩进损坏（旧版生成器 bug） | 重跑 `wv-gen-demo`（已修复），勿手工编辑 index.yaml |
| 生成超慢（>10s） | 误用了 50m 数据 | convert.py 只认 110m 文件名；换回 110m |
| 想回程序世界对比 | — | 删 `data/coastlines.geojson` 重跑 `wv-gen-demo`，输出标 `[program]` |

---

## 8. 数据许可说明

Natural Earth 数据是**公有领域（Public Domain）**：
> All Natural Earth data uses the public domain license.

对 demo 工程的含义：可以随仓库分发衍生数据、可以用于任何商业/非商业用途、
无需署名（但我们 courtesy 提一句数据来源是好习惯）。
`data/ne/` 的 shp 与 `data/*.geojson` 均已 gitignore——不入仓不是因为许可，
而是因为它们可从上面两步确定性重建（P3：仓库只留生成脚本）。
