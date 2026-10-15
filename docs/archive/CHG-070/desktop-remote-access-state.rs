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
    /// Point the blacklist at its on-disk JSON and load existing entries.
    /// Called once at Tauri setup when the app data dir is known.
    pub fn set_path_and_load(&self, path: std::path::PathBuf) {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<BlacklistEntry>>(&s) {
                *self.entries.lock() = list;
            }
        }
        *self.path.lock() = Some(path);
    }

    fn save(&self) {
        let path = self.path.lock().clone();
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(s) = serde_json::to_string_pretty(&*self.entries.lock()) {
                let _ = std::fs::write(&path, s);
            }
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
    pub fn add(&self, entry: BlacklistEntry) {
        {
            let mut entries = self.entries.lock();
            entries.retain(|e| {
                let same_dev = match (&e.device_id, &entry.device_id) {
                    (Some(a), Some(b)) => !a.is_empty() && a == b,
                    _ => false,
                };
                let same_ip = match (&e.ip, &entry.ip) {
                    (Some(a), Some(b)) => !a.is_empty() && a == b,
                    _ => false,
                };
                !(same_dev || same_ip)
            });
            entries.push(entry);
        }
        self.save();
    }

    /// Remove an entry by its UUID; returns whether anything was removed.
    pub fn remove(&self, id: &str) -> bool {
        let removed = {
            let mut entries = self.entries.lock();
            let before = entries.len();
            entries.retain(|e| e.id != id);
            entries.len() != before
        };
        if removed {
            self.save();
        }
        removed
    }
}

