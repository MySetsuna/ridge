//! 交互式 TUI（E2）。passthrough 模型：raw 模式 + alt screen 下，把会话输出字节
//! 原样写到本地终端（本地终端原生渲染 ANSI），把本地按键编码回送会话。
//!
//! 退出热键：**Ctrl+]**（telnet 同款断开键）。
//!
//! 本轮驱动 [`LocalPtySession`]（本地 shell）；同一 [`run_session`] 主循环将无改动
//! 复用于 LAN / 公网 controller（设计文档 §E4）。

pub mod dashboard;
mod keymap;
pub mod qr_display;
mod lan_proto;
pub(crate) mod lan_session;
pub mod lan_host;
mod session;
mod workspace;

pub use session::{LocalPtySession, Session};

use std::io::Write;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, terminal};
use futures_util::StreamExt;
use tokio::sync::mpsc;

/// 退出热键：Ctrl+]。
fn is_quit(ev: &KeyEvent) -> bool {
    ev.modifiers.contains(KeyModifiers::CONTROL) && matches!(ev.code, KeyCode::Char(']'))
}

/// 启动本地交互式 TUI（passthrough 本地 shell）。
pub async fn run_local(shell: Option<String>, cwd: Option<String>) -> Result<()> {
    eprintln!("rdg 交互式终端（本地 shell）。按 Ctrl+] 退出。");
    let (sess, rx) = LocalPtySession::spawn(shell.as_deref(), cwd.as_deref())?;
    run_session(sess, rx).await
}

/// 启动 LAN 控制端 TUI（E4）：连桌面 host、订阅 pane、passthrough 进同一界面。
pub async fn run_lan(host: String, code: Option<String>, token: Option<String>) -> Result<()> {
    eprintln!("rdg 远程控制台（LAN）→ {host}。连接中…（按 Ctrl+] 退出）");
    let (sess, rx) = lan_session::connect_lan(&host, code, token).await?;
    run_session(sess, rx).await
}

/// 无头协议自检（E4）：连接→握手→订阅→回显校验，不进入 raw 模式。便于在非 TTY
/// 环境（CI / 本工具链）对真实桌面 host 验证 Rust 驱动本身（TLS/握手/帧）跑通。
pub async fn run_lan_probe(
    host: String,
    code: Option<String>,
    token: Option<String>,
    seconds: u64,
) -> Result<()> {
    use tokio::time::{sleep, timeout, Duration, Instant};

    eprintln!("rdg LAN 自检 → {host}");
    let (sess, mut rx) = lan_session::connect_lan(&host, code, token).await?;

    // 等握手订阅到 pane（最多 5s）。
    let mut pane = None;
    for _ in 0..50 {
        if let Some(p) = sess.current_pane() {
            pane = Some(p);
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    let pane = pane.ok_or_else(|| anyhow!("超时未订阅到 pane（host 无可用终端？）"))?;
    eprintln!("已订阅 pane = {pane}");

    // 发一个回显标记，验证 stdin→PTY→输出 全链路。
    const MARK: &str = "RDG_RUST_OK_77";
    sess.send_input(format!("echo {MARK}\r").as_bytes())?;

    let mut buf: Vec<u8> = Vec::new();
    let mut saw = false;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(bytes)) => {
                buf.extend_from_slice(&bytes);
                if String::from_utf8_lossy(&buf).contains(MARK) {
                    saw = true;
                    break;
                }
            }
            Ok(None) => break, // 通道关闭=断开
            Err(_) => {}       // 本轮无数据，继续等
        }
    }

    eprintln!("收到 {} 字节；回显标记可见 = {}", buf.len(), saw);
    if saw {
        eprintln!("LAN 驱动自检 PASS ✅（TLS+握手+订阅+stdin 回显 全通）");
        Ok(())
    } else {
        Err(anyhow!("未见回显，自检 PARTIAL/FAIL"))
    }
}

/// 通用交互循环：任意 [`Session`] + 其输出流。负责进入/恢复终端原始模式，
/// 保证异常路径也能复位（避免把用户终端留在 raw 状态）。
pub async fn run_session<S: Session>(sess: S, out_rx: mpsc::Receiver<Vec<u8>>) -> Result<()> {
    enable_raw_mode().context("进入终端 raw 模式失败")?;
    {
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, EnterAlternateScreen);
    }

    // 初次把会话 PTY 尺寸对齐到本地终端。
    if let Ok((cols, rows)) = terminal::size() {
        let _ = sess.resize(cols, rows);
    }

    let result = event_loop(&sess, out_rx).await;

    // 复位终端（无论成功/失败/panic-free 错误）。
    {
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
    }
    let _ = disable_raw_mode();
    result
}

async fn event_loop<S: Session>(sess: &S, mut out_rx: mpsc::Receiver<Vec<u8>>) -> Result<()> {
    let mut events = EventStream::new();
    loop {
        tokio::select! {
            maybe_ev = events.next() => {
                match maybe_ev {
                    Some(Ok(Event::Key(k))) => {
                        // 仅处理按下/重复，忽略释放（Windows 控制台会发 Release）。
                        if matches!(k.kind, KeyEventKind::Release) {
                            continue;
                        }
                        if is_quit(&k) {
                            break;
                        }
                        if let Some(bytes) = keymap::encode_key(&k) {
                            sess.send_input(&bytes)?;
                        }
                    }
                    Some(Ok(Event::Resize(cols, rows))) => {
                        let _ = sess.resize(cols, rows);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e).context("读取终端事件失败"),
                    None => break,
                }
            }
            chunk = out_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        let mut so = std::io::stdout();
                        so.write_all(&bytes).context("写终端失败")?;
                        let _ = so.flush();
                    }
                    // 输出流关闭 = 会话结束（本地 shell 退出 / 远端断开）。
                    None => break,
                }
            }
        }
    }
    Ok(())
}
