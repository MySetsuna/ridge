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
    scrollback: Arc<Mutex<ScrollbackRing>>,
    modes: Arc<Mutex<ModeTracker>>,
}

impl SessionHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    pub fn subscribe_with_backlog(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let ring = self.scrollback.lock().unwrap();
        (ring.snapshot(), self.output_tx.subscribe())
    }

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
        Self {
            sessions: Vec::new(),
            pane_tree: PaneTree::new(),
            default_session_index: 0,
        }
    }

    /// Create a Kernel-owned PTY only after its stable pane identity is known.
    /// Layout mutations are prepared on a clone so a failed Kernel request
    /// cannot leave a pane without a session.
    pub fn create_session(
        &mut self,
        shell: Option<&str>,
        cwd: Option<&str>,
        split_target: Option<Uuid>,
        split_dir: SplitDirection,
    ) -> Result<Uuid> {
        let actual_cwd = cwd
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|path| path.to_str().map(str::to_owned))
            });

        let (id, next_tree) = if self.sessions.is_empty() {
            let tree = PaneTree::new();
            (tree.get_all_leaves()[0], tree)
        } else if let Some(target) = split_target {
            let mut tree = self.pane_tree.clone();
            match tree.split(target, split_dir) {
                Ok(new_id) => (new_id, tree),
                Err(_) => (Uuid::new_v4(), self.pane_tree.clone()),
            }
        } else {
            (Uuid::new_v4(), self.pane_tree.clone())
        };

        let (session, rx) = LocalPtySession::spawn_with_id(id, shell, actual_cwd.as_deref())?;
        self.pane_tree = next_tree;

        let (tx, _) = broadcast::channel(BROADCAST_CAP);
        let scrollback = Arc::new(Mutex::new(ScrollbackRing::new(DEFAULT_SCROLLBACK_CAP)));
        let modes = Arc::new(Mutex::new(ModeTracker::new()));
        let tx2 = tx.clone();
        let scrollback2 = scrollback.clone();
        let modes2 = modes.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(bytes) = rx.recv().await {
                let mut ring = scrollback2.lock().unwrap();
                ring.append(&bytes);
                modes2.lock().unwrap().feed(&bytes);
                let _ = tx2.send(bytes);
            }
        });

        self.sessions.push(SessionHandle {
            id,
            title: actual_cwd.as_deref().unwrap_or("shell").to_string(),
            cwd: actual_cwd,
            session: Arc::new(session),
            output_tx: tx,
            scrollback,
            modes,
        });
        Ok(id)
    }

    pub fn find(&self, id: Uuid) -> Option<&SessionHandle> {
        self.sessions.iter().find(|session| session.id == id)
    }

    pub fn find_index(&self, id: Uuid) -> Option<usize> {
        self.sessions.iter().position(|session| session.id == id)
    }

    pub fn default_session_id(&self) -> Option<Uuid> {
        self.sessions
            .get(self.default_session_index)
            .map(|session| session.id)
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
    pane_trees: Vec<PaneTree>,
}

impl WorkspaceManager {
    pub fn new(initial: SharedWorkspace) -> Self {
        let pane_tree = initial.lock().unwrap().pane_tree.clone();
        Self {
            workspaces: vec![initial],
            active_ws: 0,
            active_session: 0,
            pane_trees: vec![pane_tree],
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
            .and_then(|workspace| workspace.sessions.get(self.active_session).cloned())
    }

    pub fn navigate(&mut self, direction: Direction) -> bool {
        let workspace = self.workspaces[self.active_ws].lock().unwrap();
        let Some(current) = workspace.sessions.get(self.active_session) else {
            return false;
        };
        let Some(neighbor_id) = self.pane_trees[self.active_ws].neighbor(current.id, direction)
        else {
            return false;
        };
        let Some(index) = workspace.find_index(neighbor_id) else {
            return false;
        };
        self.active_session = index;
        true
    }

    pub fn split_active_session(
        &mut self,
        shell: Option<&str>,
        cwd: Option<&str>,
        direction: SplitDirection,
    ) -> Result<Uuid> {
        let split_target = {
            let workspace = self.workspaces[self.active_ws].lock().unwrap();
            workspace.sessions.get(self.active_session).map(|s| s.id)
        };
        let mut workspace = self.workspaces[self.active_ws].lock().unwrap();
        let id = workspace.create_session(shell, cwd, split_target, direction)?;
        self.pane_trees[self.active_ws] = workspace.pane_tree.clone();
        Ok(id)
    }

    pub fn resize_all(&self, cols: u16, rows: u16) {
        for workspace in &self.workspaces {
            let sessions = workspace.lock().unwrap().sessions.clone();
            for session in &sessions {
                let _ = session.resize(cols, rows);
            }
        }
    }

    pub fn switch_workspace(&mut self, number: u8) -> bool {
        let index = (number as usize).saturating_sub(1);
        if index >= self.workspaces.len() {
            return false;
        }
        let changed = self.active_ws != index;
        self.active_ws = index;
        self.active_session = self.workspaces[self.active_ws]
            .lock()
            .unwrap()
            .default_session_index;
        changed
    }

    pub fn add_workspace(&mut self, workspace: SharedWorkspace) -> bool {
        if self.workspaces.len() >= 12 {
            return false;
        }
        self.pane_trees
            .push(workspace.lock().unwrap().pane_tree.clone());
        self.workspaces.push(workspace);
        true
    }

    pub fn session_titles(&self) -> Vec<String> {
        self.workspaces[self.active_ws]
            .lock()
            .unwrap()
            .sessions
            .iter()
            .map(|session| session.title.clone())
            .collect()
    }

    pub fn status_bar_text(&self, cols: u16) -> String {
        let titles = self.session_titles();
        let mut bar = String::new();
        for (index, title) in titles.iter().enumerate() {
            if index == self.active_session {
                bar.push_str(&format!(" [{}] *{}* ", index + 1, title));
            } else {
                bar.push_str(&format!(" [{}] {} ", index + 1, title));
            }
        }
        if self.workspaces.len() > 1 {
            bar.push_str(&format!(
                "  WS:{}/{}",
                self.active_ws + 1,
                self.workspaces.len()
            ));
        }
        bar.push_str(
            "  \x1b[2mCtrl+Shift+方向\x1b[0m  \x1b[2mCtrl+F\x1b[0m ws  \x1b[2mCtrl+]\x1b[0m quit",
        );
        bar.truncate(bar.len().min(cols as usize));
        bar
    }
}
