use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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

/// Find an existing `ridge.theme` file. Search order:
///   1. `<app_data_dir>/ridge.theme` — the per-user editable copy
///      (preferred so user edits stick across upgrades).
///   2. Next to the running executable — the bundled file the installer
///      placed there (production seed).
///   3. The current working directory — only useful in `cargo run` /
///      dev, where the project root contains `ridge.theme`.
fn find_theme_path(app_data_dir: &Path) -> Option<PathBuf> {
    let user_path = app_data_dir.join("ridge.theme");
    if user_path.exists() {
        return Some(user_path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let path = parent.join("ridge.theme");
            if path.exists() {
                return Some(path);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("ridge.theme");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// First-launch bootstrap: copy the bundled `ridge.theme` into the
/// per-user editable location so future user edits survive upgrades.
/// Idempotent — does nothing once `<app_data_dir>/ridge.theme` exists.
///
/// Source picked in order: exe-dir → cwd. If neither is available we
/// just log and bail — `get_theme_data` still walks the search path on
/// every call so a later `ridge.theme` showing up in any location will
/// be picked up without restart.
pub fn ensure_theme_file_exists(app_data_dir: &Path) {
    let user_path = app_data_dir.join("ridge.theme");
    if user_path.exists() {
        return;
    }
    let source = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("ridge.theme")))
        .filter(|p| p.exists())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join("ridge.theme"))
                .filter(|p| p.exists())
        });
    let Some(src) = source else {
        tracing::warn!(
            target: "ridge::theme",
            "no ridge.theme found to bootstrap from — splash will use CSS fallbacks until one appears"
        );
        return;
    };
    if let Err(e) = std::fs::create_dir_all(app_data_dir) {
        tracing::error!(
            target: "ridge::theme",
            error = %e,
            "failed to create app_data_dir for ridge.theme bootstrap"
        );
        return;
    }
    match std::fs::copy(&src, &user_path) {
        Ok(_) => tracing::info!(
            target: "ridge::theme",
            src = %src.display(),
            dst = %user_path.display(),
            "copied ridge.theme into per-user editable location"
        ),
        Err(e) => tracing::error!(
            target: "ridge::theme",
            error = %e,
            "failed to copy ridge.theme — splash will read directly from bundle"
        ),
    }
}

fn app_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge")
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
pub fn build_splash_init_script(app_data_dir: &Path) -> String {
    let theme_id = read_active_theme_id(app_data_dir);
    let tf = get_theme_data();
    let entry = tf
        .themes
        .iter()
        .find(|t| t.id == theme_id)
        .or_else(|| tf.themes.first());
    let (loader_json, bg_json, id_json) = match entry {
        Some(t) => {
            let loader = serde_json::to_string(&t.loader)
                .unwrap_or_else(|_| "null".to_string());
            let bg = t
                .colors
                .get("bg")
                .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "null".to_string()))
                .unwrap_or_else(|| "null".to_string());
            let id = serde_json::to_string(&t.id).unwrap_or_else(|_| "null".to_string());
            (loader, bg, id)
        }
        None => ("null".to_string(), "null".to_string(), "null".to_string()),
    };
    // `Object.freeze` so a hot-reloaded module can't silently rewrite the
    // boot snapshot — debugging splash regressions is easier when these
    // values only ever come from this one source.
    format!(
        "Object.defineProperty(window,'__RIDGE_BOOT_LOADER__',{{value:Object.freeze({loader}),writable:false,configurable:false}});\
         Object.defineProperty(window,'__RIDGE_BOOT_THEME_BG__',{{value:{bg},writable:false,configurable:false}});\
         Object.defineProperty(window,'__RIDGE_BOOT_THEME_ID__',{{value:{id},writable:false,configurable:false}});",
        loader = loader_json,
        bg = bg_json,
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

#[tauri::command]
pub fn get_theme_data() -> ThemeFile {
    let data_dir = app_data_dir();
    if let Some(path) = find_theme_path(&data_dir) {
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
