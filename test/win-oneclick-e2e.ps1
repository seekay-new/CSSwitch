#requires -version 5.1
<#
.SYNOPSIS
  驱动「一键开始」的 Windows 端到端测试（真实代理 + 真实虚拟登录 + 真实 Science 启动）。

.DESCRIPTION
  跑 `platform_win_tests::one_click_login_starts_isolated_science_on_windows`：
    - 临时把 CSSwitch 配置换成指向本机 mock 上游的 custom profile（跑完原样恢复）；
    - 走真实 `one_click_login`：启动 Gateway → 伪造虚拟登录 → 原生启动 Science →
      健康检查 + 端口/PID/镜像身份校验 → stop 并确认端口关闭。
  不会调用任何真实 provider，不会写入 `%USERPROFILE%\.claude-science`。

.PARAMETER UpstreamPort
  mock 上游端口，默认 28990（代理用 +1，沙箱用 +2，需保持空闲）。
#>
[CmdletBinding()]
param(
    [int]$UpstreamPort = 28990
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

# 保证 cargo 可用：优先已有 PATH，其次用户级 rustup 安装位置。
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "找不到 cargo：请先安装 Rust（rustup，x86_64-pc-windows-gnu）"
}

# windows-gnu 需要 C 编译器（ring/rustls 的 C 源码）与 binutils。
if (-not (Get-Command gcc.exe -ErrorAction SilentlyContinue)) {
    foreach ($candidate in @(
            "C:\software\w64devkit\w64devkit\bin",
            "C:\msys64\mingw64\bin",
            "C:\mingw64\bin"
        )) {
        if (Test-Path (Join-Path $candidate "gcc.exe")) {
            $env:PATH = "$candidate;$env:PATH"
            break
        }
    }
}
if (-not (Get-Command gcc.exe -ErrorAction SilentlyContinue)) {
    throw "找不到 gcc.exe：windows-gnu 构建需要 MinGW-w64 工具链（详见 docs/operations/windows.md 第 1 节）"
}
$env:CC = "gcc"
$env:AR = "ar"

$mock = $null
$log = Join-Path $env:TEMP "csswitch-oneclick-upstream.jsonl"
Remove-Item -LiteralPath $log -ErrorAction SilentlyContinue

if (Get-Command python -ErrorAction SilentlyContinue) { $python = (Get-Command python).Source } else { throw "需要 python（mock 上游）" }

try {
    $mock = Start-Process -FilePath $python `
        -ArgumentList @((Join-Path $repo "test\win-e2e\mock_upstream.py"), "$UpstreamPort", $log) `
        -PassThru -WindowStyle Hidden
    $ready = $false
    foreach ($i in 1..40) {
        Start-Sleep -Milliseconds 200
        try { Invoke-WebRequest -Uri "http://127.0.0.1:$UpstreamPort/v1/models" -TimeoutSec 2 -UseBasicParsing | Out-Null; $ready = $true; break } catch {}
    }
    if (-not $ready) { throw "mock 上游未在 127.0.0.1:$UpstreamPort 就绪" }

    $env:CSSWITCH_WIN_E2E_UPSTREAM_PORT = "$UpstreamPort"
    Remove-Item Env:\CSSWITCH_GATEWAY_BIN -ErrorAction SilentlyContinue
    Write-Host "运行一键开始端到端测试（mock 上游 $UpstreamPort）…"
    Push-Location (Join-Path $repo "desktop\src-tauri")
    try {
        cargo test --lib one_click_login_starts_isolated_science_on_windows -- --ignored --nocapture
        $code = $LASTEXITCODE
    } finally {
        Pop-Location
        Remove-Item Env:\CSSWITCH_WIN_E2E_UPSTREAM_PORT -ErrorAction SilentlyContinue
    }

    Write-Host ""
    if (Test-Path -LiteralPath $log) {
        $hits = @(Get-Content -LiteralPath $log | Where-Object { $_ -match '/messages' })
        Write-Host ("上游收到推理请求：{0} 条" -f $hits.Count)
        if ($hits.Count -gt 0) { $hits | Select-Object -First 3 | ForEach-Object { Write-Host "  $_" } }
    }
    if ($code -ne 0) { throw "一键开始端到端测试失败（exit=$code）" }
    Write-Host "PASS：一键开始在 Windows 上跑通（代理→虚拟登录→启动→身份校验→停止）。"
} finally {
    if ($mock -and -not $mock.HasExited) { Stop-Process -Id $mock.Id -Force -ErrorAction SilentlyContinue }
}
