// ===================== Native tmux engine: GUI seam =====================
//
// The native `/api/v1/tmux/*` HTTP handlers now live in the shared, host-
// agnostic `ridge_tmux::http` router (mounted in `run_server`). The only
// desktop-specific piece is the GUI-session seam below: on the default socket
// the engine folds VISIBLE workspace sessions into find-target so
// `ls`/`has-session`/`resolve` see both GUI and native (a GUI hit returns 409,
// making the shim fall back to the GUI path). `summon` (adopt a native session
// into a visible workspace) stays desktop-only — see `route_tmux_summon`.

fn default_socket() -> String {
    "default".to_string()
}

/// 默认 socket 上参与 find-target 的 GUI 会话（工作区名 + `ridge` 别名）。
/// 自定义 socket 返回空（与默认 socket 完全隔离）。`summon` 与 GUI 会话源共用。
fn gui_sessions_for_state(state: &AppState, socket: &str) -> Vec<native::GuiSession> {
    if socket != "default" {
        return Vec::new();
    }
    let order = state.workspace_order.read().clone();
    let names = state.workspace_names.read().clone();
    let map = state.workspaces.read();
    let mut out: Vec<native::GuiSession> = Vec::new();
    for wid in order.iter() {
        if !map.contains_key(wid) {
            continue;
        }
        let label = tmux_list_sessions_label(*wid, names.get(wid).map(String::as_str));
        out.push(native::GuiSession { name: label });
    }
    if !out.iter().any(|g| g.name == "ridge") {
        out.push(native::GuiSession {
            name: "ridge".to_string(),
        });
    }
    out
}

/// GUI 工作区会话的 `ls` 行（默认/`-F`），合并到 native `list-sessions` 之前。
fn gui_session_lines_state(state: &AppState, fmt: Option<&str>) -> Vec<String> {
    let active = state.active_workspace_id();
    let order = state.workspace_order.read().clone();
    let names = state.workspace_names.read().clone();
    let map = state.workspaces.read();
    let mut lines = Vec::new();
    for wid in order.iter() {
        let Some(ws) = map.get(wid) else {
            continue;
        };
        let label = tmux_list_sessions_label(*wid, names.get(wid).map(String::as_str));
        let (cols, rows) = workspace_first_pty_size(ws);
        let attached = *wid == active;
        let line = match fmt {
            Some(f) => f
                .replace("#{session_name}", &label)
                .replace("#{session_attached}", if attached { "1" } else { "0" })
                .replace("#{session_windows}", "1")
                .replace("#{session_width}", &cols.to_string())
                .replace("#{session_height}", &rows.to_string())
                .replace("#S", &label),
            None => {
                let created_local: DateTime<Local> =
                    DateTime::<Utc>::from(ws.created_at).with_timezone(&Local);
                let date_str = created_local.format("%a %b %d %H:%M:%S %Y").to_string();
                let mut line = format!("{label}: 1 windows (created {date_str}) [{cols}x{rows}]");
                if attached {
                    line.push_str(" (attached)");
                }
                line
            }
        };
        lines.push(line);
    }
    lines
}

/// Desktop GUI-session source for the shared `ridge_tmux::http` router: folds
/// visible workspaces into default-socket find-target. (`ridge-cli` mounts the
/// same router with the headless `NoGuiSessions` instead.)
struct DesktopGuiSessions {
    state: AppState,
}

impl ridge_tmux::http::GuiSessionSource for DesktopGuiSessions {
    fn sessions_for(&self, socket: &str) -> Vec<native::GuiSession> {
        gui_sessions_for_state(&self.state, socket)
    }
    fn session_lines(&self, fmt: Option<&str>) -> Vec<String> {
        gui_session_lines_state(&self.state, fmt)
    }
}

fn native_err_to_response(e: NativeError) -> axum::response::Response {
    match e {
        // 命中 GUI 会话：让 shim 回退到 GUI 路径。
        NativeError::Gui(name) => (StatusCode::CONFLICT, format!("gui:{name}")).into_response(),
        NativeError::NotFound(m) | NativeError::Ambiguous(m) | NativeError::NoServer(m) => {
            (StatusCode::NOT_FOUND, m).into_response()
        }
        NativeError::Duplicate(m) => (StatusCode::BAD_REQUEST, m).into_response(),
        NativeError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m).into_response(),
    }
}

