use std::sync::{Arc, Mutex};

use anyhow::Result;
use rand::Rng;
use tokio::sync::broadcast;

use super::session::{LocalPtySession, Session};

const BROADCAST_CAP: usize = 256;

pub struct SessionHandle {
    pub id: String,
    pub title: String,
    /// Current working directory at session creation time. Used by the terminal
    /// page to display the cwd in the status bar / session list.
    pub cwd: Option<String>,
    pub session: Arc<LocalPtySession>,
    output_tx: broadcast::Sender<Vec<u8>>,
}

impl SessionHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    pub fn send_input(&self, data: &[u8]) -> Result<()> {
        self.session.send_input(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.session.resize(cols, rows)
    }
}

pub struct Workspace {
    pub sessions: Vec<SessionHandle>,
    pub default_session_index: usize,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            default_session_index: 0,
        }
    }

    pub fn create_session(
        &mut self,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<String> {
        let actual_cwd = cwd
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from))
            });
        let (session, rx) = LocalPtySession::spawn(shell, actual_cwd.as_deref())?;
        let mut id_bytes = [0u8; 16];
        rand::thread_rng().fill(&mut id_bytes);
        let id = uuid_str(&id_bytes);
        let (tx, _) = broadcast::channel(BROADCAST_CAP);

        let tx2 = tx.clone();
        tokio::spawn(async move {
            use tokio::sync::mpsc;
            let mut rx: mpsc::Receiver<Vec<u8>> = rx;
            while let Some(bytes) = rx.recv().await {
                let _ = tx2.send(bytes);
            }
        });

        let title = actual_cwd.as_deref().unwrap_or("shell").to_string();
        self.sessions.push(SessionHandle {
            id: id.clone(),
            title,
            cwd: actual_cwd,
            session: Arc::new(session),
            output_tx: tx,
        });
        Ok(id)
    }

    pub fn find(&self, id: &str) -> Option<&SessionHandle> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn default_session_id(&self) -> Option<&str> {
        self.sessions.get(self.default_session_index).map(|s| s.id.as_str())
    }
}

pub type SharedWorkspace = Arc<Mutex<Workspace>>;

pub fn new_shared() -> SharedWorkspace {
    Arc::new(Mutex::new(Workspace::new()))
}

fn uuid_str(bytes: &[u8; 16]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
