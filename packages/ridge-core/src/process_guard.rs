//! Shared external-process hard guards (Agents.md 2026-07-24 git pileup lessons).
//!
//! **Logic concurrency ≠ OS process lifetime.** Any spawn of an external binary
//! (git, helpers, CLIs) should:
//! 1. Prefer a single exit path that owns the child;
//! 2. Apply wall-clock timeout + **process-tree kill**;
//! 3. Release permits/counters with the process.
//!
//! Git's `commands::git` path remains the production consumer of these
//! primitives; other subsystems (future helpers) import from here instead of
//! re-copying `taskkill /T`.

use std::io;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

static TREE_KILLS: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);

/// How many process-tree kills this process has performed.
pub fn process_tree_kill_count() -> u64 {
    TREE_KILLS.load(Ordering::SeqCst)
}

/// How many wall-clock timeouts fired (subset of kills that were timeout-driven).
pub fn process_timeout_count() -> u64 {
    TIMEOUTS.load(Ordering::SeqCst)
}

#[cfg(test)]
pub fn reset_process_guard_counters_for_test() {
    TREE_KILLS.store(0, Ordering::SeqCst);
    TIMEOUTS.store(0, Ordering::SeqCst);
}

/// Kill a process tree. Windows: `taskkill /T`; Unix: TERM then KILL.
pub fn kill_process_tree(pid: u32) {
    TREE_KILLS.fetch_add(1, Ordering::SeqCst);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(50));
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Run a prepared `Command` with wall-clock timeout; on timeout kill the tree.
pub fn run_command_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> io::Result<std::process::Output> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let child = cmd.spawn()?;
    let pid = child.id();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            TIMEOUTS.fetch_add(1, Ordering::SeqCst);
            kill_process_tree(pid);
            let _ = rx.recv_timeout(Duration::from_secs(5));
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("process timed out after {timeout:?} (killed pid {pid})"),
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            kill_process_tree(pid);
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "process waiter thread disconnected",
            ))
        }
    }
}

/// Snapshot for diagnostics / IPC.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessGuardStats {
    pub tree_kills: u64,
    pub timeouts: u64,
}

pub fn process_guard_stats() -> ProcessGuardStats {
    ProcessGuardStats {
        tree_kills: process_tree_kill_count(),
        timeouts: process_timeout_count(),
    }
}

/// 测试时限的 CI 感知放宽（iter-60 R-TESTGATE 实证）：共享 2 核 runner 上
/// taskkill/kill+wait 的墙钟可达本机的数倍——把「回收要快」断言的秒级预算 ×4，
/// 意图不变（仍远小于 30-45s 的挂起预算）。本机不受影响。
#[cfg(test)]
pub(crate) fn test_time_budget(base: Duration) -> Duration {
    if std::env::var_os("CI").is_some() {
        base * 4
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn timeout_kills_hanging_binary_and_counts() {
        reset_process_guard_counters_for_test();
        #[cfg(windows)]
        let hang = {
            let dir = std::env::temp_dir().join(format!(
                "ridge-pg-hang-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let script = dir.join("hang.cmd");
            std::fs::write(&script, "@echo off\r\nping -n 30 127.0.0.1 >nul\r\n").unwrap();
            script
        };
        #[cfg(not(windows))]
        let hang = {
            let dir = std::env::temp_dir().join(format!(
                "ridge-pg-hang-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let script = dir.join("hang.sh");
            std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
            script
        };

        let start = Instant::now();
        let err = run_command_with_timeout(&mut Command::new(&hang), Duration::from_millis(400))
            .expect_err("must timeout");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < test_time_budget(Duration::from_secs(5)));
        assert!(process_timeout_count() >= 1);
        assert!(process_tree_kill_count() >= 1);
        let snap = process_guard_stats();
        assert_eq!(snap.timeouts, process_timeout_count());
        let _ = std::fs::remove_dir_all(hang.parent().unwrap());
    }

    #[test]
    fn fast_command_succeeds_without_timeout() {
        reset_process_guard_counters_for_test();
        #[cfg(windows)]
        let mut cmd = Command::new("cmd");
        #[cfg(windows)]
        {
            cmd.args(["/C", "echo ok"]);
        }
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("echo");
            c.arg("ok");
            c
        };
        let out = run_command_with_timeout(&mut cmd, Duration::from_secs(5)).expect("ok");
        assert!(out.status.success());
    }
}
