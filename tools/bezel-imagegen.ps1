# BEZEL art on the THOR pipeline (D28, D43): appends the custom BEZEL style (assets/art/bezel-style.txt, built on
# imagegen's illustration preset) and runs imagegen detached. The imagegen daemon takes the THOR gpu lease itself
# (engine/broker.py): never wrap this in thor_lease submit/take, or the daemon queues behind its own caller and deadlocks.
# Result JSON (paths, seed) lands in assets/art/jobs/<name>-<stamp>.json.
# Killing this client does NOT stop the batch: the daemon's worker keeps generating and holding the gpu lease. To stop a
# batch, end the worker.py processes by exact PID (thor_lease status lists them). Check the board before running.
#   tools\bezel-imagegen.ps1 -Name header -Prompt '<prose>' [-Size 1536x720] [-Count 8] [-Ref <png>] [-Seed <n>] [-Wait]
param(
    [Parameter(Mandatory)][string]$Name,
    [Parameter(Mandatory)][string]$Prompt,
    [string]$Size = '1024x1024',
    [int]$Count = 8,
    [string]$Ref,
    [int]$Seed,
    [switch]$Wait
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$style = (Get-Content (Join-Path $repo 'assets\art\bezel-style.txt') -Raw).Trim()
$full = "$($Prompt.Trim().TrimEnd('.')), $style"
$jobs = Join-Path $repo 'assets\art\jobs'
New-Item -ItemType Directory -Force $jobs | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$out = Join-Path $jobs "$Name-$stamp.json"

# Illustration preset's steps/guidance (imagegen engine/recipes.py) with our own suffix in place of its scaffold.
$a = @('X:\imagegen\src\imagegen\cli.py', '--json', '--style', 'raw', '--steps', '8', '--guidance', '1.2', '--size', $Size, '--count', "$Count")
if ($Ref) { $a += @('--ref', $Ref) }
if ($Seed) { $a += @('--seed', "$Seed") }
$a += $full
$quoted = $a | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }
$p = Start-Process -FilePath 'X:\imagegen\.venv\Scripts\python.exe' -ArgumentList $quoted -RedirectStandardOutput $out -RedirectStandardError "$out.err" -WindowStyle Hidden -PassThru
Write-Host "bezel-imagegen: $Name x$Count started (pid $($p.Id)); result -> $out"
if ($Wait) { $p | Wait-Process; Get-Content $out }
