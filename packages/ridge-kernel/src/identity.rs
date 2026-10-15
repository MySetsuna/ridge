//! Kernel owns the device key; callers receive only its public key or a domain-bound signature.
use crate::server::AppState;
use ridge_core::{CoreError, CoreResult, DeviceIdentity};
use serde_json::Value;
use std::sync::Arc;
use ridge_core::device_identity::DeviceBindingSigner;

pub(crate) fn initialize() -> Result<Arc<DeviceIdentity>, String> {
    let identity = if let Some(root) = std::env::var_os("RIDGE_CDP_NATIVE_DATA_DIR").filter(|_| cfg!(debug_assertions)) {
        let root = std::path::PathBuf::from(root);
        if !root.is_absolute() { return Err("CDP device identity namespace must be absolute".into()); }
        DeviceIdentity::load_or_create_at(&root.join("device_identity.key"))?
    } else if std::env::var_os("RIDGE_KERNEL_DATA_DIR").is_some_and(|root| !root.is_empty()) {
        DeviceIdentity::load_or_create_at(&crate::registry::ridge_data_dir().join("device_identity.key"))?
    } else { DeviceIdentity::load_or_create_checked()? };
    Ok(Arc::new(identity))
}

pub(crate) fn dispatch(state: &AppState, method: &str, args: &Value, native: bool) -> Option<CoreResult<Value>> {
    if !matches!(method, "get_device_identity_pub" | "sign_device_identity" | "initialize_cloud_controller_identity") { return None; }
    Some((|| {
        if !native { return Err(CoreError::InvalidArgs("device identity requires native trust".into())); }
        if method == "initialize_cloud_controller_identity" {
            let value = args.get("legacySeed").ok_or_else(|| CoreError::InvalidArgs("legacySeed evidence is required".into()))?;
            let seed = if value.is_null() { None } else {
                if !value.as_array().is_some_and(|bytes| bytes.len() == 32) { return Err(CoreError::InvalidArgs("legacy controller seed must contain exactly 32 bytes".into())); }
                Some(serde_json::from_value::<[u8;32]>(value.clone()).map_err(|_| CoreError::InvalidArgs("invalid legacy controller seed".into()))?)
            };
            return state.controller_identity.initialize(state, seed).map(|public| serde_json::json!(public)).map_err(CoreError::Io);
        }
        if method == "get_device_identity_pub" { return Ok(serde_json::json!(state.device_identity.public_bytes().to_vec())); }
        let context: Vec<u8> = serde_json::from_value(args["context"].clone()).map_err(|error| CoreError::InvalidArgs(error.to_string()))?;
        if context.len() > 65536 { return Err(CoreError::InvalidArgs("device binding context too large".into())); }
        Ok(serde_json::json!(state.device_identity.sign_binding(&context).map_err(CoreError::InvalidArgs)?.to_vec()))
    })())
}
