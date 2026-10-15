//! Runtime-owned, bounded OS watches; subscriptions follow authenticated event connections.
use crate::business::BusinessEvents;
use notify::{RecursiveMode, Watcher};
use ridge_core::{sandbox::RootScope, Ctx, EventScope, EventSink};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const ROOT_CAP: usize = 96;
const REQUEST_ROOT_CAP: usize = 32;
const PATH_CAP: usize = 256;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    roots: Vec<Spec>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    path: String,
    recursive: bool,
}
#[derive(Clone)]
struct Subscription {
    root: PathBuf,
    display: String,
    recursive: bool,
    git: bool,
    scopes: Vec<RootScope>,
}
#[derive(Default)]
struct Pending {
    paths: HashSet<PathBuf>,
    coalesced: bool,
    error: bool,
}
struct RootWatch {
    _watcher: notify::RecommendedWatcher,
    recursive: bool,
    filter: Arc<AtomicU8>,
    pending: Arc<Mutex<Pending>>,
}
#[derive(Default)]
struct Client {
    attachments: usize,
    subscriptions: Vec<Subscription>,
}
#[derive(Default)]
struct State {
    clients: HashMap<String, Client>,
    watches: HashMap<PathBuf, RootWatch>,
}
#[derive(Default)]
pub struct FsWatches {
    state: Mutex<State>,
    running: AtomicBool,
}
pub struct Attachment {
    watches: Arc<FsWatches>,
    connection: String,
}
impl Drop for Attachment {
    fn drop(&mut self) {
        let Ok(mut state) = self.watches.state.lock() else {
            return;
        };
        if let Some(client) = state.clients.get_mut(&self.connection) {
            client.attachments = client.attachments.saturating_sub(1);
            if client.attachments == 0 {
                state.clients.remove(&self.connection);
            }
        }
        let live = desired(&state.clients);
        state.watches.retain(|root, _| live.contains_key(root));
        for (root, (_, git, fs)) in live {
            if let Some(watch) = state.watches.get(&root) {
                watch
                    .filter
                    .store((fs as u8) | ((git as u8) << 1), Ordering::Release);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shared_roots_survive_one_detach_and_invalid_replacement_preserves_subscriptions() {
        let root = std::env::temp_dir().join(format!("ridge-watch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let watches = Arc::new(FsWatches::default());
        let events = BusinessEvents::default();
        let first = watches.attach("first".into(), events.clone());
        let second = watches.attach("second".into(), events);
        let scope = RootScope::from_roots([&root]);
        let ctx = |id| crate::host_core::runtime_ctx(&[root.clone()], true).with_connection(id);
        let request = json!({"roots":[{"path":root,"recursive":true}]});
        watches.sync(&request, &ctx("first"), &scope).unwrap();
        watches.sync(&request, &ctx("second"), &scope).unwrap();
        assert_eq!(watches.state.lock().unwrap().watches.len(), 1);
        assert!(watches
            .sync(
                &json!({"roots":[{"path":root.join("missing"),"recursive":true}]}),
                &ctx("first"),
                &scope
            )
            .is_err());
        assert_eq!(
            watches.state.lock().unwrap().clients["first"]
                .subscriptions
                .len(),
            1
        );
        drop(first);
        assert_eq!(watches.state.lock().unwrap().watches.len(), 1);
        drop(second);
        assert!(watches.state.lock().unwrap().watches.is_empty());
    }
    #[test]
    fn generated_noise_and_windows_display_paths_keep_existing_contract() {
        assert!(ignored(Path::new("repo/node_modules/file.js")));
        assert!(ignored(Path::new("repo/file.tmp")));
        assert!(!ignored(Path::new("repo/src/file.rs")));
        assert_eq!(
            display(Path::new(r"\\?\C:\repo\file.rs")),
            "C:/repo/file.rs"
        );
    }
    #[tokio::test]
    async fn linked_worktree_metadata_is_authorized_and_kept_separate_from_file_roots() {
        let root = std::env::temp_dir().join(format!("ridge-git-watch-{}", uuid::Uuid::new_v4()));
        let repo = root.join("checkout");
        let metadata = root.join("main/.git/worktrees/checkout");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&metadata).unwrap();
        std::fs::write(repo.join(".git"), format!("gitdir: {}", metadata.display())).unwrap();
        std::fs::write(metadata.join("commondir"), "../..").unwrap();
        let directories = git_directories(&repo).unwrap();
        assert_eq!(directories.len(), 2);
        let watches = Arc::new(FsWatches::default());
        let _attachment = watches.attach("client".into(), BusinessEvents::default());
        let restricted =
            crate::host_core::runtime_ctx(&[repo.clone()], false).with_connection("client");
        assert!(watches
            .sync_git(
                &json!({"roots":[repo]}),
                &restricted,
                &RootScope::from_roots([&root])
            )
            .is_err());
        let broad = crate::host_core::runtime_ctx(&[root.clone()], false).with_connection("client");
        watches
            .sync(
                &json!({"roots":[{"path":repo,"recursive":true}]}),
                &broad,
                &RootScope::from_roots([&root]),
            )
            .unwrap();
        watches
            .sync_git(
                &json!({"roots":[repo]}),
                &broad,
                &RootScope::from_roots([&root]),
            )
            .unwrap();
        assert_eq!(
            watches.state.lock().unwrap().clients["client"]
                .subscriptions
                .len(),
            3
        );
        watches
            .sync_git(
                &json!({"roots":[]}),
                &broad,
                &RootScope::from_roots([&root]),
            )
            .unwrap();
        assert_eq!(
            watches.state.lock().unwrap().clients["client"]
                .subscriptions
                .len(),
            1
        );
        assert!(!scm_relevant(Path::new("repo/.git/objects/item")));
        assert!(!scm_relevant(Path::new(
            "repo/.git/fsmonitor--daemon/cookies/1"
        )));
        assert!(scm_relevant(Path::new("repo/.git/index")));
    }
}
fn display(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if let Some(rest) = value.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_string()
    }
}
fn ignored(path: &Path) -> bool {
    const SEGMENTS: &[&str] = &[
        "node_modules",
        ".git",
        "target",
        "dist",
        "build",
        ".next",
        ".svelte-kit",
        ".nuxt",
        "__pycache__",
        ".venv",
        ".cache",
    ];
    path.components().any(|part| {
        part.as_os_str()
            .to_str()
            .is_some_and(|name| SEGMENTS.contains(&name))
    }) || path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name == ".DS_Store"
                || name == "Thumbs.db"
                || name.starts_with(".#")
                || (name.starts_with('#') && name.ends_with('#'))
                || name.ends_with(".tmp")
                || name.ends_with(".swp")
                || name.ends_with('~')
        })
}
fn desired(clients: &HashMap<String, Client>) -> HashMap<PathBuf, (bool, bool, bool)> {
    let mut roots = HashMap::new();
    for subscription in clients.values().flat_map(|client| &client.subscriptions) {
        let mode = roots
            .entry(subscription.root.clone())
            .or_insert((false, false, false));
        mode.0 |= subscription.recursive;
        mode.1 |= subscription.git;
        mode.2 |= !subscription.git;
    }
    roots
}
fn canonical_scope(scope: &RootScope) -> RootScope {
    RootScope::from_roots(scope.roots().iter().map(|root| {
        PathBuf::from(display(
            &std::fs::canonicalize(root).unwrap_or_else(|_| root.clone()),
        ))
    }))
}
fn scm_relevant(path: &Path) -> bool {
    let path = display(path);
    !["/objects/", "/logs/", "/info/", "fsmonitor--daemon"]
        .iter()
        .any(|part| path.contains(part))
        && !path.ends_with(".lock")
}
fn bounded_text(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut value = String::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(4097)
        .read_to_string(&mut value)
        .map_err(|e| e.to_string())?;
    if value.len() > 4096 {
        return Err("Git metadata pointer too large".into());
    }
    Ok(value)
}
fn git_directories(repo: &Path) -> Result<Vec<PathBuf>, String> {
    let dot = repo.join(".git");
    let dir = if dot.is_dir() {
        dot
    } else {
        let pointer = bounded_text(&dot)?;
        let target = pointer
            .lines()
            .find_map(|line| line.strip_prefix("gitdir:"))
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .ok_or("invalid Git directory pointer")?;
        repo.join(target)
    };
    let dir = std::fs::canonicalize(dir)
        .map_err(|e| format!("Git metadata directory unavailable: {e}"))?;
    if !dir.is_dir() {
        return Err("Git metadata path is not a directory".into());
    }
    let mut directories = vec![dir.clone()];
    let common = dir.join("commondir");
    if common.exists() {
        let target = bounded_text(&common)?;
        let common = std::fs::canonicalize(dir.join(target.trim())).map_err(|e| e.to_string())?;
        if !common.is_dir() {
            return Err("Git common metadata path is not a directory".into());
        }
        if common != dir {
            directories.push(common);
        }
    }
    Ok(directories)
}
fn event_path(path: &Path) -> PathBuf {
    let mut current = path;
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(current) {
            for part in suffix.into_iter().rev() {
                resolved.push(part);
            }
            return PathBuf::from(display(&resolved));
        }
        let Some(name) = current.file_name() else {
            return PathBuf::from(display(path));
        };
        suffix.push(name.to_os_string());
        let Some(parent) = current.parent() else {
            return PathBuf::from(display(path));
        };
        current = parent;
    }
}
impl FsWatches {
    pub fn attach(self: &Arc<Self>, connection: String, events: BusinessEvents) -> Attachment {
        self.state
            .lock()
            .expect("file watch state")
            .clients
            .entry(connection.clone())
            .or_default()
            .attachments += 1;
        if !self.running.swap(true, Ordering::AcqRel) {
            let weak = Arc::downgrade(self);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let Some(watches) = weak.upgrade() else {
                        break;
                    };
                    let events = events.clone();
                    if tokio::task::spawn_blocking(move || watches.flush(&events))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        Attachment {
            watches: self.clone(),
            connection,
        }
    }
    pub fn sync(&self, args: &Value, ctx: &Ctx, operator: &RootScope) -> Result<Value, String> {
        self.sync_kind(args, ctx, operator, false)
    }
    pub fn sync_git(&self, args: &Value, ctx: &Ctx, operator: &RootScope) -> Result<Value, String> {
        let roots = args["roots"]
            .as_array()
            .ok_or("Git watches require roots array")?;
        let mut specs = Vec::new();
        for root in roots {
            specs.push(
                json!({"path":root.as_str().ok_or("invalid repository root")?,"recursive":true}),
            );
        }
        self.sync_kind(&json!({"roots":specs}), ctx, operator, true)
    }
    fn sync_kind(
        &self,
        args: &Value,
        ctx: &Ctx,
        operator: &RootScope,
        git: bool,
    ) -> Result<Value, String> {
        let request: Request = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
        if request.roots.len() > REQUEST_ROOT_CAP {
            return Err("file watch root budget exceeded (32)".into());
        }
        let connection = ctx
            .connection()
            .as_ref()
            .ok_or("file watch requires an event connection")?;
        let scopes = vec![operator.clone(), ctx.capabilities().root_scope().clone()];
        let mut subscriptions = Vec::<Subscription>::new();
        for spec in request.roots {
            if ridge_core::dispatch::path_has_traversal(&spec.path)
                || !Path::new(&spec.path).is_absolute()
            {
                return Err("file watch requires an absolute path without traversal".into());
            }
            if !scopes.iter().all(|scope| scope.is_allowed(&spec.path)) {
                return Err("file watch outside authorized roots".into());
            }
            let root = std::fs::canonicalize(&spec.path)
                .map_err(|e| format!("cannot watch requested path: {e}"))?;
            let targets = if git {
                git_directories(&root)?
            } else {
                vec![root]
            };
            for root in targets {
                if !scopes
                    .iter()
                    .map(canonical_scope)
                    .all(|scope| scope.is_allowed(&display(&root)))
                {
                    return Err("Git metadata outside authorized roots".into());
                }
                if let Some(previous) = subscriptions.iter_mut().find(|previous| {
                    previous.root == root && (!git || previous.display == spec.path)
                }) {
                    previous.recursive |= spec.recursive;
                } else {
                    subscriptions.push(Subscription {
                        root,
                        display: if git {
                            spec.path.clone()
                        } else {
                            spec.path.replace('\\', "/")
                        },
                        recursive: spec.recursive,
                        git,
                        scopes: scopes.iter().map(canonical_scope).collect(),
                    });
                }
            }
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "file watch state unavailable")?;
        let client = state
            .clients
            .get(connection)
            .ok_or("file watch requires an attached events connection")?;
        let mut next_clients = state
            .clients
            .iter()
            .map(|(key, client)| {
                (
                    key.clone(),
                    Client {
                        attachments: client.attachments,
                        subscriptions: client.subscriptions.clone(),
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        subscriptions.extend(
            client
                .subscriptions
                .iter()
                .filter(|subscription| subscription.git != git)
                .cloned(),
        );
        next_clients.insert(
            connection.clone(),
            Client {
                attachments: client.attachments,
                subscriptions,
            },
        );
        let roots = desired(&next_clients);
        if roots.len() > ROOT_CAP {
            return Err("Runtime watch resource budget exceeded (96)".into());
        }
        let mut additions = HashMap::new();
        for (root, (recursive, git, fs)) in &roots {
            if state
                .watches
                .get(root)
                .is_some_and(|watch| watch.recursive == *recursive)
            {
                continue;
            }
            let pending = state
                .watches
                .get(root)
                .map(|watch| watch.pending.clone())
                .unwrap_or_default();
            let queue = pending.clone();
            let filter = Arc::new(AtomicU8::new((*fs as u8) | ((*git as u8) << 1)));
            let callback_filter = filter.clone();
            let mut watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if let Ok(mut queue) = queue.lock() {
                        match event {
                            Ok(event) => {
                                for path in event.paths {
                                    let flags = callback_filter.load(Ordering::Acquire);
                                    if !((flags & 1 != 0 && !ignored(&path))
                                        || (flags & 2 != 0 && scm_relevant(&path)))
                                    {
                                        continue;
                                    }
                                    if queue.paths.len() < PATH_CAP {
                                        queue.paths.insert(path);
                                    } else {
                                        queue.coalesced = true;
                                    }
                                }
                            }
                            Err(error) => {
                                queue.error = true;
                                tracing::warn!(%error,"Runtime filesystem watch failed");
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            watcher
                .watch(
                    root,
                    if *recursive {
                        RecursiveMode::Recursive
                    } else {
                        RecursiveMode::NonRecursive
                    },
                )
                .map_err(|e| e.to_string())?;
            additions.insert(
                root.clone(),
                RootWatch {
                    _watcher: watcher,
                    recursive: *recursive,
                    filter,
                    pending,
                },
            );
        }
        // Only publish after all new OS watches exist; a failure retains every old subscription.
        state.clients = next_clients;
        state.watches.retain(|root, _| roots.contains_key(root));
        state.watches.extend(additions);
        for (root, (_, git, fs)) in roots {
            if let Some(watch) = state.watches.get(&root) {
                watch
                    .filter
                    .store((fs as u8) | ((git as u8) << 1), Ordering::Release);
            }
        }
        Ok(Value::Null)
    }
    fn flush(&self, events: &BusinessEvents) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        for (root, watch) in &state.watches {
            let pending = watch
                .pending
                .lock()
                .map(|mut pending| std::mem::take(&mut *pending))
                .unwrap_or_default();
            if pending.paths.is_empty() && !pending.coalesced && !pending.error {
                continue;
            }
            for (connection, client) in &state.clients {
                for subscription in client
                    .subscriptions
                    .iter()
                    .filter(|subscription| subscription.root == *root)
                {
                    let mut paths = pending
                        .paths
                        .iter()
                        .filter_map(|path| {
                            if !subscription.git && ignored(path) {
                                return None;
                            }
                            let normalized = event_path(path);
                            let normalized_root = PathBuf::from(display(root));
                            let inside = normalized == normalized_root
                                || (subscription.recursive
                                    && normalized.starts_with(&normalized_root))
                                || (!subscription.recursive
                                    && normalized.parent() == Some(normalized_root.as_path()));
                            (inside
                                && subscription
                                    .scopes
                                    .iter()
                                    .all(|scope| scope.is_allowed(&normalized.to_string_lossy())))
                            .then(|| {
                                display(
                                    &PathBuf::from(&subscription.display).join(
                                        normalized
                                            .strip_prefix(&normalized_root)
                                            .unwrap_or(Path::new("")),
                                    ),
                                )
                            })
                        })
                        .collect::<Vec<_>>();
                    paths.sort();
                    if !paths.is_empty() || pending.coalesced {
                        if subscription.git {
                            events.emit(
                                EventScope::Connection,
                                &Some(connection.clone()),
                                "scm-repo-changed",
                                json!(subscription.display),
                            );
                        } else {
                            events.emit(EventScope::Connection,&Some(connection.clone()),"fs-changed",json!({"root":subscription.display,"paths":if pending.coalesced {Vec::new()}else{paths},"coalesced":pending.coalesced}));
                        }
                    }
                    if pending.error {
                        events.emit(EventScope::Connection,&Some(connection.clone()),"fs-watch-error",json!({"root":subscription.display,"error":"Filesystem watch failed; refresh and reconnect this root"}));
                    }
                }
            }
        }
    }
}
