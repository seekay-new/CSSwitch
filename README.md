<p align="center">
  <img src="desktop/src-tauri/icons/icon.png" alt="CSSwitch" width="112">
</p>

<p align="center">
  <strong>README</strong><br>
  <a href="./README.md">🇨🇳 中文</a> ·
  <a href="./README.en.md">🇺🇸 English</a>
</p>

<p align="center">
  <a href="https://github.com/SuperJJ007/CSSwitch"><img src="https://img.shields.io/badge/GitHub-CSSwitch-181717?style=flat-square&amp;logo=github" alt="GitHub Repository"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/github/license/SuperJJ007/CSSwitch?style=flat-square&amp;logo=opensourceinitiative" alt="MIT License"></a>
  <a href="https://github.com/SuperJJ007/CSSwitch/releases/latest"><img src="https://img.shields.io/github/v/release/SuperJJ007/CSSwitch?display_name=tag&amp;style=flat-square" alt="Latest Release"></a>
  <img src="https://img.shields.io/badge/macOS-Apple%20Silicon-000000?style=flat-square&amp;logo=apple&amp;logoColor=white" alt="macOS Apple Silicon">
  <img src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&amp;logo=tauri&amp;logoColor=white" alt="Tauri 2">
</p>

<p align="center">
  <strong>更自由、流畅的研究体验。</strong>
</p>

<p align="center">
  CSSwitch 让 Claude Science 接入你自己的模型 API。<br>
  在主流 Provider、Codex 与自定义兼容端点之间自由切换。
</p>

> Linux x64 用户可以使用 [CSSwitch v0.8.1 Linux x64 预发布版](https://github.com/SuperJJ007/CSSwitch/releases/tag/v0.8.1-linux-x64)，提供 amd64 `.deb` 安装包。

<p align="center">
  <img src="docs/assets/csswitch-v0.8-ui-demo.gif" alt="CSSwitch v0.8 系列界面演示" width="942">
</p>

---

<p align="center">
  <a href="https://github.com/SuperJJ007/CSSwitch/releases/download/v0.8.3/CSSwitch_0.8.3_aarch64.dmg">下载 v0.8.3</a> ·
  <a href="#功能介绍">功能介绍</a> ·
  <a href="#安装与启动">安装与启动</a> ·
  <a href="#provider-与模型">Provider 与模型</a> ·
  <a href="#使用-codex">Codex</a> ·
  <a href="#skill">Skill</a> ·
  <a href="#mcp">MCP</a>
</p>

## 功能介绍

| 功能 | 当前状态 | 说明 |
| --- | --- | --- |
| Provider 与模型 | 已支持 | 连接内置 Provider、中转站和自定义兼容端点，自由填写并严格映射 Science 使用的模型。 |
| Codex | 实验功能 | 使用 CSSwitch 独立浏览器登录和账号动态模型目录；默认关闭，不读取或修改原生 `~/.codex` 登录。 |
| Skill | 已支持 | 查看当前 Science 组织中的 Skill，从本地包导入，或让 Agent 从准确的公开 GitHub URL 安装。 |
| MCP | 即将支持 | v0.8.3 尚未提供面向用户的通用 MCP 添加、配置和运行管理；后续版本会继续完善。 |

## 社区

<p align="center">
  <img src="docs/assets/wechat-group.jpg" alt="CSSwitch 社区微信群二维码" width="420">
</p>

## 安装与启动

需要一台 Apple Silicon Mac（或一台 Windows 10 1809+/11 x64），已安装的 [Claude Science](https://claude.com/download)，以及可用的第三方模型 API Key 或 Codex 账号。

### Windows x64

Windows 版以便携包分发，无需管理员权限：

1. 解压 `CSSwitch_0.8.4_x64-win-portable.zip`，双击 `CSSwitch.exe`。
2. 确认已安装 Claude Science（官方安装器会放到 `%LOCALAPPDATA%\Programs\ClaudeScience\`）；
   如果放在别处，到「设置 → Claude Science 程序」点「选择程序…」指定 `claude-science.exe`。
3. 「模型连接」新增配置并「设为当前」，回首页保持「第三方模型」模式点「一键开始」。

Windows 上与 macOS 的差异（Skill/MCP 管理、系统 SSH 复用暂不支持等）以及构建方式见
[Windows 支持](docs/operations/windows.md)。

### macOS

1. 下载 [`CSSwitch_0.8.3_aarch64.dmg`](https://github.com/SuperJJ007/CSSwitch/releases/download/v0.8.3/CSSwitch_0.8.3_aarch64.dmg)，打开后将 CSSwitch 拖入「应用程序」。
2. 确认电脑上已经安装 [Claude Science](https://claude.com/download)，然后打开 CSSwitch。
3. 首次打开如被 macOS 阻止，请在 Finder 中右键 CSSwitch，选择「打开」。

### 连接第三方模型

1. 进入「模型连接」，点击「新增配置」。
2. 选择一个内置 Provider；使用中转站或自建服务时，选择对应的兼容端点并填写 `base_url`。
3. 填写 API Key 和模型名称。默认模型必须填写；高质量、快速和 Fable 模型可以留空，也可以直接填写上游提供的精确模型 ID。
4. 保存配置并点击「设为当前」。
5. 回到首页，保持「第三方模型」模式，点击「一键开始」。
6. Science 打开后，在顶部模型选择器中选择需要的模型。CSSwitch 显示的是你配置的真实模型名。

需要回到原来的 Claude 账号和官方模型时，在首页切换到「官方 Claude」并打开 Science；CSSwitch 会先停止自己管理的第三方链路。

### 使用 Codex

1. 进入「设置」，打开「启用 Codex 实验入口」。
2. 点击「浏览器登录 Codex」，在系统浏览器完成 CSSwitch 自己的独立授权。
3. 登录成功后，Codex 配置会自动出现在「模型连接」中，但不会自动替换当前 Provider。
4. 将 Codex 配置「设为当前」，回到首页点击「一键开始」。
5. 在 Science 的模型选择器中选择 `Codex / …` 模型。模型列表来自当前账号的动态目录，CSSwitch 不会补造账号中不存在的模型。

### 安装与查看 Skill

1. 先通过「一键开始」启动隔离 Science，等待运行状态正常。
2. 打开「Skill & MCP」，点击「刷新」查看当前 Science 组织中发现的 Skill、来源和绑定状态。
3. 本地包可以点击「导入本地 Skill 包」，选择 `.zip` 或 `.skill`；公开 GitHub Skill 则在 Science 中把准确 URL 交给 Agent，由 CSSwitch connector 完成安装。
4. 页面显示“已绑定”后，仍建议在当前 Agent 会话调用对应的 `skill()`，确认它已经实际加载。

## Provider 与模型

- **内置 Provider：** DeepSeek、通义千问、智谱 GLM、小米 MiMo、硅基流动、Kimi、MiniMax、OpenRouter。
- **自定义端点：** Anthropic Messages、OpenAI Chat Completions 和 OpenAI Responses 兼容 API；模型名称可以直接填写，不依赖自动发现。
- **模型选择：** 普通配置可只填一个模型，也可以分别设置质量、均衡、快速和 Fable。Science 显示真实模型名，不使用 `default` 占位名称。
- **Codex：** 使用 CSSwitch 独立浏览器登录和动态账号模型目录；不读取或修改原生 `~/.codex` 登录。

不同 Provider 对工具调用、thinking、图片、长上下文和流式输出的支持并不相同。CSSwitch 会严格按当前配置映射模型，未知模型不会静默切换到别的模型。

## Skill

CSSwitch 当前聚焦于安全地把外部 Skill 接入隔离 Science，而不是再造一个 Skill 市场或通用 MCP 管理器。

- **查看现有 Skill：** 页面读取当前 Science 组织中真实存在的 Skill，展示来源、单 Skill / bundle 形态，以及 `attached`、`detached`、`unknown` 三种 OPERON 绑定状态。Science 未运行或身份无法确认时不会猜测结果。
- **导入本地包：** 通过系统文件选择器导入 `.zip` 或 `.skill`，自动识别单 Skill 与多 Skill bundle；前端不接收或提交本地文件路径。
- **从 GitHub 安装：** 可以让 Science Agent 通过 CSSwitch 的专用安装桥接安装准确的公开 GitHub 仓库、集合或 Skill 目录 URL。CSSwitch 宿主负责匿名下载、固定 commit、校验、提交和绑定，不使用 Science 或用户的 GitHub 凭证。
- **安全提交：** 安装前检查 archive 大小、文件数量、路径穿越、符号链接、特殊文件和名称冲突；提交过程保持原子性，同名或已被修改的内容不会被静默覆盖。
- **区分发现、绑定与加载：** 列表中“已绑定”只代表 OPERON 实时回读成功，不等于当前 Agent 会话已经加载。单 Skill 安装后仍应在 Science 中调用 `skill()` 做最终验证。
- **bundle 生命周期：** bundle 会保留 `_shared` 和支持资源；从任一成员发起卸载都必须先展示完整影响列表并由用户确认，随后才整包 detach 和隔离回收，不做成员级静默删除。

公开 GitHub Skill 安装在内部使用一个窄范围 connector，只负责安装、卸载和长任务状态查询；它是 Skill 安装链路的一部分，不代表 CSSwitch 已经支持通用 MCP。详细合同见[外部 Skill bridge](./docs/features/external-skill-bridge.md)。

## MCP

通用 MCP 支持正在开发中。当前 v0.8.3 还不能让用户在 CSSwitch 中添加、编辑或管理自己的 MCP server，也不应把 Skill 安装所用的内部 connector 当成完整 MCP 功能。后续支持范围以对应版本的 README 和更新日志为准。

## 安全与隔离

- 第三方模式使用独立 HOME、data-dir 和本地回环 Gateway，不读取或修改真实 Claude 登录与 Science 数据。
- API Key 保存在本机 `~/.csswitch/config.json`，文件权限为 `0600`；凭据不会写入日志。
- 官方 Claude 模式会停止第三方代理链路，再打开真实 Science。
- CSSwitch 不下载或自动升级 Claude Science；新启动会优先采用本机官方 updater 已下载并通过本地身份校验的 runtime snapshot，否则使用当前安装的官方 App。

## 功能现状（Windows x64）

Windows 版与 macOS 版共用同一套源码与提交历史，但**可用范围不同**。下面按“可验证程度”分三档，不把未验证的东西写成已支持。

### ✅ 已实现并实测验证

| 功能 | 说明 | 验证方式 |
| --- | --- | --- |
| 一键开始（第三方模型） | 本地 Gateway → 虚拟登录 → 原生启动隔离 Science → 健康 + 监听 PID + 镜像路径三重身份校验 → 打开浏览器 | `test\win-oneclick-e2e.ps1`（真实跑通） |
| 官方 Claude 模式 | 直接启动选定/已安装的 `claude-science.exe`，抹掉 `ANTHROPIC_*` | 手工 |
| 停止全部 / 状态 / 重新打开 | 先走 Science CLI，再只对已证明身份的 PID 发信号；端口真关闭才算成功 | `test\win-e2e.ps1`（18/18） |
| Provider 与模型 | 内置 DeepSeek / 通义千问 / 智谱 GLM / 小米 MiMo / 硅基流动 / Kimi / MiniMax / OpenRouter；自定义 Anthropic Messages、OpenAI Chat、OpenAI Responses 端点；质量/均衡/快速/Fable 角色映射 | `win-e2e.ps1` 验证 Gateway→上游→模型名透传；其余与 macOS 同源 |
| Claude Science 程序定位 | 自动发现（`%LOCALAPPDATA%\Programs\ClaudeScience` 等）+「设置 → Claude Science 程序」文件选择器（PE 格式/大小/`--version` 校验后才记住） | `cargo test --lib` + 手工 |
| 隔离沙箱 | 独立 `HOME`/`USERPROFILE`/`APPDATA`/`LOCALAPPDATA`/`TEMP` + 独立 data-dir；真实 `%USERPROFILE%\.claude-science` 不被读写 | `win-e2e.ps1` 断言前后文件数/字节数一致 |
| 虚拟登录（不碰真实账号） | 写入明文 `encryption.key` + `.oauth-tokens/*.enc`；Windows 版 Science 会自行迁移进 DPAPI 并保留明文 fallback | `platform_win_tests::virtual_login_forge_roundtrips_on_windows` |
| Windows 沙箱自愈 | 检测到 `claude-science-mcp` 未就绪时，自动跑 `claude-science sandbox doctor` 并给 conda 环境补容器 SID 读/执行权限 | `~/.csswitch/logs/sandbox-doctor.log` |
| 配置与迁移 | `%USERPROFILE%\.csswitch\config.json`（schema v4）；v1→v4 迁移；早期版本误写到“当前目录”的配置会自动搬过来 | `cargo test --lib` |
| 日志与错误可读性 | `logs\` 下 science / proxy / operation / sandbox-doctor；“打开日志”在 Windows 用资源管理器；预检失败会给出**原因 + 已检查路径** | 手工 |

### ⚠️ 代码与 macOS 同源，Windows 尚未真机验收

- **Codex 实验入口**：浏览器登录、账号动态模型目录、网络路线（默认关闭）。
- **Skill 页面读取 / 本地包导入**：`list_installed_skills`、`install_local_skill_package` 本身不含 POSIX 调用，但没有在 Windows 上跑完整场景。
- **模型目录实时发现**（`/v1/models`）与自定义端点探测。

### ❌ 未实现 / 明确不支持（Windows）

| 项 | 现状 |
| --- | --- |
| Skill 的 GitHub 安装与 OPERON attach/detach 全链路 | 不提供。依赖 POSIX `openat` 原子 store；相关内部模块（`skill_manager/*`）当前未接入构建 |
| 复用系统 SSH 配置 | 不支持。win32 的 Science 不读 `~/.ssh/config` 入口；打开开关会得到明确报错 |
| 多份历史 org 的选择恢复 | 不支持。会要求先清理沙箱 data-dir |
| authority snapshot 回滚 | 简化为一条启动 journal：`serve` 之后失败会报 `environment_uncertain` 并要求人工确认，不做“看起来干净”的回滚 |
| 诊断脚本 `scripts/doctor.sh` | macOS/Linux 专用（zsh + lsof/ps）；Windows 上按钮会提示不支持并指向日志目录 |
| 安装器（NSIS/MSI） | 未构建（需联网下载 NSIS）；便携包功能等价 |
| macOS 专属校验 | 不适用：Mach-O magic、`codesign`/Team ID 在 Windows 上没有等价物。Windows 用 **PE 格式 + 大小界 + SHA-256 快照绑定**，不声称 Authenticode 来源证明 |

> 细节、平台原语替换表与手工验收步骤见 [docs/operations/windows.md](docs/operations/windows.md)。

## 当前边界

- 公开桌面包：macOS Apple Silicon（dmg）与 Windows x64（便携 zip）。Windows 上 Skill/MCP 管理、系统 SSH 配置复用与多份历史 org 恢复暂不支持，会明确报错而不是降级静默执行。
- 第三方模式不提供 Anthropic 账号权限，托管 MCP、目录连接器和部分云端能力可能不可用。
- Codex 仍是默认关闭的实验能力，当前只支持单账号浏览器登录。
- Rust Gateway 已随应用打包，不需要单独安装 Python runtime。

升级、回滚和已知限制见[项目文档](./docs/README.md)。问题反馈请使用 [GitHub Issues](https://github.com/SuperJJ007/CSSwitch/issues)。

## 开发

```bash
cd desktop
npm install
npm run tauri dev
```

完整检查：

```bash
bash test/run_all.sh
```

[更新日志](./CHANGELOG.md) · [开发与测试](./docs/operations/development.md) · [发布证据](./docs/evidence/releases/README.md)
