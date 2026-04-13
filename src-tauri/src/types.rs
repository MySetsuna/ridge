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
}