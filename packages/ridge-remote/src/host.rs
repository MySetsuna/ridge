//! `RemoteHost` trait 组 —— 让共享 `server_app` 路由用**一份代码**驱动
//! 桌面 Tauri app、`rdg` headless CLI、云端 relay 三形态的宿主抽象缝。
//!
//! **零 Tauri 依赖**：每个宿主特有类型（桌面 `crate::state::AppState`、rdg
//! `SharedWorkspace`、云端租户上下文…）都藏在这些 trait 方法之后，`server_app`
//! 只见 `Arc<dyn RemoteHost>`。
//!
//! ## 分工（设计 §1-P1 的 5 trait，路线 B）
//!
//! - [`HostMeta`] —— 静态身份 / serve 门控（`/info`、证书 SAN、`remote_enabled`）。
//! - [`HostAuth`] —— TOTP 校验 + 会话令牌 + 暴力破解节流 + 黑名单（抽象掉桌面
//!   `RemoteAuth`+`SessionStore` 与 rdg `RemoteTotp` 的差异）。
//! - [`WorkspaceProvider`] —— `/workspace/*` 控制面 + `/file` 允许根。
//! - **`PaneProvider` / `InvokeDispatcher` / `EventBus`** —— 这三者是**每连接
//!   WS 会话**内部的职责（PTY 帧扇出、`invoke`/`data-request` 分发、事件中继），
//!   在路线 B 下封装在各宿主的 [`RemoteHost::serve_websocket`] 实现里（桌面 =
//!   搬来的 `handle_ws`；rdg = 面向 `SharedWorkspace` 的 headless 版），因此
//!   `server_app` 只经 `serve_websocket` 这一个钩子驱动它们，不在本层强拆成
//!   独立 trait（拆分留待 WS leg 逐步收口时再做）。

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use axum::extract::ws::WebSocket;
use serde_json::Value;

/// WS 升级握手时抓取、鉴权通过后交给宿主 [`RemoteHost::serve_websocket`] 的
/// 单连接身份。
#[derive(Clone, Debug)]
pub struct WsConn {
    /// 对端 IP（会话列表 + 黑名单键 + 令牌绑定校验用）。
    pub remote_addr: String,
    /// 客户端稳定设备 id（移动端 localStorage UUID），可为空串。
    pub device_id: String,
    /// 会话令牌（`?token=` 连接携带；`?code=` TOTP 连接为 `None`）。
    pub token: Option<String>,
}

/// 控制面操作的粗粒度错误 → HTTP 状态映射（`server_app` 负责翻译成响应）。
pub enum HostError {
    /// 400 —— 参数非法 / 语义不允许（如"不能关闭最后一个工作区"）。
    BadRequest(String),
    /// 404 —— 目标不存在。
    NotFound(String),
}

/// 静态服务身份与 serve 配置（`/info`、证书 SAN、serve 门控）。
pub trait HostMeta {
    /// 实际监听端口。
    fn port(&self) -> u16;
    /// 对外 LAN IPv4（`/info` 展示；证书 SAN 由启动侧另行覆盖全网卡）。
    fn lan_ip(&self) -> String;
    /// 机器名（`/info` 展示 + 证书 CN）。
    fn machine_name(&self) -> String;
    /// `remote_enabled` 总开关（共享 `Arc<AtomicBool>`，供 serve fallback
    /// 自门控与 `remote_gate` 复用同一个原子量）。
    fn remote_enabled(&self) -> Arc<AtomicBool>;
    /// 是否 TLS serve —— 门控 HSTS 响应头（纯 HTTP 下 HSTS 无意义/有害）。
    fn tls_enabled(&self) -> bool;
    /// UA 分流的静态 serve 配置（移动 SPA + 可选桌面 SPA）。
    fn serve_cfg(&self) -> crate::serve::UaServeConfig;
}

/// 鉴权与反滥用面（TOTP 校验、会话令牌、暴力破解节流、黑名单）。
///
/// 抽象掉桌面（[`crate::auth::RemoteAuth`] + [`crate::auth::SessionStore`] +
/// [`crate::auth::VerifyThrottle`] + 桌面黑名单）与 rdg（`RemoteTotp`，暂无
/// 节流/黑名单）的差异：无节流/黑名单的宿主用下面的宽松默认实现即可。
pub trait HostAuth {
    /// 校验一个 6 位 TOTP 码。
    fn verify_code(&self, code: &str) -> bool;

    /// 该 设备/IP 是否被黑名单封禁（默认：不封）。
    fn is_blacklisted(&self, device_id: &str, ip: &str) -> bool {
        let _ = (device_id, ip);
        false
    }

    /// TOTP 校验前的节流/黑名单闸门：`Ok(())` 放行、`Err(())` 拒绝
    /// （默认：始终放行）。返回统一失败信息由调用方决定，避免预言机。
    #[allow(clippy::result_unit_err)]
    fn pre_verify_gate(&self, ip: &str, device_id: &str) -> Result<(), ()> {
        let _ = (ip, device_id);
        Ok(())
    }

    /// 记录一次 TOTP 校验结果到节流器（默认：no-op）。失败若首次触顶硬上限，
    /// 实现方可在此自动加入持久黑名单。
    fn post_verify_record(&self, ip: &str, device_id: &str, valid: bool) {
        let _ = (ip, device_id, valid);
    }

    /// 签发一个绑定到 设备+IP 的会话令牌（不可猜测的 CSPRNG）。
    fn create_session_token(&self, device_id: &str, ip: &str) -> String;

    /// 令牌是否仍有效（仅存在性；`/session` 探活用）。
    fn validate_token(&self, token: &str) -> bool;

    /// 令牌是否有效且满足 设备+IP 绑定（设备两端都提供时才比对，否则退化到
    /// IP 绑定）——`/file`、`/workspace/*`、health 复检用。
    fn validate_token_bound(&self, token: &str, device_id: &str, ip: &str) -> bool;

    /// 令牌是否有效且严格满足 设备绑定：设备绑定令牌必须出示同一设备 id，
    /// 空设备不能把设备绑定令牌降级为 IP 绑定（控制面用，审计 L-3）。
    fn validate_token_device_strict(&self, token: &str, device_id: &str, ip: &str) -> bool;
}

/// 工作区控制面操作（`/workspace/*` HTTP 路由）+ `/file` 允许根。
///
/// 返回体沿用桌面 WS/HTTP 已定契约（`{id,name,displaySeq,active}` 列表、
/// `{success,workspaceId}` 结果），由宿主实现决定语义（桌面多工作区；rdg
/// 单工作区下 switch/create/close 的语义为设计开放决策点）。
pub trait WorkspaceProvider {
    /// `{ "workspaces": [{id,name,displaySeq,active}, …] }`。
    fn list_workspaces_json(&self) -> Value;
    /// 切换到指定工作区，成功返回 `{success:true,workspaceId}`。
    fn switch_workspace(&self, workspace_id: &str) -> Result<Value, HostError>;
    /// 新建工作区（可选名字），成功返回 `{success:true,workspaceId}`。
    fn create_workspace(&self, name: Option<String>) -> Result<Value, HostError>;
    /// 关闭指定工作区，成功返回 `{success:true}`。
    fn close_workspace(&self, workspace_id: &str) -> Result<Value, HostError>;
    /// `/file` 端点允许读取的文件系统根（各工作区 pane cwd + 当前项目）。
    /// 空 = 不允许任何 `/file` 读取。
    fn allowed_file_roots(&self) -> Vec<PathBuf>;
}

/// 伞状宿主 trait —— 共享 [`crate::server_app`] 路由驱动的对象即 `Arc<dyn RemoteHost>`。
///
/// [`serve_websocket`](RemoteHost::serve_websocket) 封装整段每连接 WS 会话
/// （PaneProvider/InvokeDispatcher/EventBus 职责），返回 boxed future 以保持
/// `dyn` 对象安全（`async fn in trait` 不满足对象安全）。
pub trait RemoteHost: HostMeta + HostAuth + WorkspaceProvider + Send + Sync + 'static {
    /// 鉴权通过后接管这条 WS 连接直到关闭。桌面实现 = 搬来的 `handle_ws`；
    /// rdg 实现 = 面向 `SharedWorkspace` 的 headless 版（讲同一 `ridge-remote-ws`
    /// 协议）。
    fn serve_websocket(
        self: Arc<Self>,
        socket: WebSocket,
        conn: WsConn,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}
