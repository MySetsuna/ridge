//! 文件搜索 + 文件树的**线形 DTO + 到 `ridge-core` 的桥接**（契约 §9 / §11.D）。
//!
//! S5 起，搜索 / 列目录的**算法不再在本文件复刻**。`ridge-cli` 现在 `path` 依赖
//! `ridge-core`，经 `ridge_core::dispatch("search" / "get_directory_children", …)`
//! 复用与桌面端**完全相同**的 `fs::search` / `fs::tree` 实现（单一真源）。
//!
//! 本文件只保留两件事：
//!   1. JSON-RPC `search` / `get_directory_children` 结果仍序列化的**线形 DTO**
//!      （`SearchResult` / `FileNode`）——保持 controller↔host 的 JSON schema 不变。
//!   2. 一层**映射**：把 `ridge-core` 富类型结果裁剪回上述精简 DTO。
//!
//! `ridge-core` 是 zero-Tauri 的纯 crate（`cargo tree -p ridge-cli` 无 `tauri`），
//! 所以无头二进制仍然精简。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::core_host::headless_ctx;

/// 单条搜索命中（controller↔host 线形 DTO；保持原 schema，不含 `match_text`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub content: String,
}

/// 文件树节点（线形 DTO；保持原 schema：仅 name / path / is_dir）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// `get_directory_children` 的列目录上限。原 `list_dir` 返回全部子项不分页，
/// 这里给一个足够大的 limit 让 `page_children` 一次返回全部，保持原行为。
const LIST_DIR_LIMIT: usize = usize::MAX;

/// ripgrep 级文本搜索：经 `ridge_core::dispatch("search", …)` 复用桌面同款引擎。
///
/// `use_regex` / `case_sensitive` 透传；命中结果裁剪回精简 `SearchResult`
/// （丢弃 `ridge-core` 的 `match_text`，保持线形 schema 不变）。dispatch 失败
/// （能力拒绝 / 路径穿越 / 根不存在等）时返回空结果，与原 fail-soft 行为一致。
///
/// `roots` 是 host 的服务根沙箱（D-GM-9）：`root` 落在其外时 dispatch 因
/// `sandbox_guard` 拒绝 → 空结果。空 `roots` = 不限制（向后兼容）。
pub fn search(
    roots: &[PathBuf],
    root: &str,
    query: &str,
    use_regex: bool,
    case_sensitive: bool,
) -> Vec<SearchResult> {
    let ctx = headless_ctx(roots);
    let args = serde_json::json!({
        "root": root,
        "query": query,
        "useRegex": use_regex,
        "caseSensitive": case_sensitive,
    });
    let value = match ridge_core::dispatch("search", args, &ctx) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "ridge_cli::fs", error = %e, "search dispatch failed");
            return Vec::new();
        }
    };
    // `ridge-core` 返回 `Vec<fs::search::SearchResult>`；裁剪到本地 DTO。
    serde_json::from_value::<Vec<ridge_core::fs::search::SearchResult>>(value)
        .map(|hits| {
            hits.into_iter()
                .map(|h| SearchResult {
                    file: h.file,
                    line: h.line,
                    column: h.column,
                    content: h.content,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 列目录的一层子项（目录优先，字母序）：经
/// `ridge_core::dispatch("get_directory_children", …)` 复用桌面同款 `fs::tree`。
///
/// 返回 `io::Result` 以保持 `session.rs` 现有错误分支（host 不泄露内部路径）。
/// dispatch 错误映射为 `io::Error`（保留人类可读 message）。
///
/// `roots` 是 host 的服务根沙箱（D-GM-9）：`path` 落在其外时 dispatch 因
/// `sandbox_guard` 拒绝 → `io::Error`。空 `roots` = 不限制（向后兼容）。
pub fn list_dir(roots: &[PathBuf], path: &Path) -> std::io::Result<Vec<FileNode>> {
    let ctx = headless_ctx(roots);
    let args = serde_json::json!({
        "path": path.to_string_lossy(),
        "offset": 0,
        "limit": LIST_DIR_LIMIT,
    });
    let value = ridge_core::dispatch("get_directory_children", args, &ctx)
        .map_err(|e| std::io::Error::other(e.to_command_string()))?;
    // `ridge-core` 返回 `DirectoryPage`；取 entries 裁剪到本地 DTO。
    let page: ridge_core::fs::tree::DirectoryPage = serde_json::from_value(value)
        .map_err(|e| std::io::Error::other(format!("decode directory page: {e}")))?;
    Ok(page
        .entries
        .into_iter()
        .map(|n| FileNode {
            name: n.name,
            path: n.path,
            is_dir: n.is_dir,
        })
        .collect())
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
    fn search_finds_literal_match_via_core() {
        let dir = tmp("search");
        std::fs::write(dir.join("a.txt"), "hello world\nfoo bar\n").unwrap();
        // Serve the temp dir as the sole root so the sandbox admits it.
        let roots = [dir.clone()];
        let hits = search(&roots, &dir.to_string_lossy(), "foo", false, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].column, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_dir_sorts_dirs_first_via_core() {
        let dir = tmp("list");
        std::fs::write(dir.join("z.txt"), "").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let roots = [dir.clone()];
        let entries = list_dir(&roots, &dir).unwrap();
        assert_eq!(entries[0].name, "sub");
        assert!(entries[0].is_dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_dir_outside_serving_root_is_denied() {
        // The serving root is `served`; a sibling `secret` dir must be unreachable.
        let served = tmp("served");
        let secret = tmp("secret");
        std::fs::write(secret.join("creds"), "token").unwrap();
        let roots = [served.clone()];
        let denied = list_dir(&roots, &secret);
        assert!(
            denied.is_err(),
            "listing a path outside the serving root must be rejected by the sandbox"
        );
        let _ = std::fs::remove_dir_all(&served);
        let _ = std::fs::remove_dir_all(&secret);
    }

    #[test]
    fn empty_roots_remain_unrestricted() {
        // Backward-compat: no serving root → no confinement (whole-FS, legacy).
        let dir = tmp("unrestricted");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let entries = list_dir(&[], &dir).unwrap();
        assert!(entries.iter().any(|e| e.name == "sub" && e.is_dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
