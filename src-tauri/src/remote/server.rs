//! 桌面 LAN 远控 HTTP/WS 服务器的**启动壳**。
//!
//! 路由装配 / verify / ws 握手 / workspace / file / session 与整段每连接 WS
//! 会话（`handle_ws` + dispatcher）已分别下沉到共享层 `ridge_remote::server_app`
//! 与桌面侧 `crate::remote_host_impl`（`DesktopHost` 实现 `RemoteHost`）。此处只保
//! 留：后台线程 + tokio runtime、`bind_tcp(9527)`、多网卡 TLS fail-closed 决策，
//! 然后构造 `Arc<dyn RemoteHost>` 交给 `server_app::run`。

use std::sync::Arc;

use crate::state::AppState;

use super::auth::RemoteAuth;

/// Handle returned by `spawn_remote_server` — the caller receives the
/// allocated port and the background thread join handle.
pub struct ServerHandle {
    pub port: u16,
    pub thread: std::thread::JoinHandle<()>,
}

/// Spawn the remote-control WebSocket server on a background thread.
/// Binds `0.0.0.0:9527` (probing up to 10 higher ports).
///
/// Accepts a `shutdown_rx` one-shot receiver: when a value is sent on the
/// corresponding sender the server performs an orderly graceful shutdown
/// (drain in-flight requests, close listeners).
///
/// Returns `None` if binding failed.
pub fn spawn_remote_server(
    state: AppState,
    auth: Arc<RemoteAuth>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Option<ServerHandle> {
    let lan_ip = super::detect_lan_ip();

    let (port_tx, port_rx) = std::sync::mpsc::channel();

    let thread = std::thread::Builder::new()
        .name("ridge-remote-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(target: "ridge::remote", error = %e, "tokio runtime build failed");
                    let _ = port_tx.send(None);
                    return;
                }
            };
            rt.block_on(run_remote_server(state, auth, lan_ip, port_tx, shutdown_rx));
        })
        .expect("ridge-remote-http thread spawn");

    let port = port_rx.recv().ok().flatten()?;
    Some(ServerHandle { port, thread })
}

async fn run_remote_server(
    state: AppState,
    auth: Arc<RemoteAuth>,
    lan_ip: String,
    port_tx: std::sync::mpsc::Sender<Option<u16>>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // Use the shared port binding from ridge-remote (probe up to 10 higher ports).
    let (std_listener, port) = match ridge_remote::server::bind_tcp(9527) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote server bind failed");
            let _ = port_tx.send(None);
            return;
        }
    };
    tracing::info!(target: "ridge::remote", port, lan_ip = %lan_ip, "Remote control server listening");

    // Resolve the static UI dirs (mobile `static/remote` + optional desktop
    // `web-remote-dist`). 运行时候选目录探测已下沉到 `ridge_remote::serve`。
    let serve_cfg = ridge_remote::serve::UaServeConfig::resolve_ui_dirs();

    let machine_name = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());

    // SECURITY (audit H1): resolve TLS BEFORE announcing the port so a TLS
    // failure can be reported back to the caller as a start failure (fail-closed)
    // rather than the server silently coming up on plain HTTP. The remote server
    // exposes full shell + filesystem control; serving it over cleartext on a
    // hostile LAN would leak the 6-digit code and session token to any sniffer,
    // who could then replay them. We therefore REFUSE to start when TLS material
    // can't be produced — unless the operator explicitly opts into insecure HTTP
    // via `RIDGE_REMOTE_ALLOW_INSECURE_HTTP=1` (loud, never silent/automatic).
    // Cover EVERY reachable LAN address in the cert (not just the auto-chosen
    // primary), so the remote panel's IP picker can advertise any of them — Wi-Fi,
    // Tailscale, Ethernet — and the phone still gets a matching cert.
    let lan_ips = super::detect_lan_ips();
    let tls_config = super::tls::resolve_config_multi(&lan_ips, &machine_name).await;
    let allow_insecure = std::env::var("RIDGE_REMOTE_ALLOW_INSECURE_HTTP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if tls_config.is_none() && !allow_insecure {
        tracing::error!(
            target: "ridge::remote",
            "Remote TLS unavailable — REFUSING to start the remote server on plain HTTP \
             (would expose shell/file control + auth code over cleartext on the LAN). \
             Set RIDGE_REMOTE_ALLOW_INSECURE_HTTP=1 to explicitly allow insecure HTTP."
        );
        let _ = port_tx.send(None);
        return;
    }
    let tls_enabled = tls_config.is_some();

    // 构造桌面宿主实现（包装 AppState），交给共享 `server_app` 装配路由 + serve。
    let host: Arc<dyn ridge_remote::host::RemoteHost> =
        Arc::new(crate::remote_host_impl::DesktopHost {
            state,
            auth,
            port,
            lan_ip,
            machine_name,
            serve_cfg,
            tls_enabled,
        });

    let _ = port_tx.send(Some(port));
    // §sessions: serve_on captures each client's real peer IP via ConnectInfo (for
    // the session list + blacklist). TLS/bind fail-closed already decided above.
    if let Err(e) =
        ridge_remote::server_app::run(host, std_listener, tls_config, shutdown_rx, true).await
    {
        tracing::error!(target: "ridge::remote", error = %e, "remote server stopped");
    }
}
