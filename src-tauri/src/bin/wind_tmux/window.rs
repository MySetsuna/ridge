//! 窗口管理子命令。

use crate::io::post_new_window;

pub(crate) fn cmd_new_window(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut command: Option<String> = None;
    let mut window_name: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-n" if i + 1 < rest.len() => {
                window_name = Some(rest[i + 1].to_string());
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                cwd = Some(rest[i + 1].to_string());
                i += 1;
            }
            "-d" => {} // don't make it the active window
            "-a" => {} // after index
            "-t" if i + 1 < rest.len() => {
                // Target window index
                i += 1;
            }
            _ => {
                // Command to run
                command = Some(rest[i..].join(" "));
                break;
            }
        }
        i += 1;
    }

    post_new_window(url, token, command, cwd, window_name)
}

pub(crate) fn cmd_select_window(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut window_index: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                window_index = Some(&rest[i + 1]);
                i += 1;
            }
            "-l" => {} // last window
            "-n" => {} // next window
            "-p" => {} // previous window
            _ => {}
        }
        i += 1;
    }
    let _ = window_index;
    Ok(())
}

pub(crate) fn cmd_kill_window(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut window_index: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                window_index = Some(&rest[i + 1]);
                i += 1;
            }
            "-a" => {} // kill all but the current
            "-w" => {} // kill all windows
            _ => {}
        }
        i += 1;
    }
    let _ = window_index;
    Ok(())
}

pub(crate) fn cmd_rename_window(rest: &[String]) -> Result<(), ()> {
    let mut window_index: Option<&str> = None;
    let mut new_name: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                window_index = Some(&rest[i + 1]);
                i += 1;
            }
            _ => {
                // New name
                new_name = Some(rest[i..].join(" "));
                break;
            }
        }
        i += 1;
    }
    let _ = (window_index, new_name);
    Ok(())
}

pub(crate) fn cmd_move_window(rest: &[String]) -> Result<(), ()> {
    let mut source_index: Option<&str> = None;
    let mut dest_index: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-s" if i + 1 < rest.len() => {
                source_index = Some(&rest[i + 1]);
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                dest_index = Some(&rest[i + 1]);
                i += 1;
            }
            "-r" => {} // renumber all windows
            _ => {}
        }
        i += 1;
    }
    let _ = (source_index, dest_index);
    Ok(())
}

pub(crate) fn cmd_rotate_window(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-U" | "-D" => {} // direction
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_select_layout(rest: &[String]) -> Result<(), ()> {
    let mut window_index: Option<&str> = None;
    let mut layout: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                window_index = Some(&rest[i + 1]);
                i += 1;
            }
            "-n" => {} // next layout
            "-p" => {} // previous layout
            _ => {
                layout = Some(rest[i].to_string());
            }
        }
        i += 1;
    }
    let _ = (window_index, layout);
    Ok(())
}

pub(crate) fn cmd_respawn_window(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-k" => {} // kill existing
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_next_window(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_previous_window(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_last_window(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}