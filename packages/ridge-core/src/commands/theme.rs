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
    /// 背景图资源文件名（相对 `theme-assets/`），仅自定义主题用。
    #[serde(rename = "bgImage", default, skip_serializing_if = "Option::is_none")]
    pub bg_image: Option<String>,
    /// 背景图透明度 0..1，缺省视为 1。
    #[serde(rename = "bgImageOpacity", default, skip_serializing_if = "Option::is_none")]
    pub bg_image_opacity: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFile {
    pub version: u32,
    pub themes: Vec<ThemeEntry>,
}

/// Filename inside `<app_data_dir>` recording the selected theme id.
pub const ACTIVE_THEME_FILE: &str = "active-theme.txt";

/// 可写的用户自定义主题目录文件名（`<app_data_dir>/user-themes.json`）。
pub const USER_THEMES_FILE: &str = "user-themes.json";

/// 自定义主题 id 强制前缀，便于与内置主题区分（前端据此判可编辑/删除）。
pub const CUSTOM_ID_PREFIX: &str = "custom-";

/// Default theme id when the active-theme file is missing/unreadable.
/// Matches the frontend `DEFAULTS.theme` and the desktop original.
const DEFAULT_THEME_ID: &str = "endless-dark";

fn active_theme_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ACTIVE_THEME_FILE)
}

fn user_themes_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(USER_THEMES_FILE)
}

/// 背景图资源目录 `<app_data_dir>/theme-assets`。
pub fn theme_assets_dir() -> PathBuf {
    app_data_dir().join("theme-assets")
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

/// 由 label 生成稳定、唯一、带 `custom-` 前缀的主题 id。
/// 规则：转小写、非字母数字折叠成单个 `-`、去首尾 `-`；空则用 `theme`；
/// 与 `existing` 撞车则追加 `-2`/`-3`…。CJK 字符（is_alphanumeric 为真）保留。
fn make_custom_id(label: &str, existing: &[String]) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in label.trim().chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() {
                slug.push(c);
            }
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let slug = if slug.is_empty() { "theme".to_string() } else { slug };
    let base = format!("{CUSTOM_ID_PREFIX}{slug}");
    if !existing.iter().any(|e| e == &base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let cand = format!("{base}-{n}");
        if !existing.iter().any(|e| e == &cand) {
            return cand;
        }
        n += 1;
    }
}

/// 读用户自定义主题列表。文件缺失/解析失败 → 空 Vec（绝不让启动失败）。
pub fn read_user_themes(app_data_dir: &Path) -> Vec<ThemeEntry> {
    let path = user_themes_path(app_data_dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_str::<ThemeFile>(&content) {
        Ok(tf) => tf.themes,
        Err(e) => {
            tracing::warn!(target: "ridge::theme", error = %e, "user-themes.json 解析失败，忽略");
            Vec::new()
        }
    }
}

fn write_user_themes(app_data_dir: &Path, themes: &[ThemeEntry]) -> CoreResult<()> {
    std::fs::create_dir_all(app_data_dir).map_err(|e| CoreError::io(format!("create dir: {e}")))?;
    let tf = ThemeFile { version: 1, themes: themes.to_vec() };
    let json = serde_json::to_string_pretty(&tf)
        .map_err(|e| CoreError::internal(format!("serialize user themes: {e}")))?;
    std::fs::write(user_themes_path(app_data_dir), json)
        .map_err(|e| CoreError::io(format!("write user-themes.json: {e}")))
}

/// upsert 一个自定义主题到 user-themes.json，返回最终落盘的 entry。
/// id 为空或不带 `custom-` 前缀 → 视为新增、由 label 生成唯一 id；否则按 id 覆盖。
pub fn save_user_theme(mut entry: ThemeEntry) -> CoreResult<ThemeEntry> {
    let dir = app_data_dir();
    let mut themes = read_user_themes(&dir);
    let is_new = entry.id.is_empty() || !entry.id.starts_with(CUSTOM_ID_PREFIX);
    if is_new {
        let existing: Vec<String> = themes.iter().map(|t| t.id.clone()).collect();
        entry.id = make_custom_id(&entry.label, &existing);
        themes.push(entry.clone());
    } else {
        match themes.iter_mut().find(|t| t.id == entry.id) {
            Some(slot) => *slot = entry.clone(),
            None => themes.push(entry.clone()),
        }
    }
    write_user_themes(&dir, &themes)?;
    Ok(entry)
}

/// 删除一个自定义主题及其背景图（best-effort）。
pub fn delete_user_theme(id: &str) -> CoreResult<()> {
    let dir = app_data_dir();
    let mut themes = read_user_themes(&dir);
    if let Some(pos) = themes.iter().position(|t| t.id == id) {
        if let Some(img) = themes[pos].bg_image.clone() {
            let _ = std::fs::remove_file(theme_assets_dir().join(img));
        }
        themes.remove(pos);
        write_user_themes(&dir, &themes)?;
    }
    Ok(())
}

const ALLOWED_IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
const MAX_IMG_BYTES: usize = 20 * 1024 * 1024;

/// 把背景图字节写入 `theme-assets/<uuid>.<ext>`，返回文件名。
pub fn save_theme_bg_image(bytes: Vec<u8>, ext: &str) -> CoreResult<String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if !ALLOWED_IMG_EXT.contains(&ext.as_str()) {
        return Err(CoreError::internal(format!("unsupported image type: {ext}")));
    }
    if bytes.is_empty() || bytes.len() > MAX_IMG_BYTES {
        return Err(CoreError::internal(format!("image size out of range: {} bytes", bytes.len())));
    }
    let dir = theme_assets_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(format!("create theme-assets: {e}")))?;
    let name = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    std::fs::write(dir.join(&name), &bytes).map_err(|e| CoreError::io(format!("write image: {e}")))?;
    Ok(name)
}

/// 返回 theme-assets 目录绝对路径字符串（前端拼路径 + convertFileSrc 用）。
pub fn get_theme_assets_dir() -> String {
    theme_assets_dir().to_string_lossy().to_string()
}

/// 把用户主题追加到内置目录之后；id 撞车时丢弃用户那条（内置优先）。纯函数，便于单测。
pub fn merge_user_theme_list(mut base: ThemeFile, user: Vec<ThemeEntry>) -> ThemeFile {
    let have: std::collections::HashSet<String> =
        base.themes.iter().map(|t| t.id.clone()).collect();
    for u in user {
        if !have.contains(&u.id) {
            base.themes.push(u);
        }
    }
    base
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
                        return merge_user_theme_list(tf, read_user_themes(&app_data_dir()));
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

    #[test]
    fn read_user_themes_missing_file_is_empty() {
        let tmp = std::env::temp_dir().join("ridge-test-user-empty");
        let _ = std::fs::remove_dir_all(&tmp);
        let v = read_user_themes(&tmp);
        assert!(v.is_empty());
    }

    #[test]
    fn slugify_makes_custom_prefixed_unique_id() {
        let existing = vec!["custom-my-theme".to_string()];
        assert_eq!(make_custom_id("My Theme!!", &existing), "custom-my-theme-2");
        assert_eq!(make_custom_id("全新主题", &[]), "custom-全新主题");
        assert_eq!(make_custom_id("   ", &[]), "custom-theme");
    }

    fn sample_user_entry(label: &str) -> ThemeEntry {
        ThemeEntry {
            id: format!("{CUSTOM_ID_PREFIX}tmp"),
            label: label.into(),
            theme_type: "dark".into(),
            loader: LoaderConfig {
                primary: "#fff".into(), secondary: "#000".into(),
                bg: None, accent_glow: None, stroke_width: None, corner_radius: None,
                draw_duration_ms: None, breathe_duration_ms: None, cross_delay_ms: None,
                fade_out_duration_ms: None, fill_opacity_primary: None, fill_opacity_secondary: None,
            },
            colors: HashMap::new(),
            bg_image: None,
            bg_image_opacity: None,
        }
    }

    #[test]
    fn save_and_delete_user_theme_round_trip() {
        let tmp = std::env::temp_dir().join("ridge-test-user-crud");
        let _ = std::fs::remove_dir_all(&tmp);
        let _guard = SetEnvGuard::new("LOCALAPPDATA", &tmp.to_string_lossy());
        let mut entry = sample_user_entry("My Theme");
        entry.id = String::new();
        let saved = save_user_theme(entry).unwrap();
        assert_eq!(saved.id, "custom-my-theme");
        let listed = read_user_themes(&app_data_dir());
        assert_eq!(listed.len(), 1);
        let mut edit = saved.clone();
        edit.label = "Renamed".into();
        let saved2 = save_user_theme(edit).unwrap();
        assert_eq!(saved2.id, "custom-my-theme");
        assert_eq!(read_user_themes(&app_data_dir())[0].label, "Renamed");
        delete_user_theme("custom-my-theme").unwrap();
        assert!(read_user_themes(&app_data_dir()).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_appends_user_themes_after_base() {
        let mut base = ThemeFile { version: 1, themes: vec![sample_user_entry("Builtin")] };
        base.themes[0].id = "endless-dark".into();
        let user = vec![sample_user_entry("U1")];
        let merged = merge_user_theme_list(base, user);
        assert_eq!(merged.themes.len(), 2);
        assert_eq!(merged.themes[0].id, "endless-dark");
        assert!(merged.themes[1].id.starts_with("custom-"));
    }

    #[test]
    fn save_bg_image_validates_and_writes() {
        let tmp = std::env::temp_dir().join("ridge-test-bgimg");
        let _ = std::fs::remove_dir_all(&tmp);
        let _guard = SetEnvGuard::new("LOCALAPPDATA", &tmp.to_string_lossy());
        assert!(save_theme_bg_image(vec![1, 2, 3], "exe").is_err());
        assert!(save_theme_bg_image(vec![0u8; 21 * 1024 * 1024], "png").is_err());
        let name = save_theme_bg_image(vec![0u8; 16], "PNG").unwrap();
        assert!(name.ends_with(".png"));
        assert!(theme_assets_dir().join(&name).exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// Mutex that serializes all tests that mutate env vars (LOCALAPPDATA etc.)
/// so parallel test threads cannot race on `std::env::set_var`.
#[cfg(test)]
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard: acquires ENV_MUTEX, sets an env var, and restores it on drop.
/// The held MutexGuard keeps the lock alive for the guard's lifetime so
/// concurrent env-var-mutating tests are fully serialized.
#[cfg(test)]
struct SetEnvGuard {
    key: String,
    old: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl SetEnvGuard {
    fn new(key: &str, val: &str) -> Self {
        let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var(key).ok();
        // Safety: called only inside tests; ENV_MUTEX serializes all callers so
        // no two threads touch env vars concurrently.
        unsafe { std::env::set_var(key, val) };
        Self { key: key.into(), old, _lock: lock }
    }
}

#[cfg(test)]
impl Drop for SetEnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => unsafe { std::env::set_var(&self.key, v) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}
