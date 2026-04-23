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
    let shell_pid = handle._child.process_id()?;

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
    children.into_iter().max_by_key(|(pid, _)| *pid).map(|(_, name)| name)
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
            let base = lower
                .trim_end_matches(".exe")
                .trim_end_matches(".com");
            !shell_names.contains(&base)
        })
        .collect();

    let best = if !non_shell_children.is_empty() {
        non_shell_children.into_iter().max_by_key(|(_, _, start)| *start)
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
