use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::commands::{pane, terminal};
use crate::state::AppState;
use tauri::Emitter;

#[derive(Clone)]
struct TeammateCtx {
    state: AppState,
    token: Arc<String>,
    handle: tauri::AppHandle,
}

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    if headers
        .get("x-wind-token")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == token)
    {
        return true;
    }
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|v| v == token)
}

/// 后台线程跑 Axum，避免阻塞 Tauri 主循环。
/// `ready` 在 HTTP 已绑定且 `teammate_binding` 写入后发送一次，供 setup 等待首个 PTY 注入环境变量。
pub fn spawn_teammate_server(
    handle: tauri::AppHandle,
    state: AppState,
    ready: Option<std::sync::mpsc::Sender<()>>,
) {
    std::thread::Builder::new()
        .name("wind-teammate-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[wind] teammate runtime: {e}");
                    if let Some(tx) = ready {
                        let _ = tx.send(());
                    }
                    return;
                }
            };
            rt.block_on(run_server(handle, state, ready));
        })
        .ok();
}

async fn run_server(
    handle: tauri::AppHandle,
    app_state: AppState,
    ready: Option<std::sync::mpsc::Sender<()>>,
) {
    let token = uuid::Uuid::new_v4().to_string();
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wind] teammate bind failed: {e}");
            if let Some(tx) = ready {
                let _ = tx.send(());
            }
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let base_url = format!("http://127.0.0.1:{port}");
    {
        let mut b = app_state.teammate_binding.write();
        *b = Some(crate::state::TeammateBinding {
            base_url: base_url.clone(),
            token: token.clone(),
        });
    }
    if let Some(tx) = ready {
        let _ = tx.send(());
    }
    eprintln!(
        "[wind] teammate HTTP {base_url} (inject WIND_TEAMMATE_* into PTY; use wind-tmux as tmux on PATH)"
    );

    let ctx = TeammateCtx {
        state: app_state,
        token: Arc::new(token),
        handle,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/split-window", post(route_split))
        .route("/api/v1/capture-pane", get(route_capture))
        .route("/api/v1/send-keys", post(route_send_keys))
        .route("/api/v1/list-panes", get(route_list_panes))
    // Pane management
    .route("/api/v1/select-pane", post(route_select_pane))
    .route("/api/v1/kill-pane", post(route_kill_pane))
    .route("/api/v1/resize-pane", post(route_resize_pane))
    // Window management
    .route("/api/v1/new-window", post(route_new_window))
    .route("/api/v1/list-windows", get(route_list_windows))
    .route("/api/v1/list-clients", get(route_list_clients))
        .with_state(ctx);

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[wind] teammate server stopped: {e}");
    }
}

#[derive(Deserialize)]
struct SplitBody {
    #[serde(default)]
    pane_index: Option<usize>,
    /// `tmux split-window -h` → true（左右）。
    #[serde(default)]
    horizontal: bool,
    #[serde(default)]
    command: Option<String>,
    /// `tmux split-window -c start-directory`
    #[serde(default)]
    cwd: Option<String>,
    /// `tmux split-window -n` / `new-window -n` 经客户端转发时的窗格名。
    #[serde(default)]
    window_name: Option<String>,
}

async fn route_split(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<SplitBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();

    // Smart split target selection:
    // 1. If explicit pane_index from -t flag → use it
    // 2. Else use teammate_tmux_pane_cursor (main session)
    // 3. If main session pane is SMALLER than any sub-agent pane → use the largest
    let (idx, inferred_direction) = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&wid) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
        };
        let leaves = ws.pane_tree.get_all_leaves();
        let pane_count = leaves.len();

        // If explicit index provided, use it directly
        if let Some(explicit_idx) = body.pane_index {
            if explicit_idx < pane_count {
                (explicit_idx, false)
            } else {
                return (StatusCode::BAD_REQUEST, "pane_index out of range").into_response();
            }
        } else {
            // No explicit index - use smart selection
            let main_cursor = ws.teammate_tmux_pane_cursor;
            let main_size = ws.pane_sizes.get(&leaves.get(main_cursor).copied().unwrap_or_default()).copied().unwrap_or((80, 120));
            let main_area = main_size.0 as u32 * main_size.1 as u32;

            // Find the best target: prefer main cursor, but use largest pane if it's bigger
            let mut target_idx = main_cursor;
            for (i, leaf_id) in leaves.iter().enumerate() {
                if i == main_cursor {
                    continue;
                }
                if let Some(&(rows, cols)) = ws.pane_sizes.get(leaf_id) {
                    let area = rows as u32 * cols as u32;
                    if area > main_area {
                        target_idx = i;
                        break; // Use first (smallest index) larger pane
                    }
                }
            }

            // Shape-based direction inference
            let target_size = ws.pane_sizes.get(&leaves.get(target_idx).copied().unwrap_or_default()).copied().unwrap_or((80, 120));
            let inferred = target_size.1 > target_size.0; // cols > rows -> horizontal

            (target_idx, inferred)
        }
    };

    // Direction: explicit takes precedence, otherwise use inferred
    let direction = if body.horizontal {
        "horizontal"
    } else {
        if inferred_direction {
            "horizontal"
        } else {
            "vertical"
        }
    };

    let cwd = body
        .cwd
        .as_ref()
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty());

    // Track last pane before updating cursor
    {
        let mut map = ctx.state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            ws.last_pane_index = Some(ws.teammate_tmux_pane_cursor);
        }
    }

    match pane::teammate_split_pane(&ctx.state, wid, idx, direction) {
        Ok(new_id) => {
            let new_idx = {
                let map = ctx.state.workspaces.read();
                let Some(ws) = map.get(&wid) else {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "workspace missing").into_response();
                };
                ws.pane_tree
                    .get_all_leaves()
                    .iter()
                    .position(|u| *u == new_id)
                    .unwrap_or(0)
            };
            let cmd = body.command.as_deref().map(str::trim).filter(|s| !s.is_empty());
            if let Err(e) = terminal::ensure_pane_pty_workspace(
                &ctx.state,
                wid,
                new_id,
                None,
                cwd.as_deref(),
                cmd,
                Some(new_idx),
            ) {
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        let _ = ws.pane_tree.close(new_id);
                    }
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("split created pane but PTY init failed: {e}"),
                )
                    .into_response();
            }
            {
                let mut map = ctx.state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                    ws.teammate_tmux_pane_cursor = new_idx;
                    // Initialize pane size for the new pane (default, will be updated on resize)
                    ws.pane_sizes.insert(new_id, (80, 120));
                    if let Some(name) = body
                        .window_name
                        .as_ref()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                    {
                        ws.teammate_pane_titles
                            .insert(new_id, name.to_string());
                    }
                }
            }
            let _ = ctx.handle.emit("teammate-layout-changed", ());
            let _ = ctx
                .handle
                .emit("teammate-active-pane-changed", new_id.to_string());
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "new_pane_id": new_id.to_string(),
                    "new_pane_index": new_idx,
                    "source_pane_index": idx,
                    "direction_inferred": inferred_direction,
                })),
            )
                .into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn route_capture(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let pane: usize = q
        .get("pane")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let lines: usize = q
        .get("lines")
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);
    let wid = ctx.state.active_workspace_id();
    let pid = match pane::teammate_pane_uuid_at_index(&ctx.state, wid, pane) {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    let text = ctx.state.get_pty_scrollback_tail(wid, pid, lines);
    (StatusCode::OK, text).into_response()
}

#[derive(Deserialize)]
struct SendBody {
    /// 显式 `send-keys -t %N`；与 `use_tmux_current_pane` 互斥。
    #[serde(default)]
    pane: Option<usize>,
    /// `send-keys -t ""` 或未带 `-t`：与真实 tmux 一致，发往「当前」窗格（由 `split-window` / `select-pane` 维护）。
    #[serde(default)]
    use_tmux_current_pane: bool,
    text: String,
}

async fn route_send_keys(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<SendBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();
    let pane_idx = if body.use_tmux_current_pane {
        ctx.state
            .workspaces
            .read()
            .get(&wid)
            .map(|ws| ws.teammate_tmux_pane_cursor)
            .unwrap_or(0)
    } else {
        body.pane.unwrap_or(0)
    };
    let pid = match pane::teammate_pane_uuid_at_index(&ctx.state, wid, pane_idx) {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    match terminal::write_pty_bytes_workspace(&ctx.state, wid, pid, body.text.as_bytes()) {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

/// `GET /api/v1/list-panes?json=1` — Claude Code agent-teams 用：真实窗格数、`active_index`、与 `#{pane_active}` 一致。
#[derive(Serialize)]
struct ListPanesJsonBody {
    active_index: usize,
    pane_count: usize,
    panes: Vec<PaneRowJson>,
}

#[derive(Serialize)]
struct PaneRowJson {
    index: usize,
    pane_id: String,
    uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

async fn route_list_panes(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let want_json = q.get("json").map(|s| s == "1").unwrap_or(false);
    let wid = ctx.state.active_workspace_id();

    let (lines, json_body) = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&wid) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
        };
        let leaves = ws.pane_tree.get_all_leaves();
        let pane_count = leaves.len();
        let active_index = if pane_count == 0 {
            0
        } else {
            ws.teammate_tmux_pane_cursor.min(pane_count - 1)
        };

        // 与真实 tmux `list-panes` 默认输出形态对齐，供 Claude Code TmuxBackend 解析（需含 `N: [colsxrows]`、`%N`、`(active)`）。
        const DEFAULT_COLS: u16 = 120;
        const DEFAULT_ROWS: u16 = 80;
        let lines: Vec<String> = if leaves.is_empty() {
            // 空树时仍输出一行，避免 TmuxBackend 收到空 stdout 而无法判定当前窗格。
            vec![format!("0: [{DEFAULT_COLS}x{DEFAULT_ROWS}] %0 (active)")]
        } else {
            leaves
                .iter()
                .enumerate()
                .map(|(i, _u)| {
                    let mut line = format!("{i}: [{DEFAULT_COLS}x{DEFAULT_ROWS}] %{i}");
                    if i == active_index {
                        line.push_str(" (active)");
                    }
                    line
                })
                .collect()
        };

        let json_body = ListPanesJsonBody {
            active_index: if leaves.is_empty() { 0 } else { active_index },
            pane_count: if leaves.is_empty() {
                1
            } else {
                pane_count
            },
            panes: leaves
                .iter()
                .enumerate()
                .map(|(i, u)| PaneRowJson {
                    index: i,
                    pane_id: format!("%{i}"),
                    uuid: u.to_string(),
                    title: ws.teammate_pane_titles.get(u).cloned(),
                })
                .collect(),
        };
        (lines, json_body)
    };

    if want_json {
        return Json(json_body).into_response();
    }
    (StatusCode::OK, lines.join("\n")).into_response()
}

// ========== Additional Route Handlers for Complete tmux Compatibility ==========

#[derive(Deserialize)]
struct SelectPaneBody {
    #[serde(default)]
    pane_index: Option<usize>,
    #[serde(default)]
    last: Option<bool>,
}

async fn route_select_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<SelectPaneBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();

    log_stderr_server(&format!("select-pane: index={:?}, last={:?}", body.pane_index, body.last));

    // Handle last-pane: swap with previous pane
    if body.last == Some(true) && body.pane_index.is_none() {
        let (new_cursor, new_pane_id) = {
            let mut map = ctx.state.workspaces.write();
            let Some(ws) = map.get_mut(&wid) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
            };
            let old_cursor = ws.teammate_tmux_pane_cursor;
            let new_cursor = ws.last_pane_index.unwrap_or(0);
            let leaves = ws.pane_tree.get_all_leaves();
            let new_pane_id = leaves.get(new_cursor).copied();

            ws.last_pane_index = Some(old_cursor);
            ws.teammate_tmux_pane_cursor = new_cursor;

            (new_cursor, new_pane_id)
        };

        if let Some(pid) = new_pane_id {
            let _ = ctx.handle.emit("teammate-active-pane-changed", pid.to_string());
        }

        return (StatusCode::OK, Json(serde_json::json!({
            "ok": true,
            "pane_index": new_cursor
        }))).into_response();
    }

    // Standard select-pane with explicit index
    if let Some(idx) = body.pane_index {
        let leaf_id = {
            let map = ctx.state.workspaces.read();
            let Some(ws) = map.get(&wid) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
            };
            let leaves = ws.pane_tree.get_all_leaves();
            if idx >= leaves.len() {
                return (StatusCode::BAD_REQUEST, "pane_index out of range").into_response();
            }
            Some(leaves[idx])
        };

        {
            let mut map = ctx.state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                ws.last_pane_index = Some(ws.teammate_tmux_pane_cursor);
                ws.teammate_tmux_pane_cursor = idx;
            }
        }

        if let Some(pid) = leaf_id {
            let _ = ctx
                .handle
                .emit("teammate-active-pane-changed", pid.to_string());
        }

        (StatusCode::OK, Json(serde_json::json!({
            "ok": true,
            "pane_index": idx
        }))).into_response()
    } else {
        (StatusCode::BAD_REQUEST, "pane_index or last required").into_response()
    }
}

async fn route_kill_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<SelectPaneBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();

    if let Some(idx) = body.pane_index {
        match pane::teammate_pane_uuid_at_index(&ctx.state, wid, idx) {
            Ok(pid) => {
                let state_ref: &AppState = &ctx.state;
                crate::commands::terminal::kill_pty_if_present(state_ref, wid, pid).await;
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        ws.teammate_pane_titles.remove(&pid);
                    ws.pane_sizes.remove(&pid);
                        let _ = ws.pane_tree.close(pid);
                    }
                }
                let _ = ctx.handle.emit("teammate-layout-changed", ());
                (StatusCode::OK, "ok").into_response()
            }
            Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        }
    } else {
        (StatusCode::BAD_REQUEST, "pane_index required").into_response()
    }
}

#[derive(Deserialize)]
struct ResizePaneBody {
    #[serde(default)]
    pane_index: Option<usize>,
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    adjustment: Option<i32>,
}

async fn route_resize_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<ResizePaneBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }

    log_stderr_server(&format!(
        "resize-pane: index={:?}, direction={:?}, adjustment={:?}",
        body.pane_index, body.direction, body.adjustment
    ));

    (StatusCode::OK, "ok").into_response()
}

#[derive(Deserialize)]
struct NewWindowBody {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    window_name: Option<String>,
}

async fn route_new_window(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<NewWindowBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }

    let wid = ctx.state.active_workspace_id();
    let cwd = body
        .cwd
        .as_ref()
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty());
    let cmd = body.command.as_deref().map(str::trim).filter(|s| !s.is_empty());

    match pane::teammate_split_pane(&ctx.state, wid, 0, "vertical") {
        Ok(new_id) => {
            let new_idx = {
                let map = ctx.state.workspaces.read();
                let Some(ws) = map.get(&wid) else {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "workspace missing").into_response();
                };
                ws.pane_tree
                    .get_all_leaves()
                    .iter()
                    .position(|u| *u == new_id)
                    .unwrap_or(0)
            };
            if let Err(e) = crate::commands::terminal::ensure_pane_pty_workspace(
                &ctx.state,
                wid,
                new_id,
                None,
                cwd.as_deref(),
                cmd,
                Some(new_idx),
            ) {
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        let _ = ws.pane_tree.close(new_id);
                    }
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("new-window: PTY init failed: {e}"),
                )
                    .into_response();
            }
            {
                let mut map = ctx.state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                ws.last_pane_index = Some(ws.teammate_tmux_pane_cursor);
                    ws.teammate_tmux_pane_cursor = new_idx;
                    if let Some(name) = body
                        .window_name
                        .as_ref()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                    {
                        ws.teammate_pane_titles
                            .insert(new_id, name.to_string());
                    }
                }
            }
            let _ = ctx.handle.emit("teammate-layout-changed", ());
            let _ = ctx
                .handle
                .emit("teammate-active-pane-changed", new_id.to_string());
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "new_pane_id": new_id.to_string(),
                    "new_pane_index": new_idx,
                })),
            )
                .into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Serialize)]
struct ListWindowsRowJson {
    index: usize,
    name: String,
    pane_count: usize,
    active_pane_index: usize,
    active: bool,
}

#[derive(Serialize)]
struct ListWindowsJsonBody {
    windows: Vec<ListWindowsRowJson>,
}

async fn route_list_windows(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let want_json = q.get("json").map(|s| s == "1").unwrap_or(false);
    let wid = ctx.state.active_workspace_id();

    let (line, json_body) = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&wid) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
        };
        let leaves = ws.pane_tree.get_all_leaves();
        let pane_count = leaves.len().max(1);
        let active_pane_index = if leaves.is_empty() {
            0usize
        } else {
            ws.teammate_tmux_pane_cursor.min(leaves.len() - 1)
        };
        let primary_name = leaves
            .get(active_pane_index)
            .and_then(|u| ws.teammate_pane_titles.get(u))
            .cloned()
            .or_else(|| leaves.iter().find_map(|u| ws.teammate_pane_titles.get(u).cloned()))
            .unwrap_or_else(|| "wind".to_string());
        let line = format!(
            "0: {}* ({} panes) [80x24] @0 (active)",
            primary_name, pane_count
        );
        let json_body = ListWindowsJsonBody {
            windows: vec![ListWindowsRowJson {
                index: 0,
                name: primary_name.clone(),
                pane_count,
                active_pane_index,
                active: true,
            }],
        };
        (line, json_body)
    };

    if want_json {
        return Json(json_body).into_response();
    }
    (StatusCode::OK, line).into_response()
}

async fn route_list_clients(State(ctx): State<TeammateCtx>, headers: HeaderMap) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    (StatusCode::OK, "").into_response()
}

fn log_stderr_server(msg: &str) {
    eprintln!("[wind-teammate] {}", msg);
}
