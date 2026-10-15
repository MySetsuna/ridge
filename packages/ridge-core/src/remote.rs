//! Runtime-agnostic remote-host topology records.
//!
//! Hosts own transport and UI projection; this module owns the serializable
//! domain shape shared by every host surface. Credentials are deliberately
//! excluded from [`HostRecord`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Remote,
    Rdg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSessionMeta {
    pub id: String,
    pub title: String,
    pub attached: bool,
}

/// Registered topology only. No token or TOTP secret belongs here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRecord {
    pub id: String,
    pub kind: HostKind,
    pub label: String,
    pub addr: String,
    pub status: HostStatus,
    pub detail: String,
    pub sessions: Vec<HostSessionMeta>,
}

/// Pure remote-host topology aggregate. Transport and UI remain adapters.
#[derive(Clone, Debug, Default)]
pub struct RemoteHostTopology {
    hosts: HashMap<String, HostRecord>,
}

impl RemoteHostTopology {
    pub fn from_records(hosts: HashMap<String, HostRecord>) -> Self {
        Self { hosts }
    }

    pub fn records(&self) -> &HashMap<String, HostRecord> {
        &self.hosts
    }

    pub fn snapshot(&self) -> Vec<HostRecord> {
        let mut hosts = self.hosts.values().cloned().collect::<Vec<_>>();
        hosts.sort_by(|a, b| a.label.cmp(&b.label));
        hosts
    }

    pub fn get(&self, id: &str) -> Option<HostRecord> {
        self.hosts.get(id).cloned()
    }

    pub fn upsert(&mut self, host: HostRecord) {
        self.hosts.insert(host.id.clone(), host);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.hosts.remove(id).is_some()
    }
}

/// Parse `host:port` (or bare host → default 443 for https-style, 7522 for rdg heuristic).
pub fn parse_host_port(addr: &str, kind: HostKind) -> Result<(String, u16), String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err("地址不能为空".into());
    }
    if addr.contains('@') { return Err("host address must not contain credentials".into()); }
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

/// Inherited pane origin wire contract, shared by every Runtime presentation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneOriginDto {
    Headless {
        host_id: String,
        host_label: String,
        session_id: String,
    },
    Remote {
        host_id: String,
        host_label: String,
        session_id: String,
    },
    Rdg {
        host_id: String,
        host_label: String,
        session_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_origin_preserves_inherited_tags_and_snake_case_fields() {
        for (tag, origin) in [
            ("headless", PaneOriginDto::Headless { host_id: "host".into(), host_label: "设备".into(), session_id: "socket:42".into() }),
            ("remote", PaneOriginDto::Remote { host_id: "host".into(), host_label: "设备".into(), session_id: "socket:42".into() }),
            ("rdg", PaneOriginDto::Rdg { host_id: "host".into(), host_label: "设备".into(), session_id: "socket:42".into() }),
        ] {
            let encoded = serde_json::to_string(&origin).unwrap();
            assert_eq!(encoded, format!(r#"{{"kind":"{tag}","host_id":"host","host_label":"设备","session_id":"socket:42"}}"#));
            assert_eq!(serde_json::from_str::<PaneOriginDto>(&encoded).unwrap(), origin);
        }
        assert!(serde_json::from_str::<PaneOriginDto>(r#"{"kind":"local"}"#).is_err());
    }

    #[test]
    fn host_record_serializes_public_topology_only() {
        let host = HostRecord {
            id: "host-a".into(),
            kind: HostKind::Remote,
            label: "A".into(),
            addr: "127.0.0.1:9900".into(),
            status: HostStatus::Connected,
            detail: "live".into(),
            sessions: vec![],
        };
        assert_eq!(serde_json::to_value(host).unwrap()["status"], "connected");
    }

    #[test]
    fn topology_snapshots_in_label_order() {
        let mut topology = RemoteHostTopology::default();
        for label in ["B", "A"] {
            topology.upsert(HostRecord {
                id: label.into(),
                kind: HostKind::Remote,
                label: label.into(),
                addr: String::new(),
                status: HostStatus::Disconnected,
                detail: String::new(),
                sessions: vec![],
            });
        }
        assert_eq!(topology.snapshot()[0].label, "A");
    }
}
