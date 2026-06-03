use tauri::State;
use uuid::Uuid;

use crate::state::AppState;
use crate::utils::pane_id::parse_pane_id;

/// Returns the name of the foreground process running in the given pane's PTY,
/// or `None` if we cannot determine it (falls back to showing the shell name).
///
/// On Unix: reads /proc/<pgid>/comm via the PTY master's `process_group_leader()`.
/// On Windows: enumerates child processes of the shell PID via sysinfo and picks
/// the most-recently-started non-shell child. Falls back to None if unavailable.
#[tauri::command]
pub async fn get_pane_foreground_process(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
) -> Result<Option<String>, String> {
    let workspace_id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;

    let result = get_foreground_process_impl(&state, workspace_id, pane_id);
    Ok(result)
}

fn get_foreground_process_impl(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Option<String> {
    let map = state.workspaces.read();
    let ws = map.get(&workspace_id)?;
    let handle = ws.terminals.get(&pane_id)?;

    // Get the shell PID from the child process
    let shell_pid = handle._child.as_ref().and_then(|c| c.process_id())?;

    drop(map); // release lock before doing I/O

    get_foreground_process_name(shell_pid)
}

#[cfg(unix)]
fn get_foreground_process_name(shell_pid: u32) -> Option<String> {
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
fn get_foreground_process_name(shell_pid: u32) -> Option<String> {
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
fn get_foreground_process_name(_shell_pid: u32) -> Option<String> {
    None
}

/// Returns the current working directory of the shell running in the given pane,
/// by reading the OS-level cwd of the shell process. This is the reliable
/// cross-platform path — it does NOT rely on the shell emitting OSC 7, so
/// plain PowerShell / cmd on Windows also update correctly after `cd`.
///
/// 副作用：发现新的 cwd 时，顺手把它写回 `pane_tree.panes[pane].cwd` —— 这是后端
/// 唯一权威的 cwd 来源，后续 split 会从这里继承；同时触发 .ridge 自动保存。
#[tauri::command]
pub async fn get_pane_cwd(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
) -> Result<Option<String>, String> {
    let workspace_id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;

    let shell_pid = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .and_then(|handle| handle._child.as_ref().and_then(|c| c.process_id()))
    };
    let Some(shell_pid) = shell_pid else {
        return Ok(None);
    };

    let cwd_opt = get_process_cwd(shell_pid).map(normalize_cwd);

    if let Some(ref cwd) = cwd_opt {
        let path = std::path::PathBuf::from(cwd);
        let mut changed = false;
        {
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&workspace_id) {
                if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                    if pane.cwd.as_deref() != Some(path.as_path()) {
                        pane.cwd = Some(path);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            // 除了写回 tree，还发一次 PaneCwdChanged：下游（Explorer/SCM）的监听器
            // 不再需要等 2.5s 的轮询返回值才知道 cwd 变了 —— 事件路径立刻到达。
            let _ = state
                .event_tx
                .try_send(crate::types::GlobalEvent::PaneCwdChanged {
                    workspace_id,
                    pane_id,
                    cwd: cwd.clone(),
                });
            crate::commands::ridge_file::schedule_auto_save(&*state, workspace_id);
        }
    }

    Ok(cwd_opt)
}

/// Windows 下 `sysinfo` 返回反斜杠、OSC 7 和用户输入常是正斜杠。
/// 统一为正斜杠写回 store，避免 `paneCwdStore` 出现"看起来一样其实不相等"的键冲突。
fn normalize_cwd(raw: String) -> String {
    #[cfg(windows)]
    {
        raw.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        raw
    }
}

/// 供命令层在需要当前 cwd 但 tree 尚未记录时使用（例如刚创建还未被轮询过的窗格）。
/// 仅做 OS 层查询，不改写 state。
pub(crate) fn current_pane_cwd_live(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Option<String> {
    let shell_pid = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .and_then(|handle| handle._child.as_ref().and_then(|c| c.process_id()))
    }?;
    get_process_cwd(shell_pid)
}

#[cfg(unix)]
fn get_process_cwd(shell_pid: u32) -> Option<String> {
    // Read /proc/<shell_pid>/cwd symlink. If the shell has a foreground child (e.g.
    // `vim` open in a subdir), we prefer the child's cwd so the explorer tracks it.
    let pick = |pid: u32| -> Option<String> {
        let link = std::path::Path::new("/proc")
            .join(pid.to_string())
            .join("cwd");
        std::fs::read_link(&link)
            .ok()?
            .to_str()
            .map(|s| s.to_string())
    };

    // Prefer the most-recently-spawned direct child's cwd, fallback to shell's own.
    use std::io::Read;
    let proc_dir = std::path::Path::new("/proc");
    if proc_dir.exists() {
        let shell_pid_str = shell_pid.to_string();
        let mut best_child: Option<u32> = None;
        if let Ok(read_dir) = std::fs::read_dir(proc_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                let pid: u32 = match name_str.parse() {
                    Ok(p) if p != shell_pid => p,
                    _ => continue,
                };
                let status_path = entry.path().join("status");
                let Ok(mut f) = std::fs::File::open(&status_path) else {
                    continue;
                };
                let mut content = String::new();
                if f.read_to_string(&mut content).is_err() {
                    continue;
                }
                let Some(ppid_line) = content.lines().find(|l| l.starts_with("PPid:")) else {
                    continue;
                };
                let Some(ppid_str) = ppid_line.split_whitespace().nth(1) else {
                    continue;
                };
                if ppid_str == shell_pid_str {
                    best_child = match best_child {
                        Some(prev) if prev > pid => Some(prev),
                        _ => Some(pid),
                    };
                }
            }
        }
        if let Some(child_pid) = best_child {
            if let Some(cwd) = pick(child_pid) {
                return Some(cwd);
            }
        }
    }
    pick(shell_pid)
}

#[cfg(windows)]
fn get_process_cwd(shell_pid: u32) -> Option<String> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All);

    let shell_pid_sysinfo = sysinfo::Pid::from_u32(shell_pid);

    // Prefer foreground non-shell child's cwd (so running `cargo` in subdir is tracked).
    let shell_names = ["powershell", "pwsh", "cmd", "bash", "zsh", "sh", "fish"];
    let mut children: Vec<(sysinfo::Pid, u64, Option<String>)> = Vec::new();
    for (pid, process) in sys.processes() {
        if process.parent() == Some(shell_pid_sysinfo) && *pid != shell_pid_sysinfo {
            let name = process.name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            let base = lower.trim_end_matches(".exe").trim_end_matches(".com");
            if shell_names.contains(&base) {
                continue;
            }
            let cwd = process.cwd().map(|p| p.to_string_lossy().to_string());
            children.push((*pid, process.start_time(), cwd));
        }
    }
    if let Some((_, _, Some(cwd))) = children.into_iter().max_by_key(|(_, t, _)| *t) {
        if !cwd.is_empty() {
            return Some(cwd);
        }
    }

    // Fallback: shell's own cwd
    sys.process(shell_pid_sysinfo)
        .and_then(|p| p.cwd())
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(any(unix, windows)))]
fn get_process_cwd(_shell_pid: u32) -> Option<String> {
    None
}
