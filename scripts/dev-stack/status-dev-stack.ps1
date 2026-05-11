$ErrorActionPreference = "Stop"

$root = Split-Path (Split-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) -Parent) -Parent
$runDir = Join-Path $root "scripts\dev-stack\run"
$logDir = Join-Path $root "scripts\dev-stack\logs"

function Show-ManagedProcess {
    param(
        [string]$Name,
        [string]$Url
    )

    $pidPath = Join-Path $runDir "$Name.pid"
    if (-not (Test-Path $pidPath)) {
        Write-Output ("{0,-18} stopped" -f $Name)
        return
    }

    $processId = (Get-Content $pidPath -Raw).Trim()
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        Remove-Item $pidPath -Force -ErrorAction SilentlyContinue
        Write-Output ("{0,-18} stopped" -f $Name)
        return
    }

    $health = "unknown"
    try {
        $response = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 3
        $health = "$($response.StatusCode)"
    } catch {
        $health = "unreachable"
    }

    Write-Output ("{0,-18} running  pid={1}  health={2}" -f $Name, $processId, $health)
}

Show-ManagedProcess -Name "upstream-gateway" -Url "http://127.0.0.1:2080/healthz"
Show-ManagedProcess -Name "race-gateway" -Url "http://127.0.0.1:2090/healthz"
Show-ManagedProcess -Name "nyro-server" -Url "http://127.0.0.1:19530/health"

Write-Output ""
Write-Output ("logs: {0}" -f $logDir)
