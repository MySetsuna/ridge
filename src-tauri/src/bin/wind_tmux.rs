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
            eprintln!("wind-tmux shim: split-window capture-pane send-keys list-panes … (needs WIND_TEAMMATE_*)");
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
        "split-window" | "splitw" => cmd_split(rest, &url, &token),
        "capture-pane" | "capturep" => cmd_capture(rest, &url, &token),
        "send-keys" | "send" => cmd_send_keys(rest, &url, &token),
        "list-panes" | "lsp" => cmd_list_panes(rest, &url, &token),
        "has-session" | "has" => Ok(()),
        "new-session" | "new" => Ok(()),
        "list-sessions" | "ls" => {
            println!("wind: 1 windows (created Mon Jan 1 00:00:00 2020)");
            Ok(())
        }
        // 探测 / 会话生命周期：返回成功即可，避免 Claude 判定 tmux 不可用
        "display-message" | "display" => cmd_display_message(rest),
        "start-server" | "start" => Ok(()),
        "attach-session" | "attach" => Ok(()),
        "kill-session" => Ok(()),
        "kill-server" => Ok(()),
        "list-windows" | "lsw" => {
            println!("0: wind* (1 panes) [80x24] @0 (active)");
            Ok(())
        }
        _ => {
            log_stderr(&format!("unsupported subcommand={sub}"));
            eprintln!("wind-tmux: unsupported subcommand {sub:?}");
            Err(())
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
            _ => {}
        }
        i += 1;
    }
    if let Some(fmt) = format {
        println!("{}", render_tmux_format(&fmt, pane_index));
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
