//! Claude Code `teammateMode: tmux` 兼容：把 `tmux` 子命令翻译成 Ridge 本地 HTTP（见 `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN`）。
//! 使用：将本二进制放到 PATH 且命名为 `tmux`，或在 Claude 配置中指向本程序。

use std::env;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{}", d.as_secs(), d.subsec_millis()),
        Err(_) => "0.000".to_string(),
    }
}

fn log_to_file(msg: &str) {
    let line = format!("[tmux-shim][{}] {msg}", now_ts());
    log_file_append(&line);
}

fn log_file_path() -> Option<PathBuf> {
    if let Ok(p) = env::var("Ridge_TMUX_LOG") {
        let t = p.trim();
        if !t.is_empty() {
            let pb = PathBuf::from(t);
            // 允许把 `Ridge_TMUX_LOG` 设成「目录」：此前会把目录当文件 open 失败并静默落到 %TEMP%。
            if pb.is_dir() {
                return Some(pb.join("tmux-shim.log"));
            }
            return Some(pb);
        }
    }
    // 默认落到系统临时目录，避免开发模式下写入源码目录触发 Tauri watcher 重启。
    Some(env::temp_dir().join("tmux-shim.log"))
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
            let fallback = env::temp_dir().join("tmux-shim.log");
            let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&fallback) else {
                return;
            };
            let _ = writeln!(f, "{line}");
            fallback
        }
    };
    static LOG_PATH_ONCE: OnceLock<()> = OnceLock::new();
    let _ = LOG_PATH_ONCE.get_or_init(|| {
        // 必须直接写文件：若这里再调用 `log_file_append`，会重入同一个 `OnceLock` 并死锁，
        // 导致 `tmux -V` 等首条日志路径上永远到不了版本分支。
        let msg = format!("[tmux-shim][{}] file-log={}", now_ts(), actual_path.display());
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&actual_path)
        {
            let _ = writeln!(f, "{msg}");
        }
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
    let url_set = env::var("RIDGE_TEAMMATE_URL")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let token_set = env::var("RIDGE_TEAMMATE_TOKEN")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    log_to_file(&format!(
        "invoke args=[{joined_args}] tmux_env={tmux_env:?} teammate_url_set={url_set} teammate_token_set={token_set}"
    ));
    // Claude Code 等会先跑 `tmux -V` 判断是否存在 tmux；此前落到 unsupported 会导致永远不启用 split。
    for a in args.iter().skip(1) {
        if a == "-V" || a == "--version" {
            log_to_file("probe version -> tmux 3.4");
            println!("tmux 3.4");
            process::exit(0);
        }
        if a == "--help" {
            log_to_file("probe help");
            eprintln!("tmux shim: supports all tmux commands (needs Ridge_TEAMMATE_*)");
            process::exit(0);
        }
    }
    let (socket_id, sub_idx) = parse_global_flags(&args);
    let _ = SOCKET.set(socket_id);
    if sub_idx >= args.len() {
        log_to_file("missing subcommand");
        eprintln!("tmux: missing subcommand");
        process::exit(1);
    }
    let url = env::var("RIDGE_TEAMMATE_URL").unwrap_or_default();
    let token = env::var("RIDGE_TEAMMATE_TOKEN").unwrap_or_default();
    if url.is_empty() || token.is_empty() {
        log_to_file("missing RIDGE_TEAMMATE_URL/TOKEN");
        eprintln!("tmux: RIDGE_TEAMMATE_URL/TOKEN not set (run tmux inside Ridge)");
        process::exit(1);
    }
    let sub = args[sub_idx].as_str();
    let rest = &args[sub_idx + 1..];
    log_to_file(&format!("socket={} sub={sub}", socket()));
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
        "rename-window" => cmd_rename_window(rest, &url, &token),
        "move-window" | "movew" => cmd_move_window(rest),
        "rotate-window" | "rotw" => cmd_rotate_window(rest),
        "select-layout" | "selel" => cmd_select_layout(rest),
        "respawn-window" | "respawnw" => cmd_respawn_window(rest),
        "next-window" | "nextw" => cmd_next_window(rest),
        "previous-window" | "prevw" => cmd_previous_window(rest),
        "last-window" | "lastw" => cmd_last_window(rest),

        // ========== Session Management ==========
        "new-session" | "new" => cmd_new_session(rest, &url, &token),
        "has-session" | "has" => cmd_has_session(rest, &url, &token),
        "list-sessions" | "ls" => cmd_list_sessions(rest, &url, &token),
        "attach-session" | "attach" => cmd_attach_session(rest, &url, &token),
        "detach-client" | "detach" => cmd_detach_client(rest),
        "kill-session" => cmd_kill_session(rest, &url, &token),
        "kill-server" => cmd_kill_server(&url, &token),
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
        "display-message" | "display" => cmd_display_message(rest, &url, &token),
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
            log_to_file(&format!("unsupported subcommand={sub}"));
            // Still return success for unknown commands to avoid breaking tools
            Ok(())
        }
    };
    log_to_file(&format!(
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
        "X-Ridge-Token",
        reqwest::header::HeaderValue::from_str(token).expect("token header"),
    );
    // 发起方工作区身份（由 Ridge PTY 注入 `RIDGE_WORKSPACE_ID`，shim 子进程继承）：
    // 让后端把 GUI-bridge 的 split/复用/接管锁定在「发起 tmux 的会话所在工作区」。
    if let Ok(ws) = env::var("RIDGE_WORKSPACE_ID") {
        let ws = ws.trim();
        if !ws.is_empty() {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(ws) {
                m.insert("X-Ridge-Workspace", v);
            }
        }
    }
    m
}

// ===================== 全局标志 / socket / 路由助手 =====================

static SOCKET: OnceLock<String> = OnceLock::new();

/// 当前 socket 命名空间键：`default` / `L:<name>` / `S:<path>`。
fn socket() -> &'static str {
    SOCKET.get().map(String::as_str).unwrap_or("default")
}

/// 解析子命令前的全局标志，返回 (socket_id, 子命令在 args 中的起始下标)。
/// 处理 `-L name` / `-S path`，并吞掉常见的无关全局开关（`-f` 配置、`-2/-u/-v/-q`、
/// `-C/-CC` 控制模式、`-T type`、全局 `-c`）。遇到第一个非全局标志即子命令。
fn parse_global_flags(args: &[String]) -> (String, usize) {
    let mut sock = "default".to_string();
    let mut i = 1; // args[0] = 程序名
    while i < args.len() {
        match args[i].as_str() {
            "-L" if i + 1 < args.len() => {
                sock = format!("L:{}", args[i + 1]);
                i += 2;
            }
            "-S" if i + 1 < args.len() => {
                sock = format!("S:{}", args[i + 1]);
                i += 2;
            }
            // 带取值、与 socket 无关的全局标志：跳过标志 + 取值。
            "-f" | "-T" | "-c" if i + 1 < args.len() => i += 2,
            // 无取值的全局开关。
            "-2" | "-8" | "-u" | "-v" | "-q" | "-N" | "-D" | "-C" | "-CC" => i += 1,
            // 第一个非全局标志 → 子命令。
            _ => break,
        }
    }
    (sock, i)
}

/// 目标是否「会话限定」（需走 native 解析）：`=NAME` 或带非数字会话名。
/// 纯 `%N` / 纯数字 / 空 视为 GUI 当前会话的 pane（默认 socket 走 GUI 遗留路径）。
fn target_is_session_qualified(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() {
        return false;
    }
    if let Some(r) = t.strip_prefix('=') {
        return !r.trim().is_empty();
    }
    if t.starts_with('%') {
        return false;
    }
    let end = t.find(|c| c == ':' || c == '.').unwrap_or(t.len());
    let cand = &t[..end];
    !cand.is_empty() && cand.parse::<usize>().is_err()
}

/// 是否路由到 native 引擎：自定义 socket（`-L`/`-S`）一律 native；默认 socket 仅当
/// 目标会话限定时 native，否则走 GUI 遗留路径。
fn use_native(target: Option<&str>) -> bool {
    socket() != "default" || target.map(target_is_session_qualified).unwrap_or(false)
}

fn tmux_api(url: &str, path: &str) -> String {
    format!("{}/api/v1/tmux/{}", url.trim_end_matches('/'), path)
}

/// GET，返回 (status, body)；网络失败 None。
fn http_get(u: String, token: &str) -> Option<(u16, String)> {
    let res = client().get(u).headers(auth_headers(token)).send().ok()?;
    let status = res.status().as_u16();
    Some((status, res.text().unwrap_or_default()))
}

/// POST JSON，返回 (status, body)；网络失败 None。
fn http_post(u: String, token: &str, body: serde_json::Value) -> Option<(u16, String)> {
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .ok()?;
    let status = res.status().as_u16();
    Some((status, res.text().unwrap_or_default()))
}

/// 极简 query 值编码。
fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 抽取 `-t VALUE`；缺失时兜底取第一个非 flag 实参。
fn extract_target(rest: &[String]) -> Option<String> {
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-t" && i + 1 < rest.len() {
            return Some(rest[i + 1].clone());
        }
        i += 1;
    }
    rest.iter().find(|a| !a.starts_with('-')).cloned()
}

/// 抽取 `-F VALUE`。
fn extract_f(rest: &[String]) -> Option<String> {
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-F" && i + 1 < rest.len() {
            return Some(rest[i + 1].clone());
        }
        i += 1;
    }
    None
}

/// native `select`（记账）。返回 `Some(result)` 表示已处理（或硬失败），`None` 表示
/// 命中 GUI 会话（409）应回退 GUI 路径。
fn native_select(url: &str, token: &str, target: &str) -> Option<Result<(), ()>> {
    let body = serde_json::json!({ "socket": socket(), "target": target });
    match http_post(tmux_api(url, "select"), token, body) {
        Some((200, _)) => Some(Ok(())),
        Some((409, _)) => None,
        Some((_, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Some(Err(()))
        }
        None => Some(Err(())),
    }
}

/// native `kill`（scope ∈ pane/window/session/server）。语义同 `native_select`。
fn native_kill(url: &str, token: &str, target: &str, scope: &str) -> Option<Result<(), ()>> {
    let body = serde_json::json!({ "socket": socket(), "target": target, "scope": scope });
    match http_post(tmux_api(url, "kill"), token, body) {
        Some((200, _)) => Some(Ok(())),
        Some((409, _)) => None,
        Some((_, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Some(Err(()))
        }
        None => Some(Err(())),
    }
}

/// 启动本 shim 的父进程可执行文件路径（通常正是调用 `tmux` 的那个 shell）。
fn parent_process_exe() -> Option<String> {
    let me = sysinfo::get_current_pid().ok()?;
    let sys = sysinfo::System::new_all();
    let parent = sys.process(me)?.parent()?;
    let exe = sys.process(parent)?.exe()?;
    let s = exe.to_string_lossy().to_string();
    (!s.is_empty()).then_some(s)
}

/// 解析 native 面板应使用的 shell。
///
/// 关键：优先用「启动本 shim 的父进程 exe」——正是调用脚本所用的那个 shell（如 git-bash
/// 的 `bash.exe`，一个 Windows 后端可直接 spawn 的真实路径）。这样无头面板与调用脚本用
/// 同一个 shell 二进制，`/tmp` 等路径解析完全一致（`$SHELL` 在 git-bash 里是 `/usr/bin/bash`
/// 这种 MSYS 路径，Windows 后端无法直接 spawn）。父进程不是 shell 时回退 `$SHELL`。
fn resolve_shell() -> Option<String> {
    if let Some(p) = parent_process_exe() {
        let low = p.to_ascii_lowercase();
        let is_shell = ["bash", "zsh", "sh", "pwsh", "powershell", "cmd", "fish", "dash"]
            .iter()
            .any(|name| {
                low.ends_with(&format!("{name}.exe")) || low.ends_with(&format!("/{name}"))
            });
        if is_shell {
            return Some(p);
        }
    }
    env::var("SHELL").ok().filter(|s| !s.trim().is_empty())
}

fn parse_pane_target(s: &str) -> usize {
    let s = s.trim();
    // %N → pane index N (tmux pane ID format)
    if let Some(n) = s.strip_prefix('%') {
        return n.parse().unwrap_or(0);
    }
    // session:window.pane or window.pane — take part after last '.'
    if let Some(dot) = s.rfind('.') {
        let pane_part = &s[dot + 1..];
        if let Ok(n) = pane_part.parse::<usize>() {
            return n;
        }
    }
    // bare number
    s.parse().unwrap_or(0)
}

/// 与 Ridge PTY 注入的 `TMUX_PANE` / `TMUX` 对齐，供未带 `-t` 的 probe（如 `display-message -p`）推断当前窗格。
fn current_pane_index_from_env() -> usize {
    if let Ok(pane) = env::var("TMUX_PANE") {
        let t = pane.trim();
        if !t.is_empty() {
            return parse_pane_target(t);
        }
    }
    if let Ok(tmux) = env::var("TMUX") {
        // `terminal.rs`: `/ridge/teammate.sock,0,<pane_slot>`
        if let Some(third) = tmux.split(',').nth(2) {
            let t = third.trim();
            if !t.is_empty() {
                return parse_pane_target(t);
            }
        }
    }
    0
}

fn tmux_replacements(pane_index: usize) -> Vec<(&'static str, String)> {
    let pane_id = format!("%{pane_index}");
    vec![
        // ── Pane identity ──
        ("#{pane_id}", pane_id.clone()),
        ("#{pane_index}", pane_index.to_string()),
        ("#{pane_active}", "1".to_string()),
        ("#{pane_tty}", "/dev/pts/0".to_string()),
        ("#{pane_pid}", "1".to_string()),
        ("#{pane_title}", "ridge".to_string()),
        ("#{pane_current_command}", "shell".to_string()),
        // Dimensions – real size not known at shim level; use conservative defaults
        ("#{pane_width}", "120".to_string()),
        ("#{pane_height}", "80".to_string()),
        ("#{pane_left}", "0".to_string()),
        ("#{pane_top}", "0".to_string()),
        ("#{pane_right}", "119".to_string()),
        ("#{pane_bottom}", "79".to_string()),
        // ── Window ──
        ("#{window_id}", "@0".to_string()),
        ("#{window_index}", "0".to_string()),
        ("#{window_active}", "1".to_string()),
        ("#{window_name}", "ridge".to_string()),
        ("#{window_layout}", "tiled".to_string()),
        ("#{window_width}", "120".to_string()),
        ("#{window_height}", "80".to_string()),
        // window_panes is dynamic and filled by render_tmux_format_dynamic
        // ── Session ──
        ("#{session_id}", "$0".to_string()),
        ("#{session_name}", "ridge".to_string()),
        ("#{session_windows}", "1".to_string()),
        ("#{client_session}", "ridge".to_string()),
        // ── Client ──
        ("#{client_width}", "120".to_string()),
        ("#{client_height}", "80".to_string()),
        ("#{client_tty}", "/dev/pts/0".to_string()),
        // ── Short aliases ──
        ("#D", pane_id),
        ("#I", "0".to_string()),
        ("#P", pane_index.to_string()),
        ("#S", "ridge".to_string()),
        ("#W", "ridge".to_string()),
        ("#T", "ridge".to_string()),
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

fn find_pane_by_name(url: &str, token: &str, name: &str) -> Option<usize> {
    let u = format!("{}/api/v1/list-panes?json=1", url.trim_end_matches('/'));
    let res = client().get(u).headers(auth_headers(token)).send().ok()?;
    let json: serde_json::Value = res.json().ok()?;
    let panes = json.get("panes")?.as_array()?;
    // Prefer the highest index when multiple panes share a name (most recently created).
    let mut best: Option<usize> = None;
    for pane in panes {
        let title = pane.get("title").and_then(|t| t.as_str()).unwrap_or("");
        if title == name {
            if let Some(idx) = pane.get("index").and_then(|v| v.as_u64()).map(|v| v as usize) {
                best = Some(match best {
                    Some(prev) => prev.max(idx),
                    None => idx,
                });
            }
        }
    }
    best
}

fn rename_pane_http(url: &str, token: &str, pane_index: usize, name: &str) {
    let u = format!("{}/api/v1/rename-pane", url.trim_end_matches('/'));
    let body = serde_json::json!({ "pane_index": pane_index, "name": name });
    match client().post(u).headers(auth_headers(token)).json(&body).send() {
        Ok(res) => log_to_file(&format!("rename_pane: pane={pane_index} name={name} status={}", res.status())),
        Err(e) => log_to_file(&format!("rename_pane: HTTP error: {e}")),
    }
}

fn resolve_named_pane_target(v: &str, url: &str, token: &str) -> SendTarget {
    let v = v.trim();
    if let Some(n) = v.strip_prefix('%') {
        if let Ok(idx) = n.parse::<usize>() {
            return SendTarget::Index(idx);
        }
    }
    if let Some(colon) = v.find(':') {
        let after_colon = &v[colon + 1..];
        if let Some(dot) = after_colon.rfind('.') {
            if let Ok(idx) = after_colon[dot + 1..].parse::<usize>() {
                return SendTarget::Index(idx);
            }
        }
        if let Ok(idx) = after_colon.parse::<usize>() {
            return SendTarget::Index(idx);
        }
        log_to_file(&format!("resolve_named_pane_target: lookup name={after_colon:?}"));
        if let Some(idx) = find_pane_by_name(url, token, after_colon) {
            return SendTarget::Index(idx);
        }
        return SendTarget::TmuxCurrent;
    }
    if let Ok(idx) = v.parse::<usize>() {
        return SendTarget::Index(idx);
    }
    if let Some(dot) = v.rfind('.') {
        if let Ok(idx) = v[dot + 1..].parse::<usize>() {
            return SendTarget::Index(idx);
        }
    }
    SendTarget::TmuxCurrent
}

/// Minimal JSON shape returned by `/api/v1/list-panes?json=1`.
/// Only the fields we need for dynamic template substitution.
#[derive(serde::Deserialize)]
struct ListPanesJson {
    pane_count: usize,
    panes: Vec<PaneInfoJson>,
}
#[derive(serde::Deserialize)]
struct PaneInfoJson {
    index: usize,
    #[allow(dead_code)]
    pane_id: String,
    #[serde(default)]
    cwd: Option<String>,
}

/// Extends `render_tmux_format` with dynamic variables that require a backend
/// round-trip: `#{window_panes}` and `#{pane_current_path}`.
/// Falls back to static rendering if the backend is unreachable.
fn render_tmux_format_dynamic(fmt: &str, pane_index: usize, url: &str, token: &str) -> String {
    let mut out = render_tmux_format(fmt, pane_index);

    // Only query backend if the result still contains dynamic placeholders.
    let needs_pane_count = out.contains("#{window_panes}");
    let needs_cwd = out.contains("#{pane_current_path}");
    if !needs_pane_count && !needs_cwd {
        return out;
    }

    let u = format!("{}/api/v1/list-panes?json=1", url.trim_end_matches('/'));
    let resp = client()
        .get(&u)
        .headers(auth_headers(token))
        .send()
        .ok()
        .and_then(|r| if r.status().is_success() { r.json::<ListPanesJson>().ok() } else { None });

    if let Some(data) = resp {
        if needs_pane_count {
            out = out.replace("#{window_panes}", &data.pane_count.to_string());
        }
        if needs_cwd {
            let cwd = data.panes.iter()
                .find(|p| p.index == pane_index)
                .and_then(|p| p.cwd.clone())
                .unwrap_or_default();
            out = out.replace("#{pane_current_path}", &cwd);
        }
    }
    out
}

fn cmd_display_message(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane_index = current_pane_index_from_env();
    let mut format = "#{pane_id}".to_string();
    let mut raw_target: Option<String> = None;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" => {}
            "-t" if i + 1 < rest.len() => {
                raw_target = Some(rest[i + 1].clone());
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

    // 会话限定目标 / 自定义 socket → native display-message（按解析到的目标渲染 `-F`）。
    if use_native(raw_target.as_deref()) {
        let u = format!(
            "{}?socket={}&target={}&format={}",
            tmux_api(url, "display-message"),
            q(socket()),
            q(raw_target.as_deref().unwrap_or("")),
            q(&format)
        );
        match http_get(u, token) {
            Some((200, body)) => {
                println!("{body}");
                return Ok(());
            }
            Some((409, _)) => {}
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
    }

    println!("{}", render_tmux_format_dynamic(&format, pane_index, url, token));
    Ok(())
}

/// tmux `cmd-split-window.c`：`split-window -P` 且未指定 `-F` 时的默认模板。
const SPLIT_WINDOW_PRINT_DEFAULT: &str = "#{session_name}:#{window_index}.#{pane_index}";

fn cmd_split(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut horizontal = false;
    let mut pane_index: Option<usize> = None;
    let mut raw_target: Option<String> = None;
    let mut print_pane = false;
    let mut output_format: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut shell_start: Option<usize> = None;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-h" => horizontal = true,
            "-v" => horizontal = false,
            "-P" => print_pane = true,
            "-F" if i + 1 < rest.len() => {
                output_format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                raw_target = Some(rest[i + 1].clone());
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                cwd = Some(rest[i + 1].clone());
                i += 1;
            }
            "-l" | "-p" => {
                if i + 1 < rest.len() && !rest[i + 1].starts_with('-') {
                    i += 1;
                }
            }
            "-e" if i + 1 < rest.len() && !rest[i + 1].starts_with('-') => {
                i += 1;
            }
            "-b" | "-f" | "-d" | "-Z" | "-I" => {}
            "--" => {
                i += 1;
                if i < rest.len() {
                    shell_start = Some(i);
                }
                break;
            }
            s if s.starts_with('-') => {}
            _ => {
                shell_start = Some(i);
                break;
            }
        }
        i += 1;
    }

    let command = shell_start
        .and_then(|j| {
            let s = rest[j..].join(" ");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let print_template = if print_pane {
        Some(
            output_format
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| SPLIT_WINDOW_PRINT_DEFAULT.to_string()),
        )
    } else {
        None
    };

    // 会话限定目标 / 自定义 socket → native split（在该会话窗口里起新的无头面板）。
    // 命中 GUI 会话（409）则回退到下方 GUI split。
    if use_native(raw_target.as_deref()) {
        let shell = resolve_shell();
        let body = serde_json::json!({
            "socket": socket(),
            "target": raw_target.clone().unwrap_or_default(),
            "horizontal": horizontal,
            "new_window": false,
            "cwd": cwd.clone(),
            "shell": shell,
            "command": command.clone(),
            "print": print_pane,
            "print_format": output_format.clone(),
        });
        match http_post(tmux_api(url, "split-window"), token, body) {
            Some((200, out)) => {
                if !out.is_empty() {
                    println!("{out}");
                }
                return Ok(());
            }
            Some((409, _)) => {}
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
    }

    // GUI 路径：后端在发起方工作区内「先复用空闲 shell 面板，否则在最大 pane 上 split」。
    post_split(
        url,
        token,
        horizontal,
        pane_index,
        command,
        cwd,
        print_template.as_deref(),
    )
    .map(|_| ())
}

fn post_split(
    url: &str,
    token: &str,
    horizontal: bool,
    pane_index: Option<usize>,
    command: Option<String>,
    cwd: Option<String>,
    print_template: Option<&str>,
) -> Result<usize, ()> {
    log_to_file(&format!(
        "post_split: horizontal={}, pane_index={:?}, command={:?}, cwd={:?}, print={}",
        horizontal,
        pane_index,
        command,
        cwd,
        print_template.is_some()
    ));
    let mut body = serde_json::json!({ "horizontal": horizontal });
    if let Some(p) = pane_index {
        body["pane_index"] = serde_json::json!(p);
    }
    if let Some(c) = command {
        body["command"] = serde_json::json!(c);
    }
    if let Some(c) = cwd.filter(|s| !s.is_empty()) {
        body["cwd"] = serde_json::json!(c);
    }
    let u = format!("{}/api/v1/split-window", url.trim_end_matches('/'));
    log_to_file(&format!("post_split: posting to {}", u));
    let res = match client()
        .post(&u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            log_to_file(&format!("tmux: HTTP error: {e}"));
            return Err(());
        }
    };
    log_to_file(&format!("post_split: response status={}", res.status()));
    let status = res.status();
    let text = match res.text() {
        Ok(t) => t,
        Err(e) => {
            log_to_file(&format!("tmux: split-window read body: {e}"));
            return Err(());
        }
    };
    if !status.is_success() {
        log_to_file(&format!("tmux: split-window error: {}", text));
        return Err(());
    }
    let new_idx: usize = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("new_pane_index")?.as_u64())
        .map(|u| u as usize)
        .unwrap_or(0);
    log_to_file(&format!("post_split: success new_pane_index={new_idx}"));
    if let Some(tpl) = print_template {
        println!("{}", render_tmux_format(tpl, new_idx));
    }
    Ok(new_idx)
}

fn cmd_capture(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane = 0usize;
    let mut lines = 80usize;
    let mut raw_target: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" | "-e" | "-C" | "-E" | "-a" | "-q" => {}
            "-S" => {}
            "-t" if i + 1 < rest.len() => {
                raw_target = Some(rest[i + 1].clone());
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

    // 会话限定目标 / 自定义 socket → native capture-pane（grid-backed 当前屏；
    // 编排方据此回读子代理输出）。命中 GUI 会话则回退原 GUI 路径。
    if use_native(raw_target.as_deref()) {
        let u = format!(
            "{}?socket={}&target={}&lines={}",
            tmux_api(url, "capture-pane"),
            q(socket()),
            q(raw_target.as_deref().unwrap_or("")),
            lines
        );
        match http_get(u, token) {
            Some((200, body)) => {
                println!("{body}");
                return Ok(());
            }
            Some((409, _)) => {} // 命中 GUI 会话 → 回退 GUI 路径
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
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
        .map_err(|e| eprintln!("tmux: {e}"))?;
    if !res.status().is_success() {
        eprintln!("tmux: capture-pane {}", res.status());
        return Err(());
    }
    let text = res.text().map_err(|e| eprintln!("tmux: {e}"))?;
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

#[derive(Debug)]
struct StructuredLaunch {
    cwd: Option<String>,
    program: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
}

#[derive(Clone, Copy)]
enum SendTarget {
    TmuxCurrent,
    Index(usize),
}

fn split_shell_words(input: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                ' ' | '\t' => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(ch),
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

fn parse_structured_launch(line: &str) -> Option<StructuredLaunch> {
    let tokens = split_shell_words(line.trim())?;
    if tokens.is_empty() {
        return None;
    }
    let mut i = 0usize;
    let mut cwd: Option<String> = None;
    if tokens.get(i).is_some_and(|t| t == "cd") {
        cwd = tokens.get(i + 1).cloned();
        i += 2;
        if tokens.get(i).is_some_and(|t| t == "&&") {
            i += 1;
        }
    }
    if tokens.get(i).is_none_or(|t| t != "env") {
        return None;
    }
    i += 1;
    let mut envs = std::collections::HashMap::<String, String>::new();
    while let Some(tok) = tokens.get(i) {
        if let Some((k, v)) = tok.split_once('=') {
            if !k.is_empty() {
                envs.insert(k.to_string(), v.to_string());
                i += 1;
                continue;
            }
        }
        break;
    }
    if envs.is_empty() {
        return None;
    }
    let program = expand_dynamic_tokens(tokens.get(i)?);
    let args = tokens
        .get(i + 1..)
        .unwrap_or(&[])
        .iter()
        .map(|s| expand_dynamic_tokens(s))
        .collect::<Vec<_>>();
    for v in envs.values_mut() {
        *v = expand_dynamic_tokens(v);
    }
    Some(normalize_structured_launch(StructuredLaunch {
        cwd,
        program,
        args,
        env: envs,
    }))
}

fn expand_dynamic_tokens(s: &str) -> String {
    let mut out = s.to_string();
    // Claude 常见模板：`$(date +%s)`，结构化模式下手动替换为 epoch 秒。
    if out.contains("$(date +%s)") {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out = out.replace("$(date +%s)", &ts.to_string());
    }
    // Claude 常见模板：`$((RANDOM % 9000 + 1000))`，替换为 1000..9999。
    if out.contains("$((RANDOM % 9000 + 1000))") {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let n = 1000 + (nanos % 9000);
        out = out.replace("$((RANDOM % 9000 + 1000))", &n.to_string());
    }
    out
}

fn normalize_structured_launch(mut launch: StructuredLaunch) -> StructuredLaunch {
    #[cfg(windows)]
    {
        let p = launch.program.trim();
        if p.to_ascii_lowercase().ends_with(".js") {
            let script = p.to_string();
            let mut new_args = Vec::with_capacity(launch.args.len() + 1);
            new_args.push(script.clone());
            new_args.extend(launch.args);
            launch.args = new_args;

            // Prefer node.exe adjacent to nvm4w/node_modules root if present.
            let candidate = Path::new(&script)
                .ancestors()
                .find_map(|a| {
                    let name = a.file_name()?.to_string_lossy().to_ascii_lowercase();
                    if name == "node_modules" {
                        a.parent().map(|parent| parent.join("node.exe"))
                    } else {
                        None
                    }
                })
                .filter(|p| p.is_file());
            launch.program = candidate
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "node".to_string());
        }
    }
    launch
}

fn post_spawn_process(
    url: &str,
    token: &str,
    target: &SendTarget,
    launch: &StructuredLaunch,
) -> Result<(), ()> {
    let mut body = serde_json::json!({
        "cwd": &launch.cwd,
        "program": &launch.program,
        "args": &launch.args,
        "env": &launch.env,
    });
    match target {
        SendTarget::TmuxCurrent => body["use_tmux_current_pane"] = serde_json::json!(true),
        SendTarget::Index(p) => {
            body["pane"] = serde_json::json!(p);
            body["use_tmux_current_pane"] = serde_json::json!(false);
        }
    }
    let u = format!("{}/api/v1/spawn-process", url.trim_end_matches('/'));
    log_to_file(&format!("spawn-process: posting to {}", u));
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| {
            log_to_file(&format!("spawn-process: HTTP error: {e}"));
            eprintln!("tmux: {e}");
        })?;
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().unwrap_or_default();
        log_to_file(&format!(
            "spawn-process: non-success status={} body={}",
            status, text
        ));
        eprintln!("tmux: spawn-process {}", status);
        return Err(());
    }
    log_to_file("spawn-process: success");
    Ok(())
}

fn cmd_send_keys(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // `-t ""` 或未出现 `-t` 时与 tmux 一致：发往当前窗格（由 teammate HTTP 侧 `teammate_tmux_pane_cursor` 记录）。
    let mut target = SendTarget::TmuxCurrent;
    let mut raw_target: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-t" && i + 1 < rest.len() {
            let v = rest[i + 1].trim();
            raw_target = Some(v.to_string());
            if v.is_empty() {
                target = SendTarget::TmuxCurrent;
            } else {
                target = resolve_named_pane_target(v, url, token);
            }
            i += 2;
            continue;
        }
        if rest[i].starts_with('-') {
            // `-N count` 带值；其余无值开关（-l/-H/-R/-M）跳过。
            if rest[i] == "-N" && i + 1 < rest.len() {
                i += 2;
                continue;
            }
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

    // 会话限定目标 / 自定义 socket → native send-keys（写目标面板 master）。
    // 命中 GUI 会话（409）则回退到下方 GUI 路径。
    if use_native(raw_target.as_deref()) {
        let body = serde_json::json!({
            "socket": socket(),
            "target": raw_target.clone().unwrap_or_default(),
            "text": text,
        });
        match http_post(tmux_api(url, "send-keys"), token, body) {
            Some((200, _)) => return Ok(()),
            Some((409, _)) => {}
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
    }

    let candidate = text.trim_end_matches(['\r', '\n']).trim();
    // Structured launch: trigger regardless of trailing Enter. Claude Code often sends
    // the command and Enter in separate `send-keys` calls; waiting for the Enter would
    // mean the command text gets typed into the default shell (which can't execute
    // Unix `env K=V` syntax on Windows). A follow-up Enter is harmless — it just
    // sends a newline to the spawned process's stdin.
    if !candidate.is_empty() {
        if let Some(launch) = parse_structured_launch(candidate) {
            if post_spawn_process(url, token, &target, &launch).is_ok() {
                return Ok(());
            }
            log_to_file("spawn-process failed, falling back to send-keys");
        }
    }
    let body = match target {
        SendTarget::TmuxCurrent => serde_json::json!({
            "use_tmux_current_pane": true,
            "text": text,
        }),
        SendTarget::Index(p) => serde_json::json!({
            "pane": p,
            "use_tmux_current_pane": false,
            "text": text,
        }),
    };
    let u = format!("{}/api/v1/send-keys", url.trim_end_matches('/'));
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| eprintln!("tmux: {e}"))?;
    if !res.status().is_success() {
        eprintln!("tmux: send-keys {}", res.status());
        return Err(());
    }
    Ok(())
}

fn cmd_list_panes(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // Claude Code 常用 tmux `list-panes -F ...` 推断 pane/window；优先返回兼容格式。
    let mut pane_index = current_pane_index_from_env();
    let mut format: Option<String> = None;
    let mut all_panes = false;
    let mut raw_target: Option<String> = None;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                raw_target = Some(rest[i + 1].clone());
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

    // 会话限定目标 / 自定义 socket → native list-panes（真实面板 id + `-F` 渲染）。
    // 这条必须正确：验收脚本用 `list-panes -t =S -F '#{session_name}'` 作破坏性操作前的守卫。
    if use_native(raw_target.as_deref()) {
        let mut u = format!(
            "{}?socket={}&target={}",
            tmux_api(url, "list-panes"),
            q(socket()),
            q(raw_target.as_deref().unwrap_or(""))
        );
        if let Some(f) = &format {
            u.push_str(&format!("&format={}", q(f)));
        }
        if all_panes {
            u.push_str("&all=1");
        }
        match http_get(u, token) {
            Some((200, body)) => {
                println!("{body}");
                return Ok(());
            }
            Some((409, _)) => {} // 命中 GUI 会话 → 回退 GUI 路径
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
    }

    if let Some(fmt) = format {
        if all_panes {
            println!("{}", render_tmux_format_dynamic(&fmt, 0, url, token));
        } else {
            println!("{}", render_tmux_format_dynamic(&fmt, pane_index, url, token));
        }
        return Ok(());
    }

    let u = format!("{}/api/v1/list-panes", url.trim_end_matches('/'));
    let res = client()
        .get(u)
        .headers(auth_headers(token))
        .send()
        .map_err(|e| eprintln!("tmux: {e}"))?;
    if !res.status().is_success() {
        eprintln!("tmux: list-panes {}", res.status());
        return Err(());
    }
    let text = res.text().map_err(|e| eprintln!("tmux: {e}"))?;
    print!("{text}");
    Ok(())
}

// ========== Pane Management Commands ==========

fn cmd_select_pane(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // 会话限定目标 / 自定义 socket → native select（绝不落到 GUI，保护其他工作区）。
    let raw_t = extract_target(rest);
    if use_native(raw_t.as_deref()) {
        if let Some(r) = native_select(url, token, raw_t.as_deref().unwrap_or("")) {
            return r;
        }
    }
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

    // Direction flags: -l (last) is the only one Claude Code uses for pane management.
    // Spatial directions (-L/-R/-U/-D) are no-ops since Ridge's layout is tracked server-side.
    if let Some(dir) = direction {
        if dir == "last" {
            let u = format!("{}/api/v1/select-pane", url.trim_end_matches('/'));
            let body = serde_json::json!({ "last": true });
            let _ = client()
                .post(&u)
                .headers(auth_headers(token))
                .json(&body)
                .send();
        }
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
        .map_err(|e| eprintln!("tmux: {e}"))?;
    if !res.status().is_success() {
        // Don't fail - just acknowledge for compatibility
    }
    Ok(())
}

fn cmd_kill_pane(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // 会话限定目标 / 自定义 socket → native kill-pane。**必须**先于 GUI 路径：否则
    // `-L sock kill-pane` 会误杀 GUI 工作区里的真实面板。
    let raw_t = extract_target(rest);
    if use_native(raw_t.as_deref()) {
        if let Some(r) = native_kill(url, token, raw_t.as_deref().unwrap_or(""), "pane") {
            return r;
        }
    }
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

    log_to_file(&format!("kill-pane: pane={:?}, kill_all={}", pane_index, kill_all));

    // -a (kill all panes) is intentionally a no-op in Ridge: there is no
    // "kill all" concept that maps cleanly, and Claude Code rarely issues it.
    if kill_all {
        return Ok(());
    }

    // Route the kill through the teammate HTTP API so Ridge removes the pane
    // from its layout, tears down the PTY, and emits teammate-layout-changed.
    // Without this the pane lingers as a zombie after the agent exits.
    let u = format!("{}/api/v1/kill-pane", url.trim_end_matches('/'));
    let body = match pane_index {
        Some(idx) => serde_json::json!({ "pane_index": idx }),
        None => serde_json::json!({}),
    };
    let res = client()
        .post(&u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| {
            log_to_file(&format!("kill-pane HTTP error: {e}"));
        })?;
    if !res.status().is_success() {
        log_to_file(&format!("kill-pane HTTP {} from server", res.status()));
    }
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

    log_to_file(&format!(
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
    log_to_file(&format!("swap-pane: source={:?}, dest={:?}", source_pane, dest_pane));
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
    // `horizontal` is parsed from `-h` so future wiring can ask the backend
    // for the right split direction. It's unused today; include it in the
    // debug log so the compiler doesn't flag the assignment as dead.
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
    log_to_file(&format!(
        "join-pane: source={:?}, target={:?}, horizontal={}",
        source_pane, target_window, horizontal
    ));
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
    log_to_file(&format!("respawn-pane: pane={:?}, command={:?}", pane_index, command));
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
    // Pipe pane output - not supported in Ridge
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
    let mut pane_index: Option<usize> = None;
    let mut raw_target: Option<String> = None;
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
                let t = rest[i + 1].trim();
                raw_target = Some(t.to_string());
                // Only treat as pane index if numeric or %N — session names are ignored
                if t.parse::<usize>().is_ok() || t.starts_with('%') {
                    pane_index = Some(parse_pane_target(t));
                }
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

    log_to_file(&format!(
        "new-window: name={window_name:?}, cwd={cwd:?}, target={raw_target:?}, socket={}",
        socket()
    ));

    // 会话限定目标 / 自定义 socket → native new-window（在该会话里新建窗口+面板）。
    if use_native(raw_target.as_deref()) {
        let shell = resolve_shell();
        let body = serde_json::json!({
            "socket": socket(),
            "target": raw_target.clone().unwrap_or_default(),
            "new_window": true,
            "window_name": window_name.clone(),
            "cwd": cwd.clone(),
            "shell": shell,
            "command": command.clone(),
            "print": false,
        });
        match http_post(tmux_api(url, "split-window"), token, body) {
            Some((200, out)) => {
                if !out.is_empty() {
                    println!("{out}");
                }
                return Ok(());
            }
            Some((409, _)) => {}
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
    }

    // GUI 路径：复用空闲 shell / 在发起方工作区最大 pane 上 split（后端处理）。
    let new_idx = post_split(url, token, false, pane_index, command, cwd, None)?;
    if let Some(name) = &window_name {
        rename_pane_http(url, token, new_idx, name);
    }
    Ok(())
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
    log_to_file(&format!("select-window: index={:?}", window_index));
    Ok(())
}

fn cmd_kill_window(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let raw_t = extract_target(rest);
    if use_native(raw_t.as_deref()) {
        if let Some(r) = native_kill(url, token, raw_t.as_deref().unwrap_or(""), "window") {
            return r;
        }
    }
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
    log_to_file(&format!("kill-window: index={:?}", window_index));
    Ok(())
}

fn cmd_rename_window(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane_index: Option<usize> = None;
    let mut new_name: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            _ => {
                new_name = Some(rest[i..].join(" "));
                break;
            }
        }
        i += 1;
    }
    log_to_file(&format!("rename-window: pane={:?}, name={:?}", pane_index, new_name));

    let name = new_name.unwrap_or_default();
    let u = format!("{}/api/v1/rename-pane", url.trim_end_matches('/'));
    let body = match pane_index {
        Some(idx) => serde_json::json!({ "pane_index": idx, "name": name }),
        None => serde_json::json!({ "name": name }),
    };
    let _ = client()
        .post(&u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| log_to_file(&format!("rename-window HTTP error: {e}")));
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
    log_to_file(&format!("move-window: source={:?}, dest={:?}", source_index, dest_index));
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
    log_to_file(&format!("select-layout: window={:?}, layout={:?}", window_index, layout));
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

fn cmd_new_session(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // 具名/后台会话一律落到 native 引擎（无头 PTY，按 socket 隔离）。
    let mut name: Option<String> = None;
    let mut window_name: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut width: u16 = 80;
    let mut height: u16 = 24;
    let mut attach_or_create = false;
    let mut print = false;
    let mut print_format: Option<String> = None;
    let mut cmd_start: Option<usize> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-d" => {}
            "-A" => attach_or_create = true,
            "-P" => print = true,
            "-s" if i + 1 < rest.len() => {
                name = Some(rest[i + 1].clone());
                i += 1;
            }
            "-n" if i + 1 < rest.len() => {
                window_name = Some(rest[i + 1].clone());
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                cwd = Some(rest[i + 1].clone());
                i += 1;
            }
            "-x" if i + 1 < rest.len() => {
                width = rest[i + 1].parse().unwrap_or(width);
                i += 1;
            }
            "-y" if i + 1 < rest.len() => {
                height = rest[i + 1].parse().unwrap_or(height);
                i += 1;
            }
            "-F" if i + 1 < rest.len() => {
                print_format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-e" | "-t" | "-T" if i + 1 < rest.len() => {
                i += 1; // env / group target，忽略取值
            }
            "--" => {
                i += 1;
                if i < rest.len() {
                    cmd_start = Some(i);
                }
                break;
            }
            s if s.starts_with('-') => {}
            _ => {
                cmd_start = Some(i);
                break;
            }
        }
        i += 1;
    }
    let command = cmd_start
        .map(|j| rest[j..].join(" "))
        .filter(|s| !s.is_empty());
    let shell = resolve_shell();
    log_to_file(&format!("new-session: name={name:?} {width}x{height} socket={}", socket()));

    let body = serde_json::json!({
        "socket": socket(),
        "name": name,
        "window_name": window_name,
        "cwd": cwd,
        "width": width,
        "height": height,
        "shell": shell,
        "command": command,
        "attach_or_create": attach_or_create,
        "print": print,
        "print_format": print_format,
    });
    match http_post(tmux_api(url, "new-session"), token, body) {
        Some((200, out)) => {
            if !out.is_empty() {
                println!("{out}");
            }
            Ok(())
        }
        Some((_, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Err(())
        }
        None => {
            eprintln!("tmux: new-session: server unreachable");
            Err(())
        }
    }
}

fn cmd_has_session(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let target = extract_target(rest).unwrap_or_default();
    log_to_file(&format!("has-session: target={target:?} socket={}", socket()));
    let u = format!(
        "{}?socket={}&target={}",
        tmux_api(url, "has-session"),
        q(socket()),
        q(&target)
    );
    match http_get(u, token) {
        Some((200, _)) => Ok(()),
        Some((_, msg)) => {
            // 存在退 0、不存在退非 0（并把错误打到 stderr）。
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Err(())
        }
        None => Err(()),
    }
}

fn cmd_list_sessions(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    // 合并 GUI 工作区会话 + 该 socket 的 native 会话，遵循 `-F`。
    let format = extract_f(rest);
    let mut u = format!("{}?socket={}", tmux_api(url, "list-sessions"), q(socket()));
    if let Some(f) = &format {
        u.push_str(&format!("&format={}", q(f)));
    }
    match http_get(u, token) {
        Some((200, body)) => {
            if !body.is_empty() {
                println!("{body}");
            }
            Ok(())
        }
        Some((_, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Err(())
        }
        None => {
            eprintln!("tmux: list-sessions: server unreachable");
            Err(())
        }
    }
}

/// `attach`（改造语义）：不在终端里接管渲染，而是通知 Ridge 把目标 native 会话
/// **召唤进发起方工作区的 GUI 分屏**（实时可见可交互）。自定义 socket / 会话限定
/// 目标走 summon；默认 socket 非会话目标维持 no-op（harness 兼容）。
fn cmd_attach_session(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let raw_target = extract_target(rest);
    if use_native(raw_target.as_deref()) {
        let body = serde_json::json!({
            "socket": socket(),
            "target": raw_target.as_deref().unwrap_or(""),
        });
        match http_post(tmux_api(url, "summon"), token, body) {
            Some((200, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Ok(());
            }
            Some((409, _)) => {} // 命中 GUI 会话 → 回退（无操作）
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
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

fn cmd_kill_session(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let target = extract_target(rest).unwrap_or_default();
    log_to_file(&format!("kill-session: target={target:?} socket={}", socket()));
    let body = serde_json::json!({ "socket": socket(), "target": target, "scope": "session" });
    match http_post(tmux_api(url, "kill"), token, body) {
        Some((200, _)) => Ok(()),
        // 409 = 命中 GUI 会话（如 ridge）：保守 no-op，保护用户真实会话。
        Some((409, _)) => Ok(()),
        Some((_, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Err(())
        }
        None => Err(()),
    }
}

fn cmd_kill_server(url: &str, token: &str) -> Result<(), ()> {
    log_to_file(&format!("kill-server socket={}", socket()));
    // 自定义 socket：清空该 socket 上的 native 会话（独立 server 语义）。
    // 默认 socket：保守 no-op —— 绝不连带销毁 GUI 工作区/真实会话。
    if socket() == "default" {
        return Ok(());
    }
    let body = serde_json::json!({ "socket": socket(), "scope": "server" });
    match http_post(tmux_api(url, "kill"), token, body) {
        Some((200, _)) => Ok(()),
        Some((_, msg)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            Err(())
        }
        None => Err(()),
    }
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
    let mut raw_target: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-F" if i + 1 < rest.len() => {
                format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                raw_target = Some(rest[i + 1].clone());
                i += 1;
            }
            "-a" => {} // all sessions
            _ => {}
        }
        i += 1;
    }

    // 会话限定目标 / 自定义 socket → native list-windows。
    if use_native(raw_target.as_deref()) {
        let mut u = format!(
            "{}?socket={}&target={}",
            tmux_api(url, "list-windows"),
            q(socket()),
            q(raw_target.as_deref().unwrap_or(""))
        );
        if let Some(f) = &format {
            u.push_str(&format!("&format={}", q(f)));
        }
        match http_get(u, token) {
            Some((200, body)) => {
                println!("{body}");
                return Ok(());
            }
            Some((409, _)) => {}
            Some((_, msg)) => {
                if !msg.is_empty() {
                    eprintln!("{msg}");
                }
                return Err(());
            }
            None => return Err(()),
        }
    }

    if let Some(fmt) = format {
        println!(
            "{}",
            render_tmux_format(&fmt, current_pane_index_from_env())
        );
        return Ok(());
    }

    // Return default format
    println!("0: ridge* (1 panes) [80x24] @0 (active)");
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
        println!("{}", fmt.replace("#{client_tty}", "").replace("#{client_session_name}", "ridge"));
        return Ok(());
    }

    // No clients attached
    Ok(())
}

fn cmd_list_keys(rest: &[String]) -> Result<(), ()> {
    // `format` / `-T` / `-N` are accepted but not consumed — this is a no-op
    // compatibility stub for `tmux list-keys`. Ridge has no key-binding map to
    // report; real tmux callers just need the command to exit 0.
    let _format: Option<String> = None;
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

fn cmd_list_commands(_rest: &[String]) -> Result<(), ()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn global_flags_default_socket() {
        let (s, i) = parse_global_flags(&args(&["tmux", "new-session", "-d"]));
        assert_eq!(s, "default");
        assert_eq!(i, 1);
    }

    #[test]
    fn global_flags_dash_l() {
        let (s, i) = parse_global_flags(&args(&["tmux", "-L", "mysock", "new-session"]));
        assert_eq!(s, "L:mysock");
        assert_eq!(i, 3);
    }

    #[test]
    fn global_flags_dash_s_with_switch() {
        let (s, i) = parse_global_flags(&args(&["tmux", "-2", "-S", "/tmp/x.sock", "ls"]));
        assert_eq!(s, "S:/tmp/x.sock");
        assert_eq!(i, 4);
    }

    #[test]
    fn session_qualified_cases() {
        assert!(target_is_session_qualified("=probe"));
        assert!(target_is_session_qualified("probe"));
        assert!(target_is_session_qualified("probe.0"));
        assert!(target_is_session_qualified("probe:1.2"));
        assert!(!target_is_session_qualified("%3"));
        assert!(!target_is_session_qualified("0"));
        assert!(!target_is_session_qualified(""));
        assert!(!target_is_session_qualified(":1.2"));
        assert!(!target_is_session_qualified(".1"));
    }

    #[test]
    fn pane_target_forms() {
        assert_eq!(parse_pane_target("%2"), 2);
        assert_eq!(parse_pane_target("sess:1.3"), 3);
        assert_eq!(parse_pane_target("4"), 4);
    }
}
