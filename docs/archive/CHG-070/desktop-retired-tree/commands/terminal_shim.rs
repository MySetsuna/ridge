//! Wind-tmux shim + kernel structured env helpers (D11: extracted from
//! `commands/terminal.rs` so the file's install / spawn / dispatch /
//! input-sequence paths stay focused).
//!
//! The tmux shim is a synthetic `tmux(.exe)` binary placed alongside
//! the Ridge CLI so Claude Code's `TmuxBackend` finds the right
//! PATH entry on Windows + Linux. `kernel_structured_env` then
//! layers the kernel contract (RIDGE_TEAMMATE_URL/TOKEN, TMUX,
//! workspace/pane IDs) on top of the agent's base environment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;
use uuid::Uuid;

use crate::state::AppState;

/// Resolve the pre-built tmux shim once so native and kernel
/// launches apply the same PATH contract.
pub(crate) fn wind_tmux_shim_dir() -> Option<PathBuf> {
    let tmux_name = if cfg!(windows) { "tmux.exe" } else { "tmux" };

    // Dev builds: use the pre-built shim in dist/teammate-shim/ under the workspace root.
    // The cargo target dir moved to the workspace root (target/<profile>/ridge.exe) when
    // ridge-core was extracted, so don't assume a fixed depth — walk ancestors of the exe
    // until a `dist/teammate-shim` dir is found.
    #[cfg(debug_assertions)]
    let shim_dir = {
        let exe = std::env::current_exe().ok()?;
        let mut cur = exe.parent();
        loop {
            let d = cur?;
            let candidate = d.join("dist").join("teammate-shim");
            if candidate.is_dir() {
                break candidate;
            }
            cur = d.parent();
        }
    };

    // Release builds: look for tmux(.exe) beside the installed Ridge binary.
    #[cfg(not(debug_assertions))]
    let shim_dir = {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let tmux = dir.join(tmux_name);
        if !tmux.is_file() {
            return None;
        }
        dir.to_path_buf()
    };

    if !shim_dir.join(tmux_name).is_file() {
        eprintln!("[ridge] tmux shim not found at {}", shim_dir.display());
        return None;
    }
    Some(shim_dir)
}

pub(crate) fn prepend_path_with_wind_tmux_shim(cmd: &mut CommandBuilder) -> Option<PathBuf> {
    let shim_dir = wind_tmux_shim_dir()?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    let path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}{sep}{path}", shim_dir.display()));
    Some(shim_dir)
}

pub(crate) fn prepend_path_with_wind_tmux_shim_env(env: &mut HashMap<String, String>) -> Option<PathBuf> {
    let shim_dir = wind_tmux_shim_dir()?;
    let sep = if cfg!(windows) { ';' } else { ':' };
    let path = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    env.retain(|key, _| !key.eq_ignore_ascii_case("PATH"));
    env.insert(
        "PATH".to_string(),
        format!("{}{sep}{path}", shim_dir.display()),
    );
    Some(shim_dir)
}

/// tmux `TMUX` is `socket_path,session_index,pane_index`. Ridge uses a sentinel path (no real socket).
/// Claude Code's TmuxBackend on Windows may validate the first segment as a Windows path; `/ridge/...`
/// fails that check — use `{cwd|project|pwd|~/ridge}/teammate.sock` with `/` separators.
pub(crate) fn tmux_env_value(pane_slot: usize, cwd: Option<&Path>, state: &AppState) -> String {
    #[cfg(windows)]
    {
        let base = cwd
            .map(Path::to_path_buf)
            .or_else(|| state.current_project.read().clone())
            .or_else(|| std::env::current_dir().ok())
            .or_else(|| dirs::home_dir().map(|h| h.join("ridge")))
            .unwrap_or_else(|| PathBuf::from(r"C:\ridge"));
        let sock = base.join("teammate.sock");
        let prefix = sock.to_string_lossy().replace('\\', "/");
        format!("{prefix},0,{pane_slot}")
    }
    #[cfg(not(windows))]
    {
        let _ = (cwd, state);
        format!("/ridge/teammate.sock,0,{pane_slot}")
    }
}

pub(crate) fn kernel_structured_env(
    state: &AppState,
    _workspace_id: Uuid,
    _pane_id: Uuid,
    cwd: Option<&Path>,
    tmux_pane_index: Option<usize>,
    spec: &crate::terminal::StructuredPtyCommand,
) -> Result<HashMap<String, String>, String> {
    let binding = state
        .teammate_binding
        .read()
        .clone()
        .ok_or_else(|| "teammate server not ready; cannot spawn agent pane".to_string())?;
    let mut env = spec.env.clone();
    let _shim_dir = prepend_path_with_wind_tmux_shim_env(&mut env);
    env.insert(
        "RIDGE_TEAMMATE_URL".to_string(),
        binding.base_url.to_string(),
    );
    env.insert(
        "RIDGE_TEAMMATE_TOKEN".to_string(),
        binding.token.to_string(),
    );
    env.insert("RIDGE_TERMINAL".to_string(), "1".to_string());

    let pane_slot = tmux_pane_index.unwrap_or(0);
    let tmux_value = tmux_env_value(pane_slot, cwd, state);
    if let Some(sock) = tmux_value.split(',').next() {
        crate::teammate::endpoint::write_sidecar(
            sock,
            binding.base_url.as_str(),
            binding.token.as_str(),
        );
    }
    env.insert("TMUX".to_string(), tmux_value);
    env.insert("TMUX_PANE".to_string(), pane_slot.to_string());
    if let Some(log) = std::env::var("RIDGE_TMUX_LOG")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        env.insert("RIDGE_TMUX_LOG".to_string(), log);
    }
    Ok(env)
}
