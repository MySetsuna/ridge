//! 窗格管理：`select-pane`、`resize-pane` 等（部分为兼容占位）。

use crate::format::parse_pane_target;
use crate::http::{auth_headers, client};
use crate::shim_log;

pub(crate) fn cmd_select_pane(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane_index: Option<usize> = None;
    let mut direction: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                let raw = rest[i + 1].trim();
                if raw.is_empty() {
                    // `-t ""` 常见于与 `-P`/`-T` 等组合，不改变当前窗格索引。
                    pane_index = None;
                } else {
                    pane_index = Some(parse_pane_target(raw));
                }
                i += 1;
            }
            "-L" => direction = Some("left"),
            "-R" => direction = Some("right"),
            "-U" => direction = Some("up"),
            "-D" => direction = Some("down"),
            "-l" => direction = Some("last"),
            "-n" => direction = Some("next"),
            "-p" => direction = Some("previous"),
            "-T" if i + 1 < rest.len() => {
                // Set pane title - just acknowledge
                i += 1;
            }
            "-P" if i + 1 < rest.len() => {
                // Set window style - just acknowledge
                i += 1;
            }
            "-g" => {} // get (show) style
            "-e" | "-d" => {} // enable/disable input
            "-Z" => {} // zoom
            _ => {}
        }
        i += 1;
    }

    // If direction is specified (like -L, -R, -U, -D), we need special handling
    if let Some(_dir) = direction {
        // For left/right/up/down, we need to calculate the target pane
        // For now, just acknowledge the command
        return Ok(());
    }

    let u = format!("{}/api/v1/select-pane", url.trim_end_matches('/'));
    let body = match pane_index {
        Some(idx) => serde_json::json!({ "pane_index": idx }),
        None => serde_json::json!({}),
    };
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| {
            shim_log::err(&format!("select-pane request: {e}"));
            ()
        })?;
    if !res.status().is_success() {
        // Don't fail - just acknowledge for compatibility
    }
    Ok(())
}

pub(crate) fn cmd_kill_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut pane_index: Option<usize> = None;
    let mut kill_all = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-a" => kill_all = true,
            _ => {}
        }
        i += 1;
    }

    // Just acknowledge - the actual kill will happen via the PTY exit
    // In Wind, when a pane's PTY exits, the pane is automatically cleaned up
    let _ = (pane_index, kill_all);
    Ok(())
}

pub(crate) fn cmd_resize_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut pane_index: Option<usize> = None;
    let mut direction: Option<&str> = None;
    let mut adjustment: i32 = 1;
    let mut target_width: Option<usize> = None;
    let mut target_height: Option<usize> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-L" => direction = Some("left"),
            "-R" => direction = Some("right"),
            "-U" => direction = Some("up"),
            "-D" => direction = Some("down"),
            "-M" => {} // mouse drag
            "-T" => {} // trim history
            "-Z" => {} // zoom
            "-x" if i + 1 < rest.len() => {
                target_width = rest[i + 1].parse().ok();
                i += 1;
            }
            "-y" if i + 1 < rest.len() => {
                target_height = rest[i + 1].parse().ok();
                i += 1;
            }
            _ => {
                // Could be adjustment number
                if let Ok(adj) = rest[i].parse::<i32>() {
                    adjustment = adj;
                }
            }
        }
        i += 1;
    }

    let _ = (pane_index, direction, adjustment, target_width, target_height);
    Ok(())
}

pub(crate) fn cmd_last_pane(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut target_pane: Option<usize> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                let raw = rest[i + 1].trim();
                if !raw.is_empty() {
                    target_pane = Some(parse_pane_target(raw));
                }
                i += 1;
            }
            "-e" | "-d" => {} // enable/disable
            "-Z" => {} // zoom
            _ => {}
        }
        i += 1;
    }

    // If -t flag provided, send explicit pane_index. Otherwise send last:true for swap.
    let u = format!("{}/api/v1/select-pane", url.trim_end_matches('/'));
    let body = if let Some(idx) = target_pane {
        serde_json::json!({ "pane_index": idx })
    } else {
        serde_json::json!({ "last": true })
    };

    let _ = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send();

    // If server responded with pane info, extract it for TMUX_PANE update
    // (This helps keep TMUX_PANE in sync after a last-pane swap)
    Ok(())
}

pub(crate) fn cmd_swap_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut source_pane: Option<usize> = None;
    let mut dest_pane: Option<usize> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-s" if i + 1 < rest.len() => {
                source_pane = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                dest_pane = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-U" | "-D" => {} // swap with next/prev
            _ => {}
        }
        i += 1;
    }
    let _ = (source_pane, dest_pane);
    Ok(())
}

pub(crate) fn cmd_break_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                // Break specified pane into new window
                i += 1;
            }
            "-d" => {} // don't make it the active window
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_join_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut source_pane: Option<usize> = None;
    let mut target_window: Option<&str> = None;
    let mut _horizontal = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-s" if i + 1 < rest.len() => {
                source_pane = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                target_window = Some(rest[i + 1].as_str());
                i += 1;
            }
            "-h" => _horizontal = true,
            "-v" => {}
            "-l" | "-p" if i + 1 < rest.len() => {
                i += 1; // size
            }
            _ => {}
        }
        i += 1;
    }
    let _ = (source_pane, target_window);
    Ok(())
}

pub(crate) fn cmd_respawn_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut pane_index: Option<usize> = None;
    let mut command: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-k" => {} // kill existing
            _ => {
                // Command to run
                command = Some(rest[i..].join(" "));
                break;
            }
        }
        i += 1;
    }
    let _ = (pane_index, command);
    Ok(())
}

pub(crate) fn cmd_pipe_pane(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-o" => {} // only if not already piped
            _ => {
                // Command to pipe to
            }
        }
        i += 1;
    }
    // Pipe pane output - not supported in Wind
    Ok(())
}

pub(crate) fn cmd_display_panes(rest: &[String]) -> Result<(), ()> {
    // Display panes menu - show a message
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-d" => {} // don't display if only one pane
            _ => {}
        }
        i += 1;
    }
    // Just acknowledge
    Ok(())
}