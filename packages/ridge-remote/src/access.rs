//! Shared Host connection registry and persistent blacklist.
use std::collections::HashMap;
use std::sync::{Arc,atomic::AtomicBool};
/// Information about a connected remote client (WebSocket).
#[derive(Clone, Debug)]
pub struct RemoteClientInfo {
    pub id: u64,
    pub connected_at: std::time::SystemTime,
    pub remote_addr: String,
    pub user_agent: String,
    /// Stable, mobile-generated device id (localStorage UUID) sent on connect.
    /// Used as the blacklist key (survives token rotation). Empty if absent.
    pub device_id: String,
    /// The session token this connection authenticated with (if it connected
    /// via `?token=`). Force-disconnect invalidates it so the device must
    /// re-enter the auth code to reconnect.
    pub token: Option<String>,
    pub kill_flag: Arc<AtomicBool>,
}

/// Registry tracking all currently connected remote WebSocket clients.
/// Used by the desktop RemotePanel to list connected devices and
/// forcibly disconnect them.
#[derive(Default)]
pub struct RemoteClientRegistry {
    pub clients: parking_lot::Mutex<HashMap<u64, RemoteClientInfo>>,
}

impl RemoteClientRegistry {
    pub fn register(
        &self,
        addr: String,
        ua: String,
        device_id: String,
        token: Option<String>,
    ) -> (u64, Arc<AtomicBool>) {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let kill_flag = Arc::new(AtomicBool::new(false));
        self.clients.lock().insert(
            id,
            RemoteClientInfo {
                id,
                connected_at: std::time::SystemTime::now(),
                remote_addr: addr,
                user_agent: ua,
                device_id,
                token,
                kill_flag: Arc::clone(&kill_flag),
            },
        );
        (id, kill_flag)
    }

    pub fn unregister(&self, id: u64) {
        self.clients.lock().remove(&id);
    }

    pub fn list(&self) -> Vec<RemoteClientInfo> {
        self.clients.lock().values().cloned().collect()
    }

    /// Snapshot of a single client's info (for resolving token/device on
    /// disconnect / blacklist).
    pub fn info_of(&self, id: u64) -> Option<RemoteClientInfo> {
        self.clients.lock().get(&id).cloned()
    }

    pub fn kick(&self, id: u64) -> bool {
        let map = self.clients.lock();
        if let Some(info) = map.get(&id) {
            info.kill_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

/// A persistent blacklist entry. A connection is barred if its device id OR its
/// IP matches a non-empty field of any entry.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlacklistEntry {
    /// UUID for stable UI keying / removal.
    pub id: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    /// Display label (short device id or IP).
    pub label: String,
    /// Unix seconds when added.
    pub added_at: u64,
}

/// Persistent blacklist of devices/IPs barred from the remote server. Backed by
/// a JSON file under the app data dir; enforced at `/verify` and the `/ws`
/// upgrade. Survives token rotation by keying on the mobile-provided device id
/// (and/or IP).
#[derive(Default)]
pub struct RemoteBlacklist {
    entries: parking_lot::Mutex<Vec<BlacklistEntry>>,
    path: parking_lot::Mutex<Option<std::path::PathBuf>>,
}

impl RemoteBlacklist {
    pub fn open(path: std::path::PathBuf)->Result<Self,String> {
        let entries=match std::fs::read(&path) {
            Ok(raw)=>serde_json::from_slice::<Vec<BlacklistEntry>>(&raw).map_err(|e|format!("invalid remote blacklist; original preserved: {e}"))?,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Vec::new(),
            Err(e)=>return Err(e.to_string()),
        };
        Ok(Self {entries:parking_lot::Mutex::new(entries),path:parking_lot::Mutex::new(Some(path))})
    }
    fn persist(&self,entries:&[BlacklistEntry])->Result<(),String> {
        let Some(path)=self.path.lock().clone() else {return Ok(());};
        // Refuse overwriting damaged data introduced since startup.
        let previous=match std::fs::read(&path) {
            Ok(raw)=>{
                serde_json::from_slice::<Vec<BlacklistEntry>>(&raw).map_err(|e|format!("invalid remote blacklist; original preserved: {e}"))?;
                serde_json::from_slice::<Vec<serde_json::Value>>(&raw).map_err(|e|e.to_string())?
            }
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Vec::new(),
            Err(e)=>return Err(e.to_string()),
        };
        if let Some(parent)=path.parent() {std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
        let mut next=serde_json::to_value(entries).map_err(|e|e.to_string())?;
        for entry in next.as_array_mut().ok_or("invalid blacklist projection")? {
            if let Some(original)=previous.iter().find(|original|original["id"]==entry["id"]).and_then(|original|original.as_object()) {
                let mut merged=original.clone();
                merged.extend(entry.as_object().ok_or("invalid blacklist entry")?.clone());
                *entry=serde_json::Value::Object(merged);
            }
        }
        let raw=serde_json::to_vec_pretty(&next).map_err(|e|e.to_string())?;
        let tmp=path.with_extension("tmp");
        std::fs::write(&tmp,raw).map_err(|e|e.to_string())?;
        std::fs::rename(&tmp,&path).map_err(|e|e.to_string())
    }
    pub fn add_checked(&self,entry:BlacklistEntry)->Result<(),String> {
        let mut entries=self.entries.lock();let mut next=entries.clone();
        next.retain(|e| !(entry.device_id.as_ref().is_some_and(|d|!d.is_empty() && e.device_id.as_ref()==Some(d))
            || entry.ip.as_ref().is_some_and(|ip|!ip.is_empty() && e.ip.as_ref()==Some(ip))));
        next.push(entry);self.persist(&next)?;*entries=next;Ok(())
    }
    pub fn remove_checked(&self,id:&str)->Result<bool,String> {
        let mut entries=self.entries.lock();let mut next=entries.clone();next.retain(|entry|entry.id!=id);
        if next.len()==entries.len() {return Ok(false);}
        self.persist(&next)?;*entries=next;Ok(true)
    }

    /// Point the blacklist at its on-disk JSON and load existing entries.
    /// Called once at Tauri setup when the app data dir is known.
    pub fn set_path_and_load(&self,path:std::path::PathBuf) {
        match Self::open(path) {
            Ok(next)=>{*self.entries.lock()=next.entries.into_inner();*self.path.lock()=next.path.into_inner();}
            Err(error)=>tracing::warn!(%error,"remote blacklist load failed"),
        }
    }

    pub fn list(&self) -> Vec<BlacklistEntry> {
        self.entries.lock().clone()
    }

    /// True if this device id or IP is blacklisted (empty values never match).
    pub fn is_blocked(&self, device_id: &str, ip: &str) -> bool {
        self.entries.lock().iter().any(|e| {
            e.device_id
                .as_deref()
                .is_some_and(|d| !d.is_empty() && d == device_id)
                || e.ip.as_deref().is_some_and(|i| !i.is_empty() && i == ip)
        })
    }

    /// Add an entry, de-duplicating by matching device id / IP, then persist.
    pub fn add(&self,entry:BlacklistEntry) {
        if let Err(error)=self.add_checked(entry) {tracing::warn!(%error,"remote blacklist save failed");}
    }
    pub fn remove(&self,id:&str)->bool {
        match self.remove_checked(id) {Ok(removed)=>removed,Err(error)=>{tracing::warn!(%error,"remote blacklist save failed");false}}
    }
}
