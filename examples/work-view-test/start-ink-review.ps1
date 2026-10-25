#requires -Version 5.1
<#
.SYNOPSIS
水墨主题 GUI 验收一键启动（ART-INK-001 落地后）

.DESCRIPTION
启动 work-view-test 演示世界并切到「水墨」主题（ui-layout.yaml 已预置
map_theme: chinese_ink），窗口弹出即成功。首次编译约 1 分钟。

用法（任选其一）：
  1) 项目根目录执行：
     powershell -ExecutionPolicy Bypass -File examples\work-view-test\start-ink-review.ps1
  2) 资源管理器右键本文件 -> 「使用 PowerShell 运行」

背景：agent 环境内启动会被企业安全策略拦截（d3d12.dll 加载 OS Error 5），
故本脚本设计为在用户自己的桌面会话运行。
#>
$ErrorActionPreference = "Stop"

$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $repo

# ── 1) 校验主题预置 ──────────────────────────────────────────────
$layout = Join-Path $repo "examples\work-view-test\ui-layout.yaml"
$hit = Select-String -Path $layout -Pattern "^map_theme:" | Select-Object -First 1
Write-Host "[1/2] 主题设置: $($hit.Line.Trim())"
if ($hit.Line -notmatch "chinese_ink") {
    Write-Warning "map_theme 不是 chinese_ink——启动后请在命令面板手动切换「水墨」"
}

# ── 2) 验收检查单 + 启动 ─────────────────────────────────────────
Write-Host @"
[2/2] 正在启动水墨验收 GUI（首次编译约 1 分钟；窗口弹出即成功）

===== 验收检查单（方案 §11 + 2026-10-25 更新）=====
应看到：
  - 海面深浅墨阶（近岸展开、远海安静）+ 近岸三道青墨皴线 + 顿笔积墨环
  - 纸纹颗粒 / 墨晕 / 边界扰动（手绘感，非干净矢量）
  - 森林「苔点」：近看是点皴，远看融成淡墨林影
  - 焦浓重淡清墨阶标签：国名最深、地名次之、街巷最淡
  - 宣纸底 #E8E0CE，整体雅致灰调
不应看到：
  - 亮紫 POI、亮冷白雪、冷蓝雪影
  - 密匝匝的装饰符号（江南百景图式的满）
  - 格边笔直、无纸纹的「干净矢量」观感
  - 草原/沙漠/水域地界上的暖绿晕（2026-10-25 显式零规则已消除，
    如仍可见即为回归）
==================================================
"@

cargo run -p wv-app --features app --bin wv -- gui -m examples/work-view-test