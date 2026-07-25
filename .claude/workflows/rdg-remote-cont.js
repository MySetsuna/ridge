// P1 续跑：阶段 4c(rdg impl) → 5(删除 remote/) → 6(cloud 对接)。
// 阶段 1-4b 已完成并 commit 于 5a415a6（干净基线）。本脚本从该基线串行续跑，
// 每阶段 cargo check 绿后【立即 git commit】本阶段（防中断丢失）。失败即停。
//
// 运行前提：先停常驻 tauri dev（避免 target 锁）；wind 工作树基线干净（1-4b 已 commit）。

export const meta = {
  name: 'rdg-remote-cont',
  description: 'P1 续跑：rdg 接入共享 server_app(消除私有协议=P5) → 删除 src-tauri/src/remote/ → ridge-cloud ua 对接。每阶段 check 绿即 commit。',
  phases: [
    { title: '4c-rdg-impl', detail: 'rdg lan_host_impl + 重写 lan_host.rs（ridge-remote-ws + 真 SPA，删内联 HTML）' },
    { title: '5-delete', detail: '删除 src-tauri/src/remote/（core_bridge 移出）+ 改引用点' },
    { title: '6-cloud', detail: 'ridge-cloud ua.rs 改依赖 ridge_remote::ua 或产出对接方案' },
  ],
}

const BG = `你在执行 Wind(C:/code/wind) 的 P1「remote 统一大迁移」的一个阶段（续跑）。阶段 1-4b 已完成并 commit 于 5a415a6：
src-tauri/src/remote 的 auth/mdns/net/前端serve/host(RemoteHost trait)/server_app 已迁入 packages/ridge-remote；桌面
DesktopHost(src-tauri/src/remote_host_impl.rs) 实现 RemoteHost、server.rs(134 行) 经 ridge_remote::server_app::run 驱动。
共享 crate 现有模块：ridge_remote::{auth,mdns,net,tls,ua,serve,host,server_app}。
- RemoteHost trait 表面见 packages/ridge-remote/src/host.rs（HostMeta/HostAuth/WorkspaceProvider + serve_websocket 钩子，
  serve_websocket 返回 Pin<Box<dyn Future+Send>> 保对象安全）。
- 桌面实现范例见 src-tauri/src/remote_host_impl.rs（照它的模式实现 rdg 版）。
- serve 复用见 ridge_remote::serve（UaServeConfig{mobile_dir,desktop_dir:Option} + resolve_ui_dirs + serve_router）。

纪律：
- 只做本阶段范围；ridge-remote 必须保持零 Tauri 依赖。
- 【不要动】P2/P3/P4 已修代码：packages/ridge-cli/src/{main.rs,login_flow.rs,config.rs,tui/dashboard.rs} 的日志分流/
  浏览器登录/会员判定逻辑（可改这些文件里与 lan_host 接线相关的调用，但别碰上述已修功能）。
- 改完必须跑本阶段指定 cargo check；全绿才 git commit 本阶段，编译红则 blocked=true 且【不要】commit。
- git add 只加本阶段相关文件，【绝不】add scripts/tmp-*.mjs、scratchpad*、.claude/ 等无关文件。`

const SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['stage', 'checkPassed', 'committed', 'summary'],
  properties: {
    stage: { type: 'string' },
    checkPassed: { type: 'boolean', description: '本阶段指定 cargo check 是否全绿' },
    committed: { type: 'boolean', description: '是否已 git commit 本阶段' },
    commitHash: { type: 'string' },
    checkOutput: { type: 'string' },
    filesChanged: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
    notesForNext: { type: 'string' },
    blocked: { type: 'boolean' },
    blockReason: { type: 'string' },
  },
}

const STAGES = [
  {
    key: '4c-rdg-impl',
    check: 'cargo check -p ridge-cli && cargo check -p ridge-remote && (cargo tree -p ridge-remote | grep -ci tauri)',
    commitMsg: 'refactor(remote): P1 阶段4c — rdg LAN host 接入共享 server_app，消除私有协议(P5)',
    prompt: `阶段 4c（rdg impl）：让 rdg 的 LAN host 也走共享 ridge_remote::server_app，消除私有 ridge-lan-ws 协议 + 内联 HTML。
1. 新建 packages/ridge-cli/src/tui/lan_host_impl.rs：为 rdg 的 SharedWorkspace（包装 struct，参考 src-tauri/src/remote_host_impl.rs
   的 DesktopHost 模式）实现 ridge_remote::host::RemoteHost。陷阱：rdg 用 crate::totp::RemoteTotp（非 ridge_remote::auth::
   RemoteAuth），无 SessionStore/throttle/blacklist —— HostAuth::verify_code=totp.verify；is_blacklisted/pre_verify_gate/
   post_verify_record 用 trait 默认实现；create_session_token 让 rdg 自持一个 ridge_remote::auth::SessionStore（零 Tauri）。
   WorkspaceProvider 面向单工作区（create 走 create_session、close 拒绝最后一个、switch 最小支持/no-op、
   list_workspaces_json 返回单工作区 + 其 panes、allowed_file_roots 按 rdg 语义）。
2. serve_websocket：把现 lan_host.rs 的 run_ws 逻辑重写为讲 ridge-remote-ws（hello 的 protocol 字段从 'ridge-lan-ws' 改
   'ridge-remote-ws'；pane 帧 16B UUID 前缀已一致；stdin/subscribe-pane/resize 等映射到 SessionHandle）。这一步消除 P5 协议分叉。
3. 重写 packages/ridge-cli/src/tui/lan_host.rs：pub async fn run(...) 构造 Arc<dyn RemoteHost> + UaServeConfig（用
   ridge_remote::serve::resolve_ui_dirs 探测 web-remote-dist + static/remote，缺失回退移动 SPA）调 ridge_remote::server_app::run；
   删除内联 LOGIN_HTML/TERMINAL_HTML 与手写 assets_handler/verify_handler。保持 dashboard.rs 对 lan_host::run 的调用签名不变
   （dashboard 传 port/totp/workspace/shutdown_rx）——若签名必须变，同步改 dashboard 调用点但不碰其日志/登录逻辑。`,
  },
  {
    key: '5-delete',
    check: 'cargo check -p ridge && cargo check -p ridge-cli',
    commitMsg: 'refactor(remote): P1 阶段5 — 删除 src-tauri/src/remote/，core_bridge 移出顶层',
    prompt: `阶段 5：桌面(4b)/rdg(4c) 均已切共享 server_app 且全绿后，删除 src-tauri/src/remote/ 目录并改引用点：
- core_bridge.rs 依赖 tauri AppHandle，不能进 ridge-remote：移到 src-tauri/src/remote_bridge.rs（或并入 remote_host_impl.rs），
  更新其引用点（commands/settings.rs 的 core_bridge::desktop_ctx、fs_watch.rs 的 forward_event 等）。
- 确认 remote/server.rs（134 行接线）、mod.rs、mobile_page.html 已无必要后删除整个 src-tauri/src/remote/。
- mod.rs 里对 ridge_remote 的重导出（pub use ridge_remote::{auth,mdns,net,tls}）改为各调用点直接 ridge_remote::* 引用后删 mod.rs。
- src-tauri/src/lib.rs：pub mod remote 删除；forward_event/spawn_remote_server 接线改指向新位置（spawn_remote_server 现在
  在哪由 4b 决定，通常仍在 remote_host_impl 或新模块）。
- bin/remote-server.rs：陈旧 stub（终端操作全返回"需要完整 app"），优先【删除该 bin】+ 去掉 Cargo.toml 的 [[bin]] 条目；
  若删除牵扯过多，改成用 ridge_remote::server_app + 最小 headless host。
- state.rs 确认已用 ridge_remote::auth，无残留 crate::remote::auth。
cargo check -p ridge 与 -p ridge-cli 全绿；桌面 LAN 远控行为不变。`,
  },
  {
    key: '6-cloud',
    check: 'SQLX_OFFLINE=true cargo check --manifest-path C:/code/ridge-cloud/Cargo.toml',
    commitMsg: 'chore(remote): P1 阶段6 — ridge-cloud 与 ridge-remote UA-SSOT 对接',
    prompt: `阶段 6（跨仓库 ridge-cloud，C:/code/ridge-cloud）：ridge-cloud 的 src/ua.rs 目前是 wind ua.rs 的逐字镜像（Docker
构建够不到 wind 仓）。理想：把 ridge-remote 发布为 git crate（仿 ridge-signaling 的 git 依赖），ridge-cloud 改依赖
ridge_remote::ua 消除镜像漂移。但发布 git crate 需推仓库、可能超出本地范围。
本阶段务实处理：(a) 若能不发包就复用（如 path 依赖或已有 git tag）则接；(b) 否则【仅产出对接方案 + 在 ridge-cloud 的
ua.rs 顶部加注释指明 SSOT 来源与同步约定】，不强行改依赖。static_host/router 的 host 逻辑（Host-label/租户/PWA sw.js）
差异大，明确【不动】。cargo check ridge-cloud（SQLX_OFFLINE）保持绿。若本阶段只出文档不改依赖，也算完成（committed 提交该文档/注释）。
本阶段的 git commit 在 ridge-cloud 仓库执行（git -C C:/code/ridge-cloud），不推送。`,
  },
]

phase('4c-rdg-impl')
log('P1 续跑 4c→5→6：每阶段 cargo check 绿后立即 git commit（防中断丢失），失败即停')
const results = []
let prevNotes = '阶段1-4b 已完成并 commit 于 5a415a6（桌面已切 server_app，共享 crate 就绪）'
for (const s of STAGES) {
  phase(s.key)
  const repoHint = s.key === '6-cloud'
    ? `在 ridge-cloud 仓库提交：git -C C:/code/ridge-cloud add <文件> && git -C C:/code/ridge-cloud commit -m "${s.commitMsg}\\n\\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`
    : `在 wind 仓库提交：git add <本阶段精确文件> && git commit -m "${s.commitMsg}\\n\\nCo-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"`
  const r = await agent(
    `${BG}\n\n## 本阶段：${s.key}\n${s.prompt}\n\n上一阶段交接：${prevNotes}\n\n完成后运行：\n  ${s.check}\n全绿则${repoHint}\n如实填写 checkPassed/committed/commitHash。编译红则 blocked=true 且不要 commit。`,
    { label: `cont:${s.key}`, phase: s.key, schema: SCHEMA, effort: 'high' }
  )
  results.push(r)
  if (!r || r.blocked || !r.checkPassed) {
    log(`阶段 ${s.key} 未通过（checkPassed=${r?.checkPassed}, blocked=${r?.blocked}）：${r?.blockReason || r?.summary || '无返回'}。停止。`)
    break
  }
  log(`阶段 ${s.key} ✅ ${r.committed ? '已 commit ' + (r.commitHash || '') : '（未 commit！）'}`)
  prevNotes = r.notesForNext || r.summary || ''
}
return results
