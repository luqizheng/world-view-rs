---
name: final-decisions-vitepress
description: World View 项目「当前最终决策」VitePress 文档站的构建与维护工作流。当用户说"生成/刷新决策文档""决策站点""最终决策文档""现行口径视图""决策快照""改一下文档站""加个 ADR 页面""启动/构建文档站"，或需要排查文档站跳转/死链时使用。站点根即仓库 docs/ 目录，真源直出（ADR 与现行口径同为页面，不生成副本）。覆盖：新增 ADR/现行口径的处理方式 → 启动与构建 → 死链与内链校验 → 状态卡与链接翻译机制。
---

# 「当前最终决策」文档站工作流

> **工具无关**：本工作流只依赖 `node` / `npm` / 标准 shell，不调用任何特定 agent 的专有工具。
> 任何会读取 `.agents/skills/` 的客户端（Trae / OpenCode / WorkBuddy 等）均可直接执行。

## 站点定位

面向他人阅读的**最终决策视图**：把散落在 51 份 ADR 里的决策合并成「现行口径」，同时保留 ADR 原文可追溯。

**站点根 = 仓库 `docs/` 目录本身**。这是本工作流最重要的前提——**不再有内容副本**，`docs/adr/` 与 `docs/current/` 都是真源，二者同处一个 `srcDir` 内，互相跳转由 VitePress 原生解析，不需要任何生成脚本。

## 触发词

生成/刷新决策文档、决策站点、最终决策文档、现行口径视图、决策快照、启动文档站、文档站跳转不通

## 目录结构

```
docs/
├─ .vitepress/
│   ├─ config.js            # 站点配置（rewrites / 链接翻译 / 状态卡注入）
│   ├─ adr-meta.tsv          # 人工维护：ADR 主题域 + 修正关系
│   └─ tools/                # check-utf8.mjs / find-bad-utf8.mjs
├─ package.json              # npm run dev / build / preview
├─ scripts/clean-dist.mjs    # 构建前清理产物（见「坑 3」）
├─ index.md                  # 首页（全景表 + 读法）
├─ current/                  # ★ 现行口径（真源，9 篇）
├─ adr/                      # ★ ADR 原文（真源，51 份 + README 索引）
├─ proposed/                 # 提议区
├─ specs/                    # 规格索引页 + README/AGENTS 投影
├─ pending.md / evolution.md # 待决问题 / 演化史
└─ glossary.md               # GLOSSARY.md 的投影（根目录不在 srcDir 内）
```

**产物**：`docs/.vitepress/dist/`（gitignore，不入库）
**启动**：`cd docs && npm run dev` → `http://127.0.0.1:5173/`
**构建**：`cd docs && npm run build`

> ⚠️ 不要在 `docs/` 下另建内容目录。历史遗留的 `docs/vite-docs/`（含 `build.cjs`、`gen-adr.sh`、`gen-specs.sh`）已整体退役并删除。

## 三个构建期机制（改文档时无需关心，排查问题必须知道）

### 1. 动态 `rewrites` —— 真源文件名 ≠ URL

| 真源 | 对外 URL |
|---|---|
| `docs/adr/ADR-0001-faction-layer-model.md` | `/adr/adr-0001-faction-layer-model` |
| `docs/adr/README.md` | `/adr/` |
| `docs/ART_STYLE_SPEC_v0.1.md` | `/specs/art-style-spec-v0-1` |

URL 统一小写 + slug 化，因此 `current/` 里既有的小写链接不需要改动。

### 2. 渲染期链接翻译（`md.core.ruler.before('normalize')`）

ADR 正文里的链接是**仓库文件系统视角**，站点是 **URL 视角**。构建时自动分派，**ADR 原文零改动**：

| 原文写法 | 转换结果 |
|---|---|
| `ADR-0041-topographic-2-5d-vertex-z.md` / `ADR-0041` / `./adr/ADR-0041` | `/adr/adr-0041-…` |
| `ART_STYLE_SPEC_v0.1.md`（含误写成 `../../crates/.../SPEC`） | `/specs/art-style-spec-v0-1` |
| `../../crates/wv-render/src/recipe.rs`（文件真实存在） | GitHub blob 链接 |
| `../../AGENTS.md`、`../README.md` | `/specs/agents`、`/specs/readme` |
| 指向不存在的源码路径 | 退化为纯文本 |

### 3. 状态卡渲染期注入

每份 ADR 页顶部注入 `> [!NOTE] 状态卡`：**状态**取自原文首部「状态」行，**主题域 / 修正关系**取自 `.vitepress/adr-meta.tsv`。状态含「提议」→ CAUTION，含「被取代/废弃」→ WARNING，其余 NOTE。

## 三个致命坑（已踩过，勿重犯）

1. **`relativePath` 在 Windows 下带反斜杠**：任何 `path.posix.*` 处理前必须 `.replace(/\\/g, '/')`，否则路径解析全错、链接翻译静默失效。
2. **`rewrites` 之后 `relativePath` 是重写后的小写路径**（不是源文件名）。反查 ADR 真实文件时必须小写比对，否则状态卡静默不注入——构建仍成功，只是卡片没了。这类失败**不报错**，务必用验证步骤兜底。
3. **递归删除可能被拦截**：某些环境把 `fs.rmSync(recursive)` 重定向到回收站，回收站不可用时报错，导致 Vite 内置 `emptyOutDir` 必然失败。对策：`config.js` 设 `vite.build.emptyOutDir: false`，改由 `docs/scripts/clean-dist.mjs` 用 `unlink` + **非递归 `rmdir`** 自底向上清理，并挂到 `npm run build` 前置。只用 `rm -rf` 会踩同一个坑。

## 新增内容的正确姿势

| 要加什么 | 怎么做 | 是否需改 config |
|---|---|---|
| 新 ADR | 放 `docs/adr/ADR-00XX-名字.md`，并在 `.vitepress/adr-meta.tsv` 追加一行（主题域 + 修正关系） | 否 |
| 新增现行口径章节 | 放 `docs/current/xxx.md`，侧边栏自动收录 | 否 |
| 新规格文档 | 放 `docs/` 根（大写命名），自动投影到 `/specs/<slug>` | 否 |
| 新的顶层入口页 | 放 `docs/xxx.md`，并加进 `SPEC_KEEP` 与 nav | 是 |

**只写 `adr-meta.tsv` 就够**——忘记登记的 ADR 不会报错，状态卡会显示「未登记（新增 ADR 请同步 adr-meta.tsv）」，这是唯一的提示。

## 验证（每次改完必跑）

```bash
cd docs && npm run build
```

判定标准（任一不满足即为失败）：

| 项 | 期望 |
|---|---|
| 构建 | `build complete`，无 `Build failed` |
| 死链 | `ignoreDeadLinks: false` 下 **0 条** dead link |
| 页面数 | `dist/**/*.html` 约 87（51 ADR + 21 specs + 9 current + 7 其他） |
| 状态卡 | `grep -c '状态卡' dist/adr/adr-0001-*.html` ≥ 1 |

内链完整性独立扫描（不依赖 VitePress 自带检查）：

```bash
cd docs/.vitepress/dist && node -e "
const fs=require('fs'),path=require('path');
function walk(d,a=[]){for(const f of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,f.name);f.isDirectory()?walk(p,a):(f.name.endsWith('.html')&&a.push(p))}return a}
const files=walk('.');let bad=[],n=0;
for(const f of files){const h=fs.readFileSync(f,'utf8');
for(const m of h.matchAll(/href=\"([^\"]+)\"/g)){let u=m[1];
if(/^(https?:|mailto:|#|data:|javascript:)/.test(u)||u.startsWith('//'))continue;
u=u.split('#')[0].split('?')[0];if(!u)continue;
let t=u.startsWith('/')?path.join('.',u):path.resolve(path.dirname(f),u);
if(![t,t+'.html',path.join(t,'index.html')].some(c=>fs.existsSync(c)&&fs.statSync(c).isFile()))bad.push(f+' -> '+u);n++;}}
console.log('页面',files.length,'| 内链',n,'| 失效',bad.length);console.log(bad.slice(0,10).join('\n'));"
```

## 汇报格式

- 站点路径与启动命令
- 统计：现行 N / 提议 M / 修正链 J / 待决 P
- 构建结论：死链数、页面数、内链扫描结果
