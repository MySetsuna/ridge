//! G1 阶段二：OS 级进程冻结（V-G1-OS）。
//!
//! - Unix：`kill(-pgid, SIGSTOP/CONT)` 进程组（pgid≈会话首进程 pid）。
//! - Windows：`NtSuspendProcess` / `NtResumeProcess`（候选 A；不递归子进程，阶段三 Job 另议）。
//!
//! 无额外 crate：仅 kernel32/ntdll 与 C kill FFI。无效 pid → Err（fail-visible）。
//! 软门控（suspend.rs 注册表）与本模块解耦；调用方可 fail-open 忽略 OS 失败。

/// Freeze a process / process group by pid.
pub fn freeze_pid(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("invalid pid 0".into());
    }
    #[cfg(unix)]
    {
        return unix_signal(pid, sig_stop());
    }
    #[cfg(windows)]
    {
        return win_nt_suspend(pid, true);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err("os freeze unsupported on this platform".into())
    }
}

/// Thaw a previously frozen process / process group.
pub fn thaw_pid(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("invalid pid 0".into());
    }
    #[cfg(unix)]
    {
        return unix_signal(pid, sig_cont());
    }
    #[cfg(windows)]
    {
        return win_nt_suspend(pid, false);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err("os thaw unsupported on this platform".into())
    }
}

/// Try freeze; on failure return Err without side effects (caller may soft-gate only).
/// If freeze succeeds, returns Ok(Some(pid)) for later thaw.
pub fn try_freeze(pid: Option<u32>) -> Result<Option<u32>, String> {
    let Some(pid) = pid else {
        return Ok(None);
    };
    freeze_pid(pid)?;
    Ok(Some(pid))
}

/// Best-effort thaw of a previously frozen pid.
pub fn try_thaw(pid: Option<u32>) -> Result<(), String> {
    match pid {
        Some(p) => thaw_pid(p),
        None => Ok(()),
    }
}

// ── Unix ────────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn sig_stop() -> i32 {
    // Linux 19 / macOS+BSD 17
    if cfg!(target_os = "linux") {
        19
    } else {
        17
    }
}

#[cfg(unix)]
fn sig_cont() -> i32 {
    // Linux 18 / macOS+BSD 19
    if cfg!(target_os = "linux") {
        18
    } else {
        19
    }
}

#[cfg(unix)]
fn unix_signal(pid: u32, sig: i32) -> Result<(), String> {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // Negative pid = process group (PTY session leader is group leader).
    let target = -(pid as i32);
    let r = unsafe { kill(target, sig) };
    if r != 0 {
        // Fallback: signal the single process if group signal fails (e.g. not leader).
        let r2 = unsafe { kill(pid as i32, sig) };
        if r2 != 0 {
            return Err(format!("kill(pid={pid}, sig={sig}) failed"));
        }
    }
    Ok(())
}

// ── Windows ─────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn win_nt_suspend(pid: u32, suspend: bool) -> Result<(), String> {
    const PROCESS_SUSPEND_RESUME: u32 = 0x0800;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetModuleHandleA(name: *const u8) -> isize;
        fn GetProcAddress(module: isize, name: *const u8) -> *const ();
    }
    type NtFn = unsafe extern "system" fn(process: isize) -> i32;
    unsafe {
        let h = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
        if h == 0 {
            return Err(format!("OpenProcess pid={pid} failed"));
        }
        let ntdll = GetModuleHandleA(b"ntdll.dll\0".as_ptr());
        if ntdll == 0 {
            CloseHandle(h);
            return Err("ntdll missing".into());
        }
        let name: &[u8] = if suspend {
            b"NtSuspendProcess\0"
        } else {
            b"NtResumeProcess\0"
        };
        let proc = GetProcAddress(ntdll, name.as_ptr());
        if proc.is_null() {
            CloseHandle(h);
            return Err(format!(
                "{} not found",
                if suspend {
                    "NtSuspendProcess"
                } else {
                    "NtResumeProcess"
                }
            ));
        }
        let nt: NtFn = std::mem::transmute(proc);
        let status = nt(h);
        CloseHandle(h);
        if status != 0 {
            return Err(format!("Nt*Process status={status:#x}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_pid_zero() {
        assert!(freeze_pid(0).is_err());
        assert!(thaw_pid(0).is_err());
    }

    #[test]
    fn try_freeze_none_is_ok() {
        assert_eq!(try_freeze(None).unwrap(), None);
        assert!(try_thaw(None).is_ok());
    }

    #[test]
    fn freeze_unlikely_pid_does_not_panic() {
        // CI 通常无权挂起系统进程；Err 或 Ok 均可，只要不 panic。
        let _ = freeze_pid(1);
    }
}
