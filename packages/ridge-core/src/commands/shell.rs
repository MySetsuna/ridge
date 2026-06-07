//! Shell discovery + history (runtime-agnostic, **zero Tauri**).
//!
//! Verbatim port of the pure shell helpers from `src-tauri/src/commands/
//! terminal.rs` (S1 ledger). `detect_available_shells` enumerates the shells
//! installed on this machine; `get_shell_history` reads the on-disk history
//! files. Both are exactly what a headless `ridge-cli` host wants to surface to
//! a remote IDE controller — they touch only `std::env` / `std::fs` / `dirs`,
//! no AppState, no Tauri. The desktop keeps thin `#[tauri::command]` wrappers
//! that delegate here.

use std::path::PathBuf;

/// One discovered shell: a stable `id` (settings key), a human `label`, and the
/// resolved executable `program` path.
#[derive(serde::Serialize)]
pub struct ShellInfo {
    pub id: String,
    pub label: String,
    pub program: String,
}

/// Look up a command in `PATH`; also accepts an absolute path directly. No
/// extra crate (avoids a `which` dependency) — uses `PATH` + manual iteration,
/// honouring `PATHEXT` on Windows.
fn lookup_program(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(name);
    if path.is_absolute() && path.is_file() {
        return Some(path);
    }
    let path_var = std::env::var_os("PATH")?;
    #[cfg(target_os = "windows")]
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(|s| s.to_string())
        .collect();
    #[cfg(not(target_os = "windows"))]
    let exts: Vec<String> = vec![String::new()];

    for dir in std::env::split_paths(&path_var) {
        let base = dir.join(name);
        if base.is_file() {
            return Some(base);
        }
        // On Windows, try each PATHEXT extension.
        for ext in &exts {
            if ext.is_empty() {
                continue;
            }
            let with_ext = dir.join(format!("{name}{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

/// Detect installed shells. Returns `(id, label, program)` triples. Windows
/// scans pwsh / powershell / cmd / git-bash / wsl / nu / clink; Unix scans
/// zsh / bash / fish / sh / dash / nu / elvish. (Verbatim port of
/// `terminal.rs::detect_available_shells`.)
pub fn detect_available_shells() -> Vec<ShellInfo> {
    let mut found: Vec<ShellInfo> = Vec::new();
    let try_add = |list: &mut Vec<ShellInfo>, id: &str, label: &str, candidates: &[&str]| {
        for c in candidates {
            if let Some(p) = lookup_program(c) {
                let prog = p.to_string_lossy().to_string();
                if list.iter().any(|s| s.program == prog) {
                    return;
                }
                list.push(ShellInfo {
                    id: id.to_string(),
                    label: label.to_string(),
                    program: prog,
                });
                return;
            }
        }
    };

    #[cfg(target_os = "windows")]
    {
        try_add(
            &mut found,
            "pwsh",
            "PowerShell 7+ (pwsh)",
            &["pwsh.exe", "pwsh"],
        );
        try_add(
            &mut found,
            "powershell",
            "Windows PowerShell 5.1",
            &["powershell.exe", "powershell"],
        );
        try_add(&mut found, "cmd", "命令提示符 (CMD)", &["cmd.exe", "cmd"]);
        try_add(
            &mut found,
            "git-bash",
            "Git Bash",
            &[
                "bash.exe",
                "C:\\Program Files\\Git\\bin\\bash.exe",
                "C:\\Program Files\\Git\\usr\\bin\\bash.exe",
            ],
        );
        try_add(&mut found, "wsl", "WSL (Ubuntu)", &["wsl.exe", "wsl"]);
        try_add(&mut found, "nu", "Nushell", &["nu.exe", "nu"]);
        try_add(
            &mut found,
            "clink",
            "Clink (CMD 增强)",
            &["clink.exe", "clink", "cmder.exe", "Cmder.exe"],
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        try_add(&mut found, "zsh", "Zsh", &["zsh", "/bin/zsh", "/usr/bin/zsh"]);
        try_add(
            &mut found,
            "bash",
            "Bash",
            &["bash", "/bin/bash", "/usr/bin/bash"],
        );
        try_add(&mut found, "fish", "Fish", &["fish", "/usr/bin/fish"]);
        try_add(&mut found, "sh", "POSIX sh", &["sh", "/bin/sh", "/usr/bin/sh"]);
        try_add(
            &mut found,
            "dash",
            "Dash",
            &["dash", "/bin/dash", "/usr/bin/dash"],
        );
        try_add(&mut found, "nu", "Nushell", &["nu", "/bin/nu", "/usr/bin/nu"]);
        try_add(
            &mut found,
            "elvish",
            "Elvish",
            &["elvish", "/bin/elvish", "/usr/local/bin/elvish"],
        );
    }
    found
}

/// Read recent shell history (PowerShell PSReadLine, bash, zsh), deduped
/// newest-first and capped at 1000 lines. Verbatim port of
/// `terminal.rs::get_shell_history` (the legacy `shell_kind` arg was unused and
/// is dropped here; the desktop wrapper still accepts + ignores it).
pub fn get_shell_history() -> Result<Vec<String>, String> {
    let home_dir = dirs::home_dir().ok_or("无法获取 home 目录")?;
    let app_data = dirs::data_dir().ok_or("无法获取 AppData 目录")?;

    // All candidate shell history files.
    let history_files = vec![
        // PowerShell
        app_data
            .join("Microsoft")
            .join("Windows")
            .join("PowerShell")
            .join("PSReadLine")
            .join("ConsoleHost_history.txt"),
        // Bash (incl. Git Bash)
        home_dir.join(".bash_history"),
        // Zsh
        home_dir.join(".zsh_history"),
    ];

    let mut all_lines: Vec<String> = Vec::new();
    for file in &history_files {
        if !file.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Skip bash timestamp lines.
            if trimmed.starts_with('#')
                && trimmed.len() > 1
                && trimmed[1..].chars().all(|c| c.is_ascii_digit())
            {
                continue;
            }
            all_lines.push(trimmed.to_string());
        }
    }

    // Dedup by appearance order, keeping the latest (= most recently used).
    all_lines.reverse();
    let mut seen = std::collections::HashSet::new();
    all_lines.retain(|line| seen.insert(line.clone()));

    all_lines.truncate(1000);
    Ok(all_lines)
}
