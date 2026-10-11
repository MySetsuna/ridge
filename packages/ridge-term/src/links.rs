//! Logical terminal links. Detection and byte-to-cell projection share the
//! kernel's grapheme and authoritative soft-wrap data on every surface.

use crate::term::{cell::Row, Terminal};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSpan {
    pub row: usize,
    pub c0: usize,
    pub c1: usize,
    pub text: String,
    pub kind: &'static str,
    pub link_id: String,
}

fn trim_target(mut text: &str) -> &str {
    loop {
        let Some(last) = text.chars().last() else { return text; };
        let unmatched = match last {
            ')' => text.matches(')').count() > text.matches('(').count(),
            ']' => text.matches(']').count() > text.matches('[').count(),
            '}' => text.matches('}').count() > text.matches('{').count(),
            _ => false,
        };
        if unmatched || matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '>') {
            text = &text[..text.len() - last.len_utf8()];
        } else {
            return text;
        }
    }
}

fn known_extension(text: &str) -> bool {
    let Some((_, ext)) = text.rsplit_once('.') else { return false; };
    matches!(ext.to_ascii_lowercase().as_str(),
        "md" | "markdown" | "mdx" | "txt" | "log" | "json" | "jsonc" |
        "toml" | "yaml" | "yml" | "ts" | "tsx" | "js" | "jsx" | "mjs" |
        "cjs" | "svelte" | "vue" | "astro" | "rs" | "py" | "go" | "java" |
        "kt" | "swift" | "rb" | "php" | "lua" | "cs" | "c" | "h" | "cpp" |
        "hpp" | "css" | "scss" | "less" | "html" | "htm" | "sh" | "bash" |
        "zsh" | "ps1" | "cmd" | "bat" | "lock" | "env" | "cfg" | "ini" |
        "conf" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" |
        "bmp" | "pdf" | "zip" | "tar" | "gz")
}

fn without_location(mut text: &str) -> &str {
    for _ in 0..2 {
        let Some((path, suffix)) = text.rsplit_once(':') else { break; };
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) { break; }
        text = path;
    }
    text
}

fn path_candidate(text: &str) -> bool {
    let path = without_location(text);
    if path.is_empty() || path.contains("://") || path.contains('@') ||
        path.chars().any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '|' | '?' | '*' | '`')) {
        return false;
    }
    let drive = path.as_bytes().get(1) == Some(&b':') &&
        path.as_bytes()[0].is_ascii_alphabetic() &&
        path.as_bytes().get(2).is_some_and(|b| matches!(b, b'/' | b'\\'));
    if !drive && path.contains(':') { return false; }
    if drive || path.starts_with("./") || path.starts_with("../") ||
        path.starts_with(".\\") || path.starts_with("..\\") ||
        path.starts_with("~/") || path.starts_with("~\\") { return true; }
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if known_extension(file) { return true; }
    if path.starts_with('/') { return path[1..].contains('/'); }
    if path.ends_with(['/', '\\']) { return true; }
    // A slash alone cannot distinguish directories from ratios/MIME/prose.
    // Explicit relative/absolute paths remain available for arbitrary names.
    matches!(path.split(['/', '\\']).next().unwrap_or_default(),
        "src" | "lib" | "app" | "packages" | "docs" | "tests" | "scripts" |
        "assets" | "static" | "build" | "dist" | "node_modules") &&
        path.contains(['/', '\\'])
}

fn scan(text: &str) -> Vec<(usize, usize, &'static str)> {
    let mut links = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let ch = text[offset..].chars().next().unwrap();
        if ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '<' | '=' | '"' | '\'') {
            if matches!(ch, '"' | '\'') {
                let start = offset + ch.len_utf8();
                if let Some(end) = text[start..].find(ch).map(|n| start + n) {
                    let target = &text[start..end];
                    let kind = if is_safe_http_uri(target) { Some("url") }
                        else if path_candidate(target) { Some("path") } else { None };
                    if let Some(kind) = kind { links.push((start, end, kind)); }
                    offset = end + ch.len_utf8();
                    continue;
                }
            }
            offset += ch.len_utf8();
            continue;
        }
        let start = offset;
        offset += text[offset..].find(|ch: char| ch.is_whitespace() ||
            matches!(ch, '<' | '>' | '"' | '\'' | '`' | '|')).unwrap_or(text.len() - offset);
        let target = trim_target(&text[start..offset]);
        let kind = if is_safe_http_uri(target) { Some("url") }
            else if path_candidate(target) { Some("path") } else { None };
        if let Some(kind) = kind { links.push((start, start + target.len(), kind)); }
        // Delimiters not handled above must still make progress.
        if offset == start { offset += ch.len_utf8(); }
    }
    links
}

fn byte_to_cell(row: &Row, offset: usize) -> usize {
    let mut bytes = 0;
    for (col, cell) in row.cells.iter().enumerate() {
        if cell.width == 0 { continue; }
        if offset == bytes { return col; }
        bytes += row.cluster_at(col).map_or(cell.ch.len_utf8(), |cluster| cluster.text.len());
        if offset < bytes { return col; }
    }
    row.cells.len()
}

fn project(terminal: &Terminal, rows: &[(isize, String, bool)], spans: &mut Vec<LinkSpan>) {
    let text: String = rows.iter().map(|row| row.1.as_str()).collect();
    let visible_start = terminal.scrollback_len().saturating_sub(terminal.scroll_offset());
    for (start, end, kind) in scan(&text) {
        let link_id = format!("{}:{start}:{kind}", rows[0].0);
        let mut offset = 0;
        for (index, fragment, _) in rows {
            let c0 = start.saturating_sub(offset);
            let c1 = end.saturating_sub(offset).min(fragment.len());
            if *index >= 0 && (*index as usize) < terminal.rows() && c1 > c0 {
                if let Some(row) = terminal.row_at_abs(visible_start + *index as usize) {
                    spans.push(LinkSpan { row: *index as usize,
                        c0: byte_to_cell(row, c0), c1: byte_to_cell(row, c1),
                        text: text[start..end].to_owned(), kind, link_id: link_id.clone() });
                }
            }
            offset += fragment.len();
        }
    }
}

pub fn detect(terminal: &Terminal) -> Vec<LinkSpan> {
    let rows = terminal.wrapped_link_rows();
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, row) in rows.iter().enumerate() {
        if !row.2 { project(terminal, &rows[start..=index], &mut spans); start = index + 1; }
    }
    if start < rows.len() { project(terminal, &rows[start..], &mut spans); }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<LinkSpan> {
        let mut terminal = Terminal::new(4, 120, 20);
        terminal.feed(text.as_bytes());
        detect(&terminal)
    }

    #[test]
    fn ambiguous_prose_ratios_mime_and_unsupported_uris_are_inert() {
        for text in ["and/or application/json 1/2", "/help --output=/tmp", "value / count",
            "file:///C:/repo/main.ts", "mailto:user@example.com", "https://user:secret@example.com/x"] {
            assert!(links(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn balanced_url_punctuation_survives_and_sentence_punctuation_does_not() {
        let spans = links("see (https://example.org/a_(b)).");
        assert_eq!(spans[0].text, "https://example.org/a_(b)");
        assert_eq!(spans[0].c0, 5);
        assert_eq!(links("https://example.org/?q=x")[0].text, "https://example.org/?q=x");
    }

    #[test]
    fn explicit_paths_keep_quoted_spaces_and_locations() {
        for text in ["C:\\repo\\main.ts:12:3", "/home/u/main.rs:9:2", "./src/main.ts:4",
            "src/components", "\"C:\\My Project\\main.ts:7:2\"", "'../My Project/main.rs:8'"] {
            let spans = links(text);
            assert_eq!(spans.len(), 1, "{text}");
            assert_eq!(spans[0].text, text.trim_matches(['\'', '"']));
            assert_eq!(spans[0].kind, "path");
        }
    }

    #[test]
    fn wide_prefix_and_soft_wrap_use_real_cells_and_complete_targets() {
        let mut terminal = Terminal::new(2, 24, 100);
        terminal.feed("中文 https://example.com/long/path".as_bytes());
        let spans = detect(&terminal);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].c0, 5);
        assert_eq!(spans[0].c1, 24);
        assert_eq!(spans[1].c0, 0);
        assert_eq!(spans[1].text, "https://example.com/long/path");
        assert_eq!(spans[0].link_id, spans[1].link_id);
        terminal.feed(b"\r\nnext");
        assert!(detect(&terminal).iter().all(|span| span.text == "https://example.com/long/path"));
        terminal.scroll_up_view(1);
        assert_eq!(detect(&terminal)[0].c0, 5);
    }

    #[test]
    fn hard_breaks_do_not_join_and_wrap_punctuation_is_not_trimmed_per_fragment() {
        let mut terminal = Terminal::new(3, 23, 100);
        terminal.feed(b"https://example.com/foo,bar");
        let spans = detect(&terminal);
        assert_eq!(spans[0].c1, 23);
        assert_eq!(spans[1].text, "https://example.com/foo,bar");
        terminal.feed(b"\r\nhttps://example.org");
        assert!(detect(&terminal).iter().any(|span| span.text == "https://example.org"));
    }
}

pub fn is_safe_http_uri(uri: &str) -> bool {
    if uri.is_empty()
        || uri.len() > 2_048
        || uri.contains('\\')
        || uri.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return false;
    }
    let Some(rest) = uri
        .strip_prefix("https://")
        .or_else(|| uri.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    if let Some(ipv6) = authority.strip_prefix('[') {
        let Some(close) = ipv6.find(']') else {
            return false;
        };
        if close == 0 {
            return false;
        }
        let tail = &ipv6[close + 1..];
        return tail.is_empty()
            || tail.strip_prefix(':').is_some_and(|port| {
                !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())
            });
    }
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    !host.is_empty()
        && !host.contains(':')
        && port.map_or(true, |port| {
            !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())
        })
}

