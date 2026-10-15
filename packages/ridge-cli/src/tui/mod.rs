//! 交互式 TUI：共同 Host 的规范化语义帧形成展示镜像，本地终端渲染展示序列。
//!
//! 退出热键：**Ctrl+]**（telnet 同款断开键）。
//!
//! 本地与 LAN 共用会话、输入校验和有界输出通道。

pub mod dashboard;
mod keymap;
mod canonical_view;
mod performance;
mod lan_proto;
pub(crate) mod lan_session;
pub mod pager;
pub mod qr_display;
mod session;

pub use session::Session;

use std::io::Write;

use anyhow::{anyhow, Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, terminal};
use futures_util::StreamExt;
use tokio::sync::mpsc;

pub(crate) fn write_presentation(output:&mut impl Write,bytes:&[u8])->Result<()>{
    let started=performance::begin();
    let result=output.write_all(bytes).and_then(|_|output.flush());
    performance::finish("stdout",started,bytes.len(),0);
    if result.is_err(){performance::error("CLI_STDOUT_FAILED");}
    result.map_err(Into::into)
}

/// 退出热键：Ctrl+]。
fn is_quit(ev: &KeyEvent) -> bool {
    ev.modifiers.contains(KeyModifiers::CONTROL) && matches!(ev.code, KeyCode::Char(']'))
}

pub(crate) fn history_direction(key:&KeyEvent)->Option<bool>{
    if key.modifiers!=(KeyModifiers::CONTROL|KeyModifiers::SHIFT){return None;}
    match key.code{KeyCode::PageUp=>Some(true),KeyCode::PageDown=>Some(false),_=>None}
}

pub(crate) async fn wait_control<T>(action:impl std::future::Future<Output=Result<T>>,events:&mut EventStream,waiting:&str,mut resize:impl FnMut(u16,u16)->Result<()>)->Result<Option<T>>{
    tokio::pin!(action);
    loop{tokio::select!{
        result=&mut action=>return result.map(Some),
        event=events.next()=>match event{
            Some(Ok(Event::Key(key))) if matches!(key.kind,KeyEventKind::Release)=>{},
            Some(Ok(Event::Key(key))) if is_quit(&key)=>return Ok(None),
            Some(Ok(Event::Key(_)))=>eprintln!("\r\n{waiting}，按键未发送。\r"),
            Some(Ok(Event::Resize(cols,rows)))=>resize(cols,rows)?,
            Some(Err(error))=>return Err(error.into()),
            None=>return Ok(None),
            _=>{},
        },
    }}
}

/// 启动本地交互式 TUI，展示共同 Host 的规范化语义帧。
pub async fn run_local(shell: Option<String>, cwd: Option<String>) -> Result<()> {
    eprintln!("ridge 交互式终端（本地 shell）。Ctrl+Shift+PgUp/PgDn 浏览历史，Ctrl+] 退出。");
    let (sess, rx) = lan_session::connect_local(shell,cwd).await?;
    run_session(sess, rx).await
}

/// 无头仅启 LAN Remote（无仪表盘）：起 workspace + PTY + HTTPS host，打印根 URL/TOTP，
/// Ctrl+C 退出。供 9527 冒烟与自动化（REQ-RDG-REMOTE-CONNECT-01）。
pub async fn run_lan_host_only(
    port: u16,
    shell: Option<String>,
    cwd: Option<String>,
) -> Result<()> {
    use std::time::Duration;

    use crate::config;
    use crate::totp::RemoteTotp;

    let requested_port = if port == 0 { config::lan_port() } else { port };
    let endpoint=crate::kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
    lan_session::spawn_local_terminal(&endpoint,shell.as_deref(),cwd.as_deref()).context("create initial pane")?;
    let totp = ridge_kernel::auth_client::remote_auth(endpoint);
    totp.call("remote_set_totp_identity",serde_json::json!({"username":config::totp_identity()})).map_err(anyhow::Error::msg)?;
    clear_lan_host_status()?;
    totp.call("set_remote_enabled",serde_json::json!({"enabled":true,"port":requested_port})).map_err(anyhow::Error::msg)?;
    let info=crate::host::lan_info(&totp)?;
    let port=info.port;let lan_ip=info.lan_ip;let scheme=if info.tls{"https"}else{"http"};

    eprintln!("ridge LAN Remote host (headless)");
    // 本机浏览器优先 127.0.0.1：系统/终端若设了 https_proxy（如 127.0.0.1:51081），
    // 访问 https://192.168.x.x:9527 会走代理隧道 → 超时，表现为「像没启动」。
    // no_proxy 通常只含 localhost/127.0.0.1，不含局域网 IP。
    eprintln!("  本机打开: {scheme}://127.0.0.1:{port}   ← 优先用这个");
    eprintln!("  局域网  : {scheme}://{lan_ip}:{port}   （他机；本机若开了系统代理会失败）");
    eprintln!(
        "  TOTP: {} (every {}s)",
        info.totp_code,
        RemoteTotp::period_secs()
    );
    if info.tls{eprintln!("  证书自签：浏览器选「高级 → 继续访问」；不要用 http://");}
    if std::env::var_os("https_proxy").is_some()
        || std::env::var_os("HTTPS_PROXY").is_some()
        || std::env::var_os("ALL_PROXY").is_some()
    {
        eprintln!(
            "  警告: 检测到 https_proxy/ALL_PROXY。本机请用 127.0.0.1，或把 {lan_ip} 加入 no_proxy。"
        );
    }
    eprintln!("  Ctrl+C to stop");

    write_lan_host_status(
        port,
        &lan_ip,
        &info.totp_code,
        std::process::id(),
        true,
        info.tls,
    )?;
    eprintln!("  ready: listening on {port}");

    let mut last_code = info.totp_code;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let result=async {loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal?;
                eprintln!("stopping LAN host…");
                totp.call("set_remote_enabled",serde_json::json!({"enabled":false})).map_err(anyhow::Error::msg)?;
                return Ok(());
            }
            _ = tick.tick() => {
                let info=crate::host::lan_info(&totp)?;
                let code = info.totp_code;
                if code != last_code {
                    last_code = code.clone();
                    eprintln!("  TOTP: {code}");
                }
                write_lan_host_status(info.port, &info.lan_ip, &code, std::process::id(), true,info.tls)?;
            }
        }
    }}.await;
    result.and(clear_lan_host_status())
}

/// E2E / agent 可读的 LAN host 状态路径（项目根 `.ridge/lan-host-status.json`）。
fn lan_host_status_path() -> std::path::PathBuf {
    std::env::var_os("RIDGE_LAN_STATUS_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(".ridge/lan-host-status.json"))
}

fn write_lan_host_status(port: u16, lan_ip: &str, totp: &str, pid: u32, ready: bool,tls:bool)->Result<()> {
    let path = lan_host_status_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::json!({
        "schema_version": 1,
        "ready": ready,
        "pid": pid,
        "port": port,
        "lan_ip": lan_ip,
        "url_loopback": format!("{}://127.0.0.1:{port}",if tls{"https"}else{"http"}),
        "url_lan": format!("{}://{lan_ip}:{port}",if tls{"https"}else{"http"}),
        "totp": totp,
        "period_secs": crate::totp::RemoteTotp::period_secs(),
        "updated_at_unix": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    std::fs::write(&path, body.to_string())?;Ok(())
}

fn clear_lan_host_status()->Result<()> {
    match std::fs::remove_file(lan_host_status_path()){
        Ok(())=>Ok(()),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(error)=>Err(error.into()),
    }
}

/// 启动多会话 Pager TUI：创建 N 个本地 shell，支持 Ctrl+Shift+方向 切换 pane。
pub async fn run_local_pager(
    shell: Option<String>,
    cwd: Option<String>,
    session_count: usize,
) -> Result<()> {
    let count = session_count.max(1).min(12);
    eprintln!("ridge 交互式终端（{count} 会话）。Ctrl+Shift+方向键 切换 pane，Ctrl+F1..F12 切换工作区，Ctrl+Shift+PgUp/PgDn 浏览历史，Ctrl+] 退出。");

    pager::run_pager(shell,cwd,count).await
}

/// 启动 LAN 控制端 TUI：连接共同 Host，展示规范化语义帧。
pub async fn run_lan(host: String, code: Option<String>, token: Option<String>) -> Result<()> {
    eprintln!("ridge 远程控制台（LAN）→ {host}。Ctrl+Shift+PgUp/PgDn 浏览历史，Ctrl+] 退出。连接中…");
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

    eprintln!("ridge LAN 自检 → {host}");
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
                        if let Some(back)=history_direction(&k){
                            let history=async{
                                let request=sess.scroll_history(back);tokio::pin!(request);
                                loop{tokio::select!{
                                    result=&mut request=>return result,
                                    chunk=out_rx.recv()=>if chunk.is_none(){return Err(anyhow!(sess.failure().unwrap_or_else(||"CLI connection closed".into())));},
                                }}
                            };
                            match wait_control(history,&mut events,"正在加载历史",|cols,rows|sess.resize(cols,rows)).await{
                                Ok(Some(Some(bytes)))=>write_presentation(&mut std::io::stdout(),&bytes)?,
                                Ok(None)=>break,
                                Ok(Some(None))=>{},
                                Err(error)=>eprintln!("\r\n历史不可用：{error}\r"),
                            }
                            continue;
                        }
                        if let Some(bytes) = keymap::encode_key(&k) {
                            if !sess.input_ready() {
                                eprintln!("\r\n会话尚未接通，按键未发送。\r");
                                continue;
                            }
                            if let Some(bytes)=sess.follow_tail(){write_presentation(&mut std::io::stdout(),&bytes)?;}
                            sess.send_input(&bytes)?;
                        }
                    }
                    Some(Ok(Event::Resize(cols, rows))) => {
                        let _ = sess.resize(cols, rows);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e).context("读取终端事件失败"),
                    None => {
                        if let Some(error) = sess.failure() { return Err(anyhow!(error)); }
                        break;
                    },
                }
            }
            chunk = out_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        if let Some(error)=sess.failure(){return Err(anyhow!(error));}
                        // Repaint the current view: queued live frames may precede a history gesture.
                        let bytes=if bytes.starts_with(b"\x1b[?25l"){sess.presentation().unwrap_or(bytes)}else{bytes};
                        let mut so = std::io::stdout();
                        write_presentation(&mut so,&bytes).context("写终端失败")?;
                    }
                    // 输出流关闭 = 会话结束（本地 shell 退出 / 远端断开）。
                    None => {
                        if let Some(error) = sess.failure() { return Err(anyhow!(error)); }
                        break;
                    },
                }
            }
        }
    }
    Ok(())
}
