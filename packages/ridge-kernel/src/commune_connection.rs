//! One Commune queue and history owner, with protocol-specific wire drivers.
use ridge_acp::{existing::ExistingConversation, transport::CallCompletion, AcpErrorWrap};
use serde_json::Value;
use tokio::sync::oneshot;

pub(crate) enum Conversation {
    Acp(ExistingConversation),
    AppServer(crate::app_server::Conversation),
}
impl Conversation {
    pub fn is_connected(&self) -> bool { match self { Self::Acp(s)=>s.is_connected(),Self::AppServer(s)=>s.is_connected() } }
    pub fn is_busy(&self) -> bool { match self { Self::Acp(s)=>s.is_busy(),Self::AppServer(s)=>s.is_busy() } }
    pub fn agent_info(&self) -> Option<&Value> { match self { Self::Acp(s)=>s.agent_info(),Self::AppServer(s)=>Some(s.agent_info()) } }
    pub fn history_replayed(&self) -> bool { match self { Self::Acp(s)=>s.history_replayed(),Self::AppServer(_)=>true } }
    pub fn disconnect(&self) { match self { Self::Acp(s)=>s.disconnect(),Self::AppServer(s)=>s.disconnect() } }
    pub fn finish_replay(&self)->Result<(),String>{ match self {Self::Acp(_)=>Ok(()),Self::AppServer(s)=>s.finish_replay()} }
    pub fn retire_idle(&self, commit: impl FnOnce()->Result<(),String>)->Result<(),String> { match self { Self::Acp(s)=>s.retire_idle(commit),Self::AppServer(s)=>s.retire_idle(commit) } }
    pub fn prompt_observed(&self,text:String,completion:Option<CallCompletion>)->Result<oneshot::Receiver<Result<Value,AcpErrorWrap>>,AcpErrorWrap> { match self { Self::Acp(s)=>s.prompt_observed(text,completion),Self::AppServer(s)=>s.prompt_observed(text,completion) } }
    pub async fn cancel(&self)->Result<(),AcpErrorWrap> { match self { Self::Acp(s)=>s.cancel().await,Self::AppServer(s)=>s.cancel().await } }
}
