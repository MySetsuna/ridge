# Agent 暂停/恢复跨平台设计（G1 差距，iteration 8 G2——仅设计，零代码）

日期：2026-07-23。范围：单 Agent 暂停/恢复的可暂停边界、跨平台机制选型、恢复语义与 HITL 交互、失败方向。接管/回滚不在本文（G1 后续阶段）。本轮明确不实现、不加枚举变体（无实现的死状态不入产品枚举）。

## 1. 可暂停边界：三层剖面

| 层 | 对象 | 暂停含义 | 现有落点 |
| --- | --- | --- | --- |
| L-a 会话层 | agent CLI 会话（claude code 等） | 协议级 pause（若 CLI 支持） | 无统一协议，**不可依赖** |
| L-b 输入层 | Ridge→PTY 的输入投递 | 停止投递 stdin/send-keys，agent 阻塞于下次读 | `write_to_pty` / teammate send-keys 单一入口 |
| L-c 进程层 | pane 的 PTY 子进程（组） | OS 冻结：不再被调度 | engine PTY 持子进程句柄 |

结论：**暂停 = L-b + L-c 组合**；L-a 不可依赖（各 CLI 无统一 pause 协议，违「不绑定单一 Agent CLI」非目标）。仅 L-b 是「软暂停」：在飞任务（编译、网络请求）继续跑完，只挡后续指令——语义上是「不再接单」而非「停」。L-c 才是真暂停。

## 2. 跨平台机制选型

### Unix（macOS/Linux）

`kill(-pgid, SIGSTOP)` / `kill(-pgid, SIGCONT)` 对整个进程组：内核级、不可被目标忽略、天然递归子进程。注意点：
- 必须发到**进程组**（agent 常 spawn 子进程：编译器、git 等）；PTY 会话首进程即组长。
- 有 TTY 输出的被停进程恢复后可能收 `SIGTTOU`；PTY 场景默认可容忍。

### Windows（无 SIGSTOP）——三候选

| 候选 | 机制 | 评估 |
| --- | --- | --- |
| A. `NtSuspendProcess` | ntdll 未文档化 API，挂起单进程全部线程 | 事实标准（Process Explorer/pssuspend 同法）；但**不递归子进程**，需按进程树逐个挂起（快照枚举有竞态：挂起间隙新 spawn 的子进程漏网） |
| B. Job Object 冻结（`JOBOBJECT_FREEZE_INFORMATION`） | Win8+ 未文档化 job 信息类，UWP/容器同法；job 内所有进程原子冻结 | 语义最优（原子、递归、无竞态）；**前提是 pane 子进程创建时已入 job**——须在 PTY spawn 路径预先 `CreateJobObject`+`AssignProcessToJobObject`，对既存 pane 不可追溯 |
| C. 仅 L-b 输入门控 | 纯 Ridge 内 | 零 OS 依赖但只是软暂停（见 §1） |

**选型：B 为目标态，A 为过渡，C 为兜底。** 分阶段：
1. 阶段一（最小可用）：L-b 输入门控 + UI 标记（跨平台同码，纯 Ridge 状态，零特权）。
2. 阶段二（真暂停）：Unix `SIGSTOP` 进程组；Windows 走 A（进程树遍历挂起，接受竞态缺口并在文档声明）。
3. 阶段三（Windows 补强）：PTY spawn 路径预建 job → 冻结改走 B，竞态缺口关闭。job 预建改动落 `packages/ridge-core` PTY engine spawn 处，属一次性基建，需独立评审（影响所有 pane，不只 teammate）。

## 3. 状态与恢复语义

- 状态放**运行态侧表**（类比 `teammate_pane_states`），不改 `Teammate` 结构、不加枚举变体，直至阶段二真正落地实现时一并加（状态与实现同轮出现，避免死状态）。
- 语义：`suspend(agent_id)` → L-b 关闸 → L-c 冻结 → 标记 Suspended（**冻结成功才标记**）。`resume(agent_id)` → L-c 解冻 → L-b 开闸 → 标记回原状态。幂等：对已 Suspended 再 suspend 为 no-op 成功。
- 顺序不可反：先关输入再冻结（避免冻结瞬间尚有指令在投递）；恢复反序。
- pane 关闭 / agent 退出时清理侧表（沿 `remove_by_pane` 既有路径）。

## 4. 与 HITL 交互

- **Suspended agent 的既有 pending HITL**：保留挂起项，但 `request_approval` 的 120s fail-closed 计时**不暂停**——宁可超时拒绝也不无限挂（安全不变量：绝不静默放行，也绝不无限期挂起人审）。裁决到达时若 agent 仍冻结，oneshot 结果送达其恢复后生效（tokio channel 语义天然如此）。
- **暂停期间新触发 HITL**：不可能——L-b 已关闸，agent 收不到新指令；其在飞任务若触发 send-keys 网关，走正常审批（在飞任务不属「已暂停」承诺范围，文档如实声明）。

## 5. 失败方向（fail-visible，不 fail-silent）

- suspend 中途失败（如 Windows 树遍历部分成功）：**回滚已挂起的进程**并报错，状态保持 Running——绝不出现「UI 显示已暂停、进程仍在跑」的假暂停。
- resume 失败（进程已死等）：标记异常并在 UI 呈现，不静默吞。
- 全链路日志不含命令全文/敏感参数（沿既有日志纪律）。

## 6. 远端暴露（P2 衔接）

暂停/恢复是**写操作**：远端暴露须走 HITL 第二阶段同等语义（nonce/审计），阶段一至三全部只在桌面本机 IPC。`REMOTE_ALLOWLIST` 不加，直至 P2 裁决通道设计定稿。

## 7. 验收路径（实现轮的合同素材）

- 阶段一：门控开→写入被拒（确定性单测）；关→恢复投递；幂等。
- 阶段二 Unix：spawn 子进程树 → suspend → 全组 `T` 态（`ps` 判定）→ resume → 恢复；Windows：挂起后 CPU 时间不再增长（确定性可测）。
- HITL 交互：suspended + pending → 超时仍 fail-closed 拒绝。
