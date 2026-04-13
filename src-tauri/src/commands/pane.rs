use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::commands::terminal;
use crate::engine::pane_tree::{PaneNode as EnginePaneNode, SplitDirection};
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode, ROOT_PANE_ID};
use crate::utils::error::AppError;
use crate::utils::pane_id::parse_pane_id;

/// 与前端 `PaneNode`（Svelte）对齐，便于 `invoke('get_pane_layout')` 直接驱动 SplitContainer。
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LayoutNode {
    Leaf {
        id: String,
    },
    Split {
        id: String,
        direction: String,
        children: Vec<LayoutNode>,
        ratios: Vec<f32>,
    },
}

fn uuid_to_pane_id(u: Uuid) -> String {
    if u == ROOT_PANE_ID {
        "root".to_string()
    } else {
        u.to_string()
    }
}

fn engine_node_to_layout(node: &EnginePaneNode, split_counter: &mut u64) -> LayoutNode {
    match node {
        EnginePaneNode::Leaf(id) => LayoutNode::Leaf {
            id: uuid_to_pane_id(*id),
        },
        EnginePaneNode::Split {
            direction,
            children,
            ratios,
        } => {
            *split_counter += 1;
            LayoutNode::Split {
                id: format!("split-{}", split_counter),
                direction: match direction {
                    SplitDirection::Horizontal => "horizontal",
                    SplitDirection::Vertical => "vertical",
                }
                .to_string(),
                children: children
                    .iter()
                    .map(|c| engine_node_to_layout(c, split_counter))
                    .collect(),
                ratios: ratios.clone(),
            }
        }
    }
}

#[tauri::command]
pub fn get_pane_layout(state: State<'_, AppState>) -> Result<LayoutNode, String> {
    let wid = state.active_workspace_id();
    let map = state.workspaces.read();
    let ws = map
        .get(&wid)
        .ok_or_else(|| "无活动工作区".to_string())?;
    let mut c = 0u64;
    Ok(engine_node_to_layout(&ws.pane_tree.root, &mut c))
}

#[tauri::command]
pub async fn split_pane(
    state: State<'_, AppState>,
    pane_id: String,
    direction: String,
) -> Result<Uuid, String> {
    split_pane_inner(state, pane_id, direction).map_err(|e| e.to_string())
}

fn split_pane_inner(
    state: State<'_, AppState>,
    pane_id: String,
    direction: String,
) -> Result<Uuid, AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let dir = if direction == "horizontal" {
        SplitDirection::Horizontal
    } else {
        SplitDirection::Vertical
    };
    let wid = state.active_workspace_id();
    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
    ws.pane_tree.split(pane_id, dir)
}

pub(crate) fn teammate_pane_uuid_at_index(
    app: &AppState,
    workspace_id: Uuid,
    pane_index: usize,
) -> Result<Uuid, AppError> {
    let map = app.workspaces.read();
    let ws = map
        .get(&workspace_id)
        .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
    let leaves = ws.pane_tree.get_all_leaves();
    leaves
        .get(pane_index)
        .copied()
        .ok_or_else(|| AppError::InvalidPaneId(format!("pane index {pane_index}")))
}

/// Claude Code `tmux split-window`：`-h` → `horizontal`，`-v` → `vertical`（与 Wind UI / `split_pane` 一致）。
pub(crate) fn teammate_split_pane(
    app: &AppState,
    workspace_id: Uuid,
    pane_index: usize,
    direction: &str,
) -> Result<Uuid, AppError> {
    let target = {
        let map = app.workspaces.read();
        let ws = map
            .get(&workspace_id)
            .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
        let leaves = ws.pane_tree.get_all_leaves();
        leaves
            .get(pane_index)
            .copied()
            .ok_or_else(|| AppError::InvalidPaneId(format!("pane index {pane_index}")))?
    };
    let dir = if direction == "horizontal" {
        SplitDirection::Horizontal
    } else {
        SplitDirection::Vertical
    };
    let mut map = app.workspaces.write();
    let ws = map
        .get_mut(&workspace_id)
        .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
    ws.pane_tree.split(target, dir)
}

/// 关闭指定窗格：结束 PTY、从 PaneTree 移除。至少保留一个窗格。
#[tauri::command]
pub async fn close_pane(state: State<'_, AppState>, pane_id: String) -> Result<(), String> {
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let wid = state.active_workspace_id();
    let leaves: Vec<Uuid> = {
        let map = state.workspaces.read();
        let ws = map.get(&wid).ok_or_else(|| "无活动工作区".to_string())?;
        ws.pane_tree.get_all_leaves()
    };
    if leaves.len() <= 1 {
        return Err("无法关闭最后一个窗格".to_string());
    }
    if !leaves.contains(&pane_id) {
        return Err(AppError::PaneNotFound(pane_id).to_string());
    }
    terminal::kill_pty_if_present(&*state, wid, pane_id).await;
    {
        let mut map = state.workspaces.write();
        let ws = map.get_mut(&wid).ok_or_else(|| "无活动工作区".to_string())?;
        ws.pane_tree.close(pane_id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn toggle_mode(
    state: State<'_, AppState>,
    pane_id: String,
    mode: PaneMode,
) -> Result<(), String> {
    toggle_mode_inner(state, pane_id, mode)
        .await
        .map_err(|e| e.to_string())
}

async fn toggle_mode_inner(
    state: State<'_, AppState>,
    pane_id: String,
    mode: PaneMode,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let wid = state.active_workspace_id();
    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&wid)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        let pane = ws
            .pane_tree
            .panes
            .get_mut(&pane_id)
            .ok_or(AppError::PaneNotFound(pane_id))?;
        pane.mode = mode.clone();
    }
    let _ = state
        .event_tx
        .send(GlobalEvent::PaneModeChanged {
            workspace_id: wid,
            pane_id,
            mode,
        })
        .await;
    Ok(())
}
