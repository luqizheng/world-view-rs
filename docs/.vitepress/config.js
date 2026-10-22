import { defineConfig } from 'vitepress'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// 站点根 = docs/（srcDir 默认为项目根，即 .vitepress 的父目录）
const root = path.resolve(__dirname, '..')
const repoRoot = path.resolve(root, '..')
const GH_BLOB = 'https://github.com/luqizheng/world-view-rs/blob/main'

function readTitle(file) {
  const src = fs.readFileSync(file, 'utf8')
  const m = src.match(/^#\s+(.+)$/m)
  return (m?.[1] ?? path.basename(file, '.md')).replace(/[`*]/g, '').trim()
}

function filesIn(dir) {
  const abs = path.join(root, dir)
  if (!fs.existsSync(abs)) return []
  return fs.readdirSync(abs).filter((f) => f.endsWith('.md')).sort()
}

/** 规格文件名 -> 站内 slug：ART_STYLE_SPEC_v0.1 -> art-style-spec-v0-1 */
function slugify(name) {
  return name
    .replace(/\.md$/, '')
    .toLowerCase()
    .replace(/[_.]+/g, '-')
    .replace(/[^a-z0-9-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
}

// 顶层保留为独立页面的文档（其余大写规格文件统一投影到 /specs/）
const SPEC_KEEP = new Set(['index.md', 'pending.md', 'evolution.md', 'glossary.md'])

/** 根级规格：文档名（不含扩展名） -> /specs/<slug> */
const specSlugByStem = new Map()
for (const f of fs.readdirSync(root).filter((f) => f.endsWith('.md') && !SPEC_KEEP.has(f))) {
  specSlugByStem.set(f.replace(/\.md$/, ''), `/specs/${slugify(f)}`)
}

// ── ADR 状态卡（渲染期注入，不改原文）────────────────────────────────
// 状态取自原文首部「状态」行；主题域 / 修正关系取自人工维护的 adr-meta.tsv。
const THEME_SLUG = {
  数据模型与本体: 'ontology',
  地图层级与导航: 'map-hierarchy',
  渲染架构与主题: 'render-theme',
  地形与装饰: 'terrain-decor',
  编辑工具与交互: 'editing-tools',
  '呈现与 LOD': 'presentation-lod',
  外部数据: 'external-data',
  平台与基础设施: 'platform',
}

const ADR_META = new Map()
{
  const metaFile = path.join(__dirname, 'adr-meta.tsv')
  if (fs.existsSync(metaFile)) {
    for (const line of fs.readFileSync(metaFile, 'utf8').replace(/\r/g, '').split('\n')) {
      if (!line.trim()) continue
      const [key, theme, relation] = line.split('\t')
      if (key) {
        ADR_META.set(key.trim(), {
          theme: (theme || '').trim(),
          relation: (relation || '').trim(),
        })
      }
    }
  }
}

function adrStatusLine(file) {
  const head = fs.readFileSync(file, 'utf8').split('\n').slice(0, 14).join('\n')
  const m = head.match(/^\s*[-*>\s]*\*{0,2}状态\*{0,2}\s*[：:]\s*(.+)$/m)
  return m ? m[1].replace(/\*\*/g, '').trim() : ''
}

function adrBanner(relPath) {
  // 注意：rewrites 之后 relativePath 是重写后的小写路径，需回查真实文件名
  const m = relPath.match(/^adr\/(adr-[^/]+)\.md$/i)
  if (!m) return ''

  const key = filesIn('adr').find(
    (f) => f.replace(/\.md$/, '').toLowerCase() === m[1].toLowerCase(),
  )
  if (!key) return ''

  const file = path.join(root, 'adr', key)
  if (!fs.existsSync(file)) return ''

  const meta = ADR_META.get(key.replace(/\.md$/, ''))
  const status = adrStatusLine(file)
  const theme = meta?.theme || '平台与基础设施'
  const slug = THEME_SLUG[theme] || 'platform'
  const relation = meta?.relation || '未登记（新增 ADR 请同步 `.vitepress/adr-meta.tsv`）'
  const badge = /提议/.test(status) ? 'CAUTION' : /被取代|废弃/.test(status) ? 'WARNING' : 'NOTE'

  return [
    `> [!${badge}] 状态卡 · 取自原文`,
    `> **状态**：${status || '（未识别到状态行）'}`,
    `> **主题域**：${theme} · [现行口径汇总](/current/${slug})`,
    `> **修正关系**：${relation}`,
    '',
    '---',
    '',
    '',
  ].join('\n')
}

// ── 渲染期链接翻译 ────────────────────────────────────────────────────
// ADR 正文里的链接是「仓库文件系统视角」（../../crates/... 、ART_STYLE_SPEC_v0.1.md），
// 站点是「URL 视角」。此处只改渲染输入，**不改任何 ADR 原文**：
//   1) ADR 互链（含 ./adr/ 前缀错误、缺 .md 扩展名） -> /adr/<小写文件名>
//   2) 根级规格（含被误写进 ../crates/... 的情况）      -> /specs/<slug>
//   3) 仓库内真实文件                                  -> GitHub blob
//   4) 指向不存在源码的相对链接                         -> 退化为纯文本
function rewriteRepoLinks(src, relPath) {
  if (!relPath) return src

  // 文档在仓库中的所在目录（如 adr/ADR-0041-x.md -> docs/adr）
  // 注意：relativePath 在 Windows 下可能带反斜杠，先归一为 posix
  const rel = relPath.replace(/\\/g, '/')
  const baseInRepo = path.posix.join(path.basename(root), path.posix.dirname(rel))

  return src.replace(/\[([^\]\n]*)\]\(([^)\n]+)\)/g, (whole, label, href) => {
    if (/^(https?:|mailto:|tel:|#|data:)/i.test(href)) return whole

    const [rawPath, frag = ''] = href.split('#')
    const f = frag ? `#${frag}` : ''
    if (!rawPath) return whole

    // 站内绝对路径：只修历史遗留的双前缀 /adr/adr-adr-xxxx
    if (rawPath.startsWith('/')) {
      const fixed = rawPath.replace(/^\/adr\/adr-adr-/, '/adr/adr-')
      return fixed === rawPath ? whole : `[${label}](${fixed}${f})`
    }

    const stem = path.posix.basename(rawPath).replace(/\.md$/i, '')

    // 1) ADR 互链（含 ./adr/ 前缀错误、缺 .md 扩展名、只写编号）
    if (/^ADR-\d{4}$/i.test(stem)) {
      const hit = filesIn('adr').find((f) =>
        f.toUpperCase().startsWith(`${stem.toUpperCase()}-`),
      )
      if (hit) return `[${label}](/adr/${hit.replace(/\.md$/, '').toLowerCase()}${f})`
    }
    if (/^ADR-\d{4}/i.test(stem)) {
      const key = stem.toLowerCase()
      if (fs.existsSync(path.join(root, 'adr', `${key}.md`))) {
        return `[${label}](/adr/${key}${f})`
      }
    }

    // 2) 根级规格文件（含被误写成 ../../crates/.../SPEC_NAME 的情况）
    if (specSlugByStem.has(stem)) {
      return `[${label}](${specSlugByStem.get(stem)}${f})`
    }

    // 3) 解析为仓库内真实文件：多候选依次尝试（文档所在目录 / 仓库根 / docs 根）
    const stripped = rawPath.replace(/^(\.\/)+/, '')
    const cands = [
      path.posix.normalize(path.posix.join(baseInRepo, stripped)),
      path.posix.normalize(stripped),
      path.posix.normalize(path.posix.join('docs', stripped)),
    ]

    for (const cand of cands) {
      if (cand.startsWith('..')) continue
      for (const target of [cand, `${cand}.md`]) {
        if (!fs.existsSync(path.join(repoRoot, target))) continue
        // 仓库根规约：有站内投影页
        if (target === 'README.md' || target === 'AGENTS.md') {
          return `[${label}](/specs/${target.replace(/\.md$/, '').toLowerCase()}${f})`
        }
        // ADR 档案说明页
        if (target === 'docs/adr/README.md') return `[${label}](/adr/${f})`
        return `[${label}](${GH_BLOB}/${target}${f})`
      }
    }

    // 4) 指向不存在的源码路径：退化为纯文本，避免死链
    if (stripped.startsWith('crates/') || /[/\\]crates\//.test(stripped)) return label
    return whole
  })
}

// ── 动态 rewrites：真源文件名原样保留，仅映射对外 URL ──────────────────
// 1) docs/adr/ADR-0001-xxx.md  ->  /adr/adr-0001-xxx
// 2) docs/adr/README.md        ->  /adr/
// 3) docs/<SPEC>.md            ->  /specs/<slug>
const rewrites = {}

for (const f of filesIn('adr')) {
  if (f === 'README.md') {
    rewrites[`adr/${f}`] = 'adr/index.md'
  } else if (/^ADR-.*\.md$/.test(f)) {
    rewrites[`adr/${f}`] = `adr/${f.toLowerCase()}`
  }
}

for (const f of fs.readdirSync(root).filter((f) => f.endsWith('.md'))) {
  if (SPEC_KEEP.has(f)) continue
  rewrites[f] = `specs/${slugify(f)}.md`
}

function adrGroups() {
  const files = filesIn('adr').filter((f) => /^ADR-.*\.md$/.test(f))
  const groups = []
  const size = 10
  for (let i = 0; i < files.length; i += size) {
    const chunk = files.slice(i, i + size)
    groups.push({
      text: `ADR ${chunk[0].slice(4, 8)} – ${chunk[chunk.length - 1].slice(4, 8)}`,
      collapsed: true,
      items: chunk.map((f) => ({
        text: readTitle(path.join(root, 'adr', f)),
        link: `/adr/${f.replace(/\.md$/, '').toLowerCase()}`,
      })),
    })
  }
  return groups
}

function specGroup() {
  const items = [...specSlugByStem.entries()]
    .sort((a, b) => a[1].localeCompare(b[1]))
    .map(([stem, link]) => ({ text: readTitle(path.join(root, `${stem}.md`)), link }))
  items.push(
    { text: 'README（仓库根规约）', link: '/specs/readme' },
    { text: 'AGENTS（协作规约）', link: '/specs/agents' },
  )
  return items
}

function currentGroup() {
  return filesIn('current')
    .filter((f) => f !== 'index.md')
    .map((f) => ({
      text: readTitle(path.join(root, 'current', f)),
      link: `/current/${f.replace(/\.md$/, '')}`,
    }))
}

export default defineConfig({
  lang: 'zh-CN',
  title: 'World View · 当前最终决策',
  description:
    'World View 项目的现行设计口径与 ADR 原文档案，直接取自仓库 docs/ 真源。',
  cleanUrls: true,
  ignoreDeadLinks: false,
  rewrites,
  // 本机 safe-delete 钩子会让 Vite 的递归清空（emptyOutDir）失败，
  // 故关闭内置清空，改由 `npm run build` 前置的 scripts/clean-dist.mjs 逐文件清理。
  vite: {
    build: {
      emptyOutDir: false,
    },
  },
  srcExclude: [
    '.vite-docs.bak-20261022/**',
    '_legacy_reference/**',
    'superpowers/**',
    'reviews/**',
    'codex-assets/**',
    'theme/**',
  ],
  markdown: {
    config(md) {
      md.core.ruler.before('normalize', 'repo-links', (state) => {
        const rel = (state.env?.relativePath || '').replace(/\\/g, '/')
        state.src = rewriteRepoLinks(state.src, rel)
        state.src = adrBanner(rel) + state.src
      })
    },
  },
  themeConfig: {
    nav: [
      { text: '现行口径', link: '/current/' },
      { text: '原文档案', link: '/adr/' },
      { text: '提议区', link: '/proposed/' },
      { text: '待决问题', link: '/pending' },
      { text: '演化史', link: '/evolution' },
      { text: '术语表', link: '/glossary' },
      { text: '规格参考', link: '/specs/' },
    ],
    sidebar: {
      '/current/': [{ text: '现行口径总览', link: '/current/' }, ...currentGroup()],
      '/adr/': [{ text: '档案说明与状态总表', link: '/adr/' }, ...adrGroups()],
      '/proposed/': [{ text: '提议区', link: '/proposed/' }],
      '/specs/': [{ text: '规格清单', link: '/specs/' }, ...specGroup()],
    },
    search: { provider: 'local' },
    outline: { level: [2, 3], label: '本页目录' },
    docFooter: { prev: '上一页', next: '下一页' },
    darkModeSwitchLabel: '外观',
    sidebarMenuLabel: '目录',
    returnToTopLabel: '回到顶部',
    lastUpdated: false,
  },
})
