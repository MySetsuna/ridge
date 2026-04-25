use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
}

pub struct FileTree;

impl FileTree {
    /// Build a file tree from a root directory
    pub fn build(root: &Path, max_depth: usize) -> std::io::Result<FileNode> {
        Self::build_recursive(root, 0, max_depth)
    }

    fn build_recursive(path: &Path, current_depth: usize, max_depth: usize) -> std::io::Result<FileNode> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let metadata = fs::metadata(path)?;
        let is_dir = metadata.is_dir();

        if !is_dir {
            return Ok(FileNode {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: false,
                children: None,
                expanded: None,
            });
        }

        // If max depth reached, return directory without children
        if current_depth >= max_depth {
            return Ok(FileNode {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: true,
                children: None,
                expanded: Some(false),
            });
        }

        // Read directory entries
        let mut children = Vec::new();
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if Self::should_ignore(&entry_path) {
                        continue;
                    }

                    match Self::build_recursive(&entry_path, current_depth + 1, max_depth) {
                        Ok(child) => children.push(child),
                        Err(e) => {
                            // Skip entries we can't read
                            tracing::warn!("Failed to read {}: {}", entry_path.display(), e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read directory {}: {}", path.display(), e);
            }
        }

        // Sort: directories first, then by name
        children.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        Ok(FileNode {
            name,
            path: path.to_string_lossy().to_string(),
            is_dir: true,
            children: Some(children),
            expanded: Some(current_depth < 2), // Auto-expand first 2 levels
        })
    }

    /// Check if a path should be ignored
    pub fn should_ignore(path: &Path) -> bool {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Common ignore patterns
        let ignore_dirs = [
            ".git",
            "node_modules",
            "target",
            "dist",
            "build",
            ".cache",
            "__pycache__",
            ".venv",
            "venv",
            ".svn",
            ".hg",
            "vendor",
            "packages",
            ".next",
            ".nuxt",
            ".output",
            ".astro",
            ".solid",
            ".svelte-kit",
            "coverage",
            ".nyc_output",
            ".turbo",
            ".idea",
            ".vscode",
            ".DS_Store",
            "Thumbs.db",
        ];

        // Check if directory matches ignore patterns
        if path.is_dir() {
            if ignore_dirs.contains(&name.as_str()) {
                return true;
            }
            // Check for hidden files/directories
            if name.starts_with('.') && name != ".gitignore" && name != ".env" && name != ".env.example" {
                return true;
            }
        }

        // Ignore specific file extensions
        if path.is_file() {
            let ignore_extensions = [
                ".log",
                ".lock",
                ".sum",
                ".pyc",
                ".pyo",
                ".class",
                ".o",
                ".obj",
                ".exe",
                ".dll",
                ".so",
                ".dylib",
                ".bin",
                ".out",
                ".err",
            ];

            if let Some(ext) = path.extension() {
                let ext_str = format!(".{}", ext.to_string_lossy());
                if ignore_extensions.contains(&ext_str.as_str()) {
                    return true;
                }
            }

            // Ignore hidden files (but allow .gitignore, .env, etc.)
            if name.starts_with('.') && name != ".gitignore" && !name.starts_with(".env") {
                return true;
            }
        }

        false
    }

    /// Get directory contents for lazy loading
    pub fn get_children(path: &Path) -> std::io::Result<Vec<FileNode>> {
        let mut children = Vec::new();

        let entries = fs::read_dir(path)?;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if Self::should_ignore(&entry_path) {
                continue;
            }

            let name = entry_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let is_dir = entry_path.is_dir();
            let _has_children = if is_dir {
                fs::read_dir(&entry_path).map(|mut e| e.next().is_some()).unwrap_or(false)
            } else {
                false
            };

            children.push(FileNode {
                name,
                path: entry_path.to_string_lossy().to_string(),
                is_dir,
                children: None,
                expanded: None,
            });
        }

        // Sort: directories first, then by name
        children.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        Ok(children)
    }
}