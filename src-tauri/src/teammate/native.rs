//! 无头 tmux 会话引擎（native sessions）。
//!
//! 与 GUI-bridge 路径互补：GUI 路径把 `tmux` 命令映射到 Ridge 可见工作区的分屏面板；
//! 本模块则在 **同一个常驻 teammate 进程内**维护一套「程序化会话控制面」——具名/后台会话、
//! `-L`/`-S` 独立 socket，每个面板背后是一个 **真实但无头** 的 `portable-pty` 子进程。
//!
//! 设计要点（对应需求）：
//! - **零新增常驻进程 / 按需才有开销**：注册表是一张内存表，`new-session`/`-L` 命令首次
//!   出现才会有内容；不碰 tmux 的用户这里恒为空，无任何线程/PTY 成本。
//! - **socket 命名空间隔离**：`-L NAME` / `-S PATH` 作为命名空间键；`-L X ls` 只列该 socket、
//!   `kill-server` 只清该 socket，逻辑隔离。
//! - **find-target 绝不兜底**：精确 > `=NAME`(仅精确) > fnmatch > 前缀 > 子串；多命中 ambiguous、
//!   零命中 `can't find session: NAME`。未知目标一律报错，绝不回退到“当前/默认会话”。
//! - **持久**：会话活在 Ridge 进程里，启动脚本退出后仍可被后续进程寻址，随 Ridge 退出而终止。
//!
//! 线程模型：全局注册表用一把 `Mutex` 串行化（控制面 QPS 极低）。每个 native 面板启一条
//! reader 线程把 PTY master 输出读空丢弃，避免子进程因输出缓冲写满而阻塞。

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use chrono::{DateTime, Local, Utc};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use ridge_term::term::terminal::Terminal;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

/// 每个 native 面板输出环形缓冲上限（字节）。capture-pane 从这里取快照，喂一台
/// 一次性 `Terminal` 重渲当前屏。256 KiB 足以覆盖一屏 + 一段滚动历史。
const PANE_RING_CAP: usize = 256 * 1024;

// ===================== 错误类型 =====================

/// native 操作的失败原因；端点据此映射 HTTP 状态码，shim 据此决定退出码/回退。
#[derive(Debug)]
pub enum NativeError {
    /// 目标不存在（`can't find session: NAME` 等）。-> 404 / 退 1。
    NotFound(String),
    /// 目标有歧义（多命中）。-> 404 / 退 1。
    Ambiguous(String),
    /// 会话重名，无法创建。-> 409 / 退 1。
    Duplicate(String),
    /// 该 socket 上没有 server（自定义 socket 尚未起会话）。-> 404 / 退 1。
    NoServer(String),
    /// 默认 socket 上目标解析到的是一个 **GUI 会话**；shim 应回退走 GUI 路径。-> 409。
    Gui(String),
    /// 内部错误（PTY 创建失败等）。-> 500 / 退 1。
    Internal(String),
}

pub type NativeResult = Result<String, NativeError>;

impl NativeError {
    pub fn message(&self) -> String {
        match self {
            NativeError::NotFound(m)
            | NativeError::Ambiguous(m)
            | NativeError::Duplicate(m)
            | NativeError::NoServer(m)
            | NativeError::Gui(m)
            | NativeError::Internal(m) => m.clone(),
        }
    }
}

// ===================== 注册表数据结构 =====================

struct Pane {
    /// 全局唯一面板 id，渲染为 `%N`。
    global_id: usize,
    /// 保活 master，并可与 GUI 视图**共享**（召唤后 resize 经此 master 下达）。
    master: Arc<parking_lot::Mutex<Box<dyn MasterPty + Send>>>,
    /// 写端，供 `send-keys` 注入；GUI 召唤后键入也共享同一份。
    writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
    /// 子进程句柄，供 `kill-*` 终止。
    child: Box<dyn Child + Send + Sync>,
    width: u16,
    height: u16,
    cwd: Option<String>,
    /// 输出环形缓冲（有界）。reader 线程持续写入；`capture` 取快照喂一台一次性
    /// `Terminal` 重渲当前屏。无人 capture 时也只是被动存储，不解析、零额外 CPU。
    ring: Arc<Mutex<VecDeque<u8>>>,
    /// 实时输出广播：GUI 召唤时订阅，驱动领养 pane 的渲染（见 `BroadcastReader`）。
    output_tx: broadcast::Sender<Vec<u8>>,
    /// 当前挂载到哪个 (workspace, pane)；`None` 即无头。
    attachment: Option<(Uuid, Uuid)>,
}

struct Window {
    id: usize,
    /// 窗口名（`-n`，缺省取 shell/命令 basename）。
    name: String,
    panes: Vec<Pane>,
    active_pane: usize,
}

struct Session {
    id: usize,
    name: String,
    created_at: SystemTime,
    windows: Vec<Window>,
    active_window: usize,
    /// `-x`/`-y` 指定的会话默认尺寸（cols × rows）。
    width: u16,
    height: u16,
}

#[derive(Default)]
struct SocketState {
    sessions: Vec<Session>,
}

#[derive(Default)]
struct NativeServer {
    sockets: HashMap<String, SocketState>,
    next_pane_id: usize,
    next_session_id: usize,
    next_window_id: usize,
}

fn registry() -> &'static Mutex<NativeServer> {
    static REGISTRY: OnceLock<Mutex<NativeServer>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(NativeServer::default()))
}

// ===================== 请求/上下文类型 =====================

/// 默认 socket 上解析时一并参与匹配的 GUI 会话（工作区名 + `ridge`）。
/// 自定义 socket 传空切片。
pub struct GuiSession {
    pub name: String,
}

pub struct NewSessionReq {
    pub socket: String,
    pub name: Option<String>,
    pub window_name: Option<String>,
    pub cwd: Option<String>,
    pub width: u16,
    pub height: u16,
    /// 面板 shell（取自调用方 `$SHELL`）；为空时回退平台默认。
    pub shell: Option<String>,
    /// 尾部 `[shell-command]`。
    pub command: Option<String>,
    /// `-A`：存在则 attach、不存在则 create。
    pub attach_or_create: bool,
    /// `-P`：打印新会话信息；为空时用默认模板。
    pub print: Option<Option<String>>,
}

/// 解析命中的 native 目标（已落到具体 session/window/pane）。
pub struct ResolvedNative {
    pub socket: String,
    pub session: String,
    pub window_index: usize,
    pub pane_index: usize,
    pub pane_global_id: usize,
}

// ===================== 目标解析（纯逻辑，可单测） =====================

#[derive(Debug, PartialEq, Eq)]
struct ParsedTarget {
    /// `=NAME` 前缀：仅精确匹配。
    exact: bool,
    session: Option<String>,
    window: Option<usize>,
    pane: Option<usize>,
    /// `%N`：全局面板 id（无会话上下文）。
    pane_global: Option<usize>,
}

/// 把 tmux 目标串拆成 session/window/pane 分量。支持：
/// `=NAME` / `%N` / `NAME` / `NAME:W` / `NAME:W.P` / `NAME.P` / `:W.P`。
fn parse_target(raw: &str) -> ParsedTarget {
    let mut t = raw.trim();
    let mut exact = false;
    if let Some(rest) = t.strip_prefix('=') {
        exact = true;
        t = rest;
    }
    // `%N` 全局面板 id。
    if let Some(n) = t.strip_prefix('%') {
        if let Ok(id) = n.parse::<usize>() {
            return ParsedTarget { exact, session: None, window: None, pane: None, pane_global: Some(id) };
        }
    }

    let parse_win_pane = |s: &str| -> (Option<usize>, Option<usize>) {
        if let Some(dot) = s.rfind('.') {
            (s[..dot].parse::<usize>().ok(), s[dot + 1..].parse::<usize>().ok())
        } else {
            (s.parse::<usize>().ok(), None)
        }
    };

    if let Some(colon) = t.find(':') {
        let sess = &t[..colon];
        let right = &t[colon + 1..];
        let (window, pane) = parse_win_pane(right);
        let session = if sess.is_empty() { None } else { Some(sess.to_string()) };
        return ParsedTarget { exact, session, window, pane, pane_global: None };
    }

    // 无冒号：`NAME.P`（如验收用的 `S.0`）或纯会话名。
    if let Some(dot) = t.rfind('.') {
        let sess = &t[..dot];
        if let Ok(pane) = t[dot + 1..].parse::<usize>() {
            if !sess.is_empty() {
                return ParsedTarget {
                    exact,
                    session: Some(sess.to_string()),
                    window: None,
                    pane: Some(pane),
                    pane_global: None,
                };
            }
        }
    }

    let session = if t.is_empty() { None } else { Some(t.to_string()) };
    ParsedTarget { exact, session, window: None, pane: None, pane_global: None }
}

/// 极简 glob：支持 `*`（任意串）与 `?`（任一字符）。
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = text.chars().collect();
    // 经典 DP / 双指针回溯。
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn is_pattern(q: &str) -> bool {
    q.contains('*') || q.contains('?')
}

/// 在候选名集合里按 tmux 优先级匹配 `query`。返回唯一命中名，或错误。
/// `exact` 为真时只接受精确匹配。
fn match_session_name<'a>(
    query: &str,
    exact: bool,
    candidates: &'a [String],
) -> Result<&'a str, NativeError> {
    // 1) 精确
    if let Some(hit) = candidates.iter().find(|c| c.as_str() == query) {
        return Ok(hit.as_str());
    }
    if exact {
        return Err(NativeError::NotFound(format!("can't find session: {query}")));
    }
    // 2) fnmatch（仅当 query 像 pattern）
    if is_pattern(query) {
        let hits: Vec<&String> = candidates.iter().filter(|c| glob_match(query, c)).collect();
        match hits.len() {
            1 => return Ok(hits[0].as_str()),
            n if n > 1 => return Err(NativeError::Ambiguous(format!("ambiguous session: {query}"))),
            _ => {}
        }
    }
    // 3) 前缀
    let pre: Vec<&String> = candidates.iter().filter(|c| c.starts_with(query)).collect();
    match pre.len() {
        1 => return Ok(pre[0].as_str()),
        n if n > 1 => return Err(NativeError::Ambiguous(format!("ambiguous session: {query}"))),
        _ => {}
    }
    // 4) 子串
    let sub: Vec<&String> = candidates.iter().filter(|c| c.contains(query)).collect();
    match sub.len() {
        1 => Ok(sub[0].as_str()),
        n if n > 1 => Err(NativeError::Ambiguous(format!("ambiguous session: {query}"))),
        _ => Err(NativeError::NotFound(format!("can't find session: {query}"))),
    }
}

// ===================== PTY 自旋 =====================

fn default_shell() -> String {
    #[cfg(windows)]
    {
        "powershell.exe".to_string()
    }
    #[cfg(not(windows))]
    {
        if std::path::Path::new("/bin/bash").is_file() {
            "/bin/bash".to_string()
        } else {
            "/bin/sh".to_string()
        }
    }
}

/// 起一个无头 PTY 子进程，返回 `Pane`。`shell` 缺省取平台默认；`command` 非空则跑 `shell -c command`。
fn spawn_pane(
    global_id: usize,
    width: u16,
    height: u16,
    cwd: Option<&str>,
    shell: Option<&str>,
    command: Option<&str>,
) -> Result<Pane, NativeError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows: height.max(1), cols: width.max(1), pixel_width: 0, pixel_height: 0 })
        .map_err(|e| NativeError::Internal(format!("openpty: {e}")))?;

    let prog = shell
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(default_shell);

    let mut cmd = CommandBuilder::new(&prog);
    if let Some(c) = command.map(str::trim).filter(|s| !s.is_empty()) {
        // 跑一次性命令：交给 shell -c，结束即面板进程退出（与 tmux 一致）。
        cmd.arg("-c");
        cmd.arg(c);
    }
    cmd.env("TERM", "xterm-256color");
    if let Some(dir) = cwd.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.cwd(dir);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| NativeError::Internal(format!("spawn: {e}")))?;
    // slave 句柄交给子进程后即可丢弃，OS 侧 fd 仍由子进程持有。
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| NativeError::Internal(format!("reader: {e}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| NativeError::Internal(format!("writer: {e}")))?;

    // 输出广播：容量给宽；无订阅者时 send 返回 Err，忽略。
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(4096);

    // reader 线程：**存环**（供 capture 重渲）+ **广播**（供 GUI 召唤实时渲染），
    // 同时把 master 读空，防止子进程因缓冲写满而阻塞。热路径不做 vt 解析。
    let ring: Arc<Mutex<VecDeque<u8>>> = Arc::new(Mutex::new(VecDeque::new()));
    let ring_reader = ring.clone();
    let tx_reader = output_tx.clone();
    std::thread::Builder::new()
        .name(format!("ridge-native-pty-{global_id}"))
        .spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        if let Ok(mut r) = ring_reader.lock() {
                            r.extend(&chunk);
                            let overflow = r.len().saturating_sub(PANE_RING_CAP);
                            if overflow > 0 {
                                r.drain(..overflow);
                            }
                        }
                        // 广播给已召唤的 GUI 视图；无订阅者直接丢弃。
                        let _ = tx_reader.send(chunk);
                    }
                }
            }
        })
        .ok();

    Ok(Pane {
        global_id,
        master: Arc::new(parking_lot::Mutex::new(pair.master)),
        writer: Arc::new(parking_lot::Mutex::new(writer)),
        child,
        width,
        height,
        cwd: cwd.map(|s| s.to_string()),
        ring,
        output_tx,
        attachment: None,
    })
}

// ===================== 操作 API =====================

/// 候选会话名 = 该 socket 的 native 会话 ∪ GUI 会话（默认 socket 才有 GUI）。
fn candidate_names(srv: &NativeServer, socket: &str, gui: &[GuiSession]) -> Vec<String> {
    let mut names: Vec<String> = srv
        .sockets
        .get(socket)
        .map(|s| s.sessions.iter().map(|x| x.name.clone()).collect())
        .unwrap_or_default();
    for g in gui {
        if !names.iter().any(|n| n == &g.name) {
            names.push(g.name.clone());
        }
    }
    names
}

/// 解析目标到具体 native 会话/窗口/面板。命中 GUI 会话返回 `Gui(name)`。
pub fn resolve(socket: &str, target: &str, gui: &[GuiSession]) -> Result<ResolvedNative, NativeError> {
    let srv = registry().lock().unwrap();
    resolve_locked(&srv, socket, target, gui)
}

fn resolve_locked(
    srv: &NativeServer,
    socket: &str,
    target: &str,
    gui: &[GuiSession],
) -> Result<ResolvedNative, NativeError> {
    // 自定义 socket 上没有任何 server（从未起过会话）→ 按 tmux 报 "no server"。
    if socket != "default" && !srv.sockets.contains_key(socket) {
        return Err(NativeError::NoServer(format!("no server running on {socket}")));
    }
    let pt = parse_target(target);

    // `%N` 全局面板 id：在该 socket 的全部面板里找。
    if let Some(gid) = pt.pane_global {
        if let Some(sock) = srv.sockets.get(socket) {
            for s in &sock.sessions {
                for (wi, w) in s.windows.iter().enumerate() {
                    for (pi, p) in w.panes.iter().enumerate() {
                        if p.global_id == gid {
                            return Ok(ResolvedNative {
                                socket: socket.to_string(),
                                session: s.name.clone(),
                                window_index: wi,
                                pane_index: pi,
                                pane_global_id: gid,
                            });
                        }
                    }
                }
            }
        }
        return Err(NativeError::NotFound(format!("can't find pane: %{gid}")));
    }

    let Some(query) = pt.session.as_deref() else {
        // 无会话分量：交给调用方按“当前会话”处理（GUI 遗留路径）。
        return Err(NativeError::Gui(String::new()));
    };

    let names = candidate_names(srv, socket, gui);
    if names.is_empty() {
        return Err(NativeError::NotFound(format!("can't find session: {query}")));
    }
    let hit = match_session_name(query, pt.exact, &names)?;

    // 命中 GUI 会话 → 让 shim 回退。
    if gui.iter().any(|g| g.name == hit) && !sock_has_session(srv, socket, hit) {
        return Err(NativeError::Gui(hit.to_string()));
    }

    let sock = srv
        .sockets
        .get(socket)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {query}")))?;
    let s = sock
        .sessions
        .iter()
        .find(|x| x.name == hit)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {query}")))?;

    let wi = pt.window.unwrap_or(s.active_window).min(s.windows.len().saturating_sub(1));
    let w = s
        .windows
        .get(wi)
        .ok_or_else(|| NativeError::NotFound(format!("can't find window: {wi}")))?;
    let pi = pt.pane.unwrap_or(w.active_pane).min(w.panes.len().saturating_sub(1));
    let p = w
        .panes
        .get(pi)
        .ok_or_else(|| NativeError::NotFound(format!("can't find pane: {pi}")))?;

    Ok(ResolvedNative {
        socket: socket.to_string(),
        session: hit.to_string(),
        window_index: wi,
        pane_index: pi,
        pane_global_id: p.global_id,
    })
}

fn sock_has_session(srv: &NativeServer, socket: &str, name: &str) -> bool {
    srv.sockets
        .get(socket)
        .map(|s| s.sessions.iter().any(|x| x.name == name))
        .unwrap_or(false)
}

/// `has-session`：存在（native 或 GUI）→ Ok；否则 NotFound。
pub fn has_session(socket: &str, target: &str, gui: &[GuiSession]) -> NativeResult {
    let srv = registry().lock().unwrap();
    if socket != "default" && !srv.sockets.contains_key(socket) {
        return Err(NativeError::NoServer(format!("no server running on {socket}")));
    }
    let pt = parse_target(target);
    let Some(query) = pt.session.as_deref() else {
        return Err(NativeError::NotFound("can't find session".into()));
    };
    let names = candidate_names(&srv, socket, gui);
    match match_session_name(query, pt.exact, &names) {
        Ok(_) => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// `new-session`。
pub fn new_session(req: NewSessionReq, gui: &[GuiSession]) -> NativeResult {
    let mut srv = registry().lock().unwrap();

    let name = req.name.clone().unwrap_or_else(|| {
        // 缺省名：与 tmux 一致用数字索引；这里取下一个序号。
        let n = srv.next_session_id;
        format!("{n}")
    });

    // 重名检查：native 同名，或默认 socket 上撞到 GUI 会话名。
    let dup_native = sock_has_session(&srv, &req.socket, &name);
    let dup_gui = gui.iter().any(|g| g.name == name);
    if dup_native || dup_gui {
        if req.attach_or_create {
            // `-A`：已存在则视作 attach 成功（无头 attach 为 no-op）。
            return Ok(render_new_session_print(&req.print, &name, 0, 0));
        }
        return Err(NativeError::Duplicate(format!("duplicate session: {name}")));
    }

    let pane_id = srv.next_pane_id;
    srv.next_pane_id += 1;
    let window_id = srv.next_window_id;
    srv.next_window_id += 1;
    let session_id = srv.next_session_id;
    srv.next_session_id += 1;

    let pane = spawn_pane(
        pane_id,
        req.width,
        req.height,
        req.cwd.as_deref(),
        req.shell.as_deref(),
        req.command.as_deref(),
    )?;

    let win_name = req
        .window_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| basename(req.shell.as_deref().unwrap_or("shell")));

    let window = Window { id: window_id, name: win_name, panes: vec![pane], active_pane: 0 };
    let session = Session {
        id: session_id,
        name: name.clone(),
        created_at: SystemTime::now(),
        windows: vec![window],
        active_window: 0,
        width: req.width,
        height: req.height,
    };
    srv.sockets.entry(req.socket.clone()).or_default().sessions.push(session);

    Ok(render_new_session_print(&req.print, &name, 0, pane_id))
}

fn render_new_session_print(
    print: &Option<Option<String>>,
    session: &str,
    window_index: usize,
    pane_global_id: usize,
) -> String {
    match print {
        None => String::new(), // 未带 -P
        Some(fmt) => {
            let tpl = fmt
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "#{session_name}:#{window_index}.#{pane_id}".to_string());
            tpl.replace("#{session_name}", session)
                .replace("#{window_index}", &window_index.to_string())
                .replace("#{pane_id}", &format!("%{pane_global_id}"))
        }
    }
}

/// `split-window` / `new-window` 之后在某会话窗口里新增一个面板。
/// `new_window=true` 时新建窗口，否则在目标窗口追加面板。
pub fn add_pane(
    socket: &str,
    target: &str,
    gui: &[GuiSession],
    new_window: bool,
    window_name: Option<&str>,
    cwd: Option<&str>,
    shell: Option<&str>,
    command: Option<&str>,
    print: Option<Option<&str>>,
) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;

    let pane_id = srv.next_pane_id;
    srv.next_pane_id += 1;
    let new_win_id = srv.next_window_id;

    // 继承会话默认尺寸。
    let (w, h) = {
        let s = find_session(&srv, socket, &r.session).unwrap();
        (s.width, s.height)
    };
    let cwd_eff = cwd.map(|s| s.to_string()).or_else(|| {
        find_session(&srv, socket, &r.session)
            .and_then(|s| s.windows.get(r.window_index))
            .and_then(|win| win.panes.get(r.pane_index))
            .and_then(|p| p.cwd.clone())
    });
    let pane = spawn_pane(pane_id, w, h, cwd_eff.as_deref(), shell, command)?;

    let (win_index, pane_index) = {
        let s = find_session_mut(&mut srv, socket, &r.session).unwrap();
        if new_window {
            let idx = s.windows.len();
            let name = window_name.unwrap_or("shell").to_string();
            s.windows.push(Window { id: new_win_id, name, panes: vec![pane], active_pane: 0 });
            s.active_window = idx;
            (idx, 0usize)
        } else {
            let win = &mut s.windows[r.window_index];
            win.panes.push(pane);
            let pidx = win.panes.len() - 1;
            win.active_pane = pidx;
            s.active_window = r.window_index;
            (r.window_index, pidx)
        }
    };
    if new_window {
        srv.next_window_id += 1;
    }

    Ok(match print {
        None => String::new(),
        Some(fmt) => {
            let tpl = fmt
                .filter(|s| !s.is_empty())
                .unwrap_or("#{session_name}:#{window_index}.#{pane_id}");
            tpl.replace("#{session_name}", &r.session)
                .replace("#{window_index}", &win_index.to_string())
                .replace("#{pane_index}", &pane_index.to_string())
                .replace("#{pane_id}", &format!("%{pane_id}"))
        }
    })
}

/// `send-keys`：把已转好字节的文本写进目标面板的 master。
pub fn send_keys(socket: &str, target: &str, gui: &[GuiSession], text: &str) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let s = find_session_mut(&mut srv, socket, &r.session)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {}", r.session)))?;
    let win = s
        .windows
        .get_mut(r.window_index)
        .ok_or_else(|| NativeError::NotFound("can't find window".into()))?;
    let pane = win
        .panes
        .get_mut(r.pane_index)
        .ok_or_else(|| NativeError::NotFound("can't find pane".into()))?;
    {
        let mut w = pane.writer.lock();
        w.write_all(text.as_bytes())
            .and_then(|_| w.flush())
            .map_err(|e| NativeError::Internal(format!("write: {e}")))?;
    }
    Ok(String::new())
}

// ===================== capture-pane =====================

/// `capture-pane -p`：把目标面板**当前屏**渲成纯文本。从面板的输出环形缓冲取
/// 快照，喂一台与面板同尺寸的一次性 `Terminal`，再 `dump_visible_text()` —— 故能
/// 忠实还原 claude 这类全屏 TUI 的屏幕，而非堆叠转义序列。`lines` 为 `Some(n)`
/// 时只回末 n 行（取尾）。
pub fn capture(
    socket: &str,
    target: &str,
    gui: &[GuiSession],
    lines: Option<usize>,
) -> NativeResult {
    // 仅在锁内取尺寸 + 克隆 ring 的 Arc，随即释放全局锁，避免在 vt 解析期间占锁。
    let (cols, rows, ring) = {
        let srv = registry().lock().unwrap();
        let r = resolve_locked(&srv, socket, target, gui)?;
        let s = find_session(&srv, socket, &r.session)
            .ok_or_else(|| NativeError::NotFound(format!("can't find session: {}", r.session)))?;
        let w = s
            .windows
            .get(r.window_index)
            .ok_or_else(|| NativeError::NotFound("can't find window".into()))?;
        let p = w
            .panes
            .get(r.pane_index)
            .ok_or_else(|| NativeError::NotFound("can't find pane".into()))?;
        (p.width.max(1), p.height.max(1), p.ring.clone())
    };
    let snapshot: Vec<u8> = {
        let g = ring.lock().unwrap();
        g.iter().copied().collect()
    };
    let mut term = Terminal::new(rows as usize, cols as usize, 0);
    term.feed(&snapshot);
    Ok(finalize_capture(term.dump_visible_text(), lines))
}

/// 去掉尾部全空行（与 tmux `capture-pane` 默认一致），可选取末 `n` 行后 join。纯逻辑。
fn finalize_capture(mut rows_text: Vec<String>, lines: Option<usize>) -> String {
    while rows_text.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        rows_text.pop();
    }
    if let Some(n) = lines {
        if rows_text.len() > n {
            rows_text = rows_text.split_off(rows_text.len() - n);
        }
    }
    rows_text.join("\n")
}

// ===================== GUI 召唤（attach 进 Ridge 工作区） =====================

/// 召唤计划里的单个面板：携带与 native 面板**共享**的 writer/master、实时输出
/// 订阅端、首屏 replay 快照，供上层建一个领养 GUI pane（共享 PTY，不新开 shell）。
pub struct SummonPane {
    pub global_id: usize,
    pub width: u16,
    pub height: u16,
    pub cwd: Option<String>,
    pub writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,
    pub master: Arc<parking_lot::Mutex<Box<dyn MasterPty + Send>>>,
    pub rx: broadcast::Receiver<Vec<u8>>,
    pub replay: Vec<u8>,
    /// 召唤前若已挂在别处，给出旧 (ws,pane) 供上层先摘除旧视图。
    pub prev_attachment: Option<(Uuid, Uuid)>,
}

/// `attach` 语义：解析目标会话 → 列出其**活动窗口**各面板，逐个给出共享句柄 +
/// 实时订阅 + 首屏快照。不杀不改子进程，纯挂视图。
pub fn summon(
    socket: &str,
    target: &str,
    gui: &[GuiSession],
) -> Result<Vec<SummonPane>, NativeError> {
    let srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let s = find_session(&srv, socket, &r.session)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {}", r.session)))?;
    let w = s
        .windows
        .get(s.active_window)
        .ok_or_else(|| NativeError::NotFound("can't find window".into()))?;
    let mut out = Vec::with_capacity(w.panes.len());
    for p in &w.panes {
        let replay: Vec<u8> = {
            let g = p.ring.lock().unwrap();
            g.iter().copied().collect()
        };
        out.push(SummonPane {
            global_id: p.global_id,
            width: p.width,
            height: p.height,
            cwd: p.cwd.clone(),
            writer: p.writer.clone(),
            master: p.master.clone(),
            rx: p.output_tx.subscribe(),
            replay,
            prev_attachment: p.attachment,
        });
    }
    Ok(out)
}

/// 记账：标记/清除某 native 面板当前挂载到哪个 (ws,pane)。
pub fn set_attachment(socket: &str, global_id: usize, att: Option<(Uuid, Uuid)>) {
    let mut srv = registry().lock().unwrap();
    if let Some(sock) = srv.sockets.get_mut(socket) {
        for s in sock.sessions.iter_mut() {
            for win in s.windows.iter_mut() {
                for p in win.panes.iter_mut() {
                    if p.global_id == global_id {
                        p.attachment = att;
                        return;
                    }
                }
            }
        }
    }
}

/// 把 native 面板的输出广播包装成阻塞 `Read`，喂给现有 `spawn_pty_reader`，使领养
/// GUI pane 走与普通 pane **完全一致**的渲染路径。先放首屏 replay，再放实时流；
/// `cancel` 置位（detach）或广播关闭（子进程亡）→ 返回 EOF，干净结束 reader 线程。
pub struct BroadcastReader {
    rx: broadcast::Receiver<Vec<u8>>,
    buf: VecDeque<u8>,
    replay: VecDeque<u8>,
    cancel: Arc<AtomicBool>,
}

impl BroadcastReader {
    pub fn new(rx: broadcast::Receiver<Vec<u8>>, replay: Vec<u8>, cancel: Arc<AtomicBool>) -> Self {
        Self { rx, buf: VecDeque::new(), replay: replay.into(), cancel }
    }

    fn drain_into(src: &mut VecDeque<u8>, out: &mut [u8]) -> usize {
        let n = src.len().min(out.len());
        for (i, b) in src.drain(..n).enumerate() {
            out[i] = b;
        }
        n
    }
}

impl Read for BroadcastReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        // 先排空首屏 replay（保证召唤瞬间即有当前屏）。
        if !self.replay.is_empty() {
            return Ok(Self::drain_into(&mut self.replay, out));
        }
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(0); // detach → EOF
            }
            if !self.buf.is_empty() {
                return Ok(Self::drain_into(&mut self.buf, out));
            }
            match self.rx.blocking_recv() {
                Ok(chunk) => self.buf.extend(chunk),
                Err(broadcast::error::RecvError::Lagged(_)) => continue, // 丢帧，TUI 下帧自愈
                Err(broadcast::error::RecvError::Closed) => return Ok(0), // 子进程亡 → EOF
            }
        }
    }
}

// ===================== 列表 / 渲染 =====================

fn created_str(t: SystemTime) -> String {
    let dt: DateTime<Local> = DateTime::<Utc>::from(t).with_timezone(&Local);
    dt.format("%a %b %d %H:%M:%S %Y").to_string()
}

/// 渲染单个会话一行（默认格式或 `-F`）。`attached` 来自 GUI 概念，native 永远未 attach。
fn render_session_line(s: &Session, attached: bool, fmt: Option<&str>) -> String {
    match fmt {
        None => {
            let mut line = format!(
                "{}: {} windows (created {}) [{}x{}]",
                s.name,
                s.windows.len(),
                created_str(s.created_at),
                s.width,
                s.height
            );
            if attached {
                line.push_str(" (attached)");
            }
            line
        }
        Some(f) => {
            let vars = session_vars(s, attached);
            render_format(f, &vars)
        }
    }
}

fn session_vars(s: &Session, attached: bool) -> Vec<(&'static str, String)> {
    vec![
        ("#{session_name}", s.name.clone()),
        ("#{session_id}", format!("${}", s.id)),
        ("#{session_attached}", if attached { "1" } else { "0" }.to_string()),
        ("#{session_windows}", s.windows.len().to_string()),
        ("#{session_width}", s.width.to_string()),
        ("#{session_height}", s.height.to_string()),
        ("#S", s.name.clone()),
    ]
}

fn pane_vars(
    s: &Session,
    wi: usize,
    w: &Window,
    pi: usize,
    p: &Pane,
) -> Vec<(&'static str, String)> {
    vec![
        ("#{session_name}", s.name.clone()),
        ("#{session_id}", format!("${}", s.id)),
        ("#{window_index}", wi.to_string()),
        ("#{window_id}", format!("@{}", w.id)),
        ("#{window_name}", w.name.clone()),
        ("#{window_active}", if wi == s.active_window { "1" } else { "0" }.to_string()),
        ("#{window_panes}", w.panes.len().to_string()),
        ("#{pane_id}", format!("%{}", p.global_id)),
        ("#{pane_index}", pi.to_string()),
        ("#{pane_active}", if pi == w.active_pane { "1" } else { "0" }.to_string()),
        ("#{pane_width}", p.width.to_string()),
        ("#{pane_height}", p.height.to_string()),
        ("#{pane_current_path}", p.cwd.clone().unwrap_or_default()),
        ("#{pane_title}", w.name.clone()),
        ("#S", s.name.clone()),
        ("#I", wi.to_string()),
        ("#P", pi.to_string()),
        ("#D", format!("%{}", p.global_id)),
        ("#W", w.name.clone()),
    ]
}

/// 通用 `-F` 渲染：替换已知 `#{...}` token 与短别名。
fn render_format(fmt: &str, vars: &[(&'static str, String)]) -> String {
    let mut out = fmt.to_string();
    // 先替换长 token，再短别名，避免 `#S` 误伤 `#{session_name}`（其实不冲突，稳妥起见排序）。
    for (k, v) in vars.iter().filter(|(k, _)| k.starts_with("#{")) {
        out = out.replace(k, v);
    }
    for (k, v) in vars.iter().filter(|(k, _)| !k.starts_with("#{")) {
        out = out.replace(k, v);
    }
    out
}

/// 该 socket 上所有 native 会话的 `ls` 行。
pub fn list_sessions_lines(socket: &str, fmt: Option<&str>) -> Vec<String> {
    let srv = registry().lock().unwrap();
    srv.sockets
        .get(socket)
        .map(|sock| {
            sock.sessions
                .iter()
                .map(|s| render_session_line(s, false, fmt))
                .collect()
        })
        .unwrap_or_default()
}

/// 跨所有 socket 列出 native 会话的结构化信息，供 GUI 侧边栏展示。
#[derive(Debug, Clone, Serialize)]
pub struct NativeSessionInfo {
    pub socket: String,
    pub name: String,
    pub windows: usize,
    pub panes: usize,
    pub width: u16,
    pub height: u16,
    pub attached: bool,
}

/// 遍历所有 socket 的所有 native 会话，返回摘要列表。
pub fn list_all_sessions() -> Vec<NativeSessionInfo> {
    let srv = registry().lock().unwrap();
    let mut out = Vec::new();
    for (socket_name, sock) in &srv.sockets {
        for s in &sock.sessions {
            let total_panes: usize = s.windows.iter().map(|w| w.panes.len()).sum();
            let attached = s.windows.iter().any(|w| {
                w.panes.iter().any(|p| p.attachment.is_some())
            });
            out.push(NativeSessionInfo {
                socket: socket_name.clone(),
                name: s.name.clone(),
                windows: s.windows.len(),
                panes: total_panes,
                width: s.width,
                height: s.height,
                attached,
            });
        }
    }
    out
}

/// `list-panes -t SESSION`：目标会话当前窗口（或全部）每面板一行。
pub fn list_panes_lines(
    socket: &str,
    target: &str,
    gui: &[GuiSession],
    fmt: Option<&str>,
    all_windows: bool,
) -> Result<Vec<String>, NativeError> {
    let srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let s = find_session(&srv, socket, &r.session)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {}", r.session)))?;

    let mut lines = Vec::new();
    let windows: Vec<usize> = if all_windows {
        (0..s.windows.len()).collect()
    } else {
        vec![r.window_index]
    };
    for wi in windows {
        let w = &s.windows[wi];
        for (pi, p) in w.panes.iter().enumerate() {
            let line = match fmt {
                Some(f) => render_format(f, &pane_vars(s, wi, w, pi, p)),
                None => {
                    let mut l = format!("{pi}: [{}x{}] %{}", p.width, p.height, p.global_id);
                    if pi == w.active_pane && wi == s.active_window {
                        l.push_str(" (active)");
                    }
                    l
                }
            };
            lines.push(line);
        }
    }
    Ok(lines)
}

/// `list-windows -t SESSION`。
pub fn list_windows_lines(
    socket: &str,
    target: &str,
    gui: &[GuiSession],
    fmt: Option<&str>,
) -> Result<Vec<String>, NativeError> {
    let srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let s = find_session(&srv, socket, &r.session)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {}", r.session)))?;
    let mut lines = Vec::new();
    for (wi, w) in s.windows.iter().enumerate() {
        let active_pane = w.panes.get(w.active_pane);
        let line = match fmt {
            Some(f) => {
                // 用窗口的活动面板补足 pane_* 变量。
                let vars = match active_pane {
                    Some(p) => pane_vars(s, wi, w, w.active_pane, p),
                    None => vec![
                        ("#{window_index}", wi.to_string()),
                        ("#{window_name}", w.name.clone()),
                        ("#{session_name}", s.name.clone()),
                    ],
                };
                render_format(f, &vars)
            }
            None => {
                let mut l = format!(
                    "{wi}: {} ({} panes) [{}x{}] @{}",
                    w.name,
                    w.panes.len(),
                    s.width,
                    s.height,
                    w.id
                );
                if wi == s.active_window {
                    l.push_str(" (active)");
                }
                l
            }
        };
        lines.push(line);
    }
    Ok(lines)
}

/// `display-message -p -F` 针对已解析目标渲染一行。
pub fn display_message(
    socket: &str,
    target: &str,
    gui: &[GuiSession],
    fmt: &str,
) -> NativeResult {
    let srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let s = find_session(&srv, socket, &r.session)
        .ok_or_else(|| NativeError::NotFound(format!("can't find session: {}", r.session)))?;
    let w = &s.windows[r.window_index];
    let p = &w.panes[r.pane_index];
    Ok(render_format(fmt, &pane_vars(s, r.window_index, w, r.pane_index, p)))
}

// ===================== 销毁 =====================

/// `kill-session -t TARGET`。
pub fn kill_session(socket: &str, target: &str, gui: &[GuiSession]) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    if let Some(sock) = srv.sockets.get_mut(socket) {
        if let Some(pos) = sock.sessions.iter().position(|s| s.name == r.session) {
            let mut sess = sock.sessions.remove(pos);
            kill_session_panes(&mut sess);
        }
    }
    Ok(String::new())
}

/// `kill-pane -t TARGET`。
pub fn kill_pane(socket: &str, target: &str, gui: &[GuiSession]) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let mut remove_session = false;
    if let Some(s) = find_session_mut(&mut srv, socket, &r.session) {
        if let Some(w) = s.windows.get_mut(r.window_index) {
            if r.pane_index < w.panes.len() {
                let mut p = w.panes.remove(r.pane_index);
                let _ = p.child.kill();
                if w.active_pane >= w.panes.len() {
                    w.active_pane = w.panes.len().saturating_sub(1);
                }
            }
        }
        // 窗口空了则移除；会话空了则在释放 `s` 借用后再删。
        if s.windows.get(r.window_index).map(|w| w.panes.is_empty()).unwrap_or(false) {
            s.windows.remove(r.window_index);
            if s.active_window >= s.windows.len() {
                s.active_window = s.windows.len().saturating_sub(1);
            }
        }
        remove_session = s.windows.is_empty();
    }
    if remove_session {
        if let Some(sock) = srv.sockets.get_mut(socket) {
            sock.sessions.retain(|x| x.name != r.session);
        }
    }
    Ok(String::new())
}

/// `kill-window -t TARGET`。
pub fn kill_window(socket: &str, target: &str, gui: &[GuiSession]) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    let mut remove_session = false;
    if let Some(s) = find_session_mut(&mut srv, socket, &r.session) {
        if r.window_index < s.windows.len() {
            let mut w = s.windows.remove(r.window_index);
            for p in w.panes.iter_mut() {
                let _ = p.child.kill();
            }
            if s.active_window >= s.windows.len() {
                s.active_window = s.windows.len().saturating_sub(1);
            }
        }
        remove_session = s.windows.is_empty();
    }
    if remove_session {
        if let Some(sock) = srv.sockets.get_mut(socket) {
            sock.sessions.retain(|x| x.name != r.session);
        }
    }
    Ok(String::new())
}

/// `kill-server`：清空该 socket 的所有 native 会话。
pub fn kill_server(socket: &str) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    if let Some(mut sock) = srv.sockets.remove(socket) {
        for s in sock.sessions.iter_mut() {
            kill_session_panes(s);
        }
    }
    Ok(String::new())
}

/// `select-pane`/`select-window`/`select-layout` 等：记账（更新 active），其余 no-op。
pub fn select(socket: &str, target: &str, gui: &[GuiSession]) -> NativeResult {
    let mut srv = registry().lock().unwrap();
    let r = resolve_locked(&srv, socket, target, gui)?;
    if let Some(s) = find_session_mut(&mut srv, socket, &r.session) {
        s.active_window = r.window_index.min(s.windows.len().saturating_sub(1));
        if let Some(w) = s.windows.get_mut(r.window_index) {
            w.active_pane = r.pane_index.min(w.panes.len().saturating_sub(1));
        }
    }
    Ok(String::new())
}

fn kill_session_panes(s: &mut Session) {
    for w in s.windows.iter_mut() {
        for p in w.panes.iter_mut() {
            let _ = p.child.kill();
        }
    }
}

// ===================== 小工具 =====================

fn find_session<'a>(srv: &'a NativeServer, socket: &str, name: &str) -> Option<&'a Session> {
    srv.sockets.get(socket)?.sessions.iter().find(|s| s.name == name)
}

fn find_session_mut<'a>(
    srv: &'a mut NativeServer,
    socket: &str,
    name: &str,
) -> Option<&'a mut Session> {
    srv.sockets.get_mut(socket)?.sessions.iter_mut().find(|s| s.name == name)
}

fn basename(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.rsplit('/').next().unwrap_or(&p).trim_end_matches(".exe").to_string()
}

// ===================== 单元测试（纯逻辑） =====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_eq_exact() {
        let pt = parse_target("=probe");
        assert!(pt.exact);
        assert_eq!(pt.session.as_deref(), Some("probe"));
    }

    #[test]
    fn parse_session_dot_pane() {
        let pt = parse_target("probe.0");
        assert_eq!(pt.session.as_deref(), Some("probe"));
        assert_eq!(pt.pane, Some(0));
    }

    #[test]
    fn parse_session_window_pane() {
        let pt = parse_target("probe:1.2");
        assert_eq!(pt.session.as_deref(), Some("probe"));
        assert_eq!(pt.window, Some(1));
        assert_eq!(pt.pane, Some(2));
    }

    #[test]
    fn parse_pane_global() {
        let pt = parse_target("%7");
        assert_eq!(pt.pane_global, Some(7));
    }

    #[test]
    fn glob_basics() {
        assert!(glob_match("pro*", "probe"));
        assert!(glob_match("*be", "probe"));
        assert!(glob_match("p?obe", "probe"));
        assert!(!glob_match("xyz*", "probe"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn match_priority_exact_over_prefix() {
        let names = vec!["pro".to_string(), "probe".to_string()];
        assert_eq!(match_session_name("pro", false, &names).unwrap(), "pro");
    }

    #[test]
    fn match_exact_flag_rejects_prefix() {
        let names = vec!["probe".to_string()];
        assert!(match_session_name("pro", true, &names).is_err());
        assert_eq!(match_session_name("probe", true, &names).unwrap(), "probe");
    }

    #[test]
    fn match_unknown_is_not_found() {
        let names = vec!["probe".to_string()];
        match match_session_name("nope_xyz", false, &names) {
            Err(NativeError::NotFound(m)) => assert!(m.contains("can't find session")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn match_ambiguous_prefix() {
        let names = vec!["proa".to_string(), "prob".to_string()];
        assert!(matches!(
            match_session_name("pro", false, &names),
            Err(NativeError::Ambiguous(_))
        ));
    }

    #[test]
    fn render_format_session_name() {
        let s = Session {
            id: 3,
            name: "probe".to_string(),
            created_at: SystemTime::now(),
            windows: vec![],
            active_window: 0,
            width: 200,
            height: 50,
        };
        let out = render_format("X=#{session_name}", &session_vars(&s, false));
        assert_eq!(out, "X=probe");
    }

    #[test]
    fn finalize_capture_trims_and_tails() {
        let rows = vec![
            "line1".to_string(),
            "line2".to_string(),
            "".to_string(),
            "   ".to_string(),
        ];
        // 去掉尾部全空行
        assert_eq!(finalize_capture(rows.clone(), None), "line1\nline2");
        // 先去尾空行，再取末 1 行
        assert_eq!(finalize_capture(rows, Some(1)), "line2");
    }
}
