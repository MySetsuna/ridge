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
use crate::fs::search::{SearchEngine, SearchOptions, SearchResult};
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
