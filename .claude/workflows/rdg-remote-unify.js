// P1：把 src-tauri/src/remote/ 迁入 packages/ridge-remote 并删除，三形态 + 控制端两形态共用一份。
//
// 运行前提（重要）：
//   1. 先停掉常驻 tauri dev（否则 target 锁 / 自动重建会与 agent 的 cargo check 冲突）。
//   2. 在 wind 干净工作树上跑（P2/P3/P4 已 commit）。串行、失败即停。
//   3. 这是大重构，建议开着 /workflows 监督；某阶段编译不绿会停，可编辑本脚本后 resumeFromRunId 续跑。
//
// 权威蓝图：docs/superpowers/specs/2026-07-02-rdg-remote-unify-and-fixes-design.md §1-P1（8 阶段，
// 路线 B：trait 抽象、桌面保留 Tauri 命令腿增量收口）。本脚本把 8 阶段合并成 6 个「可独立编译绿」的单元
// （trait + server_app + 两端 impl 必须捆绑，否则中间态编译不过）。

export const meta = {
  name: 'rdg-remote-unify',
  description: 'P1：src-tauri/src/remote → packages/ridge-remote 迁移并删除，桌面/rdg/云端 + 手机/桌面 SPA 共用一份（串行分阶段，每阶段 cargo check 门禁）',
  phases: [
    { title: '1-auth', detail: 'auth.rs(SessionStore/VerifyThrottle/RemoteAuth) 迁 ridge-remote' },
    { title: '2-mdns-net', detail: 'mdns.rs + detect_lan_ip(s) 迁 ridge-remote::{mdns,net}' },
    { title: '3-serve', detail: 'serve.rs 下沉：UaServeConfig 双产物 + UA 分流 + index/assets/spa_fallback/ca' },
    { title: '4-trait-impls', detail: 'RemoteHost trait + server_app.rs（共享 crate 核心，已完成）' },
    { title: '4b-desktop-impl', detail: '桌面 remote_host_impl + server.rs 切 server_app::run（搬 handle_ws/dispatch）' },
    { title: '4c-rdg-impl', detail: 'rdg lan_host_impl + 重写 lan_host.rs（ridge-remote-ws + 真 SPA，删内联 HTML）' },
    { title: '5-delete', detail: '删除 src-tauri/src/remote/ + 改所有引用点（lib/commands/state/bin）' },
    { title: '6-cloud', detail: 'ridge-cloud ua.rs 改依赖 ridge_remote::ua（git crate），消除镜像' },
  ],
}

const BG = `你在为 Wind 项目（C:/code/wind）执行 P1「remote 统一大迁移」的一个阶段。
最终目标：把 src-tauri/src/remote/（桌面完整 LAN host：server.rs 3616 行 + auth 658 + mdns 160 + core_bridge，
~130 处 crate::state::AppState 耦合、invoke 分发直调 crate::commands::*）迁入共享 crate packages/ridge-remote，
用 RemoteHost trait 解耦 AppState；桌面(src-tauri)/rdg(packages/ridge-cli tui/lan_host)/云端(ridge-cloud) 三形态
+ 控制端两形态(手机 static/remote、桌面 web-remote-dist SPA) 共用一份；最后删除 src-tauri/src/remote/。

权威蓝图（务必先读）：docs/superpowers/specs/2026-07-02-rdg-remote-unify-and-fixes-design.md 的 §0 现状 + §1-P1
的 8 阶段与决策点（路线 B：trait 抽象、桌面 InvokeDispatcher 保留 Tauri 命令腿增量收口；rdg 用 ridge_core::dispatch
白名单子集）。既有相关计划：docs/plans/unified-remote-architecture-handoff-final.md、unified-remote-gap-analysis.md(G4)。
按需读 src-tauri/src/remote/*.rs、packages/ridge-remote/src/*.rs、packages/ridge-cli/src/tui/lan_host.rs。

纪律：
- 只做本阶段范围；不要改 P2/P3/P4 已修的无关代码（main.rs init_tracing、login_flow、dashboard 日志/登录）。
- ridge-remote 必须保持【零 Tauri 依赖】（cargo tree -p ridge-remote 不得出现 tauri）。
- 桌面 LAN 远控 + rdg 行为语义不可变（回归不破）。
- 改完必须跑本阶段指定的 cargo check，如实报告是否全绿（checkPassed）+ 关键错误摘要；不绿就把 blocked=true 并说明。
- 用 Read/Grep/Glob/Bash + codegraph；边迁边验，宁可本阶段少做也要保持编译绿。`

const SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['stage', 'checkPassed', 'summary'],
  properties: {
    stage: { type: 'string' },
    checkPassed: { type: 'boolean', description: '本阶段指定的 cargo check 是否全绿' },
    checkOutput: { type: 'string', description: 'cargo check 的关键输出 / 错误摘要' },
    filesChanged: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string', description: '本阶段做了什么' },
    notesForNext: { type: 'string', description: '交接给下一阶段的关键点 / 未决 / 陷阱' },
    blocked: { type: 'boolean', description: '是否被卡住（编译不绿或缺前置）' },
    blockReason: { type: 'string' },
  },
}

// check 命令：ridge-remote/ridge-cli 轻；桌面 ridge 重（首次数分钟）。阶段涉及桌面即必须验桌面回归。
const STAGES = [
  {
    key: '1-auth',
    check: 'cargo check -p ridge-remote -p ridge-cli && cargo check -p ridge',
    prompt: `阶段 1：把 src-tauri/src/remote/auth.rs 的 SessionStore / VerifyThrottle / ThrottleDecision / RemoteAuth
（已依赖 ridge_core::RemoteTotp、无 Tauri 依赖）整体迁入 packages/ridge-remote/src/auth.rs，含全部单测。
packages/ridge-remote/Cargo.toml 需加 ridge-core = { path = "../ridge-core" } 依赖。lib.rs 导出 pub mod auth。
src-tauri/src/state.rs 与 src-tauri/src/remote/mod.rs 等改引用为 ridge_remote::auth::{...}（删除 remote/auth.rs）。`,
  },
  {
    key: '2-mdns-net',
    check: 'cargo check -p ridge-remote -p ridge-cli && cargo check -p ridge',
    prompt: `阶段 2：把 src-tauri/src/remote/mdns.rs（纯 UDP，无 Tauri）迁入 ridge-remote::mdns；把 mod.rs 里的
detect_lan_ip / detect_lan_ips 迁入 ridge-remote::net。src-tauri 与 rdg 都改用 ridge_remote::{mdns,net}::*；
rdg 现用 config::detect_lan_ip，统一到 ridge_remote::net（config 里保留薄封装或直接改调用点）。`,
  },
  {
    key: '3-serve',
    check: 'cargo check -p ridge-remote -p ridge-cli && cargo check -p ridge',
    prompt: `阶段 3：把 src-tauri/src/remote/server.rs 的【前端 serve】部分下沉到 ridge-remote/src/serve.rs：
UaServeConfig { mobile_dir: PathBuf, desktop_dir: Option<PathBuf> } + resolve_ui_dirs()（泛化 server.rs:220-271 的
运行时候选目录探测）+ serve_index / spa_fallback / assets / root / ca 各 handler + security_headers/Compression layer，
UA 分流用现有 ridge_remote::ua::prefer_desktop_ui + desktop_dir.exists() 回退。桌面 server.rs 改为调用 ridge-remote 的
serve 装配（行为不变）。本阶段先让桌面复用，rdg 接入放阶段 4。`,
  },
  {
    key: '4-trait-impls',
    check: 'cargo check -p ridge-remote -p ridge-cli && cargo check -p ridge && cargo tree -p ridge-remote | grep -i tauri; echo "^应为空"',
    prompt: `阶段 4（最大块，捆绑做，务必编译绿）：在 ridge-remote 定义 RemoteHost trait 组（WorkspaceProvider /
PaneProvider / InvokeDispatcher / EventBus / HostAuth，见 design doc §1-P1）+ server_app.rs（把 server.rs 的路由/
remote_gate/verify/ws_handler/workspace_*/file/session/handle_ws 迁为对 Arc<dyn RemoteHost> 泛型）。
桌面：src-tauri/src/remote_host_impl.rs 为 AppState 实现 RemoteHost（WorkspaceProvider/PaneProvider/EventBus 转发
AppState；InvokeDispatcher = 现 dispatch_invoke_request/jsonrpc 搬来，保留 crate::commands::* 与 CORE_MIGRATED_METHODS
双腿；core_bridge 依赖 AppHandle 留 src-tauri），桌面 server.rs 改用 server_app::run。
rdg：packages/ridge-cli/src/tui/lan_host_impl.rs 为 SharedWorkspace 实现 RemoteHost（InvokeDispatcher 仅
ridge_core::dispatch 白名单子集，未迁 method 返回 MethodNotFound），重写 lan_host.rs 构造 Arc<dyn RemoteHost>+
UaServeConfig 调 server_app::run，删除内联 LOGIN_HTML/TERMINAL_HTML 与手写 WS。rdg 的 web-remote-dist 产物用运行时
目录探测（缺失回退移动 SPA）。三处 cargo check 必须全绿，且 ridge-remote 零 Tauri 依赖。`,
  },
  {
    key: '4b-desktop-impl',
    check: 'cargo check -p ridge && cargo check -p ridge-cli && (cargo tree -p ridge-remote | grep -ci tauri)',
    prompt: `阶段 4b（桌面 impl，承接阶段4 已建的 ridge_remote::host::RemoteHost + ridge_remote::server_app）。目标：让桌面
改用 server_app::run，桌面 LAN 远控回归不破。按阶段4 agent 的 notesForNext 执行：
1. 新建 **src-tauri/src/remote_host_impl.rs**（注意放 src-tauri/src/ 顶层，【不要】放进 src-tauri/src/remote/，因为该目录阶段5 要删）：
   为 AppState 的包装器 struct DesktopHost{state, auth, port, lan_ip, machine_name, serve_cfg, tls_enabled} 实现
   ridge_remote::host 的 HostMeta / HostAuth / WorkspaceProvider + RemoteHost::serve_websocket。
   - HostAuth：verify_code=auth.verify；is_blacklisted/pre_verify_gate/post_verify_record/auto_blacklist_on_ban 逐字搬
     server.rs:495-561，转发 ctx.state.remote_blacklist / remote_verify_throttle / remote_session_store。
     create_session_token/validate_token* 转发 remote_session_store。
   - WorkspaceProvider：搬 server.rs 的 workspace_list/switch/create/close_handler 主体 + allowed_file_roots(server.rs:348-364)。
   - serve_websocket：把 server.rs 的 handle_ws(958-2175) + build_remote_pane_list + apply_pane_resize +
     dispatch_data_request/dispatch_invoke_request/dispatch_invoke_jsonrpc + is_mutating_* + negotiate_hello +
     常量(REMOTE_PROTOCOL_VERSION/HOST_CAPABILITIES/CORE_MIGRATED_METHODS/JSON_RPC_*) 搬进来；保留 crate::commands::*
     与 CORE_MIGRATED 双腿（D-GM-2）。core_bridge 依赖 AppHandle，本阶段仍留 src-tauri（阶段5 再决定移哪）。
2. 桌面 server.rs 的 run_remote_server 改：构造 Arc<DesktopHost>（含 UaServeConfig 的 resolve_ui_dirs + 多网卡 TLS
   fail-closed 逻辑不变），bind_tcp(9527)+resolve_config_multi 后调 ridge_remote::server_app::run(host, listener,
   tls_config, shutdown_rx, true)；删掉 server.rs 里已迁走的内联 handle_ws/dispatch/workspace handler。
   spawn_remote_server 对外签名保持不变。src-tauri/src/lib.rs 加 mod remote_host_impl（若需要）。
体量大（搬 ~1200 行 handle_ws），可多次编辑 + 多次 cargo check 迭代；本阶段结束前【必须】cargo check -p ridge 全绿、
ridge-remote 仍零 Tauri。若确实一趟难绿，把已完成部分说清、blocked=true 并列出剩余，不要留下编译红。`,
  },
  {
    key: '4c-rdg-impl',
    check: 'cargo check -p ridge-cli && cargo check -p ridge-remote',
    prompt: `阶段 4c（rdg impl，承接 ridge_remote::host + server_app）。按阶段4 notesForNext：
1. 新建 packages/ridge-cli/src/tui/lan_host_impl.rs：为 rdg 的 SharedWorkspace（包装 struct）实现
   ridge_remote::host::RemoteHost。陷阱：rdg 用 crate::totp::RemoteTotp（非 ridge_remote::auth::RemoteAuth），
   无 SessionStore/throttle/blacklist —— verify_code=totp.verify；is_blacklisted/节流用 trait 默认实现；
   create_session_token 让 rdg 自持一个 ridge_remote::auth::SessionStore（零 Tauri）。WorkspaceProvider 面向单
   工作区（create 走 create_session、close 拒绝最后一个、switch 最小支持/no-op）。
2. serve_websocket：重写现 lan_host.rs 的 run_ws 逻辑为讲 ridge-remote-ws（hello 的 protocol 字段从 'ridge-lan-ws'
   改 'ridge-remote-ws'；pane 帧 16B UUID 前缀已一致）。这一步同时消除 P5 的协议分叉。
3. 重写 packages/ridge-cli/src/tui/lan_host.rs：构造 Arc<dyn RemoteHost> + UaServeConfig（web-remote-dist 探测用
   ridge_remote::serve::resolve_ui_dirs，缺失回退移动 SPA static/remote）调 ridge_remote::server_app::run；
   删除内联 LOGIN_HTML/TERMINAL_HTML 与手写 assets_handler。dashboard.rs 对 lan_host::run 的调用签名尽量不变。
cargo check -p ridge-cli 全绿。注意不要动 P2/P3/P4 已修的 main.rs/login_flow/dashboard 日志与登录逻辑。`,
  },
  {
    key: '5-delete',
    check: 'cargo check -p ridge && cargo check -p ridge-cli',
    prompt: `阶段 5：桌面(4b)/rdg(4c) 均已切到 server_app 且全绿后，删除 src-tauri/src/remote/ 目录，改所有引用点：
- core_bridge.rs 依赖 tauri AppHandle，不能进 ridge-remote：把它移到 src-tauri/src/remote_bridge.rs（或并入
  remote_host_impl.rs），再删整个 src-tauri/src/remote/（server.rs/mod.rs/mobile_page.html 等已迁/无用）。
- 引用点：src-tauri/src/lib.rs（pub mod remote / forward_event / spawn_remote_server 接线改指向新位置）、
  commands/{remote,fs_watch,settings}.rs、state.rs（已用 ridge_remote::auth，确认无残留 crate::remote::auth）、
  bin/remote-server.rs（陈旧 stub，优先删除该 bin 及 Cargo.toml 里的 [[bin]] 条目）。
- mod.rs 里对 ridge_remote 的重导出（pub use ridge_remote::{auth,mdns,net,tls}）改为各调用点直接引用 ridge_remote::*，
  然后删除 mod.rs。tls re-export 同理。
cargo check -p ridge 与 -p ridge-cli 全绿；桌面 LAN 远控行为不变。`,
  },
  {
    key: '6-cloud',
    check: 'SQLX_OFFLINE=true cargo check --manifest-path C:/code/ridge-cloud/Cargo.toml',
    prompt: `阶段 6（跨仓库）：ridge-cloud（C:/code/ridge-cloud）的 src/ua.rs 目前是 wind ua.rs 的逐字镜像（Docker
构建够不到 wind 仓）。按 design doc §1-P1(8)：把 ridge-remote 发布为 git crate（仿 ridge-signaling 的 git 依赖），
ridge-cloud 改依赖 ridge_remote::ua 消除镜像漂移；static_host/router 的 host 逻辑（Host-label/租户/PWA sw.js）差异
大，短期保留自有 serve，仅共用 ua 判定 + 纯函数。若 git crate 发布不便，本阶段可仅产出对接方案文档、不强改。`,
  },
]

phase('1-auth')
log('P1 remote 统一：串行分阶段迁移，每阶段 cargo check 门禁，失败即停')
const results = []
let prevNotes = '（无，首阶段）'
for (const s of STAGES) {
  phase(s.key)
  const r = await agent(
    `${BG}\n\n## 本阶段：${s.key}\n${s.prompt}\n\n上一阶段交接：${prevNotes}\n\n完成后务必运行：\n  ${s.check}\n并在返回里如实填写 checkPassed 与 checkOutput。`,
    { label: `unify:${s.key}`, phase: s.key, schema: SCHEMA, effort: 'high' }
  )
  results.push(r)
  if (!r || r.blocked || !r.checkPassed) {
    log(`阶段 ${s.key} 未通过（checkPassed=${r?.checkPassed}, blocked=${r?.blocked}）：${r?.blockReason || r?.summary || 'agent 无返回'}。停止，避免污染后续阶段。`)
    break
  }
  prevNotes = r.notesForNext || r.summary || ''
  log(`阶段 ${s.key} ✅ 编译绿，进入下一阶段。`)
}
return results
