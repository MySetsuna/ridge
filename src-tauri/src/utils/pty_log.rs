//! PTY / resize 轻量诊断日志（stderr → 开发时终端可见；release 下仅保留错误级）。

use uuid::Uuid;

#[cfg(debug_assertions)]
pub fn resize_ok(workspace_id: Uuid, pane_id: Uuid, rows: u16, cols: u16) {
    eprintln!(
        "[wind][pty] resize_ok ws={workspace_id} pane={pane_id} rows={rows} cols={cols}"
    );
}

#[cfg(not(debug_assertions))]
pub fn resize_ok(_workspace_id: Uuid, _pane_id: Uuid, _rows: u16, _cols: u16) {}

pub fn resize_err(workspace_id: Uuid, pane_id: Uuid, rows: u16, cols: u16, err: &str) {
    eprintln!(
        "[wind][pty] resize_fail ws={workspace_id} pane={pane_id} rows={rows} cols={cols} err={err}"
    );
}

pub fn pane_not_found(op: &str, workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[wind][pty] {op}_pane_missing ws={workspace_id} pane={pane_id}");
}

#[cfg(debug_assertions)]
pub fn create_skip(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[wind][pty] create_skip_exists ws={workspace_id} pane={pane_id}");
}

#[cfg(not(debug_assertions))]
pub fn create_skip(_workspace_id: Uuid, _pane_id: Uuid) {}

#[cfg(debug_assertions)]
pub fn create_spawned(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[wind][pty] create_spawned ws={workspace_id} pane={pane_id}");
}

#[cfg(not(debug_assertions))]
pub fn create_spawned(_workspace_id: Uuid, _pane_id: Uuid) {}

pub fn reader_eof(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!("[wind][pty] reader_eof ws={workspace_id} pane={pane_id}");
}

pub fn reader_io_err(workspace_id: Uuid, pane_id: Uuid, err: &std::io::Error) {
    eprintln!("[wind][pty] reader_io_err ws={workspace_id} pane={pane_id} err={err}");
}

pub fn reader_no_runtime(workspace_id: Uuid, pane_id: Uuid) {
    eprintln!(
        "[wind][pty] reader_no_tokio_runtime ws={workspace_id} pane={pane_id} (PTY 读线程退出，请检查是否在 async 上下文中 spawn)"
    );
}
