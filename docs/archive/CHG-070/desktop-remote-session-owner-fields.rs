    /// Session token store for mobile client authentication.
    /// Tokens are created via POST /verify and validated via WS ?token=.
    /// Each token expires after 3 days of inactivity.
    pub remote_session_store: Arc<SessionStore>,
    /// Brute-force throttle for TOTP verification (audit C1). Tracks failed
    /// `/verify` (+ `?code=` WS) attempts per IP and per device id, applying
    /// exponential backoff then a temp-ban; shared across both auth entry points.
    pub remote_verify_throttle: Arc<VerifyThrottle>,
    /// Registry of currently connected remote WebSocket clients.
    /// Used by the desktop RemotePanel to list + disconnect devices.
    pub remote_client_registry: Arc<RemoteClientRegistry>,
    /// Persistent blacklist of devices/IPs barred from connecting. Loaded from
    /// `<app_data_dir>/remote-blacklist.json` at startup.
    pub remote_blacklist: Arc<RemoteBlacklist>,
