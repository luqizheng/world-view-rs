$f = 'd:\kudi-projects\worldview-rs\crates\wv-app\src\brush_cursor.rs'
$b = [System.IO.File]::ReadAllBytes($f)
Write-Host ("size = " + $b.Length)
$pos = 0
$line = 1
while ($pos -lt $b.Length) {
    if ($b[$pos] -eq 0x0A) {
        $line++
    }
    if ($line -ge 46 -and $line -le 62) {
        $show = if ($line -le 62) { 1 } else { 0 }
    }
    if ($line -ge 46 -and $line -le 62 -and $b[$pos] -lt 0x20 -and $b[$pos] -ne 0x0A) {
        Write-Host ("line {0} pos 0x{1:X4} byte 0x{2:X2}" -f $line, $pos, $b[$pos])
    }
    $pos++
}
