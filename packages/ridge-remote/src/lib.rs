//! Shared LAN remote-control server logic.
//!
//! Provides TLS cert management (CA + leaf) and a common "bind → TLS → serve"
//! lifecycle used by both the desktop Tauri app and the `rdg` CLI.

/// LAN 远控鉴权原语（TOTP 持有者 `RemoteAuth`、会话令牌 `SessionStore`、
/// TOTP 暴力破解节流 `VerifyThrottle`）。已从 src-tauri 迁入共享层，零 Tauri 依赖。
pub mod auth;
/// `RemoteHost` trait 组（宿主抽象缝）：`HostMeta`/`HostAuth`/`WorkspaceProvider`
/// + `serve_websocket` 钩子。让共享 `server_app` 路由用一份代码驱动桌面/rdg/云端，
/// 零 Tauri 依赖。
pub mod host;
/// 泛型 LAN 远控应用路由（路由/门控/verify/ws 握手/workspace/file/session），
/// 泛型于 `Arc<dyn RemoteHost>`。从桌面 `server.rs` 下沉，供三形态共用。
pub mod server_app;
/// RFC 6762 multicast DNS responder for `_ridge._tcp.local.`（纯 UDP 广播，无 Tauri
/// 依赖）。从 src-tauri 迁入共享层，供桌面 LAN host 与 rdg 共用发现广播。
pub mod mdns;
/// LAN IPv4 地址探测（`detect_lan_ip` / `detect_lan_ips`）。std-only，供三形态共用。
pub mod net;
/// 远控 pane 字节流的帧格式 + 重同步策略 SSOT（`pane_frame`/`pane_resync_frame` +
/// `RESYNC_MIN_INTERVAL`/`RAW_CHAN_CAP`/scrollback 尺寸）。桌面 LAN/cloud 与 rdg
/// 三腿共用一份，消手抄漂移。
pub mod pane;
pub mod server;
/// 前端静态资源 serve（UA 分流 + SPA fallback + CA 下载 + 安全头/压缩层）。
/// 从桌面 `server.rs` 下沉，供 LAN 远控三形态共用，零 Tauri 依赖。
pub mod serve;
pub mod tls;
/// UA→UI 分叉判定（桌面 SPA vs 移动 SPA）的 SSOT，供局域网远控服务端与公网远控
/// 中继共用，避免分叉规则漂移。
pub mod ua;
