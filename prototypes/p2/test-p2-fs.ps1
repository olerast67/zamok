# P2, способ B: что видит файловая система, когда Проводник показывает папку снаружи
# (миниатюры, область навигации) и когда в неё входят.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File test-p2-fs.ps1
$ErrorActionPreference = 'Stop'
$root = 'C:\dev\zamok\tests\tmp\p2b'
$exe = 'C:\dev\zamok\prototypes\p3\gatefs\target\release\p3-gatefs.exe'
$parent = Join-Path $root 'parent'; $box = Join-Path $parent 'Box'; $src = Join-Path $root 'src'
$trace = Join-Path $root 'trace.txt'
function WaitFor([scriptblock]$cond, [int]$sec = 10) {
    $deadline = (Get-Date).AddSeconds($sec)
    while (-not (& $cond)) { if ((Get-Date) -gt $deadline) { return $false }; Start-Sleep -Milliseconds 150 }
    $true
}
$shell = New-Object -ComObject Shell.Application
function Win([string]$path) { @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $path } catch { $false } }) | Select-Object -First 1 }
function Take([string]$label) {
    Start-Sleep -Seconds 4
    $lines = if (Test-Path $trace) { Get-Content $trace } else { @() }
    Remove-Item $trace -ErrorAction SilentlyContinue
    "== $label ($($lines.Count) events)"
    $lines | ForEach-Object { $p = $_ -split "`t"; "$($p[1])`t$($p[3])" } | Group-Object | Sort-Object Count -Descending | ForEach-Object { "  $($_.Count)x  $($_.Name)" }
}

Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $parent, $src | Out-Null
1..3 | ForEach-Object { Set-Content (Join-Path $src "photo$_.txt") "x" }
Set-Content (Join-Path $parent 'neighbour.txt') 'n'
Set-Content (Join-Path $root 'control.txt') 'locked'
$env:P3_CONTROL = Join-Path $root 'control.txt'; $env:P3_MOUNT = $box; $env:P3_TRACE = $trace; $env:P3_FIT = '0'
$fs = Start-Process $exe -ArgumentList "-p `"$src`" -m `"$box`"" -PassThru -WindowStyle Hidden
$null = WaitFor { Test-Path $box }
Start-Sleep 2
Take 'idle after mount (indexer, antivirus, nobody looking)'

try {
    Start-Process explorer.exe $parent
    $null = WaitFor { (Win $parent) -ne $null }
    $w = Win $parent
    Take 'parent folder opened in Explorer (default view)'
    try { $w.Document.CurrentViewMode = 1; $w.Document.IconSize = 256 } catch {}
    Take 'parent folder switched to extra large icons (folder thumbnails)'
    $w.Refresh()
    Take 'parent folder refreshed (F5)'
    $w.Quit()
    Start-Process explorer.exe $box
    Take 'opened the locked folder in a new Explorer window (like double-click)'
    $bw = Win $box
    Take 'locked folder window stays open'
    if ($bw) { $bw.Quit() }
    $null = $shell.Namespace($parent).ParseName('Box').Verbs()
    Take 'context menu built for the folder (right-click)'

} finally {
    Stop-Process $fs.Id -ErrorAction SilentlyContinue
}
