//! 桌面进程与共享远控层之间的 **Tauri 胶水层**。
//!
//! 三部分——全部依赖 `tauri::AppHandle` / `AppState`，故不能进零 Tauri 依赖的
//! `ridge-remote`，改集中放这里（原 `src-tauri/src/remote/{mod.rs,core_bridge.rs,
//! server.rs}` 于 P1 阶段5 删除后归并至此）：
//!
//!   1. [`forward_event`] —— 把 Tauri 事件转发给桌面浏览器远控客户端。
//!   2. `ridge-core` 桥（[`desktop_ctx`] / [`remote_ctx`] / [`DesktopEventSink`]）
//!      —— 为运行时无关的 `ridge_core` 供给 State / Events / Spawner 三个 Ctx 面。
//! LAN listener ownership moved to ridge-kernel.

use std::path::PathBuf;
use std::sync::Arc;

use ridge_core::commands::settings::{HostStateAccessor, UserDefaultCwdStore};
use ridge_core::{CapabilitySet, ConnectionId, Ctx, EventScope, EventSink, TokioSpawner};
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
        // A1 同源化：与桌面 IPC 命令共用 commands::workspace::list_workspaces_entries。
        crate::commands::workspace::list_workspaces_entries(self)
    }

    fn pane_layout(&self, workspace_id: &str) -> Result<Value, String> {
        // 纯读：复用桌面 get_pane_layout_for_inner 构造 LayoutNode，序列化为 JSON。
        let node = crate::commands::pane::get_pane_layout_for_inner(self, workspace_id)?;
        serde_json::to_value(node).map_err(|e| e.to_string())
    }

    fn pane_scrollback_tail(&self, pane_id: &str, max_bytes: usize) -> Result<Value, String> {
        // 与桌面 get_pane_scrollback_tail 逐字一致：pane_id→活动区→取已存 scrollback 尾块。
        let pid = uuid::Uuid::parse_str(pane_id).map_err(|e| e.to_string())?;
        let wid = self.active_workspace_id();
        let chunk = self.get_pty_scrollback_tail(wid, pid, max_bytes);
        serde_json::to_value(chunk).map_err(|e| e.to_string())
    }

    fn pane_scrollback_before(
        &self,
        pane_id: &str,
        before_seq: u64,
        max_bytes: usize,
    ) -> Result<Value, String> {
        let pid = uuid::Uuid::parse_str(pane_id).map_err(|e| e.to_string())?;
        let wid = self.active_workspace_id();
        let chunk = self.get_pty_scrollback_before(wid, pid, before_seq, max_bytes);
        serde_json::to_value(chunk).map_err(|e| e.to_string())
    }

    fn native_sessions(&self) -> Value {
        // 全局纯读：复用桌面 list_native_sessions（native::list_all_sessions），序列化。
        serde_json::to_value(crate::commands::terminal::list_native_sessions())
            .unwrap_or_else(|_| Value::Array(Vec::new()))
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
        // A1 同源化：委托唯一实现（改名 + 落盘 + 三广播）。
        let id = uuid::Uuid::parse_str(workspace_id).map_err(|e| e.to_string())?;
        crate::commands::workspace::rename_workspace_core(self, id, name.to_string())
    }

    fn create_workspace(&self, name: Option<&str>) -> Result<String, String> {
        // 复用桌面命令核心（DRY：Workspace 字面量只在 create_workspace_core 一处）。
        Ok(crate::commands::workspace::create_workspace_core(
            self, name,
        ))
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<(), String> {
        // A1 同源化：委托唯一实现（terminals 随 Workspace drop 清理 PTY）。
        let id = uuid::Uuid::parse_str(workspace_id).map_err(|e| e.to_string())?;
        crate::commands::workspace::close_workspace_core(self, id)
    }

    fn save_workspace(&self, name: Option<&str>) -> Result<String, String> {
        // 经进程级 AppHandle 解析 app 数据目录做历史文件 IO；复用桌面命令核心（DRY）。
        let app = self
            .app_handle
            .get()
            .ok_or_else(|| "app handle 未就绪".to_string())?;
        crate::commands::workspace::save_workspace_core(app, self, name)
    }

    fn save_workspace_to_file(
        &self,
        workspace_id: &str,
        name: &str,
        path: Option<&str>,
    ) -> Result<String, String> {
        let app = self
            .app_handle
            .get()
            .ok_or_else(|| "app handle 未就绪".to_string())?;
        crate::commands::ridge_file::save_workspace_to_file_core(
            app,
            self,
            workspace_id,
            name,
            path,
        )
    }

    fn delete_workspace_file(&self, workspace_id: &str) -> Result<(), String> {
        let app = self
            .app_handle
            .get()
            .ok_or_else(|| "app handle 未就绪".to_string())?;
        crate::commands::ridge_file::delete_workspace_file_core(app, self, workspace_id)
    }

    fn resize_pane(
        &self,
        workspace_id: &str,
        pane_id: &str,
        rows: u16,
        cols: u16,
        is_alt: bool,
        is_inline_tui: bool,
    ) -> Result<(), String> {
        // resize 既有 PTY：复用桌面 resize_pane_inner（逻辑一字未改），经 app_handle 供事件。
        let app = self
            .app_handle
            .get()
            .ok_or_else(|| "app handle 未就绪".to_string())?;
        crate::commands::terminal::resize_pane_inner(
            self,
            app,
            workspace_id.to_string(),
            pane_id.to_string(),
            crate::commands::terminal::ResizeRequest {
                rows,
                cols,
                is_alt,
                is_inline_tui,
                suppress_errors: false,
                owner: crate::types::PaneResizeOwner::Remote,
            },
        )
        .map_err(|e| e.to_string())
    }

    fn create_pane(&self, pane_id: &str, shell: Option<&str>) -> Result<(), String> {
        // spawn 既有 pane 的 shell PTY：复用桌面 create_pane_inner（逻辑一字未改）。
        crate::commands::terminal::create_pane_inner(
            self,
            pane_id.to_string(),
            shell.map(String::from),
        )
        .map_err(|e| e.to_string())
    }

    fn split_pane(&self, pane_id: &str, direction: &str) -> Result<Value, String> {
        // 分裂 pane（改树 + 起新 PTY）：复用桌面 split_pane_inner（逻辑一字未改）+ 序列化结果。
        let res = crate::commands::pane::split_pane_inner(
            self,
            pane_id.to_string(),
            direction.to_string(),
        )
        .map_err(|e| e.to_string())?;
        serde_json::to_value(res).map_err(|e| e.to_string())
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
    Ctx::new(
        accessor,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::remote_default(),
    )
    .with_connection(connection_id)
}
