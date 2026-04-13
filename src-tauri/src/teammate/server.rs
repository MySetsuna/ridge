use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
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
pub fn spawn_teammate_server(handle: tauri::AppHandle, state: AppState) {
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
                    return;
                }
            };
            rt.block_on(run_server(handle, state));
        })
        .ok();
}

async fn run_server(handle: tauri::AppHandle, app_state: AppState) {
    let token = uuid::Uuid::new_v4().to_string();
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[wind] teammate bind failed: {e}");
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
    let idx = body.pane_index.unwrap_or(0);
    let dir = if body.horizontal {
        "horizontal"
    } else {
        "vertical"
    };
    match pane::teammate_split_pane(&ctx.state, wid, idx, dir) {
        Ok(new_id) => {
            let _ = ctx.handle.emit("teammate-layout-changed", ());
            if let Some(cmd) = body.command.filter(|s| !s.is_empty()) {
                let app = ctx.state.clone();
                let h = ctx.handle.clone();
                tokio::spawn(async move {
                    for _ in 0..80 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                        if terminal::write_pty_bytes_workspace(&app, wid, new_id, cmd.as_bytes())
                            .is_ok()
                        {
                            let _ =
                                terminal::write_pty_bytes_workspace(&app, wid, new_id, b"\r");
                            break;
                        }
                    }
                    let _ = h.emit("teammate-layout-changed", ());
                });
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "new_pane_id": new_id.to_string(),
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
    #[serde(default)]
    pane: usize,
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
    let pid = match pane::teammate_pane_uuid_at_index(&ctx.state, wid, body.pane) {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    };
    match terminal::write_pty_bytes_workspace(&ctx.state, wid, pid, body.text.as_bytes()) {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn route_list_panes(State(ctx): State<TeammateCtx>, headers: HeaderMap) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();
    let lines: Vec<String> = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&wid) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
        };
        ws.pane_tree
            .get_all_leaves()
            .iter()
            .enumerate()
            .map(|(i, u)| format!("%{} {}", i, u))
            .collect()
    };
    (StatusCode::OK, lines.join("\n")).into_response()
}
