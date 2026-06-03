# Ridge Cloud 商业化协议契约 v1（单一事实来源 / SSOT）

> 本文件是 Ridge 公网加速（Pro）相关 5 个组件的**唯一权威契约**。
> 后端（ridge-cloud）、桌面端 RemotePanel、Deep Root Mode、ridge-cli、Web 管理面板
> 全部按本文件实现。**任何跨组件的字段名、URL、消息形状、加密参数以本文件为准；
> 不得各自发明。** 如需变更契约，先改本文件再改代码。
>
> 语言约定：散文用简体中文，标识符/代码/JSON 字段名用英文（与现有代码库一致）。

---

## 0. 名词与拓扑

- **host（被控端）**：被远程控制的机器。两种形态：
  - 桌面端 Ridge（Tauri，本仓库 `C:\code\wind`）
  - 无头服务器 `ridge-cli`（本仓库 `packages/ridge-cli`）
- **controller（控制端）**：发起控制的浏览器（手机/另一台电脑）。两种 SPA 形态：
  - **移动 controller**：手机浏览器复用现有移动端 SPA（轻量、单 pane 视图）。
  - **桌面 controller**：桌面浏览器加载**完整桌面 SPA**（`web-remote-dist`，完整 IDE 面板）。
    LAN 下由 host 的 LAN server 按 User-Agent 分流下发；cloud 下该桌面 SPA 经**公网源**下发
    （ridge-cloud 后端在主域名兜底返回静态资源，参 §10；或 CDN），再经 WebRTC E2EE 连 host。
    > 评审 2026-06-03：原契约仅写"复用现有移动端 SPA"，未覆盖 cloud 下"桌面浏览器完整控制"形态。
    > 现接纳**桌面 controller**：其 SPA 经公网下发（区别于 LAN 同机直发），与 host 经 D9 版本/能力握手对账。
    > 取页工程（code-split / 内容指纹版本化缓存）与 §10 静态托管的衔接细节，待跨团队确认。
- **signaling relay（信令复读机）**：ridge-cloud 后端的 `/ws`，**纯转发 SDP/ICE，绝不解密任何数据**。
- **账户模型（重要简化）**：host 和 controller **必须是同一个账户**。
  即：只有账户拥有者本人能控制自己名下的设备。不支持跨账户分享（v1 范围外）。
  - host 用 **device JWT**（`scope=device`）连接自己的租户 WS。
  - controller 用 **user JWT**（`scope=user`）连接同一租户 WS。
  - 后端校验两者 `username` 一致、且该 user 为 premium、且拥有该 device。

```
浏览器(controller, user JWT) ─┐                          ┌─ 桌面端/ridge-cli(host, device JWT)
                              ├─ wss://{device}-{user}.remo2ridge.duckdns.org/ws ─┤
                              └──── 信令(SDP/ICE)转发 ────┘
        └──────────── WebRTC DataChannel (E2EE: X25519 + ChaCha20-Poly1305) ───────────┘
                       （relay 看不到明文；TURN 也看不到）
```

---

## 1. 域名与身份（解决你原方案里的歧义）

- Base zone：`remo2ridge.duckdns.org`
- 每设备公网入口：`https://{device}-{username}.remo2ridge.duckdns.org`
- WS 端点：`wss://{device}-{username}.remo2ridge.duckdns.org/ws`

### 1.1 命名规则（**契约级，覆盖原 prompt 中“username 可含连字符”的说法**）

> 原方案 `^([a-z0-9-]+)-([a-z0-9-]+)\.…$` 有**歧义**：device 和 username 都允许连字符时
> `a-b-c-d` 无法唯一切分。本契约的解法：**username 禁止连字符**，按 host label 的
> **最后一个连字符**切分，从而保证唯一可解析。

- `username`：正则 `^[a-z0-9]{3,20}$`（小写字母+数字，**不含连字符**，长度 3–20）
- `device_name`：正则 `^[a-z0-9]([a-z0-9-]*[a-z0-9])?$`，长度 3–30，
  额外约束：**不得包含 `--`（双连字符，规避 punycode `xn--`）**，不得首尾连字符。
- DNS label 长度：`device(≤30) + '-' + username(≤20) ≤ 63` ✓

### 1.2 解析算法（后端中间件 / 客户端拼接都按此）

给定 Host 首段 label `L`：
1. 若 `L` ∈ 保留字 `{www, api, ws, app, admin, static, cdn, mail}` → **不做租户解析**（按系统路由）。
2. 否则按 `L` 中**最后一个 `-`** 切分：`device = L[..lastDash]`，`username = L[lastDash+1..]`。
3. 分别用 §1.1 正则校验 `device`/`username`；任一不过 → 404（信息最少化，不回显内部细节）。
4. 解析成功后，把 `{device, username}` 作为 Request Extension 注入后续 handler。

---

## 2. 统一响应信封与错误码

成功：`{ "ok": true, "data": <T> }`
失败：`{ "ok": false, "error": { "code": "<CODE>", "message": "<人类可读>" } }`

错误码枚举（前后端共用字符串常量）：
`UNAUTHORIZED, FORBIDDEN, NOT_FOUND, INVALID_INPUT, INVALID_KEY, KEY_ALREADY_USED,
USERNAME_TAKEN, USERNAME_REQUIRED, NOT_PREMIUM, PAIRING_EXPIRED, PAIRING_NOT_FOUND,
DEVICE_NAME_TAKEN, SIGNATURE_INVALID, RATE_LIMITED, INTERNAL`

> 客户端 UI 文案不直接拼接后端 message；按 `code` 映射本地化中文文案。
> 服务端绝不在 message 中泄露内部路径/SQL/堆栈（见 rust/security.md）。

---

## 3. JWT（HS256，密钥来自环境变量 `JWT_SECRET`）

公共 claims：`iss="ridge-cloud"`, `iat`, `exp`, `sub`（user uuid 字符串）, `username`（可空，未设则 `null`）, `plan`（`"free"|"premium"`）。

两种 scope：
- **user token**：`scope="user"`，`exp` = 签发起 30 天。controller 浏览器 / Web 面板使用。
- **device token**：`scope="device"`，附加 `device`（device_name），`exp` = 签发起 180 天。
  ridge-cli / 桌面 host 使用，持久化到本地。

Header 传递：`Authorization: Bearer <jwt>`。
**WS 例外**：浏览器无法给 WebSocket 设自定义 header → WS 一律用 query 参数 `?token=<jwt>&role=<host|controller>`。

---

## 4. HTTP API（全部挂在主域名 `https://remo2ridge.duckdns.org/api/v1`）

> 路径前缀 `/api/v1`。除标注 `(Bearer)` 的需带 user/device token 外，其余公开。
> 所有请求/响应体走 §2 信封。

### 4.1 账户
- `POST /auth/register` `{email, password}` → `{token, user}`（user token）
- `POST /auth/login` `{email, password}` → `{token, user}`
- `GET  /me` (Bearer user) → `{user}`
- `POST /auth/set-username` (Bearer user) `{username}`
  - 校验 §1.1 正则 + 全局唯一；非 premium 返回 `NOT_PREMIUM`；占用返回 `USERNAME_TAKEN`。

`user` 对象形状（前后端共用）：
```json
{ "id":"uuid", "email":"a@b.com", "username":"alice"|null,
  "plan":"free"|"premium", "devices":[{"name":"my-laptop","createdAt":1690000000}] }
```

### 4.2 国内卡密激活（面包多）
- `POST /auth/activate-key` (Bearer user) `{key, username?}` →
  - 查 `license_keys` 中 `key` 是否存在且 `status='pending'`；否则 `INVALID_KEY`/`KEY_ALREADY_USED`。
  - 若用户尚无 username 且传了 `username` → 一并按 §1.1 设定（唯一性校验）。
  - 置 `user.plan='premium'`，`key.status='used'`（记录 used_by/used_at）。
  - 返回新的 `{token, user}`。
- 卡密字符串格式：`RIDGE-XXXX-XXXX-XXXX`，字符集见 §6。

### 4.3 海外订阅（Lemon Squeezy webhook）
- `POST /webhook/payment`（**公开但需 HMAC 校验，不走信封，按 LS 约定返回 200**）
  - 读 header `X-Signature`（hex）。用 `hmac-sha256(LEMON_SQUEEZY_SECRET, <raw body bytes>)` 比对，
    **必须用原始 body 字节**（在 body 解析前取出）；不匹配返回 401 `SIGNATURE_INVALID`。
  - 只处理 `event_name == "subscription_created"`（webhook header `X-Event-Name` 或 body `meta.event_name`）。
  - 由 payload 的 `data.attributes.user_email` 匹配/创建 user，置 `plan='premium'`，
    记录 `ls_subscription_id`。username 由用户随后在 Web 面板 `set-username` 设定。

### 4.4 设备配对（Device Code Flow，给无头 ridge-cli 用）
- `POST /device/code`（公开）`{}` →
  `{ pairing_code:"XA4B-97RE", poll_token:"<opaque>", expires_in:600 }`
  - 后端建 `pairing_codes` 行：`status='pending'`，`expires_at=now+600s`。
- `POST /device/poll`（公开）`{poll_token}` →
  - `{status:"pending"}` | `{status:"expired"}` |
    `{status:"bound", token:"<device JWT>", device_name, username}`
  - CLI 长轮询：建议客户端每 2s 轮询；服务端可立即返回（非 hanging）。
- `POST /device/activate` (Bearer user) `{pairing_code, device_name}` →
  - 校验 premium（否则 `NOT_PREMIUM`）；user 必须已有 username（否则 `USERNAME_REQUIRED`）。
  - 校验 `device_name` §1.1 正则；该 user 下唯一（否则 `DEVICE_NAME_TAKEN`）。
  - 找到 `pairing_code` 且未过期未绑定（否则 `PAIRING_EXPIRED`/`PAIRING_NOT_FOUND`）。
  - 创建/复用 `devices` 行；置 `pairing_codes.status='bound'` 并写入 user_id/device_name + 生成 device JWT。
  - 返回 `{ public_entry: "https://{device_name}-{username}.remo2ridge.duckdns.org" }`。
- `GET    /devices` (Bearer user) → `{devices:[...]}`
- `DELETE /devices/:name` (Bearer user) → `{ok:true}`

### 4.5 健康检查
- `GET /api/v1/health` → `{ok:true, data:{version, uptimeSecs}}`

---

## 5. 信令 WebSocket（`GET /ws`，租户域名上）

升级条件（按顺序，任一失败立即关闭，HTTP 403 或 WS close code）：
1. 中间件已从 Host 解析出 `{device, username}`（§1.2）。
2. query `token` 解析为合法 JWT；`role` ∈ `{host, controller}`。
3. `token.username == username`（Host 里的）且 `token.plan == "premium"`。
4. `role=host` 要求 `token.scope=="device"` 且 `token.device==device`。
   `role=controller` 要求 `token.scope=="user"`。
5. 该 user 名下存在 `device`。

房间（room）：key = `"{device}-{username}"`，内存维护 `Arc<RwLock<HashMap<String, Room>>>`，
每房间最多 1 host + 1 controller。新 controller 顶替旧 controller（旧的收 `error{code:"REPLACED"}` 后关闭）。

### 5.1 信令消息（JSON 文本帧，tag 字段为 `t`）
服务端 → 客户端（连接事件）：
```json
{ "t":"welcome",   "room":"my-laptop-alice", "role":"host", "peerPresent": false }
{ "t":"peer-join", "role":"controller" }
{ "t":"peer-leave","role":"controller" }
{ "t":"error",     "code":"...", "message":"..." }
```
两端互发、服务端**逐字转发给房间内另一端**（relay 不解析内容）：
```json
{ "t":"offer",  "sdp":"<RTCSessionDescription.sdp>" }
{ "t":"answer", "sdp":"<RTCSessionDescription.sdp>" }
{ "t":"ice",    "candidate": <RTCIceCandidateInit | null> }
```
协商发起方约定：**controller 收到 `welcome`(peerPresent:true) 或 `peer-join` 后，由 controller 创建 offer**。
host 永远是 answerer。

### 5.2 ICE/TURN
- v1 仅用公共 STUN：`stun:stun.l.google.com:19302`（写在客户端常量）。
- TURN 暂不部署（成本）。契约预留 `GET /api/v1/ice-servers`(Bearer) → `{iceServers:[...]}`，
  v1 返回仅含上面的 STUN；后续可加 TURN 而不改客户端。**客户端必须调用该接口取 iceServers**，不要硬编码。

---

## 6. 配对码 / 卡密字符集

- 字符集（去除易混淆 0/O/1/I/L）：`ABCDEFGHJKMNPQRSTUVWXYZ23456789`
- 配对码：`XXXX-XXXX`（8 字符 + 中划线）。输入大小写不敏感，存储统一大写。TTL 600s，单次有效。
- 面包多卡密：`RIDGE-XXXX-XXXX-XXXX`。

---

## 7. WebRTC DataChannel + 端到端加密（E2EE）

> **relay/TURN 永远看不到明文。** 加密在 DataChannel 之上再叠一层（不依赖 DTLS）。

- DataChannel：`label="ridge"`，`ordered:true`，`maxRetransmits:null`（可靠有序）。
- 内层明文 = **统一线协议帧**，按 **1 字节通道前缀 mux**（沿用 ridge-cli `protocol.rs`）：
  - `0x10 = PANE_RAW`：PTY **裸字节**（paneId 前缀 + raw bytes），客户端 wasm 终端内核 `kernel.feed()` 自行解析。
  - `0x11 = JSON`：带外 JSON 文本（控制消息、事件、invoke 请求/响应），UTF-8。
  > 评审 2026-06-03：原契约写"内层明文 = postcard 二进制增量协议帧，保持 schema 不变"。但 LAN
  > (`RemotePtyEvent::RawBytes`) 与 ridge-cli (`protocol.rs` `0x10`) 实际已收敛到 **raw-byte**；
  > per-sub postcard delta 方案因每 sub ~11MB `PaneParser` 导致 OOM、丢帧致状态脱节而被弃用
  > （见 `.kiro/specs/remote-raw-byte/`）。故 pane payload 改为 **raw-byte（`0x10`）+ 带外 JSON（`0x11`）**。
  > raw-byte **不可重放**，故迟到订阅/重连须先下发屏幕快照（见 §7.4 D10）。（待跨团队确认）

### 7.0 invoke / 控制信封：JSON-RPC 2.0（`0x11` JSON 通道内）

> 评审 2026-06-03：invoke 信封从自定义 `type`/`_reqId` 改为 **JSON-RPC 2.0** 标准信封，取得现成的
> 错误/通知/取消语义、生态工具，减少自造与 bikeshedding。控制消息（订阅/事件/元数据）与 invoke
> 请求/响应**共用同一 `0x11` JSON 通道**，靠 JSON-RPC 的 `id`/`method` 区分。（待跨团队确认）

**统一信封约定（前后端、LAN-WS 与 cloud-WebRTC、桌面 host 与 ridge-cli 必须逐字一致）**：
- 请求：`{ "jsonrpc":"2.0", "id":<num|str>, "method":<str>, "params":<obj> }`
- 成功响应：`{ "jsonrpc":"2.0", "id":<同请求>, "result":<any> }`
- 错误响应：`{ "jsonrpc":"2.0", "id":<同请求>, "error":{ "code":<int>, "message":<str>, "data"?:<any> } }`
- 单向控制消息 / 事件下发：**notification**（无 `id`）：`{ "jsonrpc":"2.0", "method":<str>, "params":<obj> }`
- 取消长任务：method `"$/cancel"`，`params:{ "id":<目标请求的 id> }`（notification，无自身 `id`）。

说明：
- 业务错误码（§2 的 `UNAUTHORIZED`/`NOT_FOUND`/… 字符串枚举）映射进 JSON-RPC `error.data`，
  `error.code`（int）按 JSON-RPC 规范用于协议级错误；业务语义不丢失（§2 信封仍用于 §4 的 HTTP API）。
- 订阅/切换/元数据/事件（如 `subscribe-pane`、`switch-workspace`、`fs-changed`）走 notification。
- 每个 request 必带超时（client 侧 reject）；重连后 in-flight request 一律 reject，再重订阅 + 重拉快照（见 §7.4）。

### 7.1 握手（DataChannel open 后，最先交换的两条**二进制**消息）
- 每端生成临时 X25519 密钥对，发送：`0x01 || ephemeral_pub(32 bytes)`。
- 收到对端 pub 后：
  - `shared = X25519(my_priv, peer_pub)`（32B）
  - `salt = sort(my_pub, peer_pub) 后拼接`（64B，**双方按字典序排序保证一致**）
  - `key = HKDF-SHA256(ikm=shared, salt=salt, info="ridge-e2ee-v1", L=32)`
- 握手完成前不得发送/接收业务帧；任何一端收到非握手首帧则断开。

### 7.2 数据帧加密（ChaCha20-Poly1305，IETF，96-bit nonce）
- 单一对称 `key`，**按方向分离 nonce 防重放/重用**：
  - nonce(12) = `[ dir(1) , 0,0,0 , counter_u64_le(8) ]`
    - `dir=0`：host→controller；`dir=1`：controller→host
    - `counter` 每方向单调自增，从 0 开始，**严禁回绕**（接近上限须重建连接）。
  - 接收端校验 nonce 的 `dir` 必须等于“对端方向”，且 `counter` 严格递增（防重放）。
- 线上帧格式：`nonce(12) || ciphertext_with_tag`（tag 16B，附于密文尾，库默认行为）。
- 库选型（双方算法/参数必须一致）：
  - **Rust（ridge-cli，以及 Rust 侧 host）**：`x25519-dalek` + `hkdf` + `sha2` + `chacha20poly1305`
  - **浏览器/WebView（Svelte）**：`@noble/curves`(x25519) + `@noble/hashes`(hkdf/sha256) + `@noble/ciphers`(chacha20poly1305)
    - 注：WebCrypto 无 ChaCha20，必须用 noble；X25519 也统一走 noble 以保证与 Rust 字节级一致。

### 7.3 版本 / 能力握手（D9）

> 评审 2026-06-03：新增 **D9**。controller SPA（cloud 经公网下发、可独立更新）与 host（随桌面/CLI
> 版本走）必然版本漂移；不协商会导致协议/命令静默错配。故 E2EE 握手（§7.1）完成、业务帧开始前，
> 双方先交换一帧版本/能力声明，取交集后降级或明确拒绝。（待跨团队确认）

- 时机：在 §7.1 的 X25519/HKDF 完成、§7.2 加密生效**之后**、任何业务帧（pane 流 / invoke）**之前**，
  作为首条 `0x11` JSON notification 交换：
  ```json
  { "jsonrpc":"2.0", "method":"$/hello",
    "params":{ "protocolVersion":1, "capabilities":["pane","invoke","fs","git","search","workspace","theme"] } }
  ```
- 协商规则：
  - `protocolVersion` 取双方都支持的**最高公共版本**；无公共版本 → 发送
    `{ "jsonrpc":"2.0","method":"$/bye","params":{ "reason":"protocol-version-mismatch" } }` 后关闭，
    controller 侧据此提示用户升级。
  - `capabilities` 取**交集**：controller 侧把缺失能力对应的 IDE 面板灰掉 / 隐藏（而非运行时报错）。
- `capabilities` 是数据驱动的能力位，与 §8.x 的 host 特权命令准入（白名单作为数据，统一项目 D8）正交：
  握手交集决定 controller **能看到**哪些面板；准入白名单决定 host **允许执行**哪些命令。两者都不得放行
  host 特权命令（`get_remote_info`/`set_remote_enabled`/`enter_deep_root_mode` 等）。

### 7.4 attach 屏幕快照（D10）

> 评审 2026-06-03：新增 **D10**。§7 改用 raw-byte 后字节流**不可重放**，迟到订阅 / 重连的 controller
> 拿不到历史，会看到空屏/错乱。故 host 侧为**每个 pane 维护一份当前屏幕缓冲**（screen buffer），
> attach 时先下发快照，随后才续 raw 流。这也是后续在 cloud 高延迟腿上引入"屏幕状态同步"的地基。（待跨团队确认）

- `subscribe-pane`（`0x11` JSON request）的**首个响应**为**屏幕快照**（screen snapshot），随后才是 `0x10`
  raw 续流。快照可以是当前屏幕的渲染序列（含光标位置、alt-screen 状态、滚动区等终端状态），
  controller 收到后先 `kernel.feed(snapshot)` 重建屏幕，再消费后续 raw 增量。
- host 侧职责：为每个活跃 pane 维护 per-pane 屏幕缓冲（可复用终端 alt-screen / repaint 能力），
  随 PTY 输出滚动更新；新 controller attach 或既有 controller 重连时按当前缓冲生成快照。
- 重连同理：bridge 重连后对每个先前订阅的 pane 重发 `subscribe-pane`，先收快照再续流；
  重连前的所有 in-flight invoke request 一律 reject（见 §7.0）。
- 每 pane 的**锁定渲染尺寸**为该 pane 的共享属性，随 attach 快照一并下发；`resize` 是任意 controller
  可发的显式共享命令（last-write-wins，notification），不再由 controller viewport 自动触发。

---

## 8. Deep Root Mode 与 host 连接归属（**解决 Prompt2/Prompt3 的根本矛盾**）

> **矛盾**：Prompt2 把 WebRTC/E2EE 放在 WebView(Svelte)；Prompt3 的 Deep Root 要“销毁 WebView 仍保活远控”。
> 若连接活在 WebView，销毁 WebView 必然断连。本契约给出**分阶段**的权威结论：

- **目标架构（终态）**：桌面 host 的「信令 WS + WebRTC + E2EE + PTY 桥」最终应迁移到 **Rust 侧**
  （`webrtc-rs`），由 `AppState` 托管。这样 Deep Root 可**销毁 WebView**仍保活，内存才能真正暴跌。
- **v1 scaffold（本次交付，必须诚实）**：
  - 桌面 host 的 `RidgeCloudProvider`（WebRTC+E2EE）先实现在 **WebView/TS**（Agent 2）。
  - **Deep Root Mode 采用 `window.hide()`（隐藏，不销毁）**：连接活在隐藏的 WebView 里，保活成立，
    内存中等下降（非 90%）。Agent 3 在代码注释中写明 destroy-based 全量方案与“需把 host WebRTC 迁到 Rust”的前置条件，并留出清晰 stub 边界。
  - 通知文案据实：使用“已转入深根模式 🌱，本地渲染窗口已隐藏，远程通道保持活跃”，
    **不要在 v1 写死“内存降低 90%”**（hide 模式达不到，避免虚假宣传）。
  - 触发前置校验：仅当存在**活跃的云端远控会话**时才允许进入 Deep Root（否则 toast 提示）。

### 8.1 前后端命令契约（Tauri）
- `enter_deep_root_mode()`：Svelte `invoke('enter_deep_root_mode')` → Rust 校验有活跃远控 → `window.hide()` + 原生通知。
- `restore_from_deep_root()`：托盘“恢复工作台”/双击托盘 → `window.show()+focus()`；前端复活渲染循环、接管现有 PTY 增量流。
- 托盘菜单：`恢复工作台`（默认双击项）、`彻底退出 Ridge`。

---

## 9. 复用既有代码（不要重写）

- 桌面端现有 LAN 远控（`src-tauri/src/remote/*`、`src/lib/remote/wsClient.ts`、`src/remote/lib/wsRemote.ts`）
  **保持不动**；云端模式是新增的并行 provider，不替换 LAN 模式。
- ridge-cli 复用 `engine::pty`、`fs::search`、`fs::tree`，**不复制**这些模块。
  > 评审 2026-06-03：现状 ridge-cli **path-依赖** `src-tauri` 的 lib，会把 Tauri 依赖拖进 headless 二进制。
  > 统一项目（D4）将抽出运行时无关的 **`ridge-core` crate** 承载这些复用模块 + 命令 handler + 工作区/分屏
  > 领域模型，ridge-cli 改链接 `ridge-core` 取代对 src-tauri lib 的 path-依赖。归属规则见 §11。（待跨团队确认）
  - 过渡期若仍需把 src-tauri 模块设为 `pub`，仅做最小可见性调整并在报告中列出。
- pane 字节流为 **raw-byte（`0x10`）**，E2EE（§7.1/§7.2）只在其外层加密；不再使用 `postcard` 增量协议帧。
  > 评审 2026-06-03：原契约写"`postcard` 增量协议帧 schema 不改"。LAN 与 ridge-cli 实际已收敛 raw-byte
  > （delta 方案因 per-sub ~11MB `PaneParser` OOM 被弃用，见 `.kiro/specs/remote-raw-byte/`）。
  > 故此条与 §7 一并改为 raw-byte；控制/invoke 改走 §7.0 的 JSON-RPC 2.0 信封。（待跨团队确认）

---

## 10. 后端技术栈与部署约束（ridge-cloud）

- Rust + Axum 0.7 + Tokio(multi-thread) + Tower。
- DB：**SQLx + Postgres**，使用**运行时查询（非 `query!` 宏）**，以便 Docker 构建阶段无需连库。
  迁移用 `sqlx::migrate!`（`migrations/` 目录，纯 SQL）。
- 安全库：`jsonwebtoken`、`argon2`(密码哈希)、`hmac`+`sha2`(LS 校验)。
- 配置全部走环境变量（Dokku `config:set` 注入）：
  `DATABASE_URL`（Dokku postgres link 自动注入）、`JWT_SECRET`、`LEMON_SQUEEZY_SECRET`、
  `BASE_DOMAIN`(默认 `remo2ridge.duckdns.org`)、`PORT`（Dokku 注入，默认 5000）。
  启动时 fail-fast 校验必需变量存在。
- 静态托管：Web 面板（SvelteKit adapter-static）产物由后端在**主域名**兜底返回
  （非 `/api`、非 `/ws` 的 GET 路由 → 返回 SPA `index.html`/静态资源）。
- Dockerfile：多阶段 + `cargo-chef` 依赖缓存，runtime 用 `debian:bookworm-slim`，目标体积 < 50MB。
- 监听 `0.0.0.0:$PORT`。健康检查 `GET /api/v1/health`。

---

## 11. 各组件文件归属（防止并行 agent 冲突 —— 严格遵守）

| 组件 | 仓库/根目录 | 拥有的路径（只在此写） | 禁止触碰 |
|---|---|---|---|
| A. 云端后端 | `C:\code\ridge-cloud` | 除 `web/` 外的一切 | `web/` |
| B. RemotePanel + E2EE provider | `C:\code\wind` | `src/`（含改 `RemotePanel.svelte`、新增 `src/lib/remote/cloud/*`）、根 `package.json`(加依赖) | `src-tauri/`、`packages/` |
| C. Deep Root Mode | `C:\code\wind` | `src-tauri/src/`（新增 `deep_root.rs` 等 + 改 `lib.rs`）、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/*` | `src/`、根 `package.json`、`packages/` |
| D. ridge-cli | `C:\code\wind` | `packages/ridge-cli/` | 其它一切（依赖 `ridge-core`，不改 `src-tauri/`、`packages/ridge-core/` 代码，最多报告所需 `pub`） |
| E. Web 管理面板 | `C:\code\ridge-cloud` | `web/` | 仓库其它路径 |
| **F. ridge-core crate（新）** | `C:\code\wind` | **`packages/ridge-core/`**（运行时无关：命令 handler + 工作区/分屏领域模型 + `Ctx` 抽象 + 能力策略层 + 复用的 pty/fs 模块），及为接入 `ridge-core` 而改的 `src-tauri/Cargo.toml`、根 `Cargo.toml`(workspace 成员) | `src/`、`web/`、`packages/ridge-cli/` 内部实现 |

> 评审 2026-06-03：新增 **F. ridge-core crate**，并明确**打破现 §11 的 B=src/ C=src-tauri/ D=packages 边界后的新归属规则**。（待跨团队确认）
>
> 评审 2026-06-03（GM 决策 **D-GM-1**）：`ridge-core` 落在 **`packages/ridge-core/`**（与 sibling `packages/ridge-cli`、`packages/ridge-term` 平级），**不**新增独立 `crates/` 源码根 —— S1 实现已在此位置 `cargo check` 通过，复用现有 `packages/` 根更一致、零代码搬迁成本。（待跨团队确认）
>
> **背景**：统一项目 D4 要把 `src-tauri/src/commands/*` 的 handler + 工作区/分屏领域模型抽成运行时无关的
> `ridge-core` crate，由桌面 host（src-tauri 薄封装）与 headless host（ridge-cli）共用，并取代 ridge-cli
> 现状对 src-tauri lib 的 path-依赖（消除 Tauri 污染 headless 二进制，见 §9）。这越过了原 §11 中
> "B 只写 `src/`、C 只写 `src-tauri/`、D 只写 `packages/`" 的边界。
>
> **新归属规则**（打破边界后的冲突避免约定）：
> 1. **复用 `packages/` 源码根**：`ridge-core` 落在 `packages/ridge-core/`，与 `packages/ridge-cli`、
>    `packages/ridge-term`（独立二进制）同级，作为**唯一**的运行时无关 Rust 共享层；**不**新增独立 `crates/` 源码根（GM D-GM-1）。
> 2. **`packages/ridge-core/` 子树仅由 F 拥有**：B、C 的"禁止触碰"已含整个 `packages/`；D 仅拥有 `packages/ridge-cli/`，一律不得写 `packages/ridge-core/`。
>    src-tauri 与 ridge-cli 通过 `Cargo.toml` 依赖声明接入 `ridge-core`，不复制其代码。
> 3. **handler 迁移的所有权移交**：从 `src-tauri/src/commands/*` 迁入 `packages/ridge-core/` 的 handler，
>    其所有权随之从 C 移交 F；C 在 src-tauri 侧只保留**薄封装**（Tauri command → 构造 `Ctx` → 调
>    `ridge_core::dispatch`）。迁移以"每功能点一 commit"推进，避免 C/F 同时大改同一逻辑产生冲突。
> 4. **workspace 清单的共享写权**：根 `Cargo.toml`（workspace 成员列表）与 `src-tauri/Cargo.toml`
>    （依赖 `ridge-core`）是 C 与 F 的**共享接触点**，仅允许追加 `ridge-core` 相关条目，改动须在 PR 描述中
>    显式列出，由 GM 把关合并顺序。
> 5. **`ridge-core` 零 Tauri 依赖硬约束**：F 实现 `Ctx`（状态 + 事件发射 trait + 后台任务派发**直依 `tokio`**、
>    不经 `tauri::async_runtime` + 错误映射），保证 headless 二进制不被 Tauri 污染（见 §9、统一计划 R3）。

- B 调用 `invoke('enter_deep_root_mode')` / `invoke('restore_from_deep_root')`（C 实现），属契约调用，无文件冲突。
- A 与 E 同处 `ridge-cloud` 仓库但子树互斥（A 不进 `web/`，E 只进 `web/`）。仓库与 `web/` 目录由编排者预先建好。
- C 与 F 同处 `C:\code\wind` 但源码根互斥（C 写 `src-tauri/src/`，F 写 `packages/ridge-core/`）；
  二者唯一共享接触点是 `src-tauri/Cargo.toml` 与根 `Cargo.toml` 的 `ridge-core` 依赖/成员声明（见上规则 4）。
