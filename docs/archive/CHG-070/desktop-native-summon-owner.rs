/// 把一个 native 会话「召唤」进工作区 `wid`：为其活动窗口各面板建一个**领养 GUI
/// pane**（共享 native PTY，不新开 shell），走与普通 pane 完全一致的渲染/输入/
/// resize 路径。关闭=detach（见 `terminal::kill_pty_if_present`）。返回展示的面板数。
///
/// 接受 `AppState` + `AppHandle` 而非 `TeammateCtx`，以允许 Tauri 命令直接复用。
fn summon_one_pane(
    state: &AppState,
    socket: &str,
    wid: Uuid,
    sp: native::SummonPane,
) -> Option<Uuid> {
    if sp.prev_attachment.map(|(w, _)| w) == Some(wid) {
        return None;
    }
    let (idx, direction) = {
        let map = state.workspaces.read();
        map.get(&wid)
            .and_then(|ws| {
                let (uuid, sdir) = pane::choose_balanced_split(ws)?;
                let leaves = ws.pane_tree.get_all_leaves();
                let idx = leaves.iter().position(|p| *p == uuid).unwrap_or(0);
                let direction = match sdir {
                    SplitDirection::Horizontal => "horizontal",
                    SplitDirection::Vertical => "vertical",
                };
                Some((idx, direction))
            })
            .unwrap_or((0, "horizontal"))
    };
    let new_id = pane::teammate_split_pane(state, wid, idx, direction).ok()?;
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let parser = Arc::new(parking_lot::Mutex::new(PaneParser::new(
        sp.height.max(1),
        sp.width.max(1),
        2000,
    )));
    let handle = PtyHandle {
        master: sp.master,
        input_sink: crate::engine::pty::PtyInputSink::new(sp.writer.clone(), "teammate:server"),
        writer: sp.writer,
        _child: None,
        native_ref: Some((socket.to_string(), sp.global_id)),
        native_cancel: Some(cancel.clone()),
        remote_ref: None,
        kernel_ref: None,
        job: None,
        child_pid: None,
        resize_silence_deadline: Arc::new(std::sync::atomic::AtomicI64::new(0)),
        parser,
        delta_mode: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        workspace: Arc::new(parking_lot::Mutex::new(wid)),
    };
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            ws.terminals.insert(new_id, handle);
            ws.pane_sizes
                .insert(new_id, (sp.height.max(1), sp.width.max(1)));
            ws.teammate_pane_titles
                .insert(new_id, format!("%{}", sp.global_id));
            if let Some(dir) = sp.cwd {
                if let Some(pane) = ws.pane_tree.panes.get_mut(&new_id) {
                    pane.cwd = Some(dir.into());
                }
            }
        }
    }
    native::set_attachment(socket, sp.global_id, Some((wid, new_id)));
    spawn_pty_reader(
        state.clone(),
        wid,
        new_id,
        Box::new(native::BroadcastReader::new(sp.rx, sp.replay, cancel)),
    );
    Some(new_id)
}

pub(crate) fn summon_into_workspace(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    socket: &str,
    target: &str,
    wid: Uuid,
) -> Result<usize, NativeError> {
    let gui = gui_sessions_for_state(state, socket);
    let panes = native::summon(socket, target, &gui)?;
    let mut shown = 0usize;
    let mut first_new: Option<Uuid> = None;
    for sp in panes {
        let Some(new_id) = summon_one_pane(state, socket, wid, sp) else {
            continue;
        };
        if first_new.is_none() {
            first_new = Some(new_id);
        }
        shown += 1;
    }
    let _ = app_handle.emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
    if let Some(fid) = first_new {
        let _ = app_handle.emit("teammate-active-pane-changed", fid.to_string());
    }
    Ok(shown)
}

#[derive(Deserialize)]
struct TmuxSummonBody {
    #[serde(default = "default_socket")]
    socket: String,
    #[serde(default)]
    target: String,
}

/// `attach`（改造语义）：把目标 native 会话召唤进**发起方工作区**的 GUI 分屏。
async fn route_tmux_summon(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<TmuxSummonBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    // T5 MEDIUM（隔离完整性）：summon **建 pane** 路由（在错工作区领养 native pane）→
    // fail-closed。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };
    match summon_into_workspace(&ctx.state, &ctx.handle, &body.socket, &body.target, wid) {
        Ok(shown) => (StatusCode::OK, format!("summoned {shown}")).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

