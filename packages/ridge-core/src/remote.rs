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
    pub fn from_records(hosts: HashMap<String, HostRecord>) -> Self { Self { hosts } }

    pub fn records(&self) -> &HashMap<String, HostRecord> { &self.hosts }

    pub fn snapshot(&self) -> Vec<HostRecord> {
        let mut hosts = self.hosts.values().cloned().collect::<Vec<_>>();
        hosts.sort_by(|a, b| a.label.cmp(&b.label));
        hosts
    }

    pub fn get(&self, id: &str) -> Option<HostRecord> { self.hosts.get(id).cloned() }

    pub fn upsert(&mut self, host: HostRecord) { self.hosts.insert(host.id.clone(), host); }

    pub fn remove(&mut self, id: &str) -> bool { self.hosts.remove(id).is_some() }
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

    #[test]
    fn topology_snapshots_in_label_order() {
        let mut topology = RemoteHostTopology::default();
        for label in ["B", "A"] {
            topology.upsert(HostRecord {
                id: label.into(), kind: HostKind::Remote, label: label.into(), addr: String::new(),
                status: HostStatus::Disconnected, detail: String::new(), sessions: vec![],
            });
        }
        assert_eq!(topology.snapshot()[0].label, "A");
    }
}
