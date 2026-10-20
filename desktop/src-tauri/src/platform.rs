//! 平台原语抽象层（macOS / Linux / Windows）。
//!
//! CSSwitch 原本直接依赖 POSIX：`libc::geteuid`、`O_NOFOLLOW`、`MetadataExt`
//! 的 `dev/ino/mode/uid`、`lsof`、`ps`、`codesign`。这一层把这些原语收敛成
//! 一组**语义等价**的跨平台接口，让上层安全校验（fail closed 的身份检查）
//! 在两个平台上保持同样的形状：
//!
//! | 语义 | POSIX | Windows |
//! | --- | --- | --- |
//! | 当前用户身份 | `geteuid()` | 当前进程 token 的 user SID（哈希成稳定 u32） |
//! | 文件身份 | `(st_dev, st_ino)` | `(volume_serial, file_index)` |
//! | 权限位 | `st_mode & 0o777` | 由只读属性 + 文件类型推导（见 [`MetadataExt::mode`]） |
//! | 不跟随链接打开 | `O_NOFOLLOW` | `FILE_FLAG_OPEN_REPARSE_POINT` |
//! | 监听端口 → PID | `lsof -t` | `netstat -ano`（按 `0.0.0.0:0` 对端识别 LISTENING） |
//! | 进程启动身份 | `ps -o lstart=` | `GetProcessTimes` 创建时间（100ns） |
//! | 可执行文件格式 | Mach-O magic | PE32/PE32+ magic |
//! | 结束进程 | `SIGTERM`/`SIGKILL` | `TerminateProcess`（先复核身份，调用方负责） |
//!
//! Windows 上不存在 group/world 权限位，因此 `mode()` 保留 `0o022`/`0o111`
//! 的形状但不表达跨用户语义；真正的"这条路径属于当前用户且不是链接"检查
//! 由 [`verify_private_path`] 用 ACL owner SID + reparse point 属性完成。

#![allow(dead_code, reason = "两侧平台原语各自按需使用")]

use std::io;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// POSIX：直接复用 std / libc 的实现
// ---------------------------------------------------------------------------

#[cfg(unix)]
pub(crate) use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

#[cfg(unix)]
pub(crate) const O_NOFOLLOW: i32 = libc::O_NOFOLLOW;
#[cfg(unix)]
pub(crate) const O_CLOEXEC: i32 = libc::O_CLOEXEC;
#[cfg(unix)]
pub(crate) const O_NONBLOCK: i32 = libc::O_NONBLOCK;
#[cfg(unix)]
pub(crate) const O_RDONLY: i32 = libc::O_RDONLY;
#[cfg(unix)]
pub(crate) const O_WRONLY: i32 = libc::O_WRONLY;
#[cfg(unix)]
pub(crate) const O_CREAT: i32 = libc::O_CREAT;
#[cfg(unix)]
pub(crate) const O_EXCL: i32 = libc::O_EXCL;
#[cfg(unix)]
pub(crate) const O_DIRECTORY: i32 = libc::O_DIRECTORY;
#[cfg(unix)]
pub(crate) const S_IFMT: u32 = libc::S_IFMT as u32;
#[cfg(unix)]
pub(crate) const S_IFDIR: u32 = libc::S_IFDIR as u32;
#[cfg(unix)]
pub(crate) const S_IFREG: u32 = libc::S_IFREG as u32;
#[cfg(unix)]
pub(crate) const S_IFLNK: u32 = libc::S_IFLNK as u32;

/// "不跟随链接 + close-on-exec" 的组合标志。
#[cfg(unix)]
pub(crate) const NOFOLLOW_CLOEXEC: i32 = libc::O_NOFOLLOW | libc::O_CLOEXEC;

#[cfg(unix)]
pub(crate) fn current_user_id() -> u32 {
    // SAFETY: geteuid 没有前置条件，也不解引用指针。
    unsafe { libc::geteuid() }
}

/// 目录 fsync：POSIX 上用它可以证明 rename 已落盘。
#[cfg(unix)]
pub(crate) fn sync_dir(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(unix)]
pub(crate) fn is_executable_file_mode(mode: u32) -> bool {
    mode & 0o111 != 0
}

/// POSIX 上"不可被 group/world 写入"的既有判定。
#[cfg(unix)]
pub(crate) fn mode_is_private(mode: u32) -> bool {
    mode & 0o022 == 0
}

#[cfg(unix)]
pub(crate) fn path_is_reparse_or_link(path: &Path) -> bool {
    path.symlink_metadata()
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(true)
}

#[cfg(unix)]
pub(crate) fn verify_private_path(path: &Path) -> Result<(), String> {
    // POSIX 上由调用方既有的 uid/mode 检查覆盖；这里只补链接检查。
    if path_is_reparse_or_link(path) {
        return Err("路径包含符号链接".into());
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn unique_listener_pid(port: u16) -> Option<u32> {
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-t"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    crate::runtime::science::parse_unique_listener_pid(&String::from_utf8(output.stdout).ok()?)
}

#[cfg(unix)]
pub(crate) fn process_start_identity(pid: u32) -> Option<String> {
    if pid <= 1 {
        return None;
    }
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .env_clear()
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 256 || !output.stderr.is_empty() {
        return None;
    }
    let identity = String::from_utf8(output.stdout).ok()?;
    let identity = identity.trim();
    if identity.is_empty()
        || identity.len() > 128
        || !identity.is_ascii()
        || identity.lines().count() != 1
    {
        return None;
    }
    Some(identity.to_string())
}

#[cfg(unix)]
pub(crate) fn process_image_path(pid: u32) -> Option<PathBuf> {
    if pid <= 1 {
        return None;
    }
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .env_clear()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

#[cfg(unix)]
pub(crate) fn terminate_process(pid: u32, force: bool) -> bool {
    if pid <= 1 {
        return false;
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: kill 只接收 pid 与信号编号。
    unsafe { libc::kill(pid as i32, signal) == 0 }
}

#[cfg(unix)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    // SAFETY: signal 0 只做存在性与权限探测。
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// POSIX 上 Mach-O magic 是"这是一个真正的 Science 可执行文件"的格式门。
#[cfg(unix)]
pub(crate) fn science_binary_format_ok(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return false;
    }
    matches!(
        magic,
        [0xfe, 0xed, 0xfa, 0xce]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xcf, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xbe, 0xba, 0xfe, 0xca]
            | [0xca, 0xfe, 0xba, 0xbf]
            | [0xbf, 0xba, 0xfe, 0xca]
    )
}

/// POSIX 用 codesign 的 embedded identity 做误选防护。Windows 上没有对应
/// 概念（未签名的小工具很常见），调用方据 [`HAS_EMBEDDED_IDENTITY`] 跳过。
#[cfg(unix)]
pub(crate) const HAS_EMBEDDED_IDENTITY: bool = true;

/// 让子进程看到一个隔离的"用户家目录"。
#[cfg(unix)]
pub(crate) fn apply_isolated_home<'a>(
    command: &'a mut std::process::Command,
    home: &Path,
) -> &'a mut std::process::Command {
    command.env("HOME", home)
}

#[cfg(unix)]
pub(crate) fn rename_no_clobber(source: &Path, target: &Path) -> Result<(), String> {
    // renameatx_np / RENAME_EXCL 在 macOS 上提供 no-clobber 语义；Rust std 没有。
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let source_c = CString::new(source.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let target_c = CString::new(target.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: 两个路径都以 NUL 结尾且指向有效内存。
    let rc = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            source_c.as_ptr(),
            libc::AT_FDCWD,
            target_c.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().to_string())
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub(crate) const O_NOFOLLOW: i32 = 0;
#[cfg(windows)]
pub(crate) const O_CLOEXEC: i32 = 0;
#[cfg(windows)]
pub(crate) const O_NONBLOCK: i32 = 0;
#[cfg(windows)]
pub(crate) const O_RDONLY: i32 = 0;
#[cfg(windows)]
pub(crate) const O_WRONLY: i32 = 1;
#[cfg(windows)]
pub(crate) const O_CREAT: i32 = 0x0100;
#[cfg(windows)]
pub(crate) const O_EXCL: i32 = 0x0200;
#[cfg(windows)]
pub(crate) const O_DIRECTORY: i32 = 0;
#[cfg(windows)]
pub(crate) const NOFOLLOW_CLOEXEC: i32 = 0;

// Windows 的 `mode()` 是推导值，但仍保持 `S_IFMT` 的形状，让上层的
// `mode & S_IFMT == S_IFDIR` 之类的判断继续成立。
#[cfg(windows)]
pub(crate) const S_IFMT: u32 = 0o170000;
#[cfg(windows)]
pub(crate) const S_IFDIR: u32 = 0o040000;
#[cfg(windows)]
pub(crate) const S_IFREG: u32 = 0o100000;
#[cfg(windows)]
pub(crate) const S_IFLNK: u32 = 0o120000;

#[cfg(windows)]
pub(crate) fn is_executable_file_mode(mode: u32) -> bool {
    // Windows 的可执行性由扩展名决定，`mode()` 里用 0o111 表达"可执行"。
    mode & 0o111 != 0
}

#[cfg(windows)]
pub(crate) fn mode_is_private(mode: u32) -> bool {
    // 不存在 group/world 位；保持形状（由 ACL / reparse 检查承担真实判定）。
    mode & 0o022 == 0
}

#[cfg(windows)]
pub(crate) const HAS_EMBEDDED_IDENTITY: bool = false;

#[cfg(windows)]
pub(crate) fn sync_dir(_path: &Path) -> io::Result<()> {
    // Windows 不允许把目录当文件打开做 fsync；MoveFileEx 已经保证元数据落盘。
    Ok(())
}

#[cfg(windows)]
pub(crate) fn rename_no_clobber(source: &Path, target: &Path) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let to_wide = |path: &Path| -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let source_w = to_wide(source);
    let target_w = to_wide(target);
    // SAFETY: 两个宽字符串都以 NUL 结尾；MOVEFILE_REPLACE_EXISTING 不传，
    // 因此目标已存在时调用失败（no-clobber）。
    let ok = unsafe { MoveFileExW(source_w.as_ptr(), target_w.as_ptr(), 0) };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error().to_string())
    }
}

#[cfg(windows)]
pub(crate) fn path_is_reparse_or_link(path: &Path) -> bool {
    // Rust 的 `FileType::is_symlink()` 在 Windows 上同时覆盖
    // IO_REPARSE_TAG_SYMLINK 与 IO_REPARSE_TAG_MOUNT_POINT（junction / mount
    // point），但不会把 OneDrive 之类的 cloud placeholder 当成链接。
    match path.symlink_metadata() {
        Ok(metadata) => metadata.file_type().is_symlink(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

/// 可执行性判定：POSIX 看 `mode & 0o111`，Windows 看扩展名。
#[cfg(unix)]
pub(crate) fn path_is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
pub(crate) fn path_is_executable(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "exe" | "com" | "cmd" | "bat"
    )
}

#[cfg(windows)]
pub(crate) fn apply_isolated_home<'a>(
    command: &'a mut std::process::Command,
    home: &Path,
) -> &'a mut std::process::Command {
    // Science 在 win32 上通过 USERPROFILE / APPDATA / LOCALAPPDATA / TEMP 定位
    // 用户态状态（见 `os.homedir()` 与 win32 环境允许表），只改 HOME 不构成隔离。
    let local_app = home.join("AppData").join("Local");
    let temp = local_app.join("Temp");
    let _ = std::fs::create_dir_all(&temp);
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("APPDATA", home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", &local_app)
        .env("TEMP", &temp)
        .env("TMP", &temp)
}

#[cfg(windows)]
mod win {
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, FILETIME, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, OpenProcessToken,
        QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, OWNER_SECURITY_INFORMATION, TOKEN_QUERY, PSID};

    fn sid_to_string(sid: PSID) -> Option<String> {
        if sid.is_null() {
            return None;
        }
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: sid 来自成功的安全 API；返回的字符串由 LocalFree 释放。
        let ok = unsafe { ConvertSidToStringSidW(sid, &mut text) };
        if ok == 0 || text.is_null() {
            return None;
        }
        let result = {
            let mut len = 0usize;
            // SAFETY: 上面已确认 text 是 NUL 结尾的宽字符串。
            unsafe {
                while *text.add(len) != 0 {
                    len += 1;
                }
                Some(String::from_utf16_lossy(std::slice::from_raw_parts(text, len)))
            }
        };
        // SAFETY: text 由 ConvertSidToStringSidW 分配。
        unsafe { LocalFree(text as *mut core::ffi::c_void) };
        result
    }

    fn current_user_sid_string() -> Option<String> {
        static CACHE: OnceLock<Option<String>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                let mut token: HANDLE = std::ptr::null_mut();
                // SAFETY: 只取当前进程伪句柄的 token。
                let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
                if opened == 0 {
                    return None;
                }
                let mut size = 0u32;
                // SAFETY: 首次调用只为获得所需缓冲区大小。
                unsafe {
                    GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
                }
                if size == 0 {
                    unsafe { CloseHandle(token) };
                    return None;
                }
                let mut buffer = vec![0u8; size as usize];
                // SAFETY: 缓冲区大小来自上一次调用。
                let ok = unsafe {
                    GetTokenInformation(
                        token,
                        TokenUser,
                        buffer.as_mut_ptr() as *mut core::ffi::c_void,
                        size,
                        &mut size,
                    )
                };
                unsafe { CloseHandle(token) };
                if ok == 0 {
                    return None;
                }
                // SAFETY: TokenUser 返回 TOKEN_USER，其第一个字段就是 SID 指针。
                let token_user = buffer.as_ptr() as *const windows_sys::Win32::Security::TOKEN_USER;
                sid_to_string(unsafe { (*token_user).User.Sid })
            })
            .clone()
    }

    pub(super) fn current_user_id() -> u32 {
        // FNV-1a：把用户 SID 折叠成稳定的 u32，仅用于“是否同一个用户”的比较。
        match current_user_sid_string() {
            Some(sid) => {
                let mut hash: u32 = 0x811c_9dc5;
                for byte in sid.as_bytes() {
                    hash ^= *byte as u32;
                    hash = hash.wrapping_mul(0x0100_0193);
                }
                hash
            }
            None => 0,
        }
    }

    fn path_owner_sid_string(path: &Path) -> Option<String> {
        let wide: Vec<u16> = {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: 宽字符串以 NUL 结尾；成功时 descriptor 需要 LocalFree。
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let result = sid_to_string(owner);
        if !descriptor.is_null() {
            // SAFETY: descriptor 由 GetNamedSecurityInfoW 分配。
            unsafe { LocalFree(descriptor) };
        }
        result
    }

    pub(super) fn owner_is_current_user(path: &Path) -> bool {
        match (path_owner_sid_string(path), current_user_sid_string()) {
            (Some(owner), Some(user)) => owner.eq_ignore_ascii_case(&user),
            _ => false,
        }
    }

    fn open_query(pid: u32) -> HANDLE {
        if pid <= 1 {
            return std::ptr::null_mut();
        }
        // SAFETY: 只读查询；调用方负责关闭句柄。
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) }
    }

    pub(super) fn process_start_identity(pid: u32) -> Option<String> {
        let handle = open_query(pid);
        if handle.is_null() {
            return None;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        let ticks = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        if ticks == 0 {
            return None;
        }
        Some(format!("win32-created:{ticks}"))
    }

    pub(super) fn process_image_path(pid: u32) -> Option<PathBuf> {
        let handle = open_query(pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32 * 1024];
        let mut size = buffer.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        buffer.truncate(size as usize);
        Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
    }

    pub(super) fn terminate_process(pid: u32, force: bool) -> bool {
        if pid <= 1 {
            return false;
        }
        if !force {
            // Windows 没有 SIGTERM。非强制路径由调用方轮询端口，随后走强制终止。
            return true;
        }
        // SAFETY: 只对调用方已复核身份的 pid 使用。
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let ok = unsafe { TerminateProcess(handle, 1) };
        if ok != 0 {
            // 等它真的消失，避免“信号已发但进程仍在监听”的假成功。
            unsafe { WaitForSingleObject(handle, 5_000) };
        }
        unsafe { CloseHandle(handle) };
        ok != 0
    }

    pub(super) fn process_is_alive(pid: u32) -> bool {
        let handle = open_query(pid);
        if handle.is_null() {
            return false;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
        let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
        unsafe { CloseHandle(handle) };
        ok != 0
    }

    pub(super) fn current_pid() -> u32 {
        // SAFETY: 无参数。
        unsafe { GetCurrentProcessId() }
    }

    /// 文件身份：`(volume_serial, file_index, link_count)`。
    /// 用 `GetFileInformationByHandle`（稳定 API）而不是 std 里仍不稳定的
    /// `windows_by_handle` 访问器。
    pub(super) fn file_identity(path: &Path) -> Option<(u64, u64, u32)> {
        use std::os::windows::fs::OpenOptionsExt as WinOpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .ok()?;
        use std::os::windows::io::AsRawHandle;
        let handle = file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: handle 来自仍然存活的 File；info 是有效可写结构。
        let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
        if ok == 0 {
            return None;
        }
        let index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
        Some((info.dwVolumeSerialNumber as u64, index, info.nNumberOfLinks))
    }
}

#[cfg(windows)]
pub(crate) fn current_user_id() -> u32 {
    win::current_user_id()
}

#[cfg(windows)]
pub(crate) fn process_start_identity(pid: u32) -> Option<String> {
    win::process_start_identity(pid)
}

#[cfg(windows)]
pub(crate) fn process_image_path(pid: u32) -> Option<PathBuf> {
    win::process_image_path(pid)
}

#[cfg(windows)]
pub(crate) fn terminate_process(pid: u32, force: bool) -> bool {
    win::terminate_process(pid, force)
}

#[cfg(windows)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    win::process_is_alive(pid)
}

#[cfg(windows)]
pub(crate) fn current_pid() -> u32 {
    win::current_pid()
}

#[cfg(unix)]
pub(crate) fn current_pid() -> u32 {
    std::process::id()
}

/// 真实用户的家目录：POSIX 上是 `HOME`，Windows 上是 `USERPROFILE`。/// 隔离沙箱靠重定向这些变量生效，因此“真实目录”必须按平台取。
#[cfg(unix)]
pub(crate) fn real_user_home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|value| !value.is_empty()).map(PathBuf::from)
}

#[cfg(not(unix))]
pub(crate) fn real_user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("HOME").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// 加密安全随机字节。POSIX 读 `/dev/urandom`；Windows 用 `getrandom`
/// （内部就是 `BCryptGenRandom`，不依赖任何文件路径）。
#[cfg(unix)]
pub(crate) fn random_bytes(n: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open("/dev/urandom")?;
    let mut buffer = vec![0u8; n];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

#[cfg(windows)]
pub(crate) fn random_bytes(n: usize) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; n];
    getrandom::getrandom(&mut buffer).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(buffer)
}

/// 端口 → 唯一监听 PID。Windows 上没有 lsof，用 netstat 的稳定形态：
/// LISTENING 行的对端地址恒为 `0.0.0.0:0`，与系统语言无关。
#[cfg(windows)]
pub(crate) fn unique_listener_pid(port: u16) -> Option<u32> {
    let output = std::process::Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut pids = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("TCP") {
            continue;
        }
        let Some(local) = fields.next() else { continue };
        let Some(remote) = fields.next() else { continue };
        if !remote.ends_with(":0") || !remote.starts_with("0.0.0.0") {
            continue;
        }
        let Some(local_port) = local.rsplit(':').next() else { continue };
        if local_port.parse::<u16>().ok() != Some(port) {
            continue;
        }
        // 最后一个字段是 PID（部分系统还会带状态列，状态在 PID 之前）。
        if let Some(raw) = line.split_whitespace().last() {
            if let Ok(pid) = raw.parse::<u32>() {
                if pid > 1 {
                    pids.push(pid);
                }
            }
        }
    }
    pids.sort_unstable();
    pids.dedup();
    match pids.as_slice() {
        [single] => Some(*single),
        _ => None,
    }
}

/// Windows 上 Science 的可执行文件是 PE。这里只做格式门（32/64 位 PE），
/// 与 POSIX 侧"Mach-O magic 只作误选防护"的口径一致。
#[cfg(windows)]
pub(crate) fn science_binary_format_ok(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut dos = [0u8; 0x40];
    if file.read_exact(&mut dos).is_err() || &dos[0..2] != b"MZ" {
        return false;
    }
    let pe_offset = u32::from_le_bytes([dos[0x3c], dos[0x3d], dos[0x3e], dos[0x3f]]) as u64;
    if file.seek(SeekFrom::Start(pe_offset)).is_err() {
        return false;
    }
    let mut signature = [0u8; 6];
    if file.read_exact(&mut signature).is_err() {
        return false;
    }
    &signature[0..4] == b"PE\0\0"
}

#[cfg(windows)]
pub(crate) fn verify_private_path(path: &Path) -> Result<(), String> {
    if path_is_reparse_or_link(path) {
        return Err("路径是符号链接/重解析点".into());
    }
    if !win::owner_is_current_user(path) {
        return Err("路径不属于当前用户".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows 文件元数据与权限位
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub(crate) trait MetadataExt {
    fn dev(&self) -> u64;
    fn ino(&self) -> u64;
    fn mode(&self) -> u32;
    fn uid(&self) -> u32;
    fn gid(&self) -> u32;
    fn nlink(&self) -> u64;
    fn size(&self) -> u64;
    fn mtime(&self) -> i64;
    fn mtime_nsec(&self) -> i64;
    fn atime(&self) -> i64;
    fn atime_nsec(&self) -> i64;
    fn ctime(&self) -> i64;
    fn ctime_nsec(&self) -> i64;
}

#[cfg(windows)]
impl MetadataExt for std::fs::Metadata {
    fn dev(&self) -> u64 {
        // std 的 volume_serial_number / file_index / number_of_links 仍在
        // windows_by_handle 不稳定门后；真正需要文件身份的地方走 file_identity()。
        0
    }

    fn ino(&self) -> u64 {
        0
    }

    /// 推导出与 POSIX 同形的 mode 位：
    /// - 文件类型位来自 `file_type()`；
    /// - 目录 `0o700`、普通文件 `0o600`；只读文件清写位；
    /// - 可执行扩展名（`.exe`/`.cmd`/`.bat`/`.com`）补 `0o111`；
    /// - `0o022`（group/world 写）恒为 0：Windows 没有跨用户位语义，
    ///   真实的所有权判定见 [`verify_private_path`]。
    fn mode(&self) -> u32 {
        use std::os::windows::fs::MetadataExt as WinMetadataExt;
        const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        let attributes = WinMetadataExt::file_attributes(self);
        let mut mode = if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            S_IFDIR | 0o700
        } else {
            S_IFREG | 0o600
        };
        if attributes & FILE_ATTRIBUTE_READONLY != 0 && mode & S_IFMT == S_IFREG {
            mode &= !0o200;
        }
        mode
    }

    fn uid(&self) -> u32 {
        current_user_id()
    }

    fn gid(&self) -> u32 {
        current_user_id()
    }

    fn nlink(&self) -> u64 {
        1
    }

    fn size(&self) -> u64 {
        self.len()
    }

    fn mtime(&self) -> i64 {
        filetime_to_unix(win_last_write_time(self)).0
    }

    fn mtime_nsec(&self) -> i64 {
        filetime_to_unix(win_last_write_time(self)).1
    }

    fn atime(&self) -> i64 {
        filetime_to_unix(win_last_access_time(self)).0
    }

    fn atime_nsec(&self) -> i64 {
        filetime_to_unix(win_last_access_time(self)).1
    }

    /// Windows 没有独立的“inode 变更时间”；用创建时间作为稳定的第三身份。
    fn ctime(&self) -> i64 {
        filetime_to_unix(win_creation_time(self)).0
    }

    fn ctime_nsec(&self) -> i64 {
        filetime_to_unix(win_creation_time(self)).1
    }
}

#[cfg(windows)]
fn win_last_write_time(metadata: &std::fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt as WinMetadataExt;
    WinMetadataExt::last_write_time(metadata)
}

#[cfg(windows)]
fn win_last_access_time(metadata: &std::fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt as WinMetadataExt;
    WinMetadataExt::last_access_time(metadata)
}

#[cfg(windows)]
fn win_creation_time(metadata: &std::fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt as WinMetadataExt;
    WinMetadataExt::creation_time(metadata)
}

/// Windows FILETIME（自 1601-01-01 起的 100ns）→ (unix 秒, 纳秒)。
#[cfg(windows)]
fn filetime_to_unix(ticks: u64) -> (i64, i64) {
    const EPOCH_DIFFERENCE_SECONDS: i64 = 11_644_473_600;
    let seconds = (ticks / 10_000_000) as i64 - EPOCH_DIFFERENCE_SECONDS;
    let nanos = ((ticks % 10_000_000) * 100) as i64;
    (seconds, nanos)
}

/// 文件身份：`(volume_serial, file_index, link_count)`。
/// POSIX 上直接取 `(dev, ino, nlink)`；Windows 上走 `GetFileInformationByHandle`。
#[cfg(unix)]
pub(crate) fn file_identity(path: &Path) -> Option<(u64, u64, u32)> {
    let metadata = path.symlink_metadata().ok()?;
    Some((metadata.dev(), metadata.ino(), metadata.nlink() as u32))
}

#[cfg(windows)]
pub(crate) fn file_identity(path: &Path) -> Option<(u64, u64, u32)> {
    win::file_identity(path)
}

#[cfg(windows)]
pub(crate) trait PermissionsExt {
    fn mode(&self) -> u32;
    fn set_mode(&mut self, mode: u32);
    fn from_mode(mode: u32) -> std::fs::Permissions;
}

#[cfg(windows)]
impl PermissionsExt for std::fs::Permissions {
    fn mode(&self) -> u32 {
        // 只读位映射到 `0o200` 缺失，其余位在 Windows 上没有意义。
        if self.readonly() {
            0o444
        } else {
            0o644
        }
    }

    fn set_mode(&mut self, mode: u32) {
        // `set_readonly(true)` 等价于清掉所有写位。
        if mode & 0o222 == 0 {
            self.set_readonly(true);
        }
    }

    fn from_mode(mode: u32) -> std::fs::Permissions {
        let mut permissions = base_permissions();
        permissions.set_readonly(mode & 0o222 == 0);
        permissions
    }
}

#[cfg(windows)]
fn base_permissions() -> std::fs::Permissions {
    use std::sync::OnceLock;
    static BASE: OnceLock<std::fs::Permissions> = OnceLock::new();
    BASE.get_or_init(|| {
        std::fs::metadata(std::env::temp_dir())
            .or_else(|_| std::fs::metadata("."))
            .or_else(|_| std::fs::metadata("C:\\"))
            .map(|metadata| metadata.permissions())
            .unwrap_or_else(|error| {
                panic!("无法读取任何可访问目录的权限作为基准：{error}")
            })
    })
    .clone()
}

#[cfg(windows)]
pub(crate) trait OpenOptionsExt {
    fn mode(&mut self, mode: u32) -> &mut Self;
    fn custom_flags(&mut self, flags: i32) -> &mut Self;
}

#[cfg(windows)]
impl OpenOptionsExt for std::fs::OpenOptions {
    fn mode(&mut self, _mode: u32) -> &mut Self {
        // Windows 没有 O_CREAT 的 mode 参数；安全语义由 ACL 承担。
        self
    }

    fn custom_flags(&mut self, flags: i32) -> &mut Self {
        use std::os::windows::fs::OpenOptionsExt as WinOpenOptionsExt;
        // `O_NOFOLLOW` 在 Windows 上翻译成"不要跟随重解析点"，这是等价的
        // fail-closed 语义：链接/联接点不会被穿透。
        let mut native = 0u32;
        if flags & O_NOFOLLOW != 0 {
            native |= 0x0020_0000; // FILE_FLAG_OPEN_REPARSE_POINT
        }
        if native != 0 {
            WinOpenOptionsExt::custom_flags(self, native);
        }
        self
    }
}
