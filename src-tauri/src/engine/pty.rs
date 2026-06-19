use parking_lot::Mutex;
use portable_pty::MasterPty;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::engine::parser::PaneParser;
use crate::state::AppState;
use crate::teammate::layout_event::{LayoutChange, TEAMMATE_LAYOUT_CHANGED};
use crate::types::GlobalEvent;
use crate::utils::pty_log;
use ridge_core::pty::cwd;
use ridge_core::pty::decode::{flush_pending_eof, take_decoded_utf8};

/// 统一 cwd 表示（Windows 下反斜杠 → 正斜杠）。逻辑单一真源在
/// `ridge_core::commands::process::normalize_cwd`（与 OS 探测路径同一份实现），
/// 这里仅做 `&str → String` 适配，避免 `paneCwdStore` 上出现 `C:\code\ridge` 与
/// `C:/code/ridge` 两个键并存的别名。
fn normalize_cwd_str(raw: &str) -> String {
    ridge_core::commands::process::normalize_cwd(raw.to_string())
}

pub struct PtyHandle {
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// 子进程句柄。普通 pane 为 `Some`（关闭即 kill）；**领养的 native 视图**为
    /// `None`（子进程归 native engine 所有，关视图=detach 不杀）。
    pub _child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    /// `Some((socket, global_id))` 表示这是某 native 面板的 GUI 视图（领养，共享 PTY）。
    pub native_ref: Option<(String, usize)>,
    /// 领养视图的 `BroadcastReader` 取消位：detach 时置位让 reader 线程 EOF 退出。
    pub native_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Resize-silence deadline in epoch milliseconds. When `> 0` and `now < deadline`,
    /// the PTY reader thread suppresses scrollback writes AND frontend emits to swallow
    /// ConPTY's viewport-replay byte storm. Cleared (set to 0) the moment a prompt OSC
    /// (`OSC 133;A/B/P` FinalTerm or `OSC 633;A/B/P` VS Code shell-integration) is seen
    /// in the byte stream, OR when the hard timeout elapses.
    pub resize_silence_deadline: Arc<AtomicI64>,
    /// P3.8 (2026-05-20) — per-pane VT parser. The main event loop's
    /// `PtyOutput` arm checks `delta_mode`; when `true` it locks this
    /// parser, calls `feed_and_diff(bytes)`, postcard-encodes the
    /// resulting `DeltaFrame`, and emits `pty-delta-{ws}-{pane}` to the
    /// frontend. The legacy `pty-output-*` text path remains the
    /// fallback for `delta_mode = false`.
    ///
    /// Lives inside `Arc<Mutex<...>>` so the main loop holds a short
    /// lock per chunk without blocking other accessors (resize commands
    /// take the lock too, for symmetry with PTY native resize).
    pub parser: Arc<Mutex<PaneParser>>,
    /// P3.8 — per-pane toggle. Driven by `set_pane_delta_mode` (P3.9).
    /// When `true`, PTY bytes go through `parser.feed_and_diff` →
    /// `encode_frame` → `pty-delta-*` emit. When `false`, bytes go
    /// through the legacy coalescer → `pty-output-*` emit. Atomic so
    /// the main loop reads it without acquiring the parser mutex.
    pub delta_mode: Arc<AtomicBool>,
}

/// 默认 resize 静默窗口（毫秒）。ConPTY 在 `ResizePseudoConsole` 后会把整个
/// viewport 通过 stdout 重发；这段重发到 PTY 读端的延迟一般在 50-300ms。
///
/// **2026-05-05 缩到 250 ms**：旧值 800 ms 在没有 shell-integration（FinalTerm
/// `OSC 133;A` 或 VS Code `OSC 633;A`）的 shell / CLI 上会把 SIGWINCH 触发的
/// redraw 整段吞掉——用户报告 resize 后 cursor 还在「之前的位置」、字符画
/// 错位、连续字符画不成功，根因都是 redraw 字节落进静默窗口被丢弃，kernel
/// grid 保留 reflow 前的内容看上去「没刷新」。把窗口缩到 250 ms 后，shell
/// 的 SIGWINCH 重画几乎都能在 250 ms 之后落入 kernel，对没有 shell-
/// integration 的环境也能让光标快速恢复正确位置；Shell 启用了 shell-
/// integration 时仍会被 prompt OSC 提前截断（早于 250 ms），保持原行为。
/// 250 ms 仍然覆盖 ConPTY replay 区间的下沿；replay tail 偶尔会泄漏少量
/// 字节进入 kernel，但相对「光标卡 800ms 在错位置」是更小的视觉事故。
/// 静默窗口硬上限。每次 `resize_pane` 后 PTY reader 在该窗口内丢弃 ConPTY
/// viewport replay 字节（直到命中 OSC 133;A / OSC 633;A 之类的 prompt OSC
/// 即提前释放）。§A.2 (2026-05-07)：从 250 ms 缩到 80 ms。原 250 ms 会把
/// PSReadLine / fish-zle / zsh-zle 的 SIGWINCH 重画字节也吞掉（它们通常在
/// SIGWINCH 后 10–50 ms 落地），导致 §1.26 「resize 后 prompt 间距塌缩 +
/// 字符残留」。80 ms 仍然覆盖 ConPTY replay 的下沿，且让合法 redraw 顺利
/// 到达 kernel；万一仍有 redraw 字节漏进窗口，§1.26 在 `grid.rs::resize`
/// 末尾对 primary 屏从 cursor.col+1 起的清理是最后兜底——后续 redraw 落
/// 在已清理过的区域上，不留鬼影。
pub const RESIZE_SILENCE_WINDOW_MS: i64 = 80;

/// 当前 epoch 毫秒；时钟异常时返回 0（导致 `silent` 判定为 false，安全降级）。
fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 从工作区表里摘掉该 pane 的 PTY（读线程结束或异常时用）。不影响其它 pane 的表项。
fn detach_terminal(state: &AppState, workspace_id: Uuid, pane_id: Uuid) {
    state.clear_pty_scrollback(workspace_id, pane_id);
    let mut map = state.workspaces.write();
    if let Some(ws) = map.get_mut(&workspace_id) {
        ws.terminals.remove(&pane_id);
    }
}

/// 在独立线程里阻塞读 PTY，经 `Handle::block_on` 写回 channel，避免占满 Tokio worker。
///
/// 隔离性说明（root 崩溃不影响兄弟窗格）：
/// - 每个窗格对应 **独立子进程 + 独立 PTY**，与 UI 上叫 `root` 还是子 UUID 无关；OS 上互为兄弟进程。
/// - 本线程只操作本 pane 的 `reader` 与本 workspace 的 **单条** `terminals[pane_id]`；`catch_unwind` 避免本线程 panic 直接终止整个进程。
/// - 读结束后 `detach_terminal` 只删除当前 id，**不会**遍历或修改其它 pane。
/// - 仍属同一 Tauri 进程：若 native 库段错误或 `panic = abort`，理论上仍可能全进程退出——需进程级隔离才有硬保证。
pub fn spawn_pty_reader(
    state: AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    mut reader: Box<dyn Read + Send>,
) {
    let handle = tokio::runtime::Handle::try_current();
    // Clone the silence-deadline Arc once at thread start (single read-lock acquisition),
    // so the per-iteration silence check is a pure atomic load with no map locking.
    // If the handle is gone (race with rapid open+close), fall back to a fresh atomic
    // — silence will simply never activate, which is safe.
    let silence_deadline: Arc<AtomicI64> = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .map(|h| h.resize_silence_deadline.clone())
            .unwrap_or_else(|| Arc::new(AtomicI64::new(0)))
    };
    // Snapshot native_ref at spawn time. When a native pane's broadcast closes
    // (child died or detach), the reader EOF must perform a *full* cleanup
    // (remove from pane_tree + emit layout-changed) — not the half-cleanup
    // that `detach_terminal` does, and NOT the `PaneClosed` event that would
    // cause the frontend to rebuild a new shell inside the dead native pane.
    let native_ref_info: Option<(String, usize)> = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .and_then(|h| h.native_ref.clone())
    };
    // Capture this PTY's generation at spawn. On EOF we compare against the
    // pane's current generation; if it advanced, the pane was torn down and
    // replaced (this reader is stale) → skip the child-exit→Idle demotion so a
    // freshly-spawned agent's Busy is never clobbered. See
    // `teardown_pane_pty_if_present`.
    let my_pty_generation: u64 = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.pty_generation.get(&pane_id).copied())
            .unwrap_or(0)
    };
    let _ = std::thread::Builder::new()
        .name(format!("pty-reader-{pane_id}"))
        .spawn(move || {
            let Ok(rt) = handle else {
                pty_log::reader_no_runtime(workspace_id, pane_id);
                return;
            };
            let mut buf = [0u8; 8192];
            let mut utf8_pending: Vec<u8> = Vec::new();
            // Carryover buffer for the BUG-3 try_send path. When event_tx is
            // momentarily full, the bytes that failed to send are stashed here
            // and prepended to the next iteration's chunk. This avoids
            // `rt.block_on(send)` on the reader thread, which would
            // back-pressure the kernel pipe and stall the child shell during
            // bursts. The consumer (the global event-loop in lib.rs) is the
            // only thing draining event_tx, so as long as it makes progress,
            // carryover stays small. No upper bound: scrollback already retains
            // history independently, so we don't need to truncate carryover.
            let mut carryover: String = String::new();
            // Domain A3：本 pane 的 StreamCleaner（跨读循环复用——TML 标记可能跨 chunk
            // 切分）。默认关闭时 `stream::apply` 直接原样返回，不触碰它（零开销）。
            let mut tml_cleaner =
                ridge_core::StreamCleaner::new(&workspace_id.to_string(), &pane_id.to_string());
            let read_result = catch_unwind(AssertUnwindSafe(|| {
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            let tail = flush_pending_eof(&mut utf8_pending);
                            if !tail.is_empty() {
                                let tail_for_cwd = tail.clone();
                                state.append_pty_scrollback(workspace_id, pane_id, &tail);
                                let _ = rt.block_on(async {
                                    state
                                        .event_tx
                                        .send(GlobalEvent::PtyOutput {
                                            workspace_id,
                                            pane_id,
                                            data: tail,
                                        })
                                        .await
                                });
                                if let Some(cwd) = cwd::parse_cwd_from_output(&tail_for_cwd) {
                                    {
                                        let mut map = state.workspaces.write();
                                        if let Some(ws) = map.get_mut(&workspace_id) {
                                            if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                                                pane.cwd = Some(std::path::PathBuf::from(normalize_cwd_str(&cwd.to_string_lossy())));
                                                tracing::debug!(target: "ridge::cwd", workspace = %workspace_id, pane = %pane_id, cwd = %cwd.display(), "OSC 7 cwd applied");
                                            }
                                        }
                                    }
                                    crate::commands::ridge_file::schedule_auto_save(&state, workspace_id);
                                    let event_tx = state.event_tx.clone();
                                    let workspace_id = workspace_id.clone();
                                    let pane_id = pane_id.clone();
                                    let cwd_clone = cwd.clone();
                                    let _ = rt.block_on(async move {
                                        let _ = event_tx
                                            .send(GlobalEvent::PaneCwdChanged {
                                                workspace_id,
                                                pane_id,
                                                cwd: normalize_cwd_str(&cwd_clone.to_string_lossy()),
                                            })
                                            .await;
                                    });
                                }
                            }
                            pty_log::reader_eof(workspace_id, pane_id);
                            break;
                        }
                        Ok(n) => {
                            let raw = take_decoded_utf8(&mut utf8_pending, &buf[..n]);
                            if raw.is_empty() {
                                continue;
                            }
                            // Domain A3（默认关，开启时隐藏 TML 区间 + 上抛 tml-message）。
                            let raw = crate::teammate::stream::apply(&state, &mut tml_cleaner, raw);
                            if raw.is_empty() {
                                continue; // 整块都是被隐藏的 TML
                            }
                            // Resize-silence gate + signal scan (ConPTY resize-replay
                            // drop → prompt/title/cwd) is the AppState-agnostic core of
                            // this loop, extracted to `ridge_core::pty::chunk` so the
                            // headless host can reuse the SAME reduction. The thread,
                            // scrollback, event routing, carryover backpressure and EOF
                            // cleanup below stay here (AppState-bound). `data_for_cwd`/
                            // `bytes_for_title` are no longer needed — `signals` carries
                            // the precomputed prompt/title/cwd. Behaviour is byte-for-byte
                            // the original gate (see chunk::process + its tests).
                            let deadline = silence_deadline.load(Ordering::Acquire);
                            let outcome =
                                ridge_core::pty::chunk::process(raw, deadline, now_epoch_ms());
                            if outcome.clear_silence {
                                silence_deadline.store(0, Ordering::Release);
                            }
                            let Some(signals) = outcome.emit else {
                                // Dropped inside the ConPTY resize-replay window.
                                continue;
                            };
                            let data = signals.text;
                            if data.is_empty() {
                                continue;
                            }
                            state.append_pty_scrollback(workspace_id, pane_id, &data);
                            // Precomputed by chunk::process. NOTE: in the silence-release
                            // case `data` is the post-OSC tail (marker stripped), so this
                            // is false there — the frontend's 800ms debounce covers it.
                            let prompt_seen = signals.prompt_seen;
                            // BUG-3: non-blocking try_send + carryover. If the
                            // global event_rx is full, stash the (combined)
                            // payload back into carryover for retry on the
                            // next iteration instead of blocking the reader.
                            let payload = if carryover.is_empty() {
                                data
                            } else {
                                let mut combined = std::mem::take(&mut carryover);
                                combined.push_str(&data);
                                combined
                            };
                            match state.event_tx.try_send(GlobalEvent::PtyOutput {
                                workspace_id,
                                pane_id,
                                data: payload,
                            }) {
                                Ok(()) => {}
                                Err(tokio::sync::mpsc::error::TrySendError::Full(ev)) => {
                                    if let GlobalEvent::PtyOutput { data, .. } = ev {
                                        carryover = data;
                                    }
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    // Receiver gone — runtime tearing down.
                                    break;
                                }
                            }
                            // BUG-1 follow-up emit. block_on (not try_send) is OK
                            // because prompt events fire ~once per command and
                            // consistency with the title/cwd block_on path
                            // matters more than reader throughput.
                            if prompt_seen {
                                let event_tx = state.event_tx.clone();
                                let _ = rt.block_on(async move {
                                    let _ = event_tx
                                        .send(GlobalEvent::PanePromptDetected {
                                            workspace_id,
                                            pane_id,
                                        })
                                        .await;
                                });
                            }
                            // T1：OSC 0/1/2 标题（由 chunk::process 扫出）。shell 提示符、
                            // Claude Code、ssh 等都用这条机制设置窗口标题，emit 后前端按
                            // teammate > OSC > 进程名 优先级合并展示。
                            if let Some(title_text) = signals.title {
                                let event_tx = state.event_tx.clone();
                                let _ = rt.block_on(async move {
                                    let _ = event_tx
                                        .send(GlobalEvent::PaneTitleChanged {
                                            workspace_id,
                                            pane_id,
                                            title: title_text,
                                        })
                                        .await;
                                });
                            }
                            if let Some(cwd) = signals.cwd {
                                // Normalize path separators on Windows so every code path
                                // (main-loop OSC 7, EOF flush, process-poll) stores and
                                // emits the SAME string for the same directory.
                                // Without this, Git Bash emits "C:/code" while PowerShell
                                // shell-integration emits "C:\code", and paneCwdStore ends
                                // up with two different keys for the same directory —
                                // preventing the Explorer file-tree column merge.
                                let normalized = normalize_cwd_str(&cwd.to_string_lossy());
                                {
                                    let mut map = state.workspaces.write();
                                    if let Some(ws) = map.get_mut(&workspace_id) {
                                        if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                                            pane.cwd = Some(std::path::PathBuf::from(&normalized));
                                        }
                                    }
                                }
                                let event_tx = state.event_tx.clone();
                                let workspace_id = workspace_id.clone();
                                let pane_id = pane_id.clone();
                                let _ = rt.block_on(async move {
                                    let _ = event_tx
                                        .send(GlobalEvent::PaneCwdChanged {
                                            workspace_id,
                                            pane_id,
                                            cwd: normalized,
                                        })
                                        .await;
                                });
                            }
                        }
                        Err(e) => {
                            let tail = flush_pending_eof(&mut utf8_pending);
                            if !tail.is_empty() {
                                let tail_for_cwd = tail.clone();
                                state.append_pty_scrollback(workspace_id, pane_id, &tail);
                                let _ = rt.block_on(async {
                                    state
                                        .event_tx
                                        .send(GlobalEvent::PtyOutput {
                                            workspace_id,
                                            pane_id,
                                            data: tail,
                                        })
                                        .await
                                });
                                if let Some(cwd) = cwd::parse_cwd_from_output(&tail_for_cwd) {
                                    {
                                        let mut map = state.workspaces.write();
                                        if let Some(ws) = map.get_mut(&workspace_id) {
                                            if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                                                pane.cwd = Some(std::path::PathBuf::from(normalize_cwd_str(&cwd.to_string_lossy())));
                                                tracing::debug!(target: "ridge::cwd", workspace = %workspace_id, pane = %pane_id, cwd = %cwd.display(), "OSC 7 cwd applied");
                                            }
                                        }
                                    }
                                    crate::commands::ridge_file::schedule_auto_save(&state, workspace_id);
                                    let event_tx = state.event_tx.clone();
                                    let workspace_id = workspace_id.clone();
                                    let pane_id = pane_id.clone();
                                    let cwd_clone = cwd.clone();
                                    let _ = rt.block_on(async move {
                                        let _ = event_tx
                                            .send(GlobalEvent::PaneCwdChanged {
                                                workspace_id,
                                                pane_id,
                                                cwd: normalize_cwd_str(&cwd_clone.to_string_lossy()),
                                            })
                                            .await;
                                    });
                                }
                            }
                            pty_log::reader_io_err(workspace_id, pane_id, &e);
                            break;
                        }
                    }
                }
            }));

            if read_result.is_err() {
                eprintln!(
                    "[ridge] PTY reader panicked (isolated to this thread) workspace={workspace_id} pane={pane_id}"
                );
            }

            if let Some((socket, gid)) = &native_ref_info {
                // Native pane died or was detached: full cleanup — remove from pane_tree,
                // clear registry attachment, emit layout-changed. Do NOT send PaneClosed
                // (which would cause the frontend to rebuild a new non-native shell).
                crate::teammate::native::set_attachment(socket, *gid, None);
                state.clear_pty_scrollback(workspace_id, pane_id);
                state.unregister_pane_delta_channel(workspace_id, pane_id);
                {
                    let mut map = state.workspaces.write();
                    if let Some(ws) = map.get_mut(&workspace_id) {
                        ws.terminals.remove(&pane_id);
                        let _ = ws.pane_tree.close(pane_id);
                        ws.pane_sizes.remove(&pane_id);
                        // DF=②: 销毁叶子前清 teammate 生命周期状态/映射，避免反向泄漏出
                        // 指向已不存在 pane 的孤儿条目。
                        ws.teammate_pane_states.remove(&pane_id);
                        ws.teammate_agent_pane_map.retain(|_, v| *v != pane_id);
                        ws.pty_generation.remove(&pane_id);
                    }
                }
                if let Some(app) = state.app_handle.get() {
                    use tauri::Emitter;
                    let _ = app.emit(
                        TEAMMATE_LAYOUT_CHANGED,
                        LayoutChange::detached(pane_id.to_string()),
                    );
                }
            } else {
                // Ordinary pane: send PaneClosed so the frontend rebuilds a shell,
                // then detach the terminal handle.
                let _ = rt.block_on(async {
                    state
                        .event_tx
                        .send(GlobalEvent::PaneClosed {
                            workspace_id,
                            pane_id,
                        })
                        .await
                });
                detach_terminal(&state, workspace_id, pane_id);
                // DF=② (child-exit → Idle): an agent (or shell) in a *teammate*
                // pane exited (reader EOF). Demote the pane to Idle and drop its
                // agent mapping so need-1 can re-use it and the AGENT badge
                // clears. SKIP when a PendingSpawn is queued for this pane — that
                // means we're mid-replacement (e.g. an agent spawn-process tore
                // down the prior shell and queued the agent), and flipping to
                // Idle here would clobber the incoming agent's just-set Busy.
                // Only flip EXISTING teammate panes (never add an entry for a
                // plain GUI pane). Driven purely by Ridge's own EOF signal — no
                // dependency on the harness calling release-pane.
                let flipped_to_idle = {
                    let mut map = state.workspaces.write();
                    if let Some(ws) = map.get_mut(&workspace_id) {
                        // Genuine exit ONLY if this reader is still the pane's
                        // current PTY (generation unchanged since spawn) AND no
                        // replacement is queued. A bumped generation means the
                        // pane was torn down + replaced (e.g. reuse/spawn-process
                        // installed an agent) → this is the OLD shell reader; we
                        // MUST NOT demote, or we'd clobber the new agent's Busy
                        // permanently. Generation closes the [teardown, register)
                        // window the pending_spawns check alone missed; the
                        // pending_spawns term stays as belt-and-suspenders.
                        let current_gen =
                            ws.pty_generation.get(&pane_id).copied().unwrap_or(0);
                        let is_current_pty = current_gen == my_pty_generation;
                        let being_replaced = ws.pending_spawns.contains_key(&pane_id);
                        if is_current_pty
                            && !being_replaced
                            && ws.teammate_pane_states.contains_key(&pane_id)
                        {
                            ws.teammate_pane_states
                                .insert(pane_id, crate::state::PaneState::Idle);
                            ws.teammate_agent_pane_map.retain(|_, v| *v != pane_id);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if flipped_to_idle {
                    if let Some(app) = state.app_handle.get() {
                        use tauri::Emitter;
                        let _ = app.emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
                    }
                }
            }
        });
}
