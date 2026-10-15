//! Runtime owns default cwd; shells retain only a read projection during migration.
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex};
static WRITES: Mutex<()> = Mutex::new(());
const FILE: &str = "runtime-settings.json";
fn read() -> Result<Value, String> {
    match std::fs::read(crate::workspace_history::directory().join(FILE)) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes).map_err(|error|
                format!("invalid Runtime settings; original preserved: {error}"))?;
            if !value.is_object() || value.get("defaultCwd").is_some_and(|path| !path.is_null() && !path.is_string())
                || value.get("hitlEnabled").is_some_and(|enabled| !enabled.is_boolean()) {
                return Err("invalid Runtime settings; original preserved".into());
            }
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(error.to_string()),
    }
}
pub(crate) fn default_cwd() -> Result<Option<String>, String> {
    let _guard = WRITES.lock().map_err(|_| "Runtime settings unavailable")?;
    Ok(read()?["defaultCwd"].as_str().map(str::to_string))
}
pub(crate) fn set(state: &crate::server::AppState, args: &Value, ctx: &ridge_core::Ctx) -> ridge_core::CoreResult<Value> {
    if args.get("path").is_some_and(|path| !path.is_null() && !path.is_string()) {
        return Err(ridge_core::CoreError::InvalidArgs("path must be a string or null".into()));
    }
    let path = ridge_core::commands::settings::normalize_default_cwd(args["path"].as_str().map(str::to_string));
    let path: Option<PathBuf> = path.map(|path| if path.is_absolute() {Ok(path)} else {
        std::env::current_dir().map(|cwd| cwd.join(path)).map_err(|error| ridge_core::CoreError::Io(error.to_string()))
    }).transpose()?;
    if path.as_ref().is_some_and(|path| !state.fs_scope.is_allowed(&path.to_string_lossy())
        || !ctx.capabilities().root_scope().is_allowed(&path.to_string_lossy())) {
        return Err(ridge_core::CoreError::OutsideSandbox);
    }
    let _guard = WRITES.lock().map_err(|_| ridge_core::CoreError::Io("Runtime settings unavailable".into()))?;
    let mut value = read().map_err(ridge_core::CoreError::Io)?;
    let next = json!(path);
    if value.get("defaultCwd") == Some(&next) {return Ok(Value::Null);}
    value["defaultCwd"] = next;
    crate::workspace_history::write(FILE, &serde_json::to_vec_pretty(&value).map_err(|error|
        ridge_core::CoreError::Io(error.to_string()))?).map_err(ridge_core::CoreError::Io)?;
    state.business_events.default_cwd_changed();
    Ok(Value::Null)
}

pub(crate) fn get(state: &crate::server::AppState, ctx: &ridge_core::Ctx) -> ridge_core::CoreResult<Value> {
    let _guard = WRITES.lock().map_err(|_| ridge_core::CoreError::Io("Runtime settings unavailable".into()))?;
    let value = read().map_err(ridge_core::CoreError::Io)?;
    if value["defaultCwd"].as_str().is_some_and(|path| !state.fs_scope.is_allowed(path)
        || !ctx.capabilities().root_scope().is_allowed(path)) {
        return Err(ridge_core::CoreError::OutsideSandbox);
    }
    Ok(json!({"configured":value.get("defaultCwd").is_some(),"path":value["defaultCwd"]}))
}

/// Shared durable preference; an absent legacy field retains the default-off gate.
pub(crate) fn gateway_enabled() -> Result<bool, String> {
    let _guard = WRITES.lock().map_err(|_| "Runtime settings unavailable")?;
    Ok(read()?["hitlEnabled"].as_bool().unwrap_or(false))
}
/// First import is conditional under the same writer lock as all settings commits.
pub(crate) fn update_gateway(args: &Value, initialize: bool) -> Result<bool, String> {
    let enabled = args["enabled"].as_bool().ok_or("enabled must be a boolean")?;
    let _guard = WRITES.lock().map_err(|_| "Runtime settings unavailable")?;
    let mut value = read()?;
    if initialize && value.get("hitlEnabled").is_some() {
        return value["hitlEnabled"].as_bool().ok_or_else(|| "invalid Runtime gateway setting".into());
    }
    if value["hitlEnabled"] != enabled {
        value["hitlEnabled"] = json!(enabled);
        crate::workspace_history::write(FILE, &serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?)?;
    }
    Ok(enabled)
}
