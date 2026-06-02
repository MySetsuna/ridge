use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// Unified event type for the single per-client mpsc channel.
#[derive(Clone, Debug)]
pub enum RemotePtyEvent {
    RawBytes {
        workspace_id: Uuid,
        pane_id: Uuid,
        bytes: Arc<Vec<u8>>,
    },
    /// Title / cwd update for a pane. Either field may be `None` when only the
    /// other changed. Sent out-of-band from the raw byte stream so the client
    /// can update its tab/document title without parsing the PTY stream itself.
    Metadata {
        workspace_id: Uuid,
        pane_id: Uuid,
        title: Option<String>,
        cwd: Option<String>,
    },
    PtyResized {
        workspace_id: Uuid,
        pane_id: Uuid,
        rows: u16,
        cols: u16,
    },
}

/// Structural change broadcast to all connected remote WS clients.
/// Sent via tokio::sync::broadcast so late joiners skip stale events (they'll
/// pull the current state on connect).
#[derive(Clone, Debug)]
pub enum RemoteStructuralEvent {
    PanesChanged { workspace_id: Uuid },
    WorkspacesChanged,
    WorkspaceRenamed { workspace_id: Uuid, name: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PaneMode {
    Terminal,
    Editor { file_path: Option<PathBuf>, language: String },
}

#[derive(Clone, Serialize)]
pub enum GlobalEvent {
    PtyOutput {
        workspace_id: Uuid,
        pane_id: Uuid,
        data: String,
    },
    PaneClosed {
        workspace_id: Uuid,
        pane_id: Uuid,
    },
    PaneModeChanged {
        workspace_id: Uuid,
        pane_id: Uuid,
        mode: PaneMode,
    },
    PaneCwdChanged {
        workspace_id: Uuid,
        pane_id: Uuid,
        cwd: String,
    },
    /// PTY 解析到 OSC 0/1/2 标题序列时 emit。前端按 teammate > OSC > 进程名
    /// 优先级合并展示，让 shell / 长跑程序（如 Claude Code）设置的标题能反映出来。
    PaneTitleChanged {
        workspace_id: Uuid,
        pane_id: Uuid,
        title: String,
    },
    /// PTY 解析到 shell-integration prompt 标记时 emit。FinalTerm `OSC 133;A`
    /// 与 VS Code `OSC 633;A` 都触发同一事件 —— 它们语义都是"shell 又回到
    /// 了交互式 prompt"，是触发 git diff 等 prompt-cycle 任务的精准信号。
    /// 前端 Pane.svelte 在 BUG-1 patch 之上叠加这个 fast path：收到事件
    /// 即立即刷新 diff，比 800ms trailing-edge debounce 反应更快，且对
    /// 持续输出（`tail -f`）不会误触发。Shell 没启用 shell-integration
    /// 时 backend 不会 emit，前端的 debounce 仍作为 fallback 保留。
    PanePromptDetected {
        workspace_id: Uuid,
        pane_id: Uuid,
    },
    /// Pane tree changed (split/close) in a workspace — desktop frontend should
    /// refresh its paneTree store. Emitted by both Tauri commands (desktop
    /// actions) and remote WS handlers.
    PaneTreeChanged {
        workspace_id: Uuid,
    },
    /// Workspace list changed (add/close/rename) — desktop frontend should
    /// refresh its workspace data.
    WorkspaceListChanged,
}