//! Tauri commands for foreign history (AC4-C6 product surface).

use tauri::State;

use crate::state::AppState;

use super::foreign_history::{history_pull_budget, DEFAULT_HISTORY_TAIL_CAP};

/// Read current attach-seed tail (base64) for diagnostics / UI preview.
#[tauri::command]
pub fn get_foreign_history_tail(
    state: State<'_, AppState>,
    host_id: String,
    session_id: String,
) -> Result<serde_json::Value, String> {
    use base64::Engine as _;
    let bytes = state.hosts.history().tail(&host_id, &session_id);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(serde_json::json!({
        "hostId": host_id,
        "sessionId": session_id,
        "bytes": bytes.len(),
        "cap": state.hosts.history().cap(),
        "dataB64": b64,
    }))
}

/// Inject synthetic history (tests / mock host replay) then optional fanout.
#[tauri::command]
pub fn append_foreign_history(
    state: State<'_, AppState>,
    host_id: String,
    session_id: String,
    data_b64: String,
    fanout: Option<bool>,
) -> Result<usize, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| e.to_string())?;
    let n = bytes.len();
    state.hosts.history().append(&host_id, &session_id, &bytes);
    if fanout.unwrap_or(false) {
        super::fanout_live_output(&state, &host_id, &session_id, &bytes);
    }
    Ok(n)
}

/// Protocol budget hint for host-side history pull.
#[tauri::command]
pub fn foreign_history_pull_budget(want_lines: Option<u16>, cols: Option<u16>) -> usize {
    history_pull_budget(want_lines.unwrap_or(24), cols.unwrap_or(80))
}

#[tauri::command]
pub fn set_foreign_history_cap(state: State<'_, AppState>, cap: u32) -> Result<usize, String> {
    let c = (cap as usize).max(1024).min(DEFAULT_HISTORY_TAIL_CAP * 4);
    state.hosts.history().set_cap(c);
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::foreign_history::ForeignHistoryStore;

    #[test]
    fn pull_budget_command_matches_pure() {
        assert_eq!(
            foreign_history_pull_budget(Some(24), Some(80)),
            history_pull_budget(24, 80)
        );
    }

    #[test]
    fn store_roundtrip_append_tail() {
        let s = ForeignHistoryStore::new();
        s.append("h", "p", b"abc");
        assert_eq!(s.tail("h", "p"), b"abc");
    }
}
