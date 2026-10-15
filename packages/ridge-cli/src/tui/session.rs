//! Shared presentation session boundary. Runtime owns PTYs and terminal state.
use anyhow::Result;

pub trait Session {
    fn input_ready(&self)->bool {true}
    fn failure(&self)->Option<String> {None}
    fn presentation(&self)->Option<Vec<u8>> {None}
    fn send_input(&self,data:&[u8])->Result<()>;
    fn resize(&self,cols:u16,rows:u16)->Result<()>;
    fn follow_tail(&self)->Option<Vec<u8>> {None}
    fn scroll_history(&self,_back:bool)->impl std::future::Future<Output=Result<Option<Vec<u8>>>>+Send {async{Ok(None)}}
}
