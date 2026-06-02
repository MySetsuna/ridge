# 任务分解：teammate 分屏放置与自适应（teammate-pane-layout）

> 状态：已批准（team-lead 终审）。决策全锁：DA=A1 / DB=H1 / DC=5b / DE=启动即Busy(id可空) / DF=②child-exit→Idle / 识别=意图驱动不自动检测。
> 引用：根因与 file:line 见 `design.md`；验收条款见 `requirements.md`。每个 task 自包含、可被 coder 认领、含 AC/测试与完成定义（DoD）。
> 约定：路径前缀 `src-tauri/src/` 简写为 `@/`；前端 `src/` 简写为 `~/`。**本阶段 TDD：先写测试（RED）→ 实现（GREEN）→ 重构。**

## 依赖图（关键序约束）

```
T0(事件封套收敛, P1前置) ──► T2(5b)         ┐
                          └─► T3(#6 生命周期) ┤
T1(1/2/3 shim auto_place) ──(数据耦合: 方向推断依赖真实 fit)──► 受益于 T2
T1 ──(DA=A1 使 reuse 成主导)──► T3 的「征用空闲 pane→Busy」
T4(#4 H1) 独立，可任意时点
T5(F4 看门狗 + active_workspace_id 解耦) 依赖 T3
```
- **硬序**：T2/T3 改 `~/routes/+page.svelte:1075` 同一 handler → 必须等 T0 的 payload 封套落地后再并行，避免冲突。
- **硬序**：T3 内 **F1 提升与 child-exit→Idle 必须同一 PR/同期**（Busy 泄漏阻塞复用，比 Starting 泄漏更糟）。
- T1 与 T2/T3 后端/前端分层不重叠，可并行；仅注意 T1 方向推断在 T2 修好前数据可能陈旧（功能不阻塞）。

---

## T0 ·〔P1 前置〕`teammate-layout-changed` payload 封套收敛  〔= 任务系统 #8，进行中〕

**目标**：把当前 4–5 种异形 payload（`{trace_id}` / `{reused,pane_id}` / `{detached_pane}` / `()` / 计划新增的 new-pane）收敛为带判别字段的统一封套，避免 T1/T2/T3 三个并行任务在同一前端 handler 上打架。

**改动点**
- 后端所有 emit 点：`@/teammate/server.rs:566-569`(reuse)、`:767-770`(new)、`@/engine/pty.rs:504-507`(detached)、`@/commands/pane.rs`(register/release)。
- 前端消费：`~/routes/+page.svelte:1075-1090`。

**实现要点**：定义 `{ kind: "split-new" | "split-reuse" | "detached" | "agent-state" | "generic", pane_id?, ... }` 封套（参照全局 patterns 的 API 信封）；前端按 `kind` 分发；保持向后兼容（缺字段时退回 generic 全量 re-sync）。

**AC / 测试**
- 单测：每种 emit 产出的封套含正确 `kind` + 必要字段。
- 前端单测：handler 按 `kind` 分发到对应分支（generic → 全量 re-sync）。
- 回归：现有 `teammate-layout-changed` 驱动的 re-sync 行为不变。

**DoD**：封套类型 + 双端单测绿；T1/T2/T3 可在其上并行改 handler 无冲突。

---

## T1 ·〔P1〕1/2/3 — shim GUI 自动放置（显式 `auto_place` 契约）  〔= #3〕

**目标**：teammate GUI split 一律 Ridge 自动放置（idle 复用→最大 pane→最长边），忽略 harness 样板 `-t 0/-h`；native 路径仍尊重 `-t`。

**根因（design §1）**：shim 把样板 `-t 0`→`pane_index=Some(0)`、`-h`→`horizontal=true` 转发（`@/bin/tmux.rs:739/746/871`），后端三套放置以 `pane_index.is_none()`/`!horizontal` 为前置被旁路（`@/teammate/server.rs:514/607-626/630-638`）。

**改动点 / 实现要点**
1. `@/teammate/server.rs` `SplitBody`(:472-497) 增 `#[serde(default)] auto_place: bool`。
2. `route_split`(:514/601/630)：三处前置条件改为「`auto_place` → 忽略 `pane_index`/`horizontal` 做选择」（显式契约，**不**复用 `pane_index=None` 隐式编码）。
3. `@/bin/tmux.rs cmd_split`(:838-848) GUI 分支（`use_native()==false`）：置 `auto_place=true`、不转发样板 `-t`/`-h`；native 分支不变。平台无关、无硬编码。

**AC（requirements 需求1）/ 测试**
- **AC1.1（shim 单测，`src/bin/tmux/socket_routing_tests.rs`）**：GUI 解析 `["-t","0","-h","-l","70%"]` → body `auto_place==true`、不含 `pane_index`、`horizontal==false`。
- **AC1.2/1.3（后端集成）**：`auto_place=true` 下 有空闲面板→`reused=true` 叶子不增；无空闲面板→切自最大叶子、方向同 `balanced_split_decision`。
- **AC1.4（回归）**：native 路径（不带 auto_place）`-t` 行为不变；`balanced_split_decision` 既有单测通过。

**DoD**：上述 AC 绿；harness `split-window -t 0 -h -l 70%` 实测走 idle-reuse / 最大-pane。

---

## T2 ·〔P1〕5b — 终端跟随 pane resize（await-Channel-then-fit）  〔= #4，依赖 T0〕

**目标**：teammate 面板创建后终端尺寸脱离 80×24 默认、跟随 pane resize 重绘。

**根因（design §3，已做实）**：attach 的 rAF fit（`~/lib/terminal/manager.ts:1685`）早于 RidgePane 的 `ensurePtyBridge+setPaneDeltaMode`（`~/lib/components/RidgePane.svelte:904/955/1009`）→ 首帧 Resize delta 丢、kernel 卡 80×24（自愈注释 `manager.ts:3994-4000`）；teammate 面板经 `~/routes/+page.svelte:1075` **不**走 GUI split 专属的 `scheduleForceFitAfterSplit`（`~/lib/stores/paneTree.ts:1399`）。可见工作区约 500ms 多自愈，**0×0/隐藏工作区永久卡死**。

**改动点 / 实现要点**
1. **主修（确定性）**：teammate 面板 attach 路径 **await Channel 注册（`setPaneDeltaMode`）后再 `fitPaneNow`**（复用记忆库 `bug_split_kernel_race` 已验证形态）。落点：`RidgePane.svelte` onMount 对 teammate-origin 面板，或 `manager.attach` 内序列化 fit 到 Channel-ready 之后。**不**让后端事件携带 `new_pane_id` 去驱动前端 fit（层次倒置）——前端 onMount/diff 自决（消费 T0 封套的 `split-new`/`split-reuse`）。
2. **安全网**：`scheduleForceFitAfterSplit` 用于 0×0→可见切换兜底（呼应 `onActiveWorkspaceChanged` `manager.ts:840`）。

**AC（requirements 需求3）/ 测试**
- **AC5.1（前端单测）**：teammate attach 中 `fitPaneNow` 首次有效调用**发生在** `setPaneDeltaMode` resolve 之后（断言顺序/因果）。
- **AC5.2（集成）**：Channel 注册后首次 fit 上报 cols/rows == 容器换算值（**未修复时必失败**，保证区分度；勿用「重试窗口内」时延断言）。
- **AC5.3（集成）**：拖分隔条改 teammate 面板行列 → PTY 收到匹配 resize、TUI 重排；**复用路径**（`split-reuse`）面板同样达标。
- **AC5.4（回归，跨平台）**：GUI split 仍正常；Windows ConPTY §1.26 prompt 不塌缩；**窄 pane** 工况 Ink 重排正常。

**DoD**：AC 绿；新建 + 复用 teammate 面板在创建即填满 pane，含 ConPTY。

---

## T3 ·〔P1〕#6 — Starting→Busy→Idle 生命周期 + badge（F1 + child-exit 同期）  〔= #5，依赖 T0〕

**目标**：teammate 面板状态真实反映 agent 生命周期：启动即 Busy（id 可空）、退出转 Idle 可复用；**新 split 与征用空闲 pane 共用同一提升/降级逻辑**；全由 Ridge 自身信号驱动，不依赖 harness。

**根因（design §3bis，已做实）**：无结构化 program → Starting（`@/teammate/server.rs:525-528/749-754`）；真实 agent 经 `route_spawn_process`(:977) 落入但不碰 `teammate_pane_states`；`register-agent`/`release-pane` 全仓无 harness 调用方。反向泄漏：child-exit 两分支（`@/engine/pty.rs:487-508` native 销毁 / `:509-522` ordinary 重建 shell）均不重置 teammate 状态。

**改动点 / 实现要点**
1. **F1 提升（DE=启动即 Busy/id 可空）**：`SpawnProcessBody`/`SplitBody` 增 `is_agent: bool`(+可选 `agent_id`)；shim 在 teammate agent 启动时透传。后端收到意图 → 置 `Busy`（有 id 则写 `teammate_agent_pane_map`）+ emit（T0 封套 `agent-state`）。
   **统一覆盖三入口**：① idle-reuse 分支(`@/teammate/server.rs:514-585`，现状 `:525` program=None 卡 Starting)；② 新 split 分支(`:749-754`)；③ `route_spawn_process`(:977，harness 主路径)。
   **〔BLOCK① 裁决 = 采纳 (i)，team-lead 终审〕** agent 启动（**含 idle-reuse**）**一律走结构化 spawn**：F1 在 reuse 分支也用 `ensure_pane_pty_workspace` 结构化起 agent PTY、**替换该 idle pane 原 shell 的 PTY**（独立 PTY，agent 退出即 EOF → 走下面 child-exit→Idle）；**禁止**走 command-写入既有 shell(`:560-565`)。
   **约束**：agent-intent **必须携结构化 program/args/env**；若仅 command 字符串无法结构化 → **拒绝该 agent 复用路径**（或降级 + 记一条 metric，由 P2 的 F4 兜底），**不得静默 command-write**。**F4 看门狗维持 P2**（不提前）。
2. **child-exit → Idle（DF=②，与 F1 同 PR）**：由 reader EOF 驱动（`@/engine/pty.rs:241` `Ok(0)`）。
   - **ordinary 分支**(`:509-522`)：`PaneClosed`→前端用同 paneId 重建 shell(`~/lib/terminal/ptyBridge.ts:121-149`)；**补**：重置 `teammate_pane_states[pid]=Idle` + 清 `teammate_agent_pane_map`。
   - **native 分支**(`:487-508`)：销毁叶子**前**清 `teammate_pane_states`/映射，无孤儿。
   - 不依赖 harness 调 release-pane。
3. badge：`get_pane_layout`(`@/commands/pane.rs:60-64`) 已序列化 `agent_state`；前端 `SplitContainer.svelte:609/630` 已渲染——经 T0 封套 re-sync 即更新。

**AC（requirements 需求4 / design AC6.x）/ 测试**
- **AC6.1/6.2（后端单测）**：带 `is_agent` 的 spawn-process 落入 Starting → `==Busy`、映射写入、emit；`get_pane_layout` 序列化 `busy`+id（因果断言）。
- **AC6.3（集成）**：split(无cmd)→STARTING；agent 启动→翻转 AGENT（断言「收到 spawn-process 后状态同步 Busy」因果）。
- **AC6.4（生命周期，按分支）**：(a) ordinary 退出→Idle、清映射、可复用；(b) native 退出→pane 销毁且无残留孤儿；均无残留 Busy/Starting。
- **AC6.5（征用空闲 pane，核心）**：reuse + 结构化 `is_agent` → 替换 idle pane 的 shell PTY、reused_pid==Busy、即时 AGENT badge；agent 退出→EOF→Idle 可复用（结构化-spawn 复用子路径单测必覆盖）。
- **AC6.5b（command-only 拒绝）**：仅 command 字符串、无法结构化的 agent-intent 复用 → **拒绝或降级 + metric**，**不**静默 command-write（单测断言不出现「写进既有 shell 后卡 Busy」）。
- **AC6.6（回归）**：内嵌-program split 仍直接 Busy；GUI `register_teammate_agent` 仍可用；new-window 一致。
- **AC6.8（spawn/command 路径）**：结构化 spawn 退出→EOF→终局（ConPTY/Unix 一致）。
- **AC6.9（并发）**：N 并发 split → N 不同叶子、无串号、无状态错配。

**DoD**：全生命周期 AC 绿；征用空闲 pane 启动 agent 立即 Busy badge、退出转 Idle 可复用；无 command-复用 pane 卡 Busy。

---

## T4 ·〔P2〕#4 — 工作区隔离 H1（锁定 + fail-closed）  〔= #6〕

**目标**：`X-Ridge-Workspace` 缺失/无效时拒绝 teammate 放置（不回退 `active_workspace_id()`），加指标/日志。

**前提（design §2，已核验）**：`RIDGE_WORKSPACE_ID` 注入(`@/commands/terminal.rs:551`)与 shim-on-PATH(`:540`)同 `(Some(bind),_)` arm 门控 → 不可达 split 的进程拿不到 shim → fail-closed 不误杀。

**改动点 / 实现要点**
1. `caller_workspace_id`(`@/teammate/server.rs:51-62`) 改返回 `Result`（或调用点判定）：头缺失/解析失败/工作区不存在 → 不回退，返回明确错误。
2. teammate 放置路由（split/spawn-process/new-window/register/release）统一 fail-closed。
3. 错误文案区分「缺失/格式错（RIDGE_WORKSPACE_ID 未传播）」与「工作区已关闭」；`teammate_metrics` 加计数 + 结构化日志。
4. 在 `@/commands/terminal.rs:540`↔`:551` 加注释 + 断言/测试钉死「shim 与 workspace-id 同 arm 门控」不变量。

**AC（requirements 需求2）/ 测试**
- **AC4.1**：带 WS-A 头、焦点 WS-B → 落 WS-A。
- **AC4.2**：缺头 → 明确错误 + 指标+1 + 活动工作区无新面板。
- **AC4.3**：头指向已关闭工作区 → 「no longer exists」错误 + 指标+1。
- **AC4.4（回归）**：正常注入 spawn 不受影响。

**DoD**：AC 绿；门控耦合不变量有测试守护。

---

## T5 ·〔P2〕F4 看门狗 + `active_workspace_id` 解耦  〔需 team-lead 建任务〕

**目标**：补「无永久 Starting/Busy」安全网；把绑 `active_workspace_id` 的命令解耦为显式 workspace。

**改动点 / 实现要点**
1. **F4 看门狗**：PTY active + 宽限期后仍 Starting → 有存活子进程则 Busy、否则清 badge；**按 pane 所属 workspace 操作，禁读 `active_workspace_id`**。亦兜底 command-复用（若 T3 未完全消除）。
2. **解耦**：`resize_pane`(`@/commands/terminal.rs:942`)、`register_teammate_agent`/`release_teammate_agent`(`@/commands/pane.rs:396/424`) 三处从 `active_workspace_id()` 改显式 `workspace_id` 入参。

**AC / 测试**
- **AC6.7**：提升/降级信号缺失 → 看门狗宽限期后修正；无面板长期 Starting、无 command-复用 pane 长期卡 Busy。
- 单测：解耦后非活动工作区面板的 resize/状态操作落在正确 workspace。

**DoD**：AC6.7 绿；解耦三处有测试。

---

## 范围外（本期不做）
- `-l <pct>` 尺寸尊重（A1 后 50/50）；5a `resize-pane` 实装（维持 no-op）。
- F2 前台进程自动检测（识别口径=意图驱动，明确不做）。

## 终验（= 任务系统 #7）
- 全部 P1/P2 task AC 绿；`cargo fmt`/`clippy -D warnings`、前端 lint/type-check、相关单测+集成绿；覆盖率达标。
- 提交（conventional commits）+ 推送 develop→origin。
