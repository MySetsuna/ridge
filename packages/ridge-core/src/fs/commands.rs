//! Read-only filesystem command handlers (S5).
//!
//! Each function is a Tauri-free, line-for-line port of the matching read-only
//! `#[tauri::command]` body in `src-tauri/src/commands/project.rs`. They are
//! **synchronous and pure** (no host state, no `AppState`): the only inputs are
//! the deserialized args, the only outputs are a value or a [`CoreError`].
//!
//! The desktop host keeps its async `#[tauri::command]` wrappers (which offload
//! to `tokio::spawn_blocking` and own the FS semaphore) and now delegate their
//! body to these — so desktop behaviour is byte-for-byte unchanged. The headless
//! `ridge-cli` host reaches them through [`crate::dispatch::dispatch`].
//!
//! Error mapping: the legacy commands returned `Result<T, String>` with specific
//! Chinese / English messages. Those exact strings are preserved by wrapping
//! them in [`CoreError::Internal`] / [`CoreError::Io`] (both render the bare
//! message through `to_command_string`), so the LAN WS `{_error}` envelope is
//! identical to before.

use std::path::PathBuf;

use crate::error::{CoreError, CoreResult};
use crate::fs::search::{InvalidGlob, ReplaceStats, SearchEngine, SearchOptions, SearchResult};
use crate::fs::tree::{DirectoryPage, FileNode, FileTree};

/// Default lazy-load depth for the Explorer's initial tree request. Just the
/// root + its direct children; descendants load on first expand via
/// `get_directory_children`. (Matches `project.rs::DEFAULT_TREE_DEPTH`.)
pub const DEFAULT_TREE_DEPTH: usize = 1;
/// Default page size for `get_directory_children`. (Matches
/// `project.rs::DEFAULT_CHILDREN_PAGE_SIZE`.)
pub const DEFAULT_CHILDREN_PAGE_SIZE: usize = 200;

/// Editor read ceiling — files larger than this are refused. (Matches the
/// `const MAX` in `project.rs::read_file_for_editor`.)
const EDITOR_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Normalise a frontend-supplied path into the native form, fixing Windows
/// `C:/a/b\c` slash mixing where `PathBuf::exists()` can spuriously return
/// false, and trimming trailing separators + whitespace.
///
/// Ported verbatim from `project.rs::normalize_path_input`.
pub fn normalize_path_input(input: &str) -> PathBuf {
    let trimmed = input.trim().trim_end_matches(['/', '\\']);
    #[cfg(windows)]
    {
        // Unify on backslash; Windows API accepts both, but mixing trips some
        // of Rust stdlib's internal path comparisons.
        let mut s = trimmed.replace('/', "\\");
        // A drive root (`C:/` / `C:\`) degrades to `C:` after trimming the
        // trailing separator; a bare `C:` is NOT "the C drive root" on Windows
        // — it is "the process's most recent cwd on C", so `read_dir` would
        // read Ridge's own working dir. Restore the separator.
        if s.len() == 2 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1] == b':' {
            s.push('\\');
        }
        PathBuf::from(s)
    }
    #[cfg(not(windows))]
    {
        // Same guard for the POSIX root `/`: trim_end_matches would empty it.
        if trimmed.is_empty() && input.contains('/') {
            PathBuf::from("/")
        } else {
            PathBuf::from(trimmed)
        }
    }
}

/// `get_file_tree`: build a depth-bounded file tree rooted at `path`.
///
/// Mirrors `project.rs::get_file_tree`: normalise → exists → is_dir →
/// `FileTree::build(root, depth.unwrap_or(DEFAULT_TREE_DEPTH))`. Error strings
/// preserved exactly.
pub fn get_file_tree(path: &str, depth: Option<usize>) -> CoreResult<FileNode> {
    let root = normalize_path_input(path);
    if !root.exists() {
        return Err(CoreError::Internal(format!(
            "Path does not exist: {}",
            root.display()
        )));
    }
    if !root.is_dir() {
        return Err(CoreError::Internal(format!(
            "Path is not a directory: {}",
            root.display()
        )));
    }
    let max_depth = depth.unwrap_or(DEFAULT_TREE_DEPTH);
    FileTree::build(&root, max_depth)
        .map_err(|e| CoreError::Internal(format!("Failed to build file tree: {}", e)))
}

/// `get_directory_children`: one page of a directory's children.
///
/// Mirrors `project.rs::get_directory_children`.
pub fn get_directory_children(
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> CoreResult<DirectoryPage> {
    let dir = normalize_path_input(path);
    if !dir.exists() {
        return Err(CoreError::Internal(format!(
            "Path does not exist: {}",
            dir.display()
        )));
    }
    if !dir.is_dir() {
        return Err(CoreError::Internal(format!(
            "Path is not a directory: {}",
            dir.display()
        )));
    }
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_CHILDREN_PAGE_SIZE);
    FileTree::page_children(&dir, offset, limit)
        .map_err(|e| CoreError::Internal(format!("Failed to get directory contents: {}", e)))
}

/// `read_file`: read a UTF-8 file's full text.
///
/// Mirrors `project.rs::read_file` (note: NO path normalisation — the legacy
/// command used `PathBuf::from(&path)` directly, preserved here).
pub fn read_file(path: &str) -> CoreResult<String> {
    let file_path = PathBuf::from(path);
    if !file_path.exists() {
        return Err(CoreError::Internal("File does not exist".to_string()));
    }
    if !file_path.is_file() {
        return Err(CoreError::Internal("Path is not a file".to_string()));
    }
    std::fs::read_to_string(&file_path)
        .map_err(|e| CoreError::Internal(format!("Failed to read file: {}", e)))
}

/// `path_exists`: whether the normalised path exists. Infallible (matches the
/// legacy `Ok(p.exists())`).
pub fn path_exists(path: &str) -> bool {
    normalize_path_input(path).exists()
}

/// Result of [`read_file_for_editor`]. Same shape as
/// `project.rs::ReadFileForEditorResult` so the wire JSON is identical.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadFileForEditorResult {
    pub content: String,
    pub is_binary: bool,
    pub size: u64,
}

/// `read_file_for_editor`: read a file for the editor with binary detection
/// (NULL-byte / control-char heuristic) and a 5 MB ceiling, returning UTF-8
/// lossy content.
///
/// Mirrors `project.rs::read_file_for_editor` (the body inside its
/// `spawn_blocking`). Error strings preserved.
pub fn read_file_for_editor(path: &str) -> CoreResult<ReadFileForEditorResult> {
    let file_path = PathBuf::from(path);
    if !file_path.exists() {
        return Err(CoreError::Internal("File does not exist".to_string()));
    }
    if !file_path.is_file() {
        return Err(CoreError::Internal("Path is not a file".to_string()));
    }

    let metadata = std::fs::metadata(&file_path).map_err(|e| CoreError::Internal(e.to_string()))?;
    let size = metadata.len();
    if size > EDITOR_MAX_BYTES {
        return Err(CoreError::Internal(format!(
            "文件过大 ({:.2} MB)，编辑器上限 5 MB",
            size as f64 / 1024.0 / 1024.0
        )));
    }

    let bytes = std::fs::read(&file_path).map_err(|e| CoreError::Internal(e.to_string()))?;
    let probe = &bytes[..bytes.len().min(8192)];
    let has_null = probe.contains(&0);
    let non_text = probe
        .iter()
        .filter(|&&b| b < 0x09 || (b > 0x0D && b < 0x20))
        .count();
    let ratio = if probe.is_empty() {
        0.0
    } else {
        non_text as f64 / probe.len() as f64
    };
    let is_binary = has_null || ratio > 0.30;

    if is_binary {
        return Ok(ReadFileForEditorResult {
            content: String::new(),
            is_binary: true,
            size,
        });
    }

    let content = String::from_utf8_lossy(&bytes).into_owned();
    Ok(ReadFileForEditorResult {
        content,
        is_binary: false,
        size,
    })
}

/// Arguments for [`search`], mirroring the optional params of
/// `project.rs::text_search`. All optionals default exactly as the legacy
/// command did.
#[derive(Debug, Default, Clone)]
pub struct TextSearchArgs {
    pub case_sensitive: Option<bool>,
    pub use_regex: Option<bool>,
    pub whole_word: Option<bool>,
    pub max_results: Option<usize>,
    pub include_globs: Option<Vec<String>>,
    pub exclude_globs: Option<Vec<String>>,
}

/// `search` (a.k.a. `text_search`): ripgrep-grade text search under `root`.
///
/// Mirrors `project.rs::text_search`: `PathBuf::from(root)` (no normalisation)
/// → exists check → build `SearchOptions` with the same defaults → run the
/// gitignore-aware walk. The `include_hidden` field is always `false` (matches
/// the legacy command, which hard-codes it).
pub fn search(root: &str, query: &str, args: &TextSearchArgs) -> CoreResult<Vec<SearchResult>> {
    let root_path = PathBuf::from(root);
    if !root_path.exists() {
        return Err(CoreError::Internal("Root path does not exist".to_string()));
    }

    let options = SearchOptions {
        case_sensitive: args.case_sensitive.unwrap_or(false),
        use_regex: args.use_regex.unwrap_or(false),
        whole_word: args.whole_word.unwrap_or(false),
        include_hidden: false,
        max_results: args.max_results.unwrap_or(1000),
        include_globs: args.include_globs.clone().unwrap_or_default(),
        exclude_globs: args.exclude_globs.clone().unwrap_or_default(),
    };

    Ok(SearchEngine::search_text(&root_path, query, &options))
}

/// `filename_search`: glob/substring match on file NAMES under `root`.
///
/// Ported verbatim from `project.rs::filename_search`: `PathBuf::from(root)`
/// (no normalisation) → exists check (same "Root path does not exist" string)
/// → `SearchEngine::search_files`. The host keeps the `spawn_blocking` offload.
pub fn filename_search(root: &str, pattern: &str) -> CoreResult<Vec<String>> {
    let root_path = PathBuf::from(root);
    if !root_path.exists() {
        return Err(CoreError::Internal("Root path does not exist".to_string()));
    }
    Ok(SearchEngine::search_files(&root_path, pattern))
}

/// `text_search_diagnostics`: parse-only validation of the include/exclude glob
/// inputs, returning ONLY the bad patterns (with which field they came from).
///
/// Ported verbatim from `project.rs::text_search_diagnostics` — microsecond-
/// cheap, runs without touching the filesystem, so the frontend can decorate the
/// glob inputs without re-running the whole search walk.
pub fn text_search_diagnostics(
    include_globs: Option<Vec<String>>,
    exclude_globs: Option<Vec<String>>,
) -> Vec<InvalidGlob> {
    use glob::Pattern;
    let mut bad: Vec<InvalidGlob> = Vec::new();
    for s in include_globs.unwrap_or_default() {
        if let Err(e) = Pattern::new(&s) {
            bad.push(InvalidGlob {
                pattern: s,
                error: e.to_string(),
                field: "include".to_string(),
            });
        }
    }
    for s in exclude_globs.unwrap_or_default() {
        if let Err(e) = Pattern::new(&s) {
            bad.push(InvalidGlob {
                pattern: s,
                error: e.to_string(),
                field: "exclude".to_string(),
            });
        }
    }
    bad
}

// ── Filesystem write commands (S1 ledger §2.1) ──
//
// Pure `std::fs` mutations ported verbatim from `project.rs` — including the
// exact Chinese error strings, which the desktop surfaces to the user via
// `alert()`. They keep the legacy `Result<T, String>` shape (rather than
// `CoreResult`) so the desktop wrappers return byte-identical errors with no
// mapping; the `dispatch` arms wrap the `String` in `CoreError::Internal`
// (which renders the bare message). The read-only gate + the sandbox / path-
// traversal guards in `dispatch` run BEFORE any of these execute.

/// Write `content` to `path` as UTF-8, creating parent dirs if missing.
/// Verbatim port of `project.rs::write_file_blocking`.
pub fn write_file(path: String, content: String) -> Result<(), String> {
    let file_path = PathBuf::from(&path);
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
    }
    std::fs::write(&file_path, content).map_err(|e| format!("写入文件失败: {}", e))
}

/// A single Monaco `IModelContentChange`. Offsets/lengths are **UTF-16 code
/// units** (JS string semantics), NOT bytes or Unicode scalar values.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range_offset: usize,
    pub range_length: usize,
    pub text: String,
}

/// Apply a sequence of Monaco content changes to a file (incremental save).
/// Verbatim port of the synchronous core of `project.rs::apply_file_edits`:
/// edits splice in UTF-16 space (re-encoding to UTF-8), applied in order; any
/// out-of-range edit errors so the caller falls back to a full `write_file`.
pub fn apply_file_edits(path: String, edits: Vec<TextEdit>) -> Result<(), String> {
    let content = std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))?;
    let mut units: Vec<u16> = content.encode_utf16().collect();
    for e in &edits {
        let start = e.range_offset;
        let end = e
            .range_offset
            .checked_add(e.range_length)
            .ok_or_else(|| "edit length overflow".to_string())?;
        if start > end || end > units.len() {
            return Err(format!(
                "edit out of range: {}..{} (len {})",
                start,
                end,
                units.len()
            ));
        }
        let repl: Vec<u16> = e.text.encode_utf16().collect();
        units.splice(start..end, repl);
    }
    let new_content = String::from_utf16(&units).map_err(|e| format!("UTF-16 解码失败: {}", e))?;
    std::fs::write(&path, new_content).map_err(|e| format!("写入文件失败: {}", e))
}

/// Rename / move a file or directory. Verbatim port of `project.rs::rename_path`.
pub fn rename_path(from: String, to: String) -> Result<(), String> {
    let from_path = PathBuf::from(&from);
    let to_path = PathBuf::from(&to);
    if !from_path.exists() {
        return Err(format!("路径不存在: {}", from));
    }
    if to_path.exists() {
        return Err(format!("目标已存在: {}", to));
    }
    std::fs::rename(&from_path, &to_path).map_err(|e| format!("重命名失败: {}", e))?;
    Ok(())
}

/// Delete a file or directory (recursively for directories). Verbatim port of
/// the synchronous core of `project.rs::delete_path`.
pub fn delete_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("路径不存在: {}", path));
    }
    let meta = std::fs::symlink_metadata(&target).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("删除目录失败: {}", e))?;
    } else {
        std::fs::remove_file(&target).map_err(|e| format!("删除文件失败: {}", e))?;
    }
    Ok(())
}

/// Create an empty file (fails if it exists; creates parent dirs). Verbatim port
/// of `project.rs::create_file`.
pub fn create_file(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if target.exists() {
        return Err(format!("文件已存在: {}", path));
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
        }
    }
    std::fs::write(&target, []).map_err(|e| format!("创建文件失败: {}", e))?;
    Ok(())
}

/// Create a directory (fails if it already exists). Verbatim port of
/// `project.rs::create_directory`.
pub fn create_directory(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if target.exists() {
        return Err(format!("目录已存在: {}", path));
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("创建目录失败: {}", e))?;
    Ok(())
}

/// Copy `from` → `to` (files + recursive directories; refuses overwrite unless
/// `overwrite=true`). Verbatim port of `project.rs::copy_path_sync`.
pub fn copy_path(from: String, to: String, overwrite: Option<bool>) -> Result<(), String> {
    let from_path = PathBuf::from(&from);
    let to_path = PathBuf::from(&to);
    if !from_path.exists() {
        return Err(format!("源路径不存在: {}", from));
    }
    let overwrite = overwrite.unwrap_or(false);
    if to_path.exists() && !overwrite {
        return Err(format!("目标已存在: {}", to));
    }
    if let Some(parent) = to_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
        }
    }
    let meta = std::fs::symlink_metadata(&from_path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.is_dir() {
        // Recursive copy via walkdir. Mirror the tree relative to `from_path`.
        std::fs::create_dir_all(&to_path).map_err(|e| format!("创建目标目录失败: {}", e))?;
        for entry in walkdir::WalkDir::new(&from_path).min_depth(1) {
            let entry = entry.map_err(|e| format!("遍历源目录失败: {}", e))?;
            let rel = entry
                .path()
                .strip_prefix(&from_path)
                .map_err(|e| format!("strip_prefix: {}", e))?;
            let target = to_path.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target)
                    .map_err(|e| format!("创建子目录失败 ({}): {}", target.display(), e))?;
            } else {
                if let Some(parent) = target.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("创建目标父目录失败 ({}): {}", parent.display(), e)
                        })?;
                    }
                }
                std::fs::copy(entry.path(), &target)
                    .map_err(|e| format!("复制文件失败 ({}): {}", target.display(), e))?;
            }
        }
    } else {
        std::fs::copy(&from_path, &to_path).map_err(|e| format!("复制失败: {}", e))?;
    }
    Ok(())
}

/// Move `from` → `to` (rename, falling back to copy+delete across filesystems).
/// Verbatim port of `project.rs::move_path_sync`.
pub fn move_path(from: String, to: String) -> Result<(), String> {
    let from_path = PathBuf::from(&from);
    let to_path = PathBuf::from(&to);
    if !from_path.exists() {
        return Err(format!("源路径不存在: {}", from));
    }
    if to_path.exists() {
        return Err(format!("目标已存在: {}", to));
    }
    if let Some(parent) = to_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
        }
    }
    if std::fs::rename(&from_path, &to_path).is_ok() {
        return Ok(());
    }
    // Cross-device fallback: copy then delete source.
    copy_path(from.clone(), to.clone(), Some(false))?;
    let meta = std::fs::symlink_metadata(&from_path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&from_path).map_err(|e| format!("删除源目录失败: {}", e))?;
    } else {
        std::fs::remove_file(&from_path).map_err(|e| format!("删除源文件失败: {}", e))?;
    }
    Ok(())
}

/// Replace text across the given `files` under `root`. Verbatim port of the
/// synchronous core of `project.rs::replace_in_files` (same `SearchOptions`
/// defaults: not whole-word, not hidden, unbounded results, no globs).
pub fn replace_in_files(
    root: String,
    search: String,
    replace: String,
    files: Vec<String>,
    case_sensitive: Option<bool>,
    use_regex: Option<bool>,
) -> Result<ReplaceStats, String> {
    let root_path = PathBuf::from(&root);
    if !root_path.exists() {
        return Err("Root path does not exist".to_string());
    }
    let options = SearchOptions {
        case_sensitive: case_sensitive.unwrap_or(false),
        use_regex: use_regex.unwrap_or(false),
        whole_word: false,
        include_hidden: false,
        max_results: usize::MAX,
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
    };
    SearchEngine::replace_in_files(&root_path, &search, &replace, &files, &options)
        .map_err(|e| format!("Replace failed: {}", e))
}

/// A directory-browse result: the resolved directory, its parent (if any), and
/// the immediate non-hidden subdirectories (sorted case-insensitively).
#[derive(Debug, serde::Serialize)]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub subdirs: Vec<String>,
}

/// Browse `path`'s immediate subdirectories + parent, for the save-workspace
/// directory picker. A non-existent input normalises to its nearest existing
/// ancestor; `None`/blank starts at the home dir. Verbatim port of
/// `ridge_file.rs::browse_directory`.
pub fn browse_directory(path: Option<String>) -> Result<DirListing, String> {
    let start = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
    };
    // Normalise: if the input does not exist, fall back to the nearest existing
    // ancestor.
    let mut cur = start.clone();
    while !cur.is_dir() {
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => {
                cur = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                break;
            }
        }
    }
    let parent = cur.parent().map(|p| p.to_string_lossy().to_string());
    let mut subdirs: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cur) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            // Filter hidden directories (Unix `.` prefix convention).
            if name_str.starts_with('.') {
                continue;
            }
            subdirs.push(name_str);
        }
    }
    subdirs.sort_by_key(|s| s.to_lowercase());
    Ok(DirListing {
        path: cur.to_string_lossy().to_string(),
        parent,
        subdirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir {
        path: PathBuf,
    }
    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
            let pid = std::process::id();
            let mut path = std::env::temp_dir();
            path.push(format!("ridge-core-fscmd-{tag}-{pid}-{n}"));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
        fn p(&self, rel: &str) -> String {
            self.path.join(rel).to_string_lossy().into_owned()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn get_file_tree_errors_on_missing_path() {
        let td = TempDir::new("tree-miss");
        let err = get_file_tree(&td.p("nope"), None).unwrap_err();
        assert_eq!(err.kind_tag(), "internal");
        assert!(err.to_command_string().contains("does not exist"));
    }

    #[test]
    fn get_file_tree_errors_on_file_target() {
        let td = TempDir::new("tree-file");
        std::fs::write(td.path.join("a.txt"), b"hi").unwrap();
        let err = get_file_tree(&td.p("a.txt"), None).unwrap_err();
        assert!(err.to_command_string().contains("not a directory"));
    }

    #[test]
    fn get_file_tree_builds_root() {
        let td = TempDir::new("tree-ok");
        std::fs::write(td.path.join("a.txt"), b"hi").unwrap();
        let node = get_file_tree(&td.p(""), Some(1)).unwrap();
        assert!(node.is_dir);
        assert!(node.children.is_some());
    }

    #[test]
    fn get_directory_children_pages() {
        let td = TempDir::new("children");
        for i in 0..5 {
            std::fs::write(td.path.join(format!("f{i}.txt")), b"").unwrap();
        }
        let page = get_directory_children(&td.p(""), Some(0), Some(3)).unwrap();
        assert_eq!(page.entries.len(), 3);
        assert_eq!(page.total, 5);
        assert!(page.has_more);
    }

    #[test]
    fn read_file_round_trips_and_validates() {
        let td = TempDir::new("read");
        std::fs::write(td.path.join("a.txt"), b"hello").unwrap();
        assert_eq!(read_file(&td.p("a.txt")).unwrap(), "hello");

        let err = read_file(&td.p("missing.txt")).unwrap_err();
        assert!(err.to_command_string().contains("does not exist"));
    }

    #[test]
    fn path_exists_reports_presence() {
        let td = TempDir::new("exists");
        std::fs::write(td.path.join("a.txt"), b"").unwrap();
        assert!(path_exists(&td.p("a.txt")));
        assert!(!path_exists(&td.p("nope.txt")));
    }

    #[test]
    fn read_file_for_editor_detects_text_and_binary() {
        let td = TempDir::new("editor");
        std::fs::write(td.path.join("t.txt"), b"plain text").unwrap();
        let r = read_file_for_editor(&td.p("t.txt")).unwrap();
        assert!(!r.is_binary);
        assert_eq!(r.content, "plain text");

        std::fs::write(td.path.join("b.bin"), [0u8, 1, 2, 3, 0]).unwrap();
        let rb = read_file_for_editor(&td.p("b.bin")).unwrap();
        assert!(rb.is_binary);
        assert!(rb.content.is_empty());
    }

    #[test]
    fn search_finds_literal_match() {
        let td = TempDir::new("search");
        std::fs::write(td.path.join("a.txt"), "hello world\nfoo bar\n").unwrap();
        let hits = search(&td.p(""), "foo", &TextSearchArgs::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].column, 1);
        assert_eq!(hits[0].match_text.as_deref(), Some("foo"));
    }

    #[test]
    fn search_errors_on_missing_root() {
        let td = TempDir::new("search-miss");
        let err = search(&td.p("nope"), "x", &TextSearchArgs::default()).unwrap_err();
        assert!(err.to_command_string().contains("Root path does not exist"));
    }
}
