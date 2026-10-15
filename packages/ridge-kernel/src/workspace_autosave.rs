//! Runtime-only bounded debounce; presentation exit never owns the writer lifetime.
use crate::{
    server::AppState,
    workspace_files::{handle, Request},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use uuid::Uuid;
#[derive(Default)]
struct Pending {
    requests: HashMap<Uuid, (Instant, Request)>,
    errors: HashMap<Uuid, String>,
    running: bool,
    in_flight: HashSet<Uuid>,
    succeeded: HashSet<Uuid>,
}
pub(crate) fn committed(id: Uuid) {
    if let Ok(mut pending) = PENDING.get_or_init(Default::default).lock() {
        pending.errors.remove(&id);
        if pending.in_flight.contains(&id) {
            pending.succeeded.insert(id);
        }
    }
}
static PENDING: OnceLock<Mutex<Pending>> = OnceLock::new();
const DEBOUNCE: Duration = Duration::from_millis(400);
pub(crate) fn error(id: Uuid) -> Option<String> {
    PENDING
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .errors
        .get(&id)
        .cloned()
}
pub(crate) fn schedule(state: AppState, mut request: Request) -> Result<(), String> {
    let id = request.workspace_id.ok_or("missing workspaceId")?;
    {
        let graph = state
            .workspaces
            .lock()
            .map_err(|_| "workspace state unavailable")?;
        let metadata = graph.workspace(id).ok_or("workspace not found")?;
        if let Some(file) = &metadata.file {
            request.path = Some(file.path.clone());
            request.name = metadata.name.clone();
        }
    }
    request.operation = "auto-save".into();
    let mut pending = PENDING
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "autosave state unavailable")?;
    let tracked = pending
        .requests
        .keys()
        .chain(pending.errors.keys())
        .chain(pending.in_flight.iter())
        .copied()
        .collect::<HashSet<_>>();
    if !tracked.contains(&id) && tracked.len() >= 256 {
        return Err("autosave request budget exceeded".into());
    }
    pending.requests.insert(id, (Instant::now(), request));
    if pending.running {
        return Ok(());
    }
    pending.running = true;
    std::thread::Builder::new()
        .name("ridge-runtime-autosave".into())
        .spawn(move || loop {
            std::thread::sleep(DEBOUNCE);
            let due = {
                let Ok(mut pending) = PENDING.get_or_init(Default::default).lock() else {
                    return;
                };
                let ids = pending
                    .requests
                    .iter()
                    .filter(|(_, (since, _))| since.elapsed() >= DEBOUNCE)
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>();
                for id in &ids {
                    pending.in_flight.insert(*id);
                }
                ids.into_iter()
                    .filter_map(|id| {
                        pending
                            .requests
                            .remove(&id)
                            .map(|(_, request)| (id, request))
                    })
                    .collect::<Vec<_>>()
            };
            for (id, request) in due {
                let result = handle(&state, request);
                let Ok(mut pending) = PENDING.get_or_init(Default::default).lock() else {
                    return;
                };
                pending.in_flight.remove(&id);
                let newer_commit = pending.succeeded.remove(&id);
                match result {
                    Ok(_) => {
                        pending.errors.remove(&id);
                    }
                    Err(error) => {
                        tracing::warn!(%id,%error,"Runtime autosave failed");
                        if !newer_commit
                            && (pending.errors.len() < 256 || pending.errors.contains_key(&id))
                        {
                            pending.errors.insert(id, error);
                        }
                    }
                }
            }
            let Ok(mut pending) = PENDING.get_or_init(Default::default).lock() else {
                return;
            };
            if pending.requests.is_empty() {
                pending.running = false;
                return;
            }
        })
        .map_err(|error| {
            pending.running = false;
            format!("start autosave worker: {error}")
        })?;
    Ok(())
}
