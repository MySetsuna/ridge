//! Shared Runtime Cloud protocol implementation.
pub use ridge_kernel::cloud::session_control::*;

#[cfg(test)]
pub struct TestTotp(ridge_core::RemoteTotp);
#[cfg(test)]
impl TestTotp {
    pub fn new()->Self {Self(ridge_core::RemoteTotp::new())}
    pub fn current_bind_tag(&self,transcript:&[u8])->[u8;32] {self.0.current_bind_tag(transcript)}
    pub fn identity(&self)->String {self.0.identity()}
    pub fn current_code(&self)->String {self.0.current_code()}
}
#[cfg(test)]
impl ControlAuth for TestTotp {
    fn verify(&self,code:&str)->bool {self.0.verify(code)}
    fn verify_bind_tag(&self,transcript:&[u8],tag:&[u8])->bool {self.0.verify_bind_tag(transcript,tag)}
    fn identity(&self)->String {self.0.identity()}
    fn current_code(&self)->String {self.0.current_code()}
    fn otpauth_uri(&self,label:&str)->String {self.0.otpauth_uri(label)}
    fn trust_check(&self,identity:&str,key:&[u8])->bool {ridge_core::grant_store::check(identity,key)}
    fn trust_record(&self,identity:&str,key:&[u8]) {ridge_core::grant_store::record(identity,key)}
}
