# CSSwitch 0.8.4 · Windows x64（非官方移植）

> 本包是 [SuperJJ007/CSSwitch](https://github.com/SuperJJ007/CSSwitch) 的**非官方 Windows 移植**，MIT，
> 原始版权归上游（见包内 `LICENSE` / `NOTICE.md`）。

## 适用环境

- Windows 10 1809+ / Windows 11，x64
- 需已安装 WebView2 Runtime（Windows 11 自带；Win10 一般随 Edge 自动安装）
- **需自备** Claude Science（本包不包含、不下载、不修改它）与你的模型 API Key

## 安装

1. 解压 `CSSwitch-0.8.4-win-x64.zip`，双击 `CSSwitch.exe`（免安装、无需管理员权限）；
2. 首次运行若被 SmartScreen 拦截：**更多信息 → 仍要运行**（本包未做代码签名）；
3. 「设置 → Claude Science 程序」选择你的 `claude-science.exe`
   （若用官方安装器装到 `%LOCALAPPDATA%\Programs\ClaudeScience\`，会自动发现）；
4. 「模型连接」新增配置并「设为当前」，回首页点「一键开始」。

## 这一版能做什么

- 一键开始：本地 Gateway → 虚拟登录 → 原生启动隔离 Science → 健康 + 监听 PID + 镜像路径
  三重身份校验 → 打开浏览器
- 官方 Claude 模式、停止全部 / 状态 / 重新打开
- 内置 8 家 Provider + 自定义 Anthropic Messages / OpenAI Chat / OpenAI Responses 端点，四角色模型映射
- 完全隔离：独立 `HOME`/`USERPROFILE`/`APPDATA`/`LOCALAPPDATA`/`TEMP` 与独立 data-dir，
  真实 `%USERPROFILE%\.claude-science` 不被读写
- Windows 沙箱自愈：`claude-science-mcp` 环境未就绪时自动 `sandbox doctor` 并补容器 SID 权限

## 不支持（Windows）

Skill 的 GitHub 安装与 OPERON attach/detach、复用系统 SSH 配置、多份历史 org 选择、
authority snapshot 回滚（改为启动 journal + 人工确认）、`scripts/doctor.sh` 诊断、
NSIS/MSI 安装器。完整矩阵见 README 的「功能现状（Windows x64）」。

## 验证记录

- `cargo test --lib`：131 passed / 7 ignored
- `test\win-e2e.ps1`：18/18（Gateway→上游→模型透传、Science 启动/健康/status、
  端口→PID→镜像身份、隔离、停止）
- `test\win-oneclick-e2e.ps1`：真实 `one_click_login` 跑通

未在本版执行：真实多家 Provider 的 live 推理、Skill/MCP 全链路、安装器（NSIS）。

## 校验

```
SHA-256: DA16AA664FF93CC54AD6DC6574512A05FA7D538796BE8B3B08D4D0910ACD8BFE
文件:    CSSwitch_0.8.4_x64-win-portable.zip (15.98 MB)
```

```powershell
Get-FileHash CSSwitch_0.8.4_x64-win-portable.zip -Algorithm SHA256
```

## 源码位置

Windows 改动在 `windows-x64` 分支；`main` 与上游同步。
构建与验收：`docs/operations/windows.md`；发布流程：`docs/operations/publish.md`。
