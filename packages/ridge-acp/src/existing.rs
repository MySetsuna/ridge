//! ACP operations for an already-running, already-authenticated endpoint.
//! Runtime must separately verify process/endpoint/session binding before
//! publishing a Commune member. Only explicit `create` calls session/new.
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
        Arc, Mutex,
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
    history_replayed: bool,
    replay_info: Option<Value>,
}
struct SessionUpdates {
    session_id: Option<String>,
    pending: Vec<Value>,
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

async fn initialize(transport: &AcpTransport) -> Result<Value, AcpErrorWrap> {
    let initialized = result(transport.call("initialize", json!({
        "protocolVersion": 1,
        "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
        "clientInfo": {"name": "ridge", "version": env!("CARGO_PKG_VERSION")}
    })).await?)?;
    if initialized["protocolVersion"] != 1 || !initialized["agentCapabilities"].is_object() {
        return Err(failure("ACP version/capability mismatch"));
    }
    Ok(initialized)
}

/// Inspect existing conversations without loading or creating one.
pub async fn list_sessions(
    transport: &AcpTransport,
    cwd: &str,
) -> Result<Vec<Value>, AcpErrorWrap> {
    let initialized = initialize(transport).await?;
    if !initialized["agentCapabilities"]["sessionCapabilities"]["list"].is_object() {
        return Err(failure("Agent did not advertise session listing"));
    }
    let mut cursor = None::<String>;
    let mut seen = std::collections::HashSet::new();
    let mut sessions = Vec::new();
    for _ in 0..8 {
        let mut params = json!({"cwd": cwd});
        if let Some(cursor) = &cursor {
            params["cursor"] = json!(cursor);
        }
        let page = result(transport.call("session/list", params).await?)?;
        let rows = page["sessions"]
            .as_array()
            .ok_or_else(|| failure("invalid ACP session page"))?;
        if seen.len() + rows.len() > 128 {
            return Err(failure(
                "ACP session listing exceeds the 128-session budget",
            ));
        }
        for row in rows {
            let id = row["sessionId"]
                .as_str()
                .filter(|id| !id.trim().is_empty() && id.len() <= 512)
                .ok_or_else(|| failure("invalid ACP session identity"))?;
            if !seen.insert(id.to_owned()) {
                return Err(failure("duplicate ACP session identity"));
            }
            if row["cwd"] == cwd {
                sessions.push(json!({"sessionId": id, "cwd": cwd,
                    "title": row["title"].as_str().map(|title| title.chars().take(160).collect::<String>())}));
            }
        }
        cursor = match &page["nextCursor"] {
            Value::Null => return Ok(sessions),
            Value::String(value)
                if !value.is_empty() && value.len() <= 4096 && cursor.as_ref() != Some(value) =>
            {
                Some(value.clone())
            }
            _ => return Err(failure("invalid ACP session cursor")),
        };
    }
    Err(failure("ACP session listing exceeds the page budget"))
}

impl ExistingConversation {
    pub async fn load(
        transport: AcpTransport,
        session_id: String,
        cwd: String,
        updates: UpdateSink,
        permission: PermissionHandler,
    ) -> Result<Self, AcpErrorWrap> {
        Self::open(transport,Some(session_id),cwd,updates,permission,None).await
    }

    /// The caller must have explicit user authorization to create a conversation.
    /// This operation is never used by discovery, history reads or attachment retries.
    pub async fn create(
        transport: AcpTransport,
        cwd: String,
        updates: UpdateSink,
        permission: PermissionHandler,
    ) -> Result<Self, AcpErrorWrap> {
        Self::open(transport,None,cwd,updates,permission,None).await
    }

    /// The caller explicitly selects one authentication method advertised by
    /// this Agent. Authentication failure never creates or retries a session.
    pub async fn create_authenticated(
        transport:AcpTransport,cwd:String,updates:UpdateSink,
        permission:PermissionHandler,method:String,
    )->Result<Self,AcpErrorWrap> {
        Self::open(transport,None,cwd,updates,permission,Some(method)).await
    }

    async fn open(
        transport: AcpTransport,
        requested_session: Option<String>,
        cwd: String,
        updates: UpdateSink,
        permission: PermissionHandler,
        authentication: Option<String>,
    ) -> Result<Self, AcpErrorWrap> {
        if requested_session.as_ref().is_some_and(String::is_empty) || !std::path::Path::new(&cwd).is_absolute() {
            transport.shutdown();
            return Err(failure("existing sessionId and absolute cwd are required"));
        }
        let selected = Arc::new(Mutex::new(SessionUpdates { session_id: requested_session.clone(), pending: Vec::new() }));
        let observed = selected.clone();
        let deliver = updates.clone();
        transport.set_notification_sink(Arc::new(move |notification| {
            if notification.method == "session/update" {
                let params = notification
                    .params
                    .as_ref()
                    .ok_or_else(|| failure("session/update missing params"))?;
                let id = params["sessionId"]
                    .as_str()
                    .ok_or_else(|| failure("session/update missing sessionId"))?;
                if !params["update"].is_object() {
                    return Err(failure("session/update missing update"));
                }
                let mut state = observed.lock().map_err(|_| failure("ACP session selection unavailable"))?;
                if let Some(selected) = &state.session_id {
                    if id == selected { deliver(params)?; }
                } else {
                    if state.pending.len() >= 512 { return Err(failure("ACP creation update buffer exhausted")); }
                    state.pending.push(params.clone());
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
            let selected = selected.clone();
            async move {
                while let Some(request) = requests.recv().await {
                    cancellation.borrow_and_update();
                    let params = request.params.unwrap_or(Value::Null);
                    let response = if request.method != "session/request_permission" {
                        Err(rpc_error(-32601, "client method not supported"))
                    } else if !selected.lock().is_ok_and(|state| state.session_id.as_deref().is_some_and(|id| params["sessionId"].as_str() == Some(id)))
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
        let mut creation_requested = false;
        let handshake = async {
            let initialized = initialize(&transport).await?;
            if let Some(method)=&authentication {
                if !initialized["authMethods"].as_array().is_some_and(|methods|methods.iter().any(|offered|offered["id"]==*method&&offered["type"]!="terminal")) {
                    return Err(failure("selected ACP authentication method is not offered for protocol authentication"));
                }
                result(transport.call("authenticate",json!({"methodId":method})).await?)?;
            }
            let Some(session_id) = &requested_session else {
                creation_requested = true;
                let created=result(transport.call("session/new",json!({"cwd":cwd,"mcpServers":[]})).await?).map_err(|error| {
                    if matches!(&error,AcpErrorWrap::Rpc(-32000,message) if message.to_ascii_lowercase().contains("auth")) {
                        let methods = initialized["authMethods"].as_array().into_iter().flatten()
                            .filter(|method| method["type"] != "terminal")
                            .filter_map(|method| method["id"].as_str()).take(16)
                            .collect::<Vec<_>>().join(", ");
                        let mut recovery = format!("{error}; sign in using the Agent's own CLI, then start a new collaboration terminal");
                        if !methods.is_empty() {
                            recovery.push_str(&format!(", or relaunch with an explicit --auth-method from: {methods}"));
                        }
                        failure(&recovery)
                    }else{error}
                })?;
                let session_id=created["sessionId"].as_str().filter(|id|!id.is_empty()).ok_or_else(||failure("session/new response missing session identity"))?.to_owned();
                let mut state=selected.lock().map_err(|_|failure("ACP session selection unavailable"))?;
                state.session_id=Some(session_id.clone());
                for update in std::mem::take(&mut state.pending) {
                    if update["sessionId"] == session_id { updates(&update)?; }
                }
                return Ok((initialized,session_id,false,None));
            };
            let replay = initialized["agentCapabilities"]["loadSession"] == true;
            let method = if replay {
                "session/load"
            } else if initialized["agentCapabilities"]["sessionCapabilities"]["resume"].is_object()
            {
                "session/resume"
            } else {
                return Err(failure(
                    "Agent cannot load or resume the existing conversation",
                ));
            };
            let loaded = result(
                transport
                    .call(
                        method,
                        json!({"sessionId":session_id,"cwd":cwd,"mcpServers":[]}),
                    )
                    .await?,
            )?;
            if !loaded.is_object() {
                return Err(failure("invalid existing-session response"));
            }
            Ok((initialized, session_id.clone(), replay, loaded.get("_meta").cloned()))
        };
        let (initialized, session_id, history_replayed, replay_info) =
            match tokio::time::timeout(Duration::from_secs(30), handshake).await {
                Ok(Ok(value)) => value,
                outcome => {
                    transport.shutdown();
                    return Err(match outcome {
                        Ok(Err(error)) => error,
                        _ => AcpErrorWrap::Transport(
                            if requested_session.is_none() && authentication.is_some() && !creation_requested { "ACP authentication timed out; no new session requested" }
                            else if requested_session.is_some() { "existing ACP session load timed out; no new session created" }
                            else { "ACP session creation timed out; result unknown and not retried" }.into(),
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
                history_replayed,
                replay_info,
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
    pub fn history_replayed(&self) -> bool {
        self.inner.history_replayed
    }
    pub fn replay_info(&self) -> Option<&Value> { self.inner.replay_info.as_ref() }
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

    /// Fence new prompt admission while committing explicit, idle membership retirement.
    pub fn retire_idle(&self,commit:impl FnOnce()->Result<(),String>)->Result<(),String>{
        if self.inner.transport.has_peer_requests()||self.inner.busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return Err("Agent turn is active; wait for completion or cancel it before removal".into());}
        match commit(){Ok(())=>{self.disconnect();Ok(())},Err(error)=>{self.inner.busy.store(false,Ordering::Release);Err(error)}}
    }

    /// Caller cancellation drops only its receiver; the owned request continues
    /// tracking the actual Agent response. No automatic prompt retry.
    pub fn prompt(
        &self,
        text: String,
    ) -> Result<oneshot::Receiver<Result<Value, AcpErrorWrap>>, AcpErrorWrap> {
        self.prompt_observed(text,None)
    }
    pub fn prompt_observed(&self, text: String, completion: Option<crate::transport::CallCompletion>) -> Result<oneshot::Receiver<Result<Value,AcpErrorWrap>>,AcpErrorWrap> {
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
        let pending = match self.inner.transport.begin_call_observed(
            "session/prompt",
            json!({"sessionId":self.inner.session_id,"prompt":[{"type":"text","text":text}]}),
            completion,
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
