//! rdg LAN 远控服务端**启动壳**。
//!
//! P1 阶段 4c：路由 / verify / ws 握手 / workspace / file / session 与整段每连接 WS
//! 会话已分别下沉到共享层 `ridge_remote::server_app` 与 rdg 侧 `super::lan_host_impl`
//! （`RdgHost` 实现 `RemoteHost`）。此处只保留：LAN IP / 机器名 / serve 目录探测、
//! `bind_tcp` + TLS 解析，然后构造 `Arc<dyn RemoteHost>` 交给 `server_app::run`。
//!
//! 私有 `ridge-lan-ws` 协议与内联 LOGIN/TERMINAL HTML 已删除 —— rdg 现与桌面共用
//! 同一 `ridge-remote-ws` 协议 + 移动/桌面 SPA 静态资源（P5 协议分叉消除）。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;

use ridge_remote::auth::SessionStore;
use ridge_remote::host::RemoteHost;
use ridge_remote::serve::UaServeConfig;

use crate::config;
use crate::totp::RemoteTotp;

use super::lan_host_impl::RdgHost;
use super::workspace::SharedWorkspace;

pub async fn run(
    port: u16,
    totp: Arc<RemoteTotp>,
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
        // rdg LAN host 运行期间恒开（无桌面式全局开关）。
        remote_enabled: Arc::new(AtomicBool::new(true)),
    });

    let actual_port =
        ridge_remote::server_app::run(host, std_listener, tls_config, shutdown_rx, true).await?;

    tracing::info!(target: "ridge_cli::lan_host", port = actual_port, "LAN remote service stopped");
    Ok(())
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
