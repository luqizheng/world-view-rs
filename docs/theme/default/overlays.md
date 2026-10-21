# 默认主题：地表覆盖物（Terrain Decor Overlay）

> 地图**森林格**的手绘树图集（`terrain_decor` 的贴图路径，ADR-0050）。
> 与 [`README.md`](README.md)（地物符号 / POI）并列，**本文是覆盖物贴图的提示词与
> 制作规范唯一权威**——重制 / 扩图时照抄，不凭记忆重写提示词。
>
> 资产由 AI 生成（智谱 `glm-image`，2026-10-21），经 `scripts/build_decor_atlas.py`
> 归一化拼接后入库；**源图不入库**（体积大、可复现），只入库 384×128 图集。

## 一、技术规范（生成与入库约定）

| 项 | 约定 |
|---|---|
| 生成尺寸 | 1024²（模型固定输出；缩小到 128 由脚本做 LANCZOS） |
| 入库尺寸 | 每格 **128×128**，三格横排 **384×128**（RGBA，straight alpha） |
| 格序 | **契约**：`0 针叶 / 1 阔叶 / 2 稀树`，与 `DecorVariant` 一一对应（乱序 = 针叶格长阔叶树） |
| 背景 | 纯白 `#FFFFFF`（脚本键控成透明；**不要**要求模型直接出透明底） |
| 文件名 | `crates/wv-render/assets/overlays/forest/decor_atlas_{day,dusk}.png` |
| 内容 | 单主体 / 一组主体、居中、完整可见、四周留白；**无文字、无边框、无地面草地** |
| 墨色 | 日间 = 深墨彩色；暮色 = 浅墨单色（`--light 242,234,216`，与既有 `tree_atlas_dusk` 同法） |
| 屏显尺寸 | **Edit 14 / Region 20 / Nation 24 px**（三档绝对值，与采样步距解耦——树是地表纹理，不许跟着步距放大） |

### 为什么要有「浅墨版」

Bevy 材质 tint 是**乘法**：深墨乘浅色仍是深墨。暗色主题（暮色旧纸）不能靠调 `symbol_color`
提亮，必须换一张浅墨资产。故日 / 暮各一套图集，`DecorRecipe` 里各指各的键。

## 二、提示词 v1（已采用 · 2026-10-21）

**通用风格段**（每条提示词都以此开头）：

> Flat hand-painted world-map symbol, 16-bit late JRPG map asset, seen from a low
> three-quarter angle, thick dark ink outline, simple flat foliage clusters (no individual
> leaves / needles), shadowless silhouette, muted antique palette, isolated on PURE WHITE
> background, no grass, no ground, no cast shadow, no text, no frame, no watermark,
> centred with generous margin.

**0 · 针叶 conifer（三株云杉一排）**

> Flat hand-painted world-map symbol, 16-bit late JRPG map asset, a small stand of THREE
> slim spruce trees side by side seen from a low three-quarter angle. Layered triangular
> crowns in deep muted green (#3B5E3A) with slightly lighter left edges, thick dark olive
> ink outline, simple flat clusters, no individual needles. Slender warm-brown trunks
> visible at the base. No grass, no ground, no cast shadow, isolated on PURE WHITE
> background, no text, no frame, no watermark, the trio centred with generous margin.

**1 · 阔叶 broadleaf（单株圆冠）**

> Flat hand-painted world-map symbol, ONE deciduous tree seen from a low three-quarter
> angle. Rounded canopy built from 4 overlapping foliage lobes in muted olive green
> (#3F7A46), with one lighter sage-green lobe on the upper-left for the light direction.
> Thick dark brown-olive ink outline, no individual leaves, no interior texture, simple
> shadowless silhouette. Short warm-brown trunk with two roots. No grass, no ground, no
> cast shadow, isolated on PURE WHITE background, no text, no frame, no watermark,
> centred with generous margin, one tree only.

**2 · 稀树 sparse（单株伞形）**

> Flat hand-painted world-map symbol, ONE sparse savanna tree seen from a low
> three-quarter angle. Thin forked warm-brown trunk and an airy canopy of 5 small round
> leaf clumps in olive-yellow green with clear white gaps between them, thick dark ink
> outline, simple flat clusters, no individual leaves. No grass, no ground, no cast
> shadow, isolated on PURE WHITE background, no text, no frame, no watermark, centred
> with generous margin, one tree only.

**负面提示词**（按供应商语法拼：`negative prompt` / 反向词）：

> grass base, ground patch, cast shadow, drop shadow, photorealistic, 3D render, isometric,
> pixel art, thin outline, text, letters, watermark, logo, signature, frame, multiple
> unrelated subjects, busy interior detail, harsh saturated colors.

### 提示词迭代记录

| 版本 | 变化 | 结果 |
|---|---|---|
| 试作 | 「hand-painted watercolour … one mature deciduous tree」 | 细节过密（单片叶子可数）、带草地底座与水印；缩到 24 px 糊成一团 |
| **v1（采用）** | 改「flat / simple flat foliage clusters / no individual leaves / no grass / no cast shadow」 | 剪影可读、无底座；水印仍在（脚本涂白） |

## 三、处理管线

`scripts/build_decor_atlas.py`：涂白右下角供应商水印 → 白底键控（`alpha = 1 - min(R,G,B)/250`
反预乘恢复墨色）→ 内容 bbox 底部居中放进 128 画布 → 三格横向拼接。

```bash
PY="C:/Users/leo/.dsh/dsh-runtimes/dsh-primary-runtime/dependencies/python/python.exe"

# 日间（源图按 JOBS 表放在仓库根的 dsh-image-gen/ 下，不入库）
"$PY" scripts/build_decor_atlas.py --out crates/wv-render/assets/overlays/forest/decor_atlas_day.png

# 暮色浅墨版（目标色 = 纸面色 #F2EAD8）
"$PY" scripts/build_decor_atlas.py \
  --out crates/wv-render/assets/overlays/forest/decor_atlas_dusk.png \
  --light 242,234,216
```

**单格特殊处理**：稀树源图底部带草地底座，在脚本 `JOBS` 表里用 `cut_bottom=780`
（1024 图中）在草线以上裁掉——保留树干，去掉地面。

## 四、验收（人工 + 可自动的部分）

| # | 口径 | 判定 |
|---|---|---|
| 1 | **24 px 可读**：缩到 24 px 仍能一眼分出「针叶 / 阔叶 / 稀树」 | 截图目视 |
| 2 | **去色剪影可辨**：三格去色后轮廓形态不同（规格 §3.2.2 原则 1） | 目视 |
| 3 | 与 `core.forest #3F7A46` / `core.grass #8CB95F` 同色族，不出现荧光绿 | 目视 |
| 4 | 暮色图集在暗底上**亮于**底图、不糊成黑块 | 实机暮色主题 |
| 5 | 缺图集 / 解码失败 → 回退程序化树，不空帧 | 单测 `embedded_decor_textures_cover_both_palettes_and_reject_unknown_keys` + 改键冒烟 |
| 6 | 格序不错位：针叶格不会被画成阔叶 | 单测 `forest_art_cell_maps_variants_to_atlas_order` |

## 五、运行时接入

- `wv-theme::decor::WATERCOLOR.forest_art = Some(DecorArt::new("overlays/forest/decor_atlas_day.png", 3))`；
  `WATERCOLOR_DUSK` 指向 `decor_atlas_dusk.png`；
- `terrain_decor` 的 Forest 桶走 UV quad 路径（ADR-0050 D3），其余族仍程序化；
- 配方经 `ViewState.active_theme.decor` 注入（ADR-0049 主题包切换后）：换包即换图集；
- 新增 / 替换图集：换文件 → 改 `DecorArt` 路径（若文件名变了）→ 跑 `cargo test -p wv-render --features app`。

## 六、已知限制

- **不能随主题调色**：一套墨色服务一套底色，换色必须重出图（见上文「浅墨版」）；
- **格数固定 3**：加第 4 变体要同时改 `DecorArt.cells`、`forest_art_cell`、图集与 ADR-0050 D2；
- **与地界树图集并存**：`feature_overlays` 仍用旧的 `tree_atlas_{day,dusk}.png`（淡彩），
  两条通道观感略有差异，合并列为后续小任务。
