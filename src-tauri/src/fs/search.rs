use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

pub struct SearchEngine;

impl SearchEngine {
    /// Search for text in all files under a root directory
    pub fn search_text(root: &Path, query: &str, options: &SearchOptions) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let pattern = Self::build_pattern(query, options);

        if pattern.is_err() {
            return results;
        }

        let pattern = pattern.unwrap();

        // Walk directory
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Skip ignored files
            if FileTree::should_ignore(path) {
                continue;
            }

            // Skip binary files (simple heuristic)
            if Self::is_binary(path) {
                continue;
            }

            // Search in file
            if let Ok(content) = fs::read_to_string(path) {
                for (line_idx, line) in content.lines().enumerate() {
                    if let Some(captures) = pattern.find(line) {
                        results.push(SearchResult {
                            file: path.to_string_lossy().to_string(),
                            line: line_idx + 1, // 1-indexed
                            column: captures.start() + 1,
                            content: line.to_string(),
                            match_text: Some(captures.as_str().to_string()),
                        });

                        if results.len() >= options.max_results {
                            return results;
                        }
                    }
                }
            }
        }

        results
    }

    /// Search for filenames matching a pattern
    pub fn search_files(root: &Path, pattern: &str) -> Vec<String> {
        let mut matches = Vec::new();
        let pattern_lower = pattern.to_lowercase();

        for entry in WalkDir::new(root)
            .follow_links(false)
            .max_depth(10) // Limit depth for performance
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if FileTree::should_ignore(path) {
                continue;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            // Simple fuzzy matching
            if name.contains(&pattern_lower) {
                matches.push(path.to_string_lossy().to_string());
            }
        }

        // Sort by relevance (exact match > starts with > contains)
        matches.sort_by(|a, b| {
            let a_name = a.to_lowercase();
            let b_name = b.to_lowercase();
            let a_exact = a_name == pattern_lower;
            let b_exact = b_name == pattern_lower;
            let a_starts = a_name.starts_with(&pattern_lower);
            let b_starts = b_name.starts_with(&pattern_lower);

            match (a_exact, b_exact) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => match (a_starts, b_starts) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a_name.len().cmp(&b_name.len()),
                },
            }
        });

        matches.truncate(100); // Limit results
        matches
    }

    /// Replace text in files
    pub fn replace_in_files(
        root: &Path,
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
                    stats.errors.push(format!("Failed to read {}: {}", file_path, e));
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
                        stats.errors.push(format!("Failed to write {}: {}", file_path, e));
                    }
                }
            }
        }

        Ok(stats)
    }

    fn build_pattern(query: &str, options: &SearchOptions) -> Result<Regex, String> {
        if options.use_regex {
            let flags = if options.case_sensitive { "" } else { "(?i)" };
            Regex::new(&format!("{}{}", flags, query))
                .map_err(|e| format!("Invalid regex: {}", e))
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
            "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib",
            "png", "jpg", "jpeg", "gif", "bmp", "ico", "svg", "webp",
            "mp3", "mp4", "wav", "avi", "mov", "mkv", "webm",
            "zip", "tar", "gz", "rar", "7z", "xz",
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
            "ttf", "otf", "woff", "woff2", "eot",
            "db", "sqlite", "sqlite3",
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