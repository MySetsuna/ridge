fn remove_terminal_sub_if_owner(state: &AppState, key: (Uuid, Uuid, u64), sub_id: u64) {
    let mut subs = state.cloud_pane_terminal_subs.lock();
    if subs.get(&key).is_some_and(|(id, _)| *id == sub_id) {
        subs.remove(&key);
    }
}

/// Activation-scoped semantic stream used by cloud Remote. Every subscriber
/// gets its own envelope fence, even when several controllers view one pane.
#[tauri::command]
pub fn subscribe_pane_terminal(
    pane_id: String,
    workspace_id: Option<String>,
    activation_id: u64,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let pane = Uuid::parse_str(&pane_id).map_err(|_| "invalid paneId".to_string())?;
    let ws = resolve_workspace(&state, workspace_id.as_deref(), pane)?;
    let key = (ws, pane, activation_id);
    {
        let mut subs = state.cloud_pane_terminal_subs.lock();
        match subs.entry(key) {
            Entry::Occupied(mut occupied) => {
                occupied.get_mut().1 += 1;
                return Ok(());
            }
            Entry::Vacant(slot) => {
                let sub_id = RemoteSubId::next();
                slot.insert((sub_id, 1));
                let (tx, mut rx) = tokio::sync::mpsc::channel(RAW_CHAN_CAP);
                let desync = Arc::new(AtomicBool::new(false));
                state.register_remote_sub(
                    ws,
                    pane,
                    RemotePaneSub {
                        id: sub_id,
                        metadata_tx: tx.clone(),
                        raw_tx: tx,
                        semantic: true,
                        desync: Arc::clone(&desync),
                    },
                );
                let Some(initial) = state.get_remote_terminal_snapshot(ws, pane) else {
                    state.unregister_remote_sub(ws, pane, sub_id);
                    subs.remove(&key);
                    return Err("terminal snapshot unavailable".into());
                };
                let event_name = format!("pane-terminal-{ws}-{pane}-{activation_id}");
                let app_state = state.inner().clone();
                tauri::async_runtime::spawn(async move {
                    let pane_ref =
                        ridge_term::terminal_protocol::PaneRef::new(ws.to_string(), pane.to_string());
                    let mut revision = initial.revision();
                    if !emit_terminal(
                        &app,
                        &event_name,
                        &initial.into_frame(pane_ref.clone(), activation_id),
                    ) {
                        app_state.unregister_remote_sub(ws, pane, sub_id);
                        remove_terminal_sub_if_owner(&app_state, key, sub_id);
                        return;
                    }
                    while let Some(event) = rx.recv().await {
                        let RemotePtyEvent::SemanticDelta {
                            workspace_id,
                            pane_id,
                            frame,
                            is_alt,
                        } = event
                        else {
                            continue;
                        };
                        if workspace_id != ws || pane_id != pane {
                            continue;
                        }
                        if frame.pane_seq > revision && frame.semantic_update.as_deref().is_none_or(|update|
                            matches!(&update.body, ridge_term::terminal_protocol::TerminalBody::Delta(delta) if delta.base_revision != revision)) {
                            desync.store(true, Ordering::Release);
                        }
                        if desync.swap(false, Ordering::AcqRel) {
                            if let Some(snapshot) = app_state.get_remote_terminal_snapshot(ws, pane)
                            {
                                revision = snapshot.revision();
                                if !emit_terminal(
                                    &app,
                                    &event_name,
                                    &snapshot.into_frame(pane_ref.clone(), activation_id),
                                ) {
                                    break;
                                }
                            }
                            continue;
                        }
                        if frame.pane_seq <= revision {
                            continue;
                        }
                        let next_revision = frame.pane_seq;
                        let Some(delta) = frame.semantic_update.as_deref().cloned() else { continue; };
                        if !emit_terminal(
                            &app,
                            &event_name,
                            &delta.into_frame(pane_ref.clone(), activation_id),
                        ) {
                            break;
                        }
                        revision = next_revision;
                    }
                    app_state.unregister_remote_sub(ws, pane, sub_id);
                    remove_terminal_sub_if_owner(&app_state, key, sub_id);
                });
            }
        }
    }
    Ok(())
}

/// Controller-private semantic recovery for DataChannel backpressure. Unlike
/// the legacy raw resync this is an exact atomic terminal state and is returned
/// to the requesting bridge only; no Tauri event is broadcast to peers.
