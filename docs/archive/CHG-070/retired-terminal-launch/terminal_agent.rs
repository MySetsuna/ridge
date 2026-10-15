//! User-command terminal integration. The sidebar only attaches existing sessions.
//! One user invocation starts one protocol backend and its terminal UI. The relay
//! learns the exact thread from that UI's RPC response, never from a name or CWD.
use anyhow::{anyhow,Context,Result};
use base64::Engine;
use futures::{SinkExt,StreamExt};
use ridge_core::process_identity::{self,ProcessInstance};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},net::{Ipv4Addr,SocketAddrV4},path::PathBuf,sync::{Arc,atomic::{AtomicBool,AtomicU64,Ordering}},time::Duration};
use tokio::{net::{TcpListener,TcpStream},process::Command,sync::watch};
use tokio_tungstenite::{tungstenite::{client::IntoClientRequest,handshake::server::{Request,Response,ErrorResponse},Message},WebSocketStream};

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Config {
    pub protocol:String,pub server_args:Vec<String>,pub remote_args:Vec<String>,
    pub passthrough:Vec<String>,pub forward_options:Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {kind:String,command:String,shell:PathBuf,config:Config,arguments:Vec<String>}

pub(crate) fn environment(workspace:Option<uuid::Uuid>,pane:uuid::Uuid,prior:&HashMap<String,String>)->HashMap<String,String>{
    let mut env=prior.clone();
    if let Some(workspace)=workspace {
        env.insert("RIDGE_COMMUNE_WORKSPACE".into(),workspace.to_string());env.insert("RIDGE_COMMUNE_PANE".into(),pane.to_string());
    }
    env
}
pub(crate) fn powershell_hook()->String {
    let context=||->Result<String>{
        let profiles=crate::agent_profiles::catalog("list_agent_profiles",&json!({})).map_err(|e|anyhow!(e))?;
        let encoded=base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&profiles)?);
        let kernel=std::env::current_exe()?.to_string_lossy().replace('\'',"''");
        Ok(format!("if ($env:RIDGE_COMMUNE_PANE) {{\n$__ridgeTerminalProfiles = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}')) | ConvertFrom-Json\n$__ridgeTerminalKernel = '{kernel}'\n{}\n}}",include_str!("terminal_agent.ps1")))
    };
    context().unwrap_or_else(|_|"Write-Warning 'Ridge Agent 清单不可读；终端继续运行，协议启动集成未加载。'".into())
}
fn encode_script(script:&str)->String {base64::engine::general_purpose::STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>())}
fn command(invocation:&Invocation,args:Vec<String>)->Result<Command>{
    let data=base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&json!({"kind":invocation.kind,"command":invocation.command,"arguments":args}))?);
    let script=format!("$ErrorActionPreference='Stop'; $v=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{data}'))|ConvertFrom-Json; $a=@($v.arguments|ForEach-Object{{[string]$_}}); if($v.kind -eq 'function'){{ & ([scriptblock]::Create($v.command)) @a }}else{{ & $v.command @a }}; if($null -ne $LASTEXITCODE){{exit $LASTEXITCODE}}");
    let mut command=Command::new(&invocation.shell);
    command.args(["-NoLogo","-NoProfile","-EncodedCommand",&encode_script(&script)]);
    command.env_remove("RIDGE_COMMUNE_LAUNCH");Ok(command)
}
struct OwnedChild {child:tokio::process::Child,instance:ProcessInstance}
impl OwnedChild {
    fn spawn(mut command:Command)->Result<Self>{let child=command.spawn()?;let instance=process_identity::instance(child.id().context("child PID missing")?)?;Ok(Self{child,instance})}
}
impl Drop for OwnedChild {fn drop(&mut self){if process_identity::validate(&self.instance).is_ok(){ridge_core::process_guard::kill_process_tree(self.instance.pid);}let _=self.child.start_kill();}}
struct SecretFile(PathBuf);
impl Drop for SecretFile {fn drop(&mut self){let _=std::fs::remove_file(&self.0);}}
#[derive(Clone,Debug,PartialEq,Eq)]
struct Selected {thread:String,cwd:String}
struct Relay {
    endpoint:SocketAddrV4,token:String,server:ProcessInstance,
    selected:watch::Sender<Option<Selected>>,terminal_connected:AtomicBool,reconnect:AtomicU64,
}
fn forwarded(config:&Config,args:&[String])->Vec<String>{
    let mut output=Vec::new();let mut index=0;
    while index<args.len(){let arg=&args[index];
        if config.forward_options.contains(arg){output.push(arg.clone());if let Some(value)=args.get(index+1){output.push(value.clone());index+=1;}}
        else if config.forward_options.iter().any(|option|arg.starts_with(&format!("{option}="))){output.push(arg.clone());}
        index+=1;
    }output
}
fn expanded(args:&[String],endpoint:&str,token:&std::path::Path)->Vec<String>{args.iter().map(|arg|arg.replace("{endpoint}",endpoint).replace("{tokenFile}",&token.to_string_lossy())).collect()}

/// Explicitly invoked by the function installed in a Ridge-owned terminal.
/// Sidebar registration/dispatch/history never enter this launch path.
pub async fn run()->Result<i32>{
    let workspace=uuid::Uuid::parse_str(&std::env::var("RIDGE_COMMUNE_WORKSPACE").context("Ridge terminal context missing")?)?;
    let pane=uuid::Uuid::parse_str(&std::env::var("RIDGE_COMMUNE_PANE").context("Ridge pane context missing")?)?;
    let encoded=std::env::var("RIDGE_COMMUNE_LAUNCH").context("terminal invocation missing")?;
    anyhow::ensure!(encoded.len()<=65536,"terminal invocation exceeds budget");
    let invocation:Invocation=serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(encoded)?)?;
    anyhow::ensure!(invocation.config.protocol=="app-server"&&matches!(invocation.kind.as_str(),"function"|"path")&&invocation.shell.is_absolute(),"unsupported terminal launch descriptor");
    let kernel=crate::registry::read_endpoint().context("existing Ridge Runtime is unavailable")?;
    let identity=process_identity::instance(std::process::id())?;
    // Establish the exact pane process scope before starting anything.
    let ptys=crate::client::request_json(&kernel,"GET","/v1/domain/ptys",None).map_err(|e|anyhow!(e))?;
    let root=ptys["ptys"].as_array().and_then(|rows|rows.iter().find(|row|row["pty_id"].as_str()==Some(&pane.to_string())&&row["workspace_id"].as_str()==Some(&workspace.to_string()))).context("invocation is outside its declared Ridge pane")?;
    let root=ProcessInstance{pid:root["child_pid"].as_u64().context("pane process missing")? as u32,start_instance:root["child_start_instance"].as_str().context("pane instance missing")?.into()};
    anyhow::ensure!(process_identity::descendant(&root,&identity)?,"terminal launcher must belong to the exact pane process tree");
    let directory=std::env::temp_dir().join(format!("ridge-commune-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&directory)?;
    let token=format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());let secret=SecretFile(directory.join("session-token"));
    let mut options=std::fs::OpenOptions::new();options.create_new(true).write(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    use std::io::Write;options.open(&secret.0)?.write_all(token.as_bytes())?;
    let reservation=std::net::TcpListener::bind((Ipv4Addr::LOCALHOST,0))?;
    let endpoint=SocketAddrV4::new(Ipv4Addr::LOCALHOST,reservation.local_addr()?.port());drop(reservation);
    let mut server_args=forwarded(&invocation.config,&invocation.arguments);server_args.extend(expanded(&invocation.config.server_args,&format!("ws://{endpoint}"),&secret.0));
    let mut server_command=command(&invocation,server_args)?;
    server_command.stdin(std::process::Stdio::null()).stdout(std::fs::File::create(directory.join("server.out"))?).stderr(std::fs::File::create(directory.join("server.err"))?);
    #[cfg(windows)] {server_command.creation_flags(0x08000000);}
    let _server=OwnedChild::spawn(server_command)?;
    let listener=TcpListener::bind((Ipv4Addr::LOCALHOST,0)).await?;let proxy=listener.local_addr()?;
    let (selected,_)=watch::channel(None);
    let relay=Arc::new(Relay{endpoint,token:token.clone(),server:_server.instance.clone(),selected,terminal_connected:AtomicBool::new(false),reconnect:AtomicU64::new(0)});
    // An occupied port or unrelated local server is never accepted as this backend.
    tokio::time::timeout(Duration::from_secs(15),async {loop{match upstream(&relay).await{Ok(socket)=>{drop(socket);break Ok::<_,anyhow::Error>(());},Err(_)=>tokio::time::sleep(Duration::from_millis(100)).await}}}).await.context("protocol backend did not become ready")??;
    let task=tokio::spawn({let relay=relay.clone();async move {while let Ok((socket,_))=listener.accept().await{let relay=relay.clone();tokio::spawn(async move {if let Err(error)=relay_socket(socket,relay).await{tracing::debug!(%error,"terminal protocol connection ended");}});}}});
    let registration=tokio::spawn({let relay=relay.clone();let mut selected=relay.selected.subscribe();let secret=secret.0.clone();let identity=identity.clone();async move {
        let mut registered=None;let mut epoch=0;
        loop {
            let current=relay.reconnect.load(Ordering::Acquire);
            if epoch!=current{epoch=current;registered=None;}
            let selection=selected.borrow_and_update().clone();
            if let Some(selection)=selection {
                if registered.as_ref()!=Some(&selection){
                    let args=json!({"automatic":true,"protocol":"app-server","workspaceId":workspace,"paneId":pane,"process":identity,"endpointOwner":identity,"endpoint":proxy.to_string(),"bearerTokenFile":secret,"sessionId":selection.thread,"cwd":selection.cwd});
                    let kernel=kernel.clone();
                    match tokio::task::spawn_blocking(move||crate::client::request_json(&kernel,"POST","/v1/domain/commune/register",Some(&args))).await {
                        Ok(Ok(reply)) if reply["ok"]==true=>registered=Some(selection),
                        Ok(Ok(reply)) if reply["error"].as_str().is_some_and(|error|error.starts_with("protocol member was explicitly removed"))=>registered=Some(selection),
                        _=>{}, // Keep the user's terminal running; retry only attachment, never prompts.
                    }
                }
            }
            tokio::select!{value=selected.changed()=>{if value.is_err(){break;}},_=tokio::time::sleep(Duration::from_secs(3))=>{}}
        }
    }});
    let mut client_args=expanded(&invocation.config.remote_args,&format!("ws://{proxy}/terminal"),&secret.0);client_args.extend(invocation.arguments.clone());
    let mut client_command=command(&invocation,client_args)?;client_command.env("RIDGE_COMMUNE_SESSION_TOKEN",token);
    let mut terminal=OwnedChild::spawn(client_command)?;
    // The TUI owns Ctrl-C. Do not let its console signal orphan the backend.
    let signals=tokio::spawn(async{while tokio::signal::ctrl_c().await.is_ok(){}});
    let result=terminal.child.wait().await;
    signals.abort();registration.abort();task.abort();
    Ok(result?.code().unwrap_or(1))
}

async fn upstream(relay:&Relay)->Result<WebSocketStream<TcpStream>>{
    let stream=TcpStream::connect(relay.endpoint).await?;
    let std::net::SocketAddr::V4(client)=stream.local_addr()? else {return Err(anyhow!("unexpected address family"));};
    let mut system=sysinfo::System::new();system.refresh_processes(sysinfo::ProcessesToUpdate::All);
    let mut descendants=HashSet::from([relay.server.pid]);
    loop{let before=descendants.len();for (pid,process) in system.processes(){if process.parent().is_some_and(|parent|descendants.contains(&parent.as_u32())){descendants.insert(pid.as_u32());}}if before==descendants.len(){break;}}
    anyhow::ensure!(descendants.into_iter().filter_map(|pid|process_identity::instance(pid).ok()).any(|candidate|process_identity::descendant(&relay.server,&candidate).unwrap_or(false)&&process_identity::owns_connection(&candidate,relay.endpoint,client).is_ok()),"protocol endpoint is not owned by this user invocation");
    let mut request=format!("ws://{}/",relay.endpoint).into_client_request()?;
    request.headers_mut().insert("authorization",format!("Bearer {}",relay.token).parse()?);
    Ok(tokio_tungstenite::client_async(request,stream).await?.0)
}
async fn relay_socket(stream:TcpStream,relay:Arc<Relay>)->Result<()> {
    let terminal=Arc::new(AtomicBool::new(false));let kind=terminal.clone();let token=relay.token.clone();
    let config=tokio_tungstenite::tungstenite::protocol::WebSocketConfig{max_message_size:Some(16*1024*1024),max_frame_size:Some(16*1024*1024),..Default::default()};
    let mut socket=tokio_tungstenite::accept_hdr_async_with_config(stream,move|request:&Request,response:Response|->std::result::Result<Response,ErrorResponse>{
        if !matches!(request.uri().path(),"/"|"/terminal")||request.headers().get("authorization").and_then(|value|value.to_str().ok())!=Some(&format!("Bearer {token}")){return Err(tokio_tungstenite::tungstenite::http::Response::builder().status(401).body(None).unwrap());}
        kind.store(request.uri().path()=="/terminal",Ordering::Release);Ok(response)
    },Some(config)).await?;
    let terminal=terminal.load(Ordering::Acquire);
    if terminal&&relay.terminal_connected.swap(true,Ordering::AcqRel){return Err(anyhow!("terminal UI already connected"));}
    struct TerminalLease(Arc<Relay>,bool);
    impl Drop for TerminalLease{fn drop(&mut self){if self.1{self.0.terminal_connected.store(false,Ordering::Release);self.0.selected.send_replace(None);}else{self.0.reconnect.fetch_add(1,Ordering::AcqRel);}}}
    let _lease=TerminalLease(relay.clone(),terminal);
    let mut upstream=upstream(&relay).await?;let mut selected=relay.selected.subscribe();let bound=selected.borrow().clone();
    let mut pending=HashSet::new();
    loop {
        tokio::select! {
            next=socket.next()=>{
                let Some(message)=next else {break;};let message=message?;
                if let Message::Text(text)=&message {
                    let value:Value=serde_json::from_str(text)?;
                    if terminal {
                        if matches!(value["method"].as_str(),Some("thread/start"|"thread/resume"|"thread/fork")){
                            anyhow::ensure!(pending.len()<128,"terminal thread request budget exceeded");pending.insert(value["id"].to_string());
                        }
                    }else if let Some(method)=value["method"].as_str(){
                        anyhow::ensure!(matches!(method,"initialize"|"initialized"|"thread/read"|"thread/resume"|"thread/turns/list"|"thread/items/list"|"turn/start"|"turn/interrupt"),"unsupported auxiliary protocol operation");
                        if !matches!(method,"initialize"|"initialized"){
                            anyhow::ensure!(value["params"]["threadId"].as_str().is_some_and(|thread|bound.as_ref().is_some_and(|selection|selection.thread==thread)),"auxiliary client addressed a different terminal conversation");
                        }
                    }
                }
                upstream.send(message).await?;
            }
            next=upstream.next()=>{
                let Some(message)=next else {break;};let message=message?;
                if terminal {if let Message::Text(text)=&message {
                    let value:Value=serde_json::from_str(text)?;
                    if pending.remove(&value["id"].to_string()) {
                        if let (Some(thread),Some(cwd))=(value["result"]["thread"]["id"].as_str(),value["result"]["thread"]["cwd"].as_str()) {
                            anyhow::ensure!(!thread.is_empty()&&thread.len()<=1024,"invalid terminal thread identity");
                            relay.selected.send_replace(Some(Selected{thread:thread.into(),cwd:cwd.into()}));
                        }
                    }
                }}
                socket.send(message).await?;
            }
            value=selected.changed(),if !terminal=>{
                if value.is_err()||*selected.borrow()!=bound{break;}
            }
        }
    }
    let _=socket.close(None).await;let _=upstream.close(None).await;Ok(())
}
