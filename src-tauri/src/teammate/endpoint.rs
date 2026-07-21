//! teammate HTTP 端点的 sidecar 重发现。
//!
//! 修「后端 teammate server panic 自重启时 `bind 127.0.0.1:0` 换了**新 ephemeral 端口**，
//! 而现存 shell 的 tmux 垫片环境变量 `RIDGE_TEAMMATE_URL` 还是旧端口 → 全部连不上、连接
//! 重试也救不了（端口错了）」这一罕见但全断的洞。
//!
//! 机制：把当前 `{url,token}` 写到
//! `temp_dir()/ridge-teammate-endpoint-<sanitize(socket_path)>.json`。`socket_path` =
//! `$TMUX` 第一段（`<pane cwd>/teammate.sock`）：后端注入 `$TMUX` 时已知、垫片从自己的
//! `$TMUX` 也能算出**同一文件名** → 无需任何额外发现协议。**不写**在 socket 路径旁，避免
//! 在用户 repo 目录落文件污染工作区/被误提交。按 socket 路径分键 → dev 与 release 双实例
//! （cwd 不同）天然不撞。
//!
//! 写入时机：① PTY spawn（`ensure_pane_pty_workspace`）按该 socket 写当前端点 +
//! 记下 socket 路径；② server (re)bind（`run_server`）用新端点**刷新所有**已记 socket，
//! 这样重启换端口后 sidecar 立即指向新端口。
//!
//! 垫片侧的对应读取逻辑在 `bin/tmux.rs`（独立二进制，重复同一份 `sanitize_socket` 纯逻辑）。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 进程级：所有已写过 sidecar 的 socket 路径，供 server (re)bind 时整体刷新。
fn known_sockets() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 非 `[A-Za-z0-9]` 一律换 `_`（确定性）。**必须**与 `bin/tmux.rs::sanitize_socket` 同实现，
/// 两端才能算出同一 sidecar 文件名。
pub fn sanitize_socket(socket_path: &str) -> String {
    socket_path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// sidecar 完整路径：`temp_dir()/ridge-teammate-endpoint-<sanitize>.json`。
pub fn sidecar_path(socket_path: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ridge-teammate-endpoint-{}.json",
        sanitize_socket(socket_path)
    ))
}

fn write_one(socket_path: &str, url: &str, token: &str) {
    let body = serde_json::json!({ "url": url, "token": token }).to_string();
    let path = sidecar_path(socket_path);
    if let Err(e) = std::fs::write(&path, body) {
        tracing::warn!(target: "ridge::teammate", "sidecar write failed {}: {e}", path.display());
    }
}

/// PTY spawn 时调用：按该 socket 路径写当前端点 + 记下供日后刷新。
pub fn write_sidecar(socket_path: &str, url: &str, token: &str) {
    let socket_path = socket_path.trim();
    if socket_path.is_empty() {
        return;
    }
    write_one(socket_path, url, token);
    if let Ok(mut set) = known_sockets().lock() {
        set.insert(socket_path.to_string());
    }
}

/// server (re)bind 后调用：用新端点刷新所有已记 sidecar（换端口后立即指向新端口）。
pub fn refresh_all(url: &str, token: &str) {
    let sockets: Vec<String> = match known_sockets().lock() {
        Ok(set) => set.iter().cloned().collect(),
        Err(_) => return,
    };
    for s in sockets {
        write_one(&s, url, token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_matches_shim_rule() {
        // 与 bin/tmux.rs::sanitize_socket 的单测口径**逐字一致**——两端文件名必须对齐。
        assert_eq!(
            sanitize_socket("C:/code/wind/teammate.sock"),
            "C__code_wind_teammate_sock"
        );
        assert_eq!(sanitize_socket("/ridge/teammate.sock"), "_ridge_teammate_sock");
        assert_eq!(sanitize_socket("abc123"), "abc123");
    }
}
