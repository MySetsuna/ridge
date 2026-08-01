//! Runtime-agnostic remote-host topology records.
//!
//! Hosts own transport and UI projection; this module owns the serializable
//! domain shape shared by every host surface. Credentials are deliberately
//! excluded from [`HostRecord`].

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
