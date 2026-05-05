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
use chrono::{DateTime, Local, Utc};
use uuid::Uuid;

use crate::commands::{pane, terminal};
use crate::state::{AppState, PaneState, Workspace};
use tauri::Emitter;

#[derive(Clone)]
struct TeammateCtx {
    state: AppState,
    token: Arc<String>,
    handle: tauri::AppHandle,
}

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    if headers
        .get("x-ridge-token")
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
///
/// 进程保护：
/// - 线程体包在 `catch_unwind` 里，路由 handler panic 不会连带把 Ridge 主进程带走；
/// - panic 捕获后，延时 1s 自动重启 server 线程（尝试最多 5 次）；
/// - tokio runtime 构建失败不触发重启（多半是 FD 耗尽等系统性原因）。
pub fn spawn_teammate_server(
    handle: tauri::AppHandle,
    state: AppState,
    ready: Option<std::sync::mpsc::Sender<()>>,
) {
    spawn_teammate_inner(handle, state, ready, 0);
}

const TEAMMATE_RESTART_MAX: u32 = 5;

fn spawn_teammate_inner(
    handle: tauri::AppHandle,
    state: AppState,
    ready: Option<std::sync::mpsc::Sender<()>>,
    attempt: u32,
) {
    let handle_for_retry = handle.clone();
    let state_for_retry = state.clone();
    let _ = std::thread::Builder::new()
        .name("ridge-teammate-http".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(target: "ridge::teammate", error = %e, "runtime build failed");
                        if let Some(tx) = ready {
                            let _ = tx.send(());
                        }
                        return;
                    }
                };
                rt.block_on(run_server(handle, state, ready));
            }));
            if result.is_err() {
                tracing::error!(
                    target: "ridge::teammate",
                    attempt = attempt,
                    "teammate HTTP thread panicked (isolated); scheduling restart"
                );
                if attempt + 1 < TEAMMATE_RESTART_MAX {
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    spawn_teammate_inner(handle_for_retry, state_for_retry, None, attempt + 1);
                } else {
                    tracing::error!(
                        target: "ridge::teammate",
                        "teammate HTTP restart budget exhausted; giving up"
                    );
                }
            }
        });
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
            eprintln!("[ridge] teammate bind failed: {e}");
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
        "[ridge] teammate HTTP {base_url} (inject Ridge_TEAMMATE_* into PTY; use tmux shim on PATH)"
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
        .route("/api/v1/spawn-process", post(route_spawn_process))
        .route("/api/v1/list-panes", get(route_list_panes))
    // Pane management
    .route("/api/v1/select-pane", post(route_select_pane))
    .route("/api/v1/kill-pane", post(route_kill_pane))
    .route("/api/v1/resize-pane", post(route_resize_pane))
    // Window management
    .route("/api/v1/new-window", post(route_new_window))
        .route("/api/v1/rename-pane", post(route_rename_pane))
        .route("/api/v1/list-windows", get(route_list_windows))
        .route("/api/v1/list-sessions", get(route_list_sessions))
        .route("/api/v1/list-clients", get(route_list_clients))
        // Agent-pane management
        .route("/api/v1/register-agent", post(route_register_agent))
        .route("/api/v1/release-pane", post(route_release_pane))
        .route("/api/v1/find-idle-pane", get(route_find_idle_pane))
        .with_state(ctx);

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[ridge] teammate server stopped: {e}");
    }
}

// ========== Agent-Pane Management Helpers ==========

/// 查找空闲 pane（返回 pane index）
fn find_idle_pane_index(state: &AppState, wid: uuid::Uuid) -> Option<usize> {
    let map = state.workspaces.read();
    let Some(ws) = map.get(&wid) else {
        return None;
    };
    let leaves = ws.pane_tree.get_all_leaves();
    for (idx, pane_id) in leaves.iter().enumerate() {
        if let Some(pane_state) = ws.teammate_pane_states.get(pane_id) {
            if *pane_state == crate::state::PaneState::Idle {
                return Some(idx);
            }
        }
    }
    None
}

/// CWD normalisation matching `engine/pty.rs::normalize_cwd_str`. Inlined
/// here to keep the helper module-private; the two callsites must stay in
/// sync if the rule ever changes (forward-slashes on Windows, identity
/// elsewhere).
fn normalize_cwd_for_match(raw: &str) -> String {
    #[cfg(windows)]
    {
        raw.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        raw.to_string()
    }
}

/// Pick the leaf-pane index that should be split when a teammate split
/// request lands without an explicit `-t`. Priority:
///
/// 1. **CWD match + idle**: a pane whose current cwd equals `target_cwd`
///    AND whose `teammate_pane_state` is `Idle` is the best candidate —
///    splitting it keeps the agent in its preferred directory and avoids
///    bumping a busy peer.
/// 2. **CWD match (any state)**: failing that, any pane with the same cwd
///    works — agents often expect their split to inherit the cwd.
/// 3. **Largest area**: no cwd match → split the biggest pane so the new
///    pane has more room to breathe. Tie-break by preferring the highest
///    leaf index (most recently created), which feels less disruptive.
///
/// Returns `None` only if the workspace has no leaves at all.
fn select_split_target(
    state: &AppState,
    wid: uuid::Uuid,
    target_cwd: Option<&std::path::Path>,
) -> Option<usize> {
    let map = state.workspaces.read();
    let ws = map.get(&wid)?;
    let leaves = ws.pane_tree.get_all_leaves();
    if leaves.is_empty() {
        return None;
    }

    let target_norm = target_cwd.map(|p| normalize_cwd_for_match(&p.to_string_lossy()));

    // Tier 1+2: scan for cwd-matched panes; remember whether each is idle.
    let cwd_match: Vec<(usize, uuid::Uuid, bool)> = leaves
        .iter()
        .enumerate()
        .filter_map(|(i, pid)| {
            let pane_norm = normalize_cwd_for_match(
                &ws.pane_tree.panes.get(pid)?.cwd.as_ref()?.to_string_lossy(),
            );
            if Some(&pane_norm) == target_norm.as_ref() {
                let is_idle = matches!(
                    ws.teammate_pane_states.get(pid),
                    Some(crate::state::PaneState::Idle)
                );
                Some((i, *pid, is_idle))
            } else {
                None
            }
        })
        .collect();

    if !cwd_match.is_empty() {
        return cwd_match
            .iter()
            .find(|(_, _, idle)| *idle)
            .or_else(|| cwd_match.first())
            .map(|(i, _, _)| *i);
    }

    // Tier 3: pick the largest pane by character area. Tie-break by
    // preferring the LOWEST leaf index — keeps the split target stable
    // across resize events (top-most pane wins consistently).
    leaves
        .iter()
        .enumerate()
        .map(|(i, pid)| {
            let (r, c) = ws.pane_sizes.get(pid).copied().unwrap_or((80, 120));
            (i, r as u32 * c as u32)
        })
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
}

/// 查找空闲 pane 的 UUID
#[allow(dead_code)] // internal helper kept for upcoming auto-assign-pane logic
fn find_idle_pane_uuid(state: &AppState, wid: uuid::Uuid) -> Option<uuid::Uuid> {
    let map = state.workspaces.read();
    let Some(ws) = map.get(&wid) else {
        return None;
    };
    let leaves = ws.pane_tree.get_all_leaves();
    for pane_id in leaves.iter() {
        if let Some(pane_state) = ws.teammate_pane_states.get(pane_id) {
            if *pane_state == crate::state::PaneState::Idle {
                return Some(*pane_id);
            }
        }
    }
    None
}

/// 注册 agent 到 pane
fn register_agent_to_pane(state: &AppState, wid: uuid::Uuid, agent_id: &str, pane_id: uuid::Uuid) {
    let mut map = state.workspaces.write();
    if let Some(ws) = map.get_mut(&wid) {
        ws.teammate_agent_pane_map.insert(agent_id.to_string(), pane_id);
        ws.teammate_pane_states.insert(pane_id, crate::state::PaneState::Busy);
    }
}

/// 释放 pane（标记为空闲）
fn release_pane(state: &AppState, wid: uuid::Uuid, pane_id: uuid::Uuid) {
    let mut map = state.workspaces.write();
    if let Some(ws) = map.get_mut(&wid) {
        ws.teammate_pane_states.insert(pane_id, crate::state::PaneState::Idle);
        // 清理 agent 映射
        ws.teammate_agent_pane_map.retain(|_, v| *v != pane_id);
    }
}

/// 通过 agent_id 查找 pane
#[allow(dead_code)] // reverse lookup retained for upcoming /focus-pane HTTP route
fn find_pane_by_agent(state: &AppState, wid: uuid::Uuid, agent_id: &str) -> Option<uuid::Uuid> {
    let map = state.workspaces.read();
    let Some(ws) = map.get(&wid) else {
        return None;
    };
    ws.teammate_agent_pane_map.get(agent_id).copied()
}

// ========== Agent-Pane Management Routes ==========

#[derive(Deserialize)]
struct RegisterAgentBody {
    agent_id: String,
    pane_index: Option<usize>,
}

async fn route_register_agent(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<RegisterAgentBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();

    // 找到对应的 pane_id
    let pane_id = if let Some(idx) = body.pane_index {
        match crate::commands::pane::teammate_pane_uuid_at_index(&ctx.state, wid, idx) {
            Ok(u) => u,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        }
    } else {
        // 如果没有指定 pane_index，使用当前 cursor
        let map = ctx.state.workspaces.read();
        let ws = map.get(&wid);
        let cursor = ws.map(|w| w.teammate_tmux_pane_cursor).unwrap_or(0);
        drop(map);
        match crate::commands::pane::teammate_pane_uuid_at_index(&ctx.state, wid, cursor) {
            Ok(u) => u,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        }
    };

    register_agent_to_pane(&ctx.state, wid, &body.agent_id, pane_id);
    // Emit so the frontend re-fetches layout and renders the "busy" indicator
    // on the newly-claimed pane.
    let _ = ctx.handle.emit("teammate-layout-changed", ());
    (StatusCode::OK, Json(serde_json::json!({ "ok": true, "pane_id": pane_id.to_string() })))
        .into_response()
}

#[derive(Deserialize)]
struct ReleasePaneBody {
    pane_index: Option<usize>,
    pane_id: Option<String>,
}

async fn route_release_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<ReleasePaneBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();

    let pane_id = if let Some(ref pid_str) = body.pane_id {
        match uuid::Uuid::parse_str(pid_str) {
            Ok(u) => u,
            Err(_) => {
                return (StatusCode::BAD_REQUEST, "invalid pane_id").into_response();
            }
        }
    } else if let Some(idx) = body.pane_index {
        match crate::commands::pane::teammate_pane_uuid_at_index(&ctx.state, wid, idx) {
            Ok(u) => u,
            Err(e) => return (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
        }
    } else {
        return (StatusCode::BAD_REQUEST, "need pane_index or pane_id").into_response();
    };

    release_pane(&ctx.state, wid, pane_id);
    // Emit layout-changed so the frontend drops the "busy" indicator.
    let _ = ctx.handle.emit("teammate-layout-changed", ());
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn route_find_idle_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();

    if let Some(idx) = find_idle_pane_index(&ctx.state, wid) {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "pane_index": idx })),
        )
            .into_response()
    } else {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "pane_index": serde_json::Value::Null })),
        )
            .into_response()
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
    /// 是否允许复用空闲 pane（默认 true）
    #[serde(default = "default_true")]
    allow_idle_reuse: bool,
}

fn default_true() -> bool {
    true
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

    // 检查是否有空闲 pane 可以复用（如果没有显式指定 pane_index）
    if body.allow_idle_reuse && body.pane_index.is_none() {
        if let Some(idle_idx) = find_idle_pane_index(&ctx.state, wid) {
            // 找到空闲 pane，标记为 Busy 并返回
            let idle_pane_id = {
                let map = ctx.state.workspaces.read();
                let ws = map.get(&wid).unwrap();
                ws.pane_tree.get_all_leaves().get(idle_idx).copied()
            };
            if let Some(pane_id) = idle_pane_id {
                let mut map = ctx.state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                    ws.teammate_pane_states.insert(pane_id, crate::state::PaneState::Busy);
                    ws.teammate_tmux_pane_cursor = idle_idx;
                }
            }
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "reused_pane_index": idle_idx,
                    "new_pane_index": idle_idx,
                    "source_pane_index": idle_idx,
                    "reused": true,
                })),
            )
                .into_response();
        }
    }

    // Split target selection:
    // 1. If explicit pane_index from -t flag → use it.
    // 2. Else delegate to `select_split_target`: prefer cwd-matched + idle,
    //    fall back to cwd-matched (any state), fall back to largest-area
    //    pane. Direction is then inferred from the chosen pane's shape so
    //    the resulting two panes stay roughly square.
    let (idx, inferred_direction) = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&wid) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
        };
        let leaves = ws.pane_tree.get_all_leaves();
        let pane_count = leaves.len();

        if let Some(explicit_idx) = body.pane_index {
            if explicit_idx < pane_count {
                (explicit_idx, false)
            } else {
                return (StatusCode::BAD_REQUEST, "pane_index out of range").into_response();
            }
        } else {
            // Drop the read lock so `select_split_target` can take its own.
            drop(map);
            let target_path = body.cwd.as_deref().map(std::path::Path::new);
            let target_idx = select_split_target(&ctx.state, wid, target_path).unwrap_or(0);

            // Re-acquire to read the chosen pane's shape for direction inference.
            let map = ctx.state.workspaces.read();
            let Some(ws) = map.get(&wid) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
            };
            let leaves = ws.pane_tree.get_all_leaves();
            let (rows, cols) = leaves
                .get(target_idx)
                .and_then(|pid| ws.pane_sizes.get(pid).copied())
                .unwrap_or((80, 120));
            let inferred = cols > rows; // wider than tall → horizontal (left/right) split

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

    // CWD resolution: explicit `-c` wins, otherwise inherit the source pane's cwd
    // so the new terminal opens in the same directory as the pane it was split from.
    let cwd = body
        .cwd
        .as_ref()
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            let map = ctx.state.workspaces.read();
            map.get(&wid).and_then(|ws| {
                let leaves = ws.pane_tree.get_all_leaves();
                leaves
                    .get(idx)
                    .and_then(|pid| ws.pane_tree.panes.get(pid))
                    .and_then(|p| p.cwd.clone())
            })
        });

    // Track last pane before updating cursor
    {
        let mut map = ctx.state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            ws.last_pane_index = Some(ws.teammate_tmux_pane_cursor);
        }
    }

    match pane::teammate_split_pane(&ctx.state, wid, idx, direction) {
        Ok(new_id) => {
            // Seed the new pane's tree-level cwd so subsequent splits off of it
            // inherit the same directory without needing shell-integration updates.
            if let Some(ref dir) = cwd {
                let mut map = ctx.state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                    if let Some(new_pane) = ws.pane_tree.panes.get_mut(&new_id) {
                        new_pane.cwd = Some(dir.clone());
                    }
                }
            }
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

            // Bookkeeping + readiness signal. The oneshot lets us *observe*
            // whether the front-end's `activate_pane_pty` actually launched
            // the child, so we can return an honest HTTP status to the agent.
            {
                let mut map = ctx.state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                    ws.teammate_metrics.split_attempts += 1;
                }
            }
            let trace_id = uuid::Uuid::new_v4().to_string();
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();

            if let Err(e) = terminal::ensure_pane_pty_workspace(
                &ctx.state,
                wid,
                new_id,
                None,
                cwd.as_deref(),
                cmd,
                None,
                Some(new_idx),
                Some(ready_tx),
                Some(trace_id.clone()),
            ) {
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        let _ = ws.pane_tree.close(new_id);
                        ws.pane_sizes.remove(&new_id);
                        *ws.teammate_metrics.failures.entry("phase1_failed".into()).or_insert(0) += 1;
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
                // Mark new pane as Busy
                ws.teammate_pane_states.insert(new_id, PaneState::Busy);
                    // Initialize pane size for the new pane (default, will be updated on resize)
                    ws.pane_sizes.insert(new_id, (80, 120));
                // Mark new pane as Busy (has an agent running)
                ws.teammate_pane_states.insert(new_id, PaneState::Busy);
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
            let _ = ctx.handle.emit(
                "teammate-layout-changed",
                serde_json::json!({ "trace_id": trace_id }),
            );
            let _ = ctx
                .handle
                .emit("teammate-active-pane-changed", new_id.to_string());

            // 30s watchdog: if the front-end never calls `activate_pane_pty`,
            // drain the orphan PendingSpawn so the slave/cmd are dropped (and
            // the pane removed from the layout). 30s is a generous budget —
            // a healthy mount completes in <1s.
            //
            // Emit `teammate-layout-changed` after cleanup so the front-end
            // re-renders without the now-dead leaf. Without this the user
            // sees a phantom pane that swallows clicks but has no PTY.
            let watch_state = ctx.state.clone();
            let watch_handle = ctx.handle.clone();
            let watch_wid = wid;
            let watch_pid = new_id;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let cleaned = {
                    let mut map = watch_state.workspaces.write();
                    if let Some(ws) = map.get_mut(&watch_wid) {
                        if ws.pending_spawns.remove(&watch_pid).is_some() {
                            let _ = ws.pane_tree.close(watch_pid);
                            ws.pane_sizes.remove(&watch_pid);
                            *ws.teammate_metrics.failures.entry("watchdog_30s".into()).or_insert(0) += 1;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if cleaned {
                    let _ = watch_handle.emit(
                        "teammate-layout-changed",
                        serde_json::Value::Null,
                    );
                }
            });

            // Wait up to 3s for the front-end to mount + fit + activate.
            // tokio::time::timeout wraps the recv future; the outer Result
            // is "did the timeout elapse"; the inner is "did the sender drop";
            // the innermost is the actual spawn outcome.
            match tokio::time::timeout(std::time::Duration::from_secs(3), ready_rx).await {
                Ok(Ok(Ok(()))) => {
                    {
                        let mut map = ctx.state.workspaces.write();
                        if let Some(ws) = map.get_mut(&wid) {
                            ws.teammate_metrics.split_success += 1;
                        }
                    }
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "ok": true,
                            "new_pane_id": new_id.to_string(),
                            "new_pane_index": new_idx,
                            "source_pane_index": idx,
                            "direction_inferred": inferred_direction,
                            "trace_id": trace_id,
                        })),
                    )
                        .into_response()
                }
                Ok(Ok(Err(e))) => {
                    {
                        let mut map = ctx.state.workspaces.write();
                        if let Some(ws) = map.get_mut(&wid) {
                            *ws.teammate_metrics.failures.entry("activate_failed".into()).or_insert(0) += 1;
                        }
                    }
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("activate_pane_pty failed: {e}"),
                    )
                        .into_response()
                }
                _ => {
                    // Timeout or sender dropped without sending. Don't tear
                    // down the pending entry here — the 30s watchdog handles
                    // that path and the front-end might still complete late.
                    {
                        let mut map = ctx.state.workspaces.write();
                        if let Some(ws) = map.get_mut(&wid) {
                            *ws.teammate_metrics.failures.entry("activate_timeout_3s".into()).or_insert(0) += 1;
                        }
                    }
                    (
                        StatusCode::GATEWAY_TIMEOUT,
                        format!("activate_pane_pty timed out after 3s (trace_id={trace_id})"),
                    )
                        .into_response()
                }
            }
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
    // `get_pty_scrollback_tail` now takes byte budget; pull generously and
    // trim to the requested line count here to preserve the old HTTP shape.
    let chunk = ctx.state.get_pty_scrollback_tail(wid, pid, 512 * 1024);
    let text = if lines == 0 {
        String::new()
    } else {
        // Walk from the end finding the Nth '\n'; return the tail after it.
        let bytes = chunk.bytes.as_bytes();
        let mut nl_seen = 0usize;
        let mut cut = 0usize;
        for i in (0..bytes.len()).rev() {
            if bytes[i] == b'\n' {
                nl_seen += 1;
                if nl_seen == lines {
                    cut = i + 1;
                    break;
                }
            }
        }
        if nl_seen < lines {
            chunk.bytes
        } else {
            // `cut` lands on a UTF-8 boundary (immediately after '\n').
            chunk.bytes[cut..].to_string()
        }
    };
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

#[derive(Deserialize)]
struct SpawnProcessBody {
    #[serde(default)]
    pane: Option<usize>,
    #[serde(default)]
    use_tmux_current_pane: bool,
    #[serde(default)]
    cwd: Option<String>,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
}

async fn route_spawn_process(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<SpawnProcessBody>,
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
    let cwd = body
        .cwd
        .as_ref()
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty());
    let mut command = terminal::StructuredPtyCommand {
        program: body.program,
        args: body.args,
        env: body.env,
    };
    // On Windows, .js files must be run via node.exe — normalize before spawning.
    #[cfg(windows)]
    {
        command = normalize_windows_command(&command);
    }
    if let Err(e) = terminal::ensure_pane_pty_workspace(
        &ctx.state,
        wid,
        pid,
        None,
        cwd.as_deref(),
        None,
        Some(command),
        Some(pane_idx),
        None,
        None,
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("structured spawn failed: {e}"),
        )
            .into_response();
    }
    {
        let mut map = ctx.state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            ws.teammate_tmux_pane_cursor = pane_idx;
        }
    }
    let _ = ctx
        .handle
        .emit("teammate-active-pane-changed", pid.to_string());
    (StatusCode::OK, "ok").into_response()
}


#[cfg(windows)]
fn normalize_windows_command(
    command: &terminal::StructuredPtyCommand,
) -> terminal::StructuredPtyCommand {
    let mut out = command.clone();
    if out.program.to_ascii_lowercase().ends_with(".js") {
        let script = out.program.clone();
        let mut args = Vec::with_capacity(out.args.len() + 1);
        args.push(script.clone());
        args.extend(out.args);
        out.args = args;

        let candidate = std::path::Path::new(&script)
            .ancestors()
            .find_map(|a| {
                let name = a.file_name()?.to_string_lossy().to_ascii_lowercase();
                if name == "node_modules" {
                    a.parent().map(|parent| parent.join("node.exe"))
                } else {
                    None
                }
            })
            .filter(|p| p.is_file());
        out.program = candidate
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "node".to_string());
    }
    out
}

async fn route_list_panes(State(ctx): State<TeammateCtx>, headers: HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
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
                    cwd: ws.pane_tree.panes.get(u)
                        .and_then(|p| p.cwd.as_ref())
                        .map(|c| c.to_string_lossy().replace('\\', "/")),
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
        // No index or direction — acknowledge silently (handles -e/-d/-Z modifier-only calls)
        (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
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
                        // Mirror the cleanup done by close_pane so a teammate-
                        // initiated kill doesn't leave an orphaned agent_state.
                        ws.teammate_pane_titles.remove(&pid);
                        ws.teammate_pane_states.remove(&pid);
                        ws.teammate_agent_pane_map.retain(|_, v| *v != pid);
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
                None,
                Some(new_idx),
                None,
                None,
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
                // Mark new pane as Busy
                ws.teammate_pane_states.insert(new_id, PaneState::Busy);
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
    /// Current working directory reported via OSC 7; None until shell integration fires.
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
}

#[derive(Serialize)]
struct ListWindowsRowJson {
    index: usize,
    name: String,
    pane_count: usize,
    active_pane_index: usize,
    active: bool,
}

// ─── rename-pane ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RenamePaneBody {
    #[serde(default)]
    pane_index: Option<usize>,
    name: String,
}

/// Set or clear the display title for a Ridge pane so `rename-window <name>`
/// from Claude Code's tmux backend is surfaced in the pane header.
async fn route_rename_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<RenamePaneBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = ctx.state.active_workspace_id();
    let name = body.name.trim().to_string();

    let target_idx = body.pane_index.unwrap_or_else(|| {
        ctx.state.workspaces.read()
            .get(&wid)
            .map(|ws| ws.teammate_tmux_pane_cursor)
            .unwrap_or(0)
    });

    match pane::teammate_pane_uuid_at_index(&ctx.state, wid, target_idx) {
        Ok(pid) => {
            {
                let mut map = ctx.state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                    if name.is_empty() {
                        ws.teammate_pane_titles.remove(&pid);
                    } else {
                        ws.teammate_pane_titles.insert(pid, name);
                    }
                }
            }
            let _ = ctx.handle.emit("teammate-layout-changed", ());
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
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
            .unwrap_or_else(|| "ridge".to_string());
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

fn workspace_first_pty_size(ws: &Workspace) -> (u16, u16) {
    for h in ws.terminals.values() {
        if let Ok(s) = h.master.lock().get_size() {
            return (s.cols.max(1), s.rows.max(1));
        }
    }
    (120, 80)
}

/// tmux 默认 `list-sessions` 行首为 `name:`，会话名不能含冒号（否则解析歧义）。
fn tmux_list_sessions_label(id: Uuid, user_name: Option<&str>) -> String {
    let from_user = user_name.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        s.chars()
            .map(|c| match c {
                ':' | '\n' | '\r' => '_',
                _ => c,
            })
            .collect::<String>()
    });
    let cleaned = from_user.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if let Some(s) = cleaned {
        return s.to_string();
    }
    let compact: String = id.to_string().replace('-', "");
    let n = compact.len().min(8);
    format!("ws{}", &compact[..n])
}

async fn route_list_sessions(State(ctx): State<TeammateCtx>, headers: HeaderMap) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let active = ctx.state.active_workspace_id();
    let order = ctx.state.workspace_order.read().clone();
    let names = ctx.state.workspace_names.read().clone();
    let map = ctx.state.workspaces.read();

    let mut lines: Vec<String> = Vec::with_capacity(order.len());
    for wid in order.iter() {
        let Some(ws) = map.get(wid) else {
            continue;
        };
        let label = tmux_list_sessions_label(*wid, names.get(wid).map(String::as_str));
        let (cols, rows) = workspace_first_pty_size(ws);
        let created_local: DateTime<Local> = DateTime::<Utc>::from(ws.created_at).with_timezone(&Local);
        let date_str = created_local.format("%a %b %d %H:%M:%S %Y").to_string();
        // Ridge 每个工作区对应 tmux 的一个 session、一个 window（多 pane 为分屏）。
        let mut line = format!(
            "{label}: 1 windows (created {date_str}) [{cols}x{rows}]"
        );
        if *wid == active {
            line.push_str(" (attached)");
        }
        lines.push(line);
    }

    (StatusCode::OK, lines.join("\n")).into_response()
}

async fn route_list_clients(State(ctx): State<TeammateCtx>, headers: HeaderMap) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    (StatusCode::OK, "").into_response()
}

fn log_stderr_server(msg: &str) {
    eprintln!("[ridge-teammate] {}", msg);
}
