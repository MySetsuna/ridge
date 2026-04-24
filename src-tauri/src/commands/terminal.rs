use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

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

	// 优先使用 pane tree 中已记录的 CWD（分屏时由 split_pane 从父 pane 继承），
	// 若已保存过 shell_kind（来自 .wind 文件恢复）也一并取出。
	let (cwd, persisted_shell): (PathBuf, Option<String>) = {
		let map = state.workspaces.read();
		let entry = map.get(&workspace_id).and_then(|ws| ws.pane_tree.panes.get(&pane_id));
		let cwd = entry.and_then(|p| p.cwd.clone());
		let sk = entry.and_then(|p| p.shell_kind.clone());
		(
			cwd.or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
				.or_else(|| std::env::current_dir().ok())
				.unwrap_or_else(|| PathBuf::from(".")),
			sk,
		)
	};

	// 调用方传 shell 时以调用方为准；否则使用 pane 上持久化的 shell_kind（.wind 恢复路径）。
	let effective_shell = shell.clone().or(persisted_shell);

	// 持久化本次实际使用的 shell 信息，便于后续 .wind 保存。
	if let Some(ref sk) = effective_shell {
		let mut map = state.workspaces.write();
		if let Some(ws) = map.get_mut(&workspace_id) {
			if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
				pane.shell_kind = Some(sk.clone());
			}
		}
	}

	ensure_pane_pty_workspace(
		&*state,
		workspace_id,
		pane_id,
		effective_shell,
		Some(&cwd),
		None,
		None,
		None,
	)?;

	// 设置 pane 的工作目录用于 git diff 跟踪
	crate::commands::git::set_pane_workdir(pane_id.to_string(), cwd.to_string_lossy().to_string()).map_err(AppError::PtyError)?;

	// 立即通知前端初始 CWD，无需等待 shell 发出 OSC 7。统一路径分隔符，
	// 与 OSC 7 / 轮询路径的规范化保持一致。
	let cwd_canon = {
		let s = cwd.to_string_lossy().to_string();
		#[cfg(windows)]
		{
			s.replace('\\', "/")
		}
		#[cfg(not(windows))]
		{
			s
		}
	};
	let _ = state.event_tx.try_send(crate::types::GlobalEvent::PaneCwdChanged {
		workspace_id,
		pane_id,
		cwd: cwd_canon,
	});

	Ok(())
}

#[derive(Clone, Debug)]
pub struct StructuredPtyCommand {
	pub program: String,
	pub args: Vec<String>,
	pub env: HashMap<String, String>,
}

/// Claude Code shells out to `tmux`, while Cargo places `tmux(.exe)` beside the main binary.
/// Returns the shim directory so callers can re-enforce it after applying extra env vars that
/// might otherwise overwrite PATH (e.g. structured-launch env from Claude Code).
fn prepend_path_with_wind_tmux_shim(cmd: &mut CommandBuilder) -> Option<PathBuf> {
	let tmux_name = if cfg!(windows) { "tmux.exe" } else { "tmux" };

	// Dev builds: use the pre-built shim in dist/teammate-shim/ under the workspace root.
	// current_exe() = …/src-tauri/target/debug/wind.exe → go up 4 levels to workspace root.
	#[cfg(debug_assertions)]
	let shim_dir = {
		let exe = std::env::current_exe().ok()?;
		let workspace = exe
			.parent()
			.and_then(|p| p.parent())
			.and_then(|p| p.parent())
			.and_then(|p| p.parent())?;
		workspace.join("dist").join("teammate-shim")
	};

	// Release builds: look for tmux(.exe) beside the installed Wind binary.
	#[cfg(not(debug_assertions))]
	let shim_dir = {
		let exe = std::env::current_exe().ok()?;
		let dir = exe.parent()?;
		let tmux = dir.join(tmux_name);
		if !tmux.is_file() {
			return None;
		}
		dir.to_path_buf()
	};

	if !shim_dir.join(tmux_name).is_file() {
		eprintln!("[wind] tmux shim not found at {}", shim_dir.display());
		return None;
	}
	let sep = if cfg!(windows) { ';' } else { ':' };
	let path = std::env::var("PATH").unwrap_or_default();
	cmd.env("PATH", format!("{}{sep}{path}", shim_dir.display()));
	Some(shim_dir)
}

/// tmux `TMUX` is `socket_path,session_index,pane_index`. Wind uses a sentinel path (no real socket).
/// Claude Code's TmuxBackend on Windows may validate the first segment as a Windows path; `/wind/...`
/// fails that check — use `{cwd|project|pwd|~/wind}/teammate.sock` with `/` separators.
fn tmux_env_value(pane_slot: usize, cwd: Option<&Path>, state: &AppState) -> String {
	#[cfg(windows)]
	{
		let base = cwd
			.map(Path::to_path_buf)
			.or_else(|| state.current_project.read().clone())
			.or_else(|| std::env::current_dir().ok())
			.or_else(|| dirs::home_dir().map(|h| h.join("wind")))
			.unwrap_or_else(|| PathBuf::from(r"C:\wind"));
		let sock = base.join("teammate.sock");
		let prefix = sock.to_string_lossy().replace('\\', "/");
		format!("{prefix},0,{pane_slot}")
	}
	#[cfg(not(windows))]
	{
		let _ = (cwd, state);
		format!("/wind/teammate.sock,0,{pane_slot}")
	}
}

/// 拆掉已有 PTY（不发 `PaneClosed` 全局事件，避免前端 `recoverPtySession` 与 teammate 重起打架）。
fn teardown_pane_pty_if_present(state: &AppState, workspace_id: Uuid, pane_id: Uuid) {
	let handle = {
		let mut map = state.workspaces.write();
		map.get_mut(&workspace_id)
			.and_then(|ws| ws.terminals.remove(&pane_id))
	};
	if handle.is_some() {
		pty_log::teammate_replace_pty(workspace_id, pane_id);
	}
	if let Some(mut handle) = handle {
		let _ = handle._child.kill();
	}
	state.clear_pty_scrollback(workspace_id, pane_id);
}

/// 确保指定 workspace/pane 存在 PTY（已存在则跳过，幂等）。
/// teammate split 路径可直接复用，避免依赖前端 Pane 挂载后才创建。
///
/// `initial_command`：Windows 上类 Unix 一行经 PowerShell `-EncodedCommand` 转交 `cmd /c`；Unix 用 `/bin/bash -c` 或 `sh -c`。
/// `tmux_pane_index`：teammate 子窗格与 `TMUX_PANE` / `TMUX` 尾缀对齐。
///
/// 若带 `initial_command` 时该 pane 已有 PTY（常见：前端 `Pane` onMount 先 `create_pane`），会先拆掉再按命令重起，避免误走 `create_skip`。
pub fn ensure_pane_pty_workspace(
	state: &AppState,
	workspace_id: Uuid,
	pane_id: Uuid,
	shell: Option<String>,
	cwd: Option<&Path>,
	initial_command: Option<&str>,
	structured_command: Option<StructuredPtyCommand>,
	tmux_pane_index: Option<usize>,
) -> Result<(), AppError> {
	let ic = initial_command.map(str::trim).filter(|s| !s.is_empty());
	let sc = structured_command
		.map(|s| StructuredPtyCommand {
			program: s.program.trim().to_string(),
			args: s.args,
			env: s.env,
		})
		.filter(|s| !s.program.is_empty());
	let has_explicit_launch = ic.is_some() || sc.is_some();

	{
		let map = state.workspaces.read();
		let ws = map
			.get(&workspace_id)
			.ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
		if ws.terminals.contains_key(&pane_id) {
			if has_explicit_launch {
				drop(map);
				teardown_pane_pty_if_present(state, workspace_id, pane_id);
			} else {
				pty_log::create_skip(workspace_id, pane_id);
				return Ok(());
			}
		}
	}

	let pty_system = native_pty_system();
	let mut cmd = if let Some(s) = shell {
		CommandBuilder::new(s)
	} else if let Some(spec) = sc.as_ref() {
		let mut c = CommandBuilder::new(&spec.program);
		for a in &spec.args {
			c.arg(a);
		}
		c
	} else if let Some(line) = ic {
		#[cfg(windows)]
		{
			let mut c = CommandBuilder::new("cmd.exe");
			c.arg("/d");
			c.arg("/s");
			c.arg("/c");
			c.arg(line);
			c
		}
		#[cfg(not(windows))]
		{
			let mut c = if Path::new("/bin/bash").is_file() {
				CommandBuilder::new("/bin/bash")
			} else {
				CommandBuilder::new("/bin/sh")
			};
			c.arg("-c");
			c.arg(line);
			c
		}
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
	let shim_dir = if let Some(ref bind) = *state.teammate_binding.read() {
		let shim_dir = prepend_path_with_wind_tmux_shim(&mut cmd);
		cmd.env("WIND_TEAMMATE_URL", bind.base_url.as_str());
		cmd.env("WIND_TEAMMATE_TOKEN", bind.token.as_str());
		cmd.env("WIND_TERMINAL", "1");
		// Claude Code `teammateMode: auto` 依赖「已在 tmux 中」；非空 TMUX 即视为 multiplexer 会话。
		let pane_slot = tmux_pane_index.unwrap_or(0);
		cmd.env("TMUX", tmux_env_value(pane_slot, cwd, state));
		// Numeric only: see comment on cmd/batch `%0` expansion when forwarding env.
		cmd.env("TMUX_PANE", format!("{pane_slot}"));
		let log_path = std::env::var("WIND_TMUX_LOG")
			.ok()
			.filter(|s| !s.trim().is_empty());
		if let Some(ref log) = log_path {
			cmd.env("WIND_TMUX_LOG", log.as_str());
		}
		shim_dir
	} else {
		None
	};
	if let Some(spec) = sc.as_ref() {
		for (k, v) in &spec.env {
			// Re-enforce shim PATH if spec overwrites it — prevents `tmux` from being lost
			// in the sub-agent's shell when Claude Code passes its own PATH in the env.
			if k.eq_ignore_ascii_case("PATH") {
				if let Some(ref dir) = shim_dir {
					let sep = if cfg!(windows) { ';' } else { ':' };
					cmd.env("PATH", format!("{}{sep}{v}", dir.display()));
					continue;
				}
			}
			cmd.env(k, v);
		}
	}
	if let Some(dir) = cwd {
		cmd.cwd(dir);
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
            ws.pane_sizes.insert(pane_id, (80, 120));
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

fn resize_pane_inner(state: State<'_, AppState>, pane_id: String, rows: u16, cols: u16,) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    // ConPTY / portable-pty: zero or absurd dimensions can break the session.
// 限制尺寸在合理范围内，防止极端尺寸导致 session 中断
	const MAX_SAFE_ROWS: u16 = 500;
	const MAX_SAFE_COLS: u16 = 500;
    let rows = rows.max(1).min(MAX_SAFE_ROWS);
    let cols = cols.max(1).min(MAX_SAFE_COLS);
    let wid = state.active_workspace_id();

    // Perform the resize within a limited scope so we can drop the read lock
    let resize_result: Result<(), AppError> = {
        let map = state.workspaces.read();
        let ws = map
            .get(&wid)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        if let Some(handle) = ws.terminals.get(&pane_id) {
            let master = handle.master.lock();
            master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }).map_err(|e| {
                let msg = e.to_string();
                pty_log::resize_err(wid, pane_id, rows, cols, &msg);
                AppError::PtyError(msg)
            })
        } else {
            pty_log::pane_not_found("resize", wid, pane_id);
            Err(AppError::PaneNotFound(pane_id))
        }
    };

    match resize_result {
        Ok(()) => {
            pty_log::resize_ok(wid, pane_id, rows, cols);
            // Now we can safely acquire a write lock to update pane_sizes
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                ws.pane_sizes.insert(pane_id, (rows, cols));
            }
            Ok(())
        }
        Err(e) => {
			// 记录错误但返回成功，避免错误传播导致 session 中断
			pty_log::resize_err(wid, pane_id, rows, cols, &e.to_string());
			Ok(())
		}
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

#[tauri::command]
pub fn get_pane_scrollback(
	state: State<'_, AppState>,
	pane_id: String,
) -> Result<String, String> {
	let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
	let workspace_id = state.active_workspace_id();
	let map = state.pty_scrollback.read();
	Ok(map.get(&(workspace_id, pane_id)).cloned().unwrap_or_default())
}