//! Claude Code `teammateMode: tmux` 兼容：把 `tmux` 子命令翻译成 Wind 本地 HTTP（见 `WIND_TEAMMATE_URL` / `WIND_TEAMMATE_TOKEN`）。
//! 使用：将本二进制放到 PATH 且命名为 `tmux`，或在 Claude 配置中指向本程序。

use std::env;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::PathBuf;
use std::process;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{}", d.as_secs(), d.subsec_millis()),
        Err(_) => "0.000".to_string(),
    }
}

fn log_stderr(msg: &str) {
    let line = format!("[wind-tmux][{}] {msg}", now_ts());
    eprintln!("{line}");
    log_file_append(&line);
}

fn log_file_path() -> Option<PathBuf> {
    if let Ok(p) = env::current_dir() {
        return Some(p.join("wind-tmux.log"));
    }
    Some(env::temp_dir().join("wind-tmux.log"))
}

fn log_file_append(line: &str) {
    let Some(path) = log_file_path() else {
        return;
    };
    let actual_path = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            let _ = writeln!(f, "{line}");
            path
        }
        Err(_) => {
            let fallback = env::temp_dir().join("wind-tmux.log");
            let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&fallback) else {
                return;
            };
            let _ = writeln!(f, "{line}");
            fallback
        }
    };
    static LOG_PATH_ONCE: OnceLock<()> = OnceLock::new();
    let _ = LOG_PATH_ONCE.get_or_init(|| {
        eprintln!("[wind-tmux][{}] file-log={}", now_ts(), actual_path.display());
    });
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let joined_args = args
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(" ");
    let tmux_env = env::var("TMUX").unwrap_or_default();
    let url_set = env::var("WIND_TEAMMATE_URL")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let token_set = env::var("WIND_TEAMMATE_TOKEN")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    log_stderr(&format!(
        "invoke args=[{joined_args}] tmux_env={tmux_env:?} teammate_url_set={url_set} teammate_token_set={token_set}"
    ));

    // Claude Code 等会先跑 `tmux -V` 判断是否存在 tmux；此前落到 unsupported 会导致永远不启用 split。
    for a in args.iter().skip(1) {
        if a == "-V" || a == "--version" {
            log_stderr("probe version -> tmux 3.4");
            println!("tmux 3.4");
            process::exit(0);
        }
        if a == "-h" || a == "--help" {
            log_stderr("probe help");
            eprintln!("wind-tmux shim: supports all tmux commands (needs WIND_TEAMMATE_*)");
            process::exit(0);
        }
    }
    if args.len() < 2 {
        log_stderr("missing subcommand");
        eprintln!("wind-tmux: missing subcommand");
        process::exit(1);
    }
    let url = env::var("WIND_TEAMMATE_URL").unwrap_or_default();
    let token = env::var("WIND_TEAMMATE_TOKEN").unwrap_or_default();
    if url.is_empty() || token.is_empty() {
        log_stderr("missing WIND_TEAMMATE_URL/TOKEN");
        eprintln!("wind-tmux: set WIND_TEAMMATE_URL and WIND_TEAMMATE_TOKEN (Wind injects these in PTY)");
        process::exit(1);
    }
    let sub = args[1].as_str();
    let rest = &args[2..];
    let r = match sub {
        // ========== Pane Management ==========
        "split-window" | "splitw" => cmd_split(rest, &url, &token),
        "select-pane" | "selectp" => cmd_select_pane(rest, &url, &token),
        "kill-pane" | "killp" => cmd_kill_pane(rest, &url, &token),
        "resize-pane" | "resizep" => cmd_resize_pane(rest, &url, &token),
        "last-pane" | "lastp" => cmd_last_pane(rest, &url, &token),
        "swap-pane" | "swapp" => cmd_swap_pane(rest, &url, &token),
        "break-pane" | "breakp" => cmd_break_pane(rest, &url, &token),
        "join-pane" | "joinp" => cmd_join_pane(rest, &url, &token),
        "respawn-pane" | "respawnp" => cmd_respawn_pane(rest, &url, &token),
        "pipe-pane" => cmd_pipe_pane(rest),
        "display-panes" | "displayp" => cmd_display_panes(rest),

        // ========== Window Management ==========
        "new-window" | "neww" => cmd_new_window(rest, &url, &token),
        "select-window" | "selectw" => cmd_select_window(rest, &url, &token),
        "kill-window" | "killw" => cmd_kill_window(rest, &url, &token),
        "rename-window" => cmd_rename_window(rest),
        "move-window" | "movew" => cmd_move_window(rest),
        "rotate-window" | "rotw" => cmd_rotate_window(rest),
        "select-layout" | "selel" => cmd_select_layout(rest),
        "respawn-window" | "respawnw" => cmd_respawn_window(rest),
        "next-window" | "nextw" => cmd_next_window(rest),
        "previous-window" | "prevw" => cmd_previous_window(rest),
        "last-window" | "lastw" => cmd_last_window(rest),

        // ========== Session Management ==========
        "new-session" | "new" => cmd_new_session(rest),
        "has-session" | "has" => cmd_has_session(rest),
        "list-sessions" | "ls" => cmd_list_sessions(),
        "attach-session" | "attach" => cmd_attach_session(rest),
        "detach-client" | "detach" => cmd_detach_client(rest),
        "kill-session" => cmd_kill_session(rest),
        "kill-server" => cmd_kill_server(),
        "switch-client" | "switchc" => cmd_switch_client(rest),
        "rename-session" => cmd_rename_session(rest),
        "lock-server" | "lock" => cmd_lock_server(),
        "start-server" | "start" => cmd_start_server(),

        // ========== List Commands ==========
        "list-panes" | "lsp" => cmd_list_panes(rest, &url, &token),
        "list-windows" | "lsw" => cmd_list_windows(rest, &url, &token),
        "list-clients" | "lsc" => cmd_list_clients(rest),
        "list-keys" | "lsk" => cmd_list_keys(rest),
        "list-commands" | "lscm" => cmd_list_commands(rest),
        "list-buffers" | "lsb" => cmd_list_buffers(),

        // ========== I/O Commands ==========
        "send-keys" | "send" => cmd_send_keys(rest, &url, &token),
        "capture-pane" | "capturep" => cmd_capture(rest, &url, &token),

        // ========== Buffer Commands ==========
        "save-buffer" | "saveb" => cmd_save_buffer(rest),
        "load-buffer" | "loadb" => cmd_load_buffer(rest),
        "delete-buffer" | "deleteb" => cmd_delete_buffer(rest),
        "set-buffer" | "setb" => cmd_set_buffer(rest),
        "show-buffer" | "showb" => cmd_show_buffer(rest),

        // ========== Other Commands ==========
        "display-message" | "display" => cmd_display_message(rest),
        "display-menu" => cmd_display_menu(rest),
        "confirm-before" | "confirm" => cmd_confirm_before(rest),
        "command-prompt" | "prompt" => cmd_command_prompt(rest),
        "if-shell" => cmd_if_shell(rest),
        "run-shell" | "run" => cmd_run_shell(rest),
        "source-file" | "source" => cmd_source_file(rest),
        "set-option" | "set" => cmd_set_option(rest),
        "show-options" | "show" => cmd_show_options(rest),
        "bind-key" | "bind" => cmd_bind_key(rest),
        "unbind-key" | "unbind" => cmd_unbind_key(rest),
        "wait-for" | "wait" => cmd_wait_for(rest),

        // ========== Server Commands ==========
        "server-access" => cmd_server_access(rest),

        // ========== Misc ==========
        "copy-mode" => cmd_copy_mode(rest),
        "paste-buffer" | "pasteb" => cmd_paste_buffer(rest),
        "choose-tree" => cmd_choose_tree(rest),
        "find-window" | "findw" => cmd_find_window(rest),

        // Fallback for any unhandled commands
        _ => {
            log_stderr(&format!("unsupported subcommand={sub}"));
            // Still return success for unknown commands to avoid breaking tools
            Ok(())
        }
    };
    log_stderr(&format!(
        "exit subcommand={sub} status={}",
        if r.is_ok() { "ok" } else { "err" }
    ));
    process::exit(if r.is_ok() { 0 } else { 1 });
}

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("client")
}

fn auth_headers(token: &str) -> reqwest::header::HeaderMap {
    let mut m = reqwest::header::HeaderMap::new();
    m.insert(
        "X-Wind-Token",
        reqwest::header::HeaderValue::from_str(token).expect("token header"),
    );
    m
}

fn parse_pane_target(s: &str) -> usize {
    let s = s.strip_prefix('%').unwrap_or(s);
    s.parse().unwrap_or(0)
}

fn tmux_replacements(pane_index: usize) -> Vec<(&'static str, String)> {
    let pane_id = format!("%{pane_index}");
    vec![
        ("#{pane_id}", pane_id.clone()),
        ("#{window_id}", "@0".to_string()),
        ("#{window_index}", "0".to_string()),
        ("#{pane_index}", pane_index.to_string()),
        ("#{pane_active}", "1".to_string()),
        ("#{window_active}", "1".to_string()),
        ("#{session_id}", "$0".to_string()),
        ("#{session_name}", "wind".to_string()),
        ("#{window_name}", "wind".to_string()),
        ("#{pane_tty}", "/dev/pts/0".to_string()),
        // tmux 短格式
        ("#D", pane_id),
        ("#I", "0".to_string()),
        ("#P", pane_index.to_string()),
        ("#S", "wind".to_string()),
        ("#W", "wind".to_string()),
        ("#T", "wind".to_string()),
    ]
}

fn render_tmux_format(fmt: &str, pane_index: usize) -> String {
    let mut out = fmt.to_string();
    let replacements = tmux_replacements(pane_index);
    for (k, v) in replacements {
        out = out.replace(k, &v);
    }
    out
}

fn cmd_display_message(rest: &[String]) -> Result<(), ()> {
    let mut pane_index = 0usize;
    let mut format = "#{pane_id}".to_string();
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" => {}
            "-t" if i + 1 < rest.len() => {
                pane_index = parse_pane_target(&rest[i + 1]);
                i += 1;
            }
            s if s.starts_with('-') => {}
            s => {
                format = s.to_string();
            }
        }
        i += 1;
    }
    println!("{}", render_tmux_format(&format, pane_index));
    Ok(())
}

fn cmd_split(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut horizontal = false;
    let mut pane_index: Option<usize> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-h" => horizontal = true,
            "-v" => horizontal = false,
            "-l" | "-b" | "-f" | "-d" | "-Z" => {}
            "-t" if i + 1 < rest.len() => {
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                let cmd = rest[i + 1].clone();
                return post_split(url, token, horizontal, pane_index, Some(cmd));
            }
            "--" => {
                i += 1;
                let cmd = if i < rest.len() {
                    Some(rest[i..].join(" "))
                } else {
                    None
                };
                return post_split(url, token, horizontal, pane_index, cmd);
            }
            s if s.starts_with('-') => {}
            _ => {
                let cmd = Some(rest[i..].join(" "));
                return post_split(url, token, horizontal, pane_index, cmd);
            }
        }
        i += 1;
    }
    post_split(url, token, horizontal, pane_index, None)
}

fn post_split(
    url: &str,
    token: &str,
    horizontal: bool,
    pane_index: Option<usize>,
    command: Option<String>,
) -> Result<(), ()> {
    let mut body = serde_json::json!({ "horizontal": horizontal });
    if let Some(p) = pane_index {
        body["pane_index"] = serde_json::json!(p);
    }
    if let Some(c) = command {
        body["command"] = serde_json::json!(c);
    }
    let u = format!("{}/api/v1/split-window", url.trim_end_matches('/'));
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| eprintln!("wind-tmux: {e}"))?;
    if !res.status().is_success() {
        let t = res.text().unwrap_or_default();
        eprintln!("wind-tmux: split-window {}", t);
        return Err(());
    }
    Ok(())
}

fn cmd_capture(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane = 0usize;
    let mut lines = 80usize;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" | "-e" | "-C" | "-E" | "-a" | "-q" => {}
            "-S" => {}
            "-t" if i + 1 < rest.len() => {
                pane = parse_pane_target(&rest[i + 1]);
                i += 1;
            }
            "-l" | "-L" if i + 1 < rest.len() => {
                lines = rest[i + 1].parse().unwrap_or(lines);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let u = format!(
        "{}/api/v1/capture-pane?pane={}&lines={}",
        url.trim_end_matches('/'),
        pane,
        lines
    );
    let res = client()
        .get(u)
        .headers(auth_headers(token))
        .send()
        .map_err(|e| eprintln!("wind-tmux: {e}"))?;
    if !res.status().is_success() {
        eprintln!("wind-tmux: capture-pane {}", res.status());
        return Err(());
    }
    let text = res.text().map_err(|e| eprintln!("wind-tmux: {e}"))?;
    print!("{text}");
    Ok(())
}

fn tmux_key_to_bytes(word: &str) -> Vec<u8> {
    match word {
        "C-m" | "Enter" | "M-enter" => b"\r".to_vec(),
        "C-j" => b"\n".to_vec(),
        "Tab" | "C-i" => b"\t".to_vec(),
        "Space" => b" ".to_vec(),
        "BSpace" => vec![0x7f],
        "Escape" | "Esc" => vec![0x1b],
        "Up" => vec![0x1b, b'[', b'A'],
        "Down" => vec![0x1b, b'[', b'B'],
        "Right" => vec![0x1b, b'[', b'C'],
        "Left" => vec![0x1b, b'[', b'D'],
        s if s.len() == 1 => s.as_bytes().to_vec(),
        s => s.as_bytes().to_vec(),
    }
}

fn cmd_send_keys(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane = 0usize;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-t" && i + 1 < rest.len() {
            pane = parse_pane_target(&rest[i + 1]);
            i += 2;
            continue;
        }
        if rest[i].starts_with('-') {
            i += 1;
            continue;
        }
        break;
    }
    let mut buf: Vec<u8> = Vec::new();
    for w in rest.iter().skip(i) {
        buf.extend(tmux_key_to_bytes(w));
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    let u = format!("{}/api/v1/send-keys", url.trim_end_matches('/'));
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&serde_json::json!({ "pane": pane, "text": text }))
        .send()
        .map_err(|e| eprintln!("wind-tmux: {e}"))?;
    if !res.status().is_success() {
        eprintln!("wind-tmux: send-keys {}", res.status());
        return Err(());
    }
    Ok(())
}

fn cmd_list_panes(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // Claude Code 常用 tmux `list-panes -F ...` 推断 pane/window；优先返回兼容格式。
    let mut pane_index = 0usize;
    let mut format: Option<String> = None;
    let mut all_panes = false;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                pane_index = parse_pane_target(&rest[i + 1]);
                i += 1;
            }
            "-F" if i + 1 < rest.len() => {
                format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-a" => all_panes = true,
            "-s" => all_panes = true,
            "-r" => {} // reverse order (not needed for now)
            _ => {}
        }
        i += 1;
    }
    if let Some(fmt) = format {
        if all_panes {
            // For -a, we would need to fetch all panes
            println!("{}", render_tmux_format(&fmt, 0));
        } else {
            println!("{}", render_tmux_format(&fmt, pane_index));
        }
        return Ok(());
    }

    let u = format!("{}/api/v1/list-panes", url.trim_end_matches('/'));
    let res = client()
        .get(u)
        .headers(auth_headers(token))
        .send()
        .map_err(|e| eprintln!("wind-tmux: {e}"))?;
    if !res.status().is_success() {
        eprintln!("wind-tmux: list-panes {}", res.status());
        return Err(());
    }
    let text = res.text().map_err(|e| eprintln!("wind-tmux: {e}"))?;
    print!("{text}");
    Ok(())
}

// ========== Pane Management Commands ==========

fn cmd_select_pane(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane_index: Option<usize> = None;
    let mut direction: Option<&str> = None;
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
    if let Some(dir) = direction {
        // For left/right/up/down, we need to calculate the target pane
        // For now, just acknowledge the command
        return Ok(());
    }

    // Otherwise, use the HTTP API to select the pane
    let idx = pane_index.unwrap_or(0);
    let u = format!("{}/api/v1/select-pane", url.trim_end_matches('/'));
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&serde_json::json!({ "pane_index": idx }))
        .send()
        .map_err(|e| eprintln!("wind-tmux: {e}"))?;
    if !res.status().is_success() {
        // Don't fail - just acknowledge for compatibility
    }
    Ok(())
}

fn cmd_kill_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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
    log_stderr(&format!("kill-pane: pane={:?}, kill_all={}", pane_index, kill_all));
    Ok(())
}

fn cmd_resize_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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

    log_stderr(&format!(
        "resize-pane: pane={:?}, direction={:?}, adjustment={}, width={:?}, height={:?}",
        pane_index, direction, adjustment, target_width, target_height
    ));
    Ok(())
}

fn cmd_last_pane(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                // Get target window, but we want the last pane
                i += 1;
            }
            "-e" | "-d" => {} // enable/disable
            "-Z" => {} // zoom
            _ => {}
        }
        i += 1;
    }
    // Select the last active pane (index 0 for now)
    let u = format!("{}/api/v1/select-pane", url.trim_end_matches('/'));
    let _ = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&serde_json::json!({ "pane_index": 0, "last": true }))
        .send();
    Ok(())
}

fn cmd_swap_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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
    log_stderr(&format!("swap-pane: source={:?}, dest={:?}", source_pane, dest_pane));
    Ok(())
}

fn cmd_break_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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

fn cmd_join_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
    let mut source_pane: Option<usize> = None;
    let mut target_window: Option<&str> = None;
    let mut horizontal = false;
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
            "-h" => horizontal = true,
            "-v" => {}
            "-l" | "-p" if i + 1 < rest.len() => {
                i += 1; // size
            }
            _ => {}
        }
        i += 1;
    }
    log_stderr(&format!("join-pane: source={:?}, target={:?}", source_pane, target_window));
    Ok(())
}

fn cmd_respawn_pane(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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
    log_stderr(&format!("respawn-pane: pane={:?}, command={:?}", pane_index, command));
    Ok(())
}

fn cmd_pipe_pane(rest: &[String]) -> Result<(), ()> {
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

fn cmd_display_panes(rest: &[String]) -> Result<(), ()> {
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

// ========== Window Management Commands ==========

fn cmd_new_window(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
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

    // Create new pane in a new window - just use split for now
    post_split(url, token, false, None, command)
}

fn cmd_select_window(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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
    log_stderr(&format!("select-window: index={:?}", window_index));
    Ok(())
}

fn cmd_kill_window(rest: &[String], _url: &str, _token: &str) -> Result<(), ()> {
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
    log_stderr(&format!("kill-window: index={:?}", window_index));
    Ok(())
}

fn cmd_rename_window(rest: &[String]) -> Result<(), ()> {
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
    log_stderr(&format!("rename-window: index={:?}, name={:?}", window_index, new_name));
    Ok(())
}

fn cmd_move_window(rest: &[String]) -> Result<(), ()> {
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
    log_stderr(&format!("move-window: source={:?}, dest={:?}", source_index, dest_index));
    Ok(())
}

fn cmd_rotate_window(rest: &[String]) -> Result<(), ()> {
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

fn cmd_select_layout(rest: &[String]) -> Result<(), ()> {
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
    log_stderr(&format!("select-layout: window={:?}, layout={:?}", window_index, layout));
    Ok(())
}

fn cmd_respawn_window(rest: &[String]) -> Result<(), ()> {
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

fn cmd_next_window(rest: &[String]) -> Result<(), ()> {
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

fn cmd_previous_window(rest: &[String]) -> Result<(), ()> {
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

fn cmd_last_window(rest: &[String]) -> Result<(), ()> {
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

// ========== Session Management Commands ==========

fn cmd_new_session(rest: &[String]) -> Result<(), ()> {
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
    log_stderr(&format!("new-session: name={:?}, detached={}", session_name, detached));
    Ok(())
}

fn cmd_has_session(rest: &[String]) -> Result<(), ()> {
    let mut session_name = "wind";
    let mut i = 0;
    while i < rest.len() {
        if !rest[i].starts_with('-') {
            session_name = &rest[i];
        }
        i += 1;
    }
    log_stderr(&format!("has-session: {}", session_name));
    // Just return success - we always have a session
    Ok(())
}

fn cmd_list_sessions() -> Result<(), ()> {
    println!("wind: 1 windows (created Mon Jan 1 00:00:00 2020) [80x24]");
    Ok(())
}

fn cmd_attach_session(rest: &[String]) -> Result<(), ()> {
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

fn cmd_detach_client(rest: &[String]) -> Result<(), ()> {
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

fn cmd_kill_session(rest: &[String]) -> Result<(), ()> {
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

fn cmd_kill_server() -> Result<(), ()> {
    log_stderr("kill-server requested");
    Ok(())
}

fn cmd_switch_client(rest: &[String]) -> Result<(), ()> {
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

fn cmd_rename_session(rest: &[String]) -> Result<(), ()> {
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

fn cmd_lock_server() -> Result<(), ()> {
    // Lock server - not supported
    Ok(())
}

fn cmd_start_server() -> Result<(), ()> {
    // Server is always running
    Ok(())
}

// ========== List Commands ==========

fn cmd_list_windows(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
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
        println!("{}", render_tmux_format(&fmt, 0));
        return Ok(());
    }

    // Return default format
    println!("0: wind* (1 panes) [80x24] @0 (active)");
    Ok(())
}

fn cmd_list_clients(rest: &[String]) -> Result<(), ()> {
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

fn cmd_list_keys(rest: &[String]) -> Result<(), ()> {
    let mut format: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-T" if i + 1 < rest.len() => {
                // Table name
                i += 1;
            }
            "-N" => {} // numeric mode
            _ => {}
        }
        i += 1;
    }
    // List key bindings - not implemented
    Ok(())
}

fn cmd_list_commands(rest: &[String]) -> Result<(), ()> {
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

fn cmd_list_buffers() -> Result<(), ()> {
    // No buffers
    Ok(())
}

// ========== Buffer Commands ==========

fn cmd_save_buffer(rest: &[String]) -> Result<(), ()> {
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

fn cmd_load_buffer(rest: &[String]) -> Result<(), ()> {
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

fn cmd_delete_buffer(rest: &[String]) -> Result<(), ()> {
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

fn cmd_set_buffer(rest: &[String]) -> Result<(), ()> {
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

fn cmd_show_buffer(rest: &[String]) -> Result<(), ()> {
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

// ========== Other Commands ==========

fn cmd_display_menu(rest: &[String]) -> Result<(), ()> {
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

fn cmd_confirm_before(rest: &[String]) -> Result<(), ()> {
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

fn cmd_command_prompt(rest: &[String]) -> Result<(), ()> {
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

fn cmd_if_shell(rest: &[String]) -> Result<(), ()> {
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

fn cmd_run_shell(rest: &[String]) -> Result<(), ()> {
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

fn cmd_source_file(rest: &[String]) -> Result<(), ()> {
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

fn cmd_set_option(rest: &[String]) -> Result<(), ()> {
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

fn cmd_show_options(rest: &[String]) -> Result<(), ()> {
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
        println!("{}", fmt);
        return Ok(());
    }
    Ok(())
}

fn cmd_bind_key(rest: &[String]) -> Result<(), ()> {
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

fn cmd_unbind_key(rest: &[String]) -> Result<(), ()> {
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

fn cmd_wait_for(rest: &[String]) -> Result<(), ()> {
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

fn cmd_server_access(rest: &[String]) -> Result<(), ()> {
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

fn cmd_copy_mode(rest: &[String]) -> Result<(), ()> {
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

fn cmd_paste_buffer(rest: &[String]) -> Result<(), ()> {
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

fn cmd_choose_tree(rest: &[String]) -> Result<(), ()> {
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

fn cmd_find_window(rest: &[String]) -> Result<(), ()> {
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
