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
