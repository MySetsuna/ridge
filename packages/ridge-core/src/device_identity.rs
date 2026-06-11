//! Ed25519 长期**设备身份**密钥（零信任 P0-1 / 方案 #2 地基）。
//!
//! 一台设备一个长期身份密钥对，**跨账户稳定**（设备级，非账户级）。私钥静态加密
//! 落盘：Windows DPAPI(user scope) / 其它平台明文 + `0600`（与 ridge-cli `auth.json`
//! 同强度）。**私钥绝不上线、绝不进 JS**（桌面端仅经 Tauri `invoke` 暴露 sign/public，
//! 见 `src-tauri/src/commands/remote.rs`；headless cli 直接持有）。
//!
//! 用途（后续 P2 握手帧接线）：E2EE 握手时 host 用本私钥**签名本次临时 X25519 公钥**，
//! controller 验签 + TOFU 固定指纹 → 被攻陷 relay 无私钥，无法替换公钥做 MITM。
//! 见 `wind/docs/superpowers/specs/2026-06-11-remote-zero-trust-crypto-design.md` §3。
//!
//! 失败一律降级：`load_or_create` 加载异常当"无身份"→ 现生成；保存失败仅 `warn`
//! （不阻断远控启动）。
//!
//! 注意：DPAPI FFI 与 `seed_store` 各持一份——本任务为**零回归**不改 `seed_store`；
//! 后续可抽共享 `dpapi` 模块统一。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};

/// Ed25519 私钥种子长度（32 字节）。
const SEED_LEN: usize = 32;
/// 签名长度（Ed25519 固定 64 字节）。
pub const SIGNATURE_LEN: usize = 64;
/// 公钥长度（Ed25519 固定 32 字节）。
pub const PUBLIC_KEY_LEN: usize = 32;

/// 一份长期设备身份（持 Ed25519 私钥）。
pub struct DeviceIdentity {
    signing: SigningKey,
}

impl DeviceIdentity {
    /// 从默认路径加载持久化身份；无/损坏则现生成并落盘（跨重启稳定的入口）。
    ///
    /// 路径：`ProjectDirs("ridge").config_dir()/device_identity.key`（与 cli `auth.json`
    /// / totp 种子同根）。解析不出配置目录时退化为**内存临时身份**（仅本进程有效，
    /// 重启即换 —— controller 需重新 TOFU；记 `warn`）。
    pub fn load_or_create() -> Self {
        match identity_path() {
            Some(path) => {
                if let Some(seed) = load_seed_at(&path) {
                    return Self::from_seed(&seed);
                }
                let seed = random_seed();
                if let Err(e) = save_seed_at(&path, &seed) {
                    tracing::warn!(
                        target: "ridge::device_identity",
                        error = %e,
                        "设备身份私钥持久化失败；本次用内存身份（重启将变更，controller 需重新信任）"
                    );
                }
                Self::from_seed(&seed)
            }
            None => {
                tracing::warn!(
                    target: "ridge::device_identity",
                    "无法解析配置目录；设备身份仅存内存（重启将变更）"
                );
                Self::from_seed(&random_seed())
            }
        }
    }

    /// 从 32 字节种子构造（确定性：同种子 → 同密钥对）。
    fn from_seed(seed: &[u8; SEED_LEN]) -> Self {
        Self {
            signing: SigningKey::from_bytes(seed),
        }
    }

    /// 设备身份**公钥**的 32 字节（controller 据此验签 + TOFU 固定）。
    pub fn public_bytes(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.signing.verifying_key().to_bytes()
    }

    /// 用设备身份私钥对 `msg` 做 Ed25519 签名，返回 64 字节签名。
    pub fn sign(&self, msg: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.signing.sign(msg).to_bytes()
    }

    /// 公钥指纹（`SHA-256(pub)` 前 8 字节，大写 hex 每 2 字节一组）。
    /// 供 TOFU 首次信任时**用户带外核对**展示（host 与 controller 显示同一串即匹配）。
    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.public_bytes())
    }
}

/// 32 字节公钥 → 指纹串（与 `DeviceIdentity::fingerprint` 同一算法；独立函数便于
/// controller 侧对收到的公钥算指纹比对）。
pub fn fingerprint_of(public_key: &[u8; PUBLIC_KEY_LEN]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(public_key);
    let mut s = String::with_capacity(20);
    for (i, b) in digest[..8].iter().enumerate() {
        if i > 0 && i % 2 == 0 {
            s.push('-');
        }
        let _ = write!(s, "{:02X}", b);
    }
    s
}

/// 32 字节随机种子（OS 熵源）。
fn random_seed() -> [u8; SEED_LEN] {
    use rand::RngCore;
    let mut buf = [0u8; SEED_LEN];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

/// 默认身份文件路径（不保证已创建；`save_seed_at` 会 `create_dir_all`）。
fn identity_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "ridge")?;
    Some(dirs.config_dir().join("device_identity.key"))
}

/// 读取并解密某路径的种子。缺失 / 损坏 / 解密失败 / 长度不符 → `None`。
fn load_seed_at(path: &Path) -> Option<[u8; SEED_LEN]> {
    let raw = std::fs::read(path).ok()?;
    let seed = decrypt(&raw)?;
    if seed.len() != SEED_LEN {
        return None;
    }
    let mut out = [0u8; SEED_LEN];
    out.copy_from_slice(&seed);
    Some(out)
}

/// 加密并原子写入种子（DPAPI/0600）。
fn save_seed_at(path: &Path, seed: &[u8; SEED_LEN]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let blob = encrypt(seed)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "encrypt failed"))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &blob)?;
    set_owner_only_perms(&tmp);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── 平台加密分层（镜像 seed_store；本任务零回归不复用其私有 mod）──────────────────

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

/// Windows DPAPI FFI（user scope，`CRYPTPROTECT_UI_FORBIDDEN` 确保 headless 不弹 UI）。
/// 与 `seed_store::dpapi` 等价实现；二者独立各一份（见模块头注释）。
#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;
    use std::ptr;

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
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    #[test]
    fn from_seed_is_deterministic() {
        let seed = [7u8; SEED_LEN];
        assert_eq!(
            DeviceIdentity::from_seed(&seed).public_bytes(),
            DeviceIdentity::from_seed(&seed).public_bytes(),
            "同种子必派生同公钥"
        );
    }

    #[test]
    fn distinct_seeds_yield_distinct_keys() {
        let a = DeviceIdentity::from_seed(&[1u8; SEED_LEN]);
        let b = DeviceIdentity::from_seed(&[2u8; SEED_LEN]);
        assert_ne!(a.public_bytes(), b.public_bytes());
    }

    #[test]
    fn signature_verifies_with_public_key() {
        let id = DeviceIdentity::from_seed(&[3u8; SEED_LEN]);
        let msg = b"ridge-id-bind-v1:ephemeral-pubkey";
        let sig = Signature::from_bytes(&id.sign(msg));
        let vk = VerifyingKey::from_bytes(&id.public_bytes()).unwrap();
        assert!(
            vk.verify(msg, &sig).is_ok(),
            "本设备签名必须能被本设备公钥验证"
        );
    }

    #[test]
    fn tampered_message_fails_verification() {
        let id = DeviceIdentity::from_seed(&[9u8; SEED_LEN]);
        let sig = Signature::from_bytes(&id.sign(b"original"));
        let vk = VerifyingKey::from_bytes(&id.public_bytes()).unwrap();
        assert!(vk.verify(b"tampered", &sig).is_err());
    }

    #[test]
    fn foreign_public_key_rejects_signature() {
        let id = DeviceIdentity::from_seed(&[4u8; SEED_LEN]);
        let other = DeviceIdentity::from_seed(&[5u8; SEED_LEN]);
        let sig = Signature::from_bytes(&id.sign(b"msg"));
        let vk_other = VerifyingKey::from_bytes(&other.public_bytes()).unwrap();
        assert!(
            vk_other.verify(b"msg", &sig).is_err(),
            "他设备公钥不得验过本设备签名（MITM 防线）"
        );
    }

    #[test]
    fn fingerprint_is_stable_uppercase_hex_groups() {
        let id = DeviceIdentity::from_seed(&[5u8; SEED_LEN]);
        let fp = id.fingerprint();
        assert_eq!(fp, id.fingerprint(), "指纹须稳定");
        assert!(fp.contains('-'), "指纹应分组");
        assert!(
            fp.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "指纹仅含 hex 与分隔符"
        );
        // host 算自己公钥的指纹，controller 用 fingerprint_of 算同一公钥 → 必一致。
        assert_eq!(fp, fingerprint_of(&id.public_bytes()));
    }

    #[test]
    fn fingerprint_golden_matches_browser() {
        // 跨实现 conformance：与浏览器 deviceTrust.test.ts 'golden' 同公钥同指纹
        // （id_pub = 0x11*32）。host(Rust) 与 controller(TS) 显示同一指纹，用户方能带外核对。
        assert_eq!(fingerprint_of(&[0x11u8; PUBLIC_KEY_LEN]), "02D4-49A3-1FBB-267C");
    }

    #[test]
    fn save_then_load_roundtrips_via_temp() {
        let dir = std::env::temp_dir().join(format!("ridge-devid-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device_identity.key");
        let seed = [42u8; SEED_LEN];
        save_seed_at(&path, &seed).unwrap();
        assert_eq!(load_seed_at(&path), Some(seed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_loads_none() {
        let path = std::env::temp_dir().join("ridge-devid-nonexistent-zzz.key");
        let _ = std::fs::remove_file(&path);
        assert!(load_seed_at(&path).is_none());
    }

    #[test]
    fn corrupt_file_loads_none() {
        let dir = std::env::temp_dir().join(format!("ridge-devid-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device_identity.key");
        std::fs::write(&path, b"\x00\x01\x02").unwrap();
        assert!(load_seed_at(&path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn windows_blob_is_encrypted_not_plaintext() {
        let dir = std::env::temp_dir().join(format!("ridge-devid-dpapi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device_identity.key");
        let seed = [5u8; SEED_LEN];
        save_seed_at(&path, &seed).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_ne!(raw.as_slice(), &seed[..], "DPAPI blob 不得等于明文 seed");
        assert_eq!(load_seed_at(&path), Some(seed), "须能解回");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
