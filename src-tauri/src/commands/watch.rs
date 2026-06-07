use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, Debouncer};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

/// Whether a single watched-path event represents an SCM-relevant change
/// (branch, refs, working-tree status, in-progress operation) rather than
/// git-internal storage churn that the SCM panel should ignore.
///
/// Without this filter, every shell-prompt git hook (powerlevel10k,
/// starship, oh-my-posh) caused `scm-repo-changed` to fire on every
/// terminal Ctrl+C → prompt redraw — triggering full SCM refresh +
/// graph reload even though branch / index / refs hadn't moved.
/// TASKS §1.16.
///
/// Noise patterns (anywhere under the watched git dir):
///   - `objects/` — pack DB churn (every write goes here; not in porcelain)
///   - `logs/`    — reflog (every git op appends; not visible in status)
///   - `info/`    — sparse-checkout / exclude config (rarely changes;
///                  user can manually refresh if they edit it)
///   - `*.lock`   — transient locks created and deleted within one op
///                  (`index.lock`, `HEAD.lock`, `config.lock`, …)
///
/// Kept (relevant):
///   - `HEAD`, `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`
///   - `index`
///   - `refs/` (heads, remotes, tags, stash)
///   - `packed-refs`
///   - `FETCH_HEAD`, `ORIG_HEAD`
fn is_scm_relevant(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/objects/") {
        return false;
    }
    if s.contains("/logs/") {
        return false;
    }
    if s.contains("/info/") {
        return false;
    }
    if s.ends_with(".lock") {
        return false;
    }
    true
}

/// Manages notify debouncers for one or more git repo roots.
/// Held in `AppState` so it lives for the entire app lifetime.
pub struct GitWatcher {
    /// repo root → debouncer handle (keeps the watcher alive)
    debouncers: Mutex<HashMap<PathBuf, Debouncer<notify::RecommendedWatcher>>>,
}

impl GitWatcher {
    pub fn new() -> Self {
        Self {
            debouncers: Mutex::new(HashMap::new()),
        }
    }

    /// Start watching `repo_root/.git` recursively. Idempotent — calling
    /// with an already-watched root is a no-op.
    pub fn watch(&self, repo_root: PathBuf, app: AppHandle) -> notify::Result<()> {
        let mut map = self.debouncers.lock();
        if map.contains_key(&repo_root) {
            return Ok(());
        }

        let root_clone = repo_root.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(500),
            move |events: Result<Vec<DebouncedEvent>, notify::Error>| {
                let Ok(events) = events else {
                    return;
                };
                // Skip when every event in the debounce window is git-internal
                // noise (objects DB / reflog / info / transient locks). Keeps
                // shell-prompt git probes from spamming the SCM panel on every
                // Ctrl+C → prompt redraw. See `is_scm_relevant` for the list.
                if !events.iter().any(|e| is_scm_relevant(&e.path)) {
                    return;
                }
                let root_str = root_clone.to_string_lossy().to_string();
                // Generic "any repo changed" event — payload is the repo root path.
                let _ = app.emit("scm-repo-changed", root_str);
            },
        )?;

        // Resolve the actual git directory.
        // In a normal repo `.git` is a directory; in a linked worktree it is a
        // file containing "gitdir: /path/to/.git/worktrees/<name>".
        // We need to watch the resolved directory so index/HEAD/refs changes
        // inside the worktree's real git dir still trigger SCM refreshes.
        let git_dot = repo_root.join(".git");
        let git_dir_to_watch: Option<PathBuf> = if git_dot.is_dir() {
            Some(git_dot)
        } else if git_dot.is_file() {
            // Parse "gitdir: <path>" — path may be absolute or relative to repo_root.
            std::fs::read_to_string(&git_dot)
                .ok()
                .and_then(|content| {
                    content
                        .lines()
                        .find(|l| l.starts_with("gitdir:"))
                        .map(|l| l["gitdir:".len()..].trim().to_string())
                })
                .map(|rel| {
                    let p = PathBuf::from(&rel);
                    if p.is_absolute() {
                        p
                    } else {
                        repo_root.join(p)
                    }
                })
                .filter(|p| p.is_dir())
        } else {
            None
        };

        if let Some(dir) = git_dir_to_watch {
            debouncer.watcher().watch(&dir, RecursiveMode::Recursive)?;
        }

        map.insert(repo_root, debouncer);
        Ok(())
    }

    /// Stop watching all roots that are NOT in `live_roots`, then start
    /// watching any `live_roots` not yet being watched.
    pub fn sync_watched(&self, live_roots: &[PathBuf], app: &AppHandle) {
        {
            let live: HashSet<&PathBuf> = live_roots.iter().collect();
            let mut map = self.debouncers.lock();
            map.retain(|root, _| live.contains(root));
        }
        for root in live_roots {
            let _ = self.watch(root.clone(), app.clone());
        }
    }
}

// GitWatcher holds a Mutex<HashMap<...>> which is Send + Sync.
// notify's RecommendedWatcher is Send (not Sync), but we only access it
// through the Mutex which serialises all access.
// SAFETY: All access to the inner HashMap is protected by the Mutex.
unsafe impl Send for GitWatcher {}
unsafe impl Sync for GitWatcher {}

// ─── Tauri command ────────────────────────────────────────────────────────────

/// Register a set of git repo roots to watch. Called from the frontend
/// after `discoverRepos` resolves a new set of roots.
///
/// Old roots that are no longer in `roots` are automatically unwatched.
#[tauri::command]
pub async fn start_watching_repos(
    roots: Vec<String>,
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    let paths: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    state.git_watcher.sync_watched(&paths, &app);
    Ok(())
}
