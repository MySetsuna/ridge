//! OS-level process introspection (runtime-agnostic, **zero Tauri**).
//!
//! Port of the pure, PID-keyed helpers from `src-tauri/src/commands/process.rs`
//! (S1 ledger §2.1). These take a shell PID and read the OS (Unix `/proc`,
//! Windows `sysinfo`) to answer "what is the foreground process / cwd" — no
//! AppState, no event emission, no Tauri. The desktop keeps the AppState
//! orchestration (resolving a pane → its PTY child PID, the cwd write-back, the
//! `PaneCwdChanged` event and `.ridge` auto-save) in `commands/process.rs` and
//! delegates the OS lookups here; the headless `ridge-cli` host (which also owns
//! PTY children with PIDs) can reuse the same logic.
//!
//! `sysinfo` is pulled only on Windows (`[target.'cfg(windows)'.dependencies]`),
//! so the Linux/VPS `ridge-cli` build stays lean and uses the `/proc` path.

/// Returns the name of the foreground process running under `shell_pid`, or
/// `None` if it cannot be determined (the caller falls back to the shell name).
///
/// On Unix: enumerates `/proc` for children whose `PPid` is `shell_pid` and
/// returns the highest-PID child's `comm` (rough proxy for most-recently
/// spawned). On Windows: enumerates `sysinfo` children and picks the
/// most-recently-started non-shell child.
#[cfg(unix)]
pub fn get_foreground_process_name(shell_pid: u32) -> Option<String> {
    use std::io::Read;

    // On Unix, the PTY master exposes the foreground process group leader via
    // process_group_leader(). However, since we only have the child pid here,
    // we try to find the foreground pgid via /proc/<shell_pid>/stat and then
    // read /proc/<pgid>/comm.

    // Strategy: enumerate children of shell_pid to find processes whose ppid is shell_pid.
    // Then pick the most recently spawned one (highest pid as a heuristic).
    // If no children, the shell itself IS the foreground process — return None.
    let proc_dir = std::path::Path::new("/proc");
    if !proc_dir.exists() {
        return None;
    }

    let shell_pid_str = shell_pid.to_string();
    let mut children: Vec<(u32, String)> = Vec::new();

    let read_dir = std::fs::read_dir(proc_dir).ok()?;
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Only numeric entries (PIDs)
        let pid: u32 = match name_str.parse() {
            Ok(p) if p != shell_pid => p,
            _ => continue,
        };

        // Read /proc/<pid>/status to find PPid
        let status_path = entry.path().join("status");
        let Ok(mut f) = std::fs::File::open(&status_path) else {
            continue;
        };
        let mut content = String::new();
        if f.read_to_string(&mut content).is_err() {
            continue;
        }

        let ppid_line = content.lines().find(|l| l.starts_with("PPid:"))?;
        let ppid_str = ppid_line.split_whitespace().nth(1)?;
        if ppid_str != shell_pid_str {
            continue;
        }

        // Read /proc/<pid>/comm for the process name
        let comm_path = entry.path().join("comm");
        let Ok(mut cf) = std::fs::File::open(&comm_path) else {
            continue;
        };
        let mut comm = String::new();
        if cf.read_to_string(&mut comm).is_err() {
            continue;
        }
        let comm = comm.trim().to_string();
        if !comm.is_empty() {
            children.push((pid, comm));
        }
    }

    // Pick the child with highest PID (rough proxy for most recently spawned)
    children
        .into_iter()
        .max_by_key(|(pid, _)| *pid)
        .map(|(_, name)| name)
}

#[cfg(windows)]
pub fn get_foreground_process_name(shell_pid: u32) -> Option<String> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All);

    let shell_pid_sysinfo = sysinfo::Pid::from_u32(shell_pid);

    // Collect children of the shell process
    let mut children: Vec<(sysinfo::Pid, String, u64)> = Vec::new();
    for (pid, process) in sys.processes() {
        if process.parent() == Some(shell_pid_sysinfo) && *pid != shell_pid_sysinfo {
            let name = process.name().to_string_lossy().to_string();
            let start = process.start_time();
            children.push((*pid, name, start));
        }
    }

    // Skip shell-like processes: pick the most recently started non-shell child
    let shell_names = ["powershell", "pwsh", "cmd", "bash", "zsh", "sh", "fish"];
    let non_shell_children: Vec<_> = children
        .iter()
        .filter(|(_, name, _)| {
            let lower = name.to_lowercase();
            let base = lower.trim_end_matches(".exe").trim_end_matches(".com");
            !shell_names.contains(&base)
        })
        .collect();

    let best = if !non_shell_children.is_empty() {
        non_shell_children
            .into_iter()
            .max_by_key(|(_, _, start)| *start)
    } else {
        children.iter().max_by_key(|(_, _, start)| *start)
    };

    best.map(|(_, name, _)| {
        // Strip .exe suffix for cleaner display
        let n = name.trim_end_matches(".exe").trim_end_matches(".EXE");
        n.to_string()
    })
}

#[cfg(not(any(unix, windows)))]
pub fn get_foreground_process_name(_shell_pid: u32) -> Option<String> {
    None
}

/// Windows `sysinfo` returns backslashes; OSC 7 and user input are usually
/// forward slashes. Normalise to forward slashes so the pane-cwd store does not
/// end up with "looks-equal-but-isn't" keys.
pub fn normalize_cwd(raw: String) -> String {
    #[cfg(windows)]
    {
        raw.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        raw
    }
}

/// Returns the current working directory of the process under `shell_pid`,
/// preferring the most-recently-spawned non-shell child's cwd (so running e.g.
/// `cargo` in a subdir is tracked), falling back to the shell's own cwd. Pure
/// OS query; does NOT touch any host state.
#[cfg(unix)]
pub fn get_process_cwd(shell_pid: u32) -> Option<String> {
    newest_child_cwd(shell_pid).or_else(|| process_cwd(shell_pid))
}

#[cfg(unix)]
fn process_cwd(pid: u32) -> Option<String> {
    std::fs::read_link(
        std::path::Path::new("/proc")
            .join(pid.to_string())
            .join("cwd"),
    )
    .ok()?
    .to_str()
    .map(str::to_owned)
}

#[cfg(unix)]
fn newest_child_cwd(shell_pid: u32) -> Option<String> {
    let proc_dir = std::path::Path::new("/proc");
    if !proc_dir.exists() {
        return None;
    }
    let shell_pid = shell_pid.to_string();
    let child = std::fs::read_dir(proc_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| direct_child_pid(entry, &shell_pid))
        .max();
    child.and_then(process_cwd)
}

#[cfg(unix)]
fn direct_child_pid(entry: std::fs::DirEntry, shell_pid: &str) -> Option<u32> {
    use std::io::Read;
    let pid = entry.file_name().to_string_lossy().parse().ok()?;
    let mut status = std::fs::File::open(entry.path().join("status")).ok()?;
    let mut content = String::new();
    status.read_to_string(&mut content).ok()?;
    let parent = content
        .lines()
        .find_map(|line| line.strip_prefix("PPid:")?.split_whitespace().next())?;
    (parent == shell_pid).then_some(pid)
}

#[cfg(windows)]
pub fn get_process_cwd(shell_pid: u32) -> Option<String> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All);
    let shell_pid = sysinfo::Pid::from_u32(shell_pid);
    let shell_names = ["powershell", "pwsh", "cmd", "bash", "zsh", "sh", "fish"];
    let mut children = Vec::new();
    for (pid, process) in sys.processes() {
        if process.parent() != Some(shell_pid) || *pid == shell_pid {
            continue;
        }
        let name = process.name().to_string_lossy().to_lowercase();
        let base = name.trim_end_matches(".exe").trim_end_matches(".com");
        if shell_names.contains(&base) {
            continue;
        }
        children.push((
            process.start_time(),
            process
                .cwd()
                .map(|path| path.to_string_lossy().into_owned()),
        ));
    }
    children
        .into_iter()
        .max_by_key(|(started, _)| *started)
        .and_then(|(_, cwd)| cwd)
        .filter(|cwd| !cwd.is_empty())
        .or_else(|| {
            sys.process(shell_pid)
                .and_then(|process| process.cwd())
                .map(|path| path.to_string_lossy().into_owned())
                .filter(|cwd| !cwd.is_empty())
        })
}

#[cfg(not(any(unix, windows)))]
pub fn get_process_cwd(_shell_pid: u32) -> Option<String> {
    None
}
