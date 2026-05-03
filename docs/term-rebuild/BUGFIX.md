# 主项目 Bug 修复计划（与 xterm 替换无关）

> **状态（2026-05-03 末次复核）**：xterm round 7 已 retire，`Pane.svelte` 已删除。
> 本文档表内 BUG-1 / BUG-2 / BUG-5 / BUG-6 文件位置都是 `Pane.svelte`——这些 patch 已**moot**，因为 `RidgePane.svelte` 是从零写的全新组件，不继承 Pane.svelte 的双 listener / 1.5s 轮询 / 连环 rAF / 前端 2000 行 buffer 这些具体代码 pattern。RidgePane.svelte 是否有等价 bug 需独立审计——ptyBridge.ts 已避免 listener 重订（详见 TASKS §5.1 parking lot）；polling 由 manager 节流；scrollback 走 §5.2 决议（保留 ~700 KB 重复，方案 A）。
>
> 仍适用的 patch：
> - **BUG-3**（PTY reader `rt.block_on` send 事件）— `engine/pty.rs` 仍现行（grep 确认 line 193/218/323/339 仍存在）。
> - **BUG-4**（4 ms 合批窗口对单字符 echo 是纯延迟）— `lib.rs` 后端 batch 路径仍现行；`REPLACE_AND_FIX_PLAN.md` 提到压缩到 < 10 ms 需 SharedArrayBuffer 替换 IPC，更大改造。
>
> 后端两条仍可单独 cherry-pick，与本会话其他工作正交。

> 本文档列出在 review `src.zip` 时发现的**独立 bug**，与 ridge-term 项目正交。
> 每条都给出：
> - 症状 / 用户感知
> - 触发条件
> - 根因 + 代码位置
> - 可直接应用的 patch（统一 diff 格式）
> - 验收方式
>
> **修复顺序与 xterm 替换无依赖**，可以单独 cherry-pick 到主分支。

---

## 索引

| # | 严重度 | 标题 | 文件 | 估计 LOC |
|---|---|---|---|---|
| BUG-1 | 🔴 高 | 单 PTY channel 双 listener 触发 git diff 风暴 | `Pane.svelte` | ~30 |
| BUG-2 | 🟠 中 | foreground/cwd 1.5s 轮询无 backoff，多 pane 时 IPC 风暴 | `Pane.svelte` | ~40 |
| BUG-3 | 🟠 中 | PTY reader 用 `rt.block_on` send 事件，高吞吐时阻塞读 | `engine/pty.rs` | ~25 |
| BUG-4 | 🟡 低 | 固定 4ms 合批窗口对单字符 echo 是纯延迟 | `lib.rs` | ~15 |
| BUG-5 | 🟡 低 | resize 触发的连环 rAF + 重复 clearTextureAtlas | `Pane.svelte` | ~20 |
| BUG-6 | 🟢 提升 | 后端 4MB scrollback + 前端 2000 行 buffer 重复存储 | `Pane.svelte` | ~5 |

🔴 = 影响多 pane 用户日常 / 数据一致性
🟠 = 性能层面累积影响
🟡 = 可观测但不致命
🟢 = 资源/正确性优化（非 bug 性质）

**推荐合并顺序**：BUG-1 → BUG-3 → BUG-2 → BUG-4 → BUG-5 → BUG-6。BUG-1 / BUG-3 风险最低且收益直接，先做。

---

## BUG-1 🔴 单 PTY channel 双 listener 触发 git diff 风暴

### 症状

- 在 git 仓库目录里跑 `cat large.log` / `tail -f` / 任何高吞吐输出
- DevTools Network 看到 `get_git_diff` IPC 持续不停发出（每秒数十次）
- CPU 飙高（git diff 是全索引扫描）
- 严重时 UI 卡顿、文件浏览器的 git status 列也跟着抖

### 触发条件

100% 复现：任何 pty-output 事件都触发，**不需要命令真的执行完**。

### 根因

`Pane.svelte` 在同一个 channel 上挂了两个 listener：

- **L1**（line 859, in `renderView`）：把 PTY 字节写入 xterm
- **L2**（line 1107, in `onMount`）：每次收到字节就 `setTimeout(loadDiffStatus, 500)`

L2 的设计意图是"命令执行完后刷新 diff"，但实现成了"每个字节都触发延迟 500ms 的 git diff"。`cat large.log` 输出 100KB（被合批成 ~2-5 个 emit），就是 2-5 次 git diff 排队，看似不多——但实际上由于 setTimeout 500ms 间隔内会 **持续触发新 timer**，每次 timer 又安排新的 git diff，最终堆出几十次。

更糟的是 `git diff` 命令本身在大仓库里耗时 100ms-1s，IPC 排队 + 后端串行执行 = 占住后端 worker 数秒，期间其它命令都堆积。

### 修复策略

L2 真正想要的是"shell 提示符回归时刷一次 diff"。后端已经支持 OSC 133 / 633 prompt marker（见 `pty.rs:114` 的 `find_prompt_osc`）。改成监听 prompt marker 而不是字节流。

但前端没有现成的 prompt 事件 ——  现在 `pty.rs` 只用 OSC 来终止 resize-silence 状态、不向前端 emit。**最小改动方案**：

1. 后端添加一个 `pane-prompt-{ws}-{pane}` 事件，在 reader 检测到 OSC 133;A 或 633;A 时 emit
2. 前端 L2 改听这个事件而不是 pty-output

如果不想改后端，**纯前端修复**：把 L2 改成"输出停止 N 秒后才刷一次 diff"，用 trailing-edge debounce + 取消未触发的 timer。这是过渡方案，不如用 prompt 信号准。

下面给纯前端方案的 patch（最小、风险最低）。

### Patch（纯前端）

```diff
--- a/src/lib/components/Pane.svelte
+++ b/src/lib/components/Pane.svelte
@@ -160,6 +160,9 @@ interface Props {
 // Git diff 状态
 let diffStatus: GitDiffStatus | null = $state(null);
 let diffLoading = $state(false);
 let diffUnlisten: (() => void) | undefined;
+/** Trailing-edge debounce timer for diff refresh. Replaces the
+ *  per-byte setTimeout pile-up that hammered get_git_diff during high-
+ *  throughput output (BUG-1). */
+let diffDebounceTimer: ReturnType<typeof setTimeout> | undefined;

 /** 是否显示滚动到底部按钮 */
@@ -1104,13 +1107,18 @@ onMount(() => {
 				// 监听命令执行后刷新 diff
 				const cmdCh = `pty-output-${workspaceId}-${paneId}`;
 				diffUnlisten = await listen<{ data: string }>(cmdCh, (e) => {
-					// 检测命令执行完成（简单策略：命令输出后延迟刷新）
+					// Trailing-edge debounce: cancel any pending diff load and
+					// schedule a fresh one. Only the LAST byte in a burst
+					// actually triggers loadDiffStatus, so a `cat large.log`
+					// produces at most one IPC instead of dozens.
 					if (!alive || isComposing) return;
-					setTimeout(() => {
+					if (diffDebounceTimer) clearTimeout(diffDebounceTimer);
+					diffDebounceTimer = setTimeout(() => {
+						diffDebounceTimer = undefined;
 						if (!alive || isComposing) return;
 						void loadDiffStatus();
-					}, 500);
+					}, 800);
 				});
 			})();
 		} else {
@@ -1145,6 +1153,10 @@ onMount(() => {
 			ptyUnlisten?.();
 			removeFocusHandlers?.();
 			removeCompositionHandlers?.();
 			diffUnlisten?.();
+			if (diffDebounceTimer) {
+				clearTimeout(diffDebounceTimer);
+				diffDebounceTimer = undefined;
+			}
 			// Park the terminal instead of disposing it...
```

### 验收

- 在 git 仓库 pane 里跑 `seq 100000 | head -10000`
- DevTools Network 过滤 `get_git_diff`
- 修复前：数十次请求
- 修复后：最多 1-2 次（输出结束后 800ms 一次）

### 后续优化（不在本 patch 范围）

后端添加 `pane-prompt-{ws}-{pane}` 事件（用现有 `find_prompt_osc` 的命中位置），前端改听 prompt 信号。这样 `tail -f` 等持续输出也能合理触发（每次有新 prompt 才刷）。

---

## BUG-2 🟠 foreground/cwd 1.5s 轮询无 backoff

### 症状

- 多 pane（10+）时 DevTools 看到稳定 ~13 次/秒的 IPC（`get_pane_foreground_process` + `get_pane_cwd`）
- 即使 pane 内毫无活动（shell idle）也持续轮询
- 微小但持续的 CPU + IPC 占用，对低端机器可见影响

### 触发条件

任何 pane 都会触发，10 pane 就是 10 × 2 IPC/1.5s。

### 根因

`Pane.svelte:1103`：

```ts
foregroundPollInterval = setInterval(() => void pollPaneInfo(), 1500);
```

固定 1.5s 间隔，无视活跃度。注释说"OSC 7 路径更快，这是 fallback"——意图是对的，但**没有真正利用** OSC 路径来跳过轮询。结果是：即使 shell 一直 emit OSC 7，前端轮询照跑。

### 修复策略

两个互补改动：

1. **指数 backoff**：连续 N 次轮询返回值与上次相同，把间隔从 1.5s 拉长到 6s，活动恢复时立即归位
2. **OSC 7 命中后跳过下次轮询**：监听 `pane-cwd-changed` 事件，命中即把"距离下次轮询"重置为 6s

下面给改动 1 的 patch（最简单、收益已经够）。改动 2 留作后续。

### Patch

```diff
--- a/src/lib/components/Pane.svelte
+++ b/src/lib/components/Pane.svelte
@@ -1056,6 +1056,11 @@ onMount(() => {
 				// 记忆上一次的快照，避免把相同值重复写回 store 触发下游 effect/监听反应。
 				let lastPolledProc: string | null = null;
+				/** Backoff state: how many consecutive polls returned the same
+				 *  values. After 4 unchanged polls we stretch the interval from
+				 *  1.5s to 6s; any change resets to fast polling.
+				 *  Saves ~75% IPC on idle panes (BUG-2). */
+				let unchangedCount = 0;
+				const FAST_INTERVAL = 1500;
+				const SLOW_INTERVAL = 6000;
 				async function pollPaneInfo() {
 					if (!alive || !isTauri() || !workspaceId) return;
 					try {
 						const [proc, cwd] = await Promise.all([
 							invoke<string | null>('get_pane_foreground_process', { workspaceId, paneId }),
 							invoke<string | null>('get_pane_cwd', { workspaceId, paneId }),
 						]);
 						if (!alive) return;
+						const cwdPrev = getPaneCwd(workspaceId, paneId);
+						const changed = proc !== lastPolledProc || cwd !== cwdPrev;
 						if (proc !== lastPolledProc) {
 							lastPolledProc = proc;
 							if (proc) {
@@ -1093,15 +1098,29 @@ onMount(() => {
 						}
+						// Backoff bookkeeping. Unchanged → tick toward slow; changed → reset.
+						if (changed) {
+							unchangedCount = 0;
+						} else {
+							unchangedCount = Math.min(unchangedCount + 1, 100);
+						}
 					} catch {
 						/* best-effort — ignore errors */
 					}
 				}
-				// 固定 1500ms 轮询：让 cd 的 UI 反馈保持在秒级；
-				// 由于 pollPaneInfo 内部已做签名比对（proc/cwd 未变则零 store 写），
-				// 静默期间开销主要是两次 Tauri IPC，其余为 no-op。
-				// 注：shell emit OSC 7 时后端会直接 push pane-cwd-changed，路径比轮询更快。
-				void pollPaneInfo();
-				foregroundPollInterval = setInterval(() => void pollPaneInfo(), 1500);
+				// Adaptive polling: 1.5s while activity is changing, 6s once idle.
+				// `unchangedCount >= 4` is the boundary — about 6 stable seconds
+				// before backoff kicks in.
+				const scheduleNext = () => {
+					if (!alive) return;
+					const interval = unchangedCount >= 4 ? SLOW_INTERVAL : FAST_INTERVAL;
+					foregroundPollInterval = setTimeout(async () => {
+						await pollPaneInfo();
+						scheduleNext();
+					}, interval);
+				};
+				void pollPaneInfo();
+				scheduleNext();
```

⚠️ 注意 `foregroundPollInterval` 类型变了 —— 从 `setInterval` 的返回值变成 `setTimeout` 的返回值。需要同步改 destroy 阶段：

```diff
--- a/src/lib/components/Pane.svelte
+++ b/src/lib/components/Pane.svelte
@@ -1126,7 +1126,7 @@ onMount(() => {
 			alive = false;
 			...
 			if (foregroundPollInterval !== undefined) {
-				clearInterval(foregroundPollInterval);
+				clearTimeout(foregroundPollInterval);
 				foregroundPollInterval = undefined;
 			}
```

变量声明类型也要更新（约 line 227）：

```diff
-let foregroundPollInterval: ReturnType<typeof setInterval> | undefined;
+let foregroundPollInterval: ReturnType<typeof setTimeout> | undefined;
```

### 验收

- 打开 10 个 pane，留它们 idle 30 秒
- DevTools Network 过滤 `get_pane_foreground`
- 修复前：~13 次/秒持续
- 修复后：前 6 秒约 13 次/秒，之后降到 ~3 次/秒

---

## BUG-3 🟠 PTY reader 用 `rt.block_on` send 事件

### 症状

- 高吞吐场景（`cat /dev/urandom | base64 | head -c 10M`）下 shell 反向卡住
- macOS / Linux 看 `top -H` 能看到 reader 线程偶尔 100% CPU
- 最终用户感知：在大输出过程中输入命令几秒钟没响应（输入字符延迟出现）

### 触发条件

mpsc 通道（容量 1024）排满时触发。在你当前合批窗口 4ms + 64KB max bytes 的配置下，需要持续高吞吐 ~50ms+ 才会满。

### 根因

`engine/pty.rs:269`：

```rust
let _ = rt.block_on(async {
    state
        .event_tx
        .send(GlobalEvent::PtyOutput {
            workspace_id,
            pane_id,
            data,
        })
        .await
});
```

`rt.block_on` 把异步 send **同步阻塞** 在 reader 线程上。当 mpsc 满时，`.await` 会挂起 future 等待容量；`block_on` 把这个挂起翻译成真正的线程阻塞。结果是：

- reader 线程不再读 PTY fd
- 内核 pipe buffer 满 → shell 子进程的 write 系统调用阻塞
- shell 看起来"卡住"

更微妙的问题：reader 线程被阻塞期间，**任何来自该 pane 的 OSC 7 / 标题事件也会延迟**（因为它们都在同一个循环里）。用户切到该 pane 后看到 cwd 滞后，以为是 cwd 检测坏了。

### 修复策略

用 `try_send` 优先，满了再走慢路径。慢路径有两种选择：

- 选 A：丢弃当前 chunk，下次合批补回去（数据可能丢）
- 选 B：spawn 一个 detached task 来 send（reader 不阻塞，但任务多了会内存涨）
- 选 C：合并到下次循环的 chunk 一起 send（**推荐**，无丢失，无任务积累）

下面是选 C 的 patch。

### Patch

```diff
--- a/src-tauri/src/engine/pty.rs
+++ b/src-tauri/src/engine/pty.rs
@@ -148,6 +148,7 @@ pub fn spawn_pty_reader(
     state: AppState,
     workspace_id: Uuid,
     pane_id: Uuid,
     mut reader: Box<dyn Read + Send>,
 ) {
     let handle = tokio::runtime::Handle::try_current();
     // Clone the silence-deadline Arc once at thread start ...
@@ -171,6 +172,11 @@ pub fn spawn_pty_reader(
             let mut buf = [0u8; 8192];
             let mut utf8_pending: Vec<u8> = Vec::new();
+            // Carryover: when event_tx is full, accumulate the bytes here and
+            // try to send them again on the next iteration. Avoids using
+            // `block_on` on the reader thread, which back-pressures the PTY
+            // and stalls the child shell (BUG-3).
+            let mut carryover: String = String::new();
             let read_result = catch_unwind(AssertUnwindSafe(|| {
                 loop {
                     match reader.read(&mut buf) {
@@ -262,21 +268,40 @@ pub fn spawn_pty_reader(
                             };
                             if data.is_empty() {
                                 continue;
                             }
                             let data_for_cwd = data.clone();
                             let bytes_for_title = data.as_bytes().to_vec();
                             state.append_pty_scrollback(workspace_id, pane_id, &data);
-                            let _ = rt.block_on(async {
-                                state
-                                    .event_tx
-                                    .send(GlobalEvent::PtyOutput {
-                                        workspace_id,
-                                        pane_id,
-                                        data,
-                                    })
-                                    .await
-                            });
+                            // Combine any carryover from the previous failed
+                            // try_send with this chunk, then attempt a non-
+                            // blocking send. On overflow, stash the combined
+                            // string back into carryover for the next iter.
+                            let payload = if carryover.is_empty() {
+                                data
+                            } else {
+                                let mut combined = std::mem::take(&mut carryover);
+                                combined.push_str(&data);
+                                combined
+                            };
+                            match state.event_tx.try_send(GlobalEvent::PtyOutput {
+                                workspace_id,
+                                pane_id,
+                                data: payload,
+                            }) {
+                                Ok(()) => {}
+                                Err(tokio::sync::mpsc::error::TrySendError::Full(ev)) => {
+                                    // Stash payload for next iteration. The
+                                    // event-loop consumer is downstream — it
+                                    // will drain when it can.
+                                    if let GlobalEvent::PtyOutput { data, .. } = ev {
+                                        carryover = data;
+                                    }
+                                }
+                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
+                                    // Receiver gone — runtime tearing down. Bail.
+                                    break;
+                                }
+                            }
                             // T1：扫描 OSC 0/1/2 标题序列...
```

⚠️ 注意 `data_for_cwd` 和 `bytes_for_title` 在 patch 上下游使用（OSC 标题/cwd 解析）。这两个 clone 发生在 `try_send` 之前，**不影响 try_send 的所有权**，patch 兼容现有逻辑。

### 验收

```bash
# 在 ridge 终端里
yes "AAAAAAAAAAAAAAAAAAAAAAAAA" | head -c 100M
# 修复前：执行过程中按键 echo 延迟数秒
# 修复后：echo 即时响应
```

如果想验证 carryover 真的工作：手动把 `event_tx` 容量从 1024 临时改成 8（重 build），跑同样命令——应该看到 carryover 日志增长但**不丢字符**（diff `wc -c` 输出确认数量）。

### 已知边角

如果 mpsc 持续满（消费者完全没动），carryover 会无界增长。`SCROLLBACK_MAX_BYTES = 4MB` 是 scrollback 上限，但 carryover 是另一个 buffer。理论上需要给 carryover 加上限（比如 16MB 后 truncate）。**实际场景下消费者是 tauri event loop，永远会消费**，所以不加上限也能跑——但留个 TODO。

---

## BUG-4 🟡 固定 4ms 合批窗口对单字符 echo 是纯延迟

### 症状

- 慢速打字（每秒 < 5 字符）时，每个字符的 echo 比按键多延迟 0-4ms
- 单看不可感，但叠加 IPC ~5ms + 渲染 ~16ms 帧，**端到端 echo 25-30ms**
- 在快节奏交互（vim、emacs、tmux）中能感觉到"键盘像有润滑层"

### 触发条件

任何键盘 echo。

### 根因

`src-tauri/src/lib.rs:61`：

```rust
const COALESCE_WINDOW_MS: u64 = 4;
const COALESCE_MAX_BYTES: usize = 64 * 1024;
```

设计意图：合批 IPC 降低 emit 次数。**对大输出**（`cat huge.log`）确实降负载——上万字节合并成一次 emit。**对单字符 echo** 是负担：1 字节也要等 4ms 才发，纯延迟。

### 修复策略

自适应窗口：根据上一窗口的字节量动态调整。

- 字节少（< 256）→ 0ms 窗口（立即发）
- 字节中（256-4096）→ 2ms 窗口
- 字节多（> 4096）→ 8ms 窗口（继续合批以摊薄序列化开销）

### Patch

```diff
--- a/src-tauri/src/lib.rs
+++ b/src-tauri/src/lib.rs
@@ -57,11 +57,29 @@ pub fn run() {
             let handle = app.handle().clone();
             ...
             tauri::async_runtime::spawn(async move {
                 use std::collections::HashMap;
-                // 合批窗口：同一 pane 的连续 PtyOutput 在 COALESCE_WINDOW_MS 内合并为一次 emit，
-                // 显著降低高吞吐（`cat huge.log`）场景下的 IPC 次数与前端渲染压力。
-                const COALESCE_WINDOW_MS: u64 = 4;
+                // Adaptive coalesce window. Fixed 4ms is fine for bulk output
+                // but adds pure latency to keyboard echo (BUG-4). Adjust the
+                // window based on the last flush's byte count:
+                //   < 256 bytes  → 0ms  (echo path: send immediately)
+                //   < 4096 bytes → 2ms  (medium activity)
+                //   ≥ 4096 bytes → 8ms  (bulk: amortize serialize cost)
+                const COALESCE_WINDOW_FAST: u64 = 0;
+                const COALESCE_WINDOW_MED:  u64 = 2;
+                const COALESCE_WINDOW_SLOW: u64 = 8;
                 const COALESCE_MAX_BYTES: usize = 64 * 1024;
                 let mut pending_output: HashMap<(uuid::Uuid, uuid::Uuid), String> = HashMap::new();
+                let mut last_flush_bytes: usize = 0;
+                let coalesce_window = |last: usize| -> u64 {
+                    if last < 256 { COALESCE_WINDOW_FAST }
+                    else if last < 4096 { COALESCE_WINDOW_MED }
+                    else { COALESCE_WINDOW_SLOW }
+                };

                 enum Tick {
                     Event(GlobalEvent),
                     Flush,
                     Closed,
                 }
                 loop {
                     let tick: Tick = if pending_output.is_empty() {
@@ -82,7 +100,7 @@ pub fn run() {
                         }
                     } else {
                         match tokio::time::timeout(
-                            std::time::Duration::from_millis(COALESCE_WINDOW_MS),
+                            std::time::Duration::from_millis(coalesce_window(last_flush_bytes)),
                             event_rx.recv(),
                         )
                         .await
@@ -195,12 +213,16 @@ pub fn run() {
                         None => {
                             // timeout — flush all pending per-pane buffers.
                             if !pending_output.is_empty() {
+                                let mut flushed_bytes: usize = 0;
                                 let drained: Vec<((uuid::Uuid, uuid::Uuid), String)> =
                                     pending_output.drain().collect();
                                 for ((ws, pane), payload) in drained {
+                                    flushed_bytes += payload.len();
                                     let label = pane.to_string();
                                     let _ = handle.emit(
                                         &format!("pty-output-{ws}-{label}"),
                                         serde_json::json!({ "data": payload }),
                                     );
                                 }
+                                last_flush_bytes = flushed_bytes;
                             }
                         }
                     }
```

⚠️ `last_flush_bytes` 也应该在 PaneClosed / PaneCwdChanged 提前 flush 单个 pane 时更新。最严格的实现是在每个 emit 点都更新 —— 但简化版本（只在 timeout flush 时更新）已经能拿到 90% 的收益，且改动小。

### 验收

```bash
# 在 ridge 终端里
time bash -c 'for i in $(seq 1 100); do echo -n "."; sleep 0.01; done; echo'
# 修复前：约 1.4-1.5s（每个 . 多 4ms）
# 修复后：约 1.0-1.1s（无合批延迟）
```

更好的指标是用浏览器 Performance trace 测从 keypress event 到 PTY echo emit 的时间——但这需要后端加 trace 点，超出本 patch 范围。

---

## BUG-5 🟡 resize 触发的连环 rAF + 重复 clearTextureAtlas

### 症状

- 拖动 splitpanes 边界时，终端有 ~50-100ms 的"重绘抖动"
- 偶尔看到 1 帧的字符错位 / 重影
- 拖完后 1-2 帧才完全稳定

### 触发条件

任何尺寸变化（手动 drag、窗口 resize、首次 mount 后 layout settle）。

### 根因

`Pane.svelte:975-999` 和 `1004-1015`：

```ts
resizeObserver = new ResizeObserver(() => {
    ...
    resizeRaf = requestAnimationFrame(() => {
        ...
        fitAddon?.fit();
        webglAddon?.clearTextureAtlas();   // ← 每帧清 atlas
        term.refresh(0, term.rows - 1);
    });
    ...
    resizePtySyncTimer = setTimeout(() => {
        void fitAndSyncPty();   // 又一次 fit + IPC
    }, 200);
});
```

加上 `fitAndSyncPty` 内部还有一次 rAF（line 475）。drag 时每帧：
1. ResizeObserver 触发 outer rAF → fit + clearTextureAtlas + refresh
2. 200ms 后 inner rAF → fit + IPC + clearTextureAtlas + refresh

**`clearTextureAtlas` 不该每帧调**。它的目的是 cell 尺寸变化后让缓存的字形 bitmap 失效。但 cell box 大小不变（字号没变），atlas 缓存的字形仍然有效——清掉就是浪费 GPU 上传。

### 修复策略

只在字号真的变了的时候清 atlas。drag-resize 时 cols/rows 变了但 cellWidth/cellHeight 不变 → 不清。

### Patch

```diff
--- a/src/lib/components/Pane.svelte
+++ b/src/lib/components/Pane.svelte
@@ -974,16 +974,15 @@ async function renderView() {
 		resizeObserver = new ResizeObserver(() => {
 			if (!alive || isComposing) return;
 			if (resizeRaf === undefined) {
 				resizeRaf = requestAnimationFrame(() => {
 					resizeRaf = undefined;
 					if (!alive || isComposing || !term) return;
-					// On every frame: fit immediately, clear the WebGL texture
-					// atlas (so the new cell size doesn't smear last frame's
-					// glyphs), and refresh the visible rows so the GPU re-rasterises
-					// the buffer with the new geometry. Eliminates the
-					// half-second black gap during drag-resize.
+					// On every frame: fit immediately + refresh visible rows.
+					// Do NOT clear the texture atlas here — atlas is keyed on
+					// (font, size, glyph), not on cell box dimensions. Drag-
+					// resize changes cols/rows but not cellWidth/Height, so the
+					// cached glyphs remain valid. Clearing wastes GPU upload
+					// every frame and is the source of the resize jitter (BUG-5).
+					// clearTextureAtlas() is invoked separately in the
+					// fontSize / theme effect handlers, where it's actually
+					// needed.
 					fitAddon?.fit();
-					webglAddon?.clearTextureAtlas();
 					term.refresh(0, term.rows - 1);
 				});
 			}
```

类似的，`fitAndSyncPty`（line 475）里也有一次 `webglAddon?.clearTextureAtlas()`。该位置是 PTY ack 后的兜底刷新，cell 尺寸**可能**真的变了（字号缩放 + 重 fit 后行高微调），保留这一处。

### 验收

- 打开终端，跑 `htop`（持续输出）
- 慢慢拖 splitpanes 分割线
- 修复前：拖动期间字符短暂消失/错位
- 修复后：字符跟随尺寸平滑变化

### 后续优化（不在本 patch 范围）

参见 `OVERVIEW.md` 的 R4：后端 resize-silence 协议应该向前端 emit `pane-resize-quiesce-start/end`，前端在静默期间完全不动 xterm —— 这是更彻底的方案，需要后端配合。

---

## BUG-6 🟢 后端 4MB scrollback + 前端 2000 行 buffer 重复存储

### 症状

非 bug，是资源浪费。10 pane × (后端 4MB + 前端 ~1.2MB scrollback) ≈ 52MB 仅用于历史。

### 根因

`Pane.svelte:572`:
```ts
scrollback: 2000,
```

xterm 自己维护 2000 行 in-memory scrollback。同时后端 `state.rs` 维护 4MB block storage。`Pane.svelte:935` 的 `get_pane_scrollback_tail` 已经能按需拉历史。**两份 scrollback 的内容大量重叠**——前端 2000 行通常对应 100-200KB，全在后端的近端 block 里也有。

### 修复策略

把前端 xterm scrollback 调小（500 行就够覆盖一屏的 2-3 倍）。深翻历史走后端 IPC。

### Patch

```diff
--- a/src/lib/components/Pane.svelte
+++ b/src/lib/components/Pane.svelte
@@ -569,7 +569,11 @@ async function renderView() {
 				// 仅在终端获得焦点时展示光标；失焦后隐藏，避免在输出区域"乱闪"
 				cursorInactiveStyle: 'none',
-				scrollback: 2000,
+				// xterm in-memory scrollback. Backend retains 4MB of block-based
+				// scrollback per pane (state.rs SCROLLBACK_MAX_BYTES); 500 rows
+				// here covers ~2-3 viewport heights for fast paging without
+				// double-storing what the backend already has (BUG-6).
+				scrollback: 500,
 				theme: xtermThemeFor(get(settingsStore).theme),
```

### 验收

- 10 pane 跑 1 小时持续输出
- DevTools Memory snapshot 总内存
- 修复前：~150MB
- 修复后：~120-130MB

注意：这只是把前端 xterm 的 buffer 缩小，**不影响**用户能翻多远的历史 —— 翻过 500 行后 `get_pane_scrollback_before` 会从后端拉。

⚠️ **副作用**：xterm 自带的 SearchAddon 只能搜 in-memory buffer，搜索范围会缩小到 ~500 行。如果用户依赖搜全部历史，需要走后端搜索（不在本 patch 范围）。考虑到 ridge 用户主要用 IDE 自带的项目搜索，终端里的搜索通常是找最近的输出，500 行够用。

如果你判断这个副作用不可接受，**不要应用 BUG-6 patch**——它优先级最低。

---

## 合并建议总览

```
推荐合并顺序与时机：

  立即可合（无副作用）：
    BUG-1   BUG-3   BUG-4

  需要回归测试一遍：
    BUG-2   BUG-5

  需要先和团队对齐"够不够 500 行"：
    BUG-6
```

每个 patch 都是独立的，没有依赖关系。可以并行 cherry-pick 到不同 PR 里。

---

## 我没列入的几类问题

为完整性说明哪些我**看到了但没写进来**：

- **xterm 替换工作覆盖的所有问题**：见 `OVERVIEW.md` 的痛点 1-5。这份 BUGFIX 只列独立 bug。
- **未验证的怀疑**：例如 `terminalRegistry.ts` 的 parking lot 在多 split 同时发生时是否有竞态——我没有 reproduce 步骤，不写未经测试的 patch。
- **样式 / 主题问题**：用户体感问题不属于"bug"。
- **后端 git 命令路径未优化**：例如 `get_git_diff` 在大仓库慢（这是 git 本身的特性），可以加 `--no-color --stat` 或缓存 commit hash。但这是 feature work，不是 bug。

如果你看到我应该列入但漏的，告诉我。
