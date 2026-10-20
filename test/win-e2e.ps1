#requires -version 5.1
<#
.SYNOPSIS
  CSSwitch Windows 端到端验收：证明「Science → 本地 Gateway → 第三方上游」这条链路
  在本机真实跑通，并核对 Windows 侧替换掉的那些 POSIX 原语。

.DESCRIPTION
  覆盖点：
    1. Gateway 本体（Rust sidecar）在 Windows 上起来并通过 /health；
    2. 携带 path-secret 的 /v1/messages 被正确转发到上游，且模型名原样透传；
    3. 用 CSSwitch 真实的启动参数与隔离环境拉起 Claude Science（serve + 端口 + --no-auto-update）；
    4. Science 侧 /health、status --json 的 running/port/pid；
    5. 端口 → PID → 可执行文件身份（Windows 上没有 lsof，用 netstat + QueryFullProcessImageName）；
    6. 隔离生效：真实 %USERPROFILE%\.claude-science 不被写入；
    7. 停止：Science stop 后端口真的关闭。

  不覆盖：桌面 UI 的点击流程、虚拟登录 forge（由 `cargo test` 的
  platform_win_tests::virtual_login_forge_roundtrips_on_windows 覆盖）、Skill/MCP。

.PARAMETER ScienceExe
  Claude Science 可执行文件。默认按 CSSwitch 的发现顺序查找。

.PARAMETER GatewayBin
  csswitch-gateway.exe 路径。默认 target\release\csswitch-gateway.exe。

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File test\win-e2e.ps1
#>
[CmdletBinding()]
param(
    [string]$ScienceExe = "",
    [string]$GatewayBin = "",
    [string]$WorkRoot = ""
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not $GatewayBin) {
    $candidates = @(
        (Join-Path $repo "desktop\src-tauri\target\release\csswitch-gateway.exe"),
        (Join-Path $repo "desktop\gateway\target\release\csswitch-gateway.exe"),
        (Join-Path $repo "target\release\csswitch-gateway.exe"),
        (Join-Path (Split-Path -Parent $repo) "target\release\csswitch-gateway.exe")
    )
    $GatewayBin = ($candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1)
    if (-not $GatewayBin) { $GatewayBin = $candidates[0] }
}
if (-not $WorkRoot) { $WorkRoot = Join-Path $env:TEMP "csswitch-win-e2e" }

$results = New-Object System.Collections.Generic.List[object]
function Add-Result {
    param([string]$Name, [bool]$Ok, [string]$Detail = "")
    $results.Add([pscustomobject]@{ Check = $Name; Ok = $Ok; Detail = $Detail }) | Out-Null
    $mark = if ($Ok) { "PASS" } else { "FAIL" }
    Write-Host ("[{0}] {1} {2}" -f $mark, $Name, $(if ($Detail) { "— $Detail" } else { "" }))
}
function Fail([string]$Name, [string]$Detail) {
    Add-Result $Name $false $Detail
    Complete
}

# 调用外部 exe 并拿到 stdout/exit code，同时避开两个坑：
#   1. PowerShell 5.1 把 native stderr 当终止错误；
#   2. 用管道接 `cmd /c` 时，`--detached` 的 daemon 继承了管道句柄，
#      PowerShell 会一直等管道关闭（看起来就是“卡死”）。
# 所以输出一律落到文件，绝不走管道。
function Invoke-Native {
    param(
        [Parameter(Mandatory)][string]$Exe,
        [string[]]$Argv = @(),
        [string]$OutFile = "",
        [int]$TimeoutSeconds = 90
    )
    $quoted = ($Argv | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }) -join ' '
    $line = '"' + $Exe + '" ' + $quoted
    $stdout = if ($OutFile) { $OutFile } else { Join-Path $env:TEMP ("csswitch-native-" + [guid]::NewGuid().ToString("n") + ".out") }
    # 用 .cmd 包装文件执行：Start-Process 的 -ArgumentList 会把带引号的
    # 重定向字符串拼坏，直接写脚本最稳。
    $wrapper = Join-Path $env:TEMP ("csswitch-native-" + [guid]::NewGuid().ToString("n") + ".cmd")
    $body = "@echo off`r`n" + $line + ' > "' + $stdout + '" 2>&1' + "`r`n"
    Set-Content -LiteralPath $wrapper -Value $body -Encoding Default
    $process = Start-Process -FilePath "cmd.exe" -ArgumentList @("/c", $wrapper) -PassThru -WindowStyle Hidden
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        try { $process.Kill() } catch {}
        Remove-Item -LiteralPath $wrapper -Force -ErrorAction SilentlyContinue
        return @{ Exit = -1; Output = ""; TimedOut = $true }
    }
    $text = if (Test-Path -LiteralPath $stdout) { (Get-Content -LiteralPath $stdout -Raw) } else { "" }
    Remove-Item -LiteralPath $wrapper -Force -ErrorAction SilentlyContinue
    if (-not $OutFile -and $stdout) { Remove-Item -LiteralPath $stdout -Force -ErrorAction SilentlyContinue }
    return @{ Exit = $process.ExitCode; Output = [string]$text; TimedOut = $false }
}

function Complete {
    Write-Host ""
    $failed = @($results | Where-Object { -not $_.Ok })
    Write-Host ("==== 结果：{0} 项通过 / {1} 项失败 ====" -f ($results.Count - $failed.Count), $failed.Count)
    if ($failed.Count -gt 0) { exit 1 }
    exit 0
}

function Resolve-ScienceExe {
    param([string]$Explicit)
    $candidates = @()
    if ($Explicit) { $candidates += $Explicit }
    if ($env:SCIENCE_BIN) { $candidates += $env:SCIENCE_BIN }
    if ($env:LOCALAPPDATA) {
        $candidates += (Join-Path $env:LOCALAPPDATA "Programs\ClaudeScience\claude-science.exe")
        $candidates += (Join-Path $env:LOCALAPPDATA "Programs\Claude Science\claude-science.exe")
    }
    if ($env:USERPROFILE) {
        $candidates += (Join-Path $env:USERPROFILE ".claude-science\bin\claude-science.exe")
    }
    Get-ChildItem -Path $repo -Filter "Claude-Science-*.exe" -ErrorAction SilentlyContinue |
        ForEach-Object { $candidates += $_.FullName }
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { return (Resolve-Path -LiteralPath $candidate).Path }
    }
    return $null
}

# ---------- 0. 准备 ----------
if (-not (Test-Path -LiteralPath $GatewayBin)) { Write-Host "缺少 Gateway：$GatewayBin（先 cargo build --release）"; exit 2 }
$science = Resolve-ScienceExe $ScienceExe
if (-not $science) { Write-Host "未找到 Claude Science 可执行文件；用 -ScienceExe 指定"; exit 2 }
Write-Host "Science : $science"
Write-Host "Gateway : $GatewayBin"
Write-Host "WorkRoot: $WorkRoot"

$python = (Get-Command python -ErrorAction SilentlyContinue)
if (-not $python) { Write-Host "需要 python（用于 mock 上游）"; exit 2 }

$gatewayPort = 28991
$upstreamPort = 28990
$sciencePort = 28980
$previewPort = 28981
$secret = "win-e2e-secret"
$sandboxHome = Join-Path $WorkRoot "home"
$dataDir = Join-Path $sandboxHome ".claude-science"
$upstreamLog = Join-Path $WorkRoot "upstream.jsonl"

if (Test-Path -LiteralPath $WorkRoot) {
    # 先确保没有残留的 daemon 占着端口
    Invoke-Native -Exe $science -Argv @("stop", "--data-dir", $dataDir) | Out-Null
    Remove-Item -LiteralPath $WorkRoot -Recurse -Force -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $sandboxHome "AppData\Local\Temp") | Out-Null

$realData = if ($env:USERPROFILE) { Join-Path $env:USERPROFILE ".claude-science" } else { $null }
$realDataBefore = $null
if ($realData -and (Test-Path -LiteralPath $realData)) {
    $realDataBefore = (Get-ChildItem -LiteralPath $realData -Recurse -File -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum)
}

$mock = $null; $gw = $null
try {
    # ---------- 1. mock 上游 ----------
    $mockArgs = @((Join-Path $repo "test\win-e2e\mock_upstream.py"), "$upstreamPort", $upstreamLog)
    $mock = Start-Process -FilePath $python.Source -ArgumentList $mockArgs -PassThru -WindowStyle Hidden
    $upReady = $false
    foreach ($i in 1..40) {
        Start-Sleep -Milliseconds 200
        try { Invoke-WebRequest -Uri "http://127.0.0.1:$upstreamPort/v1/models" -TimeoutSec 2 -UseBasicParsing | Out-Null; $upReady = $true; break } catch {}
    }
    if (-not $upReady) { Fail "mock 上游就绪" "127.0.0.1:$upstreamPort 未响应" }
    Add-Result "mock 上游就绪" $true "127.0.0.1:$upstreamPort"

    # ---------- 2. gateway ----------
    $gwEnv = @{
        CSSWITCH_AUTH_TOKEN                 = $secret
        CSSWITCH_LAUNCH_ID                  = "win-e2e"
        CSSWITCH_RELAY_KEY                  = "win-e2e-key"
        CSSWITCH_RELAY_BASE_URL             = "http://127.0.0.1:$upstreamPort"
        CSSWITCH_TOOLUSE_SHIM               = "off"
        # relay 是“一个 adapter 多个 contract”的共享 adapter：必须显式钉住契约身份。
        CSSWITCH_PROVIDER_CONTRACT_ID       = "anthropic-relay"
        CSSWITCH_PROVIDER_CONTRACT_DIGEST   = "141a8f7a43055ab08ff820ab807139630989e631ff8dd58629eea588bdb1dc14"
    }
    # CSSwitch 桌面端会把自己的静态模型目录交给 Gateway；这里用最小合法载荷代替。
    # catalog_fp 必须等于 Gateway 自己算出的指纹（按其 fingerprint 规范现场算）。
    $catalog = @{
        schema_version      = 1
        adapter            = "relay"
        catalog_fp         = ("0" * 64)
        default_selector_id = "claude-win-e2e-model"
        routes              = @(@{
                selector_id    = "claude-win-e2e-model"
                display_name   = "claude-win-e2e-model"
                upstream_model = "claude-win-e2e-model"
                supports_tools = $true
                capabilities   = @{}
            })
        role_bindings       = @{
            sonnet = "claude-win-e2e-model"
            opus   = "claude-win-e2e-model"
            haiku  = "claude-win-e2e-model"
            fable  = "claude-win-e2e-model"
        }
        legacy_aliases      = @()
    }
    $sha = $null
    $catalogJson = $catalog | ConvertTo-Json -Depth 8 -Compress
    $catalogFile = Join-Path $WorkRoot "static-catalog.json"
    [IO.File]::WriteAllText($catalogFile, $catalogJson, [Text.UTF8Encoding]::new($false))
    $fpText = ((& $python.Source (Join-Path $repo "test\win-e2e\catalog_fp.py") $catalogFile) | Out-String).Trim()
    if ($fpText.Length -ne 64) { Fail "静态模型目录指纹计算" "catalog_fp.py 返回异常：$fpText" }
    $catalog.catalog_fp = $fpText
    $gwEnv["CSSWITCH_STATIC_MODEL_CATALOG_V1"] = ($catalog | ConvertTo-Json -Depth 8 -Compress)
    $gwLog = Join-Path $WorkRoot "gateway.log"
    $gwArgs = @("--provider", "relay", "--port", "$gatewayPort")
    if ($PSVersionTable.PSVersion.Major -ge 7) {
        $gw = Start-Process -FilePath $GatewayBin -ArgumentList $gwArgs -PassThru -WindowStyle Hidden `
            -RedirectStandardError $gwLog -RedirectStandardOutput "$gwLog.out" -Environment $gwEnv
    } else {
        # Windows PowerShell 5.1 的 Start-Process 没有 -Environment：临时设置再启动。
        $saved = @{}
        foreach ($k in $gwEnv.Keys) {
            $saved[$k] = [Environment]::GetEnvironmentVariable($k)
            [Environment]::SetEnvironmentVariable($k, $gwEnv[$k])
        }
        try {
            $gw = Start-Process -FilePath $GatewayBin -ArgumentList $gwArgs -PassThru -WindowStyle Hidden `
                -RedirectStandardError $gwLog -RedirectStandardOutput "$gwLog.out"
        } finally {
            foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
        }
    }
    $gwReady = $false
    foreach ($i in 1..50) {
        Start-Sleep -Milliseconds 200
        try {
            $health = Invoke-WebRequest -Uri "http://127.0.0.1:$gatewayPort/$secret/health" -TimeoutSec 2 -UseBasicParsing
            if ($health.StatusCode -eq 200) { $gwReady = $true; break }
        } catch {}
    }
    if (-not $gwReady) {
        $tail = if (Test-Path $gwLog) { (Get-Content $gwLog -Tail 5) -join " | " } else { "" }
        Fail "Gateway 就绪" "127.0.0.1:$gatewayPort/$secret/health 未响应；$tail"
    }
    Add-Result "Gateway 就绪（path-secret 生效）" $true "127.0.0.1:$gatewayPort/$secret/health"

    # 错 secret 必须被拒绝
    $rejected = $false
    try {
        $bad = Invoke-WebRequest -Uri "http://127.0.0.1:$gatewayPort/wrong-secret/health" -TimeoutSec 2 -UseBasicParsing
        $rejected = ($bad.StatusCode -ne 200)
    } catch { $rejected = $true }
    Add-Result "错误 path-secret 被拒绝" $rejected ""

    # ---------- 3. 推理转发 ----------
    $payload = @{
        model      = "claude-win-e2e-model"
        max_tokens = 16
        messages   = @(@{ role = "user"; content = "ping" })
    } | ConvertTo-Json -Depth 5 -Compress
    $resp = $null
    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$gatewayPort/$secret/v1/messages" -Method POST `
            -ContentType "application/json" -Body $payload -TimeoutSec 10 -UseBasicParsing
    } catch {
        Fail "Gateway 转发 /v1/messages" "$($_.Exception.Message)"
    }
    Add-Result "Gateway 转发 /v1/messages" ($resp.StatusCode -eq 200) "HTTP $($resp.StatusCode)"
    $body = $resp.Content | ConvertFrom-Json
    Add-Result "响应体来自 mock 上游" ($body.content[0].text -eq "ok-from-mock-upstream") $body.content[0].text

    $upstreamSaw = $false
    $modelForwarded = $false
    if (Test-Path -LiteralPath $upstreamLog) {
        foreach ($line in Get-Content -LiteralPath $upstreamLog) {
            $entry = $line | ConvertFrom-Json
            if ($entry.path -like "*/messages") { $upstreamSaw = $true; if ($entry.model -eq "claude-win-e2e-model") { $modelForwarded = $true } }
        }
    }
    Add-Result "上游收到 /v1/messages" $upstreamSaw ""
    Add-Result "模型名原样透传到上游" $modelForwarded "claude-win-e2e-model"

    # ---------- 4. 以 CSSwitch 的方式启动 Science ----------
    $savedEnv = @{}
    foreach ($pair in @(
            @("HOME", $sandboxHome),
            @("USERPROFILE", $sandboxHome),
            @("APPDATA", (Join-Path $sandboxHome "AppData\Roaming")),
            @("LOCALAPPDATA", (Join-Path $sandboxHome "AppData\Local")),
            @("TEMP", (Join-Path $sandboxHome "AppData\Local\Temp")),
            @("TMP", (Join-Path $sandboxHome "AppData\Local\Temp")),
            @("ANTHROPIC_BASE_URL", "http://127.0.0.1:$gatewayPort/$secret")
            # 不要把 HTTPS_PROXY 指向自己：win32 的 Science 会在启动自检里挂住。
            @("NO_PROXY", "127.0.0.1,localhost,::1")
        )) {
        $savedEnv[$pair[0]] = [Environment]::GetEnvironmentVariable($pair[0])
        [Environment]::SetEnvironmentVariable($pair[0], $pair[1])
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $sandboxHome "AppData\Roaming"), (Join-Path $sandboxHome "AppData\Local") | Out-Null
    $scienceLog = Join-Path $WorkRoot "science.log"
    try {
        $serve = Invoke-Native -Exe $science -OutFile $scienceLog -TimeoutSeconds 120 -Argv @(
            "serve", "--data-dir", $dataDir, "--host", "127.0.0.1",
            "--port", "$sciencePort", "--sandbox-port", "$previewPort",
            "--no-browser", "--no-auto-update", "--detached"
        )
        $serveExit = $serve.Exit
    } finally {
        foreach ($k in $savedEnv.Keys) { [Environment]::SetEnvironmentVariable($k, $savedEnv[$k]) }
    }
    Add-Result "Science serve 退出码为 0" ($serveExit -eq 0 -and -not $serve.TimedOut) "exit=$serveExit timeout=$($serve.TimedOut)"

    $scienceHealthy = $false
    foreach ($i in 1..40) {
        Start-Sleep -Milliseconds 200
        try {
            $h = Invoke-WebRequest -Uri "http://127.0.0.1:$sciencePort/health" -TimeoutSec 2 -UseBasicParsing
            if ($h.StatusCode -eq 200 -and ($h.Content | ConvertFrom-Json).status -eq "healthy") { $scienceHealthy = $true; break }
        } catch {}
    }
    if (-not $scienceHealthy) { Fail "Science /health" "127.0.0.1:$sciencePort 未就绪；尾部日志：$((Get-Content $scienceLog -Tail 5) -join ' | ')" }
    Add-Result "Science /health healthy" $true "127.0.0.1:$sciencePort"

    # ---------- 5. status / 身份 ----------
    $statusRaw = (Invoke-Native -Exe $science -Argv @("status", "--data-dir", $dataDir)).Output
    $status = $statusRaw | ConvertFrom-Json
    Add-Result "status.running=true" ($status.running -eq $true) ""
    Add-Result "status.port 匹配" ($status.port -eq $sciencePort) "$($status.port)"

    $listenerLine = (netstat -ano -p tcp | Select-String ":$sciencePort " | Select-String "LISTENING" | Select-Object -First 1)
    $listenerPid = $null
    if ($listenerLine) { $listenerPid = [int](($listenerLine.Line -split '\s+') | Where-Object { $_ } | Select-Object -Last 1) }
    Add-Result "端口 $sciencePort 有唯一监听 PID" ($null -ne $listenerPid) "pid=$listenerPid"
    if ($listenerPid) {
        Add-Result "status.pid == 监听 PID" ($status.pid -eq $listenerPid) "status=$($status.pid) netstat=$listenerPid"
        $procPath = (Get-Process -Id $listenerPid -ErrorAction SilentlyContinue).Path
        $expected = (Resolve-Path -LiteralPath $science).Path
        Add-Result "监听进程的可执行文件就是选定 Science" ($procPath -eq $expected) "$procPath"
    }

    # ---------- 6. 隔离 ----------
    Add-Result "隔离 data-dir 生成了账号/密钥状态" `
        ((Test-Path (Join-Path $dataDir ".oauth-tokens")) -and (Test-Path (Join-Path $dataDir "encryption.key"))) ""
    Add-Result "隔离 data-dir 生成了 DPAPI 控制状态" (Test-Path (Join-Path $dataDir "daemon-control.dpapi")) ""
    if ($realData -and (Test-Path -LiteralPath $realData)) {
        $realDataAfter = (Get-ChildItem -LiteralPath $realData -Recurse -File -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum)
        Add-Result "真实 %USERPROFILE%\.claude-science 未被写入" `
            ($realDataAfter.Count -eq $realDataBefore.Count -and $realDataAfter.Sum -eq $realDataBefore.Sum) `
            "before=$($realDataBefore.Count)/$($realDataBefore.Sum) after=$($realDataAfter.Count)/$($realDataAfter.Sum)"
    } else {
        Add-Result "真实 %USERPROFILE%\.claude-science 未被创建" (-not ($realData -and (Test-Path -LiteralPath $realData))) "$realData"
    }

    # ---------- 7. 停止 ----------
    Invoke-Native -Exe $science -Argv @("stop", "--data-dir", $dataDir) | Out-Null
    $stopped = $false
    foreach ($i in 1..50) {
        Start-Sleep -Milliseconds 200
        $still = netstat -ano -p tcp | Select-String ":$sciencePort " | Select-String "LISTENING"
        if (-not $still) { $stopped = $true; break }
    }
    Add-Result "Science stop 后端口真的关闭" $stopped "port=$sciencePort"
} finally {
    if ($gw -and -not $gw.HasExited) { Stop-Process -Id $gw.Id -Force -ErrorAction SilentlyContinue }
    if ($mock -and -not $mock.HasExited) { Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $dataDir) { Invoke-Native -Exe $science -Argv @("stop", "--data-dir", $dataDir) | Out-Null }
}

Complete
