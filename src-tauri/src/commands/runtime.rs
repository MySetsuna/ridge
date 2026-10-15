//! Transitional registrations call Runtime and preserve errors; no desktop business fallback.
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use uuid::Uuid;
pub(crate) fn request<T: DeserializeOwned>(
    method: &str,
    args: Value,
    native: bool,
    connection: Uuid,
) -> Result<T, String> {
    let value = request_result(method,args,native,connection).map_err(|error|
        error["message"].as_str().unwrap_or("Runtime command failed").to_string())?;
    serde_json::from_value(value).map_err(|error|format!("invalid Runtime response: {error}"))
}
pub(crate) fn request_result(method: &str,args:Value,native:bool,connection:Uuid) -> Result<Value,Value> {
    let error=|message:String|ridge_core::CoreError::internal(message).to_json_rpc();
    let endpoint=crate::kernel_lifecycle::ensure_kernel_running().map_err(error)?;
    let response=ridge_kernel::client::request_json(&endpoint,"POST","/v1/domain/commands",
        Some(&json!({"method":method,"args":args,"native":native,"roots":[],"connectionId":connection}))).map_err(error)?;
    if response["ok"] != true {
        return Err(if response["error"]["code"].is_number() {response["error"].clone()} else {error("Runtime omitted structured error".into())});
    }
    Ok(response["value"].clone())
}

pub(crate) fn workspace_file<T: DeserializeOwned>(args: Value) -> Result<T, String> {
    let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
    let response = ridge_kernel::client::request_json(
        &endpoint,
        "POST",
        "/v1/domain/workspace-files",
        Some(&args),
    )?;
    if response["ok"] != true {
        return Err(response["error"]
            .as_str()
            .unwrap_or("Runtime workspace file operation failed")
            .into());
    }
    serde_json::from_value(response["value"].clone())
        .map_err(|error| format!("invalid Runtime workspace file response: {error}"))
}
/// One nonsecret OS client identity per shell window, reused by transitional registrations.
pub(crate) fn window_connection(label: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    static SHELL: std::sync::OnceLock<Uuid> = std::sync::OnceLock::new();
    let mut hash = Sha256::new();
    hash.update(SHELL.get_or_init(Uuid::new_v4).as_bytes());
    hash.update(label.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}
