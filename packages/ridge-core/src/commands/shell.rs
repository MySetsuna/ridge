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
    /// Launch args (e.g. WSL distro `["-d","Ubuntu"]`, VS `["/k","...VsDevCmd.bat"]`).
    /// Empty means launch `program` directly. `#[serde(default)]` keeps backward
    /// compatibility with old deserialization paths.
    #[serde(default)]
    pub args: Vec<String>,
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
                    args: vec![],
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
        // WSL: enumerate each installed distro (each becomes `wsl -d <distro>`).
        // Fall back to a single bare wsl entry if enumeration returns nothing.
        if let Some(wsl) = lookup_program("wsl.exe").or_else(|| lookup_program("wsl")) {
            let prog = wsl.to_string_lossy().to_string();
            let distros = list_wsl_distros();
            if distros.is_empty() {
                found.push(ShellInfo {
                    id: "wsl".to_string(),
                    label: "WSL".to_string(),
                    program: prog,
                    args: vec![],
                });
            } else {
                for d in distros {
                    found.push(ShellInfo {
                        id: format!("wsl-{d}"),
                        label: format!("WSL: {d}"),
                        program: prog.clone(),
                        args: vec!["-d".to_string(), d],
                    });
                }
            }
        }
        try_add(&mut found, "nu", "Nushell", &["nu.exe", "nu"]);
        try_add(
            &mut found,
            "clink",
            "Clink (CMD 增强)",
            &["clink.exe", "clink", "cmder.exe", "Cmder.exe"],
        );
        found.extend(detect_vs_dev_shells());
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

/// Parse the stdout of `wsl.exe -l -q` (UTF-16LE encoded) into a list of
/// distro names, stripping empty lines, CR characters and NUL padding.
/// This is a pure function (no I/O) so it can be unit-tested cross-platform.
pub fn parse_wsl_list(stdout: &[u8]) -> Vec<String> {
    let u16s: Vec<u16> = stdout
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u16s)
        .lines()
        .map(|l| l.trim().trim_matches('\0').trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// Run `wsl.exe -l -q` and return the list of installed distro names.
#[cfg(target_os = "windows")]
fn list_wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match Command::new("wsl.exe")
        .args(["-l", "-q"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) if o.status.success() => parse_wsl_list(&o.stdout),
        _ => Vec::new(),
    }
}

/// Detect Visual Studio developer shells via vswhere. Returns entries for
/// "Developer Command Prompt for VS" (cmd /k VsDevCmd.bat) and
/// "Developer PowerShell for VS" (powershell -NoExit -Command ...) when the
/// VS installation is found.
#[cfg(target_os = "windows")]
fn detect_vs_dev_shells() -> Vec<ShellInfo> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut out = Vec::new();
    let pf86 = match std::env::var("ProgramFiles(x86)") {
        Ok(p) if !p.is_empty() => p,
        _ => return out,
    };
    let vswhere = PathBuf::from(&pf86)
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.is_file() {
        return out;
    }
    let install_path = match Command::new(&vswhere)
        .args(["-latest", "-property", "installationPath"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return out,
    };
    if install_path.is_empty() {
        return out;
    }
    let install = PathBuf::from(&install_path);

    // Developer Command Prompt: cmd /k VsDevCmd.bat
    let vsdevcmd = install.join("Common7").join("Tools").join("VsDevCmd.bat");
    if vsdevcmd.is_file() {
        if let Some(cmd) = lookup_program("cmd.exe") {
            out.push(ShellInfo {
                id: "vs-devcmd".to_string(),
                label: "Developer Command Prompt for VS".to_string(),
                program: cmd.to_string_lossy().to_string(),
                args: vec!["/k".to_string(), vsdevcmd.to_string_lossy().to_string()],
            });
        }
    }

    // Developer PowerShell: powershell -NoExit -Command "Import-Module DevShell.dll; Enter-VsDevShell ..."
    let devshell = install
        .join("Common7")
        .join("Tools")
        .join("Microsoft.VisualStudio.DevShell.dll");
    if devshell.is_file() {
        if let Some(ps) = lookup_program("powershell.exe") {
            let script = format!(
                "Import-Module '{}'; Enter-VsDevShell -VsInstallPath '{}' -SkipAutomaticLocation",
                devshell.to_string_lossy(),
                install.to_string_lossy()
            );
            out.push(ShellInfo {
                id: "vs-pwsh".to_string(),
                label: "Developer PowerShell for VS".to_string(),
                program: ps.to_string_lossy().to_string(),
                args: vec!["-NoExit".to_string(), "-Command".to_string(), script],
            });
        }
    }
    out
}

/// Read recent shell history (PowerShell PSReadLine, bash, zsh), deduped
/// newest-first and capped at 1000 lines. Verbatim port of
/// `terminal.rs::get_shell_history` (the legacy `shell_kind` arg was unused and
/// is dropped here; the desktop wrapper still accepts + ignores it).
/// Parse a single line from a shell history file.
/// Returns `None` for blank / skip-worthy lines; otherwise returns the
/// command text.
fn parse_history_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Bash timestamp lines: "#1234567890"
    if trimmed.starts_with('#')
        && trimmed.len() > 1
        && trimmed[1..].chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    // Zsh extended history: ": <timestamp>:<duration>;<command>"
    if trimmed.starts_with(": ") {
        if let Some(semi) = trimmed.find(';') {
            let cmd = trimmed[semi + 1..].trim();
            if cmd.is_empty() {
                return None;
            }
            return Some(cmd.to_string());
        }
        // ":" prefix without "; " — treat as a regular command.
        // (Corner case: a command that starts with ": " followed by
        // non-metadata content. Fall through to return trimmed.)
    }
    Some(trimmed.to_string())
}

/// Read a shell history file and return its commands in **newest-first**
/// order, deduped within this file (case-sensitive, keeping each command's
/// most recent occurrence). Returns an empty vec when the file does not
/// exist or cannot be read.
fn read_history_file_newest_first(path: &std::path::Path) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut lines: Vec<String> = Vec::new();
    for raw in content.lines() {
        if let Some(cmd) = parse_history_line(raw) {
            lines.push(cmd);
        }
    }

    // Reverse to newest-first (history files append new entries at the
    // end), then dedup keeping the first occurrence (= newest).
    lines.reverse();
    let mut seen = std::collections::HashSet::new();
    lines.retain(|line| seen.insert(line.clone()));
    lines
}

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

    // Read each file independently (newest-first, file-internal dedup)
    // and concatenate in file order. The file order gives priority to
    // the primary Windows shell (PSReadLine) over bash/zsh for the
    // global dedup pass below.
    let mut all_lines: Vec<String> = Vec::new();
    for file in &history_files {
        let file_lines = read_history_file_newest_first(file);
        all_lines.extend(file_lines);
    }

    // Global dedup over the merged list, keeping the FIRST occurrence
    // of each command. Since each file segment is already newest-first,
    // and PSReadLine entries appear first, the most recently used shell's
    // command supersedes older duplicates from other shells.
    let mut seen = std::collections::HashSet::new();
    all_lines.retain(|line| seen.insert(line.clone()));

    all_lines.truncate(1000);
    Ok(all_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn parse_wsl_list_decodes_utf16le_and_trims() {
        // `wsl -l -q` 输出 UTF-16LE，每行一个发行版，可能带 CR / 尾随空行。
        let bytes = utf16le("Ubuntu\r\nDebian\r\n\r\n");
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu".to_string(), "Debian".to_string()]);
    }

    #[test]
    fn parse_wsl_list_empty_is_empty() {
        assert_eq!(parse_wsl_list(&utf16le("")), Vec::<String>::new());
    }

    #[test]
    fn parse_wsl_list_strips_nul_padding() {
        // 某些环境会夹带 NUL；不应产生空条目。
        let mut bytes = utf16le("Ubuntu");
        bytes.extend_from_slice(&[0, 0]); // trailing NUL u16
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu".to_string()]);
    }
}
