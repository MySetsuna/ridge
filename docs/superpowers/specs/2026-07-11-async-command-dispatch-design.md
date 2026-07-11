# 异步命令经 ridge-core dispatch 路由 — 设计（2026-07-11）

> 收官 #19：把最后一批 **async** 命令（`close_pane`、`write_to_pty`，及未来同类）
> 接入 `ridge_core::dispatch`。本会话已迁完所有 **sync** 命令（读全路由 + 工作区写
> 八命令 + resize/create/split_pane）；这两条卡在 **sync dispatch 无法 await** 上。
> 本设计定路线、评风险，供一个专注会话按图施工 + 真机验收，而非在长会话尾盲改核心路由。

## 背景与硬约束

- `pub fn dispatch(method, args, ctx) -> CoreResult<Value>` 是**同步**中央路由；桌面
  `core_bridge`、rdg `fs_reuse`、远控 WS handler 全走它。它已承载 ~24 条命令。
- `close_pane` 体内 `terminal::kill_pty_if_present(&*state, wid, pane, true).await`（真
  async 杀 PTY）；`write_to_pty` 走 `write_to_pty_async(...).await`（真 async + 每键热路径）。
- 端口 trait 经 `Arc<dyn HostState>` 作 **trait object**；stable Rust 原生 `async fn in
  trait` **非 object-safe** → 端口若加 async 方法须 `async-trait`（Box 化）。

## 方案评估（三选一）

### A. dispatch 全异步化（`async fn dispatch` + `async-trait` 端口）
- 改：`dispatch` → `async`；`WorkspaceWriter` 加 `async` 方法（`async-trait` 依赖 + Box）；
  全体 caller `.await`。
- 优：语义最干净，未来所有 async 命令统一走 dispatch。
- 险：**动中央路由签名 + 新依赖 + 端口重设计**，触及全部命令 route；一处 caller 非
  async 或跨 await 借用错即断整条命令链。**须真机验证全命令仍可路由**。**不宜长会话尾盲发**。

### B. sync dispatch + 旁路异步执行（**推荐**）
- 保 `dispatch` 同步。async 命令的 arm 经 `ctx` 已有的 **`TaskSpawner`**（tokio-free 抽象）
  投递异步工作，结果经一次性 channel（`oneshot`）回收：arm 内
  `spawner.spawn(async { let r = port.close_pane_async(...).await; tx.send(r) })`，然后
  `rx.blocking_recv()`（仅当 dispatch 运行在**非 async** 线程时安全）。
- 关键前提核实项：dispatch 的**调用线程模型**——若 caller 在 async 任务内同步调 dispatch，
  `blocking_recv` 会阻塞 worker（回到 #13 楔死类问题）。故 B 仅在 dispatch 由**专用阻塞线程**
  （`spawn_blocking` 包裹）调用时成立。须先核实各 caller（core_bridge / fs_reuse / WS handler）
  的线程上下文。
- 优：不动 dispatch 签名、不加 async-trait、不碰其余 23 条命令。改面最小。
- 险：`blocking_recv` 的线程模型前提；须逐 caller 核实 + 真机验不楔死。

### C. 专用异步旁路（不进 dispatch）
- `close_pane`/`write_to_pty` 走远控 host 的**专用 async handler**（write_to_pty 现已有
  `Method::WritePty` 专用通道即此形）；不强塞进 dispatch。
- 优：热路径（每键 write）本就该走专用通道，不付 dispatch JSON round-trip；close_pane 同置。
- 险：与「命令统一经 dispatch」的 D-S1 内核化目标背离；两套路由。write_to_pty 明显归 C，
  close_pane 归 A/B 更一致。

## 推荐

- **write_to_pty → C**：每键热路径，保留/规范其专用异步通道，**不进 dispatch**（性能 + 语义）。
- **close_pane（及未来非热 async 命令）→ B 优先，A 兜底**：先核实 dispatch 各 caller 线程
  上下文；若都在可 `spawn_blocking` 隔离的边界，用 B（改面最小、不动签名）；若线程模型不
  允许安全 `blocking_recv`，则排期 A（专注会话 + 全命令真机回归）。

## 实施顺序（专注会话）

1. **核实**：列出 `ridge_core::dispatch` 全部 caller 及其线程上下文（async 任务 vs 阻塞线程）。
   —— 决定 B 是否可行的唯一前提。
2. 若 B 可行：端口加 `close_pane`（sync 签名，内部经 spawner+oneshot 驱动 async kill）；
   dispatch 加 arm；AppState 实现委派现有 `kill_pty_if_present`；单测（fake 端口）+ **真机验**
   远端关 pane 的 PTY 清理无泄漏/无楔死。
3. write_to_pty：规范其专用异步通道（C），补远端每键写路径的接线 + 真机验时延。
4. 若 B 不可行：另起会话做 A（async dispatch + async-trait），带**全命令真机回归**。

## 测试 / 验收

- 单测：端口 fake 断言 close_pane 转发（本会话 sync 命令同款 fake 模式已就绪）。
- **真机**（须）：远端关 pane → 宿主 PTY 子进程确被回收（无孤儿）、无 #13 楔死；每键
  write 时延达标。编译证不了这些，故列真机验收项。

## 与本会话已完成部分的衔接

本会话已把 #19 全部 sync 命令迁 core（端口-适配器 + dispatch arm + fake 单测范式生产级，
ridge-core 327 测）。本设计只补最后 async 两条的路由机制；范式、端口聚合、fake 测试样板
均已就位，按上表施工即可。相关：`docs/plans/s1-migration-ledger.md`、[[project_remote_lsp_task_ledger]]。
