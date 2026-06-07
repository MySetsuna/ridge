# D-GM-10 · E2EE 公钥↔身份绑定 设计（B3）

> 状态：**设计稿**。基于对两仓库实际代码的核实（wind `src/lib/remote/cloud/e2ee.ts`、`cloudHostBridge.ts`；ridge-cloud `src/crypto.rs`、`src/auth/jwt.rs`、`src/ws/`）。**仅设计**——E2EE 是安全特性，按「设计→改契约→实现→cloud e2e 验证」顺序推进，不可跳过设计直接写 crypto（错误的绑定 = 假安全，比现状更糟）。
> 触发：2026-06-07 /goal，用户授权改 ridge-cloud。核实后确认实现被两条硬条件阻塞（见 §5），故先交付设计。

## 1. 现状与缺口（核实）

E2EE 握手是**无认证的 X25519 临时 DH**（`e2ee.ts`）：首帧 `0x01 || ephemeral_pub(32B)`，`decodeHandshakeFrame` 只校验帧格式，**不校验对端临时公钥是否绑定到配对设备/账户身份**。`cloudHostBridge.verifyPeerKey`（host 侧挂载点）默认 `return true`（relay-trust v1，代码已显式接受）。

**威胁**：撮合双方的 relay/信令层若被攻陷（或一个独立的 relay 节点而非 auth 后端），可在 E2EE 腿做主动 MITM——给 A 发自己的公钥、给 B 发自己的公钥，分别建两条 E2EE，居中解密。当前无法检测。

**接受的边界**（doc 既定）：完全攻陷 cloud auth 后端不在防护范围（你已 deviceJWT 信任它）；目标是把"任何能转发握手的中间人都能 MITM"收窄到"只有持 cloud 信令认证态的一方能 MITM"。

## 2. 设计选型：认证信令旁路确认（不需新增非对称密钥）

`crypto.rs` 现仅有 **HMAC-SHA256（对称）**；非对称签名（ed25519）方案需新增签名密钥 + 向两端分发公钥 + 公钥固定，重且引入新密钥管理。**更简方案利用既有的「已认证信令通道」做旁路确认**：

- 信令 `/ws` 上，两端都已用 **deviceJWT 认证**（`auth/jwt.rs`），且走 TLS。
- 令两端把自己的**临时公钥**经该已认证信令上报 cloud；cloud 把对端公钥经**各自的已认证信令通道**转发回来。
- 每端 `verifyPeerKey`（E2EE 握手收到对端公钥时）校验：**E2EE 握手帧里的对端公钥 == cloud 经信令转发的该对端公钥**。不一致 → 判定 MITM、拒绝会话（回 `$/bye`、上层断开）。

**为何成立**：E2EE 腿（WebRTC DataChannel）上的网络 MITM 无法同时篡改**另一条**已认证 TLS 信令通道上转发的公钥；两通道公钥不一致即暴露。仍信任 cloud 信令认证（既定边界），但把"裸 relay 转发即可 MITM"提升为"须同时攻陷 TLS 信令 + 持双方 deviceJWT 认证态"。

> 备选（更强、更重）：cloud 用 ed25519 私钥签 `(sessionId, role, pubkey)`，两端用固定的 cloud 公钥验签。可防"独立 relay 节点篡改信令转发"。列为后续增强，v1 取认证信令旁路确认即可达 doc 既定目标。

## 3. 协议改动（**须先改契约** `docs/ridge-cloud-protocol.md` §7）

新增信令消息（`/ws` 信令通道，JSON）：
- `{ t: "e2ee-pubkey", sessionId, role, pubkey: <base64 32B> }`（peer → cloud，认证态下上报本端临时公钥）。
- `{ t: "e2ee-peer-pubkey", sessionId, peerRole, pubkey: <base64 32B> }`（cloud → peer，转发对端公钥）。
- 时序：信令建立 + deviceJWT 认证 → 双方各发 `e2ee-pubkey` → cloud 配对后各回 `e2ee-peer-pubkey` → 双方开始 E2EE 握手 → `verifyPeerKey` 比对。
- 兜底/兼容：未收到 `e2ee-peer-pubkey` 时按 v1 relay-trust（`verifyPeerKey` 放行）以免老 controller 回归；新增能力位 `e2ee-bind` 经 D9 `$/hello` 协商，双方都支持才启用严格比对。

## 4. 两仓库触点

**ridge-cloud**（`src/ws/`）：信令处理加 `e2ee-pubkey` 入站缓存（按 sessionId+role）+ 配对后向两端推 `e2ee-peer-pubkey`。纯转发，cloud 不持久化、不参与 E2EE。**单测**：给定两端上报，断言各收到对端公钥。

**wind**：
- `ridgeCloudProvider.ts`：握手生成临时密钥对后，经信令 `sendSignal({t:'e2ee-pubkey',...})` 上报；监听 `e2ee-peer-pubkey` 存入会话。
- `cloudHostBridge.verifyPeerKey` / controller 侧对应校验：把"信令转发的对端公钥"与"E2EE 握手帧公钥"比对（`compareBytes`，已在 e2ee.ts），不一致返回 false → 现有逻辑回 `$/bye` 拒绝。
- **单测**（vitest，**纯、无需 cloud**）：①公钥一致 → verify 通过；②**篡改/调包公钥 → verify 失败**（这是安全属性的可单测核心）；③未协商 `e2ee-bind` 能力 → 放行（兼容）。

## 5. 实现被阻塞的硬条件（为何本轮止于设计）

1. **契约 WIP 冲突**：B3 须改 `docs/ridge-cloud-protocol.md`（§3 新消息），而该文件正处于**用户未提交的域名迁移 WIP**（`remo2ridge.duckdns.org→9527127.xyz` + 租户子域路由）。「改契约在先」+ 不覆盖在制品 → 须等该 WIP 提交/落定后再在干净基线上改契约。
2. **cloud e2e 验证**：MITM 抵抗的端到端属性须 live cloud 会话验证（两端 + relay）。crypto 绑定本身可单测（§4 篡改即拒），但威胁模型属性需真机。

## 6. 落地顺序（待 §5 解锁）

`用户提交域名迁移 WIP（干净基线）` → `改契约 protocol.md §7（§3 消息 + e2ee-bind 能力位）` → `ridge-cloud /ws 转发 + 单测` → `wind provider 上报 + verifyPeerKey 比对 + vitest（篡改即拒）` → `cloud e2e 验证 MITM 抵抗` →（可选增强）`cloud ed25519 签名 attestation`。

## 7. 一句话

B3 的安全方案已定（认证信令旁路确认，复用既有 deviceJWT 信令 + e2ee.ts 的 compareBytes，无需新非对称密钥）；**安全属性的可单测核心**（篡改公钥即拒）明确；实现须等用户的 protocol.md 域名迁移 WIP 落定（避免契约在制品冲突）+ live cloud 验证 MITM 抵抗。

---

## 8. 实现状态（2026-06-07 ✅ 已实现 + 实测验证）

§5 的两条阻塞均已解除（protocol.md 域名迁移 WIP 已提交 `350e7fc`；cloud e2e harness 已就绪）。B3 已在 **wind 单仓库**完整实现并实测，**ridge-cloud 零代码改动**。

### 8.1 关键设计调整：启用门改用「信令公钥到达性」而非 D9 `$/hello`
原设计 §3 用 `$/hello` 协商 `e2ee-bind` 能力位作启用门。核实发现 **`$/hello` 在 E2EE 握手完成 *之后* 才由 L2(rpcClient/bridge) 发出**——那时连接已 `connected`，太晚，无法在握手时据它决定拒绝。改用更稳健的**信令旁路公钥到达性**作隐式协商（见 `keyBinding.decideKeyBinding`）：
- 收到对端信令公钥 → 强制比对（DataChannel 握手公钥 == 信令公钥？不等即判 MITM 拒绝）。
- 宽限期（3s）内未收到 → `wait`；过期仍无 → 回落 relay-trust（对端疑为不发信令公钥的旧端）。
- **DataChannel 网络 MITM 无法借「回落」逃逸**：它无法阻止信令公钥经**独立 TLS 信令**通道到达；能让你收不到信令公钥的，只有攻陷信令本身（既定边界外）。

### 8.2 实现要点
- **wire 原语**：`e2ee.ts` 加 `bytesToBase64`/`base64ToBytes`（信令 JSON 传公钥）。
- **判定核心（纯、可单测）**：`keyBinding.ts` `decideKeyBinding(handshakePub, signalingPub|null, graceExpired) → 'accept'|'reject'|'wait'` + `KeyBindingMode`。
- **两端 provider 对称接线**：握手时经信令发本端临时公钥（host 带 cid，per-controller 独立）；`onSignal` 收对端公钥；握手完成后**先判定再标记 connected**（绑定未决期丢弃业务帧，处理信令-vs-DataChannel 到达竞态 + 宽限计时）。host 在 accept 才 `createBridge`（业务帧门控天然成立：未连上 `conn.bridge` 为 null）。
- 既有 `cloudHostBridge.verifyPeerKey` 钩子保留（更一般的注入式机制，默认 relay-trust=true），与本 provider 级信令比对并存不冲突。

### 8.3 验证（实测）
- **单测**：`keyBinding.test.ts` decideKeyBinding 5 例（一致 accept / 不一致 reject / 未到 wait / 过期回落 / 非法长度 reject）；`controllerCloudProvider.test.ts` enforced→connected + 不一致→判 MITM 断开。全绿。
- **实机 cloud e2e**（dev:cdp 真链路经本地 relay）：
  - 正路：`keyBindingMode='enforced'` + connected，dir-children 正常 → 信令公钥交换 + 比对端到端生效。
  - 反路（`tamperBinding`：host 发错误信令公钥模拟 MITM 调包）：controller 判 MITM → `connected=false` / `ctrl:error`，**拒绝会话**。

### 8.4 契约（protocol.md）待补 + relay 透明转发
B3 依赖 relay **透明转发**信令对象（`route_controller_to_host` 注入 cid 转发任意对象 / `route_host_frame` 按 cid 转发）——**已实测**：B3 经当前运行的 relay 跑通，**无需 ridge-cloud 代码改动**。但**契约文档 `docs/ridge-cloud-protocol.md` §7 应补记**新信令消息（避免将来 relay 收紧为消息类型白名单时误伤）：
- `{ t: "e2ee-pubkey", pubkey: <base64 32B>, cid? }`：controller→host（无 cid，relay 注入）/ host→controller（带 cid 定向）。各端经此旁路上报本端临时公钥，对端比对 DataChannel 握手公钥。
- 此 protocol.md 补记为**纯文档**（relay 行为不变），留待用户下次动 ridge-cloud 时一并提交（本会话遵「不动 ridge-cloud 除安全推 origin 外」未改）。
