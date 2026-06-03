use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// True when the path is matched by the cwd's `.gitignore` chain.
    /// `None` when the cwd is not inside a git repo (no ancestor `.git/` found),
    /// or when the field was not populated by this build path.
    /// Frontend renders the row grayed when true; behaviour stays interactive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ignored: Option<bool>,
    /// Total number of immediate entries in the directory, regardless of
    /// pagination. `None` for files. Reserved for future per-row hints —
    /// the authoritative count for paginated displays comes from
    /// [`DirectoryPage::total`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
}

/// One page of a directory's children. The IPC returns this from
/// `get_directory_children` so the frontend can render `limit` rows
/// at a time and append more on user-triggered "load more" clicks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryPage {
    pub entries: Vec<FileNode>,
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
}

/// Stack of `.gitignore` matchers covering the path being walked, plus
/// a flag for whether the start dir lives inside a git repo at all.
///
/// Per the gitignore spec, deeper rules override shallower ones, so the
/// stack is checked top-down (deepest first). Whitelist matches (`!pat`)
/// short-circuit before reaching shallower layers.
///
/// `in_git_repo` controls whether `matches` returns `Some(_)` or `None`;
/// outside a git repo we don't want to surface false negatives — the UI
/// then renders nothing grayed.
pub struct FileTreeContext {
    gitignore_stack: Vec<(PathBuf, Gitignore)>,
    in_git_repo: bool,
}

impl FileTreeContext {
    /// Build an initial context for `start_dir`:
    ///   1. Walk ancestors looking for `.git/` (file OR directory — covers
    ///      worktrees). First hit becomes the repo root.
    ///   2. From repo root down through every ancestor up to `start_dir`,
    ///      try to load each level's `.gitignore` and push it on the stack.
    ///
    /// Cost: bounded by repo depth (typically < 10). One `metadata` call
    /// per ancestor on the way up, one `read_to_string` per `.gitignore`
    /// found on the way down. No directory enumeration.
    pub fn for_path(start_dir: &Path) -> Self {
        let Some(repo_root) = find_git_root(start_dir) else {
            return Self {
                gitignore_stack: Vec::new(),
                in_git_repo: false,
            };
        };

        let mut stack: Vec<(PathBuf, Gitignore)> = Vec::new();
        // Repo root itself.
        if let Some(gi) = build_gitignore(&repo_root) {
            stack.push((repo_root.clone(), gi));
        }
        // Descend toward start_dir, pushing each intermediate `.gitignore`.
        if let Ok(rel) = start_dir.strip_prefix(&repo_root) {
            let mut current = repo_root.clone();
            for component in rel.components() {
                current.push(component);
                if current == repo_root {
                    continue;
                }
                if let Some(gi) = build_gitignore(&current) {
                    stack.push((current.clone(), gi));
                }
            }
        }

        Self {
            gitignore_stack: stack,
            in_git_repo: true,
        }
    }

    /// Push `dir/.gitignore` if it exists. Returns true when a matcher was
    /// added (so the caller knows to balance with `exit`).
    pub fn enter(&mut self, dir: &Path) -> bool {
        if !self.in_git_repo {
            return false;
        }
        // Skip the repo root re-push — `for_path` already loaded it.
        if let Some((top_anchor, _)) = self.gitignore_stack.last() {
            if top_anchor == dir {
                return false;
            }
        }
        if let Some(gi) = build_gitignore(dir) {
            self.gitignore_stack.push((dir.to_path_buf(), gi));
            true
        } else {
            false
        }
    }

    /// Pop the deepest matcher off the stack. No-op on an empty stack.
    pub fn exit(&mut self) {
        self.gitignore_stack.pop();
    }

    /// Return `Some(true)` if `path` is gitignored under the current stack,
    /// `Some(false)` if not, or `None` when the start dir was not inside a
    /// git repo (in which case nothing should be rendered grayed).
    pub fn matches(&self, path: &Path, is_dir: bool) -> Option<bool> {
        if !self.in_git_repo {
            return None;
        }
        for (anchor, gi) in self.gitignore_stack.iter().rev() {
            if !path.starts_with(anchor) {
                continue;
            }
            match gi.matched(path, is_dir) {
                ignore::Match::Ignore(_) => return Some(true),
                ignore::Match::Whitelist(_) => return Some(false),
                ignore::Match::None => continue,
            }
        }
        Some(false)
    }
}

fn find_git_root(start_dir: &Path) -> Option<PathBuf> {
    let mut current: Option<&Path> = Some(start_dir);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn build_gitignore(dir: &Path) -> Option<Gitignore> {
    let path = dir.join(".gitignore");
    if !path.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(dir);
    // `add` returns Option<Error> (None = success); on parse failure we
    // skip this layer rather than aborting the whole walk.
    if builder.add(&path).is_some() {
        return None;
    }
    builder.build().ok()
}

pub struct FileTree;

impl FileTree {
    /// Build a file tree from a root directory. Each node's `is_ignored`
    /// is populated relative to the cwd's gitignore chain (or `None` when
    /// the cwd is not inside a git repo).
    pub fn build(root: &Path, max_depth: usize) -> std::io::Result<FileNode> {
        let mut ctx = FileTreeContext::for_path(root);
        Self::build_recursive(root, 0, max_depth, &mut ctx)
    }

    fn build_recursive(
        path: &Path,
        current_depth: usize,
        max_depth: usize,
        ctx: &mut FileTreeContext,
    ) -> std::io::Result<FileNode> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let metadata = fs::metadata(path)?;
        let is_dir = metadata.is_dir();
        let is_ignored = ctx.matches(path, is_dir);

        if !is_dir {
            return Ok(FileNode {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: false,
                is_ignored,
                child_count: None,
                children: None,
                expanded: None,
            });
        }

        // Depth boundary — return the dir as a placeholder. Subsequent
        // on-demand loads via `page_children` build their own context.
        if current_depth >= max_depth {
            return Ok(FileNode {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: true,
                is_ignored,
                child_count: None,
                children: None,
                expanded: Some(false),
            });
        }

        // Descend. Push this dir's .gitignore (if any) so children see it.
        let pushed = ctx.enter(path);

        let mut children = Vec::new();
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if Self::should_ignore(&entry_path) {
                        continue;
                    }
                    match Self::build_recursive(&entry_path, current_depth + 1, max_depth, ctx) {
                        Ok(child) => children.push(child),
                        Err(e) => {
                            tracing::warn!("Failed to read {}: {}", entry_path.display(), e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read directory {}: {}", path.display(), e);
            }
        }

        if pushed {
            ctx.exit();
        }

        children.sort_by(|a, b| sort_entries(a.is_dir, &a.name, b.is_dir, &b.name));

        Ok(FileNode {
            name,
            path: path.to_string_lossy().to_string(),
            is_dir: true,
            is_ignored,
            child_count: Some(children.len()),
            children: Some(children),
            expanded: Some(current_depth < 2),
        })
    }

    /// OS-level garbage we never want to surface, regardless of what
    /// `.gitignore` says. Everything else — dotfiles, build outputs,
    /// gitignored entries — is shown so the user can inspect the cwd
    /// in full. `.gitignore` membership controls only the *visual*
    /// graying via `FileNode.is_ignored`.
    pub fn should_ignore(path: &Path) -> bool {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        matches!(name.as_str(), ".DS_Store" | "Thumbs.db")
    }

    /// Read a single page of `path`'s children: `limit` entries starting
    /// at `offset`. The directory is read once in full (so the total can
    /// be reported), filtered with `should_ignore`, sorted (dirs first,
    /// then alphabetic), and sliced. `is_ignored` is populated against a
    /// fresh context built for `path`.
    pub fn page_children(
        path: &Path,
        offset: usize,
        limit: usize,
    ) -> std::io::Result<DirectoryPage> {
        let ctx = FileTreeContext::for_path(path);

        let mut all: Vec<FileNode> = Vec::new();
        for entry in fs::read_dir(path)?.flatten() {
            let entry_path = entry.path();
            if Self::should_ignore(&entry_path) {
                continue;
            }
            let name = entry_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_dir = entry_path.is_dir();
            let is_ignored = ctx.matches(&entry_path, is_dir);

            all.push(FileNode {
                name,
                path: entry_path.to_string_lossy().to_string(),
                is_dir,
                is_ignored,
                child_count: None,
                children: None,
                expanded: None,
            });
        }

        all.sort_by(|a, b| sort_entries(a.is_dir, &a.name, b.is_dir, &b.name));

        let total = all.len();
        let start = offset.min(total);
        let end = start.saturating_add(limit).min(total);
        let has_more = end < total;
        let entries = if start == end {
            Vec::new()
        } else {
            all.drain(start..end).collect()
        };

        Ok(DirectoryPage {
            entries,
            total,
            offset: start,
            has_more,
        })
    }
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
            path.push(format!("ridge-tree-test-{tag}-{pid}-{n}"));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn should_ignore_keeps_dotfiles_and_buildouts() {
        // Pre-rewrite, this list was hidden. The new policy surfaces them
        // (the UI grays them via .gitignore matching, never hiding).
        for name in [
            ".git",
            "node_modules",
            "target",
            "dist",
            ".env",
            ".log",
            "build.exe",
            ".gitignore",
        ] {
            assert!(
                !FileTree::should_ignore(Path::new(name)),
                "should_ignore must NOT hide {name}"
            );
        }
    }

    #[test]
    fn should_ignore_blocks_only_os_garbage() {
        assert!(FileTree::should_ignore(Path::new(".DS_Store")));
        assert!(FileTree::should_ignore(Path::new("Thumbs.db")));
    }

    #[test]
    fn context_outside_git_repo_returns_none() {
        let td = TempDir::new("nogit");
        // No .git/ anywhere up the tree (TempDir's parent is OS temp).
        let ctx = FileTreeContext::for_path(&td.path);
        let entry = td.path.join("anything.log");
        std::fs::write(&entry, b"").unwrap();
        assert_eq!(ctx.matches(&entry, false), None);
    }

    #[test]
    fn context_inside_git_repo_marks_gitignored_entries() {
        let td = TempDir::new("git");
        // Fake git root: `.git/` directory satisfies `find_git_root`.
        std::fs::create_dir_all(td.path.join(".git")).unwrap();
        std::fs::write(td.path.join(".gitignore"), "*.log\nbuilt/\n").unwrap();
        std::fs::write(td.path.join("foo.log"), b"").unwrap();
        std::fs::write(td.path.join("foo.txt"), b"").unwrap();
        std::fs::create_dir_all(td.path.join("built")).unwrap();
        std::fs::create_dir_all(td.path.join("src")).unwrap();

        let ctx = FileTreeContext::for_path(&td.path);
        assert_eq!(ctx.matches(&td.path.join("foo.log"), false), Some(true));
        assert_eq!(ctx.matches(&td.path.join("foo.txt"), false), Some(false));
        assert_eq!(ctx.matches(&td.path.join("built"), true), Some(true));
        assert_eq!(ctx.matches(&td.path.join("src"), true), Some(false));
    }

    #[test]
    fn nested_gitignore_overrides_parent() {
        let td = TempDir::new("nested");
        std::fs::create_dir_all(td.path.join(".git")).unwrap();
        std::fs::write(td.path.join(".gitignore"), "*.log\n").unwrap();
        std::fs::create_dir_all(td.path.join("sub")).unwrap();
        // Subdir whitelists *.log.
        std::fs::write(td.path.join("sub/.gitignore"), "!*.log\n").unwrap();
        std::fs::write(td.path.join("sub/keep.log"), b"").unwrap();
        std::fs::write(td.path.join("root.log"), b"").unwrap();

        let ctx = FileTreeContext::for_path(&td.path.join("sub"));
        assert_eq!(
            ctx.matches(&td.path.join("sub/keep.log"), false),
            Some(false)
        );

        let ctx_root = FileTreeContext::for_path(&td.path);
        assert_eq!(
            ctx_root.matches(&td.path.join("root.log"), false),
            Some(true)
        );
    }

    #[test]
    fn page_children_pages_correctly() {
        let td = TempDir::new("page");
        for i in 0..7 {
            std::fs::write(td.path.join(format!("f{i:02}.txt")), b"").unwrap();
        }
        // Page size 3 → 3 pages: [0,1,2] [3,4,5] [6].
        let p0 = FileTree::page_children(&td.path, 0, 3).unwrap();
        assert_eq!(p0.entries.len(), 3);
        assert_eq!(p0.total, 7);
        assert_eq!(p0.offset, 0);
        assert!(p0.has_more);
        assert_eq!(p0.entries[0].name, "f00.txt");

        let p1 = FileTree::page_children(&td.path, 3, 3).unwrap();
        assert_eq!(p1.entries.len(), 3);
        assert_eq!(p1.entries[0].name, "f03.txt");
        assert!(p1.has_more);

        let p2 = FileTree::page_children(&td.path, 6, 3).unwrap();
        assert_eq!(p2.entries.len(), 1);
        assert_eq!(p2.entries[0].name, "f06.txt");
        assert!(!p2.has_more);

        // Past-end offset returns empty.
        let p3 = FileTree::page_children(&td.path, 99, 3).unwrap();
        assert!(p3.entries.is_empty());
        assert!(!p3.has_more);
    }

    #[test]
    fn page_children_sorts_dirs_first() {
        let td = TempDir::new("sort");
        std::fs::write(td.path.join("a.txt"), b"").unwrap();
        std::fs::create_dir_all(td.path.join("z_dir")).unwrap();
        std::fs::write(td.path.join("b.txt"), b"").unwrap();

        let page = FileTree::page_children(&td.path, 0, 10).unwrap();
        assert_eq!(page.entries.len(), 3);
        assert!(page.entries[0].is_dir, "dir should sort first");
        assert_eq!(page.entries[0].name, "z_dir");
        assert_eq!(page.entries[1].name, "a.txt");
        assert_eq!(page.entries[2].name, "b.txt");
    }

    #[test]
    fn page_children_drops_os_garbage() {
        let td = TempDir::new("garbage");
        std::fs::write(td.path.join("real.txt"), b"").unwrap();
        std::fs::write(td.path.join(".DS_Store"), b"").unwrap();
        std::fs::write(td.path.join("Thumbs.db"), b"").unwrap();

        let page = FileTree::page_children(&td.path, 0, 10).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].name, "real.txt");
    }

    #[test]
    fn page_children_marks_ignored_inside_git_repo() {
        let td = TempDir::new("page-ignore");
        std::fs::create_dir_all(td.path.join(".git")).unwrap();
        std::fs::write(td.path.join(".gitignore"), "ignored.log\n").unwrap();
        std::fs::write(td.path.join("ignored.log"), b"").unwrap();
        std::fs::write(td.path.join("kept.txt"), b"").unwrap();

        let page = FileTree::page_children(&td.path, 0, 10).unwrap();
        let by_name: std::collections::HashMap<_, _> = page
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.is_ignored))
            .collect();
        assert_eq!(by_name.get("ignored.log"), Some(&Some(true)));
        assert_eq!(by_name.get("kept.txt"), Some(&Some(false)));
    }
}

fn sort_entries(a_is_dir: bool, a_name: &str, b_is_dir: bool, b_name: &str) -> std::cmp::Ordering {
    match (a_is_dir, b_is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a_name.to_lowercase().cmp(&b_name.to_lowercase()),
    }
}
