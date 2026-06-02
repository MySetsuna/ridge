# 设计文档：teammate 分屏放置与自适应（teammate-pane-layout）

> 状态：**已批准 / ready-for-tasks**（team-lead 终审通过，吸收 reviewer round-1/2）。决策全锁：DA=A1（auto_place 契约）/ DB=H1（锁定+fail-closed）/ DC=5b（resize-pane no-op）/ DE=启动即 Busy（id 可空）/ DF=② child-exit→Idle 可复用 / 识别=意图驱动不自动检测。
> 链路：`src/bin/tmux.rs`(shim) → `POST /api/v1/split-window` → `teammate/server.rs::route_split` → `commands/pane.rs`；前端 `manager.ts` / `paneTree.ts` / `RidgePane.svelte` / `+page.svelte`。

## 0. 调用链与关键证据（已核对 · file:line）

| 环节 | 位置 | 行为 |
|---|---|---|
| shim 解析 `-h`/`-t` | `tmux.rs:739` / `:746-753` | `horizontal=true` / 裸 `0`→`pane_index=Some(0)`（`parse_pane_target:480-481` bare-number 分支） |
| shim native 判定 | `tmux.rs:338 use_native` + `target_is_session_qualified:317` | `-t 0` 非会话限定 → GUI 路径（reviewer 已坐实） |
| shim GUI 转发 | `tmux.rs:838-848 post_split` / `:871-874` | `horizontal` 无条件写、`pane_index` 仅 Some 时写 |
| 空闲复用门槛 | `server.rs:514` | `allow_idle_reuse && pane_index.is_none()` |
| 最大-pane 选择 + 方向推断 | `server.rs:607-626` / `:619-623` / `select_split_target:275` | 仅 None 才走；方向用 `pane_sizes`（依赖真实 fit） |
| 最长边切分（纯函数+单测） | `pane.rs:539 balanced_split_decision` | 已实现且有单测 |
| 工作区锁定 + 回退 | `server.rs:51 caller_workspace_id` / `:62` | 头优先，失败回退 `active_workspace_id()` |
| 新面板尺寸种子 | `server.rs:755` | `(80,120)` |
| GUI split 补 fit | `paneTree.ts:1399 scheduleForceFitAfterSplit`（定义 `:1422`，重试 2f/50/150/400ms） | GUI split 专属 |
| teammate 事件→同步 | `+page.svelte:1075-1090` | 仅 `syncPaneLayoutFromBackend()`，**不**补 fit |
| RidgePane 挂载顺序 | `RidgePane.svelte`: attach `:904` → ensurePtyBridge `:955` → setPaneDeltaMode `:1009` | attach rAF fit 必早于 Channel 注册 |
| fit 自愈/0×0 守卫 | `manager.ts:3994-4000`（自愈）/ `:3918-3927`（0×0 early-return，不更新 lastReported） | — |
| 其它 attach 期 fit | `manager.ts:1685`（attach rAF）/ `:1663`（worker bindCanvas）/ ResizeObserver 初次回调 `:1612`→`:3815`（~500ms） | reviewer 补充 |
| Starting 构造 | `server.rs:525-528` / `:749-754` | `program.is_some()?Busy:Starting` |
| spawn-process 不改状态 | `server.rs:977 route_spawn_process` | 只设 cursor + emit active-pane-changed |
| **new-window 恒 Busy** | `server.rs:1368` | 第三态（reviewer 补充） |
| register-agent 提升器 | `server.rs:374` / `register_agent_to_pane:338` | **全仓无 shim/harness 调用方** |
| release-pane | `server.rs:347 release_pane`（走 caller_workspace_id）/ 路由 `:416` | **同样无 harness 调用方**（reviewer 坐实） |
| **child-exit 现成钩子** | `pty.rs:487-508`(native→close 销毁) / `:509-522`(ordinary→`PaneClosed` 重建 shell) | 退出语义因 pane 类型而异 |
| active_workspace_id 绑定面 | `terminal.rs:942 resize_pane` / `pane.rs:396 register_teammate_agent` / `:424 release_teammate_agent` | 范围比原稿广 |
| 事件 payload 形状 | `{trace_id}`/`{reused,pane_id}`/`{detached_pane}`/`()` | `teammate-layout-changed` 已过载 |

---

## 1. 观察项 1/2/3：智能放置被样板参数旁路（DA=A1）

### 根因确认（CONFIRMED · reviewer 坐实）
harness `tmux split-window -t 0 -h -l 70%`；`use_native(Some("0"))==false`→GUI 路径；`-t 0`→`pane_index=Some(0)`、`-h`→`horizontal=true`；后端三套放置（`server.rs:514`/`:607-626`/`:630-638`）以 `pane_index.is_none()`/`!horizontal` 为前置被旁路。

### 修复方案（A1 + 显式契约，已采纳 reviewer 建议）
- shim 在 GUI 路径（`use_native()==false`，`tmux.rs:838`）发起自动放置。**采用显式 `auto_place: bool` 契约**而非用 `pane_index=None` 隐式编码意图：`SplitBody` 增 `auto_place`（`#[serde(default)]`）；shim GUI 路径置 `auto_place=true` 且不转发样板 `-t`/`-h`；后端三处前置条件改为「`auto_place`（忽略 pane_index/horizontal 做选择）」。消除 `pane_index=None` 的重载二义性（缺省未传 vs 显式自动）。native 路径不传 `auto_place`，行为不变。
- 改动点：`tmux.rs:cmd_split` GUI 分支 `:838-848`；`server.rs SplitBody:472-497` + `route_split:514/601/630`。

### 风险
- A1 结构上不触达 native 分支（命中即 `return`，reviewer 坐实），安全。
- **[与 5b 耦合，新增]**：最大-pane 方向推断 `server.rs:619-623` 依赖 `pane_sizes`；5b 未修时该值可能是陈旧 `(80,120)` 种子（`:755`）→ 方向推断退化（可能恒定一边）。故 1/2/3 与 5b 存在数据依赖，并行时需注意：5b 修好后方向推断才完全可信。
- `-l 70%` 尺寸提示丢弃 → 新面板 50/50（详见 §3 与 §5，与 5b 窄-pane 回归耦合）。

### 分期：**P1**（一次修 1/2/3）。

### 验收标准（含测试）
- **AC1.1（shim 单测，已修正措辞）**：GUI 路径解析 `["-t","0","-h","-l","70%"]` → 提交 body `auto_place==true`、不含 `pane_index`、`horizontal==false`（机制不同：pane_index 省略 / horizontal 显式 false / auto_place 显式 true）。落点 `src/bin/tmux/socket_routing_tests.rs`。
- **AC1.2/1.3（后端集成）**：`auto_place=true` 下 有空闲面板→`reused=true` 叶子不增；无空闲面板→切自最大叶子、方向同 `balanced_split_decision`。
- **AC1.4（回归）**：native 路径（不带 auto_place）`-t` 行为不变；`balanced_split_decision` 既有单测通过。

---

## 2. 观察项 4：工作区隔离（DB=H1 锁定 + fail-closed，实做项）

### 现状与缺口
`caller_workspace_id`(`server.rs:51-62`) 头优先锁定发起方工作区；头缺失/无效/指向不存在工作区时**静默回退 `active_workspace_id()`**(`:62`) → 跨区误放。

### 设计前提（已核验 + round-2 reviewer 纠正论证基础）
**fail-closed 不会误杀「可达 split 的合法进程」** —— 真因是「到不了 `route_split` 的进程根本拿不到 Ridge shim」，而非「注入无条件」（reviewer 纠正：注入对 `(None,None)` arm 是**跳过**的）：
- **门控耦合（关键不变量）**：`prepend_path_with_wind_tmux_shim`(`commands/terminal.rs:540`，把 Ridge `tmux` shim 加到 PATH) 与 `RIDGE_WORKSPACE_ID` 注入(`:551`) **位于同一 `(Some(bind),_)` arm、被同一条件门控**。
  - 凡能经 Ridge shim 发起 split 的进程 → 必同时拿到 shim 与 workspace-id 头。
  - `(None,None)` arm(`:560`)起的 shell → **既无 workspace-id 也无 shim** → 它跑 `tmux split` 命中系统 tmux，**根本到不了 `route_split`**，fail-closed 不处理它 ⇒ 不误杀。
- `(None, Some(_))`(`:534-538`)：teammate server 未绑定时结构化命令 spawn 已 fail-loud。
- 嵌套 teammate：shim 回传头(`tmux.rs:231-238`)，agent 子进程继承 env → 携带。
- **不变量须钉死的是 `:540`↔`:551` 两行同 arm**（而非单钉 `:551`）：加注释 + 一条断言/测试保证「shim 与 workspace-id 同生同灭」。若未来拆到不同条件（某类 shell 拿 shim 但不拿 id），fail-closed 立即误杀。

### 修复方案（H1）
- 改 `caller_workspace_id` 返回 `Result`（或在 `route_split`/`route_spawn_process` 调用点判定）：头缺失/解析失败/工作区不存在 → **不回退**，返回明确错误（如 HTTP 400/409），**拒绝该 teammate 放置**。
- 错误文案区分两类（便于排障）：
  - 缺失/格式错：`teammate split rejected: missing or invalid X-Ridge-Workspace header (RIDGE_WORKSPACE_ID not propagated to agent env)`。
  - 工作区不存在/已关闭：`teammate split rejected: originating workspace <id> no longer exists`。
- 可观测：`teammate_metrics` 增计数（如 `failures["workspace_rejected_missing_header"]` / `["workspace_rejected_unknown"]`）+ 结构化日志（含来源判别）。
- 范围：仅 teammate HTTP 放置路由（split / spawn-process / new-window / register/release）统一走 fail-closed；GUI 内部命令不受影响。

### 风险
- 若未来出现「合法但 env 未注入」的新启动路径，fail-closed 会拒绝 → 需保证任何新 agent 启动路径都经 `terminal.rs:551` 注入（已是现状）；在该处加注释钉死此不变量。
- 发起工作区在 agent 存活期间被关闭 → 后续 split 被拒（合理；agent 应随工作区生命周期）。

### 分期：**P1**（与 1/2/3 同属 shim/后端浅层改动，独立）。

### 验收标准（含测试）
- **AC4.1（集成）**：带 `X-Ridge-Workspace=WS-A`、焦点 WS-B → 面板落 WS-A（锁定不变）。
- **AC4.2（集成，H1）**：缺头 → 返回明确错误、对应指标 +1、活动工作区**无**新面板。
- **AC4.3（集成，H1）**：头指向已关闭工作区 → 返回「workspace no longer exists」错误、指标 +1、无误放。
- **AC4.4（回归）**：正常注入路径（RIDGE_WORKSPACE_ID 已设，当前会话）spawn 不受影响、行为不变。

---

## 3. 观察项 5b：终端尺寸不跟随 pane resize（DC，根因已按 reviewer 修正）

### 根因确认（修正版 · PARTIAL→精确化）
- **race 真实**（CONFIRMED）：RidgePane onMount = attach(`:904`)→ensurePtyBridge(`:955`)→setPaneDeltaMode(`:1009`)；attach 的 rAF fit（`manager.ts:1685`，~16ms）必早于 pty-delta Channel 注册 → **首帧 Resize delta 被丢，kernel 卡编译期默认 80×24**（`manager.ts:3985-4006` 自愈注释坐实）。
- **teammate 面板缺「确定性补 fit」**：teammate 经 `+page.svelte:1075` 仅 `syncPaneLayoutFromBackend()`，**不**走 GUI split 专属的 `scheduleForceFitAfterSplit`（`paneTree.ts:1399`）。
- **「永久卡死」需收紧（reviewer 纠正原稿过度声明）**：可见工作区下，attach 之后还有 ResizeObserver 初次回调（`manager.ts:1612`→`:3815`，~500ms）与 worker bindCanvas fit（`:1663`）会再触发 fit，自愈守卫 `rows!==kernelRows`（`:3994-4000`）**通常能在 ~500ms 内自愈**。真正**永久卡死的子集 = 0×0/隐藏工作区**：`fitPane` 在 `wCss<=0` 时 early-return 且不更新 lastReported（`:3918-3927`），所有 fit 空转。故把原「次要」的 `active_workspace_id`/0×0 **提为 co-primary**；可见工作区表现为「创建后最长约 500ms 的错误尺寸窗口 + 时序不利/窄容器下偶发持续」。

### 修复方案（已按 reviewer 改为确定性根治）
- **主修（确定性，复用已验证 fix）**：让 teammate 面板 attach 路径 **await Channel 注册（`setPaneDeltaMode`）完成后再 `fitPaneNow`**——即记忆库 `bug_split_kernel_race.md` 已验证的形态（await 把 fit 排到 Channel 注册之后，而非赌定时重试）。落点：`RidgePane.svelte` onMount 对 teammate-origin 面板，或 `manager.attach` 内序列化 fit 到 Channel-ready 之后。
- **安全网（降级，非主修）**：`scheduleForceFitAfterSplit` 用于 0×0→可见切换等兜底；**不**让后端事件携带 `new_pane_id` 去驱动前端 fit 时序（层次倒置，reviewer 反对）——改由前端 onMount/diff 自决。
- 0×0/隐藏工作区：切回可见时强制重 fit（呼应 `onActiveWorkspaceChanged` `manager.ts:840`）。

### 风险
- 触及 fit/resize 热路径与 ConPTY 静默窗口（§1.24/§A.3/§1.26）；**多发 resize 会放大撞静默窗口概率**——故主修走 await-then-fit（单次确定性）优于多时点重发。Windows ConPTY 重点回归。
- **窄-pane 回归（与 `-l` 耦合）**：A1 后 teammate 面板可能 50/50 比 harness 期望的 70/30 更窄 → cols 更小 → Ink 重排更剧烈。ConPTY 回归须含「窄 pane」工况。

### 分期
- **P1**：await-Channel-then-fit 确定性主修 + 安全网。
- **P2**：`resize_pane` 等从 `active_workspace_id` 解耦（见 §5）。

### 验收标准（含测试，改为因果断言）
- **AC5.1（前端单测）**：teammate 面板 attach 流程中，`fitPaneNow` 的首次有效调用**发生在** `setPaneDeltaMode` resolve 之后（断言顺序/因果，非时延）。
- **AC5.2（集成）**：Channel 注册完成后的首次 fit 上报的 cols/rows == 容器换算值（**未修复时此断言会失败**——保证验收有区分度；勿用「重试窗口内」这类 flaky 时延断言）。
- **AC5.3（集成）**：拖分隔条改变 teammate 面板行列 → PTY 收到匹配 resize、TUI 重排；**复用路径**（`{reused,pane_id}` 事件）创建的面板同样达标（覆盖 reuse 分支）。
- **AC5.4（回归，跨平台）**：GUI split 仍正常；Windows ConPTY §1.26 prompt 不塌缩；窄-pane 工况下 Ink 重排正常。

---

## 3bis. 观察项 6：状态卡在 Starting（根因 CONFIRMED + reviewer 修正/补充）

### 根因确认（concrete · file:line）
**主因：tmux-harness 启动流里没有代码把 teammate 面板从 `Starting` 提升到 `Busy`。**
- 无结构化 program → `Starting`（`server.rs:525-528`/`:749-754`）；`split-window -t 0 -h -l 70%` 无 trailing command → 新建即 Starting。
- 真实 agent 经 `route_spawn_process`(`server.rs:977`，send-keys 结构化启动 `tmux.rs:1212`→`post_spawn_process`→`spawn-process`)落入，**不碰** `teammate_pane_states` → 永远 Starting。
- 提升器 `/api/v1/register-agent`(`server.rs:374`)与 `release-pane`(`server.rs:347`) **全仓均无 shim/harness 调用方**（reviewer 双确认）→ Starting→Busy 与 Busy→Idle 两端在 harness 流程里都没有触发器。
- **③（前端不刷新）排除**：`teammate-layout-changed`→`syncPaneLayoutFromBackend()` 已接线，`get_pane_layout` 序列化 `agent_state`（`pane.rs:60-64`）。翻转即更新；问题是翻转从不发生。

**reviewer 修正/补充（已吸收）：**
- **「结构化 program→直接 Busy」措辞收紧**：仅当 **program 内嵌于 split-window 命令本身**（`body.program.is_some()`）才直接 Busy。Claude Code 真实主路径 = split(无 cmd)→Starting + send-keys 结构化(spawn-process)→**仍 Starting**，根本不经过 Busy 分支。即主路径**必卡 Starting**，F1 的提升入口**必须覆盖 send-keys/spawn-process**，而非只覆盖内嵌-program split。
- **第三态 new-window**：`route_new_window`(`server.rs:1368`)**恒 Busy**。故三路径状态语义不一：new-window→Busy / split→Starting / spawn-process→不变。F1「消除不对称」需声明是否一并覆盖 new-window。
- **child-exit 终局因 pane 类型而异（关键，新增 DF）**：`pty.rs:487-508` native 分支 → `pane_tree.close` **销毁** pane；`:509-522` ordinary 分支 → `PaneClosed` **重建 shell**（但两分支都**不重置** `teammate_pane_states`）。GUI teammate split 多为 ordinary（route_split 普通 PTY）→ 退出后 shell 重建但状态可能残留 Busy/Starting（反向泄漏）。AC6.4「退出→Idle 可复用」与现状（ordinary 重建 shell / native 销毁）均不一致 → 需用户决策 DF。

### 识别口径（用户已定）：意图驱动，**不做自动检测**
- 手动在普通 pane 敲 `claude` **不要求**识别为 agent（不引入前台进程名嗅探）。**故原备选 F2（前台进程驱动）明确移出范围。**
- agent 身份完全由「F1 意图位」驱动；`agent_id` 为可选元数据（DE：启动即 Busy，能解析则填，否则 Busy 无 id）。

### 修复方案（F1 提升 + child-exit→Idle + F4 安全网）
- **F1（确定性提升器，DE=启动即 Busy/id 可空）**：`SpawnProcessBody`/`SplitBody` 增意图位（`is_agent: bool`，附可选 `agent_id`）；shim 在 teammate agent 启动时透传。收到意图 → 置 `Busy` + （若有 id）写 `teammate_agent_pane_map` + emit。
  - **必须覆盖 `route_split` 的两条路径 + spawn-process（与 #1 衔接，核心）**：
    - ① **idle-reuse 分支**（`server.rs:514-585`）：现状 `:525-529` 仅 `program.is_some()` 才 Busy、否则 Starting；而真实 agent 走 command/spawn-process（program 常 None）→ **征用的空闲 pane 会卡 Starting**。F1 意图位须让复用路径**立即 Busy**。
    - ② **新 split 分支**（`server.rs:749-754`）：同样按意图位 Busy。
    - ③ **send-keys → spawn-process**（`server.rs:977`，harness 真实主路径）：置 Busy（当前完全不改状态）。
  - 背景：DA=A1 后 **idle-reuse 是主导放置路径**，「征用空闲 pane → 立即 Busy badge」是核心场景，非边角。
- **child-exit → Idle（DF=②，P1 必需，与 F1 同期）**：agent 退出 → 面板按分支达成终局；**reviewer round-2 已坐实反向泄漏真实存在，并细分了严重度与平台/路径差异**：
  - **ordinary 分支（`pty.rs:509-522`，GUI teammate split 即此类）→ DF=② 成立**：`PaneClosed`→前端 `ptyBridge.ts:121-149` **用同一 paneId 重建 shell**、叶子在 pane_tree 存活、重建经 `(Some(bind),_)` arm 拿到 shim+id。故**只需重置 `teammate_pane_states[pid]=Idle` + 清 agent 映射**即足（无需重建 pane，reviewer 确认 design 此判断正确）。现状 `detach_terminal:175-181` 与 `PaneClosed` 消费 `lib.rs:316-335` 均**不**重置 teammate 状态 → 必补。
  - **native 分支（`pty.rs:487-508`）→ DF=② 语义不成立**：叶子已 `pane_tree.close` 销毁，无 pane 可保留为 Idle。终局退化为「**销毁 + 销毁前清 `teammate_pane_states`/agent 映射**」防孤儿。**AC6.4 须按分支分裂断言**（ordinary→Idle 可复用；native→pane 消失且 map 无残留）。GUI teammate split 多为 ordinary，主路径无碍。
  - **[BLOCK①·已裁决 = 采纳 (i)，team-lead 终审]**：idle-reuse 的 command-注入子路径（`server.rs:560-565` 把 `claude\n` 写进既有 shell，不建独立 PTY/reader → agent 退出不 EOF → 永卡 Busy）**禁止用于 agent 启动**。**裁决**：agent 启动（含 idle-reuse）**一律走结构化 spawn**——F1 在 reuse 分支也用 `ensure_pane_pty_workspace` 结构化起 agent PTY、**替换该 idle pane 原 shell 的 PTY**；agent 退出 → EOF → child-exit → ordinary 终局（重建 Idle shell，DF②）。
    - **实现约束**：agent-intent **必须携结构化 program/args/env**；若仅拿到 command 字符串无法结构化 → **拒绝该 agent 复用路径**（或降级并记一条 metric，由 F4 兜底），**不得静默走 command-write**。
    - **F4 看门狗维持 P2**（不提前到 P1）——结构化 spawn 已确定性提供 EOF，F4 仅作残余兜底。
  - **F1↔child-exit 必须同期上线**：F1 把更多 pane 提到 Busy，而 Busy 泄漏会**阻塞复用**（比 Starting 泄漏更糟，后者 `find_idle_pane_index` 仍可复用）→ 单独上 F1 会制造比现状更严重的 Busy 卡死。此为 P1 内部硬序约束。
  - 由 Ridge 自身信号（结构化 spawn 的 reader EOF / F4 探测）驱动，**不依赖 harness 调 release-pane**（已证无调用方）。
- **F4（看门狗安全网，P2）**：PTY active + 宽限期后仍 Starting → 有存活子进程则 Busy，否则清 badge。**按 pane 所属 workspace 操作，禁读 `active_workspace_id`**。

### 风险
- 过度声明 → 用显式意图位规避。
- **生命周期反向泄漏（已升级为 P1 必需）**：见上 child-exit。
- 跨工作区：提升/释放走 HTTP/`caller_workspace_id`，**勿走** 绑 `active_workspace_id` 的 GUI 命令（`pane.rs:396/424`）；F4 同此约束。
- 并发：cursor 兜底映射（`server.rs:392-399`）竞态 → 优先显式 pid 贯穿。

### 分期（已修正自相矛盾）
- **P1**：spike（证实主路径 split-无cmd→spawn-process；grep 确认 register-agent/release-pane 无调用方；确认 #6 面板属 ordinary 还是 native 分支、PaneClosed 是否重置 teammate 状态）→ F1 提升（覆盖 spawn-process）→ **child-exit→终局清理（与 F1 同期）**。
- **P2**：F4 看门狗；`active_workspace_id` 解耦。

### 验收标准（含测试，覆盖全生命周期 + badge + 跨平台 + 并发）
- **AC6.1（后端单测）**：带意图位的 spawn-process 落入 Starting 面板 → `teammate_pane_states[pid]==Busy`、映射写入、emit（**因果断言，非时延**）。
- **AC6.2（后端单测）**：`get_pane_layout` 序列化 `agent_state=="busy"` + `agent_id`。
- **AC6.3（集成）**：split(无cmd)→`STARTING`；send-keys 结构化 agent 启动 → 翻转 `AGENT`（断言「收到 spawn-process 后状态同步为 Busy」这一因果，而非「N ms 内」）。
- **AC6.4（生命周期，DF=②，按分支分裂）**：(a) **ordinary** 面板 agent 退出 → 置 `Idle`、清 `teammate_agent_pane_map`、badge 消失、可被需求 1 复用；(b) **native** 面板 agent 退出 → pane 销毁且 `teammate_pane_states`/映射**无残留孤儿**。两者 `teammate_pane_states` 均不得残留 Busy/Starting。
- **AC6.5（征用空闲 pane，核心 · 后端单测+集成）**：reuse 分支 `program=None` + 意图位 → `teammate_pane_states[reused_pid]==Busy`、前端立即 AGENT badge；agent 退出 → 转 `Idle`。**须分别覆盖结构化-spawn 复用 与 command-注入复用两子路径**（后者无独立 reader EOF，验证所选降级方案（结构化 spawn 或 F4 探测）确实把它降回 Idle，而非静默卡 Busy）。
- **AC6.6（回归，措辞收紧）**：**内嵌-program** split 仍直接 Busy；GUI `register_teammate_agent` 仍可用；new-window 行为按 F1 决议一致。
- **AC6.7（无永久 Starting/Busy · F4）**：提升/降级信号缺失时看门狗在宽限期后修正；断言无面板长期停 Starting，且无 command-复用 pane 长期卡 Busy。
- **AC6.8（按 spawn/command 路径，非纯平台）**：(a) 结构化 spawn 的 agent 退出 → reader EOF（`pty.rs:241` `Ok(0)`，Unix slave 全关 / Windows ConPTY pipe 关闭抽象一致）→ 终局达成，**Windows ConPTY 与 Unix PTY 一致**；(b) command-注入到既有 shell → 两平台**都不 EOF**，须由所选降级方案兜底（呼应 AC6.5/6.7）。
- **AC6.9（并发，新增）**：N 个并发 split-window → N 个不同叶子、无 pane_index 串号、无状态错配。

---

## 4. 决策汇总（全部锁定 · team-lead 终审通过）

| 编号 | 决策点 | 状态（全部锁定） | 影响 |
|---|---|---|---|
| **DA** | GUI 路径定向 | **已定：A1 自动放置 + 显式 auto_place 契约**；native 仍尊重 | 需求 1 |
| **DB** | 工作区隔离/回退 | **已定：H1 锁定 + fail-closed（拒绝 + 指标，不回退）** | 需求 2 |
| **DC** | resize-pane / 自适应目标 | **已定：resize-pane no-op，目标=终端跟随 pane resize（5b）** | 需求 3 |
| **DE** | `Busy` 是否必须带 `agent_id` | **已定：启动即 Busy，agent_id 可选元数据（能解析则填）** | 需求 4 |
| **DF** | agent 退出后面板终局 | **已定：② 保留为 Idle 可复用（child-exit→Idle）** | 需求 4 / AC6.4 |
| 识别口径 | agent 身份识别方式 | **已定：意图驱动，不做自动检测（F2 移出范围）** | 需求 4 |

## 5. 范围外 / 候选追加
- `-l <pct>` 尺寸尊重（当前丢弃，50/50）——候选 P3；但**5b 的 ConPTY 回归须先覆盖窄-pane 工况**（与 5b 耦合）。
- `active_workspace_id` 解耦——范围含 `resize_pane`(`terminal.rs:942`) + `register_teammate_agent`/`release_teammate_agent`(`pane.rs:396/424`)三处（P2）。
- **`teammate-layout-changed` payload 收敛为带 `kind` 判别字段的封套**——A1/5b/6 三个 P1 任务都会动这个 handler（`+page.svelte:1075`），是它们唯一真实耦合点；**应在三任务并行前先做**（P1 前置）。

## 6. 总分期视图（已与细分期对齐）
- **P1 前置**：`teammate-layout-changed` payload 封套收敛（解并行冲突）。
- **P1（三项，注意 1/2/3↔5b 数据耦合）**：①1/2/3 shim 自动放置 + auto_place 契约；②5b await-Channel-then-fit 确定性主修；③6 的 F1 提升 **+ child-exit→终局清理（同期）**。
- **P2**：6 的 F4 看门狗；`active_workspace_id` 三处解耦。
- **P3/范围外**：`-l <pct>`；5a resize-pane 实装（当前明确不做）。

## 6bis. 决策 round-2 更新（team-lead 转达，已并入）
- **DB 更正**：从「保持现状」→ **H1 锁定 + fail-closed**（见 §2，已确认 RIDGE_WORKSPACE_ID 注入可靠 `terminal.rs:551`，fail-closed 不误杀正常 spawn）。#4 由「无需改动」升为 P1 实做项。
- **DE 锁定**：启动即 Busy、agent_id 可选。
- **DF 锁定**：② child-exit→Idle 可复用。
- **识别口径**：意图驱动，不做自动检测 → F2 前台嗅探移出范围。
- **征用空闲 pane → Busy（核心）**：F1 提升须统一覆盖 `route_split` idle-reuse 分支（`server.rs:514-585`，现状 program=None 卡 Starting）+ 新 split + spawn-process；DA=A1 后 reuse 为主导路径，已加 AC6.5 + 单测要求。

## 7. reviewer round-1 已吸收项（审计追踪）
- [CRITICAL] 5b 根因过度声明 → 已收紧，0×0/隐藏工作区提为 co-primary，主修改为 await-then-fit，AC 改因果断言。
- [HIGH] 6「结构化→Busy」措辞 → 已限定为内嵌-program 子集，明确主路径必卡 Starting、F1 须覆盖 spawn-process。
- [HIGH] release-pane 无调用方 + child-exit→Idle → 提为 P1 必需，新增 DF 决策与 AC6.4/6.7。
- [HIGH] 显式 `auto_place` 契约 → 已采纳，AC1.1 改断言意图位。
- [HIGH] 事件 payload 过载 → 列为 P1 前置封套收敛。
- [MEDIUM] new-window 第三态 / 1-3↔5b 耦合 / active_workspace_id 范围更广 / 并发 + 复用路径 + 跨平台测试缺口 → 均已补入风险与 AC。

## 7bis. reviewer round-2 已吸收项（审计追踪）
- **[BLOCK①·已裁决=采纳(i)]** command-注入复用无 reader EOF → **agent 启动一律结构化 spawn**（reuse 也替换 idle pane 的 shell PTY，独立 PTY 可 EOF）；command-only intent 无法结构化 → 拒绝/降级+metric，**不得静默 command-write**；**F4 维持 P2**。已落 §3bis + AC6.5/6.7/6.8 + tasks T3。
- **[BLOCK②·已裁决=接受分裂语义]** native 分支 DF② 不成立：ordinary→重置 Idle 可复用；native→销毁 + 清 `teammate_pane_states` 条目。不变量：两分支**绝不残留 Busy/Starting 孤儿**。AC6.4 ordinary/native 分裂保留。
- **[应修订] §2 H1 论证基础错误**：fail-closed 安全真因是「不可达 split 的进程拿不到 shim」（`terminal.rs:540`↔`:551` 同 arm 门控），非「注入无条件」。已改写 §2，钉死门控耦合不变量。
- **[应修订] requirements 头部 stale**（DB=保持现状/DE 待审）→ 已同步为锁定决策。
- **[应修订] AC6.8 纯平台二分 → 改为按 spawn/command 路径断言 EOF 终局**；AC6.5 补结构化-vs-command 复用差异断言。
- **[已确认正确]** H1 fail-closed 结论安全；idle-reuse 卡 Starting 真实；反向泄漏真实；ordinary 分支「重置状态即足」（PaneClosed 用同 paneId 重建 shell `ptyBridge.ts:121-149`）。
