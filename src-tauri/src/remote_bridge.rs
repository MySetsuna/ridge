//! 桌面进程与共享远控层之间的 **Tauri 胶水层**。
//!
//! 三部分——全部依赖 `tauri::AppHandle` / `AppState`，故不能进零 Tauri 依赖的
//! `ridge-remote`，改集中放这里（原 `src-tauri/src/remote/{mod.rs,core_bridge.rs,
//! server.rs}` 于 P1 阶段5 删除后归并至此）：
//!
//!   1. [`forward_event`] —— 把 Tauri 事件转发给桌面浏览器远控客户端。
//!   2. `ridge-core` 桥（[`desktop_ctx`] / [`remote_ctx`] / [`DesktopEventSink`]）
//!      —— 为运行时无关的 `ridge_core` 供给 State / Events / Spawner 三个 Ctx 面。
//!   3. [`spawn_remote_server`] —— 桌面 LAN 远控 HTTP/WS 服务器的启动壳：后台线程
//!      + tokio runtime + `bind_tcp(9527)` + 多网卡 TLS fail-closed，然后构造
//!      `Arc<dyn RemoteHost>`（`DesktopHost`）交给 `ridge_remote::server_app::run`。

use std::path::PathBuf;
use std::sync::Arc;

use ridge_core::commands::settings::{HostStateAccessor, UserDefaultCwdStore};
use ridge_core::{CapabilitySet, ConnectionId, Ctx, EventScope, EventSink, TokioSpawner};
use ridge_remote::auth::RemoteAuth;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use crate::types::RemoteUiEvent;

// ════════════════════════════════════════════════════════════════════════════
// 1. Tauri 事件 → 桌面浏览器远控客户端 转发
// ════════════════════════════════════════════════════════════════════════════

/// Forward a Tauri event to all connected desktop-browser remote clients (the
/// "desktop UI in a browser" mode), so the browser's `listen()` shim dispatches
/// it exactly like a native event. Add a call next to any `app.emit(...)` whose
/// event the desktop UI subscribes to. No-op when AppState isn't managed.
pub fn forward_event<S: serde::Serialize>(app: &tauri::AppHandle, name: &str, payload: S) {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    let _ = state.remote_ui_event_tx.send(crate::types::RemoteUiEvent {
        name: name.to_string(),
        payload: value,
    });
}

// ════════════════════════════════════════════════════════════════════════════
// 2. ridge-core 桥（原 remote/core_bridge.rs）
// ════════════════════════════════════════════════════════════════════════════

/// `AppState` exposes the one field `set_user_default_cwd` needs.
impl UserDefaultCwdStore for AppState {
    fn set_user_default_cwd(&self, path: Option<PathBuf>) {
        *self.user_default_cwd.write() = path;
    }
}

/// R0 内核化样板 B：桌面 `AppState` 作为 `WorkspaceReader` 端口，向 `ridge_core` 供给
/// 某工作区的**原始**快照数据（pane 树 JSON + 各 pane cwd + pane 标题）。加工（git 仓库根
/// 去重）归 core 的 `build_workspace_snapshot`。逻辑与 `commands/ridge_file.rs::
/// snapshot_workspace` 一致（此处只取原料、不落盘）。连同 `UserDefaultCwdStore`，`AppState`
/// 经 blanket impl 自动成为 `ridge_core` 的聚合 `HostState`（`desktop_ctx`/`remote_ctx` 注册）。
impl ridge_core::commands::workspace::WorkspaceReader for AppState {
    fn workspace_raw(
        &self,
        workspace_id: &str,
    ) -> Option<ridge_core::commands::workspace::WorkspaceRaw> {
        let wid = uuid::Uuid::parse_str(workspace_id).ok()?;
        let map = self.workspaces.read();
        let ws = map.get(&wid)?;
        let pane_tree = serde_json::to_value(&ws.pane_tree).ok()?;
        let pane_cwds = ws
            .pane_tree
            .panes
            .values()
            .filter_map(|p| p.cwd.as_ref())
            .map(|c| c.to_string_lossy().to_string())
            .collect();
        let pane_titles = ws
            .teammate_pane_titles
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        Some(ridge_core::commands::workspace::WorkspaceRaw {
            pane_tree,
            pane_cwds,
            pane_titles,
        })
    }

    fn active_workspace(&self) -> String {
        self.active_workspace_id().to_string()
    }

    fn workspaces_list(&self) -> Vec<ridge_core::commands::workspace::WorkspaceEntry> {
        // 与 commands::workspace::list_workspaces 逐字一致（order + names + display_seq）。
        let order = self.workspace_order.read();
        let names = self.workspace_names.read();
        let map = self.workspaces.read();
        order
            .iter()
            .enumerate()
            .map(|(i, id)| ridge_core::commands::workspace::WorkspaceEntry {
                id: id.to_string(),
                index: i,
                name: names.get(id).cloned(),
                display_seq: map.get(id).map(|w| w.display_seq).unwrap_or(0),
            })
            .collect()
    }
}

/// R0 内核化：桌面 `AppState` 作为 `WorkspaceWriter` 端口。逻辑与
/// `commands::workspace::switch_workspace` 逐字一致（解析 uuid → 校验存在 → 置活动）；
/// 不触 PTY。远端 controller 的 `switch_workspace` 经 dispatch 落到此。
impl ridge_core::commands::workspace::WorkspaceWriter for AppState {
    fn set_active_workspace(&self, workspace_id: &str) -> Result<(), String> {
        let id = uuid::Uuid::parse_str(workspace_id).map_err(|e| e.to_string())?;
        if !self.workspaces.read().contains_key(&id) {
            return Err("工作区不存在".into());
        }
        *self.active_workspace.write() = id;
        Ok(())
    }

    fn reorder_workspaces(&self, from_index: usize, to_index: usize) -> Result<(), String> {
        // 与 commands::workspace::reorder_workspaces 逐字一致（越界拒 → remove/insert）。
        let mut order = self.workspace_order.write();
        if from_index >= order.len() || to_index >= order.len() {
            return Err("无效的索引".into());
        }
        let item = order.remove(from_index);
        order.insert(to_index, item);
        Ok(())
    }

    fn rename_workspace(&self, workspace_id: &str, name: &str) -> Result<(), String> {
        // 与 commands::workspace::rename_workspace 逐字一致：改名 + 落盘 + 广播。
        let id = uuid::Uuid::parse_str(workspace_id).map_err(|e| e.to_string())?;
        self.workspace_names.write().insert(id, name.to_string());
        // 立刻反映到 .ridge 文件（未保存工作区为 no-op）。
        crate::commands::ridge_file::schedule_auto_save(self, id);
        let display_name = self
            .workspace_names
            .read()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let _ = self.remote_structural_tx.send(
            crate::types::RemoteStructuralEvent::WorkspaceRenamed {
                workspace_id: id,
                name: display_name,
            },
        );
        let _ = self
            .remote_structural_tx
            .send(crate::types::RemoteStructuralEvent::WorkspacesChanged);
        let _ = self
            .event_tx
            .try_send(crate::types::GlobalEvent::WorkspaceListChanged);
        Ok(())
    }
}

/// Event sink that mirrors `ridge_core` emits onto the desktop's event
/// surfaces. `Broadcast` events go to both the native WebView (`AppHandle::
/// emit`) and the desktop-browser remote clients (`remote_ui_event_tx`).
/// `Connection`-scoped events are addressed to a single browser connection;
/// for the in-process desktop path there is one implicit connection, so they
/// also go through `AppHandle::emit`. (No vertical-slice handler emits yet —
/// this is the seam later slices will use.)
pub struct DesktopEventSink {
    app: AppHandle,
    ui_event_tx: tokio::sync::broadcast::Sender<RemoteUiEvent>,
}

impl DesktopEventSink {
    pub fn new(app: AppHandle, state: &AppState) -> Self {
        Self {
            app,
            ui_event_tx: state.remote_ui_event_tx.clone(),
        }
    }
}

impl EventSink for DesktopEventSink {
    fn emit(&self, scope: EventScope, _connection: &ConnectionId, name: &str, payload: Value) {
        // Native WebView listeners always get the event.
        let _ = self.app.emit(name, payload.clone());
        // Broadcast events additionally fan out to desktop-browser clients.
        // (Per-connection routing for the browser path is refined in S3/S4
        // once the transport carries a connection id end-to-end.)
        if scope == EventScope::Broadcast {
            let _ = self.ui_event_tx.send(RemoteUiEvent {
                name: name.to_string(),
                payload,
            });
        }
    }
}

/// Build a `ridge_core::Ctx` for the **in-process desktop IPC** path. State is
/// the user-default-cwd accessor over `AppState`; capabilities are `allow_all`
/// because Tauri command registration already gates admission here.
pub fn desktop_ctx(app: &AppHandle, state: &AppState) -> Ctx {
    let accessor: Arc<dyn ridge_core::CoreState> =
        Arc::new(HostStateAccessor(Arc::new(state.clone())));
    let events: Arc<dyn EventSink> = Arc::new(DesktopEventSink::new(app.clone(), state));
    Ctx::new(
        accessor,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::allow_all(),
    )
}

/// Build a `ridge_core::Ctx` for the **browser-facing remote** path, carrying
/// the canonical remote allow-list (D8) and the originating `connection_id`.
/// Used by the remote server's invoke dispatcher as handlers migrate in.
pub fn remote_ctx(app: &AppHandle, state: &AppState, connection_id: impl Into<String>) -> Ctx {
    let accessor: Arc<dyn ridge_core::CoreState> =
        Arc::new(HostStateAccessor(Arc::new(state.clone())));
    let events: Arc<dyn EventSink> = Arc::new(DesktopEventSink::new(app.clone(), state));
    // Mirror the session's read-only flag into the capability set so the
    // `ridge_core::dispatch` read-only gate (D-GM-9) is authoritative for the
    // browser-facing path too. `is_mutating_invoke` keeps its own pre-check as
    // a backstop during the migration window (same rejection + message), so
    // this is belt-and-suspenders with zero behaviour change.
    let readonly = state
        .remote_fs_readonly
        .load(std::sync::atomic::Ordering::Relaxed);
    Ctx::new(
        accessor,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::remote_default().with_readonly(readonly),
    )
    .with_connection(connection_id)
}

// ════════════════════════════════════════════════════════════════════════════
// 3. 桌面 LAN 远控服务器启动壳（原 remote/server.rs）
//
// 路由装配 / verify / ws 握手 / workspace / file / session 与整段每连接 WS
// 会话（`handle_ws` + dispatcher）已分别下沉到共享层 `ridge_remote::server_app`
// 与桌面侧 `crate::remote_host_impl`（`DesktopHost` 实现 `RemoteHost`）。此处只保
// 留：后台线程 + tokio runtime、`bind_tcp(9527)`、多网卡 TLS fail-closed 决策，
// 然后构造 `Arc<dyn RemoteHost>` 交给 `server_app::run`。
// ════════════════════════════════════════════════════════════════════════════

/// Handle returned by `spawn_remote_server` — the caller receives the
/// allocated port and the background thread join handle.
pub struct ServerHandle {
    pub port: u16,
    pub thread: std::thread::JoinHandle<()>,
}

/// Spawn the remote-control WebSocket server on a background thread.
/// Binds `0.0.0.0:9527` (probing up to 10 higher ports).
///
/// Accepts a `shutdown_rx` one-shot receiver: when a value is sent on the
/// corresponding sender the server performs an orderly graceful shutdown
/// (drain in-flight requests, close listeners).
///
/// Returns `None` if binding failed.
pub fn spawn_remote_server(
    state: AppState,
    auth: Arc<RemoteAuth>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Option<ServerHandle> {
    let lan_ip = ridge_remote::net::detect_lan_ip();

    let (port_tx, port_rx) = std::sync::mpsc::channel();

    let thread = std::thread::Builder::new()
        .name("ridge-remote-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(target: "ridge::remote", error = %e, "tokio runtime build failed");
                    let _ = port_tx.send(None);
                    return;
                }
            };
            rt.block_on(run_remote_server(state, auth, lan_ip, port_tx, shutdown_rx));
        })
        .expect("ridge-remote-http thread spawn");

    let port = port_rx.recv().ok().flatten()?;
    Some(ServerHandle { port, thread })
}

async fn run_remote_server(
    state: AppState,
    auth: Arc<RemoteAuth>,
    lan_ip: String,
    port_tx: std::sync::mpsc::Sender<Option<u16>>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // Use the shared port binding from ridge-remote (probe up to 10 higher ports).
    let (std_listener, port) = match ridge_remote::server::bind_tcp(9527) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote server bind failed");
            let _ = port_tx.send(None);
            return;
        }
    };
    tracing::info!(target: "ridge::remote", port, lan_ip = %lan_ip, "Remote control server listening");

    // Resolve the static UI dirs (mobile `static/remote` + optional desktop
    // `web-remote-dist`). 运行时候选目录探测已下沉到 `ridge_remote::serve`。
    let serve_cfg = ridge_remote::serve::UaServeConfig::resolve_ui_dirs();

    let machine_name = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());

    // SECURITY (audit H1): resolve TLS BEFORE announcing the port so a TLS
    // failure can be reported back to the caller as a start failure (fail-closed)
    // rather than the server silently coming up on plain HTTP. The remote server
    // exposes full shell + filesystem control; serving it over cleartext on a
    // hostile LAN would leak the 6-digit code and session token to any sniffer,
    // who could then replay them. We therefore REFUSE to start when TLS material
    // can't be produced — unless the operator explicitly opts into insecure HTTP
    // via `RIDGE_REMOTE_ALLOW_INSECURE_HTTP=1` (loud, never silent/automatic).
    // Cover EVERY reachable LAN address in the cert (not just the auto-chosen
    // primary), so the remote panel's IP picker can advertise any of them — Wi-Fi,
    // Tailscale, Ethernet — and the phone still gets a matching cert.
    let lan_ips = ridge_remote::net::detect_lan_ips();
    let tls_config = ridge_remote::tls::resolve_config_multi(&lan_ips, &machine_name).await;
    let allow_insecure = std::env::var("RIDGE_REMOTE_ALLOW_INSECURE_HTTP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if tls_config.is_none() && !allow_insecure {
        tracing::error!(
            target: "ridge::remote",
            "Remote TLS unavailable — REFUSING to start the remote server on plain HTTP \
             (would expose shell/file control + auth code over cleartext on the LAN). \
             Set RIDGE_REMOTE_ALLOW_INSECURE_HTTP=1 to explicitly allow insecure HTTP."
        );
        let _ = port_tx.send(None);
        return;
    }
    let tls_enabled = tls_config.is_some();

    // 构造桌面宿主实现（包装 AppState），交给共享 `server_app` 装配路由 + serve。
    let host: Arc<dyn ridge_remote::host::RemoteHost> =
        Arc::new(crate::remote_host_impl::DesktopHost {
            state,
            auth,
            port,
            lan_ip,
            machine_name,
            serve_cfg,
            tls_enabled,
        });

    let _ = port_tx.send(Some(port));
    // §sessions: serve_on captures each client's real peer IP via ConnectInfo (for
    // the session list + blacklist). TLS/bind fail-closed already decided above.
    if let Err(e) =
        ridge_remote::server_app::run(host, std_listener, tls_config, shutdown_rx, true).await
    {
        tracing::error!(target: "ridge::remote", error = %e, "remote server stopped");
    }
}
