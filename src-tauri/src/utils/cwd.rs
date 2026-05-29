use std::path::{Path, PathBuf};

/// Whether ridge was launched from a CLI/terminal context or from the start
/// menu / file explorer (cwd happens to equal the exe directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupCwdKind {
    /// Launched from a terminal: `current_dir()` is the user's working
    /// directory and is used as the default cwd for the first workspace.
    Cli,
    /// Launched by double-click / start menu: `current_dir()` is the exe
    /// directory and must NOT be used as the default cwd.
    Menu,
}

/// Detect the launch kind. Returns the kind plus the captured cwd (only
/// non-None when kind == Cli).
pub fn detect_startup_cwd_kind() -> (StartupCwdKind, Option<PathBuf>) {
    let Ok(cwd) = std::env::current_dir() else {
        return (StartupCwdKind::Menu, None);
    };
    let exe_parent = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let is_menu = match exe_parent {
        Some(dir) => paths_equal(&cwd, &dir),
        None => false,
    };
    if is_menu {
        (StartupCwdKind::Menu, None)
    } else {
        (StartupCwdKind::Cli, Some(cwd))
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Resolve the default cwd for a fresh workspace.
/// Priority: cli_cwd > user_cwd > home > "." fallback.
pub fn resolve_default_cwd(
    cli_cwd: Option<&Path>,
    user_cwd: Option<&Path>,
) -> PathBuf {
    if let Some(p) = cli_cwd {
        if p.is_dir() {
            return p.to_path_buf();
        }
    }
    if let Some(p) = user_cwd {
        if p.is_dir() {
            return p.to_path_buf();
        }
    }
    if let Some(home) = dirs::home_dir() {
        if home.is_dir() {
            return home;
        }
    }
    PathBuf::from(".")
}
