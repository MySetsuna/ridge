//! One encrypted controller connection over the common Runtime command and terminal paths.
use super::{chunk,e2ee,ice::IceServerConfig,key_binding::{decide_key_binding,KeyBindingDecision},mux,rtc::{HostPeer,PeerInbound,PeerOutbound,WebRtcHost},session_control,signaling::{SignalMsg,SignalSender},workspace_scope};
use crate::server::AppState;
use ridge_core::device_identity::DeviceBindingSigner;
use serde_json::{json,Value};
use std::{collections::HashMap,time::Duration};
use tokio::sync::{mpsc,oneshot};
use uuid::Uuid;
use futures::StreamExt;

pub async fn run(state:AppState,config:super::manager::Config,cid:String,signals:SignalSender,mut incoming:mpsc::Receiver<SignalMsg>,ice:Vec<IceServerConfig>,scope:Option<workspace_scope::Scope>,mut stop:oneshot::Receiver<()>,status:mpsc::Sender<(String,String)>)->Result<(),String> {
    let (in_tx,in_rx)=mpsc::channel(64);let (out_tx,mut out_rx)=mpsc::channel(64);
    let mut io=WebRtcHost.answer(ice,in_rx,out_tx).await.map_err(|error|error.to_string())?;
    let result=drive(&state,&config,&cid,&signals,&mut incoming,&in_tx,&mut out_rx,scope,&mut stop,&mut io,status).await;
    io.close().await;result
}
struct Streams(HashMap<(Uuid,Uuid),(u64,tokio::task::JoinHandle<()>)>);
impl Drop for Streams {fn drop(&mut self){for (_,task) in self.0.values(){task.abort();}}}
async fn send_cipher(io:&super::rtc::DataChannelIo,bytes:&[u8],message:&mut u32)->Result<(),String> {
    *message=message.wrapping_add(1);
    for wire in chunk::encode(bytes,*message)? {io.tx.send(wire).await.map_err(|_|"data channel closed")?;}Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn drive(state:&AppState,config:&super::manager::Config,cid:&str,signals:&SignalSender,incoming:&mut mpsc::Receiver<SignalMsg>,in_tx:&mpsc::Sender<PeerInbound>,out_rx:&mut mpsc::Receiver<PeerOutbound>,scope:Option<workspace_scope::Scope>,stop:&mut oneshot::Receiver<()>,io:&mut super::rtc::DataChannelIo,status:mpsc::Sender<(String,String)>)->Result<(),String> {
    let handshake=e2ee::Handshake::new();let host_pub=handshake.public_bytes();let mut handshake=Some(handshake);
    signals.send(SignalMsg::E2eePubkey {pubkey:session_control::b64_encode(&host_pub),cid:Some(cid.into())}).await.map_err(|_|"signaling closed")?;
    let mut crypto=None::<e2ee::Session>;let mut transcript=None;let mut peer_key=None::<[u8;32]>;let mut hint=None::<[u8;32]>;let mut bound=false;
    let started=std::time::Instant::now();let mut tick=tokio::time::interval(Duration::from_millis(100));
    let mut verified=scope.is_some();let mut trust=session_control::TrustState::default();let mut chunks=chunk::Reassembler::default();let mut message=0;
    let connection=Uuid::new_v4().to_string();
    let (control_tx,mut controls)=mpsc::channel(64);
    let (output_tx,mut output)=mpsc::channel::<(Uuid,Uuid,u64,Vec<u8>)>(16);let mut streams=Streams(HashMap::new());
    let mut events=state.business_events.subscribe();let mut watches=None;
    loop {
        tokio::select! {
            _=&mut *stop=>return Ok(()),
            _=tick.tick()=>{
                if io.closed(){return Err("Cloud RTC connection closed".into());}
                if state.remote_auth.current_identity()!=config.username {return Err("Cloud host identity changed".into());}
                if crypto.is_none()&&started.elapsed()>Duration::from_secs(30){return Err("Cloud handshake timed out".into());}
                if let Some(peer)=peer_key {
                    if !bound {match decide_key_binding(&peer,hint.as_ref().map(|hint:&[u8;32]|hint.as_slice()),started.elapsed()>Duration::from_secs(3)) {
                        KeyBindingDecision::Reject=>return Err("Cloud key binding mismatch".into()),KeyBindingDecision::Accept=>{bound=true;let _=status.send((cid.into(),"connected".into())).await;},KeyBindingDecision::Wait=>{},
                    }}
                }
                if trust.totp_failures>=session_control::MAX_TOTP_ATTEMPTS {return Err("Cloud authentication attempts exhausted".into());}
                if verified&&bound&&watches.is_none(){if let Ok(inner)=crate::remote_invoke::stream_connection(state,&connection,cid){
                    watches=Some(state.fs_watches.attach(inner.clone(),state.business_events.clone()));
                    use ridge_core::EventSink;state.business_events.emit(ridge_core::EventScope::Connection,&Some(inner),"workspace-list-changed",json!({"initial":true}));
                }}
            }
            signal=incoming.recv()=>match signal {
                Some(SignalMsg::Offer{sdp,..})=>in_tx.send(PeerInbound::Offer(sdp)).await.map_err(|_|"RTC signaling closed")?,
                Some(SignalMsg::Ice{candidate,..})=>in_tx.send(PeerInbound::Ice(candidate)).await.map_err(|_|"RTC signaling closed")?,
                Some(SignalMsg::E2eePubkey{pubkey,..})=>{hint=session_control::b64_decode_pubkey(&pubkey);if hint.is_none(){return Err("invalid Cloud key binding".into());}},
                Some(SignalMsg::PeerLeave{..})|None=>return Ok(()),_=>{},
            },
            signal=out_rx.recv()=>{
                let signal=match signal {Some(PeerOutbound::Answer(sdp))=>SignalMsg::Answer{sdp,cid:Some(cid.into())},Some(PeerOutbound::Ice(candidate))=>SignalMsg::Ice{candidate,cid:Some(cid.into())},Some(PeerOutbound::Offer(_))=>return Err("unexpected host RTC offer".into()),None=>return Err("RTC signaling closed".into())};
                signals.send(signal).await.map_err(|_|"signaling closed")?;
            }
            packet=io.rx.recv(),if crypto.is_none()||bound=>{
                let packet=packet.ok_or("data channel closed")?;
                if crypto.is_none() {
                    let _=status.send((cid.into(),"handshaking".into())).await;
                    let peer=e2ee::Handshake::parse_peer_frame(&packet).map_err(|error|error.to_string())?;
                    let context=e2ee::build_id_bind_context(&host_pub,&peer,&config.device_name,&config.username).map_err(|error|error.to_string())?;
                    let signature=state.device_identity.sign_binding(&context)?;
                    io.tx.send(e2ee::encode_signed_frame(&host_pub,&state.device_identity.public_bytes(),&signature)).await.map_err(|_|"data channel closed")?;
                    transcript=Some(e2ee::build_bind_transcript(&host_pub,&peer));peer_key=Some(peer);
                    crypto=Some(handshake.take().ok_or("handshake already consumed")?.into_session(peer,e2ee::Dir::HostToController).map_err(|error|error.to_string())?);
                    continue;
                }
                let Some(ciphertext)=chunks.push(&packet) else {continue;};
                let crypto=crypto.as_mut().ok_or("Cloud encryption unavailable")?;
                let plain=crypto.open(&ciphertext).map_err(|error|error.to_string())?;
                if plain.len()>4*1024*1024 {return Err("Cloud control frame too large".into());}
                match mux::demux(&plain) {
                    mux::Inbound::Control(body)=>{
                        if !bound {continue;}
                        if trust.totp_failures>=session_control::MAX_TOTP_ATTEMPTS {return Err("Cloud authentication attempts exhausted".into());}
                        session_control::handle_control(&body,crypto,&control_tx,state.remote_auth.as_ref(),&config.username,&mut verified,&mut trust,transcript.as_deref()).await.map_err(|error|error.to_string())?;
                        while let Ok(cipher)=controls.try_recv(){send_cipher(io,&cipher,&mut message).await?;}
                    }
                    mux::Inbound::Json(body)=>{
                        let request:Value=serde_json::from_slice(&body).map_err(|_|"invalid Cloud RPC")?;
                        let Some(method)=request["method"].as_str().or(request["type"].as_str()) else {continue;};
                        let args=if request["jsonrpc"]=="2.0"{request.get("params").cloned().unwrap_or_else(||json!({}))}else{request.clone()};
                        if method=="$/hello" {
                            let hello=ridge_remote::rpc::negotiate_hello_with_capabilities(&args,crate::host::implemented_capabilities());
                            let reply=if request.get("id").is_some(){ridge_remote::rpc::result_response(&request["id"],hello.get("params").cloned().unwrap_or(hello))}else{hello};
                            let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;continue;
                        }
                        if !verified||!bound {
                            if request.get("id").is_some(){let reply=json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32003,"message":"Cloud authentication required"}});let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;}continue;
                        }
                        if request["jsonrpc"]!="2.0"&&!matches!(method,"subscribe-pane"|"unsubscribe-pane"){
                            if let Some(root)=&config.root {if matches!(request["type"].as_str(),Some("data-request"|"invoke-request")){
                                let name=request["cmd"].as_str().or(request["method"].as_str()).unwrap_or("");let args=if request["type"]=="invoke-request"{&request["args"]}else{&request};
                                if let Err(error)=ridge_core::dispatch::validate_request(name,args,&crate::host_core::runtime_ctx(&[root.clone()],false)){
                                    let reply=json!({"type":if request["type"]=="data-request"{"data-result"}else{"invoke-result"},"_reqId":request["_reqId"],"_error":error.to_command_string()});
                                    let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;continue;
                                }
                            }}
                            let reply=if scope.is_some(){Some(json!({"type":"invoke-result","_reqId":request["_reqId"],"_error":"workspace grant requires scoped common RPC"}))}else{
                                match crate::remote_invoke::frame(state.clone(),connection.clone(),cid,request.clone()).await {
                                    Ok(reply)=>reply,Err(error)=>Some(json!({"type":"invoke-result","_reqId":request["_reqId"],"_error":error})),
                                }
                            };
                            if let Some(reply)=reply{let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;}continue;
                        }
                        let (method,args,result)=if let Some(scope)=&scope {
                            match workspace_scope::plan(state,scope,method,&args) {
                                Ok(plan)=>plan,
                                Err(reason)=>{
                                    if request.get("id").is_some(){let reply=json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32003,"message":reason,"data":{"kind":"scope_denied","workspaceId":scope.workspace_id}}});let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;}continue;
                                }
                            }
                        }else{(method.to_owned(),args,None)};
                        if matches!(method.as_str(),"subscribe-pane"|"unsubscribe-pane") {
                            let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or("missing terminal workspace")?).map_err(|_|"invalid terminal workspace")?;
                            let pane=Uuid::parse_str(args["paneId"].as_str().ok_or("missing terminal pane")?).map_err(|_|"invalid terminal pane")?;
                            let activation=args["activationId"].as_u64().filter(|id|*id>0).ok_or("invalid terminal activation")?;
                            crate::remote_invoke::execute(state.clone(),json!({"clientId":cid,"workspaceId":workspace,"paneId":pane,"activationId":activation,"active":method=="subscribe-pane"}),true,connection.clone(),true).await.map_err(|error|error["message"].as_str().unwrap_or("terminal binding failed").to_owned())?;
                            if method=="unsubscribe-pane" {
                                if streams.0.get(&(workspace,pane)).is_some_and(|(current,_)|*current==activation) {if let Some((_,task))=streams.0.remove(&(workspace,pane)){task.abort();}}
                            } else {
                                let endpoint=crate::registry::KernelEndpoint {pid:state.pid,port:state.port,token:state.token.clone(),started_at_unix:state.started_at_unix};
                                let identity=crate::remote_invoke::stream_connection(state,&connection,cid)?;
                                let mut stream=crate::client::connect_terminal_stream_for(endpoint,workspace,pane,activation,&identity).await?;
                                let target=output_tx.clone();
                                if let Some((_,old))=streams.0.remove(&(workspace,pane)){old.abort();}
                                let task=tokio::spawn(async move {while let Some(Ok(packet))=stream.next().await {if let tokio_tungstenite::tungstenite::Message::Binary(bytes)=packet {if target.send((workspace,pane,activation,bytes)).await.is_err(){break;}}}});
                                streams.0.insert((workspace,pane),(activation,task));
                            }
                            continue;
                        }
                        if let Some(root)=&config.root {
                            if let Err(error)=ridge_core::dispatch::validate_request(&method,&args,&crate::host_core::runtime_ctx(&[root.clone()],false)) {
                                if request.get("id").is_some(){let reply=json!({"jsonrpc":"2.0","id":request["id"],"error":error.to_json_rpc()});let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;}continue;
                            }
                        }
                        let result=if let Some(result)=result {Ok(result)}else{crate::remote_invoke::execute(state.clone(),json!({"clientId":cid,"method":method,"args":args}),true,connection.clone(),false).await};
                        if request.get("id").is_some() {
                            let reply=match result {Ok(result)=>json!({"jsonrpc":"2.0","id":request["id"],"result":result}),Err(error)=>json!({"jsonrpc":"2.0","id":request["id"],"error":error})};
                            let mut reply=reply;
                            if method=="list_workspaces" {if let Some(scope)=&scope {if let Some(rows)=reply["result"].as_array_mut(){rows.retain(|row|row["id"]==scope.workspace_id);}}}
                            let cipher=crypto.seal(&mux::encode_json(&reply)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;
                        }
                    }
                    _=>{},
                }
            }
            packet=output.recv()=>{
                if let Some((workspace,pane,activation,bytes))=packet {
                    if verified&&bound&&streams.0.get(&(workspace,pane)).is_some_and(|(current,_)|*current==activation) {
                        let cipher=crypto.as_mut().ok_or("Cloud encryption unavailable")?.seal(&bytes).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;
                    }
                }
            }
            event=events.recv(),if verified&&bound=>{
                let event=event.map_err(|_|"Cloud business event gap; reconnect and refresh required")?;
                let inner=crate::remote_invoke::stream_connection(state,&connection,cid).unwrap_or_default();
                if !crate::business::targets_connection(&event,&inner){continue;}
                if let Some(scope)=&scope{
                    if event["name"]!="workspace-list-changed"&&(event["payload"]["workspaceId"]!=scope.workspace_id||workspace_scope::plan(state,scope,"event",&event["payload"]).is_err()){continue;}
                }
                let Some(mut event)=crate::business::project_event(event,false)else{continue;};event["runtimeEpoch"]=json!(state.runtime_epoch);
                let cipher=crypto.as_mut().ok_or("Cloud encryption unavailable")?.seal(&mux::encode_json(&event)).map_err(|error|error.to_string())?;send_cipher(io,&cipher,&mut message).await?;
            }
        }
    }
}
