# 会话交接 — 全本地 WebRTC cloud e2e + Dev 桌面浏览器 LAN 远控 /verify 网络错误

> 日期：2026-06-07　目标分支：`wind` develop（HEAD 当时 = `f83967c`）
> 用途：把本会话「已改未验 / 进行中 / 未做 / 新发现 bug / 运行态」一次性交接，供新会话无缝接续。
> 关联文档：`docs/plans/local-cloud-e2e-setup.md`（本地 cloud 后端起法 + B1 诊断）、
> `docs/plans/d-gm-10-e2ee-key-binding-design.md`（B3）、
> `docs/plans/unified-remote-architecture-handoff-final.md`（总路线 + D11 排期）。

---

## 0. TL;DR（最重要的三件事）

1. ~~本会话有 3 个未提交、未验证的改动（cloud 连接 scheme 本地化）~~ → **已验证并提交**（`4e2022a`，含单测 + `pnpm check` 全绿），见 §1。**提交时切记只 `git add` 指名文件，勿 `-A`**（工作树常有并发会话 WIP，如 `src/remote/lib/TerminalCanvas.svelte`、`settings.ts` 等，非本会话的，勿带上）。
2. **进行中的大任务**：用户已批准「推全 WebRTC e2e（我全包）」——在本机起完整 host↔controller 云链路，复现 B1 / 验证 B2·B3。后端已跑通，前端接线 + 种子数据 + harness 尚未写。完整下一步见 §3。
3. **用户新报 bug（最后一条消息）**：Dev 模式下用桌面浏览器做局域网远控，**输入 6 位验证码 → 报「网络错误」**。已确认**不是**本会话改动引入的回归（LAN 网关走相对路径 `fetch('/verify')`，不经 cloud apiClient）。根因未定，调查状态 + 假设见 §4。

---

## 1. cloud 连接 scheme 本地化（✅ 已验证并提交 `4e2022a`）

> 更新（会话末）：本节原为「已改未验/未提交」，现已补单测 + `pnpm check` 全绿（0 错 0 警，4497 文件）+ vitest 6 测全过，并**单独提交为 `4e2022a`**（4 文件：3 cloud 源 + `apiClient.test.ts`）。下面保留改动说明供参考；§1 的「验证欠账」已全部完成。

属于**本会话**的改动（均 cloud 连接层，目的：让 `RIDGE_CLOUD_BASE_DOMAIN` 指向本机回环时走明文 http/ws，而非 TLS https/wss，以便本地自托管 / 调试 cloud；生产域名 `remo2ridge.duckdns.org` 恒走 https/wss，不受影响）：

- **`src/lib/remote/cloud/apiClient.ts`**：新增纯函数
  - `isInsecureCloudDomain(domain)` — 判定 `localhost` / `*.localhost` / `127.0.0.0/8` / `0.0.0.0` / `[::1]`（可带端口）；
  - `cloudHttpScheme(domain)` / `cloudWsScheme(domain)`；
  - `API_BASE` 改为 `${cloudHttpScheme(BASE_DOMAIN)}://${BASE_DOMAIN}/api/v1`。
- **`src/lib/remote/cloud/ridgeCloudProvider.ts`**：`import { BASE_DOMAIN, cloudWsScheme }`；`openSignaling` 的 URL 由 `wss://` 改为 `${cloudWsScheme(this.config.baseDomain)}://`。
- **`src/lib/remote/cloud/controllerCloudProvider.ts`**：同上（controller 腿）。

**验证欠账（已全部完成）**：
- [x] `apiClient.test.ts` 已补（6 测：localhost/带端口/`x.localhost`/回环IP含裸 `::1`/公网域名/误判防护）。**单测期间发现并修复一个实现 bug**：原 `:\d+$` 端口剥离会把裸 `::1` 误删成 `::`，已改为先识别括号/裸 IPv6。
- [x] `pnpm check`（svelte-check）全绿：0 错 0 警，4497 文件。**注**：担心的 `controllerCloudProvider.ts:88` `hostDevice` TS6133 **未在项目 tsconfig 下报错**（IDE 语言服务更严，但 `pnpm check` 不拦），故未动该字段；若将来想清理，删 :88 字段 + :125 赋值即可（确认无外部读取）。
- [x] `npx vitest run src/lib/remote/cloud/apiClient.test.ts` → 6 passed。

已提交：`4e2022a feat(remote): cloud 连接按 base 域回环判定 http/ws（本地自托管/调试）`（4 文件；只 add 指名文件，未带并发 WIP）。**尚未 push origin**（本机共享 tree，另开会话可直接见本地 commit；需要远端可 `git push`）。

---

## 2. 已提交并 push（本会话此前成果，供上下文）

`origin/develop` 上本会话提交链（截至 `b052d7e`，其后 `f83967c` 为并发会话所加）：
`4277ed6, 057ed06, 748126d, 6d98e2e, 15b7571, 6d0e83c, 419908d, 4b5bb08, 8a1ada2, 550ce2a, 8fa7456, d95135e, b052d7e`。
要点：D11 设计文档 + WorkspaceGraph/PaneTree 下沉 core（156 测）、conformance describe.each（LAN+cloud，32 测）、S3 审计、B3 设计 + `keyBinding.ts` 校验器（8 测）、B2 wind 半 `cloudHostPaneSource.ts`（7 测）、B1 主机分页探针、`local-cloud-e2e-setup.md`。

---

## 3. 进行中：全本地 WebRTC cloud e2e（用户已批准「我全包」）

**目的**：在本机起完整 host↔controller 云链路，(a) 真机复现/确认 B1（dir-children 经云返回空），(b) 走通 B2/B3 的可验证部分。

**已查清的协议事实（关键，避免重复踩坑）**：
- 房间 = `Host` header 首段 label，经 `ridge-cloud/src/validation.rs::parse_host_header` 解析为 `{device,username}`（去端口、最后一个 `-` 切分）。**`{device}-{username}.localhost` 在 Chromium/WebView2 自动解析到 127.0.0.1**，故本地无需配 DNS/子域。
- WS 握手门控（`ridge-cloud/src/ws/handler.rs`）：host 需 device JWT（scope=device，claims.username==租户.username，claims.device==租户.device，且库里该 user 名下有此 device）；controller 需 user JWT（scope=user，username 匹配，**且按库实时判定 premium**——JWT 里的 plan 不算数）。
- `POST /api/v1/device/bind`（Bearer user）一步建 device 行 + 直接下发 device JWT（免 device-code 配对回环，且不要求 premium）——**取 host token 最快路径**。
- CORS = `allow_origin(Any)`（`router.rs`），故 webview 跨源 fetch 到 `localhost:5050` 可行。ICE 接口（`api/ice.rs`）只回 Google STUN；本机回环靠 host candidate 直连，STUN 不可达也不阻塞。
- 控制器 invoke 链路：`RpcClient`（`src/lib/transport/remote/rpcClient.ts`，`request/hello/notify`）→ `CloudWebrtcAdapter`（`cloudWebrtcAdapter.ts`）→ provider.sendFrame → E2EE → WebRTC → host `RidgeCloudHost`（`ridgeCloudProvider.ts`）→ `CloudHostBridge`（`cloudHostBridge.ts`，`invoke('get_directory_children', normalizeParams(params))`）→ 真 Tauri invoke。
- `get_directory_children(path, offset, limit) → DirectoryPage{entries,total,offset,has_more}`；UI 调 `invoke('get_directory_children',{path,offset,limit})`（`src/lib/stores/fileExplorer.ts:481`）。

**推荐做法：单 realm WebRTC harness**（最稳，省去双浏览器 + UI 配对）：
在 dev:cdp 的 Tauri webview 内同时实例化 host provider + controller provider（同一 JS realm，两个 RTCPeerConnection 经本地 relay 互连）；host `createBridge` 注入**真 Tauri invoke**；controller 用 `RpcClient.request('get_directory_children', {path, offset, limit})` 跑多 offset → 观察是否分页正确。这条链路真实经过 WebRTC+E2EE+mux+dispatch+真后端，直接回答 B1。

**下一步清单（按序）**：
1. **种子数据**（curl 到 `http://localhost:5050/api/v1`，默认 Host=localhost → System 路由）：
   - `POST /auth/register {email:"alice@example.com", password:"<16+位>"}` → 拿 user JWT + user.id。（无 RESEND 配置时注册即视为已验证，见 §6 日志。）
   - `POST /auth/set-username` Bearer userJWT `{username:"alice"}`。
   - premium：`docker exec ridge-pg psql -U postgres -d ridge_cloud -c "UPDATE users SET plan='premium', premium_expires_at=NULL WHERE username='alice';"`。
   - `POST /auth/login {email,password}` → **新** user JWT（带 username=alice + plan=premium）。
   - `POST /device/bind` Bearer 新 userJWT `{device_name:"mylaptop"}` → device JWT。
   - 产物：`userToken`（controller）、`deviceToken`（host）。房间 = `mylaptop-alice`，WS host = `mylaptop-alice.localhost:5050`。
2. **harness 模块**：新建 `src/lib/remote/cloud/__cloudE2eHarness.ts`（dev-only，无人 import 故生产 tree-shake 掉），导出 `runCloudDirChildrenE2E({deviceToken,userToken,username,device,path,offsets,limit})`：起 host.goOnline(device) → 起 controller adapter+RpcClient → 等 connected → `rpc.hello()` → 逐 offset `rpc.request('get_directory_children',...)` → 回收结果。import `invoke` from `@tauri-apps/api/core`（dev:cdp 是真 Tauri，非 web-remote，故是真 invoke）。
3. **跑** via CDP（9222）：`evaluate_script` 动态 `import('/src/lib/remote/cloud/__cloudE2eHarness.ts')` 并调用，断言各 offset 的 entries/total/has_more。
4. 若分页正确 → B1 结论 = 非 transport/host（与 §2 静态分析一致），bug 在 controller UI 懒加载（`fileExplorer.ts:469-491` 的 `!isTauri()` mock 分支或 :490 catch），大概率已随多次提交修复；否则就地定位。

**前置条件**：dev:cdp 必须以 `RIDGE_CLOUD_BASE_DOMAIN=localhost:5050` 启动（让全局 `BASE_DOMAIN`→本地，API_BASE + provider scheme 全转本地）；§1 的 scheme 改动也必须在（HMR 生效即可）。当前 dev:cdp（任务 `b3y4xakon`）已用该 env 起着（见 §5）。

**B2/B3 在本 harness 的边界（诚实记录）**：
- B2（终端经云）：需要 **Rust 半未写**（见 §4.B2），harness 无法验证 pane 流，只能验证 invoke 往返。
- B3（E2EE 公钥绑定）：wind 校验器已单测（`keyBinding.test.ts` 8 测）；harness 只能演示**默认 relay-trust 路径**连通（即今天生产行为），无法黑盒验证「公钥匹配」分支（controller 临时公钥每会话随机，拿不到预期值）。跨仓库 relay 半未写（见 §4.B3）。

---

## 4. 未做 / 阻塞项

### 4.1 用户新报 bug：Dev 桌面浏览器 LAN 远控 `/verify` → 网络错误（**未根因**）
- **现象**：Dev 模式，桌面浏览器做局域网远控，输入 6 位验证码点验证 → 「网络错误，请重试」（`main.remoteGateErrNetwork`）。
- **非本会话回归**：桌面 web-remote 网关 `src/routes/+layout.svelte:159-184` 的 `submitCode` 用相对 `fetch('/verify',{POST})`，`.catch` → 网络错误；不经 cloud apiClient/BASE_DOMAIN。本会话只改 cloud 文件，无关。
- **已查清**：`/verify` 是 remote-server 真实路由（`src-tauri/src/remote/server.rs:311` `verify_handler_post`），非代理到 vite。桌面 SPA 由 `web-remote-dist/`（`scripts/build-desktop-web.mjs` 产物，`server.rs:263` `desktop_dir`）提供，dev 下 remote-server 会 spawn vite。
- **待验假设**（按可能性）：
  1. **页面是从 vite dev server 直开**（如 `http://<ip>:5175`）而非 remote-server（`https://<ip>:9529` dev:cdp / `:9527` 既有实例）。主 `vite.config.js` 无 `/verify` 代理（只有 `vite.remote.config.js` 给**移动端** 5174 代理到 9527），故 `/verify` 落到 vite → 返回 HTML → `r.json()` 抛 → catch → 网络错误。
  2. **HTTPS 自签证书未信任**：remote-server 走 TLS（`server.rs:340+`，WebGPU 需安全上下文）。若浏览器未点过证书例外，同源 `fetch('/verify')` 被 `ERR_CERT_*` 拦 → catch。有 `CertTrustGuide.svelte` 提示流程。
  3. dev 下 `web-remote-dist/` 未构建 → 桌面网关根本没被正确提供（served 内容陈旧/错配）。
- **下一步诊断**（务必先拿真实失败请求，勿盲改）：
  - 问/确认用户打开的**确切 URL 与端口**（是 `https://<lan-ip>:9527|9529` 还是 `http://<ip>:5173|5175`），以及连的是哪个 ridge 实例（既有 vs 新起 dev:cdp）。
  - 让用户开浏览器 DevTools → Network 看 `/verify` 的实际状态/错误（CERT? 404? text/html?）。
  - 据此定位：若是 (1) → 引导走 remote-server origin，或给主 vite 配 `/verify` 代理（仅 dev）；若是 (2) → 证书信任引导；若是 (3) → 先 `pnpm build:desktop-web`。
- 我中断前正在读 `server.rs:215-344`（static_dir/desktop_dir 解析 + 路由装配），下一步本应读 `root_handler`/`spa_fallback_handler`(`server.rs:521-594`) 看 dev 桌面 SPA 到底是「proxy 到 vite」还是「serve web-remote-dist」，以判定假设 (1)/(3)。

### 4.2 B2 — 终端经云（Rust 半未写，阻塞）
wind 半已完成（`cloudHostPaneSource.ts` + 测，已提交）。**缺 Rust 半**：`src-tauri` 需新增 `subscribe_pane_raw`/`unsubscribe_pane_raw` 命令 + `pane-raw-{paneId}` event（复用 `RemotePtyEvent::RawBytes`，b64）。**阻塞**：要改 `src-tauri/src/commands/remote.rs`，是并发会话 WIP，勿与其抢改；需协调或等其落地。

### 4.3 B3 — E2EE 公钥↔身份绑定（跨仓库半未写，阻塞）
wind 半已完成（`keyBinding.ts` + `cloudHostBridge.verifyPeerKey` 接入点 + 8 测，已提交）。设计见 `d-gm-10-e2ee-key-binding-design.md`（认证信令 relay 回传对端公钥确认，无需新非对称密钥）。**缺 ridge-cloud 半**：`/ws` 把对端握手公钥经带外通道回传 + 协议文档。**阻塞**：用户在改 `ridge-cloud` 的 `docs/ridge-cloud-protocol.md`（域名迁移 WIP），勿碰；需等其稳定。

### 4.4 C — S6 部署；D — 审计
均未开始（属总 /goal 的 C/D）。S6 见总路线图。

---

## 5. 运行态（本会话起的进程；新会话可复用，收尾需清理）

| 进程 | 端口/标识 | 说明 |
|---|---|---|
| docker `ridge-pg` | :5433（postgres 15.8 supabase 镜像） | db `ridge_cloud`，超级用户 `postgres`/`ridge`。起着。 |
| ridge-cloud（dev cargo run） | :5050 | env：`DATABASE_URL=postgres://postgres:ridge@localhost:5433/ridge_cloud JWT_SECRET=0123456789abcdef0123456789abcdef0123456789abcdef LEMON_SQUEEZY_SECRET=dummy_local_dev_secret BASE_DOMAIN=localhost PORT=5050`。`/healthz`→200。 |
| dev:cdp（任务 `b3y4xakon`） | vite :5175 / remote-server :9529(HTTPS) / CDP :9222 | 以 `RIDGE_CLOUD_BASE_DOMAIN=localhost:5050 CDP_PORT=9222 node scripts/tauri-dev-cdp.mjs` 起；启动时 remote 已 enabled。**这是 e2e harness 的 host webview。** |
| **既有/正式 ridge（托管本 CC 会话）** | remote-server :9527 / vite :5173,5174 | **勿杀**——杀了本会话就断。 |

清理（确认不再需要时）：`docker rm -f ridge-pg`；停 :5050 cargo run；停 dev:cdp 任务 `b3y4xakon`。

---

## 6. 注意事项（踩过的坑）

- **共享 working tree + 并发会话**：本机 develop 常多会话共用一个 tree。HEAD 已被并发会话推进到 `f83967c`。提交前务必核对 HEAD；优先**新建 commit 非 amend**；`git add` 指名道姓（勿 `-A`，会带上 `src/remote/lib/TerminalCanvas.svelte` 等并发 WIP）。force 用 `--force-with-lease`。见记忆 `feedback_shared_tree_git_amend`。
- **勿碰的并发 WIP 文件**：`src/remote/lib/TerminalCanvas.svelte`（当前未提交，非本会话）；`src-tauri/src/commands/remote.rs`、`terminal.rs`、`scripts/remote-*.mjs`（历史并发热点）。
- **ridge-cloud**：本会话只读未改，工作树 clean（develop）。用户曾有域名迁移 WIP（`config.rs`/`router.rs`/`docs/ridge-cloud-protocol.md`），勿碰。
- **rustfmt**：整 crate `cargo fmt` 会污染无关文件；只跑单文件 `rustfmt --edition 2021 <file>` 或 `--check`。
- **后端 rebuild**：改 `src-tauri` 后真正生效要 rebuild + 重启 ridge（会杀正式版/会话）；用 dev:cdp 独立实例验证（见记忆 `feedback_self_verify_via_cdp` / `env_cdp_dev_testing`）。
- **本机 PostgreSQL 18 安装损坏 + 网络限速**：别再尝试本地装 PG / 下二进制；用 docker（已就绪）。

---

## 7. 关键文件锚点

- 桌面 web-remote 网关（/verify bug）：`src/routes/+layout.svelte:159-184`（submitCode）、`:194-219`（cloud TOTP）。
- 移动端验证：`src/remote/AuthScreen.svelte:29-47`。
- remote-server 路由 + /verify + 静态目录：`src-tauri/src/remote/server.rs:304-333`（router）、`:311`(/verify)、`:235-302`(static_dir/desktop_dir)、`:521-594`(spa_fallback)、`:715-760`(verify handlers)。
- dev 代理（移动端 only）：`vite.remote.config.js:107-119`。dev:cdp 启动器：`scripts/tauri-dev-cdp.mjs`。
- cloud scheme 改动：`src/lib/remote/cloud/apiClient.ts`（helper + API_BASE）、`ridgeCloudProvider.ts:33-34,366-368`、`controllerCloudProvider.ts:40-41,245-247`。
- cloud e2e 链路：`ridgeCloudProvider.ts`、`controllerCloudProvider.ts`、`cloudHostBridge.ts`、`cloudWebrtcAdapter.ts`、`rpcClient.ts`、`cloudControllerBoot.ts`、`e2ee.ts`。
- ridge-cloud：`src/ws/handler.rs`（门控）、`src/validation.rs`（租户解析）、`src/api/device_routes.rs:176`（/device/bind）、`src/auth/jwt.rs`、`src/router.rs`（CORS/路由）、`src/api/ice.rs`。

---

## 8. 终态总结（会话末，2026-06-07 —— A/B/C/D 推进）

### 本会话提交（均本地，未 push origin 除非注明）
**wind develop**（链：`...→4e2022a→680eab7→79b0bcb→a44a982→89b58ae→2ef0771`，叠在并发会话 commit 之上）：
- `4e2022a` cloud 连接按 base 域回环判定 http/ws（+单测，scheme 使能）
- `79b0bcb` 远控+云链路安全审计（D，三层多视角）
- `a44a982` 本地 cloud WebRTC e2e harness（`__cloudE2eHarness.ts`+`cdp-cloud-seed.mjs`+`app.html` CSP 放行 localhost）
- `89b58ae` **LAN 远控加固**（C1 /verify 爆破节流+C2 OsRng+H3 /workspace 鉴权+C4 /file 收敛）—— 来自 agent，已审阅+`cargo check --all-targets` 0/0
- `2ef0771` **云桥命令白名单门控**（堵审计①-1 远程 RCE）+ `remoteAllowlist.ts`（镜像 capability.rs）+单测
- 交接文档若干（含本文件）

**ridge-cloud**：安全修复**仅推 origin 分支** `security/pre-deploy-2026-06-07`（`f4ebfb0`，C-2 配对码锁定/H-1 房间按user_id/H-3限流/H-4/M-1，52 测绿）。**未部署、未并入 origin/develop、本地 develop 已还原回用户域名迁移 tip `350e7fc`**（用户选「只推 origin 不部署」）。

### A/B/C/D 状态
- **A 协议收敛/D11** ✅ 完成（早先）。
- **B 云完成**：
  - **B1** ✅ **实机证伪**：单 realm WebRTC harness 跑通真云链路，`get_directory_children` 经云分页正确（total=92），非云层 bug（疑 UI 懒加载已修）。
  - **B2**（终端经云 Rust 半 `subscribe_pane_raw`）❌ **未做**——需改 `src-tauri/src/commands/remote.rs`（并发会话 WIP），须隔离/协调。wind 半 `cloudHostPaneSource.ts` 早已提交。
  - **B3**（E2EE 公钥↔身份绑定）✅ **已实现 + 实测验证**（commit `7a9199a`）。认证信令旁路确认：两端经信令互报临时公钥→比对 DataChannel 握手公钥，不一致即判 MITM 拒绝。启用门改用「信令公钥到达性」(非 $/hello，后者握手后才发太晚)，宽限期回落 relay-trust 兼容旧端。**relay 透明转发，ridge-cloud 零代码改动**（已实测经运行中 relay 跑通）。实测：正路 `keyBindingMode=enforced` 连通；反路(篡改信令公钥)→拒绝断开。全绿(vitest 690)。详见 `d-gm-10-e2ee-key-binding-design.md` §8。**唯一文档欠账**：`ridge-cloud/docs/ridge-cloud-protocol.md` §7 应补记 `e2ee-pubkey` 信令消息（纯文档，relay 行为不变，留待动 ridge-cloud 时）。
- **C S6 部署**：按用户选择 = **只推 origin 不部署**（已完成）。真正上线 dokku 由用户择机：⚠️ 部署 develop 会**连带域名迁移**（`b200a8e` duckdns→9527127.xyz）上 prod，须先确认 9527127.xyz DNS/TLS/dokku BASE_DOMAIN 就绪；或只部署 `security/pre-deploy-2026-06-07`（已在 origin，基于当前 prod 基线，cherry-pick 干净 + cargo check 绿）。
- **D 审计** ✅ 完成 + **①-1 实测坐实**（get_remote_info 经云泄露宿主 TOTP 密钥）→ 已修（`2ef0771`，实测修复后被拒）。

### 安全修复落地路径（重要）
- **桌面侧修复**（LAN 加固 `89b58ae` + 云桥白名单 `2ef0771`）随**桌面 app 发布**生效——用户需 rebuild/分发安装版才在真机 host 生效（dev:cdp 已实测云桥修复有效）。
- **ridge-cloud 安全修复**在 `origin/security/pre-deploy-2026-06-07`，待用户决定如何部署（见 C）。

### 运行态（会话末仍在跑；下次可复用/清理）
- docker `ridge-pg`（:5433）、ridge-cloud dev（:5050，**旧码**，无安全修复，仅供 harness）、dev:cdp（任务见会话日志，:5173/CDP9222，已含云桥修复经 HMR）。docker pg 按用户要求保留。
- 清理：`docker rm -f ridge-pg` + 停 :5050 + 停 dev:cdp。

### 复现 B1 / RCE 验证
`node scripts/cdp-cloud-seed.mjs`（:5050+docker 在跑）→ 取 user/device JWT → dev:cdp(CDP9222) `import('/src/lib/remote/cloud/__cloudE2eHarness.ts')` 调 `runCloudDirChildrenE2E({...,exploit:{method:'get_remote_info'}})`。前置：dev:cdp 以 `RIDGE_CLOUD_BASE_DOMAIN=localhost:5050` 起 + `app.html` CSP（已提交）。
