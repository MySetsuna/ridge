//! 文件搜索 + 文件树（契约 §9：复用 `src-tauri/src/fs/{search,tree}.rs`）。
//!
//! TODO(reuse, 契约 §9/§11.D): 理想做法是 `path` 依赖 `ridge_lib` 并直接调用
//! `ridge_lib::fs::SearchEngine` / `ridge_lib::fs::FileTree`。但目前
//! `src-tauri/src/lib.rs` 把模块声明为私有（`mod fs;`），从外部 crate 不可见。
//! 需要的最小 `pub` 调整（已在交付报告中列出）：
//!   - `src-tauri/src/lib.rs`: `mod fs;` → `pub mod fs;`
//! 一旦上游放开可见性，删除本文件、改用 path 依赖即可（签名已对齐）。
//!
//! 在那之前，这里用与上游**完全相同的纯依赖**（`ignore` = ripgrep walker、
//! `glob`、`regex`）做一份签名兼容的薄复刻，不引入任何新算法。

use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 单条搜索命中（与 `ridge_lib::fs::SearchResult` 同形）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
}

/// 搜索选项（精简自上游 `SearchOptions`，保留远控需要的字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub whole_word: bool,
    pub include_hidden: bool,
    pub max_results: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            use_regex: false,
            whole_word: false,
            include_hidden: false,
            max_results: 1000,
        }
    }
}

/// 文件树节点（与 `ridge_lib::fs::FileNode` 兼容的精简形）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// 永远忽略的 OS 垃圾文件（对齐上游 `FileTree::should_ignore`）。
fn should_ignore(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    matches!(name.as_str(), ".DS_Store" | "Thumbs.db")
}

fn build_pattern(query: &str, opts: &SearchOptions) -> Result<Regex, String> {
    let flags = if opts.case_sensitive { "" } else { "(?i)" };
    if opts.use_regex {
        Regex::new(&format!("{flags}{query}")).map_err(|e| format!("invalid regex: {e}"))
    } else {
        let escaped = regex::escape(query);
        let pat = if opts.whole_word {
            format!(r"\b{escaped}\b")
        } else {
            escaped
        };
        Regex::new(&format!("{flags}{pat}")).map_err(|e| format!("invalid regex: {e}"))
    }
}

fn is_binary(path: &Path) -> bool {
    const BIN: &[&str] = &[
        "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib", "png", "jpg", "jpeg", "gif",
        "bmp", "ico", "webp", "mp3", "mp4", "wav", "avi", "mov", "mkv", "webm", "zip", "tar", "gz",
        "rar", "7z", "xz", "pdf", "ttf", "otf", "woff", "woff2", "eot", "db", "sqlite", "sqlite3",
    ];
    path.extension()
        .map(|e| BIN.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// ripgrep 级文本搜索：尊重 `.gitignore` / `.ignore`（`ignore` crate，与上游同引擎）。
pub fn search_text(root: &Path, query: &str, opts: &SearchOptions) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let pattern = match build_pattern(query, opts) {
        Ok(p) => p,
        Err(_) => return results,
    };

    for entry in WalkBuilder::new(root)
        .follow_links(false)
        .hidden(!opts.include_hidden)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .require_git(false)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map_or(false, |ft| ft.is_file()))
    {
        let path = entry.path();
        if should_ignore(path) || is_binary(path) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            for (idx, line) in content.lines().enumerate() {
                if let Some(m) = pattern.find(line) {
                    results.push(SearchResult {
                        file: path.to_string_lossy().to_string(),
                        line: idx + 1,
                        column: m.start() + 1,
                        content: line.to_string(),
                    });
                    if results.len() >= opts.max_results {
                        return results;
                    }
                }
            }
        }
    }
    results
}

/// 列出目录的一层子项（目录优先，字母序）。文件树的远控最小面。
pub fn list_dir(path: &Path) -> std::io::Result<Vec<FileNode>> {
    let mut out: Vec<FileNode> = Vec::new();
    for entry in std::fs::read_dir(path)?.flatten() {
        let p = entry.path();
        if should_ignore(&p) {
            continue;
        }
        out.push(FileNode {
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            path: p.to_string_lossy().to_string(),
            is_dir: p.is_dir(),
        });
    }
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static C: AtomicUsize = AtomicUsize::new(0);

    fn tmp(tag: &str) -> std::path::PathBuf {
        let n = C.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("ridge-cli-fs-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn search_finds_literal_match() {
        let dir = tmp("search");
        std::fs::write(dir.join("a.txt"), "hello world\nfoo bar\n").unwrap();
        let opts = SearchOptions::default();
        let hits = search_text(&dir, "foo", &opts);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].column, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_dir_sorts_dirs_first() {
        let dir = tmp("list");
        std::fs::write(dir.join("z.txt"), "").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let entries = list_dir(&dir).unwrap();
        assert_eq!(entries[0].name, "sub");
        assert!(entries[0].is_dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
