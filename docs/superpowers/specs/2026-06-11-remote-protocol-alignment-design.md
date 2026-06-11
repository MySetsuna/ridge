# Remote 协议对齐修复设计（LAN ↔ cloud 双端统一）

- 日期：2026-06-11
- 状态：设计稿，待评审（本轮只出方案，不改代码）
- 涉及仓库：`wind`（`packages/ridge-cli`、`src-tauri`、`packages/ridge-core`、前端 `src/`）、`ridge-cloud`（`src/ws`）、**新增** `ridge-signaling`（跨仓共享 crate）
- 上游依据：`.agent-team/findings-align.md`（P0×1、P1×3、P2×4）
- 协调：信令 schema 的 **crypto 字段语义**与 crypto-reviewer 对齐（见 §0 边界约定与 §7）

## 背景

审核（findings-align.md）确认业务面（JSON-RPC 2.0 / `$/hello` / 0x10·0x11·0x12 mux / X25519+ChaCha20 E2EE）三端对齐良好且有跨实现 conformance 测试，但**信令面**存在一条致命漂移与若干结构性不一致：

1. **P0**：`ridge-cli` 无头 host 信令仍是 §5.1 单控制方模型、**全程无 `cid`**（`packages/ridge-cli/src/signaling.rs:16-42`），而 relay 的 §5.3 实现要求 host→controller 的 answer/ice/kick **必须带 cid**，缺失即丢弃（`ridge-cloud/src/ws/handler.rs:425-468`）→ 浏览器 controller 连不上 ridge-cli host。
2. **P1**：信令消息类型在 **4 处各自手写**并已漂移（relay `messages.rs`、桌面 host `ridgeCloudProvider.ts`、桌面 controller `controllerCloudProvider.ts`、cli `signaling.rs`）；ridge-cli 还缺 `e2ee-pubkey` → 防 relay-MITM 的公钥旁路对无头 host **静默退化 relay-trust**。
3. **P1**：LAN 与 cloud 的鉴权状态机不同构（LAN 传输层 token gate；cloud E2EE 后 0x12 带内 TOTP）。

根因是**信令契约没有机器可校验的单一事实来源（SSOT）**，靠 4 份手写 + 散文契约同步。本设计把信令 schema 收敛为 SSOT，并据此修复 P0/P1。

## 目标 / 非目标

**目标**
- 信令消息 schema 收敛为 SSOT：Rust 共享 crate（relay + ridge-cli 真共享）+ 生成/对照的 TS 类型 + 跨语言 golden-fixture conformance 测试。
- 修 P0：ridge-cli 无头 host 全链路承载 `cid`，让浏览器 controller 能连上（与现网 relay 即时互通，无需 relay 改动）。
- 补 `e2ee-pubkey`：ridge-cli 参与 §7.3 公钥旁路绑定（承载字段由本设计定，**语义由 crypto 定**）。
- LAN/cloud 鉴权状态机收敛为传输无关的统一 `authState` 抽象（不削弱任一腿的真实鉴权）。
- 为 crypto 零信任方案定**载体**：B 层 0x02 握手帧 tag 登记、C 层 `totp-bind` 与 `$/hello` 能力位 `device-id`/`totp-bind`（字节/算法/验证归 crypto，§0/§2.2/FIX-5）。

**非目标**
- 不写实现代码、不动任何源文件（本轮纯设计）。
- 不定义 B 层 0x02 握手帧的字节布局/Ed25519 算法、不定义 `totp-bind` 的 HMAC 算法（属 crypto §7）；本文档仅**登记 tag、指定载体归属**（§2.2、FIX-5）。
- 不改 0x10/0x11/0x12 mux **帧框架**（已对齐）；但 0x12 内的 `SessionControl` payload 新增 `totp-bind` 变体（C 层，FIX-5），JSON-RPC 业务信封不变。
- crypto 绑定方案的**字段语义/算法/验证规则/签名密钥来源**不在本文档（属 crypto 设计文档，本文档只定载体并引用之）。
- 移动端 legacy SPA（`src/remote/`，`stdin`/`invoke-request`，LAN-only）不纳入统一（保留旧路径）。

## §0 与 crypto-reviewer 的边界约定（已对齐）

> crypto-reviewer 2026-06-11 回函（`docs/superpowers/specs/2026-06-11-remote-zero-trust-crypto-design.md §7`）。分工：**align 定载体 schema，crypto 定字段语义/校验**。下方为对齐后的结论。

**关键澄清——"信令 schema" 实际跨 3 个 SSOT 层**（crypto 5 项字段分属不同层，不能全塞进一个 crate）：

| 层 | 物理通道 | relay 可见性 | SSOT 载体 | align 是否拥有 |
|---|---|---|---|---|
| **A 信令 JSON** | relay WS 文本帧 | relay 读 `t`/`cid` 路由 | **`ridge-signaling` crate**（本文档 §2） | ✅ 是 |
| **B E2EE 握手二进制首帧** | DataChannel，**加密前** | relay 不经手 | `e2ee.ts`/`e2ee.rs`（握手 tag） | ❌ crypto 定字节，本文档仅登记 tag |
| **C E2EE 内明文 mux** | DataChannel，**加密后** | relay 不经手 | mux SSOT + SessionControl SSOT + `$/hello` 能力 SSOT | ❌ 业务面，§6 收敛 |

**crypto 5 项字段 → 层归属 + 载体形态（align 拍板）**：

| crypto 项 | 归层 | 载体形态（align 定） | 语义（crypto 定，见其文档） |
|---|---|---|---|
| 1. `cid` | A | `cid: Option<Cid>`（§2.1 路由规则）。**注记：cid 仅寻址，客户端不得据它做信任决策** | 不变 |
| 2. `e2ee-pubkey`（语义升级） | A | **保持最小**：`{ t:"e2ee-pubkey", pubkey:<base64 32B>, cid? }`，**不**加 sig/alg/ts（签名走 B 层 0x02） | 旁路 eph_pub 与 0x02 帧比对 + 验签 |
| 3. 握手 tag `0x02` | **B** | **不进信令 crate**；登记进 §2.2 tag 注册表（防与 0x01/0x10-0x12 冲突）。布局 `0x02‖eph_pub(32)‖id_pub(32)‖sig(64)` 由 crypto 在 `e2ee.*` 定字节 | Ed25519 设备身份签名覆盖 eph_pub |
| 4. `totp-bind` | **C** | `SessionControl`（0x12 CONTROL）新增变体 `TotpBind { tag: String }`（base64 HMAC），并存旧 `TotpVerify{code}` 作回退 | 信道绑定 HMAC |
| 5. `$/hello` 新能力 `device-id`/`totp-bind` | **C** | 加入能力集 SSOT（字符串位）；交集双方都宣告才启用 fail-closed，否则回退（现有 $/hello 交集机制天然满足） | fail-closed 启用门 |

**承载层不变量（align 保证）**：
- A 层：relay 对 e2ee-pubkey 仅读 `t`/`cid` 路由、**零解析 pubkey**（不变）。crypto 的设备身份签名**不进 A 层 JSON**，改走 B 层 DataChannel 0x02 帧 → relay 永不经手任何密码学材料，零信任边界更干净（比原占位的"relay 透传 sig"更优）。
- 错误码 `SIGNATURE_INVALID`（契约 §2，第 68-70 行）：绑定/验签失败发生在 host 端（E2EE 之后），**不经 relay**。载体 = 0x11 业务通道的 `$/bye { reason:"signature-invalid" }`（D9 语义，与现有 keyBinding reject→teardown 一致），**非**信令 `error` 帧。待 crypto 回执确认此载体。

---

## §1 关键设计决策

### D1：SSOT 载体 —— 跨仓共享 crate + golden-fixture 对照（核心决策）

**约束**：`ridge-cloud`（relay）与 `wind`（ridge-cli 在 `packages/ridge-cli`，是 wind workspace 成员）是**两个独立仓库/workspace**（`ridge-cloud/Cargo.toml` 为独立包；`wind/Cargo.toml:12-21` 为含 ridge-cli 的虚拟 workspace）。两个 Rust 端无法用 `path` 依赖共享，TS 端更不能依赖 Rust crate。

**决策（推荐 = 方案 C 混合）**：

1. **新建独立微仓 `ridge-signaling`**（纯 serde 数据模型，无重依赖），作为 Rust 侧真正的单一来源：
   - `ridge-cloud` 与 `ridge-cli` 都以 **`git` 依赖**（锁定 tag/rev）引入它，**彻底消除两个 Rust 端的漂移**（这正是 P0 的根源 —— Rust×Rust 漂移）。
2. **TS 侧**：用 **`ts-rs`** 在 `ridge-signaling` 上 derive，`cargo test` 产出 `signaling.ts`（committed），CI 校验"重新生成无 diff"——满足"生成 TS 类型"。
3. **跨语言 golden-fixture conformance**：crate 内 `fixtures/signaling/*.json` 为权威帧语料；Rust 端 `tests/conformance.rs` 解析+序列化往返断言，TS 端 vitest 加载**同一份** fixtures 断言往返一致——满足"对照"。fixtures 经 `scripts/sync-signaling-fixtures` 同步进 wind（`src/lib/remote/cloud/__fixtures__/signaling/`）+ checksum CI 校验，解决跨仓取数。

**备选（更轻，若团队拒绝跨仓 git 依赖）= 方案 B**：canonical 留在 `ridge-cloud/src/ws/messages.rs`，ridge-cli **vendored 一份**，两侧各跑 conformance 对 golden fixtures。代价：Rust 仍两份（但被 fixtures 机器钉死，非散文）。**推荐 C**，因为它真正消灭了 P0 的漂移面。

**取舍写明（需团队接受）**：方案 C 引入**跨仓发布耦合**——schema 改动需 `ridge-signaling` 发 rev + 两仓 bump。对小团队可接受，因为信令 schema 变更频率低（属契约级），且换来"再不可能两份漂移"。

### D2：P0 修复是「向前修复」而非「flag day」
relay 现网**已**强制 cid（注入 + 要求）。ridge-cli 是落后方。给 ridge-cli 补 cid 是**纯增量**：新 ridge-cli 一上线即与现网 relay 互通；旧 ridge-cli 本就连不上（无回归）。**relay 不需要任何改动**即可让 P0 修复生效。

### D3：鉴权状态机收敛 = 统一抽象，不强行统一机制
不强迫 LAN 也跑 0x12 带内 TOTP（LAN 不用 mux，且其 token 在 WS 升级已校验、并绑定 device+IP，强度不弱）。而是在 L2/bridge 引入**传输无关**的 `authState: 'pending'|'authorized'|'denied'`：
- LAN adapter：`connected` 即 `authorized`（token 升级时已验）。
- cloud adapter：`connected` → `pending`（驱动 0x12 TOTP）→ `totp-result{ok}` → `authorized`。
controller UI 只 gate 在单一 `authorized` 信号上，消除按 transport 分支的 ready 逻辑——状态机同构，各腿用各自机制满足。

### D4：业务面 SSOT 与信令面分离
`$/hello` 版本/能力协商属**业务面**（E2EE 之后、relay 不可见），不进 `ridge-signaling`。它的 SSOT 落 `packages/ridge-core`（wind 内两个 Rust host —— `src-tauri` 与 `ridge-cli` —— 都依赖 ridge-core），TS 侧镜像 + 同款 conformance。属次要收敛项（§6）。

---

## §2 SSOT crate：`ridge-signaling`

### 模块结构
```
ridge-signaling/                      （新微仓）
  Cargo.toml                          serde + (dev) ts-rs；无 tokio/axum 等重依赖
  src/lib.rs                          SignalMsg / Role / Cid / 错误码常量 + (de)序列化
  fixtures/signaling/*.json           跨语言权威帧语料（golden）
  tests/conformance.rs                Rust 往返断言
  bindings/signaling.ts               ts-rs 生成（committed），供 wind 对照
```

### 数据模型（保留**现有线上字段名**，确保零线协议变更）
- `tag = "t"`、`rename_all = "kebab-case"`、`#[serde(other)] Unknown` 兜底前向兼容。
- `peerPresent` 维持 camelCase（现网即如此），`candidate`/`sdp`/`pubkey`/`code`/`message` 维持原名。

枚举（superset，覆盖四端 + crypto 预留位）：
```
Welcome   { room: String, role: Role, cid: Option<Cid>, #[serde(rename="peerPresent")] peer_present: bool }
PeerJoin  { role: Role, cid: Option<Cid> }
PeerLeave { role: Role, cid: Option<Cid> }
Error     { code: String, message: String }
Offer     { sdp: String, cid: Option<Cid> }
Answer    { sdp: String, cid: Option<Cid> }
Ice       { candidate: Option<serde_json::Value>, cid: Option<Cid> }
Kick      { cid: Cid }
E2eePubkey{ pubkey: String, cid: Option<Cid> }   // 仅 eph_pub 旁路；设备身份签名走 B 层 0x02（crypto §7）
Unknown   (#[serde(other)])
```
错误码常量：`CODE_KICKED="KICKED"`、`CODE_CONTROLLER_LIMIT_REACHED="CONTROLLER_LIMIT_REACHED"`、`CODE_REPLACED="REPLACED"`（迁出 `messages.rs:84`、`handler.rs:231`）。

> **A 层零密码学材料**（crypto 对齐结论）：信令 JSON 不再承载任何 sig/alg/ts；设备身份 Ed25519 签名只在 B 层 DataChannel 0x02 握手帧里传，relay 永不经手 → 零信任边界更干净。e2ee-pubkey 仍是"经独立 TLS 信令旁路 eph_pub，供对端与 0x02 帧比对防 relay-MITM"的唯一用途。

### §2.2 协议 tag 注册表（防跨层冲突，crypto 项 3）
握手 tag（B 层，加密前）与 mux tag（C 层，加密后明文）处于**不同相位**，物理上不共享字节空间，但统一登记以防混淆：

| tag | 层 | 含义 | 线形 | SSOT 家 |
|---|---|---|---|---|
| `0x01` | B 握手 | 旧·裸临时公钥 | `0x01‖eph_pub(32)` = 33B | `e2ee.ts`/`e2ee.rs`（现有 `HANDSHAKE_TAG`） |
| `0x02` | B 握手 | **新·带设备身份签名**（crypto） | `0x02‖eph_pub(32)‖id_pub(32)‖sig(64)` = 129B | `e2ee.ts`/`e2ee.rs`（crypto 定字节 + 跨实现 conformance） |
| `0x10` | C mux | PANE_RAW | `0x10‖paneIdLen‖paneId‖raw` | `cloudMux.ts`/`mux.rs` |
| `0x11` | C mux | JSON-RPC 业务 | `0x11‖utf8(json)` | 同上 |
| `0x12` | C mux | CONTROL（SessionControl） | `0x12‖utf8(json)` | 同上 + `protocol.rs::SessionControl` |

`0x02` **不属 `ridge-signaling` crate**（它是 DataChannel 二进制，非 relay JSON）；归 e2ee SSOT，由 crypto 定字节布局，本表仅登记占位以保证 `0x02` 不与未来 tag 冲突。

### §2.1 cid 寻址 + relay 路由规则（align 权威定义）
| 帧 | 方向 | 发送端 cid | relay 行为 |
|---|---|---|---|
| Offer / Ice / E2eePubkey | controller→host | **不带**（None） | 加盖该 controller 的 cid 后转发给 host（`handler.rs:397-421`） |
| Answer / Ice / E2eePubkey | host→controller | **必带**（Some） | 按 cid 定向投递；缺失/未知 → 丢弃（`handler.rs:425-468`） |
| Kick | host→relay | **必带** | 取该 cid 的 controller，发 `error{KICKED}` 后关闭 |
| Welcome | server→controller | controller 自身 cid | host 收到为 None |
| PeerJoin/PeerLeave(role=controller) | server→host | 该 controller 的 cid | role=host 时为 None |

---

## §3 修复项（每项含：改动文件清单+行号锚点 / 迁移兼容 / 依赖）

### FIX-1【P0】ridge-cli 无头 host 全链路承载 cid
**改动文件（wind/packages/ridge-cli）**
- `src/signaling.rs:16-42` — 删本地 `SignalMsg` 枚举，改 `pub use ridge_signaling::SignalMsg;`；`Signaling::connect/读写任务`（66-123）改用共享类型（序列化形状不变）。
- `src/rtc.rs:20-27, 29-36` — `PeerInbound::Offer(String)` → `Offer{ sdp, cid: Cid }`；`PeerInbound::Ice` 加 `cid`；`PeerOutbound::Answer` / `Ice` 加 `cid`。`handle_offer`（190-201）回 `PeerOutbound::Answer{ sdp, cid }`。
- `src/session.rs:225-247` — 入站 `SignalMsg::Offer{ sdp, cid }` 记下会话 `cid`；`213-222` 出站把 `cid` 缝进 `SignalMsg::Answer{ sdp, cid }` / `Ice{ candidate, cid }`。
- `src/daemon.rs:92-100` — `peer-join{role:controller,cid}` 取出 cid 传给 `RemoteSession::run`（单控制方先取首个 cid）。
- `Cargo.toml:30-` — 加 `ridge-signaling = { git = "...", rev = "..." }`。

**对照锚点（relay 侧，无需改）**：`ridge-cloud/src/ws/handler.rs:397-421`（注入）/`425-468`（要求）。

**迁移/兼容窗口**：纯向前修复（D2）。新 ridge-cli 与**现网 relay 即时互通**；relay 零改动；旧 ridge-cli 本就不通（无回归）。无 flag day。
**依赖**：§2 crate 就绪。

### FIX-2【P1】ridge-cli 参与 `e2ee-pubkey`（A 层承载，仅 eph_pub 旁路）
**改动文件（wind/packages/ridge-cli）**
- `src/session.rs:124-149` — E2EE 握手发出本端 ephemeral pub 时，**同时**经信令发 `SignalMsg::E2eePubkey{ pubkey: base64(eph_pub), cid }`（**最小帧，不含 sig/alg/ts**——设备身份签名走 FIX-5 的 B 层 0x02 帧）；并处理入站 `E2eePubkey`（与 DataChannel 握手帧公钥比对，逻辑镜像 `ridgeCloudProvider.ts:357-410` 的 keyBinding）。
- 新增 `src/key_binding.rs`（镜像 `src/lib/remote/cloud/keyBinding.ts::decideKeyBinding` 的三态 accept/reject/wait 纯判定 + 3s grace）。
- `src/signaling.rs` — `E2eePubkey` 变体已在 §2 crate。

**对照锚点**：桌面 host `ridgeCloudProvider.ts:301-317, 522-534`；controller `controllerCloudProvider.ts:202-211, 412-420`；grace `KEY_BIND_GRACE_MS=3000`。

**迁移/兼容窗口**：增量。新 ridge-cli 发 e2ee-pubkey → controller `enforced`；旧 ridge-cli 不发 → controller 3s grace 回落 relay-trust（现状，不回归）。relay 零解析透传。
**依赖**：§2 crate。**与 crypto 解耦**：本项只做 eph_pub 旁路（relay-trust 防 MITM 的现有能力补齐到 ridge-cli）；完整设备身份强绑定属 FIX-5（B 层），可后续叠加。

### FIX-5【crypto 承载】跨 B/C 层接入 crypto 字段（载体由本文档定，语义见 crypto §7）
> 这些**不在** `ridge-signaling`（A 层）里。本项确保 align 的载体决策与 crypto 设计一致，供其回执。

- **B 层 0x02 握手帧**（crypto 项 2/3）：`e2ee.ts` / `packages/ridge-cli/src/e2ee.rs` / （桌面 host）扩握手 tag —— `0x01` 保留，新增 `0x02 = eph_pub(32)‖id_pub(32)‖sig(64)`（字节布局 + Ed25519 验签由 crypto 定，本文档 §2.2 登记 tag 防冲突）。`decodeHandshakeFrame`/`parse_peer_frame`（`e2ee.ts:60-65`、`e2ee.rs:83-101`）按 tag 分支。跨实现 conformance（同 e2ee 现有模式）锁字节一致。
- **C 层 `totp-bind`**（crypto 项 4）：`packages/ridge-cli/src/protocol.rs:18-25` `SessionControl` 加变体 `TotpBind { tag: String }`（base64 HMAC），保留 `TotpVerify { code }` 作回退；TS 端 `cloudHostBridge.ts:286-324`、`cloudControllerBoot.ts:144-168` 对称加 `totp-bind` 分支。SessionControl 提升为带 conformance 的小 SSOT（protocol.rs ↔ TS）。
- **C 层 `$/hello` 新能力位 `device-id` / `totp-bind`**（crypto 项 5，fail-closed 启用门）：加入能力集 SSOT（见 §6 P2-2）。双方交集都含该位才启用强校验；任一缺失 → 回退（现有 `$/hello` 交集机制天然 fail-open-compatible，强校验本身 fail-closed）。
- **绑定失败信令**：host 端验签/绑定失败 → 0x11 业务通道发 `$/bye { reason:"signature-invalid" }`（复用契约 `SIGNATURE_INVALID` 语义）+ teardown；**不经 relay、不发信令 error 帧**。

**迁移/兼容窗口**：全部由 `$/hello` 能力位 gating —— 只有双方都宣告 `device-id`+`totp-bind` 才启用 0x02/totp-bind 强路径，否则各自回退 0x01/totp-verify（旧端零感知）。无 flag day。
**依赖**：crypto §7 定字节/算法/验证；§6 P2-2 能力集 SSOT 提供能力位；与 FIX-2 叠加（先 eph_pub 旁路、后设备身份强绑定）。

### FIX-3【P1】信令四份手写收敛到 SSOT
**改动文件**
- `ridge-cloud/src/ws/messages.rs:41-71` — `ServerEvent` 改为 `pub use ridge_signaling::SignalMsg`（或保留 server-emitted 子集为 `From`/re-export）；`handler.rs:425-468` 的 `route_host_frame` 由 `serde_json::Value` 最小解析**升级为** typed `SignalMsg`（仍只读 `t`/`cid` 路由，但类型安全）；`Cargo.toml` 加 git 依赖。
- `wind/src/lib/remote/cloud/signaling.ts`（**新建**）— 由 `ridge-signaling` ts-rs 生成的类型 + (de)序列化；`ridgeCloudProvider.ts:67-77` 与 `controllerCloudProvider.ts:53-63` 删本地 `SignalIn`，改 import 此模块。
- `wind/src/lib/remote/cloud/__fixtures__/signaling/`（**新建**）+ `signaling.conformance.test.ts`（**新建**）— vitest 对 golden fixtures 往返。
- `scripts/sync-signaling-fixtures.*` + CI checksum 校验。

**迁移/兼容窗口**：纯重构，**线协议字节不变**（fixtures 锁死现有形状，含 peerPresent camelCase）。各端可**独立**切换（任意顺序），因线上不变。
**依赖**：§2 crate（含 ts-rs 生成 + fixtures）。

### FIX-4【P1】LAN/cloud 鉴权状态机收敛（D3）
**改动文件（wind/src）**
- `src/lib/transport/remote/types.ts:79-115` — `ChannelTransport` 加 `authState(): AuthState` + `onAuthChange(cb)`；新增 `export type AuthState = 'pending'|'authorized'|'denied'`。
- `src/lib/transport/remote/lanWsAdapter.ts:102-119` — `connected` 即置 `authorized`（token 升级已验，`server.rs:939-985`）。
- `src/lib/transport/remote/cloudWebrtcAdapter.ts:131-141, 208-219` — `connected`→`pending`；把 0x12 `totp-result{ok:true}` 升为 `authorized`、`{ok:false,locked}` 为 `denied`（现 `sendSessionControl`/`onSessionControl` 已具备通道）。
- `src/lib/remote/cloud/cloudControllerBoot.ts:144-168` — `verifyTotpOverControl` 成功后驱动 adapter 置 authState。
- `src/lib/remote/RemotePanel.svelte:~293` 周边 — UI ready/gate 改读统一 `authState`，删按 transport 分支。

**对照锚点**：LAN gate `server.rs:897-935, 939-985`；cloud gate `cloudHostBridge.ts:178-192, 256-324`、cli `session.rs:69-83, 361-390`。

**迁移/兼容窗口**：纯 controller 端抽象，**无线协议变更**；LAN 行为不变（authorized-on-connect），cloud 行为不变（TOTP-gated）。各 host 无需改。
**依赖**：无（独立于 §2 crate，可并行）。

---

## §6 次要收敛项（findings P1-2 / P2，建议同批，单独 commit）

- **P1-2 ridge-cli 多控制方**：`daemon.rs:82-120` 改 `HashMap<Cid, RemoteSession>` 多路复用；处理 `kick`/`peer-leave{cid}` 定向拆除；或在 `$/hello` 公告 ridge-cli 为单控制方并让 relay 对其房间限容 1（`rooms.rs:147-155` 已有 `max_controllers` 闸）。**依赖 FIX-1**（cid 先通）。
- **P2-1 `$/hello` 版本协商**（三份：`server.rs:3239-3275`、`cloudHostBridge.ts:643-663`、`rpc.rs:133-161`）：`peer<host→$/bye` 改 `min(peer,host)` 且回 `min`，仅 `min<1` 才 `$/bye`；落 ridge-core SSOT（D4）。
- **P2-2 能力集 SSOT（⚠ crypto 强依赖，提升优先级）**：`server.rs:3134`、`cloudHostBridge.ts:45`、`rpcClient.ts:51`、`rpc.rs:33` 四份 → Rust 落 `ridge-core::remote_protocol`（server.rs + ridge-cli 共享），TS 镜像 + 同款 conformance（注意 cli 故意子集 pane/fs/search 需保留为"声明的子集"而非漂移）。**新增能力位 `device-id`、`totp-bind`**（crypto 项 5 的 fail-closed 启用门）必须进同一 SSOT —— 这是 FIX-5 强校验的开关，故本项从"次要"提升为与 FIX-5 协同的前置。
- **P2-3 welcome.room**：`handler.rs:252-260` 回 `{uuid}/{device}` 与契约文档 `{device}-{username}` 不符；二选一钉死（改文档示例，或服务端回 label）。仅文档/日志，无功能影响。
- **P2-4 nonce 保留字节**（属任务 #2/crypto）：`e2ee.ts:202-221` 增 `nonce[1..4]==0` 校验对齐 `e2ee.rs:205-208`；本设计仅记引用，归 crypto 文档。

---

## §7 安全与边界

- **relay 零信任更干净**（crypto 对齐后）：`ridge-signaling`（A 层）不含任何密码学材料 —— 设备身份 Ed25519 签名改走 B 层 DataChannel 0x02 帧，**relay 永不经手 sig/id_pub**，仅读 `t`/`cid` 路由 e2ee-pubkey 的 eph_pub 旁路。比原占位"relay 透传 sig"边界更窄。
- **crypto 边界**：本设计只定**载体**——A 层 `E2eePubkey`/`cid` schema、B 层 0x02 tag 登记、C 层 `totp-bind`/能力位的承载形态与 relay 透传不变量。**字节布局/算法/验证/防重放/签名密钥来源由 crypto 设计文档定义**，本文档 §0/§2.2/FIX-5 引用之。三层归属见 §0 表。
- **鉴权不削弱**：FIX-4 是抽象统一，LAN 保留 token+device+IP 绑定，cloud 保留 E2EE 后带内 TOTP（FIX-5 后升级为 totp-bind 信道绑定）；两腿真实强度不变。
- **向后兼容**：FIX-1/2 增量、FIX-3 线上零变更、FIX-4 纯客户端抽象、FIX-5 全程 `$/hello` 能力位 gating —— 无 flag day，无需双端同时升级。

## §8 测试

- **`ridge-signaling`**：`tests/conformance.rs` 对每个 fixtures 帧做 parse→serialize 往返 == 原 JSON；`Unknown` 兜底未知 `t`；cid 可选性（controller 发无 cid / relay 注入后有 cid）双形态。
- **TS 对照**：`signaling.conformance.test.ts` 加载同一 fixtures 往返一致；ts-rs 生成物 CI "regen 无 diff"。
- **FIX-1**：ridge-cli 单测——入站 `offer{sdp,cid}` → 出站 `answer{sdp,cid}` cid 原样回盖；集成（feature `rtc`）端到端：浏览器 controller mock ↔ ridge-cli，断言 answer 带 cid 且被 relay 路由（可对 `route_host_frame` 写单测：无 cid→丢，有 cid→投递）。
- **FIX-2**：ridge-cli keyBinding 三态纯单测（与 `keyBinding.test.ts` 对齐），仅 eph_pub 旁路。
- **FIX-4**：lanWsAdapter `connected→authorized`；cloudWebrtcAdapter `connected→pending→(totp ok)authorized/(locked)denied`；controller ready 只依赖 authState 的回归测试。
- **FIX-5**（crypto 协同）：B 层 0x02 握手帧 `e2ee.ts↔e2ee.rs` 字节级 conformance + 篡改 sig→reject；C 层 `SessionControl::TotpBind` 往返 + protocol.rs↔TS conformance；`$/hello` 含/缺 `device-id`+`totp-bind` 时的强校验启用/回退分支。具体断言以 crypto §7 为准。

## §9 实施顺序（建议，逐项单独 commit；跨仓需协调 rev）

1. 建 `ridge-signaling` crate（数据模型 + fixtures + Rust conformance + ts-rs 生成）。【SSOT 地基】
2. relay 接入（FIX-3 relay 侧）：`messages.rs`/`handler.rs` 改 typed，线上不变。
3. ridge-cli 接入 crate + **FIX-1 cid**（含 rtc/session/daemon 缝合）。【解 P0】
4. ridge-cli **FIX-2 e2ee-pubkey** A 层 eph_pub 旁路（仅补 relay-trust 防 MITM，与 crypto 解耦）。
5. TS 接入 crate 生成类型 + fixtures conformance（FIX-3 TS 侧）。
6. **FIX-4** 鉴权状态机收敛（可与 1–5 并行，独立）。
7. 能力集 SSOT（§6 P2-2）+ 新增 `device-id`/`totp-bind` 能力位 —— **crypto fail-closed 的前置开关**。
8. **FIX-5** crypto 载体接入（B 层 0x02 握手帧 + C 层 `totp-bind`），字节/算法/验证以 crypto §7 为准；由步骤 7 能力位 gating。
9. 其余次要收敛项（§6：ridge-cli 多控制方依赖步骤 3；$/hello 版本协商；welcome.room 文档；nonce 保留字节归 crypto）。

**关键依赖**：步骤 1 是 2/3/5 的前置；FIX-1（步骤 3）是 P0 与多控制方的前置；FIX-4（步骤 6）无依赖可先行；步骤 7（能力位 SSOT）是步骤 8（FIX-5 强校验）的前置开关。FIX-5 与 crypto §7 协同定稿，但**不阻塞** P0（步骤 3）与 A 层收敛（步骤 1-5）。
