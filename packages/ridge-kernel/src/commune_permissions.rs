//! Runtime-owned ACP permission requests and their durable conversation ledger.
use crate::business::BusinessEvents;
use ridge_core::EventSink;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;
use uuid::Uuid;

pub struct HistoryLog(Mutex<(File, u64, bool, u64)>);
impl HistoryLog {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut sequence = 0;
        if path.exists() {
            for line in BufReader::new(File::open(path).map_err(|e| e.to_string())?).lines() {
                let record: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                sequence = (sequence + 1).max(record["sequence"].as_u64().unwrap_or(0));
            }
            let mut file = File::open(path).map_err(|e| e.to_string())?;
            if file.metadata().map_err(|e| e.to_string())?.len() > 0 {
                file.seek(SeekFrom::End(-1)).map_err(|e| e.to_string())?;
                let mut tail = [0];
                file.read_exact(&mut tail).map_err(|e| e.to_string())?;
                if tail != [b'\n'] {
                    return Err(
                        "conversation ledger has an incomplete tail; preserved for recovery".into(),
                    );
                }
            }
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        Ok(Self(Mutex::new((file, sequence, false, length))))
    }
    pub fn append(&self, mut event: Value) -> Result<Value, String> {
        let mut log = self
            .0
            .lock()
            .map_err(|_| "conversation ledger unavailable")?;
        if log.2 {
            return Err("conversation ledger write outcome unknown; recovery required".into());
        }
        let sequence = log
            .1
            .checked_add(1)
            .ok_or("conversation sequence exhausted")?;
        event["sequence"] = json!(sequence);
        event["timestamp"] = json!(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as u64);
        log.2 = true;
        writeln!(log.0, "{event}").map_err(|e| e.to_string())?;
        log.0.sync_data().map_err(|e| e.to_string())?;
        log.3 = log.0.metadata().map_err(|e| e.to_string())?.len();
        log.1 = sequence;
        log.2 = false;
        Ok(event)
    }
}

#[derive(Clone)]
pub struct ApprovalBinding {
    pub agent_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub session_id: String,
    pub generation: u64,
    pub lease: String,
}
struct Pending {
    binding: ApprovalBinding,
    request: Value,
    nonce: String,
    sender: oneshot::Sender<String>,
    log: Arc<HistoryLog>,
}
#[derive(Default)]
pub struct Permissions(
    Mutex<HashMap<String, Pending>>,
    Mutex<HashMap<String, std::sync::Weak<HistoryLog>>>,
);
impl Permissions {
    /// Snapshot committed bytes only; history reads never create files or writers.
    pub fn history_length(&self, agent_id: &str, path: &Path) -> Result<Option<u64>, String> {
        let writer = self
            .1
            .lock()
            .map_err(|_| "conversation writers unavailable")?
            .get(agent_id)
            .and_then(std::sync::Weak::upgrade);
        if let Some(writer) = writer {
            return Ok(Some(
                writer
                    .0
                    .lock()
                    .map_err(|_| "conversation ledger unavailable")?
                    .3,
            ));
        }
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(Some(metadata.len())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn history(&self, agent_id: &str, path: &Path) -> Result<Arc<HistoryLog>, String> {
        let mut logs = self
            .1
            .lock()
            .map_err(|_| "conversation writers unavailable")?;
        logs.retain(|_, log| log.strong_count() > 0);
        if let Some(log) = logs.get(agent_id).and_then(std::sync::Weak::upgrade) {
            return Ok(log);
        }
        let log = Arc::new(HistoryLog::open(path)?);
        logs.insert(agent_id.into(), Arc::downgrade(&log));
        Ok(log)
    }
    pub async fn request(
        self: Arc<Self>,
        binding: ApprovalBinding,
        params: Value,
        log: Arc<HistoryLog>,
        events: BusinessEvents,
    ) -> Option<String> {
        let id = Uuid::new_v4().to_string();
        let nonce = Uuid::new_v4().simple().to_string();
        let (sender, receiver) = oneshot::channel();
        let inserted = (|| -> Result<Value, String> {
            let mut pending = self
                .0
                .lock()
                .map_err(|_| "permission registry unavailable")?;
            if pending.len() >= 256 {
                return Err("permission queue full".into());
            }
            let record = log.append(json!({"kind":"permission_requested","id":id,"agentId":binding.agent_id,"workspaceId":binding.workspace_id,"sessionId":binding.session_id,"params":params}))?;
            let title = params["toolCall"]["title"]
                .as_str()
                .or(params["toolCall"]["toolCallId"].as_str())
                .unwrap_or("ACP tool call");
            let action = if params["toolCall"]["rawInput"].is_null() {
                title.to_owned()
            } else {
                format!(
                    "{title}\n{}",
                    serde_json::to_string_pretty(&params["toolCall"]["rawInput"])
                        .map_err(|e| e.to_string())?
                )
            };
            let request = json!({"id":id,"initiator":binding.pane_id,"agentId":binding.agent_id,"workspaceId":binding.workspace_id,
                "sessionId":binding.session_id,"generation":binding.generation,"lease":binding.lease,
                "kind":"approval","protocol":"acp","executor":"ACP Agent","level":"Dangerous","reason":"Agent 请求工具权限",
                "action":action,
                "toolCall":params["toolCall"],"options":params["options"],"createdAt":record["timestamp"],"resolutionNonce":nonce});
            pending.insert(
                id.clone(),
                Pending {
                    binding,
                    request: request.clone(),
                    nonce,
                    sender,
                    log,
                },
            );
            Ok(request)
        })();
        let request = match inserted {
            Ok(request) => request,
            Err(error) => {
                tracing::error!(%id,%error,"permission not exposed without durable acceptance");
                return None;
            }
        };
        let _waiting = Waiting {
            permissions: self.clone(),
            id,
            events: events.clone(),
        };
        events.broadcast("teammate://hitl-approval-required", request);
        events.broadcast("teammate://hitl-pending-changed", Value::Null);
        receiver.await.ok()
    }
    pub fn snapshot(&self, local: bool) -> Result<Value, String> {
        let pending = self
            .0
            .lock()
            .map_err(|_| "permission registry unavailable")?;
        let mut rows = pending.values().map(|entry| {
            if local { entry.request.clone() }
            else { json!({"id":entry.request["id"],"initiator":entry.binding.pane_id,"workspaceId":entry.binding.workspace_id,
                "level":"Dangerous","reason":"Agent 请求工具权限","createdAt":entry.request["createdAt"],"resolutionNonce":entry.nonce}) }
        }).collect::<Vec<_>>();
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Ok(json!(rows))
    }
    pub fn binding(&self, id: &str) -> Option<ApprovalBinding> {
        self.0
            .lock()
            .ok()?
            .get(id)
            .map(|entry| entry.binding.clone())
    }
    pub fn resolve(
        &self,
        id: &str,
        option: Option<&str>,
        verdict: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<&'static str, String> {
        let mut pending = self
            .0
            .lock()
            .map_err(|_| "permission registry unavailable")?;
        let Some(entry) = pending.get(id) else {
            return Ok("already-resolved");
        };
        if let Some(nonce) = nonce {
            if nonce.len() != entry.nonce.len()
                || nonce
                    .as_bytes()
                    .iter()
                    .zip(entry.nonce.as_bytes())
                    .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                    != 0
            {
                return Ok("nonce-mismatch");
            }
        }
        let options = entry.request["options"]
            .as_array()
            .ok_or("invalid offered permission options")?;
        let option = if let Some(option) = option {
            if nonce.is_some()
                || !options
                    .iter()
                    .any(|offered| offered["optionId"].as_str() == Some(option))
            {
                return Ok("bad-verdict");
            }
            option.to_owned()
        } else {
            let kind = match verdict {
                Some("approve") => "allow_once",
                Some("reject") => "reject_once",
                _ => return Ok("bad-verdict"),
            };
            let matching = options
                .iter()
                .filter(|offered| offered["kind"] == kind)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Ok("bad-verdict");
            }
            matching[0]["optionId"]
                .as_str()
                .ok_or("permission option has no ID")?
                .to_owned()
        };
        if entry.sender.is_closed() {
            entry.log.append(json!({"kind":"permission_cancelled","id":id,"agentId":entry.binding.agent_id,"sessionId":entry.binding.session_id}))?;
            pending.remove(id);
            return Ok("already-resolved");
        }
        entry.log.append(json!({"kind":"permission_decided","id":id,"agentId":entry.binding.agent_id,"sessionId":entry.binding.session_id,"optionId":option}))?;
        let entry = pending.remove(id).expect("checked under lock");
        Ok(if entry.sender.send(option).is_ok() {
            "consumed"
        } else {
            "already-resolved"
        })
    }
}
struct Waiting {
    permissions: Arc<Permissions>,
    id: String,
    events: BusinessEvents,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.permissions.0.lock() {
            if let Some(entry) = pending.remove(&self.id) {
                if let Err(error) = entry.log.append(json!({"kind":"permission_cancelled","id":self.id,"agentId":entry.binding.agent_id,"sessionId":entry.binding.session_id})) {
                    tracing::error!(%error,"cancelled permission could not be persisted");
                }
            }
        }
        self.events
            .broadcast("teammate://hitl-pending-changed", Value::Null);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        Arc<Permissions>,
        Arc<HistoryLog>,
        std::path::PathBuf,
        ApprovalBinding,
    ) {
        let path = std::env::temp_dir().join(format!("ridge-permissions-{}.jsonl", Uuid::new_v4()));
        (
            Arc::new(Permissions::default()),
            Arc::new(HistoryLog::open(&path).unwrap()),
            path,
            ApprovalBinding {
                agent_id: "agent".into(),
                workspace_id: "workspace".into(),
                pane_id: "pane".into(),
                session_id: "existing-session".into(),
                generation: 2,
                lease: "lease-2".into(),
            },
        )
    }
    fn params() -> Value {
        json!({"sessionId":"existing-session","toolCall":{"toolCallId":"tool","title":"secret-operation","rawInput":{"secret":"private-argument"}},
            "options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"always","name":"Allow always","kind":"allow_always"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]})
    }
    async fn waiting(permissions: &Permissions) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = permissions.snapshot(true).unwrap();
                if !snapshot.as_array().unwrap().is_empty() {
                    return snapshot[0].clone();
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn actual_option_is_durable_single_use_and_remote_projection_is_sanitized() {
        let (permissions, log, path, binding) = fixture();
        let request = tokio::spawn(permissions.clone().request(
            binding,
            params(),
            log,
            BusinessEvents::default(),
        ));
        let pending = waiting(&permissions).await;
        let remote = permissions.snapshot(false).unwrap().to_string();
        for private in [
            "secret-operation",
            "private-argument",
            "options",
            "action",
            "toolCall",
            "lease-2",
        ] {
            assert!(!remote.contains(private));
        }
        let id = pending["id"].as_str().unwrap();
        assert_eq!(
            permissions
                .resolve(id, None, Some("approve"), Some("wrong"))
                .unwrap(),
            "nonce-mismatch"
        );
        assert_eq!(
            permissions
                .resolve(id, Some("made-up"), None, None)
                .unwrap(),
            "bad-verdict"
        );
        assert_eq!(
            permissions
                .snapshot(true)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let left = {
            let permissions = permissions.clone();
            let id = id.to_owned();
            std::thread::spawn(move || permissions.resolve(&id, Some("once"), None, None).unwrap())
        };
        let right = {
            let permissions = permissions.clone();
            let id = id.to_owned();
            let nonce = pending["resolutionNonce"].as_str().unwrap().to_owned();
            std::thread::spawn(move || {
                permissions
                    .resolve(&id, None, Some("approve"), Some(&nonce))
                    .unwrap()
            })
        };
        let outcomes = [left.join().unwrap(), right.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == "consumed")
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == "already-resolved")
                .count(),
            1
        );
        assert_eq!(request.await.unwrap(), Some("once".into()));
        let records = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["kind"], "permission_requested");
        assert_eq!(records[1]["kind"], "permission_decided");
        assert_eq!(records[1]["optionId"], "once");
        assert!(!records.iter().any(|record| record
            .to_string()
            .contains(pending["resolutionNonce"].as_str().unwrap())));
        let restored = HistoryLog::open(&path).unwrap();
        assert_eq!(
            restored.append(json!({"kind":"after-reopen"})).unwrap()["sequence"],
            3
        );
    }
    #[tokio::test]
    async fn cancelled_handler_retires_approval_and_stale_choices_cannot_resume_it() {
        let (permissions, log, path, binding) = fixture();
        let request = tokio::spawn(permissions.clone().request(
            binding,
            params(),
            log,
            BusinessEvents::default(),
        ));
        let pending = waiting(&permissions).await;
        request.abort();
        let _ = request.await;
        assert!(permissions
            .snapshot(true)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(
            permissions
                .resolve(pending["id"].as_str().unwrap(), Some("once"), None, None)
                .unwrap(),
            "already-resolved"
        );
        assert!(std::fs::read_to_string(path)
            .unwrap()
            .contains("permission_cancelled"));
    }
    #[test]
    fn reconnecting_consumers_share_one_live_ledger_writer() {
        let path =
            std::env::temp_dir().join(format!("ridge-reconnect-ledger-{}.jsonl", Uuid::new_v4()));
        let permissions = Permissions::default();
        let old = permissions.history("agent", &path).unwrap();
        let new = permissions.history("agent", &path).unwrap();
        assert!(Arc::ptr_eq(&old, &new));
        assert_eq!(
            old.append(json!({"kind":"old-connection-cancelled"}))
                .unwrap()["sequence"],
            1
        );
        assert_eq!(
            new.append(json!({"kind":"reconnected"})).unwrap()["sequence"],
            2
        );
        drop(old);
        drop(new);
        assert_eq!(
            permissions
                .history("agent", &path)
                .unwrap()
                .append(json!({"kind":"reopened"}))
                .unwrap()["sequence"],
            3
        );
    }
    #[test]
    fn partial_ledger_is_preserved_and_not_silently_appended_to() {
        let path =
            std::env::temp_dir().join(format!("ridge-partial-history-{}.jsonl", Uuid::new_v4()));
        let original = b"{\"sequence\":1,\"text\":\"partial\"}";
        std::fs::write(&path, original).unwrap();
        assert!(HistoryLog::open(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}
