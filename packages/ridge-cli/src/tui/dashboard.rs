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
            MenuItem::StartLanRemote => "Start LAN remote (https://...)",
            MenuItem::StopLanRemote => "Stop LAN remote",
            MenuItem::StartDaemon => "Start daemon (cloud)",
            MenuItem::StopDaemon => "Stop daemon",
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
            lan_addr: format!("https://{lan_ip}:{port}/login"),
            lan_running: false,
            lan_shutdown_tx: None,
            public_entry: auth.as_ref().map(|a| a.public_entry()),
            action_tx,
            workspace,
            session_count: 1,
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
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Action>();
    let mut app = App::new(tx.clone());
    // 自动启动 LAN 远程服务
    let _ = tx.send(Action::StartLanRemote);
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
                    let result = if let Some(client) = client {
                        login_flow::run_login(&client).await
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
                    match daemon_ctl::start_daemon() {
                        Ok(()) => {
                            app.log("Daemon started".into());
                            if let Ok(Some(_auth)) = config::load_auth() {
                                let shell: Option<String> = None;
                                let cwd: Option<String> = None;
                                let root: Option<String> = None;
                                tokio::spawn(async move {
                                    if let Err(e) = crate::daemon::run(shell, cwd, root).await {
                                        eprintln!("Daemon exited with error: {e}");
                                    }
                                });
                                app.log("Daemon task spawned".into());
                            } else {
                                app.log("Device not activated — run Login first".into());
                            }
                        }
                        Err(e) => app.log(format!("Start failed: {e}")),
                    }
                }
                MenuItem::StopDaemon => match daemon_ctl::stop_daemon() {
                    Ok(()) => app.log("Daemon stopped".into()),
                    Err(e) => app.log(format!("Stop failed: {e}")),
                },
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

    let mut status_lines = vec![Line::from(format!("  Daemon: {}", daemon_ctl::status()))];

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
