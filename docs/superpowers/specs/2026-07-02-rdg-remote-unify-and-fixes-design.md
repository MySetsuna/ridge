# rdg 远控统一 + 五项故障修复（设计）

> 目标（用户原话汇总）：当前 `rdg` 不可用，需修复 5 个问题；核心诉求是把远控**收敛到一份代码**：
> 把 `src-tauri/src/remote/`（桌面完整实现）迁入 `packages/ridge-remote`，做好兼容后**删除** `src-tauri/src/remote/`；
> 让 **tui lan（rdg）/ 桌面 lan（src-tauri）/ 云端 relay（ridge-cloud）** 三形态、以及**控制端两形态（手机浏览器 / 桌面浏览器）**都用同一份代码。
> 用户倾向用工作流（多 agent 并行诊断 + 落地）推进。

状态：活文档。诊断由后台工作流并行产出（membership 已确认；remote-unify/tui-logspam/browser-login/connect-fail 串行重跑中）。本文随诊断结果补全后再 commit、再实施。

---

## 0. 现状分析（已核实）

### 0.1 远控的三形态与两套后端实现

| 形态 | 后端实现 | 前端 | 协议 |
|---|---|---|---|
| 桌面 app LAN host | `src-tauri/src/remote/`（完整：server ~1400 行 + auth/mdns/core_bridge）| serve 真 SPA：`static/remote`(移动)+`web-remote-dist`(桌面)，UA 分流 | `ridge-remote-ws`（mux + JSON-RPC 2.0）|
| **rdg TUI LAN host** | `packages/ridge-cli/src/tui/lan_host.rs`（**独有简化实现**）| **内联手写 HTML**（`LOGIN_HTML`/`TERMINAL_HTML`，CDN xterm.js）；`static_dir` 死代码 | **`ridge-lan-ws`（私有简化协议）** |
| 云端 relay | ridge-cloud（`spa_fallback` serve 同步来的 wind 前端）| 同一套 SPA（`sync:cloud-controller`）| `ridge-remote-ws` |

- 共享 crate `packages/ridge-remote` 目前只承载 **TLS 证书 + bind/serve 生命周期 + UA 分流(ua.rs)**；`server::serve` 只是把调用方的 axum `Router` 套上 TLS/mDNS 外壳。桌面 `server.rs` 与 rdg `lan_host.rs` 都调它，但各自塞进去的 `Router` 完全不同。
- **关键结论**：rdg LAN 的"独有构建" = `lan_host.rs` 自带内联 HTML + 私有 `ridge-lan-ws` 协议，与桌面 SPA + 统一协议不兼容。这是 P1 要消灭的对象，也很可能是 P5 连接失败的根源之一。

### 0.2 控制端（前端）两形态
- 源码一份：`src/remote/`（Svelte，含移动组件 `VirtualKeyboard`/`BottomTabBar`/`keyboardOffset`/`CloudAuthScreen`）。
- 两套构建产物：移动 SPA `pnpm build:remote`(vite.remote.config.js)→`static/remote/`；桌面 SPA `pnpm build:desktop-web`(scripts/build-desktop-web.mjs)→`web-remote-dist/`。
- UA 分流 SSOT 已在 `ridge_remote::ua::prefer_desktop_ui`（桌面 `server.rs` 已用；rdg `lan_host.rs` 未用；ridge-cloud `spa_fallback` 应复用）。

---

## 1. 五项故障：根因与修复方案

### P4 会员等级误判（会员 → 体验会员）— 根因已确认（前后端双因）
- **后端主因**（ridge-cloud `db/user_repo.rs::checkin_grant_2h`，284-307）：`decided` CASE 只跳过 `already_today` 与 `already_permanent`(`plan='premium' AND premium_expires_at IS NULL`)；**时限付费会员**（plan='premium' 但到期为未来）签到落入 `ELSE 'granted'`，被 `SET is_trial=true` 污染 → 之后 `is_real_premium()=false`、DTO 输出 `premiumActive=true & isTrial=true & isRealPremium=false`。
- **CLI 次因**（wind `login_flow.rs:34-44,269-282`）：`UserBrief` 只解析 `premiumActive`/`isTrial`，用 `premiumActive && isTrial → TRIAL` 判定，**忽略后端专门提供的权威字段 `isRealPremium`**（dto.rs:53-60）。
- **修复**：
  1. ridge-cloud `checkin_grant_2h`：跳过分支扩为"当前有效付费会员"——`WHEN plan='premium' AND (premium_expires_at IS NULL OR premium_expires_at > now())`，时限会员签到不再置 trial。
  2. ridge-cloud 新增数据修复迁移：`UPDATE users SET is_trial=false WHERE plan='premium';`（清历史污染）。
  3. wind `login_flow.rs`：`UserBrief` 增 `#[serde(rename="isRealPremium", default)] is_real_premium: bool`；判定改 `if is_real_premium {PREMIUM} else if premium_active {TRIAL} else {FREE}`；同步更新单测。

### P2 TUI 日志冲刷界面 — 根因已确认
- `main.rs::init_tracing()`（279-288）无条件把 tracing 以 `info` 级写 `stderr`；TUI（`dashboard.rs::run` 141-143 进 `EnterAlternateScreen`+raw mode）存活期间，LAN host `tokio::spawn` 里 `tracing::warn!`（dashboard.rs:197）、daemon task `eprintln!`（dashboard.rs:295）、`lan_host.rs` 的 `tracing::info!`（62,99）等都直接糊屏。
- **修复方向**：`init_tracing` 按运行模式分流——TUI/dashboard/connect 等交互模式把 tracing 重定向到**文件**（如 `~/.config/ridge/rdg.log`）或 TUI 内置日志面板（dashboard 已有 `log_lines`/Log 区，可接一个 channel MakeWriter）；daemon/tmux/非 TTY 保持 stderr。消除 TUI 期间的裸 `eprintln!/println!`（改走 app.log 或文件）。
- 详细写点清单：待 tui-logspam agent 补全。

### P3 TUI 浏览器登录结果无法回传（WSL）— 根因已确认
- rdg **未接入**后端的浏览器授权登录流：`packages/ridge-cli/src` 对 `/auth/request`、`authorize_url`、`/auth/poll`、`/auth/approve` **零命中**。
- 现状：`login_flow.rs::print_login_banner`（256-263）打印"在浏览器打开 `https://{base}/login` 登录"诱导用户，但 rdg 实际只在 stdin 等邮箱密码；dashboard 的 "Login" 菜单也只调 `login_flow::run_login`（stdin）。浏览器登录的 session **无任何回传通道** → "浏览器登录了但结果没回 TUI、没带 ticket/token"。WSL 只是让缺陷更明显（宿主浏览器与 WSL CLI 本就无通道）。
- 后端已有纯轮询授权流（`dto.rs:265-308`）：`POST /auth/request{client}` → `{request_code, poll_token, authorize_url, ...}`；用户浏览器打开 authorize_url 登录并 `/auth/approve`；CLI 轮询 `/auth/poll{poll_token}` 直到 `approved` 拿 user JWT。**不依赖 localhost 回调 → WSL 友好。**
- **修复方向**：rdg 新增 `browser_login`（仿 `device_flow` 的轮询 UX）：调 `/auth/request(client=cli)` → 打印/展示 authorize_url（可选 QR）→ 轮询 `/auth/poll` → 拿 user JWT 后接入现有 set-username/device-bind。TUI 明确展示 URL 与状态。stdin 密码登录保留为 fallback。
- 契约细节（authorize_url 是否带 request_code、approve 是否需登录态）：待 browser-login agent 读 ridge-cloud `auth_routes.rs`/`auth_request_repo.rs` 补全确认。

### P1 remote 统一 — 迁移蓝图（整合 remote-unify 诊断，high 置信）
- **背景**：这是既有『统一远控 S0–S8』大计划的收尾（见 `docs/plans/unified-remote-architecture-handoff-final.md`、`unified-remote-gap-analysis.md`）。ridge-core 地基(S1)/协议骨干(S3)/headless MVP(S5) 已完成；本 P1 = 桌面 host 后端下沉共享 + 删除旧址。规模：`src-tauri/src/remote/server.rs` **3616 行** + auth 658 + mdns 160 + core_bridge，**~130 处 `ctx.state.*` 耦合 AppState**，invoke 分发直调 `crate::commands::*`（仅 11 项已下沉 core，其余属既有 gap **G4**）。
- **trait 边界（`RemoteHost` 组）**：`WorkspaceProvider`/`PaneProvider`/`InvokeDispatcher`/`EventBus`/`HostAuth`，server 泛型化 `Arc<dyn RemoteHost>`。桌面用 `AppState` 实现（InvokeDispatcher 保留 Tauri 命令腿 = **路线 B**）；rdg 用 `SharedWorkspace` headless 实现（InvokeDispatcher 仅 `ridge_core::dispatch` 白名单子集，未迁 method 返回 MethodNotFound 由 controller 按 capabilities 灰掉）。
- **分阶段（每步 `cargo check` 绿 + 桌面 LAN 回归不破）**：
  1. `auth.rs` 迁 ridge-remote（SessionStore/VerifyThrottle/RemoteAuth，无 Tauri 依赖，含单测）；`state.rs` 改引用。
  2. `mdns.rs` + `net.rs`(detect_lan_ip/s) 迁 ridge-remote；rdg `config::detect_lan_ip` 统一到 `net`。
  3. `serve.rs` 下沉：`UaServeConfig{mobile_dir, desktop_dir:Option}` + `resolve_ui_dirs` + serve_index/spa_fallback/assets/ca + security_headers/compression（UA 分流用 `ridge_remote::ua`）。
  4. `RemoteHost` trait + `server_app.rs`（路由/verify/ws/workspace/file/handle_ws 泛型化）。
  5. 桌面 `remote_host_impl.rs` 实现 RemoteHost（转发 AppState + `dispatch_invoke_*` 搬来 + core_bridge 依赖 AppHandle 留 src-tauri）。
  6. rdg `lan_host_impl.rs` 实现 RemoteHost + 重写 `lan_host.rs`（构造 `Arc<dyn RemoteHost>`+UaServeConfig 调 `server_app::run`，删内联 HTML/手写 WS）；产物获取 = 运行时目录探测 `web-remote-dist`（缺失回退移动）+ 构建脚本铺产物（否决 include_dir 嵌入，~20MB 体积）。
  7. 回归全绿后**删除** `src-tauri/src/remote/`，改引用点（lib.rs / commands/{remote,fs_watch,settings} / state.rs / bin/remote-server.rs——bin 陈旧 stub 优先删）。
  8. ridge-cloud：`ua.rs` 逐字镜像改依赖 `ridge_remote::ua`（发 git crate，仿 ridge-signaling）；`static_host`/`router` 因 Docker 够不到 wind 仓，短期保留自有 serve（额外承担 Host-label/租户/PWA sw.js），仅共用 ua + 纯函数。
- **决策点（实施前需定）**：InvokeDispatcher 路线 A(先完 G4) vs **B**(trait 抽象增量收口，推荐)；rdg 产物分发形态（单 exe 带资源目录）；rdg 单 workspace 下 WorkspaceProvider switch/create/close 语义；ridge-remote 是否发 git crate；attach 屏幕快照(D10)/scrollback 是否补(G5b)。
- 完整逐文件 fixPlan 见诊断输出（`tasks/wawp4bw8q.output` / journal `wf_9f60a07c-7c9`）。

### P5 rdg 连接失败 — 根因（inline 补诊断）
- **协议分叉（核心，与 P1 同源）**：rdg 的 LAN host(`lan_host.rs`) 与 LAN controller(`lan_session.rs`) 都用私有 `ridge-lan-ws`（list-panes/subscribe-pane/stdin/claim-pane，text JSON + 16B 前缀二进制帧，`lan_proto.rs`），而桌面 host(`server.rs`) 用统一 `ridge-remote-ws`(mux+JSON-RPC)。rdg controller 连桌面 host 时协议不匹配（连上但无数据/黑屏）。**P1 统一后从根消除**。[实施 P1 前读 `server.rs` handle_ws 确认桌面是否也兼容 subscribe-pane 协议（移动 SPA 可能用它），以决定 controller 侧改动幅度]
- **端口（已核实非 bug，撤销原判断）**：`connect_lan`(`lan_session.rs:162-166`) 缺省端口 `9527` 是**对的**——`rdg connect` 连的是**桌面 host**（`main.rs:73` 明确），桌面 `bind_tcp(9527)` 固定（`src-tauri/.../server.rs:202`，dev/prod 都 9527）；`config::lan_port()`(dev=5002) 是 rdg 自身 lan_host 的端口，不适用于连桌面。故不改 `lan_session.rs`。
- TLS 自签已接受（`AcceptAnyServerCert` `lan_session.rs:33-93`），wss 失败回退 ws；端口对了 TLS 不是障碍。
- 云端连接(daemon/rtc/signaling)：daemon host 已用 mux/JSON-RPC 收敛桌面同款（见 main.rs 头注释）；若连接失败在云端，与订阅门控(P4——但 `can_use_remote()` 对未过期 trial 也 true，未必拒)相关，待真机验证。

---

## 2. 实施顺序（定稿）

**阶段一：独立低风险修复（主 loop 即时落地，各单独 commit）**
1. P2 日志分流（`init_tracing(tui)` + `config::log_path` + 收编 stray print）— 收益最大、最独立。
2. P4 CLI 会员（`login_flow` UserBrief + `isRealPremium` 判定 + 单测）。
3. P3 浏览器授权流（rdg 新增 `browser_login` 接入 `/auth/request`+`/auth/poll` 轮询）。
   （P5 dev 端口经核实非 bug，已撤销；P5 随 P1 协议统一从根消除。）

**阶段二：跨仓库（ridge-cloud）**
5. P4 后端 `checkin_grant_2h` 修跳过分支 + 数据修复迁移（`UPDATE users SET is_trial=false WHERE plan='premium'`）。

**阶段三：P1 大迁移（分步，每步 `cargo check` 绿 + 桌面 LAN 回归；额度恢复后其无状态迁移块可上工作流并行）**
6. 按 §1-P1 分阶段 1→8 执行，P5 协议分叉随之消除。

> 约束：当前 session 额度受限（resets 1:20pm SGT）无法派 subagent；阶段一/二主 loop 直接落地；阶段三 P1 体量大（3616 行迁移 + trait 解耦），分步推进，可能跨会话。

## 3. 验收
- 每步 `cargo check`/`cargo test` 绿；桌面 LAN/云端远控回归不破。
- rdg：TUI 无日志糊屏；浏览器登录（含 WSL）结果能回传；正式会员显示 PREMIUM；LAN/连接可用且与桌面 host 同协议、同 SPA。
- `src-tauri/src/remote/` 删除后全局仅 `packages/ridge-remote` 一份；三形态 + 控制端两形态共用。
- 运行时（真机/双进程/浏览器/会员账号）验证项由各问题诊断的 runtimeVerification 汇总，单独跟踪。
