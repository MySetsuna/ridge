use std::collections::HashMap;
use std::path::PathBuf;

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

const EMBEDDED_THEMES: &str = include_str!("../../../ridge.theme");

fn default_theme_file() -> ThemeFile {
    serde_json::from_str(EMBEDDED_THEMES).expect("embedded ridge.theme must be valid JSON")
}

fn find_theme_path() -> Option<PathBuf> {
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

#[tauri::command]
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
                        "ridge.theme has no themes or invalid version, using embedded defaults"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        target: "ridge::theme",
                        path = %path.display(),
                        error = %e,
                        "failed to parse ridge.theme, using embedded defaults"
                    );
                }
            },
            Err(e) => {
                tracing::error!(
                    target: "ridge::theme",
                    path = %path.display(),
                    error = %e,
                    "failed to read ridge.theme, using embedded defaults"
                );
            }
        }
    }
    default_theme_file()
}
