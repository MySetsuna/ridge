//! 其余 tmux 子命令占位实现（无 teammate 映射时返回成功）。

use crate::shim_log;
pub(crate) fn cmd_display_menu(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-T" | "-M" | "-O" | "-x" | "-y" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_confirm_before(rest: &[String]) -> Result<(), ()> {
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
    // Just run the command without confirmation
    Ok(())
}

pub(crate) fn cmd_command_prompt(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-p" | "-I" | "-O" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_if_shell(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" => {} // background
            "-C" => {} // continue on error
            "-F" => {} // format
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_run_shell(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" => {} // background
            "-d" => {} // don't display output
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_source_file(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-n" => {} // don't execute commands
            _ => {}
        }
        i += 1;
    }
    // Source file - just acknowledge
    Ok(())
}

pub(crate) fn cmd_set_option(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-g" | "-u" | "-w" | "-s" | "-a" => {}
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-F" | "-o" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_show_options(rest: &[String]) -> Result<(), ()> {
    let mut format: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-g" | "-w" | "-s" => {}
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-F" if i + 1 < rest.len() => {
                format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-v" => {} // show only values
            _ => {}
        }
        i += 1;
    }

    if let Some(fmt) = format {
        shim_log::out_line(&fmt);
        return Ok(());
    }
    Ok(())
}

pub(crate) fn cmd_bind_key(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-n" | "-r" | "-N" | "-M" => {}
            "-T" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_unbind_key(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-a" | "-n" | "-M" => {}
            "-T" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_wait_for(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-S" | "-L" => {} // signal / lock
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_server_access(rest: &[String]) -> Result<(), ()> {
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

pub(crate) fn cmd_copy_mode(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-e" | "-M" | "-N" | "-U" => {} // flags
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_paste_buffer(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-b" | "-d" | "-r" | "-s" if i + 1 < rest.len() => {
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_choose_tree(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-Z" | "-F" | "-O" | "-s" | "-N" | "-W" | "-w" => {}
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn cmd_find_window(rest: &[String]) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                i += 1;
            }
            "-C" | "-i" | "-N" | "-T" if i + 1 < rest.len() => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}
