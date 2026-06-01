use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

/// Splash loader contract. `primary` / `secondary` are required and feed
/// the SVG stroke and accent fill in `src/app.html`. The remaining
/// fields are optional knobs themes may set to override the hardcoded
/// CSS-variable fallbacks (animation timing, stroke width, opacities,
/// etc.). Numbers are interpreted on the JS side: `*Width` / `*Radius`
/// as CSS px, `*DurationMs` / `*DelayMs` as milliseconds, opacities as
/// raw 0..1 scalars.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderConfig {
    pub primary: String,
    pub secondary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_glow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_duration_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breathe_duration_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_delay_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out_duration_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_opacity_primary: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_opacity_secondary: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeEntry {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub theme_type: String,
    pub loader: LoaderConfig,
    pub colors: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFile {
    pub version: u32,
    pub themes: Vec<ThemeEntry>,
}

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

/// Filename used inside `<app_data_dir>` to record the currently selected
/// theme id. Pure text, one line. Survives crashes / unclean shutdowns so
/// the very first frame of the next launch can pick the right loader
/// colors before any JS has run.
pub const ACTIVE_THEME_FILE: &str = "active-theme.txt";

/// Default theme id used when the active-theme file is missing or
/// unreadable. Matches the frontend's `DEFAULTS.theme` in
/// `src/lib/stores/settings.ts`.
const DEFAULT_THEME_ID: &str = "endless-dark";

fn active_theme_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ACTIVE_THEME_FILE)
}

/// Read the persisted active theme id. Falls back to the default on any
/// IO / parse error — startup must never fail here, the splash uses the
/// builtin defaults if nothing is recorded yet.
pub fn read_active_theme_id(app_data_dir: &Path) -> String {
    match std::fs::read_to_string(active_theme_path(app_data_dir)) {
        Ok(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                DEFAULT_THEME_ID.to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(_) => DEFAULT_THEME_ID.to_string(),
    }
}

fn write_active_theme_id(app_data_dir: &Path, id: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(app_data_dir)?;
    std::fs::write(active_theme_path(app_data_dir), id.trim())
}

/// Build the JS snippet that gets injected into the WebView *before*
/// any page script runs. Sets two globals consumed by `src/app.html`'s
/// inline splash bootstrap:
///   - `window.__RIDGE_BOOT_LOADER__`  — the active theme's `loader`
///      block (primary/secondary + optional knobs)
///   - `window.__RIDGE_BOOT_THEME_BG__` — the active theme's `colors.bg`
///      (fallback for `loader.bg`)
///   - `window.__RIDGE_BOOT_THEME_ID__` — the active theme id itself,
///      handy for diagnostics
///
/// Falls back to an empty object on disk-read failure so the splash
/// still renders (using its own CSS fallbacks).
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
            let loader = serde_json::to_string(&t.loader)
                .unwrap_or_else(|_| "null".to_string());
            let bg = t
                .colors
                .get("bg")
                .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()))
                .unwrap_or_else(|| "null".to_string());
            // Full colour palette — every `--rg-*` CSS var the app reads,
            // so `app.html`'s inline bootstrap script can paint chrome
            // (sidebar, body, file editor, etc.) with the active theme's
            // bg/fg/accent BEFORE SvelteKit hydrates. Without this the
            // first launch (no localStorage cache) shows a flash of
            // browser-default white between splash dismiss and the
            // async `initThemeSystem → initSettingsBoot` chain landing.
            let colors = serde_json::to_string(&t.colors)
                .unwrap_or_else(|_| "null".to_string());
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
    // `Object.freeze` so a hot-reloaded module can't silently rewrite the
    // boot snapshot — debugging splash regressions is easier when these
    // values only ever come from this one source.
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

/// Frontend → backend: persist the user's theme choice so the *next*
/// launch's splash can use it. Called from `setTheme` in
/// `src/lib/stores/settings.ts`.
///
/// Uses the same `<LOCALAPPDATA>\ridge` directory that
/// `lib.rs::run` set up for logs / `projects.db`. We compute it inline
/// rather than threading the path through AppState to keep the command
/// independent — failure to compute or write never blocks the UI; the
/// frontend's localStorage write is still authoritative within a
/// session.
#[tauri::command]
pub fn set_active_theme(theme_id: String) -> Result<(), String> {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge");
    write_active_theme_id(&dir, &theme_id).map_err(|e| {
        tracing::error!(
            target: "ridge::theme",
            error = %e,
            "failed to persist active theme id"
        );
        format!("write active-theme.txt: {e}")
    })
}

/// Locate `ridge.theme` without an `AppHandle`. The remote-server thread only
/// holds `AppState`, so it can't use Tauri's resource resolver — but in every
/// real layout the catalog sits next to the running exe (packaged install and
/// `target/<profile>/ridge.theme` in dev), with an ancestor walk as a fallback
/// for unusual repo layouts.
pub fn find_theme_path_no_handle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let next = dir.join("ridge.theme");
    if next.exists() {
        return Some(next);
    }
    let mut d = dir;
    while let Some(parent) = d.parent() {
        let candidate = parent.join("ridge.theme");
        if candidate.exists() {
            return Some(candidate);
        }
        d = parent;
    }
    None
}

/// AppHandle-free resolution of the desktop's currently active theme, used by
/// the remote server to push the live theme to browser clients. Reads the same
/// `active-theme.txt` `set_active_theme` writes and the same `ridge.theme`
/// catalog `get_theme_data` parses. Returns `None` (remote keeps its own CSS
/// fallbacks) when either is unavailable.
pub fn active_theme_entry_no_handle() -> Option<ThemeEntry> {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge");
    let theme_id = read_active_theme_id(&dir);
    let path = find_theme_path_no_handle()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let tf: ThemeFile = serde_json::from_str(&content).ok()?;
    let mut themes = tf.themes;
    if themes.is_empty() {
        return None;
    }
    let idx = themes.iter().position(|t| t.id == theme_id).unwrap_or(0);
    Some(themes.swap_remove(idx))
}

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
    // No usable file: return an empty catalog. Frontend gracefully
    // applies no CSS overrides; splash falls through to its CSS-variable
    // defaults. Putting any theme dictionary here would re-introduce the
    // hardcoded fallback we just removed.
    ThemeFile {
        version: 1,
        themes: Vec::new(),
    }
}
