# 远控 + 云链路安全审计（2026-06-07）

> 范围：本会话「全本地 WebRTC e2e」涉及的整条远控攻击面，分三层多视角审计（security-reviewer agent ×3，read-only）：
> ① 云 WebRTC + E2EE 客户端层（wind TS）；② ridge-cloud 信令 relay + 鉴权（Rust）；③ 局域网 remote-server（wind src-tauri，Rust）。
> 这是 /goal 的 **D（审计）** 交付物。**本审计为只读分析，未改任何代码。**
> ⚠️ 标 CRITICAL 的几项请在「上线/分发前」务必人工复核并修复——尤其 ②③ 的 LAN/配对面。

## 0. 总体结论（最重要）

整条链路的密码学原语（E2EE：X25519+HKDF-SHA256+ChaCha20-Poly1305、方向分离 nonce、严格递增 counter 防重放、counter 溢出前重连）**实现正确**；ridge-cloud 的 JWT alg-confusion 已堵、premium 门控 DB 权威、SQL 全参数化、租户 join 鉴权正确——这些是 PASS。

但**授权边界**在三层都有严重缺口，按风险从高到低：
1. **LAN remote-server 是最危险面**：仅靠一个 6 位 TOTP 把全机 shell/文件控制暴露给局域网，而该 TOTP **无任何爆破防护** + **密钥用弱 PRNG 生成**（可预测）→ 敌意局域网下数分钟内可拿到 shell。叠加 `/file` 任意绝对路径读 + `/workspace/*` 完全无鉴权。
2. **云桥越过了 LAN host 的命令白名单**：controller 的 JSON-RPC method 直接进 Tauri `invoke`，**无白名单/无只读门/无路径穿越防护/无能力集**——握手+TOTP 后即等价远程 RCE，比 LAN 面更宽。
3. **配对码可跨用户爆破** → 把受害主机绑到攻击者账号（设备/配对劫持）。
4. **E2EE 身份绑定（D-GM-10）目前在生产里是空跑**（验证器写了、测了，但未接线、且只单边）——relay 被攻陷即可 MITM。

## 1. ① 云 WebRTC + E2EE 客户端层（wind TS）

| # | 级别 | 发现 | 位置 | 状态 |
|---|---|---|---|---|
| 1 | **CRITICAL** | 云桥把 controller 任意 method 直送 Tauri `invoke`——**无白名单/只读门/`..`防护/能力集**，与其声称对齐的 `server.rs dispatch_invoke_request` 边界不符 → 远程 RCE（可读 TOTP 密钥、enter_deep_root_mode、任意 fs/shell） | `cloudHostBridge.ts:425`、`RemotePanel.svelte:266`；对比 `server.rs:2386,2895` | NEW |
| 2 | HIGH | E2EE 公钥↔身份绑定（D-GM-10/§5.5）：验证器正确且单测，但**生产未接线**（`RemotePanel.svelte:264` 未传 `keyBindingVerifier`）、**仅 host 单边**（controller 无钩子）、host 失败路径在 teardown 前给 `conn.bridge` 赋了被拒桥（脆弱） | `RemotePanel.svelte:264`、`ridgeCloudProvider.ts:301-309`、`controllerCloudProvider.ts:195-205` | KNOWN+NEW |
| 3 | MEDIUM | TOTP 门控对 invoke/pane 有效，但 `totp-verify` **无限次重试**（±90s/6 位，可经 CONTROL 通道爆破，无锁定）；`pushPaneOutput` 只查 `rejected` 不查 `verified` | `cloudHostBridge.ts:243,271-292,486`；`auth.rs:32-49` | NEW |
| 4 | MEDIUM | 解密后 `JSON.parse`/`TextDecoder` **无帧大小上限** → 连上的对端发超大帧 OOM/卡死 UI 线程 | `cloudMux.ts:108-138`；两个 provider 的 onDataChannelMessage | NEW |
| 5 | LOW | `isInsecureCloudDomain`（本会话新增）对 `localhost.evil.com`/`127.0.0.1.evil.com` 等**不可被绕过降级**；且 `BASE_DOMAIN` 是构建期常量、非运行时注入 → **判定安全**。仅建议：若将来 base 改成运行时可配，需加 `@`/多冒号拒绝 + 用 `import.meta.env.DEV` 闸门 | `apiClient.ts:33-49` | NEW（已验证安全） |
| — | PASS | E2EE crypto（nonce 方向分离/counter 严格递增防重放/溢出重连/HKDF/AEAD）正确；rpcClient 关联与重连拒绝正确 | `e2ee.ts:109-203`、`rpcClient.ts` | PASS |

## 2. ② ridge-cloud 信令 relay + 鉴权（Rust）

| # | 级别 | 发现 | 位置 | 状态 |
|---|---|---|---|---|
| C-2 | **CRITICAL** | 配对码激活**跨用户可寻址** + 仅 General 限流 + 码空间因并发活跃码而缩小 → 攻击者扫 `/device/activate` 命中即把受害主机绑到**攻击者账号**（设备/配对劫持） | `device_routes.rs:88-170`、`router.rs:91-93`、`crypto.rs:51` | NEW |
| H-1 | HIGH | room key 用 `{device}-{username}` 拼接、**未按 user_id 命名空间**；当前靠「username 全局唯一且无连字符」不变量才不冲突，一旦放宽即跨租户房间劫持 | `rooms.rs:89-91` | NEW |
| H-2 | HIGH | cid 用**全局顺序** AtomicU64 → 跨租户连接计数侧信道 + 可猜 | `rooms.rs:29-34`、`handler.rs:415-459` | NEW |
| H-3 | HIGH | `/device/poll`（签发 180 天 device token）仅 General 限流（`/auth/poll` 已在严格档，二者不一致） | `device_routes.rs:46-86`、`router.rs:92` | NEW |
| H-4 | HIGH | `/ws` 在取并发许可**前**先做 JWT verify + 1~2 次 DB 查询 → 单 token 多 IP 制造 DB 放大 DoS | `handler.rs:95-120` | NEW |
| H-5 | HIGH | CORS `allow_origin(Any)` + bearer：无 CSRF（无 cookie），但任意 web 源可用泄露的 token；建议白名单本产品源 | `router.rs:118-122` | NEW |
| M-1 | MEDIUM | WS `extract_client_ip` 信任 XFF 最左（可伪造）→ 会话列表 IP 可被伪造误导 owner（rate_limit.rs 反而优先 X-Real-IP，二者不一致） | `handler.rs:147-169` | NEW |
| M-2 | MEDIUM | JWT 无 `jti`/吊销；180 天 device token 除删设备外不可提前失效 | `jwt.rs:90-158` | NEW |
| M-3 | MEDIUM | `/auth/register` 返回 `EmailTaken` → 邮箱枚举 oracle | `auth_routes.rs:58-60` | NEW |
| M-4 | MEDIUM | 匿名 `/device/code` 可刷量、放大 C-2 搜索空间 | `device_routes.rs:28-43` | NEW |
| — | PASS | JWT alg-confusion/`none` 已堵；premium DB 权威；SQL 全参数化；租户 join 鉴权（username==tenant + 设备归属 + scope 绑定）正确；pairing bind 原子防双绑；WS permit RAII 正确 | 多处 | PASS |

## 3. ③ 局域网 remote-server（wind src-tauri，Rust）

> 威胁模型：服务 bind `0.0.0.0`，把整机控制（shell stdin、任意文件读写、git、起进程）暴露给局域网，仅 6 位 TOTP 把关。按**敌意局域网**（咖啡馆/会议/宿舍/被攻陷 IoT）评估。

| # | 级别 | 发现 | 位置 | 威胁模型 |
|---|---|---|---|---|
| C1 | **CRITICAL** | `POST /verify` **无爆破防护**（无失败计数/锁定/限流，黑名单需手动）；6 位码 + ±1 窗口（同时 3 码有效）→ 局域网内数秒~数分钟穷举 | `server.rs:722-755`、`auth.rs:33-49` | 敌意 LAN |
| C2 | **CRITICAL** | TOTP 密钥 + 会话 token 用**非加密 PRNG**（`SimpleRng` xorshift，种子 `nanos^pid` 低熵）→ 可预测/可伪造（推种子算所有码，或观测 token 反推状态伪造 token 绕过 TOTP） | `auth.rs:71-82,171-179,206-240` | 敌意 LAN |
| C3 | **CRITICAL** | 已认证 controller = 完整 RCE + 任意绝对路径文件读写；remote 能力集**空 roots（沙箱关闭）**，只挡 `..` 不挡绝对路径；**只读模式不挡 shell stdin/write_to_pty**（即只读≈无隔离） | `server.rs:2511-2814,1536-1548`、`core_bridge.rs:96-115`、`capability.rs:144` | 双（设计） |
| C4 | **CRITICAL** | `/file` 凭 token 读**任意绝对路径**文件（仅挡 `..`，无 canonicalize containment）；token 走 query string（进日志/历史） | `server.rs:601-632` | 双 |
| H1 | HIGH | TLS 失败**静默降级明文 HTTP** → 码/token 明文过局域网，可被嗅探重放 | `server.rs:340-379`、`tls.rs:95-126` | 敌意 LAN |
| H2 | HIGH | mDNS **持续广播**控制端点到全网段（攻击者无需扫描） | `mdns.rs:15-115` | 敌意 LAN |
| H3 | HIGH | `/workspace/{list,switch,create,close}` **完全无 token 鉴权**（仅受 `remote_enabled` 闸）→ 任意局域网peer 可枚举/销毁/篡改工作区（无认证 DoS+数据丢失） | `server.rs:320-323,845-942` | 双 |
| H4 | HIGH | 无默认拒绝的 token 中间件，鉴权靠各 handler 手动检查（H3 即由此产生）；`.fallback` 未被 route_layer 包裹（已自闸 remote_enabled，未发现实际逃逸，但结构脆弱） | `server.rs:324-333,521-588` | 双 |
| H5 | HIGH | 会话 token 3 天 TTL、不绑设备/IP、无轮换 | `auth.rs:158-204`、`server.rs:778-779` | 双 |
| M1 | MEDIUM | 公开 `/info` 泄露 lan_ip+hostname（**已正确不含 TOTP 密钥**） | `server.rs:697-706` | 敌意 LAN |
| M2 | MEDIUM | 所有响应**无安全头**（HSTS/nosniff/X-Frame/CSP）→ 可点击劫持 + 无 HSTS 防降级 | `server.rs:462-695` | 双 |
| M3 | MEDIUM | `/verify` 区分「黑名单」vs「码错误」→ 轻微枚举 | `server.rs:734-754` | 敌意 LAN |
| — | PASS | `REMOTE_ALLOWLIST` 正确排除 host 特权命令（get_remote_info/set_remote_enabled/deep_root/黑名单），有测试；constant_time_eq 正确；resize clamp 合理 | `capability.rs:153-250` | PASS |

## 4. 修复优先级建议（不在本会话执行；留给后续）

1. **LAN C1+C2（最高、且改动小且局部）**：`/verify` 加 per-IP+per-device 失败锁定 + 全局验证限流（同时覆盖 `/ws ?code=` 路径）；TOTP 密钥与 token 改用 `OsRng`/`getrandom`（`tls.rs` 已有 ring 依赖）。
2. **LAN H3**：把 `/workspace/*` 移到 token 鉴权后（或并入已认证 `/ws`）。
3. **LAN C4 / C3**：`/file` 加 canonicalize+containment 到 workspace roots；把现成的 `RootScope` 沙箱接进 `core_bridge::remote_ctx`（`with_roots`）。
4. **云 ①-1（CRITICAL）**：云桥的 invoke 改为走与 LAN 同一 Rust 边界（新增一个内部调 `dispatch_invoke_jsonrpc` 的命令），一处同时获得白名单+只读+穿越+能力集；并加 per-cid TOTP 锁定。
5. **cloud ②-C-2**：`/device/*` 全移到严格限流档 + 配对码失败锁定 + 主机侧确认激活账号后再发 device JWT。
6. **②-H-1/H-2**：room key 改用 `user_id`+device（用设备名禁用的分隔符如 `/`）；cid 改 per-room 随机。
7. 其余 HIGH/MEDIUM 见各表。

## 5.5 实测验证（本会话单 realm WebRTC e2e harness）

用 `src/lib/remote/cloud/__cloudE2eHarness.ts` + `scripts/cdp-cloud-seed.mjs` 在 dev:cdp
真 Tauri webview 里跑通了完整云链路（host+controller 同 realm，经本地 ridge-cloud relay
互连），结论：

- **B1（dir-children 经云返回空）= 证伪**：`get_directory_children` 经云分页**完全正确**
  （offset 0/3/6 各返回不同条目 .baseline/.codegraph/.kiro，total=92，has_more=true）。
  叠加 connected=true + D9 能力协商 `[pane,invoke,fs,git,search,workspace,theme]`。
  → 「空」不在 host/transport/E2EE/dispatch，是 controller UI 懒加载窄边角（疑已修）。
  这是**整条云栈 + scheme 改动 + CSP 放行的首次端到端实跑验证**。
- **①-1（CRITICAL，云桥无白名单）= 实测确认**：controller 经云调 `get_remote_info` →
  host **原样返回 LAN 远控 TOTP 密钥**（`otpauth://...secret=2S37Z5RT44AKGY3IUC7T2RGVC3PMOYEF`）。
  即云控制端可读取宿主的 LAN 配对密钥（→ 推导所有 LAN 远控码）并可调任意命令（RCE）。
  这条 LAN allowlist 明确排除的命令，经云桥畅通无阻 → **必须在分发前修复**。
- 顺带实测确认：审计列的 E2EE crypto / D9 协商 / mux / relay 路由 / premium 门控（DB）
  在真链路上工作正常（PASS 项得到运行时背书）。

复现：`node scripts/cdp-cloud-seed.mjs`（起 premium 用户+设备，:5050 须在跑）→ 取 token →
CDP `import('/src/lib/remote/cloud/__cloudE2eHarness.ts')` 调 `runCloudDirChildrenE2E(...)`
（带 `exploit:{method:'get_remote_info'}` 即复现 RCE 验证）。前置：dev:cdp 以
`RIDGE_CLOUD_BASE_DOMAIN=localhost:5050` 启动 + app.html CSP 已放行 localhost。

## 5. 复核提示

- 本审计由 agent 多视角生成，CRITICAL 项（尤其 ①-1 云桥无白名单、③-C3 只读不挡 stdin）建议人工对照源码二次确认后再动手——这些断言已交叉引用 `server.rs`/`capability.rs`/`dispatch.rs`，可信度较高，但修复前值得 5 分钟复核。
- 三个 agent 句柄（如需追问继续）：① `a6c7377c7f0b05cc0`、② `a5fdd135868a4bcb6`、③ `a4b7972576856979f`。
