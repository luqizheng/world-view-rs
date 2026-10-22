---
layout: home
hero:
  name: 当前最终决策
  text: World View 设计文档站
  tagline: 直接读取仓库 docs/ 真源 · 51 份 ADR 原文 + 9 篇现行口径 · 改文档即改站点
  actions:
    - theme: brand
      text: 现行口径总览
      link: /current/
    - theme: alt
      text: 决策演化史
      link: /evolution
    - theme: alt
      text: 原文档案
      link: /adr/
features:
  - title: 只看结果，不看过程
    details: 修正链（如日间色板 ADR-0016 → ADR-0035 → ADR-0042）已在「现行口径」合并为最终状态，每条附溯源链。
  - title: 现行与提议分离
    details: 提议态 ADR 单独成区——未获决策者确认前，它们不是本项目的现行约定。
  - title: 真源直出，不再同步副本
    details: 站点根就是 docs/ 本身。ADR 与现行口径同目录，互相跳转无需生成脚本；改完 Markdown 刷新即见。
---

## 全景表

| 主题域 | 现行决策 | 提议中 | 关键修正链（谁推翻了谁） |
|---|---|---|---|
| [数据模型与本体](/current/ontology) | 12 | — | ADR-0002 Q4 → ADR-0004（轨迹归单位）；ADR-0029 D5 → ADR-0032（词汇统一） |
| [地图层级与导航](/current/map-hierarchy) | 7 | — | ADR-0009 §2⑤ → ADR-0038（缩放换级 → 双击钻入） |
| [渲染架构与主题](/current/render-theme) | 12 | — | ADR-0008 §2④ → ADR-0012；ADR-0016 日间色 → ADR-0035 → ADR-0042；ADR-0021 D2 → ADR-0023 D4 |
| [地形与装饰](/current/terrain-decor) | 5 | 1 | ADR-0031 D2 → ADR-0036（Nation 从「不画」改为「画装饰带」） |
| [编辑工具与交互](/current/editing-tools) | 4 | — | ADR-0025 D1 → ADR-0033（意图生效后保留选区） |
| [呈现与 LOD](/current/presentation-lod) | 2 | 1 | ADR-0030 词表 → ADR-0047 扩表（非取代） |
| [外部数据](/current/external-data) | 2 | — | 无 |
| [平台与基础设施](/current/platform) | 3 | 1 | 无 |

## 怎么读这个站

| 分区 | 回答的问题 |
|---|---|
| [现行口径](/current/) | **现在到底是什么？**——修正链合并后的最终状态（本区是主视图） |
| [原文档案](/adr/) | 某条决策的原始正文、决策者与理由 |
| [提议区](/proposed/) | 哪些决策还没定稿（未获决策者确认） |
| [待决问题](/pending) | README §12 的开放问题清单（尚未收敛的事项） |
| [演化史](/evolution) | **某条决策被谁推翻了、为什么**——推翻链与理由 |
| [规格参考](/specs/) | 基线 / 美术 / 音频等规格文档（可能含历史小节） |
| [术语表](/glossary) | GLOSSARY.md 的站内投影 |

## 内容来源与构建

- **站点根 = `docs/`**。`docs/.vitepress/config.js` 中 `srcDir` 即 `docs/`，不另设内容副本。
- **ADR**：`docs/adr/ADR-*.md`（真源，51 份）。页面 URL 统一为小写 `/adr/adr-00xx-…`，由 `rewrites` 在构建期映射。
- **现行口径**：`docs/current/*.md`（真源，9 篇）。
- **规格**：`docs/` 顶层规格文档为真源，构建期投影到 `/specs/<slug>`。
- **ADR 状态卡**：状态取自原文「状态」行，主题域 / 修正关系取自 `.vitepress/adr-meta.tsv`，均在渲染期注入——**ADR 原文零改动**。
- **ADR 内指向源码的相对链接**（如 `../../crates/...`）在渲染期自动转换为 GitHub 仓库链接；指向本站页面的转换为站内 URL。
- 启动：在 `docs/` 下执行 `npm run dev`；构建 `npm run build`（产物 `docs/.vitepress/dist/`）。

> [!TIP] 单一权威源
> 本站不产出任何新的文档副本。任何冲突以仓库 `docs/adr/`、`docs/current/`、根 `README.md` 与 `GLOSSARY.md` 为准。
