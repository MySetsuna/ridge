use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderColors {
    pub primary: String,
    pub secondary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeEntry {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub theme_type: String,
    pub loader: LoaderColors,
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
