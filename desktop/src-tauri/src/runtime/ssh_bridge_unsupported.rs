//! Windows 上的系统 SSH 桥接占位实现。
//!
//! CSSwitch 的 SSH 桥接依赖 POSIX 语义：`~/.ssh/config`、`Include` 指令、
//! `chmod 600` 的 wrapper 脚本、以及把 wrapper 目录前置到 `PATH`。Windows 上
//! Science 的沙箱不读这些入口，因此这里**明确报告不支持**，而不是伪造一个
//! 看起来能用、实际不生效的桥接。
//!
//! 所有函数都只在用户显式开启「复用系统 SSH 配置」时才会被调用；默认关闭时
//! 上层根本不会走到这里。

use std::path::Path;

const UNSUPPORTED: &str =
    "系统 SSH 配置复用目前只在 macOS / Linux 上提供；Windows 版请关闭「复用系统 SSH 配置」。";

pub(crate) fn is_concrete_alias(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    value.len() <= 255
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '@' | '%' | '+' | '-'))
}

pub(crate) fn system_ssh_hosts_for_home(_home: &Path) -> Result<Vec<String>, String> {
    Err(UNSUPPORTED.to_string())
}

pub(crate) fn system_ssh_hosts() -> Result<Vec<String>, String> {
    Err(UNSUPPORTED.to_string())
}

pub(crate) fn system_ssh_bridge_fingerprint(_enabled: bool) -> Result<String, String> {
    Err(UNSUPPORTED.to_string())
}

pub(crate) fn prepare_science_ssh_bridge_for(
    _sandbox_home: &Path,
    _system_home: &Path,
) -> Result<Vec<String>, String> {
    Err(UNSUPPORTED.to_string())
}

pub(crate) fn prepare_science_ssh_bridge(_sandbox_home: &Path) -> Result<Vec<String>, String> {
    Err(UNSUPPORTED.to_string())
}

pub(crate) fn prevalidate_science_ssh_bridge(
    _sandbox_home: &Path,
    enabled: bool,
) -> Result<Vec<String>, String> {
    if enabled {
        return Err(UNSUPPORTED.to_string());
    }
    Ok(Vec::new())
}

pub(crate) fn revoke_science_ssh_bridge(_sandbox_home: &Path) -> Result<(), String> {
    // 撤销是无害操作：Windows 上从不创建桥接，因此直接成功。
    Ok(())
}

pub(crate) fn validate_science_ssh_bridge(_sandbox_home: &Path) -> Result<Vec<String>, String> {
    Err(UNSUPPORTED.to_string())
}
