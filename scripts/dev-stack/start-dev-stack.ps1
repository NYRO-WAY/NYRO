param(
    [switch]$Rebuild
)

$ErrorActionPreference = "Stop"

$root = Split-Path (Split-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) -Parent) -Parent
$runDir = Join-Path $root "scripts\dev-stack\run"
$logDir = Join-Path $root "scripts\dev-stack\logs"
$dataDir = Join-Path $root "scripts\dev-stack\data"

New-Item -ItemType Directory -Force -Path $runDir, $logDir, $dataDir | Out-Null

function Test-ManagedProcessRunning {
    param([string]$Name)

    $pidPath = Join-Path $runDir "$Name.pid"
    if (-not (Test-Path $pidPath)) {
        return $false
    }

    $processId = (Get-Content $pidPath -Raw).Trim()
    if (-not $processId) {
        Remove-Item $pidPath -Force -ErrorAction SilentlyContinue
        return $false
    }

    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        Remove-Item $pidPath -Force -ErrorAction SilentlyContinue
        return $false
    }

    return $true
}

function Start-ManagedProcess {
    param(
        [string]$Name,
        [string]$Workdir,
        [string]$FilePath,
        [string[]]$ArgumentList = @(),
        [hashtable]$Environment = @{}
    )

    if (Test-ManagedProcessRunning -Name $Name) {
        Write-Output "$Name already running"
        return
    }

    $stdout = Join-Path $logDir "$Name.out.log"
    $stderr = Join-Path $logDir "$Name.err.log"
    $pidPath = Join-Path $runDir "$Name.pid"

    if (Test-Path $stdout) { Remove-Item $stdout -Force }
    if (Test-Path $stderr) { Remove-Item $stderr -Force }

    $originalEnv = @{}
    foreach ($entry in $Environment.GetEnumerator()) {
        $originalEnv[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key, "Process")
        [Environment]::SetEnvironmentVariable($entry.Key, [string]$entry.Value, "Process")
    }

    try {
        if ($ArgumentList.Count -gt 0) {
            $process = Start-Process `
                -FilePath $FilePath `
                -WorkingDirectory $Workdir `
                -ArgumentList $ArgumentList `
                -RedirectStandardOutput $stdout `
                -RedirectStandardError $stderr `
                -WindowStyle Hidden `
                -PassThru
        } else {
            $process = Start-Process `
                -FilePath $FilePath `
                -WorkingDirectory $Workdir `
                -RedirectStandardOutput $stdout `
                -RedirectStandardError $stderr `
                -WindowStyle Hidden `
                -PassThru
        }
    } finally {
        foreach ($entry in $originalEnv.GetEnumerator()) {
            [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process")
        }
    }

    Set-Content -Path $pidPath -Value $process.Id -Encoding ascii
    Write-Output ("started {0} (pid={1})" -f $Name, $process.Id)
}

function Build-IfNeeded {
    param(
        [string]$Workdir,
        [string]$BinaryPath,
        [string]$BuildCommand
    )

    if ($Rebuild -or -not (Test-Path $BinaryPath)) {
        Write-Output "building $BinaryPath"
        & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command $BuildCommand
        if ($LASTEXITCODE -ne 0) {
            throw "build failed for $BinaryPath"
        }
    }
}

$raceSyncScript = Join-Path $root "race-gateway\webui\sync-assets.ps1"
if (Test-Path $raceSyncScript) {
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $raceSyncScript
    if ($LASTEXITCODE -ne 0) {
        throw "failed to sync race-gateway admin assets"
    }
}

$nyroBinary = Join-Path $root "target\debug\nyro-server.exe"
$upstreamBinary = Join-Path $root "upstream-gateway\target\debug\upstream-gateway.exe"
$raceBinary = Join-Path $root "race-gateway\target\debug\race-gateway.exe"

Build-IfNeeded `
    -Workdir $root `
    -BinaryPath $nyroBinary `
    -BuildCommand "Set-Location '$root'; cargo build -p nyro-server"

Build-IfNeeded `
    -Workdir (Join-Path $root "upstream-gateway") `
    -BinaryPath $upstreamBinary `
    -BuildCommand "Set-Location '$root\upstream-gateway'; cargo build"

Build-IfNeeded `
    -Workdir (Join-Path $root "race-gateway") `
    -BinaryPath $raceBinary `
    -BuildCommand "Set-Location '$root\race-gateway'; cargo build"

$upstreamBootstrap = Join-Path $root "upstream-gateway\examples\gemini-first\bootstrap.local.json"
$raceBootstrap = Join-Path $root "race-gateway\examples\dev\bootstrap.local.json"
$nyroDataDir = Join-Path $dataDir "nyro-data"
$upstreamDb = Join-Path $dataDir "upstream-gateway.db"
$raceDb = Join-Path $dataDir "race-gateway.db"

New-Item -ItemType Directory -Force -Path $nyroDataDir | Out-Null

if (-not (Test-Path $upstreamBootstrap)) {
    throw "missing upstream bootstrap file: $upstreamBootstrap"
}

if (-not (Test-Path $raceBootstrap)) {
    throw "missing race bootstrap file: $raceBootstrap"
}

Start-ManagedProcess `
    -Name "upstream-gateway" `
    -Workdir (Join-Path $root "upstream-gateway") `
    -FilePath $upstreamBinary `
    -Environment @{
        UPSTREAM_GATEWAY_PROXY_BIND_ADDR = "127.0.0.1:2080"
        UPSTREAM_GATEWAY_ADMIN_BIND_ADDR = "127.0.0.1:2081"
        UPSTREAM_GATEWAY_DATABASE_URL = "sqlite:///$($upstreamDb -replace '\\','/')"
        UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH = $upstreamBootstrap
        UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS = "300"
    }
Start-Sleep -Seconds 2
Start-ManagedProcess `
    -Name "race-gateway" `
    -Workdir (Join-Path $root "race-gateway") `
    -FilePath $raceBinary `
    -Environment @{
        RACE_GATEWAY_PROXY_BIND_ADDR = "127.0.0.1:2090"
        RACE_GATEWAY_ADMIN_BIND_ADDR = "127.0.0.1:2091"
        RACE_GATEWAY_DATABASE_URL = "sqlite:///$($raceDb -replace '\\','/')"
        RACE_GATEWAY_BOOTSTRAP_JSON_PATH = $raceBootstrap
    }
Start-Sleep -Seconds 2
Start-ManagedProcess `
    -Name "nyro-server" `
    -Workdir $root `
    -FilePath $nyroBinary `
    -ArgumentList @(
        "--proxy-host", "127.0.0.1",
        "--proxy-port", "19530",
        "--admin-host", "127.0.0.1",
        "--admin-port", "19531",
        "--data-dir", $nyroDataDir
    )

Write-Output ""
Write-Output "expected endpoints:"
Write-Output "  nyro proxy  : http://127.0.0.1:19530/health"
Write-Output "  nyro admin  : http://127.0.0.1:19531/"
Write-Output "  upstream    : http://127.0.0.1:2080/healthz"
Write-Output "  upstream ui : http://127.0.0.1:2081/admin"
Write-Output "  race        : http://127.0.0.1:2090/healthz"
Write-Output "  race ui     : http://127.0.0.1:2091/admin"
