# ADR-0100: CellMap 底图转正「方格平涂」为默认分支

- 状态：已接受（2026-11-01，用户裁定「平涂转正」）
- 关联：ADR-0051 §5（取色语义被部分修订）、ADR-0054 D4（固定色真源沿用）、
  ADR-0088 Task 4（探针家族先例）、追记十七（SPEC，色带契约被修订）、
  README §12-59（本决策的待裁条目，随之收敛）、GLOSSARY「方格平涂」

## 处境

CellMap（Topographic 工作区）的底图长期由场管线 `BiomeRaw` 分支承担：
5 档按族**近似色**（P1 占位色板 `biome_raw_approx`，真色表一直没搬进 GPU）+
海拔色带（`elev_ramp_continuous`，强度 = `view.hypsometric_tint`）+ 三光晕渲
（`tri_scale_relief`）+ 海岸三段色带 + 近岸白滩 + 岸线墨线 + 水陆亚格过渡 +
域扭曲。追记十七把「方格边界 + 全程深浅色带」定为该分支的**数据视图契约**。

2026-11-01 用户提出：「cell-map 只显示格子的群系设置的颜色——定义群系是什么
颜色，它就是什么颜色；海岸线和大陆的过渡不加任何特效，就是一个个的格子。」
同日在 `feat/cellmap-flat-biome`（a5bc292）落地实验分支 `ViewBranch::CellFlat`
并实机 A/B（35% 全景 + 250% 近观，jrpg-world）：对照显示旧档的色带/晕渲/
白滩环让「格子 = 数据」的判读变差，近观尤其明显。用户拍板「平涂转正」。

## 决策

1. **Topographic 的默认取色分支 = `CellFlat`**（wv-render `field_branch_of`，
   唯一选择点）。逐格 = 冻结群系表真色（21 槽）+ 场景分区固定色（12 槽），
   经 uniform `biome_table: [Vec4; 33]` + `biome_none_fallback: [Vec4; 2]`
   （std140 **尾部追加**）进 shader；WGSL 在一切加工前**早退**——晕渲 /
   海拔色带 / 海岸三段色带 / 近岸白滩 / 岸线墨线 / 水陆亚格过渡 / 域扭曲
   全部天然豁免，attr 为 Nearest 采样，海岸与群系边界即**格子硬边**。
   NONE 格按水陆回退（`biome_raw::pick` 条款的 GPU 镜像），未知编号按陆面
   回退、不得越界。
2. **CPU/GPU 同一份色表真源**：uniform 装填直接引用 `biome_raw` 常量
   （不抄值）——「CellMap 颜色与主题无关」（ADR-0051 §5 核心约束）继续成立，
   且从「按族近似」升级为「逐编号精确」。
3. **旧 `BiomeRaw` 分支不删**：WGSL 分支与 `terrain_field_demo`（H 键切换）
   保留；主 App 侧降级为 **`WV_CELLRAW` 环境探针**的 A/B 对照窗，缺省不可达
   （沿 `WV_FIELD` 转正后保留显式钉死路径的先例）。转正前的触发探针
   `WV_CELLFLAT` 语义反转并更名。
4. **CPU 烘焙回退路径不跟进**：`field_render` 关闭时 Topographic 走 CPU
   管线，其 `ShadeMode::Topographic` 至今是 ADR-0039 PR-1 的**纸面占位**
   （cells.rs `shade_topographic`）——本决策不扩大范围去补它；该回退档的
   呈现欠账维持原状，另行立项。

## 理由

- **数据视图的诚实落在「真色直出」上**：编辑器的操作对象是格子，「每格 =
  自己定义的颜色、无任何加工」让视觉与数据一一对应；加工层（白滩环、海拔
  染色、浮雕）在编辑场景里是判读噪声（实机对照结论）。
- **海拔参照不丢**：ADR-0039/0056 的等高线叠加与 cell 高亮仍在
  （`ViewState.isolines`、海拔分层滑条），高度信息有独立通道，不需要靠
  底图变色承载。
- **GPU 平涂与 `biome_raw::pick` 语义同源**：色相不再有 CPU 近似与 GPU
  近似的两套漂移面。

## 代价

- **追记十七契约被修订**：「方格边界 + 全程深浅色带」中的色带条款不再属于
  CellMap 默认观感（方格边界条款保留——平涂的硬格边正是它的强化版）；
  本 ADR 即该修订的留痕，SPEC 追记不回改。
- **uniform 增 544 字节**（33+2 槽）；`FieldParams` 因 `[Vec4; 33]` 超出
  数组 `Default` 上限而**摘除 `Default` derive**（零初始化本就是非法态，
  `default_params` 是唯一合法构造入口）。
- 旧观感需要 `WV_CELLRAW=1` 重启才能看到（探针是启动常量，不进运行时
  开关/命令面板）。
- 格子硬边在远观（< 1 px/格）无 LOD 色域混合——attr 是 ID 纹理不能插值，
  与旧 BiomeRaw 同一约束，非本决策新增的损失。

## 被否决的备选

- **保持 BiomeRaw 默认、平涂仅作探针**（等用户慢慢适应）——被否：用户
  已实机对照并明确拍板转正；编辑器的底图就该像数据。
- **在 BiomeRaw 上逐层关加工**（WV_STRIP 扩位）——被否：位域契约明文
  「探针不得越支」（位域是 Themed 专属诊断通道）；且「逐层关」的终点
  仍带近似色与色带残留，永远到不了「真色直出」的干净语义。
- **平涂做成第六个 ViewMode**——被否：CellMap 是编辑工作台而非浏览档，
  这是同一工作区的呈现修正，开新 ViewMode 会让「Topographic」一词承担
  两套含义。

## 实施

- 分支 `feat/cellmap-flat-biome`：a5bc292（CellFlat 分支 + 真色表 +
  `WV_CELLFLAT` 探针）→ 本 ADR 对应的转正提交（`cell_flat` → `cell_raw`
  语义反转、默认分支切换、测试与留痕更新）。
- 门禁：改动面测试 / clippy（双 feature）/ fmt；实机 A/B 截图见
  worktree `.target/shot_{cellflat,biomeraw,flat_near,raw_near}.png`。
