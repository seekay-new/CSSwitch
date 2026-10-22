#requires -version 5.1
<#
.SYNOPSIS
  打 Windows 便携版发布包（无需管理员权限、无需安装）。

.DESCRIPTION
  产出 `dist\CSSwitch_<version>_x64-win-portable\` 与同名 zip：
    CSSwitch.exe            Tauri 桌面程序
    WebView2Loader.dll      windows-gnu 构建动态链接的 WebView2 加载器
    csswitch-gateway.exe    Rust 本地 Gateway（sidecar，由 app 在同目录发现）
    scripts\，docs\         诊断脚本与文档
  另附 BUILD-INFO.txt 记录来源与构建环境。

.NOTES
  安装器（NSIS/MSI）需要 tauri-cli 与 NSIS 下载，见 docs/operations/windows.md。
#>
[CmdletBinding()]
param(
    [string]$OutRoot = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not $OutRoot) { $OutRoot = Join-Path $repo "dist" }

# 自己准备构建环境：不依赖调用者已经 dot-source 过某个脚本。
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "找不到 cargo：请先安装 Rust（rustup，x86_64-pc-windows-gnu），详见 docs/operations/windows.md"
}
if (-not (Get-Command gcc.exe -ErrorAction SilentlyContinue)) {
    $candidates = @()
    if ($env:CSSWITCH_MINGW_BIN) { $candidates += $env:CSSWITCH_MINGW_BIN }
    if ($env:W64DEVKIT_HOME) { $candidates += (Join-Path $env:W64DEVKIT_HOME 'w64devkit\bin'), (Join-Path $env:W64DEVKIT_HOME 'bin') }
    $candidates += @(
        (Join-Path $env:ProgramFiles 'w64devkit\w64devkit\bin'),
        (Join-Path $env:ProgramFiles 'w64devkit\bin'),
        'C:\msys64\mingw64\bin'
    )
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path (Join-Path $candidate 'gcc.exe'))) {
            $env:PATH = "$candidate;$env:PATH"
            break
        }
    }
}
if (-not (Get-Command gcc.exe -ErrorAction SilentlyContinue)) {
    throw "找不到 gcc.exe：windows-gnu 构建需要 MinGW-w64（可用环境变量 CSSWITCH_MINGW_BIN / W64DEVKIT_HOME 指向其 bin 目录）"
}
# 保持与已验证环境一致，并且让指纹稳定：cc/ring 的构建脚本会监听 CC/AR，
# 时而设置时而不设置会导致重复重建（增量缓存失效）。
$env:CC = "gcc"
$env:AR = "ar"

$tauriDir = Join-Path $repo "desktop\src-tauri"
$version = (Select-String -Path (Join-Path $tauriDir "tauri.conf.json") -Pattern '"version":\s*"([^"]+)"').Matches[0].Groups[1].Value
$targetProfile = "release"

if (-not $SkipBuild) {
    Write-Host "构建 Gateway 与桌面程序（release，windows-gnu）…"
    Push-Location (Join-Path $repo "desktop\gateway")
    try { cargo build --release; if ($LASTEXITCODE -ne 0) { throw "gateway 构建失败" } } finally { Pop-Location }
    Push-Location $tauriDir
    try { cargo build --release; if ($LASTEXITCODE -ne 0) { throw "桌面程序构建失败" } } finally { Pop-Location }
}

function Find-Artifact([string]$name) {
    $candidates = @(
        # 默认（无 workspace）：crate 自己的 target 目录
        (Join-Path $tauriDir "target\$targetProfile\$name"),
        (Join-Path $repo "desktop\gateway\target\$targetProfile\$name"),
        # 显式指定过 target-dir / workspace 根的情况
        (Join-Path $repo "target\$targetProfile\$name"),
        (Join-Path (Split-Path -Parent $repo) "target\$targetProfile\$name"),
        (Join-Path $tauriDir "binaries\$name")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) { return (Resolve-Path -LiteralPath $candidate).Path }
    }
    return $null
}

$appExe = Find-Artifact "desktop.exe"
$gatewayExe = Find-Artifact "csswitch-gateway.exe"
$loader = (Resolve-Path (Join-Path $tauriDir "resources\WebView2Loader.dll")).Path
foreach ($pair in @(@("desktop.exe", $appExe), @("csswitch-gateway.exe", $gatewayExe))) {
    if (-not $pair[1]) { throw "找不到构建产物 $($pair[0])；先运行不带 -SkipBuild 的打包脚本" }
}

$name = "CSSwitch_${version}_x64-win-portable"
$dest = Join-Path $OutRoot $name

# 安全护栏：绝不删掉用户数据。旧版本曾直接把整个输出目录 Remove-Item，
# 而用户双击便携包时 CSSwitch 会把数据写在同一目录的 `.csswitch\` 下。
$userData = @((Join-Path $dest ".csswitch"), (Join-Path $dest "sandbox"))
foreach ($path in $userData) {
    if (Test-Path -LiteralPath $path) {
        throw "输出目录里存在用户数据（$path）。请先备份并移走，再重新打包；脚本不会替你删除它。"
    }
}
if (Get-Process CSSwitch -ErrorAction SilentlyContinue | Where-Object { $_.Path -and ($_.Path -like "$dest\*") }) {
    throw "目标目录里的 CSSwitch 正在运行，无法替换可执行文件。先退出应用（托盘 → 退出）再重试；或者用 -OutRoot 输出到另一个目录。"
}
New-Item -ItemType Directory -Force -Path $dest | Out-Null

Copy-Item $appExe (Join-Path $dest "CSSwitch.exe")
Copy-Item $gatewayExe (Join-Path $dest "csswitch-gateway.exe")
Copy-Item $loader (Join-Path $dest "WebView2Loader.dll")
foreach ($extra in @("LICENSE", "NOTICE.md", "README.md", "CHANGELOG.md")) {
    $path = Join-Path $repo $extra
    if (Test-Path -LiteralPath $path) { Copy-Item $path (Join-Path $dest $extra) }
}
foreach ($dir in @("scripts", "docs")) {
    $path = Join-Path $repo $dir
    if (Test-Path -LiteralPath $path) {
        $target = Join-Path $dest $dir
        New-Item -ItemType Directory -Force -Path $target | Out-Null
        Copy-Item -Path (Join-Path $path "*") -Destination $target -Recurse -Force
    }
}
$info = @"
CSSwitch $version —— Windows x64 便携版
构建时间: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss K")
平台:     Windows ($([Environment]::OSVersion.VersionString))
工具链:   $(if (Get-Command rustc -ErrorAction SilentlyContinue) { (rustc -V) } else { "rustc 未在 PATH 中" })
目标:     x86_64-pc-windows-gnu（不依赖 MSVC 运行时；WebView2 Runtime 需系统已安装）

使用:
  1. 双击 CSSwitch.exe
  2. 首次运行到「设置 → Claude Science 程序」选择 claude-science.exe
     （或先把 Claude Science 安装到 %LOCALAPPDATA%\Programs\ClaudeScience\）
  3. 到「模型连接」新增配置并设为当前，然后点「一键开始」

数据位置:
  %USERPROFILE%\.csswitch\                 CSSwitch 配置 / 日志 / 隔离沙箱
  %USERPROFILE%\.csswitch\sandbox\home\   隔离 HOME（Science 的 data-dir 在其中的 .claude-science）

注意:
  配置始终在 %USERPROFILE%\.csswitch，不在本文件夹。
  直接从本文件夹双击运行不会污染发布包。
"@
[IO.File]::WriteAllText((Join-Path $dest "BUILD-INFO.txt"), $info, [Text.UTF8Encoding]::new($false))

$zip = Join-Path $OutRoot "$name.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $dest "*") -DestinationPath $zip -CompressionLevel Optimal

Write-Host ""
Write-Host "目录: $dest"
Write-Host "压缩: $zip"
Write-Host ("大小: {0:N1} MB" -f ((Get-Item $zip).Length / 1MB))
