//! R17-CTX: scan workspace root for agent convention files (AGENTS.md / CLAUDE.md).
//! Pure filesystem read — no prompt injection into remote; caller decides how to use.

use std::path::{Path, PathBuf};

/// Filenames scanned in order (first match per name wins at root only).
pub const CONTEXT_FILENAMES: &[&str] = &["AGENTS.md", "CLAUDE.md", "Agents.md", "Claude.md"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFile {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
}

/// Read known convention files from `root` (non-recursive). Missing files skipped.
pub fn scan_context_files(root: &Path) -> Vec<ContextFile> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in CONTEXT_FILENAMES {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        // Dedup case-insensitive on Windows (AGENTS.md vs Agents.md same file).
        let key = name.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => out.push(ContextFile {
                name: (*name).to_string(),
                path,
                content,
            }),
            Err(_) => continue,
        }
    }
    out
}

/// Concatenate scanned files into a single prompt block (empty if none).
pub fn format_context_block(files: &[ContextFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut s = String::from("## Workspace agent conventions\n\n");
    for f in files {
        s.push_str("### ");
        s.push_str(&f.name);
        s.push_str("\n\n");
        s.push_str(f.content.trim());
        s.push_str("\n\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_empty_root() {
        let dir = std::env::temp_dir().join(format!("ridge-ctx-empty-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        assert!(scan_context_files(&dir).is_empty());
        assert!(format_context_block(&[]).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_reads_agents_and_formats() {
        let dir = std::env::temp_dir().join(format!("ridge-ctx-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AGENTS.md"), "rule: no secrets").unwrap();
        fs::write(dir.join("CLAUDE.md"), "style: terse").unwrap();
        let files = scan_context_files(&dir);
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.content.contains("no secrets")));
        let block = format_context_block(&files);
        assert!(block.contains("AGENTS.md"));
        assert!(block.contains("no secrets"));
        assert!(block.contains("style: terse"));
        let _ = fs::remove_dir_all(&dir);
    }
}
