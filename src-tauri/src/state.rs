use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::{Mutex, RwLock};
use portable_pty::{CommandBuilder, MasterPty, SlavePty};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::commands::fs_watch::FsWatcher;
use crate::commands::watch::GitWatcher;
use crate::db::ProjectStore;
use crate::engine::pane_tree::PaneTree;
use crate::engine::pty::PtyHandle;
use crate::remote::auth::{RemoteAuth, SessionStore, VerifyThrottle};
use crate::types::{GlobalEvent, RemotePtyEvent};
use crate::utils::cwd::{detect_startup_cwd_kind, StartupCwdKind};

/// Two-stage PTY spawn record.
///
/// Phase 1 (`ensure_pane_pty_workspace`) opens a PTY pair, clones the reader,
/// captures the writer, and builds the `CommandBuilder` — but does **not**
/// spawn a child process. The pre-built record lands here so the front-end
/// can call `activate_pane_pty` once the xterm container has stable
/// dimensions. This eliminates the "spawn before mount" race that produced
/// black panes when the shell wrote its initial banner before xterm was
/// ready to receive it.
///
/// Phase 2 (`activate_pane_pty`) consumes the record: optionally resizes the
/// master to the front-end-reported size, calls `slave.spawn_command(cmd)`,
/// installs the resulting `PtyHandle` into `Workspace.terminals`, and signals
/// the optional `ready_tx` so callers (e.g. teammate `split-window`) can
/// observe success/failure.
/// The bundle of one-shot, !Sync handles that activation consumes.
/// Kept inside one `Option` so a single `take()` flips `PendingSpawn` to
/// "drained" atomically.
pub struct PendingSpawnInner {
    pub command: CommandBuilder,
    pub slave: Box<dyn SlavePty + Send>,
    pub reader: Box<dyn std::io::Read + Send>,
}

pub struct PendingSpawn {
    /// `Mutex<Option<...>>`: the `Mutex` keeps `PendingSpawn` `Sync`
    /// (the inner contents are only `Send`), and the `Option` lets
    /// `activate_pane_pty` `take()` everything atomically on consumption.
    pub inner: Mutex<Option<PendingSpawnInner>>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub ready_tx: Mutex<Option<tokio::sync::oneshot::Sender<Result<(), String>>>>,
    pub trace_id: String,
}

/// Aggregate counters surfaced to the SettingsPanel "Agent 统计" section so
/// users can see how many teammate-initiated splits succeeded vs. failed and
/// why. Counts are per-workspace and serialised straight to the front-end.
#[derive(Default, Clone, serde::Serialize)]
pub struct TeammateMetrics {
    pub split_attempts: u64,
    pub split_success: u64,
    pub failures: HashMap<String, u64>,
}

/// Claude Code tmux shim 连接本地控制面所需（注入到子 shell）。
#[derive(Clone, Debug)]
pub struct TeammateBinding {
    pub base_url: String,
    pub token: String,
}

/// 终端 pane 状态：跟踪 agent 是否在运行
#[derive(Clone, Debug, PartialEq)]
pub enum PaneState {
    /// 空闲 pane，可复用
    Idle,
    /// 有 agent 运行中
    Busy,
    /// Pane 正在启动中（agent register 已发但 PTY 还没收到首条 prompt 输出时使用）
    #[allow(dead_code)]
    // half-built: enum + serialization (commands/pane.rs:60-64) + TS union + UI badge (SplitContainer.svelte:592-599) all in place, but teammate/server.rs:register_agent_to_pane goes Idle→Busy directly. See TASKS §1.14.
    Starting,
}

impl Default for PaneState {
    fn default() -> Self {
        PaneState::Idle
    }
}

/// 单个根会话：独立分屏树 + 终端句柄（多工作区互不共享 pane id 命名空间下的 PTY 表）。
pub struct Workspace {
    pub pane_tree: PaneTree,
    pub terminals: HashMap<Uuid, PtyHandle>,
    /// Claude `send-keys -t ""` / 无 `-t` 时 tmux「当前窗格」：在 Ridge 里对应 `split-window` / `select-pane` 最后指向的 pane 索引。
    pub teammate_tmux_pane_cursor: usize,
    /// `new-window -n` / `split-window -n` 等经 teammate 写入的窗格展示名（按 pane id）。
    pub teammate_pane_titles: HashMap<Uuid, String>,
    /// Per-pane dimensions (rows, cols) for split target selection algorithm.
    pub pane_sizes: HashMap<Uuid, (u16, u16)>,
    /// Previous pane index for tmux `last-pane` swap functionality.
    pub last_pane_index: Option<usize>,
    /// 工作区创建时间（`list-sessions` 等 tmux 兼容输出用）。
    pub created_at: SystemTime,
    /// Pane 状态跟踪：空闲/忙碌，用于 agent 记忆和复用
    pub teammate_pane_states: HashMap<Uuid, PaneState>,
    /// Agent 到 pane 的映射：记录哪个 agent（通过唯一 ID）在哪个 pane
    pub teammate_agent_pane_map: HashMap<String, Uuid>,
    /// Ridge 亲手为 teammate 创建的 pane 集合（经 `teammate_split_pane`）。
    /// 宿主原始 pane 与任意用户手开 pane **永不**进此集合，因此 idle-reuse
    /// (`find_idle_pane_index`) 与 teammate `kill-pane` / `spawn-process` 只能
    /// 命中 Ridge 自己起的 pane，绝不会碰到 parent agent 自己的 pane。
    /// 见 2026-06-11 宿主 pane 保护修复。
    pub teammate_owned_panes: HashSet<Uuid>,
    /// 关联的 .ridge 文件绝对路径。`Some` 表示该工作区已保存到磁盘；
    /// 后续任何 cwd/布局/git 变化都会触发防抖自动回写。
    pub associated_file_path: Option<PathBuf>,
    /// Phase-1 PTY records waiting for `activate_pane_pty` to spawn the child.
    /// Keyed by pane id. See `PendingSpawn` for the rationale behind splitting
    /// `openpty` from `spawn_command` into two stages.
    pub pending_spawns: HashMap<Uuid, PendingSpawn>,
    /// Monotonic per-pane PTY generation. Bumped on every teardown/replace
    /// (`teardown_pane_pty_if_present`) BEFORE the old child is killed, so a
    /// reader that captured an older generation at spawn knows, on EOF, that it
    /// is no longer the pane's current PTY — and must NOT run the child-exit→Idle
    /// demotion (which would clobber a freshly-spawned agent's Busy during the
    /// [teardown, new-PTY-live) window). See `engine::pty` reader cleanup.
    pub pty_generation: HashMap<Uuid, u64>,
    /// Per-workspace counters for teammate-initiated splits (success / failure
    /// reasons). Surfaced read-only via `get_teammate_metrics`.
    pub teammate_metrics: TeammateMetrics,
    /// 监控递增的展示序号：未命名工作区在 UI 上显示为「工作区 N」，N 在创建时分配
    /// 后不再变化（关闭其他工作区或拖拽重排序都不影响）。关闭后不复用，避免歧义。
    pub display_seq: u64,
}

/// Block-based PTY scrollback store. See `docs/TERMINAL_SCROLLBACK.md` for the
/// design context. Byte content is stored as immutable blocks + a mutable
/// `current` partial block; each block records its starting `seq` so callers
/// can paginate with "give me bytes before seq X".
///
/// Eviction drops from the front (oldest block) when `total_bytes` exceeds
/// `MAX_BYTES`. Blocks are raw PTY bytes — we never re-parse; we guarantee
/// every slice we return starts and ends at a UTF-8 char boundary so the
/// frontend can blindly `decode_utf8` without worrying about mid-codepoint cuts.
#[derive(Clone, Debug)]
pub struct PaneScrollback {
    /// Completed blocks, oldest at front.
    pub blocks: std::collections::VecDeque<Arc<Vec<u8>>>,
    /// Starting seq (monotonic byte counter) of each block in `blocks`, same order.
    pub block_start_seqs: std::collections::VecDeque<u64>,
    /// Active partial block; flushes to `blocks` on reaching `BLOCK_SIZE`.
    pub current: Vec<u8>,
    /// Starting seq of the `current` block.
    pub current_start_seq: u64,
    /// Sum of bytes in `blocks` + `current.len()`. Used for cap eviction.
    pub total_bytes: usize,
}

/// One page of scrollback bytes returned by `get_pane_scrollback_tail` /
/// `_before`. Always UTF-8-safe at both ends.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScrollbackChunk {
    /// UTF-8 string. Clone of the underlying bytes; block data is cached
    /// behind `Arc` so the clone is cheap per block.
    pub bytes: String,
    /// Starting seq of `bytes`. For `get_pane_scrollback_before` callers use
    /// this as the next `before_seq` to page further up.
    pub start_seq: u64,
    /// True when this response includes the very first retained byte — no
    /// older data is available from the store (may still exist in xterm's
    /// own buffer if it's still mounted).
    pub at_oldest: bool,
}

/// Block size for completed scrollback pages (bytes). Chosen so a single
/// tail read pulls ~128 KiB (2 blocks) without getting too chunky on the
/// wire to the renderer.
pub const SCROLLBACK_BLOCK_SIZE: usize = 64 * 1024;
/// Hard retention cap per pane. 8 MiB covers ~60k lines of 120-char output,
/// enough to retain a full long build log or vim session. Doubled from the
/// original 4 MiB to match the raised cloud replay cap (256 KiB) — no point
/// replaying more than what we've retained. Memory cost: at most 8 MiB *
/// number of active panes (typically ≤ 10), so ≤ ~80 MiB worst case.
pub const SCROLLBACK_MAX_BYTES: usize = 8 * 1024 * 1024;

impl PaneScrollback {
    pub fn new() -> Self {
        Self {
            blocks: std::collections::VecDeque::new(),
            block_start_seqs: std::collections::VecDeque::new(),
            current: Vec::with_capacity(SCROLLBACK_BLOCK_SIZE),
            current_start_seq: 0,
            total_bytes: 0,
        }
    }

    /// Seq of the first byte still available to callers.
    pub fn oldest_seq(&self) -> u64 {
        if let Some(&s) = self.block_start_seqs.front() {
            s
        } else {
            self.current_start_seq
        }
    }

    /// Return the seq-sorted flat view of every retained byte. Used by the
    /// paging helpers. `total` matches `self.total_bytes` at the top of the
    /// call; we snapshot into a Vec<Arc<Vec<u8>>> so the caller can read
    /// without holding the outer RwLock longer than needed.
    pub fn snapshot_blocks(&self) -> Vec<(u64, Arc<Vec<u8>>)> {
        let mut out: Vec<(u64, Arc<Vec<u8>>)> = Vec::with_capacity(self.blocks.len() + 1);
        for (i, b) in self.blocks.iter().enumerate() {
            out.push((self.block_start_seqs[i], Arc::clone(b)));
        }
        if !self.current.is_empty() {
            out.push((self.current_start_seq, Arc::new(self.current.clone())));
        }
        out
    }
}

/// P4.1 (2026-05-21) — per-pane delta-byte sender. Wraps a Tauri
/// `Channel<Vec<u8>>::send` (or any other sink — e.g. a postMessage shim
/// to a render worker in P4.9) so the `pty-delta-*` emit sites in `lib.rs`
/// and `commands/terminal.rs` can stay agnostic of *how* bytes reach the
/// frontend. `Arc` so the main loop can clone out of the RwLock and call
/// `send` without holding any state lock.
///
/// Returning `()` is intentional: send failures are best-effort (the
/// frontend has gone away if they fail, and the next pane-close cleanup
/// will drop the sender). Callers log errors inside the closure.
pub type PaneOutputSender = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
pub type PaneDeltaSender = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

pub struct RemotePaneSub {
    pub id: u64,
    pub raw_tx: mpsc::Sender<RemotePtyEvent>,
    /// Set by the PTY fan-out (lib.rs) when a `raw_tx.try_send` is dropped
    /// because this sub's channel is full. The WS task observes it on the
    /// next forwarded frame and emits a terminal hard-reset (RIS) + fresh
    /// scrollback so the client's vte parser re-synchronises instead of
    /// staying corrupted by the hole in the byte stream.
    pub desync: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct PaneRegistry {
    pub output_cb: Option<PaneOutputSender>,
    pub delta_cb: Option<PaneDeltaSender>,
    pub remote_subs: Vec<RemotePaneSub>,
}

/// Monotonic remote subscriber ID counter. Each `handle_ws` invocation
/// grabs a unique id for registration/deregistration.
pub struct RemoteSubId;

impl RemoteSubId {
    pub fn next() -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

/// Information about a connected remote client (WebSocket).
#[derive(Clone, Debug)]
pub struct RemoteClientInfo {
    pub id: u64,
    pub connected_at: std::time::SystemTime,
    pub remote_addr: String,
    pub user_agent: String,
    /// Stable, mobile-generated device id (localStorage UUID) sent on connect.
    /// Used as the blacklist key (survives token rotation). Empty if absent.
    pub device_id: String,
    /// The session token this connection authenticated with (if it connected
    /// via `?token=`). Force-disconnect invalidates it so the device must
    /// re-enter the auth code to reconnect.
    pub token: Option<String>,
    pub kill_flag: Arc<AtomicBool>,
}

/// Registry tracking all currently connected remote WebSocket clients.
/// Used by the desktop RemotePanel to list connected devices and
/// forcibly disconnect them.
#[derive(Default)]
pub struct RemoteClientRegistry {
    pub clients: parking_lot::Mutex<HashMap<u64, RemoteClientInfo>>,
}

impl RemoteClientRegistry {
    pub fn register(
        &self,
        addr: String,
        ua: String,
        device_id: String,
        token: Option<String>,
    ) -> (u64, Arc<AtomicBool>) {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let kill_flag = Arc::new(AtomicBool::new(false));
        self.clients.lock().insert(
            id,
            RemoteClientInfo {
                id,
                connected_at: std::time::SystemTime::now(),
                remote_addr: addr,
                user_agent: ua,
                device_id,
                token,
                kill_flag: Arc::clone(&kill_flag),
            },
        );
        (id, kill_flag)
    }

    pub fn unregister(&self, id: u64) {
        self.clients.lock().remove(&id);
    }

    pub fn list(&self) -> Vec<RemoteClientInfo> {
        self.clients.lock().values().cloned().collect()
    }

    /// Snapshot of a single client's info (for resolving token/device on
    /// disconnect / blacklist).
    pub fn info_of(&self, id: u64) -> Option<RemoteClientInfo> {
        self.clients.lock().get(&id).cloned()
    }

    pub fn kick(&self, id: u64) -> bool {
        let map = self.clients.lock();
        if let Some(info) = map.get(&id) {
            info.kill_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

/// A persistent blacklist entry. A connection is barred if its device id OR its
/// IP matches a non-empty field of any entry.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlacklistEntry {
    /// UUID for stable UI keying / removal.
    pub id: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    /// Display label (short device id or IP).
    pub label: String,
    /// Unix seconds when added.
    pub added_at: u64,
}

/// Persistent blacklist of devices/IPs barred from the remote server. Backed by
/// a JSON file under the app data dir; enforced at `/verify` and the `/ws`
/// upgrade. Survives token rotation by keying on the mobile-provided device id
/// (and/or IP).
#[derive(Default)]
pub struct RemoteBlacklist {
    entries: parking_lot::Mutex<Vec<BlacklistEntry>>,
    path: parking_lot::Mutex<Option<std::path::PathBuf>>,
}

impl RemoteBlacklist {
    /// Point the blacklist at its on-disk JSON and load existing entries.
    /// Called once at Tauri setup when the app data dir is known.
    pub fn set_path_and_load(&self, path: std::path::PathBuf) {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<BlacklistEntry>>(&s) {
                *self.entries.lock() = list;
            }
        }
        *self.path.lock() = Some(path);
    }

    fn save(&self) {
        let path = self.path.lock().clone();
        if let Some(path) = path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(s) = serde_json::to_string_pretty(&*self.entries.lock()) {
                let _ = std::fs::write(&path, s);
            }
        }
    }

    pub fn list(&self) -> Vec<BlacklistEntry> {
        self.entries.lock().clone()
    }

    /// True if this device id or IP is blacklisted (empty values never match).
    pub fn is_blocked(&self, device_id: &str, ip: &str) -> bool {
        self.entries.lock().iter().any(|e| {
            e.device_id
                .as_deref()
                .is_some_and(|d| !d.is_empty() && d == device_id)
                || e.ip.as_deref().is_some_and(|i| !i.is_empty() && i == ip)
        })
    }

    /// Add an entry, de-duplicating by matching device id / IP, then persist.
    pub fn add(&self, entry: BlacklistEntry) {
        {
            let mut entries = self.entries.lock();
            entries.retain(|e| {
                let same_dev = match (&e.device_id, &entry.device_id) {
                    (Some(a), Some(b)) => !a.is_empty() && a == b,
                    _ => false,
                };
                let same_ip = match (&e.ip, &entry.ip) {
                    (Some(a), Some(b)) => !a.is_empty() && a == b,
                    _ => false,
                };
                !(same_dev || same_ip)
            });
            entries.push(entry);
        }
        self.save();
    }

    /// Remove an entry by its UUID; returns whether anything was removed.
    pub fn remove(&self, id: &str) -> bool {
        let removed = {
            let mut entries = self.entries.lock();
            let before = entries.len();
            entries.retain(|e| e.id != id);
            entries.len() != before
        };
        if removed {
            self.save();
        }
        removed
    }
}

#[derive(Clone)]
pub struct AppState {
    pub workspaces: Arc<RwLock<HashMap<Uuid, Workspace>>>,
    pub workspace_order: Arc<RwLock<Vec<Uuid>>>,
    pub workspace_names: Arc<RwLock<HashMap<Uuid, String>>>,
    pub active_workspace: Arc<RwLock<Uuid>>,
    /// 下一个未命名工作区的展示序号；每次新建（包括从 .ridge 还原）`fetch_add 1`。
    /// 关闭工作区不会回收已发出的序号 —— 用户期望「工作区 2」一旦创建就不会被另一个
    /// 重新顶替，避免标签语义漂移。
    pub next_workspace_seq: Arc<RwLock<u64>>,
    pub event_tx: mpsc::Sender<GlobalEvent>,
    /// 供 `capture-pane` 读取的最近输出（与 UI 展示同源 PTY 流）。
    /// Block-based store — see `PaneScrollback` and
    /// `docs/TERMINAL_SCROLLBACK.md`.
    pub pty_scrollback: Arc<RwLock<HashMap<(Uuid, Uuid), PaneScrollback>>>,
    /// 本进程 teammate HTTP 绑定信息；存在时新 PTY 会注入 Ridge_TEAMMATE_*。
    pub teammate_binding: Arc<RwLock<Option<TeammateBinding>>>,
    /// 进程级 AppHandle，setup 时 stash。teammate HTTP server 改为「按需启动」后，
    /// `ensure_teammate_started` 用它在首个 PTY 创建时惰性拉起 server（避免压在冷启动路径上）。
    pub app_handle: Arc<std::sync::OnceLock<tauri::AppHandle>>,
    /// Project store for managing projects
    pub project_store: Option<Arc<ProjectStore>>,
    /// Current active project path
    pub current_project: Arc<RwLock<Option<PathBuf>>>,
    /// Git filesystem watcher — keeps notify debouncers alive for each watched repo.
    /// Wrapped in Arc so that cloning AppState shares the same watcher instance.
    pub git_watcher: Arc<GitWatcher>,
    /// 通用文件系统 watcher：覆盖 Explorer 列出的 cwd 和编辑器打开的外部文件，
    /// emit `fs-changed` 事件供前端文件树/编辑器订阅。
    pub fs_watcher: Arc<FsWatcher>,
    /// 启动方式：cli（终端调用 `ridge`）/ menu（资源管理器、开始菜单双击 ridge.exe）。
    /// 关键差异：menu 模式下进程的 `current_dir()` 等于 ridge.exe 所在目录，**不**应作为
    /// 默认工作目录；cli 模式下它是用户期望的工作目录。
    /// §4 启动恢复读它：cli 模式下跳过 restore set，让 cwd 接管首个工作区。
    pub startup_cwd_kind: StartupCwdKind,
    /// cli 模式下捕获的启动 cwd；menu 模式为 None。后续 §2 用户配置 defaultCwd
    /// 时按 cli > user > home 的优先级合并（utils::cwd::resolve_default_cwd）。
    pub startup_cli_cwd: Option<PathBuf>,
    /// 用户在设置面板配置的默认工作目录（front-end localStorage `defaultCwd` 字段
    /// 在启动时通过 `set_user_default_cwd` 命令同步到这里）。menu 启动模式下，
    /// 这是首个 pane 的 cwd 来源（cli 启动时被 startup_cli_cwd 覆盖，仍然优先）。
    pub user_default_cwd: Arc<RwLock<Option<PathBuf>>>,
    /// Unified per-pane registry for desktop callbacks and remote WS subscribers.
    /// Keyed by (workspace_id, pane_id). Each entry can hold:
    ///   - output_cb: desktop Tauri Channel for coalesced pty-output
    ///   - delta_cb:  desktop Tauri Channel for delta frames (replaces pty_delta_channels)
    ///   - remote_subs: mobile WS client subscribers with per-client mpsc channels
    pub pty_pane_registry: Arc<RwLock<HashMap<(Uuid, Uuid), PaneRegistry>>>,
    /// Remote Control (主线一): the port the WebSocket server is listening on.
    /// 0 means the server is not running (failed to bind or disabled).
    pub remote_port: Arc<RwLock<u16>>,
    /// Remote Control auth — shared TOTP generator. Created on app startup;
    /// the same secret persists for the process lifetime.
    pub remote_auth: Arc<RemoteAuth>,
    /// 零信任 #2：Ed25519 长期**设备身份**。私钥留 Rust（DPAPI/0600），仅经 invoke
    /// 暴露公钥/签名能力，**绝不进 JS/localStorage**。进程启动 `load_or_create`。
    pub device_identity: Arc<ridge_core::DeviceIdentity>,
    /// Global remote control toggle. When `false`, the remote server handlers
    /// return 503 and the WebSocket upgrade is refused. Set via the settings
    /// panel's "Remote Control" switch.
    pub remote_enabled: Arc<AtomicBool>,
    /// Handle to the remote server background thread. `None` when the server
    /// is not running. Used to join the thread on shutdown / restart.
    pub remote_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    /// One-shot sender to signal the remote server to gracefully shut down.
    /// Drained (taken) after each start/stop cycle.
    pub remote_shutdown: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// Handle to the remote server dev-mode process (`pnpm dev:remote`).
    /// `None` when not in dev mode or server not running.
    pub remote_dev_process: Arc<Mutex<Option<std::process::Child>>>,
    /// Session token store for mobile client authentication.
    /// Tokens are created via POST /verify and validated via WS ?token=.
    /// Each token expires after 3 days of inactivity.
    pub remote_session_store: Arc<SessionStore>,
    /// Brute-force throttle for TOTP verification (audit C1). Tracks failed
    /// `/verify` (+ `?code=` WS) attempts per IP and per device id, applying
    /// exponential backoff then a temp-ban; shared across both auth entry points.
    pub remote_verify_throttle: Arc<VerifyThrottle>,
    /// mDNS broadcast thread handle + stop flag. `None` when the server
    /// is not running. Set the flag and join the handle to stop.
    pub remote_mdns: Arc<Mutex<Option<(std::thread::JoinHandle<()>, Arc<AtomicBool>)>>>,
    /// Registry of currently connected remote WebSocket clients.
    /// Used by the desktop RemotePanel to list + disconnect devices.
    pub remote_client_registry: Arc<RemoteClientRegistry>,
    /// Persistent blacklist of devices/IPs barred from connecting. Loaded from
    /// `<app_data_dir>/remote-blacklist.json` at startup.
    pub remote_blacklist: Arc<RemoteBlacklist>,
    /// 「主机 / Hosts」外部主机注册表（远端 ridge / rdg）。P3/P4 基础层：登记 +
    /// 状态管理；live PTY 流传输为下一里程（见 crate::hosts）。
    pub hosts: Arc<crate::hosts::HostRegistry>,
    /// When `true`, the remote `data-request` dispatcher rejects every mutating
    /// filesystem/git method (write/delete/rename/create/copy/move + git
    /// commit/push/pull/reset/checkout/clean/…). Reads stay allowed. Defaults to
    /// `false` (writable) to preserve the existing remote file-editor behaviour;
    /// the desktop "Remote Control" panel can flip it via `set_remote_fs_readonly`
    /// for view-only sessions. NOTE: an authenticated remote already has shell
    /// stdin, so this is defence-in-depth, not an isolation boundary.
    pub remote_fs_readonly: Arc<AtomicBool>,
    /// Broadcast channel for structural changes (pane/workspace add/close/rename)
    /// that remote WS clients subscribe to. Late joiners skip stale events —
    /// they pull current state on connect or on demand.
    pub remote_structural_tx: tokio::sync::broadcast::Sender<crate::types::RemoteStructuralEvent>,
    /// Broadcast bus for generic Tauri events forwarded to desktop-browser remote
    /// clients (the "desktop UI in a browser" mode). Any host event source that
    /// the desktop UI subscribes to via `listen()` (fs-changed, teammate-*, …)
    /// publishes here; the WS handler relays each as a `{type:'event'}` frame.
    pub remote_ui_event_tx: tokio::sync::broadcast::Sender<crate::types::RemoteUiEvent>,
    /// Deep Root Mode（§8）—— 「是否存在活跃云端远控会话」标志。云端 WebRTC/E2EE
    /// provider 活在 WebView（v1），Rust 侧无法直接观测连接状态，故由前端在
    /// DataChannel open/close 时通过 `set_cloud_remote_active` 命令上报到此处。
    /// `enter_deep_root_mode` 据此做前置校验：无活跃远控时拒绝进入深根。
    pub cloud_remote_active: Arc<AtomicBool>,
    /// Deep Root Mode（§8）—— 「正在真正退出」标志。默认 `false`：窗口 close-requested
    /// 时拦截关闭并隐藏到托盘（避免误退出）。仅托盘「彻底退出 Ridge」会先置 `true`
    /// 再 `app.exit(0)`，让 close-requested 处理放行真正的退出（保存恢复集 + 停远控）。
    pub quitting: Arc<AtomicBool>,
    /// B2（D-GM-11）：cloud pane 裸字节订阅表 `pane_id → (workspace_id, sub_id)`。
    /// `subscribe_pane_raw` 登记一条 `RemotePaneSub`（把该 pane 的 RawBytes 经 Tauri
    /// event `pane-raw-{pane}` 转给本 WebView），`unsubscribe_pane_raw` 据此注销。
    pub cloud_pane_raw_subs: Arc<Mutex<HashMap<Uuid, (Uuid, u64)>>>,
}

impl AppState {
    pub fn new(event_tx: mpsc::Sender<GlobalEvent>) -> Self {
        let id = Uuid::new_v4();
        let mut map = HashMap::new();
        let mut pane_tree = PaneTree::new();
        let (startup_cwd_kind, startup_cli_cwd) = detect_startup_cwd_kind();
        // 仅 cli 启动时把 cwd 种入默认 pane；menu 启动时让 create_pane_inner 走
        // HOME / 用户配置的 fallback，避免默认终端落到 ridge.exe 安装目录。
        if let Some(cwd) = startup_cli_cwd.clone() {
            if let Some(&root_id) = pane_tree.panes.keys().next() {
                if let Some(pane) = pane_tree.panes.get_mut(&root_id) {
                    pane.cwd = Some(cwd);
                }
            }
        }
        map.insert(
            id,
            Workspace {
                pane_tree,
                terminals: HashMap::new(),
                teammate_tmux_pane_cursor: 0,
                teammate_pane_titles: HashMap::new(),
                pane_sizes: HashMap::new(),
                last_pane_index: None,
                created_at: SystemTime::now(),
                teammate_pane_states: HashMap::new(),
                teammate_agent_pane_map: HashMap::new(),
                teammate_owned_panes: HashSet::new(),
                associated_file_path: None,
                pending_spawns: HashMap::new(),
                pty_generation: HashMap::new(),
                teammate_metrics: TeammateMetrics::default(),
                display_seq: 1,
            },
        );
        Self {
            workspaces: Arc::new(RwLock::new(map)),
            workspace_order: Arc::new(RwLock::new(vec![id])),
            workspace_names: Arc::new(RwLock::new(HashMap::new())),
            active_workspace: Arc::new(RwLock::new(id)),
            next_workspace_seq: Arc::new(RwLock::new(2)),
            event_tx,
            pty_scrollback: Arc::new(RwLock::new(HashMap::new())),
            teammate_binding: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(std::sync::OnceLock::new()),
            project_store: None,
            current_project: Arc::new(RwLock::new(None)),
            git_watcher: Arc::new(GitWatcher::new()),
            fs_watcher: Arc::new(FsWatcher::new()),
            startup_cwd_kind,
            startup_cli_cwd,
            user_default_cwd: Arc::new(RwLock::new(None)),
            pty_pane_registry: Arc::new(RwLock::new(HashMap::new())),
            remote_port: Arc::new(RwLock::new(0)),
            remote_auth: Arc::new(RemoteAuth::new()),
            device_identity: Arc::new(ridge_core::DeviceIdentity::load_or_create()),
            remote_enabled: Arc::new(AtomicBool::new(false)),
            remote_thread: Arc::new(Mutex::new(None)),
            remote_shutdown: Arc::new(Mutex::new(None)),
            remote_dev_process: Arc::new(Mutex::new(None)),
            remote_session_store: Arc::new(SessionStore::new()),
            remote_verify_throttle: Arc::new(VerifyThrottle::new()),
            remote_mdns: Arc::new(Mutex::new(None)),
            remote_client_registry: Arc::new(RemoteClientRegistry::default()),
            remote_blacklist: Arc::new(RemoteBlacklist::default()),
            hosts: Arc::new(crate::hosts::HostRegistry::default()),
            remote_fs_readonly: Arc::new(AtomicBool::new(false)),
            remote_structural_tx: {
                let (tx, _) = tokio::sync::broadcast::channel(64);
                tx
            },
            remote_ui_event_tx: {
                let (tx, _) = tokio::sync::broadcast::channel(256);
                tx
            },
            cloud_remote_active: Arc::new(AtomicBool::new(false)),
            quitting: Arc::new(AtomicBool::new(false)),
            cloud_pane_raw_subs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn active_workspace_id(&self) -> Uuid {
        *self.active_workspace.read()
    }

    /// 取下一个未命名工作区的展示序号并自增。仅在创建/还原工作区时调用一次。
    pub fn allocate_workspace_seq(&self) -> u64 {
        let mut seq = self.next_workspace_seq.write();
        let n = *seq;
        *seq = seq.saturating_add(1);
        n
    }

    pub fn append_pty_scrollback(&self, ws: Uuid, pane: Uuid, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let chunk_bytes = chunk.as_bytes();
        let mut map = self.pty_scrollback.write();
        let entry = map.entry((ws, pane)).or_insert_with(PaneScrollback::new);

        // Fast path: append into `current`; when it crosses BLOCK_SIZE, freeze.
        entry.current.extend_from_slice(chunk_bytes);
        entry.total_bytes += chunk_bytes.len();

        while entry.current.len() >= SCROLLBACK_BLOCK_SIZE {
            // Freeze one block. We walk back from BLOCK_SIZE to the nearest
            // UTF-8 char boundary so the frozen slice never ends mid-codepoint.
            // Max 3 bytes of rewind since UTF-8 sequences are ≤ 4 bytes.
            let mut boundary = SCROLLBACK_BLOCK_SIZE;
            while boundary > 0 && !is_utf8_char_boundary(&entry.current, boundary) {
                boundary -= 1;
            }
            if boundary == 0 {
                // `current` starts with a continuation byte — shouldn't happen
                // because the previous freeze stopped at a boundary, but guard
                // anyway by punting: let it grow to the next boundary match.
                break;
            }
            let frozen = entry.current.drain(..boundary).collect::<Vec<u8>>();
            let frozen_start_seq = entry.current_start_seq;
            entry.blocks.push_back(Arc::new(frozen));
            entry.block_start_seqs.push_back(frozen_start_seq);
            entry.current_start_seq = frozen_start_seq + boundary as u64;

            // Cap eviction: drop oldest blocks until we're back under MAX_BYTES.
            while entry.total_bytes > SCROLLBACK_MAX_BYTES && entry.blocks.len() > 1 {
                if let Some(evicted) = entry.blocks.pop_front() {
                    entry.block_start_seqs.pop_front();
                    entry.total_bytes -= evicted.len();
                }
            }
        }
    }

    pub fn clear_pty_scrollback(&self, ws: Uuid, pane: Uuid) {
        self.pty_scrollback.write().remove(&(ws, pane));
    }

    /// Return the tail of up-to `max_bytes` bytes, starting on a UTF-8 char
    /// boundary and ending at the latest byte. Walks blocks newest-first so
    /// we allocate only what the caller asked for.
    pub fn get_pty_scrollback_tail(
        &self,
        ws: Uuid,
        pane: Uuid,
        max_bytes: usize,
    ) -> ScrollbackChunk {
        let map = self.pty_scrollback.read();
        let Some(entry) = map.get(&(ws, pane)) else {
            return ScrollbackChunk {
                bytes: String::new(),
                start_seq: 0,
                at_oldest: true,
            };
        };
        let snapshot = entry.snapshot_blocks();
        let oldest_seq = entry.oldest_seq();
        drop(map);

        if snapshot.is_empty() || max_bytes == 0 {
            return ScrollbackChunk {
                bytes: String::new(),
                start_seq: oldest_seq,
                at_oldest: true,
            };
        }

        // Walk blocks from the end, collecting up to max_bytes. `need` tracks
        // remaining capacity.
        let mut rev_pieces: Vec<&[u8]> = Vec::new();
        let mut start_seq = 0u64;
        let mut need = max_bytes;
        let mut at_oldest = true;
        for (seq, block) in snapshot.iter().rev() {
            if need == 0 {
                at_oldest = false;
                break;
            }
            if block.len() <= need {
                rev_pieces.push(&block[..]);
                need -= block.len();
                start_seq = *seq;
            } else {
                // Partial: take the tail of this block. Align to UTF-8 boundary.
                let take = block.len() - need;
                let mut aligned = take;
                while aligned < block.len() && !is_utf8_char_boundary(block, aligned) {
                    aligned += 1;
                }
                if aligned < block.len() {
                    rev_pieces.push(&block[aligned..]);
                    start_seq = *seq + aligned as u64;
                    need = 0;
                }
                at_oldest = false;
                break;
            }
        }

        let mut out: Vec<u8> = Vec::with_capacity(max_bytes - need);
        for piece in rev_pieces.iter().rev() {
            out.extend_from_slice(piece);
        }
        let bytes = String::from_utf8_lossy(&out).into_owned();
        ScrollbackChunk {
            bytes,
            start_seq,
            at_oldest: at_oldest && start_seq == oldest_seq,
        }
    }

    /// Return up-to `max_bytes` ending at (exclusive) `before_seq`. Use for
    /// paginating backwards: pass `chunk.start_seq` as the next `before_seq`.
    pub fn get_pty_scrollback_before(
        &self,
        ws: Uuid,
        pane: Uuid,
        before_seq: u64,
        max_bytes: usize,
    ) -> ScrollbackChunk {
        let map = self.pty_scrollback.read();
        let Some(entry) = map.get(&(ws, pane)) else {
            return ScrollbackChunk {
                bytes: String::new(),
                start_seq: 0,
                at_oldest: true,
            };
        };
        let snapshot = entry.snapshot_blocks();
        let oldest_seq = entry.oldest_seq();
        drop(map);

        if snapshot.is_empty() || max_bytes == 0 {
            return ScrollbackChunk {
                bytes: String::new(),
                start_seq: before_seq,
                at_oldest: before_seq <= oldest_seq,
            };
        }

        // Collect bytes with `seq < before_seq`, newest-first, up to max_bytes.
        let mut rev_pieces: Vec<Vec<u8>> = Vec::new();
        let mut start_seq = before_seq;
        let mut need = max_bytes;
        for (block_seq, block) in snapshot.iter().rev() {
            if need == 0 {
                break;
            }
            let block_end = *block_seq + block.len() as u64;
            if *block_seq >= before_seq {
                // Entire block is too new.
                continue;
            }
            // Take the portion with seq < before_seq.
            let end_within_block = (before_seq - *block_seq).min(block.len() as u64) as usize;
            let portion_start = if end_within_block <= need {
                0usize
            } else {
                end_within_block - need
            };
            // Align portion_start to UTF-8 char boundary (forward).
            let mut aligned = portion_start;
            while aligned < end_within_block && !is_utf8_char_boundary(block, aligned) {
                aligned += 1;
            }
            if aligned < end_within_block {
                rev_pieces.push(block[aligned..end_within_block].to_vec());
                let taken = end_within_block - aligned;
                need -= taken;
                start_seq = *block_seq + aligned as u64;
            }
            // Continue scanning older blocks if we still have capacity.
            let _ = block_end;
        }

        let mut out: Vec<u8> = Vec::with_capacity(max_bytes - need);
        for piece in rev_pieces.iter().rev() {
            out.extend_from_slice(piece);
        }
        let bytes = String::from_utf8_lossy(&out).into_owned();
        ScrollbackChunk {
            bytes,
            start_seq,
            at_oldest: start_seq <= oldest_seq,
        }
    }

    /// P4.1 — install (or replace) the delta-byte sender for `(ws, pane)`.
    /// Subsequent `pty-delta-*` emit sites route bytes through this sender
    /// instead of `app.emit`. Idempotent — a second register for the same
    /// pane replaces the previous sender (the old `Arc` is dropped).
    pub fn register_pane_delta_channel(
        &self,
        workspace_id: Uuid,
        pane_id: Uuid,
        sender: PaneDeltaSender,
    ) {
        self.pty_pane_registry
            .write()
            .entry((workspace_id, pane_id))
            .or_default()
            .delta_cb = Some(sender);
    }

    pub fn unregister_pane_delta_channel(&self, workspace_id: Uuid, pane_id: Uuid) {
        let mut reg = self.pty_pane_registry.write();
        let key = (workspace_id, pane_id);
        if let Some(entry) = reg.get_mut(&key) {
            entry.delta_cb = None;
            if entry.is_empty() {
                reg.remove(&key);
            }
        }
    }

    pub fn get_pane_delta_channel(
        &self,
        workspace_id: Uuid,
        pane_id: Uuid,
    ) -> Option<PaneDeltaSender> {
        self.pty_pane_registry
            .read()
            .get(&(workspace_id, pane_id))
            .and_then(|e| e.delta_cb.clone())
    }

    pub fn register_pane_output_channel(
        &self,
        workspace_id: Uuid,
        pane_id: Uuid,
        sender: PaneOutputSender,
    ) {
        self.pty_pane_registry
            .write()
            .entry((workspace_id, pane_id))
            .or_default()
            .output_cb = Some(sender);
    }

    pub fn unregister_pane_output_channel(&self, workspace_id: Uuid, pane_id: Uuid) {
        let mut reg = self.pty_pane_registry.write();
        let key = (workspace_id, pane_id);
        if let Some(entry) = reg.get_mut(&key) {
            entry.output_cb = None;
            if entry.is_empty() {
                reg.remove(&key);
            }
        }
    }

    pub fn get_pane_output_channel(
        &self,
        workspace_id: Uuid,
        pane_id: Uuid,
    ) -> Option<PaneOutputSender> {
        self.pty_pane_registry
            .read()
            .get(&(workspace_id, pane_id))
            .and_then(|e| e.output_cb.clone())
    }

    /// Retrieve the most recent PTY scrollback bytes for a pane, up to
    /// `max_bytes`. Used to seed a newly-created mobile `PaneParser` so its
    /// state mirrors the desktop parser before the first delta frame is sent.
    pub fn get_recent_scrollback_for(&self, ws: Uuid, pane: Uuid, max_bytes: usize) -> Vec<u8> {
        let sb = self.pty_scrollback.read();
        let Some(scrollback) = sb.get(&(ws, pane)) else {
            return Vec::new();
        };
        let blocks = scrollback.snapshot_blocks();
        let mut total: usize = 0;
        // Walk most-recent block first.
        let mut recent: Vec<Vec<u8>> = Vec::new();
        for (_, bytes) in blocks.into_iter().rev() {
            if total + bytes.len() <= max_bytes {
                recent.push((*bytes).clone());
                total += bytes.len();
            } else if total < max_bytes {
                let remaining = max_bytes - total;
                let start = bytes.len().saturating_sub(remaining);
                recent.push(bytes[start..].to_vec());
                total = max_bytes;
                break;
            } else {
                break;
            }
        }
        recent.reverse(); // restore chronological order
        let mut result = Vec::with_capacity(total);
        for block in recent {
            result.extend_from_slice(&block);
        }
        result
    }

    pub fn register_remote_sub(&self, ws: Uuid, pane: Uuid, sub: RemotePaneSub) {
        self.pty_pane_registry
            .write()
            .entry((ws, pane))
            .or_default()
            .remote_subs
            .push(sub);
    }

    pub fn unregister_remote_sub(&self, ws: Uuid, pane: Uuid, sub_id: u64) {
        let mut reg = self.pty_pane_registry.write();
        let key = (ws, pane);
        if let Some(entry) = reg.get_mut(&key) {
            entry.remote_subs.retain(|s| s.id != sub_id);
            if entry.is_empty() {
                reg.remove(&key);
            }
        }
    }

    /// Fan a non-byte remote event (title/cwd metadata, PTY resize) out to every
    /// subscriber of `(ws, pane)`. Cheap: the registry read lock is held only for
    /// the duration of the `try_send` loop and the event is cloned per sub (a few
    /// strings at most). Drops are ignored — metadata is advisory, and the next
    /// update supersedes a lost one. Raw PTY bytes use a dedicated hot path in
    /// `lib.rs` (single `Arc<Vec<u8>>` shared across subs) and do NOT go through
    /// here.
    pub fn broadcast_remote_event(&self, ws: Uuid, pane: Uuid, event: RemotePtyEvent) {
        let reg = self.pty_pane_registry.read();
        if let Some(entry) = reg.get(&(ws, pane)) {
            for sub in &entry.remote_subs {
                let _ = sub.raw_tx.try_send(event.clone());
            }
        }
    }
}

impl PaneRegistry {
    fn is_empty(&self) -> bool {
        self.output_cb.is_none() && self.delta_cb.is_none() && self.remote_subs.is_empty()
    }
}

/// Same semantics as `str::is_char_boundary` but on a raw `&[u8]`. Returns
/// true at index `0`, `len`, and any position where the next byte is NOT
/// a UTF-8 continuation byte (`10xxxxxx`).
fn is_utf8_char_boundary(bytes: &[u8], index: usize) -> bool {
    if index == 0 || index == bytes.len() {
        return true;
    }
    if index > bytes.len() {
        return false;
    }
    // A boundary is any byte that is NOT a continuation byte.
    (bytes[index] & 0b1100_0000) != 0b1000_0000
}

#[cfg(test)]
mod scrollback_tests {
    use super::*;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    /// Build an isolated AppState for tests. `AppState::new` seeds a default
    /// workspace + PaneTree — fine for scrollback tests since we key by
    /// `(Uuid, Uuid)` directly.
    fn make_state() -> (AppState, Uuid, Uuid) {
        let (tx, _rx) = mpsc::channel::<GlobalEvent>(1);
        let state = AppState::new(tx);
        let ws = state.active_workspace_id();
        let pane = Uuid::new_v4();
        (state, ws, pane)
    }

    #[test]
    fn append_tail_round_trip_small() {
        let (state, ws, pane) = make_state();
        state.append_pty_scrollback(ws, pane, "hello\n");
        state.append_pty_scrollback(ws, pane, "world\n");
        let chunk = state.get_pty_scrollback_tail(ws, pane, 1024);
        assert_eq!(chunk.bytes, "hello\nworld\n");
        assert!(chunk.at_oldest);
        assert_eq!(chunk.start_seq, 0);
    }

    #[test]
    fn tail_respects_max_bytes_from_end() {
        let (state, ws, pane) = make_state();
        // Push 4 full blocks worth (256 KiB total).
        let chunk = "x".repeat(SCROLLBACK_BLOCK_SIZE);
        for _ in 0..4 {
            state.append_pty_scrollback(ws, pane, &chunk);
        }
        // Ask for just 1024 bytes — must come from the latest block.
        let got = state.get_pty_scrollback_tail(ws, pane, 1024);
        assert_eq!(got.bytes.len(), 1024);
        assert!(!got.at_oldest);
        // Start seq equals total - 1024.
        assert_eq!(got.start_seq, (4 * SCROLLBACK_BLOCK_SIZE - 1024) as u64);
    }

    #[test]
    fn before_pages_backwards_until_oldest() {
        let (state, ws, pane) = make_state();
        // 3 × 64 KiB + a short trailing partial.
        let full = "A".repeat(SCROLLBACK_BLOCK_SIZE);
        state.append_pty_scrollback(ws, pane, &full);
        let full_b = "B".repeat(SCROLLBACK_BLOCK_SIZE);
        state.append_pty_scrollback(ws, pane, &full_b);
        let full_c = "C".repeat(SCROLLBACK_BLOCK_SIZE);
        state.append_pty_scrollback(ws, pane, &full_c);
        state.append_pty_scrollback(ws, pane, "tail");

        // Start from tail.
        let tail = state.get_pty_scrollback_tail(ws, pane, 10);
        assert_eq!(tail.bytes.len(), 10);

        let before = state.get_pty_scrollback_before(ws, pane, tail.start_seq, 1024);
        assert_eq!(before.bytes.len(), 1024);
        assert_eq!(before.start_seq, tail.start_seq - 1024);

        // Page all the way back.
        let mut cursor = tail.start_seq;
        let mut total_read: u64 = 0;
        loop {
            let page = state.get_pty_scrollback_before(ws, pane, cursor, 32 * 1024);
            total_read += page.bytes.len() as u64;
            if page.at_oldest || page.bytes.is_empty() {
                break;
            }
            cursor = page.start_seq;
        }
        let tail_len = tail.bytes.len() as u64;
        assert_eq!(total_read + tail_len, 3 * SCROLLBACK_BLOCK_SIZE as u64 + 4);
    }

    #[test]
    fn eviction_when_over_cap() {
        let (state, ws, pane) = make_state();
        // Push 5 MiB → should evict oldest blocks until ≤ SCROLLBACK_MAX_BYTES.
        let chunk = "x".repeat(SCROLLBACK_BLOCK_SIZE);
        for _ in 0..80 {
            // 80 * 64 KiB = 5 MiB
            state.append_pty_scrollback(ws, pane, &chunk);
        }
        let map = state.pty_scrollback.read();
        let entry = map.get(&(ws, pane)).expect("entry");
        assert!(
            entry.total_bytes <= SCROLLBACK_MAX_BYTES,
            "post-cap total_bytes = {}",
            entry.total_bytes
        );
    }

    #[test]
    fn utf8_multibyte_never_split_on_block_boundary() {
        let (state, ws, pane) = make_state();
        // Fill up to 1 byte short of a block then push a 3-byte CJK char.
        let padding = "a".repeat(SCROLLBACK_BLOCK_SIZE - 1);
        state.append_pty_scrollback(ws, pane, &padding);
        state.append_pty_scrollback(ws, pane, "中文"); // 2 × 3-byte each

        // The frozen block should end on a UTF-8 boundary, not mid-codepoint.
        let map = state.pty_scrollback.read();
        let entry = map.get(&(ws, pane)).expect("entry");
        assert!(!entry.blocks.is_empty(), "block should have frozen");
        let front = entry.blocks.front().unwrap();
        // Decoding must succeed — if we split mid-codepoint it would fail.
        assert!(std::str::from_utf8(front).is_ok());
    }

    #[test]
    fn tail_empty_when_pane_unknown() {
        let (state, ws, _pane) = make_state();
        let unknown_pane = Uuid::new_v4();
        let chunk = state.get_pty_scrollback_tail(ws, unknown_pane, 1024);
        assert!(chunk.bytes.is_empty());
        assert!(chunk.at_oldest);
    }

    #[test]
    fn clear_removes_pane_entry() {
        let (state, ws, pane) = make_state();
        state.append_pty_scrollback(ws, pane, "hi");
        state.clear_pty_scrollback(ws, pane);
        let chunk = state.get_pty_scrollback_tail(ws, pane, 1024);
        assert!(chunk.bytes.is_empty());
    }
}

#[cfg(test)]
mod pty_delta_channel_tests {
    //! P4.1 unit tests — verify the `(ws, pane)` keyed delta-sender
    //! registry: registration/lookup, idempotent replace, key isolation
    //! across panes and across workspaces, and removal on unregister.
    //! The senders are plain closures so these tests do NOT need a Tauri
    //! runtime — that boundary lives in `commands/terminal.rs`.

    use super::*;
    use parking_lot::Mutex as PMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    fn make_state() -> AppState {
        let (tx, _rx) = mpsc::channel::<GlobalEvent>(1);
        AppState::new(tx)
    }

    /// Build a counting sender: each call increments `count` and records the
    /// payload length. Returns `(sender, count, last_len)` so the test can
    /// assert how many bytes the channel was asked to send.
    fn counting_sender() -> (PaneDeltaSender, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let last_len = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&count);
        let last_len_clone = Arc::clone(&last_len);
        let sender: PaneDeltaSender = Arc::new(move |bytes: Vec<u8>| {
            count_clone.fetch_add(1, Ordering::SeqCst);
            last_len_clone.store(bytes.len(), Ordering::SeqCst);
        });
        (sender, count, last_len)
    }

    #[test]
    fn get_returns_none_when_unregistered() {
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane = Uuid::new_v4();
        assert!(state.get_pane_delta_channel(ws, pane).is_none());
    }

    #[test]
    fn register_then_get_returns_same_sender() {
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let (sender, count, last_len) = counting_sender();

        state.register_pane_delta_channel(ws, pane, sender);
        let fetched = state
            .get_pane_delta_channel(ws, pane)
            .expect("registered sender must be retrievable");
        fetched(vec![1, 2, 3, 4]);

        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(last_len.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn register_is_idempotent_replace() {
        // Second register on the same key replaces the first sender; the
        // old Arc is dropped (which we verify via a sentinel flag).
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane = Uuid::new_v4();

        let dropped = Arc::new(AtomicUsize::new(0));
        let drop_flag = Arc::clone(&dropped);
        struct DropFlag(Arc<AtomicUsize>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let first_flag = PMutex::new(Some(DropFlag(drop_flag)));
        let first: PaneDeltaSender = Arc::new(move |_| {
            // Touch the flag so the closure owns it.
            let _ = first_flag.lock().is_some();
        });
        state.register_pane_delta_channel(ws, pane, first);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);

        let (second, count, _) = counting_sender();
        state.register_pane_delta_channel(ws, pane, second);

        // After replacement, the first sender's Arc count reached zero and
        // the embedded DropFlag fired exactly once.
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        // The new sender is the one we retrieve.
        let fetched = state.get_pane_delta_channel(ws, pane).expect("present");
        fetched(vec![0]);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn different_panes_in_same_workspace_are_isolated() {
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let pane_b = Uuid::new_v4();
        let (sender_a, count_a, _) = counting_sender();
        let (sender_b, count_b, _) = counting_sender();

        state.register_pane_delta_channel(ws, pane_a, sender_a);
        state.register_pane_delta_channel(ws, pane_b, sender_b);

        state.get_pane_delta_channel(ws, pane_a).expect("pane_a")(vec![0]);
        state.get_pane_delta_channel(ws, pane_a).expect("pane_a")(vec![0, 0]);
        state.get_pane_delta_channel(ws, pane_b).expect("pane_b")(vec![0]);

        assert_eq!(count_a.load(Ordering::SeqCst), 2);
        assert_eq!(count_b.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn same_pane_id_in_different_workspaces_is_isolated() {
        // Tauri pane UUIDs are globally unique in practice, but the map key
        // is (workspace, pane) so even a colliding pane uuid across two
        // workspaces must dispatch to the correct sender.
        let state = make_state();
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let (sender_a, count_a, _) = counting_sender();
        let (sender_b, count_b, _) = counting_sender();

        state.register_pane_delta_channel(ws_a, pane, sender_a);
        state.register_pane_delta_channel(ws_b, pane, sender_b);

        state.get_pane_delta_channel(ws_a, pane).expect("a")(vec![1]);
        state.get_pane_delta_channel(ws_b, pane).expect("b")(vec![2]);
        state.get_pane_delta_channel(ws_b, pane).expect("b")(vec![3]);

        assert_eq!(count_a.load(Ordering::SeqCst), 1);
        assert_eq!(count_b.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn unregister_drops_the_sender() {
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let (sender, count, _) = counting_sender();

        state.register_pane_delta_channel(ws, pane, sender);
        assert!(state.get_pane_delta_channel(ws, pane).is_some());

        state.unregister_pane_delta_channel(ws, pane);
        assert!(state.get_pane_delta_channel(ws, pane).is_none());

        // Cleanup must NOT increment the counter — nothing was sent.
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn unregister_unknown_pane_is_silent() {
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane = Uuid::new_v4();
        // No panic / no observable side effect on an unknown key.
        state.unregister_pane_delta_channel(ws, pane);
        assert!(state.get_pane_delta_channel(ws, pane).is_none());
    }

    #[test]
    fn unregister_only_affects_matching_key() {
        let state = make_state();
        let ws = Uuid::new_v4();
        let pane_keep = Uuid::new_v4();
        let pane_drop = Uuid::new_v4();
        let (keep, _, _) = counting_sender();
        let (drop_it, _, _) = counting_sender();

        state.register_pane_delta_channel(ws, pane_keep, keep);
        state.register_pane_delta_channel(ws, pane_drop, drop_it);
        state.unregister_pane_delta_channel(ws, pane_drop);

        assert!(state.get_pane_delta_channel(ws, pane_keep).is_some());
        assert!(state.get_pane_delta_channel(ws, pane_drop).is_none());
    }

    #[test]
    fn cloned_appstate_shares_channel_registry() {
        // AppState is `#[derive(Clone)]` and the registry is wrapped in
        // `Arc<RwLock<...>>`. Cloning the state must NOT fork the registry
        // — both clones see the same registrations. Otherwise the emit-site
        // clone in `lib.rs` would see an empty registry and silently fall
        // back to app.emit for every pane.
        let state_a = make_state();
        let state_b = state_a.clone();
        let ws = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let (sender, count, _) = counting_sender();

        state_a.register_pane_delta_channel(ws, pane, sender);
        let from_b = state_b
            .get_pane_delta_channel(ws, pane)
            .expect("clone must see registrations made on the original");
        from_b(vec![9]);
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Unregister via clone B; clone A should see the empty slot too.
        state_b.unregister_pane_delta_channel(ws, pane);
        assert!(state_a.get_pane_delta_channel(ws, pane).is_none());
    }
}
