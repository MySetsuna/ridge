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

#[derive(serde::Deserialize, serde::Serialize)]
struct PersistedBinding {
    base_url: String,
    token: String,
}

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

/// Stable desktop teammate binding. Existing Agent processes keep the
/// `RIDGE_TEAMMATE_*` values injected at spawn time; persisting the loopback
/// endpoint lets a restarted desktop rebind the same control plane and token.
pub fn binding_path(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.join("teammate-binding.json")
}

fn loopback_port(base_url: &str) -> Option<u16> {
    let port = base_url.strip_prefix("http://127.0.0.1:")?.parse().ok()?;
    (port != 0).then_some(port)
}

/// Read only loopback bindings written by Ridge itself. Invalid or stale files
/// are ignored so a damaged app-data file never blocks desktop startup.
pub fn read_binding(app_data_dir: &std::path::Path) -> Option<(String, String)> {
    let path = binding_path(app_data_dir);
    let binding: PersistedBinding = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    loopback_port(&binding.base_url)?;
    if binding.token.trim().is_empty() {
        return None;
    }
    Some((binding.base_url, binding.token))
}

/// Persist the binding with a fully-written temporary file before replacing
/// the old value. The token never enters logs; app-data ACLs protect it on
/// Windows and Unix mode 0600 is applied where available.
pub fn write_binding(
    app_data_dir: &std::path::Path,
    base_url: &str,
    token: &str,
) -> std::io::Result<()> {
    if loopback_port(base_url).is_none() || token.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid loopback teammate binding",
        ));
    }
    std::fs::create_dir_all(app_data_dir)?;
    let path = binding_path(app_data_dir);
    let tmp = app_data_dir.join(format!(".teammate-binding-{}.tmp", uuid::Uuid::new_v4()));
    let body = serde_json::to_vec(&PersistedBinding {
        base_url: base_url.to_string(),
        token: token.to_string(),
    })
    .map_err(std::io::Error::other)?;
    let write_result = (|| {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(&tmp, &path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write_result
}

fn write_at_path(path: &std::path::Path, url: &str, token: &str) {
    use std::io::Write;

    let body = serde_json::json!({ "url": url, "token": token }).to_string();
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    if let Err(e) = options
        .open(&path)
        .and_then(|mut file| file.write_all(body.as_bytes()))
    {
        tracing::warn!(target: "ridge::teammate", "sidecar write failed {}: {e}", path.display());
    }
}

fn write_one(socket_path: &str, url: &str, token: &str) {
    write_at_path(&sidecar_path(socket_path), url, token);
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

    // A desktop restart starts with an empty in-process socket registry. Scan
    // only Ridge endpoint sidecars whose current token matches ours so an
    // unrelated dev/release instance is never redirected. This covers the
    // rare preferred-port collision where a surviving Agent needs the new URL.
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("ridge-teammate-endpoint-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(value) =
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap_or_default())
        else {
            continue;
        };
        if value.get("token").and_then(|v| v.as_str()) == Some(token) {
            write_at_path(&path, url, token);
        }
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
        assert_eq!(
            sanitize_socket("/ridge/teammate.sock"),
            "_ridge_teammate_sock"
        );
        assert_eq!(sanitize_socket("abc123"), "abc123");
    }

    #[test]
    fn persists_only_valid_loopback_binding() {
        let dir =
            std::env::temp_dir().join(format!("ridge-endpoint-test-{}", uuid::Uuid::new_v4()));
        write_binding(&dir, "http://127.0.0.1:43123", "test-token").unwrap();
        assert_eq!(
            read_binding(&dir),
            Some((
                "http://127.0.0.1:43123".to_string(),
                "test-token".to_string()
            ))
        );
        std::fs::write(
            binding_path(&dir),
            r#"{"base_url":"http://0.0.0.0:43123","token":"bad"}"#,
        )
        .unwrap();
        assert!(read_binding(&dir).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refreshes_sidecar_after_process_restart_when_token_matches() {
        let socket = format!("ridge-endpoint-restart-{}.sock", uuid::Uuid::new_v4());
        let path = sidecar_path(&socket);
        write_at_path(&path, "http://127.0.0.1:41000", "restart-token");
        refresh_all("http://127.0.0.1:41001", "restart-token");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["url"], "http://127.0.0.1:41001");
        let _ = std::fs::remove_file(path);
    }
}
