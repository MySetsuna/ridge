use std::path::PathBuf;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// 与前端 `paneId === 'root'` 对齐；`PaneTree::new` 也使用该 id 作为根叶子。
pub const ROOT_PANE_ID: Uuid = Uuid::from_u128(1);

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
}