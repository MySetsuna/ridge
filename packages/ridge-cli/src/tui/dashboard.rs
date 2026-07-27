use std::io::stdout;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use futures_util::StreamExt;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use crate::config;
use crate::daemon_ctl;
use crate::login_flow;
use crate::totp::RemoteTotp;
use ridge_core::workspace::pane_tree::SplitDirection;
use super::qr_display;
use super::workspace::{new_shared, SharedWorkspace};

#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Main,
    QrCode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuItem {
    ShowQrCode,
    StartLanRemote,
    StopLanRemote,
    StartDaemon,
    StopDaemon,
    Login,
    Quit,
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            MenuItem::ShowQrCode => "Show TOTP QR code",
            MenuItem::StartLanRemote => "Start LAN Remote",
            MenuItem::StopLanRemote => "Stop LAN Remote",
            MenuItem::StartDaemon => "Start public Remote",
            MenuItem::StopDaemon => "Stop public Remote",
            MenuItem::Login => "Login / activate device",
            MenuItem::Quit => "Quit",
        }
    }
}

const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::ShowQrCode,
    MenuItem::StartLanRemote,
    MenuItem::StopLanRemote,
    MenuItem::StartDaemon,
    MenuItem::StopDaemon,
    MenuItem::Login,
    MenuItem::Quit,
];

enum Action {
    RunLogin,
    Refresh,
    StartLanRemote,
    StopLanRemote,
}

/// 仪表盘登录方式：邮箱密码（默认，无浏览器环境）或浏览器授权（WSL / 远端友好）。
enum LoginMethod {
    Email,
    Browser,
}

/// 在（已退出 alternate screen / raw mode 的）普通终端里询问登录方式。
/// 回车或非 `2` 输入默认走邮箱密码登录，保留无浏览器环境下的登录能力。
fn prompt_login_method() -> LoginMethod {
    use std::io::Write;
    println!();
    println!("  选择登录方式：");
    println!("    1) 邮箱 + 密码登录（默认）");
    println!("    2) 浏览器授权登录（WSL / 远端终端友好）");
    print!("  输入 1 或 2（回车默认 1）: ");
    let _ = stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_ok() && line.trim() == "2" {
        LoginMethod::Browser
    } else {
        LoginMethod::Email
    }
}

pub struct App {
    view: View,
    selected: usize,
    auth: Option<config::AuthFile>,
    log_lines: Vec<String>,
    quit: bool,
    totp: Arc<RemoteTotp>,
    qr_text: String,
    totp_code: String,
    lan_addr: String,
    lan_running: bool,
    lan_shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    public_entry: Option<String>,
    action_tx: mpsc::UnboundedSender<Action>,
    workspace: SharedWorkspace,
    session_count: usize,
    /// 进程内云守护任务句柄。`start_daemon()` 记的 PID 就是本 TUI 自己的 PID
    /// （守护跑在本进程的 tokio 上），所以「Stop daemon」**绝不能**去 taskkill 那个
    /// PID —— 那会把整个 rdg 打死。改为 abort 这个任务。
    daemon_task: Option<tokio::task::JoinHandle<()>>,
}

impl App {
    fn new(action_tx: mpsc::UnboundedSender<Action>) -> Self {
        let auth = config::load_auth().ok().flatten();
        let lan_ip = config::detect_lan_ip();
        let port = config::lan_port();
        let totp = Arc::new(RemoteTotp::load_or_create(&config::totp_identity()));
        let totp_code = totp.current_code();
        let workspace = new_shared();
        {
            let mut w = workspace.lock().unwrap();
            let _ = w.create_session(None, None, None, SplitDirection::Horizontal);
        }
        Self {
            view: View::Main,
            selected: 0,
            auth: auth.clone(),
            log_lines: vec!["Ridge CLI v0.1.0".into()],
            quit: false,
            totp,
            qr_text: String::new(),
            totp_code,
            lan_addr: lan_origin(&lan_ip, port),
            lan_running: false,
            lan_shutdown_tx: None,
            public_entry: auth.as_ref().map(|a| a.public_entry()),
            action_tx,
            workspace,
            session_count: 1,
            daemon_task: None,
        }
    }

    fn update_totp(&mut self) {
        let new_code = self.totp.current_code();
        if new_code != self.totp_code {
            self.totp_code = new_code;
        }
        self.session_count = self.workspace.lock().unwrap().sessions.len();
    }

    fn log(&mut self, msg: String) {
        const MAX_LOG: usize = 100;
        self.log_lines.push(msg);
        if self.log_lines.len() > MAX_LOG {
            self.log_lines.remove(0);
        }
    }
}

pub async fn run() -> Result<()> {
    crate::TUI_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();
    let mut app = App::new(tx.clone());
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        while let Ok(action) = rx.try_recv() {
            match action {
                Action::Refresh => {
                    app.auth = config::load_auth().ok().flatten();
                    app.public_entry = app.auth.as_ref().map(|a| a.public_entry());
                    app.log(format!("Daemon: {}", daemon_ctl::status()));
                }
                Action::RunLogin => {
                    drop(terminal);
                    stdout().execute(LeaveAlternateScreen)?;
                    disable_raw_mode()?;

                    let client = reqwest::Client::builder().build().ok();
                    // 让用户选择登录方式：邮箱密码登录（默认，无浏览器环境）或浏览器授权
                    // 登录（纯轮询，WSL / 远端终端下登录结果也能带回本端）。仪表盘运行在
                    // 真 TTY 上，stdin 可用，故邮箱密码登录在此始终可行——保留该入口。
                    let result = if let Some(client) = client {
                        match prompt_login_method() {
                            LoginMethod::Email => login_flow::run_login(&client).await,
                            LoginMethod::Browser => login_flow::run_browser_login(&client).await,
                        }
                    } else {
                        Err(anyhow::anyhow!("无法创建 HTTP client"))
                    };

                    enable_raw_mode()?;
                    stdout().execute(EnterAlternateScreen)?;
                    let backend_new = ratatui::backend::CrosstermBackend::new(stdout());
                    terminal = Terminal::new(backend_new)?;
                    app.log(match &result {
                        Ok(_) => "Login successful".into(),
                        Err(e) => format!("Login failed: {e}"),
                    });
                    app.auth = config::load_auth().ok().flatten();
                    app.public_entry = app.auth.as_ref().map(|a| a.public_entry());
                }
                Action::StartLanRemote => {
                    if app.lan_running {
                        app.log("LAN remote already running".into());
                    } else {
                        let port = config::lan_port();
                        let totp = app.totp.clone();
                        let workspace = app.workspace.clone();
                        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
                        app.lan_shutdown_tx = Some(shutdown_tx);
                        app.lan_running = true;
                        app.log(format!("Starting LAN remote on port {port}..."));
                        tokio::spawn(async move {
                            if let Err(e) = super::lan_host::run(port, totp, workspace, shutdown_rx).await {
                                tracing::warn!(target: "ridge_cli::dashboard", error = %e, "LAN remote stopped");
                            }
                        });
                    }
                }
                Action::StopLanRemote => {
                    if let Some(tx) = app.lan_shutdown_tx.take() {
                        let _ = tx.send(());
                        app.lan_running = false;
                        app.log("LAN remote stopped".into());
                    } else {
                        app.log("LAN remote not running".into());
                    }
                }
            }
        }

        if app.quit {
            break;
        }

        terminal.draw(|f| render(f, &app))?;

        let next_event = pin!(events.next());
        tokio::select! {
            maybe_ev = next_event => {
                match maybe_ev {
                    Some(Ok(Event::Key(key))) => {
                        if key.kind == KeyEventKind::Press {
                            match app.view {
                                View::Main => handle_main_key(&mut app, key.code),
                                View::QrCode => handle_qr_key(&mut app, key.code),
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        app.log(format!("Event error: {e}"));
                    }
                    None => break,
                }
            }
            _ = rx.recv() => {}
            _ = tick.tick() => {
                app.update_totp();
            }
        }
    }

    // 退出时关闭 LAN remote
    if let Some(tx) = app.lan_shutdown_tx.take() {
        let _ = tx.send(());
    }

    drop(terminal);
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    crate::TUI_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

fn handle_main_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.selected = app.selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.selected = (app.selected + 1).min(MENU_ITEMS.len() - 1);
        }
        KeyCode::Enter => {
            let item = MENU_ITEMS[app.selected];
            match item {
                MenuItem::ShowQrCode => {
                    let device_name = app.auth.as_ref().map(|a| a.device_name.as_str()).unwrap_or("rdg");
                    let uri = app.totp.otpauth_uri(device_name);
                    let qr = qr_display::render_qr(&uri);
                    app.qr_text = format!(
                        "{qr}\n  URI: {uri}\n  验证码: {} (每 {} 秒刷新)\n\n  请用手机 Authenticator 扫描上方二维码",
                        app.totp_code,
                        RemoteTotp::period_secs(),
                    );
                    app.view = View::QrCode;
                }
                MenuItem::StartLanRemote => {
                    let _ = app.action_tx.send(Action::StartLanRemote);
                }
                MenuItem::StopLanRemote => {
                    let _ = app.action_tx.send(Action::StopLanRemote);
                }
                MenuItem::StartDaemon => {
                    // 先验激活：未激活时旧实现照样 write_pid + 报「Daemon started」，
                    // 用户以为主机已上线，控制端却永远「远程主机当前不在线」。
                    if config::load_auth().ok().flatten().is_none() {
                        app.log("本机未激活云端设备：先选 Login / activate device".into());
                    } else if app.daemon_task.is_some() {
                        app.log("云守护已在本进程内运行".into());
                    } else {
                        match daemon_ctl::start_daemon() {
                            Ok(()) => {
                                let entry = app
                                    .auth
                                    .as_ref()
                                    .map(|a| a.public_entry())
                                    .unwrap_or_default();
                                app.daemon_task = Some(tokio::spawn(async move {
                                    if let Err(e) = crate::daemon::run(None, None, None).await {
                                        // 不能 eprintln!（会糊 TUI）——落 tracing（TUI 模式写文件）。
                                        tracing::error!(target: "ridge_cli::dashboard", error = %e, "daemon exited");
                                    }
                                }));
                                app.log(format!("云守护已启动，入口 {entry}"));
                            }
                            Err(e) => app.log(format!("Start failed: {e}")),
                        }
                    }
                }
                MenuItem::StopDaemon => {
                    if let Some(task) = app.daemon_task.take() {
                        task.abort();
                        daemon_ctl::remove_pid();
                        app.log("云守护已停止".into());
                    } else {
                        // 非本进程记录的守护（外部 `rdg remote`）才走 PID 路径。
                        match daemon_ctl::stop_daemon() {
                            Ok(()) => app.log("Daemon stopped".into()),
                            Err(e) => app.log(format!("Stop failed: {e}")),
                        }
                    }
                }
                MenuItem::Login => {
                    let _ = app.action_tx.send(Action::RunLogin);
                }
                MenuItem::Quit => app.quit = true,
            }
            let _ = app.action_tx.send(Action::Refresh);
        }
        KeyCode::Char('q') => app.quit = true,
        _ => {}
    }
}

fn handle_qr_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.view = View::Main;
        }
        _ => {}
    }
}

fn render(frame: &mut Frame, app: &App) {
    match app.view {
        View::Main => render_main(frame, app),
        View::QrCode => render_qr(frame, app),
    }
}

fn render_main(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Min(1),
            Constraint::Length(8),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(Span::styled(
        "  RIDGE CLI  ",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )))
    .block(Block::default().borders(Borders::ALL).title(" Dashboard "));
    frame.render_widget(title, chunks[0]);

    let mut status_lines = vec![Line::from(format!(
        "  Public:  {}",
        daemon_ctl::status()
    ))];

    let lan_status = if app.lan_running { "Running" } else { "Stopped" };
    let lan_style = if app.lan_running {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    };
    status_lines.push(Line::from(vec![
        Span::raw("  LAN:     "),
        Span::styled(app.lan_addr.as_str(), Style::default().fg(Color::Cyan)),
        Span::raw("  ["),
        Span::styled(lan_status, lan_style),
        Span::raw("]"),
    ]));

    status_lines.push(Line::from(format!(
        "  Sessions: {} active  |  Port: {}",
        app.session_count,
        config::lan_port(),
    )));

    if let Some(auth) = &app.auth {
        status_lines.push(Line::from(format!(
            "  Device:  {}  |  User: {}",
            auth.device_name, auth.username
        )));
        status_lines.push(Line::from(format!("  Entry:   {}", auth.public_entry())));
    } else {
        status_lines.push(Line::from("  Device:  not activated"));
        status_lines.push(Line::from("  Entry:   run login first"));
    }
    let totp_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let totp_text = format!("  TOTP:    {} (每 {}s 刷新)  [Enter] 查看二维码",
        app.totp_code, RemoteTotp::period_secs());
    status_lines.push(Line::from(Span::styled(totp_text, totp_style)));

    let status = Paragraph::new(status_lines)
        .block(Block::default().borders(Borders::ALL).title(" Status "));
    frame.render_widget(status, chunks[1]);

    let log_spans: Vec<ListItem> = app
        .log_lines
        .iter()
        .map(|s| ListItem::new(s.as_str()))
        .collect();
    let log = ratatui::widgets::List::new(log_spans)
        .block(Block::default().borders(Borders::ALL).title(" Log "));
    frame.render_widget(log, chunks[2]);

    let menu_items: Vec<ListItem> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let prefix = if i == app.selected { ">" } else { " " };
            ListItem::new(format!(" {}  {}", prefix, item.label()))
        })
        .collect();
    let menu = ratatui::widgets::List::new(menu_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Menu (↑↓ enter, q=quit) "),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(menu, chunks[3]);
}

fn render_qr(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" TOTP QR Code ")
        .title_alignment(Alignment::Center);

    let text = Paragraph::new(app.qr_text.as_str())
        .block(block)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);
    frame.render_widget(text, area);

    let hint = Line::from(Span::styled(
        "  q/ESC = 返回  ",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(
        Paragraph::new(hint).alignment(Alignment::Center),
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area)[1],
    );
}

fn lan_origin(lan_ip: &str, port: u16) -> String {
    format!("https://{lan_ip}:{port}")
}

#[cfg(test)]
mod tests {
    use super::{lan_origin, MenuItem};

    #[test]
    fn lan_status_shows_origin_without_login_path() {
        assert_eq!(lan_origin("172.21.130.235", 9527), "https://172.21.130.235:9527");
    }

    #[test]
    fn remote_menu_uses_product_names() {
        assert_eq!(MenuItem::StartLanRemote.label(), "Start LAN Remote");
        assert_eq!(MenuItem::StopLanRemote.label(), "Stop LAN Remote");
        assert_eq!(MenuItem::StartDaemon.label(), "Start public Remote");
        assert_eq!(MenuItem::StopDaemon.label(), "Stop public Remote");
    }
}
