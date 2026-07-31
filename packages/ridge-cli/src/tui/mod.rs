//! 交互式 TUI（E2）。passthrough 模型：raw 模式 + alt screen 下，把会话输出字节
//! 原样写到本地终端（本地终端原生渲染 ANSI），把本地按键编码回送会话。
//!
//! 退出热键：**Ctrl+]**（telnet 同款断开键）。
//!
//! 本轮驱动 [`LocalPtySession`]（本地 shell）；同一 [`run_session`] 主循环将无改动
//! 复用于 LAN / 公网 controller（设计文档 §E4）。

pub mod dashboard;
mod keymap;
pub mod pager;
pub mod qr_display;
mod lan_proto;
pub(crate) mod lan_session;
pub mod lan_host;
mod lan_host_impl;
mod scrollback;
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
use ridge_core::workspace::pane_tree::SplitDirection;
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

/// 无头仅启 LAN Remote（无仪表盘）：起 workspace + PTY + HTTPS host，打印根 URL/TOTP，
/// Ctrl+C 退出。供 9527 冒烟与自动化（REQ-RDG-REMOTE-CONNECT-01）。
pub async fn run_lan_host_only(port: u16, shell: Option<String>, cwd: Option<String>) -> Result<()> {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::config;
    use crate::totp::RemoteTotp;

    let port = if port == 0 { config::lan_port() } else { port };
    let lan_ip = config::detect_lan_ip();
    let workspace = workspace::new_shared();
    {
        let mut w = workspace.lock().map_err(|e| anyhow!("workspace lock: {e}"))?;
        w.create_session(shell.as_deref(), cwd.as_deref(), None, SplitDirection::Horizontal)
            .context("create initial pane")?;
    }
    let totp = Arc::new(RemoteTotp::new());
    let totp_ui = totp.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    eprintln!("rdg LAN Remote host (headless)");
    // 本机浏览器优先 127.0.0.1：系统/终端若设了 https_proxy（如 127.0.0.1:51081），
    // 访问 https://192.168.x.x:9527 会走代理隧道 → 超时，表现为「像没启动」。
    // no_proxy 通常只含 localhost/127.0.0.1，不含局域网 IP。
    eprintln!("  本机打开: https://127.0.0.1:{port}   ← 优先用这个");
    eprintln!("  局域网  : https://{lan_ip}:{port}   （他机；本机若开了系统代理会失败）");
    eprintln!(
        "  TOTP: {} (every {}s)",
        totp_ui.current_code(),
        RemoteTotp::period_secs()
    );
    eprintln!("  证书自签：浏览器选「高级 → 继续访问」；不要用 http://");
    if std::env::var_os("https_proxy").is_some()
        || std::env::var_os("HTTPS_PROXY").is_some()
        || std::env::var_os("ALL_PROXY").is_some()
    {
        eprintln!(
            "  警告: 检测到 https_proxy/ALL_PROXY。本机请用 127.0.0.1，或把 {lan_ip} 加入 no_proxy。"
        );
    }
    eprintln!("  Ctrl+C to stop");

    let host = tokio::spawn(async move {
        if let Err(e) = lan_host::run(port, totp, workspace, shutdown_rx).await {
            eprintln!("LAN host exited: {e}");
        }
    });

    // 等 TCP 真正 LISTEN 再写 status（E2E 只认 ready=true，避免「文件在、端口未开」）。
    let ready = wait_tcp_ready(port, Duration::from_secs(15)).await;
    if host.is_finished() || !ready {
        let _ = host.await;
        clear_lan_host_status();
        return Err(anyhow!(
            "LAN host failed to listen on port {port} (busy, TLS error, or early exit)"
        ));
    }
    write_lan_host_status(port, &lan_ip, &totp_ui.current_code(), std::process::id(), true);
    eprintln!("  ready: listening on {port}");

    let mut last_code = totp_ui.current_code();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprintln!("stopping LAN host…");
                let _ = shutdown_tx.send(());
                let _ = host.await;
                clear_lan_host_status();
                return Ok(());
            }
            _ = tick.tick() => {
                if host.is_finished() {
                    let _ = host.await;
                    clear_lan_host_status();
                    return Err(anyhow!("LAN host exited unexpectedly"));
                }
                let code = totp_ui.current_code();
                if code != last_code {
                    last_code = code.clone();
                    eprintln!("  TOTP: {code}");
                }
                write_lan_host_status(port, &lan_ip, &code, std::process::id(), true);
            }
        }
    }
}

async fn wait_tcp_ready(port: u16, budget: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + budget;
    while tokio::time::Instant::now() < deadline {
        match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
            Ok(_) => return true,
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
    false
}

/// E2E / agent 可读的 LAN host 状态路径（项目根 `.ridge/lan-host-status.json`）。
fn lan_host_status_path() -> std::path::PathBuf {
    std::env::var_os("RIDGE_LAN_STATUS_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(".ridge/lan-host-status.json"))
}

fn write_lan_host_status(port: u16, lan_ip: &str, totp: &str, pid: u32, ready: bool) {
    let path = lan_host_status_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = serde_json::json!({
        "schema_version": 1,
        "ready": ready,
        "pid": pid,
        "port": port,
        "lan_ip": lan_ip,
        "url_loopback": format!("https://127.0.0.1:{port}"),
        "url_lan": format!("https://{lan_ip}:{port}"),
        "totp": totp,
        "period_secs": crate::totp::RemoteTotp::period_secs(),
        "updated_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    let _ = std::fs::write(&path, body.to_string());
}

fn clear_lan_host_status() {
    let _ = std::fs::remove_file(lan_host_status_path());
}

/// 启动多会话 Pager TUI：创建 N 个本地 shell，支持 Ctrl+Shift+方向 切换 pane。
pub async fn run_local_pager(
    shell: Option<String>,
    cwd: Option<String>,
    session_count: usize,
) -> Result<()> {
    let count = session_count.max(1).min(12);
    eprintln!("rdg 交互式终端（{count} 会话）。Ctrl+Shift+方向键 切换 pane，Ctrl+] 退出。");

    let mut mgr = workspace::WorkspaceManager::new(workspace::new_shared());
    // 首个 session
    {
        let mut ws = mgr.active_workspace_mut();
        let label = cwd.as_deref().unwrap_or("shell").to_string();
        ws.create_session(shell.as_deref(), cwd.as_deref(), None, SplitDirection::Horizontal)?;
        if let Some(s) = ws.sessions.last_mut() {
            s.title = label;
        }
    }
    // 后续 session：依次 split 前一个
    for _ in 1..count {
        let id = mgr.split_active_session(
            shell.as_deref(),
            cwd.as_deref(),
            SplitDirection::Horizontal,
        )?;
        let idx = {
            let ws = mgr.active_workspace_mut();
            ws.sessions.iter().position(|s| s.id == id)
        };
        if let Some(i) = idx {
            let mut ws = mgr.active_workspace_mut();
            ws.sessions[i].title = format!("shell-{}", ws.sessions.len());
        }
    }
    pager::run_pager(&mut mgr).await
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
