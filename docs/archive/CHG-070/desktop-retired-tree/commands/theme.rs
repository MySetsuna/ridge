use std::path::Path;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::AppHandle;

// Transitional IPC registration delegates to the same authenticated Runtime boundary.
// It owns no theme files and never retries against desktop storage.
pub(crate) fn runtime_command<T: DeserializeOwned>(
    method: &str,
    args: Value,
    native: bool,
) -> Result<T, String> {
    super::runtime::request(method, args, native, uuid::Uuid::new_v4())
}

// Shared types preserve the existing presentation surface during IPC retirement.
#[allow(unused_imports)]
pub use ridge_core::commands::theme::{LoaderConfig, ACTIVE_THEME_FILE};
pub use ridge_core::commands::theme::{ThemeEntry, ThemeFile};

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
/// Read-only splash projection uses the same catalog loader as Runtime.
pub fn build_splash_init_script(_app: &AppHandle, app_data_dir: &Path) -> String {
    let theme_id = read_active_theme_id(app_data_dir);
    let tf = ridge_core::commands::theme::get_theme_data();
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

/// Persist the theme choice in Runtime for the next splash projection.
#[tauri::command]
pub fn set_active_theme(theme_id: String) -> Result<(), String> {
    runtime_command("set_active_theme", json!({"themeId":theme_id}), true)
}

/// 保存（新增/编辑）一个自定义主题，返回最终落盘的 entry。
#[tauri::command]
pub fn save_user_theme(entry: ThemeEntry) -> Result<ThemeEntry, String> {
    runtime_command("save_user_theme", json!({"entry":entry}), true)
}

/// 删除一个自定义主题及其背景图。
#[tauri::command]
pub fn delete_user_theme(id: String) -> Result<(), String> {
    runtime_command("delete_user_theme", json!({"id":id}), true)
}

/// 写入背景图字节，返回文件名（相对 theme-assets/）。
#[tauri::command]
pub fn save_theme_bg_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    runtime_command(
        "save_theme_bg_image",
        json!({"bytes":bytes,"ext":ext}),
        true,
    )
}

/// 从磁盘路径读取并写入背景图，返回文件名（前端无 fs 插件时用 dialog 选路径走此命令）。
#[tauri::command]
pub fn save_theme_bg_image_from_path(path: String) -> Result<String, String> {
    runtime_command("save_theme_bg_image_from_path", json!({"path":path}), true)
}

/// 返回 theme-assets 目录绝对路径（前端 convertFileSrc 用）。
#[tauri::command]
pub fn get_theme_assets_dir() -> Result<String, String> {
    runtime_command("get_theme_assets_dir", json!({}), true)
}

/// AppHandle-free resolution of the desktop's currently active theme, used by
/// the remote server to push the live theme to browser clients. Delegates to
/// the core's handle-free resolver.
pub fn active_theme_entry_no_handle() -> Option<ThemeEntry> {
    ridge_core::commands::theme::active_theme_entry()
}

/// Read the Runtime active theme; preserve service errors instead of masking them.
#[tauri::command]
pub fn get_active_theme_entry() -> Result<Option<ThemeEntry>, String> {
    runtime_command("get_active_theme_entry", json!({}), true)
}

/// Read the Runtime catalog through the same command boundary as CLI/Remote.
#[tauri::command]
pub fn get_theme_data() -> Result<ThemeFile, String> {
    runtime_command("get_theme_data", json!({}), true)
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
        assert_eq!(
            opens.len(),
            closes.len(),
            "number of Object.defineProperty opens ({}) must match number of close parens ({})",
            opens.len(),
            closes.len()
        );
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
