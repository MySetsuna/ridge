//! App-server protocol for an OS-attested, already-loaded terminal conversation.
//! Never starts a process, thread, model client, or reads a provider's history files.
use futures::{SinkExt, StreamExt};
use ridge_acp::{existing::{UpdateSink, PermissionHandler}, transport::{AcpTransport, CallCompletion}, AcpErrorWrap};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use tokio::{io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader}, sync::oneshot};
use tokio_tungstenite::{tungstenite::{client::IntoClientRequest, Message}, WebSocketStream};

fn error(message: impl Into<String>)->AcpErrorWrap { AcpErrorWrap::Transport(message.into()) }
async fn call(transport:&AcpTransport,method:&str,params:Value)->Result<Value,AcpErrorWrap> {
    let reply=transport.call(method,params).await?;
    if let Some(e)=reply.error { return Err(e.into()); }
    reply.result.ok_or_else(||error("app-server response has no result"))
}
async fn initialize(transport: &AcpTransport) -> Result<Value, AcpErrorWrap> {
    let initialized = call(
        transport,
        "initialize",
        json!({
            "clientInfo": {"name": "ridge_commune", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"experimentalApi": true}
        }),
    )
    .await?;
    transport.notify("initialized", Value::Null).await?;
    Ok(initialized)
}

fn same_cwd(actual: &Value, expected: &str) -> bool {
    actual.as_str().is_some_and(|actual| {
        let actual = std::path::Path::new(actual);
        let expected = std::path::Path::new(expected);
        actual == expected
            || (crate::commune_endpoint::local_path(actual)
                && crate::commune_endpoint::local_path(expected)
                && std::fs::canonicalize(actual)
                    .ok()
                    .zip(std::fs::canonicalize(expected).ok())
                    .is_some_and(|(actual, expected)| actual == expected))
    })
}

pub(crate) async fn list_sessions(
    transport: &AcpTransport,
    cwd: &str,
) -> Result<Vec<Value>, AcpErrorWrap> {
    initialize(transport).await?;
    let mut cursor = None::<String>;
    let mut seen = std::collections::HashSet::new();
    let mut sessions = Vec::new();
    for _ in 0..8 {
        let mut params = json!({"limit": 16});
        if let Some(cursor) = &cursor {
            params["cursor"] = json!(cursor);
        }
        let page = call(transport, "thread/loaded/list", params).await?;
        let rows = page["data"]
            .as_array()
            .ok_or_else(|| error("invalid loaded app-server thread page"))?;
        if seen.len() + rows.len() > 128 {
            return Err(error("app-server listing exceeds the 128-thread budget"));
        }
        for row in rows {
            let id = row
                .as_str()
                .filter(|id| !id.trim().is_empty() && id.len() <= 512)
                .ok_or_else(|| error("invalid app-server thread identity"))?;
            if !seen.insert(id.to_owned()) {
                return Err(error("duplicate loaded app-server thread identity"));
            }
            let read = call(transport, "thread/read", json!({"threadId": id})).await?;
            let thread = &read["thread"];
            if thread["id"] == id
                && same_cwd(&thread["cwd"], cwd)
                && thread["canAcceptDirectInput"] == true
                && matches!(thread["status"]["type"].as_str(), Some("idle" | "active"))
            {
                sessions.push(json!({"sessionId": id, "cwd": cwd,
                    "title": thread["name"].as_str().map(|title| title.chars().take(160).collect::<String>())}));
            }
        }
        cursor = match &page["nextCursor"] {
            Value::Null => return Ok(sessions),
            Value::String(value)
                if !value.is_empty() && value.len() <= 4096 && cursor.as_ref() != Some(value) =>
            {
                Some(value.clone())
            }
            _ => return Err(error("invalid app-server thread cursor")),
        };
    }
    Err(error("app-server listing exceeds the page budget"))
}

async fn bounded_line<R:tokio::io::AsyncBufRead+Unpin>(reader:&mut R)->std::io::Result<Option<String>> {
    let mut bytes=Vec::new();
    let count=reader.take(16*1024*1024+1).read_until(b'\n',&mut bytes).await?;
    if count==0{return Ok(None);}
    if count>16*1024*1024{return Err(std::io::Error::other("protocol line exceeds message budget"));}
    String::from_utf8(bytes).map(Some).map_err(|_|std::io::Error::other("invalid protocol text"))
}

/// Adapt WebSocket messages to the existing bounded RPC correlator. App-server
/// intentionally omits jsonrpc on the wire; this field exists only inside it.
pub(crate) async fn transport<S: crate::commune_endpoint::LocalIo + 'static>(stream:S,url:&str,token:Option<String>)->Result<AcpTransport,String> {
    websocket_transport(stream,url,token,true).await
}
pub(crate) async fn acp_transport<S: crate::commune_endpoint::LocalIo + 'static>(stream:S,url:&str,token:Option<String>)->Result<AcpTransport,String> {
    websocket_transport(stream,url,token,false).await
}
async fn websocket_transport<S: crate::commune_endpoint::LocalIo + 'static>(stream:S,url:&str,token:Option<String>,app_server:bool)->Result<AcpTransport,String> {
    let mut request=url.into_client_request().map_err(|e|e.to_string())?;
    if let Some(token)=token {
        request.headers_mut().insert("authorization",format!("Bearer {token}").parse().map_err(|_|"invalid protocol credential")?);
    }
    let config=tokio_tungstenite::tungstenite::protocol::WebSocketConfig { max_message_size:Some(16*1024*1024),max_frame_size:Some(16*1024*1024),..Default::default() };
    let (socket,_)=tokio_tungstenite::client_async_with_config(request,stream,Some(config)).await.map_err(|_|"protocol WebSocket handshake failed (check endpoint and authentication)".to_owned())?;
    Ok(bridge_with_framing(socket,app_server))
}
pub(crate) fn stdio_transport<W,R>(mut stdin:W,stdout:R)->AcpTransport
where W:tokio::io::AsyncWrite+Unpin+Send+'static,R:tokio::io::AsyncRead+Unpin+Send+'static {
    let (rpc,wire)=tokio::io::duplex(64*1024);
    let (read,write)=tokio::io::split(rpc);
    let (input,mut output)=tokio::io::split(wire);
    tokio::spawn(async move {
        let outgoing=async {
            let mut lines=BufReader::new(input);
            while let Ok(Some(line))=bounded_line(&mut lines).await {
                let Ok(mut value)=serde_json::from_str::<Value>(&line) else {break;};
                let Some(object)=value.as_object_mut() else {break;};
                object.remove("jsonrpc");
                let bytes=format!("{value}\n");
                if stdin.write_all(bytes.as_bytes()).await.is_err(){break;}
            }
        };
        let incoming=async {
            let mut lines=BufReader::new(stdout);
            while let Ok(Some(line))=bounded_line(&mut lines).await {
                let Ok(mut value)=serde_json::from_str::<Value>(&line) else {break;};
                let Some(object)=value.as_object_mut() else {break;};
                object.insert("jsonrpc".into(),json!("2.0"));
                let bytes=format!("{value}\n");
                if output.write_all(bytes.as_bytes()).await.is_err(){break;}
            }
        };
        tokio::select!{_=outgoing=>{},_=incoming=>{}}
    });
    AcpTransport::from_io(write,read)
}
pub(crate) fn bridge_with_framing<S:tokio::io::AsyncRead+tokio::io::AsyncWrite+Unpin+Send+'static>(socket:WebSocketStream<S>,app_server:bool)->AcpTransport {
    let (rpc,wire)=tokio::io::duplex(64*1024);
    let (read,write)=tokio::io::split(rpc);
    tokio::spawn(async move {
        let (mut sender,mut receiver)=socket.split();
        let (input,mut output)=tokio::io::split(wire);
        let mut input=BufReader::new(input);
        loop {
            tokio::select! {
                next=bounded_line(&mut input)=>{
                    let Ok(Some(line))=next else {break;};
                    let Ok(mut value)=serde_json::from_str::<Value>(&line) else {break;};
                    if app_server { value.as_object_mut().map(|object|object.remove("jsonrpc")); }
                    if sender.send(Message::Text(value.to_string())).await.is_err(){break;}
                }
                next=receiver.next()=>match next {
                    Some(Ok(Message::Text(text)))=>{
                        let Ok(mut value)=serde_json::from_str::<Value>(&text) else {break;};
                        let Some(object)=value.as_object_mut() else {break;};
                        if app_server { object.insert("jsonrpc".into(),json!("2.0")); }
                        let mut bytes=value.to_string().into_bytes();bytes.push(b'\n');
                        if output.write_all(&bytes).await.is_err(){break;}
                    }
                    Some(Ok(Message::Ping(bytes)))=>{if sender.send(Message::Pong(bytes)).await.is_err(){break;}}
                    Some(Ok(Message::Pong(_)))=>{},
                    _=>break,
                }
            }
        }
    });
    AcpTransport::from_io(write,read)
}

struct Turn {
    id:Option<String>,
    // Notifications may arrive before the turn/start response. Keep one terminal
    // result until its id is correlated; never mistake the start receipt for completion.
    terminal:Option<Value>,
    pending:Option<oneshot::Sender<Result<Value,AcpErrorWrap>>>,
    completion:Option<CallCompletion>,
}
struct Inner {
    transport:AcpTransport,thread:String,info:Value,busy:AtomicBool,turn:Mutex<Turn>,
    updates:UpdateSink,buffer:Mutex<Option<Vec<Value>>>,
    requests:Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl Drop for Inner {fn drop(&mut self){self.transport.shutdown();if let Ok(task)=self.requests.get_mut(){if let Some(task)=task.take(){task.abort();}}}}
pub struct Conversation { inner:Arc<Inner> }
impl Conversation {
    pub(crate) async fn create(transport:AcpTransport,cwd:String,updates:UpdateSink,permission:PermissionHandler)->Result<Self,AcpErrorWrap> {
        let start=async {
            let initialized=initialize(&transport).await?;
            let created=call(&transport,"thread/start",json!({"cwd":cwd})).await?;
            let thread=created["thread"]["id"].as_str().filter(|id|!id.is_empty()).ok_or_else(||error("created app-server thread has no identity"))?.to_owned();
            Self::open(transport.clone(),thread,cwd,updates,permission,Some(initialized)).await
        };
        match tokio::time::timeout(std::time::Duration::from_secs(30),start).await {
            Ok(Ok(session))=>Ok(session),
            result=>{transport.shutdown();Err(match result {Ok(Err(error))=>error,_=>error("app-server creation outcome unknown; creation was not retried")})}
        }
    }
    pub(crate) async fn load(transport:AcpTransport,thread:String,cwd:String,updates:UpdateSink,permission:PermissionHandler)->Result<Self,AcpErrorWrap> {
        Self::open(transport,thread,cwd,updates,permission,None).await
    }
    async fn open(transport:AcpTransport,thread:String,cwd:String,updates:UpdateSink,permission:PermissionHandler,created:Option<Value>)->Result<Self,AcpErrorWrap> {
        // userAgent incorporates the client's chosen name. Preserve the wire
        // value as metadata rather than presenting it as the Agent's name.
        let info=json!({"name":"app-server","_meta":{"userAgent":created.as_ref().and_then(|info|info["userAgent"].as_str())}});
        let inner=Arc::new(Inner {transport:transport.clone(),thread:thread.clone(),info,busy:AtomicBool::new(false),turn:Mutex::new(Turn{id:None,terminal:None,pending:None,completion:None}),updates:updates.clone(),buffer:Mutex::new(Some(Vec::new())),requests:Mutex::new(None)});
        let weak=Arc::downgrade(&inner);
        let notifications=updates.clone();
        transport.set_notification_sink(Arc::new(move |notification| {
            let Some(inner)=weak.upgrade() else {return Ok(());};
            let params=notification.params.as_ref().unwrap_or(&Value::Null);
            if params["threadId"].as_str()!=Some(&inner.thread) {return Ok(());}
            let mut completed=None;
            match notification.method.as_str() {
                "turn/started"=>{inner.busy.store(true,Ordering::Release);},
                "thread/status/changed"=>{inner.busy.store(params["status"]["type"]=="active" || inner.turn.lock().map_err(|_|error("turn unavailable"))?.pending.is_some(),Ordering::Release);},
                "turn/completed"=>{
                    let terminal=params["turn"].clone();
                    if terminal["id"].as_str().is_none(){return Err(error("app-server completion missing turn id"));}
                    completed=Some(terminal);
                },
                _=>{},
            }
            let update=match notification.method.as_str() {
                "item/agentMessage/delta"=>json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":params["delta"]}}),
                "item/reasoning/textDelta"|"item/reasoning/summaryTextDelta"=>json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":params["delta"]}}),
                // Full completed items contain canonical text; do not also index
                // message deltas into the same history view.
                "item/completed"|"item/started"=>json!({"sessionUpdate":"protocol_item","item":params["item"],"method":notification.method}),
                _=>json!({"sessionUpdate":"protocol_event","method":notification.method,"params":params}),
            };
            let event=json!({"sessionId":inner.thread,"protocol":"app-server","update":update,"wire":{"method":notification.method,"params":params}});
            let mut buffer=inner.buffer.lock().map_err(|_|error("protocol replay buffer unavailable"))?;
            if let Some(buffer)=buffer.as_mut(){if buffer.len()>=512{return Err(error("protocol replay buffer exhausted; reconnect required"));}buffer.push(event);}
            else{notifications(&event)?;}
            if let Some(terminal)=completed {
                let mut turn=inner.turn.lock().map_err(|_|error("turn unavailable"))?;
                if turn.pending.is_some() {
                    if turn.id.as_deref()==terminal["id"].as_str(){settle(&mut turn,terminal);}
                    else if turn.id.is_none(){turn.terminal=Some(terminal);}
                }
                inner.busy.store(turn.pending.is_some(),Ordering::Release);
            }
            Ok(())
        }))?;
        let mut requests=transport.take_peer_requests().ok_or_else(||error("protocol request receiver missing"))?;
        let request_transport=transport.clone();
        let request_thread=thread.clone();
        *inner.requests.lock().map_err(|_|error("request handler unavailable"))?=Some(tokio::spawn(async move {
            while let Some(request)=requests.recv().await {
                let params=request.params.unwrap_or(Value::Null);
                let supported=matches!(request.method.as_str(),"item/commandExecution/requestApproval"|"item/fileChange/requestApproval");
                if !supported || params["threadId"]!=request_thread {
                    let _=request_transport.respond(&request.id,Err(ridge_acp::protocol::AcpErrorObj{code:-32601,message:"Unsupported terminal approval; no permission granted".into(),data:None}));
                    continue;
                }
                let options=params["availableDecisions"].as_array().cloned().unwrap_or_else(||vec![json!("accept"),json!("decline"),json!("cancel")]);
                let offered=options.iter().filter_map(|option|option.as_str()).filter(|option|matches!(*option,"accept"|"acceptForSession"|"decline"|"cancel")).collect::<Vec<_>>();
                let choice=permission(json!({"sessionId":request_thread,"toolCall":{"toolCallId":params["itemId"],"title":params["reason"],"rawInput":params},"options":offered.iter().map(|option|json!({"optionId":option,"name":option,"kind":if option.starts_with("accept"){"allow_once"}else{"reject_once"}})).collect::<Vec<_>>() })).await;
                let decision=choice.as_deref().filter(|choice|offered.contains(choice)).unwrap_or("cancel");
                if request_transport.respond(&request.id,Ok(json!({"decision":decision}))).is_err(){break;}
            }
        }));
        let handshake=async {
            if created.is_some() {return Ok(());}
            initialize(&transport).await?;
            let before=call(&transport,"thread/read",json!({"threadId":thread})).await?;
            let existing=&before["thread"];
            if existing["id"]!=thread || existing["status"]["type"]=="notLoaded" || existing["canAcceptDirectInput"]!=true {
                return Err(error("app-server thread is not an already-loaded controllable terminal conversation"));
            }
            if !same_cwd(&existing["cwd"], &cwd) {
                return Err(error("protocol conversation cwd does not match registration"));
            }
            inner.busy.store(existing["status"]["type"]=="active",Ordering::Release);
            // Only an explicit control attachment subscribes to this existing
            // loaded thread. No settings/history/path overrides are sent.
            let resumed=call(&transport,"thread/resume",json!({"threadId":thread,"excludeTurns":true})).await?;
            if resumed["thread"]["id"]!=thread {return Err(error("app-server resumed a different conversation"));}
            replay(&transport,&thread,&updates).await
        };
        match tokio::time::timeout(std::time::Duration::from_secs(30),handshake).await {
            Ok(Ok(()))=>Ok(Self{inner}),
            outcome=>{transport.shutdown();Err(match outcome{Ok(Err(e))=>e,_=>error("existing app-server attachment timed out; no new conversation created")})}
        }
    }
    pub fn agent_info(&self)->&Value{&self.inner.info}
    pub fn session_id(&self)->&str{&self.inner.thread}
    pub fn finish_replay(&self)->Result<(),String>{
        let mut buffer=self.inner.buffer.lock().map_err(|_|"protocol replay buffer unavailable")?;
        if let Some(events)=buffer.take(){for event in events{(self.inner.updates)(&event).map_err(|e|e.to_string())?;}}
        Ok(())
    }
    pub fn is_connected(&self)->bool{!self.inner.transport.is_closed()}
    pub fn is_busy(&self)->bool{self.inner.busy.load(Ordering::Acquire)}
    pub fn disconnect(&self){self.inner.transport.shutdown();if let Ok(mut turn)=self.inner.turn.lock(){fail(&mut turn,"app-server connection closed; execution outcome requires confirmation");}}
    pub fn retire_idle(&self,commit:impl FnOnce()->Result<(),String>)->Result<(),String>{
        if self.inner.busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return Err("Agent turn is active; wait or cancel before removal".into());}
        match commit(){Ok(())=>{self.disconnect();Ok(())},Err(e)=>{self.inner.busy.store(false,Ordering::Release);Err(e)}}
    }
    pub fn prompt_observed(&self,text:String,completion:Option<CallCompletion>)->Result<oneshot::Receiver<Result<Value,AcpErrorWrap>>,AcpErrorWrap>{
        if !self.is_connected(){return Err(AcpErrorWrap::SessionClosed);}
        if self.inner.busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){return Err(error("existing conversation is busy"));}
        let (sender,receiver)=oneshot::channel();
        {let mut turn=self.inner.turn.lock().map_err(|_|error("turn unavailable"))?;turn.id=None;turn.terminal=None;turn.pending=Some(sender);turn.completion=completion;}
        let inner=self.inner.clone();
        tokio::spawn(async move {
            let reply=call(&inner.transport,"turn/start",json!({"threadId":inner.thread,"input":[{"type":"text","text":text,"text_elements":[]}]})).await;
            match reply {
                Ok(reply) if reply["turn"]["id"].is_string()=>{
                    let mut turn=inner.turn.lock().expect("turn lock");turn.id=reply["turn"]["id"].as_str().map(str::to_owned);
                    if let Some(terminal)=turn.terminal.take(){if terminal["id"].as_str()==turn.id.as_deref(){settle(&mut turn,terminal);}}
                }
                reply=>{let reason=reply.err().map_or_else(||"app-server start response missing turn identity".into(),|e|e.to_string());fail(&mut inner.turn.lock().expect("turn lock"),&reason);inner.busy.store(false,Ordering::Release);}
            }
            // A dropped caller must not cancel or resend an already-sent turn.
            while !inner.transport.is_closed() && inner.turn.lock().is_ok_and(|turn|turn.pending.is_some()) {tokio::time::sleep(std::time::Duration::from_millis(100)).await;}
            if inner.transport.is_closed(){fail(&mut inner.turn.lock().expect("turn lock"),"app-server connection lost; execution outcome requires confirmation");}
        });
        Ok(receiver)
    }
    pub async fn cancel(&self)->Result<(),AcpErrorWrap>{
        let id=self.inner.turn.lock().map_err(|_|error("turn unavailable"))?.id.clone().ok_or_else(||error("turn identity is not confirmed; no cancellation sent"))?;
        call(&self.inner.transport,"turn/interrupt",json!({"threadId":self.inner.thread,"turnId":id})).await?;Ok(())
    }
}
fn fail(turn:&mut Turn,reason:&str){if let Some(sender)=turn.pending.take(){let _=sender.send(Err(error(reason)));}if let Some(done)=turn.completion.take(){done();}}
fn settle(turn:&mut Turn,terminal:Value){
    if let Some(sender)=turn.pending.take(){let response=match terminal["status"].as_str(){Some("completed")=>Ok(json!({"stopReason":"end_turn","turnId":terminal["id"]})),Some("interrupted")=>Ok(json!({"stopReason":"cancelled","turnId":terminal["id"]})),_=>Err(error(format!("app-server turn ended: {}",terminal["status"])))};let _=sender.send(response);}
    if let Some(done)=turn.completion.take(){done();}
}
async fn replay(transport:&AcpTransport,thread:&str,updates:&UpdateSink)->Result<(),AcpErrorWrap>{
    let mut cursor=Value::Null;let mut seen=std::collections::HashSet::new();
    for _ in 0..256 {
        let page=call(transport,"thread/turns/list",json!({"threadId":thread,"cursor":cursor,"limit":50,"sortDirection":"asc","itemsView":"full"})).await?;
        for turn in page["data"].as_array().ok_or_else(||error("invalid protocol turn history"))? {
            let items=turn["items"].as_array().ok_or_else(||error("protocol history omitted items"))?;
            for item in items {
                updates(&json!({"sessionId":thread,"protocol":"app-server","update":{"sessionUpdate":"protocol_item","method":"history/item","item":item},"wire":{"turnId":turn["id"],"item":item}}))?;
            }
        }
        cursor=page["nextCursor"].clone();if cursor.is_null(){return Ok(());}
        if !cursor.is_string()||!seen.insert(cursor.to_string()){return Err(error("protocol history cursor did not advance"));}
    }
    Err(error("protocol history exceeds bounded replay; original history preserved"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acp_websocket_preserves_standard_jsonrpc_and_authenticates_before_messages() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();
            let mut socket=tokio_tungstenite::accept_hdr_async(stream,|request:&tokio_tungstenite::tungstenite::handshake::server::Request,response:tokio_tungstenite::tungstenite::handshake::server::Response| {
                assert_eq!(request.headers().get("authorization").unwrap(),"Bearer owned-test-token");
                Ok(response)
            }).await.unwrap();
            let request:Value=serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
            assert_eq!(request["jsonrpc"],"2.0");
            assert_eq!(request["method"],"initialize");
            socket.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"protocolVersion":1,"agentCapabilities":{}}}).to_string())).await.unwrap();
        });
        let stream=tokio::net::TcpStream::connect(address).await.unwrap();
        let transport=acp_transport(stream,&format!("ws://{address}/"),Some("owned-test-token".into())).await.unwrap();
        let response=transport.call("initialize",json!({"protocolVersion":1})).await.unwrap();
        assert_eq!(response.result.unwrap()["protocolVersion"],1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn acp_websocket_authentication_refusal_never_opens_a_protocol_transport() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();
            let result=tokio_tungstenite::accept_hdr_async(stream,|_:&tokio_tungstenite::tungstenite::handshake::server::Request,_:tokio_tungstenite::tungstenite::handshake::server::Response| {
                Err(tokio_tungstenite::tungstenite::http::Response::builder().status(401).body(None).unwrap())
            }).await;
            assert!(result.is_err());
        });
        let stream=tokio::net::TcpStream::connect(address).await.unwrap();
        let error=acp_transport(stream,&format!("ws://{address}/"),Some("owned-test-token".into())).await.err().unwrap();
        assert!(error.contains("authentication"));
        assert!(!error.contains("owned-test-token"));
        server.await.unwrap();
    }
    async fn fixture(loaded:bool)->(AcpTransport,Arc<Mutex<Vec<String>>>) {
        let (client,server)=tokio::io::duplex(64*1024);
        let calls=Arc::new(Mutex::new(Vec::new()));let observed=calls.clone();
        tokio::spawn(async move {
            let mut socket=WebSocketStream::from_raw_socket(server,tokio_tungstenite::tungstenite::protocol::Role::Server,None).await;
            while let Some(Ok(Message::Text(text)))=socket.next().await {
                let request:Value=serde_json::from_str(&text).unwrap();assert!(request.get("jsonrpc").is_none());
                let method=request["method"].as_str().unwrap();observed.lock().unwrap().push(method.into());
                let result=match method {
                    "initialize"=>json!({"userAgent":"owned fixture"}),
                    "initialized"=>continue,
                    "thread/loaded/list"=>json!({"data":["existing","other-cwd","read-only","second"]}),
                    "thread/read"=>{
                        let id=request["params"]["threadId"].as_str().unwrap();
                        json!({"thread":{"id":id,"cwd":if id=="other-cwd"{"C:/elsewhere"}else{"C:/owned"},
                            "status":{"type":if loaded{"idle"}else{"notLoaded"}},"canAcceptDirectInput":loaded&&id!="read-only"}})
                    },
                    "thread/resume"=>{assert_eq!(request["params"],json!({"threadId":"existing","excludeTurns":true}));json!({"thread":{"id":"existing"}})},
                    "thread/turns/list"=>json!({"data":[{"id":"before","items":[{"id":"before-message","type":"agentMessage","text":"existing history"}]}],"nextCursor":null}),
                    "turn/start"=>json!({"turn":{"id":"owned-turn","status":"inProgress"}}),
                    "turn/interrupt"=>{assert_eq!(request["params"],json!({"threadId":"existing","turnId":"owned-turn"}));json!({})},
                    _=>panic!("unexpected protocol mutation: {method}"),
                };
                socket.send(Message::Text(json!({"id":request["id"],"result":result}).to_string())).await.unwrap();
                if method=="turn/interrupt" {
                    socket.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":"unrelated","turn":{"id":"owned-turn","status":"completed"}}}).to_string())).await.unwrap();
                    socket.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":"existing","turn":{"id":"owned-turn","status":"interrupted"}}}).to_string())).await.unwrap();
                }
            }
        });
        let socket=WebSocketStream::from_raw_socket(client,tokio_tungstenite::tungstenite::protocol::Role::Client,None).await;
        (bridge_with_framing(socket,true),calls)
    }
    #[tokio::test]
    async fn listing_includes_only_loaded_controllable_threads_for_the_exact_directory() {
        let (transport, calls) = fixture(true).await;
        let sessions = list_sessions(&transport, "C:/owned").await.unwrap();
        assert_eq!(sessions.iter().map(|session|session["sessionId"].as_str().unwrap()).collect::<Vec<_>>(),
            ["existing", "second"]);
        assert_eq!(*calls.lock().unwrap(), ["initialize", "initialized", "thread/loaded/list",
            "thread/read", "thread/read", "thread/read", "thread/read"]);
        transport.shutdown();
    }

    #[tokio::test]
    async fn unloaded_history_never_resumes_or_creates_a_terminal_agent(){
        let (transport,calls)=fixture(false).await;
        let result=Conversation::load(transport,"existing".into(),"C:/owned".into(),Arc::new(|_|Ok(())),Arc::new(|_|Box::pin(async{None}))).await;
        assert!(result.is_err());
        assert_eq!(*calls.lock().unwrap(),vec!["initialize","initialized","thread/read"]);
    }
    #[tokio::test]
    async fn completion_waits_for_exact_thread_and_turn_and_cancel_is_protocol_only(){
        let (transport,calls)=fixture(true).await;
        let updates=Arc::new(Mutex::new(Vec::new()));let observed=updates.clone();
        let session=Conversation::load(transport,"existing".into(),"C:/owned".into(),Arc::new(move|update|{observed.lock().unwrap().push(update.clone());Ok(())}),Arc::new(|_|Box::pin(async{None}))).await.unwrap();
        session.finish_replay().unwrap();
        assert_eq!(updates.lock().unwrap()[0]["update"]["item"]["text"],"existing history");
        let completed=Arc::new(AtomicBool::new(false));let done=completed.clone();
        let mut pending=session.prompt_observed("same conversation".into(),Some(Arc::new(move||{done.store(true,Ordering::Release);}))).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2),async{loop{if session.inner.turn.lock().unwrap().id.is_some(){break;}tokio::task::yield_now().await;}}).await.unwrap();
        assert!(matches!(pending.try_recv(),Err(oneshot::error::TryRecvError::Empty)),"turn/start is only a receipt");
        assert!(!completed.load(Ordering::Acquire));assert!(session.prompt_observed("duplicate".into(),None).is_err());
        session.cancel().await.unwrap();
        let outcome=tokio::time::timeout(std::time::Duration::from_secs(2),pending).await.unwrap().unwrap().unwrap();
        assert_eq!(outcome["stopReason"],"cancelled");assert!(completed.load(Ordering::Acquire));
        assert_eq!(calls.lock().unwrap().iter().filter(|method|method.as_str()=="turn/start").count(),1);
        assert!(!calls.lock().unwrap().iter().any(|method|matches!(method.as_str(),"thread/start"|"thread/fork")));
        session.disconnect();
    }
}
