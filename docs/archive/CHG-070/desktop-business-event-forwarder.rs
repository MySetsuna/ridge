const COALESCE_MAX_BYTES: usize = 64 * 1024;
// Agent activity only nudges the control plane; terminal bytes stay lossless
// and immediate. Four updates per second are enough for status/highlight UI
// without competing with the terminal render lane during agent output bursts.
const OUTPUT_ACTIVITY_INTERVAL: Duration = Duration::from_millis(250);

fn coalesce_window_for(_last_bytes: usize) -> u64 {
    // Never insert a timer gap between PTY packets. The forwarder still
    // drains packets already ready in the channel, while interactive output
    // (notably repeated Ctrl+C) reaches the renderer without a periodic
    // 2/8ms cadence.
    0
}

type PendingOutput = HashMap<(uuid::Uuid, uuid::Uuid), String>;
type OutputActivityAt = HashMap<(uuid::Uuid, uuid::Uuid), Instant>;

fn output_activity_due(last: Option<Instant>, now: Instant) -> bool {
    last.map_or(true, |at| {
        now.duration_since(at) >= OUTPUT_ACTIVITY_INTERVAL
    })
}

fn emit_pane_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: String,
) {
    let label = pane_id.to_string();
    // Terminal bytes are the foreground lane. Emit them before the optional
    // Agent control-plane hint so status refresh listeners cannot get ahead
    // of the visible shell/Codex output.
    let _ = handle.emit(
        &format!("pty-output-{workspace_id}-{label}"),
        serde_json::json!({ "data": data }),
    );

    emit_pane_activity(handle, activity_at, workspace_id, pane_id);
}

fn emit_pane_activity(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
) {
    // Activity is a low-priority control-plane hint, not terminal data. Keep
    // it bounded while the PTY emits a burst; the terminal frame remains
    // lossless and immediate.
    let label = pane_id.to_string();
    let key = (workspace_id, pane_id);
    let state = handle.state::<AppState>();
    let is_teammate_pane = state
        .workspaces
        .read()
        .get(&workspace_id)
        .is_some_and(|workspace| workspace.teammate_pane_states.contains_key(&pane_id));
    if is_teammate_pane {
        let now = Instant::now();
        if output_activity_due(activity_at.get(&key).copied(), now) {
            activity_at.insert(key, now);
            let _ = handle.emit(
                "pane-output-activity",
                serde_json::json!({
                    "workspaceId": workspace_id.to_string(), "paneId": label,
                }),
            );
        }
    }
}

fn emit_pane_delta(
    handle: &tauri::AppHandle,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    frame: Vec<u8>,
) {
    let state = handle.state::<AppState>();
    if let Some(sender) = state.get_pane_delta_channel(workspace_id, pane_id) {
        sender(frame);
    } else {
        let label = pane_id.to_string();
        let _ = handle.emit(&format!("pty-delta-{workspace_id}-{label}"), frame);
    }
}

fn flush_pane_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
) {
    if let Some(data) = pending.remove(&(workspace_id, pane_id)) {
        emit_pane_output(handle, activity_at, workspace_id, pane_id, data);
    }
}

fn flush_pending_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
) -> usize {
    let drained: Vec<_> = pending.drain().collect();
    let bytes = drained.iter().map(|(_, data)| data.len()).sum();
    for ((workspace_id, pane_id), data) in drained {
        emit_pane_output(handle, activity_at, workspace_id, pane_id, data);
    }
    bytes
}

fn forward_raw_pty_output(
    handle: &tauri::AppHandle,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: &str,
) {
    handle
        .state::<AppState>()
        .forward_remote_pty_bytes(workspace_id, pane_id, data);
}

fn handle_pty_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: String,
) {
    forward_raw_pty_output(handle, workspace_id, pane_id, &data);
    // The reader thread always keeps the native parser warm. A raw event that
    // was queued just before delta enable is therefore already represented by
    // the full reframe sent during the handoff; do not parse it a second time.
    let delta_enabled = handle
        .state::<AppState>()
        .workspaces
        .read()
        .get(&workspace_id)
        .and_then(|workspace| workspace.terminals.get(&pane_id))
        .is_some_and(|terminal| terminal.delta_mode.load(Ordering::Acquire));
    if delta_enabled {
        return;
    }
    let entry = pending.entry((workspace_id, pane_id)).or_default();
    entry.push_str(&data);
    if entry.len() >= COALESCE_MAX_BYTES {
        let payload = std::mem::take(entry);
        pending.remove(&(workspace_id, pane_id));
        emit_pane_output(handle, activity_at, workspace_id, pane_id, payload);
    }
}

fn handle_pty_delta_output(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    data: String,
    frame: Vec<u8>,
) {
    // Remote subscribers still receive the lossless raw stream. The local
    // desktop path receives only the already-parsed binary frame.
    forward_raw_pty_output(handle, workspace_id, pane_id, &data);
    if !frame.is_empty() {
        emit_pane_delta(handle, workspace_id, pane_id, frame);
    }
    emit_pane_activity(handle, activity_at, workspace_id, pane_id);
}

fn handle_cwd_changed(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    cwd: String,
) {
    flush_pane_output(handle, activity_at, pending, workspace_id, pane_id);
    let label = pane_id.to_string();
    let state = handle.state::<AppState>();
    if state.remote_enabled.load(Ordering::Relaxed) {
        state.broadcast_remote_event(
            workspace_id,
            pane_id,
            crate::types::RemotePtyEvent::Metadata {
                workspace_id,
                pane_id,
                title: None,
                cwd: Some(cwd.clone()),
            },
        );
    }
    let _ = handle.emit(
        "pane-meta-changed",
        serde_json::json!({
            "workspaceId": workspace_id.to_string(), "paneId": label, "cwd": &cwd,
        }),
    );
    let _ = handle.emit(
        &format!("pane-cwd-changed-{workspace_id}-{label}"),
        serde_json::json!({ "cwd": cwd }),
    );
}

fn handle_title_changed(
    handle: &tauri::AppHandle,
    workspace_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    title: String,
) {
    let label = pane_id.to_string();
    let state = handle.state::<AppState>();
    if state.remote_enabled.load(Ordering::Relaxed) {
        state.broadcast_remote_event(
            workspace_id,
            pane_id,
            crate::types::RemotePtyEvent::Metadata {
                workspace_id,
                pane_id,
                title: Some(title.clone()),
                cwd: None,
            },
        );
    }
    let _ = handle.emit(
        "pane-meta-changed",
        serde_json::json!({
            "workspaceId": workspace_id.to_string(), "paneId": label, "title": &title,
        }),
    );
    let _ = handle.emit(
        &format!("pane-title-changed-{workspace_id}-{label}"),
        serde_json::json!({ "title": title }),
    );
}

fn handle_global_event(
    handle: &tauri::AppHandle,
    activity_at: &mut OutputActivityAt,
    pending: &mut PendingOutput,
    event: GlobalEvent,
) {
    match event {
        GlobalEvent::PtyOutput {
            workspace_id,
            pane_id,
            data,
        } => {
            handle_pty_output(handle, activity_at, pending, workspace_id, pane_id, data);
        }
        GlobalEvent::PtyDeltaOutput {
            workspace_id,
            pane_id,
            data,
            frame,
        } => {
            handle_pty_delta_output(handle, activity_at, workspace_id, pane_id, data, frame);
        }
        GlobalEvent::PaneClosed {
            workspace_id,
            pane_id,
        } => {
            flush_pane_output(handle, activity_at, pending, workspace_id, pane_id);
            activity_at.remove(&(workspace_id, pane_id));
            let _ = handle.emit(
                "pane-pty-closed",
                serde_json::json!({
                    "workspaceId": workspace_id.to_string(), "paneId": pane_id.to_string(),
                }),
            );
        }
        GlobalEvent::PaneModeChanged {
            workspace_id,
            pane_id,
            mode,
        } => {
            let mode = match mode {
                PaneMode::Terminal => "Terminal",
                PaneMode::Editor { .. } => "Editor",
            };
            let _ = handle.emit(
                &format!("pane-mode-changed-{workspace_id}-{pane_id}"),
                serde_json::json!({ "mode": mode }),
            );
        }
        GlobalEvent::PaneCwdChanged {
            workspace_id,
            pane_id,
            cwd,
        } => handle_cwd_changed(handle, activity_at, pending, workspace_id, pane_id, cwd),
        GlobalEvent::PaneTitleChanged {
            workspace_id,
            pane_id,
            title,
        } => handle_title_changed(handle, workspace_id, pane_id, title),
        GlobalEvent::PanePromptDetected {
            workspace_id,
            pane_id,
        } => {
            let _ = handle.emit(
                &format!("pane-prompt-{workspace_id}-{pane_id}"),
                serde_json::json!({}),
            );
        }
        GlobalEvent::PaneTreeChanged { workspace_id } => {
            let _ = handle.emit(
                "pane-tree-changed",
                serde_json::json!({ "workspaceId": workspace_id.to_string() }),
            );
        }
        GlobalEvent::WorkspaceListChanged => {
            let _ = handle.emit("workspace-list-changed", serde_json::json!({}));
        }
    }
}

enum ForwardTick {
    Event(GlobalEvent),
    Flush,
    Closed,
}

async fn next_forward_tick(
    event_rx: &mut mpsc::Receiver<GlobalEvent>,
    pending: &PendingOutput,
    last_flush_bytes: usize,
) -> ForwardTick {
    if pending.is_empty() {
        return event_rx
            .recv()
            .await
            .map_or(ForwardTick::Closed, ForwardTick::Event);
    }
    if coalesce_window_for(last_flush_bytes) == 0 {
        return match event_rx.try_recv() {
            Ok(event) => ForwardTick::Event(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => ForwardTick::Flush,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => ForwardTick::Closed,
        };
    }
    match tokio::time::timeout(
        std::time::Duration::from_millis(coalesce_window_for(last_flush_bytes)),
        event_rx.recv(),
    )
    .await
    {
        Ok(Some(event)) => ForwardTick::Event(event),
        Ok(None) => ForwardTick::Closed,
        Err(_) => ForwardTick::Flush,
    }
}

async fn run_event_forwarder(handle: tauri::AppHandle, mut event_rx: mpsc::Receiver<GlobalEvent>) {
    let mut pending = PendingOutput::new();
    let mut activity_at = OutputActivityAt::new();
    let mut last_flush_bytes = 0;
    loop {
        match next_forward_tick(&mut event_rx, &pending, last_flush_bytes).await {
            ForwardTick::Event(event) => {
                handle_global_event(&handle, &mut activity_at, &mut pending, event)
            }
            ForwardTick::Flush => {
                last_flush_bytes = flush_pending_output(&handle, &mut activity_at, &mut pending)
            }
            ForwardTick::Closed => {
                flush_pending_output(&handle, &mut activity_at, &mut pending);
                break;
            }
        }
    }
}

fn spawn_event_forwarder(handle: tauri::AppHandle, event_rx: mpsc::Receiver<GlobalEvent>) {
    tauri::async_runtime::spawn(run_event_forwarder(handle, event_rx));
}
#[cfg(test)]
mod window_launch_tests {
    use super::{
        coalesce_window_for, is_auth_focus_launch, output_activity_due, OUTPUT_ACTIVITY_INTERVAL,
    };
    use std::time::Instant;

    #[test]
    fn tiny_pty_bursts_use_a_bounded_coalesce_window() {
        assert_eq!(coalesce_window_for(0), 0);
        assert_eq!(coalesce_window_for(255), 0);
        assert_eq!(coalesce_window_for(256), 0);
        assert_eq!(coalesce_window_for(64 * 1024), 0);
    }

    #[test]
    fn output_activity_is_rate_limited_but_resumes_after_quiet_window() {
        let now = Instant::now();
        assert!(output_activity_due(None, now));
        assert!(!output_activity_due(
            Some(now),
            now + OUTPUT_ACTIVITY_INTERVAL / 2
        ));
        assert!(output_activity_due(
            Some(now),
            now + OUTPUT_ACTIVITY_INTERVAL
        ));
    }

    #[test]
    fn only_auth_deep_link_reuses_the_main_window() {
        assert!(is_auth_focus_launch(&[
            "ridge.exe".into(),
            "ridge://auth/focus".into()
        ]));
        assert!(is_auth_focus_launch(&[
            "ridge.exe".into(),
            "RIDGE://AUTH/FOCUS?approved=1".into()
        ]));
        assert!(!is_auth_focus_launch(&["ridge.exe".into()]));
        assert!(!is_auth_focus_launch(&[
            "ridge.exe".into(),
            "C:\\work\\project".into()
        ]));
    }
}
