# Windows 支持（x64）

CSSwitch 0.8.4 的 Windows 移植。目标只有一个：在 Windows 上也能用「一键开始」把
Claude Science 接到你自己的模型 API，并且保留 macOS 版那套 fail-closed 的身份校验。

- 支持平台：Windows 10 1809+ / Windows 11（x64），无需管理员权限
- 目标三元组：`x86_64-pc-windows-gnu`（不依赖 MSVC 运行时）
- 运行时前提：系统已安装 WebView2 Runtime（Win11 自带；Win10 由 Windows Update 分发）

---

## 1. 目录与交付物

```
dist\CSSwitch_0.8.4_x64-win-portable\
  CSSwitch.exe            Tauri 桌面程序
  csswitch-gateway.exe    Rust 本地 Gateway（sidecar）
  WebView2Loader.dll      windows-gnu 构建动态链接的 WebView2 加载器
  scripts\ docs\          诊断脚本与文档
```

重新生成：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
```

构建环境（全部用户级，无需管理员）：

```powershell
# 1) Rust（GNU 目标，不需要 Visual Studio Build Tools）
curl.exe -sSL -o "$env:TEMP\rustup-init.exe" https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe
& "$env:TEMP\rustup-init.exe" -y --profile minimal --default-toolchain stable-x86_64-pc-windows-gnu --no-modify-path

# 2) C 编译器 + binutils（ring/rustls 需要；提供 dlltool/ld/windres）
#    解压 w64devkit 到 C:\software\w64devkit，然后：
$env:PATH = "C:\software\w64devkit\w64devkit\bin;$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CC = "gcc"; $env:AR = "ar"
```

> 注意：`cargo:rustc-link-arg` 与 `windres` 是这两条"能编过"的关键，见第 3 节。

---

## 2. 首次使用

1. 双击 `CSSwitch.exe`。
2. 安装 Claude Science（任选一种）：
   - 官方安装：在下载的 `Claude-Science-<ver>.exe` 上运行 `claude-science install`，它会复制到
     `%LOCALAPPDATA%\Programs\ClaudeScience\claude-science.exe` 并注册卸载项；
   - 或者把它放在任意位置，然后在 CSSwitch 的「设置 → Claude Science 程序」里点「选择程序…」。
     CSSwitch 会校验 PE 格式、大小界和 `--version` 通过后才记住路径。
3. 「模型连接」新增配置（内置 Provider 或自定义兼容端点），保存后「设为当前」。
4. 回到首页，保持「第三方模型」模式，点「一键开始」。
5. Science 起来后在顶部模型选择器里选你要的模型名。

数据位置：

| 路径 | 内容 |
| --- | --- |
| `%USERPROFILE%\.csswitch\config.json` | 配置（文件权限收紧为只读除属主） |
| `%USERPROFILE%\.csswitch\science-bin.v1.json` | 用户指定的 Science 程序路径 |
| `%USERPROFILE%\.csswitch\logs\` | `science.log` / `operation.log` / `proxy.log` |
| `%USERPROFILE%\.csswitch\sandbox\home\` | 隔离 HOME（`HOME`/`USERPROFILE`/`APPDATA`/`LOCALAPPDATA`/`TEMP` 全部重定向到这里） |
| `%USERPROFILE%\.csswitch\sandbox\home\.claude-science\` | 隔离 Science data-dir（虚拟登录、org、skills、DPAPI 包装的密钥） |

真实 `%USERPROFILE%\.claude-science` 不会被读写；`test/win-e2e.ps1` 会把这一点作为断言检查。

---

## 3. 移植设计：哪些原语被替换了

macOS 版依赖一批 POSIX 原语。Windows 上用**语义等价**的实现替换，并保持"不满足就拒绝"
的形状，而不是降级成"能用就行"。

| 语义 | macOS / Linux | Windows 实现 |
| --- | --- | --- |
| 当前用户身份 | `geteuid()` | 当前进程 token 的 user SID → FNV-1a 折叠成稳定 `u32`（`platform::current_user_id`） |
| 目录/文件所有者 | `st_uid` | `GetNamedSecurityInfoW` 取 owner SID 与当前用户比对（`platform::verify_private_path`） |
| 文件身份绑定 | `(st_dev, st_ino)` | `GetFileInformationByHandle` → `(dwVolumeSerialNumber, nFileIndex, nNumberOfLinks)`（`platform::file_identity`） |
| 权限位 `mode()` | `st_mode & 0o777` | 由只读属性 + 文件类型推导（目录 `0o700`、文件 `0o600`）；`0o022` 恒为 0，真实判定走 ACL owner + reparse |
| 可执行判定 | `mode & 0o111` | 扩展名 `.exe/.com/.cmd/.bat` |
| 不跟随链接打开 | `O_NOFOLLOW` | `FILE_FLAG_OPEN_REPARSE_POINT` |
| 识别 junction/symlink | `FileTypeExt::is_symlink` | `FileType::is_symlink()`（同时覆盖 `IO_REPARSE_TAG_SYMLINK` 与 `IO_REPARSE_TAG_MOUNT_POINT`） |
| 监听端口 → PID | `lsof -t -iTCP:port -sTCP:LISTEN` | `netstat -ano -p tcp`，用「对端恒为 `0.0.0.0:0`」识别 LISTENING（与系统语言无关） |
| 进程启动身份 | `ps -o lstart=` | `GetProcessTimes` 的创建时间（100ns）→ `win32-created:<ticks>` |
| 进程可执行路径 | `lsof -d txt` | `QueryFullProcessImageNameW` |
| 结束进程 | `SIGTERM` / `SIGKILL` | `TerminateProcess`（调用方仍须前后复核同一身份；端口真关闭才算成功） |
| 可执行文件格式门 | Mach-O magic | PE32/PE32+ magic |
| 代码签名身份 | `codesign -d --verbose=4` + Team ID | 无等价概念：跳过 embedded identity，保留大小界 + PE 格式 + SHA-256 快照绑定 |
| 随机数 | `/dev/urandom` | `getrandom`（`BCryptGenRandom`） |
| 打开浏览器 | `open <url>` | `cmd /C start "" <url>` |
| 一键启动 | `zsh scripts/launch-virtual-sandbox.sh` | 原生 Rust 启动（`runtime/sandbox_session_win.rs`），不依赖任何 shell |

### 三个真实平台差异（移植中被实测发现）

1. **密钥存储**：macOS 走 Keychain，Windows 走 `encryption.key.dpapi`
   （`CryptProtectData`，`CRYPTPROTECT_UI_FORBIDDEN`，无 entropy）。
   实测 Windows 版 Science 会**读取明文 `encryption.key` 并迁移进 DPAPI store，同时保留明文做 fallback**，
   因此 macOS 版的 forge（明文 `encryption.key` + `.oauth-tokens/*.enc`）在 Windows 上直接可用。
2. **daemon 端口**：Windows 版 Science 把 daemon API 端口记在 `daemon-control.dpapi` 里，
   并**拒绝信任 lockfile 的明文端口**。所以 CSSwitch 只能通过 CLI（`status --data-dir` / `url`）
   获取端口，不能自己解析 lockfile。
3. **不要设置 `HTTPS_PROXY`**：macOS 启动脚本把 `HTTPS_PROXY` 指向自己的端口来"快速失败"，
   Windows 上会让 `serve --detached` 的启动自检一直等下去（实测挂住不返回）。
   win32 的 Science 自带结构性网络围栏，这里只注入 `ANTHROPIC_BASE_URL`。

### 构建期的两个坑

- `windows-gnu` 构建动态链接 `WebView2Loader.dll`：`build.rs` 会自动把它复制到
  `target/<profile>/` 和 `desktop/src-tauri/binaries/`，打包时必须随包分发。
- Tauri/winit 会导入 `comctl32.dll` 的 `TaskDialogIndirect` 等符号，只有**带 Common Controls v6 清单**
  时才存在。`build.rs` 用 `windres` 编清单并链入（含 `cargo test` 的测试二进制，否则测试体直接
  `0xC0000139` 退出）。正式 bin 同时带 tauri-build 的清单，`ld` 会打印一条
  `.rsrc merge failure: multiple non-default manifests` 警告，产物仍正确。

---

## 4. Windows 上不支持 / 降级的功能

如实标注，不做"看起来能用"的假实现：

| 功能 | 状态 | 说明 |
| --- | --- | --- |
| 第三方模型一键开始 | ✅ 完整 | 代理 + 虚拟登录 + 原生启动 + 身份校验 + 打开界面 |
| 官方 Claude 模式 | ✅ 完整 | 直接启动选定的 `claude-science.exe` |
| 停止 / 状态 / 重新打开 | ✅ 完整 | CLI + 端口/PID/镜像路径三重校验；端口真关闭才算成功 |
| Provider / 模型目录 / Codex 桥接 | ✅ 与 macOS 相同 | 纯网络与配置逻辑，无平台耦合 |
| Skill / MCP 管理 | ⚠️ 未启用 | 依赖 POSIX 的 `openat` 原子 store；Windows 上相关命令返回明确错误，不做半实现 |
| 复用系统 SSH 配置 | ❌ 不支持 | Windows 的 Science 不读 `~/.ssh/config` 入口；设置里开启会得到明确报错 |
| 历史 org 选择（多份恢复） | ❌ 不支持 | 会明确报错并要求清理沙箱 data-dir |
| authority snapshot 回滚 | ⚠️ 简化 | 用一条 journal（`win32_science_launch_started`）代替：中途失败时下一次启动要求人工确认，报 `environment_uncertain` |
| 安装器（NSIS/MSI） | ⚠️ 可选 | 需要 `tauri-cli` + NSIS 下载；便携包不需要。见第 6 节 |

---

## 5. 验证

### 单元与集成测试

```powershell
cd desktop\src-tauri
cargo test --lib        # Windows 上与平台无关的测试 + platform_win_tests
```

构建产物默认在 `desktop\src-tauri\target\release\`（`desktop.exe`、`WebView2Loader.dll`、
stage 好的 `csswitch-gateway.exe`）；`scripts\package-windows.ps1` 会自动按这个位置找。

Windows 专属断言（`src/platform_win_tests.rs`）：

- PE 格式门接受真实 exe、拒绝文本；
- 可执行判定走扩展名；
- `mode()` 保持 POSIX 形状（`S_IFMT` 位、`0o022` 恒 0）；
- `unique_listener_pid` 能把本进程监听的端口映射回自己的 PID；
- 进程启动身份稳定、镜像路径指向自身；
- `terminate_process` 拒绝 pid 0/1，非强制路径不发信号；
- junction 被识别为链接且 `verify_private_path` 拒绝；
- `file_identity` 能识别 hard link（`nNumberOfLinks == 2`）；
- `rename_no_clobber` 目标存在时失败；
- `config::save_to` → `load_from` 往返（覆盖 Windows 版 `SecureDir`）；
- **虚拟登录 forge 往返**（`ensure_virtual_login` → `login_intact` → 幂等 `Reused`）；
- Science 候选发现顺序首选 `%LOCALAPPDATA%\Programs\ClaudeScience`。

macOS 专属测试（openat/FIFO/symlink 语义、`lsof`/`ps` 假二进制）在 Windows 上以
`#[cfg(all(test, unix))]` 关闭，语言保持"测的是平台语义"，不是删测试。

### 端到端（无真实 API Key，无需 UI 点击）

```powershell
powershell -ExecutionPolicy Bypass -File test\win-e2e.ps1
powershell -ExecutionPolicy Bypass -File test\win-oneclick-e2e.ps1   # 真实跑一次“一键开始”
```

`test\win-oneclick-e2e.ps1` 驱动 `platform_win_tests::one_click_login_starts_isolated_science_on_windows`：
临时把配置换成指向本机 mock 上游的 profile，跑**真实的** `one_click_login`
（Gateway → 虚拟登录 → 原生启动 Science → 健康 + 监听 PID + 镜像路径身份校验 → stop 并确认端口关闭），
然后用备份恢复用户配置。它捕获了四个只在真实组合路径上才会暴露的 bug：

1. Windows 上 `Permissions::mode()` 不含 `0o111`，曾被用来判断可执行位 → 所有真实 Science 都被拒；
2. `canonicalize()` 在 Windows 返回 `\\?\C:\...`，与保存的 `C:\...` 直接比较永远不等（Skill host context / managed launch 都受影响）；
3. `File::open(目录).sync_all()` 在 Windows 永远失败（目录不能当文件打开）→ managed launch 记录写不进；
4. `Permissions::mode()` 返回 `0o644` 时 `mode & 0o077 == 0` 不成立 → 受管启动记录被判“不是私有文件”。

`test\win-e2e.ps1` 覆盖 18 项检查，全部针对真实组件：

1. mock 上游就绪；
2. Gateway 起来并通过 `/<secret>/health`；
3. 错误 path-secret 被拒绝；
4. `/<secret>/v1/messages` 转发成功（HTTP 200）；
5. 响应体来自 mock 上游；
6. 上游确实收到 `/v1/messages`；
7. 模型名原样透传；
8. Science `serve` 退出码 0；
9. Science `/health` healthy；
10. `status.running`、`status.port`；
11. 端口 → 唯一监听 PID；
12. `status.pid` 与 netstat PID 一致；
13. 监听进程镜像路径 == 选定 Science；
14. 隔离 data-dir 生成账号/密钥状态；
15. 隔离 data-dir 生成 `daemon-control.dpapi`；
16. 真实 `%USERPROFILE%\.claude-science` 未被写入（文件数/字节数前后一致）；
17. `stop` 后端口真的关闭；
18. （含 1-17 合计 18 项断言）。

### 手工验收（覆盖 UI 组合路径）

1. `powershell -File test\win-e2e\mock_upstream.py 28990 $env:TEMP\upstream.jsonl` 起 mock 上游；
2. 「模型连接」新增自定义 **Anthropic Messages** 配置：
   `base_url = http://127.0.0.1:28990`，模型名 `claude-win-e2e-model`，任意 key；
3. 「设为当前」→ 首页「一键开始」；
4. Science 打开后发一条消息，观察 `$env:TEMP\upstream.jsonl` 里出现该模型名；
5. 「停止全部」，确认 `netstat -ano -p tcp | findstr 8990` 无监听。

---

## 6. 安装器（可选）

```powershell
cd desktop
npm install
npx tauri build --bundles nsis
```

需要联网下载 NSIS。若网络受限，直接用第 1 节的便携包即可；便携包与安装器产物等价，
只是没有开始菜单项与卸载登记。

---

## 7. 已知问题

- `cargo build` 在 `windows-gnu` 上会打印 `.rsrc merge failure: multiple non-default manifests`
  警告（见第 3 节），属于预期。
- `scripts/*.sh` 与 `test/*.sh` 仍是 macOS/Linux 资产；Windows 侧的等价入口是
  `scripts/package-windows.ps1` 与 `test/win-e2e.ps1`。
- `scratch` 的跨进程探针单测在 Windows 上 `#[ignore]`，由 `test/win-e2e.ps1` 覆盖同一行为。
