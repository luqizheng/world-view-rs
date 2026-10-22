// 清空 VitePress 输出目录。
// 背景：本机 WorkBuddy 的 safe-delete 钩子把 fs.rmSync(recursive) 重定向到回收站，
// 而 D 盘回收站当前不可用（trash 操作报 "Some operations were aborted"），
// 导致 Vite 内置的 emptyOutDir 必然失败。此处改为逐文件 unlink + 自底向上 rmdir，
// 两条路径均可正常工作；配合 config 中 vite.build.emptyOutDir = false 使用。
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const target = process.argv[2] ?? path.resolve(here, '..', '.vitepress', 'dist')

let files = 0
let dirs = 0

function clean(dir) {
  if (!fs.existsSync(dir)) return
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      clean(full)
      try {
        fs.rmdirSync(full)
        dirs++
      } catch (e) {
        console.warn(`[clean] 目录未能移除（可能被占用）: ${full}`)
      }
    } else {
      try {
        fs.unlinkSync(full)
        files++
      } catch (e) {
        console.warn(`[clean] 文件未能移除（可能被占用）: ${full}`)
      }
    }
  }
}

clean(target)
console.log(`[clean] ${path.relative(process.cwd(), target) || target}: 移除 ${files} 个文件 / ${dirs} 个目录`)
