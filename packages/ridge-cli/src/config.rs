//! 配置与凭据持久化。
//!
//! device JWT 等敏感凭据存到 `~/.config/ridge/auth.json`（Linux）。用
//! `directories` 解析 XDG 配置目录，跨平台一致。

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 默认 Base zone（契约 §1）。可被 `RIDGE_BASE_DOMAIN` 环境变量覆盖（便于自托管 / 测试）。
pub const DEFAULT_BASE_DOMAIN: &str = "remo2ridge.duckdns.org";

/// 取 Base zone。优先环境变量 `RIDGE_BASE_DOMAIN`，否则契约默认值。
pub fn base_domain() -> String {
    std::env::var("RIDGE_BASE_DOMAIN").unwrap_or_else(|_| DEFAULT_BASE_DOMAIN.to_string())
}

/// HTTP API 根（契约 §4）：`https://{base}/api/v1`。
pub fn api_base() -> String {
    format!("https://{}/api/v1", base_domain())
}

/// 设备激活引导页（契约：用户访问 `https://{base}/activate`）。
pub fn activate_url() -> String {
    format!("https://{}/activate", base_domain())
}

/// 持久化的设备凭据（契约 §3 device token + §4.4 绑定结果）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFile {
    /// device JWT（scope=device，180 天）。
    pub token: String,
    /// 绑定的设备名（device_name）。
    pub device_name: String,
    /// 账户 username。
    pub username: String,
}

impl AuthFile {
    /// 该设备的租户域名（契约 §1）：`{device}-{username}.{base}`。
    pub fn tenant_host(&self) -> String {
        format!("{}-{}.{}", self.device_name, self.username, base_domain())
    }

    /// 信令 WS 端点（契约 §5）：`wss://{tenant}/ws?token=&role=host`。
    pub fn signaling_ws_url(&self) -> String {
        format!(
            "wss://{}/ws?token={}&role=host",
            self.tenant_host(),
            self.token
        )
    }

    /// 公网入口（契约 §4.4 返回值）。
    pub fn public_entry(&self) -> String {
        format!("https://{}", self.tenant_host())
    }
}

/// 解析配置目录 `~/.config/ridge`（Linux）。创建（若不存在）。
fn config_dir() -> Result<PathBuf> {
    // qualifier/org 留空，application = "ridge" → Linux 下解析为 ~/.config/ridge。
    let dirs = ProjectDirs::from("", "", "ridge")
        .context("cannot resolve config directory (no $HOME?)")?;
    let dir = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create config dir {}", dir.display()))?;
    Ok(dir)
}

/// `auth.json` 完整路径。
pub fn auth_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("auth.json"))
}

/// 读取已持久化的凭据（不存在返回 `Ok(None)`）。
pub fn load_auth() -> Result<Option<AuthFile>> {
    let path = auth_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let auth: AuthFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(auth))
}

/// 写入凭据。Linux 上设 0600 权限（凭据保护）。
pub fn save_auth(auth: &AuthFile) -> Result<()> {
    let path = auth_path()?;
    let json = serde_json::to_string_pretty(auth).context("failed to serialize auth")?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    set_owner_only_perms(&path);
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_perms(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_owner_only_perms(_path: &std::path::Path) {
    // Windows: 依赖 NTFS 默认 ACL；不做额外处理。
}

#[cfg(test)]
mod tests {
    use super::*;

    // `RIDGE_BASE_DOMAIN` 是进程级全局状态；两段断言合并到**单个**测试里顺序执行，
    // 避免与并行测试争用同一环境变量（test 默认多线程并行）。
    #[test]
    fn domain_and_url_shaping() {
        // 1) 未设环境变量 → 契约默认域名。
        std::env::remove_var("RIDGE_BASE_DOMAIN");
        let auth = AuthFile {
            token: "JWT123".to_string(),
            device_name: "my-laptop".to_string(),
            username: "alice".to_string(),
        };
        assert_eq!(auth.tenant_host(), "my-laptop-alice.remo2ridge.duckdns.org");
        assert_eq!(
            auth.public_entry(),
            "https://my-laptop-alice.remo2ridge.duckdns.org"
        );
        assert_eq!(
            auth.signaling_ws_url(),
            "wss://my-laptop-alice.remo2ridge.duckdns.org/ws?token=JWT123&role=host"
        );

        // 2) 设环境变量 → 覆盖域名。
        std::env::set_var("RIDGE_BASE_DOMAIN", "example.test");
        assert_eq!(base_domain(), "example.test");
        assert_eq!(api_base(), "https://example.test/api/v1");
        assert_eq!(activate_url(), "https://example.test/activate");

        // 复位，避免污染其它测试。
        std::env::remove_var("RIDGE_BASE_DOMAIN");
    }
}
