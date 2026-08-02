//! LAN 出站 PTY 客户端（完整 WS 里程的可测核心）。
//!
//! 设计不变量（与规划 note OP-WS-PTY 对齐）：
//! - 前端永不直连多 host；本模块由桌面 Rust 持有出站会话。
//! - 帧语义对齐 ridge-cli mux：0x10 pane raw / 0x11 JSON-RPC / 0x12 control。
//! - 生产可接真 WebSocket；单测用 [`MockOutboundTransport`] 驱动 shipped 路径。
//! - 关本地 foreign 视图 = detach；同 host 尚有视图则保连接，最后一个关闭则断连接；均不杀远端 PTY。

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 出站会话状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboundState {
    Idle,
    Connecting,
    HelloOk,
    Listed,
    Subscribed,
    Error,
    Disconnected,
}

/// 远端会话列表项（hello/list 结果）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RemoteSessionInfo {
    pub id: String,
    pub title: String,
}

/// 传输层抽象：生产 WS 与 Mock 共用。
pub trait OutboundTransport: Send + Sync {
    fn send_json_rpc(&self, method: &str, params: Value) -> Result<Value, String>;
    fn send_raw(&self, frame: &[u8]) -> Result<(), String>;
    /// 拉取已缓冲的入站 pane raw 帧（host→controller 0x10）。
    fn drain_pane_raw(&self) -> Vec<(String, Vec<u8>)>;
    /// 关闭底层连接。无状态测试传输可用默认空实现。
    fn close(&self) {}
}

/// 线形 mux 辅助（与 ridge-cli `mux` 对齐的最小子集，避免跨 crate 依赖）。
pub mod wire {
    pub const PANE_RAW: u8 = 0x10;
    /// parity with ridge-cli mux channel tags
    pub const JSON: u8 = 0x11;
    #[allow(dead_code)]
    pub const CONTROL: u8 = 0x12;
    pub const MAX_PANE_ID_BYTES: usize = 255;

    pub fn encode_json(body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + body.len());
        out.push(JSON);
        out.extend_from_slice(body);
        out
    }

    pub fn encode_pane(pane_id: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let id = pane_id.as_bytes();
        if id.len() > MAX_PANE_ID_BYTES {
            return Err(format!("paneId too long ({})", id.len()));
        }
        let mut out = Vec::with_capacity(2 + id.len() + bytes.len());
        out.push(PANE_RAW);
        out.push(id.len() as u8);
        out.extend_from_slice(id);
        out.extend_from_slice(bytes);
        Ok(out)
    }

    pub fn demux_pane(frame: &[u8]) -> Option<(String, Vec<u8>)> {
        if frame.len() < 3 || frame[0] != PANE_RAW {
            return None;
        }
        let n = frame[1] as usize;
        if frame.len() < 2 + n {
            return None;
        }
        let id = std::str::from_utf8(&frame[2..2 + n]).ok()?.to_string();
        let bytes = frame[2 + n..].to_vec();
        Some((id, bytes))
    }
}

/// 可注入的 mock 传输：记录 RPC，可回灌 pane raw。
#[derive(Default)]
pub struct MockOutboundTransport {
    pub rpc_log: Mutex<Vec<(String, Value)>>,
    #[allow(dead_code)]
    pub raw_log: Mutex<Vec<Vec<u8>>>,
    /// method → 预设 result
    pub rpc_results: Mutex<HashMap<String, Value>>,
    pub inbound_pane: Mutex<Vec<(String, Vec<u8>)>>,
    pub fail_next: Mutex<Option<String>>,
}

impl MockOutboundTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn preset_list(&self, sessions: &[RemoteSessionInfo]) {
        let arr: Vec<Value> = sessions
            .iter()
            .map(|s| json!({ "id": s.id, "title": s.title }))
            .collect();
        self.rpc_results
            .lock()
            .insert("list_panes".into(), json!({ "panes": arr }));
        self.rpc_results
            .lock()
            .insert("$/hello".into(), json!({ "ok": true, "role": "host" }));
        self.rpc_results
            .lock()
            .insert("subscribe-pane".into(), json!({ "ok": true }));
        self.rpc_results
            .lock()
            .insert("unsubscribe-pane".into(), json!({ "ok": true }));
        self.rpc_results
            .lock()
            .insert("write_to_pty".into(), json!({ "ok": true }));
        self.rpc_results
            .lock()
            .insert("resize_pane".into(), json!({ "ok": true }));
    }

    pub fn inject_pane_output(&self, pane_id: &str, bytes: &[u8]) {
        self.inbound_pane
            .lock()
            .push((pane_id.to_string(), bytes.to_vec()));
    }
}

impl OutboundTransport for MockOutboundTransport {
    fn send_json_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        if let Some(err) = self.fail_next.lock().take() {
            return Err(err);
        }
        self.rpc_log.lock().push((method.to_string(), params));
        self.rpc_results
            .lock()
            .get(method)
            .cloned()
            .ok_or_else(|| format!("mock: no result for {method}"))
    }

    fn send_raw(&self, frame: &[u8]) -> Result<(), String> {
        if let Some(err) = self.fail_next.lock().take() {
            return Err(err);
        }
        self.raw_log.lock().push(frame.to_vec());
        Ok(())
    }

    fn drain_pane_raw(&self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut *self.inbound_pane.lock())
    }
}

/// 单 host 出站客户端会话（每 host 一份，隔离任务）。
pub struct OutboundClient {
    pub host_id: String,
    pub state: Mutex<OutboundState>,
    transport: Arc<dyn OutboundTransport>,
    sessions: Mutex<Vec<RemoteSessionInfo>>,
    /// remote_pane_id → subscribed
    subscriptions: Mutex<HashMap<String, bool>>,
    /// One resize per host transport at a time. The transport call is
    /// synchronous, so holding this gate also closes the check/send race when
    /// two ResizeObservers report the same dimensions concurrently.
    resize_gate: Mutex<()>,
    /// Last dimensions acknowledged by the remote pane. Cleared on detach or
    /// reconnect so the first resize after a new subscription is never lost.
    resize_last_applied: Mutex<HashMap<String, (u16, u16)>>,
    /// 观测计数（可测，不落盘）
    pub stats: OutboundStats,
}

#[derive(Default)]
pub struct OutboundStats {
    pub hello_ok: AtomicU64,
    pub list_ok: AtomicU64,
    pub subscribe_ok: AtomicU64,
    pub unsubscribe_ok: AtomicU64,
    pub write_ok: AtomicU64,
    pub resize_ok: AtomicU64,
    pub resize_suppressed: AtomicU64,
    pub fanout_bytes: AtomicU64,
    pub reconnect_attempts: AtomicU64,
    pub resubscribe_ok: AtomicU64,
    pub errors: AtomicU64,
    /// 背压：丢弃的超额输出字节（reserved for host telemetry)
    #[allow(dead_code)]
    pub dropped_bytes: AtomicU64,
}

/// live 输出环形缓冲默认上限（每 remote pane）。
pub const DEFAULT_LIVE_OUTPUT_CAP: usize = 256 * 1024;

impl OutboundClient {
    pub fn new(host_id: impl Into<String>, transport: Arc<dyn OutboundTransport>) -> Self {
        Self {
            host_id: host_id.into(),
            state: Mutex::new(OutboundState::Idle),
            transport,
            sessions: Mutex::new(Vec::new()),
            subscriptions: Mutex::new(HashMap::new()),
            resize_gate: Mutex::new(()),
            resize_last_applied: Mutex::new(HashMap::new()),
            stats: OutboundStats::default(),
        }
    }

    pub fn state(&self) -> OutboundState {
        *self.state.lock()
    }

    #[allow(dead_code)]
    pub fn sessions(&self) -> Vec<RemoteSessionInfo> {
        self.sessions.lock().clone()
    }

    pub fn is_subscribed(&self, remote_pane_id: &str) -> bool {
        self.subscriptions
            .lock()
            .get(remote_pane_id)
            .copied()
            .unwrap_or(false)
    }

    /// T1: hello + list_panes
    pub fn connect_and_list(&self) -> Result<Vec<RemoteSessionInfo>, String> {
        *self.state.lock() = OutboundState::Connecting;
        match self
            .transport
            .send_json_rpc("$/hello", json!({ "role": "controller" }))
        {
            Ok(_) => {
                self.stats.hello_ok.fetch_add(1, Ordering::SeqCst);
                *self.state.lock() = OutboundState::HelloOk;
            }
            Err(e) => {
                self.stats.errors.fetch_add(1, Ordering::SeqCst);
                *self.state.lock() = OutboundState::Error;
                return Err(e);
            }
        }
        let result = match self.transport.send_json_rpc("list_panes", json!({})) {
            Ok(v) => v,
            Err(e) => {
                self.stats.errors.fetch_add(1, Ordering::SeqCst);
                *self.state.lock() = OutboundState::Error;
                return Err(e);
            }
        };
        let list = parse_session_list(&result)?;
        *self.sessions.lock() = list.clone();
        self.stats.list_ok.fetch_add(1, Ordering::SeqCst);
        *self.state.lock() = OutboundState::Listed;
        Ok(list)
    }

    /// T2: subscribe-pane
    pub fn subscribe(&self, remote_pane_id: &str) -> Result<(), String> {
        let st = self.state();
        if !matches!(
            st,
            OutboundState::Listed | OutboundState::Subscribed | OutboundState::HelloOk
        ) {
            return Err(format!("cannot subscribe in state {st:?}"));
        }
        let _gate = self.resize_gate.lock();
        self.transport
            .send_json_rpc("subscribe-pane", json!({ "paneId": remote_pane_id }))?;
        self.subscriptions
            .lock()
            .insert(remote_pane_id.to_string(), true);
        self.resize_last_applied.lock().remove(remote_pane_id);
        self.stats.subscribe_ok.fetch_add(1, Ordering::SeqCst);
        *self.state.lock() = OutboundState::Subscribed;
        Ok(())
    }

    /// detach: unsubscribe，保留 host 连接
    pub fn unsubscribe(&self, remote_pane_id: &str) -> Result<(), String> {
        let _gate = self.resize_gate.lock();
        self.transport
            .send_json_rpc("unsubscribe-pane", json!({ "paneId": remote_pane_id }))?;
        self.subscriptions.lock().remove(remote_pane_id);
        self.resize_last_applied.lock().remove(remote_pane_id);
        self.stats.unsubscribe_ok.fetch_add(1, Ordering::SeqCst);
        if self.subscriptions.lock().is_empty() {
            *self.state.lock() = OutboundState::Listed;
        }
        Ok(())
    }

    /// T3: write_to_pty
    pub fn write_pty(&self, remote_pane_id: &str, data: &[u8]) -> Result<(), String> {
        if !self.is_subscribed(remote_pane_id) {
            return Err(format!("not subscribed: {remote_pane_id}"));
        }
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(data);
        self.transport.send_json_rpc(
            "write_to_pty",
            json!({ "paneId": remote_pane_id, "data": b64 }),
        )?;
        self.stats.write_ok.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// T3: resize_pane
    pub fn resize_pane(&self, remote_pane_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        if !self.is_subscribed(remote_pane_id) {
            return Err(format!("not subscribed: {remote_pane_id}"));
        }
        // Serialize the check and the wire call. Without this gate two
        // concurrent layout observers can both miss the cache and enqueue the
        // same resize before either response returns.
        let _gate = self.resize_gate.lock();
        if !self.is_subscribed(remote_pane_id) {
            return Err(format!("not subscribed: {remote_pane_id}"));
        }
        if self.resize_last_applied.lock().get(remote_pane_id).copied() == Some((rows, cols)) {
            self.stats.resize_suppressed.fetch_add(1, Ordering::SeqCst);
            return Ok(());
        }
        self.transport.send_json_rpc(
            "resize_pane",
            json!({ "paneId": remote_pane_id, "rows": rows, "cols": cols }),
        )?;
        self.resize_last_applied
            .lock()
            .insert(remote_pane_id.to_string(), (rows, cols));
        self.stats.resize_ok.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// 拉取 transport 入站 raw 并累计 fanout 字节（调用方再喂 parser）。
    pub fn pump_output(&self) -> Vec<(String, Vec<u8>)> {
        let frames = self.transport.drain_pane_raw();
        let n: u64 = frames.iter().map(|(_, b)| b.len() as u64).sum();
        self.stats.fanout_bytes.fetch_add(n, Ordering::SeqCst);
        frames
    }

    /// 断线后重订：对仍标记 attached 的 pane 重新 subscribe（无双订：先清再订）。
    pub fn reconnect_resubscribe(&self, pane_ids: &[String]) -> Result<(), String> {
        self.stats.reconnect_attempts.fetch_add(1, Ordering::SeqCst);
        *self.state.lock() = OutboundState::Connecting;
        {
            let _gate = self.resize_gate.lock();
            self.resize_last_applied.lock().clear();
        }
        self.connect_and_list()?;
        for id in pane_ids {
            // 先确保本地 map 干净，避免双订
            self.subscriptions.lock().remove(id);
            self.subscribe(id)?;
            self.stats.resubscribe_ok.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    /// Schedule helper: delay before attempt N using shared reconnect_policy.
    pub fn reconnect_delay_ms(attempt: u32) -> Option<u64> {
        // 4 attempts: 200, 400, 800, 1600 (capped 1600) then stop.
        if attempt >= 4 {
            return None;
        }
        Some(crate::reconnect_policy::backoff_ms(attempt, 200, 1600))
    }

    pub fn mark_disconnected(&self) {
        let _gate = self.resize_gate.lock();
        *self.state.lock() = OutboundState::Disconnected;
        self.subscriptions.lock().clear();
        self.resize_last_applied.lock().clear();
    }

    pub fn disconnect(&self) {
        self.mark_disconnected();
        self.transport.close();
    }
}

fn parse_session_list(v: &Value) -> Result<Vec<RemoteSessionInfo>, String> {
    let panes = v
        .get("panes")
        .and_then(|p| p.as_array())
        .ok_or_else(|| "list_panes: missing panes[]".to_string())?;
    let mut out = Vec::new();
    for p in panes {
        let id = p
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| "pane missing id".to_string())?
            .to_string();
        let title = p
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or(&id)
            .to_string();
        out.push(RemoteSessionInfo { id, title });
    }
    Ok(out)
}

/// 带容量上限的 live 输出缓冲（OP-BP-GUARD）。
pub fn append_capped(buf: &mut Vec<u8>, bytes: &[u8], cap: usize) -> usize {
    if cap == 0 {
        return bytes.len();
    }
    if bytes.len() >= cap {
        buf.clear();
        buf.extend_from_slice(&bytes[bytes.len() - cap..]);
        return bytes.len() - cap;
    }
    let next = buf.len() + bytes.len();
    if next <= cap {
        buf.extend_from_slice(bytes);
        return 0;
    }
    let overflow = next - cap;
    if overflow >= buf.len() {
        buf.clear();
        // After clear, keep the tail of new bytes that fits.
        let start = bytes.len().saturating_sub(cap);
        buf.extend_from_slice(&bytes[start..]);
        return overflow;
    }
    buf.drain(0..overflow);
    buf.extend_from_slice(bytes);
    overflow
}

/// 多 host 出站注册表（每 host 独立 OutboundClient）。
#[derive(Default)]
pub struct OutboundRegistry {
    clients: Mutex<HashMap<String, Arc<OutboundClient>>>,
}

impl OutboundRegistry {
    pub fn insert(&self, client: Arc<OutboundClient>) {
        self.clients.lock().insert(client.host_id.clone(), client);
    }

    pub fn get(&self, host_id: &str) -> Option<Arc<OutboundClient>> {
        self.clients.lock().get(host_id).cloned()
    }

    pub fn remove(&self, host_id: &str) -> Option<Arc<OutboundClient>> {
        self.clients.lock().remove(host_id)
    }

    pub fn host_ids(&self) -> Vec<String> {
        self.clients.lock().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_client() -> (Arc<MockOutboundTransport>, Arc<OutboundClient>) {
        let mock = Arc::new(MockOutboundTransport::new());
        mock.preset_list(&[
            RemoteSessionInfo {
                id: "main".into(),
                title: "shell".into(),
            },
            RemoteSessionInfo {
                id: "agent-2".into(),
                title: "agent".into(),
            },
        ]);
        let client = Arc::new(OutboundClient::new("lan:127.0.0.1:7522", mock.clone()));
        (mock, client)
    }

    #[test]
    fn hello_list_subscribe_write_resize_detach() {
        let (mock, client) = mock_client();
        let list = client.connect_and_list().expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(client.state(), OutboundState::Listed);
        client.subscribe("main").unwrap();
        assert!(client.is_subscribed("main"));
        client.write_pty("main", b"echo hi\n").unwrap();
        client.resize_pane("main", 40, 120).unwrap();
        client.unsubscribe("main").unwrap();
        assert!(!client.is_subscribed("main"));
        assert_eq!(client.state(), OutboundState::Listed);

        let log = mock.rpc_log.lock().clone();
        let methods: Vec<_> = log.iter().map(|(m, _)| m.as_str()).collect();
        assert!(methods.contains(&"$/hello"));
        assert!(methods.contains(&"list_panes"));
        assert!(methods.contains(&"subscribe-pane"));
        assert!(methods.contains(&"write_to_pty"));
        assert!(methods.contains(&"resize_pane"));
        assert!(methods.contains(&"unsubscribe-pane"));
        assert_eq!(client.stats.write_ok.load(Ordering::SeqCst), 1);
        assert_eq!(client.stats.resize_ok.load(Ordering::SeqCst), 1);
        assert_eq!(client.stats.unsubscribe_ok.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resize_coalesces_same_dimensions_and_resets_after_reconnect() {
        let (mock, client) = mock_client();
        client.connect_and_list().unwrap();
        client.subscribe("main").unwrap();

        client.resize_pane("main", 24, 80).unwrap();
        client.resize_pane("main", 24, 80).unwrap();
        client.resize_pane("main", 30, 100).unwrap();

        let resize_calls = mock
            .rpc_log
            .lock()
            .iter()
            .filter(|(method, _)| method == "resize_pane")
            .count();
        assert_eq!(resize_calls, 2);
        assert_eq!(client.stats.resize_ok.load(Ordering::SeqCst), 2);
        assert_eq!(client.stats.resize_suppressed.load(Ordering::SeqCst), 1);

        client.mark_disconnected();
        client.reconnect_resubscribe(&["main".into()]).unwrap();
        client.resize_pane("main", 30, 100).unwrap();
        let resize_calls_after_reconnect = mock
            .rpc_log
            .lock()
            .iter()
            .filter(|(method, _)| method == "resize_pane")
            .count();
        assert_eq!(resize_calls_after_reconnect, 3);
    }

    #[test]
    fn failed_resize_does_not_poison_duplicate_suppression_cache() {
        let (mock, client) = mock_client();
        client.connect_and_list().unwrap();
        client.subscribe("main").unwrap();
        *mock.fail_next.lock() = Some("resize transport failed".into());

        assert!(client.resize_pane("main", 24, 80).is_err());
        client.resize_pane("main", 24, 80).unwrap();

        assert_eq!(client.stats.resize_ok.load(Ordering::SeqCst), 1);
        assert_eq!(client.stats.resize_suppressed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn pump_output_counts_fanout_bytes() {
        let (mock, client) = mock_client();
        client.connect_and_list().unwrap();
        client.subscribe("main").unwrap();
        mock.inject_pane_output("main", b"hello");
        mock.inject_pane_output("main", b" world");
        let frames = client.pump_output();
        assert_eq!(frames.len(), 2);
        assert_eq!(client.stats.fanout_bytes.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn reconnect_resubscribe_no_double_sub_flag() {
        let (_mock, client) = mock_client();
        client.connect_and_list().unwrap();
        client.subscribe("main").unwrap();
        client.mark_disconnected();
        assert!(!client.is_subscribed("main"));
        client
            .reconnect_resubscribe(&["main".into()])
            .expect("resub");
        assert!(client.is_subscribed("main"));
        assert_eq!(client.stats.reconnect_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(client.stats.resubscribe_ok.load(Ordering::SeqCst), 1);
        // second resubscribe still single true flag
        client.reconnect_resubscribe(&["main".into()]).unwrap();
        assert!(client.is_subscribed("main"));
    }

    #[test]
    fn write_without_subscribe_fails() {
        let (_mock, client) = mock_client();
        client.connect_and_list().unwrap();
        assert!(client.write_pty("main", b"x").is_err());
    }

    #[test]
    fn multi_host_registry_isolates_clients() {
        let reg = OutboundRegistry::default();
        let m1 = Arc::new(MockOutboundTransport::new());
        m1.preset_list(&[RemoteSessionInfo {
            id: "a".into(),
            title: "a".into(),
        }]);
        let m2 = Arc::new(MockOutboundTransport::new());
        m2.preset_list(&[RemoteSessionInfo {
            id: "b".into(),
            title: "b".into(),
        }]);
        let c1 = Arc::new(OutboundClient::new("lan:h1", m1));
        let c2 = Arc::new(OutboundClient::new("lan:h2", m2));
        reg.insert(c1.clone());
        reg.insert(c2.clone());
        c1.connect_and_list().unwrap();
        c2.connect_and_list().unwrap();
        c1.subscribe("a").unwrap();
        assert!(c1.is_subscribed("a"));
        assert!(!c2.is_subscribed("a"));
        assert_eq!(reg.host_ids().len(), 2);
        reg.remove("lan:h1");
        assert!(reg.get("lan:h1").is_none());
        assert!(reg.get("lan:h2").is_some());
    }

    #[test]
    fn append_capped_drops_head() {
        let mut buf = Vec::new();
        let drop1 = append_capped(&mut buf, b"abcdef", 4);
        assert_eq!(buf, b"cdef");
        assert_eq!(drop1, 2);
        let drop2 = append_capped(&mut buf, b"xy", 4);
        assert_eq!(buf, b"efxy");
        assert_eq!(drop2, 2);
    }

    #[test]
    fn wire_pane_roundtrip() {
        let frame = wire::encode_pane("p1", b"hi").unwrap();
        let (id, bytes) = wire::demux_pane(&frame).unwrap();
        assert_eq!(id, "p1");
        assert_eq!(bytes, b"hi");
    }

    #[test]
    fn reconnect_delay_uses_shared_policy_bounds() {
        assert_eq!(OutboundClient::reconnect_delay_ms(0), Some(200));
        assert_eq!(OutboundClient::reconnect_delay_ms(1), Some(400));
        assert_eq!(OutboundClient::reconnect_delay_ms(2), Some(800));
        assert_eq!(OutboundClient::reconnect_delay_ms(3), Some(1600));
        assert_eq!(OutboundClient::reconnect_delay_ms(4), None);
    }
}
