use std::sync::{Arc, Mutex};

use anyhow::Result;
use ridge_core::workspace::pane_tree::{Direction, PaneTree, SplitDirection};
use ridge_term::term::modes::Modes;
use ridge_term::term::ModeTracker;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::scrollback::{ScrollbackRing, DEFAULT_SCROLLBACK_CAP};
use super::session::{LocalPtySession, Session};

const BROADCAST_CAP: usize = 256;

#[derive(Clone)]
pub struct SessionHandle {
    pub id: Uuid,
    pub title: String,
    pub cwd: Option<String>,
    pub session: Arc<LocalPtySession>,
    output_tx: broadcast::Sender<Vec<u8>>,
    /// 该 pane 的原始 PTY 输出环（#21）：subscribe 时回放历史消除新连/重连黑屏。
    /// 写者任务与 [`subscribe_with_backlog`](Self::subscribe_with_backlog) 共用此锁，
    /// 令每个输出块恰落 {backlog, live} 之一。
    scrollback: Arc<Mutex<ScrollbackRing>>,
    /// §mode-reattach — 该 pane 的 live 终端模式追踪（复用 `ridge_term` 解析核，不
    /// 渲染）。writer task 每 PTY chunk feed；subscribe/resync 时取快照，令控制端镜像
    /// 内核重建鼠标上报/alt 屏等一次性开启态（早滑出 scrollback 尾）。与 scrollback 同
    /// 在写者任务的临界区内推进，故 backlog 与 modes 快照一致。
    modes: Arc<Mutex<ModeTracker>>,
}

impl SessionHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    /// 原子快照该 pane 的 scrollback backlog 并订阅实时输出。两步在同一把 scrollback
    /// 锁下完成，使 (backlog, live) 接缝无竞态：写者任务也在同一把锁下 append+send，
    /// 故每个输出块恰好落入 {backlog, live} 之一，既不重也不漏。
    pub fn subscribe_with_backlog(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let ring = self.scrollback.lock().unwrap();
        let backlog = ring.snapshot();
        let rx = self.output_tx.subscribe();
        (backlog, rx)
    }

    /// §mode-reattach — 当前模式 + alt-screen 快照，供 subscribe/resync 帧经共享
    /// `ridge_remote::pane::pane_resync_frame` 重建控制端一次性开启态。与桌面
    /// `PaneParser`/`get_pane_modes` 同经 `ridge_term` 一个原语取值。
    pub fn modes_snapshot(&self) -> (Modes, bool) {
        self.modes.lock().unwrap().snapshot()
    }

    pub fn send_input(&self, data: &[u8]) -> Result<()> {
        self.session.send_input(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.session.resize(cols, rows)
    }
}

#[derive(Clone)]
pub struct Workspace {
    pub sessions: Vec<SessionHandle>,
    pub pane_tree: PaneTree,
    pub default_session_index: usize,
}

impl Workspace {
    pub fn new() -> Self {
        let pane_tree = PaneTree::new();
        Self {
            sessions: Vec::new(),
            pane_tree,
            default_session_index: 0,
        }
    }

    /// 创建一个新 PTY session，同时将其注册到 PaneTree 布局树。
    /// split_target 为 None 且 sessions 非空时仅创建 session 但不修改布局。
    pub fn create_session(
        &mut self,
        shell: Option<&str>,
        cwd: Option<&str>,
        split_target: Option<Uuid>,
        split_dir: SplitDirection,
    ) -> Result<Uuid> {
        let actual_cwd = cwd
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from))
            });
        let (session, rx) = LocalPtySession::spawn(shell, actual_cwd.as_deref())?;

        let (tx, _) = broadcast::channel(BROADCAST_CAP);
        let scrollback = Arc::new(Mutex::new(ScrollbackRing::new(DEFAULT_SCROLLBACK_CAP)));
        let modes = Arc::new(Mutex::new(ModeTracker::new()));
        let tx2 = tx.clone();
        let scrollback2 = scrollback.clone();
        let modes2 = modes.clone();
        tokio::spawn(async move {
            use tokio::sync::mpsc;
            let mut rx: mpsc::Receiver<Vec<u8>> = rx;
            while let Some(bytes) = rx.recv().await {
                // 同锁 append + feed modes + 广播 send，与 subscribe_with_backlog /
                // modes_snapshot 串行化 → backlog、modes 与 live 接缝一致无竞态。锁在
                // 本轮迭代内即取即放，绝不跨 await（下次 recv 前已释放）。modes 锁在
                // ring 锁内短暂嵌套取用（无 await）；读侧 subscribe/modes_snapshot 顺序
                // 取用不嵌套，故无死锁。
                let mut ring = scrollback2.lock().unwrap();
                ring.append(&bytes);
                modes2.lock().unwrap().feed(&bytes);
                let _ = tx2.send(bytes);
            }
        });

        let title = actual_cwd.as_deref().unwrap_or("shell").to_string();

        // 决定 session id：由 PaneTree 生成 leaf id，PTY session 与之对齐。
        let id = if self.sessions.is_empty() {
            // 首个 session：创建新 PaneTree，取 root leaf 的 id
            let tree = PaneTree::new();
            let root_id = tree.get_all_leaves()[0];
            self.pane_tree = tree;
            root_id
        } else if let Some(target) = split_target {
            // Split 目标 pane → PaneTree 生成新 leaf id
            match self.pane_tree.split(target, split_dir) {
                Ok(new_id) => new_id,
                Err(_) => {
                    // split 失败时回退到随机 id（session 仍在，只是没有布局关系）
                    Uuid::new_v4()
                }
            }
        } else {
            // 无 split 目标：随机 id，不修改布局树
            Uuid::new_v4()
        };

        self.sessions.push(SessionHandle {
            id,
            title,
            cwd: actual_cwd,
            session: Arc::new(session),
            output_tx: tx,
            scrollback,
            modes,
        });
        Ok(id)
    }

    pub fn find(&self, id: Uuid) -> Option<&SessionHandle> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn find_index(&self, id: Uuid) -> Option<usize> {
        self.sessions.iter().position(|s| s.id == id)
    }

    pub fn default_session_id(&self) -> Option<Uuid> {
        self.sessions.get(self.default_session_index).map(|s| s.id)
    }
}

pub type SharedWorkspace = Arc<Mutex<Workspace>>;

pub fn new_shared() -> SharedWorkspace {
    Arc::new(Mutex::new(Workspace::new()))
}

pub struct WorkspaceManager {
    workspaces: Vec<SharedWorkspace>,
    active_ws: usize,
    active_session: usize,
    /// 每个 workspace 对应的 PaneTree 缓存（单线程使用，不受 Mutex 保护）。
    pane_trees: Vec<PaneTree>,
}

impl WorkspaceManager {
    pub fn new(initial: SharedWorkspace) -> Self {
        let pt = initial.lock().unwrap().pane_tree.clone();
        Self {
            workspaces: vec![initial],
            active_ws: 0,
            active_session: 0,
            pane_trees: vec![pt],
        }
    }

    pub fn active_workspace_mut(&mut self) -> std::sync::MutexGuard<'_, Workspace> {
        self.workspaces[self.active_ws].lock().unwrap()
    }

    pub fn active_session_index(&self) -> usize {
        self.active_session
    }

    pub fn session_count(&self) -> usize {
        self.workspaces[self.active_ws]
            .lock()
            .unwrap()
            .sessions
            .len()
    }

    pub fn active_session_handle(&self) -> Option<SessionHandle> {
        self.workspaces[self.active_ws]
            .lock()
            .ok()
            .and_then(|ws| ws.sessions.get(self.active_session).cloned())
    }

    /// 沿物理方向导航到相邻 pane。
    pub fn navigate(&mut self, dir: Direction) -> bool {
        let ws = self.workspaces[self.active_ws].lock().unwrap();
        let Some(current) = ws.sessions.get(self.active_session) else {
            return false;
        };
        let pt = &self.pane_trees[self.active_ws];
        let Some(neighbor_id) = pt.neighbor(current.id, dir) else {
            return false;
        };
        let Some(idx) = ws.find_index(neighbor_id) else {
            return false;
        };
        self.active_session = idx;
        true
    }

    /// Split 当前活动 session 并创建新 session。
    pub fn split_active_session(
        &mut self,
        shell: Option<&str>,
        cwd: Option<&str>,
        dir: SplitDirection,
    ) -> Result<Uuid> {
        let split_target = {
            let ws = self.workspaces[self.active_ws].lock().unwrap();
            ws.sessions.get(self.active_session).map(|s| s.id)
        };
        let mut ws = self.workspaces[self.active_ws].lock().unwrap();
        let id = ws.create_session(shell, cwd, split_target, dir)?;
        // 同步 PaneTree 缓存
        self.pane_trees[self.active_ws] = ws.pane_tree.clone();
        Ok(id)
    }

    pub fn resize_all(&self, cols: u16, rows_for_content: u16) {
        for ws in &self.workspaces {
            let sessions = ws.lock().unwrap().sessions.clone();
            for s in &sessions {
                let _ = s.resize(cols, rows_for_content);
            }
        }
    }

    /// 用 Ctrl+F1..F12 切换工作区。n 从 1 开始（F1=1, …, F12=12）。
    pub fn switch_workspace(&mut self, n: u8) -> bool {
        let idx = (n as usize).saturating_sub(1);
        if idx >= self.workspaces.len() {
            return false;
        }
        let old_ws = self.active_ws;
        self.active_ws = idx;
        self.active_session = self.workspaces[self.active_ws]
            .lock()
            .unwrap()
            .default_session_index;
        self.active_ws != old_ws
    }

    pub fn add_workspace(&mut self, ws: SharedWorkspace) -> bool {
        if self.workspaces.len() >= 12 {
            return false;
        }
        let pt = ws.lock().unwrap().pane_tree.clone();
        self.workspaces.push(ws);
        self.pane_trees.push(pt);
        true
    }

    pub fn session_titles(&self) -> Vec<String> {
        self.workspaces[self.active_ws]
            .lock()
            .unwrap()
            .sessions
            .iter()
            .map(|s| s.title.clone())
            .collect()
    }

    pub fn status_bar_text(&self, cols: u16) -> String {
        let titles = self.session_titles();
        let active = self.active_session;
        let mut bar = String::new();

        for (i, title) in titles.iter().enumerate() {
            if i == active {
                bar.push_str(&format!(" [{}] *{}* ", i + 1, title));
            } else {
                bar.push_str(&format!(" [{}] {} ", i + 1, title));
            }
        }

        let ws_info = if self.workspaces.len() > 1 {
            format!("  WS:{}/{}", self.active_ws + 1, self.workspaces.len())
        } else {
            String::new()
        };

        bar.push_str(&ws_info);
        bar.push_str("  \x1b[2mCtrl+Shift+方向\x1b[0m  \x1b[2mCtrl+F\x1b[0m ws  \x1b[2mCtrl+]\x1b[0m quit");

        if bar.len() > cols as usize {
            bar.truncate(cols as usize);
        }
        bar
    }
}
