# 意图笔刷（智能地形笔刷）MVP 实施计划（用户已批准 · 按 worktree 实施）

## 目标

用户只给「意图 + 大致区域」（拖一条线 / 点一下），程序生成自然地形细节（山脊走向、高原平台、火山锥、湖盆、河谷链），消除现有硬圆笔刷的程序味。机制移植自 civ-atlas `.ref/civ-atlas/src/gen/terrainEdits.ts` 的算法思路与数值比例（AGPL 项目，只参考思路，代码全部重写为 Rust）。

**已拍板的架构**：意图 = 一次性 UI 动作 → `wv-terrain` 纯函数生成 → cell diff → `EditCommand` → `History`。意图不进 YAML、不新增实体、不做「完成」状态机。

## 实施方式（按用户指示）

在 **git worktree** 中实施：新分支 `feat/terrain-intent`，基于 `feat/ui-figma-refactor` HEAD（72bd00a，因 brush_panel.rs 在该分支有 193 行重构）。worktree 目录 `.worktrees/terrain-intent`（确认已 ignore）。分三个独立提交：wv-terrain → wv-edit → wv-app。

## 改动一：wv-terrain 新增 `src/intent.rs`（~650 行含测试）

纯函数、零依赖（复用 `generate.rs::Perlin`），不动现有 `edit.rs`。

**地基（私有）**：`ridge(x,y)` 山脊噪声（1-|fbm|）、`warp` 域扭曲、`PolylineField` 点到折线距离场（含沿线参数与总长）、`blur_region` box blur。

**公开 API**：

```rust
pub enum Intent {
    MountainRange { pts, radius, strength },  // 拖一条线
    Plateau { pts, radius, strength },
    Hills { pts, radius, strength },
    Volcano { center, radius, strength },     // 点一下
    Lake { center, radius, strength },        // 点一下
    River { pts, width, strength },           // 源头拖到河口
}
pub fn apply_intent(grid: &mut TerrainGrid, intent: &Intent, seed: u64, cell_size: f32) -> usize;
```

**六意图实现（数值换算成米）**：

| 意图 | 高度公式 | kind 写入 |
|---|---|---|
| 山脉 | `2400·s·taper(a)·(0.78+0.32·ridged(warped))·exp(-1.4t²)`，两头收尖 | 场强→mountain(6)；水→自动岛链 |
| 高原 | `h = max(h, 1200·s·(0.85+0.15·fbm))` 边缘收 | 场强→hill(5) |
| 丘陵 | `h += 300·s·fbm(warped)·falloff` | 超阈→hill(5) |
| 火山 | 锥 `2400·s·(1-t)^1.6` + 火山口凹陷 | 锥→mountain(6)；海里→岛 |
| 湖 | 纯拓扑：`target = rim−6−50·s·(1−t²)`，rim=碗外最低邻居 | 碗内→water_shallow(1) |
| 河 | 格链拓扑挖深：`h = max(1, min(h, 两岸最低−12·s, prev−0.4))` | 链上→water_shallow(1) |

确定性：噪声 = `Perlin::new(hash(seed, 用途))`；同 seed 同 intent 逐格相同。

**测试 8 项**（§7.2 手算期望值）：山脊起伏/收尖、火山锥手算、火山口、湖底低于 rim−6、河链单调、跨海成岛、seed 确定性、越界不 panic。

## 改动二：wv-edit `command.rs` 增 `IntentTerrainCommand`（~80 行）

镜像 PaintTerrainCommand 的 apply/revert（快照→apply_intent→diff→写回 / 按表回滚）。seed 构造时 `hash(map_id, intent 内容)` 并存入命令——redo 重放同 seed。

## 改动三：wv-app 接线（~150 行）

- `state.rs`：`BrushTool` 枚举 + `BrushState` 增 `tool`/`strength`
- `ui/brush_panel.rs`：工具行选择器；意图工具时藏 terrain_grid、显示强度滑条；交互提示
- `edit/editing.rs`：`commit_stroke_on_release` 按 tool 分支；意图构造 + `IntentTerrainCommand` 单命令事务；脏矩形 = pts bbox 外扩 2.6·radius

## 原则自检

不触及 P1–P7（不新增实体、不进 YAML、撤销走现有 History）；wv-terrain 保持零依赖；AGPL 红线——只移植思路不复制代码。

## 执行顺序

1. 建 worktree + cargo build 基线 + 测试基线
2. wv-terrain `intent.rs`（地基 + 6 函数 + 8 测试）→ 门槛绿 → 提交
3. wv-edit 命令 + 2 测试 → 门槛绿 → 提交
4. wv-app 三处接线 → 全门槛 + GUI 烟测 → 提交
5. 实机启动调数值（plan 中高度为初始默认，实机校准）