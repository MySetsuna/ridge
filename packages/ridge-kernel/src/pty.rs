//! Shell-independent local PTY primitive shared by kernel hosts.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use ridge_term::term::terminal::Terminal;
use tokio::sync::{mpsc, Notify};
use uuid::Uuid;

const READ_BUF: usize = 8192;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const SCROLLBACK_CAP: usize = 1024 * 1024;
const RENDER_SCROLLBACK_ROWS: usize = 4096;
// The lease is a bounded replay seam, not a second unbounded scrollback.
// Keep this cap small enough for reconnects while preserving backpressure.
const OUTPUT_REPLAY_CAP_BYTES: usize = 256 * 1024;
const OUTPUT_REPLAY_CAP_FRAMES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyOutputFrame {
    pub seq: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyOutputRead {
    Data(Vec<PtyOutputFrame>),
    /// The caller's cursor fell behind the bounded replay window. It must
    /// explicitly call [`PtyOutputLease::resync`] before reading again.
    Lagged {
        requested_seq: u64,
        oldest_seq: u64,
        latest_seq: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyOutputLeaseError {
    Detached,
    Closing,
    Closed,
    TimedOut,
    InvalidBatchSize,
}

impl std::fmt::Display for PtyOutputLeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Detached => "PTY output lease detached",
            Self::Closing => "PTY output is closing",
            Self::Closed => "PTY output is closed",
            Self::TimedOut => "PTY output read timed out",
            Self::InvalidBatchSize => "PTY output batch size must be greater than zero",
        })
    }
}

impl std::error::Error for PtyOutputLeaseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputLifecycle {
    Open,
    Closing,
    Closed,
}

struct OutputState {
    lifecycle: OutputLifecycle,
    next_seq: u64,
    bytes: usize,
    frames: VecDeque<PtyOutputFrame>,
    leases: HashMap<Uuid, u64>,
}

/// In-memory, bounded output protocol for PTYs created through the domain
/// registry. The HTTP layer can later expose the same lease operations without
/// changing PTY ownership or introducing another output queue.
struct PtyOutputHub {
    state: Mutex<OutputState>,
    notify: Notify,
}

impl PtyOutputHub {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputState {
                lifecycle: OutputLifecycle::Open,
                next_seq: 1,
                bytes: 0,
                frames: VecDeque::new(),
                leases: HashMap::new(),
            }),
            notify: Notify::new(),
        }
    }

    fn publish(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut state = self.state.lock();
        if state.lifecycle != OutputLifecycle::Open {
            return;
        }
        let mut data = bytes.to_vec();
        if data.len() > OUTPUT_REPLAY_CAP_BYTES {
            data = data[data.len() - OUTPUT_REPLAY_CAP_BYTES..].to_vec();
        }
        let frame = PtyOutputFrame {
            seq: state.next_seq,
            data,
        };
        state.next_seq = state.next_seq.saturating_add(1);
        state.bytes += frame.data.len();
        state.frames.push_back(frame);
        while state.frames.len() > OUTPUT_REPLAY_CAP_FRAMES || state.bytes > OUTPUT_REPLAY_CAP_BYTES
        {
            if let Some(oldest) = state.frames.pop_front() {
                state.bytes = state.bytes.saturating_sub(oldest.data.len());
            } else {
                break;
            }
        }
        drop(state);
        self.notify.notify_waiters();
    }

    fn attach(
        self: &Arc<Self>,
        after_seq: Option<u64>,
    ) -> Result<PtyOutputLease, PtyOutputLeaseError> {
        let mut state = self.state.lock();
        match state.lifecycle {
            OutputLifecycle::Closing => return Err(PtyOutputLeaseError::Closing),
            OutputLifecycle::Closed => return Err(PtyOutputLeaseError::Closed),
            OutputLifecycle::Open => {}
        }
        let oldest = state
            .frames
            .front()
            .map(|frame| frame.seq)
            .unwrap_or(state.next_seq);
        let cursor = after_seq.map(|seq| seq.saturating_add(1)).unwrap_or(oldest);
        let id = Uuid::new_v4();
        state.leases.insert(id, cursor);
        Ok(PtyOutputLease {
            hub: Arc::clone(self),
            id,
        })
    }

    fn detach(&self, id: Uuid) -> bool {
        let removed = self.state.lock().leases.remove(&id).is_some();
        if removed {
            self.notify.notify_waiters();
        }
        removed
    }

    fn lifecycle_error(lifecycle: OutputLifecycle) -> PtyOutputLeaseError {
        match lifecycle {
            OutputLifecycle::Open => PtyOutputLeaseError::Detached,
            OutputLifecycle::Closing => PtyOutputLeaseError::Closing,
            OutputLifecycle::Closed => PtyOutputLeaseError::Closed,
        }
    }

    fn probe(&self, id: Uuid, max_frames: usize) -> ProbeResult {
        let mut state = self.state.lock();
        if state.lifecycle != OutputLifecycle::Open {
            return ProbeResult::Ready(Err(Self::lifecycle_error(state.lifecycle)));
        }
        let Some(cursor) = state.leases.get(&id).copied() else {
            return ProbeResult::Ready(Err(PtyOutputLeaseError::Detached));
        };
        let oldest = state
            .frames
            .front()
            .map(|frame| frame.seq)
            .unwrap_or(state.next_seq);
        let latest = state.next_seq.saturating_sub(1);
        if cursor < oldest {
            return ProbeResult::Ready(Ok(PtyOutputRead::Lagged {
                requested_seq: cursor,
                oldest_seq: oldest,
                latest_seq: latest,
            }));
        }
        let frames = state
            .frames
            .iter()
            .filter(|frame| frame.seq >= cursor)
            .take(max_frames)
            .cloned()
            .collect::<Vec<_>>();
        if frames.is_empty() {
            return ProbeResult::Pending;
        }
        let next_seq = frames
            .last()
            .map(|frame| frame.seq.saturating_add(1))
            .unwrap_or(cursor);
        if let Some(cursor) = state.leases.get_mut(&id) {
            *cursor = next_seq;
        }
        ProbeResult::Ready(Ok(PtyOutputRead::Data(frames)))
    }

    fn resync(&self, id: Uuid) -> Result<u64, PtyOutputLeaseError> {
        let mut state = self.state.lock();
        if state.lifecycle != OutputLifecycle::Open {
            return Err(Self::lifecycle_error(state.lifecycle));
        }
        let oldest = state
            .frames
            .front()
            .map(|frame| frame.seq)
            .unwrap_or(state.next_seq);
        let cursor = state
            .leases
            .get_mut(&id)
            .ok_or(PtyOutputLeaseError::Detached)?;
        *cursor = oldest;
        Ok(oldest)
    }

    fn close(&self) {
        let mut state = self.state.lock();
        state.lifecycle = OutputLifecycle::Closed;
        state.leases.clear();
        drop(state);
        self.notify.notify_waiters();
    }

    fn begin_closing(&self) {
        let mut state = self.state.lock();
        if state.lifecycle == OutputLifecycle::Open {
            state.lifecycle = OutputLifecycle::Closing;
            state.leases.clear();
        }
        drop(state);
        self.notify.notify_waiters();
    }

    fn cancel_closing(&self) {
        let mut state = self.state.lock();
        if state.lifecycle == OutputLifecycle::Closing {
            state.lifecycle = OutputLifecycle::Open;
        }
    }
}

enum ProbeResult {
    Ready(Result<PtyOutputRead, PtyOutputLeaseError>),
    Pending,
}

pub struct PtyOutputLease {
    hub: Arc<PtyOutputHub>,
    id: Uuid,
}

impl PtyOutputLease {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub async fn next(
        &self,
        timeout: Duration,
        max_frames: usize,
    ) -> Result<PtyOutputRead, PtyOutputLeaseError> {
        if max_frames == 0 {
            return Err(PtyOutputLeaseError::InvalidBatchSize);
        }
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.hub.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.hub.probe(self.id, max_frames) {
                ProbeResult::Ready(result) => return result,
                ProbeResult::Pending => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(PtyOutputLeaseError::TimedOut);
            }
            if tokio::time::timeout(remaining, &mut notified)
                .await
                .is_err()
            {
                return Err(PtyOutputLeaseError::TimedOut);
            }
        }
    }

    pub fn resync(&self) -> Result<u64, PtyOutputLeaseError> {
        self.hub.resync(self.id)
    }

    pub fn detach(&self) -> Result<(), PtyOutputLeaseError> {
        if self.hub.detach(self.id) {
            Ok(())
        } else {
            Err(PtyOutputLeaseError::Detached)
        }
    }
}

impl Drop for PtyOutputLease {
    fn drop(&mut self) {
        self.hub.detach(self.id);
    }
}

/// A PTY's process handles and ordered output stream. It has no UI, RPC, or
/// Tauri dependency; lifecycle owners decide how to route its output.
pub struct PtyBridge {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

/// Kernel-side PTY lifecycle authority. Shells may retain an output receiver,
/// but every write, resize, and destroy resolves through this registry.
#[derive(Default)]
pub struct PtyRegistry {
    ptys: Mutex<HashMap<Uuid, ManagedPty>>,
    next_index: AtomicUsize,
}

struct ManagedPty {
    bridge: Arc<PtyBridge>,
    scrollback: Arc<Mutex<Vec<u8>>>,
    renderer: Arc<Mutex<Terminal>>,
    output: Option<Arc<PtyOutputHub>>,
    closing: std::sync::atomic::AtomicBool,
    info: PtyInfo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyInfo {
    pub id: Uuid,
    pub pane_index: usize,
    pub workspace_id: Option<Uuid>,
    pub role: String,
    pub program: Option<String>,
    pub launch_profile: Option<String>,
    pub cwd: Option<String>,
    pub status: String,
    pub child_pid: Option<u32>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy)]
pub struct PtyLaunch<'a> {
    pub id: Uuid,
    pub program: Option<&'a str>,
    pub args: &'a [String],
    pub cwd: Option<&'a str>,
    pub workspace_id: Option<Uuid>,
    pub role: &'a str,
    pub launch_profile: Option<&'a str>,
    pub env: Option<&'a HashMap<String, String>>,
}

impl PtyRegistry {
    pub fn spawn(&self, shell: Option<&str>, cwd: Option<&str>) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.spawn_command_for(PtyLaunch {
            id,
            program: shell,
            args: &[],
            cwd,
            workspace_id: None,
            role: "shell",
            launch_profile: None,
            env: None,
        })
    }

    pub fn spawn_command_for(&self, mut launch: PtyLaunch<'_>) -> Result<Uuid> {
        launch.env = None;
        self.spawn_command_for_with_env(launch)
    }

    /// Spawn a PTY with an explicit argv and bounded caller-provided
    /// environment. The kernel remains the child-process authority; callers
    /// only supply launch data and never receive native PTY handles.
    pub fn spawn_command_for_with_env(&self, launch: PtyLaunch<'_>) -> Result<Uuid> {
        if self.contains(launch.id) {
            anyhow::bail!("PTY already exists: {}", launch.id);
        }
        let empty_env = HashMap::new();
        let env = launch.env.unwrap_or(&empty_env);
        let (bridge, output) = PtyBridge::spawn_command_with_env(
            launch.program,
            launch.args,
            launch.cwd,
            launch.launch_profile,
            env,
        )?;
        let bridge = Arc::new(bridge);
        let scrollback = Arc::new(Mutex::new(Vec::new()));
        let renderer = Arc::new(Mutex::new(Terminal::new(
            DEFAULT_ROWS as usize,
            DEFAULT_COLS as usize,
            RENDER_SCROLLBACK_ROWS,
        )));
        let output_hub = Arc::new(PtyOutputHub::new());
        let sink = scrollback.clone();
        let screen = renderer.clone();
        let hub = output_hub.clone();
        tokio::spawn(async move {
            let mut output = output;
            while let Some(bytes) = output.recv().await {
                screen.lock().feed(&bytes);
                let mut retained = sink.lock();
                retained.extend_from_slice(&bytes);
                if retained.len() > SCROLLBACK_CAP {
                    let drop_count = retained.len() - SCROLLBACK_CAP;
                    retained.drain(..drop_count);
                }
                hub.publish(&bytes);
            }
            hub.close();
        });
        let info = PtyInfo {
            id: launch.id,
            pane_index: self.next_index.fetch_add(1, Ordering::Relaxed),
            workspace_id: launch.workspace_id,
            role: launch.role.to_string(),
            program: launch.program.map(str::to_string),
            launch_profile: launch.launch_profile.map(str::to_string),
            cwd: launch.cwd.map(str::to_string),
            status: "Idle".to_string(),
            child_pid: bridge.process_id(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        };
        self.ptys.lock().insert(
            launch.id,
            ManagedPty {
                bridge,
                scrollback,
                renderer,
                output: Some(output_hub),
                closing: std::sync::atomic::AtomicBool::new(false),
                info,
            },
        );
        Ok(launch.id)
    }

    /// Create a managed PTY while handing its single lossless output stream to
    /// a shell projection. The registry still owns write/resize/destroy.
    pub fn spawn_with_output(
        &self,
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Uuid, mpsc::Receiver<Vec<u8>>)> {
        let (bridge, output) = PtyBridge::spawn(shell, cwd)?;
        let child_pid = bridge.process_id();
        let id = Uuid::new_v4();
        self.ptys.lock().insert(
            id,
            ManagedPty {
                bridge: Arc::new(bridge),
                scrollback: Arc::new(Mutex::new(Vec::new())),
                renderer: Arc::new(Mutex::new(Terminal::new(
                    DEFAULT_ROWS as usize,
                    DEFAULT_COLS as usize,
                    RENDER_SCROLLBACK_ROWS,
                ))),
                output: None,
                closing: std::sync::atomic::AtomicBool::new(false),
                info: PtyInfo {
                    id,
                    pane_index: self.next_index.fetch_add(1, Ordering::Relaxed),
                    workspace_id: None,
                    role: "shell".to_string(),
                    program: shell.map(str::to_string),
                    launch_profile: None,
                    cwd: cwd.map(str::to_string),
                    status: "Idle".to_string(),
                    child_pid,
                    cols: DEFAULT_COLS,
                    rows: DEFAULT_ROWS,
                },
            },
        );
        Ok((id, output))
    }

    /// Attach a bounded output lease to a PTY created through the domain
    /// registry. `after_seq` is inclusive replay's predecessor; `None` starts
    /// at the oldest frame still retained by the bounded window.
    pub fn attach_output(&self, id: Uuid, after_seq: Option<u64>) -> Result<PtyOutputLease> {
        let hub = self
            .ptys
            .lock()
            .get(&id)
            .filter(|managed| !managed.closing.load(Ordering::Acquire))
            .and_then(|managed| managed.output.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY output not available: {id}"))?;
        hub.attach(after_seq)
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    pub fn write(&self, id: Uuid, data: &[u8]) -> Result<()> {
        self.get(id)?.write_input(data)
    }

    pub fn resize(&self, id: Uuid, cols: u16, rows: u16) -> Result<()> {
        let bridge = self.get(id)?;
        bridge.resize(cols, rows)?;
        let mut ptys = self.ptys.lock();
        let managed = ptys
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        managed.renderer.lock().resize(rows as usize, cols as usize);
        managed.info.cols = cols;
        managed.info.rows = rows;
        Ok(())
    }

    pub fn destroy(&self, id: Uuid) -> Result<()> {
        let bridge = self.begin_destroy(id)?;
        if let Err(error) = bridge.destroy() {
            self.cancel_destroy(id);
            return Err(error);
        }
        self.finish_destroy(id)?;
        Ok(())
    }

    /// Linearize pane destruction before the irreversible child kill. After
    /// this point new writes/resizes fail; callers may cancel only if kill or
    /// persistence fails, keeping the registry entry for retry.
    pub fn begin_destroy(&self, id: Uuid) -> Result<Arc<PtyBridge>> {
        let ptys = self.ptys.lock();
        let managed = ptys
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        if managed
            .closing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            anyhow::bail!("PTY is already closing: {id}");
        }
        if let Some(output) = &managed.output {
            output.begin_closing();
        }
        Ok(managed.bridge.clone())
    }

    pub fn finish_destroy(&self, id: Uuid) -> Result<()> {
        let removed = self
            .ptys
            .lock()
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        if let Some(output) = removed.output {
            output.close();
        }
        Ok(())
    }

    pub fn cancel_destroy(&self, id: Uuid) {
        if let Some(managed) = self.ptys.lock().get(&id) {
            managed.closing.store(false, Ordering::Release);
            if let Some(output) = &managed.output {
                output.cancel_closing();
            }
        }
    }

    pub fn contains(&self, id: Uuid) -> bool {
        self.ptys.lock().contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.ptys.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.ptys.lock().is_empty()
    }

    pub fn info(&self, id: Uuid) -> Result<PtyInfo> {
        self.ptys
            .lock()
            .get(&id)
            .map(|managed| managed.info.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))
    }

    pub fn list(&self) -> Vec<PtyInfo> {
        let mut entries = self
            .ptys
            .lock()
            .values()
            .map(|managed| managed.info.clone())
            .collect::<Vec<_>>();
        entries.sort_by_key(|info| info.pane_index);
        entries
    }

    /// Return the bounded output window for a live PTY. The second value is
    /// the next sequence number, so a reconnect can resume after `next - 1`.
    pub fn output_bounds(&self, id: Uuid) -> Result<(u64, u64)> {
        let hub = self
            .ptys
            .lock()
            .get(&id)
            .filter(|managed| !managed.closing.load(Ordering::Acquire))
            .and_then(|managed| managed.output.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY output not available: {id}"))?;
        let state = hub.state.lock();
        let oldest = state
            .frames
            .front()
            .map(|frame| frame.seq)
            .unwrap_or(state.next_seq);
        Ok((oldest, state.next_seq))
    }

    pub fn set_status(&self, id: Uuid, status: &str) -> Result<()> {
        let mut ptys = self.ptys.lock();
        let managed = ptys
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        managed.info.status = status.to_string();
        Ok(())
    }

    pub fn scrollback(&self, id: Uuid, max_bytes: usize) -> Result<Vec<u8>> {
        let scrollback = self
            .ptys
            .lock()
            .get(&id)
            .map(|managed| managed.scrollback.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        let retained = scrollback.lock();
        let start = retained.len().saturating_sub(max_bytes.min(SCROLLBACK_CAP));
        Ok(retained[start..].to_vec())
    }

    /// Release retained output for a live pane. This is the backend half of a
    /// terminal clear; callers still clear their renderer separately.
    pub fn clear_scrollback(&self, id: Uuid) -> Result<()> {
        let managed = self
            .ptys
            .lock()
            .get(&id)
            .map(|managed| (managed.scrollback.clone(), managed.renderer.clone()))
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        managed.0.lock().clear();
        managed.1.lock().clear_terminal();
        Ok(())
    }

    pub fn rendered(&self, id: Uuid, lines: usize) -> Result<String> {
        let renderer = self
            .ptys
            .lock()
            .get(&id)
            .map(|managed| managed.renderer.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))?;
        let mut rows = renderer.lock().dump_visible_text();
        while rows.last().is_some_and(|line| line.trim().is_empty()) {
            rows.pop();
        }
        let start = rows.len().saturating_sub(lines.max(1));
        Ok(rows[start..].join("\n"))
    }

    fn get(&self, id: Uuid) -> Result<Arc<PtyBridge>> {
        self.ptys
            .lock()
            .get(&id)
            .filter(|managed| !managed.closing.load(Ordering::Acquire))
            .map(|managed| managed.bridge.clone())
            .ok_or_else(|| anyhow::anyhow!("PTY not found: {id}"))
    }
}

impl Drop for PtyRegistry {
    fn drop(&mut self) {
        let bridges = std::mem::take(self.ptys.get_mut());
        for managed in bridges.into_values() {
            if let Some(output) = managed.output {
                output.close();
            }
            let _ = managed.bridge.destroy();
        }
    }
}

#[cfg(test)]
pub(crate) fn test_output_lease() -> PtyOutputLease {
    Arc::new(PtyOutputHub::new())
        .attach(None)
        .expect("test output lease")
}

impl PtyBridge {
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        Self::spawn_command(shell, &[], cwd, None)
    }

    pub fn spawn_command(
        program: Option<&str>,
        args: &[String],
        cwd: Option<&str>,
        launch_profile: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        Self::spawn_command_with_env(program, args, cwd, launch_profile, &HashMap::new())
    }

    pub fn spawn_command_with_env(
        program: Option<&str>,
        args: &[String],
        cwd: Option<&str>,
        launch_profile: Option<&str>,
        env: &HashMap<String, String>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;
        let program = resolve_shell(program);
        let mut command = CommandBuilder::new(&program);
        command.args(args);
        if let Some(dir) = cwd {
            command.cwd(dir);
        }
        for (key, value) in env {
            command.env(key, value);
        }
        if let Ok(path) = std::env::var("RIDGE_CDP_SHELL_PATH") {
            if !path.trim().is_empty() {
                command.env("PATH", path);
            }
        }
        command.env("TERM", "xterm-256color");
        apply_shell_integration(&mut command, &program, launch_profile);
        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("failed to spawn shell '{program}'"))?;
        drop(pair.slave);
        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;
        let (tx, rx) = mpsc::channel(256);
        spawn_reader_thread(reader, tx);
        Ok((
            Self {
                writer: Arc::new(Mutex::new(writer)),
                master: Arc::new(Mutex::new(pair.master)),
                child: Mutex::new(child),
            },
            rx,
        ))
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock();
        writer.write_all(data).context("PTY write failed")?;
        writer.flush().context("PTY flush failed")?;
        Ok(())
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.lock().process_id()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .lock()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("PTY resize failed")
    }

    /// Explicitly stop the child before releasing the PTY handles. Dropping a
    /// handle alone is not a lifecycle guarantee on every platform.
    pub fn destroy(&self) -> Result<()> {
        let mut child = self.child.lock();
        if child.try_wait().context("PTY status failed")?.is_some() {
            return Ok(());
        }
        let pid = child.process_id();
        child.kill().context("PTY kill failed")?;
        if let Some(pid) = pid {
            ridge_core::process_guard::kill_process_tree(pid);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if child
                .try_wait()
                .context("PTY reap status failed")?
                .is_some()
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                anyhow::bail!("PTY process did not exit within 2s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The desktop parser derives live cwd/title state from shell OSC markers.
/// Kernel-owned shells must emit the same markers or a reattached pane would
/// silently regress to its spawn cwd. Keep this opt-in: structured launches
/// and callers outside the desktop contract retain their original argv/env.
fn apply_shell_integration(
    command: &mut CommandBuilder,
    program: &str,
    launch_profile: Option<&str>,
) {
    if launch_profile != Some("ridge-interactive") {
        return;
    }
    let name = std::path::Path::new(program)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    match name.as_str() {
        #[cfg(windows)]
        "powershell" | "pwsh" => {
            // -EncodedCommand is UTF-16LE base64, so PowerShell receives a
            // shell-safe prompt wrapper even when the user's cwd has spaces.
            let script = r#"$Global:__ridge_origPrompt = (Get-Item function:prompt).ScriptBlock; function global:prompt { $r = & $Global:__ridge_origPrompt; try { $c = $PWD.ProviderPath } catch { $c = (Get-Location).Path }; try { [Console]::Write(([string][char]27) + ']7;file:///' + $c + ([string][char]7)) } catch {}; try { [Console]::Write(([string][char]27) + ']133;A' + ([string][char]7)) } catch {}; $r }"#;
            command.arg("-NoLogo");
            command.arg("-NoExit");
            command.arg("-EncodedCommand");
            command.arg(encode_powershell_utf16le_base64(script));
        }
        "bash" => {
            let existing = std::env::var("PROMPT_COMMAND").unwrap_or_default();
            let marker = r#"printf '\033]7;file://%s\a\033]133;A\a' "$PWD""#;
            let value = if existing.trim().is_empty() {
                marker.to_string()
            } else {
                format!("{existing}; {marker}")
            };
            command.env("PROMPT_COMMAND", value);
        }
        #[cfg(unix)]
        "zsh" => {
            let user_zdotdir = std::env::var_os("ZDOTDIR")
                .filter(|value| !value.is_empty())
                .or_else(|| std::env::var_os("HOME").filter(|value| !value.is_empty()));
            if let Some(user_zdotdir) = user_zdotdir {
                if let Ok(zdotdir) = prepare_zsh_zdotdir() {
                    command.env("USER_ZDOTDIR", user_zdotdir);
                    command.env("ZDOTDIR", zdotdir);
                }
            }
        }
        _ => {}
    }
}

#[cfg(windows)]
fn encode_powershell_utf16le_base64(script: &str) -> String {
    use base64::Engine as _;
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(unix)]
const RIDGE_ZSH_ZSHENV: &str = r#"if [[ -f "$USER_ZDOTDIR/.zshenv" ]]; then RIDGE_ZDOTDIR=$ZDOTDIR; ZDOTDIR=$USER_ZDOTDIR; . "$USER_ZDOTDIR/.zshenv"; [[ $ZDOTDIR == $USER_ZDOTDIR ]] && ZDOTDIR=$RIDGE_ZDOTDIR; fi
"#;

#[cfg(unix)]
const RIDGE_ZSH_ZPROFILE: &str = r#"if [[ -f "$USER_ZDOTDIR/.zprofile" ]]; then RIDGE_ZDOTDIR=$ZDOTDIR; ZDOTDIR=$USER_ZDOTDIR; . "$USER_ZDOTDIR/.zprofile"; [[ $ZDOTDIR == $USER_ZDOTDIR ]] && ZDOTDIR=$RIDGE_ZDOTDIR; fi
"#;

#[cfg(unix)]
const RIDGE_ZSH_ZSHRC: &str = r#"if [[ -f "$USER_ZDOTDIR/.zshrc" ]]; then RIDGE_ZDOTDIR=$ZDOTDIR; ZDOTDIR=$USER_ZDOTDIR; . "$USER_ZDOTDIR/.zshrc"; [[ $ZDOTDIR == $USER_ZDOTDIR ]] && ZDOTDIR=$RIDGE_ZDOTDIR; fi
__ridge_emit_cwd() { printf '\033]7;file://%s\a\033]133;A\a' "$PWD"; }
autoload -Uz add-zsh-hook 2>/dev/null
if (( ${+functions[add-zsh-hook]} )); then add-zsh-hook precmd __ridge_emit_cwd; elif [[ -z ${precmd_functions[(r)__ridge_emit_cwd]} ]]; then precmd_functions+=(__ridge_emit_cwd); fi
[[ $options[login] == off ]] && ZDOTDIR=$USER_ZDOTDIR
"#;

#[cfg(unix)]
const RIDGE_ZSH_ZLOGIN: &str = r#"if [[ -f "$USER_ZDOTDIR/.zlogin" ]]; then RIDGE_ZDOTDIR=$ZDOTDIR; ZDOTDIR=$USER_ZDOTDIR; . "$USER_ZDOTDIR/.zlogin"; [[ $ZDOTDIR == $USER_ZDOTDIR ]] && ZDOTDIR=$RIDGE_ZDOTDIR; fi
ZDOTDIR=$USER_ZDOTDIR
"#;

#[cfg(unix)]
fn prepare_zsh_zdotdir() -> std::io::Result<std::path::PathBuf> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "default".to_string());
    let dir = std::env::temp_dir()
        .join(format!("ridge-shell-integration-{user}"))
        .join("zsh");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(".zshenv"), RIDGE_ZSH_ZSHENV)?;
    std::fs::write(dir.join(".zprofile"), RIDGE_ZSH_ZPROFILE)?;
    std::fs::write(dir.join(".zshrc"), RIDGE_ZSH_ZSHRC)?;
    std::fs::write(dir.join(".zlogin"), RIDGE_ZSH_ZLOGIN)?;
    Ok(dir)
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        // Best effort only: explicit `destroy` surfaces an operational error,
        // while drop must never panic during registry or kernel shutdown.
        let _ = self.child.get_mut().kill();
    }
}

fn resolve_shell(shell: Option<&str>) -> String {
    if let Some(shell) = shell.filter(|value| !value.trim().is_empty()) {
        return shell.to_string();
    }
    #[cfg(unix)]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            if !shell.is_empty() {
                return shell;
            }
        }
        for candidate in ["/bin/bash", "/usr/bin/bash", "/bin/zsh", "/bin/sh"] {
            if std::path::Path::new(candidate).exists() {
                return candidate.to_string();
            }
        }
        "/bin/sh".to_string()
    }
    #[cfg(windows)]
    {
        for candidate in ["pwsh.exe", "powershell.exe"] {
            if which_on_path(candidate) {
                return candidate.to_string();
            }
        }
        std::env::var("COMSPEC")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }
}

#[cfg(windows)]
fn which_on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(exe).is_file()))
        .unwrap_or(false)
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    std::thread::Builder::new()
        .name("ridge-kernel-pty-reader".to_string())
        .spawn(move || {
            let mut buf = [0u8; READ_BUF];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(size) if tx.blocking_send(buf[..size].to_vec()).is_err() => break,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(target: "ridge_kernel::pty", %error, "PTY reader exiting");
                        break;
                    }
                }
            }
        })
        .expect("failed to spawn PTY reader thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_shell_prefers_explicit_value() {
        assert_eq!(resolve_shell(Some("custom-shell")), "custom-shell");
        assert!(!resolve_shell(Some(" ")).is_empty());
    }

    #[test]
    fn empty_registry_has_no_pane() {
        let registry = PtyRegistry::default();
        assert_eq!(registry.len(), 0);
        assert!(!registry.contains(Uuid::new_v4()));
        assert!(registry.clear_scrollback(Uuid::new_v4()).is_err());
    }

    #[tokio::test]
    async fn explicit_launch_environment_reaches_child_process() {
        let mut env = HashMap::new();
        env.insert("RIDGE_KERNEL_TEST_ENV".to_string(), "kernel-ok".to_string());
        #[cfg(windows)]
        let (program, args) = (
            Some("cmd.exe"),
            vec![
                "/d".to_string(),
                "/c".to_string(),
                "echo %RIDGE_KERNEL_TEST_ENV%".to_string(),
            ],
        );
        #[cfg(not(windows))]
        let (program, args) = (
            Some("/bin/sh"),
            vec![
                "-c".to_string(),
                "printf '%s' \"$RIDGE_KERNEL_TEST_ENV\"".to_string(),
            ],
        );
        let (bridge, mut output) =
            PtyBridge::spawn_command_with_env(program, &args, None, None, &env)
                .expect("spawn explicit environment child");
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut data = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(chunk) = tokio::time::timeout(remaining, output.recv())
                .await
                .expect("child output timeout")
            else {
                break;
            };
            data.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&data).contains("kernel-ok") {
                break;
            }
        }
        assert!(
            String::from_utf8_lossy(&data).contains("kernel-ok"),
            "child output did not contain explicit env: {:?}",
            data
        );
        bridge.destroy().expect("destroy env test child");
    }

    #[tokio::test]
    async fn output_lease_sequences_and_reports_lag_for_bounded_replay() {
        let hub = Arc::new(PtyOutputHub::new());
        for seq in 0..(OUTPUT_REPLAY_CAP_FRAMES + 4) {
            hub.publish(&[(seq % 256) as u8]);
        }
        let lease = hub.attach(Some(0)).expect("attach open output");
        assert_eq!(
            lease.next(Duration::ZERO, 8).await,
            Ok(PtyOutputRead::Lagged {
                requested_seq: 1,
                oldest_seq: 5,
                latest_seq: 260,
            })
        );
        assert_eq!(lease.resync(), Ok(5));
        let read = lease.next(Duration::ZERO, 2).await.expect("replay");
        assert_eq!(
            read,
            PtyOutputRead::Data(vec![
                PtyOutputFrame {
                    seq: 5,
                    data: vec![4]
                },
                PtyOutputFrame {
                    seq: 6,
                    data: vec![5]
                },
            ])
        );
    }

    #[tokio::test]
    async fn output_lease_timeout_detach_and_close_fail_closed() {
        let hub = Arc::new(PtyOutputHub::new());
        let lease = hub.attach(None).expect("attach open output");
        assert_eq!(
            lease.next(Duration::from_millis(1), 1).await,
            Err(PtyOutputLeaseError::TimedOut)
        );
        lease.detach().expect("detach lease");
        assert_eq!(
            lease.next(Duration::ZERO, 1).await,
            Err(PtyOutputLeaseError::Detached)
        );

        let pending = hub.attach(None).expect("second lease");
        let closer = hub.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            closer.close();
        });
        assert_eq!(
            pending.next(Duration::from_millis(100), 1).await,
            Err(PtyOutputLeaseError::Closed)
        );
    }

    #[tokio::test]
    async fn output_lease_closing_rejects_attach_then_cancel_reopens_for_new_lease() {
        let hub = Arc::new(PtyOutputHub::new());
        let old = hub.attach(None).expect("attach open output");
        hub.begin_closing();
        assert_eq!(
            old.next(Duration::ZERO, 1).await,
            Err(PtyOutputLeaseError::Closing)
        );
        assert!(matches!(
            hub.attach(None),
            Err(PtyOutputLeaseError::Closing)
        ));
        hub.cancel_closing();
        let fresh = hub.attach(None).expect("cancel reopens output");
        hub.publish(b"ok");
        assert_eq!(
            fresh.next(Duration::ZERO, 1).await,
            Ok(PtyOutputRead::Data(vec![PtyOutputFrame {
                seq: 1,
                data: b"ok".to_vec(),
            }]))
        );
    }
}
