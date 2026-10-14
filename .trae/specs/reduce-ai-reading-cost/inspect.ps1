$lines = [System.IO.File]::ReadAllLines('d:\kudi-projects\worldview-rs\crates\wv-app\src\brush_cursor.rs')
for ($i = 45; $i -le 62; $i++) {
    Write-Host ("L{0,3}: {1}" -f ($i + 1), $lines[$i])
}
