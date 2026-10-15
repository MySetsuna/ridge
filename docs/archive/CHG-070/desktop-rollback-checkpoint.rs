//! G1 回滚语义（V-G1-RB）：workspace git worktree 补丁快照。
//!
//! `checkpoint` = `git diff` + `git status --porcelain` → sidecar `rollbackPatches[]`。
//! `rollback` = 已跟踪路径 `git checkout HEAD -- path`；删除快照时未跟踪的新增文件。
//! 非 git 仓库 → 明确 Err，不静默。**不做**全盘文件系统快照。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPatch {
    /// 快照时刻（epoch ms）。
    pub ts: u64,
    /// 可选标签（agent pane / 人工备注）。
    pub label: String,
    /// `git status --porcelain` 全文（相对 workspace root）。
    pub porcelain: String,
    /// `git diff`（工作区相对 HEAD / index 的 unified diff；含未暂存）。
    pub diff: String,
    /// porcelain 解析出的路径列表（正斜杠）。
    pub paths: Vec<String>,
}

async fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = ridge_core::commands::git::run_git_guarded(
        cwd.to_string_lossy().into_owned(),
        args.iter().map(|arg| (*arg).to_string()).collect(),
    )
    .await
    .map_err(|e| format!("git spawn failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {} failed: {err}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 确认 `root` 在 git 工作树内；否则 Err。
pub async fn ensure_git_repo(root: &Path) -> Result<(), String> {
    let out = ridge_core::commands::git::run_git_guarded(
        root.to_string_lossy().into_owned(),
        vec!["rev-parse".into(), "--is-inside-work-tree".into()],
    )
    .await
    .map_err(|e| format!("git spawn failed: {e}"))?;
    if !out.status.success() {
        return Err("not a git repository".into());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    if s.trim() != "true" {
        return Err("not a git repository".into());
    }
    Ok(())
}

/// 从 porcelain 行解析路径（支持 rename `R  a -> b` 取目标侧）。
pub fn paths_from_porcelain(porcelain: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in porcelain.lines() {
        if line.len() < 4 {
            continue;
        }
        // XY + space + path  (status codes are 2 chars)
        let rest = line[3..].trim();
        if rest.is_empty() {
            continue;
        }
        let path = if let Some((_, to)) = rest.split_once(" -> ") {
            to
        } else {
            rest.trim_matches('"')
        };
        let norm = path.replace('\\', "/");
        if !norm.is_empty() && !paths.contains(&norm) {
            paths.push(norm);
        }
    }
    paths
}

/// 对 workspace root 做 checkpoint，追加到 sidecar `rollbackPatches`。
pub async fn checkpoint(
    dir: &Path,
    wid: Uuid,
    workspace_root: &Path,
    label: impl Into<String>,
) -> Result<RollbackPatch, String> {
    ensure_git_repo(workspace_root).await?;
    let porcelain = run_git(workspace_root, &["status", "--porcelain"]).await?;
    // 工作区 + 暂存区相对 HEAD 的完整 diff（含 binary 占位）。
    let diff = run_git(workspace_root, &["diff", "HEAD"]).await?;
    let paths = paths_from_porcelain(&porcelain);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let patch = RollbackPatch {
        ts,
        label: label.into(),
        porcelain,
        diff,
        paths,
    };
    let patch_json = serde_json::to_value(&patch).map_err(|e| e.to_string())?;
    super::memory::update(dir, wid, |doc| {
        let arr = doc
            .entry("rollbackPatches".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if let Some(list) = arr.as_array_mut() {
            list.push(patch_json);
            // 环形上限 20
            const CAP: usize = 20;
            let overflow = list.len().saturating_sub(CAP);
            if overflow > 0 {
                list.drain(0..overflow);
            }
        }
    });
    Ok(patch)
}

/// 将列出路径恢复到快照时 blob：已跟踪 `git checkout HEAD -- path`；
/// 未跟踪新增（porcelain `??`）删除文件。
pub async fn rollback(workspace_root: &Path, patch: &RollbackPatch) -> Result<(), String> {
    ensure_git_repo(workspace_root).await?;
    let (tracked, untracked) = rollback_paths(&patch.porcelain);
    if !tracked.is_empty() {
        checkout_tracked(workspace_root, &tracked).await?;
    }
    remove_untracked(workspace_root, &untracked)?;
    Ok(())
}

fn rollback_paths(porcelain: &str) -> (Vec<String>, Vec<String>) {
    let mut tracked = Vec::new();
    let mut untracked = Vec::new();
    for line in porcelain.lines().filter(|line| line.len() >= 4) {
        let code = &line[..2];
        let rest = line[3..].trim();
        let path = rest
            .split_once(" -> ")
            .map_or(rest, |(_, to)| to)
            .trim_matches('"')
            .replace('\\', "/");
        if code == "??" {
            untracked.push(path);
        } else if !path.is_empty() {
            tracked.push(path);
        }
    }
    (tracked, untracked)
}

async fn checkout_tracked(workspace_root: &Path, tracked: &[String]) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["checkout", "HEAD", "--"];
    args.extend(tracked.iter().map(String::as_str));
    run_git(workspace_root, &args).await.map(|_| ())
}

fn remove_untracked(workspace_root: &Path, paths: &[String]) -> Result<(), String> {
    for path in paths {
        let full = workspace_root.join(path);
        if full.is_file() {
            std::fs::remove_file(&full).map_err(|e| format!("remove untracked {path}: {e}"))?;
        } else if full.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|e| format!("remove untracked dir {path}: {e}"))?;
        }
    }
    Ok(())
}

/// 从 sidecar 取最新一条 rollback patch（若有）。
pub fn latest_patch(dir: &Path, wid: Uuid) -> Option<RollbackPatch> {
    let doc = super::memory::read(dir, wid)?;
    let arr = doc.get("rollbackPatches")?.as_array()?;
    let last = arr.last()?;
    serde_json::from_value(last.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_temp_repo() -> PathBuf {
        let root = std::env::temp_dir().join(format!("ridge-rb-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]).await.unwrap();
        run_git(&root, &["config", "user.email", "t@t.test"])
            .await
            .unwrap();
        run_git(&root, &["config", "user.name", "t"]).await.unwrap();
        // avoid default branch noise
        let _ = run_git(&root, &["checkout", "-b", "main"]).await;
        std::fs::write(root.join("a.txt"), "v1\n").unwrap();
        run_git(&root, &["add", "a.txt"]).await.unwrap();
        run_git(&root, &["commit", "-m", "init"]).await.unwrap();
        root
    }

    #[test]
    fn paths_from_porcelain_basic() {
        let p = " M src/foo.rs\n?? new.txt\nR  old.rs -> new.rs\n";
        let paths = paths_from_porcelain(p);
        assert!(paths.contains(&"src/foo.rs".into()));
        assert!(paths.contains(&"new.txt".into()));
        assert!(paths.contains(&"new.rs".into()));
    }

    #[tokio::test]
    async fn checkpoint_and_rollback_restores_tracked() {
        let root = init_temp_repo().await;
        let mem = std::env::temp_dir().join(format!("ridge-rb-mem-{}", Uuid::new_v4()));
        let wid = Uuid::new_v4();

        std::fs::write(root.join("a.txt"), "v2-dirty\n").unwrap();
        std::fs::write(root.join("new.txt"), "untracked\n").unwrap();

        let patch = checkpoint(&mem, wid, &root, "test")
            .await
            .expect("checkpoint");
        assert!(patch.paths.iter().any(|p| p == "a.txt"));
        assert!(patch.paths.iter().any(|p| p == "new.txt"));
        assert!(std::fs::read_to_string(root.join("a.txt"))
            .unwrap()
            .contains("v2"));

        rollback(&root, &patch).await.expect("rollback");
        let restored = std::fs::read_to_string(root.join("a.txt")).unwrap();
        // Windows git may normalize line endings → compare without CR.
        assert_eq!(restored.replace('\r', ""), "v1\n");
        assert!(!root.join("new.txt").exists(), "untracked must be removed");

        let latest = latest_patch(&mem, wid).expect("sidecar patch");
        assert_eq!(latest.label, "test");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&mem);
    }

    #[tokio::test]
    async fn checkpoint_rejects_non_git() {
        let root = std::env::temp_dir().join(format!("ridge-rb-nogit-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mem = std::env::temp_dir().join(format!("ridge-rb-mem2-{}", Uuid::new_v4()));
        let err = checkpoint(&mem, Uuid::new_v4(), &root, "x")
            .await
            .unwrap_err();
        assert!(err.contains("not a git"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&mem);
    }
}
