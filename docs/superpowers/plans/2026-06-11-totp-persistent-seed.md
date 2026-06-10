# TOTP 种子持久化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** TOTP 种子跨重启稳定（不再重扫），加密落 AppData（Windows DPAPI / Unix 0600），按「Windows 用户 × 云账号」分隔，桌面/CLI 共用，登录态实时切换，桌面端二维码上方加重置按钮；零后台进程。

**Architecture:** 持久化下沉 `ridge-core` 新模块 `seed_store`（路径解析 + 平台加密落盘）。`RemoteTotp`（唯一权威实现）加 `load_or_create`/`reset`/`switch_identity`。`src-tauri` 的 `RemoteAuth` 删重复实现、改为内部持 `RwLock<RemoteTotp>` 委托。登录态由前端集中订阅 `cloudAuth` store → Tauri 命令实时切种子并发事件刷新 UI。

**Tech Stack:** Rust（ridge-core / src-tauri / ridge-cli）、手写 DPAPI FFI（crypt32）、`directories`/`sha2`、Tauri v2 命令 + 事件、Svelte 5 + vitest。

设计文档：`docs/superpowers/specs/2026-06-11-totp-persistent-seed-design.md`

---

## Task 1: ridge-core `seed_store` 模块（持久化层）

**Files:**
- Modify: `packages/ridge-core/Cargo.toml`（加 `directories = "5"`）
- Create: `packages/ridge-core/src/seed_store.rs`
- Modify: `packages/ridge-core/src/lib.rs:40-41`（加 `mod seed_store;`）

- [ ] **Step 1: 加 `directories` 依赖**

在 `packages/ridge-core/Cargo.toml` 的 `[dependencies]` 段，紧接 `dirs = "5"` 那行之后加一行（与 ridge-cli 同版本，workspace Cargo.lock 已有）：

```toml
# TOTP 种子持久化路径解析（与 ridge-cli auth.json 同根：ProjectDirs("ridge")
# config_dir）。纯 crate，无 Tauri。见 src/seed_store.rs。
directories = "5"
```

- [ ] **Step 2: 注册模块**

`packages/ridge-core/src/lib.rs`，在 `pub mod sandbox;`（第 40 行）与 `pub mod totp;`（第 41 行）之间插入：

```rust
mod seed_store;
```

（私有：仅 `totp.rs` 内部使用。）

- [ ] **Step 3: 写 `seed_store.rs`（含失败测试 + 实现 + DPAPI FFI）**

完整写入 `packages/ridge-core/src/seed_store.rs`：

```rust
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
        let mut out = DataBlob { cb_data: 0, pb_data: ptr::null_mut() };
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
        let result =
            unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
        unsafe { LocalFree(out.pb_data as *mut c_void) };
        Some(result)
    }

    pub fn unprotect(blob: &[u8]) -> Option<Vec<u8>> {
        let in_blob = DataBlob {
            cb_data: blob.len() as u32,
            pb_data: blob.as_ptr() as *mut u8,
        };
        let mut out = DataBlob { cb_data: 0, pb_data: ptr::null_mut() };
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
        let result =
            unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
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
```

- [ ] **Step 4: 跑 seed_store 测试**

Run: `cargo test -p ridge-core seed_store`
Expected: PASS（Windows 上含 `windows_blob_is_encrypted_not_plaintext` 与 5 个跨平台测试；Unix 上含 `unix_file_is_owner_only`）。

> 注意（项目环境）：常驻 `tauri dev` 可能占用 workspace 构建锁，`cargo test` 会等待——属正常，**不要杀** dev 进程（见用户 memory）。

- [ ] **Step 5: 提交**

```bash
git add packages/ridge-core/Cargo.toml packages/ridge-core/src/seed_store.rs packages/ridge-core/src/lib.rs
git commit -m "feat(totp): ridge-core seed_store 种子加密持久化（DPAPI/0600）"
```

---

## Task 2: `RemoteTotp` 加持久化方法

**Files:**
- Modify: `packages/ridge-core/src/totp.rs`

- [ ] **Step 1: 给结构体加 `identity` 字段 + 改 `new()`**

`packages/ridge-core/src/totp.rs`，把结构体与 `new()` 改为：

```rust
/// 一份 TOTP 上下文。`secret` 可由 `load_or_create` 从磁盘恢复（跨重启稳定），
/// `identity` 记住其归属身份（`"default"` 或云账号 username），供 reset/switch 用。
pub struct RemoteTotp {
    secret: Vec<u8>,
    identity: String,
}

impl RemoteTotp {
    /// 生成随机 secret 的新实例（OS 熵源），**不落盘**。供单测与不关心持久化处。
    pub fn new() -> Self {
        Self {
            secret: generate_secret(),
            identity: String::new(),
        }
    }
```

（其余 `current_code` / `verify` / `otpauth_uri` / `period_secs` 不动。）

- [ ] **Step 2: 在 `period_secs()` 之后、`impl` 块结束 `}` 之前，加持久化方法**

```rust
    /// 按身份加载持久化 secret；无则生成并落盘（跨重启稳定的入口）。
    pub fn load_or_create(identity: &str) -> Self {
        let secret = crate::seed_store::load(identity).unwrap_or_else(|| {
            let s = generate_secret();
            crate::seed_store::save(identity, &s);
            s
        });
        Self {
            secret,
            identity: identity.to_string(),
        }
    }

    /// 仅替换内存 secret（不落盘）——拆出来便于无磁盘单测。
    fn regenerate(&mut self) {
        self.secret = generate_secret();
    }

    /// 重置当前身份的 secret：新生成 + 覆盖落盘（旧验证器即失效）。
    pub fn reset(&mut self) {
        self.regenerate();
        crate::seed_store::save(&self.identity, &self.secret);
    }

    /// 切换到另一身份的 secret（无则现生成并落盘）。
    pub fn switch_identity(&mut self, identity: &str) {
        let next = Self::load_or_create(identity);
        self.secret = next.secret;
        self.identity = next.identity;
    }
```

- [ ] **Step 3: 加一个无磁盘单测（reset 的内存效果）**

在 `totp.rs` 的 `#[cfg(test)] mod tests` 内，`base32_known_vector` 测试之后加：

```rust
    #[test]
    fn regenerate_changes_secret_and_self_verifies() {
        let mut totp = RemoteTotp::new();
        let before = totp.secret.clone();
        totp.regenerate();
        assert_ne!(totp.secret, before, "regenerate 必须换掉 secret");
        let code = totp.current_code();
        assert!(totp.verify(&code), "regenerate 后的码须能自洽校验");
    }
```

- [ ] **Step 4: 跑 totp 测试**

Run: `cargo test -p ridge-core totp`
Expected: PASS（原有 RFC6238 / base32 向量 + 新增 `regenerate_changes_secret_and_self_verifies`）。

- [ ] **Step 5: 提交**

```bash
git add packages/ridge-core/src/totp.rs
git commit -m "feat(totp): RemoteTotp 加 load_or_create/reset/switch_identity"
```

---

## Task 3: `RemoteAuth` 接入 `RemoteTotp`、删重复实现

**Files:**
- Modify: `src-tauri/src/remote/auth.rs:1-159`（替换文件顶部的 TOTP 段）

- [ ] **Step 1: 替换 import + `RemoteAuth`（删掉重复 RFC6238 实现）**

把 `src-tauri/src/remote/auth.rs` 顶部第 1 行到第 159 行（即 `use ...` 到 `base32_encode` 函数结束，紧接 `SESSION_TTL` 常量注释之前）**整段替换**为：

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use ridge_core::RemoteTotp;

/// 本机 TOTP 持有者（契约 §4）。内部委托 ridge-core 的 `RemoteTotp`（唯一权威
/// 实现），secret 由其持久化层跨重启恢复。`RwLock` 支持登录态变化时**实时切换**
/// 活动种子（远控 server 持 `Arc<RemoteAuth>`，需内部可变）。
pub struct RemoteAuth {
    totp: RwLock<RemoteTotp>,
}

impl RemoteAuth {
    /// 启动时桌面尚未登录 → 先用 `"default"` 身份的持久化种子；登录后由
    /// `switch_identity` 切到账号专属种子。
    pub fn new() -> Self {
        Self {
            totp: RwLock::new(RemoteTotp::load_or_create("default")),
        }
    }

    /// 生成当前 6 位 TOTP。
    pub fn current_code(&self) -> String {
        self.totp.read().current_code()
    }

    /// 校验用户输入的 code（±1 窗口在 `RemoteTotp::verify` 内）。
    pub fn verify(&self, code: &str) -> bool {
        self.totp.read().verify(code)
    }

    /// 当前 code + otpauth URI 一次取（同一把读锁，避免时间步在两次调用间跳变）。
    pub fn code_and_uri(&self, machine_name: &str) -> (String, String) {
        let g = self.totp.read();
        (g.current_code(), g.otpauth_uri(machine_name))
    }

    /// 供二维码生成的 `otpauth://` URI。
    pub fn otpauth_uri(&self, machine_name: &str) -> String {
        self.totp.read().otpauth_uri(machine_name)
    }

    /// 重置当前身份的种子（已配对验证器即失效，需重新扫码）。
    pub fn reset_totp(&self) {
        self.totp.write().reset();
    }

    /// 切换活动种子到指定云身份（`None` → `"default"`）。登录/登出时调用。
    pub fn switch_identity(&self, username: Option<&str>) {
        self.totp.write().switch_identity(username.unwrap_or("default"));
    }

    /// 测试用：临时随机种子，不落盘（避免单测写真实 AppData）。
    #[cfg(test)]
    fn ephemeral() -> Self {
        Self {
            totp: RwLock::new(RemoteTotp::new()),
        }
    }
}
```

> 说明：原 `use std::time::{... SystemTime, UNIX_EPOCH}` 收窄为 `{Duration, Instant}`（`now_secs` 已删，`SESSION_TTL` 用 `Duration`、`SessionRecord` 用 `Instant`）；原 `use sha2::{Digest, Sha256}` 整行删除（HMAC 已不在本文件）。`SessionStore` / `VerifyThrottle` / `generate_session_token` 及其下所有内容**保持不动**。

- [ ] **Step 2: 删掉失效的旧测试**

在 `auth.rs` 的 `#[cfg(test)] mod tests` 内，删除整段 `totp_secret_is_20_bytes_and_varies` 测试（它引用已删除的 `generate_secret`；secret 生成逻辑已在 ridge-core `totp.rs` 测过）：

```rust
    #[test]
    fn totp_secret_is_20_bytes_and_varies() {
        let s1 = generate_secret();
        let s2 = generate_secret();
        assert_eq!(s1.len(), 20, "RFC 6238 / our otpauth uses a 160-bit seed");
        assert_ne!(s1, s2, "two CSPRNG draws must differ");
    }
```

- [ ] **Step 3: 加一个委托自洽测试（无磁盘）**

在同一 `mod tests` 内（紧接 `session_tokens_are_distinct` 之后）加：

```rust
    #[test]
    fn ephemeral_auth_verifies_its_own_code() {
        let auth = RemoteAuth::ephemeral();
        let (code, uri) = auth.code_and_uri("My Machine");
        assert!(auth.verify(&code), "RemoteAuth 须能校验自己当前的 code");
        assert!(uri.starts_with("otpauth://totp/Ridge:"), "otpauth URI 形状正确");
    }
```

- [ ] **Step 4: 验证编译**

Run: `cargo build -p ridge --tests`
Expected: 编译通过（test profile 编译即覆盖 `remote::auth` 全部代码 + 新 `ephemeral_auth_verifies_its_own_code`；不再有 `generate_secret` 引用）。`reset_totp`/`switch_identity` 暂报 dead_code 警告（Task 4 接入后消除）。

> ⚠️ 已知限制（本会话实测确认，预先存在、与本改动无关）：`cargo test -p ridge --lib`
> 的**测试 harness exe 无法独立启动**——它从 `rfd`/`tauri-plugin-dialog` 链入
> `TaskDialogIndirect`（comctl32 v6），而裸测试 exe 没有 Common-Controls v6 应用清单，
> 加载器报 `STATUS_ENTRYPOINT_NOT_FOUND`。真实 `ridge.exe`（tauri-build 嵌了清单）不受影响。
> 故 src-tauri 内的单测**只编译验证**；`remote::auth` 的运行期行为由 ridge-core 的
> RemoteTotp 测试（已跑通）+ Task 6 真机烟雾共同保证。
> 若常驻 `tauri dev` 占用构建锁，命令会等待——正常，勿杀 dev。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/remote/auth.rs
git commit -m "refactor(totp): RemoteAuth 委托 ridge-core RemoteTotp，删重复 RFC6238 实现"
```

---

## Task 4: 桌面命令 `remote_reset_totp` + `remote_set_totp_identity`

**Files:**
- Modify: `src-tauri/src/commands/remote.rs:1-6`（import）+ 在 `verify_remote_totp` 之后插入两命令
- Modify: `src-tauri/src/lib.rs:701-704`（注册命令）

- [ ] **Step 1: 加 Emitter import**

`src-tauri/src/commands/remote.rs` 顶部，在 `use tauri::State;`（第 3 行）之后加：

```rust
use tauri::{AppHandle, Emitter};
```

- [ ] **Step 2: 加两个命令**

在 `verify_remote_totp` 函数（`pub fn verify_remote_totp(...) -> bool { ... }`，约第 80-83 行）之后插入：

```rust
/// §totp-persist：重置本机 TOTP 种子。重新生成 + 覆盖落盘（DPAPI/0600），已配对
/// 的 authenticator 立即失效，须重新扫码。发 `remote-totp-changed` 事件让面板刷新。
#[tauri::command]
pub fn remote_reset_totp(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    state.remote_auth.reset_totp();
    let _ = app.emit("remote-totp-changed", ());
    tracing::info!(target: "ridge::remote", "TOTP secret reset by user");
    Ok(())
}

/// §totp-persist：把活动 TOTP 种子切到指定云身份（`None`/登出 → `"default"`）。
/// 由前端在云登录态变化时调用，实现「不同账号不同种子」的实时切换。发
/// `remote-totp-changed` 事件让面板刷新二维码/验证码。
#[tauri::command]
pub fn remote_set_totp_identity(
    app: AppHandle,
    state: State<AppState>,
    username: Option<String>,
) -> Result<(), String> {
    state.remote_auth.switch_identity(username.as_deref());
    let _ = app.emit("remote-totp-changed", ());
    Ok(())
}
```

- [ ] **Step 3: 注册命令**

`src-tauri/src/lib.rs`，在 `commands::remote::verify_remote_totp,`（第 703 行）之后加两行：

```rust
            commands::remote::remote_reset_totp,
            commands::remote::remote_set_totp_identity,
```

- [ ] **Step 4: 验证编译**

Run: `cargo build -p ridge`
Expected: 编译通过，无 `unused`/未注册命令警告。

> 命令层为薄封装，逻辑已在 Task 1/2/3 单测覆盖；此处以编译 + 后续 Task 6 的手测烟雾验证接线。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/remote.rs src-tauri/src/lib.rs
git commit -m "feat(totp): 桌面命令 remote_reset_totp + remote_set_totp_identity（含事件）"
```

---

## Task 5: 前端登录态实时同步模块

**Files:**
- Create: `src/lib/remote/totpIdentitySync.ts`
- Create: `src/lib/remote/totpIdentitySync.test.ts`
- Modify: `src/routes/+layout.svelte:35-39`（桌面分支接入）

- [ ] **Step 1: 写失败测试**

完整写入 `src/lib/remote/totpIdentitySync.test.ts`：

```ts
import { describe, it, expect, vi } from 'vitest';
import { writable } from 'svelte/store';
import { startTotpIdentitySync } from './totpIdentitySync';
import type { CloudAuthState } from './cloud/auth';

function state(username: string | null): CloudAuthState {
  return {
    userToken: username ? 'tok' : null,
    user: username ? ({ username } as unknown as CloudAuthState['user']) : null,
    deviceToken: null,
    deviceName: null,
  };
}

describe('startTotpIdentitySync', () => {
  it('登录时按 username 调命令，重复同值不再调，登出回 null', () => {
    const store = writable<CloudAuthState>(state(null));
    const invoke = vi.fn().mockResolvedValue(undefined);

    const stop = startTotpIdentitySync(invoke, store);
    // 初次订阅：当前 username=null。
    expect(invoke).toHaveBeenLastCalledWith('remote_set_totp_identity', { username: null });
    expect(invoke).toHaveBeenCalledTimes(1);

    store.set(state('alice'));
    expect(invoke).toHaveBeenLastCalledWith('remote_set_totp_identity', { username: 'alice' });
    expect(invoke).toHaveBeenCalledTimes(2);

    // 同一 username 再次推送（如 user 对象刷新）→ 不重复调用。
    store.set(state('alice'));
    expect(invoke).toHaveBeenCalledTimes(2);

    store.set(state(null));
    expect(invoke).toHaveBeenLastCalledWith('remote_set_totp_identity', { username: null });
    expect(invoke).toHaveBeenCalledTimes(3);

    stop();
  });
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm vitest run src/lib/remote/totpIdentitySync.test.ts`
Expected: FAIL（`startTotpIdentitySync` 未定义 / 模块不存在）。

- [ ] **Step 3: 写实现**

完整写入 `src/lib/remote/totpIdentitySync.ts`：

```ts
// §totp-persist：把云登录态实时同步给 Rust 侧 TOTP 种子选择。
//
// 订阅 cloudAuth store，仅在 username **真正变化**时调 `remote_set_totp_identity`
// （登录→账号专属种子；登出→默认种子）。只在真实桌面 host 启用（见 +layout 守卫），
// 绝不在 web-remote controller 跑——否则会把「控制端」的登录态隧道到 host，污染 host 种子。

import { cloudAuth as cloudAuthStore } from './cloud/auth';
import type { CloudAuthState } from './cloud/auth';

type InvokeFn = <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type StoreLike = { subscribe: (run: (s: CloudAuthState) => void) => () => void };

/**
 * 启动同步。返回取消订阅函数。
 * @param invoke 真实 Tauri `invoke`（测试可注入 mock）。
 * @param store  cloudAuth store（默认全局；测试可注入 writable）。
 */
export function startTotpIdentitySync(
  invoke: InvokeFn,
  store: StoreLike = cloudAuthStore,
): () => void {
  // undefined 哨兵：确保首次订阅必触发一次（与任何真实 username 都不等）。
  let last: string | null | undefined = undefined;
  return store.subscribe((s) => {
    const username = s.user?.username ?? null;
    if (username === last) return;
    last = username;
    void invoke('remote_set_totp_identity', { username });
  });
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm vitest run src/lib/remote/totpIdentitySync.test.ts`
Expected: PASS

- [ ] **Step 5: 接入桌面 boot（`+layout.svelte`）**

`src/routes/+layout.svelte`：先在 `<script>` 顶部 import 段（`import { t, tr } from '$lib/i18n';` 第 9 行之后）加：

```ts
  import { invoke } from '@tauri-apps/api/core';
  import { startTotpIdentitySync } from '$lib/remote/totpIdentitySync';
```

再把 `onMount` 里的桌面分支（第 36-39 行）：

```ts
    if (!WEB_REMOTE) {
      setTransport(new TauriDataProvider());
      return;
    }
```

改为：

```ts
    if (!WEB_REMOTE) {
      setTransport(new TauriDataProvider());
      // §totp-persist：仅真实桌面 host 同步登录态→TOTP 种子（web-remote 已被
      // WEB_REMOTE 分支排除，不会到这）。
      const stopTotpSync = startTotpIdentitySync(invoke);
      return () => stopTotpSync();
    }
```

- [ ] **Step 6: 验证类型 + 测试**

Run: `pnpm vitest run src/lib/remote/totpIdentitySync.test.ts && pnpm svelte-check --threshold error`
Expected: 测试 PASS；svelte-check 无新增 error（仅检查不被既有警告阻断）。

- [ ] **Step 7: 提交**

```bash
git add src/lib/remote/totpIdentitySync.ts src/lib/remote/totpIdentitySync.test.ts src/routes/+layout.svelte
git commit -m "feat(totp): 前端登录态实时同步活动 TOTP 种子"
```

---

## Task 6: 桌面二维码上方「重置」按钮

**Files:**
- Modify: `src/lib/i18n/messages.ts`（加 `remote.resetTotp` / `remote.resetTotpConfirm`，zh + en）
- Modify: `src/lib/remote/RemotePanel.svelte`（onMount 监听事件 + reset 处理 + 按钮）

- [ ] **Step 1: 加 i18n 文案**

在 `src/lib/i18n/messages.ts` 中找到现有 `remote.totpCode` / `remote.qrBindAuth` 所在的 `remote` 文案块，在中文（zh）对象的 `remote` 段加：

```ts
    resetTotp: '重置 TOTP 密钥',
    resetTotpConfirm: '重置后已配对的验证器将立即失效，需要重新扫码。确定重置？',
```

在英文（en）对象的 `remote` 段加：

```ts
    resetTotp: 'Reset TOTP secret',
    resetTotpConfirm: 'Resetting invalidates all paired authenticators — you must re-scan. Reset now?',
```

> 若 messages.ts 还有其它语言对象，按相同两键各补一条（值可暂用英文）。键名必须与上面完全一致。

- [ ] **Step 2: RemotePanel 引入 confirm + reset 处理 + 事件监听**

`src/lib/remote/RemotePanel.svelte` `<script>` 内：

(a) 在 `refreshRemoteInfo()` 函数（约第 142-150 行）之后，新增重置处理函数：

```ts
  // §totp-persist：重置本机 TOTP 种子（桌面 host 专属；web-remote 不渲染该按钮）。
  // 二次确认后调命令；Rust 发 remote-totp-changed → onMount 的 listener 刷新二维码。
  let resettingTotp = $state(false);
  async function resetTotp(): Promise<void> {
    if (resettingTotp) return;
    const { confirm } = await import('@tauri-apps/plugin-dialog');
    const ok = await confirm($t('remote.resetTotpConfirm'), { title: $t('remote.resetTotp'), kind: 'warning' });
    if (!ok) return;
    resettingTotp = true;
    try {
      await invoke('remote_reset_totp');
      await refreshRemoteInfo();
    } catch (e: unknown) {
      connectError = e instanceof Error ? e.message : tr('remote.toggleFailed');
    } finally {
      resettingTotp = false;
    }
  }
```

(b) 在 `onMount`（约第 335-347 行）里，`refreshRemoteInfo();` 之后加一行事件监听，并在 cleanup 取消：

把：

```ts
  onMount(() => {
    refreshRemoteInfo();
```

改为：

```ts
  onMount(() => {
    refreshRemoteInfo();
    // §totp-persist：种子被重置 / 登录态切换后，Rust 发此事件 → 刷新二维码+码。
    const unlistenTotp = listen('remote-totp-changed', () => { void refreshRemoteInfo(); });
```

并把 `onMount` 的 return cleanup：

```ts
    return () => {
      if (totpTimer) clearInterval(totpTimer);
      if (devicesTimer) clearInterval(devicesTimer);
      host?.goOffline();
    };
```

改为：

```ts
    return () => {
      if (totpTimer) clearInterval(totpTimer);
      if (devicesTimer) clearInterval(devicesTimer);
      host?.goOffline();
      void unlistenTotp.then((un) => un());
    };
```

- [ ] **Step 3: 加按钮（二维码上方）**

`RemotePanel.svelte` 模板里，共享 TOTP 块（约第 382-393 行）内，把 QR 那段：

```svelte
        <div class="flex flex-col items-center gap-1 pt-1">
          <p class="text-[10px] text-[var(--rg-fg-muted)]">{$t('remote.qrBindAuth')}</p>
          <QrCode value={remoteInfo.otpauthUri} size={132} />
        </div>
```

改为（在 `<p>` 提示与 `<QrCode>` 之间、即二维码**上方**插入重置按钮，并用 `RIDGE_WEB_REMOTE` 守卫只在真实桌面显示）：

```svelte
        <div class="flex flex-col items-center gap-1 pt-1">
          <p class="text-[10px] text-[var(--rg-fg-muted)]">{$t('remote.qrBindAuth')}</p>
          {#if import.meta.env.RIDGE_WEB_REMOTE !== true}
            <button
              onclick={resetTotp}
              disabled={resettingTotp}
              class="flex items-center gap-1 text-[10px] text-[var(--rg-fg-muted)] hover:text-red-400 transition-colors disabled:opacity-50"
              title={$t('remote.resetTotp')}
            >
              <RefreshCw class="w-3 h-3 {resettingTotp ? 'animate-spin' : ''}" />
              {$t('remote.resetTotp')}
            </button>
          {/if}
          <QrCode value={remoteInfo.otpauthUri} size={132} />
        </div>
```

（`RefreshCw`、`invoke`、`listen`、`$t`、`tr`、`connectError` 均已在本组件 import/声明。）

- [ ] **Step 4: 类型检查**

Run: `pnpm svelte-check --threshold error`
Expected: 无新增 error。

- [ ] **Step 5: 手测烟雾（功能验证）**

借常驻 `tauri dev`（勿新起实例、勿杀发布版 ridge）：

1. 打开 Remote 面板，启用远控 → 记下二维码/验证码。
2. 点「重置 TOTP 密钥」→ 确认 → 二维码与验证码**立即变化**（事件刷新生效）。
3. 用手机 authenticator 扫新码 → 远控验证通过。
4. 重启 ridge → 二维码/验证码**与重启前一致**（持久化生效，无需重扫）。
5.（可选）登录另一云账号 → 二维码切换为该账号专属；登出 → 回默认种子。

Expected: 1-4 全部符合；零后台进程（关闭 ridge 后任务管理器无残留）。

- [ ] **Step 6: 提交**

```bash
git add src/lib/i18n/messages.ts src/lib/remote/RemotePanel.svelte
git commit -m "feat(totp): 桌面二维码上方加重置按钮 + 监听 remote-totp-changed 刷新"
```

---

## Task 7: CLI 接入持久化种子

**Files:**
- Modify: `packages/ridge-cli/src/config.rs`（加 `totp_identity()` 辅助）
- Modify: `packages/ridge-cli/src/session.rs`（构造点）
- Modify: `packages/ridge-cli/src/tui/dashboard.rs:96`（构造点）

- [ ] **Step 1: 加身份辅助函数**

`packages/ridge-cli/src/config.rs`，在 `load_auth()` 函数之后加：

```rust
/// 本机 CLI 的 TOTP 身份：已激活则取 `auth.json` 的 username，否则 `"default"`
/// （与桌面端登出态共用同一份默认种子 → 双端同账号自然同种子）。
pub fn totp_identity() -> String {
    load_auth()
        .ok()
        .flatten()
        .map(|a| a.username)
        .unwrap_or_else(|| "default".to_string())
}
```

- [ ] **Step 2: 改 `session.rs` 的生产构造点**

`packages/ridge-cli/src/session.rs`，把**非测试**代码里的 `let totp = RemoteTotp::new();`（约第 143 行）改为：

```rust
        let totp = RemoteTotp::load_or_create(&crate::config::totp_identity());
```

> 注意：第 687 行附近若位于 `#[cfg(test)] mod` 内，**保持 `RemoteTotp::new()` 不变**（测试要的是临时随机种子，不落盘）。仅改生产路径的构造点。

- [ ] **Step 3: 改 `dashboard.rs` 的构造点**

`packages/ridge-cli/src/tui/dashboard.rs`，把第 96 行：

```rust
        let totp = Arc::new(RemoteTotp::new());
```

改为：

```rust
        let totp = Arc::new(RemoteTotp::load_or_create(&crate::config::totp_identity()));
```

- [ ] **Step 4: 验证编译**

Run: `cargo build -p ridge-cli`
Expected: 编译通过。

- [ ] **Step 5: 提交**

```bash
git add packages/ridge-cli/src/config.rs packages/ridge-cli/src/session.rs packages/ridge-cli/src/tui/dashboard.rs
git commit -m "feat(totp): ridge-cli 用持久化种子（load_or_create，身份取 auth.json）"
```

---

## 收尾：全量验证

- [ ] **Step 1: ridge-core 全测**

Run: `cargo test -p ridge-core`
Expected: PASS（seed_store + totp 全绿）。

- [ ] **Step 2: 前端单测**

Run: `pnpm vitest run src/lib/remote/totpIdentitySync.test.ts`
Expected: PASS

- [ ] **Step 3: 桌面/CLI 编译**

Run: `cargo build -p ridge && cargo build -p ridge-cli`
Expected: 均通过（若 dev 占锁则等待，勿杀 dev）。

- [ ] **Step 4: 回归手测**：复跑 Task 6 Step 5 的 1-4 项（重置即时生效 + 重启后稳定 + 零后台进程）。
