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

/// Bootstrap content used ONLY when no `ridge.theme` exists in any of
/// the search locations (exe-dir / cwd / app_data_dir). Contains a single
/// "无尽深色" theme — every other theme the user wants must be added to
/// the on-disk file. No multi-theme catalog is ever embedded in the
/// binary, so the file is the sole source of truth for the available
/// themes.
const BOOTSTRAP_THEME_JSON: &str = r##"{
  "version": 1,
  "themes": [
    {
      "id": "endless-dark",
      "label": "无尽深色",
      "type": "dark",
      "loader": { "primary": "#eeeeee", "secondary": "#888888" },
      "colors": {
        "bg": "#000000",
        "bg-raised": "#0a0a0a",
        "surface": "#141414",
        "surface-2": "#1e1e1e",
        "glass": "rgba(20,20,20,0.72)",
        "border": "rgba(255,255,255,0.06)",
        "border-bright": "rgba(255,255,255,0.12)",
        "fg": "#e0e0e0",
        "fg-muted": "#666666",
        "accent": "#eeeeee",
        "accent-glow": "rgba(238,238,238,0.18)",
        "term-bg": "#000000",
        "tui-bg": "#000000",
        "scrollbar": "rgba(255,255,255,0.08)",
        "scrollbar-hover": "rgba(238,238,238,0.40)",
        "title-proc": "#eeeeee",
        "title-sep": "#2a2a2a",
        "title-cwd": "#888888"
      }
    }
  ]
}
"##;

/// Find an existing `ridge.theme` file. Search order:
///   1. Next to the running executable (production install).
///   2. The current working directory (dev: project root).
///   3. `<app_data_dir>/ridge.theme` (per-user fallback /
///      bootstrap target).
fn find_theme_path(app_data_dir: &Path) -> Option<PathBuf> {
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
    let user_path = app_data_dir.join("ridge.theme");
    if user_path.exists() {
        return Some(user_path);
    }
    None
}

/// First-launch bootstrap: write `BOOTSTRAP_THEME_JSON` to
/// `<app_data_dir>/ridge.theme`. Idempotent — only runs when no
/// theme file exists in any of the search locations.
fn bootstrap_theme_file(app_data_dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(app_data_dir)?;
    let path = app_data_dir.join("ridge.theme");
    std::fs::write(&path, BOOTSTRAP_THEME_JSON)?;
    Ok(path)
}

/// Public entry point used by `lib.rs` at startup: ensure a usable
/// `ridge.theme` exists before any frontend code runs. If a file is
/// already present somewhere in the search path this is a no-op; if
/// nothing is found, the bootstrap content is materialized into
/// `<app_data_dir>/ridge.theme`. Errors are logged but never propagate —
/// `get_theme_data` itself has a final in-memory fallback to the same
/// bootstrap content if writing failed.
pub fn ensure_theme_file_exists(app_data_dir: &Path) {
    if find_theme_path(app_data_dir).is_some() {
        return;
    }
    match bootstrap_theme_file(app_data_dir) {
        Ok(p) => tracing::info!(
            target: "ridge::theme",
            path = %p.display(),
            "bootstrapped ridge.theme with default 无尽深色 theme"
        ),
        Err(e) => tracing::error!(
            target: "ridge::theme",
            error = %e,
            "failed to bootstrap ridge.theme — splash will use in-memory default"
        ),
    }
}

fn default_theme_file() -> ThemeFile {
    serde_json::from_str(BOOTSTRAP_THEME_JSON)
        .expect("BOOTSTRAP_THEME_JSON must be valid JSON")
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
                        "ridge.theme has no themes or invalid version, using bootstrap default"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        target: "ridge::theme",
                        path = %path.display(),
                        error = %e,
                        "failed to parse ridge.theme, using bootstrap default"
                    );
                }
            },
            Err(e) => {
                tracing::error!(
                    target: "ridge::theme",
                    path = %path.display(),
                    error = %e,
                    "failed to read ridge.theme, using bootstrap default"
                );
            }
        }
    }
    default_theme_file()
}
