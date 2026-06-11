use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::commands::{pane, terminal};
use crate::state::{AppState, PaneState, Workspace};
use tauri::Emitter;

use super::layout_event::{LayoutChange, TEAMMATE_LAYOUT_CHANGED};
use super::native::{self, NativeError};
use crate::engine::pane_tree::SplitDirection;
use crate::engine::parser::PaneParser;
use crate::engine::pty::{spawn_pty_reader, PtyHandle};

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

/// teammate 放置被拒原因（H1 fail-closed）：用于区分排障文案 + 指标判别。
#[derive(Debug)]
enum WorkspaceReject {
    /// `X-Ridge-Workspace` 头缺失或非法 UUID → `RIDGE_WORKSPACE_ID` 未传播到 agent env。
    MissingOrInvalidHeader,
    /// 头合法但指向的工作区已不存在（发起工作区在 agent 存活期间被关闭）。
    UnknownWorkspace(uuid::Uuid),
}

/// 严格解析「发起方所在工作区」：**fail-closed**，不回退 `active_workspace_id()`。
/// 仅当 `X-Ridge-Workspace` 头存在、是合法 UUID、且指向一个**活着的**工作区时才返回。
///
/// 前提（已核验，见 `commands/terminal.rs` `(Some(bind), _)` arm）：能拿到 shim 的
/// PTY 必同时被注入 `RIDGE_WORKSPACE_ID` → 合法 teammate 调用恒带本头；fail-closed 只
/// 拒绝「env 被剥离 / 非 teammate / 发起工作区已关闭」等异常，不误杀正常 spawn。
fn caller_workspace_id_strict(
    ctx: &TeammateCtx,
    headers: &HeaderMap,
) -> Result<uuid::Uuid, WorkspaceReject> {
    let id = parse_workspace_header(headers)?;
    if ctx.state.workspaces.read().contains_key(&id) {
        Ok(id)
    } else {
        Err(WorkspaceReject::UnknownWorkspace(id))
    }
}

/// Pure header parse (no state): `X-Ridge-Workspace` → UUID, else MissingOrInvalid.
/// Existence check lives in `caller_workspace_id_strict`. Split out so the
/// missing/invalid-vs-valid classification is unit-testable without a full ctx.
fn parse_workspace_header(headers: &HeaderMap) -> Result<uuid::Uuid, WorkspaceReject> {
    let raw = headers
        .get("x-ridge-workspace")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(WorkspaceReject::MissingOrInvalidHeader)?;
    uuid::Uuid::parse_str(raw).map_err(|_| WorkspaceReject::MissingOrInvalidHeader)
}

/// 宽松解析：缺头/无效时回退 GUI 活动工作区。
///
/// 【使用约束，勿误用】**仅限只读 / 装饰性路由**（list-panes、capture、list-windows、
/// select-pane[改 cursor]、rename-pane[改标题]——误 targeting 至多读/装饰错对象，无害）。
/// **破坏性（kill-pane）/ 注入（send-keys）/ 建-pane（split、spawn-process、new-window、
/// summon）/ 状态（register/release）路由必须改用 `caller_workspace_id_strict` +
/// `workspace_reject_response` 走 fail-closed**——partial-env-strip 异常下回退 active_ws
/// 会造成真实误操作（删错 pane / 写错 PTY / 在错工作区建 pane）。新增任何破坏性路由
/// 务必用 strict，勿顺手用本函数。
fn caller_workspace_id_or_active(ctx: &TeammateCtx, headers: &HeaderMap) -> uuid::Uuid {
    caller_workspace_id_strict(ctx, headers).unwrap_or_else(|_| ctx.state.active_workspace_id())
}

/// 把 `WorkspaceReject` 映射成明确的 HTTP 错误 + 递增可观测指标 + 结构化日志。
/// 指标记在 GUI 当前活动工作区（被拒调用本身没有合法工作区可归属）。
fn workspace_reject_response(
    ctx: &TeammateCtx,
    reject: WorkspaceReject,
) -> axum::response::Response {
    let (metric_key, status, msg) = match reject {
        WorkspaceReject::MissingOrInvalidHeader => (
            "workspace_rejected_missing_header",
            StatusCode::BAD_REQUEST,
            "teammate placement rejected: missing or invalid X-Ridge-Workspace header \
             (RIDGE_WORKSPACE_ID not propagated to agent env)"
                .to_string(),
        ),
        WorkspaceReject::UnknownWorkspace(id) => (
            "workspace_rejected_unknown",
            StatusCode::CONFLICT,
            format!("teammate placement rejected: originating workspace {id} no longer exists"),
        ),
    };
    let wid = ctx.state.active_workspace_id();
    {
        let mut map = ctx.state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            *ws.teammate_metrics
                .failures
                .entry(metric_key.into())
                .or_insert(0) += 1;
        }
    }
    tracing::warn!(target: "ridge::teammate", reason = metric_key, "{msg}");
    (status, msg).into_response()
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

/// 「按需启动」：首个 PTY 创建时调用，幂等地拉起 teammate HTTP server 并阻塞等其绑定，
/// 保证 `RIDGE_TEAMMATE_*` 在 shell 启动前就绪。已绑定则立即返回（绝大多数调用走此快路径，
/// 包括 agent 在自己的 teammate PTY 里再发 split —— 那时 server 早已在跑）。
pub fn ensure_teammate_started(state: &AppState) {
    if state.teammate_binding.read().is_some() {
        return;
    }
    static START_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = START_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap();
    // 双检查：等锁期间可能已被并发的 PTY 创建启动。
    if state.teammate_binding.read().is_some() {
        return;
    }
    let Some(handle) = state.app_handle.get().cloned() else {
        // setup 尚未 stash handle（理论上不会发生）；放弃惰性启动，留待下次 PTY 创建。
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_teammate_server(handle, state.clone(), Some(tx));
    let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
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
                // 控制面 QPS 极低（偶发的 split/send-keys/list），单线程运行时足矣：
                // 把多线程运行时按核数摊出的 N 条常驻空闲 worker 线程塌成 1 条，显著降占用。
                let rt = match tokio::runtime::Builder::new_current_thread()
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

    // The native `/api/v1/tmux/*` surface (everything except GUI-only summon) is
    // the SAME shared router the headless `ridge-cli` host mounts — one source of
    // truth in `ridge_tmux::http`. The desktop supplies a GUI-backed session
    // source so default-socket find-target still folds in visible workspaces.
    let native_ctx = ridge_tmux::http::NativeHttpCtx {
        token: ctx.token.clone(),
        gui: Arc::new(DesktopGuiSessions {
            state: ctx.state.clone(),
        }),
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
        // GUI-only native route: summon a headless session into a VISIBLE
        // workspace (needs AppState/AppHandle), so it stays on TeammateCtx
        // rather than in the shared, host-agnostic router below.
        .route("/api/v1/tmux/summon", post(route_tmux_summon))
        .with_state(ctx)
        // All other native tmux routes are served by the shared engine crate.
        .merge(ridge_tmux::http::native_router(native_ctx));

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[ridge] teammate server stopped: {e}");
    }
}

// ========== Agent-Pane Management Helpers ==========

/// 查找可复用的「空闲 shell 模式」pane（返回 pane index）。
///
/// 复用判定（按需求）：`mode == Terminal`（shell 面板）**且** 非 `Busy`。
///  - Editor 模式面板不是 shell，跳过；
///  - 未登记到 `teammate_pane_states` 的面板（用户手动开、停在提示符的空闲 shell）视为可复用，
///    这样「直接接管空闲 shell」也覆盖手动留下的终端。
/// 仅在传入的 `wid`（发起方工作区）内查找，绝不跨工作区。
fn find_idle_pane_index(state: &AppState, wid: uuid::Uuid) -> Option<usize> {
    let map = state.workspaces.read();
    let ws = map.get(&wid)?;
    let leaves = ws.pane_tree.get_all_leaves();
    for (idx, pane_id) in leaves.iter().enumerate() {
        // §host-guard (2026-06-11): only reuse panes Ridge itself created for a
        // teammate. The originating (host) pane — where the parent agent runs —
        // is never teammate-owned, so idle-reuse can never select it. Restores
        // the pre-refactor invariant (first teammate gets a fresh pane, idx≥1)
        // and stops spawn-process from clobbering the host PTY via leaf 0.
        if !ws.teammate_owned_panes.contains(pane_id) {
            continue;
        }
        let is_terminal = matches!(
            ws.pane_tree.panes.get(pane_id).map(|p| &p.mode),
            Some(crate::types::PaneMode::Terminal)
        );
        if !is_terminal {
            continue;
        }
        let busy = matches!(
            ws.teammate_pane_states.get(pane_id),
            Some(crate::state::PaneState::Busy)
        );
        if !busy {
            return Some(idx);
        }
    }
    None
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
        ws.teammate_agent_pane_map
            .insert(agent_id.to_string(), pane_id);
        ws.teammate_pane_states
            .insert(pane_id, crate::state::PaneState::Busy);
    }
}

/// 释放 pane（标记为空闲）
fn release_pane(state: &AppState, wid: uuid::Uuid, pane_id: uuid::Uuid) {
    let mut map = state.workspaces.write();
    if let Some(ws) = map.get_mut(&wid) {
        ws.teammate_pane_states
            .insert(pane_id, crate::state::PaneState::Idle);
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
    // H1 fail-closed：发起工作区头缺失/无效/已关闭 → 拒绝，绝不回退活动工作区。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };

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
    let _ = ctx
        .handle
        .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "pane_id": pane_id.to_string() })),
    )
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
    // H1 fail-closed：拒绝跨工作区释放（不回退活动工作区）。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };

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
    let _ = ctx
        .handle
        .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn route_find_idle_pane(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let wid = caller_workspace_id_or_active(&ctx, &headers);

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
    /// Structured launch: program + args + env, parsed from `env K=V program args`.
    #[serde(default)]
    program: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<std::collections::HashMap<String, String>>,
    /// `tmux split-window -c start-directory`
    #[serde(default)]
    cwd: Option<String>,
    /// `tmux split-window -n` / `new-window -n` 经客户端转发时的窗格名。
    #[serde(default)]
    window_name: Option<String>,
    /// 是否允许复用空闲 pane（默认 true）
    #[serde(default = "default_true")]
    allow_idle_reuse: bool,
    /// 显式自动放置契约（DA=A1）：GUI 路径置 true → 后端忽略 `pane_index`/
    /// `horizontal`，一律自动放置（idle 复用 → 最大 pane → 最长边推断方向）。
    /// 取代「`pane_index=None` 隐式编码自动」的二义性；native 路径不传 → false。
    #[serde(default)]
    auto_place: bool,
    /// F1 agent 意图位（DE=启动即 Busy）：shim 在结构化 `env … program` launch 上置 true →
    /// 后端把该面板提升为 `Busy`。裸 shell / 普通命令为 false（保持 Starting）。
    #[serde(default)]
    is_agent: bool,
    /// 可选 agent 元数据；能解析则写入 `teammate_agent_pane_map`，否则 Busy 无 id。
    #[serde(default)]
    agent_id: Option<String>,
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
    // H1 fail-closed（DB）：发起工作区头缺失/无效/已关闭 → 拒绝该 split，绝不回退
    // GUI 活动工作区（避免跨区误放）。合法 teammate 恒带头，详见 caller_workspace_id_strict。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };

    // 自动放置（DA=A1）下才尝试复用空闲 pane。显式契约：只看 `auto_place`，
    // 不再用 `pane_index.is_none()` 隐式编码意图（消除「缺省未传 vs 显式自动」二义性）。
    // 【有意行为变更，team-lead 已裁决】new-window GUI 回退走 `auto_place=false` →
    // 不再复用空闲 pane，一律在最大 pane 上新建（new-window 语义本就是「新建」）。
    if body.allow_idle_reuse && body.auto_place {
        if let Some(idle_idx) = find_idle_pane_index(&ctx.state, wid) {
            let idle_pane_id = {
                let map = ctx.state.workspaces.read();
                map.get(&wid)
                    .and_then(|ws| ws.pane_tree.get_all_leaves().get(idle_idx).copied())
            };
            if let Some(pane_id) = idle_pane_id {
                // BLOCK① 裁决：agent 复用**必须**走结构化 spawn（独立 PTY，退出即 EOF→Idle）。
                // 仅带 command 字符串、无结构化 program 的 agent 意图 → 拒绝 + 记 metric，
                // 不静默写进既有 shell（无 EOF 陷阱、会卡 Busy）。F4 看门狗（P2）兜底。
                if body.is_agent && body.program.is_none() {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        *ws.teammate_metrics
                            .failures
                            .entry("agent_reuse_requires_structured".into())
                            .or_insert(0) += 1;
                    }
                    return (
                        StatusCode::BAD_REQUEST,
                        "agent reuse rejected: structured program/args/env required (no command-only agent spawn)",
                    )
                        .into_response();
                }
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        // F1（征用空闲 pane，核心）：agent 意图（结构化）或内嵌 program →
                        // 立即 Busy（启动即 Busy）+ 写映射。
                        // 【AC6.6 修复】非-agent 复用（裸 split 命中 idle pane）**不置 Starting**：
                        // reuse 分支只 emit 后 return、不挂看门狗，置 Starting 会**永久卡 Starting**。
                        // 无 agent 进入 = 该 idle pane 语义上仍 Idle → 保持其原状态不动。
                        // normal harness 流（裸 split→reuse→spawn-process）由 spawn-process 的
                        // F1 提升为 Busy，不受影响。
                        if body.is_agent || body.program.is_some() {
                            ws.teammate_pane_states
                                .insert(pane_id, crate::state::PaneState::Busy);
                            if let Some(aid) = body
                                .agent_id
                                .as_ref()
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                            {
                                ws.teammate_agent_pane_map.insert(aid.to_string(), pane_id);
                            }
                        }
                        ws.teammate_tmux_pane_cursor = idle_idx;
                        if let Some(name) = body
                            .window_name
                            .as_ref()
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                        {
                            ws.teammate_pane_titles.insert(pane_id, name.to_string());
                        }
                    }
                }
                let structured_cmd = body.program.as_ref().map(|prog| {
                    let mut sc = terminal::StructuredPtyCommand {
                        program: prog.clone(),
                        args: body.args.clone().unwrap_or_default(),
                        env: body.env.clone().unwrap_or_default(),
                    };
                    #[cfg(windows)]
                    {
                        sc = normalize_windows_command(&sc);
                    }
                    sc
                });
                if let Some(ref sc) = structured_cmd {
                    let spawn_cwd = body
                        .cwd
                        .as_ref()
                        .map(|s| std::path::PathBuf::from(s.trim()))
                        .filter(|p| !p.as_os_str().is_empty());
                    let _ = terminal::ensure_pane_pty_workspace(
                        &ctx.state,
                        wid,
                        pane_id,
                        None,
                        spawn_cwd.as_deref(),
                        None,
                        Some(sc.clone()),
                        Some(idle_idx),
                        None,
                        None,
                    );
                } else if let Some(cmd) = body
                    .command
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let data = format!("{cmd}\n");
                    let _ = terminal::write_pty_bytes_workspace(
                        &ctx.state,
                        wid,
                        pane_id,
                        data.as_bytes(),
                    );
                }
                let _ = ctx.handle.emit(
                    TEAMMATE_LAYOUT_CHANGED,
                    LayoutChange::reused(pane_id.to_string()),
                );
                let _ = ctx
                    .handle
                    .emit("teammate-active-pane-changed", pane_id.to_string());
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
    }

    // Target + direction selection (single source of truth):
    // 1. 显式定向（非 auto_place 且带 `-t`）→ 尊重 `pane_index`；方向按 `-h`（默认 vertical）。
    // 2. auto_place（及非定向回退）→ 复用 `choose_balanced_split`：选「面积最大叶子」并按
    //    **加权最长边**（cell 高≈2×宽）推断方向，与 `balanced_split_decision` 单测口径一致。
    //    避免出现「裸 cols>rows」与 balanced 加权两套不一致公式（H-DIR）。
    let (idx, direction, direction_inferred) = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&wid) else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "no workspace").into_response();
        };
        let leaves = ws.pane_tree.get_all_leaves();
        let pane_count = leaves.len();

        if let Some(explicit_idx) = body.pane_index.filter(|_| !body.auto_place) {
            if explicit_idx >= pane_count {
                return (StatusCode::BAD_REQUEST, "pane_index out of range").into_response();
            }
            let dir = if body.horizontal {
                "horizontal"
            } else {
                "vertical"
            };
            (explicit_idx, dir, false)
        } else {
            match pane::choose_balanced_split(ws) {
                Some((uuid, sdir)) => {
                    let idx = leaves.iter().position(|p| *p == uuid).unwrap_or(0);
                    let dir = match sdir {
                        SplitDirection::Horizontal => "horizontal",
                        SplitDirection::Vertical => "vertical",
                    };
                    (idx, dir, true)
                }
                None => (0, "vertical", true),
            }
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
                    return (StatusCode::INTERNAL_SERVER_ERROR, "workspace missing")
                        .into_response();
                };
                ws.pane_tree
                    .get_all_leaves()
                    .iter()
                    .position(|u| *u == new_id)
                    .unwrap_or(0)
            };
            let cmd = body
                .command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let structured_cmd = body.program.as_ref().map(|prog| {
                let mut sc = terminal::StructuredPtyCommand {
                    program: prog.clone(),
                    args: body.args.clone().unwrap_or_default(),
                    env: body.env.clone().unwrap_or_default(),
                };
                #[cfg(windows)]
                {
                    sc = normalize_windows_command(&sc);
                }
                sc
            });

            let is_structured = structured_cmd.is_some();
            let initial_cmd = if is_structured { None } else { cmd };

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
                initial_cmd,
                structured_cmd,
                Some(new_idx),
                Some(ready_tx),
                Some(trace_id.clone()),
            ) {
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        let _ = ws.pane_tree.close(new_id);
                        ws.pane_sizes.remove(&new_id);
                        *ws.teammate_metrics
                            .failures
                            .entry("phase1_failed".into())
                            .or_insert(0) += 1;
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
                    // F1（新 split 入口）：仅当**确有 agent 落入**（结构化 program / is_agent）
                    // 才标 Busy（显示 agent badge）。裸 split（无 agent，纯 tmux 拉起的 shell
                    // pane）**不写任何 teammate 状态** → 不打 agent 标（与普通用户 pane 同款）。
                    // harness 主路径 split→spawn-process(is_agent) 由 spawn-process 适时标 Busy。
                    // （用户需求 2026-06-11：tmux 拉起但未运行 agent 的 pane 不要 agent 标。）
                    if body.is_agent || is_structured {
                        ws.teammate_pane_states
                            .insert(new_id, PaneState::Busy);
                    }
                    if let Some(aid) = body
                        .agent_id
                        .as_ref()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                    {
                        ws.teammate_agent_pane_map.insert(aid.to_string(), new_id);
                    }
                    ws.pane_sizes.insert(new_id, (80, 120));
                    if let Some(name) = body
                        .window_name
                        .as_ref()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                    {
                        ws.teammate_pane_titles.insert(new_id, name.to_string());
                    }
                }
            }
            let _ = ctx.handle.emit(
                TEAMMATE_LAYOUT_CHANGED,
                LayoutChange::split(trace_id.clone()),
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
            // Carry the originating split's trace id so the watchdog-drained
            // `removed` event correlates with the split that created the pane
            // (L3 — cross-stack log/diagnostics).
            let watch_trace = trace_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let cleaned = {
                    let mut map = watch_state.workspaces.write();
                    if let Some(ws) = map.get_mut(&watch_wid) {
                        if ws.pending_spawns.remove(&watch_pid).is_some() {
                            let _ = ws.pane_tree.close(watch_pid);
                            ws.pane_sizes.remove(&watch_pid);
                            *ws.teammate_metrics
                                .failures
                                .entry("watchdog_30s".into())
                                .or_insert(0) += 1;
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
                        TEAMMATE_LAYOUT_CHANGED,
                        LayoutChange::removed_with_trace(watch_pid.to_string(), watch_trace),
                    );
                }
            });

            // 裸 split 不再标 Starting、不挂 agent 看门狗（见上 F1 入口，用户需求 2026-06-11）：
            // 无 agent 的纯 tmux shell pane 不打 agent 标；真 agent 经 spawn-process(is_agent)
            // 在落入时自行标 Busy。

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
                            "direction_inferred": direction_inferred,
                            "trace_id": trace_id,
                        })),
                    )
                        .into_response()
                }
                Ok(Ok(Err(e))) => {
                    {
                        let mut map = ctx.state.workspaces.write();
                        if let Some(ws) = map.get_mut(&wid) {
                            *ws.teammate_metrics
                                .failures
                                .entry("activate_failed".into())
                                .or_insert(0) += 1;
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
                            *ws.teammate_metrics
                                .failures
                                .entry("activate_timeout_3s".into())
                                .or_insert(0) += 1;
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
    let pane: usize = q.get("pane").and_then(|s| s.parse().ok()).unwrap_or(0);
    let lines: usize = q.get("lines").and_then(|s| s.parse().ok()).unwrap_or(80);
    let wid = caller_workspace_id_or_active(&ctx, &headers);
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
    // T5 MEDIUM（隔离完整性）：send-keys 是**注入**路由（写错 pane PTY 会真伤害）→
    // fail-closed，不回退 active_workspace_id。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };
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
    /// F1 agent 意图位：spawn-process 恒由结构化 launch 触发 → shim 置 true。
    #[serde(default)]
    is_agent: bool,
    /// 可选 agent 元数据；能解析则写入 `teammate_agent_pane_map`。
    #[serde(default)]
    agent_id: Option<String>,
}

async fn route_spawn_process(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Json(body): Json<SpawnProcessBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    // H1 fail-closed：spawn-process 是真实 agent 落点 → 发起工作区头缺失/无效/已关闭
    // 即拒绝，绝不回退活动工作区（避免把 agent 起到错误工作区）。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };
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
    // §host-guard (2026-06-11): spawn-process tears down + replaces the target
    // pane's PTY (ensure_pane_pty_workspace). If pane_idx (default 0 / stale
    // cursor) resolves to the originating host pane, that replacement kills the
    // parent agent. Only teammate-owned panes are valid spawn targets.
    {
        let owned = ctx
            .state
            .workspaces
            .read()
            .get(&wid)
            .is_some_and(|ws| ws.teammate_owned_panes.contains(&pid));
        if !owned {
            return (
                StatusCode::BAD_REQUEST,
                "refusing to spawn onto a non-teammate pane",
            )
                .into_response();
        }
    }
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
            // F1（harness 主路径核心）：send-keys 结构化 agent 落入既有 Starting 面板 →
            // 立即提升为 Busy（启动即 Busy）。有 agent_id 则写映射，供 badge/退出清理。
            if body.is_agent {
                ws.teammate_pane_states.insert(pid, PaneState::Busy);
                if let Some(aid) = body
                    .agent_id
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    ws.teammate_agent_pane_map.insert(aid.to_string(), pid);
                }
            }
        }
    }
    // 提升后 emit，让前端 re-sync 渲染 AGENT badge（T0 封套 generic state）。
    if body.is_agent {
        let _ = ctx
            .handle
            .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
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

async fn route_list_panes(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let want_json = q.get("json").map(|s| s == "1").unwrap_or(false);
    let wid = caller_workspace_id_or_active(&ctx, &headers);

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
            pane_count: if leaves.is_empty() { 1 } else { pane_count },
            panes: leaves
                .iter()
                .enumerate()
                .map(|(i, u)| PaneRowJson {
                    index: i,
                    pane_id: format!("%{i}"),
                    uuid: u.to_string(),
                    title: ws.teammate_pane_titles.get(u).cloned(),
                    cwd: ws
                        .pane_tree
                        .panes
                        .get(u)
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
    let wid = caller_workspace_id_or_active(&ctx, &headers);

    log_stderr_server(&format!(
        "select-pane: index={:?}, last={:?}",
        body.pane_index, body.last
    ));

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
            let _ = ctx
                .handle
                .emit("teammate-active-pane-changed", pid.to_string());
        }

        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "pane_index": new_cursor
            })),
        )
            .into_response();
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

        (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "pane_index": idx
            })),
        )
            .into_response()
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
    // T5 MEDIUM（隔离完整性）：kill-pane 是**破坏性**路由（删错 pane）→ fail-closed。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };

    if let Some(idx) = body.pane_index {
        match pane::teammate_pane_uuid_at_index(&ctx.state, wid, idx) {
            Ok(pid) => {
                // §host-guard (2026-06-11): refuse to kill any pane Ridge did not
                // create for a teammate. claude-sdk emits `kill-pane -t %0` during
                // teammate teardown; if %0 resolves to the originating (host) pane,
                // an unguarded kill destroys the parent session. Non-teammate panes
                // → silent OK no-op so the teardown flow stays happy.
                let is_owned = ctx
                    .state
                    .workspaces
                    .read()
                    .get(&wid)
                    .is_some_and(|ws| ws.teammate_owned_panes.contains(&pid));
                if !is_owned {
                    return (StatusCode::OK, "ignored: not a teammate pane").into_response();
                }
                let state_ref: &AppState = &ctx.state;
                crate::commands::terminal::kill_pty_if_present(state_ref, wid, pid, true).await;
                {
                    let mut map = ctx.state.workspaces.write();
                    if let Some(ws) = map.get_mut(&wid) {
                        // Mirror the cleanup done by close_pane so a teammate-
                        // initiated kill doesn't leave an orphaned agent_state.
                        ws.teammate_pane_titles.remove(&pid);
                        ws.teammate_pane_states.remove(&pid);
                        ws.teammate_agent_pane_map.retain(|_, v| *v != pid);
                        ws.pane_sizes.remove(&pid);
                        ws.teammate_owned_panes.remove(&pid);
                        let _ = ws.pane_tree.close(pid);
                    }
                }
                let _ = ctx.handle.emit(
                    TEAMMATE_LAYOUT_CHANGED,
                    LayoutChange::removed(pid.to_string()),
                );
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

    // H1 fail-closed：new-window 也是放置路由 → 发起工作区头缺失/无效/已关闭即拒绝。
    let wid = match caller_workspace_id_strict(&ctx, &headers) {
        Ok(w) => w,
        Err(r) => return workspace_reject_response(&ctx, r),
    };
    let cwd = body
        .cwd
        .as_ref()
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty());
    let cmd = body
        .command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match pane::teammate_split_pane(&ctx.state, wid, 0, "vertical") {
        Ok(new_id) => {
            let new_idx = {
                let map = ctx.state.workspaces.read();
                let Some(ws) = map.get(&wid) else {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "workspace missing")
                        .into_response();
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
                        ws.teammate_pane_titles.insert(new_id, name.to_string());
                    }
                }
            }
            let _ = ctx
                .handle
                .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
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
    let wid = caller_workspace_id_or_active(&ctx, &headers);
    let name = body.name.trim().to_string();

    let target_idx = body.pane_index.unwrap_or_else(|| {
        ctx.state
            .workspaces
            .read()
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
            let _ = ctx
                .handle
                .emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
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
    let wid = caller_workspace_id_or_active(&ctx, &headers);

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
            .or_else(|| {
                leaves
                    .iter()
                    .find_map(|u| ws.teammate_pane_titles.get(u).cloned())
            })
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
    let cleaned = from_user
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(s) = cleaned {
        return s.to_string();
    }
    let compact: String = id.to_string().replace('-', "");
    let n = compact.len().min(8);
    format!("ws{}", &compact[..n])
}

async fn route_list_sessions(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
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
        let created_local: DateTime<Local> =
            DateTime::<Utc>::from(ws.created_at).with_timezone(&Local);
        let date_str = created_local.format("%a %b %d %H:%M:%S %Y").to_string();
        // Ridge 每个工作区对应 tmux 的一个 session、一个 window（多 pane 为分屏）。
        let mut line = format!("{label}: 1 windows (created {date_str}) [{cols}x{rows}]");
        if *wid == active {
            line.push_str(" (attached)");
        }
        lines.push(line);
    }

    (StatusCode::OK, lines.join("\n")).into_response()
}

async fn route_list_clients(
    State(ctx): State<TeammateCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    (StatusCode::OK, "").into_response()
}

fn log_stderr_server(msg: &str) {
    eprintln!("[ridge-teammate] {}", msg);
}

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

/// 把一个 native 会话「召唤」进工作区 `wid`：为其活动窗口各面板建一个**领养 GUI
/// pane**（共享 native PTY，不新开 shell），走与普通 pane 完全一致的渲染/输入/
/// resize 路径。关闭=detach（见 `terminal::kill_pty_if_present`）。返回展示的面板数。
///
/// 接受 `AppState` + `AppHandle` 而非 `TeammateCtx`，以允许 Tauri 命令直接复用。
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
        if sp.prev_attachment.map(|(w, _)| w) == Some(wid) {
            continue;
        }
        // 与 route_split 同一来源：`choose_balanced_split`（最大面积叶子 + 加权最长边
        // 方向），统一 tie-break，消除 summon 残留的「裸 cols>rows」公式（H-DIR#2 / M1）。
        let (idx, direction) = {
            let map = state.workspaces.read();
            map.get(&wid)
                .and_then(|ws| {
                    let (uuid, sdir) = pane::choose_balanced_split(ws)?;
                    let leaves = ws.pane_tree.get_all_leaves();
                    let idx = leaves.iter().position(|p| *p == uuid).unwrap_or(0);
                    let dir = match sdir {
                        SplitDirection::Horizontal => "horizontal",
                        SplitDirection::Vertical => "vertical",
                    };
                    Some((idx, dir))
                })
                .unwrap_or((0, "horizontal"))
        };
        let new_id = match pane::teammate_split_pane(state, wid, idx, direction) {
            Ok(id) => id,
            Err(_) => continue,
        };
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let parser = Arc::new(parking_lot::Mutex::new(PaneParser::new(
            sp.height.max(1),
            sp.width.max(1),
            2000,
        )));
        let handle = PtyHandle {
            master: sp.master,
            writer: sp.writer,
            _child: None,
            native_ref: Some((socket.to_string(), sp.global_id)),
            native_cancel: Some(cancel.clone()),
            resize_silence_deadline: Arc::new(std::sync::atomic::AtomicI64::new(0)),
            parser,
            delta_mode: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        {
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                ws.terminals.insert(new_id, handle);
                ws.pane_sizes
                    .insert(new_id, (sp.height.max(1), sp.width.max(1)));
                ws.teammate_pane_titles
                    .insert(new_id, format!("%{}", sp.global_id));
                if let Some(ref dir) = sp.cwd {
                    if let Some(p) = ws.pane_tree.panes.get_mut(&new_id) {
                        p.cwd = Some(dir.clone().into());
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

#[cfg(test)]
mod workspace_header_tests {
    //! T4 (H1 fail-closed): the missing/invalid-vs-valid classification of the
    //! `X-Ridge-Workspace` header is pure (no ctx/state), so it is unit-testable
    //! here. The "workspace exists vs closed" branch lives in
    //! `caller_workspace_id_strict` and needs a live workspace map → covered by
    //! integration / end-to-end (AC4.1/4.3).
    use super::{parse_workspace_header, WorkspaceReject};
    use axum::http::{HeaderMap, HeaderValue};

    fn headers_with(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-ridge-workspace", HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn missing_header_is_rejected() {
        let h = HeaderMap::new();
        assert!(matches!(
            parse_workspace_header(&h),
            Err(WorkspaceReject::MissingOrInvalidHeader)
        ));
    }

    #[test]
    fn empty_or_whitespace_header_is_rejected() {
        assert!(matches!(
            parse_workspace_header(&headers_with("   ")),
            Err(WorkspaceReject::MissingOrInvalidHeader)
        ));
    }

    #[test]
    fn non_uuid_header_is_rejected() {
        assert!(matches!(
            parse_workspace_header(&headers_with("not-a-uuid")),
            Err(WorkspaceReject::MissingOrInvalidHeader)
        ));
    }

    #[test]
    fn valid_uuid_header_parses() {
        let id = uuid::Uuid::new_v4();
        let parsed = parse_workspace_header(&headers_with(&id.to_string())).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let id = uuid::Uuid::new_v4();
        let parsed = parse_workspace_header(&headers_with(&format!("  {id}  "))).unwrap();
        assert_eq!(parsed, id);
    }
}
