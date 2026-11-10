# Release build of the Rust programs, as reproducible as the toolchain allows (RELEASING.md):
# - toolchain pinned in core\rust-toolchain.toml, dependencies pinned with --locked;
# - local paths (repository, cargo registry, rustup) are replaced in panic messages and debug
#   info with /zamok, /cargo, /rustup (--remap-path-prefix);
# - the linker writes no timestamp (/Brepro) and only the PDB file name, not its full path
#   (/PDBALTPATH:%_PDB%);
# - SOURCE_DATE_EPOCH is the time of the last commit, for tools that read it.
# Results: core\target\release\zamok-core.exe, zamok-recover.exe, zamok_shell.dll
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-core-release.ps1
param([string[]]$Packages = @('zamok-core', 'zamok-shell', 'zamok-recover'))
$ErrorActionPreference = 'Stop'
$Packages = @($Packages | ForEach-Object { $_ -split ',' })
$repo = (Resolve-Path "$PSScriptRoot\..").Path

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe" }
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { "$env:USERPROFILE\.cargo" }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { "$env:USERPROFILE\.rustup" }

$epoch = & git -C $repo log -1 --format=%ct
if ($LASTEXITCODE -or -not $epoch) { throw 'git log failed' }
$env:SOURCE_DATE_EPOCH = $epoch.Trim()

# The environment variable replaces build.rustflags from core\.cargo\config.toml, so
# crt-static from there is repeated here. Separator of CARGO_ENCODED_RUSTFLAGS is 0x1F.
$flags = @(
    '-Ctarget-feature=+crt-static',
    "--remap-path-prefix=$repo=/zamok",
    "--remap-path-prefix=$cargoHome=/cargo",
    "--remap-path-prefix=$rustupHome=/rustup",
    '-Clink-arg=/Brepro',
    '-Clink-arg=/PDBALTPATH:%_PDB%'
)
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]0x1F

$pkgArgs = foreach ($p in $Packages) { '-p'; $p }
Push-Location "$repo\core"
try {
    & $cargo build -q --release --locked @pkgArgs
    if ($LASTEXITCODE) { throw 'cargo build failed' }
} finally {
    Pop-Location
    Remove-Item Env:CARGO_ENCODED_RUSTFLAGS
}
Write-Host "Built release of $($Packages -join ', '), SOURCE_DATE_EPOCH=$env:SOURCE_DATE_EPOCH"
