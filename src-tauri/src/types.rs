use std::path::PathBuf;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

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
}