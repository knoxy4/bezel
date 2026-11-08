# S3 harness: runs spike_s3_appbars through every S3 check with no hands, and reads the work area from outside.
# Restarts Explorer (the taskbar blinks out, open File Explorer windows close) and resizes maximized windows while the
# bars are docked. Ask Knox before running it. Writes s3-harness.log (rects only) next to spike_s3.log.
#   tools\s3-harness.ps1 [-SkipExplorer]
param([switch]$SkipExplorer)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
$exe = Join-Path (Get-Location) 'target\debug\examples\spike_s3_appbars.exe'
if (-not (Test-Path $exe)) { Write-Host "build it first: cargo build -p bezel-os-win32 --example spike_s3_appbars"; exit 2 }
$log = Join-Path (Get-Location) 's3-harness.log'

function Say($m) { $l = "[{0:HH:mm:ss}] $m" -f (Get-Date); Write-Host $l; Add-Content -Path $log -Value $l }
function WorkArea { (& $exe --print-workarea).Trim() }
function Wait-Shell {
    $t = Get-Date
    while (((Get-Date) - $t).TotalSeconds -lt 20) {
        if (Get-Process explorer -ErrorAction SilentlyContinue) { Start-Sleep -Seconds 6; return $true }
        Start-Sleep -Milliseconds 500
    }
    Say "explorer did not come back by itself in 20 s; starting it"
    Start-Process explorer.exe
    Start-Sleep -Seconds 8
    return $false
}
function Restart-Explorer($why) {
    Say "restarting Explorer ($why)"
    Get-Process explorer -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 800
    [void](Wait-Shell)
}
function Check($name, $got, $want) {
    $ok = $got -eq $want
    Say ("{0} {1}: {2}" -f ($(if ($ok) { 'PASS' } else { 'FAIL' })), $name, $got)
    if (-not $ok) { Say "      expected: $want" }
    return $ok
}

Add-Content -Path $log -Value ("════ S3 harness run {0:yyyy-MM-dd HH:mm:ss} ════" -f (Get-Date))
$base = WorkArea
Say "baseline: $base"
# Expected while docked: the primary monitor loses 144 px on the left and 112 px at the bottom (100% scaling).
if ($base -notmatch 'm(\d)\* work=\((-?\d+),(-?\d+),(-?\d+),(-?\d+)\)') { Say "can't parse primary work area"; exit 2 }
$pm = $Matches
$wantPrimary = "m$($pm[1])* work=($([int]$pm[2] + 144),$($pm[3]),$($pm[4]),$([int]$pm[5] - 112))"
$docked = $base -replace [regex]::Escape("m$($pm[1])* work=($($pm[2]),$($pm[3]),$($pm[4]),$($pm[5]))"), $wantPrimary
$results = @()

# 1. Normal exit path: dock, then quit by itself; the work area must return to baseline.
Say "step 1: normal exit (--quit-after 4)"
$p = Start-Process -FilePath $exe -ArgumentList '--quit-after', '4' -PassThru -WindowStyle Minimized
Start-Sleep -Seconds 2
$results += Check 'docked work area' (WorkArea) $docked
$p | Wait-Process -Timeout 15
Start-Sleep -Seconds 1
$results += Check 'after normal exit' (WorkArea) $base

# 2. Explorer restart while docked: the bars must re-register.
$p = Start-Process -FilePath $exe -PassThru -WindowStyle Minimized
Start-Sleep -Seconds 2
$results += Check 'docked again' (WorkArea) $docked
if (-not $SkipExplorer) {
    Say "step 2: Explorer restart while docked"
    Restart-Explorer 'S3 re-register check'
    $results += Check 'docked after Explorer restart' (WorkArea) $docked
} else { Say "step 2: skipped (-SkipExplorer)" }

# 3. Force-kill: no cleanup runs. Does the reservation leak, and does the shell reclaim it by itself?
Say "step 3: force-kill"
Stop-Process -Id $p.Id -Force
$reclaimed = $false
for ($i = 1; $i -le 15; $i++) {
    Start-Sleep -Seconds 1
    if ((WorkArea) -eq $base) { $reclaimed = $true; Say "shell reclaimed the space $i s after the kill"; break }
}
$results += Check 'after force-kill (15 s)' (WorkArea) $base
if (-not $reclaimed) {
    Say "LEAK: the reservation outlived the process"
    if (-not $SkipExplorer) {
        Restart-Explorer 'clean up the leak'
        [void](Check 'after cleanup restart' (WorkArea) $base)
    } else { Say "run an Explorer restart to clear it" }
}

$final = WorkArea
[void](Check 'final == baseline' $final $base)
$fails = ($results | Where-Object { -not $_ }).Count
Say ("summary: {0}/{1} checks passed{2}" -f ($results.Count - $fails), $results.Count, $(if ($reclaimed) { '' } else { '; force-kill LEAKS until Explorer restarts' }))
exit $(if ($fails -eq 0) { 0 } else { 1 })
