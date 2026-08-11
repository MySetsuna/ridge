use std::io::stdout;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use super::qr_display;
use super::workspace::{new_shared, SharedWorkspace};
use crate::config;
use crate::daemon_ctl;
use crate::kernel_ctl;
use crate::login_flow;
use crate::totp::RemoteTotp;
use ridge_core::workspace::pane_tree::SplitDirection;

#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Main,
    QrCode,
}

type DashboardTerminal = Terminal<CrosstermBackend<std::io::Stdout>>;

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuItem {
    ShowQrCode,
    StartLanRemote,
    StopLanRemote,
    StartDaemon,
    StopDaemon,
    Login,
    /// 仅退出 rdg UI（内核若由桌面承载则继续）。
    Quit,
    /// 彻底退出内核；桌面仍在时命令行 Y/N 确认。
    QuitKernel,
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
            MenuItem::Quit => "Quit rdg (keep kernel)",
            MenuItem::QuitKernel => "彻底退出内核",
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
    MenuItem::QuitKernel,
];

enum Action {
    RunLogin,
    /// 彻底退出内核：须先退 TUI 再做 stdin Y/N。
    QuitKernel,
    Refresh,
    StartLanRemote,
    StopLanRemote,
    /// LAN host task 退出/失败：清 running 并写 log（禁止 UI 假绿）。
    LanRemoteFailed(String),
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
    // Thin shell cannot run without its authoritative kernel.
    let kernel = kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
    tracing::info!(
        target: "ridge_cli::dashboard",
        pid = kernel.pid,
        port = kernel.port,
        "ridge-kernel ready"
    );
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
            handle_action(action, &mut app, &mut terminal).await?;
        }

        if app.quit {
            break;
        }

        // 精确监视启动时 attach 的 PID；HTTP 短错不误退，PID 死亡立即联动退出。
        if !kernel_ctl::is_kernel_process_alive(kernel.pid) {
            app.log("内核已退出，rdg 联动退出".into());
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

async fn handle_action(
    action: Action,
    app: &mut App,
    terminal: &mut DashboardTerminal,
) -> Result<()> {
    match action {
        Action::Refresh => refresh_app(app),
        Action::RunLogin => run_login_action(app, terminal).await?,
        Action::QuitKernel => quit_kernel_action(app, terminal)?,
        Action::StartLanRemote => start_lan_remote(app),
        Action::StopLanRemote => stop_lan_remote(app),
        Action::LanRemoteFailed(msg) => {
            app.lan_shutdown_tx = None;
            app.lan_running = false;
            app.log(format!("LAN remote failed: {msg}"));
        }
    }
    Ok(())
}

fn refresh_app(app: &mut App) {
    app.auth = config::load_auth().ok().flatten();
    app.public_entry = app.auth.as_ref().map(|auth| auth.public_entry());
    app.log(format!("Daemon: {}", daemon_ctl::status()));
}

async fn run_login_action(app: &mut App, terminal: &mut DashboardTerminal) -> Result<()> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    let result = run_login_flow().await;
    restore_dashboard_terminal(terminal)?;
    app.log(match &result {
        Ok(_) => "Login successful".into(),
        Err(error) => format!("Login failed: {error}"),
    });
    refresh_app(app);
    Ok(())
}

async fn run_login_flow() -> Result<()> {
    let client = reqwest::Client::builder().build().ok();
    let Some(client) = client else {
        return Err(anyhow::anyhow!("无法创建 HTTP client"));
    };
    let _auth = match prompt_login_method() {
        LoginMethod::Email => login_flow::run_login(&client).await?,
        LoginMethod::Browser => login_flow::run_browser_login(&client).await?,
    };
    Ok(())
}

fn quit_kernel_action(app: &mut App, terminal: &mut DashboardTerminal) -> Result<()> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    if kernel_ctl::desktop_kernel_running() {
        println!("{}", kernel_ctl::status_line());
        if !kernel_ctl::confirm_quit_kernel_with_desktop() {
            println!("已取消彻底退出内核");
            restore_dashboard_terminal(terminal)?;
            app.log("已取消彻底退出内核".into());
            return Ok(());
        }
    }
    match kernel_ctl::stop_kernel() {
        Ok(()) => println!("内核已结束；rdg 退出"),
        Err(error) => println!("{error}；rdg 退出"),
    }
    restore_dashboard_terminal(terminal)?;
    app.quit = true;
    Ok(())
}

fn restore_dashboard_terminal(terminal: &mut DashboardTerminal) -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    *terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    Ok(())
}

fn start_lan_remote(app: &mut App) {
    if app.lan_running {
        app.log("LAN remote already running".into());
        return;
    }
    let port = config::lan_port();
    let totp = app.totp.clone();
    let workspace = app.workspace.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    app.lan_shutdown_tx = Some(shutdown_tx);
    app.lan_running = true;
    app.log(format!("Starting LAN remote on port {port}..."));
    let fail_tx = app.action_tx.clone();
    tokio::spawn(async move {
        if let Err(error) = super::lan_host::run(port, totp, workspace, shutdown_rx).await {
            tracing::warn!(target: "ridge_cli::dashboard", error = %error, "LAN remote stopped");
            let _ = fail_tx.send(Action::LanRemoteFailed(error.to_string()));
        }
    });
}

fn stop_lan_remote(app: &mut App) {
    if let Some(tx) = app.lan_shutdown_tx.take() {
        let _ = tx.send(());
        app.lan_running = false;
        app.log("LAN remote stopped".into());
    } else {
        app.log("LAN remote not running".into());
    }
}

fn handle_main_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.selected = app.selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.selected = (app.selected + 1).min(MENU_ITEMS.len() - 1);
        }
        KeyCode::Enter => handle_menu_selection(app),
        KeyCode::Char('q') => app.quit = true,
        _ => {}
    }
}

fn handle_menu_selection(app: &mut App) {
    match MENU_ITEMS[app.selected] {
        MenuItem::ShowQrCode => show_qr_code(app),
        MenuItem::StartLanRemote => send_action(app, Action::StartLanRemote),
        MenuItem::StopLanRemote => send_action(app, Action::StopLanRemote),
        MenuItem::StartDaemon => start_daemon(app),
        MenuItem::StopDaemon => stop_daemon(app),
        MenuItem::Login => send_action(app, Action::RunLogin),
        MenuItem::Quit => app.quit = true,
        MenuItem::QuitKernel => send_action(app, Action::QuitKernel),
    }
    send_action(app, Action::Refresh);
}

fn send_action(app: &App, action: Action) {
    let _ = app.action_tx.send(action);
}

fn show_qr_code(app: &mut App) {
    let device_name = app
        .auth
        .as_ref()
        .map(|auth| auth.device_name.as_str())
        .unwrap_or("rdg");
    let uri = app.totp.otpauth_uri(device_name);
    let qr = qr_display::render_qr(&uri);
    app.qr_text = format!(
        "{qr}\n  URI: {uri}\n  验证码: {} (每 {} 秒刷新)\n\n  请用手机 Authenticator 扫描上方二维码",
        app.totp_code,
        RemoteTotp::period_secs(),
    );
    app.view = View::QrCode;
}

fn start_daemon(app: &mut App) {
    if config::load_auth().ok().flatten().is_none() {
        app.log("本机未激活云端设备：先选 Login / activate device".into());
        return;
    }
    if app.daemon_task.is_some() {
        app.log("云守护已在本进程内运行".into());
        return;
    }
    match daemon_ctl::start_daemon() {
        Ok(()) => {
            let entry = app
                .auth
                .as_ref()
                .map(|auth| auth.public_entry())
                .unwrap_or_default();
            app.daemon_task = Some(tokio::spawn(async move {
                if let Err(error) = crate::daemon::run(None, None, None).await {
                    tracing::error!(target: "ridge_cli::dashboard", error = %error, "daemon exited");
                }
            }));
            app.log(format!("云守护已启动，入口 {entry}"));
        }
        Err(error) => app.log(format!("Start failed: {error}")),
    }
}

fn stop_daemon(app: &mut App) {
    if let Some(task) = app.daemon_task.take() {
        task.abort();
        daemon_ctl::remove_pid();
        app.log("云守护已停止".into());
        return;
    }
    match daemon_ctl::stop_daemon() {
        Ok(()) => app.log("Daemon stopped".into()),
        Err(error) => app.log(format!("Stop failed: {error}")),
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

    let mut status_lines = vec![Line::from(format!("  Public:  {}", daemon_ctl::status()))];

    let lan_status = if app.lan_running {
        "Running"
    } else {
        "Stopped"
    };
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
    let totp_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let totp_text = format!(
        "  TOTP:    {} (每 {}s 刷新)  [Enter] 查看二维码",
        app.totp_code,
        RemoteTotp::period_secs()
    );
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
        assert_eq!(
            lan_origin("172.21.130.235", 9527),
            "https://172.21.130.235:9527"
        );
    }

    #[test]
    fn remote_menu_uses_product_names() {
        assert_eq!(MenuItem::StartLanRemote.label(), "Start LAN Remote");
        assert_eq!(MenuItem::StopLanRemote.label(), "Stop LAN Remote");
        assert_eq!(MenuItem::StartDaemon.label(), "Start public Remote");
        assert_eq!(MenuItem::StopDaemon.label(), "Stop public Remote");
        assert_eq!(MenuItem::Quit.label(), "Quit rdg (keep kernel)");
        assert_eq!(MenuItem::QuitKernel.label(), "彻底退出内核");
    }
}
