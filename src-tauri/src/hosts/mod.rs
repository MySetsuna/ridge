//! 外部主机注册表（「主机 / Hosts」面板的远端 ridge / rdg host）。
//!
//! 承载已登记远端主机、会话元数据、连接状态，以及 **LAN 出站 PTY 客户端**
//!（[`outbound`]：hello/list/subscribe/write/resize/detach/重订；Mock 可测）。
//! 真机 WebSocket 接线走同一 `OutboundTransport` trait。

pub mod desktop_surface;
pub mod foreign_history;
pub mod history_commands;
pub mod lan_transport;
pub mod live_backpressure;
pub mod outbound;
pub mod reconnect_supervisor;

use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::state::AppState;
use outbound::{
    append_capped, MockOutboundTransport, OutboundClient, OutboundRegistry, OutboundTransport,
    RemoteSessionInfo, DEFAULT_LIVE_OUTPUT_CAP,
};
pub use ridge_core::remote::{HostKind, HostRecord, HostSessionMeta, HostStatus};
use tauri::State;

fn mirror_kernel_host(method: &str, path: &str, body: Option<serde_json::Value>) {
    let Some(endpoint) = ridge_kernel::client::running_endpoint() else {
        return;
    };
    if let Err(error) = ridge_kernel::client::request_json(&endpoint, method, path, body.as_ref()) {
        tracing::debug!(target: "ridge::hosts", %error, method, path, "kernel host topology mirror unavailable");
    }
}

pub(crate) fn kernel_host_snapshot() -> Result<Vec<HostRecord>, String> {
    let endpoint = ridge_kernel::client::running_endpoint()
        .ok_or_else(|| "ridge-kernel domain endpoint unavailable".to_string())?;
    ridge_kernel::client::read_domain_remote_hosts(&endpoint)
        .map(|snapshot| snapshot.hosts)
        .map_err(|error| format!("ridge-kernel remote-host snapshot failed: {error}"))
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendHostSession {
    pub id: String,
    pub title: String,
}

/// 一个 foreign pane 指向的远端会话引用（`PtyHandle.remote_ref`）。live 传输里程接线。
#[derive(Clone, Debug)]
pub struct RemoteRef {
    pub host_id: String,
    pub host_label: String,
    pub remote_pane_id: String,
    pub kind: HostKind,
}

/// Live input sink for a remote pane (V-H1-LIVE). Production WS client plugs here;
/// tests inject an mpsc/channel-backed sink.
pub type LiveInputSink = std::sync::Arc<dyn Fn(&[u8]) + Send + Sync>;

/// R17: foreign pane attachment (local pane_id ↔ remote session).
#[derive(Clone, Debug)]
pub struct ForeignAttachment {
    pub pane_id: uuid::Uuid,
    pub remote: RemoteRef,
}

/// 进程内主机注册表（AppState 持有 `Arc<HostRegistry>`）。
pub struct HostRegistry {
    hosts: RwLock<ridge_core::remote::RemoteHostTopology>,
    /// (host_id, remote_pane_id) → stdin sink toward outbound transport.
    live_sinks: RwLock<HashMap<(String, String), LiveInputSink>>,
    /// (host_id, remote_pane_id) → stdout bytes injected from host (R17-HOST-OUT).
    live_outputs: RwLock<HashMap<(String, String), Vec<u8>>>,
    /// local pane_id → foreign attachment metadata (R17-HOST-PANE).
    foreign_by_pane: RwLock<HashMap<uuid::Uuid, ForeignAttachment>>,
    /// 每 host 独立出站客户端（OP-WS-PTY / OP-RECONN-HOST）。
    outbound: OutboundRegistry,
    /// live 输出缓冲上限（字节/会话）；背压超限丢头部。
    live_output_cap: RwLock<usize>,
    /// Accumulated dropped bytes from live inject (AC4-C8).
    live_dropped_total: std::sync::atomic::AtomicU64,
    /// AC4-C8: per-session backpressure registry (Hosts aggregate UI).
    live_bp: live_backpressure::LiveBackpressureRegistry,
    /// AC4-C5: cancelable outbound reconnect tasks.
    reconnect: reconnect_supervisor::ReconnectSupervisor,
    /// AC4-C6: attach-time history tail per remote session.
    history: foreign_history::ForeignHistoryStore,
}

impl Default for HostRegistry {
    fn default() -> Self {
        Self {
            hosts: RwLock::new(ridge_core::remote::RemoteHostTopology::default()),
            live_sinks: RwLock::new(HashMap::new()),
            live_outputs: RwLock::new(HashMap::new()),
            foreign_by_pane: RwLock::new(HashMap::new()),
            outbound: OutboundRegistry::default(),
            live_output_cap: RwLock::new(DEFAULT_LIVE_OUTPUT_CAP),
            live_dropped_total: std::sync::atomic::AtomicU64::new(0),
            live_bp: live_backpressure::LiveBackpressureRegistry::new(
                DEFAULT_LIVE_OUTPUT_CAP as u64,
            ),
            reconnect: reconnect_supervisor::ReconnectSupervisor::new(),
            history: foreign_history::ForeignHistoryStore::new(),
        }
    }
}

impl HostRegistry {
    pub fn reconnect_supervisor(&self) -> &reconnect_supervisor::ReconnectSupervisor {
        &self.reconnect
    }

    pub fn history(&self) -> &foreign_history::ForeignHistoryStore {
        &self.history
    }

    pub fn live_bp(&self) -> &live_backpressure::LiveBackpressureRegistry {
        &self.live_bp
    }

    /// On host disconnect: schedule cancelable resubscribe for attached panes.
    pub fn on_host_disconnected_schedule_reconnect(&self, host_id: &str) {
        let panes: Vec<String> = self
            .foreign_by_pane
            .read()
            .values()
            .filter(|f| f.remote.host_id == host_id)
            .map(|f| f.remote.remote_pane_id.clone())
            .collect();
        if !panes.is_empty() {
            self.reconnect.schedule(host_id, panes);
        } else {
            self.reconnect.cancel(host_id);
        }
    }

    pub fn snapshot(&self) -> Vec<HostRecord> {
        self.hosts.read().snapshot()
    }

    /// Kernel owns durable host topology. A shell rebuilds only this logical
    /// projection; live transports and pane attachments stay process-local.
    pub fn restore_topology(&self, records: Vec<HostRecord>) {
        *self.hosts.write() = ridge_core::remote::RemoteHostTopology::from_records(
            records
                .into_iter()
                .map(|record| (record.id.clone(), record))
                .collect(),
        );
    }

    pub fn upsert(&self, rec: HostRecord) {
        if let Ok(body) = serde_json::to_value(&rec) {
            mirror_kernel_host("POST", "/v1/domain/remote-hosts", Some(body));
        }
        self.hosts.write().upsert(rec);
    }

    pub fn remove(&self, id: &str) -> bool {
        self.live_sinks.write().retain(|(hid, _), _| hid != id);
        let removed = self.hosts.write().remove(id);
        if removed {
            mirror_kernel_host("DELETE", &format!("/v1/domain/remote-hosts/{id}"), None);
        }
        removed
    }

    pub fn set_status(&self, id: &str, status: HostStatus, detail: impl Into<String>) {
        let hosts = self.hosts.write();
        if let Some(mut h) = hosts.get(id) {
            h.status = status;
            h.detail = detail.into();
            drop(hosts);
            self.upsert(h);
        }
    }

    /// Register live stdin sink for a remote pane (V-H1-LIVE).
    pub fn set_live_sink(&self, host_id: &str, remote_pane_id: &str, sink: LiveInputSink) {
        self.live_sinks
            .write()
            .insert((host_id.to_string(), remote_pane_id.to_string()), sink);
    }

    /// Route bytes to live sink if present. Returns true if delivered.
    pub fn write_live(&self, host_id: &str, remote_pane_id: &str, bytes: &[u8]) -> bool {
        let map = self.live_sinks.read();
        if let Some(sink) = map.get(&(host_id.to_string(), remote_pane_id.to_string())) {
            sink(bytes);
            true
        } else {
            false
        }
    }

    /// R17-HOST-OUT: inject PTY output from remote host into local buffer (capped).
    /// Returns bytes dropped due to backpressure.
    pub fn inject_live_output(&self, host_id: &str, remote_pane_id: &str, bytes: &[u8]) -> usize {
        let cap = *self.live_output_cap.read();
        let key = (host_id.to_string(), remote_pane_id.to_string());
        let mut map = self.live_outputs.write();
        let buf = map.entry(key).or_default();
        let dropped = append_capped(buf, bytes, cap);
        if dropped > 0 {
            self.live_dropped_total
                .fetch_add(dropped as u64, std::sync::atomic::Ordering::SeqCst);
        }
        // C8 product path: per-session registry for Hosts aggregate badges.
        self.live_bp
            .record_inject(host_id, remote_pane_id, buf.len() as u64, dropped as u64);
        dropped
    }

    pub fn live_dropped_total(&self) -> u64 {
        self.live_dropped_total
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn live_buffer_bytes_for(&self, host_id: &str, remote_pane_id: &str) -> usize {
        self.live_outputs
            .read()
            .get(&(host_id.to_string(), remote_pane_id.to_string()))
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub fn set_live_output_cap(&self, cap: usize) {
        let c = cap.max(1);
        *self.live_output_cap.write() = c;
        self.live_bp.set_cap(c as u64);
    }

    pub fn live_output_cap(&self) -> usize {
        *self.live_output_cap.read()
    }

    /// Bind a mock or production outbound client for this host.
    pub fn bind_outbound(&self, client: Arc<OutboundClient>) {
        self.outbound.insert(client);
    }

    pub fn outbound_client(&self, host_id: &str) -> Option<Arc<OutboundClient>> {
        self.outbound.get(host_id)
    }

    pub fn remove_outbound(&self, host_id: &str) {
        self.outbound.remove(host_id);
    }

    /// Detach local foreign view. Keep the host connection while another pane from that host is
    /// attached; disconnect it after the last pane. Never kills the remote PTY.
    pub fn detach_foreign(&self, pane_id: uuid::Uuid) -> Result<RemoteRef, String> {
        let att = self
            .foreign_by_pane
            .write()
            .remove(&pane_id)
            .ok_or_else(|| format!("pane {pane_id} is not foreign"))?;
        let remote = att.remote;
        let remaining = self.foreign_by_pane.read();
        let remote_still_attached = remaining.values().any(|f| {
            f.remote.host_id == remote.host_id && f.remote.remote_pane_id == remote.remote_pane_id
        });
        let host_still_attached = remaining
            .values()
            .any(|f| f.remote.host_id == remote.host_id);
        drop(remaining);

        if let Some(client) = self.outbound.get(&remote.host_id) {
            if !remote_still_attached {
                let _ = client.unsubscribe(&remote.remote_pane_id);
            }
            if !host_still_attached {
                client.disconnect();
            }
        }
        if !remote_still_attached {
            self.live_sinks
                .write()
                .remove(&(remote.host_id.clone(), remote.remote_pane_id.clone()));
            // Drop live buffer bytes for this session (keep history tail for re-attach seed).
            self.live_outputs
                .write()
                .remove(&(remote.host_id.clone(), remote.remote_pane_id.clone()));
            self.live_bp
                .clear_session(&remote.host_id, &remote.remote_pane_id);
            self.set_session_attached(&remote.host_id, &remote.remote_pane_id, false);
        }
        if !host_still_attached {
            self.reconnect.cancel(&remote.host_id);
            self.set_status(
                &remote.host_id,
                HostStatus::Disconnected,
                "最后一个接入 pane 已关闭",
            );
        }
        self.publish_control_plane();
        Ok(remote)
    }

    /// Apply list_panes result onto HostRecord.sessions.
    pub fn replace_sessions(&self, host_id: &str, sessions: Vec<HostSessionMeta>) {
        let hosts = self.hosts.write();
        if let Some(mut h) = hosts.get(host_id) {
            h.sessions = sessions;
            drop(hosts);
            self.upsert(h);
        }
    }

    /// Pane ids registered as foreign for this remote session.
    pub fn panes_for_remote(&self, host_id: &str, remote_pane_id: &str) -> Vec<uuid::Uuid> {
        self.foreign_by_pane
            .read()
            .values()
            .filter(|f| f.remote.host_id == host_id && f.remote.remote_pane_id == remote_pane_id)
            .map(|f| f.pane_id)
            .collect()
    }

    /// Drain/copy live output buffer (tests + future fan-out).
    pub fn live_output_snapshot(&self, host_id: &str, remote_pane_id: &str) -> Vec<u8> {
        self.live_outputs
            .read()
            .get(&(host_id.to_string(), remote_pane_id.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    pub fn register_foreign(&self, pane_id: uuid::Uuid, remote: RemoteRef) {
        self.foreign_by_pane
            .write()
            .insert(pane_id, ForeignAttachment { pane_id, remote });
        self.publish_control_plane();
    }

    /// foreign attachment count (control-plane publish).
    pub fn foreign_count(&self) -> usize {
        self.foreign_by_pane.read().len()
    }

    pub fn outbound_connected_count(&self) -> usize {
        self.hosts
            .read()
            .snapshot()
            .into_iter()
            .filter(|h| h.status == HostStatus::Connected)
            .count()
    }

    /// Push multi-host counters into orch_health (OP-AGENT-CP).
    pub fn publish_control_plane(&self) {
        crate::teammate::orch_health::publish_hosts_control_plane(
            self.foreign_count(),
            self.outbound_connected_count(),
        );
    }

    pub fn outbound_stats_snapshot(
        &self,
        host_id: &str,
    ) -> Option<desktop_surface::OutboundStatsDto> {
        let client = self.outbound.get(host_id)?;
        let state = format!("{:?}", client.state());
        let subscribed: Vec<String> = {
            // derive from session attached flags + client is_subscribed
            self.list_sessions(host_id)
                .unwrap_or_default()
                .into_iter()
                .filter(|s| client.is_subscribed(&s.id))
                .map(|s| s.id)
                .collect()
        };
        use std::sync::atomic::Ordering;
        let buf_bytes: u64 = subscribed
            .iter()
            .map(|sid| self.live_buffer_bytes_for(host_id, sid) as u64)
            .sum();
        Some(desktop_surface::OutboundStatsDto {
            host_id: host_id.to_string(),
            state,
            subscribed,
            hello_ok: client.stats.hello_ok.load(Ordering::SeqCst),
            list_ok: client.stats.list_ok.load(Ordering::SeqCst),
            subscribe_ok: client.stats.subscribe_ok.load(Ordering::SeqCst),
            write_ok: client.stats.write_ok.load(Ordering::SeqCst),
            resize_ok: client.stats.resize_ok.load(Ordering::SeqCst),
            resize_suppressed: client.stats.resize_suppressed.load(Ordering::SeqCst),
            fanout_bytes: client.stats.fanout_bytes.load(Ordering::SeqCst),
            reconnect_attempts: client.stats.reconnect_attempts.load(Ordering::SeqCst),
            resubscribe_ok: client.stats.resubscribe_ok.load(Ordering::SeqCst),
            errors: client.stats.errors.load(Ordering::SeqCst),
            live_buffer_cap: self.live_output_cap() as u64,
            live_buffer_bytes: buf_bytes,
            live_dropped_bytes: self.live_dropped_total(),
        })
    }

    pub fn foreign_for_pane(&self, pane_id: uuid::Uuid) -> Option<ForeignAttachment> {
        self.foreign_by_pane.read().get(&pane_id).cloned()
    }

    pub fn get(&self, id: &str) -> Option<HostRecord> {
        self.hosts.read().get(id)
    }

    /// R17-HOST-LIST: sessions for a host (empty if missing).
    pub fn list_sessions(&self, host_id: &str) -> Option<Vec<HostSessionMeta>> {
        self.hosts.read().get(host_id).map(|h| h.sessions.clone())
    }

    /// Mark a session attached flag.
    pub fn set_session_attached(&self, host_id: &str, session_id: &str, attached: bool) {
        let hosts = self.hosts.write();
        if let Some(mut h) = hosts.get(host_id) {
            if let Some(s) = h.sessions.iter_mut().find(|s| s.id == session_id) {
                s.attached = attached;
            }
            drop(hosts);
            self.upsert(h);
        }
    }
}

/// Parse `host:port` (or bare host → default 443 for https-style, 7522 for rdg heuristic).
pub fn parse_host_port(addr: &str, kind: HostKind) -> Result<(String, u16), String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err("地址不能为空".into());
    }
    // strip scheme if present
    let stripped = addr
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    let (host, port) = if let Some((h, p)) = stripped.rsplit_once(':') {
        let port: u16 = p
            .split('/')
            .next()
            .unwrap_or(p)
            .parse()
            .map_err(|_| format!("无效端口: {p}"))?;
        (h.to_string(), port)
    } else {
        let host = stripped.split('/').next().unwrap_or(stripped).to_string();
        let port = match kind {
            HostKind::Rdg => 7522,
            HostKind::Remote => 443,
        };
        (host, port)
    };
    if host.is_empty() {
        return Err("主机名为空".into());
    }
    Ok((host, port))
}

/// TCP reachability probe (V-H1 minimal live path). `timeout_ms` caps wait.
pub fn probe_tcp(host: &str, port: u16, timeout_ms: u64) -> Result<(), String> {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let addr = format!("{host}:{port}");
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS/解析失败: {e}"))?;
    let sock: SocketAddr = addrs
        .next()
        .ok_or_else(|| format!("无法解析地址: {addr}"))?;
    TcpStream::connect_timeout(&sock, Duration::from_millis(timeout_ms))
        .map_err(|e| format!("不可达 {addr}: {e}"))?;
    Ok(())
}

/// 快照所有已登记远端主机（读，供前端 Hosts 面板与 headless 会话合并展示）。
fn project_kernel_host_snapshot(
    state: &HostRegistry,
    result: Result<Vec<HostRecord>, String>,
) -> Result<Vec<HostRecord>, String> {
    let records = result?;
    state.restore_topology(records.clone());
    Ok(records)
}

#[tauri::command]
pub fn host_list_snapshot(state: State<'_, AppState>) -> Result<Vec<HostRecord>, String> {
    project_kernel_host_snapshot(&state.hosts, kernel_host_snapshot())
}

/// Register topology discovered by a desktop-owned RemoteLink. Desktop-only: never admitted
/// through the remote invoke surface, so a shared workspace cannot re-export its origin.
#[tauri::command]
pub fn register_frontend_host(
    state: State<'_, AppState>,
    host_id: String,
    kind: HostKind,
    label: String,
    sessions: Vec<FrontendHostSession>,
) {
    let rows = sessions
        .into_iter()
        .map(|session| HostSessionMeta {
            attached: !state
                .hosts
                .panes_for_remote(&host_id, &session.id)
                .is_empty(),
            id: session.id,
            title: session.title,
        })
        .collect();
    state.hosts.upsert(HostRecord {
        id: host_id,
        kind,
        label,
        addr: "frontend-remote-link".into(),
        status: HostStatus::Connected,
        detail: "RemoteLink topology".into(),
        sessions: rows,
    });
}

/// 登记并探测一台远端主机（V-H1：TCP 可达 → Connected，否则 Error）。
/// 凭据（`token`）不落库。完整 PTY 字节流仍可后续挂 `RemoteRef`。
#[tauri::command]
pub fn connect_host(
    state: State<'_, AppState>,
    kind: String,
    label: Option<String>,
    addr: String,
    token: Option<String>,
) -> Result<String, String> {
    let addr = addr.trim().to_string();
    if addr.is_empty() {
        return Err("地址不能为空".to_string());
    }
    let _ = token;
    let kind = match kind.as_str() {
        "rdg" => HostKind::Rdg,
        _ => HostKind::Remote,
    };
    let id = format!(
        "{}:{}",
        if kind == HostKind::Rdg { "rdg" } else { "lan" },
        addr
    );
    let label = label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.clone());

    let (host, port) = parse_host_port(&addr, kind)?;
    state.hosts.upsert(HostRecord {
        id: id.clone(),
        kind,
        label: label.clone(),
        addr: addr.clone(),
        status: HostStatus::Connecting,
        detail: format!("探测 {host}:{port} …"),
        sessions: Vec::new(),
    });

    match probe_tcp(&host, port, 1500) {
        Ok(()) => {
            state.hosts.upsert(HostRecord {
                id: id.clone(),
                kind,
                label,
                addr,
                status: HostStatus::Connected,
                detail: format!("TCP {host}:{port} 可达（live PTY 会话列表待协商）"),
                sessions: vec![HostSessionMeta {
                    id: "probe".into(),
                    title: "reachability-ok".into(),
                    attached: false,
                }],
            });
        }
        Err(e) => {
            state.hosts.upsert(HostRecord {
                id: id.clone(),
                kind,
                label,
                addr,
                status: HostStatus::Error,
                detail: e,
                sessions: Vec::new(),
            });
        }
    }
    Ok(id)
}

/// Attach foreign session only when host is Connected (V-H1 gate).
/// Pure check against a status snapshot — used by attach command surface.
pub fn ensure_host_status_connected(status: HostStatus, detail: &str) -> Result<(), String> {
    if status != HostStatus::Connected {
        return Err(format!("主机未连接（status={status:?}）: {detail}"));
    }
    Ok(())
}

/// Attach foreign session only when host is Connected (V-H1 gate).
/// Kept public for attach command surface; unit-tested via status helper + registry.
#[allow(dead_code)] // attach wire-up reuses this; not yet on a Tauri command.
pub fn ensure_host_connected(state: &AppState, host_id: &str) -> Result<(), String> {
    let snap = state.hosts.snapshot();
    let Some(h) = snap.iter().find(|h| h.id == host_id) else {
        return Err(format!("未知主机: {host_id}"));
    };
    ensure_host_status_connected(h.status, &h.detail)
}

/// 断开一台远端主机（置 `Disconnected`；不移除登记；清 outbound 订阅）。
#[tauri::command]
pub fn disconnect_host(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    disconnect_host_outbound(&state.hosts, &host_id);
    Ok(())
}

/// 忘记一台远端主机（移除登记 + 出站客户端）。
#[tauri::command]
pub fn forget_host(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    state.hosts.remove_outbound(&host_id);
    state.hosts.remove(&host_id);
    Ok(())
}

/// 为已登记 host 绑定 mock/生产出站客户端并 hello+list，填充 sessions（OP-WS-PTY T1）。
/// 测试与后续真 WS 接线共用；无 transport 时仍可用旧 probe sessions。
pub fn bind_outbound_and_list(
    hosts: &HostRegistry,
    host_id: &str,
    transport: Arc<dyn OutboundTransport>,
) -> Result<Vec<HostSessionMeta>, String> {
    let client = Arc::new(OutboundClient::new(host_id, transport));
    let list = client.connect_and_list()?;
    let sessions: Vec<HostSessionMeta> = list
        .into_iter()
        .map(|s: RemoteSessionInfo| HostSessionMeta {
            id: s.id,
            title: s.title,
            attached: false,
        })
        .collect();
    hosts.replace_sessions(host_id, sessions.clone());
    hosts.bind_outbound(client);
    hosts.set_status(
        host_id,
        HostStatus::Connected,
        format!("outbound listed {} session(s)", sessions.len()),
    );
    hosts.publish_control_plane();
    Ok(sessions)
}

/// Desktop diagnostics: outbound client counters for a host.
#[tauri::command]
pub fn get_outbound_stats(
    state: State<'_, AppState>,
    host_id: String,
) -> Result<desktop_surface::OutboundStatsDto, String> {
    state
        .hosts
        .outbound_stats_snapshot(&host_id)
        .ok_or_else(|| format!("no outbound client for {host_id}"))
}

/// AC4-C8: aggregate live backpressure for Hosts panel (desktop-only).
#[tauri::command]
pub fn get_live_backpressure(
    state: State<'_, AppState>,
    host_id: String,
) -> live_backpressure::AggregateBp {
    state.hosts.live_bp().aggregate_for_host(&host_id)
}

/// V-H1-LIVE / R17-HOST-PANE：把远端会话接入为 foreign 视图（需 Connected）。
/// 注册 live stdin sink、foreign 元数据；若有 outbound 客户端则 **subscribe**。
#[tauri::command]
pub fn attach_host_session(
    state: State<'_, AppState>,
    host_id: String,
    session_id: String,
    workspace_id: Option<String>,
) -> Result<String, String> {
    ensure_host_connected(&state, &host_id)?;
    let host = state
        .hosts
        .get(&host_id)
        .ok_or_else(|| format!("未知主机: {host_id}"))?;
    if !host.sessions.iter().any(|s| s.id == session_id) {
        return Err(format!("未知会话: {session_id}"));
    }
    let host_label = host.label.clone();
    let kind = host.kind;

    let wid = match workspace_id {
        Some(s) => uuid::Uuid::parse_str(&s).map_err(|e| e.to_string())?,
        None => state.active_workspace_id(),
    };

    // Prefer split of first leaf so layout gains a real pane id; fall back to fresh uuid.
    let pane_id = {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            let leaves = ws.pane_tree.get_all_leaves();
            if let Some(target) = leaves.first().copied() {
                use ridge_core::workspace::pane_tree::SplitDirection;
                if let Ok(id) = ws.pane_tree.split(target, SplitDirection::Vertical) {
                    id
                } else {
                    uuid::Uuid::new_v4()
                }
            } else {
                uuid::Uuid::new_v4()
            }
        } else {
            uuid::Uuid::new_v4()
        }
    };

    let remote = RemoteRef {
        host_id: host_id.clone(),
        host_label,
        remote_pane_id: session_id.clone(),
        kind,
    };

    // Prefer outbound client write path when bound; else buffer sink for tests.
    if let Some(client) = state.hosts.outbound_client(&host_id) {
        client.subscribe(&session_id)?;
        let client_c = client.clone();
        let session_id_c = session_id.clone();
        state.hosts.set_live_sink(
            &host_id,
            &session_id,
            Arc::new(move |bytes: &[u8]| {
                if let Err(e) = client_c.write_pty(&session_id_c, bytes) {
                    tracing::warn!(target: "ridge::hosts", error = %e, "outbound write_pty");
                }
            }),
        );
    } else {
        let host_id_c = host_id.clone();
        let session_id_c = session_id.clone();
        let sink_buf: Arc<parking_lot::Mutex<Vec<u8>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));
        let buf_c = sink_buf.clone();
        state.hosts.set_live_sink(
            &host_id,
            &session_id,
            Arc::new(move |bytes: &[u8]| {
                buf_c.lock().extend_from_slice(bytes);
                tracing::trace!(
                    target: "ridge::hosts",
                    host = %host_id_c,
                    pane = %session_id_c,
                    n = bytes.len(),
                    "live stdin"
                );
            }),
        );
    }
    state.hosts.register_foreign(pane_id, remote.clone());
    state
        .hosts
        .set_session_attached(&host_id, &session_id, true);

    // Install foreign terminal handle so write_pty routes via remote_ref.
    {
        use portable_pty::{native_pty_system, PtySize};
        use std::sync::atomic::{AtomicBool, AtomicI64};
        use std::sync::Arc;
        let pty_system = native_pty_system();
        if let Ok(pair) = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            let portable_pty::PtyPair {
                master,
                slave: _slave,
            } = pair;
            if let Ok(w) = master.take_writer() {
                let parser = Arc::new(parking_lot::Mutex::new(
                    crate::engine::parser::PaneParser::new(24, 80, 2000),
                ));
                let handle = crate::engine::pty::PtyHandle {
                    master: Arc::new(parking_lot::Mutex::new(master)),
                    writer: Arc::new(parking_lot::Mutex::new(w)),
                    _child: None,
                    native_ref: None,
                    native_cancel: None,
                    remote_ref: Some(remote),
                    job: None,
                    child_pid: None,
                    resize_silence_deadline: Arc::new(AtomicI64::new(0)),
                    parser,
                    delta_mode: Arc::new(AtomicBool::new(false)),
                };
                // AC4-C6: seed scrollback once via history API (uses seed_parser_feed).
                // Keep tail after first attach so re-attach can re-seed; reattach
                // clear is a product option (plan_attach_seed clear_after).
                let parser_c = handle.parser.clone();
                let _seeded =
                    state
                        .hosts
                        .history()
                        .seed_parser_feed(&host_id, &session_id, |bytes| {
                            let _ = parser_c.lock().feed_and_diff(bytes);
                        });
                let mut map = state.workspaces.write();
                if let Some(ws) = map.get_mut(&wid) {
                    ws.terminals.insert(pane_id, handle);
                }
            }
        }
    }

    Ok(pane_id.to_string())
}

/// R17-HOST-LIST：列出主机会话（须已登记）。
#[tauri::command]
pub fn list_host_sessions(
    state: State<'_, AppState>,
    host_id: String,
) -> Result<Vec<HostSessionMeta>, String> {
    state
        .hosts
        .list_sessions(&host_id)
        .ok_or_else(|| format!("未知主机: {host_id}"))
}

/// R17-HOST-OUT：注入远端输出并 fan-out 到 foreign pane 的 VT parser（可见回灌）。
#[tauri::command]
pub fn inject_host_output(
    state: State<'_, AppState>,
    host_id: String,
    session_id: String,
    data_b64: String,
) -> Result<(), String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| e.to_string())?;
    fanout_live_output(&state, &host_id, &session_id, &bytes);
    Ok(())
}

/// Buffer + feed every attached foreign pane's parser (shipped fan-out path).
pub fn fanout_live_output(state: &AppState, host_id: &str, session_id: &str, bytes: &[u8]) {
    state.hosts.history().append(host_id, session_id, bytes);
    state.hosts.inject_live_output(host_id, session_id, bytes);
    let panes = state.hosts.panes_for_remote(host_id, session_id);
    if panes.is_empty() {
        return;
    }
    let map = state.workspaces.read();
    for ws in map.values() {
        for pid in &panes {
            if let Some(handle) = ws.terminals.get(pid) {
                if handle
                    .remote_ref
                    .as_ref()
                    .is_some_and(|rr| rr.host_id == host_id && rr.remote_pane_id == session_id)
                {
                    // Feed VT mirror so scrollback/grid reflect remote output.
                    let _ = handle.parser.lock().feed_and_diff(bytes);
                }
            }
        }
    }
}

/// Route stdin for a foreign remote_ref (called from write_pty path).
pub fn route_foreign_input(state: &AppState, rr: &RemoteRef, bytes: &[u8]) -> Result<(), String> {
    if state
        .hosts
        .write_live(&rr.host_id, &rr.remote_pane_id, bytes)
    {
        Ok(())
    } else {
        Err(format!(
            "no live sink for {}/{}",
            rr.host_id, rr.remote_pane_id
        ))
    }
}

/// Route resize for foreign pane through outbound client when bound.
pub fn route_foreign_resize(
    state: &AppState,
    rr: &RemoteRef,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    if let Some(client) = state.hosts.outbound_client(&rr.host_id) {
        return client.resize_pane(&rr.remote_pane_id, rows, cols);
    }
    // No outbound: accept no-op (local foreign parser still resized by caller).
    Ok(())
}

/// Detach foreign local pane (OP-WS-LIFE). Removes terminal handle if present.
#[tauri::command]
pub fn detach_host_session(
    state: State<'_, AppState>,
    pane_id: String,
    workspace_id: Option<String>,
) -> Result<(), String> {
    let pid = uuid::Uuid::parse_str(&pane_id).map_err(|e| e.to_string())?;
    let remote = state.hosts.detach_foreign(pid)?;
    let wid = match workspace_id {
        Some(s) => uuid::Uuid::parse_str(&s).map_err(|e| e.to_string())?,
        None => state.active_workspace_id(),
    };
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            ws.terminals.remove(&pid);
        }
    }
    tracing::info!(
        target: "ridge::hosts",
        host = %remote.host_id,
        remote_pane = %remote.remote_pane_id,
        local_pane = %pane_id,
        "detached foreign view (remote session continues)"
    );
    Ok(())
}

/// Pump outbound inbound raw → fanout (shipped read-loop body).
/// Called by [`pump_host_output`] Tauri command (Hosts panel poll / attach follow-up)
/// and unit tests. Production WS read task injects into transport then invokes this.
pub fn pump_outbound_to_fanout(state: &AppState, host_id: &str) -> Result<usize, String> {
    let client = state
        .hosts
        .outbound_client(host_id)
        .ok_or_else(|| format!("no outbound for {host_id}"))?;
    let frames = client.pump_output();
    let mut n = 0usize;
    for (session_id, bytes) in frames {
        fanout_live_output(state, host_id, &session_id, &bytes);
        n += bytes.len();
    }
    Ok(n)
}

/// Desktop command: drain outbound inbound buffers into foreign pane parsers.
/// Hosts UI polls this for each Connected host that has an outbound client.
#[tauri::command]
pub fn pump_host_output(state: State<'_, AppState>, host_id: String) -> Result<usize, String> {
    pump_outbound_to_fanout(&state, &host_id)
}

/// Bind mock/LAN transport + list, then return sessions (desktop helper for tests/UI).
#[tauri::command]
pub fn bind_mock_outbound_and_list(
    state: State<'_, AppState>,
    host_id: String,
    sessions_json: String,
) -> Result<Vec<HostSessionMeta>, String> {
    let parsed: Vec<RemoteSessionInfo> =
        serde_json::from_str(&sessions_json).map_err(|e| format!("sessions_json: {e}"))?;
    // Ensure host record exists
    if state.hosts.get(&host_id).is_none() {
        return Err(format!("unknown host {host_id}; connect_host first"));
    }
    let mock = Arc::new(MockOutboundTransport::new());
    mock.preset_list(&parsed);
    bind_outbound_and_list(&state.hosts, &host_id, mock)
}

/// Host disconnect: mark outbound disconnected + host status (subscriptions cleared).
pub fn disconnect_host_outbound(hosts: &HostRegistry, host_id: &str) {
    if let Some(c) = hosts.outbound_client(host_id) {
        c.mark_disconnected();
    }
    // Clear live buffers for this host; keep history tails for re-attach seed.
    {
        let mut outs = hosts.live_outputs.write();
        outs.retain(|(h, _), _| h != host_id);
    }
    hosts.live_bp().clear_host(host_id);
    hosts.set_status(host_id, HostStatus::Disconnected, "已断开");
    hosts.on_host_disconnected_schedule_reconnect(host_id);
}

/// Drive one reconnect supervisor step (Hosts poll / tests).
/// Response includes attempt / cancelled / last_error so UI need not re-query.
#[tauri::command]
pub fn step_host_reconnect(
    state: State<'_, AppState>,
    host_id: String,
    host_reachable: bool,
) -> Result<String, String> {
    let sup = state.hosts.reconnect_supervisor();
    // Already terminal + reachable → collapse to Idle (uses SupervisorPhase::Idle).
    if host_reachable {
        if matches!(
            sup.phase(&host_id),
            Some(reconnect_supervisor::SupervisorPhase::Succeeded)
        ) {
            sup.mark_idle(&host_id);
            return Ok("phase=Idle attempt=0 cancelled=0 terminal".into());
        }
    }
    let client = state
        .hosts
        .outbound_client(&host_id)
        .ok_or_else(|| format!("no outbound for {host_id}"))?;
    let delay = sup.step_once(&host_id, &client, host_reachable);
    let phase = sup.phase_str(&host_id).unwrap_or("None");
    let attempt = sup.attempt(&host_id).unwrap_or(0);
    let cancelled = if sup.is_cancelled(&host_id) { 1 } else { 0 };
    let err = sup
        .last_error(&host_id)
        .map(|e| format!(" err={}", e.replace(' ', "_")))
        .unwrap_or_default();
    Ok(match delay {
        Some(d) => format!(
            "phase={phase} attempt={attempt} cancelled={cancelled} next_delay_ms={}{err}",
            d.as_millis()
        ),
        None => format!("phase={phase} attempt={attempt} cancelled={cancelled} terminal{err}"),
    })
}

#[tauri::command]
pub fn cancel_host_reconnect(state: State<'_, AppState>, host_id: String) -> Result<bool, String> {
    Ok(state.hosts.reconnect_supervisor().cancel(&host_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn parse_host_port_defaults() {
        let (h, p) = parse_host_port("192.168.1.2", HostKind::Rdg).unwrap();
        assert_eq!(h, "192.168.1.2");
        assert_eq!(p, 7522);
        let (h, p) = parse_host_port("example.com:8443", HostKind::Remote).unwrap();
        assert_eq!((h.as_str(), p), ("example.com", 8443));
    }

    #[test]
    fn probe_tcp_localhost_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let _ = listener.accept();
        });
        thread::sleep(Duration::from_millis(20));
        probe_tcp("127.0.0.1", port, 1000).expect("should reach local listener");
        probe_tcp("127.0.0.1", 1, 200).expect_err("port 1 should fail");
    }

    #[test]
    fn ensure_connected_gate() {
        let reg = HostRegistry::default();
        reg.upsert(HostRecord {
            id: "lan:x".into(),
            kind: HostKind::Remote,
            label: "x".into(),
            addr: "x".into(),
            status: HostStatus::Disconnected,
            detail: "n/a".into(),
            sessions: vec![],
        });
        let snap = reg.snapshot();
        let h = snap.iter().find(|h| h.id == "lan:x").unwrap();
        assert!(ensure_host_status_connected(h.status, &h.detail).is_err());
        assert!(ensure_host_status_connected(HostStatus::Connected, "ok").is_ok());
        assert!(ensure_host_status_connected(HostStatus::Error, "boom").is_err());
    }

    #[test]
    fn restore_topology_replaces_shell_projection() {
        let reg = HostRegistry::default();
        reg.upsert(HostRecord {
            id: "stale".into(),
            kind: HostKind::Remote,
            label: "stale".into(),
            addr: "stale".into(),
            status: HostStatus::Disconnected,
            detail: String::new(),
            sessions: vec![],
        });
        reg.restore_topology(vec![HostRecord {
            id: "kernel".into(),
            kind: HostKind::Rdg,
            label: "kernel".into(),
            addr: "127.0.0.1".into(),
            status: HostStatus::Connected,
            detail: "restored".into(),
            sessions: vec![],
        }]);
        let hosts = reg.snapshot();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].id, "kernel");
    }

    #[test]
    fn kernel_host_snapshot_failure_is_fail_closed() {
        let reg = HostRegistry::default();
        reg.upsert(HostRecord {
            id: "stale-shell-only".into(),
            kind: HostKind::Remote,
            label: "stale".into(),
            addr: "stale".into(),
            status: HostStatus::Connected,
            detail: "shell cache".into(),
            sessions: vec![],
        });

        let error = project_kernel_host_snapshot(
            &reg,
            Err("ridge-kernel domain endpoint unavailable".into()),
        )
        .expect_err("kernel failure must be visible to the caller");
        assert!(error.contains("ridge-kernel"));
        assert_eq!(reg.snapshot()[0].id, "stale-shell-only");
    }

    #[test]
    fn live_sink_routes_bytes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let reg = HostRegistry::default();
        let n = std::sync::Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        reg.set_live_sink(
            "lan:h",
            "p1",
            std::sync::Arc::new(move |b: &[u8]| {
                n2.fetch_add(b.len(), Ordering::SeqCst);
            }),
        );
        assert!(reg.write_live("lan:h", "p1", b"abc"));
        assert_eq!(n.load(Ordering::SeqCst), 3);
        assert!(!reg.write_live("lan:h", "missing", b"x"));
    }

    #[test]
    fn inject_live_output_and_register_foreign() {
        let reg = HostRegistry::default();
        reg.inject_live_output("lan:h", "p1", b"hello");
        reg.inject_live_output("lan:h", "p1", b"!");
        assert_eq!(reg.live_output_snapshot("lan:h", "p1"), b"hello!");
        let pane = uuid::Uuid::new_v4();
        reg.register_foreign(
            pane,
            RemoteRef {
                host_id: "lan:h".into(),
                host_label: "h".into(),
                remote_pane_id: "p1".into(),
                kind: HostKind::Remote,
            },
        );
        let f = reg.foreign_for_pane(pane).expect("foreign");
        assert_eq!(f.remote.remote_pane_id, "p1");
        assert_eq!(reg.panes_for_remote("lan:h", "p1"), vec![pane]);
    }

    #[test]
    fn fanout_feeds_foreign_pane_parser() {
        use portable_pty::{native_pty_system, PtySize};
        use std::sync::atomic::{AtomicBool, AtomicI64};
        use std::sync::Arc;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let state = crate::state::AppState::new(tx);
        let wid = state.active_workspace_id();
        let pane = uuid::Uuid::new_v4();
        let remote = RemoteRef {
            host_id: "lan:h".into(),
            host_label: "h".into(),
            remote_pane_id: "p1".into(),
            kind: HostKind::Remote,
        };
        state.hosts.register_foreign(pane, remote.clone());
        // Minimal foreign handle with real parser.
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let portable_pty::PtyPair { master, slave: _ } = pair;
        let w = master.take_writer().expect("writer");
        let handle = crate::engine::pty::PtyHandle {
            master: Arc::new(parking_lot::Mutex::new(master)),
            writer: Arc::new(parking_lot::Mutex::new(w)),
            _child: None,
            native_ref: None,
            native_cancel: None,
            remote_ref: Some(remote),
            job: None,
            child_pid: None,
            resize_silence_deadline: Arc::new(AtomicI64::new(0)),
            parser: Arc::new(parking_lot::Mutex::new(
                crate::engine::parser::PaneParser::new(24, 80, 200),
            )),
            delta_mode: Arc::new(AtomicBool::new(false)),
        };
        {
            let mut map = state.workspaces.write();
            map.get_mut(&wid).unwrap().terminals.insert(pane, handle);
        }
        fanout_live_output(&state, "lan:h", "p1", b"hello from host");
        assert_eq!(
            state.hosts.live_output_snapshot("lan:h", "p1"),
            b"hello from host"
        );
        // Assert parser grid actually contains injected printable text (not just buffer).
        let map = state.workspaces.read();
        let h = map.get(&wid).unwrap().terminals.get(&pane).unwrap();
        let line0 = h.parser.lock().viewport_line0_text();
        assert!(
            line0.contains("hello from host"),
            "expected injected text in parser viewport, got {line0:?}"
        );
    }

    #[test]
    fn list_sessions_on_connected_record() {
        let reg = HostRegistry::default();
        reg.upsert(HostRecord {
            id: "lan:x".into(),
            kind: HostKind::Remote,
            label: "x".into(),
            addr: "x".into(),
            status: HostStatus::Connected,
            detail: "ok".into(),
            sessions: vec![HostSessionMeta {
                id: "probe".into(),
                title: "reachability-ok".into(),
                attached: false,
            }],
        });
        let s = reg.list_sessions("lan:x").unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, "probe");
        assert!(reg.list_sessions("missing").is_none());
    }

    #[test]
    fn bind_outbound_lists_and_attach_subscribes_write() {
        let reg = HostRegistry::default();
        reg.upsert(HostRecord {
            id: "lan:h".into(),
            kind: HostKind::Remote,
            label: "h".into(),
            addr: "127.0.0.1:1".into(),
            status: HostStatus::Connecting,
            detail: String::new(),
            sessions: vec![],
        });
        let mock = Arc::new(MockOutboundTransport::new());
        mock.preset_list(&[
            RemoteSessionInfo {
                id: "main".into(),
                title: "shell".into(),
            },
            RemoteSessionInfo {
                id: "second".into(),
                title: "shell-2".into(),
            },
        ]);
        let sessions = bind_outbound_and_list(&reg, "lan:h", mock.clone()).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(reg.get("lan:h").unwrap().status, HostStatus::Connected);

        let client = reg.outbound_client("lan:h").unwrap();
        client.subscribe("main").unwrap();
        let client_c = client.clone();
        reg.set_live_sink(
            "lan:h",
            "main",
            Arc::new(move |b: &[u8]| {
                let _ = client_c.write_pty("main", b);
            }),
        );
        assert!(reg.write_live("lan:h", "main", b"echo\n"));
        assert_eq!(
            client
                .stats
                .write_ok
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        let pane = uuid::Uuid::new_v4();
        reg.register_foreign(
            pane,
            RemoteRef {
                host_id: "lan:h".into(),
                host_label: "h".into(),
                remote_pane_id: "main".into(),
                kind: HostKind::Remote,
            },
        );
        reg.set_session_attached("lan:h", "main", true);
        let second_pane = uuid::Uuid::new_v4();
        client.subscribe("second").unwrap();
        reg.register_foreign(
            second_pane,
            RemoteRef {
                host_id: "lan:h".into(),
                host_label: "h".into(),
                remote_pane_id: "second".into(),
                kind: HostKind::Remote,
            },
        );
        reg.set_session_attached("lan:h", "second", true);

        let detached = reg.detach_foreign(pane).unwrap();
        assert_eq!(detached.remote_pane_id, "main");
        assert!(reg.foreign_for_pane(pane).is_none());
        assert!(!client.is_subscribed("main"));
        assert!(client.is_subscribed("second"));
        assert_eq!(client.state(), outbound::OutboundState::Subscribed);
        assert_eq!(reg.get("lan:h").unwrap().status, HostStatus::Connected);

        reg.detach_foreign(second_pane).unwrap();
        assert_eq!(client.state(), outbound::OutboundState::Disconnected);
        assert_eq!(reg.get("lan:h").unwrap().status, HostStatus::Disconnected);
    }

    #[test]
    fn live_output_cap_drops_overflow() {
        let reg = HostRegistry::default();
        reg.set_live_output_cap(8);
        let d1 = reg.inject_live_output("h", "p", b"0123456789"); // 10 bytes
        assert!(d1 >= 2);
        let snap = reg.live_output_snapshot("h", "p");
        assert!(snap.len() <= 8);
        assert!(snap.ends_with(b"89") || snap.len() == 8);
    }

    #[test]
    fn pump_outbound_to_fanout_feeds_parser() {
        use portable_pty::{native_pty_system, PtySize};
        use std::sync::atomic::{AtomicBool, AtomicI64};
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let state = crate::state::AppState::new(tx);
        let wid = state.active_workspace_id();
        let pane = uuid::Uuid::new_v4();
        let mock = Arc::new(MockOutboundTransport::new());
        mock.preset_list(&[RemoteSessionInfo {
            id: "main".into(),
            title: "t".into(),
        }]);
        state.hosts.upsert(HostRecord {
            id: "lan:h".into(),
            kind: HostKind::Remote,
            label: "h".into(),
            addr: "x".into(),
            status: HostStatus::Connected,
            detail: "ok".into(),
            sessions: vec![],
        });
        bind_outbound_and_list(&state.hosts, "lan:h", mock.clone()).unwrap();
        let client = state.hosts.outbound_client("lan:h").unwrap();
        client.subscribe("main").unwrap();
        let remote = RemoteRef {
            host_id: "lan:h".into(),
            host_label: "h".into(),
            remote_pane_id: "main".into(),
            kind: HostKind::Remote,
        };
        state.hosts.register_foreign(pane, remote.clone());
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let portable_pty::PtyPair { master, slave: _ } = pair;
        let w = master.take_writer().unwrap();
        let handle = crate::engine::pty::PtyHandle {
            master: Arc::new(parking_lot::Mutex::new(master)),
            writer: Arc::new(parking_lot::Mutex::new(w)),
            _child: None,
            native_ref: None,
            native_cancel: None,
            remote_ref: Some(remote),
            job: None,
            child_pid: None,
            resize_silence_deadline: Arc::new(AtomicI64::new(0)),
            parser: Arc::new(parking_lot::Mutex::new(
                crate::engine::parser::PaneParser::new(24, 80, 200),
            )),
            delta_mode: Arc::new(AtomicBool::new(false)),
        };
        {
            let mut map = state.workspaces.write();
            map.get_mut(&wid).unwrap().terminals.insert(pane, handle);
        }
        mock.inject_pane_output("main", b"pumped-live");
        let n = pump_outbound_to_fanout(&state, "lan:h").unwrap();
        assert!(n >= 11);
        let map = state.workspaces.read();
        let h = map.get(&wid).unwrap().terminals.get(&pane).unwrap();
        let line0 = h.parser.lock().viewport_line0_text();
        assert!(
            line0.contains("pumped-live"),
            "pump must feed foreign parser, got {line0:?}"
        );
    }

    #[test]
    fn disconnect_clears_outbound_subs() {
        let reg = HostRegistry::default();
        reg.upsert(HostRecord {
            id: "lan:h".into(),
            kind: HostKind::Remote,
            label: "h".into(),
            addr: "x".into(),
            status: HostStatus::Connected,
            detail: "ok".into(),
            sessions: vec![],
        });
        let mock = Arc::new(MockOutboundTransport::new());
        mock.preset_list(&[RemoteSessionInfo {
            id: "main".into(),
            title: "t".into(),
        }]);
        bind_outbound_and_list(&reg, "lan:h", mock).unwrap();
        let c = reg.outbound_client("lan:h").unwrap();
        c.subscribe("main").unwrap();
        disconnect_host_outbound(&reg, "lan:h");
        assert!(!c.is_subscribed("main"));
        assert_eq!(reg.get("lan:h").unwrap().status, HostStatus::Disconnected);
    }
}
