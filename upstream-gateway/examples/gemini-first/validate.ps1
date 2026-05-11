param(
    [string]$Model = "gemini-2.5-pro",
    [string]$GatewayProxyBindAddr = "127.0.0.1:2080",
    [string]$GatewayAdminBindAddr = "127.0.0.1:2081",
    [string]$NyroProxyHost = "127.0.0.1",
    [int]$NyroProxyPort = 19530,
    [switch]$SkipStreamTest
)

$ErrorActionPreference = "Stop"

function Wait-HttpOk {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [int]$TimeoutSeconds = 60
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-WebRequest -UseBasicParsing -Uri $Url -TimeoutSec 5
            if ($response.StatusCode -ge 200 -and $response.StatusCode -lt 500) {
                return
            }
        } catch {
        }
        Start-Sleep -Milliseconds 500
    }

    throw "Timed out waiting for $Url"
}

function New-TempDir {
    $path = Join-Path ([System.IO.Path]::GetTempPath()) ("upstream-gateway-gemini-first-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $path | Out-Null
    return $path
}

function Wait-TcpPort {
    param(
        [Parameter(Mandatory = $true)][string]$Host,
        [Parameter(Mandatory = $true)][int]$Port,
        [int]$TimeoutSeconds = 60
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $client = $null
        try {
            $client = [System.Net.Sockets.TcpClient]::new()
            $async = $client.BeginConnect($Host, $Port, $null, $null)
            if ($async.AsyncWaitHandle.WaitOne(1000) -and $client.Connected) {
                $client.EndConnect($async)
                return
            }
        } catch {
        } finally {
            if ($client) {
                $client.Dispose()
            }
        }
        Start-Sleep -Milliseconds 250
    }

    throw "Timed out waiting for TCP ${Host}:${Port}"
}

if ([string]::IsNullOrWhiteSpace($env:GEMINI_API_KEY)) {
    throw "Set GEMINI_API_KEY before running this script."
}

$ExampleRoot = Resolve-Path (Join-Path $PSScriptRoot ".")
$GatewayRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$NyroRoot = Resolve-Path (Join-Path $GatewayRoot "..")
$TempDir = New-TempDir
$BootstrapPath = Join-Path $TempDir "bootstrap.json"
$StreamOutputPath = Join-Path $TempDir "stream.txt"

$bootstrap = Get-Content -Raw -Path (Join-Path $ExampleRoot "bootstrap.template.json") | ConvertFrom-Json
$bootstrap.providers[0].keys[0].api_key = $env:GEMINI_API_KEY
$bootstrap.providers[0].model_rules[0].model = $Model
$bootstrap.providers[0].model_rules[0].tokenizer_model = $Model
$bootstrap.providers[0].model_rules[1].tokenizer_model = $Model
$bootstrap | ConvertTo-Json -Depth 20 | Set-Content -Path $BootstrapPath -Encoding UTF8

$oldGatewayBind = $env:UPSTREAM_GATEWAY_BIND_ADDR
$oldGatewayProxyBind = $env:UPSTREAM_GATEWAY_PROXY_BIND_ADDR
$oldGatewayAdminBind = $env:UPSTREAM_GATEWAY_ADMIN_BIND_ADDR
$oldGatewayDb = $env:UPSTREAM_GATEWAY_DATABASE_URL
$oldGatewayBootstrap = $env:UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH

$env:UPSTREAM_GATEWAY_PROXY_BIND_ADDR = $GatewayProxyBindAddr
$env:UPSTREAM_GATEWAY_ADMIN_BIND_ADDR = $GatewayAdminBindAddr
Remove-Item Env:UPSTREAM_GATEWAY_BIND_ADDR -ErrorAction SilentlyContinue
$env:UPSTREAM_GATEWAY_DATABASE_URL = "sqlite://$($TempDir.Replace('\', '/'))/gateway.db"
$env:UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH = $BootstrapPath

$gatewayProcess = $null
$nyroProcess = $null

try {
    $gatewayProcess = Start-Process cargo `
        -ArgumentList @("run", "--bin", "upstream-gateway") `
        -WorkingDirectory $GatewayRoot `
        -WindowStyle Hidden `
        -PassThru

    Wait-HttpOk -Url "http://$GatewayProxyBindAddr/healthz"
    Wait-HttpOk -Url "http://$GatewayAdminBindAddr/admin/healthz"

    $nyroProcess = Start-Process cargo `
        -ArgumentList @(
            "run",
            "-p",
            "nyro-server",
            "--",
            "--config",
            (Join-Path $ExampleRoot "nyro.standalone.yaml")
        ) `
        -WorkingDirectory $NyroRoot `
        -WindowStyle Hidden `
        -PassThru

    Wait-TcpPort -Host $NyroProxyHost -Port $NyroProxyPort

    $requestBody = @{
        model = $Model
        max_tokens = 128
        messages = @(
            @{
                role = "user"
                content = "Please answer in one short sentence: what does upstream-gateway do?"
            }
        )
    } | ConvertTo-Json -Depth 10

    $nonStreamResponse = Invoke-RestMethod `
        -Uri "http://$NyroProxyHost`:$NyroProxyPort/v1/messages" `
        -Method Post `
        -Headers @{
            "content-type" = "application/json"
            "x-api-key" = "local-test"
        } `
        -Body $requestBody

    if (-not $nonStreamResponse.content) {
        throw "Nyro non-stream validation failed: missing content field"
    }

    if (-not $SkipStreamTest) {
        $streamBody = @{
            model = $Model
            max_tokens = 128
            stream = $true
            messages = @(
                @{
                    role = "user"
                    content = "Stream a short answer about upstream-gateway."
                }
            )
        } | ConvertTo-Json -Depth 10

        $curlArgs = @(
            "-sS",
            "-N",
            "http://$NyroProxyHost`:$NyroProxyPort/v1/messages",
            "-H",
            "content-type: application/json",
            "-H",
            "x-api-key: local-test",
            "-d",
            $streamBody
        )

        & curl.exe @curlArgs | Tee-Object -FilePath $StreamOutputPath | Out-Null
        $streamText = Get-Content -Raw -Path $StreamOutputPath
        if ($streamText -notmatch "event: message_stop") {
            throw "Nyro stream validation failed: missing message_stop event"
        }
    }

    $runtime = Invoke-RestMethod -Uri "http://$GatewayAdminBindAddr/admin/providers/gemini-prod/runtime"

    Write-Host ""
    Write-Host "Validation succeeded."
    Write-Host "Gateway runtime summary:"
    $runtime.summary | ConvertTo-Json -Depth 10
} finally {
    if ($gatewayProcess -and -not $gatewayProcess.HasExited) {
        Stop-Process -Id $gatewayProcess.Id -Force
    }
    if ($nyroProcess -and -not $nyroProcess.HasExited) {
        Stop-Process -Id $nyroProcess.Id -Force
    }

    if ($null -ne $oldGatewayBind) {
        $env:UPSTREAM_GATEWAY_BIND_ADDR = $oldGatewayBind
    } else {
        Remove-Item Env:UPSTREAM_GATEWAY_BIND_ADDR -ErrorAction SilentlyContinue
    }
    if ($null -ne $oldGatewayProxyBind) {
        $env:UPSTREAM_GATEWAY_PROXY_BIND_ADDR = $oldGatewayProxyBind
    } else {
        Remove-Item Env:UPSTREAM_GATEWAY_PROXY_BIND_ADDR -ErrorAction SilentlyContinue
    }
    if ($null -ne $oldGatewayAdminBind) {
        $env:UPSTREAM_GATEWAY_ADMIN_BIND_ADDR = $oldGatewayAdminBind
    } else {
        Remove-Item Env:UPSTREAM_GATEWAY_ADMIN_BIND_ADDR -ErrorAction SilentlyContinue
    }
    if ($null -ne $oldGatewayDb) {
        $env:UPSTREAM_GATEWAY_DATABASE_URL = $oldGatewayDb
    } else {
        Remove-Item Env:UPSTREAM_GATEWAY_DATABASE_URL -ErrorAction SilentlyContinue
    }
    if ($null -ne $oldGatewayBootstrap) {
        $env:UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH = $oldGatewayBootstrap
    } else {
        Remove-Item Env:UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH -ErrorAction SilentlyContinue
    }

    Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
