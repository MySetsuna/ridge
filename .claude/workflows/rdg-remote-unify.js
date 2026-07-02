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
    { title: '4-trait-impls', detail: 'RemoteHost trait + server_app.rs + 桌面 remote_host_impl + rdg lan_host_impl（捆绑，编译绿）' },
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
    key: '5-delete',
    check: 'cargo check -p ridge && cargo check -p ridge-cli',
    prompt: `阶段 5：迁移全绿后删除 src-tauri/src/remote/（server.rs/auth.rs/mdns.rs/core_bridge.rs/mod.rs 及内联
mobile_page.html），改所有引用点：src-tauri/src/lib.rs（pub mod remote / forward_event / spawn_remote_server 接线）、
commands/{remote,fs_watch,settings}.rs、state.rs、bin/remote-server.rs（陈旧 stub，优先删除该 bin）。forward_event /
core_bridge::desktop_ctx 迁 src-tauri 新模块。tls re-export 改直接用 ridge_remote::tls。桌面 cargo check 全绿。`,
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
