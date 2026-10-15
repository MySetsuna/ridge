//! PTY / resize 轻量诊断日志（stderr → 开发时终端可见；release 下仅保留错误级）。

use uuid::Uuid;

#[cfg(debug_assertions)]
pub fn resize_ok(workspace_id: Uuid, pane_id: Uuid, rows: u16, cols: u16) {
    eprintln!("[ridge][pty] resize_ok ws={workspace_id} pane={pane_id} rows={rows} cols={cols}");
}

#[cfg(not(debug_assertions))]
pub fn resize_ok(_workspace_id: Uuid, _pane_id: Uuid, _rows: u16, _cols: u16) {}

pub fn resize_err(workspace_id: Uuid, pane_id: Uuid, rows: u16, cols: u16, err: &str) {
    eprintln!(
        "[ridge][pty] resize_fail ws={workspace_id} pane={pane_id} rows={rows} cols={cols} err={err}"
    );
}

pub fn pane_not_found(op: &str, workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[ridge][pty] {op}_pane_missing ws={workspace_id} pane={pane_id}");
}

#[cfg(debug_assertions)]
pub fn create_skip(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[ridge][pty] create_skip_exists ws={workspace_id} pane={pane_id}");
}

#[cfg(not(debug_assertions))]
pub fn create_skip(_workspace_id: Uuid, _pane_id: Uuid) {}

#[cfg(debug_assertions)]
pub fn create_spawned(workspace_id: Uuid, pane_id: Uuid, trace_id: &str) {
    eprintln!("[ridge][pty] create_spawned ws={workspace_id} pane={pane_id} trace={trace_id}");
}

#[cfg(not(debug_assertions))]
pub fn create_spawned(_workspace_id: Uuid, _pane_id: Uuid, _trace_id: &str) {}

pub fn reader_eof(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[ridge][pty] reader_eof ws={workspace_id} pane={pane_id}");
}

pub fn reader_io_err(workspace_id: Uuid, pane_id: Uuid, err: &std::io::Error) {
    eprintln!("[ridge][pty] reader_io_err ws={workspace_id} pane={pane_id} err={err}");
}

pub fn reader_no_runtime(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!(
        "[ridge][pty] reader_no_tokio_runtime ws={workspace_id} pane={pane_id} (PTY 读线程退出，请检查是否在 async 上下文中 spawn)"
    );
}

/// 前端 `create_pane` 先于 teammate split 挂了交互 shell，拆掉以便按 `initial_command` 重起。
#[cfg(debug_assertions)]
pub fn teammate_replace_pty(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!(
        "[ridge][pty] teammate_replace_pty ws={workspace_id} pane={pane_id} (remove existing PTY for split command)"
    );
}

#[cfg(not(debug_assertions))]
pub fn teammate_replace_pty(_workspace_id: Uuid, _pane_id: Uuid) {}

/// Phase-1 spawn: PTY pair opened, child not yet started. Trace id rides
/// through to `activate_pane_pty` for cross-correlation in logs.
#[cfg(debug_assertions)]
pub fn create_pending(workspace_id: Uuid, pane_id: Uuid, trace_id: &str) {
    eprintln!("[ridge][pty] create_pending ws={workspace_id} pane={pane_id} trace={trace_id}");
}

#[cfg(not(debug_assertions))]
pub fn create_pending(_workspace_id: Uuid, _pane_id: Uuid, _trace_id: &str) {}

/// Phase-2 spawn failed: child process couldn't start. Always logged at
/// error severity since it indicates an actionable problem (bad PATH,
/// permission denied, etc.).
pub fn activate_err(workspace_id: Uuid, pane_id: Uuid, trace_id: &str, err: &str) {
    eprintln!(
        "[ridge][pty] activate_err ws={workspace_id} pane={pane_id} trace={trace_id} err={err}"
    );
}
