# Check that release binaries contain no local paths of the build machine: the home folder
# (C:\Users\<name>), the repository folder and the cargo registry. Searches ASCII and UTF-16.
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-paths.ps1 <file> [<file> ...]
# Without arguments checks core\target\release\zamok-core.exe, zamok-recover.exe, zamok_shell.dll.
# Exit code 1 if something is found.
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Files)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..").Path
if (-not $Files -or $Files.Count -eq 0) {
    $rel = "$repo\core\target\release"
    $Files = @("$rel\zamok-core.exe", "$rel\zamok-recover.exe", "$rel\zamok_shell.dll") | Where-Object { Test-Path $_ }
}
if (-not $Files -or $Files.Count -eq 0) { throw 'no files to check' }

# What must not appear. 'Users\' catches any home folder, also of another account.
$needles = New-Object System.Collections.Generic.List[string]
$needles.Add('Users\')
$needles.Add('Users/')
$needles.Add($repo)
$needles.Add($repo.Replace('\', '/'))
$needles.Add('.cargo\registry')
$needles.Add('.cargo/registry')
$needles.Add('.rustup\toolchains')
if ($env:USERNAME) { $needles.Add("\$env:USERNAME\") }

$bad = 0
foreach ($f in $Files) {
    $bytes = [IO.File]::ReadAllBytes((Resolve-Path $f).Path)
    # Latin-1 keeps one char per byte, so IndexOf works on raw bytes
    $ascii = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $wide = [Text.Encoding]::Unicode.GetString($bytes)
    $wide1 = if ($bytes.Length -gt 1) { [Text.Encoding]::Unicode.GetString($bytes, 1, $bytes.Length - 1) } else { '' }
    $found = @()
    foreach ($n in $needles) {
        foreach ($text in @($ascii, $wide, $wide1)) {
            $i = $text.IndexOf($n, [StringComparison]::OrdinalIgnoreCase)
            if ($i -ge 0) {
                $s = [math]::Max(0, $i - 40)
                $ctx = $text.Substring($s, [math]::Min(120, $text.Length - $s)) -replace '[^\x20-\x7E]', '.'
                $found += "  '$n': ...$ctx..."
                break
            }
        }
    }
    if ($found.Count) {
        $bad++
        Write-Host "FOUND in $f"
        $found | ForEach-Object { Write-Host $_ }
    } else {
        Write-Host "clean: $f"
    }
}
if ($bad) { exit 1 }
