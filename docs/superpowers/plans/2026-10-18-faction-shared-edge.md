# 势力划分 M2：切弧拼接（共边逐点保真）

> 日期：2026-10-18 · 状态：**已完成**（T0–T6；终审 2 Critical + 4 Important 已修）
> 上游：ADR-0028（已接受，文末「追记」把 M1 的亚格偏差显式记为 M2 候选）· spec `2026-10-18-world-from-zero-map-design.md` D3/D5/D8 + 追记一

## 问题（ADR-0028 追记的两种近似）

1. **贴边切割**（意图贴着宿主外缘切，余部无洞）：新势力环与余部外环各自独立 chaikin/DP 平滑，
   共享的那条切弧不逐点相等（< 0.2 格偏差）。
2. **未触碰的外沿与既有洞**：每次划分都把宿主整条边界重新栅格化 + 平滑，反复划分会漂移。

## 方案：切弧拼接 + 旧路径兜底

`wv-terrain::divide` 新增一条**拼接路径**（默认启用，任何不确定自动回退旧路径）：

1. **原始环**：`mask_boundary_rings` 拆分出 `raw_mask_boundary_rings`（不平滑的格边阶梯），
   旧函数保留「raw → smooth」原语义（`marker.rs` 等既有调用零变化）。
2. **分类**：把新势力掩膜的原始环按「顶点到宿主原环的最近距离 ≤ 1 格」切成
   **宿主弧 / 切口弧**交替段（`classify_against_host`）。宿主弧记录为宿主环上的
   投影位置（段号 + 段内比例），方向由最长近段的标量推进多数票决定。
3. **拼接**：宿主弧**逐点照抄宿主原顶点**（含折点），切口弧只平滑一次；
   余部 = 宿主**补弧**（同一份宿主原顶点）+ **反序的同一条切口弧**——共边由构造逐点相等。
4. **吸附只动切口**：`divide()` 的贴地模式加密/吸附/钳制**只作用于切口弧**，两端宿主投影
   点固定不动；宿主外沿与既有洞永不被吸附改写。
5. **兜底**：多切口（会把宿主切成多块）、意图切到既有洞、整条环贴宿主、面积校验
   （拼接环 vs 旧路径环差异 > 35%）→ 完全走 M1 的旧路径（含 `mirror_shared_edge`）。

## 代价

- 分类是**几何启发式**（容差 1 格），不是精确布尔运算；误判由面积校验兜底回退，
  因此极端形状可能退回 M1 的亚格近似（可接受：不劣于现状）。
- 拼接路径下「意图切到既有洞」仍走旧路径（洞的分捡超出本批范围）。
- 宿主弧逐点保真意味着**宿主原环上的顶点密度被原样继承**（多次划分后顶点数会累积，
  不会自动抽稀）——这是保真的必然代价。
- `divide_land_host`（空势力路径）不变：那里没有邻接势力，无共边语义。

## 测试

| 层次 | 用例 |
|---|---|
| wv-terrain | `edge_touching_cut_splices_host_arc_verbatim`（宿主折点逐点带出）· `edge_touching_cut_mirrors_shared_arc_pointwise`（切口弧两侧逐点互为反序）· `second_division_preserves_untouched_neighbour_edges_pointwise`（两次划分邻边不变）· 既有 10 条（嵌套洞/碎块拒绝/贴地吸附/越界钳制/无地物退化）全绿 |
| wv-edit | `two_divisions_keep_untouched_host_edges_pointwise_and_undo`（两连划分 + 两级 undo 逐位还原） |

## 门禁

`cargo test --workspace` · `cargo clippy --workspace --all-targets -- -D warnings` ·
`cargo fmt --all -- --check` · `cargo app-test` · `cargo app-clippy` · `bash scripts/smoke-gui.sh` ·
§2.2 依赖断言。

## 终审结论与修复（2026-10-18）

独立 reviewer 判「需修后合」，2 Critical + 4 Important：

| # | 问题 | 修复 |
|---|---|---|
| C1 | 近段只有单个原始顶点（斜边宿主贴边小切口）→ 宿主弧退化，余部塌缩成新势力，宿主静默丢地 | `try_spliced` 拒绝 span≈0 或 ≈整环的宿主弧；`build_spliced` 校验余部环 ≥3 点且面积 >0 |
| C2 | 顺时针 winding 宿主：补弧取回的是 inside 自己那条弧，余部同样塌缩 | `cut_neighbors` 返回 reverse 标志；reverse 时余部用 forward(cut_end→cut_start) 反序 |
| I1 | 面积校验只查 inside | 改为 inside + 余部**净面积**双端校验 |
| I2 | 容差 `<=1.0` 把边界内侧正好 1 格的意图外扩到宿主边界 | 改严格 `d < 1.0`，并在 ADR 记录该语义后果 |
| I3 | 兜底路径复用 `finish_division`（会再取拼接结果）→ 吸附破坏共边；`mirror_shared_edge` 可误配既有洞 | 兜底直接用 `legacy_geometry(&core)`；mirror 跳过点列近似等于既有洞的候选 |
| I4 | ADR 追记二与实现不符 | 追记二补齐真实回退条件与 winding/吸附约束 |

新增回归：`clockwise_host_winding_keeps_remainder_intact` · `slanted_host_tiny_edge_cut_never_collapses_remainder` ·
`snapped_edge_cut_keeps_shared_arc_pointwise` · `edge_cut_preserves_existing_host_hole`。
