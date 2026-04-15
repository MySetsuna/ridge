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

	// 获取工作目录（优先使用环境变量 HOME 或当前目录）
	let cwd = std::env::var("HOME")
		.or_else(|_| std::env::current_dir().map(|p| p.to_string_lossy().to_string()))
		.unwrap_or_else(|_| ".".to_string());

	// 创建 pane 后自动设置工作目录
	ensure_pane_pty_workspace(
		&*state,
		workspace_id,
		pane_id,
		shell,
		None,
		None,
		None,
	)?;

	// 设置 pane 的工作目录用于 git diff 跟踪
	crate::commands::git::set_pane_workdir(pane_id.to_string(), cwd).map_err(AppError::PtyError)?;

	Ok(())
}

/// Claude agent-teams 常见一行：`cd ... && env ...`（类 sh）；Windows 上由 PowerShell 包一层再交给 `cmd /c`。
fn looks_like_unix_one_liner(cmd: &str) -> bool {
	let t = cmd.trim();
	t.contains(" env ")
		|| t.starts_with("env ")
		|| (t.contains("&&") && (t.contains("CLAUDE") || t.contains("ANTHROPIC")))
}

fn base64_standard(data: &[u8]) -> String {
	const CHARS: &[u8; 64] =
		b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
	let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
	let mut i = 0;
	while i + 3 <= data.len() {
		let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
		out.push(CHARS[((n >> 18) & 63) as usize] as char);
		out.push(CHARS[((n >> 12) & 63) as usize] as char);
		out.push(CHARS[((n >> 6) & 63) as usize] as char);
		out.push(CHARS[(n & 63) as usize] as char);
		i += 3;
	}
	let rem = data.len() - i;
	if rem == 1 {
		let n = (data[i] as u32) << 16;
		out.push(CHARS[((n >> 18) & 63) as usize] as char);
		out.push(CHARS[((n >> 12) & 63) as usize] as char);
		out.push('=');
		out.push('=');
	} else if rem == 2 {
		let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
		out.push(CHARS[((n >> 18) & 63) as usize] as char);
		out.push(CHARS[((n >> 12) & 63) as usize] as char);
		out.push(CHARS[((n >> 6) & 63) as usize] as char);
		out.push('=');
	}
	out
}

/// `powershell.exe -EncodedCommand` 要求：UTF-16LE 字节再 Base64。
fn powershell_encoded_command_utf16le(script: &str) -> String {
	let utf16: Vec<u16> = script.encode_utf16().collect();
	let mut bytes = Vec::with_capacity(utf16.len() * 2);
	for u in utf16 {
		bytes.extend_from_slice(&u.to_le_bytes());
	}
	base64_standard(&bytes)
}

/// Windows：用内置 PowerShell 解码 UTF-8 命令后 `cmd /c` 执行，避免把类 Unix 一行直接塞进交互式 PS。
fn windows_powershell_cmd_c_for_line(line: &str) -> CommandBuilder {
	let inner_b64 = base64_standard(line.as_bytes());
	let ps = format!(
		r#"$b=[System.Convert]::FromBase64String('{inner_b64}');$s=[System.Text.Encoding]::UTF8.GetString($b);$x=[System.IO.Path]::Combine($env:windir,'System32','cmd.exe');$a='/c '+$s;$p=Start-Process -FilePath $x -ArgumentList $a -NoNewWindow -PassThru -Wait;exit $p.ExitCode"#,
	);
	let enc = powershell_encoded_command_utf16le(&ps);
	let mut c = CommandBuilder::new("powershell.exe");
	c.arg("-NoLogo");
	c.arg("-NoProfile");
	c.arg("-EncodedCommand");
	c.arg(enc);
	c
}

/// Claude Code shells out to `tmux`, while Cargo places `wind-tmux(.exe)` beside the main binary.
fn prepend_path_with_wind_tmux_shim(cmd: &mut CommandBuilder) {
	let Ok(exe) = std::env::current_exe() else {
		return;
	};
	let Some(dir) = exe.parent() else {
		return;
	};
	let wind_tmux = dir.join(if cfg!(windows) {
		"wind-tmux.exe"
	} else {
		"wind-tmux"
	});
	let tmux = dir.join(if cfg!(windows) { "tmux.exe" } else { "tmux" });
	if !wind_tmux.is_file() {
		return;
	}
	if !tmux.is_file() {
		if std::fs::hard_link(&wind_tmux, &tmux).is_err() {
			#[cfg(unix)]
			{
				let _ = std::os::unix::fs::symlink(&wind_tmux, &tmux);
			}
		}
	}
	if !tmux.is_file() {
		return;
	}
	let sep = if cfg!(windows) { ';' } else { ':' };
	let path = std::env::var("PATH").unwrap_or_default();
	let new_path = format!("{}{}{}", dir.display(), sep, path);
	cmd.env("PATH", new_path);
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
	tmux_pane_index: Option<usize>,
) -> Result<(), AppError> {
	let ic = initial_command.map(str::trim).filter(|s| !s.is_empty());

	{
		let map = state.workspaces.read();
		let ws = map
			.get(&workspace_id)
			.ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
		if ws.terminals.contains_key(&pane_id) {
			if ic.is_some() {
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
	} else if let Some(line) = ic {
		#[cfg(windows)]
		{
			if looks_like_unix_one_liner(line) {
				windows_powershell_cmd_c_for_line(line)
			} else {
				let mut c = CommandBuilder::new("cmd.exe");
				c.arg("/c");
				c.arg(line);
				c
			}
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
	if let Some(ref bind) = *state.teammate_binding.read() {
		prepend_path_with_wind_tmux_shim(&mut cmd);
		cmd.env("WIND_TEAMMATE_URL", bind.base_url.as_str());
		cmd.env("WIND_TEAMMATE_TOKEN", bind.token.as_str());
		cmd.env("WIND_TERMINAL", "1");
		// Claude Code `teammateMode: auto` 依赖「已在 tmux 中」；非空 TMUX 即视为 multiplexer 会话。
		let pane_slot = tmux_pane_index.unwrap_or(0);
		cmd.env("TMUX", tmux_env_value(pane_slot, cwd, state));
		// Numeric only: see comment on cmd/batch `%0` expansion when forwarding env.
		cmd.env("TMUX_PANE", format!("{pane_slot}"));
		// let log_path = std::env::var("WIND_TMUX_LOG")
		let log_path = Some(("D:/novel/wind-tmux.log".to_string()))
			.ok()
			.filter(|s| !s.trim().is_empty())
			.or_else(|| {
				#[cfg(windows)]
				{
					let novel = std::path::Path::new(r"D:\novel");
					if novel.exists() || std::fs::create_dir_all(novel).is_ok() {
						Some(r"D:\novel\wind-tmux.log".to_string())
					} else {
						None
					}
				}
				#[cfg(not(windows))]
				{
					None
				}
			});
		if let Some(ref path) = log_path {
			cmd.env("WIND_TMUX_LOG", path.as_str());
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