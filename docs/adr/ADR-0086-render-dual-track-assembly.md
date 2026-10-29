# ADR-0086: 渲染系统双轨装配的分界收口

状态：已接受（2026-10-29）

## 处境

wv-render 的系统存在**两份装配清单**：

1. **编辑器路径**：`wv-app::app_bootstrap::schedules.rs` 系统直装（带 `perf_timed!`
   与精确排序），是唯一日常入口（`wv gui`）。
2. **游戏消费路径**：`wv_render::plugins` 三 Plugin（`MapRenderPlugin` /
   `EditorOverlayPlugin` / `MapShellPlugin`，ADR-0023 D5），被
   `wv-app/examples/game_consumer.rs` 装配——非 Bevy 原生宿主复用渲染层的出口。

编辑器**从不**装配三 Plugin。该双轨已两次酿成线上缺陷：`EditorOverlayPlugin`
注册的系统被误以为"已装"，ADR-0040 单格金色 outline 从未出现（2026-10-24，
schedules.rs 注释记载"二次踩"）。P0-4 排查再坐实三处**顺序约束漂移**——ADR-0073
的 `DrillLabelChain` 次序、cellmap 四系统的链序、gizmo 六系统的互斥定序，都只在
Plugin 路径存在（或两边都缺），编辑器路径帧间行为不定。

## 决策

**保留双轨，写死分界，不收编。** 具体四件事：

1. **编辑器侧补齐三处顺序约束**（`schedules.rs`）：
   `update_street_labels.after(DrillLabelChain)`；cellmap 四系统 `.chain()`；
   gizmo 六系统 `.chain()`。
2. **plugins.rs 头注释改写**：双轨表（路径→权威装配点→宿主）、"新系统两处都要
   装、顺序约束两处都要落"的纪律、历史事故引用。
3. **本 ADR** 记录处境与被否决的备选。
4. 不改 `game_consumer` 与三 Plugin 的注册行为。

## 理由

- 双轨的**成因是真实需求**：编辑器要 `perf_timed!` 与链式精确排序；游戏宿主要
  一个 `add_plugins` 就能复用的包。两者编排方式不同，硬合一处必然互相妥协。
- **收编**（让编辑器装 `EditorOverlayPlugin` 等）被否决：Plugin 内清单与
  编辑器主链差异大（`perf_timed!` 包裹、`.after(camera::track_cursor_world)` 等
  编辑器特有锚点），收编要重构全部排序且 GUI 行为无法在 CI 验证（smoke 只能
  证"启动不崩"），风险与收益不成比例。
- **删 Plugin**（只留编辑器路径）被否决：`game_consumer` 真实消费三 Plugin，
  删了等于砍掉"渲染层可被外部宿主复用"的架构出口（ADR-0023 D5 的立项目标）。
- 双轨的真实风险不是"存在两份清单"，而是**漂移不被看见**。把分界与同步纪律
  写进 plugins.rs 头注释（改清单必经之地）+ 修掉已知漂移，比强行单轨更稳。

## 代价

- 两份清单的同步仍靠**纪律**（注释 + code review），无编译期强制。缓解：
  plugins.rs 头注释给出具体核对动作（`grep schedules.rs`）。
- 编辑器与游戏路径的**顺序约束可能继续单边演化**。缓解：本 ADR 后，新排序
  约束的评审问题固定为"另一侧要不要跟"。
- 若未来游戏路径成为真实产品入口（而非 example），双轨应重新裁决为"单一
  装配函数 + 两个薄壳"（共享系统数组常量）——留作收敛条件，不在本轮。

## 被否决的备选

| 备选 | 否决理由 |
|---|---|
| A1 编辑器改装三 Plugin（收编） | 排序体系重建 + GUI 行为 CI 不可验证，回归风险大 |
| A2 删三 Plugin，只留编辑器 | 砍掉 ADR-0023 D5 的外部宿主复用出口，game_consumer 失效 |
| A3 共享"系统数组常量"两壳复用 | Plugin 的 in_set/after 修饰无法与编辑器 perf_timed! 包裹无损共享，牵动面大收益小 |

## 关联

- 修正对象：ADR-0023 D5（Plugin 拆分）的实施偏差——原文未料到双轨漂移。
- 缺口实例：ADR-0073（`DrillLabelChain` 次序）、ADR-0040/0052（cellmap 链序）。
