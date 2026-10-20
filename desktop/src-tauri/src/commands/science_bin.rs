//! 「Claude Science 程序路径」相关命令。
//!
//! macOS 上 Science 固定在 `/Applications/Claude Science.app`，不需要配置。
//! Windows 上它可能来自官方安装器（`%LOCALAPPDATA%\Programs\ClaudeScience\`），
//! 也可能只是一份用户自己下载的单文件 exe，因此必须能显式指定：
//!
//! * [`science_bin_status`] 报告当前会使用哪一个、来源是什么、版本是多少；
//! * [`choose_science_bin`] 打开系统文件选择器，校验（格式 + 大小 + `--version`）
//!   通过后才持久化；
//! * [`clear_science_bin`] 撤掉用户固定，回到自动发现。

use serde_json::{json, Value};

use crate::runtime::science;

fn describe(path: Option<std::path::PathBuf>, source: &str) -> Value {
    let Some(path) = path else {
        return json!({
            "schema_version": 1,
            "found": false,
            "source": "none",
            "path": Value::Null,
            "version": Value::Null,
            "platform": std::env::consts::OS,
        });
    };
    let version = science::safe_science_version_public(&path);
    json!({
        "schema_version": 1,
        "found": true,
        "source": source,
        "path": path.to_string_lossy(),
        "version": version,
        "platform": std::env::consts::OS,
    })
}

/// 当前生效的 Science 可执行文件（env 覆盖 > 用户固定 > 自动发现）。
#[tauri::command]
pub(crate) fn science_bin_status() -> Result<Value, String> {
    if let Some(explicit) = std::env::var_os("SCIENCE_BIN") {
        let path = std::path::PathBuf::from(explicit);
        return Ok(describe(Some(path), "env"));
    }
    if let Some(persisted) = science::persisted_science_bin() {
        return Ok(describe(Some(persisted), "user"));
    }
    let discovered = science::installed_science_bin();
    if discovered.is_file() {
        Ok(describe(Some(discovered), "auto"))
    } else {
        Ok(describe(None, "none"))
    }
}

/// 让用户挑一个 Claude Science 程序；只有通过完整校验才会保存。
#[tauri::command]
pub(crate) async fn choose_science_bin(app: tauri::AppHandle) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = crate::run_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title("选择 Claude Science 程序（claude-science.exe）");
        #[cfg(windows)]
        {
            dialog = dialog.add_filter("Claude Science", &["exe"]);
        }
        Ok(dialog.blocking_pick_file())
    })
    .await?;
    let Some(picked) = picked else {
        return Ok(json!({
            "schema_version": 1,
            "status": "cancelled",
            "unchanged": true,
        }));
    };
    let path = picked
        .into_path()
        .map_err(|error| format!("无法解析所选路径：{error}"))?;
    let version = science::validate_science_binary_candidate(&path)?;
    science::set_persisted_science_bin(&path)?;
    Ok(json!({
        "schema_version": 1,
        "status": "saved",
        "path": path.to_string_lossy(),
        "version": version,
    }))
}

/// 撤销用户固定的路径，回到自动发现（或 `SCIENCE_BIN`）。
#[tauri::command]
pub(crate) fn clear_science_bin() -> Result<Value, String> {
    science::clear_persisted_science_bin()?;
    Ok(json!({ "schema_version": 1, "status": "cleared" }))
}
