# Ridge 在途需求与质量闭环交接（2026-08-11）

## 结论

本轮已接收并执行 60 项已批准需求；`docs/PENDING-REQUIREMENTS.md` 无待审批项，需求门禁通过。代码侧完成真实修复、测试补强与重复/死代码清理，未用 `NOSONAR`、规则降级或新增静态排除掩盖问题。

已提交的本轮相关提交：

| 提交 | 内容 |
| --- | --- |
| `c3504871` | Sonar 问题对应的真实 Rust/TypeScript 修复、测试补强与覆盖率提升 |
| `a151f538` | 修正 Git smoke 测试对可选 branch 的断言，恢复全 Rust 测试编译 |
| `1f86da34` | 删除 `ridge-mcp` 路由中已被真实实现取代的整段死注释分支 |
| `34bcf503` | 删除 `tmux` shim 中已被真实实现取代的死注释分支 |

## 可复验证据

### 代码与测试

- `pnpm check`：`0 errors / 0 warnings`。
- `pnpm test`：216 个文件通过，2008 passed，5 skipped，共 2013 项测试；退出码 0。
- `pnpm test:coverage:sonar`：216 个文件通过，退出码 0；归一化报告写入 `coverage/lcov.info`。本地 V8 未过滤汇总为 statements `67.46%`、branches `61.83%`、functions `69.35%`、lines `71.54%`；该汇总包含不属于 Sonar 产品覆盖率口径的本地脚本/辅助路径，不能替代 Sonar project metric。
- `cargo test -p ridge-core --lib`：344 passed。
- `cargo test -p ridge-mcp --lib`：90 passed，0 failed。
- `cargo test --manifest-path src-tauri/Cargo.toml --bin tmux`：27 passed，0 failed。
- `cargo test --workspace --all-targets`：全工作区通过；此前唯一编译阻断为 Git smoke 测试把 `Option<String>` 当成 `String`，已在 `a151f538` 修正。
- `git diff --check`：相关代码差异无空白错误。

### Sonar

以下为本轮已取得的**认证分析快照**，不是未认证 API 的推测：

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

Sonar 的最新认证快照已通过 Quality Gate；本轮后续提交只涉及测试契约修正与删除死代码，不会重新引入生产逻辑问题。当前 shell 未配置 Sonar 凭据，未伪造新的 CE/Gate 结果；若网页仍显示 `Failed`，须打开该次分析的 Quality Gate conditions 与 Background Task，确认页面显示的是哪一个 analysis，不能只看项目卡片上的覆盖率数字。

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

## 边界与后续验收

以下不是本机源码可凭空证明的事实，交接时必须保留为外部验收项：真实手机/平板键盘与 PWA 几何、公开网络 WebRTC/TURN/E2EE、WebView2 长时 heap/RSS、双窗口/双 Host、PowerShell/PTY/DPR、真实 NTFS 与云凭据、第三方 Agent Runtime/A2A 私有协议。它们不能用 fixture 绿灯或 Sonar 覆盖率替代。

本轮未向 Codex 之外的 CLI、Agent 或 teammate 发消息，未 push、tag 或 release。工作区已有的 `.iteration`、coverage、截图、扫描目录和运行态文件属于既有运行产物，交接时不纳入本轮代码提交。
