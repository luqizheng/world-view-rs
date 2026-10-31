# ADR-0093: 地物符号全模式统一「图标钉」——城堡/小镇插画退役

- 状态：已接受（2026-10-31 用户裁定「POI 图标全部地图都要，去除那些贴图，全部采用图标」+「优先 Lucide，其余 AI 生成 SVG」+「rank≥2 先行」）
- 关联：ADR-0047（呈现级别 rank——rank≥2 进本系统）、ADR-0064（PoiKind 分类符号封闭枚举）、ADR-0074（选中钉染金语义）

## 1. 处境（Context）

rank≥2 地点的符号按模式分流：Topographic/GoogleMap 画 map-pin（圆头尖尾 + 白芯），其余模式（Browse 等）画程序化插画（Citadel 城堡 / Town 小屋，顶点色 mesh）。问题：

1. **语义不可辨**：插画与 pin 都不表达「这是港口还是神殿」——作者在图上摆了一排城，视觉上只有大小差异；
2. **两套画法维护成本**：同一实体两种形态，切换模式时观感断裂（2026-10-25 曾实机踩过「切模式还是城堡、缩放一下才变 pin」的合批键缺陷）；
3. **AI 作者没有图标**：世界观创作常需要「遗迹 / 竞技场 / 传送门」这类语义，插画路线只能继续手写几何。

用户交付了 5 个 AI 生成 SVG（ruins / arena / shrine / portal / danger）并裁定：图标全地图铺开、撤插画、Lucide 优先。

## 2. 决策（Decision）

1. **全模式统一「图标钉」**：rank≥2 地点在所有视图模式画同一形态——彩色钉身（按 `PoiKind` 分类配色，ADR-0064 色表）+ 钉头内**白色含义图形**；选中 = 钉身染金（既有语义不变）。
2. **图形来源纪律**：Lucide 直取为优先（12 类），Lucide 缺的好形由 AI 生成 SVG 补缺（5 类）。SVG 源落 `crates/wv-ui/assets/ui/poi_glyphs/`，经 `scripts/normalize_poi_glyphs.py` 归一化（24×24、y 向上、内容适配 20×20、全元素线稿化）生成 `wv-render::poi_glyphs` 折线数据——**渲染侧无 SVG 解析器、无位图、无新依赖**。
3. **`PoiKind` 扩 5 变体**：`Ruins / Arena / Shrine / Portal / Danger`（封闭枚举原则不变：加类 = 加变体 + 配色 + 图标源）。
4. **Citadel/Town 插画退役**：`LandmarkKind` 枚举与 `push_citadel / push_town` 删除；`LandmarkDot.kind` → `LandmarkDot.poi: PoiKind`。
5. **rank<2 点标不动**：圆点 + 外环保留（编辑态工具语义，`sync_poi` 路径零改动）。

## 3. 理由（Rationale）

- **同一实体单一形态**（P4 只存引用的精神在表现层的推广）：符号语义由「分类色 + 含义图形」承载，模式切换零形变；
- **缩放不糊的根**：符号尺寸锁屏幕像素（`landmark_size_tier × view_scale`），矢量折线随档重烘 mesh——不需要位图 mipmap，也规避了 2026-10-30 标签发糊同类的纹理路径；
- **依赖纪律（§9）**：不做运行时 SVG 解析（拒绝 svg/lyon 依赖），折线数据由脚本预生成进 Rust 常量，图形源的 single source of truth 仍是 assets 下的 SVG 文件。

## 4. 代价（Consequences）

- **线稿化损失填充感**：AI 源里的实心形状（arena/danger）按轮廓描边渲染，与 Lucide stroke 家族一致但失去剪影分量——11px 尺度下不可辨，接受；
- **顶点量上升**：每个图标 ≈ 200 采样点 × 描边四边形；17 类各自合批，视口内地标数十个量级，实测无感（wv-render 测试全绿）；
- **圆角线帽缺席**：方帽近似，11px 下亚像素差异，接受；
- **加类成本上升为三处**（变体 + 配色 + SVG 源 + 重跑生成器）——这是封闭枚举要逼出的显式决策，非意外；
- **被否决的备选**：
  - 位图纹理 Sprite 路线（栅格化 SVG → PNG → Sprite）：引入纹理资产与 lod 偏糊风险，弃；
  - 运行时 SVG 解析 + lyon 曲细分：新增两个重依赖，违反 §9 默认拒绝，弃；
  - 保留插画作为 Browse 主题观感：维持两套画法的维护成本与模式断裂，用户明确裁定撤下，弃。

## 5. 追记

- 2026-10-31 实施落盘：wv-core `PoiKind` 17 变体、wv-default-theme 5 新分类色、wv-render `poi_glyphs`（生成）/`glyphs`（映射）/`landmark_symbols::push_icon_pin`、`sync_landmarks` 按类分池；生成器 `scripts/normalize_poi_glyphs.py`。
- 遗留：rank<2 点标是否升级为小图标钉，待实机观感后另议。
