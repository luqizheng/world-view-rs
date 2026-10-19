# work-view-test：策展 JRPG 演示世界

后续所有 demo / 视觉验收 / 新模块试跑都复用这一个工程，**不再新建一次性工程**。

## 立场

- **底座**：Natural Earth 110m（公共领域）→ wv-overture 管线 → 简化到 ≤ 2k 顶点的 chunk
- **装饰**：JRPG A 调色板 + M1 装饰（Nation 山形/沙漠/浮雕）+ M2 河流/城市
- **内容**：手作 ~30 实体（地名 + 势力 + 事件 + 路线）
- **风格自洽**：底图风格由 wv-render 决定，**不被 Natural Earth 风格牵走**

## 数据准备（一次性）

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

```bash
# 1) 一次生成：自然地底座 + 装饰层 + 内容
cargo run -p wv-app --features app --example gen_work_view_world -- examples/work-view-test

# 2) 校验
cargo run -p wv-app --features app -- validate examples/work-view-test

# 3) 跑 GUI 看效果
cargo run -p wv-app --features app -- gui -m examples/work-view-test
```

## 选区

**地中海 + 北非**——伊比利亚 / 意大利 / 希腊半岛 + 北非沙漠 + 爱琴海群岛。JRPG 经典蓝本，海岸线戏剧性最强（半岛 / 岛屿 / 海湾密集），跨气候轴（南岸暖沙漠 / 北岸温带森林），与现成 ColdNorth 调色板一拍即合。

## 暂存区

`data/ne/` 落原始 Natural Earth shapefile（gitignore）。生成器会就地解析，**不**写中间格式。
