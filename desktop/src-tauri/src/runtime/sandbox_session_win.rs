//! Windows 版「一键开始」会话。
//!
//! macOS 上的同一件事由 `sandbox_session.rs` 用 `zsh` 启动脚本 + `openat` 事务快照
//! 完成。Windows 没有 zsh、没有 `openat` 目录句柄、也没有 `lsof`/`ps`，因此这里
//! 用另一个更小的实现满足同一组**可观察合同**：
//!
//! 1. 启动前必须能确认 runtime 身份（canonical path + `--version` + SHA-256 指纹）；
//! 2. 只有 `status` 说 running、`/health` 说 healthy、端口监听 PID 的 executable
//!    与选定 runtime 一致时，才把实例当成“我们的”；
//! 3. 停止只对上面证明过的那一个 PID 发信号，端口真的关掉才算成功；
//! 4. 若在 `serve` 之后失败，绝不做“看起来干净”的回滚——Science 可能已经迁移了
//!    自己的环境，因此留一条 journal 并报 `environment_uncertain`；
//! 5. 隔离靠重定向 HOME/USERPROFILE/APPDATA/LOCALAPPDATA/TEMP，而不是只改 HOME。
//!
//! 与 macOS 的差异（如实记录）：Windows 版不提供 authority snapshot 快照
//! 回滚、不提供 Skill 路由注册与系统 SSH 桥接。

use serde_json::{json, Value};
use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::Runtime;

use crate::config;
use crate::lifecycle;
use crate::lock;
use crate::proc;
use crate::oauth_forge;
use crate::runtime::proxy::ProxyAction;
use crate::runtime::proxy_lifecycle::ensure_proxy;
use crate::runtime::science::{
    probe_known_runtime, probe_sandbox_runtime_cached, record_managed_science_launch,
    sandbox_data_dir, sandbox_home, sandbox_url, select_science_runtime_cached, stop_sandbox,
    SandboxScienceState, ScienceManagedLaunchToken, ScienceRuntimeIdentity,
};
use crate::runtime::system::{log_path, open_in_browser};
use crate::SharedAppState;

/// 与 macOS 版同名的受保护状态清单：Windows 版不做快照回滚，但诊断与
/// 未来版本仍按这份清单识别 Science 自有状态。
pub(crate) const SCIENCE_PROTECTED_AUTHORITY_ENTRIES: [&str; 10] = [
    "encryption.key",
    ".oauth-tokens",
    "active-org.json",
    ".key-backups",
    "auth-owner.lock",
    "config.toml",
    "csswitch-ssh-bridge.v1.json",
    "mcp",
    ".csswitch-route-state.json",
    "orgs",
];

const VIRTUAL_EMAIL: &str = "virtual@localhost.invalid";
const LAUNCH_JOURNAL_STAGE: &str = "win32_science_launch_started";
const HEALTH_TIMEOUT: Duration = Duration::from_secs(90);

/// Windows 版没有 authority 快照，因此任何 stage 都不需要“保留快照”。
///
/// 这个函数在 macOS 上用来决定“中断的事务是否必须人工恢复”；Windows 的等价
/// 保护是 [`LAUNCH_JOURNAL_STAGE`] 那条 journal，见 [`interrupted_launch_stage`]。
pub(crate) fn runtime_transaction_requires_snapshot_preservation(_stage: &str) -> bool {
    false
}

/// 上一次启动是否停在“Science 环境可能已被改动”的边界上。
pub(crate) fn interrupted_launch_stage(cfg: &config::Config) -> Option<String> {
    cfg.runtime_transaction
        .as_ref()
        .map(|journal| journal.stage.clone())
        .filter(|stage| stage == LAUNCH_JOURNAL_STAGE)
}

#[derive(Debug)]
pub(crate) enum ReconcileScienceError {
    PriorScienceRestored { cause: String },
    EnvironmentUncertain { cause: String },
    RestartRequired { cause: String },
}

impl ReconcileScienceError {
    pub(crate) fn cause(&self) -> &str {
        match self {
            Self::PriorScienceRestored { cause }
            | Self::EnvironmentUncertain { cause }
            | Self::RestartRequired { cause } => cause,
        }
    }

    pub(crate) fn prior_science_restored(&self) -> bool {
        matches!(self, Self::PriorScienceRestored { .. })
    }

    pub(crate) fn environment_uncertain(&self) -> bool {
        matches!(self, Self::EnvironmentUncertain { .. })
    }
}

/// 一键开始：代理 → 虚拟登录 → 启动隔离 Science → 打开界面。
///
/// 调用方必须持有命令串行锁。返回值形状与 macOS 版一致，前端只依赖
/// `status` / `action` / `stage` / `msg` / `fallback_url` 这几个字段。
pub(crate) fn one_click_login<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: SharedAppState,
    lifecycle: &lifecycle::Lifecycle,
    runtime_choice: Option<&str>,
    auth_proof: Option<&crate::codex_auth_supervisor::CodexAuthReadyProof>,
) -> Result<Value, String> {
    one_click_login_with_options(app, state, lifecycle, runtime_choice, auth_proof, true, None)
}

fn one_click_login_with_options<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: SharedAppState,
    lifecycle: &lifecycle::Lifecycle,
    runtime_choice: Option<&str>,
    auth_proof: Option<&crate::codex_auth_supervisor::CodexAuthReadyProof>,
    open_surface: bool,
    reconcile_disposition: Option<&mut PriorScienceDisposition>,
) -> Result<Value, String> {
    let dir = config::default_dir();
    let cfg = config::load_from(&dir).map_err(|error| error.to_string())?;
    let active_profile = cfg
        .active_profile()
        .cloned()
        .ok_or("未配置生效 profile，请先在面板选择或新建一条配置。")?;
    config::require_template_enabled(&cfg, &active_profile.template_id)?;
    crate::runtime::settings::validate_runtime_ports(cfg.proxy_port, cfg.sandbox_port)?;
    if cfg.mode == "official" {
        return Err("官方模式不需要第三方代理；请先切换到「第三方模型」模式。".into());
    }
    let sport = cfg.sandbox_port;
    let sbx_home = sandbox_home();
    let auth_dir = sbx_home.join(".claude-science");
    let version_cache = { lock(&state).science_version_cache.clone() };

    // ---- 1. 现状判定：能证明是我们的 daemon 就复用，否则严格失败关闭 ----
    let (remembered_runtime, confirmed_stopped) = {
        let st = lock(&state);
        (
            st.science_runtime.clone(),
            st.science_confirmed_stopped.clone(),
        )
    };
    let (science_state, running_runtime) = match remembered_runtime {
        Some(runtime) => {
            let state_now = probe_known_runtime(sport, &runtime);
            let running = (state_now == SandboxScienceState::RunningHealthy).then_some(runtime);
            (state_now, running)
        }
        None if confirmed_stopped.is_some() && !proc::loopback_port_in_use(sport, 100) => {
            (SandboxScienceState::Stopped, None)
        }
        None => probe_sandbox_runtime_cached(sport, &version_cache)?,
    };

    let interrupted = interrupted_launch_stage(&cfg);
    if let Some(stage) = interrupted.as_deref() {
        // 上一次启动停在环境暴露边界。只有“当前 daemon 就是我们记录的那个
        // runtime 且健康”时才可以继续；否则要求人工确认。
        let recoverable = science_state == SandboxScienceState::RunningHealthy
            && running_runtime.is_some();
        if !recoverable {
            return Err(format!(
                "上次启动在 Science 环境暴露边界中断（stage={stage}），且当前 daemon 身份无法确认；\
                 已拒绝自动重试。请先停止占用端口 {sport} 的进程后重试；\
                 environment_uncertain；recovery_status=manual_recovery_required"
            ));
        }
    }

    // ---- 2. 已在运行：只负责重开界面 / 重建代理，绝不动 Science ----
    if let (SandboxScienceState::RunningHealthy, Some(runtime)) =
        (science_state, running_runtime.clone())
    {
        if oauth_forge::login_intact(&auth_dir, VIRTUAL_EMAIL, &sbx_home) {
            let (proxy_port, _secret, proxy_action) = ensure_proxy(
                &app,
                &state,
                lifecycle,
                Some(&runtime),
                None,
                auth_proof,
            )?;
            let url = sandbox_url(sport, &runtime);
            {
                let mut current = lock(&state);
                current.sandbox_port = sport;
                current.sandbox_url = Some(url.clone());
                current.science_runtime = Some(runtime.clone());
                current.science_confirmed_stopped = None;
            }
            let base = match proxy_action {
                ProxyAction::Reused => "已在运行",
                ProxyAction::Restarted => "已用新配置重启代理，Science 沿用不变",
            };
            let (message, fallback_url) = if open_surface {
                match open_in_browser(&url) {
                    Ok(()) => (format!("{base}，已向系统浏览器发送打开请求。"), None),
                    Err(_) => (
                        format!("{base}，服务已就绪；自动打开失败。"),
                        Some(url.clone()),
                    ),
                }
            } else {
                (format!("{base}，Science 绑定保持不变。"), None)
            };
            clear_launch_journal();
            return Ok(json!({
                "msg": message,
                "action": "reopened",
                "stage": "complete",
                "status": "ok",
                "recovery_status": "not_needed",
                "fallback_url": fallback_url,
                "external_skill_installer": Value::Null,
                "environment_status": if interrupted.is_some() { json!("uncertain") } else { json!("clean") },
            }));
        }
        // 登录不完整（例如用户清空过 data-dir）→ 需要重启 Science 才能重建。
    }

    if science_state == SandboxScienceState::Unknown {
        return Err(format!(
            "无法确认隔离 Science 状态（端口 {sport} 或 data-dir 状态不一致）。\
             请先停止占用该端口的进程后重试。"
        ));
    }

    // ---- 3. 选定 runtime ----
    let launch_runtime = match running_runtime.clone() {
        Some(runtime) if science_state == SandboxScienceState::RunningHealthy => runtime,
        _ => select_science_runtime_cached(runtime_choice, &version_cache)?,
    };
    let interrupted_runtime_mismatch = interrupted.is_some()
        && running_runtime
            .as_ref()
            .is_some_and(|runtime| runtime.path != launch_runtime.path);

    // ---- 4. 先停掉旧实例（只有身份被证明过的那个 PID 会被发信号）----
    {
        let mut sandbox = { lock(&state).sandbox.take() };
        let mut sandbox_url_slot = { lock(&state).sandbox_url.take() };
        let prior = running_runtime.clone();
        if let Err(error) = stop_sandbox(
            &app,
            &mut sandbox,
            &mut sandbox_url_slot,
            prior.as_ref(),
        ) {
            let mut current = lock(&state);
            current.sandbox = sandbox;
            current.sandbox_url = sandbox_url_slot;
            return Err(error);
        }
        let mut current = lock(&state);
        current.sandbox = sandbox;
        current.sandbox_url = sandbox_url_slot;
    }

    // ---- 5. 代理 ----
    let (proxy_port, secret, proxy_action) = ensure_proxy(
        &app,
        &state,
        lifecycle,
        Some(&launch_runtime),
        None,
        auth_proof,
    )?;
    if secret.is_empty() {
        return Err("代理 secret 为空；已拒绝把无鉴权的回环地址交给 Science。".into());
    }
    let proxy_url = format!("http://127.0.0.1:{proxy_port}/{secret}");

    // ---- 6. 虚拟登录（明文 encryption.key + .oauth-tokens/*.enc）----
    let login_action = match oauth_forge::ensure_virtual_login(&auth_dir, VIRTUAL_EMAIL, &sbx_home) {
        Ok((_forge, action)) => action,
        Err(oauth_forge::EnsureVirtualLoginError::Message(message)) => {
            return Err(format!("准备隔离登录失败：{message}"))
        }
        Err(oauth_forge::EnsureVirtualLoginError::HistoryChoiceRequired(_)) => {
            return Err(
                "检测到多份历史记录，Windows 版暂不支持历史选择；请清理沙箱 data-dir 后重试"
                    .into(),
            )
        }
    };

    // ---- 7. 启动（环境暴露边界从这里开始）----
    if interrupted_runtime_mismatch {
        return Err(
            "中断记录对应的 runtime 与当前选定 runtime 不一致；已拒绝跨 runtime 自动启动；\
             environment_uncertain；recovery_status=manual_recovery_required"
                .into(),
        );
    }
    write_launch_journal(&active_profile.id)?;
    let preview_port = sport
        .checked_add(1)
        .ok_or("sandbox 端口必须小于 65535，才能分配隔离预览端口")?;
    let spawn_result = spawn_science_daemon(&launch_runtime, sport, preview_port, &proxy_url);
    if let Err(error) = spawn_result {
        // `serve` 还没真正跑起来 → 环境未被改动，可以安全清掉 journal。
        clear_launch_journal();
        return Err(error);
    }

    // ---- 8. 等待健康 + 建立 managed launch 身份 ----
    let launch_token = match wait_for_healthy_launch(sport, &launch_runtime) {
        Ok(token) => token,
        Err(error) => {
            // Science 已经被调用过：可能已写入自己的环境，因此保留 journal，
            // 并把“无法证明已停止”如实报给用户。
            let mut current = lock(&state);
            current.sandbox_url = None;
            current.science_runtime = Some(launch_runtime.clone());
            return Err(format!(
                "{error}；Science 已被启动，环境状态为 environment_uncertain；\
                 recovery_status=manual_recovery_required"
            ));
        }
    };
    let url = sandbox_url(sport, &launch_runtime);
    {
        let mut current = lock(&state);
        current.sandbox_port = sport;
        current.sandbox_url = Some(url.clone());
        current.science_runtime = Some(launch_runtime.clone());
        current.science_confirmed_stopped = None;
    }
    clear_launch_journal();
    let _ = &launch_token;
    // Windows 专属：沙箱的 ro-root ACL 盖章与 conda 环境创建有先有后。全新
    // data-dir 上，`claude-science-mcp` 往往在盖章之后才被创建，于是容器
    // 无法 spawn 环境里的 python.exe/node.exe → 自检 exit 113 → MCP 环境
    // 永远 Failed。Science 自带的修复入口就是 `sandbox doctor`（无需管理员），
    // 它会重新给 conda envs 打上容器 SID 的读/执行权限。这里在后台跑一次。
    schedule_sandbox_doctor_once(&launch_runtime);

    let (message, fallback_url) = if open_surface {
        match open_in_browser(&url) {
            Ok(()) => (
                "已启动隔离 Science，推理指向本地代理；已向系统浏览器发送打开请求。".to_string(),
                None,
            ),
            Err(_) => (
                "已启动隔离 Science，推理指向本地代理；自动打开失败。".to_string(),
                Some(url.clone()),
            ),
        }
    } else {
        ("已启动隔离 Science，推理指向本地代理。".to_string(), None)
    };
    let mut value = json!({
        "msg": message,
        "action": "started",
        "stage": "complete",
        "status": "ok",
        "recovery_status": "not_needed",
        "fallback_url": fallback_url,
        "external_skill_installer": Value::Null,
        "environment_status": "clean",
    });
    value["proxy_action"] = json!(proxy_action.as_str());
    value["login_action"] = json!(format!("{login_action:?}"));
    value["science_version"] = json!(launch_runtime.version.clone());
    value["science_source"] = json!(launch_runtime.source.code());
    if let Some(disposition) = reconcile_disposition {
        *disposition = PriorScienceDisposition::Restarted;
    }
    Ok(value)
}

/// 应用 `serve` 的子进程；成功返回代表“命令已被接受并进入后台”。
fn spawn_science_daemon(
    runtime: &ScienceRuntimeIdentity,
    port: u16,
    preview_port: u16,
    proxy_url: &str,
) -> Result<(), String> {
    let log_path = log_path("science.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("无法打开 Science 日志：{error}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("无法复制 Science 日志句柄：{error}"))?;

    let mut command = Command::new(&runtime.path);
    command
        .arg("serve")
        .arg("--data-dir")
        .arg(sandbox_data_dir())
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--sandbox-port")
        .arg(preview_port.to_string())
        .arg("--no-browser")
        .arg("--no-auto-update")
        .arg("--detached")
        .env("ANTHROPIC_BASE_URL", proxy_url)
        // 注意：不要在 Windows 上像 macOS 启动脚本那样把 HTTPS_PROXY 指向自己的端口。
        // macOS 用这个“快失败”技巧堵住真实 Anthropic 出口；win32 的 Science 有自己的
        // 结构性网络围栏（sandbox egress fence），而把代理指向本机回环会让
        // `serve --detached` 的启动自检一直等下去（实测会挂住，不返回）。
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    crate::platform::apply_isolated_home(&mut command, &sandbox_home());

    let status = command
        .status()
        .map_err(|error| format!("启动 Science 失败：{error}"))?;
    if !status.success() {
        let tail = crate::runtime::system::tail_file(&log_path, 2400);
        let redacted = crate::runtime::system::redact(&tail, proxy_url);
        // Science 的失败原因往往埋在进度输出后面（例如 “port 8990 is busy”）；
        // 先挑出像原因的几行，再退化为最后几行。
        let focus: Vec<&str> = redacted
            .lines()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                ["error", "busy", "failed", "refus", "denied", "sandbox", "admin"]
                    .iter()
                    .any(|needle| lower.contains(needle))
            })
            .rev()
            .take(4)
            .collect();
        let detail = if focus.is_empty() {
            redacted.lines().rev().take(4).collect::<Vec<_>>().join(" | ")
        } else {
            focus.into_iter().rev().collect::<Vec<_>>().join(" | ")
        };
        return Err(format!(
            "Science 启动命令非零退出（{:?}）。关键日志：{}",
            status.code(),
            detail
        ));
    }
    Ok(())
}

/// 等到“端口健康 + 监听 PID 的 executable 就是选定 runtime”同时成立。
fn wait_for_healthy_launch(
    port: u16,
    runtime: &ScienceRuntimeIdentity,
) -> Result<ScienceManagedLaunchToken, String> {
    let deadline = Instant::now() + HEALTH_TIMEOUT;
    let mut identity_deadline: Option<Instant> = None;
    let mut last_identity_error = String::new();
    loop {
        if proc::http_health(port, None, 400) {
            match record_managed_science_launch(port, runtime) {
                Ok(token) => return Ok(token),
                Err(error) => {
                    last_identity_error = error.message().to_string();
                    // 健康但身份长期不成立 = 真的不对劲（不是还在启动），快速失败。
                    let first = *identity_deadline.get_or_insert_with(Instant::now);
                    if first.elapsed() >= Duration::from_secs(8) {
                        return Err(format!(
                            "Science 已在端口 {port} 提供服务，但身份校验持续失败：{last_identity_error}"
                        ));
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(if last_identity_error.is_empty() {
                format!("等待 Science 在端口 {port} 就绪超时")
            } else {
                format!("等待 Science 在端口 {port} 就绪超时；最后一次身份校验失败：{last_identity_error}")
            });
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 一次性（每个 Science 版本）跑 `claude-science sandbox doctor`。
///
/// 背景：Windows 原生沙箱是一个 AppContainer。Science 在 `prepare` 阶段才会
/// 给 `conda/envs`、`conda/pkgs` 等 ro-root 打上容器 SID 的读/执行 ACE。而在
/// **全新 data-dir** 上，`claude-science-mcp` 环境是在盖章之后才被创建的，于是
/// 容器无法 spawn 环境内的 `python.exe` / `node.exe`：
///
/// ```text
/// operon-winsbx-stdio-owner: cannot spawn the payload ...\python.exe: 拒绝访问 (0x80070005)
/// [conda/win32] the sandboxed interpreter self-check ... did not complete (exit 113)
/// ```
///
/// 结果就是 MCP 环境永远停在 Failed，声明包（mcp / pandas / pywin32 / requests）
/// 永远不会被安装。`sandbox doctor` 是 Science 自带的修复入口（无需管理员），
/// 它会重新 prepare 并重新盖章。这里在后台跑一次，不阻塞 UI。
fn schedule_sandbox_doctor_once(runtime: &ScienceRuntimeIdentity) {
    let marker = config::default_dir().join("sandbox-doctor.v1.json");
    let version = runtime.version.clone().unwrap_or_default();
    if marker
        .is_file()
        .then(|| fs::read(&marker).ok())
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("science_version")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .as_deref()
        == Some(version.as_str())
    {
        return;
    }

    let bin = runtime.path.clone();
    let data_dir = sandbox_data_dir();
    let log_path = crate::runtime::system::log_path("sandbox-doctor.log");
    let home = sandbox_home();
    std::thread::spawn(move || {
        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let output = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path);
        let mut command = Command::new(&bin);
        command
            .arg("sandbox")
            .arg("doctor")
            .arg("--data-dir")
            .arg(&data_dir)
            .stdin(Stdio::null());
        crate::platform::apply_isolated_home(&mut command, &home);
        match output {
            Ok(file) => {
                let clone = file.try_clone();
                if let Ok(clone) = clone {
                    let _ = command
                        .stdout(Stdio::from(clone))
                        .stderr(Stdio::from(file))
                        .status();
                }
            }
            Err(_) => {
                let _ = command
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        // 不管成功与否都记下版本：doctor 失败时不应每次启动都重跑它。
        let payload = serde_json::json!({
            "schema_version": 1,
            "science_version": version,
            "completed_at_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_millis())
                .unwrap_or(0),
        });
        if let Ok(bytes) = serde_json::to_vec_pretty(&payload) {
            let _ = fs::write(&marker, bytes);
        }
    });
}

fn write_launch_journal(profile_id: &str) -> Result<(), String> {    let dir = config::default_dir();
    let mut cfg = config::load_from(&dir).map_err(|error| error.to_string())?;
    cfg.runtime_transaction = Some(config::RuntimeTransactionJournal {
        transaction_id: format!(
            "win32-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_millis())
                .unwrap_or(0)
        ),
        target_profile_id: profile_id.to_string(),
        stage: LAUNCH_JOURNAL_STAGE.to_string(),
        previous_binding: None,
        previous_gateway: None,
    });
    config::save_to(&dir, &cfg).map_err(|error| error.to_string())
}

fn clear_launch_journal() {
    let dir = config::default_dir();
    let Ok(mut cfg) = config::load_from(&dir) else {
        return;
    };
    if cfg.runtime_transaction.is_none() {
        return;
    }
    cfg.runtime_transaction = None;
    let _ = config::save_to(&dir, &cfg);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PriorScienceDisposition {
    RestartRequired,
    Restarted,
}

/// 诊断用的第三方链路核验。Windows 版没有 Skill 路由注册，因此这里只如实
/// 报告状态，不假装修复过路由文件。
pub(crate) fn force_third_party_reconcile<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    state: &SharedAppState,
) -> Result<String, String> {
    let cfg = config::load_from(&config::default_dir()).map_err(|error| error.to_string())?;
    if cfg.mode == "official" {
        return Ok("官方模式无需核验第三方链路。".into());
    }
    let remembered = { lock(state).science_runtime.clone() };
    let Some(runtime) = remembered else {
        return Ok("Science 未运行；下次一键开始会重新建立第三方链路。".into());
    };
    match probe_known_runtime(cfg.sandbox_port, &runtime) {
        SandboxScienceState::RunningHealthy => {
            Ok("Science 正在使用第三方代理；Windows 版不包含 Skill 路由注册，无需核验。".into())
        }
        _ => Ok("Science 未以已证明的身份运行；已安排下次一键开始重新建立链路。".into()),
    }
}

/// 切换 profile 后让 Science 重新绑定模型：Windows 版直接重启受管 daemon。
pub(crate) fn reconcile_science_for_active<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: SharedAppState,
    lifecycle: &lifecycle::Lifecycle,
    auth_proof: Option<&crate::codex_auth_supervisor::CodexAuthReadyProof>,
) -> Result<Value, ReconcileScienceError> {
    let mut disposition = PriorScienceDisposition::RestartRequired;
    one_click_login_with_options(
        app,
        state,
        lifecycle,
        None,
        auth_proof,
        true,
        Some(&mut disposition),
    )
    .map_err(|cause| {
        if cause.contains("environment_uncertain") {
            ReconcileScienceError::EnvironmentUncertain { cause }
        } else {
            ReconcileScienceError::RestartRequired { cause }
        }
    })
}

/// 强制重启受管 Science（保持当前 runtime 与登录）。
pub(crate) fn force_restart_science_for_active<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: SharedAppState,
    lifecycle: &lifecycle::Lifecycle,
    auth_proof: Option<&crate::codex_auth_supervisor::CodexAuthReadyProof>,
) -> Result<Value, String> {
    one_click_login_with_options(app, state, lifecycle, None, auth_proof, false, None)
}

/// 后台预热：Windows 版没有需要预热的独立 updater runtime，直接返回。
pub(crate) fn prewarm_science_runtime(
    _state: &SharedAppState,
    _version_cache: &crate::runtime::science::ScienceVersionCache,
) {
}
