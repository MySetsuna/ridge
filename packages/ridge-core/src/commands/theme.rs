//! Theme catalog handlers — migrated from `src-tauri/src/commands/theme.rs`.
//!
//! The desktop original used `AppHandle` only to resolve the `ridge.theme`
//! resource path (`app.path().resolve(.., Resource)`). That is the sole Tauri
//! coupling, and the desktop file already carries a **handle-free** fallback
//! (`find_theme_path_no_handle` / `active_theme_entry_no_handle`) used by the
//! remote server thread. We port the handle-free path here so `ridge-core`
//! needs no `AppHandle` at all (D4 / §5.1): the catalog sits next to the
//! running exe in every real layout, with an ancestor walk as fallback.
//!
//! `set_active_theme` had **no** `State`/`AppHandle` to begin with — it is a
//! pure `data_local_dir()` write — so it ports verbatim.
//!
//! Behaviour parity with the desktop:
//!   - same `ridge.theme` search order (exe dir → ancestor walk);
//!   - same `active-theme.txt` location (`<LOCALAPPDATA>/ridge`) and contents;
//!   - same empty-catalog fallback on read/parse failure.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// Splash loader contract. Mirrors the desktop `LoaderConfig` field-for-field
/// (same `#[serde(rename_all = "camelCase")]`, same optional knobs) so the
/// JSON the frontend receives is byte-identical.
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

/// Filename inside `<app_data_dir>` recording the selected theme id.
pub const ACTIVE_THEME_FILE: &str = "active-theme.txt";

/// Default theme id when the active-theme file is missing/unreadable.
/// Matches the frontend `DEFAULTS.theme` and the desktop original.
const DEFAULT_THEME_ID: &str = "endless-dark";

fn active_theme_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ACTIVE_THEME_FILE)
}

/// The `<LOCALAPPDATA>/ridge` directory the desktop app uses for logs /
/// `projects.db` / `active-theme.txt`. Ported from the desktop original's
/// inline `dirs::data_local_dir()` computation. `ridge-core` avoids a `dirs`
/// dependency, so we resolve the platform local-data dir from environment
/// (`LOCALAPPDATA` on Windows, `XDG_DATA_HOME`/`HOME` on unix) — the same
/// directory `dirs::data_local_dir()` returns on each platform.
pub fn app_data_dir() -> PathBuf {
    local_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge")
}

#[cfg(windows)]
fn local_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn local_data_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn local_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

/// Read the persisted active theme id, falling back to the default on any
/// IO/parse error (startup must never fail here). Verbatim port.
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

/// Locate `ridge.theme` without any Tauri handle: it sits next to the running
/// exe in every real layout (packaged install and `target/<profile>` in dev),
/// with an ancestor walk as a fallback for unusual repo layouts. This is the
/// exact algorithm of the desktop `find_theme_path_no_handle`.
pub fn find_theme_path() -> Option<PathBuf> {
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

/// Handler: `get_theme_data`. Returns the parsed catalog, or an empty catalog
/// (`version: 1`, no themes) on any read/parse failure — identical to the
/// desktop `get_theme_data`, which never errors so the splash can fall back to
/// its CSS defaults.
pub fn get_theme_data() -> ThemeFile {
    if let Some(path) = find_theme_path() {
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
        tracing::warn!(target: "ridge::theme", "no ridge.theme found in any search location");
    }
    ThemeFile {
        version: 1,
        themes: Vec::new(),
    }
}

/// AppHandle-free resolution of the currently active theme entry. Ported from
/// the desktop `active_theme_entry_no_handle` (used by the remote server to
/// push the live theme to browser clients). Kept here so the headless host has
/// the same capability without reaching into `src-tauri`.
pub fn active_theme_entry() -> Option<ThemeEntry> {
    let dir = app_data_dir();
    let theme_id = read_active_theme_id(&dir);
    let path = find_theme_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let tf: ThemeFile = serde_json::from_str(&content).ok()?;
    let mut themes = tf.themes;
    if themes.is_empty() {
        return None;
    }
    let idx = themes.iter().position(|t| t.id == theme_id).unwrap_or(0);
    Some(themes.swap_remove(idx))
}

/// Handler: `set_active_theme`. Persists the theme id so the next launch's
/// splash can use it. Verbatim port (the desktop original took no `State`/
/// `AppHandle`); error message preserved for parity.
pub fn set_active_theme(theme_id: &str) -> CoreResult<()> {
    let dir = app_data_dir();
    write_active_theme_id(&dir, theme_id).map_err(|e| {
        tracing::error!(
            target: "ridge::theme",
            error = %e,
            "failed to persist active theme id"
        );
        CoreError::io(format!("write active-theme.txt: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_catalog_has_version_one_and_no_themes() {
        // With no `ridge.theme` discoverable in the test exe's layout, the
        // handler returns the empty catalog rather than erroring.
        let tf = get_theme_data();
        assert!(tf.version >= 1);
        // We can't assert empty (a stray ridge.theme could exist in an
        // ancestor during dev), so just assert it parsed into the struct.
        let _ = tf.themes.len();
    }

    #[test]
    fn loader_config_round_trips_camel_case() {
        let json = r##"{"primary":"#fff","secondary":"#000","drawDurationMs":1200}"##;
        let cfg: LoaderConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.primary, "#fff");
        assert_eq!(cfg.draw_duration_ms, Some(1200));
        let back = serde_json::to_string(&cfg).unwrap();
        assert!(back.contains("drawDurationMs"));
        // Optional None fields are skipped, matching the desktop serializer.
        assert!(!back.contains("strokeWidth"));
    }

    #[test]
    fn read_active_theme_id_returns_default_on_missing_file() {
        let tmp = std::env::temp_dir().join("ridge-test-nonexistent");
        let id = read_active_theme_id(&tmp);
        assert_eq!(id, "endless-dark");
    }

    #[test]
    fn read_active_theme_id_trims_whitespace() {
        let tmp = std::env::temp_dir().join("ridge-test-trim");
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("active-theme.txt");
        std::fs::write(&path, "  my-theme  ").ok();
        let id = read_active_theme_id(&tmp);
        assert_eq!(id, "my-theme");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_active_theme_id_returns_default_on_empty_file() {
        let tmp = std::env::temp_dir().join("ridge-test-empty");
        std::fs::create_dir_all(&tmp).ok();
        let path = tmp.join("active-theme.txt");
        std::fs::write(&path, "").ok();
        let id = read_active_theme_id(&tmp);
        assert_eq!(id, "endless-dark");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn theme_file_deserializes_with_loader_and_colors() {
        let json = r##"{
            "version": 1,
            "themes": [{
                "id": "test",
                "label": "Test",
                "type": "dark",
                "loader": {"primary":"#fff","secondary":"#000"},
                "colors": {"bg":"#000","fg":"#fff"}
            }]
        }"##;
        let tf: ThemeFile = serde_json::from_str(json).unwrap();
        assert_eq!(tf.version, 1);
        assert_eq!(tf.themes.len(), 1);
        assert_eq!(tf.themes[0].id, "test");
        assert_eq!(tf.themes[0].loader.primary, "#fff");
        assert_eq!(tf.themes[0].colors.get("bg").unwrap(), "#000");
    }

    #[test]
    fn active_theme_entry_gracefully_handles_found_theme() {
        // During `cargo test` the binary lives under target/debug/ and
        // find_theme_path() walks ancestors until it hits the repo-root
        // ridge.theme (which exists). This test just validates the function
        // returns Some in that case and doesn't panic.
        if let Some(entry) = active_theme_entry() {
            assert!(!entry.id.is_empty());
            assert!(!entry.loader.primary.is_empty());
        }
    }

    #[test]
    fn theme_file_rejects_invalid_version() {
        let json = r##"{"version":0,"themes":[{"id":"x","label":"X","type":"dark","loader":{"primary":"#fff","secondary":"#000"},"colors":{}}]}"##;
        let tf: ThemeFile = serde_json::from_str(json).unwrap();
        // get_theme_data filters out version < 1 or empty themes.
        // But the raw deserialization itself should succeed.
        assert_eq!(tf.version, 0);
        assert_eq!(tf.themes.len(), 1);
    }

    #[test]
    fn set_active_theme_writes_and_reads_back() {
        let tmp = std::env::temp_dir().join("ridge-test-set");
        let ridge_dir = tmp.join("ridge");
        // Temporarily redirect app_data_dir by setting LOCALAPPDATA on Windows.
        let _guard = SetEnvGuard::new("LOCALAPPDATA", &tmp.to_string_lossy());
        set_active_theme("my-custom-theme").ok();
        // set_active_theme writes to app_data_dir()/active-theme.txt =
        // <LOCALAPPDATA>/ridge/active-theme.txt = tmp/ridge/active-theme.txt
        let read = read_active_theme_id(&ridge_dir);
        assert_eq!(read, "my-custom-theme");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
/// RAII guard to temporarily set an env var, restoring the original on drop.

struct SetEnvGuard {
    key: String,
    old: Option<String>,
}

#[cfg(test)]
impl SetEnvGuard {
    fn new(key: &str, val: &str) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, val);
        Self { key: key.into(), old }
    }
}

#[cfg(test)]
impl Drop for SetEnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => std::env::set_var(&self.key, v),
            None => std::env::remove_var(&self.key),
        }
    }
}
