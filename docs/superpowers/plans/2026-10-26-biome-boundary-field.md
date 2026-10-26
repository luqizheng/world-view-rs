# ADR-0072 实施计划：biome 距离场——群系填色 + 亚格平滑边界

> ADR：`docs/adr/ADR-0072-biome-boundary-distance-field.md`（已接受 2026-10-26）。
> 直接动因：场管线 Themed 分支无群系面——雪原格画成土黄海拔色后，装饰层的
> 雪原白弧全部凸出（2026-10-26 用户实机「白点点」反馈）；绿地色块台阶同批。

## 目标（用户视角）

1. GoogleMap/Themed 底图出现**群系类别色**（雪原白、森林绿、沙漠黄……按主题
   调制），装饰符号重新融入底色；
2. 群系色块的边界 = 亚格连续 + 圆角（与海岸同款语法），台阶消失；
3. CellMap/BiomeRaw 分支只吃边界平滑（它本来就有群系色）。

## 任务

### T1 · 生成器 `biome_sdf.rs`（纯函数，无 feature 门）
`biome_distance_field(w, h, biome_of, cap) -> (d: Vec<f32>, nb: Vec<u8>)`：
- 语义与海岸无符号距离同族：d = 格心到最近**异类群系**格心的距离（格），
  nb = 该异类格的群系 ID（argmin 传播）；
- 算法：按群系类多源 Dijkstra（8 邻，直 1 / 斜 √2），从该类全体格出发、
  距离 > cap 早退；cap 缺省 8（与 DASH_MAX_CELLS 同族——平滑带只有
  ~1.5 格宽，cap 外饱和为 FAR，shader 侧 t 恒为 own 色）；
- 单测（手摆网格、完整英文短句名）：对半网格边界 d≈1 / 对角 √2 /
  cap 饱和 / 三界角点 argmin / 全同质饱和。

### T2 · pack 通道 `biome_tex`
- FieldPack 增一张纹理：`R = d / cap`（Linear 采样）、`G = own biome`、
  `B = nb biome`（G/B 走 Nearest 采样器——ID 不可插值，同 attr 惯例）；
- `pack_content_hash` 纳入；`patch_region` 一致性测试按海岸模板
  parameterized 扩展（补丁后逐字节 == 全量重打）。

### T3 · 主题群系色调制表（wv-theme + 桥接）
- DecorRecipe 同族：`biome_tint: 每群系 (RGB, strength)`（Themed 分支用；
  BiomeRaw 分支保持群系固定色不动）；
- `FieldPalette` 扩展 + `cellmap_bridge` conv 接线（主题 → uniform）。

### T4 · shader 消费
- 填色：`t = smoothstep(0.5, 0.5 + half, d)`，`half = min(0.5,
  BIOME_EDGE_HALF_PX / px_per_cell)`（屏幕封顶，与海岸同语法）；
  `rgb = mix(nb_color, own_color, t)` 再向 themed 海拔阶混合 tint；
- BiomeRaw 分支：现有 `biome_raw_approx(biome_id)` 保持为 own_color，
  仅边界平滑生效；
- T27 的 4-邻墨线在此之后**退役**（连续边界 + 屏幕封顶已覆盖其职责），
  墨线是否沿 d=0.5 等值线保留一条 0.8px 细线 → 实机对比后定。

### T5 · 留痕与验收
- SPEC 追记十六（T40+ 编号：biome 场通道 / 屏幕封顶常量 / T27 退役）；
- GLOSSARY：「biome 距离场 / biome distance field」；
- 门禁全绿 + 实机：白点融底、绿地回归且边界平滑、CellMap 无回归。

## 执行顺序

T1 → T2 → T4（用假 tint 直出验证边界）→ T3 → T5。T1/T2 纯数据层可独立
合入；每步保持全门禁绿。
