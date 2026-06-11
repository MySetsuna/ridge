# Remote 零信任加密修复方案（纵深防御）— 实现级设计

> **状态**：设计 spec（本轮**只出方案、不改代码**）。
> **作者**：crypto-reviewer（remote-review 团队）｜**日期**：2026-06-11
> **关联**：审核报告 `.agent-team/findings-crypto.md`（P0×2/P1×3/P2×4）；本文档给出其修复落地方案。
> **协调边界**：信令字段的**载体 schema**以 align-reviewer 的「信令 schema 单一事实来源」为准；**本文档定义字段语义与校验规则**（见 §7）。

---

## 0. 目标与威胁模型

**威胁模型（评判基准）**：ridge-cloud 公网服务器被攻击者**完全攻破**（磁盘/内存/数据库/TLS 私钥全失）后仍须满足：
- **(a)** 无法解密任何进行中/历史远控会话内容；
- **(b)** 无法冒充控制方/被控方、无法注入指令、无法接管会话；
- **(c)** 服务器只是盲中继：只转发密文，不持任何能解密会话的长期密钥或明文。

**当前差距（审核结论）**：已有真正的应用层 E2EE（X25519 + HKDF-SHA256 + ChaCha20-Poly1305），但**密钥协商无服务器无法伪造的信任锚**——临时公钥的旁路校验（`e2ee-pubkey`）与 DataChannel 同经被攻陷中继转发，且校验失败 fail-open 回落 `relay-trust`。身份完全植根于服务器独持的 HS256 对称密钥。故被攻陷的**主动**中继可 MITM 解密/注入，(a)(b)(c) 均不满足。

**为什么必须纵深（五层缺一不可）**：

| # | 修复项 | 关闭的攻击面 | 单独是否足够 |
|---|---|---|---|
| 1 | TOTP 信道绑定（SAS/PAKE-lite） | 中继 MITM（用户在场/输码场景） | 否——需配合 #3 去降级、#5 强制门控 |
| 2 | Ed25519 设备身份签名 + TOFU pin | 中继 MITM（无人值守/自动重连场景） | 否——首次信任靠 TOFU，需 #1 的 SAS 兜底核验 |
| 3 | fail-open → fail-closed | 降级逃逸（中继丢弃旁路帧强制回落） | 否——只堵逃逸，不提供信任锚 |
| 4 | JWT HS256 → EdDSA 非对称 | 只读泄露伪造 token；缩小伪造面 | 否——全主机攻陷仍可拿私钥，但 token 不再等于解密能力（靠 #1#2） |
| 5 | cloud TOTP 门控默认开启 | 「未注入校验器即零二次验证」 | 否——二次验证仍可被 MITM 旁路（需 #1 把它升级为信道绑定） |

核心思想：**#1 + #2 提供信任锚（让中继无法替换密钥）；#3 杜绝降级绕过；#4 让 token 泄露不等于身份伪造；#5 保证二次验证始终在线**。五者叠加后，"服务器持有的任何东西"都不再能解密或注入会话。

---

## 1. 现状基线（精确锚点，便于改动定位）

**E2EE 实现（三端，字节级对齐，契约 §7）**
- TS（controller + 桌面 host/WebView）：`src/lib/remote/cloud/e2ee.ts:42-222`（`generateEphemeralKeyPair`/`deriveSessionKey`/`E2eeSession.seal|open`），握手帧 `encodeHandshakeFrame`=`0x01||pub32`（:48-65）。
- Rust（ridge-cli 无头 host `rdg`）：`packages/ridge-cli/src/e2ee.rs:55-234`（`Handshake`/`Session`），帧 `0x01||pub32`（:74-101）。
- 派生：`X25519 → salt=sort(myPub,peerPub) → HKDF-SHA256(info="ridge-e2ee-v1",L=32)`；nonce=`dir(1)||000||counter_u64_le`，counter 严格递增防重放（已正确）。

**密钥绑定（D-GM-10 / B3，现状缺陷）**
- `src/lib/remote/cloud/keyBinding.ts:100-113`（`decideKeyBinding`：旁路公钥未到 + `graceExpired` → **`accept`（fail-open）**）。
- host：`src/lib/remote/cloud/ridgeCloudProvider.ts:301-410,522-534`；controller：`src/lib/remote/cloud/controllerCloudProvider.ts:202-294,412-420`（`KEY_BIND_GRACE_MS=3000`）。
- 旁路通道 `e2ee-pubkey` 经 relay 转发：`ridge-cloud/src/ws/handler.rs:395-469`（转发任意信令 JSON）。

**TOTP（契约 §4）**
- 唯一权威实现：`packages/ridge-core/src/totp.rs:29-181`（HMAC-SHA256，6 位，±1 窗口，恒定时间比较）。
- 种子持久化：`packages/ridge-core/src/seed_store.rs:21-205`（Windows **DPAPI** user-scope，类 Unix `0600`，文件名 `hex(sha256(identity)[..8]).seed`）；**种子绝不上线**。
- 桌面 host 校验入口：`src-tauri/src/remote/auth.rs:10-61`（`RemoteAuth::verify`）；cloud 桥注入：`src/lib/remote/RemotePanel.svelte:293`（`verify_remote_totp`）。
- cloud 门控（缺陷）：`src/lib/remote/cloud/cloudHostBridge.ts:178-199,286-324`（`verified = !config.totpVerifier` → **未注入校验器即默认放行**）；controller 侧：`src/lib/remote/cloud/cloudControllerBoot.ts:140-166`（明文 `{t:'totp-verify',code}`）。

**JWT（契约 §3，缺陷）**
- `ridge-cloud/src/auth/jwt.rs:81-159`（**HS256 对称**，`EncodingKey/DecodingKey::from_secret`，同一密钥签+验）。
- 配置：`ridge-cloud/src/config.rs:79-84`（`JWT_SECRET`，最小仅 16 字符）；构造：`ridge-cloud/src/state.rs:33`。
- 签发点：`ridge-cloud/src/api/auth_routes.rs:78,143,222,319,518`（user token）、`ridge-cloud/src/api/device_routes.rs:84,273`（device token）。
- 验签点：`ridge-cloud/src/auth/extract.rs:37`（HTTP）、`ridge-cloud/src/ws/handler.rs:60`（WS）。

**设备凭据存储**
- ridge-cli：`packages/ridge-cli/src/config.rs:68-75`（`AuthFile{token,device_name,username}`，**无设备密钥对**；Linux `0600`，Windows 仅靠 NTFS ACL）。
- 桌面：`src/lib/remote/cloud/auth.ts:18,55-75`（**localStorage** `ridge.cloud.deviceToken` 等——**不可存私钥**）。

**库现状**
- TS：`@noble/curves`（x25519/ed25519）、`@noble/hashes`（hkdf/sha256/hmac）、`@noble/ciphers`（chacha20poly1305）——**已具备本方案所需全部原语**。
- Rust：`x25519-dalek`/`chacha20poly1305`/`hkdf`/`sha2`/`getrandom`/`rand`（已用）；`jsonwebtoken`（已用，支持 EdDSA）；**需新增 `ed25519-dalek`**。

---

## 2. 方案 #1 — TOTP 信道绑定（SAS / PAKE-lite，挡中继 MITM，优先不改服务器协议）

### 2.1 设计
把现有「明文发 6 位 TOTP 码给 host 校验」升级为**信道绑定 MAC**：6 位码同时承担「证明知码」与「锚定本次密钥协商」。中继即便 MITM 了 X25519，也因**不知 TOTP 码**而无法算出正确 MAC → 被 host 拒绝业务帧。

**Transcript（绑定材料，双方各自本地计算，不上线）**：
```
transcript = "ridge-e2ee-bind-v1"
           || sorted(host_eph_pub32, controller_eph_pub32)   // 排序后拼接，64B
           || host_dtls_fp || controller_dtls_fp             // 见 2.4，DTLS 指纹（可选增强）
```
**绑定标签**：
```
K   = HKDF-SHA256(ikm = ascii(totp_code_6), salt = transcript, info = "ridge-bind", L = 32)
tag = HMAC-SHA256(K, transcript)            // controller 计算并上送；host 重算比对（恒定时间）
```
- controller 端用户**输入当前 6 位码**（沿用现有 UX，无新增交互）；host 端用本机种子在 **±1 时间步**各算一遍 tag 比对（容忍漂移，复用 `totp.rs` 窗口逻辑）。
- `totp_code` 仅作 KDF 输入，**绝不**明文上线（与现状"明文发码"相比，连码本身都不再过中继）。

**为什么挡 MITM**：MITM 中继与两端各跑一次 X25519，两侧 transcript 中的 `host/controller_eph_pub` 不同（中继塞了自己的公钥）；攻击者不知 6 位码无法为"真 host 期望的 transcript"生成正确 tag。6 位熵低，但**在线**爆破受 §6 门控（单桥 5 次 + LAN throttle）限制；攻击者每次猜测都要重建会话，不可离线。

### 2.2 库与原语
- **TS**：`@noble/hashes/hkdf` + `@noble/hashes/sha2`（sha256）+ `@noble/hashes/hmac`（均已依赖）。
- **Rust**：`hkdf` + `sha2` + 复用 `totp.rs` 既有手写 `hmac_sha256`（`packages/ridge-core/src/totp.rs:154-169`）。
- **恒定时间比较**：TS `keyBinding.ts:29-36 constantTimeEqual`；Rust `totp.rs:172-181 constant_time_eq`（均已存在，直接复用）。

### 2.3 密钥生命周期与存储
- 无新增持久密钥。`totp_code` 每 30s 轮换（既有）；`K`/`tag` 为单次会话临时值，握手完成即弃。
- TOTP 种子仍由 `seed_store.rs`（DPAPI/0600）持久化，**不出本机、不入本方案的线上数据**。

### 2.4 DTLS 指纹绑定（可选增强，二期）
将 WebRTC 本地/远端 SDP 的 DTLS fingerprint 纳入 transcript，可额外锚定到 DTLS 层，进一步收窄中继换指纹的空间。
- 取值：`RTCPeerConnection.localDescription.sdp` 解析 `a=fingerprint:sha-256 ...` 行（TS 端可得）。
- 标注：一期可省略（X25519 公钥绑定已足够挡 MITM）；二期纳入以做"密钥+传输"双锚。

### 2.5 UX 流程（无新增交互，复用现有"输码"）
1. controller 连上 → DataChannel open → E2EE 握手（X25519）完成。
2. controller 提示用户输入 host 屏幕上展示的 6 位码（**现状已有此步**）。
3. controller 本地算 `tag` → 经 CONTROL 通道（`0x12`）发 `{t:'totp-bind', tag}`（替代明文 `{t:'totp-verify',code}`）。
4. host 用本机种子（±1 窗口）重算 `tag` 比对：通过 → `verified=true` 放行业务帧；否则计失败次数（§6 门控）。
5. UI 不变；仅"码不再明文上送"，对用户透明。

### 2.6 向后兼容与迁移
- D9 `$/hello` 能力协商新增 `totp-bind`（见 §7）。双方都宣告 → 走 `totp-bind`；任一缺失 → 回退现状 `totp-verify`（明文码）但 UI 标注"二次验证未绑定信道（建议升级双端）"。
- 老 controller / 老 host 不回归；新双端自动获得信道绑定。

### 2.7 改动文件清单（实现期）
- `src/lib/remote/cloud/e2ee.ts`：新增 `computeBindTranscript(hostPub,ctrlPub[,fps])` 与 `computeBindTag(code,transcript)`（紧邻 `deriveSessionKey` :104-122）。
- `src/lib/remote/cloud/cloudControllerBoot.ts:140-166`：`totp-verify` → 计算并发 `{t:'totp-bind',tag}`。
- `src/lib/remote/cloud/cloudHostBridge.ts:286-324`：`handleSessionControl` 支持 `totp-bind`，调注入的 `totpBindVerifier(transcript,tag)`（替代 `totpVerifier(code)`）。
- `src/lib/remote/RemotePanel.svelte:293`：注入 `totpBindVerifier: (transcript,tag)=>invoke('verify_remote_totp_bind',{transcript,tag})`。
- `packages/ridge-core/src/totp.rs`：新增 `bind_tag(&self, transcript, time)`、`verify_bind_tag(&self, transcript, tag)`（±1 窗口）。
- `src-tauri/src/commands/remote.rs`：新增命令 `verify_remote_totp_bind(transcript,tag)`（旁挂现有 `verify_remote_totp`）。
- `packages/ridge-cli/src/e2ee.rs` + `packages/ridge-cli/src/session.rs`：Rust host 侧在握手后用 `RemoteTotp::verify_bind_tag` 校验 CONTROL 通道的 `totp-bind`。
- transcript 需要双方临时公钥：host 侧在 `into_session`/`deriveSessionKey` 时已同时持有两公钥，传出供绑定计算。

---

## 3. 方案 #2 — Ed25519 长期设备身份签名 + Controller TOFU 指纹固定

### 3.1 设计
为每个 host 设备引入**长期 Ed25519 身份密钥对**（首次启动生成、持久化）。E2EE 握手时，host 用身份私钥对「本次临时 X25519 公钥 + 上下文」签名；controller **TOFU 固定**该身份公钥指纹（首见即记，后续比对，类 SSH known_hosts）。中继无身份私钥 → 无法替换临时公钥 → MITM 失败。这是**无需用户输码**的自动信任锚（覆盖无人值守/自动重连场景，与 #1 互补）。

**签名握手帧（新增 tag `0x02`，与 `0x01` 并存向后兼容）**：
```
0x02 || eph_pub(32) || id_pub(32) || sig(64)
sig = Ed25519_sign(id_priv, "ridge-id-bind-v1" || eph_pub || room_context)
room_context = utf8(device_name) || utf8(username)     // 绑定到具体设备/账户身份
```
controller 校验：
1. 帧 tag=`0x02` → 取 `eph_pub/id_pub/sig`；`Ed25519_verify(id_pub, msg, sig)` 必须通过；
2. `id_pub` 指纹（`SHA-256(id_pub)` 前 8~16 字节，base32 分组展示）查 TOFU 库：
   - 首见 → 走 §3.4 首次信任 UX（展示指纹 + 可选 SAS 核对）→ 记入 pin；
   - 已固定且一致 → 放行；不一致 → **拒绝（疑似 MITM 或换机）**，提示用户。
3. 与 §2 的 transcript 绑定串联：`eph_pub` 同时被签名（#2）与 MAC（#1）覆盖。

### 3.2 库与原语
- **Rust**：新增 `ed25519-dalek = "2"`（签名/验签/密钥生成）。host 私钥操作只在 Rust 侧。
- **TS（controller 验签）**：`@noble/curves/ed25519`（`ed25519.verify`）——已依赖。
- 指纹：`@noble/hashes/sha2` / Rust `sha2`（已用）。

### 3.3 密钥生命周期与存储（关键安全约束）
- **桌面 host（WebView/TS）**：身份私钥**绝不进 JS / 绝不入 localStorage**。生成与签名全部在 **Rust(src-tauri)** 侧，DPAPI 加密落盘（复用 `seed_store.rs` 机制，新增 `device_identity` 文件）。JS 仅通过 invoke 调 `sign_ephemeral_pubkey(eph_pub,context)→sig` 与 `get_device_identity_pub()→id_pub`。
- **ridge-cli host（Rust）**：私钥存 `~/.config/ridge/device_identity.key`，DPAPI（Windows）/`0600`（Unix）；与 `auth.json` 同根。**顺带修复 P2**：`auth.json` 当前 Windows 仅 ACL，建议一并改 DPAPI。
- **controller（浏览器）**：只存**对端 host 的身份公钥 pin**（非秘密），localStorage `ridge.cloud.trust.<device-username>` 即可（公钥泄露无害）。
- 生命周期：身份密钥跨重启稳定；提供 `reset_device_identity`（换机/疑似泄露时重生成，触发所有 controller 重新 TOFU）。

### 3.4 首次信任（TOFU）UX 流程
首次连接某 host（无 pin）时：
1. controller 展示 host 身份指纹（如 `RID-3F2A-9C7B-...`，base32 分组）；
2. host 屏幕/TUI 同步展示**同一指纹** + 一个 **6 位 SAS**（由 §2 transcript 派生：`SAS = HKDF(transcript,"sas")` 取 6 位十进制）；
3. 用户带外（肉眼/口头）核对指纹或 SAS 一致 → 点"信任此设备" → 写入 pin；
4. 之后该设备自动放行（除非指纹变更）。
> SAS 与 #1 共享 transcript，使"首次 TOFU 接受"也受带外短串保护，缓解 TOFU 首次信任固有弱点。

### 3.5 向后兼容与迁移
- 握手 tag `0x01`（无签名）与 `0x02`（带签名）并存：收到 `0x01` 视为旧端 → 走 relay-trust（标注未绑定）；`0x02` 走强校验。
- D9 能力位 `device-id`（见 §7）双方宣告才强制 `0x02` + fail-closed。
- 迁移顺序：先发版让 host 生成并广播身份公钥（controller 端"静默学习"pin，不强制）→ 一个灰度周期后开启 fail-closed。

### 3.6 改动文件清单（实现期）
- 新增 `packages/ridge-core/src/device_identity.rs`：`DeviceIdentity{load_or_create(), public(), sign(msg)}`，落盘复用 `seed_store` 的 DPAPI/0600。
- `packages/ridge-core/src/seed_store.rs`：抽出通用 `protect/unprotect` 供 `device_identity` 复用（或并列新增 `load_key/save_key`）。
- `packages/ridge-core/src/lib.rs`：导出 `DeviceIdentity`。
- `packages/ridge-core/Cargo.toml`：加 `ed25519-dalek`。
- `src-tauri/src/commands/remote.rs`：新增 `get_device_identity_pub`、`sign_ephemeral_pubkey`、`reset_device_identity`（注册到 `src-tauri/src/lib.rs` invoke handler）。
- TS E2EE 帧：`src/lib/remote/cloud/e2ee.ts`：新增 `encodeSignedHandshakeFrame`/`decodeSignedHandshakeFrame`（tag `0x02`）+ `verifyIdentitySignature`。
- 新增 `src/lib/remote/cloud/deviceTrust.ts`：TOFU pin 读写（localStorage）+ 指纹/SAS 格式化。
- host provider：`src/lib/remote/cloud/ridgeCloudProvider.ts:301-317`（`startE2eeHandshake` 发 `0x02`，经 invoke 取签名）；controller：`src/lib/remote/cloud/controllerCloudProvider.ts:202-235`（验签 + 查 pin）。
- ridge-cli host：`packages/ridge-cli/src/e2ee.rs:74-101`（`encode_signed_frame`/`parse_signed_peer_frame`）+ `session.rs`（握手接 `DeviceIdentity`）。
- UI：`src/lib/remote/RemotePanel.svelte` + controller boot 增加 TOFU 确认弹窗（首次）。

---

## 4. 方案 #3 — fail-open 全部改 fail-closed

### 4.1 设计
当双方经 D9 协商出绑定能力（`device-id` 或 `totp-bind`）时，绑定校验**必须**完成且通过才放行；旁路/签名缺失或超时一律**拒绝**，不再回落 `relay-trust`。仅对**确未宣告**能力的旧端保留 relay-trust（并 UI 明示）。

### 4.2 具体改动
- `src/lib/remote/cloud/keyBinding.ts:100-113`（`decideKeyBinding`）：新增"是否强制绑定"入参；强制时 `graceExpired` 分支返回 **`reject`**（而非 `accept`）；缺 `signalingPub`/签名 → `reject`。
- host：`src/lib/remote/cloud/ridgeCloudProvider.ts:367-396`（`decideBinding`/`armBindGrace`）：gating 依据从"旁路是否到达"改为"**能力是否协商**"——协商了即 fail-closed。
- controller：`src/lib/remote/cloud/controllerCloudProvider.ts:264-294`：同上。
- `src/lib/remote/cloud/keyBinding.ts:62-70`（`makeKeyBindingVerifier`）：`enabled` 来源改为 D9 能力交集（已有 `enabled` 语义，接通即可）。

### 4.3 向后兼容
- 旧端（未宣告能力）：`enabled=false` → 仍放行，UI 标注"未端到端校验"。这是有意保留的兼容窗口；可配置一个"严格模式"开关，强制拒绝所有未绑定连接（高安全部署用）。

---

## 5. 方案 #4 — JWT：HS256 对称 → EdDSA 非对称（服务器只持公钥验签）

### 5.1 设计
签名私钥与验签公钥分离：**签发服务持私钥（理想置于 KMS/HSM 或独立签发组件），验签侧（WS/HTTP 中间件）只持公钥**。只读磁盘/库/内存泄露**公钥**无法伪造 token。改 EdDSA(Ed25519)，`jsonwebtoken` 原生支持。

> 注意边界：**全主机攻陷**仍可能拿到私钥（若私钥与服务同机）→ 仍可伪造 token。故 #4 是**纵深收窄**（堵只读泄露），真正让"token≠解密/注入能力"的是 #1/#2——token 仅授权"进房间"，进房后仍被设备签名 + TOTP 绑定挡在业务帧外。理想部署应把签名私钥隔离到独立签发微服务/KMS，使 relay/WS 节点被攻陷也拿不到签名私钥。

### 5.2 库与密钥
- `jsonwebtoken`（已用）：`Algorithm::EdDSA`，`EncodingKey::from_ed_pem(priv_pem)` / `DecodingKey::from_ed_pem(pub_pem)`。
- 密钥生成：`ed25519-dalek` 或 `openssl genpkey -algorithm ed25519`，PKCS8 PEM。
- 配置（`ridge-cloud/src/config.rs`）：新增 `JWT_ED25519_PRIVATE_PEM`（仅签发侧需要）、`JWT_ED25519_PUBLIC_PEM`（验签侧）；env/secret 注入，**不入源码**。

### 5.3 改动文件清单
- `ridge-cloud/src/auth/jwt.rs:81-159`：`JwtCodec` 改持 Ed25519 `EncodingKey`/`DecodingKey`；`issue()` 用 `Header::new(Algorithm::EdDSA)`；`Validation::new(Algorithm::EdDSA)`。`issue_user`/`issue_device`/`verify` **接口不变**。
- `ridge-cloud/src/config.rs:79-94`：读 EdDSA PEM；过渡期保留 `JWT_SECRET` 供旧 token 验签。
- `ridge-cloud/src/state.rs:33`：`JwtCodec::new` 改签名（传 PEM）。
- 签发点 `auth_routes.rs:78,143,222,319,518` / `device_routes.rs:84,273`：**无需改**（只调 `issue_*`）。
- 验签点 `extract.rs:37` / `ws/handler.rs:60`：**无需改**（只调 `verify`）。

### 5.4 向后兼容与迁移（双算法并行期）
1. **阶段 A**：`JwtCodec::verify` 先试 EdDSA，失败再试 HS256（旧 token）；签发仍可暂发 HS256。
2. **阶段 B**：签发切 EdDSA；验签双接受。
3. **阶段 C**：旧 token 自然过期（user 30d / device 180d）后，移除 HS256 验签分支与 `JWT_SECRET`。
- device token 180 天偏长——建议同期缩短并加**服务端吊销表**（key_repo 旁），支持撤销被盗 token（独立小改进）。

---

## 6. 方案 #5 — cloud 会话 TOTP 门控默认开启（去掉"注入才生效"的脆弱默认关）

### 6.1 设计
`CloudHostBridge` 改为**默认要求二次验证**：未配置校验器即**拒绝**所有业务帧（fail-closed），而非现状 `verified = !totpVerifier` 的"未注入即放行"。

### 6.2 改动
- `src/lib/remote/cloud/cloudHostBridge.ts:178-199`：
  - 构造改为显式 `requireTotp: boolean`（默认 `true`）+ `totpBindVerifier`；`this.verified = false` 初始（除非显式 `requireTotp:false` 用于受控测试）。
  - 缺校验器且 `requireTotp` → 业务帧一律走 `rejectUnverified`（:330-345），不放行。
- `src/lib/remote/cloud/cloudHostBridge.ts:286-324`：`handleSessionControl` 接 `totp-bind`（#1）；保留 `MAX_TOTP_ATTEMPTS=5`（:73）爆破上限。
- `src/lib/remote/RemotePanel.svelte:283-294`：已注入校验器，保持；移除任何"未注入即放行"的隐式路径。
- `src/lib/remote/cloud/__cloudE2eHarness.ts` 与 `cloudHostBridge.test.ts`：更新——测试需显式 `requireTotp:false` 才走无门控路径（防"默认关"复活）。

### 6.3 兼容
- 仅影响"未注入校验器"的接入路径（生产桌面已注入）。harness/测试显式声明即可。无线协议变更。

---

## 7. 信令字段：语义与校验规则（载体 schema 交 align-reviewer）

> **分工**：align-reviewer 定义字段的**线上载体**（JSON schema / 字段名 / 编码，单一事实来源）；本节定义**语义与校验规则**。请 align-reviewer 据此收口字段，本人回执确认。

> **架构决策（2026-06-11，与 align-reviewer 对齐）**：本方案的两个安全绑定机制——设备身份签名（#2）与 TOTP 信道绑定（#1）——**全部承载于 DataChannel 内**（E2EE 之后、P2P DTLS，**relay 不可见、不可篡改、不可丢弃**），**不**放在经 relay 转发的信令 WS JSON 上。推论：
> - 信令 `e2ee-pubkey` 旁路在新方案下**不再承载安全语义**，标记 **deprecated/可选**（仅留灰度窗口兼容旧端的 B3 交叉校验；新双端不依赖它）。安全锚由"双通道交叉"改为"**签名自证 + TOFU**"。
> - align-reviewer 的**信令 schema 无需为加密绑定新增承载字段**（签名/MAC 都在 DataChannel 二进制帧内）。需统一登记的仅是**协议常量**（握手帧 tag `0x01`/`0x02`、mux tag `0x10`/`0x11`/`0x12`、CONTROL 子类型 `totp-verify`/`totp-bind`/`totp-result`），建议由 `ridge-signaling` crate 设常量模块承载，避免四端漂移/冲突。
> - relay 保持真正"零解析盲中继"，**不旁带任何公钥/签名分发字段**。
> - **为何不把签名放信令旁路**：信令旁路经 relay 转发，relay 可丢弃以强制降级（即便 fail-closed 拒绝，也等于把"绑定可达性"交给 relay 控制）；放 DataChannel 则 relay 连碰都碰不到（P2P DTLS）。对"服务器完全攻陷"模型，这是实质增强。

| 字段 | 出现于 | 语义 | 校验规则（安全相关） |
|---|---|---|---|
| `cid` | offer/answer/ice/peer-join/kick | relay 分配的房间内 controller 唯一寻址 id；**relay 注入、客户端只读** | host 按 cid 定向路由；客户端不得据 cid 做任何信任决策（仅寻址）。已随机不可枚举（`ws/rooms.rs:50`）。 |
| `e2ee-pubkey`（**deprecated**） | 双向旁路（信令 WS） | 旧 B3 的临时公钥带外交叉确认 | 新方案改由 DataChannel `0x02` 签名帧**自证**，**不再需要**此旁路；保留仅为灰度期兼容旧端。新端协商 `device-id` 能力后忽略之。 |
| 设备签名（DataChannel `0x02` 帧，**非信令层**） | host→controller 握手 | host 设备身份对临时公钥的 Ed25519 签名 | controller 验签（帧内 `id_pub`）+ `id_pub` 命中 TOFU pin；payload=`"ridge-id-bind-v1"‖host_eph_pub‖controller_eph_pub‖device_name‖username`（`eph_pub` 充当防重放 nonce，**不含 cid**）。relay 零解析透传。 |
| `capabilities`（D9 `$/hello`） | 握手后协商 | 能力集；新增 `device-id`、`totp-bind` | 双方都宣告 → 启用 fail-closed（#3）+ 强制 `0x02` 签名帧（#2）/ `totp-bind`（#1）。任一缺失 → 兼容回退并 UI 标注。 |
| 握手帧 tag | DataChannel 首帧 | `0x01`=旧裸公钥；`0x02`=`eph_pub(32)||id_pub(32)||sig(64)` | `0x02` 必须验签通过 + `id_pub` 命中 TOFU pin；首见走 TOFU UX。 |
| `totp-bind` | CONTROL 通道 `0x12` | controller→host 的信道绑定 MAC（替代明文 `totp-verify`） | host 用本机种子 ±1 窗口重算 `tag` 恒定时间比对；失败计入 `MAX_TOTP_ATTEMPTS`。 |

**错误码**：复用契约已有 `SIGNATURE_INVALID`（协议 §77 行）表达设备签名/绑定校验失败；TOTP 绑定失败沿用 `{t:'totp-result',ok:false[,locked]}`。

---

## 7.1 载体定稿（2026-06-11 与 align-reviewer 对齐；schema 已落地 `C:\code\ridge-signaling`）

三层归属（"信令 schema" 实跨 3 层，各有 SSOT）：
- **A 层 信令 JSON**（relay WS，relay 读 `t`/`cid` 路由）→ `ridge-signaling` crate。**零密码学材料**：
  - `cid: Option<Cid>`（仅寻址，客户端不得据它做信任决策）。
  - `SignalMsg::E2eePubkey { pubkey: base64(eph_pub32), cid? }` —— **不含** sig/alg/ts。设备签名走 B 层；A 层只承载 eph_pub 旁路（旧端 relay-trust 交叉校验用，强绑定落地后降级）。
  - **`id_pub` 绝不在 A 层旁带** —— 否则 relay 经手公钥、可误导 TOFU；TOFU 只锚 B 层 0x02 帧内经 sig 自证的 `id_pub`。
- **B 层 E2EE 握手首帧**（DataChannel，加密前，relay 不经手）→ `e2ee.ts`/`e2ee.rs`（本文档 territory）：
  - `0x01`（旧）= `tag‖eph_pub32` = 33B；`0x02`（新）= `tag‖eph_pub32‖id_pub32‖sig64` = 129B。
  - `sig = Ed25519(device_id_priv, "ridge-id-bind-v1" ‖ context)`，`context = host_eph_pub ‖ controller_eph_pub ‖ len(device_name)‖device_name ‖ len(username)‖username`（**变长字段加 1B 长度前缀**避免拼接歧义）。
  - tag 数值由 `ridge-signaling::tags::handshake{LEGACY_PUBKEY=0x01, DEVICE_BOUND=0x02}` 登记防冲突；字节布局/验签由本文档定，conformance 锁字节。
- **C 层 E2EE 内明文**（DataChannel，加密后）：
  - SessionControl(0x12)：`TotpBind { tag: base64(HMAC) }`，**保留** `TotpVerify { code }` 回退。CONTROL 子类型串引用 `ridge-signaling::control{TOTP_VERIFY/TOTP_BIND/TOTP_RESULT}`。
  - `$/hello` 能力位 `device-id` / `totp-bind` → **`ridge-core::remote_protocol`**（+ TS 镜像）。双方交集都含才启用强校验，否则回退 0x01/totp-verify（fail-closed-with-fallback，旧端零感知、无 flag day）。
- **失败信号**：验签/绑定失败在 host 端（E2EE 之后，**不经 relay**）→ 0x11 业务通道发 `$/bye { reason: "signature-invalid" }`（D9 语义，与现有 keyBinding reject→teardown 一致）。`SIGNATURE_INVALID` 仅在 `ridge-signaling::error_code` 做语义登记，relay 不主动发。

依赖接线（P2）：`ridge-core`/`ridge-cli`/`src-tauri`/`controller(TS)` 引用 `ridge-signaling` 的 tag/串常量；relay(`ridge-cloud`) 用其 A 层 + tag 注册表。不再各自分叉常量。

## 8. 实施顺序与依赖

```
阶段 1（信任锚地基，可并行）
  ├─ #4 JWT EdDSA（纯服务端，独立）        ── ridge-cloud
  └─ #2 设备身份密钥生成+持久化(Rust/DPAPI) ── ridge-core / src-tauri / ridge-cli
阶段 2（绑定校验，依赖阶段 1 的密钥/能力位）
  ├─ #2 签名握手帧 0x02 + controller TOFU
  ├─ #1 TOTP 信道绑定 MAC（依赖 transcript = 双方 eph_pub）
  └─ D9 能力位 device-id / totp-bind（与 align-reviewer 收口字段）
阶段 3（关阀，依赖阶段 2 全部就绪 + 灰度学习期）
  ├─ #3 fail-open → fail-closed（按能力位）
  └─ #5 cloud TOTP 默认门控开启
阶段 4（清理）
  └─ #4 阶段 C：移除 HS256 验签 + JWT_SECRET
```
> 关键约束：**#3/#5 的 fail-closed 必须晚于 #1/#2 双端发版 + 一个灰度学习周期**，否则旧端被误拒。

---

## 9. 修复后威胁模型复核（服务器被完全攻破）

逐条论证 (a)(b)(c)：

### (a) 无法解密任何进行中/历史会话 ✓
- **进行中**：会话密钥 = `HKDF(X25519(eph))`。relay 要解密必须 MITM 这次 ECDH（替换 `eph_pub`）。但 `eph_pub` 同时被：(#2) host 设备 Ed25519 私钥**签名**（relay 无此私钥，存 Rust/DPAPI，**不在服务器**）；(#1) TOTP 6 位码绑定 **MAC**（relay 不知码，种子**不在服务器**）。任一校验都会因 relay 替换公钥而失败 → controller 拒绝 → 无共享密钥 → ChaCha20 密文不可解。(#3) fail-closed 杜绝"丢弃旁路帧强制回落"的降级逃逸。
- **历史**：纯**临时** X25519，用后即焚，无长期 E2EE 解密密钥可供事后从磁盘/内存窃取（前向保密）。relay 历史只存密文。
- 结论：达标。残余仅"全主机攻陷 + 社工诱导用户在 TOFU 首次接受攻击者指纹"，由 #3.4 的带外 SAS 核对缓解。

### (b) 无法冒充控制方/被控方、无法注入/接管 ✓
- **冒充 host**：需设备 Ed25519 私钥（不在服务器、不在 localStorage、DPAPI 保护）**且**命中 controller 已固定的 TOFU pin。两者 relay 都无法满足。
- **冒充 controller / 注入指令**：需当前 6 位 TOTP 码（host 本机种子派生，**不在服务器**）算出正确 `totp-bind` MAC；(#5) host 默认门控、(#6) 5 次爆破上限 + LAN throttle 限制在线猜测；MITM 路径已被 (a) 堵死，无法骑乘合法会话。
- **token 伪造**：(#4) 只读泄露拿公钥无法伪造；全主机攻陷虽可拿签名私钥伪造 token，但 token 仅授权"进房间"，进房后业务帧仍被 #1/#2 拦截 → **token ≠ 解密/注入能力**。
- 结论：达标。

### (c) 服务器只做盲中继、不持可解密会话的长期密钥或明文 ✓
- relay 仍只转发密文 SDP/帧（`ws/handler.rs` 不解密业务负载）；它持有的 EdDSA 签名私钥（若同机）只能签"进房授权"token，**不能**解密会话、**不能**伪造设备身份签名（无设备私钥）、**不能**算 TOTP MAC（无种子/码）。
- 唯一能做的"主动"动作——把伪造 token 的自己塞进房间当 controller——会被 host 的 TOTP 门控(#5)+信道绑定(#1) 挡在业务帧之外，且无法解密既有会话。
- 结论：达标（达到"盲中继"语义）。

### 残余风险（明确不在本轮消除）
1. TOFU 首次信任 + 社工：用带外 SAS 核对缓解，无法 100% 消除（信任引导固有）。
2. TOTP 6 位熵：依赖在线限次；**终态**建议演进到 **CPace/SPAKE2（口令=TOTP 码）** 直接产出认证密钥，消除"输码明文校验"与低熵在线猜测窗口（库：Rust 候选 `cpace`/手写 `curve25519-dalek`；TS 用 `@noble/curves` ristretto255 手写——成本较高，列为二期）。
3. 签名私钥与 relay 同机时的全主机攻陷：建议部署层把 JWT 签发私钥隔离到独立组件/KMS（运维改进，非代码）。

---

## 10. 自检（spec 覆盖）
- [x] #1 SAS/PAKE 信道绑定 → §2（含库/生命周期/UX/兼容/文件清单）
- [x] #2 Ed25519 设备身份 + TOFU → §3
- [x] #3 fail-open → fail-closed → §4
- [x] #4 JWT HS256 → EdDSA → §5
- [x] #5 cloud TOTP 默认门控 → §6
- [x] 每项含 算法/库（Rust+TS 分列）/密钥生命周期与存储/TOFU·SAS UX/向后兼容迁移/改动文件清单（带行号）
- [x] 末尾 (a)(b)(c) 逐条威胁模型复核 → §9
- [x] 信令字段语义与校验规则（与 align-reviewer 边界）→ §7
- [x] 实施顺序与依赖（fail-closed 晚于灰度）→ §8
