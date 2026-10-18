# 生物群系标记 M2：重生成（feat/marker-regen）

> 日期：2026-10-18 · 状态：**执行中**（三决策已由用户授权按推荐执行）
> 上游：ADR-0029（已接受）· spec `2026-10-18-world-from-zero-map-design.md` D9/D13/D14 + 追记二

## 已拍板决策（2026-10-18「你来决定」）

| # | 决策 | 内容 |
|---|---|---|
| D-A | **R2 替换** | 重生成 = 删 owner 旧 span、写入一条 `[当前日, None)`。不追加历史。理由：边界是「生成快照」（ADR-0029 D2）非领域状态区间；地形格不分区间版本化，保留旧 span 会在时间轴回拨时出现「旧边界盖新地形」的错位；R1 在回拨后重生成会产出 `to < from` 非法区间。P6 不受影响（约束的是领域状态区间，不是快照）。落盘：spec 追记三，不改 ADR 正文 |
| D-B | **Region 面板入口** | 「生物群系标记」折叠小节：列当前图 `biome` 非空 Place，每行名称 + 群系名 + [重生成边界] |
| D-C | **多块不做、洞做** | 多块（不连通）仍只取最大外环——io 层 place_geometry 文件键 `map__owner__from` 同 owner 同 from 两条 span 互相覆盖，多块需 io 改动，溢出本任务。洞（单外环 + 内岛）用既有 `GeometryData::PolygonWithHoles`，零 io 改动 |

顺带（默认执行）：创建命令 `PlaceBiomeMarkerCommand` 一并切带洞存储（M1 留有
`let _ = &region.holes;` 与「M2 扩洞支持」注释），两命令共用 `marker_geometry` 助手。

## 硬约束（来自任务交代 + AGENTS）

- wv-terrain 零第三方依赖（本任务不动 wv-terrain）；
- 格尺寸 = `marker_cell_size(world, map)` 从目标图 raster 推导
  （`extent / terrain.width`，镜像 `grid_sync.rs` 口径但纯从 World 数据算），
  **禁止**当前会话 `GridConfig`（ADR-0029 终审 C1 教训）；
- wv-edit 不依赖 bevy / 日志框架；
- 测试名完整英文短句、数值断言附实际值；clippy 退出码显式检查不接管道。

## 任务

| # | 内容 | 关键测试 |
|---|---|---|
| T1 | wv-edit `marker_cell_size` | extent/raster 推导 + 根图回退 + 无地形 None |
| T2 | 创建命令切 `marker_geometry`（带洞） | 生成/识别两分支存洞；单环回归 |
| T3 | wv-core `place_territory_at` → `TerritoryShape`（Solid/WithHoles） | 洞返回 + 既有用例改形保持绿 |
| T4 | `RegenerateBiomeMarkerCommand`：读已存 envelope 重跑 mark_region；识别分支格子逐位不动；R2 替换 span；锚点重拟合；单事务 revert | 笔刷后重生成收缩边界且地形不动；生成分支四者逐位还原；两次重生成 span 恒 1 条；洞存储；raster 推导格尺寸；缺 biome/envelope 报错且零改动 |
| T5 | wv-render：晕（earcut 复用 + UV 洞版）/ `forest_sites` 跳洞 | 洞内无三角形质心 / 洞内无采样点；单环回归 |
| T6 | wv-app `regenerate_marker_now` + Region 面板小节 + i18n | 动作提交 + 脏纹理；播放中拒绝；列表只列当前图标记 |
| T7 | spec 追记三 + GLOSSARY + 全量门禁（workspace 三条 + app 两条 + smoke-gui + §2.2 依赖断言） | — |
| T8 | 独立 reviewer 终审 → 修 Critical/Important → 本地 `--no-ff` 合 main（不推送） | — |

## 范围外

重圈改 envelope（D13 完整路径）、多块存储、RIVER 线状标记、笔刷自动联动边界（D2 已裁不自动）。
