use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::commands::terminal;
use crate::engine::pane_tree::{DockRegion, PaneNode as EnginePaneNode, SplitDirection};
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};
use crate::utils::error::AppError;
use crate::utils::pane_id::parse_pane_id;

/// Returned by `split_pane` so the frontend can immediately seed `paneCwdStore`
/// without waiting for the first `pane-cwd-changed` event from shell integration.
#[derive(Debug, Serialize)]
pub struct SplitPaneResult {
    pub pane_id: String,
    pub initial_cwd: Option<String>,
}

/// 与前端 `PaneNode`（Svelte）对齐，便于 `invoke('get_pane_layout')` 直接驱动 SplitContainer。
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LayoutNode {
    Leaf {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    Split {
        id: String,
        direction: String,
        children: Vec<LayoutNode>,
        ratios: Vec<f32>,
    },
}

fn engine_node_to_layout(
    node: &EnginePaneNode,
    split_counter: &mut u64,
    titles: &HashMap<Uuid, String>,
    panes: &std::collections::HashMap<Uuid, crate::engine::pane_tree::Pane>,
) -> LayoutNode {
    match node {
        EnginePaneNode::Leaf(id) => LayoutNode::Leaf {
            id: id.to_string(),
            title: titles.get(id).cloned(),
            cwd: panes.get(id).and_then(|p| p.cwd.as_ref().map(|c| c.to_string_lossy().into_owned())),
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
                    .map(|c| engine_node_to_layout(c, split_counter, titles, panes))
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
    Ok(engine_node_to_layout(
        &ws.pane_tree.root,
        &mut c,
        &ws.teammate_pane_titles,
        &ws.pane_tree.panes,
    ))
}

#[tauri::command]
pub async fn split_pane(
    state: State<'_, AppState>,
    pane_id: String,
    direction: String,
) -> Result<SplitPaneResult, String> {
    split_pane_inner(state, pane_id, direction).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_split_ratios_at_path(
    state: State<'_, AppState>,
    path: Vec<usize>,
    ratios: Vec<f32>,
) -> Result<(), String> {
    let wid = state.active_workspace_id();
    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| "无活动工作区".to_string())?;
    ws.pane_tree
        .set_split_ratios_at_path(&path, ratios)
        .map_err(|e| e.to_string())?;
    drop(map);
    crate::commands::wind_file::schedule_auto_save(&*state, wid);
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct SplitRatioUpdate {
    pub path: Vec<usize>,
    pub ratios: Vec<f32>,
}

#[tauri::command]
pub async fn set_split_ratios_batch(
    state: State<'_, AppState>,
    updates: Vec<SplitRatioUpdate>,
) -> Result<(), String> {
    let wid = state.active_workspace_id();
    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| "无活动工作区".to_string())?;
    let pairs: Vec<(Vec<usize>, Vec<f32>)> = updates
        .into_iter()
        .map(|u| (u.path, u.ratios))
        .collect();
    ws.pane_tree
        .set_split_ratios_batch(&pairs)
        .map_err(|e| e.to_string())?;
    drop(map);
    crate::commands::wind_file::schedule_auto_save(&*state, wid);
    Ok(())
}

#[tauri::command]
pub async fn dock_pane(
    state: State<'_, AppState>,
    source_pane_id: String,
    target_pane_id: String,
    region: String,
) -> Result<(), String> {
    let region = match region.to_lowercase().as_str() {
        "left" => DockRegion::Left,
        "right" => DockRegion::Right,
        "top" => DockRegion::Top,
        "bottom" => DockRegion::Bottom,
        "center" => DockRegion::Center,
        _ => return Err(format!("invalid dock region: {region}")),
    };
    let source = parse_pane_id(&source_pane_id).map_err(|e| e.to_string())?;
    let target = parse_pane_id(&target_pane_id).map_err(|e| e.to_string())?;
    let wid = state.active_workspace_id();
    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| "无活动工作区".to_string())?;
    ws.pane_tree
        .dock_pane(source, target, region)
        .map_err(|e| e.to_string())?;
    drop(map);
    crate::commands::wind_file::schedule_auto_save(&*state, wid);
    Ok(())
}

fn split_pane_inner(
    state: State<'_, AppState>,
    pane_id: String,
    direction: String,
) -> Result<SplitPaneResult, AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let dir = if direction == "horizontal" {
        SplitDirection::Horizontal
    } else {
        SplitDirection::Vertical
    };
    let wid = state.active_workspace_id();

    // 取父 pane 的 cwd：优先从 pane_tree 读（已被 OSC 7 或定时轮询同步过）；
    // 若 tree 尚未记录（例如 PowerShell/cmd 无 OSC 7 且刚 spawn 还未被轮询），
    // 就现场查 shell 进程 OS 层 cwd，保证"分屏一定继承当前目录"。
    let parent_cwd: Option<String> = {
        let map = state.workspaces.read();
        map.get(&wid)
            .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
            .and_then(|p| p.cwd.as_ref().map(|c| c.to_string_lossy().into_owned()))
    }
    .or_else(|| crate::commands::process::current_pane_cwd_live(&*state, wid, pane_id));

    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
    // 如果现场探到了新的 cwd，顺手回填父 pane，使其后续 split 也能走 tree 快路径。
    if let Some(ref cwd_str) = parent_cwd {
        if let Some(parent) = ws.pane_tree.panes.get_mut(&pane_id) {
            if parent.cwd.is_none() {
                parent.cwd = Some(std::path::PathBuf::from(cwd_str));
            }
        }
    }
    let new_pane_id = ws.pane_tree.split(pane_id, dir)?;
    if let Some(ref cwd_str) = parent_cwd {
        if let Some(new_pane) = ws.pane_tree.panes.get_mut(&new_pane_id) {
            new_pane.cwd = Some(std::path::PathBuf::from(cwd_str));
        }
    }
    drop(map);
    crate::commands::wind_file::schedule_auto_save(&*state, wid);
    Ok(SplitPaneResult {
        pane_id: new_pane_id.to_string(),
        initial_cwd: parent_cwd,
    })
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
        ws.teammate_pane_titles.remove(&pane_id);
        ws.pane_tree.close(pane_id).map_err(|e| e.to_string())?;
    }
    crate::commands::wind_file::schedule_auto_save(&*state, wid);
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
