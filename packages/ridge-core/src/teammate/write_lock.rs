//! Domain D3 —— 文件并发写锁与冲突仲裁（Concurrency Write Lock）。
//!
//! 纯注册表：`规范化路径 → 当前持有者(pane/agent id)`。当两个 Agent 试图同时写同一
//! 文件时，后手得到 [`LockOutcome::Conflict`]，上层（`src-tauri`）据此拦截后手、
//! 暂停双方并弹出 Monaco Diff 冲突视图交人类裁决。本模块**只做登记与冲突判定**，
//! 不触碰文件系统、不发事件（零运行时耦合，可单测）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 申请写锁的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockOutcome {
    /// 路径无主，已授予调用者。
    Acquired,
    /// 调用者本人已持有该锁（可重入，放行）。
    AlreadyHeld,
    /// 路径被他人持有 → 冲突。
    Conflict { holder: String },
}

impl LockOutcome {
    /// 是否允许写入继续（Acquired 或 AlreadyHeld）。
    pub fn is_writable(&self) -> bool {
        matches!(self, LockOutcome::Acquired | LockOutcome::AlreadyHeld)
    }
}

/// 进程级文件写锁注册表。
#[derive(Debug, Default)]
pub struct WriteLockRegistry {
    locks: HashMap<String, String>,
}

impl WriteLockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 申请对 `path` 的写锁。无主→`Acquired`（登记 owner）；本人已持→`AlreadyHeld`；
    /// 他人持→`Conflict`（**不**改变现有持有者）。
    pub fn try_acquire(&mut self, path: &str, owner: &str) -> LockOutcome {
        let key = normalize(path);
        match self.locks.get(&key) {
            None => {
                self.locks.insert(key, owner.to_string());
                LockOutcome::Acquired
            }
            Some(h) if h == owner => LockOutcome::AlreadyHeld,
            Some(h) => LockOutcome::Conflict { holder: h.clone() },
        }
    }

    /// 释放：仅当 `owner` 是当前持有者时才解锁。返回是否实际解锁。
    pub fn release(&mut self, path: &str, owner: &str) -> bool {
        let key = normalize(path);
        if self.locks.get(&key).map(String::as_str) == Some(owner) {
            self.locks.remove(&key);
            true
        } else {
            false
        }
    }

    /// 当前持有者。
    pub fn holder(&self, path: &str) -> Option<&str> {
        self.locks.get(&normalize(path)).map(String::as_str)
    }

    /// 释放某 owner 持有的全部锁（pane 关闭 / agent 失联时清理）。返回释放数量。
    pub fn release_all(&mut self, owner: &str) -> usize {
        let before = self.locks.len();
        self.locks.retain(|_, h| h != owner);
        before - self.locks.len()
    }

    /// 当前持锁条数。
    pub fn len(&self) -> usize {
        self.locks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locks.is_empty()
    }
}

/// 规范化路径键：统一分隔符为 `/`，去尾随分隔符；Windows 下大小写不敏感（小写化）。
/// 不做物理 canonicalize（纯逻辑、不触盘）。
fn normalize(path: &str) -> String {
    let unified = path.trim().replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    let base = if trimmed.is_empty() { "/" } else { trimmed };
    if cfg!(windows) {
        base.to_lowercase()
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_then_conflict() {
        let mut r = WriteLockRegistry::new();
        assert_eq!(r.try_acquire("src/main.rs", "pane1"), LockOutcome::Acquired);
        // 后手他人 → 冲突，且不夺锁。
        assert_eq!(
            r.try_acquire("src/main.rs", "pane2"),
            LockOutcome::Conflict {
                holder: "pane1".into()
            }
        );
        assert_eq!(r.holder("src/main.rs"), Some("pane1"));
    }

    #[test]
    fn reentrant_same_owner_is_already_held() {
        let mut r = WriteLockRegistry::new();
        r.try_acquire("a.txt", "p1");
        assert_eq!(r.try_acquire("a.txt", "p1"), LockOutcome::AlreadyHeld);
        assert!(r.try_acquire("a.txt", "p1").is_writable());
    }

    #[test]
    fn release_only_by_holder() {
        let mut r = WriteLockRegistry::new();
        r.try_acquire("f", "p1");
        assert!(!r.release("f", "p2")); // 非持有者不能解
        assert!(r.release("f", "p1"));
        // 解锁后他人可获取。
        assert_eq!(r.try_acquire("f", "p2"), LockOutcome::Acquired);
    }

    #[test]
    fn path_normalization_matches_separators_and_case() {
        let mut r = WriteLockRegistry::new();
        r.try_acquire("src/main.rs", "p1");
        // 反斜杠 + 尾随斜杠归一到同一键。
        let other = r.try_acquire("src\\main.rs/", "p2");
        assert_eq!(
            other,
            LockOutcome::Conflict {
                holder: "p1".into()
            }
        );
        if cfg!(windows) {
            // Windows 大小写不敏感。
            assert_eq!(
                r.try_acquire("SRC/MAIN.RS", "p3"),
                LockOutcome::Conflict {
                    holder: "p1".into()
                }
            );
        }
    }

    #[test]
    fn release_all_clears_owner_locks() {
        let mut r = WriteLockRegistry::new();
        r.try_acquire("a", "p1");
        r.try_acquire("b", "p1");
        r.try_acquire("c", "p2");
        assert_eq!(r.release_all("p1"), 2);
        assert_eq!(r.len(), 1);
        assert_eq!(r.holder("c"), Some("p2"));
    }
}
