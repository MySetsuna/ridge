//! Domain C2 — `ridge://` 资源 URI 解析 + 内存 Stash 中转站。由 Phase 1 / TM-C 填充。

use std::collections::{HashMap, VecDeque};

use uuid::Uuid;

// ─── RidgeUri ────────────────────────────────────────────────────────────────

/// Ridge 资源 URI 的解析结果。
///
/// 支持的格式：
/// - `ridge://workspace/active-panes`
/// - `ridge://workspace/git-status`
/// - `ridge://workspace/editor-context`
/// - `ridge://cache/<uuid>`
#[derive(Debug, Clone, PartialEq)]
pub enum RidgeUri {
    WorkspaceActivePanes,
    WorkspaceGitStatus,
    WorkspaceEditorContext,
    Cache(String),
}

impl RidgeUri {
    /// 将 URI 字符串解析为 `RidgeUri`，失败返回 `UriError`。
    pub fn parse(uri: &str) -> Result<RidgeUri, UriError> {
        let rest = uri
            .strip_prefix("ridge://")
            .ok_or(UriError::NotRidgeScheme)?;

        match rest {
            "workspace/active-panes" => Ok(RidgeUri::WorkspaceActivePanes),
            "workspace/git-status" => Ok(RidgeUri::WorkspaceGitStatus),
            "workspace/editor-context" => Ok(RidgeUri::WorkspaceEditorContext),
            _ => {
                if let Some(id) = rest.strip_prefix("cache/") {
                    if id.is_empty() {
                        return Err(UriError::EmptyCacheId);
                    }
                    Ok(RidgeUri::Cache(id.to_string()))
                } else {
                    Err(UriError::UnknownPath)
                }
            }
        }
    }

    /// 将枚举值转换回规范 URI 字符串。
    pub fn to_uri(&self) -> String {
        match self {
            RidgeUri::WorkspaceActivePanes => "ridge://workspace/active-panes".to_string(),
            RidgeUri::WorkspaceGitStatus => "ridge://workspace/git-status".to_string(),
            RidgeUri::WorkspaceEditorContext => "ridge://workspace/editor-context".to_string(),
            RidgeUri::Cache(id) => format!("ridge://cache/{id}"),
        }
    }
}

// ─── UriError ────────────────────────────────────────────────────────────────

/// `ridge://` URI 解析错误。
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UriError {
    #[error("URI 不是 ridge:// 方案")]
    NotRidgeScheme,
    #[error("未知的 ridge:// 路径")]
    UnknownPath,
    #[error("cache URI 的 ID 部分为空")]
    EmptyCacheId,
}

// ─── StashStore ──────────────────────────────────────────────────────────────

/// 内存 Stash 中转站：以 UUID v4 为键存储二进制 blob，支持条数与总字节双门限 FIFO 淘汰。
pub struct StashStore {
    map: HashMap<String, Vec<u8>>,
    /// 插入顺序队列，用于 FIFO 淘汰
    order: VecDeque<String>,
    max_entries: usize,
    max_total_bytes: usize,
    total_bytes: usize,
}

impl StashStore {
    /// 构造指定上限的 StashStore。
    pub fn new(max_entries: usize, max_total_bytes: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max_entries,
            max_total_bytes,
            total_bytes: 0,
        }
    }

    /// 以默认上限（64 条 / 32 MiB）构造 StashStore。
    pub fn with_defaults() -> Self {
        Self::new(64, 32 * 1024 * 1024)
    }

    /// 存入内容，返回新分配的 UUID v4 ID（不含 `ridge://cache/` 前缀）。
    ///
    /// 存入后立即按 FIFO 逐出，直到条数与总字节均不超限。
    pub fn stash(&mut self, content: Vec<u8>) -> String {
        let id = Uuid::new_v4().to_string();
        self.total_bytes += content.len();
        self.map.insert(id.clone(), content);
        self.order.push_back(id.clone());

        // FIFO 淘汰：任一条件超限即继续淘汰
        while self.order.len() > self.max_entries || self.total_bytes > self.max_total_bytes {
            if let Some(oldest) = self.order.pop_front() {
                if let Some(removed) = self.map.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(removed.len());
                }
            } else {
                break;
            }
        }

        id
    }

    /// 存入内容，返回完整的 `ridge://cache/<id>` URI。
    pub fn stash_uri(&mut self, content: Vec<u8>) -> String {
        let id = self.stash(content);
        format!("ridge://cache/{id}")
    }

    /// 按 ID 读取内容（不带 URI 前缀）。
    pub fn read(&self, id: &str) -> Option<&[u8]> {
        self.map.get(id).map(Vec::as_slice)
    }

    /// 解析 `ridge://cache/<id>` URI 后读取内容；非 Cache 变体返回 `None`。
    pub fn read_uri(&self, uri: &str) -> Option<&[u8]> {
        if let Ok(RidgeUri::Cache(id)) = RidgeUri::parse(uri) {
            self.read(&id)
        } else {
            None
        }
    }

    /// 当前存储条目数。
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 当前所有 blob 的总字节数。
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RidgeUri ──

    #[test]
    fn parse_active_panes() {
        assert_eq!(
            RidgeUri::parse("ridge://workspace/active-panes").unwrap(),
            RidgeUri::WorkspaceActivePanes
        );
    }

    #[test]
    fn parse_git_status() {
        assert_eq!(
            RidgeUri::parse("ridge://workspace/git-status").unwrap(),
            RidgeUri::WorkspaceGitStatus
        );
    }

    #[test]
    fn parse_editor_context() {
        assert_eq!(
            RidgeUri::parse("ridge://workspace/editor-context").unwrap(),
            RidgeUri::WorkspaceEditorContext
        );
    }

    #[test]
    fn parse_cache_uri() {
        let uri = "ridge://cache/abc-123";
        assert_eq!(
            RidgeUri::parse(uri).unwrap(),
            RidgeUri::Cache("abc-123".to_string())
        );
    }

    #[test]
    fn parse_rejects_non_ridge_scheme() {
        assert_eq!(
            RidgeUri::parse("https://example.com").unwrap_err(),
            UriError::NotRidgeScheme
        );
    }

    #[test]
    fn parse_rejects_unknown_path() {
        assert_eq!(
            RidgeUri::parse("ridge://workspace/unknown").unwrap_err(),
            UriError::UnknownPath
        );
    }

    #[test]
    fn parse_rejects_empty_cache_id() {
        assert_eq!(
            RidgeUri::parse("ridge://cache/").unwrap_err(),
            UriError::EmptyCacheId
        );
    }

    #[test]
    fn to_uri_round_trips() {
        let cases = [
            RidgeUri::WorkspaceActivePanes,
            RidgeUri::WorkspaceGitStatus,
            RidgeUri::WorkspaceEditorContext,
            RidgeUri::Cache("test-id".to_string()),
        ];
        for case in &cases {
            let uri = case.to_uri();
            assert_eq!(&RidgeUri::parse(&uri).unwrap(), case);
        }
    }

    // ── StashStore ──

    #[test]
    fn stash_and_read_roundtrip() {
        let mut store = StashStore::with_defaults();
        let id = store.stash(b"hello".to_vec());
        assert_eq!(store.read(&id).unwrap(), b"hello");
    }

    #[test]
    fn stash_uri_returns_ridge_prefix() {
        let mut store = StashStore::with_defaults();
        let uri = store.stash_uri(b"data".to_vec());
        assert!(uri.starts_with("ridge://cache/"));
    }

    #[test]
    fn read_uri_resolves_cache_uri() {
        let mut store = StashStore::with_defaults();
        let uri = store.stash_uri(b"content".to_vec());
        assert_eq!(store.read_uri(&uri).unwrap(), b"content");
    }

    #[test]
    fn read_uri_returns_none_for_workspace_uri() {
        let store = StashStore::with_defaults();
        assert!(store.read_uri("ridge://workspace/active-panes").is_none());
    }

    #[test]
    fn len_and_is_empty() {
        let mut store = StashStore::with_defaults();
        assert!(store.is_empty());
        store.stash(b"x".to_vec());
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());
    }

    #[test]
    fn total_bytes_tracks_content_size() {
        let mut store = StashStore::with_defaults();
        store.stash(b"hello".to_vec()); // 5 bytes
        store.stash(b"world".to_vec()); // 5 bytes
        assert_eq!(store.total_bytes(), 10);
    }

    #[test]
    fn fifo_eviction_by_entry_count() {
        let mut store = StashStore::new(2, usize::MAX);
        let id1 = store.stash(b"first".to_vec());
        let _id2 = store.stash(b"second".to_vec());
        let _id3 = store.stash(b"third".to_vec()); // should evict id1
        assert_eq!(store.len(), 2);
        assert!(store.read(&id1).is_none(), "oldest entry must be evicted");
    }

    #[test]
    fn fifo_eviction_by_byte_limit() {
        // max 10 bytes, insert two 6-byte blobs — first must be evicted
        let mut store = StashStore::new(usize::MAX, 10);
        let id1 = store.stash(b"123456".to_vec()); // 6 bytes
        let _id2 = store.stash(b"abcdef".to_vec()); // 6 bytes — total 12 > 10
        assert!(
            store.read(&id1).is_none(),
            "oldest entry must be evicted by bytes"
        );
        assert!(store.total_bytes() <= 10);
    }

    #[test]
    fn read_returns_none_for_missing_id() {
        let store = StashStore::with_defaults();
        assert!(store.read("no-such-id").is_none());
    }
}
