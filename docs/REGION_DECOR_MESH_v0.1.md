# 选中自然区装饰 → 合批 Mesh2d（缺陷修复 + 两条手势）v0.1

> 日期：2026-10-24 · 迭代档缺陷修复（**非新功能**，无新依赖）
> 前序：`docs/PERF_CURSOR_AND_OVERLAY_v0.2.md` §六.3 已把这条路径记为待实测技术债
> （原文：「`draw_region_decor` 仍每帧把阶梯边逐条 `gizmos.line_2d`：选中一个
> 大区域时边数上万，未实测其帧成本（…若实测 >2ms，下一步改成 `Mesh2d` 合批）」）。
> 本轮即该债的兑现。

---

## 一、用户症状与范围

| # | 症状（用户原文） | 本轮处置 |
|---|---|---|
| 1 | 开「显示自然区与候选」→ 点选一个自然区 → **锚点出现，FPS 掉到 20 多** | **修**（§2.1 合批 mesh） |
| 2 | 锚点要改成**实心白色小方块** | **改**（§2.2；Gizmos 画不出填充面，与 1 同一处改动） |
| 3 | **左键点选**自然区、**右键取消选择** | **加**（§2.3 两条手势） |

**不做**：`enumerate_candidates` 的候选虚线高亮（另一条 gizmo 路径，与「选中」无关）；
区域轮廓 / 底色覆盖（上一轮已合批）；不改 `[profile.dev]`；不碰 `examples/*/yaml`。

---

## 二、实现

### 2.1 症状 1 —— 逐条 gizmo ⇒ 合批 mesh

**根因**：`world_projection::draw_map_gizmos` 里对 `plan.stepped_edges` **逐条**
`gizmos.line_2d`，对手柄**逐个** `gizmos.rect_2d + 2 × line_2d`。Gizmos 是即时线元：
每条一次装配 + 一个渲染实例，且**每帧重来**。选中大区域时阶梯边 = mask 边界格边，
量级随「边界长度」而不是「每帧脏数据」走（碎片化群系可达上万条）。
选中前该路径被 `nature_overlay` 门控挡掉 ⇒ 症状精确表现为「一点选就掉帧、取消就恢复」。

**处置**（新文件 `crates/wv-render/src/region_decor.rs`）：

| 项 | 内容 |
|---|---|
| `RegionDecorPool`（Resource） | 两块合批 mesh 的槽位 + 共享材质 + 计数器 `edge_rebuilds` / `anchor_rebuilds` / `visible` |
| `DecorKey` | `{ fingerprint, size_bits }`；`size_bits` 取**缩放档中心**（`scale_of_bucket`）而不是 `view.scale` 原值 |
| `BatchUpdate` | **三态**：`Keep`（键没变，一个字节都不动）/ `Rebuild(key, mesh)` / `Clear` |
| `sync_region_decor` | 求计划（复用 `plan_region_handles` + `RegionDecorCache`）→ 比对键 → 只在键变时建 mesh → 写计数器 |
| z | 描边 `0.0785`、锚点 `0.0790`（压在区域轮廓 0.076 / 候选预览 0.077 之上，道路 0.09 之下） |

**为什么阶梯边的指纹要缓存**（`RegionDecorCache::edges_fp`）：边数上万时逐帧
`4 × E` 次 `to_bits` 会把合批省下的钱又花回去。指纹只在 `DecorKey` 变（多边形 /
网格 / 幅面变了）时随 `edges` 一起重算。锚点抽稀后只有几百个，逐帧现算即可。

**踩到的坑（值得记）**：第一版 `apply_batch` 用 `Option` 表达三态，把「键没变」
错当成「本帧无内容」⇒ 隔帧 despawn 再重建。`steady_state_never_rebuilds`
数出「60 帧重建 31 次」才发现。**三态是必须的，两态在这里是错的。**

### 2.2 症状 2 —— 实心白色方块

- 尺寸：半边 = `HANDLE_HALF_PX`(3.5px) × 档中心 scale ⇒ 屏幕约 7px 见方；
  间距 24px 下互不粘连，比 1px 格线明显，又不压住它所指的格边。
- 颜色：纯白（`Color::WHITE`，不随主题——数据视图口径）。
- 一块 mesh 内每锚点 4 顶点 / 6 索引；选中锚点与**未物化候选**锚点同一套视觉。

### 2.3 症状 3 —— 两条手势（`crates/wv-app/src/edit/region_select.rs`）

| 手势 | 判据 | 边界 |
|---|---|---|
| **左键点在自然区内** | `just_pressed(Left)` ∧ 非 UI ∧ 非播放 ∧ `Topographic ∧ nature_overlay` ∧ `region_adjust` 未在拖锚点 | 只写 `WorldProjection.selection`（会话态，不落盘、不经 `EditCommand`）；`Area` 类**不**参与（CellMap 里它本来就不显示） |
| **右键单击** | `just_released(Right)` ∧ 按下到松手的**屏幕位移 < 4px** | 右键拖动 = 平移，不做任何选择操作；`ui.wants_pointer` 时不生效 |

嵌套区域命中取**外接框最小**的那个（点最里面）；`holes` 内的点不属于该区域。
两条系统的调度位置：`.after(drill_pick)`（它在空白单击时清选中）、
`.after(region_adjust_input)`（拽锚点优先），单独 `add_systems` 注册——
**主链是一整条 `.chain()`，混进 `.after()` 会让整条链的约束求不出来（实测编译失败）**。

---

## 三、判据（写断言前先过四条）

| 判据 | 落点 |
|---|---|
| 独立复算 | 期望值全是字面量：`256` 条边（64×64 格区域手算）、`(1,1)` 首帧重建、`0` 稳态增量、方块角点 `(7,17)/(13,17)/(13,23)/(7,23)`（中心 + 半边长 3 手算） |
| 误伤评估 | 关层仍**保留选中态**（`req_handle_plan_is_empty_when_overlay_off` 与 `neg_overlay_off_keeps_projection_selection` 仍绿）；锚点拖动不被选中系统抢走（判 `preview_polygon`/`drag_vertex`） |
| 负向断言 | 关层 ⇒ 装饰实体 **0**、mesh 资产 **0**、`handles_drawn`/`outline_segments_drawn` **0**；取消选中 ⇒ 装饰消失 |
| 一致 + 可达 | 同档 `scale × 1.02` ⇒ Δ0；跨 3 档 ⇒ **恰好 1** 次重建（证明键真的跟缩放走，不是恒不重建） |

**自验**：`first_frame_builds_edges_and_anchors` 是阳性对照（不建就跑不出「0 次重建」）；
`ctl_`/`req_` 一对都绿才算数。

---

## 四、验证命令

```bash
cargo check -p wv-render --features app --lib
cargo check -p wv-app   --features app
cargo test  -p wv-render --features app --lib region_decor        # 8 passed
cargo test  -p wv-app    --features app --lib region_select       # 7 passed
cargo test  -p wv-render --features app --test acceptance_nature_overlay   # 17 passed（回归）
SMOKE_SECS=6 bash scripts/smoke-gui.sh                            # 启动不崩
```

**人工复核帧率**（本轮未做，见 §六）：

```bash
WV_PERF=1 cargo run -p wv-app --features app
```

期望：开「显示自然区与候选」→ 点选一个自然区 → 日志里
`wv_perf window: … top5: … | sync_region_decor max=… avg=…`；
`sync_region_decor` 稳态 ms 应**与区域大小无关**（每帧只比对两个 u64），
且不再出现 `draw_map_gizmos` 因手柄/描边而变慢。

---

## 五、资产接入清单

**无新资产**（不加贴图 / 字体 / shader；方块与描边都是纯色 `ColorMaterial`）。

| 资产类型 | 来源 | 接入方式 | 验证点 |
|---|---|---|---|
| —（无） | — | — | — |

---

## 六、诚实标注（本轮**没**做到的）

1. **未做真人帧率实测**：本环境没有可交互的 GUI 会话，20 FPS → ? 的对比要用户
   用 `WV_PERF=1` 复核。已做的是「把每帧 N 条即时图元降到 2 个 draw call」，
   以及「稳态 60 帧零重建」的计数断言。
2. **未贴截图**：方块 7px、纯白、单 mesh 的行为由几何断言钉住（顶点/索引/角点），
   但「看起来对不对」仍需一眼。
3. **候选虚线高亮仍是逐条 gizmo**：`candidate_cache.outlines` 在
   `draw_map_gizmos` 里逐条 `line_2d`——候选多且环长时同样会慢，**本轮范围外**。
4. **`draw_region_decor` 的 `pairs`（多边形顶点缓存）** 仍保留在
   `RegionDecorCache` 里（抽稀输入），未画——不是新债，是旧字段。
