use parking_lot::Mutex;
use portable_pty::MasterPty;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::engine::cwd;
use crate::engine::parser::PaneParser;
use crate::engine::title;
use crate::state::AppState;
use crate::types::GlobalEvent;
use crate::utils::pty_log;

const PTY_READ_UTF8_PENDING_MAX: usize = 64 * 1024;

/// 统一 cwd 表示（Windows 下反斜杠 → 正斜杠），与 `process::normalize_cwd` 对齐，
/// 避免 `paneCwdStore` 上出现 `C:\code\ridge` 与 `C:/code/ridge` 两个键并存的别名。
fn normalize_cwd_str(raw: &str) -> String {
    #[cfg(windows)]
    {
        raw.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        raw.to_string()
    }
}

/// Extend `pending` with `chunk`, then drain leading complete UTF-8 into `String`.
/// Incomplete trailing bytes remain in `pending` for the next read.
fn take_decoded_utf8(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    if !chunk.is_empty() {
        pending.extend_from_slice(chunk);
    }
    if pending.len() > PTY_READ_UTF8_PENDING_MAX {
        let bytes = std::mem::replace(pending, Vec::new());
        return String::from_utf8_lossy(&bytes).into_owned();
    }
    let mut out = String::new();
    loop {
        if pending.is_empty() {
            break;
        }
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    out.push_str(unsafe { std::str::from_utf8_unchecked(&pending[..valid]) });
                    pending.drain(..valid);
                    continue;
                }
                if let Some(elen) = e.error_len() {
                    out.push_str(&String::from_utf8_lossy(&pending[..elen]));
                    pending.drain(..elen);
                    continue;
                }
                break;
            }
        }
    }
    out
}

fn flush_pending_eof(pending: &mut Vec<u8>) -> String {
    if pending.is_empty() {
        return String::new();
    }
    let bytes = std::mem::replace(pending, Vec::new());
    String::from_utf8_lossy(&bytes).into_owned()
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

/// 在 `data` 中查找最早出现的 shell-integration prompt OSC 起始字节偏移。
///
/// 检测下列序列起始（任一前 7 字节，不要求匹配 ST/BEL 终止符 —— xterm.js 会
/// 在收到流后自行解析完整序列）：
/// - `\x1b]133;A` / `\x1b]133;B` / `\x1b]133;P` — FinalTerm 语义 prompt 协议
/// - `\x1b]633;A` / `\x1b]633;B` / `\x1b]633;P` — VS Code shell-integration 扩展
///
/// 返回首个命中的字节偏移（基于原 `data: &str` 的字节位置，可安全用于
/// `data[off..]` 切片）。若未命中，返回 `None`。
fn find_prompt_osc(data: &str) -> Option<usize> {
    const MARKERS: [&str; 6] = [
        "\x1b]133;A",
        "\x1b]133;B",
        "\x1b]133;P",
        "\x1b]633;A",
        "\x1b]633;B",
        "\x1b]633;P",
    ];
    let mut earliest: Option<usize> = None;
    for m in MARKERS.iter() {
        if let Some(idx) = data.find(m) {
            earliest = Some(earliest.map_or(idx, |e| e.min(idx)));
        }
    }
    earliest
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
    // Clone the silence-deadline Arc once at thread start (single read-lock acquire),
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
                            // Resize silence: while ConPTY is replaying its viewport
                            // post-`ResizePseudoConsole`, drop bytes from BOTH scrollback
                            // and frontend emit until the next prompt OSC (FinalTerm
                            // OSC 133;A / VS Code OSC 633;A) tells us the shell is back
                            // at a clean prompt. Hard timeout (800ms) auto-releases for
                            // shells without shell-integration so we don't permanently
                            // mute the pane.
                            let deadline = silence_deadline.load(Ordering::Acquire);
                            let silenced = deadline > 0 && now_epoch_ms() < deadline;
                            let data = if silenced {
                                match find_prompt_osc(&raw) {
                                    Some(off) => {
                                        // Prompt OSC found — release silence and keep
                                        // only the post-OSC tail. Pre-OSC bytes are
                                        // ConPTY reflow noise; dropping them is the
                                        // whole point of this gate.
                                        silence_deadline.store(0, Ordering::Release);
                                        raw[off..].to_string()
                                    }
                                    None => {
                                        // Still inside reflow storm; drop bytes.
                                        // (Original outputs were already captured into
                                        // scrollback BEFORE the resize, so this drop
                                        // doesn't lose user-visible history.)
                                        continue;
                                    }
                                }
                            } else {
                                // Either never silenced, or silenced-but-timed-out.
                                // Reset deadline opportunistically so the next iteration
                                // doesn't redo the time math.
                                if deadline > 0 {
                                    silence_deadline.store(0, Ordering::Release);
                                }
                                raw
                            };
                            if data.is_empty() {
                                continue;
                            }
                            let data_for_cwd = data.clone();
                            let bytes_for_title = data.as_bytes().to_vec();
                            state.append_pty_scrollback(workspace_id, pane_id, &data);
                            // BUG-1 follow-up: scan for shell-integration prompt
                            // mark (FinalTerm OSC 133;A / VS Code OSC 633;A) BEFORE
                            // the try_send below moves `data`. We only need the
                            // boolean — the offset that find_prompt_osc returns
                            // isn't useful here. The actual emit happens after
                            // try_send so the cheaper `Ok` path stays hot.
                            // NOTE: in the silence-release case `data` is the
                            // post-OSC tail (the marker was stripped), so this
                            // scan misses that one chunk — the frontend's 800ms
                            // debounce fallback covers it.
                            let prompt_seen = find_prompt_osc(&data).is_some();
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
                            // T1：扫描 OSC 0/1/2 标题序列。shell 提示符、Claude Code、
                            // ssh 等都用这条机制设置窗口标题，emit 后前端按 teammate >
                            // OSC > 进程名 优先级合并展示。
                            if let Some(title_text) =
                                title::parse_title_from_output(&bytes_for_title)
                            {
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
                            if let Some(cwd) = cwd::parse_cwd_from_output(&data_for_cwd) {
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
        });
}