# 事后分析：Ridge `git.exe` 堆积 / 重生风暴

日期：2026-07-24  
相关：CONTRACT-20 / iteration 20b / 本机进程观测 + 提权清理

## 1. 现象（运行事实）

| 观测 | 含义 |
| --- | --- |
| 十余个～二十个 `git.exe` 同刻出现 | 非偶发单命令卡死，是批量并发 |
| 父进程均为 `ridge.exe`（或 git 子进程） | 来自应用内 SCM/status 路径，非用户终端手敲 |
| 普通权限 `Stop-Process` / `taskkill` 部分失败（Access denied / not supported） | 子进程树/Job/权限态异常；卡死 OS 进程难优雅结束 |
| 杀 `git` 后立刻出现新 PID | **重生风暴**：上层持续派发，队列在消化 |
| 仅提权后同时杀 `ridge` + `git` 才清零 | 根在宿主持续 fan-out，不在单颗僵尸 |

## 2. 因果链（根因，多层）

```
触发面（多路同时要 git）
  pane cwd 变化 / SCM 侧栏 / watcher 刷新 / 5min heartbeat / 多 pane 同仓
        │
        ▼
前端 mapLimit + AbortSignal
  · 能停「尚未 dispatch」的 fan-out
  · 不能取消已 invoke 的后端工作          ← 护栏缺口 A（产品语义误判）
        │
        ▼
后端 Semaphore（逻辑并发 2–12）
  · 限制的是 spawn_blocking 许可，不是「永远只有 N 个 git.exe」
  · 许可内仍串行/多次 Command::output()
  · 一任务可再拉 credential/helper 子进程 → 进程数可 > permit
        │
        ▼
Command::output() 无墙钟超时、无杀树     ← 护栏缺口 B（技术根因）
  · git 卡索引/网络/锁/坏盘 → 阻塞线程 + 占 permit 直至外部杀
  · 外部杀进程 → wait 返回 → permit 释放 → 排队请求立刻再 spawn
        │
        ▼
重生风暴 + UI/资源卡死
```

### 2.1 直接技术根因

**所有 git 子进程走无限等待的 `Command::output()`，没有「超时 → 杀进程树 → 释放许可」闭环。**

Semaphore 只防「同时跑太多 blocking 任务」，**不防「已经 spawn 的 OS 进程永不结束」**。  
前端 Abort 只停新 launch，**不回收 inflight OS 子进程**。两者叠在一起，文档上曾把 Bug4 标成「已实现」（仅有 debouncer + cap），与运行事实矛盾。

### 2.2 放大因子（为何变成风暴）

1. **多触发源叠加**：paneGitStatus（每 pane）、SCM 多仓 fan-out、fs/git watcher debounce 后刷新、周期 heartbeat——任一持续触发即可维持队列。
2. **Windows CreateProcess 贵**：并发一高，阻塞池与其它 IPC（如文件树）一起饿死 → 体感「整窗卡死」。
3. **杀 git 等于帮队列「续跑」**：卡死时 permit 被占；外部杀子进程后 permit 空出，下一波立刻起来——观测上像「杀不死」。
4. **进程树**：git 可再拉 helper；Windows 上仅 `Child::kill` 不够，需 `/T` 树杀（本轮 `taskkill /T`）。

### 2.3 非根因（避免误诊）

- 不是「没写 semaphore」——有，且 clamp 2–12。
- 不是「前端完全没限流」——有 `mapLimit` / `recommendedGitConcurrency`。
- 不是单一「debouncer 没开」——watcher 已有噪声过滤；风暴仍可由正常刷新路径驱动。
- 不是权限漏洞本身——Access denied 是清理阶段的表象，不是堆积的起因。

## 3. 修复对应关系（20b）

| 缺口 | 修复 |
| --- | --- |
| 无限 `output()` | `run_command_with_timeout` + 超时 `taskkill /T`（Unix kill） |
| 许可排队无界 | `spawn_git_blocking` 上 `acquire` 超时，失败关闭 |
| 可观测性 | `git_active_child_count` / peak |
| 双端 cap 漂移 | 前后端 `GIT_CONCURRENCY_MIN/MAX = 2/12` 对齐 |
| 假绿 | `guard_tests`：并发峰值 ≤ cap、挂起子进程超时回收、真 `get_scm_status` 冒烟 |

## 4. 教训（可复用）

### L1 — 逻辑并发 ≠ OS 进程生命周期

Semaphore / 线程池 / p-limit **只约束「同时进入某段代码的任务数」**。  
凡 `spawn` 出的外部进程，必须另有：**超时、取消、杀树、permit/引用与进程同生死**。  
验收信号应含「峰值进程」或「超时后活跃计数归零」，不能只 assert「permit ≤ N」。

### L2 — AbortSignal / drop future 不杀子进程

IPC/async 取消默认只停**调度**。已交给 OS 的 `git/ffmpeg/node` 仍活着。  
产品文案若写「取消刷新」，实现必须落到 **kill 子进程树**，否则用户看到「取消了还在烧 CPU」。

### L3 — 「已实现」必须以运行事实重开

Bug4 曾记为 debouncer 已实现。本机风暴证明：**文档/对账表不能替代进程观测**。  
规则：标关闭须有（1）代码符号（2）确定性测（3）与现场故障模式同构的断言。缺 3 只算「部分」。

### L4 — 外部杀进程可能喂养队列

运维「先杀子进程」在**有界队列 + 无超时**系统里会制造重生。正确应急是：**先停派发方（ridge）再清子进程**，或应用内超时自回收。  
写 runbook / 客服话术时写清顺序。

### L5 — 双端 cap 必须同常量、同测

前端 12、后端 12 若各写魔法数，半年后必漂。  
共享具名常量（或契约测试读两边）+ 注释交叉引用。本轮 `GIT_CONCURRENCY_*`。

### L6 — Windows 子进程要按「树」设计

`CREATE_NO_WINDOW`、Job Object、`taskkill /T`、权限提升失败路径都要进设计清单。  
单元测用**真挂起假二进制**走超时杀，比 mock `output()` 有价值。

### L7 — 多触发同源要共闸

watcher / heartbeat / pane / SCM 必须汇入**同一** git 出口闸。  
新入口若直接 `Command::new("git")` 绕过，护栏形同虚设 → CONTRACT-21 可选旁路静态门禁。

### L8 — 发布与修复节奏

「要快」可以先合硬护栏 + 确定性测；**带资产的 versioned Release 仍须等矩阵**，禁止空 tag 交差（AGENTS Releases 硬规矩）。

## 5. 检查清单（以后同类子系统）

- [ ] 外部进程是否经唯一出口？
- [ ] 是否有墙钟超时 + 杀进程树？
- [ ] 取消/超时是否释放全部许可与计数？
- [ ] 前端 abort 与后端回收是否文档一致？
- [ ] 并发 cap 前后端是否同常量？
- [ ] 测试是否覆盖：并发峰值、挂起超时回收、真二进制冒烟？
- [ ] 应急 runbook：先停宿主还是先杀子进程？

## 6. 一句话

**限流管「同时进门几个人」，不管「进门后有人睡死不出来」；睡死的必须闹钟（超时）+ 拖走（杀树），否则门外排队的人会在你踹门（外部杀进程）后一拥而入，看起来像杀不死的风暴。**
