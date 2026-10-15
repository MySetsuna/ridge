//! ridge LAN 远控服务端**启动壳**。
//!
//! P1 阶段 4c：路由 / verify / ws 握手 / workspace / file / session 与整段每连接 WS
//! 会话已分别下沉到共享层 `ridge_remote::server_app` 与 ridge 侧 `super::lan_host_impl`
//! （`RdgHost` 实现 `RemoteHost`）。此处只保留：LAN IP / 机器名 / serve 目录探测、
//! `bind_tcp` + TLS 解析，然后构造 `Arc<dyn RemoteHost>` 交给 `server_app::run`。
//!
//! 私有 `ridge-lan-ws` 协议与内联 LOGIN/TERMINAL HTML 已删除 —— ridge 现与桌面共用
//! 同一 `ridge-remote-ws` 协议 + 移动/桌面 SPA 静态资源（P5 协议分叉消除）。

use std::sync::Arc;

use anyhow::Result;


use ridge_remote::auth::RemoteAuth;


pub async fn run(
    port:u16,
    runtime:Arc<RemoteAuth>,
    shutdown_rx:tokio::sync::oneshot::Receiver<()>,
)->Result<()> {
    runtime.call("set_remote_enabled",serde_json::json!({"enabled":true,"port":port})).map_err(anyhow::Error::msg)?;
    let _=shutdown_rx.await;
    runtime.call("set_remote_enabled",serde_json::json!({"enabled":false})).map_err(anyhow::Error::msg)?;
    Ok(())
}
