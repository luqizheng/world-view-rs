# 笔刷面板 3 段控件式工具选择（保留其他所有改动）

## 范围

仅 3 文件改动，纯 wv-app UI 层：

1. **`top_bar.rs` + 新文件 `widgets/segmented_control.rs`**：把 `top_bar` 私有 `segmented` 提取为公共抽象。`top_bar` 既有 2 处调用（Browse/Edit 段 + 主题段）改路径，视觉零变化。
2. **`ui/brush_panel.rs`**：工具区从「3 行 `tool_group_row`」改为「1 行 3 段控件 + 段下 1 个 `row_for(group)` 渲染该组瓷砖」。`BrushState` 加 `tool_group: BrushToolGroup`（默认 = `PaintKind.group()`）；段/瓷砖命令面板 3 路径都用同一 `sync_tool_group` 函数同步。

## 状态机

```
点段 [塑]  →  brush.tool_group = Shape
            →  brush.tool = SHAPE[0]   ← 同步避免段/瓷砖分裂
            →  brush.enabled = true     ← 选工具 = 激活（沿用提交 2 语义）
点瓷砖 [火山]→ brush.tool = Volcano
            → brush.tool_group = Place  ← 反向同步
            → brush.enabled = true
SelectBrushTool(4)  → brush.tool = Volcano; tool_group = Volcano.group()
                      enabled = true
```

## 新面板布局

```
┌ 地形设计器 ────────── [ACTIVE] ┐
│ [Brush] 真切换                │
│ 工具                          │
│ [涂] [塑] [放] ← 3 段,组 N 高亮│
│ 段下瓷砖（随 tool_group 渲染） │
│ 意图时：强度/半径/提示行        │
│ 涂抹时：地形材料 5×2 + 半径+软度│
│ ▸ 画路 · 0 段                  │
└──────────────────────────┘
```

## 保留不动

- 6 个新工具图标 + `ICON_*` 常量
- i18n 23 Key（`BrushGroupPaint/Shape/Place` 给段标签用，`BrushModeShort` 保留）
- `grid_lines` 搬 `ViewState` + 网格瓷砖上工具条
- `SelectBrushTool/ToggleGrid` 命令面板注册 + 分发器
- editing 分派闸 `is_intent()`（用 `group() != Paint` 不变）
- `BrushTool::SHAPE`/`PLACE` 常量（段下瓷砖行直接消费）

## 验收

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
cargo test -p wv-app --features app && cargo clippy -p wv-app --all-targets --features app -- -D warnings
bash scripts/smoke-gui.sh
```

WV_SHOT 探针复用截图配方验收：3 段控件高亮、段下瓷砖可见、跨段切工具状态机一致。

## 提交（2 个，分支 `feat/terrain-panel-groups` 上）

1. `wv-app: 笔刷面板 3 段控件式工具选择（涂/塑/放 + 段下瓷砖）`
2. `wv-app: 提取 widgets::segmented_control 公共抽象（顶栏 2 段复用）`