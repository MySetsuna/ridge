// ─── Auto-save scheduler ───────────────────────────────────────────────────

struct AutoSaveState {
    /// 最近一次请求保存该工作区的时间戳。
    pending: HashMap<Uuid, Instant>,
    worker_running: bool,
}

static AUTO_SAVE: Lazy<Arc<Mutex<AutoSaveState>>> = Lazy::new(|| {
    Arc::new(Mutex::new(AutoSaveState {
        pending: HashMap::new(),
        worker_running: false,
    }))
});

/// 命令层在 cwd / 布局 / git 变化后调用。仅当工作区已关联 .ridge 文件时实际落盘，
/// 否则函数退化为无副作用的记录调用。
pub fn schedule_auto_save(state: &AppState, workspace_id: Uuid) {
    // Cheap gate: only panicked paths bother with this entry if there's an
    // associated file.
    let has_file = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.associated_file_path.as_ref())
            .is_some()
    };
    if !has_file {
        return;
    }
    let state_clone = state.clone();
    let mut guard = match AUTO_SAVE.lock() {
        Ok(g) => g,
        Err(e) => {
            tracing::error!(target: "ridge::autosave", error = %e, "mutex poisoned");
            return;
        }
    };
    guard.pending.insert(workspace_id, Instant::now());
    if guard.worker_running {
        return;
    }
    guard.worker_running = true;
    drop(guard);

    std::thread::Builder::new()
        .name("ridge-autosave".into())
        .spawn(move || auto_save_worker(state_clone))
        .ok();
}

fn auto_save_worker(state: AppState) {
    loop {
        std::thread::sleep(Duration::from_millis(AUTO_SAVE_DEBOUNCE_MS));
        let Some(due) = take_due_auto_saves(Instant::now()) else {
            return;
        };
        write_due_snapshots(&state, due);
        if !auto_save_worker_active() {
            return;
        }
    }
}

fn take_due_auto_saves(now: Instant) -> Option<Vec<Uuid>> {
    let mut guard = AUTO_SAVE.lock().ok()?;
    let due: Vec<Uuid> = guard
        .pending
        .iter()
        .filter(|(_, timestamp)| {
            now.duration_since(**timestamp) >= Duration::from_millis(AUTO_SAVE_DEBOUNCE_MS)
        })
        .map(|(&id, _)| id)
        .collect();
    for id in &due {
        guard.pending.remove(id);
    }
    if guard.pending.is_empty() {
        guard.worker_running = false;
    }
    Some(due)
}

fn write_due_snapshots(state: &AppState, due: Vec<Uuid>) {
    for id in due {
        if let Err(error) = write_workspace_snapshot(state, id) {
            tracing::warn!(
                target: "ridge::autosave",
                workspace = %id,
                error = %error,
                "auto-save failed"
            );
        }
    }
}

fn auto_save_worker_active() -> bool {
    AUTO_SAVE
        .lock()
        .map(|guard| guard.worker_running)
        .unwrap_or(false)
}

fn write_workspace_snapshot(state: &AppState, workspace_id: Uuid) -> Result<(), String> {
    let (path, name) = {
        let map = state.workspaces.read();
        let ws = map
            .get(&workspace_id)
            .ok_or_else(|| "workspace missing".to_string())?;
        let Some(path) = ws.associated_file_path.clone() else {
            return Ok(());
        };
        let name = state
            .workspace_names
            .read()
            .get(&workspace_id)
            .cloned()
            .unwrap_or_else(|| "workspace".to_string());
        (path, name)
    };
    let _:serde_json::Value=super::runtime::workspace_file(serde_json::json!({"operation":"save","workspaceId":workspace_id,"name":name,"path":path}))?;
    Ok(())
}

