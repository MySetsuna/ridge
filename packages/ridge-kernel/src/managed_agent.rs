//! Explicit collaboration clients. Discovery never creates a protocol session.
pub use crate::commune_connection::Conversation as Session;
use ridge_acp::{
    existing::{ExistingConversation, PermissionHandler, UpdateSink},
    transport::AcpTransport,
};
use ridge_acp::{protocol::{AcpErrorObj, AUTH_REQUIRED}, AcpErrorWrap};
use ridge_core::process_identity::{self, ProcessInstance};
use serde_json::{json, Value};
use std::{collections::VecDeque, path::PathBuf, sync::Mutex, time::Duration};
use std::ffi::{OsStr, OsString};
use std::{path::Path, process::Stdio, sync::Arc};
use tokio::process::{Child, Command};

fn failure(message: impl Into<String>) -> AcpErrorWrap {
    AcpErrorWrap::Transport(message.into())
}

#[cfg(windows)]
fn launch_command(program: &Path) -> Result<Command, String> {
    use ridge_core::commands::shell::lookup_program;
    if program.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
    }) {
        return Ok(Command::new(program));
    }
    let name = program.to_str().ok_or("invalid executable path")?;
    if program.extension().is_none() {
        if let Some(executable) = lookup_program(&format!("{name}.exe")) {
            return Ok(Command::new(executable));
        }
    }
    let shim = if program.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("cmd")) {
        lookup_program(name)
    } else {
        lookup_program(&format!("{name}.cmd"))
    }.ok_or_else(|| format!("{name} is unavailable. Install its protocol adapter explicitly or supply --command with its executable"))?;
    let directory = shim.parent().ok_or("executable directory unavailable")?;
    let script =
        std::fs::read_to_string(&shim).map_err(|_| "could not read the installed command shim")?;
    if script.len() > 65536 || !script.contains("CALL :find_dp0") {
        return Err("use the executable or script entry for this custom command wrapper".into());
    }
    // Standard npm shims declare the actual entry in a quoted %dp0% path.
    // Invoke it directly so user arguments never enter a command shell.
    let entry = script
        .lines()
        .filter(|line| line.contains("%*"))
        .flat_map(|line| line.split('"').skip(1).step_by(2))
        .filter_map(|token| token.strip_prefix("%dp0%"))
        .map(|token| directory.join(token.trim_start_matches(['\\', '/'])))
        .filter(|entry| entry.is_file())
        .last()
        .ok_or("installed npm shim has no executable entry")?;
    if entry
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Ok(Command::new(entry));
    }
    if !entry.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("js")
            || extension.eq_ignore_ascii_case("mjs")
            || extension.eq_ignore_ascii_case("cjs")
    }) {
        return Err("unsupported npm command entry; supply its executable explicitly".into());
    }
    let local_node = directory.join("node.exe");
    let node = if local_node.is_file() {
        local_node
    } else {
        lookup_program("node.exe").ok_or("Node.js required by the installed protocol adapter")?
    };
    let mut command = Command::new(node);
    command.arg(entry);
    Ok(command)
}
#[cfg(not(windows))]
fn launch_command(program: &Path) -> Result<Command, String> {
    Ok(Command::new(program))
}

fn managed_no_proxy(existing: Option<&OsStr>) -> OsString {
    let mut bypass = existing.unwrap_or_default().to_os_string();
    if bypass.to_str().is_some_and(|value| value.trim() == "*") {
        return bypass;
    }
    for host in ["localhost", "127.0.0.1", "::1"] {
        if bypass.to_str().is_some_and(|value| {
            value.split(',').any(|entry| entry.trim().eq_ignore_ascii_case(host))
        }) {
            continue;
        }
        if !bypass.is_empty() {
            bypass.push(",");
        }
        bypass.push(host);
    }
    bypass
}

struct Peer {
    id: uuid::Uuid,
    transport: AcpTransport,
    updates: tokio::sync::mpsc::Sender<BridgeEvent>,
}
enum BridgeEvent {
    Update(Value),
    Reply(Value, Result<Value, AcpErrorObj>),
}
#[derive(Default)]
struct BridgeState {
    history: VecDeque<Value>,
    history_bytes: usize,
    truncated: bool,
    peer: Option<Peer>,
}

/// The terminal owns this handle. Disconnecting an external client does not
/// drop it or recreate its session. Dropping it terminates its owned process.
pub struct CollaborationTerminal {
    server: tokio::task::JoinHandle<()>,
    published: PublishedSession,
    agent: TerminalAgent,
}
enum TerminalAgent {
    Owned(StdioAgent),
    Shared { process: ProcessInstance, session: Arc<Session> },
}
impl TerminalAgent {
    fn process(&self) -> &ProcessInstance {
        match self {Self::Owned(agent) => agent.process(), Self::Shared {process, ..} => process}
    }
    fn session(&self) -> Arc<Session> {
        match self {Self::Owned(agent) => agent.session(), Self::Shared {session, ..} => session.clone()}
    }
}
impl Drop for TerminalAgent {
    fn drop(&mut self) {
        if let Self::Shared {session, ..} = self {session.disconnect();}
    }
}
enum AgentSource<'a> {
    Owned { program: &'a Path, arguments: &'a [String] },
    Shared { endpoint: &'a str, token_file: Option<&'a Path> },
}
impl Drop for CollaborationTerminal {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl CollaborationTerminal {
    pub async fn start(
        protocol: &str,
        program: &Path,
        arguments: &[String],
        cwd: &Path,
        updates: UpdateSink,
        local_permission: PermissionHandler,
        authentication: Option<String>,
    ) -> Result<Self, String> {
        Self::open(protocol, AgentSource::Owned {program, arguments}, cwd, updates, local_permission, authentication).await
    }
    /// Explicitly create this client's session on an existing, OS-attested
    /// daemon. No arbitrary session ID or ownership sidecar is accepted.
    pub async fn connect_shared(
        protocol: &str,
        endpoint: &str,
        token_file: Option<&Path>,
        cwd: &Path,
        updates: UpdateSink,
        local_permission: PermissionHandler,
        authentication: Option<String>,
    ) -> Result<Self, String> {
        Self::open(protocol, AgentSource::Shared {endpoint, token_file}, cwd, updates, local_permission, authentication).await
    }
    async fn open(
        protocol: &str,
        source: AgentSource<'_>,
        cwd: &Path,
        updates: UpdateSink,
        local_permission: PermissionHandler,
        authentication: Option<String>,
    ) -> Result<Self, String> {
        let state = Arc::new(Mutex::new(BridgeState::default()));
        let observed = state.clone();
        let routed = state.clone();
        let permission: PermissionHandler = Arc::new(move |params| {
            let peer = routed
                .lock()
                .ok()
                .and_then(|state| state.peer.as_ref().map(|peer| peer.transport.clone()));
            let local = local_permission.clone();
            Box::pin(async move {
                if let Some(peer) = peer {
                    let response = peer.call("session/request_permission", params).await.ok()?;
                    let result = response.result?;
                    if result["outcome"]["outcome"] == "selected" {
                        result["outcome"]["optionId"].as_str().map(str::to_owned)
                    } else {
                        None
                    }
                } else {
                    local(params).await
                }
            })
        });
        let observed_updates: UpdateSink = Arc::new(move |event| {
                updates(event)?;
                let mut state = observed
                    .lock()
                    .map_err(|_| failure("terminal history unavailable"))?;
                let bytes = event.to_string().len();
                if bytes > 4 * 1024 * 1024 {
                    return Err(failure("terminal update exceeds replay budget"));
                }
                while state.history.len() >= 512 || state.history_bytes + bytes > 4 * 1024 * 1024 {
                    if let Some(removed) = state.history.pop_front() {
                        state.history_bytes -= removed.to_string().len();
                        state.truncated = true;
                    }
                }
                state.history.push_back(event.clone());
                state.history_bytes += bytes;
                if let Some(peer) = state.peer.as_ref() {
                    if peer
                        .updates
                        .try_send(BridgeEvent::Update(event.clone()))
                        .is_err()
                    {
                        // Preserve the Agent and its real result. A slow external
                        // projection must reconnect and replay, never resend a turn.
                        peer.transport.shutdown();
                        state.peer = None;
                    }
                }
                Ok(())
            });
        let agent = match source {
            AgentSource::Owned {program, arguments} => TerminalAgent::Owned(StdioAgent::start_protocol(
                protocol, program, arguments, cwd, observed_updates, permission, authentication,
            ).await?),
            AgentSource::Shared {endpoint, token_file} => {
                if !cwd.is_absolute() || !cwd.is_dir() {return Err("an existing absolute working directory is required".into());}
                if !matches!(protocol, "acp" | "acp-ws" | "app-server") || (protocol == "app-server" && authentication.is_some()) {
                    return Err("shared collaboration requires ACP or app-server; ACP authentication is protocol-specific".into());
                }
                if protocol == "acp" && token_file.is_some() {return Err("ACP byte streams do not accept a WebSocket credential".into());}
                let endpoint = crate::commune_endpoint::LocalEndpoint::try_from(endpoint.to_owned())?;
                let token = crate::commune_endpoint::read_token(token_file)?;
                let (stream, binding) = endpoint.connect_attested().await?;
                let transport = if protocol == "acp" {
                    let (read, write) = tokio::io::split(stream);
                    AcpTransport::from_io(write, read)
                } else {
                    let upgrade = async {
                        if protocol == "app-server" {crate::app_server::transport(stream, &endpoint.websocket_url(), token).await}
                        else {crate::app_server::acp_transport(stream, &endpoint.websocket_url(), token).await}
                    };
                    tokio::time::timeout(Duration::from_secs(5), upgrade).await.map_err(|_| "shared protocol handshake timed out")??
                };
                transport.set_connection_guard(Arc::new(move || binding.verify().map_err(failure))).map_err(|error| error.to_string())?;
                let directory = cwd.to_string_lossy().into_owned();
                let created = if protocol == "app-server" {
                    crate::app_server::Conversation::create(transport.clone(), directory, observed_updates, permission).await.map(Session::AppServer)
                } else {
                    match authentication {
                        Some(method) => ExistingConversation::create_authenticated(transport.clone(), directory, observed_updates, permission, method).await,
                        None => ExistingConversation::create(transport.clone(), directory, observed_updates, permission).await,
                    }.map(Session::Acp)
                };
                let session = match created {
                    Ok(session) => session,
                    Err(error) => {transport.shutdown(); return Err(error.to_string());}
                };
                session.finish_replay()?;
                if !session.is_connected() {return Err("shared daemon binding expired during startup; no retry was sent".into());}
                TerminalAgent::Shared {process: process_identity::instance(std::process::id()).map_err(|error| error.to_string())?, session: Arc::new(session)}
            }
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| error.to_string())?;
        let token = uuid::Uuid::new_v4().to_string() + &uuid::Uuid::new_v4().to_string();
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let published = PublishedSession::write(
            agent.process(),
            address,
            &token,
            agent.session().session_id(),
            cwd,
            agent.session().agent_info(),
        )?;
        let session = agent.session();
        let directory = cwd.to_owned();
        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let Ok((stream,_))=accepted else {break;};
                        if connections.len()>=16 {continue;}
                        let token = token.clone();
                        let state = state.clone();
                        let session = session.clone();
                        let directory = directory.clone();
                        connections.spawn(async move {
                            serve_connection(stream, token, state, session, directory).await;
                        });
                    }
                    Some(_)=connections.join_next()=>{}
                }
            }
        });
        Ok(Self {
            server,
            published,
            agent,
        })
    }
    pub fn session(&self) -> Arc<Session> {
        self.agent.session()
    }
    pub fn process(&self) -> &ProcessInstance {
        self.agent.process()
    }
    pub fn descriptor_path(&self) -> &Path {
        &self.published.descriptor
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    token: String,
    state: Arc<Mutex<BridgeState>>,
    session: Arc<Session>,
    directory: PathBuf,
) {
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    let accept = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        move |request: &Request, response: Response| {
            if request.uri().path() != "/"
                || request
                    .headers()
                    .get("authorization")
                    .and_then(|header| header.to_str().ok())
                    != Some(format!("Bearer {token}").as_str())
            {
                return Err(Response::builder()
                    .status(401)
                    .body(None)
                    .expect("valid refusal"));
            }
            Ok(response)
        },
        Some(tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
            max_message_size: Some(16 * 1024 * 1024),
            max_frame_size: Some(16 * 1024 * 1024),
            ..Default::default()
        }),
    );
    let Ok(Ok(socket)) = tokio::time::timeout(Duration::from_secs(5), accept).await else {
        return;
    };
    let transport = crate::app_server::bridge_with_framing(socket, false);
    let Some(mut requests) = transport.take_peer_requests() else {
        return;
    };
    let id = uuid::Uuid::new_v4();
    let (cancel, cancelled) = tokio::sync::mpsc::channel(1);
    let selected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let selection = selected.clone();
    let identity = session.session_id().to_owned();
    if transport
        .set_notification_sink(Arc::new(move |notification| {
            if notification.method == "session/cancel"
                && selection.load(std::sync::atomic::Ordering::Acquire)
                && notification
                    .params
                    .as_ref()
                    .is_some_and(|params| params["sessionId"] == identity)
            {
                let _ = cancel.try_send(());
            }
            Ok(())
        }))
        .is_err()
    {
        return;
    }
    let (events, mut updates) = tokio::sync::mpsc::channel(512);
    let mut cancelled = cancelled;
    let mut turns = tokio::task::JoinSet::new();
    let mut initialized = false;
    loop {
        tokio::select! {
            // The reader queues a request before its following notification.
            // Drain that request first so an immediate cancel cannot overtake
            // the prompt it belongs to.
            biased;
            next=requests.recv()=>{
                let Some(request) = next else { break; };
                if !session.is_connected() {
                    let _ = transport.respond(&request.id, Err(rpc_error("terminal session binding expired; reconnect explicitly".into())));
                    break;
                }
                let params = request.params.unwrap_or(Value::Null);
                let result = match request.method.as_str() {
                    "initialize" if !initialized => {
                        initialized = true;
                        Ok(json!({
                            "protocolVersion":1, "agentInfo":session.agent_info(),
                            "agentCapabilities":{"loadSession":true},
                            "_meta":{"historySource":"terminal_bridge_observed"}
                        }))
                    }
                    "session/load" if initialized
                        && !selected.load(std::sync::atomic::Ordering::Acquire)
                        && params["sessionId"] == session.session_id()
                        && params["cwd"].as_str().is_some_and(|cwd| {
                            std::fs::canonicalize(cwd).ok()
                                .zip(std::fs::canonicalize(&directory).ok())
                                .is_some_and(|(actual,expected)| actual==expected)
                        }) => {
                        let snapshot = {
                            let mut state = state.lock().expect("bridge state");
                            if state.peer.as_ref().is_some_and(|peer| !peer.transport.is_closed()) {
                                None
                            } else {
                                let snapshot = (state.history.clone(), state.truncated);
                                state.peer = Some(Peer { id, transport:transport.clone(), updates:events.clone() });
                                Some(snapshot)
                            }
                        };
                        if let Some((history,truncated))=snapshot {
                            selected.store(true,std::sync::atomic::Ordering::Release);
                            let mut replayed = true;
                            for event in history {
                                if transport.notify("session/update", event).await.is_err() {
                                    replayed = false;
                                    break;
                                }
                            }
                            if !replayed { break; }
                            Ok(json!({"_meta":{"historySource":"terminal_bridge_observed","historyTruncated":truncated}}))
                        } else {
                            Err("this terminal session already has a control connection".to_owned())
                        }
                    }
                    "session/prompt" if selected.load(std::sync::atomic::Ordering::Acquire) && params["sessionId"]==session.session_id()=>{
                        let prompt = params["prompt"].as_array()
                            .filter(|blocks| !blocks.is_empty())
                            .and_then(|blocks| blocks.iter().map(|block| {
                                if block["type"] == "text" { block["text"].as_str() } else { None }
                            }).collect::<Option<Vec<_>>>());
                        match prompt {
                            Some(blocks)=>match session.prompt(blocks.join("\n")) {
                                Ok(pending)=>{
                                    let replies=events.clone();
                                    let upstream_is_acp = matches!(session.as_ref(), Session::Acp(_));
                                    turns.spawn(async move {
                                        let outcome = match pending.await {
                                            Ok(Ok(result)) => Ok(result),
                                            Ok(Err(AcpErrorWrap::Rpc(AUTH_REQUIRED, _))) if upstream_is_acp => Err(AcpErrorObj {
                                                code: AUTH_REQUIRED,
                                                message: "Authentication required".into(),
                                                data: None,
                                            }),
                                            Ok(Err(error)) => Err(rpc_error(error.to_string())),
                                            Err(_) => Err(rpc_error("Agent result unknown; no retry was sent".into())),
                                        };
                                        let _=replies.send(BridgeEvent::Reply(request.id,outcome)).await;
                                    });
                                    continue;
                                }
                                Err(error)=>Err(error.to_string()),
                            },
                            None=>Err("terminal bridge accepts text prompt blocks".into()),
                        }
                    }
                    _=>Err("unsupported request or session identity; no new session created".into()),
                };
                if transport.respond(&request.id,result.map_err(rpc_error)).is_err(){break;}
            }
            Some(event)=updates.recv()=>{
                if !session.is_connected() {break;}
                let result = match event {
                    BridgeEvent::Update(event) => transport.notify("session/update",event).await,
                    BridgeEvent::Reply(id,result) => transport.respond(&id,result),
                };
                if result.is_err(){break;}
            }
            Some(())=cancelled.recv()=>{let _=session.cancel().await;}
            Some(_)=turns.join_next()=>{}
            _=tokio::time::sleep(Duration::from_millis(100))=>{if transport.is_closed()||!session.is_connected(){break;}}
        }
    }
    transport.shutdown();
    if let Ok(mut state) = state.lock() {
        if state.peer.as_ref().is_some_and(|peer| peer.id == id) {
            state.peer = None;
        }
    }
}
fn rpc_error(message: String) -> AcpErrorObj {
    AcpErrorObj {
        code: -32603,
        message,
        data: None,
    }
}

struct PublishedSession {
    descriptor: PathBuf,
    credential: PathBuf,
    descriptor_owned: bool,
    credential_owned: bool,
}
impl Drop for PublishedSession {
    fn drop(&mut self) {
        if self.descriptor_owned {
            let _ = std::fs::remove_file(&self.descriptor);
        }
        if self.credential_owned {
            let _ = std::fs::remove_file(&self.credential);
        }
    }
}
pub(crate) fn descriptor_path(process: &ProcessInstance) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = format!("{:x}", Sha256::digest(process.start_instance.as_bytes()));
    crate::registry::ridge_data_dir()
        .join("agent-connections")
        .join(format!("{}-{digest}.json", process.pid))
}
impl PublishedSession {
    fn write(
        process: &ProcessInstance,
        endpoint: std::net::SocketAddr,
        token: &str,
        session: &str,
        cwd: &Path,
        info: Option<&Value>,
    ) -> Result<Self, String> {
        use std::io::Write;
        let descriptor = descriptor_path(process);
        let credential = descriptor.with_extension("token");
        let directory = descriptor
            .parent()
            .ok_or("connection directory unavailable")?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        protect_directory(directory)
            .map_err(|_| "could not protect the local Agent connection registry")?;
        let mut publication = Self {
            descriptor,
            credential,
            descriptor_owned: false,
            credential_owned: false,
        };
        let data = json!({"agentName":info.and_then(|info|info["name"].as_str()).unwrap_or("ACP Agent"),"process":process,"endpointOwner":process_identity::instance(std::process::id()).map_err(|error|error.to_string())?,"endpoint":endpoint,"protocol":"acp-ws","sessionId":session,"cwd":cwd,"bearerTokenFile":publication.credential});
        for (path, bytes, owned) in [
            (
                &publication.credential,
                token.as_bytes().to_vec(),
                &mut publication.credential_owned,
            ),
            (
                &publication.descriptor,
                serde_json::to_vec(&data).map_err(|error| error.to_string())?,
                &mut publication.descriptor_owned,
            ),
        ] {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(path)
                .map_err(|error| format!("could not publish collaboration endpoint: {error}"))?;
            *owned = true;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("could not publish collaboration endpoint: {error}"))?;
        }
        Ok(publication)
    }
}

#[cfg(unix)]
fn protect_directory(directory: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::symlink_metadata(directory)?
        .file_type()
        .is_symlink()
    {
        return Err(std::io::Error::other("registry must be a real directory"));
    }
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
}
#[cfg(windows)]
fn protect_directory(directory: &Path) -> std::io::Result<()> {
    use std::{
        ffi::c_void,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
        ptr,
    };
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    if std::fs::symlink_metadata(directory)?.file_attributes() & 0x400 != 0 {
        return Err(std::io::Error::other(
            "registry must not be a reparse point",
        ));
    }
    // Protected DACL. Only the owning account and SYSTEM inherit full access.
    let sddl = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let path = directory
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    // SAFETY: both strings are NUL-terminated, the OS allocates descriptor,
    // and LocalFree releases it after SetFileSecurityW completes.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let applied = SetFileSecurityW(path.as_ptr(), 0x80000004, descriptor);
        let result = if applied == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        };
        LocalFree(descriptor);
        result
    }
}

struct OwnedProcess {
    child: Child,
    instance: ProcessInstance,
}
impl OwnedProcess {
    fn terminate(&mut self) {
        if process_identity::validate(&self.instance).is_ok() {
            ridge_core::process_guard::kill_process_tree(self.instance.pid);
        }
        let _ = self.child.start_kill();
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub struct StdioAgent {
    // Own the process before the stream so drop reclaims descendants before stdin closes.
    owned: OwnedProcess,
    session: Arc<Session>,
}
impl StdioAgent {
    pub async fn start(
        program: &Path,
        arguments: &[String],
        cwd: &Path,
        updates: UpdateSink,
        permission: PermissionHandler,
    ) -> Result<Self, String> {
        Self::start_protocol("acp", program, arguments, cwd, updates, permission, None).await
    }
    pub async fn start_protocol(
        protocol: &str,
        program: &Path,
        arguments: &[String],
        cwd: &Path,
        updates: UpdateSink,
        permission: PermissionHandler,
        authentication: Option<String>,
    ) -> Result<Self, String> {
        if !matches!(protocol, "acp" | "app-server") {
            return Err("collaboration requires acp or app-server".into());
        }
        if protocol != "acp" && authentication.is_some() {
            return Err("protocol authentication selection requires ACP".into());
        }
        if program.as_os_str().is_empty() || !cwd.is_absolute() || !cwd.is_dir() {
            return Err(
                "an executable and an existing absolute working directory are required".into(),
            );
        }
        let mut command = launch_command(program)?;
        // A protocol child can start its own loopback service.
        let upper = std::env::var_os("NO_PROXY");
        let lower = std::env::var_os("no_proxy");
        command.env("NO_PROXY", managed_no_proxy(upper.as_deref().or(lower.as_deref())));
        #[cfg(not(windows))]
        command.env("no_proxy", managed_no_proxy(lower.as_deref().or(upper.as_deref())));
        command
            .args(arguments)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let child = command
            .spawn()
            .map_err(|error| format!("could not start collaboration agent: {error}"))?;
        let instance =
            process_identity::instance(child.id().ok_or("started Agent has no process identity")?)
                .map_err(|error| error.to_string())?;
        let mut owned = OwnedProcess { child, instance };
        let input = owned.child.stdin.take().ok_or("Agent stdin unavailable")?;
        let output = owned
            .child
            .stdout
            .take()
            .ok_or("Agent stdout unavailable")?;
        let cwd = cwd.to_string_lossy().into_owned();
        let session = if protocol == "acp" {
            let transport = AcpTransport::from_io(input, output);
            let conversation = match authentication {
                Some(method) => {
                    ExistingConversation::create_authenticated(
                        transport, cwd, updates, permission, method,
                    )
                    .await
                }
                None => ExistingConversation::create(transport, cwd, updates, permission).await,
            }
            .map_err(|error| error.to_string())?;
            Session::Acp(conversation)
        } else {
            Session::AppServer(
                crate::app_server::Conversation::create(
                    crate::app_server::stdio_transport(input, output),
                    cwd,
                    updates,
                    permission,
                )
                .await
                .map_err(|error| error.to_string())?,
            )
        };
        session.finish_replay()?;
        process_identity::validate(&owned.instance).map_err(|error| error.to_string())?;
        Ok(Self {
            owned,
            session: Arc::new(session),
        })
    }

    pub fn process(&self) -> &ProcessInstance {
        &self.owned.instance
    }
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }
    pub async fn stop(mut self) -> Result<(), String> {
        self.owned.terminate();
        let result = self
            .owned
            .child
            .wait()
            .await
            .map_err(|error| error.to_string());
        self.session.disconnect();
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_loopback_bypass_preserves_existing_exclusions() {
        assert_eq!(managed_no_proxy(None), "localhost,127.0.0.1,::1");
        assert_eq!(
            managed_no_proxy(Some(OsStr::new(".example.com, LOCALHOST"))),
            ".example.com, LOCALHOST,127.0.0.1,::1"
        );
        assert_eq!(managed_no_proxy(Some(OsStr::new("*"))), "*");
    }
}
