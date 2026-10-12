//! ACP operations for an already-running, already-authenticated endpoint.
//! Runtime must separately verify process/endpoint/session binding before
//! publishing a Commune member. This driver never spawns or calls session/new.
use crate::{
    error::AcpErrorWrap,
    protocol::{AcpErrorObj, AcpResponse},
    transport::AcpTransport,
};
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

pub type UpdateSink = Arc<dyn Fn(&Value) -> Result<(), AcpErrorWrap> + Send + Sync>;
pub type PermissionHandler =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;
pub struct ExistingConversation {
    inner: Arc<Inner>,
}
struct Inner {
    transport: AcpTransport,
    session_id: String,
    busy: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    cancellation: watch::Sender<u64>,
    permissions: JoinHandle<()>,
    agent_info: Option<Value>,
    capabilities: Value,
}
struct PendingLoad {
    transport: AcpTransport,
    permissions: Option<JoinHandle<()>>,
}
impl Drop for PendingLoad {
    fn drop(&mut self) {
        if let Some(task) = self.permissions.take() {
            task.abort();
            self.transport.shutdown();
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.permissions.abort();
        self.transport.shutdown();
    }
}
fn failure(message: &str) -> AcpErrorWrap {
    AcpErrorWrap::InvalidFrame(message.into())
}
fn result(response: AcpResponse) -> Result<Value, AcpErrorWrap> {
    if response.jsonrpc != "2.0" || response.result.is_some() == response.error.is_some() {
        return Err(failure("invalid ACP response envelope"));
    }
    if let Some(error) = response.error {
        return Err(error.into());
    }
    Ok(response.result.expect("result validated"))
}
fn rpc_error(code: i64, message: &str) -> AcpErrorObj {
    AcpErrorObj {
        code,
        message: message.into(),
        data: None,
    }
}

impl ExistingConversation {
    pub async fn load(
        transport: AcpTransport,
        session_id: String,
        cwd: String,
        updates: UpdateSink,
        permission: PermissionHandler,
    ) -> Result<Self, AcpErrorWrap> {
        if session_id.is_empty() || !std::path::Path::new(&cwd).is_absolute() {
            transport.shutdown();
            return Err(failure("existing sessionId and absolute cwd are required"));
        }
        let selected = session_id.clone();
        transport.set_notification_sink(Arc::new(move |notification| {
            if notification.method == "session/update" {
                let params = notification
                    .params
                    .as_ref()
                    .ok_or_else(|| failure("session/update missing params"))?;
                let id = params["sessionId"]
                    .as_str()
                    .ok_or_else(|| failure("session/update missing sessionId"))?;
                if id == selected {
                    if !params["update"].is_object() {
                        return Err(failure("session/update missing update"));
                    }
                    updates(params)?;
                }
            }
            Ok(())
        }))?;
        let mut requests = transport
            .take_peer_requests()
            .ok_or_else(|| failure("ACP endpoint already has a request owner"))?;
        let busy = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (cancellation, _) = watch::channel(0u64);
        let permissions = tokio::spawn({
            let transport = transport.clone();
            let busy = busy.clone();
            let cancelled = cancelled.clone();
            let mut cancellation = cancellation.subscribe();
            let selected = session_id.clone();
            async move {
                while let Some(request) = requests.recv().await {
                    cancellation.borrow_and_update();
                    let params = request.params.unwrap_or(Value::Null);
                    let response = if request.method != "session/request_permission" {
                        Err(rpc_error(-32601, "client method not supported"))
                    } else if params["sessionId"].as_str() != Some(&selected)
                        || params["toolCall"]["toolCallId"]
                            .as_str()
                            .is_none_or(str::is_empty)
                        || !params["options"].is_array()
                        || params["options"].as_array().is_some_and(|options| {
                            let mut ids = std::collections::HashSet::new();
                            options.is_empty()
                                || options.len() > 256
                                || options.iter().any(|option| {
                                    option["optionId"]
                                        .as_str()
                                        .is_none_or(|id| id.is_empty() || !ids.insert(id))
                                        || option["name"].as_str().is_none_or(str::is_empty)
                                        || !option["kind"].is_string()
                                })
                        })
                    {
                        Err(rpc_error(-32602, "invalid permission request"))
                    } else {
                        let options = params["options"]
                            .as_array()
                            .expect("options validated")
                            .clone();
                        let choice = if !busy.load(Ordering::Acquire)
                            || cancelled.load(Ordering::Acquire)
                        {
                            None
                        } else {
                            tokio::select! {biased; _=cancellation.changed()=>None, choice=permission(params)=>choice}
                        };
                        match choice {
                            Some(option)
                                if options
                                    .iter()
                                    .any(|item| item["optionId"].as_str() == Some(&option)) =>
                            {
                                Ok(json!({"outcome":{"outcome":"selected","optionId":option}}))
                            }
                            Some(_) => {
                                Err(rpc_error(-32602, "permission option not offered by Agent"))
                            }
                            None => Ok(json!({"outcome":{"outcome":"cancelled"}})),
                        }
                    };
                    if transport.respond(&request.id, response).is_err() {
                        transport.shutdown();
                        break;
                    }
                }
            }
        });
        let mut pending_load = PendingLoad {
            transport: transport.clone(),
            permissions: Some(permissions),
        };
        let handshake = async {
            let initialized=result(transport.call("initialize",json!({"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"ridge","version":env!("CARGO_PKG_VERSION")}})).await?)?;
            if initialized["protocolVersion"] != 1 || !initialized["agentCapabilities"].is_object()
            {
                return Err(failure("ACP version/capability mismatch"));
            }
            if initialized["agentCapabilities"]["loadSession"] != true {
                return Err(failure("Agent cannot load the existing conversation"));
            }
            let loaded = result(
                transport
                    .call(
                        "session/load",
                        json!({"sessionId":session_id,"cwd":cwd,"mcpServers":[]}),
                    )
                    .await?,
            )?;
            if !loaded.is_object() {
                return Err(failure("invalid session/load response"));
            }
            Ok(initialized)
        };
        let initialized = match tokio::time::timeout(Duration::from_secs(30), handshake).await {
            Ok(Ok(value)) => value,
            outcome => {
                transport.shutdown();
                return Err(match outcome {
                    Ok(Err(error)) => error,
                    _ => AcpErrorWrap::Transport(
                        "existing ACP session load timed out; no new session created".into(),
                    ),
                });
            }
        };
        Ok(Self {
            inner: Arc::new(Inner {
                transport,
                session_id,
                busy,
                cancelled,
                cancellation,
                permissions: pending_load
                    .permissions
                    .take()
                    .expect("load owns request handler"),
                agent_info: initialized.get("agentInfo").cloned(),
                capabilities: initialized["agentCapabilities"].clone(),
            }),
        })
    }
    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }
    pub fn agent_info(&self) -> Option<&Value> {
        self.inner.agent_info.as_ref()
    }
    pub fn capabilities(&self) -> &Value {
        &self.inner.capabilities
    }
    pub fn is_busy(&self) -> bool {
        self.inner.busy.load(Ordering::Acquire)
    }
    pub fn is_connected(&self) -> bool {
        !self.inner.transport.is_closed()
    }

    /// Retires this connection only. Runtime owns the external process lifetime.
    pub fn disconnect(&self) {
        self.inner.permissions.abort();
        self.inner.transport.shutdown();
    }

    /// Caller cancellation drops only its receiver; the owned request continues
    /// tracking the actual Agent response. No automatic prompt retry.
    pub fn prompt(
        &self,
        text: String,
    ) -> Result<oneshot::Receiver<Result<Value, AcpErrorWrap>>, AcpErrorWrap> {
        if self.inner.transport.is_closed() {
            return Err(AcpErrorWrap::SessionClosed);
        }
        if self.inner.transport.has_peer_requests()
            || self
                .inner
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(AcpErrorWrap::Transport(
                "existing conversation is busy".into(),
            ));
        }
        self.inner.cancelled.store(false, Ordering::Release);
        let pending = match self.inner.transport.begin_call(
            "session/prompt",
            json!({"sessionId":self.inner.session_id,"prompt":[{"type":"text","text":text}]}),
        ) {
            Ok(pending) => pending,
            Err(error) => {
                self.inner.busy.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let (send, receive) = oneshot::channel();
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let response = pending.wait().await.and_then(result).and_then(|value| {
                if !value["stopReason"].is_string() {
                    Err(failure("session/prompt missing actual stopReason"))
                } else {
                    Ok(value)
                }
            });
            if matches!(&response, Err(AcpErrorWrap::InvalidFrame(_))) {
                inner.transport.shutdown();
            }
            inner.busy.store(false, Ordering::Release);
            let _ = send.send(response);
        });
        Ok(receive)
    }
    pub async fn cancel(&self) -> Result<(), AcpErrorWrap> {
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner
            .cancellation
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        self.inner
            .transport
            .notify("session/cancel", json!({"sessionId":self.inner.session_id}))
            .await
    }
}
