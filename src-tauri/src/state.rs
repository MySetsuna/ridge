use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
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
use crate::types::GlobalEvent;

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
    #[allow(dead_code)] // half-built: enum + serialization (commands/pane.rs:60-64) + TS union + UI badge (SplitContainer.svelte:592-599) all in place, but teammate/server.rs:register_agent_to_pane goes Idle→Busy directly. See TASKS §1.14.
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
    /// 关联的 .ridge 文件绝对路径。`Some` 表示该工作区已保存到磁盘；
    /// 后续任何 cwd/布局/git 变化都会触发防抖自动回写。
    pub associated_file_path: Option<PathBuf>,
    /// Phase-1 PTY records waiting for `activate_pane_pty` to spawn the child.
    /// Keyed by pane id. See `PendingSpawn` for the rationale behind splitting
    /// `openpty` from `spawn_command` into two stages.
    pub pending_spawns: HashMap<Uuid, PendingSpawn>,
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
/// Hard retention cap per pane. 4 MiB covers ~30k lines of 120-char output,
/// which comfortably outlives a typical `cat log.txt`.
pub const SCROLLBACK_MAX_BYTES: usize = 4 * 1024 * 1024;

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
        let mut out: Vec<(u64, Arc<Vec<u8>>)> =
            Vec::with_capacity(self.blocks.len() + 1);
        for (i, b) in self.blocks.iter().enumerate() {
            out.push((self.block_start_seqs[i], Arc::clone(b)));
        }
        if !self.current.is_empty() {
            out.push((self.current_start_seq, Arc::new(self.current.clone())));
        }
        out
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
}

impl AppState {
    pub fn new(event_tx: mpsc::Sender<GlobalEvent>) -> Self {
        let id = Uuid::new_v4();
        let mut map = HashMap::new();
        let mut pane_tree = PaneTree::new();
        // 将启动 cwd 种入默认 pane：从命令行 / 资源管理器启动时，用户期望默认终端
        // 落在他们当前所在的目录，而不是 HOME 兜底。若无 .ridge 工作区覆盖这颗默认树，
        // 这个 cwd 将直接被 create_pane 采用。
        if let Ok(cwd) = std::env::current_dir() {
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
                associated_file_path: None,
                pending_spawns: HashMap::new(),
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
            project_store: None,
            current_project: Arc::new(RwLock::new(None)),
            git_watcher: Arc::new(GitWatcher::new()),
            fs_watcher: Arc::new(FsWatcher::new()),
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