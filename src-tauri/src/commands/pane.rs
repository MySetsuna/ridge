use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::commands::terminal;
use crate::engine::pane_tree::{DockRegion, PaneNode as EnginePaneNode, SplitDirection};
use crate::state::AppState;
use crate::teammate::layout_event::{LayoutChange, TEAMMATE_LAYOUT_CHANGED};
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
/// `agent_state` 与 `agent_id` 在 Claude Code teammate 通过
/// `/api/v1/register-agent` 记下某个 pane 为 Busy 时出现；前端据此在标题栏
/// 画一个"运行中"指示，让 orchestrator 能一眼看出哪些 pane 有 sub-agent。
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LayoutNode {
    Leaf {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// "idle" | "busy" | "starting"；`None` 表示从未被 teammate 接触过。
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_state: Option<String>,
        /// 若 pane 当前有注册的 agent，回传其 `agent_id`。
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
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
    pane_states: &HashMap<Uuid, crate::state::PaneState>,
    agent_by_pane: &HashMap<Uuid, String>,
) -> LayoutNode {
    match node {
        EnginePaneNode::Leaf(id) => {
            let agent_state = pane_states.get(id).map(|s| {
                match s {
                    crate::state::PaneState::Idle => "idle",
                    crate::state::PaneState::Busy => "busy",
                    crate::state::PaneState::Starting => "starting",
                }
                .to_string()
            });
            LayoutNode::Leaf {
                id: id.to_string(),
                title: titles.get(id).cloned(),
                cwd: panes
                    .get(id)
                    .and_then(|p| p.cwd.as_ref().map(|c| c.to_string_lossy().into_owned())),
                agent_state,
                agent_id: agent_by_pane.get(id).cloned(),
            }
        }
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
                    .map(|c| {
                        engine_node_to_layout(
                            c,
                            split_counter,
                            titles,
                            panes,
                            pane_states,
                            agent_by_pane,
                        )
                    })
                    .collect(),
                ratios: ratios.clone(),
            }
        }
    }
}

#[tauri::command]
pub fn get_pane_layout(state: State<'_, AppState>) -> Result<LayoutNode, String> {
    let wid = state.active_workspace_id();
    get_pane_layout_for_inner(&state, &wid.to_string())
}

/// §4a workspace keep-alive: read any workspace's layout without
/// switching to it. Used by the frontend to prefetch every workspace's
/// tree on boot so all SplitContainers can mount in parallel and
/// workspace switches become CSS-only.
#[tauri::command]
pub fn get_pane_layout_for(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<LayoutNode, String> {
    get_pane_layout_for_inner(&state, &workspace_id)
}

fn get_pane_layout_for_inner(
    state: &State<'_, AppState>,
    workspace_id: &str,
) -> Result<LayoutNode, String> {
    let wid =
        Uuid::parse_str(workspace_id).map_err(|e| format!("workspace_id 不是合法 UUID: {e}"))?;
    let map = state.workspaces.read();
    let ws = map
        .get(&wid)
        .ok_or_else(|| format!("workspace {} 不存在", workspace_id))?;
    let mut c = 0u64;
    let mut agent_by_pane: HashMap<Uuid, String> = HashMap::new();
    for (agent_id, pane_id) in &ws.teammate_agent_pane_map {
        agent_by_pane.insert(*pane_id, agent_id.clone());
    }
    Ok(engine_node_to_layout(
        &ws.pane_tree.root,
        &mut c,
        &ws.teammate_pane_titles,
        &ws.pane_tree.panes,
        &ws.teammate_pane_states,
        &agent_by_pane,
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
    crate::commands::ridge_file::schedule_auto_save(&*state, wid);
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
    let pairs: Vec<(Vec<usize>, Vec<f32>)> =
        updates.into_iter().map(|u| (u.path, u.ratios)).collect();
    ws.pane_tree
        .set_split_ratios_batch(&pairs)
        .map_err(|e| e.to_string())?;
    drop(map);
    crate::commands::ridge_file::schedule_auto_save(&*state, wid);
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

    // 找到 source / target 各自所属的 workspace（pane id 全局唯一，扫一遍即可）。
    // 同 workspace → 走原有 PaneTree::dock_pane；跨 workspace → 走迁移路径。
    let (source_wid, target_wid) = {
        let map = state.workspaces.read();
        let mut s = None;
        let mut t = None;
        for (wid, ws) in map.iter() {
            if ws.pane_tree.panes.contains_key(&source) {
                s = Some(*wid);
            }
            if ws.pane_tree.panes.contains_key(&target) {
                t = Some(*wid);
            }
        }
        (
            s.ok_or_else(|| "source pane 不在任何工作区".to_string())?,
            t.ok_or_else(|| "target pane 不在任何工作区".to_string())?,
        )
    };

    if source_wid == target_wid {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&source_wid)
            .ok_or_else(|| "工作区已消失".to_string())?;
        ws.pane_tree
            .dock_pane(source, target, region)
            .map_err(|e| e.to_string())?;
        drop(map);
        crate::commands::ridge_file::schedule_auto_save(&*state, source_wid);
        return Ok(());
    }

    // 跨工作区路径：搬节点 + PTY，不重启 shell。
    let mut map = state.workspaces.write();

    // 1. 从 source workspace 摘下 leaf + 取走 pane 元数据 / PTY / 标题。
    let (pane_meta, pty_handle, pane_size, teammate_title, source_now_empty) = {
        let src_ws = map
            .get_mut(&source_wid)
            .ok_or_else(|| "source 工作区已消失".to_string())?;
        let leaves = src_ws.pane_tree.get_all_leaves();
        if !leaves.contains(&source) {
            return Err("source pane 不是叶子节点".into());
        }
        let was_only = leaves.len() == 1;
        if !src_ws.pane_tree.detach_external_leaf(source) {
            return Err("从 source 摘除节点失败".into());
        }
        let meta = src_ws.pane_tree.panes.remove(&source);
        let pty = src_ws.terminals.remove(&source);
        let size = src_ws.pane_sizes.remove(&source);
        let title = src_ws.teammate_pane_titles.remove(&source);
        (meta, pty, size, title, was_only)
    };

    // 2. 注入 target workspace：先放元数据 / PTY，再把 leaf 拼到 target 节点边上。
    {
        let tgt_ws = map
            .get_mut(&target_wid)
            .ok_or_else(|| "target 工作区已消失".to_string())?;
        if let Some(meta) = pane_meta {
            tgt_ws.pane_tree.panes.insert(source, meta);
        }
        if let Some(pty) = pty_handle {
            tgt_ws.terminals.insert(source, pty);
        }
        if let Some(size) = pane_size {
            tgt_ws.pane_sizes.insert(source, size);
        }
        if let Some(title) = teammate_title {
            tgt_ws.teammate_pane_titles.insert(source, title);
        }
        tgt_ws
            .pane_tree
            .attach_external_leaf(source, target, region)
            .map_err(|e| e.to_string())?;
    }

    // 3. source workspace 若被掏空（仅一个 leaf 时），整体关闭，避免留下空 tab。
    if source_now_empty {
        map.remove(&source_wid);
        drop(map);
        let mut order = state.workspace_order.write();
        order.retain(|id| id != &source_wid);
        drop(order);
        let mut names = state.workspace_names.write();
        names.remove(&source_wid);
        drop(names);
        // active_workspace 指向被关闭的 ws → 切到 target，保证前端切到迁入处。
        let mut active = state.active_workspace.write();
        if *active == source_wid {
            *active = target_wid;
        }
    } else {
        drop(map);
        crate::commands::ridge_file::schedule_auto_save(&*state, source_wid);
    }

    crate::commands::ridge_file::schedule_auto_save(&*state, target_wid);
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
    crate::commands::ridge_file::schedule_auto_save(&*state, wid);
    // Broadcast pane tree change to remote clients and desktop frontend.
    let _ = state
        .remote_structural_tx
        .send(crate::types::RemoteStructuralEvent::PanesChanged { workspace_id: wid });
    let _ = state
        .event_tx
        .try_send(crate::types::GlobalEvent::PaneTreeChanged { workspace_id: wid });
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

/// Frontend-accessible registration of a running teammate agent against a
/// pane. Mirrors the internal `register_agent_to_pane` in the HTTP server so
/// a UI "Run Claude Code agent here" button can mark a pane busy without
/// waiting for the HTTP round-trip. Idempotent.
///
/// Emits `teammate-layout-changed` so the SplitContainer re-renders with the
/// busy indicator.
#[tauri::command]
pub async fn register_teammate_agent(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
    agent_id: String,
) -> Result<(), String> {
    use tauri::Emitter;
    let pane_uuid = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    // 解耦 active_workspace_id（T5）：在面板**所属**工作区上注册，调用方显式传入。
    let wid = Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspace_id".to_string())?;
    register_teammate_agent_in(&state, wid, pane_uuid, agent_id)?;
    let _ = app.emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
    Ok(())
}

/// Core of `register_teammate_agent`, decoupled from Tauri `State`/`AppHandle`
/// (T5) so the workspace-targeting invariant — operate on the **passed** `wid`,
/// never `active_workspace_id` — is unit-testable.
pub(crate) fn register_teammate_agent_in(
    state: &AppState,
    wid: Uuid,
    pane_uuid: Uuid,
    agent_id: String,
) -> Result<(), String> {
    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| "工作区不存在".to_string())?;
    if !ws.pane_tree.panes.contains_key(&pane_uuid) {
        return Err(format!("pane {pane_uuid} not in workspace {wid}"));
    }
    ws.teammate_agent_pane_map.insert(agent_id, pane_uuid);
    ws.teammate_pane_states
        .insert(pane_uuid, crate::state::PaneState::Busy);
    Ok(())
}

/// Mark a teammate agent as no longer running against its pane. Removes the
/// agent → pane mapping and flips the pane state back to Idle.
#[tauri::command]
pub async fn release_teammate_agent(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    use tauri::Emitter;
    let pane_uuid = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    // 解耦 active_workspace_id（T5）：在面板**所属**工作区上释放，调用方显式传入。
    let wid = Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspace_id".to_string())?;
    release_teammate_agent_in(&state, wid, pane_uuid)?;
    let _ = app.emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
    Ok(())
}

/// Core of `release_teammate_agent`, decoupled from Tauri `State`/`AppHandle`
/// (T5): operate on the **passed** `wid`, never `active_workspace_id`.
pub(crate) fn release_teammate_agent_in(
    state: &AppState,
    wid: Uuid,
    pane_uuid: Uuid,
) -> Result<(), String> {
    let mut map = state.workspaces.write();
    let ws = map
        .get_mut(&wid)
        .ok_or_else(|| "工作区不存在".to_string())?;
    ws.teammate_pane_states
        .insert(pane_uuid, crate::state::PaneState::Idle);
    ws.teammate_agent_pane_map.retain(|_, v| *v != pane_uuid);
    Ok(())
}

/// Claude Code `tmux split-window`：`-h` → `horizontal`，`-v` → `vertical`（与 Ridge UI / `split_pane` 一致）。
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
        let ws = map
            .get_mut(&wid)
            .ok_or_else(|| "无活动工作区".to_string())?;
        // Drop every teammate map entry tied to this pane so rebuilt layouts
        // don't leak a dead `agent_state=busy` marker or a stale title.
        ws.teammate_pane_titles.remove(&pane_id);
        ws.teammate_pane_states.remove(&pane_id);
        ws.teammate_agent_pane_map.retain(|_, v| *v != pane_id);
        ws.pane_sizes.remove(&pane_id);
        // Drop any not-yet-activated PendingSpawn so a recycled pane_id
        // can't accidentally resurrect a dead PTY pair on next activate.
        ws.pending_spawns.remove(&pane_id);
        ws.pane_tree.close(pane_id).map_err(|e| e.to_string())?;
    }
    crate::commands::ridge_file::schedule_auto_save(&*state, wid);
    // Broadcast pane tree change to remote clients and desktop frontend.
    let _ = state
        .remote_structural_tx
        .send(crate::types::RemoteStructuralEvent::PanesChanged { workspace_id: wid });
    let _ = state
        .event_tx
        .try_send(crate::types::GlobalEvent::PaneTreeChanged { workspace_id: wid });
    Ok(())
}

/// §6 — balanced-split chooser for remote terminal creation. Picks the
/// largest-area leaf and splits it along its longer (pixel) axis so the two
/// resulting panes stay as close to square as possible:
///   wide pane → `Horizontal` (left/right);  tall pane → `Vertical` (top/bottom).
/// (Per SplitContainer: horizontal → 左右 / splits width, vertical → 上下.)
/// Cells are ~2× taller than wide, so rows are weighted accordingly.
pub(crate) fn choose_balanced_split(
    ws: &crate::state::Workspace,
) -> Option<(Uuid, SplitDirection)> {
    let sizes: Vec<(Uuid, u16, u16)> = ws
        .pane_tree
        .get_all_leaves()
        .iter()
        .map(|&id| {
            let (r, c) = ws.pane_sizes.get(&id).copied().unwrap_or((24, 80));
            (id, r, c)
        })
        .collect();
    balanced_split_decision(&sizes)
}

/// Pure core of `choose_balanced_split` (testable without a full `Workspace`):
/// given each leaf's (id, rows, cols), pick the largest-area leaf and split it
/// along its longer pixel axis (cells are ~2× taller than wide).
fn balanced_split_decision(sizes: &[(Uuid, u16, u16)]) -> Option<(Uuid, SplitDirection)> {
    let mut best = sizes.first()?.0;
    let mut best_area = 0u32;
    let mut best_rc = (24u16, 80u16);
    for &(id, rows, cols) in sizes {
        let area = rows as u32 * cols as u32;
        if area >= best_area {
            best_area = area;
            best = id;
            best_rc = (rows, cols);
        }
    }
    let (rows, cols) = best_rc;
    let width_px = cols as f32; // cell width ≈ 1 unit
    let height_px = rows as f32 * 2.0; // cell height ≈ 2 units
    let dir = if width_px >= height_px {
        SplitDirection::Horizontal // wide → left/right
    } else {
        SplitDirection::Vertical // tall → top/bottom
    };
    Some((best, dir))
}

/// §6 — create a terminal in a SPECIFIC workspace (used by the remote WS server,
/// which owns a per-client active workspace and must not depend on the global
/// `active_workspace_id()`). If the workspace has no live terminal yet, the
/// existing (PTY-less) root leaf is given a PTY; otherwise the largest leaf is
/// split via `choose_balanced_split` and the new leaf gets a PTY. Returns the
/// new pane id. Mirrors `split_pane_inner` + `create_pane_inner`.
pub(crate) fn remote_create_pane(
    state: &AppState,
    ws_id: Uuid,
    shell: Option<String>,
) -> Result<Uuid, AppError> {
    // Decide: attach to the existing leaf (first terminal) or split the largest.
    let (target_pane, split_dir) = {
        let map = state.workspaces.read();
        let ws = map
            .get(&ws_id)
            .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
        if ws.terminals.is_empty() {
            let leaf = *ws
                .pane_tree
                .get_all_leaves()
                .first()
                .ok_or_else(|| AppError::PtyError("workspace has no pane".into()))?;
            (leaf, None)
        } else {
            let (target, dir) = choose_balanced_split(ws)
                .ok_or_else(|| AppError::PtyError("workspace has no pane".into()))?;
            (target, Some(dir))
        }
    };

    // Inherit cwd from the target pane (tree first, then live OS cwd) when splitting.
    let parent_cwd: Option<String> = if split_dir.is_some() {
        {
            let map = state.workspaces.read();
            map.get(&ws_id)
                .and_then(|ws| ws.pane_tree.panes.get(&target_pane))
                .and_then(|p| p.cwd.as_ref().map(|c| c.to_string_lossy().into_owned()))
        }
        .or_else(|| crate::commands::process::current_pane_cwd_live(state, ws_id, target_pane))
    } else {
        None
    };

    let new_pane_id = if let Some(dir) = split_dir {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&ws_id)
            .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
        let id = ws.pane_tree.split(target_pane, dir)?;
        if let Some(ref cwd_str) = parent_cwd {
            if let Some(np) = ws.pane_tree.panes.get_mut(&id) {
                np.cwd = Some(std::path::PathBuf::from(cwd_str));
            }
        }
        id
    } else {
        target_pane
    };

    let cwd_path = parent_cwd.as_ref().map(std::path::PathBuf::from);
    terminal::ensure_pane_pty_workspace(
        state,
        ws_id,
        new_pane_id,
        shell,
        cwd_path.as_deref(),
        None,
        None,
        None,
        None,
        None,
    )?;
    crate::commands::ridge_file::schedule_auto_save(state, ws_id);
    Ok(new_pane_id)
}

/// §6 — close a terminal in a SPECIFIC workspace (remote counterpart of
/// `close_pane`, which is bound to the global active workspace). Keeps at least
/// one pane. Async because PTY teardown is async.
pub(crate) async fn remote_close_pane(
    state: &AppState,
    ws_id: Uuid,
    pane_id: Uuid,
) -> Result<(), AppError> {
    let leaves: Vec<Uuid> = {
        let map = state.workspaces.read();
        let ws = map
            .get(&ws_id)
            .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
        ws.pane_tree.get_all_leaves()
    };
    if leaves.len() <= 1 {
        return Err(AppError::PtyError("无法关闭最后一个窗格".into()));
    }
    if !leaves.contains(&pane_id) {
        return Err(AppError::PaneNotFound(pane_id));
    }
    terminal::kill_pty_if_present(state, ws_id, pane_id).await;
    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&ws_id)
            .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
        ws.teammate_pane_titles.remove(&pane_id);
        ws.teammate_pane_states.remove(&pane_id);
        ws.teammate_agent_pane_map.retain(|_, v| *v != pane_id);
        ws.pane_sizes.remove(&pane_id);
        ws.pending_spawns.remove(&pane_id);
        ws.pane_tree.close(pane_id)?;
    }
    crate::commands::ridge_file::schedule_auto_save(state, ws_id);
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

#[cfg(test)]
mod balanced_split_tests {
    use super::{balanced_split_decision, SplitDirection};
    use uuid::Uuid;

    #[test]
    fn empty_is_none() {
        assert!(balanced_split_decision(&[]).is_none());
    }

    #[test]
    fn single_wide_pane_splits_left_right() {
        let id = Uuid::new_v4();
        // 80 cols × 24 rows → width 80 vs height 48 → wide → Horizontal.
        let (chosen, dir) = balanced_split_decision(&[(id, 24, 80)]).unwrap();
        assert_eq!(chosen, id);
        assert!(matches!(dir, SplitDirection::Horizontal));
    }

    #[test]
    fn single_tall_pane_splits_top_bottom() {
        let id = Uuid::new_v4();
        // 20 cols × 50 rows → width 20 vs height 100 → tall → Vertical.
        let (chosen, dir) = balanced_split_decision(&[(id, 50, 20)]).unwrap();
        assert_eq!(chosen, id);
        assert!(matches!(dir, SplitDirection::Vertical));
    }

    #[test]
    fn picks_largest_area_leaf() {
        let small = Uuid::new_v4();
        let big = Uuid::new_v4();
        let (chosen, _) = balanced_split_decision(&[(small, 10, 40), (big, 40, 100)]).unwrap();
        assert_eq!(chosen, big, "must split the largest-area pane");
    }

    #[test]
    fn equal_area_tie_breaks_to_last_leaf_deterministically() {
        // M2: 等面积时 `area >= best_area` 让循环取**最后**（最大序号）叶子 ——
        // 确定性、跨 resize 稳定。route_split 与 summon 统一复用本判定后 tie-break
        // 一致；保留 `>=` 不改 remote 行为。
        let first = Uuid::new_v4();
        let last = Uuid::new_v4();
        let (chosen, _) = balanced_split_decision(&[(first, 24, 80), (last, 24, 80)]).unwrap();
        assert_eq!(
            chosen, last,
            "equal area → deterministic last (highest-index) leaf"
        );
    }
}

#[cfg(test)]
mod workspace_decoupling_tests {
    //! T5: register/release_teammate_agent operate on the EXPLICIT workspace id
    //! (decoupled from `active_workspace_id`). These cover the AC「非活动工作区面
    //! 板的状态操作落在正确 workspace」without Tauri State/AppHandle.
    use super::{register_teammate_agent_in, release_teammate_agent_in};
    use crate::state::{AppState, PaneState};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    fn test_state() -> AppState {
        let (tx, _rx) = mpsc::channel(8);
        AppState::new(tx)
    }

    /// First leaf of a workspace's pane tree (the seeded root pane).
    fn root_pane(state: &AppState, wid: Uuid) -> Uuid {
        state
            .workspaces
            .read()
            .get(&wid)
            .unwrap()
            .pane_tree
            .get_all_leaves()[0]
    }

    #[test]
    fn register_targets_the_passed_workspace() {
        let state = test_state();
        let wid = state.active_workspace_id();
        let pane = root_pane(&state, wid);
        register_teammate_agent_in(&state, wid, pane, "agent-1".into()).unwrap();
        let map = state.workspaces.read();
        let ws = map.get(&wid).unwrap();
        assert!(matches!(
            ws.teammate_pane_states.get(&pane),
            Some(PaneState::Busy)
        ));
        assert_eq!(ws.teammate_agent_pane_map.get("agent-1"), Some(&pane));
    }

    #[test]
    fn register_rejects_unknown_workspace_without_active_fallback() {
        let state = test_state();
        let unknown = Uuid::new_v4(); // not in the workspace map
        let pane = Uuid::new_v4();
        let err = register_teammate_agent_in(&state, unknown, pane, "agent-1".into())
            .expect_err("unknown workspace must be rejected");
        // Decoupling proof: keyed on the PASSED wid (unknown → workspace error),
        // NOT silently falling back to the (existing) active workspace.
        assert!(err.contains("工作区不存在"), "unexpected error: {err}");
    }

    #[test]
    fn release_targets_the_passed_workspace() {
        let state = test_state();
        let wid = state.active_workspace_id();
        let pane = root_pane(&state, wid);
        register_teammate_agent_in(&state, wid, pane, "agent-1".into()).unwrap();
        release_teammate_agent_in(&state, wid, pane).unwrap();
        let map = state.workspaces.read();
        let ws = map.get(&wid).unwrap();
        assert!(matches!(
            ws.teammate_pane_states.get(&pane),
            Some(PaneState::Idle)
        ));
        assert!(ws.teammate_agent_pane_map.is_empty());
    }

    #[test]
    fn release_rejects_unknown_workspace() {
        let state = test_state();
        let err = release_teammate_agent_in(&state, Uuid::new_v4(), Uuid::new_v4())
            .expect_err("unknown workspace must be rejected");
        assert!(err.contains("工作区不存在"), "unexpected error: {err}");
    }
}
