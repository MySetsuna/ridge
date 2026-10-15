//! Runtime-owned Cloud controller. Existing signaling, signed E2EE and trust wire formats.
use super::{chunk,e2ee,ice,manager,signaling::{Signaling,SignalMsg},rtc::{DataChannelIo,PeerInbound,PeerOutbound,WebRtcHost},mux,session_control};
use cloud_ws::tungstenite::Message;
use ridge_core::DeviceIdentity;
use serde::Deserialize;
use serde_json::{json,Value};
use std::{sync::Arc,time::Duration};
use tokio::sync::{mpsc,oneshot,Semaphore,OwnedSemaphorePermit};

#[derive(Clone,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Config {
    pub user_token:String,pub username:String,pub device_name:String,pub base_domain:String,
    #[serde(default)]pub plaintext:bool,pub code:String,
    #[serde(default)]pub legacy_fingerprint:Option<String>,
}
impl Config {
    fn host(&self)->manager::Config {manager::Config{device_token:self.user_token.clone(),username:self.username.clone(),device_name:self.device_name.clone(),base_domain:self.base_domain.clone(),root:None,plaintext:self.plaintext}}
    pub(crate) fn validate(&self)->Result<(),String>{
        self.host().signal_url()?;
        if self.legacy_fingerprint.as_deref().is_some_and(|pin|!super::device_pins::valid_fingerprint(pin)){return Err("invalid legacy Cloud fingerprint".into());}
        if self.code.len()!=6||!self.code.bytes().all(|byte|byte.is_ascii_digit()){return Err("Cloud TOTP must contain six digits".into());}Ok(())
    }
    fn signal_url(&self)->Result<String,String>{
        let mut url=url::Url::parse(&self.host().signal_url()?).map_err(|_|"invalid Cloud endpoint")?;
        url.set_query(None);url.query_pairs_mut().append_pair("token",&self.user_token).append_pair("role","controller").append_pair("cli",&uuid::Uuid::new_v4().to_string());Ok(url.into())
    }
}
const LIMIT:usize=ridge_term::terminal_protocol::MAX_FRAME_BYTES;
struct Packet {message:Message,_budget:OwnedSemaphorePermit}
pub(crate) struct Socket {tx:mpsc::Sender<Packet>,rx:mpsc::Receiver<Packet>,budget:Arc<Semaphore>,task:tokio::task::JoinHandle<()>,warning:Option<String>}
impl Drop for Socket {fn drop(&mut self){self.task.abort();}}
impl Socket {
    pub(crate) fn identity_warning(&self)->Option<&str>{self.warning.as_deref()}
    pub(crate) async fn send(&mut self,message:Message)->Result<(),String>{
        let size=match &message{Message::Text(text)=>text.len(),Message::Binary(bytes)|Message::Ping(bytes)|Message::Pong(bytes)=>bytes.len(),_=>1}.max(1);
        if size>LIMIT{return Err("Cloud frame exceeds bound".into());}
        let budget=self.budget.clone().acquire_many_owned(size as u32).await.map_err(|_|"Cloud connection closed")?;
        self.tx.send(Packet{message,_budget:budget}).await.map_err(|_|"Cloud connection closed".into())
    }
    pub(crate) async fn next(&mut self)->Option<Result<Message,String>>{self.rx.recv().await.map(|packet|Ok(packet.message))}
    pub(crate) async fn close(&mut self){self.task.abort();let _=(&mut self.task).await;self.budget.close();}
}
pub(crate) async fn open(config:Config,identity:Arc<DeviceIdentity>)->Result<Socket,String>{
    config.validate()?;
    #[cfg(not(feature="rtc"))]
    {let _=identity;return Err("Cloud RTC disabled in this Runtime build".into());}
    #[cfg(feature="rtc")]
    {
        let (tx,mut input)=mpsc::channel::<Packet>(64);let (output,rx)=mpsc::channel::<Packet>(64);
        let budget=Arc::new(Semaphore::new(2*LIMIT));let incoming=Arc::new(Semaphore::new(2*LIMIT));
        let (ready,started)=oneshot::channel();
        let task=tokio::spawn(async move {
            let mut ready=Some(ready);
            let result=run(config,identity,&mut input,output,incoming,&mut ready).await;
            if let Some(ready)=ready {let _=ready.send(result.and(Err("Cloud connection closed before authentication".into())));}
        });
        let mut socket=Socket{tx,rx,budget,task,warning:None};
        match tokio::time::timeout(Duration::from_secs(40),started).await {
            Ok(Ok(Ok(warning)))=>{socket.warning=warning;Ok(socket)},
            result=>{socket.close().await;Err(match result{Ok(Ok(Err(error)))=>error,_=>"Cloud controller connection timed out".into()})}
        }
    }
}
async fn encrypted(io:&DataChannelIo,crypto:&mut e2ee::Session,plain:&[u8],sequence:&mut u32)->Result<(),String>{
    let cipher=crypto.seal(plain).map_err(|_|"Cloud encryption failed")?;*sequence=sequence.wrapping_add(1);
    for wire in chunk::encode(&cipher,*sequence)?{io.tx.send(wire).await.map_err(|_|"Cloud data channel closed")?;}Ok(())
}
#[cfg(feature="rtc")]
async fn run(config:Config,identity:Arc<DeviceIdentity>,input:&mut mpsc::Receiver<Packet>,output:mpsc::Sender<Packet>,budget:Arc<Semaphore>,ready:&mut Option<oneshot::Sender<Result<Option<String>,String>>>)->Result<(),String>{
    let host=config.host();let http=reqwest::Client::builder().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).danger_accept_invalid_certs(host.local());
    let http=if host.local(){http.no_proxy()}else{http};let http=http.build().map_err(|_|"Cloud HTTP initialization failed")?;
    let ice=ice::fetch_ice_servers(&http,&config.user_token,&host.api_base()).await;
    let mut signaling=Signaling::<Value>::connect(&config.signal_url()?).await.map_err(|_|"Cloud signaling connection failed")?;
    let signals=signaling.sender();let (peer_tx,peer_rx)=mpsc::channel(64);let (rtc_tx,mut rtc_rx)=mpsc::channel(64);
    let mut io=WebRtcHost.offer(ice,peer_rx,rtc_tx).await.map_err(|_|"Cloud RTC initialization failed")?;
    let result=drive(&config,&identity,&mut signaling,&signals,peer_tx,&mut rtc_rx,&mut io,input,output,budget,ready).await;
    io.close().await;result
}
#[cfg(feature="rtc")]
#[allow(clippy::too_many_arguments)]
async fn drive(config:&Config,identity:&DeviceIdentity,signaling:&mut Signaling<Value>,signals:&super::signaling::SignalSender,peer:mpsc::Sender<PeerInbound>,rtc:&mut mpsc::Receiver<PeerOutbound>,io:&mut DataChannelIo,input:&mut mpsc::Receiver<Packet>,output:mpsc::Sender<Packet>,budget:Arc<Semaphore>,ready:&mut Option<oneshot::Sender<Result<Option<String>,String>>>)->Result<(),String>{
    let handshake=e2ee::Handshake::new();let own=handshake.public_bytes();io.tx.send(handshake.encode_frame()).await.map_err(|_|"Cloud channel closed")?;
    let mut handshake=Some(handshake);let mut crypto=None::<e2ee::Session>;let mut transcript=None::<Vec<u8>>;
    let mut ready_host=false;let mut pending=Vec::new();let mut hint=None::<[u8;32]>;let mut remote_key=None::<[u8;32]>;
    let mut sequence=0;let mut reassembler=chunk::Reassembler::default();let mut tick=tokio::time::interval(Duration::from_millis(100));
    let mut authorized=false;let mut trust_pending=false;let mut totp_pending=false;let mut signaling_live=true;let mut warning=None;let mut legacy_wait=None::<std::time::Instant>;let mut trust_started=None::<std::time::Instant>;
    loop {tokio::select!{
        _=tick.tick()=>{
            if io.closed(){return Err("Cloud RTC connection closed".into());}
            if let (Some(since),Some(key))=(legacy_wait,remote_key){
                match super::key_binding::decide_key_binding(&key,hint.as_ref().map(|key:&[u8;32]|key.as_slice()),since.elapsed()>=Duration::from_secs(3)){
                    super::key_binding::KeyBindingDecision::Reject=>return Err("Cloud key binding mismatch".into()),
                    super::key_binding::KeyBindingDecision::Accept=>{
                        encrypted(io,crypto.as_mut().ok_or("Cloud encryption unavailable")?,&mux::encode_control(&json!({"t":"totp-trust-hello","pub":session_control::b64_encode(&identity.public_bytes())})),&mut sequence).await?;
                        trust_pending=true;trust_started=Some(std::time::Instant::now());legacy_wait=None;
                    },super::key_binding::KeyBindingDecision::Wait=>{},
                }
            }
            if transcript.is_none()&&trust_pending&&trust_started.is_some_and(|since|since.elapsed()>=Duration::from_secs(2))&&!authorized{
                encrypted(io,crypto.as_mut().ok_or("Cloud encryption unavailable")?,&mux::encode_control(&json!({"t":"totp-verify","code":config.code})),&mut sequence).await?;trust_pending=false;totp_pending=true;
            }
        },
        frame=signaling.incoming.recv(),if signaling_live=>{
            let Some(frame)=frame else {if authorized{signaling_live=false;continue;}return Err("Cloud signaling closed before authentication".into());};
            if frame["t"]=="error"{return Err("Cloud signaling rejected connection".into());}
            if frame["t"]=="welcome"&&frame["peerPresent"]==true||frame["t"]=="peer-join"&&frame["role"]=="host"{
                if !ready_host{ready_host=true;signals.send(SignalMsg::E2eePubkey{pubkey:session_control::b64_encode(&own),cid:None}).await.map_err(|_|"Cloud signaling closed")?;
                    for signal in pending.drain(..){signals.send(signal).await.map_err(|_|"Cloud signaling closed")?;}}
            }
            if let Ok(signal)=serde_json::from_value::<SignalMsg>(frame){match signal{
                SignalMsg::Answer{sdp,..}=>peer.send(PeerInbound::Answer(sdp)).await.map_err(|_|"Cloud RTC signaling closed")?,
                SignalMsg::Ice{candidate,..}=>peer.send(PeerInbound::Ice(candidate)).await.map_err(|_|"Cloud RTC signaling closed")?,
                SignalMsg::E2eePubkey{pubkey,..}=>{let value=session_control::b64_decode_pubkey(&pubkey).ok_or("invalid Cloud key binding")?;if remote_key.is_some_and(|key|key!=value){return Err("Cloud key binding mismatch".into());}hint=Some(value);},
                SignalMsg::PeerLeave{..}=>return Err("Cloud host left".into()),_=>{},
            }}
        },
        signal=rtc.recv()=>{
            let signal=match signal{Some(PeerOutbound::Offer(sdp))=>SignalMsg::Offer{sdp,cid:None},Some(PeerOutbound::Ice(candidate))=>SignalMsg::Ice{candidate,cid:None},_=>return Err("Cloud RTC signaling closed".into())};
            if ready_host{signals.send(signal).await.map_err(|_|"Cloud signaling closed")?;}else{if pending.len()>=128{return Err("Cloud pending signaling bound reached".into());}pending.push(signal);}
        },
        packet=io.rx.recv()=>{
            let packet=packet.ok_or("Cloud data channel closed")?;
            if let Some(handshake)=handshake.take(){
                let host_pub=if packet.first()==Some(&e2ee::DEVICE_BOUND_TAG){
                    let (host_pub,id_pub,signature)=e2ee::parse_signed_frame(&packet).map_err(|_|"invalid Cloud signed handshake")?;
                    let mut context=e2ee::ID_BIND_DOMAIN.to_vec();context.extend_from_slice(&e2ee::build_id_bind_context(&host_pub,&own,&config.device_name,&config.username).map_err(|_|"Cloud host binding invalid")?);
                    if !ridge_core::device_identity::verify(&id_pub,&context,&signature){return Err("Cloud host signature invalid".into());}
                    warning=super::device_pins::check(&crate::registry::ridge_data_dir(),&format!("{}-{}",config.device_name,config.username),&ridge_core::device_identity::fingerprint_of(&id_pub),config.legacy_fingerprint.as_deref())?;
                    transcript=Some(e2ee::build_bind_transcript(&host_pub,&own));host_pub
                }else{let host_pub=e2ee::Handshake::parse_peer_frame(&packet).map_err(|_|"invalid Cloud handshake")?;legacy_wait=Some(std::time::Instant::now());host_pub};
                if hint.is_some_and(|key|key!=host_pub){return Err("Cloud key binding mismatch".into());}remote_key=Some(host_pub);
                let mut session=handshake.into_session(host_pub,e2ee::Dir::ControllerToHost).map_err(|_|"Cloud key derivation failed")?;
                if transcript.is_some(){encrypted(io,&mut session,&mux::encode_control(&json!({"t":"totp-trust-hello","pub":session_control::b64_encode(&identity.public_bytes())})),&mut sequence).await?;trust_pending=true;}
                crypto=Some(session);continue;
            }
            let Some(cipher)=reassembler.push(&packet)else{continue;};let session=crypto.as_mut().ok_or("Cloud encryption unavailable")?;
            let plain=session.open(&cipher).map_err(|_|"Cloud authenticated frame rejected")?;if plain.len()>LIMIT{return Err("Cloud plaintext exceeds bound".into());}
            if plain.first()==Some(&mux::channel::CONTROL){let control:Value=serde_json::from_slice(&plain[1..]).map_err(|_|"invalid Cloud session control")?;
                match control["t"].as_str(){
                    Some("totp-trust-challenge") if trust_pending=>{
                        let nonce=session_control::b64_decode_pubkey(control["nonce"].as_str().ok_or("missing Cloud trust challenge")?).ok_or("invalid Cloud trust challenge")?;
                        let mut message=session_control::TRUST_DOMAIN.to_vec();message.extend_from_slice(&nonce);if let Some(transcript)=&transcript{message.extend_from_slice(transcript);}
                        trust_started=None;
                        encrypted(io,session,&mux::encode_control(&json!({"t":"totp-trust-proof","sig":session_control::b64_encode(&identity.sign(&message))})),&mut sequence).await?;
                    },
                    Some("totp-trust-result") if trust_pending=>{trust_pending=false;
                        if control["trusted"]==true{authorized=true;}else{let control=if let Some(transcript)=&transcript{json!({"t":"totp-bind","tag":session_control::b64_encode(&ridge_core::totp::bind_tag_at(&config.code,transcript))})}else{json!({"t":"totp-verify","code":config.code})};encrypted(io,session,&mux::encode_control(&control),&mut sequence).await?;totp_pending=true;}
                    },
                    Some("totp-result") if totp_pending=>{totp_pending=false;if control["ok"]!=true{return Err("Cloud TOTP rejected".into());}authorized=true;},_=>{},
                }
                if authorized{if let Some(ready)=ready.take(){let _=ready.send(Ok(warning.take()));}}continue;
            }
            if !authorized{continue;}
            let message=match plain.first(){Some(&mux::channel::JSON)=>Message::Text(String::from_utf8(plain[1..].to_vec()).map_err(|_|"invalid Cloud JSON encoding")?),Some(&0x13)=>Message::Binary(plain),_=>continue};
            let size=match &message{Message::Text(text)=>text.len(),Message::Binary(bytes)=>bytes.len(),_=>1}.max(1);
            let permit=budget.clone().acquire_many_owned(size as u32).await.map_err(|_|"Cloud receive budget closed")?;
            output.send(Packet{message,_budget:permit}).await.map_err(|_|"Cloud receiver closed")?;
        },
        packet=input.recv(),if authorized=>{
            let Some(packet)=packet else{return Ok(());};let plain=match packet.message{
                Message::Text(text)=>{let mut plain=vec![mux::channel::JSON];plain.extend_from_slice(text.as_bytes());plain},Message::Binary(bytes)=>bytes,
                Message::Close(_)=>return Ok(()),Message::Ping(_)|Message::Pong(_)=>continue,_=>return Err("invalid Cloud outgoing frame".into()),
            };
            encrypted(io,crypto.as_mut().ok_or("Cloud encryption unavailable")?,&plain,&mut sequence).await?;
        },
    }}
}
