# Iteration 86 Contract — Remote PWA UI, Query data, Git workflow, Agent parity

## Approved scope

Intake: `INTAKE-20260802-REMOTE-PWA-GIT-AGENT-01` (executable; no Pending
records). The five Active requirements are:

- `REQ-MOBILE-REMOTE-PWA-SAFE-AREA-01`
- `REQ-REMOTE-QUERY-CACHE-01`
- `REQ-GIT-INTERACTIVE-PUBLISH-GRAPH-01`
- `REQ-AGENT-COMMUNE-REMOTE-PARITY-01`
- `REQ-MOBILE-REMOTE-PWA-INSTALL-01`

Approval evidence is the user's explicit pre-approval of the next iteration,
including the follow-up that mobile Remote must manage Agent groups and expose
an Agent history Tab.

### Implementation checkpoint (2026-08-02)

P0 PWA groundwork is now implemented locally: the one-shot install event is
captured before the app mounts, the Remote header/empty state exposes an install
action when the browser supplies a prompt, iOS gets an explicit manual
add-to-home-screen branch, and the drawer header/body consume top/bottom
safe-area insets. Controller tests cover accepted, dismissed, timeout, error,
standalone/display-mode, iOS and idempotent teardown; the mobile production
build contains `manifest.webmanifest`, `sw.js` and the install event handler.
Browser HTTPS install, real standalone transition and physical notch-device
interaction remain evidence gates below; no browser install success is claimed
from the local build alone.

The Query portion of P0 is also wired locally: sidebar file/Git/search/read/diff
reads use session- and cwd-scoped keys with a bounded stale window and Query
single-flight; successful file writes invalidate the sidebar prefix. This does
not claim the later Git mutation/Graph or Agent parity packages.

Release gate evidence: tag `v0.1.34` from `1901eb0` completed workflow
`30730231317`, was published from draft with all 12 Windows/Linux/macOS assets.
The separate Remote artifact workflow `30731241075` built latest `main` at
`b62d94b` and activated `0.1.34+gb62d94b`; ridge-cloud workflow
`30731408697` deployed `67f7126`, and health remains `ok=true`.

## Baseline and constraints

- Current code baseline is `origin/main` after the v0.1.34 tag is built; do not
  mix Iteration 86 feature code into the release gate.
- Remote desktop, mobile web, and PWA must share query/data facts and protocol;
  CSS/display-mode differences are layout projection only.
- Preserve terminal core rendering, PTY rows/cols, pane composite identity,
  capability gates, process guards, HITL approval, and structured Agent resume.
- Keep one Query/cache source per fact. No title/CWD identity guesses, shell
  concatenation, forced GC, hidden Console errors, or unbounded retry/timer.

## Dependency-ordered work packages

### 1. PWA installability and safe-area/drawer geometry (P0)

现状：普通移动 Remote 可用；PWA standalone 刘海屏抽屉顶部操作区偏高，按钮/关闭
键落入不可点击区域；安装入口未显示，`beforeinstallprompt`/manifest/service-worker
资格链路尚未有真实证据。

目标：在满足资格的 HTTPS 浏览器显示真实 Install App 入口并正确完成安装/反馈；
browser 与 standalone 在刘海/挖孔/手势区、横竖屏、键盘和 viewport 抖动下，抽屉
header、操作按钮、关闭按钮均处于可见、可点击、可聚焦区域。

方案：先诊断 manifest、scope、icons、SW、HTTPS、display-mode 与
`beforeinstallprompt` 事件；用 install controller 只注册一次并缓存事件，明确已安装、
拒绝、不支持和 iOS 分支。再统一 safe-area inset adapter；将 `env(safe-area-inset-*)`
与 `visualViewport`/display-mode 投影到抽屉布局；用 CSS min/max 约束与现有键盘偏移
收敛，避免 UA 分叉、无限 RAF 或重复监听。

验收：Chromium HTTPS fixture 观测 `beforeinstallprompt`、点击安装并进入 standalone；
已安装/拒绝/不支持/iOS 分支反馈准确；manifest/SW/scope/icons 可检查。browser/PWA
两种 display mode + 刘海 top/bottom inset + 旋转 + 键盘开合 fixture；按钮命中 100%、
焦点顺序/aria 正确，打开/关闭/销毁监听与 timer 归零；`e2e:rdg-mobile-keyboard`
扩展到 drawer/Install 真实交互并保留 selection/keyboard 断言。

回归：`pnpm check`、Remote geometry/keyboard/worker suites、LAN desktop/mobile
matrix、移动端无障碍/触控 E2E。

### 2. Query 管理 Git/File 远程数据 (P0)

现状：部分 Remote Host/Explorer/Git 数据已有 Query，仍需核对打开 Tab/抽屉时的
fetcher、key、失效边界，消除重复加载。

目标：同一 host/workspace/pane/path/branch 请求单飞；重开复用缓存；提交、推送、
文件变更仅精确失效受影响 key；断线显示 stale 与状态，不堆 pending RPC。

方案：盘点现有 `remoteQueryKeys`/TanStack Query hooks，建立 Git/File key factory
与统一 fetcher；设置明确 staleTime/gcTime；将 mutation 结果映射到精确 invalidate；
取消信号贯穿 Host/Pane 销毁，保留现有 RPC scheduler/backoff。

验收：Vitest 证明重复打开一次请求、key 隔离、精确失效、断线/取消无 late commit；
LAN/cloud E2E 记录 RPC 数、loading、stale/error；不得以全局 invalidate 过度刷新。

回归：SCM negative-cache/5-minute polling、RPC dedupe/timeout、Host topology、
Explorer/FileViewer、Remote reconnect 与 memory counters。

### 3. Git commit/push workflow and GitGraph Tab (P1)

现状：Git 状态、分支、diff 查询已有内核 SSOT 与护栏；UI 尚缺安全的提交/推送闭环
及图形化提交历史。

目标：用户可在 UI 选择变更、输入提交信息、提交、推送；权限、确认、进度、取消、
冲突、无 upstream、非 Git、超时、失败均可见且不假成功；GitGraph 展示 branches、
HEAD、merge 节点连线与选中提交详情，支持键盘/触控。

方案：复用 ridge-core Git/process guard 与 capability；新增 typed command/RPC，
参数结构化传递并沿既有 spawn 超时/杀树/日志聚合；mutation 成功后精确失效 Query；
Graph 先做有界 commit DTO + deterministic layout，再接 Tab/虚拟化视图。

验收：临时真实 Git repo 覆盖 clean/dirty、commit、push 成功/拒绝、无 upstream、
冲突、非 Git、取消/超时；Graph branch/merge/HEAD/选中详情与移动触控通过；RPC、
git 子进程并发、Console 聚合和权限门测试绿。

回归：Git guard/SCM polling/PaneGitPill 单层显示、remote Git dispatch、RPC timeout
退避、错误日志聚合、桌面 Git 面板。

### 4. Agent Commune mobile parity (P1)

现状：桌面 Commune 已有状态卡片、历史/resume/CWD 与 Pane border projection；
Remote 手机已有部分 Agent roster，但编组管理与独立历史 Tab 未对齐。

目标：Remote web/PWA 与桌面共享 Agent DTO/query/cache，支持编组 CRUD、成员/会话
定位、状态/审批、历史 Tab、结构化恢复、workspace/pane 聚焦和同源边框反馈。

方案：提取桌面已验证 `agentPaneStatus`/history/resume projection 为共享纯模型；
Remote 侧用 Query 管理 roster/groups/history，按稳定 `(workspaceId,paneId,sessionId)`
寻址；编组 mutation 走 capability/HITL；触控 drawer/Tab 复用 safe-area adapter。

验收：多 Agent/CWD/session、编组 CRUD、运行/空闲/等待审批/停止/失败、损坏历史
fixture；Remote 与桌面状态/成员/历史一致；重复操作单飞，刷新/断线/跨窗口不串组或
Pane；LAN/cloud mobile E2E + desktop parity snapshot 通过。

回归：Agent history parser/unknown Agent retention、structured resume CWD、HITL、
Pane lifecycle/pending cancellation、多窗口 workspace singleton、PWA focus/a11y。

## Execution order and gates

1. CodeGraph/局部查询核对现有 drawer、Query keys、Git commands、Agent DTO；不全库重扫。
2. 先交付 PWA installability/safe-area geometry + Query key/fetcher contract（P0），各自纯测与小 E2E。
3. 在 Query contract 稳定后接 Git mutation/Graph（P1），再接 Agent mobile parity（P1），
   共享 DTO 与 capability 先于 UI。
4. 每包执行确定性测试、`pnpm check`、相关 Rust tests；合并前跑全 Vitest、LAN
   desktop/mobile、Remote artifact smoke。
5. 发布前执行 release version contract；Release 失败不得再升版本；成功须核验全部
   Windows/Linux/macOS assets，Remote artifact 与 cloud health 分线记录。
6. 完成后更新 `PROJECT-STATE.md`、本合同、`events-YYYY-MM.jsonl`；归档本迭代 intake
   与证据，删除/标记已完成的临时笔记，不把未验证外部闸写成完成。

## Non-claims until evidence

Physical iOS/Android notch device, installed PWA on a real notch device, public Remote
long-run, WebView2 heap soak, dual-window/dual-Host physical E2E, and real remote push
credentials remain external gates until their evidence is captured.
