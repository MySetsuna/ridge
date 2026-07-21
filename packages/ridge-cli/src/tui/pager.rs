//! 多会话 Tab 式 TUI（Pager）。在 Decorative Scroll Region (DECSTBM) 保护下，
//! 顶部 2 行用 crossterm 状态栏显示会话列表，主区域用 passthrough 输出活动会话。
//!
//! 控制快捷键：
//!   Ctrl+Shift+←→↑↓ → 切换 pane（会话）
//!   Ctrl+F1..F12    → 切换工作区
//!   Ctrl+]           → 退出

use std::io::Write;

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::terminal::ClearType;
use crossterm::{execute, terminal};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use ridge_core::workspace::pane_tree::Direction;
use super::keymap;
use super::workspace::WorkspaceManager;

/// 顶部占用行数（状态栏 + 分隔线）。
const STATUS_LINES: u16 = 2;
/// 输出 mpsc channel 缓冲数。
const OUT_CHAN_BUF: usize = 64;

/// 进入多会话 Pager TUI。
pub async fn run_pager(manager: &mut WorkspaceManager) -> Result<()> {
    enable_raw_mode().context("进入 raw mode 失败")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("进入 alt screen 失败")?;

    let (cols, rows) = terminal::size()?;

    // 把所有 session 对齐到内容区尺寸。
    let content_rows = rows.saturating_sub(STATUS_LINES);
    manager.resize_all(cols, content_rows);

    // DECSTBM：设 scroll region 从第 STATUS_LINES+1 行到屏幕底部。
    write!(stdout, "\x1b[{};{}r", STATUS_LINES + 1, rows)?;
    // DECOM (Origin Mode)：让所有光标定位序列相对于 scroll region。
    write!(stdout, "\x1b[?6h")?;

    draw_chrome(&mut stdout, manager, cols)?;

    // 输出路由：output_task 从当前活动 session 读 broadcast → 写 out_tx，
    // 事件循环从 out_rx 读 → 写 stdout。
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(OUT_CHAN_BUF);
    let mut output_task: Option<JoinHandle<()>> = None;
    spawn_output_task(&mut output_task, manager, out_tx.clone());

    let mut events = EventStream::new();

    let result: Result<()> = loop {
        tokio::select! {
            maybe_ev = events.next() => {
                match maybe_ev {
                    Some(Ok(Event::Key(k))) => {
                        if matches!(k.kind, KeyEventKind::Release) {
                            continue;
                        }

                        // Ctrl+] = 退出
                        if is_quit(&k) {
                            break Ok(());
                        }

                        // 控制快捷键（pane/ws 切换）
                        if handle_shortcut(&k, manager, &mut stdout, cols)? {
                            spawn_output_task(&mut output_task, manager, out_tx.clone());
                            continue;
                        }

                        // 普通按键：编码发给活动 session
                        if let Some(bytes) = keymap::encode_key(&k) {
                            if let Some(sess) = manager.active_session_handle() {
                                sess.send_input(&bytes)?;
                            }
                        }
                    }
                    Some(Ok(Event::Resize(cols, rows))) => {
                        let content_rows = rows.saturating_sub(STATUS_LINES);
                        manager.resize_all(cols, content_rows);
                        write!(stdout, "\x1b[{};{}r", STATUS_LINES + 1, rows)?;
                        draw_chrome(&mut stdout, manager, cols)?;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break Err(e).context("读终端事件失败"),
                    None => break Ok(()),
                }
            }
            chunk = out_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                    }
                    // 输出流关闭 = 会话结束。
                    None => break Ok(()),
                }
            }
        }
    };

    // 清理：停止输出任务、复位 scroll region/DECOM、离开 alt screen。
    if let Some(h) = output_task.take() {
        h.abort();
    }
    write!(stdout, "\x1b[r\x1b[?6l")?;
    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    result
}

/// 退出热键：Ctrl+]。
fn is_quit(ev: &KeyEvent) -> bool {
    ev.modifiers.contains(KeyModifiers::CONTROL) && matches!(ev.code, KeyCode::Char(']'))
}

/// 解析并执行控制快捷键。返回 true 表示按键已被消费（不应再转发给 PTY）。
fn handle_shortcut(
    ev: &KeyEvent,
    manager: &mut WorkspaceManager,
    stdout: &mut impl Write,
    cols: u16,
) -> Result<bool> {
    if !keymap::is_control_shortcut(ev) {
        return Ok(false);
    }

    let switched = match ev.code {
        KeyCode::Right => manager.navigate(Direction::Right),
        KeyCode::Left => manager.navigate(Direction::Left),
        KeyCode::Up => manager.navigate(Direction::Up),
        KeyCode::Down => manager.navigate(Direction::Down),
        KeyCode::F(n) => manager.switch_workspace(n),
        _ => false,
    };

    if switched {
        draw_chrome(stdout, manager, cols)?;
    }
    Ok(true)
}

/// 用 crossterm 直接写出顶部状态栏（临时关闭 DECOM 以操作绝对坐标）。
fn draw_chrome(stdout: &mut impl Write, manager: &WorkspaceManager, cols: u16) -> Result<()> {
    use crossterm::cursor::MoveTo;
    use crossterm::terminal::Clear;

    // 临时关闭 DECOM 以便用绝对坐标写 1-2 行。
    write!(stdout, "\x1b[?6l")?;

    // 第 1 行：状态栏
    execute!(stdout, MoveTo(0, 0), Clear(ClearType::CurrentLine))?;
    let bar = manager.status_bar_text(cols);
    write!(stdout, "{}", bar)?;

    // 第 2 行：分隔线
    execute!(stdout, MoveTo(0, 1), Clear(ClearType::CurrentLine))?;
    let sep: String = std::iter::repeat('─')
        .take(cols.min(120) as usize)
        .collect();
    write!(stdout, "{}", sep)?;

    // 重新启用 DECOM
    write!(stdout, "\x1b[?6h")?;

    // 将光标移到内容区起点（相对于 region 的第 1 行第 1 列）。
    write!(stdout, "\x1b[1;1H")?;
    stdout.flush()?;
    Ok(())
}

/// 为当前活动 session spawn 一个输出转发任务（broadcast → mpsc）。
/// 先 abort 已有任务（如果有）。
fn spawn_output_task(
    task: &mut Option<JoinHandle<()>>,
    manager: &WorkspaceManager,
    out_tx: mpsc::Sender<Vec<u8>>,
) {
    if let Some(h) = task.take() {
        h.abort();
    }

    let Some(sess) = manager.active_session_handle() else {
        return;
    };

    *task = Some(tokio::spawn(async move {
        let mut rx = sess.subscribe();
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    if out_tx.send(bytes).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));
}
