use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use tauri::State;
use uuid::Uuid;

use crate::commands::terminal::teardown_pane_pty_if_present;
use crate::engine::kernel_pty::{make_master, make_writer, KernelPtyRef};
use crate::engine::parser::PaneParser;
use crate::engine::pty::{spawn_pty_reader, PtyHandle};
use crate::state::AppState;
use super::terminal::StructuredPtyCommand;
use super::terminal_shim::kernel_structured_env;
use crate::utils::error::AppError;
use crate::utils::pty_log;

/// 若带 `initial_command` 时该 pane 已有 PTY（常见：前端 `Pane` onMount 先 `create_pane`），会先拆掉再按命令重起，避免误走 `create_skip`。
pub(crate) fn install_kernel_pty(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    reference: KernelPtyRef,
    cols: u16,
    rows: u16,
) -> Result<bool, String> {
    let master = make_master(reference.clone(), cols, rows);
    let reader = master
        .lock()
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = make_writer(reference.clone());
    let input_sink = crate::engine::pty::PtyInputSink::new(writer.clone());
    let parser = Arc::new(Mutex::new(PaneParser::new(rows.max(1), cols.max(1), 2000)));
    let handle = PtyHandle {
        master,
        writer,
        input_sink,
        _child: None,
        native_ref: None,
        native_cancel: None,
        remote_ref: None,
        kernel_ref: Some(reference.clone()),
        job: None,
        child_pid: None,
        resize_silence_deadline: Arc::new(AtomicI64::new(0)),
        parser,
        delta_mode: Arc::new(AtomicBool::new(false)),
        workspace: Arc::new(Mutex::new(workspace_id)),
    };
    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&workspace_id)
            .ok_or_else(|| "workspace not found while attaching kernel PTY".to_string())?;
        if ws.terminals.contains_key(&pane_id) {
            return Ok(false);
        }
        ws.terminals.insert(pane_id, handle);
    }
    spawn_pty_reader(state.clone(), workspace_id, pane_id, reader);
    Ok(true)
}

/// Resolve the singleton kernel for every desktop pane PTY. Production never
/// falls back to a Tauri-owned child: a missing or unhealthy kernel is an
/// explicit launch error. Unit tests retain the local seam below so they do
/// not spawn a detached kernel as a side effect of `cargo test`.
pub(crate) fn kernel_endpoint_for_shell() -> Result<ridge_kernel::registry::KernelEndpoint, String> {
    if let Some(endpoint) = ridge_kernel::client::running_endpoint() {
        return Ok(endpoint);
    }
    #[cfg(test)]
    {
        return Err("ridge-kernel disabled in unit tests".to_string());
    }
    #[cfg(not(test))]
    crate::kernel_lifecycle::ensure_kernel_running()
}

pub(crate) fn attach_or_spawn_kernel_pty(
    state: &AppState,
    endpoint: ridge_kernel::registry::KernelEndpoint,
    workspace_id: Uuid,
    pane_id: Uuid,
    shell: Option<&str>,
    cwd: Option<&Path>,
    initial_size: Option<(u16, u16)>,
) -> Result<bool, String> {
    attach_or_spawn_kernel_command(
        state,
        endpoint,
        KernelCommandLaunch {
            workspace_id,
            pane_id,
            program: shell,
            args: &[],
            env: &HashMap::new(),
            cwd,
            role: "shell",
            launch_profile: Some("ridge-interactive"),
            initial_size,
        },
    )
}

pub(crate) struct KernelCommandLaunch<'a> {
    workspace_id: Uuid,
    pane_id: Uuid,
    program: Option<&'a str>,
    args: &'a [String],
    env: &'a HashMap<String, String>,
    cwd: Option<&'a Path>,
    role: &'a str,
    launch_profile: Option<&'a str>,
    initial_size: Option<(u16, u16)>,
}

pub(crate) fn attach_or_spawn_kernel_command(
    state: &AppState,
    endpoint: ridge_kernel::registry::KernelEndpoint,
    launch: KernelCommandLaunch<'_>,
) -> Result<bool, String> {
    let info = ridge_kernel::client::list_domain_ptys(&endpoint)?
        .into_iter()
        .find(|entry| entry.pty_id == launch.pane_id || entry.id == launch.pane_id);
    let cwd_string = launch.cwd.map(|path| path.to_string_lossy().into_owned());
    let (pty_id, after_seq, cols, rows) = if let Some(info) = info {
        (
            info.pty_id,
            Some(info.next_seq.saturating_sub(1)),
            info.cols,
            info.rows,
        )
    } else {
        let (cols, rows) = launch.initial_size.unwrap_or((80, 24));
        let pty_id = ridge_kernel::client::create_domain_pty_with_command(
            &endpoint,
            ridge_kernel::client::DomainPtyLaunch {
                pty_id: launch.pane_id,
                program: launch.program,
                args: launch.args,
                cwd: cwd_string.as_deref(),
                workspace_id: Some(launch.workspace_id),
                role: launch.role,
                launch_profile: launch.launch_profile,
                env: Some(launch.env),
                cols: Some(cols),
                rows: Some(rows),
            },
        )?;
        (pty_id, None, cols, rows)
    };
    let reference = KernelPtyRef {
        endpoint,
        id: pty_id,
        after_seq,
    };
    let installed = install_kernel_pty(
        state,
        launch.workspace_id,
        launch.pane_id,
        reference.clone(),
        cols,
        rows,
    )?;
    crate::commands::workspace::sync_kernel_workspace_topology(state, launch.workspace_id);
    if state.active_workspace_id() == launch.workspace_id {
        crate::commands::workspace::sync_kernel_active_workspace(launch.workspace_id);
    }
    if !installed && after_seq.is_none() {
        let _ = reference.destroy();
    }
    Ok(true)
}

/// Rebind kernel-owned PTYs to the restored pane tree. Pane UUIDs are the
/// stable key, so restoring a `.ridge` workspace into a new workspace UUID
/// still reconnects the original terminal instead of spawning a replacement.
#[tauri::command]
pub async fn reattach_kernel_ptys(state: State<'_, AppState>) -> Result<usize, String> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || reattach_kernel_ptys_inner(&st))
        .await
        .map_err(|error| error.to_string())?
}

pub(crate) fn reattach_kernel_ptys_inner(state: &AppState) -> Result<usize, String> {
    let Some(endpoint) = ridge_kernel::client::running_endpoint() else {
        return Ok(0);
    };
    let infos = ridge_kernel::client::list_domain_ptys(&endpoint)?;
    let mut attached = 0usize;
    let mut orphaned = 0usize;
    for info in infos {
        let pane_id = info.pty_id;
        let target = {
            let map = state.workspaces.read();
            map.iter().find_map(|(workspace_id, workspace)| {
                (workspace.pane_tree.get_all_leaves().contains(&pane_id)
                    && !workspace.terminals.contains_key(&pane_id))
                .then_some((*workspace_id, pane_id))
            })
        };
        let Some((workspace_id, pane_id)) = target else {
            orphaned += 1;
            continue;
        };
        let reference = KernelPtyRef {
            endpoint: endpoint.clone(),
            id: info.pty_id,
            // A desktop restart has no parser state. Replay the kernel's
            // bounded retained window so the restored pane reconstructs its
            // visible history before consuming future output.
            after_seq: None,
        };
        if install_kernel_pty(
            state,
            workspace_id,
            pane_id,
            reference,
            info.cols,
            info.rows,
        )? {
            attached += 1;
        }
    }
    if orphaned > 0 {
        tracing::warn!(
            target: "ridge::kernel_pty",
            orphaned,
            "kernel PTYs have no restored desktop pane; leaving them alive for explicit recovery"
        );
    }
    Ok(attached)
}

pub(crate) fn prepare_pane_for_pty(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    has_explicit_launch: bool,
) -> Result<bool, AppError> {
    let map = state.workspaces.read();
    let ws = map
        .get(&workspace_id)
        .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
    if !ws.pane_tree.get_all_leaves().contains(&pane_id) {
        pty_log::create_skip(workspace_id, pane_id);
        return Ok(true);
    }
    if ws.terminals.contains_key(&pane_id) {
        if has_explicit_launch {
            drop(map);
            teardown_pane_pty_if_present(state, workspace_id, pane_id);
        } else {
            pty_log::create_skip(workspace_id, pane_id);
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn mark_kernel_ready(ready_tx: &mut Option<tokio::sync::oneshot::Sender<Result<(), String>>>) {
    if let Some(tx) = ready_tx.take() {
        let _ = tx.send(Ok(()));
    }
}

pub struct KernelPtyInstall<'a> {
    pub state: &'a AppState,
    pub workspace_id: Uuid,
    pub pane_id: Uuid,
    pub shell: Option<&'a str>,
    pub cwd: Option<&'a Path>,
    pub structured_command: Option<&'a StructuredPtyCommand>,
    pub tmux_pane_index: Option<usize>,
    pub kernel_candidate: bool,
    pub has_explicit_launch: bool,
    pub initial_size: Option<(u16, u16)>,
    pub ready_tx: &'a mut Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
}

pub(crate) fn try_install_kernel_pty(mut request: KernelPtyInstall<'_>) -> Option<Result<(), AppError>> {
    let structured_command = request.structured_command;
    if let Some(spec) = structured_command {
        return Some(install_structured_kernel_pty(&mut request, spec));
    }
    if request.kernel_candidate && !request.has_explicit_launch {
        return Some(install_shell_kernel_pty(&mut request));
    }
    None
}

pub(crate) fn install_structured_kernel_pty(
    request: &mut KernelPtyInstall<'_>,
    spec: &StructuredPtyCommand,
) -> Result<(), AppError> {
    crate::teammate::ensure_teammate_started(request.state);
    let endpoint = kernel_endpoint_for_shell().map_err(|error| {
        AppError::PtyError(format!("ridge-kernel unavailable for Agent PTY: {error}"))
    })?;
    let env = kernel_structured_env(
        request.state,
        request.workspace_id,
        request.pane_id,
        request.cwd,
        request.tmux_pane_index,
        spec,
    )
    .map_err(AppError::PtyError)?;
    attach_or_spawn_kernel_command(
        request.state,
        endpoint,
        KernelCommandLaunch {
            workspace_id: request.workspace_id,
            pane_id: request.pane_id,
            program: Some(&spec.program),
            args: &spec.args,
            env: &env,
            cwd: request.cwd,
            role: "agent",
            launch_profile: None,
            initial_size: None,
        },
    )
    .map_err(|error| AppError::PtyError(format!("ridge-kernel Agent PTY unavailable: {error}")))?;
    mark_kernel_ready(request.ready_tx);
    Ok(())
}

pub(crate) fn install_shell_kernel_pty(request: &mut KernelPtyInstall<'_>) -> Result<(), AppError> {
    let endpoint = kernel_endpoint_for_shell().map_err(|error| {
        AppError::PtyError(format!("ridge-kernel unavailable for shell PTY: {error}"))
    })?;
    attach_or_spawn_kernel_pty(
        request.state,
        endpoint,
        request.workspace_id,
        request.pane_id,
        request.shell,
        request.cwd,
        request.initial_size,
    )
    .map_err(|error| AppError::PtyError(format!("ridge-kernel shell PTY unavailable: {error}")))?;
    mark_kernel_ready(request.ready_tx);
    Ok(())
}

