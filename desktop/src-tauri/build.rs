use std::path::{Path, PathBuf};
use std::process::Command;

fn gateway_bin_name() -> &'static str {
    if cfg!(windows) {
        "csswitch-gateway.exe"
    } else {
        "csswitch-gateway"
    }
}

fn stage_gateway_sidecar() {
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out_dir =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("Desktop build must provide an OUT_DIR"));
    let manifest_dir = PathBuf::from(manifest_dir);
    let gateway_dir = manifest_dir.join("../gateway");
    if !gateway_dir.join("Cargo.toml").is_file() {
        return;
    }

    println!(
        "cargo:rerun-if-changed={}",
        gateway_dir.join("Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        gateway_dir.join("src").display()
    );
    println!("cargo:rerun-if-env-changed=CSSWITCH_SKIP_GATEWAY_STAGE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_ACCEPTANCE_BUILD");
    let acceptance_build = std::env::var_os("CARGO_FEATURE_ACCEPTANCE_BUILD").is_some();
    if std::env::var_os("CSSWITCH_SKIP_GATEWAY_STAGE").is_some() {
        panic!(
            "CSSwitch builds cannot skip Gateway staging; Desktop and Gateway build variants must match"
        );
    }

    // Never inherit the outer CARGO_TARGET_DIR for the nested build and then
    // read from a different default target directory. A private target root
    // under this exact Desktop build binds the copied sidecar to this build,
    // target triple, and feature selection.
    let gateway_target_dir = out_dir.join("gateway-target");
    let mut command = Command::new(&cargo);
    command
        .current_dir(&gateway_dir)
        .env("CARGO_TARGET_DIR", &gateway_target_dir)
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg(&target);
    if acceptance_build {
        command.arg("--features").arg("acceptance-build");
    }
    let status = command.status();
    if !matches!(status, Ok(s) if s.success()) {
        panic!("failed to build csswitch-gateway sidecar for target {target}");
    }

    let built = gateway_target_dir
        .join(&target)
        .join("release")
        .join(gateway_bin_name());
    let out_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&out_dir).expect("failed to create sidecar binaries dir");
    let mut staged_name = format!("csswitch-gateway-{target}");
    if cfg!(windows) {
        staged_name.push_str(".exe");
    }
    let staged = out_dir.join(staged_name);
    copy_executable(&built, &staged);
}

fn copy_executable(src: &Path, dst: &Path) {
    std::fs::copy(src, dst).unwrap_or_else(|e| {
        panic!(
            "failed to stage csswitch-gateway sidecar from {} to {}: {e}",
            src.display(),
            dst.display()
        )
    });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dst)
            .expect("failed to stat staged csswitch-gateway sidecar")
            .permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(dst, perms)
            .expect("failed to mark staged csswitch-gateway sidecar executable");
    }
}

fn main() {
    stage_gateway_sidecar();
    stage_windows_webview2_loader();
    embed_windows_comctl32_manifest();
    tauri_build::build()
}

/// windows-gnu 构建会动态链接 `WebView2Loader.dll`（MSVC 构建是静态链接）。
/// 因此必须把 DLL 放到可执行文件旁边，否则进程一启动就是 `0xC0000135`。
/// Tauri 打包器不会为 GNU 目标自动做这一步，这里在构建期补齐。
fn stage_windows_webview2_loader() {
    if !cfg!(windows) {
        return;
    }
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    if !target.contains("windows-gnu") || !target.starts_with("x86_64") {
        return;
    }
    let Some(source) = locate_webview2_loader() else {
        println!(
            "cargo:warning=找不到 WebView2Loader.dll：windows-gnu 产物启动时会报 0xC0000135。"
        );
        return;
    };
    let Ok(out_dir) = std::env::var("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out → target/<profile>
    if let Some(profile_dir) = out_dir.ancestors().nth(3) {
        let _ = std::fs::copy(&source, profile_dir.join("WebView2Loader.dll"));
    }
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let binaries = PathBuf::from(manifest_dir).join("binaries");
        let _ = std::fs::create_dir_all(&binaries);
        let _ = std::fs::copy(&source, binaries.join("WebView2Loader.dll"));
    }
}

/// 找 WebView2Loader.dll：显式环境变量 → 仓库内置副本 → cargo registry 里的
/// `webview2-com-sys` 副本。
fn locate_webview2_loader() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("CSSWITCH_WEBVIEW2_LOADER_DLL") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let vendored = PathBuf::from(&manifest_dir).join("resources").join("WebView2Loader.dll");
        if vendored.is_file() {
            return Some(vendored);
        }
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".cargo")))?;
    let registry_src = cargo_home.join("registry").join("src");
    let registries = std::fs::read_dir(registry_src).ok()?;
    for registry in registries.flatten() {
        let Ok(crates) = std::fs::read_dir(registry.path()) else {
            continue;
        };
        for entry in crates.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("webview2-com-sys-") {
                continue;
            }
            let candidate = entry.path().join("x64").join("WebView2Loader.dll");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 在 windows-gnu 工具链上嵌入应用清单（manifest）。///
/// 必需原因：Tauri / winit 会导入 `comctl32.dll` 的 `TaskDialogIndirect`、
/// `SetWindowSubclass` 等符号，而它们只在 **Common Controls v6**（带清单加载）
/// 中存在。不带清单时进程会以 `STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139)` 直接
/// 退出——包括 `cargo test` 的测试二进制。MSVC 工具链由 tauri-build 写清单，
/// GNU 工具链没有对应机制，因此这里用 `windres` 把 RT_MANIFEST 资源编译成
/// 目标文件并链入所有可链接目标（exe 与测试体）。
fn embed_windows_comctl32_manifest() {
    if !cfg!(windows) {
        return;
    }
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    if !target.contains("windows-gnu") {
        return;
    }
    let Ok(out_dir) = std::env::var("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let manifest = out_dir.join("csswitch-app.manifest");
    let rc = out_dir.join("csswitch-app.rc");
    let obj = out_dir.join("csswitch-app-manifest.o");

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="0.8.4.0" processorArchitecture="amd64" name="CSSwitch" type="win32"/>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="amd64" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">permonitorv2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
</assembly>
"#;

    if std::fs::write(&manifest, MANIFEST).is_err() {
        return;
    }
    if std::fs::write(&rc, "1 24 \"csswitch-app.manifest\"\n").is_err() {
        return;
    }

    let windres = local_windres();
    let Some(windres) = windres else {
        println!(
            "cargo:warning=windres 未找到：windows-gnu 构建的可执行文件将缺少 Common Controls v6 清单，可能以 0xC0000139 退出"
        );
        return;
    };
    let status = Command::new(&windres)
        .current_dir(&out_dir)
        .arg("--input")
        .arg(&rc)
        .arg("--output")
        .arg(&obj)
        .arg("--output-format=coff")
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("cargo:warning=windres 编译清单失败：windows-gnu 目标将缺少 Common Controls v6 清单");
        return;
    }
    // `cargo:rustc-link-arg` 作用于所有可链接目标（bin / test / bench / example）。
    // 本仓库的 cargo 版本不提供 `rustc-link-arg-tests`，而测试二进制同样需要这份
    // 清单（否则 `cargo test` 直接以 0xC0000139 退出）。副作用：正式 bin 会同时
    // 带有 tauri-build 自己的清单，ld 只会打印一条
    // ".rsrc merge failure: multiple non-default manifests" 警告，产物仍正确。
    println!("cargo:rustc-link-arg={}", obj.display());
    println!("cargo:rerun-if-changed={}", rc.display());
}

/// 找 windres：优先显式环境变量，其次 PATH，最后常见的 MinGW 安装位置
/// （自建环境可按需设置 `WINDRES`，不必改代码）。
fn local_windres() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("WINDRES") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let from_path = Command::new("windres.exe").arg("--version").output();
    if matches!(&from_path, Ok(output) if output.status.success()) {
        return Some(PathBuf::from("windres.exe"));
    }
    // 常见位置仅作便利候选；不存在就跳过。
    let mut candidates: Vec<PathBuf> = Vec::new();
    for base in [
        std::env::var_os("W64DEVKIT_HOME").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(|value| PathBuf::from(value).join("w64devkit")),
        std::env::var_os("SystemDrive").map(|value| PathBuf::from(value).join("msys64")),
    ]
    .into_iter()
    .flatten()
    {
        candidates.push(base.join("w64devkit").join("bin").join("windres.exe"));
        candidates.push(base.join("bin").join("windres.exe"));
        candidates.push(base.join("mingw64").join("bin").join("windres.exe"));
    }
    candidates.into_iter().find(|path| path.is_file())
}
