use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_FAMILIES: usize = 32;
const MAX_FONT_FILES: usize = 32;
const MAX_FONT_BYTES: usize = 96 * 1024 * 1024;
const MAX_SINGLE_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_FONT_CHUNK_BYTES: usize = 2 * 1024 * 1024;
const MAX_FONT_CACHE_BYTES: usize = MAX_FONT_BYTES * 2;

#[derive(Default)]
struct TerminalFontCache {
    faces: HashMap<String, Arc<[u8]>>,
    byte_len: usize,
}

fn terminal_font_cache() -> &'static Mutex<TerminalFontCache> {
    static CACHE: OnceLock<Mutex<TerminalFontCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(TerminalFontCache::default()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalFontFace {
    family: String,
    content_hash: String,
    byte_len: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalFontResponse {
    stack_hash: String,
    faces: Vec<TerminalFontFace>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalFontChunk {
    content_hash: String,
    offset: usize,
    byte_len: usize,
    data_base64: String,
    eof: bool,
}

fn normalize_families(families: Vec<String>) -> Result<Vec<String>, String> {
    let mut unique = Vec::new();
    let is_color_emoji = |family: &&String| {
        matches!(
            family
                .trim()
                .trim_matches(['\'', '"'])
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "segoe ui emoji" | "apple color emoji" | "noto color emoji"
        )
    };
    for family in families
        .iter()
        .filter(|family| is_color_emoji(family))
        .chain(families.iter().filter(|family| !is_color_emoji(family)))
    {
        let family = family.trim().trim_matches(['\'', '"']).trim();
        if family.is_empty() {
            continue;
        }
        if family.len() > 128 || family.contains('\0') {
            return Err("FONT_DATA_INVALID: invalid terminal font family".to_string());
        }
        if !unique
            .iter()
            .any(|seen: &String| seen.eq_ignore_ascii_case(family))
        {
            unique.push(family.to_string());
        }
        if unique.len() == MAX_FAMILIES - 1 {
            break;
        }
    }
    if !unique
        .iter()
        .any(|family| family.eq_ignore_ascii_case("monospace"))
    {
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

fn normalize_known_hashes(hashes: Vec<String>) -> Result<HashSet<String>, String> {
    if hashes.len() > MAX_FONT_FILES {
        return Err("FONT_DATA_INVALID: too many cached font hashes".to_string());
    }
    let mut result = HashSet::new();
    for hash in hashes {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("FONT_DATA_INVALID: invalid cached font hash".to_string());
        }
        result.insert(hash.to_ascii_lowercase());
    }
    Ok(result)
}

pub fn load_terminal_font_faces(
    families: Vec<String>,
    known_hashes: Option<Vec<String>>,
) -> Result<TerminalFontResponse, String> {
    let families = normalize_families(families)?;
    normalize_known_hashes(known_hashes.unwrap_or_default())?;
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
    let mut resolved_data = Vec::new();
    let mut stack_hasher = Sha256::new();
    for id in ids {
        let Some(data) = db.with_face_data(id, |bytes, _| bytes.to_vec()) else {
            continue;
        };
        let content_hash = format!("{:x}", Sha256::digest(&data));
        if !seen.insert(content_hash.clone()) {
            continue;
        }
        total = total.saturating_add(data.len());
        if data.is_empty()
            || data.len() > MAX_SINGLE_FONT_BYTES
            || result.len() >= MAX_FONT_FILES
            || total > MAX_FONT_BYTES
        {
            return Err("FONT_DATA_LIMIT: selected terminal font stack is too large".to_string());
        }
        stack_hasher.update(content_hash.as_bytes());
        let family = db
            .face(id)
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        result.push(TerminalFontFace {
            family,
            byte_len: data.len(),
            content_hash: content_hash.clone(),
        });
        resolved_data.push((content_hash, Arc::<[u8]>::from(data)));
    }
    if result.is_empty() {
        return Err(format!(
            "FONT_DATA_MISSING: no installed face matched {}",
            families.join(", ")
        ));
    }

    let new_bytes = resolved_data
        .iter()
        .map(|(_, data)| data.len())
        .sum::<usize>();
    let mut cache = terminal_font_cache()
        .lock()
        .map_err(|_| "FONT_DATA_IO: Host font cache lock is poisoned".to_string())?;
    if cache.byte_len.saturating_add(new_bytes) > MAX_FONT_CACHE_BYTES {
        cache.faces.clear();
        cache.byte_len = 0;
    }
    for (hash, data) in resolved_data {
        if let Some(previous) = cache.faces.insert(hash, Arc::clone(&data)) {
            cache.byte_len = cache.byte_len.saturating_sub(previous.len());
        }
        cache.byte_len = cache.byte_len.saturating_add(data.len());
    }
    Ok(TerminalFontResponse {
        stack_hash: format!("{:x}", stack_hasher.finalize()),
        faces: result,
    })
}

pub fn read_terminal_font_face_chunk(
    content_hash: String,
    offset: usize,
    length: usize,
) -> Result<TerminalFontChunk, String> {
    let content_hash = content_hash.to_ascii_lowercase();
    if content_hash.len() != 64
        || !content_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || length == 0
        || length > MAX_FONT_CHUNK_BYTES
    {
        return Err("FONT_DATA_INVALID: invalid Host font chunk request".to_string());
    }
    let cache = terminal_font_cache()
        .lock()
        .map_err(|_| "FONT_DATA_IO: Host font cache lock is poisoned".to_string())?;
    let data = cache
        .faces
        .get(&content_hash)
        .ok_or_else(|| "FONT_DATA_MISSING: Host font face cache expired".to_string())?;
    if offset >= data.len() {
        return Err("FONT_DATA_INVALID: Host font chunk offset is out of range".to_string());
    }
    let end = offset.saturating_add(length).min(data.len());
    Ok(TerminalFontChunk {
        content_hash,
        offset,
        byte_len: end - offset,
        data_base64: base64::engine::general_purpose::STANDARD.encode(&data[offset..end]),
        eof: end == data.len(),
    })
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
    fn retains_color_emoji_in_a_long_fallback_stack() {
        let mut families = (0..40)
            .map(|index| format!("Fallback {index}"))
            .collect::<Vec<_>>();
        families.push("Segoe UI Emoji".into());
        let normalized = normalize_families(families).unwrap();
        assert!(normalized.iter().any(|family| family == "Segoe UI Emoji"));
        assert!(normalized.iter().any(|family| family == "monospace"));
    }

    #[test]
    fn rejects_untrusted_inputs() {
        assert!(normalize_families(vec!["bad\0font".into()]).is_err());
        assert!(normalize_families(vec!["x".repeat(129)]).is_err());
        assert!(normalize_known_hashes(vec!["not-a-sha256".into()]).is_err());
        assert!(normalize_known_hashes(vec!["a".repeat(64)]).is_ok());
        assert!(read_terminal_font_face_chunk("x".into(), 0, 1).is_err());
        assert!(read_terminal_font_face_chunk("a".repeat(64), 0, 0).is_err());
        assert!(
            read_terminal_font_face_chunk("a".repeat(64), 0, MAX_FONT_CHUNK_BYTES + 1).is_err()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolves_segoe_ui_emoji_from_the_host() {
        let response =
            load_terminal_font_faces(vec!["Cascadia Mono".into(), "Segoe UI Emoji".into()], None)
                .unwrap();
        assert!(response
            .faces
            .iter()
            .any(|face| face.family.eq_ignore_ascii_case("Segoe UI Emoji")));
    }
}
