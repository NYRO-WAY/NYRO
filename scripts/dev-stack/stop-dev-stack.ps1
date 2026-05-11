$ErrorActionPreference = "Stop"

$root = Split-Path (Split-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) -Parent) -Parent
$runDir = Join-Path $root "scripts\dev-stack\run"

function Stop-ManagedProcess {
    param([string]$Name)

    $pidPath = Join-Path $runDir "$Name.pid"
    if (-not (Test-Path $pidPath)) {
        Write-Output "$Name not running"
        return
    }

    $processId = (Get-Content $pidPath -Raw).Trim()
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if ($null -ne $process) {
        Stop-Process -Id $processId -Force
        Write-Output ("stopped {0} (pid={1})" -f $Name, $processId)
    } else {
        Write-Output "$Name already stopped"
    }

    Remove-Item $pidPath -Force -ErrorAction SilentlyContinue
}

Stop-ManagedProcess -Name "nyro-server"
Stop-ManagedProcess -Name "race-gateway"
Stop-ManagedProcess -Name "upstream-gateway"
