//! 外部主机注册表（「主机 / Hosts」面板的远端 ridge / rdg host）。
//!
//! P3/P4 基础层：本模块承载**已登记的远端主机**及其会话元数据、连接状态，并暴露
//! `host_list_snapshot` / `connect_host` / `disconnect_host` / `forget_host` 命令面。
//!
//! **边界（本里程）**：这里只做主机登记与状态管理；真正的**出站连接 + 远端 PTY 流
//! 接管**（把远端 pane 当本地 foreign pane，经 `PtyHandle.remote_ref` 路由 I/O）是
//! 明确的下一里程，需 rebuild + 一台真实远端主机联调验证。见
//! `docs/superpowers/specs/2026-06-30-multi-host-foreign-terminal-hosts-design.md` §2/§9。

use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;

use crate::state::AppState;
use tauri::State;

/// 主机类型：远端 ridge（LAN/cloud）或 rdg（ridge-cli headless host）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Remote,
    Rdg,
}

/// 主机连接状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

/// 远端主机上的一个会话（pane）的元数据。
#[derive(Clone, Debug, Serialize)]
pub struct HostSessionMeta {
    /// 远端 pane id（provider 域内）。
    pub id: String,
    pub title: String,
    /// 是否已被本地某工作区领养。
    pub attached: bool,
}

/// 一台已登记的远端主机记录（序列化给前端 Hosts 面板）。**不含凭据**。
#[derive(Clone, Debug, Serialize)]
pub struct HostRecord {
    pub id: String,
    pub kind: HostKind,
    pub label: String,
    /// 地址（`ip:port` 或 rdg 地址）。凭据（token/TOTP）故意不落库、不序列化。
    pub addr: String,
    pub status: HostStatus,
    /// 面向用户的状态说明（面板顶部/主机行提示）。
    pub detail: String,
    pub sessions: Vec<HostSessionMeta>,
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
#[derive(Default)]
pub struct HostRegistry {
    hosts: RwLock<HashMap<String, HostRecord>>,
    /// (host_id, remote_pane_id) → stdin sink toward outbound transport.
    live_sinks: RwLock<HashMap<(String, String), LiveInputSink>>,
    /// (host_id, remote_pane_id) → stdout bytes injected from host (R17-HOST-OUT).
    live_outputs: RwLock<HashMap<(String, String), Vec<u8>>>,
    /// local pane_id → foreign attachment metadata (R17-HOST-PANE).
    foreign_by_pane: RwLock<HashMap<uuid::Uuid, ForeignAttachment>>,
}

impl HostRegistry {
    pub fn snapshot(&self) -> Vec<HostRecord> {
        let mut v: Vec<HostRecord> = self.hosts.read().values().cloned().collect();
        v.sort_by(|a, b| a.label.cmp(&b.label));
        v
    }

    pub fn upsert(&self, rec: HostRecord) {
        self.hosts.write().insert(rec.id.clone(), rec);
    }

    pub fn remove(&self, id: &str) -> bool {
        self.live_sinks
            .write()
            .retain(|(hid, _), _| hid != id);
        self.hosts.write().remove(id).is_some()
    }

    pub fn set_status(&self, id: &str, status: HostStatus, detail: impl Into<String>) {
        if let Some(h) = self.hosts.write().get_mut(id) {
            h.status = status;
            h.detail = detail.into();
        }
    }

    /// Register live stdin sink for a remote pane (V-H1-LIVE).
    pub fn set_live_sink(&self, host_id: &str, remote_pane_id: &str, sink: LiveInputSink) {
        self.live_sinks.write().insert(
            (host_id.to_string(), remote_pane_id.to_string()),
            sink,
        );
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

    /// R17-HOST-OUT: inject PTY output from remote host into local buffer.
    pub fn inject_live_output(&self, host_id: &str, remote_pane_id: &str, bytes: &[u8]) {
        let key = (host_id.to_string(), remote_pane_id.to_string());
        self.live_outputs
            .write()
            .entry(key)
            .or_default()
            .extend_from_slice(bytes);
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
        self.foreign_by_pane.write().insert(
            pane_id,
            ForeignAttachment {
                pane_id,
                remote,
            },
        );
    }

    pub fn foreign_for_pane(&self, pane_id: uuid::Uuid) -> Option<ForeignAttachment> {
        self.foreign_by_pane.read().get(&pane_id).cloned()
    }

    pub fn get(&self, id: &str) -> Option<HostRecord> {
        self.hosts.read().get(id).cloned()
    }

    /// R17-HOST-LIST: sessions for a host (empty if missing).
    pub fn list_sessions(&self, host_id: &str) -> Option<Vec<HostSessionMeta>> {
        self.hosts.read().get(host_id).map(|h| h.sessions.clone())
    }

    /// Mark a session attached flag.
    pub fn set_session_attached(&self, host_id: &str, session_id: &str, attached: bool) {
        if let Some(h) = self.hosts.write().get_mut(host_id) {
            if let Some(s) = h.sessions.iter_mut().find(|s| s.id == session_id) {
                s.attached = attached;
            }
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
#[tauri::command]
pub fn host_list_snapshot(state: State<'_, AppState>) -> Vec<HostRecord> {
    state.hosts.snapshot()
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
    let id = format!("{}:{}", if kind == HostKind::Rdg { "rdg" } else { "lan" }, addr);
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

/// 断开一台远端主机（置 `Disconnected`；不移除登记）。
#[tauri::command]
pub fn disconnect_host(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    state
        .hosts
        .set_status(&host_id, HostStatus::Disconnected, "已断开");
    Ok(())
}

/// 忘记一台远端主机（移除登记）。
#[tauri::command]
pub fn forget_host(state: State<'_, AppState>, host_id: String) -> Result<(), String> {
    state.hosts.remove(&host_id);
    Ok(())
}

/// V-H1-LIVE / R17-HOST-PANE：把远端会话接入为 foreign 视图（需 Connected）。
/// 注册 live stdin sink、foreign 元数据；可选挂到工作区首 leaf 的 split 新 pane。
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

    let host_id_c = host_id.clone();
    let session_id_c = session_id.clone();
    let sink_buf: std::sync::Arc<parking_lot::Mutex<Vec<u8>>> =
        std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let buf_c = sink_buf.clone();
    state.hosts.set_live_sink(
        &host_id,
        &session_id,
        std::sync::Arc::new(move |bytes: &[u8]| {
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
            let portable_pty::PtyPair { master, slave: _slave } = pair;
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
    if state.hosts.write_live(&rr.host_id, &rr.remote_pane_id, bytes) {
        Ok(())
    } else {
        Err(format!(
            "no live sink for {}/{}",
            rr.host_id, rr.remote_pane_id
        ))
    }
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
}
