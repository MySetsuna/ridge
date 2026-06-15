# TOTP 种子持久化设计

- 日期：2026-06-11
- 状态：已评审，待实施
- 涉及仓库：`wind`（`packages/ridge-core`、`src-tauri`、前端、`packages/ridge-cli`）

## 背景

远控二次验证用 RFC 6238 TOTP（契约 §4）。当前 `ridge-core` 的 `RemoteTotp`（唯一权威实现）与 `src-tauri` 的 `RemoteAuth`（历史重复实现）都在**每个进程启动时随机生成一份 secret、只活在内存里**（`RemoteTotp::new()` / `RemoteAuth::new()`）。

后果：每次重启 ridge → 全新种子 → 手机 authenticator 里旧的对不上 → 用户必须重新扫码。

用户诉求：种子跨重启稳定（不再重扫），但**绝不为此常驻任何后台进程**。

关键事实：TOTP 验证码 = `f(secret, 墙上时钟时间 / 30s)` 是纯函数。手机端在本地算，ridge 侧仅在「有人来验证时」按需现算一次比对。所以「码稳定」与「关闭后无后台进程」**毫不冲突**——当前要重扫的唯一根因是 secret 未落盘。

## 目标 / 非目标

**目标**
- TOTP 种子跨重启稳定：持久化落盘，启动时加载已有种子，无则生成并保存。
- 种子加密落 `%APPDATA%` 下（Windows DPAPI），按「Windows 用户 × 云账号」分隔。
- 桌面端、CLI、（dev 的 remote-server）三进程**共用同一份种子**。
- 云登录态变化时**实时切换**活动种子（登出用默认种子，登录 alice 用 alice 的种子）。
- 桌面端二维码上方提供「重置 TOTP 密钥」按钮。
- `RemoteAuth`（src-tauri）**接入 `RemoteTotp` 复用**，删掉重复的 RFC6238 实现。

**非目标**
- 不引入任何常驻/后台进程（TOTP 仍按需现算）。
- 不改 `VerifyThrottle` / `pre_verify_gate` 防爆破逻辑。
- 不给 CLI（TUI）加重置按钮（后续需要再说）。
- 不为 dev 的独立 `remote-server` 二进制做跨进程 IPC——它与桌面主进程不并发，读同一文件即可。

## 关键设计决策

1. **种子身份键 = 云账号 username**（非 username+device）。同一台机器一份设备绑定；区分的是不同云账号。未登录身份固定字面量 `"default"`。
2. **每身份一个文件**（非单一 JSON map）：并发简单、reset 只动一个文件、避免多进程改同一 map。
3. **Windows 用 DPAPI user-scope**（`CryptProtectData`，不加 `CRYPTPROTECT_LOCAL_MACHINE`）：绑当前 Windows 账户，拷走文件别的用户解不开；叠加 per-user AppData，天然实现「不同 Windows 用户种子不一样」。
4. **Unix 回退明文 + `0600`**（CLI 可能跑 Linux）：沿用 `config.rs::set_owner_only_perms` 的 cfg 写法，与现有 `auth.json` 落盘强度一致。
5. **持久化逻辑下沉 ridge-core**：路径解析由 ridge-core 统一（`directories::ProjectDirs("ridge")` config 目录，和 CLI `auth.json` 同根），保证三进程路径必然一致；与近期「路径判定下沉 ridge-core」方向一致。
6. **实时切换由前端驱动**：云登录态在前端（`cloud/auth.ts`），登录/登出时调 Tauri 命令通知 Rust 切种子并发事件刷新 UI。

## 架构与组件

### A. 持久化层 —— `ridge-core` 新增 `seed_store`

新增模块 `packages/ridge-core/src/seed_store.rs`，职责单一：种子的定位、加密落盘、读取。

- **目录**：`ProjectDirs::from("", "", "ridge").config_dir()` 下的 `totp/` 子目录（Windows = `%APPDATA%\ridge\config\totp\`，`%APPDATA%` 即 Roaming；与 CLI `auth.json` 同根；不存在则创建）。
- **身份 → 文件名**：`hex(sha256(identity))[..16] + ".seed"`（不在文件名泄露 username）；`"default"` 也走同一哈希规则。
- **接口**（`Identity` 用 `&str`）：
  - `fn load(identity: &str) -> Option<[u8; 20]>`：读文件 → 平台解密 → `Some`；缺失/损坏/解密失败 → `None`。
  - `fn save(identity: &str, secret: &[u8; 20]) -> Result<()>`：平台加密 → 原子写（临时文件 + rename）。
- **平台分层**（cfg）：
  - Windows：DPAPI `CryptProtectData` / `CryptUnprotectData`（user scope）。优先用已在依赖树的 `windows` crate 的 `Win32::Security::Cryptography`；若无则评估 `winapi`。文件内容 = DPAPI blob。
  - Unix：原始 20 字节，写后 `set_owner_only_perms`（0600）。
- **失败策略**：`save` 失败仅 `tracing::warn`（不阻断远控启动）；`load` 任何异常一律当作「无种子」走生成路径——宁可让用户重扫，不让远控起不来。

### B. `RemoteTotp` 改造（`ridge-core/src/totp.rs`）

在现有结构上加「身份 + 持久化」，纯算法（`totp_at`/`hmac_sha256`/`base32`/RFC 向量测试）一字不动。

- 结构体加 `identity: String` 字段。
- `fn load_or_create(identity: &str) -> Self`：`seed_store::load` 命中则用之；否则 `generate_secret()`（OsRng）并 `seed_store::save`。
- `fn reset(&mut self)`：重新生成 secret，`save` 覆盖当前身份，替换内存（旧验证器即失效）。
- `fn switch_identity(&mut self, identity: &str)`：对新身份 `load_or_create` 的逻辑替换 `secret` + `identity`。
- `fn new()` 保留为「随机、不落盘」，供单测与不关心持久化的场景；`Default` 仍走 `new()`。

### C. `RemoteAuth`（`src-tauri/src/remote/auth.rs`）接入 `RemoteTotp`

- **删除**重复实现：`totp_at`、`hmac_sha256`、`constant_time_eq`、`base32_encode`、`generate_secret`、`now_secs` 及常量。
- `RemoteAuth` 内部持 `RwLock<RemoteTotp>`（server 持 `Arc<RemoteAuth>`，实时切换需内部可变）。
- `current_code` / `verify` / `otpauth_uri` / `code_and_uri` 委托给内部 `RemoteTotp`。
- `RemoteAuth::new()` 内部改为 `RemoteTotp::load_or_create("default")`（启动时桌面尚未登录，先用默认种子；登录后由命令切换）。
- `SessionStore` / `VerifyThrottle` 保持不动。
- 新增 `fn reset_totp(&self)` 和 `fn switch_identity(&self, identity: Option<&str>)`（`None` → `"default"`），各自取 `RwLock` 写锁转调内部。

### D. 实时切换接线（desktop）

- 新增命令（`src-tauri/src/commands/remote.rs`）：
  - `remote_set_totp_identity(state, username: Option<String>)` → `remote_auth.switch_identity(...)` → 发 `remote-info-changed` 事件。
  - `remote_reset_totp(state)` → `remote_auth.reset_totp()` → 发 `remote-info-changed` 事件（并返回新的 `get_remote_info` payload，便于调用方直接刷新）。
  - 两命令在 `lib.rs` 的 `invoke_handler` 注册。
- 前端：
  - 在 `cloud/auth.ts` 设置云态成功处调 `invoke('remote_set_totp_identity', { username })`，清除云态处调 `invoke('remote_set_totp_identity', { username: null })`。
  - 渲染桌面二维码的组件监听 `remote-info-changed`，触发重新 `get_remote_info`。

### E. 重置按钮（UI）

- 在桌面端渲染 TOTP 二维码的组件，**二维码上方**加「重置 TOTP 密钥」按钮。
- 点击 → 二次确认（文案：「重置后已配对的验证器将失效，需重新扫码」）→ `invoke('remote_reset_totp')` → 用返回值/事件刷新二维码与验证码。
- 落点：`src/lib/remote/RemotePanel.svelte` 与/或 `src/routes/remote/+page.svelte`（写计划时定准哪个真正渲染桌面二维码；移动端/web-remote 页不加）。

### F. CLI（`packages/ridge-cli`）

- `session.rs`、`tui/dashboard.rs` 的 `RemoteTotp::new()` → `RemoteTotp::load_or_create(identity)`。
- 身份取 `config::load_auth()?.username`，无则 `"default"`。
- CLI 登录在启动时确定，**不做实时切换、不加重置按钮**。

## 数据流

```
启动（任一进程）
  RemoteTotp::load_or_create(identity)
    └─ seed_store::load(identity)
         命中 → 用持久化种子（码稳定，无需重扫）
         未命中 → OsRng 生成 → seed_store::save（DPAPI/0600 落盘）

验证（有人连入时，按需）
  RemoteAuth::verify(code) → RwLock 读 → RemoteTotp::verify

桌面登录态变化（前端驱动）
  cloud/auth.ts set/clear
    → invoke remote_set_totp_identity(username|null)
       → RemoteAuth::switch_identity → RwLock 写 → load_or_create(新身份)
       → emit remote-info-changed → 前端重拉 get_remote_info → 二维码刷新

重置（用户点按钮）
  invoke remote_reset_totp
    → RemoteAuth::reset_totp → 新种子覆盖保存
    → emit remote-info-changed → 二维码刷新（旧验证器失效）
```

## 安全与边界

- DPAPI user-scope + per-user AppData：比现有 `auth.json`（明文 + NTFS ACL）**只强不弱**。
- 种子落盘是新增的「静态密钥」，已由 DPAPI 缓解；威胁模型与 `auth.json` 中的 device JWT 一致。
- 不触碰 `VerifyThrottle` / `pre_verify_gate` / `post_verify_record` 的防爆破。
- **零后台进程**性质不变：verify 仍按需现算，关闭即无残留进程。

## 测试

- **seed_store**：
  - 存取往返（save 后 load 得同种子）。
  - 缺失文件 → `None`；损坏内容 → `None`（不 panic）。
  - 两身份隔离：两文件、两不同种子。
  - Windows：DPAPI 加密后磁盘字节 ≠ 明文种子（cfg(windows) 测）。
  - Unix：落盘权限为 0600（cfg(unix) 测）。
- **RemoteTotp**：
  - `load_or_create` 二次调用同身份 → 同种子（持久化生效）。
  - `reset` 后 secret 变化、仍自洽（current_code 能 verify）。
  - `switch_identity` 切换后 code 随之变化。
  - 现有 RFC6238 向量 / base32 向量不回归。
- **RemoteAuth 委托**：`code_and_uri`/`verify` 行为不变；`reset_totp`/`switch_identity` 后码变化。
- **前端**（轻量）：identity 变化触发 `get_remote_info` 重拉；重置按钮二次确认后调命令。

## 实施顺序（建议，逐项单独 commit）

1. ridge-core `seed_store`（含平台分层 + 测试）。
2. `RemoteTotp` 加 `load_or_create`/`reset`/`switch_identity`（+ 测试）。
3. `RemoteAuth` 接入 `RemoteTotp`、删重复实现。
4. desktop 两命令 + 注册 + 事件。
5. 前端登录态接线 + 二维码组件监听刷新。
6. 重置按钮 UI。
7. CLI `load_or_create` 接线。
