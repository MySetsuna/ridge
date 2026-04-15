//! wind-tmux 文件日志：诊断与 HTTP 错误不写终端，避免 Claude / teammate 解析 PTY 时误吞。
//! 协议性 stdout 仍原样输出，并镜像到日志（标注 `[OUT]`）。
//!
//! - `WIND_TMUX_LOG`：日志文件路径（追加）。未设置时使用 `{LOCALDATA}/wind/wind-tmux-shim.log`（无 dirs 时回退到临时目录）。
//! - 不在日志中写入 `WIND_TEAMMATE_TOKEN`；`[CMD]` 仅记录 argv。

use chrono::Local;
use parking_lot::Mutex;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

struct LogState {
    #[allow(dead_code)]
    path: PathBuf,
    file: Option<std::fs::File>,
}

static LOG: Mutex<Option<LogState>> = Mutex::new(None);

fn default_log_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("wind")
        .join("wind-tmux-shim.log")
}

fn resolve_log_path() -> PathBuf {
    std::env::var("WIND_TMUX_LOG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_log_path)
}

/// 须在其它 shim_log 调用之前执行一次。
pub fn init() {
    let path = resolve_log_path();
    if let Some(dir) = path.parent() {
        let _ = create_dir_all(dir);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let mut g = LOG.lock();
    *g = Some(LogState {
        path: path.clone(),
        file,
    });
    write_tag("[INIT]", &format!("log_path={}", path.display()));
}

fn write_tag(tag: &str, msg: &str) {
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let mut g = LOG.lock();
    let Some(state) = g.as_mut() else {
        return;
    };
    let Some(f) = state.file.as_mut() else {
        return;
    };
    if msg.is_empty() {
        let _ = writeln!(f, "{ts} {tag} ");
    } else {
        for line in msg.lines() {
            let _ = writeln!(f, "{ts} {tag} {line}");
        }
        if msg.ends_with('\n') {
            let _ = writeln!(f, "{ts} {tag} ");
        }
    }
    let _ = f.flush();
}

/// 完整命令行（argv），不含环境变量中的 token。
pub fn cmd_argv(args: &[String]) {
    write_tag("[CMD]", &join_argv(args));
}

fn join_argv(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.chars().any(|c| c.is_whitespace()) {
                format!("{a:?}")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn err(msg: &str) {
    write_tag("[ERR]", msg);
}

pub fn warn(msg: &str) {
    write_tag("[WARN]", msg);
}

pub fn help(msg: &str) {
    write_tag("[HELP]", msg);
}

pub fn http_fail(op: &str, status: impl std::fmt::Display) {
    write_tag("[HTTP]", &format!("{op} status={status}"));
}

/// tmux 协议单行 stdout：照常打印，并写入 `[OUT]`。
pub fn out_line(line: &str) {
    println!("{line}");
    write_tag("[OUT]", line);
}

/// `println!` 整块（可含多行）：与 tmux 行为一致，并逐行写 `[OUT]`。
pub fn out_lines_body(body: &str) {
    println!("{body}");
    write_tag("[OUT]", "--- stdout block begin ---");
    for line in body.lines() {
        write_tag("[OUT]|", line);
    }
    write_tag("[OUT]", "--- stdout block end ---");
}

/// 无末尾换行的原始 stdout（如 capture-pane / list-panes 纯文本）。
pub fn out_raw(text: &str) {
    print!("{text}");
    write_tag("[OUT]", &format!("(raw stdout, {} bytes)", text.len()));
    for line in text.lines() {
        write_tag("[OUT]|", line);
    }
    if text.ends_with('\n') {
        write_tag("[OUT]|", "");
    }
}

pub fn exit_status(ok: bool) {
    write_tag(
        "[EXIT]",
        if ok {
            "status=0 ok"
        } else {
            "status=1 err"
        },
    );
}

// --- Claude Code 方向性审计（便于在日志中 grep `[wind-claude-code]`）---

/// 控制字符与换行折叠，避免污染日志；仅用于 send-keys 等预览。
pub fn sanitize_preview(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in s.chars().take(max_chars) {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// Claude 经 tmux shim **注入** 窗格侧（send-keys、split-window 等）。
pub fn claude_code_send(detail: &str) {
    write_tag("[wind-claude-code][send]", detail);
}

/// Claude 经 tmux shim **读取** 窗格/布局侧（capture-pane、list-panes、display-message 等）。
pub fn claude_code_recv(detail: &str) {
    write_tag("[wind-claude-code][recv]", detail);
}
