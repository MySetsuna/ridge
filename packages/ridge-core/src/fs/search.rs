//! Text / filename search + replace — migrated verbatim from
//! `src-tauri/src/fs/search.rs` (S5).
//!
//! Pure logic: the `ignore` crate's ripgrep-grade walker + `glob` + `regex`.
//! **Zero Tauri dependency** — the desktop host re-exports these types from
//! `src-tauri/src/fs/mod.rs`; the headless `ridge-cli` host reaches them
//! through `ridge_core::dispatch`. Behaviour is byte-for-byte identical to the
//! pre-migration desktop module.

use glob::Pattern;
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

use super::tree::FileTree;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_text: Option<String>,
}

/// Carries one bad glob pattern back to the frontend. When `text_search`
/// returns these alongside the regular results the UI can decorate the
/// offending input field (red ring + tooltip) the way VS Code does for
/// invalid `files.exclude` entries — instead of the previous silent-drop
/// behaviour where a `[unclosed` typo just made the whole filter pretend
/// nothing matched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidGlob {
    /// The raw pattern the user typed.
    pub pattern: String,
    /// Best-effort error message from `glob::Pattern::new`.
    pub error: String,
    /// `"include"` or `"exclude"` — lets the UI surface the right field.
    pub field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub whole_word: bool,
    pub include_hidden: bool,
    pub max_results: usize,
    /// Optional include globs (e.g. `**/*.ts`). When non-empty, only files
    /// whose absolute path matches at least one pattern are searched. Empty
    /// means "no include filter" (matches everything).
    #[serde(default)]
    pub include_globs: Vec<String>,
    /// Optional exclude globs (e.g. `**/dist/**`). Files matching any pattern
    /// are skipped before any IO is performed. Applied after `include_globs`.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            use_regex: false,
            whole_word: false,
            include_hidden: false,
            max_results: 1000,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
        }
    }
}

pub struct SearchEngine;

fn compile_globs(patterns: &[String], field: &'static str) -> (Vec<Pattern>, Vec<InvalidGlob>) {
    let mut compiled = Vec::with_capacity(patterns.len());
    let mut invalid = Vec::new();
    for pattern in patterns {
        match Pattern::new(pattern) {
            Ok(value) => compiled.push(value),
            Err(error) => invalid.push(InvalidGlob {
                pattern: pattern.clone(),
                error: error.to_string(),
                field: field.to_string(),
            }),
        }
    }
    (compiled, invalid)
}

fn matches_glob(path: &str, forward_path: &str, glob: &Pattern) -> bool {
    glob.matches(path) || glob.matches(forward_path)
}

fn append_file_matches(
    path: &Path,
    pattern: &Regex,
    max_results: usize,
    results: &mut Vec<SearchResult>,
) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    for (line_idx, line) in content.lines().enumerate() {
        let Some(captures) = pattern.find(line) else {
            continue;
        };
        results.push(SearchResult {
            file: path.to_string_lossy().to_string(),
            line: line_idx + 1,
            column: captures.start() + 1,
            content: line.to_string(),
            match_text: Some(captures.as_str().to_string()),
        });
        if results.len() >= max_results {
            return true;
        }
    }
    false
}

fn search_file_candidate(
    root: &Path,
    path: &Path,
    query: &str,
) -> Option<(String, String, String)> {
    if FileTree::should_ignore(path) || !path.is_file() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_lowercase();
    let absolute = path.to_string_lossy().replace('\\', "/").to_lowercase();
    (name.contains(query) || relative.contains(query) || absolute.contains(query))
        .then(|| (path.to_string_lossy().to_string(), name, relative))
}

fn search_relevance(item: &(String, String, String), query: &str) -> u8 {
    if item.1 == query {
        0
    } else if item.1.starts_with(query) {
        1
    } else if item.2 == query {
        2
    } else if item.2.starts_with(query) {
        3
    } else {
        4
    }
}

impl SearchEngine {
    /// Search for text in all files under a root directory.
    ///
    /// Bad include / exclude globs no longer get silently dropped — they're
    /// collected and returned via the new `search_text_with_globs` variant.
    /// The legacy entry point keeps "drop bad globs, return only matches"
    /// behaviour for backwards compat.
    pub fn search_text(root: &Path, query: &str, options: &SearchOptions) -> Vec<SearchResult> {
        let (results, _bad) = Self::search_text_with_globs(root, query, options);
        results
    }

    /// Variant that also reports invalid include / exclude globs so the
    /// frontend can decorate the input. Bad globs are still dropped from
    /// the active filter (a typo shouldn't error out the whole search and
    /// strand the user with no results), but their existence is signalled.
    pub fn search_text_with_globs(
        root: &Path,
        query: &str,
        options: &SearchOptions,
    ) -> (Vec<SearchResult>, Vec<InvalidGlob>) {
        let mut results = Vec::new();
        let mut bad_globs: Vec<InvalidGlob> = Vec::new();
        let pattern = match Self::build_pattern(query, options) {
            Ok(p) => p,
            Err(_) => return (results, bad_globs),
        };

        // Compile include / exclude globs once. Bad patterns get dropped
        // from the active filter (so a typo doesn't strand the user with
        // zero results) but are collected for caller surfacing.
        let (includes, mut invalid_includes) = compile_globs(&options.include_globs, "include");
        let (excludes, mut invalid_excludes) = compile_globs(&options.exclude_globs, "exclude");
        bad_globs.append(&mut invalid_includes);
        bad_globs.append(&mut invalid_excludes);

        // Walk directory — respects .gitignore, .git/info/exclude, and .ignore files
        // via the `ignore` crate (same engine as ripgrep). FileTree::should_ignore
        // is kept as a belt-and-suspenders fallback for the static SKIP_DIRS list.
        for entry in WalkBuilder::new(root)
            .follow_links(false)
            .hidden(!options.include_hidden) // hidden(true) = skip dot-files
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .require_git(false)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        {
            let path = entry.path();

            // Belt-and-suspenders: static SKIP_DIRS list (node_modules, target, etc.)
            if FileTree::should_ignore(path) {
                continue;
            }

            // Apply include/exclude globs against the absolute path string.
            // matches_path uses the OS separator on Windows; we also match
            // against a forward-slash-normalised version so users can write
            // `src/**/*.ts` cross-platform.
            let path_str = path.to_string_lossy();
            let path_fwd = path_str.replace('\\', "/");
            if !includes.is_empty()
                && !includes
                    .iter()
                    .any(|g| matches_glob(&path_str, &path_fwd, g))
            {
                continue;
            }
            if excludes
                .iter()
                .any(|g| matches_glob(&path_str, &path_fwd, g))
            {
                continue;
            }

            // Skip binary files (simple heuristic)
            if Self::is_binary(path) {
                continue;
            }

            // Search in file
            if append_file_matches(path, &pattern, options.max_results, &mut results) {
                return (results, bad_globs);
            }
        }

        (results, bad_globs)
    }

    /// Search for files matching a case-insensitive substring of either the
    /// basename or the root-relative path.  Including the relative path is
    /// important for Quick Open: `src/lib/term` and `lib/term` now narrow the
    /// result set instead of being treated as an impossible filename.
    pub fn search_files(root: &Path, pattern: &str) -> Vec<String> {
        let pattern_lower = pattern
            .trim()
            .replace('\\', "/")
            .trim_matches('/')
            .to_lowercase();
        let pattern_lower = pattern_lower
            .strip_prefix("./")
            .unwrap_or(&pattern_lower)
            .to_string();
        if pattern_lower.is_empty() {
            return Vec::new();
        }

        let mut matches: Vec<(String, String, String)> = Vec::new();

        for entry in WalkBuilder::new(root)
            .follow_links(false)
            .hidden(true) // skip dot-files/dirs
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .require_git(false)
            .max_depth(Some(10)) // Limit depth for performance
            .build()
            .filter_map(|e| e.ok())
        {
            if let Some(candidate) = search_file_candidate(root, entry.path(), &pattern_lower) {
                matches.push(candidate);
            }
        }

        // Sort by relevance (exact filename > filename prefix > path prefix >
        // path substring), then by the shorter relative path for stability.
        matches.sort_by(|a, b| {
            search_relevance(a, &pattern_lower)
                .cmp(&search_relevance(b, &pattern_lower))
                .then_with(|| a.2.len().cmp(&b.2.len()))
                .then_with(|| a.0.cmp(&b.0))
        });

        matches.truncate(100); // Limit results
        matches.into_iter().map(|(path, _, _)| path).collect()
    }

    /// Replace text in files
    pub fn replace_in_files(
        _root: &Path,
        search: &str,
        replace: &str,
        files: &[String],
        options: &SearchOptions,
    ) -> io::Result<ReplaceStats> {
        let pattern = Self::build_pattern(search, options)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let mut stats = ReplaceStats {
            files_processed: 0,
            files_modified: 0,
            replacements: 0,
            errors: Vec::new(),
        };

        for file_path in files {
            let path = Path::new(file_path);
            if !path.exists() || path.is_dir() {
                continue;
            }

            stats.files_processed += 1;

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    stats
                        .errors
                        .push(format!("Failed to read {}: {}", file_path, e));
                    continue;
                }
            };

            let new_content = if options.use_regex {
                pattern.replace_all(&content, replace).to_string()
            } else if options.case_sensitive {
                content.replace(search, replace)
            } else {
                Self::replace_case_insensitive(&content, search, replace)
            };

            if new_content != content {
                match fs::write(path, &new_content) {
                    Ok(_) => {
                        stats.files_modified += 1;
                        stats.replacements += 1;
                    }
                    Err(e) => {
                        stats
                            .errors
                            .push(format!("Failed to write {}: {}", file_path, e));
                    }
                }
            }
        }

        Ok(stats)
    }

    fn build_pattern(query: &str, options: &SearchOptions) -> Result<Regex, String> {
        if options.use_regex {
            let flags = if options.case_sensitive { "" } else { "(?i)" };
            Regex::new(&format!("{}{}", flags, query)).map_err(|e| format!("Invalid regex: {}", e))
        } else {
            let escaped = regex::escape(query);
            let pattern = if options.whole_word {
                format!(r"\b{}\b", escaped)
            } else {
                escaped
            };
            let flags = if options.case_sensitive { "" } else { "(?i)" };
            Regex::new(&format!("{}{}", flags, pattern))
                .map_err(|e| format!("Invalid regex: {}", e))
        }
    }

    fn is_binary(path: &Path) -> bool {
        let binary_extensions = [
            "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib", "png", "jpg", "jpeg",
            "gif", "bmp", "ico", "svg", "webp", "mp3", "mp4", "wav", "avi", "mov", "mkv", "webm",
            "zip", "tar", "gz", "rar", "7z", "xz", "pdf", "doc", "docx", "xls", "xlsx", "ppt",
            "pptx", "ttf", "otf", "woff", "woff2", "eot", "db", "sqlite", "sqlite3",
        ];

        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            return binary_extensions.contains(&ext_str.as_str());
        }

        // Check for shebang
        if let Ok(content) = fs::read_to_string(path) {
            if content.starts_with("#!") {
                return false;
            }
        }

        false
    }

    fn replace_case_insensitive(text: &str, search: &str, replace: &str) -> String {
        let search_lower = search.to_lowercase();
        let mut result = String::new();
        let mut remaining = text;

        while !remaining.is_empty() {
            let lower_remaining = remaining.to_lowercase();
            if let Some(pos) = lower_remaining.find(&search_lower) {
                result.push_str(&remaining[..pos]);
                result.push_str(replace);
                remaining = &remaining[pos + search.len()..];
            } else {
                result.push_str(remaining);
                break;
            }
        }

        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceStats {
    pub files_processed: usize,
    pub files_modified: usize,
    pub replacements: usize,
    pub errors: Vec<String>,
}
