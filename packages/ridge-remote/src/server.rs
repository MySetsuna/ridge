//! Common LAN remote-control server infrastructure.
//!
//! Provides the canonical "bind → TLS → serve" lifecycle shared by the desktop
//! Tauri app and the `rdg` CLI. Each consumer supplies its own `axum::Router`
//! with app-specific routes; the common code handles:
//!
//! - TCP listener binding (std → tokio, port probe)
//! - TLS cert resolution via [`crate::tls`] (CA-signed leaf, user-provided,
//!   or fallback)
//! - Graceful shutdown via `oneshot::Receiver`
//! - `ConnectInfo` layer for peer-address capture

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use tokio::sync::oneshot;

use crate::tls;

/// Bind a `std::net::TcpListener` on `0.0.0.0:{port}`. Tries the exact port
/// first, then probes up to 10 higher ports if the first is busy.
pub fn bind_tcp(port: u16) -> Result<(std::net::TcpListener, u16)> {
    let base = port;
    for offset in 0..10u16 {
        let addr: SocketAddr = format!("0.0.0.0:{}", base + offset)
            .parse()
            .context("invalid bind address")?;
        match std::net::TcpListener::bind(addr) {
            Ok(listener) => {
                let actual = listener.local_addr().map(|a| a.port()).unwrap_or(base + offset);
                listener.set_nonblocking(true)?;
                return Ok((listener, actual));
            }
            Err(_) if offset < 9 => continue,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "failed to bind port {} (tried {}+10): {e}",
                    base, base
                ));
            }
        }
    }
    unreachable!()
}

/// Resolve an `axum_server` TLS config from cached/stored material, or
/// generate a fresh CA + leaf for the given `lan_ip` / `hostname`.
///
/// Returns `None` when no TLS material can be produced (caller decides
/// whether to serve plain HTTP or refuse to start).
pub async fn resolve_tls(lan_ip: &str, hostname: &str) -> Option<RustlsConfig> {
    tls::resolve_config(lan_ip, hostname).await
}

/// Serve a pre-built `axum::Router` on a caller-provided TCP listener.
///
/// The caller is responsible for binding the listener and must set it to
/// non-blocking mode before calling this function. TLS certs are resolved
/// via [`crate::tls::resolve_config`].
///
/// - `std_listener` — a bound, non-blocking `std::net::TcpListener`.
/// - `router` — fully configured `axum::Router` (state applied by caller).
/// - `tls_config` — optional TLS config; `None` serves plain HTTP.
/// - `shutdown_rx` — trigger graceful shutdown.
/// - `require_tls` — if `true` and no `tls_config`, refuse to start.
///
/// Returns the actual port from the listener.
pub async fn serve_on(
    std_listener: std::net::TcpListener,
    router: Router<()>,
    tls_config: Option<RustlsConfig>,
    shutdown_rx: oneshot::Receiver<()>,
    require_tls: bool,
) -> Result<u16> {
    let actual_port = std_listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let allow_insecure = std::env::var("RIDGE_REMOTE_ALLOW_INSECURE_HTTP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let make_svc = router.into_make_service_with_connect_info::<SocketAddr>();

    match tls_config {
        Some(tls_config) => {
            tracing::info!(
                target: "ridge::remote",
                port = actual_port,
                "Serving HTTPS (TLS)"
            );
            let handle = axum_server::Handle::new();
            let h = handle.clone();
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                h.graceful_shutdown(Some(Duration::from_secs(3)));
            });
            axum_server::from_tcp_rustls(std_listener, tls_config)
                .handle(handle)
                .serve(make_svc)
                .await
                .context("TLS server failed")?;
        }
        None => {
            if require_tls && !allow_insecure {
                return Err(anyhow::anyhow!(
                    "TLS unavailable and RIDGE_REMOTE_ALLOW_INSECURE_HTTP not set"
                ));
            }
            tracing::warn!(
                target: "ridge::remote",
                "Serving plain HTTP (insecure)"
            );
            let listener = tokio::net::TcpListener::from_std(std_listener)
                .context("failed to create tokio listener")?;
            let shutdown_signal = async { drop(shutdown_rx.await) };
            axum::serve(listener, make_svc)
                .with_graceful_shutdown(shutdown_signal)
                .await
                .context("HTTP server failed")?;
        }
    }

    Ok(actual_port)
}

/// Serve a pre-built `axum::Router` with TLS or plain HTTP.
///
/// This is the canonical entry point for both the desktop Tauri app and
/// the `rdg` CLI. The caller provides:
///
/// - `port` — desired listen port (probed upward on conflict).
/// - `router` — fully configured `axum::Router` (state must be applied by the
///   caller via `.with_state(...)` before passing it in).
/// - `lan_ip` / `hostname` — for TLS cert SANs.
/// - `shutdown_rx` — trigger graceful shutdown by dropping the sender.
/// - `require_tls` — if `true`, refuse to serve plain HTTP; start fails.
///
/// Returns the actual port the server is listening on.
pub async fn serve(
    port: u16,
    router: Router<()>,
    lan_ip: &str,
    hostname: &str,
    shutdown_rx: oneshot::Receiver<()>,
    require_tls: bool,
) -> Result<u16>
{
    let (std_listener, _actual_port) = bind_tcp(port)?;
    let tls_config = resolve_tls(lan_ip, hostname).await;
    serve_on(std_listener, router, tls_config, shutdown_rx, require_tls).await
}
