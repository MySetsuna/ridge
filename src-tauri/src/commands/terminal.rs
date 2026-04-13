use std::io::Write;

use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

use crate::engine::pty::{spawn_pty_reader, PtyHandle};
use crate::state::AppState;
use crate::utils::error::AppError;
use crate::utils::pane_id::parse_pane_id;
use crate::utils::pty_log;

#[tauri::command]
pub async fn create_pane(
    state: State<'_, AppState>,
    pane_id: String,
    shell: Option<String>,
) -> Result<(), String> {
    create_pane_inner(state, pane_id, shell).map_err(|e| e.to_string())
}

fn create_pane_inner(
    state: State<'_, AppState>,
    pane_id: String,
    shell: Option<String>,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let workspace_id = state.active_workspace_id();
    ensure_pane_pty_workspace(&*state, workspace_id, pane_id, shell)
}

/// 确保指定 workspace/pane 存在 PTY（已存在则跳过，幂等）。
/// teammate split 路径可直接复用，避免依赖前端 Pane 挂载后才创建。
pub fn ensure_pane_pty_workspace(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    shell: Option<String>,
) -> Result<(), AppError> {
    {
        let map = state.workspaces.read();
        let ws = map
            .get(&workspace_id)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        if ws.terminals.contains_key(&pane_id) {
            pty_log::create_skip(workspace_id, pane_id);
            return Ok(());
        }
    }

    let pty_system = native_pty_system();
    let mut cmd = if let Some(s) = shell {
        CommandBuilder::new(s)
    } else {
        #[cfg(target_os = "windows")]
        {
            let mut c = CommandBuilder::new("powershell.exe");
            c.arg("-NoLogo");
            c
        }
        #[cfg(not(target_os = "windows"))]
        {
            CommandBuilder::new("zsh")
        }
    };
    cmd.env("TERM", "xterm-256color");
    if let Some(ref bind) = *state.teammate_binding.read() {
        cmd.env("WIND_TEAMMATE_URL", bind.base_url.as_str());
        cmd.env("WIND_TEAMMATE_TOKEN", bind.token.as_str());
        cmd.env("WIND_TERMINAL", "1");
        // Claude Code `teammateMode: auto` 依赖「已在 tmux 中」；非空 TMUX 即视为 multiplexer 会话。
        // 值格式与 tmux 类似（socket 占位 + ,session,pane），Wind 不解析该路径。
        cmd.env("TMUX", "/wind/teammate.sock,0,0");
        // 某些 tmux 客户端逻辑会优先读取 TMUX_PANE 再回退 display-message 查询。
        cmd.env("TMUX_PANE", "%0");
    }

    let pair = pty_system
        .openpty(PtySize {
            rows: 80,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| AppError::PtyError(e.to_string()))?;

    let master = pair.master;
    let reader = master
        .try_clone_reader()
        .map_err(|e| AppError::PtyError(e.to_string()))?;
    let writer = master
        .take_writer()
        .map_err(|e| AppError::PtyError(e.to_string()))?;

    let master = Arc::new(Mutex::new(master));
    let writer = Arc::new(Mutex::new(writer));

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| AppError::PtyError(e.to_string()))?;

    let mut handle = PtyHandle {
        master,
        writer,
        _child: child,
    };

    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&workspace_id)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        if ws.terminals.contains_key(&pane_id) {
            pty_log::create_skip(workspace_id, pane_id);
            let _ = handle._child.kill();
            return Ok(());
        }
        ws.terminals.insert(pane_id, handle);
    }

    pty_log::create_spawned(workspace_id, pane_id);
    let st = state.clone();
    spawn_pty_reader(st, workspace_id, pane_id, reader);
    Ok(())
}

#[tauri::command]
pub async fn write_to_pty(
    state: State<'_, AppState>,
    pane_id: String,
    data: String,
) -> Result<(), String> {
    write_to_pty_inner(state, pane_id, data).map_err(|e| e.to_string())
}

fn write_to_pty_inner(
    state: State<'_, AppState>,
    pane_id: String,
    data: String,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let wid = state.active_workspace_id();
    let map = state.workspaces.read();
    let ws = map
        .get(&wid)
        .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
    if let Some(handle) = ws.terminals.get(&pane_id) {
        let mut w = handle.writer.lock();
        w.write_all(data.as_bytes())?;
        w.flush()?;
        Ok(())
    } else {
        pty_log::pane_not_found("write", wid, pane_id);
        Err(AppError::PaneNotFound(pane_id))
    }
}

#[tauri::command]
pub async fn resize_pane(
    state: State<'_, AppState>,
    pane_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    resize_pane_inner(state, pane_id, rows, cols).map_err(|e| e.to_string())
}

fn resize_pane_inner(
    state: State<'_, AppState>,
    pane_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    // ConPTY / portable-pty: zero or absurd dimensions can break the session.
    let rows = rows.max(1);
    let cols = cols.max(1);
    let wid = state.active_workspace_id();
    let map = state.workspaces.read();
    let ws = map
        .get(&wid)
        .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
    if let Some(handle) = ws.terminals.get(&pane_id) {
        let master = handle.master.lock();
        let r = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        match r {
            Ok(()) => {
                pty_log::resize_ok(wid, pane_id, rows, cols);
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                pty_log::resize_err(wid, pane_id, rows, cols, &msg);
                Err(AppError::PtyError(msg))
            }
        }
    } else {
        pty_log::pane_not_found("resize", wid, pane_id);
        Err(AppError::PaneNotFound(pane_id))
    }
}

/// 在指定工作区内移除并结束 PTY（若存在）。
pub async fn kill_pty_if_present(state: &AppState, workspace_id: Uuid, pane_id: Uuid) {
    state.clear_pty_scrollback(workspace_id, pane_id);
    let handle = {
        let mut map = state.workspaces.write();
        map.get_mut(&workspace_id)
            .and_then(|ws| ws.terminals.remove(&pane_id))
    };
    if let Some(mut handle) = handle {
        let _ = handle.writer.lock().write_all(b"exit\n");
        let _ = handle._child.kill();
        let _ = state
            .event_tx
            .send(crate::types::GlobalEvent::PaneClosed {
                workspace_id,
                pane_id,
            })
            .await;
    }
}

#[tauri::command]
pub async fn kill_pane(state: State<'_, AppState>, pane_id: String) -> Result<(), String> {
    kill_pane_inner(state, pane_id).await.map_err(|e| e.to_string())
}

/// 供 teammate HTTP 面向指定 workspace 写字节（不依赖当前 active 以外的逻辑）。
pub fn write_pty_bytes_workspace(
    app: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    data: &[u8],
) -> Result<(), AppError> {
    let map = app.workspaces.read();
    let ws = map
        .get(&workspace_id)
        .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
    let handle = ws
        .terminals
        .get(&pane_id)
        .ok_or(AppError::PaneNotFound(pane_id))?;
    let mut w = handle.writer.lock();
    w.write_all(data)?;
    w.flush()?;
    Ok(())
}

async fn kill_pane_inner(state: State<'_, AppState>, pane_id: String) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let wid = state.active_workspace_id();
    let exists = {
        let map = state.workspaces.read();
        map.get(&wid)
            .map(|ws| ws.terminals.contains_key(&pane_id))
            .unwrap_or(false)
    };
    if !exists {
        return Err(AppError::PaneNotFound(pane_id));
    }
    kill_pty_if_present(&*state, wid, pane_id).await;
    Ok(())
}
