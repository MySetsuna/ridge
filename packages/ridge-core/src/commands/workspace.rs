//! Workspace 快照 handler —— R0 内核化样板 B（2026-07-11）。
//!
//! 把桌面 `src-tauri/src/commands/ridge_file.rs::snapshot_workspace` 的**纯加工逻辑**
//! （pane 树 JSON + 各 pane cwd 发现 git 仓库根去重 + pane 标题）下沉 ridge-core，
//! 让桌面/rdg 无头两端共用一份。宿主经 [`WorkspaceReader`] 端口提供**原始数据**，
//! core 负责加工——与 [`super::settings`] 的 `HostStateAccessor` 同款「宿主状态经 trait
//! 端口注入」范式。
//!
//! **本刀范围（安全、可离线测）**：core 端 trait + 纯加工 + handler + 单测。dispatch
//! 路由 + 桌面/rdg adapter 接线为下一刀（需真机验证桌面路径，见 R0 spec）。届时
//! [`WorkspaceAccessor`] 与 `settings::HostStateAccessor` 应合并为聚合 host-state trait
//! （一个 `Ctx` 只有一个 state；同时服务两域的宿主需聚合，R0 spec 风险项）。

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;
use serde_json::Value;

use crate::ctx::Ctx;
use crate::error::{CoreError, CoreResult};

/// 宿主提供的**原始**工作区数据（未加工）。桌面从 `AppState.workspaces` 填，rdg 从
/// daemon 状态填。
#[derive(Debug, Clone)]
pub struct WorkspaceRaw {
    /// pane 树的 JSON 快照（宿主已序列化其 `PaneTree`）。
    pub pane_tree: Value,
    /// 各 pane 的 cwd（用于向上发现 git 仓库根）。
    pub pane_cwds: Vec<String>,
    /// pane 标题（teammate 显示名），键 = pane UUID 串。
    pub pane_titles: HashMap<String, String>,
}

/// 宿主暴露「读某工作区原始数据」的端口（D4：core 不认桌面 `AppState`）。
pub trait WorkspaceReader: Send + Sync {
    /// 返回该工作区的原始快照数据；工作区不存在 → `None`。
    fn workspace_raw(&self, workspace_id: &str) -> Option<WorkspaceRaw>;
}

/// 加工后的工作区快照（`git_repos`/`pane_titles` 语义与桌面 `snapshot_workspace` 一致）。
#[derive(Debug, Serialize, PartialEq)]
pub struct WorkspaceSnapshot {
    pub pane_tree: Value,
    pub git_repos: Vec<String>,
    pub pane_titles: HashMap<String, String>,
}

/// 纯加工：从原始数据算出快照。`git_repos` = 各 cwd 向上找 `.git` 根、去重 + 排序
/// （`BTreeSet`，与桌面逐字一致）。纯函数、可离线测（不碰宿主状态）。
pub fn build_workspace_snapshot(raw: WorkspaceRaw) -> WorkspaceSnapshot {
    let git_repos: Vec<String> = raw
        .pane_cwds
        .into_iter()
        .filter_map(crate::commands::git::find_git_repo_root)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    WorkspaceSnapshot {
        pane_tree: raw.pane_tree,
        git_repos,
        pane_titles: raw.pane_titles,
    }
}

/// handler：`get_workspace_snapshot`。经 [`WorkspaceAccessor`] 取宿主原始数据 → 加工 →
/// JSON。工作区不存在 → [`CoreError::InvalidArgs`]。
pub fn get_workspace_snapshot(ctx: &Ctx, workspace_id: &str) -> CoreResult<Value> {
    // 经**聚合** accessor 取宿主的 WorkspaceReader（R0：一个 Ctx 一个 state，settings 与
    // workspace 等多域共用 `super::settings::HostStateAccessor`，见 `settings::HostState`）。
    let reader = ctx.state::<super::settings::HostStateAccessor>()?;
    let raw = reader
        .0
        .workspace_raw(workspace_id)
        .ok_or_else(|| CoreError::InvalidArgs(format!("workspace {workspace_id} 不存在")))?;
    serde_json::to_value(build_workspace_snapshot(raw)).map_err(CoreError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::commands::settings::HostStateAccessor;
    use crate::ctx::test_support::ctx_with_state;
    use std::sync::Arc;

    fn tmp_git_repo(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("ridge_ws_{tag}_{nanos}"));
        std::fs::create_dir_all(d.join(".git")).unwrap();
        d
    }

    #[test]
    fn build_snapshot_dedups_and_sorts_git_repos() {
        let repo = tmp_git_repo("dedup");
        let sub = repo.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let raw = WorkspaceRaw {
            pane_tree: serde_json::json!({ "root": true }),
            // 两个 cwd（repo 与 repo/sub）都归到同一个 .git 根 → 去重后仅 1。
            pane_cwds: vec![
                repo.to_string_lossy().to_string(),
                sub.to_string_lossy().to_string(),
            ],
            pane_titles: HashMap::new(),
        };
        let snap = build_workspace_snapshot(raw);
        assert_eq!(snap.git_repos.len(), 1);
        assert_eq!(snap.pane_tree, serde_json::json!({ "root": true }));
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn build_snapshot_empty_cwds_no_repos() {
        let raw = WorkspaceRaw {
            pane_tree: Value::Null,
            pane_cwds: vec![],
            pane_titles: HashMap::new(),
        };
        assert!(build_workspace_snapshot(raw).git_repos.is_empty());
    }

    struct FakeReader(Option<WorkspaceRaw>);
    impl WorkspaceReader for FakeReader {
        fn workspace_raw(&self, _id: &str) -> Option<WorkspaceRaw> {
            self.0.clone()
        }
    }
    // 聚合 HostState 也要求 UserDefaultCwdStore；本测试只走 workspace 端口 → 空实现。
    impl crate::commands::settings::UserDefaultCwdStore for FakeReader {
        fn set_user_default_cwd(&self, _path: Option<std::path::PathBuf>) {}
    }

    #[test]
    fn handler_missing_workspace_is_invalid_args() {
        let accessor: Arc<dyn crate::ctx::CoreState> =
            Arc::new(HostStateAccessor(Arc::new(FakeReader(None))));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let err = get_workspace_snapshot(&ctx, "nope").unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[test]
    fn handler_returns_snapshot_json() {
        let raw = WorkspaceRaw {
            pane_tree: serde_json::json!({ "a": 1 }),
            pane_cwds: vec![],
            pane_titles: HashMap::new(),
        };
        let accessor: Arc<dyn crate::ctx::CoreState> =
            Arc::new(HostStateAccessor(Arc::new(FakeReader(Some(raw)))));
        let (ctx, _s) = ctx_with_state(accessor, CapabilitySet::allow_all());
        let out = get_workspace_snapshot(&ctx, "ws1").unwrap();
        assert_eq!(out["pane_tree"], serde_json::json!({ "a": 1 }));
        assert_eq!(out["git_repos"], serde_json::json!([]));
    }
}
