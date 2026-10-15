//! Runtime owns natural exit/rebuild decisions; clients only rebind projections.
use crate::{pty::RuntimeExit, server::AppState};
use ridge_core::EventSink;
use serde_json::json;
use std::sync::atomic::Ordering;

pub(crate) fn start(state: &AppState) -> tokio::task::JoinHandle<()> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(256);
    state.ptys.observe_exits(sender);
    let state = state.clone();
    tokio::spawn(async move {
        while let Some(exit) = receiver.recv().await {
            if state.shutting_down.load(Ordering::Acquire) { break; }
            let state = state.clone();
            let result = tokio::task::spawn_blocking(move || handle(&state, exit)).await;
            if let Err(error) = result { tracing::error!(%error, "terminal lifecycle worker failed"); }
        }
    })
}

fn handle(state: &AppState, exit: RuntimeExit) {
    let Ok(retired) = state.retired_workspaces.lock() else { return; };
    if state.shutting_down.load(Ordering::Acquire) { return; }
    let Ok(graph) = state.workspaces.lock() else { return; };
    let Some((info, mut program)) = state.ptys.exited_view(exit.pane, &exit.terminal_epoch) else { return; };
    let Some(workspace) = info.workspace_id.filter(|workspace| !retired.contains(workspace)) else { return; };
    let Some(metadata) = graph.workspace(workspace).filter(|meta| meta.pane_tree.get_all_leaves().contains(&exit.pane)) else { return; };
    let Ok(roster) = state.roster.lock() else { return; };
    if roster.agent_identities().iter().any(|member| member.pane_id == exit.pane.to_string()
        && member.protocol_binding.is_some() && !roster.protocol_member_retired(&member.agent_id)) { program = None; }
    drop(roster);
    let replacement = program.map(|program| {
        let cwd = metadata.pane_tree.panes.get(&exit.pane).and_then(|pane| pane.cwd.clone())
            .or_else(|| info.cwd.as_ref().map(std::path::PathBuf::from))
            .map(Ok).unwrap_or_else(std::env::current_dir).map_err(|error| error.to_string())?;
        let path = cwd.to_string_lossy();
        if !cwd.is_absolute() || ridge_core::dispatch::path_has_traversal(&path) || !state.fs_scope.is_allowed(&path) {
            return Err("shell recovery cwd is outside Runtime roots".to_string());
        }
        state.ptys.replace_exited_shell(&exit, &info, &program, Some(&path)).map_err(|error| error.to_string())
    }).transpose();
    let (epoch, error) = match replacement { Ok(epoch) => (epoch, None), Err(error) => (None, Some(error)) };
    state.ptys.set_exit_error(exit.pane, &exit.terminal_epoch, error.clone());
    let metadata = state.ptys.terminal_metadata(exit.pane, Some(epoch.as_deref().unwrap_or(&exit.terminal_epoch)));
    drop(graph); drop(retired);
    state.business_events.broadcast("pane-pty-closed", json!({"workspaceId":workspace,"paneId":exit.pane,
        "terminalEpoch":exit.terminal_epoch,"replacementEpoch":epoch,"exitCode":exit.code,"error":error}));
    if let Some(metadata) = metadata { state.business_events.broadcast("pane-meta-changed", metadata); }
}
