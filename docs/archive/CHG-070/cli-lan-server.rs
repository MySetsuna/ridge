pub async fn run(requested_port: u16) -> Result<()> {
    let endpoint = tokio::task::spawn_blocking(kernel_ctl::ensure_kernel_running)
        .await
        .context("kernel bootstrap task failed")?
        .map_err(anyhow::Error::msg)?;
    let port = if requested_port == 0 {
        config::lan_port()
    } else {
        requested_port
    };
    let lan_ip = config::detect_lan_ip();
    let machine_name = host_name();
    let serve_cfg = UaServeConfig::resolve_ui_dirs();
    let (listener, actual_port) = ridge_remote::server::bind_tcp(port)?;
    let tls_config = ridge_remote::server::resolve_tls(&lan_ip, &machine_name).await;
    let tls_enabled = tls_config.is_some();
    let registry_path = registry_path()?;
    let enabled_file = enabled_path(&registry_path);
    let registry = HostRegistry {
        schema: 1,
        pid: std::process::id(),
        port: actual_port,
        lan_ip: lan_ip.clone(),
        tls: tls_enabled,
        enabled: read_enabled_flag(&enabled_file),
        started_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default(),
    };
    write_registry(&registry_path, &registry)?;

    let totp = Arc::new(ridge_kernel::auth_client::remote_auth(endpoint.clone()));
    totp.call("remote_set_totp_identity", serde_json::json!({"username":config::totp_identity()})).map_err(anyhow::Error::msg)?;
    let remote_enabled = Arc::new(AtomicBool::new(registry.enabled));
    let enabled_state = remote_enabled.clone();
    tokio::spawn(async move {
        loop {
            enabled_state.store(read_enabled_flag(&enabled_file), Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    });
    let host: Arc<dyn RemoteHost> = Arc::new(KernelHost {
        endpoint,
        totp: totp.clone(),
        totp_throttle: Arc::new(ridge_remote::auth::VerifyThrottle::new()),
        sessions: Arc::new(SessionStore::new()),
            access: Default::default(),
        port: actual_port,
        lan_ip: lan_ip.clone(),
        machine_name: machine_name.clone(),
        serve_cfg,
        tls_enabled,
        remote_enabled,
    });
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let mdns = ridge_remote::mdns::spawn_mdns_broadcast(actual_port);

    eprintln!(
        "ridge host ready: {}://{}:{} (kernel pid={}, tls={})",
        if tls_enabled { "https" } else { "http" },
        lan_ip,
        actual_port,
        host_kernel_pid(&host),
        tls_enabled
    );
    // P0-12 (audit C13 fix): printing the raw 6-digit TOTP code to
    // stderr leaks the active seed for the next 30 seconds to any
    // session that captures stdout (CI logs, `script`, systemd
    // journal). The TUI shows the code on the dashboard; CLI exposes
    // it only when `--print-totp` is passed (or `RIDGE_PRINT_TOTP=1`).
    if std::env::var("RIDGE_PRINT_TOTP").ok().as_deref() == Some("1")
        || std::env::args().any(|a| a == "--print-totp")
    {
        eprintln!("TOTP: {}", totp.current_code());
    } else {
        tracing::info!(
            "host ready; TOTP code visible in the TUI dashboard (not printed to stderr). \
             Pass --print-totp or RIDGE_PRINT_TOTP=1 to print to stderr."
        );
    }

    let server = ridge_remote::server_app::run(
        host,
        Arc::new(ridge_remote::lan_trust::LanTrustState::new()),
        listener,
        tls_config,
        shutdown_rx,
        true,
    );
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => result.map(|_| ()),
        signal = tokio::signal::ctrl_c() => {
            if signal.is_ok() {
                let _ = shutdown_tx.send(());
            }
            server.await.map(|_| ())
        }
    };
    mdns.1.store(true, Ordering::Relaxed);
    let _ = mdns.0.join();
    clear_registry(&registry_path, registry.pid);
    result
}

