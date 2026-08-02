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
use std::time::{Duration, Instant};

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

/// Configure a child as a process-group leader on Unix.
///
/// Every descendant that does not deliberately detach inherits this group, so
/// the timeout path can signal the whole tree instead of leaving shell/grandchild
/// processes behind. `pre_exec` only invokes the async-signal-safe `setpgid`.
#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // `pre_exec` runs between fork and exec; the closure must not allocate or
    // touch locks. `setpgid(0, 0)` makes the child its own process-group leader.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

/// Kill a process tree. Windows: `taskkill /T`; Unix: TERM then KILL to the
/// dedicated process group (with a PID fallback for callers that predate the
/// group setup).
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
    #[cfg(unix)]
    {
        let group = -(pid as libc::pid_t);
        let group_term_ok = unsafe { libc::kill(group, libc::SIGTERM) == 0 };
        if !group_term_ok {
            // A process launched before process-group setup (or one whose
            // group already exited) still gets the best-effort PID fallback.
            let _ = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
        std::thread::sleep(Duration::from_millis(50));
        if group_term_ok {
            // Do not send a second PID signal after a successful group TERM:
            // the original PID may have exited and been reused by then.
            let _ = unsafe { libc::kill(group, libc::SIGKILL) };
        } else {
            let _ = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Keep a portable fallback for targets without a native process-group
        // API. Production desktop/CLI targets are covered above.
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
    configure_process_group(cmd);
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

/// Run a command with null stdio and a wall-clock timeout. Use this for
/// launcher commands that intentionally detach a long-lived child: piped
/// capture can otherwise keep Windows pipe handles open past launcher exit.
pub fn run_command_status_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> io::Result<std::process::ExitStatus> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    configure_process_group(cmd);
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            TIMEOUTS.fetch_add(1, Ordering::SeqCst);
            kill_process_tree(pid);
            let reap_deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < reap_deadline {
                if child.try_wait()?.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("process timed out after {timeout:?} (killed pid {pid})"),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
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

        let status_err =
            run_command_status_with_timeout(&mut Command::new(&hang), Duration::from_millis(400))
                .expect_err("status-only launcher must timeout");
        assert_eq!(status_err.kind(), io::ErrorKind::TimedOut);
        assert!(process_timeout_count() >= 2);
        assert!(process_tree_kill_count() >= 2);
        let _ = std::fs::remove_dir_all(hang.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_unix_process_group_descendants() {
        reset_process_guard_counters_for_test();
        let dir = std::env::temp_dir().join(format!(
            "ridge-pg-tree-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hang-tree.sh");
        let pid_file = dir.join("pids");
        // The shell and its sleep child both inherit the process group created
        // by configure_process_group. Record both PIDs so the assertion checks
        // the descendant, not only the root waiter.
        let pid_path = pid_file.to_string_lossy().replace('\'', "'\\''");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf '%s %s' \"$$\" \"$child\" > '{pid_path}'\nwait \"$child\"\n"
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();

        let err = run_command_with_timeout(&mut Command::new(&script), Duration::from_millis(400))
            .expect_err("must timeout");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);

        let deadline = Instant::now() + Duration::from_secs(2);
        let (group_pid, child_pid) = loop {
            if let Ok(raw) = std::fs::read_to_string(&pid_file) {
                let mut pids = raw.split_whitespace().filter_map(|p| p.parse::<i32>().ok());
                if let (Some(group_pid), Some(child_pid)) = (pids.next(), pids.next()) {
                    break (group_pid, child_pid);
                }
            }
            assert!(Instant::now() < deadline, "timed out waiting for child PID");
            std::thread::sleep(Duration::from_millis(10));
        };

        // The timeout path must reclaim the whole group, not just the shell.
        // Wait briefly for init to reap a killed descendant before asserting.
        while Instant::now() < deadline {
            let group_alive = unsafe { libc::kill(-(group_pid as libc::pid_t), 0) == 0 };
            let child_alive = unsafe { libc::kill(child_pid as libc::pid_t, 0) == 0 };
            if !group_alive && !child_alive {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!unsafe { libc::kill(-(group_pid as libc::pid_t), 0) == 0 });
        assert!(!unsafe { libc::kill(child_pid as libc::pid_t, 0) == 0 });
        let _ = std::fs::remove_dir_all(dir);
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
