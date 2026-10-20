//! Windows 平台原语测试。
//!
//! macOS 上的对应行为由 `#[cfg(unix)]` 的既有测试覆盖（openat / lsof / ps /
//! Mach-O / 权限位）。这里验证 Windows 侧替代实现是否保持了同样的 fail-closed
//! 形状：端口→PID、进程启动身份、镜像路径、终止、链接检测、文件身份、PE 格式、
//! 权限位推导、以及 Science 程序发现顺序。

use crate::platform;
use crate::platform::{MetadataExt, OpenOptionsExt, PermissionsExt};

#[test]
fn pe_format_gate_accepts_real_windows_executable() {
    let current = std::env::current_exe().expect("测试进程本身必须可定位");
    assert!(
        platform::science_binary_format_ok(&current),
        "PE 格式门必须接受真实的 Windows 可执行文件"
    );
}

#[test]
fn pe_format_gate_rejects_plain_text() {
    let dir = std::env::temp_dir().join(format!("csswitch-pe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    let path = dir.join("fake.exe");
    std::fs::write(&path, b"MZnot-a-real-pe-file").expect("写入测试文件");
    assert!(!platform::science_binary_format_ok(&path));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn executable_detection_uses_extension_not_mode_bits() {
    let dir = std::env::temp_dir().join(format!("csswitch-exe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    let exe = dir.join("claude-science.exe");
    let txt = dir.join("claude-science.txt");
    std::fs::write(&exe, b"MZ").expect("写入 exe");
    std::fs::write(&txt, b"MZ").expect("写入 txt");
    assert!(platform::path_is_executable(&exe));
    assert!(!platform::path_is_executable(&txt));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn metadata_mode_keeps_posix_shape() {
    let dir = std::env::temp_dir().join(format!("csswitch-mode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    let file = dir.join("config.json");
    std::fs::write(&file, b"{}").expect("写入文件");

    let dir_meta = std::fs::symlink_metadata(&dir).expect("目录 metadata");
    assert_eq!(dir_meta.mode() & platform::S_IFMT, platform::S_IFDIR);
    assert_eq!(dir_meta.mode() & 0o022, 0, "Windows 上不应报告 group/world 可写");

    let file_meta = std::fs::symlink_metadata(&file).expect("文件 metadata");
    assert_eq!(file_meta.mode() & platform::S_IFMT, platform::S_IFREG);
    assert_eq!(file_meta.uid(), platform::current_user_id());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn permissions_from_mode_controls_readonly_bit() {
    let readonly = std::fs::Permissions::from_mode(0o400);
    assert!(readonly.readonly(), "0o400（无写位）必须映射为只读");
    let writable = std::fs::Permissions::from_mode(0o600);
    assert!(!writable.readonly(), "0o600 保留 owner 写位，必须可写");
    let mode_755 = std::fs::Permissions::from_mode(0o755);
    assert!(!mode_755.readonly(), "0o755 必须保持可写");
}

#[test]
fn unique_listener_pid_finds_this_process() {
    use std::net::TcpListener;
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("绑定临时端口");
    let port = listener.local_addr().expect("取本地地址").port();
    let pid = platform::unique_listener_pid(port);
    assert_eq!(
        pid,
        Some(std::process::id()),
        "netstat 解析必须把监听端口映射到本进程 tid"
    );
    drop(listener);
}

#[test]
fn process_identity_and_image_path_describe_this_process() {
    let pid = platform::current_pid();
    let first = platform::process_start_identity(pid).expect("读取创建时间");
    let second = platform::process_start_identity(pid).expect("再次读取创建时间");
    assert_eq!(first, second, "同一进程的启动身份必须稳定");
    assert!(first.starts_with("win32-created:"));

    let image = platform::process_image_path(pid).expect("读取镜像路径");
    let current = std::env::current_exe().expect("当前可执行文件");
    assert!(
        image
            .canonicalize()
            .ok()
            .zip(current.canonicalize().ok())
            .is_some_and(|(left, right)| left == right),
        "镜像路径必须指向当前可执行文件"
    );
    assert!(platform::process_is_alive(pid));
    assert!(
        !platform::process_is_alive(4),
        "System (pid 4) 无法被普通用户打开，必须报告为不可用"
    );
}

#[test]
fn terminate_refuses_invalid_pid_and_non_force_is_a_noop() {
    assert!(!platform::terminate_process(0, true));
    assert!(!platform::terminate_process(1, true));
    // 非强制路径在 Windows 上不发送信号，交给调用方轮询端口。
    assert!(platform::terminate_process(std::process::id(), false));
}

#[test]
fn junction_is_reported_as_link() {
    let dir = std::env::temp_dir().join(format!("csswitch-junction-{}", std::process::id()));
    let target = dir.join("target");
    let link = dir.join("link");
    std::fs::create_dir_all(&target).expect("创建目标目录");
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("调用 mklink");
    if status.success() {
        assert!(
            platform::path_is_reparse_or_link(&link),
            "junction 必须被识别为链接"
        );
        assert!(platform::verify_private_path(&link).is_err());
        let normal = dir.join("plain");
        std::fs::create_dir_all(&normal).expect("创建普通目录");
        assert!(platform::verify_private_path(&normal).is_ok());
        assert!(platform::verify_private_path(&target).is_ok());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_identity_reports_hard_link_count() {
    let dir = std::env::temp_dir().join(format!("csswitch-identity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    let source = dir.join("a.bin");
    let hard = dir.join("b.bin");
    std::fs::write(&source, b"payload").expect("写入源文件");
    std::fs::hard_link(&source, &hard).expect("创建 hard link");

    let source_identity = platform::file_identity(&source).expect("源文件身份");
    let hard_identity = platform::file_identity(&hard).expect("硬链接身份");
    assert_eq!(source_identity.0, hard_identity.0, "同一卷序列号");
    assert_eq!(source_identity.1, hard_identity.1, "同一 file index");
    assert_eq!(source_identity.2, 2, "nNumberOfLinks 必须为 2");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_no_clobber_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("csswitch-rename-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    let source = dir.join("source.json");
    let target = dir.join("target.json");
    std::fs::write(&source, b"new").expect("写源");
    std::fs::write(&target, b"old").expect("写目标");

    assert!(
        platform::rename_no_clobber(&source, &target).is_err(),
        "目标已存在时 no-clobber rename 必须失败"
    );
    assert_eq!(std::fs::read(&target).expect("目标仍存在"), b"old");

    let fresh = dir.join("fresh.json");
    platform::rename_no_clobber(&source, &fresh).expect("目标不存在时必须成功");
    assert_eq!(std::fs::read(&fresh).expect("新目标"), b"new");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn canonical_helpers_agree_on_real_science_binary() {
    let bin = crate::runtime::science::installed_science_bin();
    if !bin.is_file() {
        return;
    }
    let canonical = platform::canonicalize(&bin).expect("canonicalize 必须成功");
    assert!(
        !canonical.to_string_lossy().starts_with(r"\\?\"),
        "canonicalize 必须去掉 Windows verbatim 前缀：{}",
        canonical.display()
    );
    assert!(
        platform::same_path(&canonical, &bin),
        "规范化后的路径必须与原始路径指向同一对象：{} vs {}",
        canonical.display(),
        bin.display()
    );
    assert!(platform::is_canonical(&bin), "真实 Science 路径应被判定为规范形式");
}

#[test]
fn virtual_login_forge_roundtrips_on_windows() {
    // 虚拟登录是「不碰真实账号也能启动 Science」的前提。这里在临时目录里
    // 跑一遍真实的 forge：写 encryption.key + .oauth-tokens/*.enc + active-org.json，
    // 再用同一套校验函数回读，证明 Windows 上的路径/权限实现没有把流程弄坏。
    let root = std::env::temp_dir().join(format!("csswitch-forge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let auth_dir = home.join(".claude-science");
    std::fs::create_dir_all(&auth_dir).expect("创建隔离 data-dir");

    let email = "virtual@localhost.invalid";
    assert!(!crate::oauth_forge::login_intact(&auth_dir, email, &home));

    let created = crate::oauth_forge::ensure_virtual_login(&auth_dir, email, &home);
    let (forge, action) = match created {
        Ok(value) => value,
        Err(error) => panic!("虚拟登录生成失败：{error}"),
    };
    assert_eq!(action, crate::oauth_forge::LoginAction::Created);
    assert!(forge.enc_file.is_file(), "必须写出会话 token 文件");
    assert!(auth_dir.join("encryption.key").is_file());
    assert!(auth_dir.join("active-org.json").is_file());

    // 回读：同目录必须判定为“登录完整”，否则 Science 会被要求真实登录。
    assert!(
        crate::oauth_forge::login_intact(&auth_dir, email, &home),
        "刚生成的虚拟登录必须被判定为完整"
    );

    // 再次调用必须是 Reused（幂等），不得重新铸 org。
    let again = crate::oauth_forge::ensure_virtual_login(&auth_dir, email, &home);
    assert!(
        matches!(again, Ok((_, crate::oauth_forge::LoginAction::Reused))),
        "重复调用必须复用既有虚拟登录"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "需要本机已安装 Claude Science（或已保存程序路径）"]
fn runtime_selection_accepts_a_real_windows_science_binary() {
    // 这条回归测试锁定一个真实移植 bug：Windows 上 `Permissions::mode()` 不包含
    // `0o111`，而 fingerfprint 曾用它判断可执行性，导致所有真实 Science 都被
    // 拒绝，一键开始永远报“不可用”。现在改为按路径+扩展名判定。
    let bin = crate::runtime::science::installed_science_bin();
    assert!(
        bin.is_file(),
        "未安装 Claude Science；期望位置：{}",
        bin.display()
    );
    let version = crate::runtime::science::validate_science_binary_candidate(&bin)
        .expect("真实 Science 必须通过候选校验");
    assert!(version.contains("claude-science") || !version.is_empty());

    let cache = crate::runtime::science::ScienceVersionCache::default();
    let runtime = crate::runtime::science::select_science_runtime_cached(None, &cache)
        .expect("真实 Science 必须能被选为 runtime");
    assert_eq!(runtime.path, bin, "应选中已发现/已保存的那个可执行文件");
    assert_eq!(runtime.version.as_deref(), Some(version.as_str()));
}

#[test]
fn legacy_cwd_config_is_migrated_and_never_overwritten() {
    let root = std::env::temp_dir().join(format!("csswitch-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let legacy = root.join("pkg").join(".csswitch");
    let target = root.join("home").join(".csswitch");
    std::fs::create_dir_all(&legacy).expect("创建旧位置");
    std::fs::write(legacy.join("config.json"), b"{\"schema_version\":4}").expect("旧配置");
    std::fs::write(legacy.join("science-bin.v1.json"), b"{\"schema_version\":1}").expect("旧覆盖");

    assert!(crate::config::migrate_legacy_config_from(&legacy, &target));
    assert!(target.join("config.json").is_file());
    assert!(target.join("science-bin.v1.json").is_file());
    // 源必须保留（迁移只复制）。
    assert!(legacy.join("config.json").is_file());

    // 目标已有配置时不得覆盖，也不重复报告迁移。
    std::fs::write(target.join("config.json"), b"{\"schema_version\":5}").expect("新配置");
    assert!(!crate::config::migrate_legacy_config_from(&legacy, &target));
    assert_eq!(
        std::fs::read(target.join("config.json")).expect("读回"),
        b"{\"schema_version\":5}".to_vec()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "需 mock 上游：由 test/win-oneclick-e2e.ps1 驱动（真实跑一次一键开始）"]
fn one_click_login_starts_isolated_science_on_windows() {
    use std::sync::{Arc, Mutex};
    use tauri::Manager;

    // 真实跑一次「一键开始」：代理 → 虚拟登录 → 原生启动 → 健康与身份校验 → 停止。
    // 上游指向本机 mock，不碰任何真实 provider 凭据。
    let Ok(upstream_port) = std::env::var("CSSWITCH_WIN_E2E_UPSTREAM_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or(())
    else {
        panic!("需要 CSSWITCH_WIN_E2E_UPSTREAM_PORT（mock 上游端口）");
    };

    // 备份真实配置目录，跑完原样恢复，绝不把测试配置留给用户。
    let config_dir = crate::config::default_dir();
    let backup = std::env::temp_dir().join(format!("csswitch-oneclick-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&backup);
    let had_config = config_dir.exists();
    if had_config {
        copy_tree(&config_dir, &backup).expect("备份配置目录");
    }
    let guard = ConfigDirGuard {
        config_dir: config_dir.clone(),
        backup: had_config.then(|| backup.clone()),
    };

    let (model_catalog, default_model_route_id, role_bindings) =
        crate::model_catalog::new_profile_catalog("custom", "anthropic", Some("win-e2e-model"))
            .or_else(|_| crate::model_catalog::new_profile_catalog("custom", "anthropic", None))
            .expect("custom 模板必须能生成模型目录");
    let model = model_catalog
        .iter()
        .find(|route| route.selector_id == default_model_route_id)
        .map(|route| route.upstream_model.clone())
        .unwrap_or_else(|| "win-e2e-model".to_string());
    let profile = crate::config::Profile {
        id: "win-e2e".into(),
        name: "win-e2e".into(),
        template_id: "custom".into(),
        category: "custom".into(),
        api_format: "anthropic".into(),
        base_url: format!("http://127.0.0.1:{upstream_port}"),
        api_key: "win-e2e-key".into(),
        model,
        model_catalog,
        default_model_route_id,
        role_bindings,
        model_policy: crate::provider_contracts::ModelPolicy::SavedCatalog,
        ..Default::default()
    };
    let proxy_port = upstream_port + 1;
    let sandbox_port = upstream_port + 2;
    let cfg = crate::config::Config {
        profiles: vec![profile.clone()],
        active_id: profile.id.clone(),
        mode: "proxy".into(),
        proxy_port,
        sandbox_port,
        ..Default::default()
    };
    crate::config::save_to(&config_dir, &cfg).expect("写入测试配置");

    let state = Arc::new(Mutex::new(super::AppState::default())) as super::SharedAppState;
    let lifecycle = Arc::new(crate::lifecycle::Lifecycle::new());
    let app = tauri::test::mock_builder()
        .manage(state.clone())
        .manage(lifecycle.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();

    let result = crate::runtime::sandbox_session::one_click_login(
        handle.clone(),
        state.clone(),
        lifecycle.as_ref(),
        None,
        None,
    );
    let value = match result {
        Ok(value) => value,
        Err(error) => {
            drop(guard);
            panic!("一键开始失败：{error}");
        }
    };
    assert_eq!(value["status"], "ok", "一键开始必须返回 status=ok：{value}");
    assert_eq!(value["action"], "started");

    let runtime = crate::lock(&state)
        .science_runtime
        .clone()
        .expect("必须记录实际启动的 runtime 身份");
    assert_eq!(
        crate::runtime::science::probe_known_runtime(sandbox_port, &runtime),
        crate::runtime::science::SandboxScienceState::RunningHealthy,
        "启动后的监听者必须被证明是选定 runtime"
    );
    let data_dir = crate::runtime::science::sandbox_data_dir();
    assert!(data_dir.join("encryption.key").is_file(), "虚拟登录必须已写入隔离 data-dir");
    assert!(data_dir.join(".oauth-tokens").is_dir());

    // 停下并确认端口真的关闭（与产品路径同一个 stop 实现）。
    let mut no_child = None;
    let mut no_url = None;
    crate::runtime::science::stop_sandbox(&handle, &mut no_child, &mut no_url, Some(&runtime))
        .expect("停止受管 Science");
    let stopped = crate::runtime::science::probe_known_runtime(sandbox_port, &runtime)
        != crate::runtime::science::SandboxScienceState::RunningHealthy;
    drop(guard);
    assert!(stopped, "stop 之后不应再是 RunningHealthy");
}

struct ConfigDirGuard {
    config_dir: std::path::PathBuf,
    backup: Option<std::path::PathBuf>,
}

impl Drop for ConfigDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.config_dir);
        if let Some(backup) = &self.backup {
            let _ = copy_tree(backup, &self.config_dir);
            let _ = std::fs::remove_dir_all(backup);
        }
    }
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[test]
fn config_save_then_load_roundtrips_in_temp_dir() {
    let dir = std::env::temp_dir().join(format!("csswitch-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = crate::config::Config::default();
    crate::config::save_to(&dir, &cfg).expect("保存配置");
    let path = dir.join("config.json");
    assert!(path.is_file(), "保存后必须存在 config.json");
    let plain = std::fs::read(&path).expect("普通 std 读取必须成功");
    assert!(plain.starts_with(b"{"), "配置必须是 JSON");
    let listing: Vec<String> = std::fs::read_dir(&dir)
        .expect("列目录")
        .filter_map(|entry| entry.ok().map(|entry| {
            let md = entry.metadata().ok();
            format!(
                "{} ({} bytes, readonly={:?})",
                entry.file_name().to_string_lossy(),
                md.as_ref().map(|md| md.len()).unwrap_or(0),
                md.as_ref().map(|md| md.permissions().readonly())
            )
        }))
        .collect();
    eprintln!("临时配置目录内容: {listing:?}");
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(crate::platform::O_NOFOLLOW)
        .open(&path)
        .expect("按平台的 no-follow 标志打开必须成功");
    drop(opened);
    let loaded = crate::config::load_from(&dir).expect("读回配置");
    assert_eq!(loaded.schema_version, cfg.schema_version);
    // 二次保存必须能覆盖旧文件（Windows 上 rename 到已存在目标需要权限正确）。
    crate::config::save_to(&dir, &cfg).expect("覆盖保存配置");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn science_binary_candidates_prefer_official_install_location() {
    let candidates = crate::runtime::science::test_science_bin_candidates();
    let local_app = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .expect("Windows 上必须有 LOCALAPPDATA");
    assert_eq!(
        candidates.first().cloned(),
        Some(
            local_app
                .join("Programs")
                .join("ClaudeScience")
                .join("claude-science.exe")
        ),
        "首选位置必须是官方安装器的 %LOCALAPPDATA%\\Programs\\ClaudeScience"
    );
}
