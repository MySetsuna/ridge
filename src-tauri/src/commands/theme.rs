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
    format_splash_init_script(entry)
}

/// Pure function: produce the JS snippet for a given ThemeEntry (or None).
/// Extracted for unit-testability.
fn format_splash_init_script(entry: Option<&ThemeEntry>) -> String {
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

/// 保存（新增/编辑）一个自定义主题，返回最终落盘的 entry。
#[tauri::command]
pub fn save_user_theme(entry: ThemeEntry) -> Result<ThemeEntry, String> {
    ridge_core::commands::theme::save_user_theme(entry).map_err(|e| e.to_command_string())
}

/// 删除一个自定义主题及其背景图。
#[tauri::command]
pub fn delete_user_theme(id: String) -> Result<(), String> {
    ridge_core::commands::theme::delete_user_theme(&id).map_err(|e| e.to_command_string())
}

/// 写入背景图字节，返回文件名（相对 theme-assets/）。
#[tauri::command]
pub fn save_theme_bg_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    ridge_core::commands::theme::save_theme_bg_image(bytes, &ext).map_err(|e| e.to_command_string())
}

/// 返回 theme-assets 目录绝对路径（前端 convertFileSrc 用）。
#[tauri::command]
pub fn get_theme_assets_dir() -> String {
    ridge_core::commands::theme::get_theme_assets_dir()
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
                        let user = ridge_core::commands::theme::read_user_themes(
                            &ridge_core::commands::theme::app_data_dir(),
                        );
                        return ridge_core::commands::theme::merge_user_theme_list(tf, user);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_entry() -> ThemeEntry {
        ThemeEntry {
            id: "test-dark".into(),
            label: "Test Dark".into(),
            theme_type: "dark".into(),
            loader: LoaderConfig {
                primary: "#fff".into(),
                secondary: "#000".into(),
                bg: Some("#07090c".into()),
                accent_glow: Some("#36c26e".into()),
                stroke_width: Some(2.0),
                corner_radius: Some(4.0),
                draw_duration_ms: Some(800),
                breathe_duration_ms: Some(2000),
                cross_delay_ms: Some(600),
                fade_out_duration_ms: Some(400),
                fill_opacity_primary: Some(0.9),
                fill_opacity_secondary: Some(0.6),
            },
            colors: {
                let mut c = HashMap::new();
                c.insert("bg".into(), "#07090c".into());
                c.insert("fg".into(), "#c8e8d4".into());
                c.insert("accent".into(), "#36c26e".into());
                c
            },
            bg_image: None,
            bg_image_opacity: None,
        }
    }

    #[test]
    fn format_with_entry_produces_all_four_define_property_calls() {
        let entry = sample_entry();
        let js = format_splash_init_script(Some(&entry));
        assert!(js.contains("__RIDGE_BOOT_LOADER__"));
        assert!(js.contains("__RIDGE_BOOT_THEME_BG__"));
        assert!(js.contains("__RIDGE_BOOT_THEME_COLORS__"));
        assert!(js.contains("__RIDGE_BOOT_THEME_ID__"));
        assert!(js.contains("Object.defineProperty"));
        assert!(js.contains("Object.freeze"));
        assert!(js.contains("#07090c"));
        assert!(js.contains("#36c26e"));
    }

    #[test]
    fn format_with_none_uses_null_defaults() {
        let js = format_splash_init_script(None);
        assert!(js.contains("value:null"));
        // All four properties should still exist, just with null values.
        assert!(js.contains("__RIDGE_BOOT_LOADER__"));
        assert!(js.contains("__RIDGE_BOOT_THEME_BG__"));
        assert!(js.contains("__RIDGE_BOOT_THEME_COLORS__"));
        assert!(js.contains("__RIDGE_BOOT_THEME_ID__"));
    }

    #[test]
    fn format_js_syntax_is_valid_assignment() {
        let entry = sample_entry();
        let js = format_splash_init_script(Some(&entry));
        // The output is a JS statement that ends with a semicolon.
        assert!(js.ends_with(';'), "JS must end with statement terminator");
        // Each defineProperty call should be properly closed.
        let opens: Vec<_> = js.match_indices("Object.defineProperty").collect();
        let closes: Vec<_> = js.match_indices("});").collect();
        assert_eq!(opens.len(), closes.len(),
            "number of Object.defineProperty opens ({}) must match number of close parens ({})",
            opens.len(), closes.len());
    }

    #[test]
    fn loader_config_fields_are_camel_cased_in_output() {
        let entry = sample_entry();
        let js = format_splash_init_script(Some(&entry));
        // The loader JSON inside should use camelCase keys.
        assert!(js.contains("drawDurationMs"));
        assert!(js.contains("breatheDurationMs"));
        assert!(js.contains("fadeOutDurationMs"));
        assert!(js.contains("fillOpacityPrimary"));
        // snake_case keys should NOT appear.
        assert!(!js.contains("draw_duration_ms"));
    }

    #[test]
    fn colors_map_is_frozen_with_all_entries() {
        let entry = sample_entry();
        let js = format_splash_init_script(Some(&entry));
        // The colors object should contain all our color keys.
        assert!(js.contains("\"bg\":\"#07090c\""));
        assert!(js.contains("\"fg\":\"#c8e8d4\""));
        assert!(js.contains("\"accent\":\"#36c26e\""));
    }
}
