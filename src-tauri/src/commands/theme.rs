use std::path::{Path, PathBuf};

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

// S1: the theme catalog types and the handle-free resolution / persistence
// logic now live in `ridge_core::commands::theme` (the runtime-agnostic core).
// src-tauri re-exports the types so existing references (`ThemeFile`,
// `ThemeEntry`, `LoaderConfig`) keep compiling against a single source of
// truth, and the desktop-specific helpers (which use Tauri's `Resource`
// resolver — richer than the no-handle ancestor walk) delegate to the core
// where they don't need an `AppHandle`.
// `ACTIVE_THEME_FILE` / `LoaderConfig` are part of the original public surface
// of this module (the desktop file exported them); they're re-exported here so
// that surface is preserved even though src-tauri has no internal caller today.
#[allow(unused_imports)]
pub use ridge_core::commands::theme::{ACTIVE_THEME_FILE, LoaderConfig};
pub use ridge_core::commands::theme::{ThemeEntry, ThemeFile};

/// Resolve `ridge.theme` via Tauri's `BaseDirectory::Resource`. The
/// `bundle.resources` map in `tauri.conf.json` declares `ridge.theme`
/// as a resource that lands at the resource root in both modes:
///   - dev (`pnpm tauri dev`): `<repo>/src-tauri/target/<profile>/ridge.theme`
///   - packaged: `<install-dir>/ridge.theme` (next to `ridge.exe`)
///
/// Falls back (debug-only) to walking ancestors of the running exe so
/// editing the repo-root `ridge.theme` takes effect without waiting for
/// cargo to re-stage the resource. The walk is gated on
/// `cfg!(debug_assertions)` so a release exe in an unfamiliar layout
/// never accidentally pulls in a `ridge.theme` from a parent directory.
fn find_theme_path(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(p) = app.path().resolve("ridge.theme", BaseDirectory::Resource) {
        if p.exists() {
            return Some(p);
        }
    }

    if cfg!(debug_assertions) {
        let exe = std::env::current_exe().ok()?;
        let mut dir = exe.parent()?;
        while let Some(parent) = dir.parent() {
            let candidate = parent.join("ridge.theme");
            if candidate.exists() {
                return Some(candidate);
            }
            dir = parent;
        }
    }

    None
}

/// Read the persisted active theme id. Delegates to the core (same
/// `active-theme.txt` location and default-fallback behaviour).
pub fn read_active_theme_id(app_data_dir: &Path) -> String {
    ridge_core::commands::theme::read_active_theme_id(app_data_dir)
}

/// Build the JS snippet that gets injected into the WebView *before*
/// any page script runs. Sets globals consumed by `src/app.html`'s inline
/// splash bootstrap (`__RIDGE_BOOT_LOADER__` / `__RIDGE_BOOT_THEME_BG__` /
/// `__RIDGE_BOOT_THEME_COLORS__` / `__RIDGE_BOOT_THEME_ID__`).
///
/// Unchanged from the pre-S1 desktop behaviour: still uses the `AppHandle`
/// `Resource` resolver via `get_theme_data` so it sees the bundled catalog.
pub fn build_splash_init_script(app: &AppHandle, app_data_dir: &Path) -> String {
    let theme_id = read_active_theme_id(app_data_dir);
    let tf = get_theme_data(app.clone());
    let entry = tf
        .themes
        .iter()
        .find(|t| t.id == theme_id)
        .or_else(|| tf.themes.first());
    let (loader_json, bg_json, colors_json, id_json) = match entry {
        Some(t) => {
            let loader = serde_json::to_string(&t.loader).unwrap_or_else(|_| "null".to_string());
            let bg = t
                .colors
                .get("bg")
                .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()))
                .unwrap_or_else(|| "null".to_string());
            let colors = serde_json::to_string(&t.colors).unwrap_or_else(|_| "null".to_string());
            let id = serde_json::to_string(&t.id).unwrap_or_else(|_| "null".to_string());
            (loader, bg, colors, id)
        }
        None => (
            "null".to_string(),
            "null".to_string(),
            "null".to_string(),
            "null".to_string(),
        ),
    };
    format!(
        "Object.defineProperty(window,'__RIDGE_BOOT_LOADER__',{{value:Object.freeze({loader}),writable:false,configurable:false}});\
         Object.defineProperty(window,'__RIDGE_BOOT_THEME_BG__',{{value:{bg},writable:false,configurable:false}});\
         Object.defineProperty(window,'__RIDGE_BOOT_THEME_COLORS__',{{value:Object.freeze({colors}),writable:false,configurable:false}});\
         Object.defineProperty(window,'__RIDGE_BOOT_THEME_ID__',{{value:{id},writable:false,configurable:false}});",
        loader = loader_json,
        bg = bg_json,
        colors = colors_json,
        id = id_json,
    )
}

/// Frontend → backend: persist the user's theme choice so the *next* launch's
/// splash can use it. **S1 thin wrapper** — the write logic lives in the core;
/// the error is mapped back to the legacy `Result<(), String>` shape.
#[tauri::command]
pub fn set_active_theme(theme_id: String) -> Result<(), String> {
    ridge_core::commands::theme::set_active_theme(&theme_id).map_err(|e| e.to_command_string())
}

/// AppHandle-free resolution of the desktop's currently active theme, used by
/// the remote server to push the live theme to browser clients. Delegates to
/// the core's handle-free resolver.
pub fn active_theme_entry_no_handle() -> Option<ThemeEntry> {
    ridge_core::commands::theme::active_theme_entry()
}

/// Resolve the active theme catalog. Uses the `AppHandle` `Resource` resolver
/// (so it picks up the bundled `ridge.theme`); falls back to an empty catalog
/// on read/parse failure exactly as before. Behaviour unchanged from pre-S1.
#[tauri::command]
pub fn get_theme_data(app: AppHandle) -> ThemeFile {
    if let Some(path) = find_theme_path(&app) {
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<ThemeFile>(&content) {
                Ok(tf) => {
                    if tf.version >= 1 && !tf.themes.is_empty() {
                        return tf;
                    }
                    tracing::warn!(
                        target: "ridge::theme",
                        path = %path.display(),
                        "ridge.theme has no themes or invalid version"
                    );
                }
                Err(e) => tracing::error!(
                    target: "ridge::theme",
                    path = %path.display(),
                    error = %e,
                    "failed to parse ridge.theme"
                ),
            },
            Err(e) => tracing::error!(
                target: "ridge::theme",
                path = %path.display(),
                error = %e,
                "failed to read ridge.theme"
            ),
        }
    } else {
        tracing::warn!(
            target: "ridge::theme",
            "no ridge.theme found in any search location"
        );
    }
    ThemeFile {
        version: 1,
        themes: Vec::new(),
    }
}
