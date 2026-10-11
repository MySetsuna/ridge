fn start_subscription(
    args: &Value,
    host: &Arc<KernelHost>,
    snapshot: &KernelSnapshot,
    runtime: &tokio::runtime::Handle,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<(Uuid, Uuid)>>>,
) {
    let Some((workspace_id, pane_id)) = subscription_target(args, snapshot) else {
        return;
    };
    let Ok(mut active) = subscriptions.lock() else {
        return;
    };
    if !active.insert((workspace_id, pane_id)) {
        return;
    }
    drop(active);
    let endpoint = host.current_endpoint();
    let tx = out_tx.clone();
    let subscriptions_for_task = subscriptions.clone();
    runtime.spawn(async move {
        // P0-5 + P0-4 (audit C5 + C4 fix): lease + subscriptions
        // slot are guarded by a Drop type so any early-return path
        // (scrollback error / next-seq error / attach error / tx send
        // failure / panicking inner future) still detaches the lease
        // and releases the (ws, pane) subscription slot. The original
        // implementation only detached on the normal-exit branch
        // (`let _ = detach_domain_pty_output(...)` at line 1026), so a
        // scrollback/attach failure would orphan both the kernel lease
        // and the local subscription set.
        struct SubscriptionGuard {
            endpoint: KernelEndpoint,
            pane_id: Uuid,
            lease: Option<uuid::Uuid>,
            subscriptions:
                Arc<Mutex<std::collections::HashSet<(Uuid, Uuid)>>>,
            workspace_id: Uuid,
        }
        impl Drop for SubscriptionGuard {
            fn drop(&mut self) {
                if let Some(lease) = self.lease.take() {
                    let _ = ridge_kernel::client::detach_domain_pty_output(
                        &self.endpoint,
                        self.pane_id,
                        lease,
                    );
                }
                if let Ok(mut active) = self.subscriptions.lock() {
                    active.remove(&(self.workspace_id, self.pane_id));
                }
            }
        }

        let mut guard = SubscriptionGuard {
            endpoint: endpoint.clone(),
            pane_id,
            lease: None,
            subscriptions: subscriptions_for_task.clone(),
            workspace_id,
        };

        let scrollback = tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || scrollback_domain_pty(&endpoint, pane_id, 262_144)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
        // CHG-050 §2.2: time the first blocking hop. The `bytes`
        // value is the actual scrollback length returned (0 if the
        // kernel rejected or returned an error); the `dt_us` covers
        // spawn_blocking + std TcpStream I/O + JSON parse.
        crate::host_trace!(
            ev = "scroll",
            pane_id = %pane_id,
            bytes = scrollback.len()
        );
        let after_seq = tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || {
                list_domain_ptys(&endpoint).ok().and_then(|ptys| {
                    ptys.into_iter()
                        .find(|pty| pty.pty_id == pane_id)
                        .map(|pty| pty.next_seq.saturating_sub(1))
                })
            }
        })
        .await
        .ok()
        .flatten();
        crate::host_trace!(
            ev = "list_seq",
            pane_id = %pane_id,
            next_seq = after_seq.unwrap_or(0)
        );
        let lease = match tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || attach_domain_pty_output(&endpoint, pane_id, after_seq)
        })
        .await
        {
            Ok(Ok(lease)) => lease,
            _ => return,
        };
        crate::host_trace!(
            ev = "attach",
            pane_id = %pane_id,
            lease_id = %lease
        );
        // Hand the lease to the guard so Drop will detach it.
        guard.lease = Some(lease);
        // P3-D9 (audit D9 surface layer): the bytes emitted to the
        // rdg controller use `ridge_remote::pane::pane_frame` /
        // `pane_resync_frame` — the same wire format that the legacy
        // mux channel `PANE_RAW` produced. When the kernel is reached
        // via RTP1 (`RIDGE_RTP1_KERNEL=1`), this is the
        // mux-PANE_RAW adapter: it consumes RTP1 output frames and
        // emits the legacy mux byte format so existing rdg
        // controllers continue to work. New code should consume RTP1
        // directly; this adapter remains only as a transport-boundary
        // seam (SPEC-L2-PROTO-001 §3.9 P5).
        let frame = ridge_remote::pane::pane_resync_frame(
            pane_id,
            &scrollback,
            &ridge_term::term::modes::Modes::default(),
            false,
        );
        if tx.send(Message::Binary(frame)).is_err() {
            return;
        }
        let mut metadata_buffer = Vec::new();
        loop {
            let poll_t0 = Instant::now();
            let result = tokio::task::spawn_blocking({
                let endpoint = endpoint.clone();
                move || poll_domain_pty_output(&endpoint, pane_id, lease, 1000, 64)
            })
            .await;
            let poll_dt_us = poll_t0.elapsed().as_micros() as u64;
            match result {
                Ok(Ok(KernelPtyOutput::Data(bytes))) => {
                    // CHG-050 §2.2: emit `poll_out` once per delivery
                    // so the trace can correlate the kernel-side
                    // `lease_deliver` (under `ridge_kernel::trace`)
                    // with the host-side frame emit.
                    crate::host_trace!(
                        ev = "poll_out",
                        pane_id = %pane_id,
                        lease_id = %lease,
                        frames = 1u64,
                        bytes = bytes.len() as u64,
                        dt_us = poll_dt_us
                    );
                    if !send_subscription_data(
                        workspace_id,
                        pane_id,
                        &bytes,
                        &mut metadata_buffer,
                        &tx,
                    ) {
                        break;
                    }
                }
                Ok(Ok(KernelPtyOutput::Lagged)) => {
                    let _ = resync_domain_pty_output(&endpoint, pane_id, lease);
                }
                Ok(Ok(KernelPtyOutput::Timeout)) => {
                    // CHG-050 §2.2: surface the long-poll ceiling so
                    // we can tell whether idle polls reach the 1000ms
                    // timeout or wake early via `notify_waiters`.
                    crate::host_trace!(
                        ev = "poll_timeout",
                        pane_id = %pane_id,
                        lease_id = %lease,
                        dt_us = poll_dt_us
                    );
                }
                _ => break,
            }
        }
        let _ = ridge_kernel::client::detach_domain_pty_output(&endpoint, pane_id, lease);
    });
}

/// RTP1-over-WebSocket variant of [`start_subscription`]. Activated
/// when `RIDGE_RTP1_KERNEL=1`. Drops the HTTP long-poll loop entirely
/// (SPEC-L2-PROTO-001 §3.9 P5: RTP1 stays transport-neutral; the
/// HTTP path remains as the legacy adapter fallback).
fn start_subscription_rtp1(
    args: &Value,
    host: &Arc<KernelHost>,
    snapshot: &KernelSnapshot,
    runtime: &tokio::runtime::Handle,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<(Uuid, Uuid)>>>,
) {
    let Some((workspace_id, pane_id)) = subscription_target(args, snapshot) else {
        return;
    };
    let Ok(mut active) = subscriptions.lock() else {
        return;
    };
    if !active.insert((workspace_id, pane_id)) {
        return;
    }
    drop(active);

    let endpoint = host.current_endpoint();
    let tx = out_tx.clone();
    let host_for_task = host.clone();
    let subscriptions_for_task = subscriptions.clone();
    runtime.spawn(async move {
        // Fetch the host_info / runtime_epoch via the standard status
        // endpoint first. The kernel status body carries host_id +
        // runtime_epoch (set in server.rs).
        let info = match fetch_host_info(&endpoint).await {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!(target: "ridge_cli::rtp1_kernel",
                    %error, "host_info fetch failed; falling back to HTTP");
                return;
            }
        };
        let client = Rtp1KernelClient::new(endpoint.clone(), info.host_id, info.runtime_epoch);
        let (mut sink, mut output_rx) = match client
            .connect(pane_id, workspace_id.to_string(), None)
            .await
        {
            Ok(pair) => pair,
            Err(error) => {
                tracing::warn!(target: "ridge_cli::rtp1_kernel",
                    %error, "RTP1 connect failed");
                // Best-effort cleanup: release the subscription slot so
                // a future re-subscribe on the same (ws, pane) is not
                // silently dropped (audit C4).
                if let Ok(mut active) = subscriptions_for_task.lock() {
                    active.remove(&(workspace_id, pane_id));
                }
                return;
            }
        };
        let mut metadata_buffer = Vec::new();

        // P0-3 (audit C3 fix): the initial resync must be sent BEFORE
        // waiting for the first output frame. The legacy HTTP path
        // promises the controller a scrollback + modes bundle on
        // subscribe; the RTP1 path used to defer that until the first
        // byte arrived, so a PTY that produced 0 bytes (e.g. `sleep
        // infinity`) never received the initial resync and the
        // controller hung on an empty stream.
        let scrollback = match tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || scrollback_domain_pty(&endpoint, pane_id, 262_144)
        })
        .await
        {
            Ok(Ok(bytes)) => bytes,
            _ => Vec::new(),
        };
        let resync = ridge_remote::pane::pane_resync_frame(
            pane_id,
            &scrollback,
            &ridge_term::term::modes::Modes::default(),
            false,
        );
        if tx.send(Message::Binary(resync)).is_err() {
            // Controller closed before we could deliver the initial
            // resync; release the slot and exit cleanly.
            if let Ok(mut active) = subscriptions_for_task.lock() {
                active.remove(&(workspace_id, pane_id));
            }
            return;
        }

        loop {
            tokio::select! {
                out = output_rx.recv() => {
                    let Some(frame) = out else { break };
                    let chunks: Vec<u8> = frame
                        .frames
                        .iter()
                        .flat_map(|c| {
                            ridge_kernel::rtp1::b64_decode(&c.data_b64).unwrap_or_default()
                        })
                        .collect();
                    if !chunks.is_empty()
                        && !send_subscription_data(
                            workspace_id,
                            pane_id,
                            &chunks,
                            &mut metadata_buffer,
                            &tx,
                        )
                    {
                        break;
                    }
                }
                state_check = async {
                    client.state().await
                } => {
                    if matches!(
                        state_check,
                        Rtp1ClientState::Closing | Rtp1ClientState::Detached | Rtp1ClientState::Failed
                    ) {
                        break;
                    }
                }
            }
        }
        // Best-effort: tell the kernel we are gone. Even if the
        // request fails (e.g. socket already closed) the kernel will
        // eventually reap via the WS close detection.
        if let Err(error) = sink.send_detach(pane_id, Some("subscription-closed".into())).await {
            tracing::debug!(target: "ridge_cli::rtp1_kernel",
                %error, "send_detach on close (best effort)");
        }
        let _ = sink.close().await;
        // Release the subscription slot so a future subscribe on the
        // same (ws, pane) is not silently dropped.
        if let Ok(mut active) = subscriptions_for_task.lock() {
            active.remove(&(workspace_id, pane_id));
        }
        let _ = host_for_task;
    });
}

async fn fetch_host_info(
    endpoint: &KernelEndpoint,
) -> Result<HostInfo, String> {
    // /v1/status is the only canonical source of (host_id,
    // runtime_epoch); we go through the public HTTP client. The
    // client is blocking (uses std::thread internally), so wrap in
    // spawn_blocking.
    let endpoint_clone = endpoint.clone();
    let body = tokio::task::spawn_blocking(move || {
        ridge_kernel::client::request_json(&endpoint_clone, "GET", "/v1/status", None)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let host_id = body
        .get("host_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "status missing host_id".to_string())?
        .to_string();
    let runtime_epoch = body
        .get("runtime_epoch")
        .and_then(Value::as_str)
        .ok_or_else(|| "status missing runtime_epoch".to_string())?
        .to_string();
    Ok(HostInfo {
        host_id,
        runtime_epoch,
        max_realtime_frame: ridge_kernel::rtp1::MAX_REALTIME_FRAME,
        max_snapshot_chunk: 256 * 1024,
        replay_cap_bytes: 256 * 1024,
        replay_cap_frames: 256,
        features: vec!["rtp1.v1".into()],
    })
}

fn send_subscription_data(
    workspace_id: Uuid,
    pane_id: Uuid,
    bytes: &[u8],
    metadata_buffer: &mut Vec<u8>,
    tx: &mpsc::UnboundedSender<Message>,
) -> bool {
    if bytes.is_empty() {
        return true;
    }
    metadata_buffer.extend_from_slice(bytes);
    if metadata_buffer.len() > 8192 {
        let keep_from = metadata_buffer.len() - 8192;
        metadata_buffer.drain(..keep_from);
    }
    if let Some(metadata) = pty_metadata_frame(workspace_id, pane_id, metadata_buffer) {
        if tx.send(Message::Text(metadata.to_string())).is_err() {
            return false;
        }
    }
    tx.send(Message::Binary(ridge_remote::pane::pane_frame(
        pane_id, bytes,
    )))
    .is_ok()
}

