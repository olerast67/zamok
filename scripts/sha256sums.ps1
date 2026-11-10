# Write SHA256SUMS.txt for release files, in the format of sha256sum from coreutils:
# "<lowercase hex>  <file name>" per line, LF line ends, sorted by name.
# Check on Windows: Get-FileHash <file> -Algorithm SHA256, compare with the line.
# Check in Git Bash, Linux or macOS: sha256sum -c SHA256SUMS.txt
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\sha256sums.ps1 -Dir installer\out -Files a.exe,b.msi
param(
    [Parameter(Mandatory = $true)][string]$Dir,
    [Parameter(Mandatory = $true)][string[]]$Files,
    [string]$Out = 'SHA256SUMS.txt'
)
$ErrorActionPreference = 'Stop'
# Through -File a list arrives as one string with commas
$Files = @($Files | ForEach-Object { $_ -split ',' } | Where-Object { $_ -ne '' } | Sort-Object)
$dirPath = (Resolve-Path $Dir).Path
$lines = foreach ($f in $Files) {
    $p = Join-Path $dirPath $f
    if (-not (Test-Path $p -PathType Leaf)) { throw "missing $p" }
    $h = (Get-FileHash $p -Algorithm SHA256).Hash.ToLowerInvariant()
    "$h  $f"
}
$text = ($lines -join "`n") + "`n"
$outPath = Join-Path $dirPath $Out
[IO.File]::WriteAllText($outPath, $text, (New-Object Text.UTF8Encoding $false))
Write-Host "Wrote $outPath"
Write-Host $text
