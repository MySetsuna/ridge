use parking_lot::Mutex;
use portable_pty::MasterPty;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use uuid::Uuid;

use crate::engine::cwd;
use crate::state::AppState;
use crate::types::GlobalEvent;
use crate::utils::pty_log;

const PTY_READ_UTF8_PENDING_MAX: usize = 64 * 1024;

/// 统一 cwd 表示（Windows 下反斜杠 → 正斜杠），与 `process::normalize_cwd` 对齐，
/// 避免 `paneCwdStore` 上出现 `C:\code\wind` 与 `C:/code/wind` 两个键并存的别名。
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
    pub _child: Box<dyn portable_pty::Child + Send + Sync>,
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
    let _ = std::thread::Builder::new()
        .name(format!("pty-reader-{pane_id}"))
        .spawn(move || {
            let Ok(rt) = handle else {
                pty_log::reader_no_runtime(workspace_id, pane_id);
                return;
            };
            let mut buf = [0u8; 8192];
            let mut utf8_pending: Vec<u8> = Vec::new();
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
                                                tracing::debug!(target: "wind::cwd", workspace = %workspace_id, pane = %pane_id, cwd = %cwd.display(), "OSC 7 cwd applied");
                                            }
                                        }
                                    }
                                    crate::commands::wind_file::schedule_auto_save(&state, workspace_id);
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
                            let data = take_decoded_utf8(&mut utf8_pending, &buf[..n]);
                            if data.is_empty() {
                                continue;
                            }
                            let data_for_cwd = data.clone();
                            state.append_pty_scrollback(workspace_id, pane_id, &data);
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
                            if let Some(cwd) = cwd::parse_cwd_from_output(&data_for_cwd) {
                                {
                                    let mut map = state.workspaces.write();
                                    if let Some(ws) = map.get_mut(&workspace_id) {
                                        if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                                            pane.cwd = Some(cwd.clone());
                                        }
                                    }
                                }
                                let event_tx = state.event_tx.clone();
                                let workspace_id = workspace_id.clone();
                                let pane_id = pane_id.clone();
                                let cwd_clone = cwd.clone();
                                let _ = rt.block_on(async move {
                                    let _ = event_tx
                                        .send(GlobalEvent::PaneCwdChanged {
                                            workspace_id,
                                            pane_id,
                                            cwd: cwd_clone.to_string_lossy().to_string(),
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
                                                tracing::debug!(target: "wind::cwd", workspace = %workspace_id, pane = %pane_id, cwd = %cwd.display(), "OSC 7 cwd applied");
                                            }
                                        }
                                    }
                                    crate::commands::wind_file::schedule_auto_save(&state, workspace_id);
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
                    "[wind] PTY reader panicked (isolated to this thread) workspace={workspace_id} pane={pane_id}"
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