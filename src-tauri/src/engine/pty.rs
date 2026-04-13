use parking_lot::Mutex;
use portable_pty::MasterPty;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use uuid::Uuid;

use crate::state::AppState;
use crate::types::GlobalEvent;
use crate::utils::pty_log;

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
            let read_result = catch_unwind(AssertUnwindSafe(|| {
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            pty_log::reader_eof(workspace_id, pane_id);
                            break;
                        }
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buf[0..n]).into_owned();
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
                        }
                        Err(e) => {
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
