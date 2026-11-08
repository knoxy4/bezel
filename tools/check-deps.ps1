# Enforces the ADR-0001 §7 dependency rule. Exit 0 = pass, 1 = violation, 2 = tool error.
#   1. No windows* crate anywhere in the normal/build dependency tree of bezel-sim, bezel-layout or bezel-physics (all targets).
#   2. Only bezel-os-win32 and bezel-render-d2d depend directly on a windows* crate.
#   3. Workspace crates depend only on the workspace crates the layering allows (normal + build edges; dev edges are free).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
if (-not (Test-Path $cargo)) { Write-Host "check-deps: cargo not found"; exit 2 }

$failures = @()

# 1. Pure crates stay free of Win32 bindings.
$pure = @('bezel-sim', 'bezel-layout', 'bezel-physics')
$treeArgs = @('tree', '--edges', 'normal,build', '--target', 'all', '--prefix', 'none', '--format', '{p}')
foreach ($p in $pure) { $treeArgs += @('-p', $p) }
$tree = & $cargo @treeArgs 2>&1
if ($LASTEXITCODE -ne 0) { Write-Host "check-deps: cargo tree failed:`n$tree"; exit 2 }
$hits = $tree | Where-Object { $_ -match '^windows(-[a-z0-9_]+)?\s' } | Sort-Object -Unique
if ($hits) { $failures += "pure crates pull Win32 bindings: $($hits -join ', ')" }

# 2 + 3. Direct edges, from cargo metadata.
$metaJson = & $cargo metadata --format-version 1 --no-deps 2>$null
if ($LASTEXITCODE -ne 0) { Write-Host "check-deps: cargo metadata failed"; exit 2 }
$meta = $metaJson | ConvertFrom-Json

$windowsAllowed = @('bezel-os-win32', 'bezel-render-d2d')
$all = @('bezel-core', 'bezel-physics', 'bezel-layout', 'bezel-sim', 'bezel-render-api', 'bezel-render-d2d',
    'bezel-render-null', 'bezel-os-api', 'bezel-os-win32', 'bezel-os-fake', 'bezel-steam', 'bezel-ui-settings')
$allowed = @{
    'bezel-core'        = @()
    'bezel-physics'     = @('bezel-core')
    'bezel-layout'      = @('bezel-core')
    'bezel-sim'         = @('bezel-core', 'bezel-physics')
    'bezel-render-api'  = @('bezel-core')
    'bezel-render-d2d'  = @('bezel-core', 'bezel-render-api')
    'bezel-render-null' = @('bezel-core', 'bezel-render-api')
    'bezel-os-api'      = @('bezel-core')
    'bezel-os-win32'    = @('bezel-core', 'bezel-os-api')
    'bezel-os-fake'     = @('bezel-core', 'bezel-os-api')
    'bezel-steam'       = @('bezel-core')
    'bezel-ui-settings' = @('bezel-core', 'bezel-layout', 'bezel-os-api')
    'bezel-app'         = $all
    'bezel-simctl'      = $all
    'bezel-replay'      = $all
}

foreach ($pkg in $meta.packages) {
    if (-not $allowed.ContainsKey($pkg.name)) { $failures += "unknown workspace crate '$($pkg.name)': add it to tools/check-deps.ps1"; continue }
    foreach ($d in $pkg.dependencies) {
        $isDev = $d.kind -eq 'dev'
        if ($d.name -match '^windows(-[a-z0-9_]+)?$' -and -not $isDev -and $windowsAllowed -notcontains $pkg.name) {
            $failures += "$($pkg.name) depends directly on $($d.name) (only os-win32 and render-d2d may)"
        }
        if ($d.name -like 'bezel-*' -and -not $isDev -and $allowed[$pkg.name] -notcontains $d.name) {
            $failures += "$($pkg.name) -> $($d.name) breaks the layering rule"
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Host "check-deps: FAIL"
    $failures | ForEach-Object { Write-Host "  - $_" }
    exit 1
}
Write-Host "check-deps: pass ($($meta.packages.Count) crates; no windows* in sim/layout/physics trees)"
exit 0
