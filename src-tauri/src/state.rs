use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::RwLock;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::db::ProjectStore;
use crate::engine::pane_tree::PaneTree;
use crate::engine::pty::PtyHandle;
use crate::types::GlobalEvent;

/// Claude Code `wind-tmux` 连接本地控制面所需（注入到子 shell）。
#[derive(Clone, Debug)]
pub struct TeammateBinding {
    pub base_url: String,
    pub token: String,
}

/// 单个根会话：独立分屏树 + 终端句柄（多工作区互不共享 pane id 命名空间下的 PTY 表）。
pub struct Workspace {
    pub pane_tree: PaneTree,
    pub terminals: HashMap<Uuid, PtyHandle>,
    /// Claude `send-keys -t ""` / 无 `-t` 时 tmux「当前窗格」：在 Wind 里对应 `split-window` / `select-pane` 最后指向的 pane 索引。
    pub teammate_tmux_pane_cursor: usize,
    /// 工作区创建时间（`list-sessions` 等 tmux 兼容输出用）。
    pub created_at: SystemTime,
}

#[derive(Clone)]
pub struct AppState {
    pub workspaces: Arc<RwLock<HashMap<Uuid, Workspace>>>,
    pub workspace_order: Arc<RwLock<Vec<Uuid>>>,
    pub workspace_names: Arc<RwLock<HashMap<Uuid, String>>>,
    pub active_workspace: Arc<RwLock<Uuid>>,
    pub event_tx: mpsc::Sender<GlobalEvent>,
    /// 供 `capture-pane` 读取的最近输出（与 UI 展示同源 PTY 流）。
    pub pty_scrollback: Arc<RwLock<HashMap<(Uuid, Uuid), String>>>,
    /// 本进程 teammate HTTP 绑定信息；存在时新 PTY 会注入 WIND_TEAMMATE_*。
    pub teammate_binding: Arc<RwLock<Option<TeammateBinding>>>,
    /// Project store for managing projects
    pub project_store: Option<Arc<ProjectStore>>,
    /// Current active project path
    pub current_project: Arc<RwLock<Option<PathBuf>>>,
}

impl AppState {
    pub fn new(event_tx: mpsc::Sender<GlobalEvent>) -> Self {
        let id = Uuid::new_v4();
        let mut map = HashMap::new();
        map.insert(
            id,
            Workspace {
                pane_tree: PaneTree::new(),
                terminals: HashMap::new(),
                teammate_tmux_pane_cursor: 0,
                created_at: SystemTime::now(),
            },
        );
        Self {
            workspaces: Arc::new(RwLock::new(map)),
            workspace_order: Arc::new(RwLock::new(vec![id])),
        workspace_names: Arc::new(RwLock::new(HashMap::new())),
            active_workspace: Arc::new(RwLock::new(id)),
            event_tx,
            pty_scrollback: Arc::new(RwLock::new(HashMap::new())),
            teammate_binding: Arc::new(RwLock::new(None)),
            project_store: None,
            current_project: Arc::new(RwLock::new(None)),
        }
    }

    pub fn active_workspace_id(&self) -> Uuid {
        *self.active_workspace.read()
    }

    pub fn append_pty_scrollback(&self, ws: Uuid, pane: Uuid, chunk: &str) {
        const MAX: usize = 384 * 1024;
        let mut map = self.pty_scrollback.write();
        let buf = map.entry((ws, pane)).or_insert_with(String::new);
        buf.push_str(chunk);
    if buf.len() > MAX {
        // Drain requires character boundaries, not byte offsets.
        // Find the nearest valid character boundary before the cut point.
        let cut_point = buf.len() - MAX;
        let mut valid_boundary = cut_point;
        while !buf.is_char_boundary(valid_boundary) {
            valid_boundary += 1;
        }
        buf.drain(..valid_boundary);
    }
    }

    pub fn clear_pty_scrollback(&self, ws: Uuid, pane: Uuid) {
        self.pty_scrollback.write().remove(&(ws, pane));
    }

    pub fn get_pty_scrollback_tail(&self, ws: Uuid, pane: Uuid, max_lines: usize) -> String {
        let map = self.pty_scrollback.read();
        let Some(s) = map.get(&(ws, pane)) else {
            return String::new();
        };
        let lines: Vec<&str> = s.split('\n').collect();
        if lines.len() <= max_lines {
            return s.clone();
        }
        lines[lines.len() - max_lines..].join("\n")
    }
}
