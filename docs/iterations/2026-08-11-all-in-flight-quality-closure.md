# Ridge 在途需求与质量闭环交接（2026-08-11）

## 结论

本轮已接收并执行 60 项已批准需求；`docs/PENDING-REQUIREMENTS.md` 无待审批项，需求门禁通过。结论并非“所有 NLM 来源需求均已落地”：本机可确定性验证的代码、测试与本地 Cloud 链路已闭合，真实设备、公网、第三方运行时及当前 Sonar 认证仍有未闭合项，详见下方矩阵。代码侧未用 `NOSONAR`、规则降级或新增静态排除掩盖问题。

已提交的本轮相关提交：

| 提交 | 内容 |
| --- | --- |
| `c3504871` | Sonar 问题对应的真实 Rust/TypeScript 修复、测试补强与覆盖率提升 |
| `a151f538` | 修正 Git smoke 测试对可选 branch 的断言，恢复全 Rust 测试编译 |
| `1f86da34` | 删除 `ridge-mcp` 路由中已被真实实现取代的整段死注释分支 |
| `34bcf503` | 删除 `tmux` shim 中已被真实实现取代的死注释分支 |
| `09e01384` | 放行受限的 Cloud 租户子域 CSP（本地 `*.localhost` 与生产 `*.9527127.xyz`），补 CSP 回归测试 |

## 可复验证据

### 代码与测试

- `pnpm check`：`0 errors / 0 warnings`。
- `pnpm test`：216 个文件通过，2009 passed，5 skipped，共 2014 项测试；退出码 0。
- `pnpm test:coverage:sonar`：216 个文件通过，退出码 0；归一化报告写入 `coverage/lcov.info`。本地 V8 未过滤汇总为 statements `67.46%`、branches `61.83%`、functions `69.35%`、lines `71.54%`；该汇总包含不属于 Sonar 产品覆盖率口径的本地脚本/辅助路径，不能替代 Sonar project metric。
- `cargo test -p ridge-core --lib`：344 passed。
- `cargo test -p ridge-mcp --lib`：90 passed，0 failed。
- `cargo test --manifest-path src-tauri/Cargo.toml --bin tmux`：27 passed，0 failed。
- `cargo test --workspace --all-targets`：全工作区通过；此前唯一编译阻断为 Git smoke 测试把 `Option<String>` 当成 `String`，已在 `a151f538` 修正。
- `codegraph sync`：完成；Cloud 信令调用链复核覆盖 `cloudWsScheme → ControllerCloudProvider/RidgeCloudHost → WebSocket`，以及远端、workspace、pane、Agent 生命周期相关符号。
- `git diff --check`：相关代码差异无空白错误。
- `pnpm build`：退出码 0；耗时约 3 分 54 秒。仍有既有 dynamic-import/chunk-size warning，不影响构建成功。
- `cargo fmt --all -- --check`：未通过；差异集中在工作区既有 `src-tauri/src/commands/terminal.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/remote_host_impl.rs`、`src-tauri/src/teammate/server.rs` 等 dirty 文件，本轮未擅自格式化或纳入提交。

### 真实本地 Cloud / CDP

- `pnpm build:remote:desktop`：退出码 0；`pnpm build:remote:mobile`：退出码 0。串行 `pnpm build:remote` 在 240 秒包装超时，非构建失败；拆分门禁通过。
- `CDP_PORT=7615 pnpm cdp:smoke`：退出码 0。
- 修复前完整 Cloud E2E：host/controller 均在 WebSocket 握手前 `NETWORK`；CDP 直连显示 relay 原始 Upgrade 为 `101`，根因乃 `src/app.html` `connect-src` 漏租户子域。
- 修复后 `scripts/cdp-cloud-full-e2e.mjs`：`ok=true`、`connected=true`、能力集含 `pane/invoke/fs/git/search/workspace/theme/teammate`，目录 offset `0/3/6` 均成功，`keyBindingMode=enforced`。

### Sonar：历史快照与当前状态分离

以下指标是已取得的**历史认证分析快照**，不是当前 HEAD 的新结果：

- 项目：`MySetsuna_ridge`
- 分析：`b6a080ee-fc82-44aa-90c9-9c839acd2f81`
- unresolved issues：`0`
- Quality Gate：`OK`
- overall coverage：`82.6%`
- overall line coverage：`88.9%`
- overall branch coverage：`73.6%`
- duplication：`2.3%`
- new coverage：`84.5%`
- new line coverage：`92.9%`
- new branch coverage：`76.5%`
- Bugs / Vulnerabilities / Code Smells / Violations / Security Hotspots：均为 `0`

本轮当前复核：本地 SonarQube `26.7.0.124771` `/api/system/status` 为 `UP`；`.scannerwork-quality/report-task.txt` 记录 CE task `b7c45c6a-aa22-4781-a645-d2f56f8108fe`，但当前 shell 对项目状态、指标与分析接口均返回 `401`。故尚未取得当前 HEAD 的 CE 完成、Quality Gate、issues 或 project coverage 证据，`REQ-SONAR-COVERAGE-80-01` 仍记为 `PARTIAL`；不把历史 `OK` 快照冒充本轮闭合，也不把账户密码写入仓库。

接管动作：用当前有效凭据登录 `http://127.0.0.1:9000`，确认 Background Task 完成后再读取 `/api/qualitygates/project_status`、`/api/measures`、`/api/issues/search`；凭据只放本机密码管理器/环境变量，不写脚本、日志或文档。

截图中那类“覆盖率已过但项目仍 Failed”的直接原因已在旧分析记录中确认：分析 `c271e74b-ac3f-4277-bbef-74418f48b822` 的 `new_coverage=80.1%`、重复率 `0.84181%` 均通过，但 `new_violations=1`，触发 `rust:S3776`，故 Gate 为 `ERROR`；项目卡片的总覆盖率 `80.5%` 不会覆盖该条件。相关复杂度拆分及页面 title 修复已落地，后续认证分析 `b6a080ee-fc82-44aa-90c9-9c839acd2f81` 已记录为 issues `0` / Gate `OK`。

## 需求归纳：Agent 通信架构重构

### 来源链

1. 基线笔记：`Ridge 项目现状、愿景与规划基线（2026-07-21）`。
2. 深化来源：NotebookLM source `9516749e-c317-4f13-9cda-b64b00cec465`，标题 `Agent 通信架构重构`，关联笔记 `66919cb9-1329-4ddf-955c-f426d15a9fe6`。
3. 同等重点来源：临时对话 source `df4d5dcc-9813-4c61-ae9f-1e9199cb7555`，关联笔记 `f6ffd900-708d-44ee-9818-1a3269c533fc`。该来源补充 cookie/API 边界、MIME/大小/SHA256 校验、轮询与重试上限、token/cookie 脱敏、取消必须穿透到外部进程/队列的约束。
4. 仓库落点：`docs/REQUIREMENTS-SPEC.md` 的 `REQ-AGENT-COMMUNICATION-ARCH-REBUILD-01`；本地最终依据为 CodeGraph、源码、确定性测试与运行事实，NotebookLM 仅作候选与架构假设来源。

### 目标

将 Agent 身份、生命周期、通信、历史、恢复、编组和跨端投影收敛为 Kernel/Teammate 权威平面。桌面、Remote、`rdg` CLI、`ridge-mcp`、headless host 只能通过同一类型化契约读写，不得由 UI、CWD、pane 标题或单个 Tauri invoke 路径自行推断第二份身份事实。

### 必须满足的契约

1. **身份单一且可验证**：identity 至少携带 `agent_id`、`session_id`、`workspace_id`、`pane_id`、`cwd`、`executable/argv`、lifecycle、generation、lease、status、online、`last_seen` 与 capabilities。
2. **生命周期可观测**：`discovered → spawning/attaching → online → working/waiting/attention → completed/stopped/failed`；只有 spawn/attach 成功才提交 active entry，destroy/lease closure 成功才撤销；失败保留 diagnostic-only 记录。
3. **旧消息必拒**：重连/复用递增 generation，以 lease fencing 拒绝旧 generation、旧 lease、离线或已撤销目标。
4. **编组显式化**：group、leader、成员顺序、加入和移除均以 `group_id` 与 `agent_id` 维护，不按标题或 CWD 猜测。
5. **统一 envelope**：task/event/control/artifact/reply 均带 `message_id`、`idempotency_key`、conversation/task、from/to identity、workspace/pane、generation/lease、kind、sequence、timestamp、priority、deadline/cancellation、payload/artifact、ack/nack 与 typed error。
6. **发送前有界校验**：获取有界 roster 快照，校验目标 identity/generation/lease/online/capability；最多一次有界刷新，随后单次发送或返回稳定的 missing/offline/stale/generation mismatch/capability denied/timeout 错误。
7. **Hub 是控制平面**：`ridge-mcp` 负责 typed communication control plane；Message Hub 负责 inbox/topic/task/event/artifact 与 delivery，持久化使用 Rust/SQLite 或等价受控实现；工具返回 message/task/delivery ID 与 typed error，不以“已写入终端”冒充完成。
8. **投递有优先级**：Runtime API/SDK/app-server/HTTP/ACP 优先，其次 MCP pull，最后才是满足 agent idle、agent prompt、无审批竞争、目标前台等条件的 PTY fallback；PTY 只能 best-effort，失败保留 Inbox，不静默丢消息、不打断用户输入。
9. **历史与恢复隔离**：历史是 bounded cold path，按稳定 identity/session 分组；局部源失败可诊断但不阻断 live roster/input；resume 只使用结构化 executable/argv/CWD/session identity，不拼接未经转义的 shell 字符串，且操作 single-flight。
10. **跨端同语义**：桌面、Remote、headless 对 status、label、attention、`aria-label`、identity、history 使用同一 DTO/投影；跨 workspace 始终携带 `workspace_id`。
11. **安全与运维边界**：入口校验 workspace/agent/generation/lease、capability、权限；日志不记录 cookie、token、浏览器存储或未脱敏 payload；必须可观测 trace/message/ack latency/queue depth/cancel/stale rejection/teardown residue/recovery 结果。

### 当前代码落点

- Kernel roster/identity：`packages/ridge-kernel`。
- typed envelope、delivery policy、Hub/inbox/receipt：`packages/ridge-mcp`、`packages/ridge-mcp-bridge`。
- Teammate/Agent 适配与生命周期：`src-tauri/src/teammate`、`packages/ridge-core/src/teammate`、`rdg` adapters。
- 桌面 Agent Center/Commune、Remote roster/history/group：`src/lib/teammate`、`src/remote`。
- 稳定性与回收：process guard、PTY safety proof、generation/lease fencing、bounded queue、取消与 teardown 测试。

## NLM 来源需求：未闭合项推荐与接手矩阵

“需求门无 Pending”只表示 60 项已批准输入可执行，不表示所有验收事实已经发生。当前判定如下：

- **本轮已闭合（本机可复现）**：Cloud 租户 WebSocket CSP；本地 relay Upgrade、host/controller WebRTC + E2EE、D9 能力协商、目录分页 `offset=0/3/6`；前端/Rust 回归门；Remote desktop/mobile 拆分构建。
- **本轮部分闭合**：Agent 通信架构、Kernel/Teammate SSOT、typed envelope、generation/lease、Hub/adapter 与确定性 E2E 已有落点；真实 Agent CLI 五条件动态切换仍未以宿主运行快照证明。
- **尚未闭合**：当前 Sonar 认证分析、公网/TURN、物理移动端、双真实窗口/Host、PowerShell/PTY/DPR/NTFS 现场、第三方 Runtime/A2A。

| 优先级 | 追踪项 / 状态 | 责任落点 | 进入条件 | 闭合证据 |
| --- | --- | --- | --- | --- |
| P0 | Agent CLI 五条件运行态：`PARTIAL` | `packages/ridge-kernel`、`src-tauri/src/kernel_lifecycle`、RidgePane/Agent adapters | 解决或明确 Windows Job Object 的 `CREATE_BREAKAWAY_FROM_JOB` 边界；接入同一 host-owned runtime snapshot | 真 Agent CLI 在 discovered→spawning/attaching→online→working/waiting/attention→completed/stopped/failed 动态切换；generation/lease、PTY/审批/前台/用户输入五条件同一快照，且 teardown 归零 |
| P1 | Sonar 当前 HEAD：`PARTIAL` | `.tools` scanner、本地 SonarQube `:9000`、项目 `MySetsuna_ridge` | 有效凭据登录；CE task 完成；读取当前 project status/measures/issues | 当前分析 `Quality Gate=OK`、project coverage ≥80%、new violations=0；历史 `b6a080ee...` 不可替代当前证据 |
| P1 | 真实移动端 PWA/IME/后台/安全区：`EXTERNAL` | `src/remote`、mobile PWA | 真机或同等可控设备，含软键盘、旋转、后台恢复、safe-area、`visualViewport` | 真实设备视频/日志 + e2e：键盘弹出后 viewport/terminal geometry 稳定，断后台重连无重复 pane/残留 listener |
| P1 | 公网 WebRTC/TURN/E2EE/重连：`EXTERNAL` | `packages/remote` Cloud transport、relay/TURN | 公开域名、TLS、TURN 凭据、两端异网；不得用本地 `localhost` 替代 | host/controller 跨网 connected；ICE/TURN、重连、token refresh、E2EE binding、弱网/断网恢复均有脱敏日志 |
| P1 | 双窗口/双 Host/焦点隔离：`EXTERNAL` | Tauri deep-root、workspace/pane projection | 两个真实窗口与两个 Host 进程、不同 workspace/agent | 交叉操作、关闭/重连后 identity/workspace/pane 不串线；句柄、listener、子进程归零 |
| P1 | PowerShell/PTY/DPR/NTFS 跨卷权限：`EXTERNAL` | `src-tauri` terminal/filesystem、`packages/ridge-term`、Remote UI | Windows 真实 PowerShell/PTY、DPR/像素矩阵、不同卷与 ACL | 编解码/resize/IME/像素尺寸、跨卷 move/copy/delete、拒绝访问与回滚均有真实 e2e 证据 |
| P1 | 第三方 Agent Runtime/A2A 私有协议：`EXTERNAL` | `ridge-mcp`、`ridge-mcp-bridge`、runtime adapters | 明确第三方协议、凭据与可运行 sandbox | initialize/list/send/ack/reconnect/stop 的真实互操作；未知协议保持 typed failure，不宣称兼容 |
| P2 | Rust 编译警告债：`OPEN` | 各 Rust crate owner | 逐 crate 归因 dead/unused 与 `ts-rs` 解析告警，不改变语义 | `cargo test --workspace --all-targets` + `cargo clippy`/编译输出无新增未解释 warning；每一类有确定性测试 |

### 推荐下一迭代顺序

1. 先取得当前 Sonar CE/Gate 认证证据，避免用历史快照判断本轮质量。
2. 再解决 CDP/Windows Job Object 下的 Kernel 运行边界，完成五条件真实快照；该项是本机唯一 P0 未闭合项。
3. 随后按“公网 Cloud → 双窗口/Host → PowerShell/PTY/DPR/NTFS → 物理移动端”执行现场矩阵；每项先锁定环境与脱敏证据，再改代码。
4. 最后处理第三方 Runtime/A2A 与 Rust warning 债；发布、push、tag、Release 仍须用户另行授权。

## 边界与后续验收

以下不是本机源码可凭空证明的事实，交接时必须保留为外部验收项：真实手机/平板键盘与 PWA 几何、公开网络 WebRTC/TURN/E2EE、WebView2 长时 heap/RSS、双窗口/双 Host、PowerShell/PTY/DPR、真实 NTFS 与云凭据、第三方 Agent Runtime/A2A 私有协议。它们不能用 fixture 绿灯或 Sonar 覆盖率替代。

本轮未向 Codex 之外的 CLI、Agent 或 teammate 发消息，未 push、tag 或 release。工作区已有的 `.iteration`、coverage、截图、扫描目录和运行态文件属于既有运行产物，交接时不纳入本轮代码提交。
