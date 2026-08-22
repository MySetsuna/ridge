use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use base64::Engine;
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use serde::Serialize;

const MAX_FAMILIES: usize = 16;
const MAX_FONT_FILES: usize = 32;
const MAX_FONT_BYTES: usize = 96 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalFontFace {
    family: String,
    data_base64: String,
}

fn normalize_families(families: Vec<String>) -> Result<Vec<String>, String> {
    let mut unique = Vec::new();
    for family in families {
        let family = family.trim().trim_matches(['\'', '"']).trim();
        if family.is_empty() {
            continue;
        }
        if family.len() > 128 || family.contains('\0') {
            return Err("FONT_DATA_INVALID: invalid terminal font family".to_string());
        }
        if !unique.iter().any(|seen: &String| seen.eq_ignore_ascii_case(family)) {
            unique.push(family.to_string());
        }
        if unique.len() == MAX_FAMILIES {
            break;
        }
    }
    if !unique.iter().any(|family| family.eq_ignore_ascii_case("monospace")) {
        unique.push("monospace".to_string());
    }
    Ok(unique)
}

fn query_family<'a>(name: &'a str) -> Family<'a> {
    match name.to_ascii_lowercase().as_str() {
        "monospace" | "ui-monospace" => Family::Monospace,
        "serif" => Family::Serif,
        "sans-serif" | "system-ui" => Family::SansSerif,
        _ => Family::Name(name),
    }
}

/// Resolve only the requested family stack. Raw font data never enters the
/// remote-command allow-list; browser Remote must use client-side Local Font
/// Access, preventing host-font exfiltration and preserving client typography.
fn resolve_terminal_font_faces(families: Vec<String>) -> Result<Vec<TerminalFontFace>, String> {
    let families = normalize_families(families)?;
    let mut db = Database::new();
    db.load_system_fonts();

    let variants = [
        (Weight::NORMAL, Style::Normal),
        (Weight::BOLD, Style::Normal),
        (Weight::NORMAL, Style::Italic),
        (Weight::BOLD, Style::Italic),
    ];
    let mut ids = Vec::new();
    for family_name in &families {
        let family = query_family(family_name);
        for (weight, style) in variants {
            let query = Query {
                families: std::slice::from_ref(&family),
                weight,
                stretch: Stretch::Normal,
                style,
            };
            if let Some(id) = db.query(&query) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut total = 0_usize;
    let mut result = Vec::new();
    for id in ids {
        let Some(data) = db.with_face_data(id, |bytes, _| bytes.to_vec()) else {
            continue;
        };
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        if !seen.insert(hasher.finish()) {
            continue;
        }
        total = total.saturating_add(data.len());
        if result.len() >= MAX_FONT_FILES || total > MAX_FONT_BYTES {
            return Err("FONT_DATA_LIMIT: selected terminal font stack is too large".to_string());
        }
        let family = db
            .face(id)
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        result.push(TerminalFontFace {
            family,
            data_base64: base64::engine::general_purpose::STANDARD.encode(data),
        });
    }
    if result.is_empty() {
        return Err(format!(
            "FONT_DATA_MISSING: no installed face matched {}",
            families.join(", ")
        ));
    }
    Ok(result)
}

#[tauri::command]
pub async fn load_terminal_font_faces(
    families: Vec<String>,
) -> Result<Vec<TerminalFontFace>, String> {
    tauri::async_runtime::spawn_blocking(move || resolve_terminal_font_faces(families))
        .await
        .map_err(|error| format!("FONT_DATA_IO: font resolver task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_css_names_and_adds_generic_fallback() {
        assert_eq!(
            normalize_families(vec![" 'Cascadia Code' ".into(), "Consolas".into()]).unwrap(),
            vec!["Cascadia Code", "Consolas", "monospace"]
        );
    }

    #[test]
    fn rejects_untrusted_family_payloads() {
        assert!(normalize_families(vec!["bad\0font".into()]).is_err());
        assert!(normalize_families(vec!["x".repeat(129)]).is_err());
    }
}
