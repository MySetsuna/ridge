//! TOTP 种子持久化：按身份加密落盘，跨重启稳定。
//!
//! - 路径：`directories::ProjectDirs("ridge").config_dir()/totp/`，由 ridge-core
//!   统一解析，桌面 / CLI / dev remote-server 三处必然一致（Windows =
//!   `%APPDATA%\ridge\config\totp\`，与 ridge-cli `auth.json` 同根）。
//! - 身份 → 文件名：`hex(sha256(identity))[..8字节]` + `.seed`（不在文件名泄露 username）。
//! - 静态加密：Windows 用 DPAPI（`CryptProtectData`，user scope，绑当前 Windows
//!   账户）；其它平台明文 + `0600`（与 `ridge-cli/config.rs` 的 auth.json 同强度）。
//! - 失败一律降级：`load` 任何异常当「无种子」，`save` 失败仅 warn——宁可让用户
//!   重扫，绝不让远控因持久化故障起不来。

use std::fmt::Write as _;
use std::path::Path;

use sha2::{Digest, Sha256};

/// 与 `totp.rs` 一致的 20 字节（160-bit）seed 长度。
const SECRET_LEN: usize = 20;

/// 读取并解密某身份的种子。缺失 / 损坏 / 解密失败 / 长度不符 → `None`。
pub fn load(identity: &str) -> Option<Vec<u8>> {
    load_in(&seed_dir()?, identity)
}

/// 加密并原子写入某身份的种子。失败仅 `warn`（不阻断远控启动）。
pub fn save(identity: &str, secret: &[u8]) {
    let Some(dir) = seed_dir() else {
        tracing::warn!(target: "ridge::totp", "no seed dir resolvable; TOTP secret not persisted");
        return;
    };
    if let Err(e) = save_in(&dir, identity, secret) {
        tracing::warn!(target: "ridge::totp", error = %e, "failed to persist TOTP secret");
    }
}

/// 种子目录（不保证已创建；`save_in` 会 `create_dir_all`）。
fn seed_dir() -> Option<std::path::PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "ridge")?;
    Some(dirs.config_dir().join("totp"))
}

/// 身份 → 文件名：`hex(sha256(identity)[..8])` + `.seed`。
fn seed_filename(identity: &str) -> String {
    let digest = Sha256::digest(identity.as_bytes());
    let mut name = String::with_capacity(21);
    for b in &digest[..8] {
        let _ = write!(name, "{:02x}", b);
    }
    name.push_str(".seed");
    name
}

/// 可注入目录的读取（供测试用临时目录，不污染真实 AppData）。
fn load_in(dir: &Path, identity: &str) -> Option<Vec<u8>> {
    let raw = std::fs::read(dir.join(seed_filename(identity))).ok()?;
    let secret = decrypt(&raw)?;
    (secret.len() == SECRET_LEN).then_some(secret)
}

/// 可注入目录的原子写入。
fn save_in(dir: &Path, identity: &str, secret: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let blob = encrypt(secret)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "encrypt failed"))?;
    let path = dir.join(seed_filename(identity));
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &blob)?;
    set_owner_only_perms(&tmp);
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

// ── 平台加密分层 ────────────────────────────────────────────────────────────

#[cfg(windows)]
fn encrypt(secret: &[u8]) -> Option<Vec<u8>> {
    dpapi::protect(secret)
}
#[cfg(windows)]
fn decrypt(blob: &[u8]) -> Option<Vec<u8>> {
    dpapi::unprotect(blob)
}
#[cfg(not(windows))]
fn encrypt(secret: &[u8]) -> Option<Vec<u8>> {
    Some(secret.to_vec())
}
#[cfg(not(windows))]
fn decrypt(blob: &[u8]) -> Option<Vec<u8>> {
    Some(blob.to_vec())
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

/// Windows DPAPI FFI（手写，避免引入庞大的 `windows` crate；与本仓库手写
/// HMAC/base32 的一贯做法一致）。`CryptProtectData`/`CryptUnprotectData` 走
/// **user scope**（`dwFlags` 不含 `CRYPTPROTECT_LOCAL_MACHINE`），并置
/// `CRYPTPROTECT_UI_FORBIDDEN` 确保无头/服务器场景绝不弹 UI。
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

    pub fn protect(secret: &[u8]) -> Option<Vec<u8>> {
        let in_blob = DataBlob {
            cb_data: secret.len() as u32,
            pb_data: secret.as_ptr() as *mut u8,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试独立临时目录（不碰真实 AppData；按测试名隔离，可并行）。
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ridge-totp-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = temp_dir("roundtrip");
        let secret = [7u8; SECRET_LEN];
        save_in(&dir, "alice", &secret).unwrap();
        assert_eq!(load_in(&dir, "alice").as_deref(), Some(&secret[..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none() {
        let dir = temp_dir("missing");
        assert!(load_in(&dir, "nobody").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_none() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join(seed_filename("bob")), b"\x00\x01\x02").unwrap();
        assert!(load_in(&dir, "bob").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn distinct_identities_isolated() {
        let dir = temp_dir("isolated");
        assert_ne!(seed_filename("alice"), seed_filename("bob"));
        save_in(&dir, "alice", &[1u8; SECRET_LEN]).unwrap();
        save_in(&dir, "bob", &[2u8; SECRET_LEN]).unwrap();
        assert_eq!(load_in(&dir, "alice").unwrap()[0], 1);
        assert_eq!(load_in(&dir, "bob").unwrap()[0], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_load_returns_same_secret() {
        // load_or_create 的「重启后稳定」语义在 store 层的根保证。
        let dir = temp_dir("stable");
        save_in(&dir, "alice", &[9u8; SECRET_LEN]).unwrap();
        assert_eq!(load_in(&dir, "alice"), load_in(&dir, "alice"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn windows_blob_is_encrypted_not_plaintext() {
        let dir = temp_dir("dpapi");
        let secret = [5u8; SECRET_LEN];
        save_in(&dir, "alice", &secret).unwrap();
        let raw = std::fs::read(dir.join(seed_filename("alice"))).unwrap();
        assert_ne!(raw.as_slice(), &secret[..], "DPAPI blob 不得等于明文 secret");
        assert!(raw.len() > SECRET_LEN, "DPAPI blob 比明文更长");
        assert_eq!(load_in(&dir, "alice").as_deref(), Some(&secret[..]), "须能解回");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn unix_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perms");
        save_in(&dir, "alice", &[1u8; SECRET_LEN]).unwrap();
        let meta = std::fs::metadata(dir.join(seed_filename("alice"))).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
