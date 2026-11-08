# Play BEZEL on THOR under the gpu lease, the safe way: thor_lease `run` waits for the lease, ties it to itself, starts
# bezel-app in its own console, and releases when the app exits. The cop counts a `run`-started process as use, so the
# lease can't be reaped mid-session (phase-3 run 4 lost a plain `take` that way and kept drawing over a CAGED capture).
#   tools\run-bezel.ps1 [-Seconds 300] [-EstMin 6]
param([int]$Seconds = 300, [int]$EstMin = 6)
$repo = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $repo 'target\debug\bezel-app.exe'
if (-not (Test-Path $exe)) { Write-Host "build first: cargo build -p bezel-app"; exit 2 }
& C:\Users\KNX\CAGED\tools\thor_lease.ps1 run -By knox-bezel -Lease gpu -NoQuiet -Exe $exe -ArgList "--seconds $Seconds" -WorkDir $repo -For "BEZEL play session ($Seconds s)" -EstMin $EstMin
exit $LASTEXITCODE
