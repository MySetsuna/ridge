//! 会话管理子命令。

use chrono::Local;

use crate::format::{render_tmux_format_ex, TmuxFormatContext};
use crate::http::fetch_pane_layout;
use crate::io::post_split;

pub(crate) fn cmd_new_session(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut session_name: Option<String> = None;
    let mut detached = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-n" if i + 1 < rest.len() => {
                i += 1;
            }
            "-d" => detached = true,
            "-s" if i + 1 < rest.len() => {
                session_name = Some(rest[i + 1].to_string());
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                i += 1;
            }
            "-t" | "-T" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {
                // Command to run
                break;
            }
        }
        i += 1;
    }
    let _ = (session_name, detached);

    // tmux new-session creates window 0 with one pane by default.
    // We need to create at least one pane to match tmux semantics.
    // The split creates a new pane (pane 1) and returns success.
    // Claude Code will use this as the working pane for the team session.
    post_split(url, token, false, None, None, None, None, None)
}

pub(crate) fn cmd_has_session(rest: &[String]) -> Result<(), ()> {
    let mut target: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                let t = rest[i + 1].trim();
                if !t.is_empty() {
                    target = Some(t);
                }
                i += 1;
            }
            s if !s.starts_with('-') => {
                target = Some(s.trim());
            }
            _ => {}
        }
        i += 1;
    }
    let _ = target;
    // 单会话：与 `list-sessions` 首列 `0:` 及名称 `wind` 一致。
    Ok(())
}

/// 与 tmux 默认 `list-sessions` 对齐，供 Claude Code TmuxBackend 解析当前会话/附着状态。
/// Wind 注入的 `TMUX` 为 `{…}/teammate.sock,0,{pane}`（Windows 上首段为盘符路径）；中段为会话索引 `0`，故默认首列为 `0:`。
pub(crate) fn cmd_list_sessions(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut output_format: Option<String> = None;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-F" if i + 1 < rest.len() => {
                output_format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-f" => {} // session filter; Wind 仅单会话
            _ => {}
        }
        i += 1;
    }

    if let Some(fmt) = output_format {
        let (active_idx, pc, ctx) = match fetch_pane_layout(url, token) {
            Ok(l) => (
                l.active_index,
                l.pane_count.max(1),
                TmuxFormatContext::from_list_panes(&l),
            ),
            Err(()) => (0, 1, TmuxFormatContext::default()),
        };
        println!("{}", render_tmux_format_ex(&fmt, 0, active_idx, pc, &ctx));
        return Ok(());
    }

    let created = Local::now().format("%a %b %d %H:%M:%S %Y");
    println!("0: 1 windows (created {created}) [120x80] (attached)");
    Ok(())
}

pub(crate) fn cmd_attach_session(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-d" => {} // detach other clients
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_detach_client(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-P" | "-E" => {} // attachment flags
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_kill_session(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-a" => {} // kill all but current
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_kill_server() -> Result<(), ()> {
    Ok(())
}

pub(crate) fn cmd_switch_client(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_rename_session(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {
                // New name
                break;
            }
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_lock_server() -> Result<(), ()> {
    // Lock server - not supported
    Ok(())
}

pub(crate) fn cmd_start_server() -> Result<(), ()> {
    // Server is always running
    Ok(())
}