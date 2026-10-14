$srcDir = 'd:\kudi-projects\worldview-rs\crates\wv-app\src'
$skip = 'colors','colors_google','colors_jrpg','app_bootstrap','world_projection','ui_command_palette'
Get-ChildItem -Path (Join-Path $srcDir '*.rs') | ForEach-Object {
    $name = $_.BaseName
    if ($skip -contains $name) { Write-Host ("SKIP: " + $_.Name); return }
    $content = Get-Content -Raw $_.FullName
    $orig = $content
    $content = $content.Replace('crate::palette::', 'crate::colors::')
    $content = $content.Replace('use crate::palette;', 'use crate::colors;')
    $content = $content.Replace('use crate::palette_google', 'use crate::colors_google')
    $content = $content.Replace('crate::palette_google::', 'crate::colors_google::')
    $content = $content.Replace('palette_jrpg', 'colors_jrpg')
    $content = $content.Replace('crate::projection::', 'crate::world_projection::')
    $content = $content.Replace('use crate::projection::', 'use crate::world_projection::')
    $content = $content.Replace('crate::gui::', 'crate::app_bootstrap::')
    if ($content -ne $orig) {
        Set-Content -Path $_.FullName -Value $content -Encoding UTF8 -NoNewline
        Write-Host ("MODIFIED: " + $_.Name)
    } else {
        Write-Host ("unchanged: " + $_.Name)
    }
}
