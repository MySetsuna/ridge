//! 列表与剪贴板类子命令。

use crate::format::{pane_index_from_env, render_tmux_format_ex, TmuxFormatContext};
use crate::http::{fetch_list_windows_plain, fetch_pane_layout};

pub(crate) fn cmd_list_windows(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut format: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-F" if i + 1 < rest.len() => {
                format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                // Target session/window
                i += 1;
            }
            "-a" => {} // all sessions
            _ => {}
        }
        i += 1;
    }

    if let Some(fmt) = format {
        let (active_idx, pc, ctx) = match fetch_pane_layout(url, token) {
            Ok(l) => (
                l.active_index,
                l.pane_count.max(1),
                TmuxFormatContext::from_list_panes(&l),
            ),
            Err(()) => (
                pane_index_from_env().unwrap_or(0),
                1,
                TmuxFormatContext::default(),
            ),
        };
        println!(
            "{}",
            render_tmux_format_ex(&fmt, 0, active_idx, pc, &ctx)
        );
        return Ok(());
    }

    match fetch_list_windows_plain(url, token) {
        Ok(line) => println!("{line}"),
        Err(()) => println!("0: wind* (1 panes) [80x24] @0 (active)"),
    }
    Ok(())
}

pub(crate) fn cmd_list_clients(rest: &[String]) -> Result<(), ()> {
    let mut format: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-F" if i + 1 < rest.len() => {
                format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    if let Some(fmt) = format {
        // Format: #{client_tty}, #{client_session_name}, etc.
        // For now, just return nothing - no clients attached
        println!("{}", fmt.replace("#{client_tty}", "").replace("#{client_session_name}", "wind"));
        return Ok(());
    }

    // No clients attached
    Ok(())
}

pub(crate) fn cmd_list_keys(_rest: &[String]) -> Result<(), ()> {
    // List key bindings - not implemented
    Ok(())
}

pub(crate) fn cmd_list_commands(_rest: &[String]) -> Result<(), ()> {
    // List all tmux commands
    println!("\
split-window (splitw)\n\
select-pane (selectp)\n\
kill-pane (killp)\n\
resize-pane (resizep)\n\
send-keys (send)\n\
capture-pane (capturep)\n\
list-panes (lsp)\n\
list-windows (lsw)\n\
new-window (neww)\n\
list-sessions (ls)");
    Ok(())
}

pub(crate) fn cmd_list_buffers() -> Result<(), ()> {
    // No buffers
    Ok(())
}

// ========== Buffer Commands ==========

pub(crate) fn cmd_save_buffer(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" if i + 1 < rest.len() => {
                i += 1;
            }
            "-a" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_load_buffer(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_delete_buffer(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_set_buffer(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" if i + 1 < rest.len() => {
                i += 1;
            }
            "-n" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_show_buffer(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}
