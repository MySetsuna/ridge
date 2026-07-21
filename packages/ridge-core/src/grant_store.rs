//! TOTP 信任控制端授权持久化：记录「哪个 controller 曾手动通过 TOTP 验证」及时间戳，
//! 供 host 在 24h 内跳过二次 TOTP（"Remember this controller"）。
//!
//! 设计与 `seed_store` 完全镜像：
//! - 路径：`directories::ProjectDirs("ridge").config_dir()/grants/`（与 `totp/` 同根）。
//! - 身份 → 文件名：`hex(sha256(identity)[..8])` + `.grants`。
//! - 文件格式：JSON `{ "ctrl_pub_hash_hex": unix_i64, ... }`（复用 `serde_json`，
//!   crate 已依赖）。
//! - 静态加密：Windows DPAPI（user scope）；其它平台明文 + `0600`——与 seed_store 一致。
//! - 失败降级：读取任何异常 → 当「无授权」；写失败仅 warn——不阻断远控。
//! - 原子写：tmp → rename（与 seed_store 一致）。
//!
//! 公开 API 见模块末尾 `pub fn check / record / revoke_all`。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// 信任有效期（秒）：24 小时。
pub const TRUST_TTL_SECS: i64 = 24 * 3600;

// ── 平台加密（直接复用 seed_store 内部 DPAPI 手写实现，但 seed_store 的 dpapi mod
// 是 crate-private；此处用 cfg_attr 按平台内联等价实现，保持与 seed_store 的对称性）。
// ── 注：不修改 seed_store 暴露 dpapi，而是在此模块独立保留同款手写 FFI，与 seed_store
// 头注释「避免引入庞大 windows crate」的一贯原则完全一致。

#[cfg(windows)]
fn encrypt(data: &[u8]) -> Option<Vec<u8>> {
    dpapi::protect(data)
}
#[cfg(windows)]
fn decrypt(blob: &[u8]) -> Option<Vec<u8>> {
    dpapi::unprotect(blob)
}
#[cfg(not(windows))]
fn encrypt(data: &[u8]) -> Option<Vec<u8>> {
    Some(data.to_vec())
}
#[cfg(not(windows))]
fn decrypt(blob: &[u8]) -> Option<Vec<u8>> {
    Some(blob.to_vec())
}

// ── 路径与文件名 ─────────────────────────────────────────────────────────────

/// grants 目录（不保证已创建；`write_grants` 会 `create_dir_all`）。
fn grants_dir() -> Option<std::path::PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "ridge")?;
    Some(dirs.config_dir().join("grants"))
}

/// 身份 → 文件名：`hex(sha256(identity)[..8])` + `.grants`。
fn grants_filename(identity: &str) -> String {
    let digest = Sha256::digest(identity.as_bytes());
    let mut name = String::with_capacity(21);
    for b in &digest[..8] {
        let _ = write!(name, "{:02x}", b);
    }
    name.push_str(".grants");
    name
}

/// controller 公钥 → 存储 key：`hex(sha256(ctrl_pub_bytes))`（32 字节全量）。
fn ctrl_pub_hash_hex(ctrl_pub: &[u8]) -> String {
    let digest = Sha256::digest(ctrl_pub);
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

// ── 读写（可注入目录，供单测使用临时目录）──────────────────────────────────────

/// 从磁盘加载该身份的 grants map；任何失败返回空 map（降级）。
fn load_grants_in(dir: &Path, identity: &str) -> HashMap<String, i64> {
    let path = dir.join(grants_filename(identity));
    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return HashMap::new(),
    };
    let plain = match decrypt(&raw) {
        Some(p) => p,
        None => {
            tracing::warn!(target: "ridge::grant_store", "DPAPI 解密失败，视为无授权");
            return HashMap::new();
        }
    };
    match serde_json::from_slice::<HashMap<String, i64>>(&plain) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(target: "ridge::grant_store", error = %e, "grants 文件解析失败，视为无授权");
            HashMap::new()
        }
    }
}

/// 将 grants map 原子写入磁盘；失败仅 warn。
fn write_grants_in(dir: &Path, identity: &str, map: &HashMap<String, i64>) {
    if let Err(e) = write_grants_in_inner(dir, identity, map) {
        tracing::warn!(target: "ridge::grant_store", error = %e, "写 grants 文件失败，授权不会持久化");
    }
}

fn write_grants_in_inner(dir: &Path, identity: &str, map: &HashMap<String, i64>) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec(map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let blob = encrypt(&json)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "DPAPI 加密失败"))?;
    let path = dir.join(grants_filename(identity));
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &blob)?;
    set_owner_only_perms(&tmp);
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}
#[cfg(not(unix))]
fn set_owner_only_perms(_path: &Path) {
    // Windows：DPAPI 已绑当前账户 + NTFS 默认 ACL；不额外处理。
}

// ── 公开 API ──────────────────────────────────────────────────────────────────

/// 检查 `(identity, ctrl_pub)` 是否持有有效的信任授权。
///
/// 返回 `true` 当且仅当：该 controller 曾被记录，且 `now - last_verified_at < TRUST_TTL_SECS`。
/// 任何 I/O / 解密 / 解析失败均返回 `false`（降级）。
pub fn check(identity: &str, ctrl_pub: &[u8]) -> bool {
    let Some(dir) = grants_dir() else { return false };
    check_in(&dir, identity, ctrl_pub)
}

/// 记录/更新 `(identity, ctrl_pub)` 的信任时间戳为「当前时刻」。
///
/// 幂等：多次调用只会刷新时间戳。写失败仅 warn，不阻断调用方。
pub fn record(identity: &str, ctrl_pub: &[u8]) {
    let Some(dir) = grants_dir() else {
        tracing::warn!(target: "ridge::grant_store", "无法解析 grants 目录，授权不会持久化");
        return;
    };
    record_in(&dir, identity, ctrl_pub);
}

/// 撤销某身份的全部授权（删除对应 grants 文件）。
///
/// 用户切换账号或主动「忘记所有受信控制端」时调用。
pub fn revoke_all(identity: &str) {
    let Some(dir) = grants_dir() else { return };
    revoke_all_in(&dir, identity);
}

// ── 可注入目录的实现（供单测隔离）──────────────────────────────────────────────

fn check_in(dir: &Path, identity: &str, ctrl_pub: &[u8]) -> bool {
    let map = load_grants_in(dir, identity);
    let key = ctrl_pub_hash_hex(ctrl_pub);
    let Some(&ts) = map.get(&key) else { return false };
    let now = now_unix_i64();
    now - ts < TRUST_TTL_SECS
}

fn record_in(dir: &Path, identity: &str, ctrl_pub: &[u8]) {
    let mut map = load_grants_in(dir, identity);
    let key = ctrl_pub_hash_hex(ctrl_pub);
    map.insert(key, now_unix_i64());
    write_grants_in(dir, identity, &map);
}

fn revoke_all_in(dir: &Path, identity: &str) {
    let path = dir.join(grants_filename(identity));
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(target: "ridge::grant_store", error = %e, "删除 grants 文件失败");
        }
    }
}

fn now_unix_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── Windows DPAPI FFI（与 seed_store 完全一致，独立内联避免跨模块可见性依赖）────────
#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;
    use std::ptr;

    /// 禁止任何 DPAPI UI（headless 安全）。
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            pp_sz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        let in_blob = DataBlob {
            cb_data: data.len() as u32,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut out = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 || out.pb_data.is_null() {
            return None;
        }
        let result = unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
        unsafe { LocalFree(out.pb_data as *mut c_void) };
        Some(result)
    }

    pub fn unprotect(blob: &[u8]) -> Option<Vec<u8>> {
        let in_blob = DataBlob {
            cb_data: blob.len() as u32,
            pb_data: blob.as_ptr() as *mut u8,
        };
        let mut out = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 || out.pb_data.is_null() {
            return None;
        }
        let result = unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
        unsafe { LocalFree(out.pb_data as *mut c_void) };
        Some(result)
    }
}

// ── 单元测试 ──────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试独立临时目录（不碰真实 AppData；按测试名+进程 ID 隔离，可并行）。
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("ridge-grants-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pub_key(byte: u8) -> Vec<u8> {
        vec![byte; 32]
    }

    #[test]
    fn record_then_check_is_true() {
        let dir = temp_dir("record-check");
        record_in(&dir, "alice", &pub_key(1));
        assert!(check_in(&dir, "alice", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_pubkey_is_false() {
        let dir = temp_dir("diff-key");
        record_in(&dir, "alice", &pub_key(1));
        assert!(!check_in(&dir, "alice", &pub_key(2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_grant_is_false() {
        let dir = temp_dir("expired");
        // 直接写入一个 25 小时前的时间戳（超过 TRUST_TTL_SECS）
        let old_ts = now_unix_i64() - 25 * 3600;
        let key = ctrl_pub_hash_hex(&pub_key(1));
        let mut map = HashMap::new();
        map.insert(key, old_ts);
        write_grants_in(&dir, "alice", &map);
        assert!(!check_in(&dir, "alice", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revoke_all_clears_grants() {
        let dir = temp_dir("revoke");
        record_in(&dir, "alice", &pub_key(1));
        assert!(check_in(&dir, "alice", &pub_key(1)));
        revoke_all_in(&dir, "alice");
        assert!(!check_in(&dir, "alice", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_returns_false_no_panic() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(grants_filename("bob")), b"\x00\x01\x02garbage").unwrap();
        // 不应 panic，降级返回 false
        assert!(!check_in(&dir, "bob", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_returns_false_no_panic() {
        let dir = temp_dir("missing");
        assert!(!check_in(&dir, "nobody", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn distinct_identities_are_isolated() {
        let dir = temp_dir("isolated");
        record_in(&dir, "alice", &pub_key(1));
        // bob 没有授权
        assert!(!check_in(&dir, "bob", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_is_idempotent_and_refreshes_timestamp() {
        let dir = temp_dir("idempotent");
        record_in(&dir, "alice", &pub_key(1));
        record_in(&dir, "alice", &pub_key(1)); // 第二次覆盖时间戳
        assert!(check_in(&dir, "alice", &pub_key(1)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
