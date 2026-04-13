//! Claude Code `teammateMode: tmux` 兼容：把 `tmux` 子命令翻译成 Wind 本地 HTTP（见 `WIND_TEAMMATE_URL` / `WIND_TEAMMATE_TOKEN`）。
//! 使用：将本二进制放到 PATH 且命名为 `tmux`，或在 Claude 配置中指向本程序。

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    // Claude Code 等会先跑 `tmux -V` 判断是否存在 tmux；此前落到 unsupported 会导致永远不启用 split。
    for a in args.iter().skip(1) {
        if a == "-V" || a == "--version" {
            println!("tmux 3.4");
            process::exit(0);
        }
        if a == "-h" || a == "--help" {
            eprintln!("wind-tmux shim: split-window capture-pane send-keys list-panes … (needs WIND_TEAMMATE_*)");
            process::exit(0);
        }
    }
    if args.len() < 2 {
        eprintln!("wind-tmux: missing subcommand");
        process::exit(1);
    }
    let url = env::var("WIND_TEAMMATE_URL").unwrap_or_default();
    let token = env::var("WIND_TEAMMATE_TOKEN").unwrap_or_default();
    if url.is_empty() || token.is_empty() {
        eprintln!("wind-tmux: set WIND_TEAMMATE_URL and WIND_TEAMMATE_TOKEN (Wind injects these in PTY)");
        process::exit(1);
    }
    let sub = args[1].as_str();
    let rest = &args[2..];
    let r = match sub {
        "split-window" | "splitw" => cmd_split(rest, &url, &token),
        "capture-pane" | "capturep" => cmd_capture(rest, &url, &token),
        "send-keys" | "send" => cmd_send_keys(rest, &url, &token),
        "list-panes" | "lsp" => cmd_list_panes(&url, &token),
        "has-session" | "has" => Ok(()),
        "new-session" | "new" => Ok(()),
        "list-sessions" | "ls" => {
            println!("wind: 1 windows (created Mon Jan 1 00:00:00 2020)");
            Ok(())
        }
        // 探测 / 会话生命周期：返回成功即可，避免 Claude 判定 tmux 不可用
        "display-message" | "display" => {
            println!("%0");
            Ok(())
        }
        "start-server" | "start" => Ok(()),
        "attach-session" | "attach" => Ok(()),
        "kill-session" => Ok(()),
        "kill-server" => Ok(()),
        "list-windows" | "lsw" => {
            println!("0: wind* (1 panes) [80x24] @0 (active)");
            Ok(())
        }
        _ => {
            eprintln!("wind-tmux: unsupported subcommand {sub:?}");
            Err(())
        }
    };
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

fn cmd_list_panes(url: &str, token: &str) -> Result<(), ()> {
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
