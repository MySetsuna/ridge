# 2026-08-12 下一迭代：native A2A、Remote 观测与遗留项矩阵

## 范围与依据

- 用户批准证据：`所有已知遗留项，都纳入本次迭代`。
- Active 需求：`REQ-NEXT-ITERATION-20260812-01`、`REQ-A2A-NATIVE-SERVER-01`。
- NotebookLM 仅作候选与实验建议；代码、CodeGraph、测试及运行证据为事实来源。
- NLM 本轮建议先做 bounded Remote trace，再做最小 native A2A boundary；未证明的 ICE 时序、scrollback 抢占、移动 context loss 假设不得当作根因。

## 已落地代码

### Native A2A

- `packages/ridge-mcp/src/native_a2a.rs` 新增原生 A2A HTTP boundary：
  - `/.well-known/agent-card.json` 与兼容的 `/api/v1/a2a/agent-card`；
  - `/api/v1/a2a` JSON-RPC 2.0；
  - `SendMessage`、`GetTask`、`ListTasks`、`CancelTask`；
  - Agent Card 明确声明 streaming/push/extended card 不支持，相关方法 typed reject；
  - `x-ridge-token`/Bearer 认证、tenant 校验、请求/响应上限；
  - 外部 A2A 消息只经既有 `call_tool_rpc` 进入 Message Hub，任务状态复用 receipt；
  - target 必须显式提供 `paneId`，generation/lease/workspace 继续由 Hub/host 围栏校验。
- `ridge-tmux` 与 Tauri teammate server 均挂载同一 native A2A adapter；未新增第二消息存储或旁路 PTY。

### Agent history follow-up

- 根因：历史入口原先只发现 Claude/Codex 原生 JSONL；本机 Codex 文件格式本身可解析，
  但来源扫描与全局展示上限会让来源不公平；Cursor Agent 的实际记录位于
  `~/.cursor/projects/**/agent-transcripts/**/*.jsonl`，其根记录是
  `role=assistant` + `message.content`，既有 parser 未接入该形状。
- 修复：Claude、Codex、Cursor Agent 分源限额发现；Cursor transcript 目录/文件名回退
  session id；兼容根级 assistant 记录；UI 24 条投影先保留每个 Agent 最新行，再按时间补齐。
  Grok 继续使用既有独立 adapter。
- 本机盘点：`.gemini` 当前只有项目历史 Git 工作树，没有可验证聊天记录；`.aider` 不存在。
  Gemini/Aider 因无可靠本地格式证据，继续只做进程识别、历史与恢复 fail-closed，不猜路径或字段。
- 专项测试：`commands::project::tests` 为 `29 passed; 0 failed`；包含 Codex fixture、
  Cursor parser、Cursor 目录发现、各来源公平限额。

### Remote 性能/可用性

- `RidgeCloudHost` 接入既有 bounded `remotePerfTrace`：host 端 raw receive、control/pane wire send、pane drop、WebRTC stats。
- trace 默认关闭，最多保留 256 个样本，不保存 payload；仅用于定位真实网络、设备与 WebView2 问题。
- 未凭 NLM 假设修改 ICE watchdog、重连 deadline 或 priority queue 策略。

## 本轮验证

| 范围 | 结果 |
| --- | --- |
| native A2A 定向 | 5 passed：Agent Card、认证/unsupported、target/tenant/fence、send/get/list/cancel、超大请求 |
| `ridge-mcp` 全 lib | 108 passed |
| Tauri 全量 `cargo check` | exit 0；161 条既有 warning，无新增 error |
| Remote 云 host/controller/queue/trace/weak-net | 5 files / 62 tests passed |
| Agent history | Rust `commands::project::tests`: 29 passed；Codex fixture、Cursor transcript 发现与来源公平限额通过 |
| CSP/样式 | `sync-generated-csp` + `app-csp`: 8 tests passed；开发响应头允许 Vite runtime style，生产仍 hash-only；CDP 截图确认黑色 Ridge UI、布局正常、阻断样式 0 |
| Remote LAN/mobile E2E | LAN desktop/mobile pass；mobile keyboard pass；CA 由本机 remote TLS 环境注入 |
| Playwright E2E | clean run: 18 passed、1 skipped；首次 cold Vite 的 `commune.spec.ts` 启动抖动经预热后全量复跑通过 |
| `dev:cdp`/CDP smoke | dynamic CDP target 成功发现，`pnpm cdp:smoke` pass；截图检查已完成 |
| `git diff --check` | passed |
| Sonar | CLOSED（本轮有效副本扫描） | `C:\sonar-next-source-20260812e` 经 LF 归一化并含 sidecar/生成 `tsconfig`；776 files indexed；Quality Gate `PASSED`；coverage `82.6%`、line `89.0%`、branch `73.6%`、violations `0`；new coverage `82.6%`、new duplication `1.93048%`、new violations `0` | 当前工作树直扫仍受 Windows CRLF analyzer offset 限制；下次代码批次须重扫，不复用临时 token 或把扫描副本当发布产物 |
| 真实设备/公网/第三方 Agent | 本轮未具备证据，保持 OPEN/BLOCKED |

## 遗留项与下一步

| 项目 | 状态 | 已知事实 | 下一步/停机条件 |
| --- | --- | --- | --- |
| Message Hub/A2A SSOT | CODE CLOSED | native A2A 复用 Hub receipt、identity、capability、generation、lease | 第三方 live probe 前不宣称互操作完成 |
| A2A streaming/push/extended card | BLOCKED | Card 明确 false；方法返回 `-32601`，本轮未伪造 SSE/push | 若要开放，先补协议合同、资源预算、订阅/取消/断线测试 |
| 第三方 Agent/Card live interoperability | OPEN | 未提供第三方 endpoint/credential；本地 fixture 不能代替现场 | 取得 endpoint 与最小凭据后运行 probe，记录 task lifecycle/headers/receipt mapping |
| Remote ICE watchdog/deadline | OPEN | NLM 仅给出 15s/12s 假设；现有弱网 fixture 通过，尚未证明现场根因 | 开启 trace 做 auth→ICE→hello→pane recovery→rebuild 分段采样；若无现场数据不改时序 |
| Remote scrollback/input 抢占 | PARTIAL | 双 lane、bounded priority queue、host/controller trace 已在代码；真实输入延迟未采样 | 注入 scrollback flood + input，记录 enqueue-to-send、queue peak、drop/resync |
| 移动 PWA / WebGL 或 Canvas context loss | OPEN | 本地 mobile build/Chromium fixture 不等价于真机 | 真机触发 context loss/后台恢复/resize，核验 heartbeat、input、worker 计数归零 |
| 公网 WebRTC/TURN 与旧线上 artifact | OPEN | 本地协议与 LAN fixture 通过；公网 artifact/UA 与当前 checkout 是否一致仍需现场核验 | 用户授权发布后核对 artifact fingerprint，再做四路径公网验证 |
| WebView2 长跑、双窗口/双 Host、force-kill 重连 | OPEN | Tauri compile 通过；尚无完整物理运行证据 | `dev:cdp`/打包运行 + 过程树、重连、PTY 五条件原子快照 |
| PTY 五条件与真实 Agent CLI | OPEN | Hub/PTY fallback fail-closed；fixture 不证明真实进程 | 真实 Kernel/Agent 运行时采样 generation、lease、workspace、pane、PTY handle |
| workspace 跨盘/多窗口 | PARTIAL | composite workspace identity 与路径公共祖先已有确定性测试 | 补真实跨卷 Explorer、多窗口切换与恢复证据 |
| ridge-term/Codex render 分支 | PARTIAL | 本地 parser/renderer 单测已有；本轮未发现新静态根因 | 以 dev:cdp 验证 monotonic frame、clear、resize、OSC8/history overlay |
| Sonar 当前 HEAD | PARTIAL | 本轮有效 LF 副本 CE/Gate 已成功；当前工作树直扫仍受 CRLF analyzer offset 限制 | 下次代码批次在规范化副本或统一 LF 工作树重扫；临时 token 用后立即撤销 |
| NotebookLM 单一来源卫生 | OPEN | 认证已恢复；notebook 仍有多来源历史 | 新状态源索引确认后再做不可逆清理；不影响本轮代码事实 |

## 发布边界

- 本轮不以“本地测试通过”替代真实公网、真机、WebView2、第三方 Agent 证据。
- 不提交 `.iteration` 运行态、Cookie、Sonar token 或密码。
- 版本化 Release 仍须满足工作区洁净、HEAD 已推送、tag/安装包矩阵一致；缺资产只记阻塞，不建空 Release。
