//! Transitional Runtime approval client; no desktop pending registry or decision writer.
use serde_json::{json,Value};
use ridge_mcp::server::ExternalExecutionRejection;
pub fn is_enabled()->Result<bool,String>{crate::commands::runtime::request("get_hitl_enabled",json!({}),true,crate::commands::runtime::window_connection("hitl-gateway"))}
pub async fn submit_input(wid:uuid::Uuid,pane:uuid::Uuid,initiator:&str,text:&str)->Result<Value,String>{
 let args=json!({"workspaceId":wid,"paneId":pane,"initiator":initiator,"text":text});
 tokio::task::spawn_blocking(move||crate::commands::runtime::request("submit_gateway_input",args,true,crate::commands::runtime::window_connection("hitl-gateway"))).await.map_err(|error|error.to_string())?
}
/// External reports use Runtime; no local card or audit fallback survives here.
pub fn report_external_rejection(rejection: &ExternalExecutionRejection) -> Result<String, String> {
    crate::commands::runtime::request("report_execution_rejection", serde_json::json!({
        "initiator":rejection.initiator,"action":rejection.action,"executor":rejection.executor,
        "policySource":rejection.policy_source,"requestId":rejection.request_id,"reason":rejection.reason,"nextStep":rejection.next_step
    }), true, crate::commands::runtime::window_connection("execution-rejections"))
}
