/// Explicit Remote stop controls Runtime; window detach never calls this.
pub fn stop_remote_server(_state:&AppState) {
    if let Err(error)=auth_request::<()>("set_remote_enabled",serde_json::json!({"enabled":false})) {tracing::warn!(%error,"Remote stop failed");}
}
