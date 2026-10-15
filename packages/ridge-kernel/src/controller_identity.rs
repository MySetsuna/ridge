//! Native controller migration retains the browser key; Host identity stays separate.
use crate::server::AppState;
use ridge_core::DeviceIdentity;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct ControllerIdentity(Mutex<Option<Arc<DeviceIdentity>>>);
fn path() -> std::path::PathBuf { crate::registry::ridge_data_dir().join("cloud-controller-identity.key") }
impl ControllerIdentity {
    pub(crate) fn get(&self) -> Result<Arc<DeviceIdentity>, String> {
        let mut current = self.0.lock().map_err(|_| "controller identity unavailable")?;
        if let Some(identity) = current.as_ref() { return Ok(identity.clone()); }
        let identity = Arc::new(DeviceIdentity::load_existing_at(&path())?.ok_or("controller identity migration required before Cloud connection")?);
        *current = Some(identity.clone()); Ok(identity)
    }
    pub(crate) fn initialize(&self, state: &AppState, legacy: Option<[u8;32]>) -> Result<Vec<u8>, String> {
        let mut current = self.0.lock().map_err(|_| "controller identity unavailable")?;
        let candidate = legacy.map(|seed| DeviceIdentity::from_seed(&seed));
        let existing = if let Some(identity) = current.as_ref() { Some(identity.clone()) }
            else { DeviceIdentity::load_existing_at(&path())?.map(Arc::new) };
        if let Some(identity) = existing {
            if candidate.as_ref().is_some_and(|candidate| candidate.public_bytes() != identity.public_bytes()) {
                return Err("controller identity migration conflict; original preserved".into());
            }
            *current = Some(identity.clone()); return Ok(identity.public_bytes().to_vec());
        }
        let prior_cloud = state.remote_hosts.lock().map_err(|_| "host topology unavailable")?
            .snapshot().iter().any(|host| host.id.starts_with("cloud:"));
        if prior_cloud && candidate.as_ref().is_some_and(|candidate| candidate.public_bytes() != state.device_identity.public_bytes()) {
            return Err("prior Runtime controller identity conflicts with browser identity; originals preserved".into());
        }
        let source = if prior_cloud { state.device_identity.as_ref() } else { candidate.as_ref().unwrap_or(state.device_identity.as_ref()) };
        let identity = Arc::new(source.persist_at(&path())?);
        let public = identity.public_bytes().to_vec(); *current = Some(identity); Ok(public)
    }
}
