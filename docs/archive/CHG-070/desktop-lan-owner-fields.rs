    /// Handle to the remote server background thread. `None` when the server
    /// is not running. Used to join the thread on shutdown / restart.
    pub remote_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    /// One-shot sender to signal the remote server to gracefully shut down.
    /// Drained (taken) after each start/stop cycle.
    pub remote_shutdown: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// Handle to the remote server dev-mode process (`pnpm dev:remote`).
    /// `None` when not in dev mode or server not running.
    pub remote_dev_process: Arc<Mutex<Option<std::process::Child>>>,
    /// mDNS broadcast thread handle + stop flag. `None` when the server
    /// is not running. Set the flag and join the handle to stop.
    pub remote_mdns: Arc<Mutex<Option<(std::thread::JoinHandle<()>, Arc<AtomicBool>)>>>,
