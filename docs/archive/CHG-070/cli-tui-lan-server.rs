pub async fn run(
    port: u16,
    totp: Arc<RemoteAuth>,
    workspace: SharedWorkspace,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let lan_ip = config::detect_lan_ip();
    let machine_name = host_name();
    // 运行时统一 `remote-dist` 产物根探测已下沉
    // 到共享 serve 层；缺失时回退移动 SPA 提示页。
    let serve_cfg = UaServeConfig::resolve_ui_dirs();

    // 先绑定拿到实际端口（9527 起探测至多 10 个），再据此填 HostMeta::port。
    let (std_listener, actual_port) = ridge_remote::server::bind_tcp(port)?;
    // 解析 TLS（CA 自签 leaf，覆盖本机 LAN IP + 机器名 SAN）。require_tls=true 下
    // 无 TLS 材料即 fail-closed（与旧 `server::serve(.., true)` 行为一致）。
    let tls_config = ridge_remote::server::resolve_tls(&lan_ip, &machine_name).await;
    let tls_enabled = tls_config.is_some();

    tracing::info!(
        target: "ridge_cli::lan_host",
        lan_ip = %lan_ip,
        port = actual_port,
        tls = tls_enabled,
        "LAN remote service starting"
    );

    let host: Arc<dyn RemoteHost> = Arc::new(RdgHost {
        workspace,
        totp,
        sessions: SessionStore::new(),
        ws_id: Uuid::new_v4(),
        port: actual_port,
        lan_ip,
        machine_name,
        serve_cfg,
        tls_enabled,
        // ridge LAN host 运行期间恒开（无桌面式全局开关）。
        remote_enabled: Arc::new(AtomicBool::new(true)),
    });

    let actual_port = ridge_remote::server_app::run(
        host,
        Arc::new(ridge_remote::lan_trust::LanTrustState::new()),
        std_listener,
        tls_config,
        shutdown_rx,
        true,
    )
    .await?;

    tracing::info!(target: "ridge_cli::lan_host", port = actual_port, "LAN remote service stopped");
    Ok(())
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
