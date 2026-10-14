Set-Location 'd:\kudi-projects\worldview-rs'
$files = @(
    'app_bootstrap.rs',
    'colors.rs',
    'colors_google.rs',
    'colors_jrpg.rs',
    'ui_command_palette.rs',
    'world_projection.rs'
)
foreach ($name in $files) {
    $p = Join-Path 'crates\wv-app\src' $name
    if (Test-Path $p) {
        Remove-Item $p
        Write-Host ("deleted: " + $p)
    }
}
