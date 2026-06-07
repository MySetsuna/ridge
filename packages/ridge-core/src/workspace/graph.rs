//! `WorkspaceGraph` —— D11 的**共享实体图谱**：一份权威的 workspaces 集合，每个
//! workspace 持一棵 [`PaneTree`] 布局 + pane 的**共享属性**（锁定渲染尺寸）。
//!
//! ## 归属（详 `docs/plans/d11-workspace-graph-pane-decoupling-design.md` §1.5）
//!
//! 图谱只持 **pane 身份 + 布局 + 共享属性**。`PtyHandle`/`PtyBridge` 句柄、teammate
//! 生命周期、`pty_generation`、scrollback 等**留各 host 旁表**，按同一 pane id 对齐。
//!
//! ## 谁用它
//!
//! - **headless `ridge-cli` / cloud host**：把它当作工作区/pane 存储（等价于桌面
//!   `AppState.workspaces` 的 pane-tree 部分）。当前 cli 是单 workspace + 单 leaf 的
//!   退化形态；S3 统一协议落地后采用（P4）。
//! - **桌面**：**不**改用它作存储——桌面已有 `AppState.workspaces`，且 pane 模型的
//!   实质复用已在 P1（共享 [`PaneTree`]）达成；桌面采用图谱属 S3 旁的较大重构，按
//!   定稿延后。
//!
//! ## 不发事件
//!
//! 图谱方法是**纯**的（mutate + 返回结构结果），**不**自己 emit。调用方（host）在
//! **释放锁之后**经 [`crate::ctx::EventSink`] 广播结构变更——保留"先解锁再发"的安全序，
//! 并让 host 保留 respawn-vs-suppress 决策（详定稿 §1.5 B）。

use crate::error::CoreError;
use crate::workspace::pane_tree::{DockRegion, PaneNode, PaneTree, SplitDirection};
use std::collections::HashMap;
use uuid::Uuid;

/// 单个 workspace 的图谱数据：布局树 + 名称 + 每 pane 锁定渲染尺寸（共享属性）。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceMeta {
    pub pane_tree: PaneTree,
    pub name: Option<String>,
    /// 每 pane 的**锁定渲染尺寸** `(cols, rows)`（D11 共享属性，last-write-wins，
    /// 随 attach 快照下发）。与桌面 split-target 选择用的"实测尺寸"是两回事——后者
    /// 是 GUI 旁表，不进图谱。
    pub locked_sizes: HashMap<Uuid, (u16, u16)>,
}

/// 一份权威的 workspaces 集合 + 当前活动 workspace。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceGraph {
    workspaces: HashMap<Uuid, WorkspaceMeta>,
    active: Option<Uuid>,
}

impl WorkspaceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// 新建一个空 workspace（单 leaf 树）；若是首个则置为 active。返回新 workspace id。
    pub fn create_workspace(&mut self) -> Uuid {
        let wid = Uuid::new_v4();
        self.workspaces.insert(wid, WorkspaceMeta::default());
        if self.active.is_none() {
            self.active = Some(wid);
        }
        wid
    }

    /// 以既有 meta 插入一个 workspace（host 从 `.ridge` 还原时用）；首个则置 active。
    pub fn insert_workspace(&mut self, wid: Uuid, meta: WorkspaceMeta) {
        self.workspaces.insert(wid, meta);
        if self.active.is_none() {
            self.active = Some(wid);
        }
    }

    /// 删除 workspace 并返回其 meta。若删的是 active，则回退到任意剩余 workspace
    /// （悬空回退的图谱侧，host 侧 ViewState 回退见 Wave C）。
    pub fn remove_workspace(&mut self, wid: Uuid) -> Option<WorkspaceMeta> {
        let removed = self.workspaces.remove(&wid);
        if removed.is_some() && self.active == Some(wid) {
            self.active = self.workspaces.keys().next().copied();
        }
        removed
    }

    pub fn active(&self) -> Option<Uuid> {
        self.active
    }

    pub fn set_active(&mut self, wid: Uuid) -> Result<(), CoreError> {
        if self.workspaces.contains_key(&wid) {
            self.active = Some(wid);
            Ok(())
        } else {
            Err(Self::no_such_workspace(wid))
        }
    }

    pub fn workspace(&self, wid: Uuid) -> Option<&WorkspaceMeta> {
        self.workspaces.get(&wid)
    }

    pub fn workspace_mut(&mut self, wid: Uuid) -> Option<&mut WorkspaceMeta> {
        self.workspaces.get_mut(&wid)
    }

    pub fn workspace_ids(&self) -> impl Iterator<Item = &Uuid> {
        self.workspaces.keys()
    }

    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }

    // ── pane CRUD（委托给目标 workspace 的 PaneTree；workspace 缺失→InvalidArgs）──

    pub fn split(
        &mut self,
        wid: Uuid,
        target: Uuid,
        direction: SplitDirection,
    ) -> Result<Uuid, CoreError> {
        self.meta_mut(wid)?.pane_tree.split(target, direction)
    }

    pub fn close(&mut self, wid: Uuid, pane: Uuid) -> Result<(), CoreError> {
        let meta = self.meta_mut(wid)?;
        meta.pane_tree.close(pane)?;
        meta.locked_sizes.remove(&pane);
        Ok(())
    }

    pub fn dock(
        &mut self,
        wid: Uuid,
        source: Uuid,
        target: Uuid,
        region: DockRegion,
    ) -> Result<(), CoreError> {
        self.meta_mut(wid)?
            .pane_tree
            .dock_pane(source, target, region)
    }

    pub fn set_split_ratios_at_path(
        &mut self,
        wid: Uuid,
        path: &[usize],
        ratios: Vec<f32>,
    ) -> Result<(), CoreError> {
        self.meta_mut(wid)?
            .pane_tree
            .set_split_ratios_at_path(path, ratios)
    }

    pub fn set_split_ratios_batch(
        &mut self,
        wid: Uuid,
        updates: &[(Vec<usize>, Vec<f32>)],
    ) -> Result<(), CoreError> {
        self.meta_mut(wid)?
            .pane_tree
            .set_split_ratios_batch(updates)
    }

    pub fn layout(&self, wid: Uuid) -> Result<PaneNode, CoreError> {
        Ok(self.meta(wid)?.pane_tree.root.clone())
    }

    pub fn leaves(&self, wid: Uuid) -> Result<Vec<Uuid>, CoreError> {
        Ok(self.meta(wid)?.pane_tree.get_all_leaves())
    }

    // ── 锁定尺寸（共享属性，last-write-wins）──

    /// 设置某 pane 的锁定渲染尺寸。pane 必须存在于该 workspace 的树里。
    pub fn set_locked_size(
        &mut self,
        wid: Uuid,
        pane: Uuid,
        cols: u16,
        rows: u16,
    ) -> Result<(), CoreError> {
        let meta = self.meta_mut(wid)?;
        if !meta.pane_tree.panes.contains_key(&pane) {
            return Err(CoreError::PaneNotFound(pane));
        }
        meta.locked_sizes.insert(pane, (cols, rows));
        Ok(())
    }

    pub fn locked_size(&self, wid: Uuid, pane: Uuid) -> Option<(u16, u16)> {
        self.workspaces
            .get(&wid)
            .and_then(|m| m.locked_sizes.get(&pane).copied())
    }

    // ── 内部 ──

    fn meta(&self, wid: Uuid) -> Result<&WorkspaceMeta, CoreError> {
        self.workspaces
            .get(&wid)
            .ok_or_else(|| Self::no_such_workspace(wid))
    }

    fn meta_mut(&mut self, wid: Uuid) -> Result<&mut WorkspaceMeta, CoreError> {
        self.workspaces
            .get_mut(&wid)
            .ok_or_else(|| Self::no_such_workspace(wid))
    }

    fn no_such_workspace(wid: Uuid) -> CoreError {
        CoreError::InvalidArgs(format!("workspace not found: {wid}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_seeds_single_leaf_and_active() {
        let mut g = WorkspaceGraph::new();
        let wid = g.create_workspace();
        assert_eq!(g.active(), Some(wid));
        assert_eq!(g.leaves(wid).unwrap().len(), 1);
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn split_then_close_round_trips_leaf_count() {
        let mut g = WorkspaceGraph::new();
        let wid = g.create_workspace();
        let root = g.leaves(wid).unwrap()[0];
        let new = g.split(wid, root, SplitDirection::Horizontal).unwrap();
        assert_eq!(g.leaves(wid).unwrap().len(), 2);
        g.close(wid, new).unwrap();
        assert_eq!(g.leaves(wid).unwrap().len(), 1);
    }

    #[test]
    fn missing_workspace_is_invalid_args() {
        let mut g = WorkspaceGraph::new();
        let ghost = Uuid::new_v4();
        let err = g
            .split(ghost, Uuid::new_v4(), SplitDirection::Vertical)
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidArgs(_)));
        assert!(g.layout(ghost).is_err());
    }

    #[test]
    fn locked_size_set_get_and_missing_pane_errors() {
        let mut g = WorkspaceGraph::new();
        let wid = g.create_workspace();
        let pane = g.leaves(wid).unwrap()[0];
        assert!(g.locked_size(wid, pane).is_none());
        g.set_locked_size(wid, pane, 120, 40).unwrap();
        assert_eq!(g.locked_size(wid, pane), Some((120, 40)));
        // setting on a non-existent pane is PaneNotFound (verbatim message path)
        let err = g.set_locked_size(wid, Uuid::new_v4(), 80, 24).unwrap_err();
        assert!(matches!(err, CoreError::PaneNotFound(_)));
    }

    #[test]
    fn close_drops_locked_size() {
        let mut g = WorkspaceGraph::new();
        let wid = g.create_workspace();
        let root = g.leaves(wid).unwrap()[0];
        let new = g.split(wid, root, SplitDirection::Horizontal).unwrap();
        g.set_locked_size(wid, new, 100, 30).unwrap();
        assert_eq!(g.locked_size(wid, new), Some((100, 30)));
        g.close(wid, new).unwrap();
        assert_eq!(
            g.locked_size(wid, new),
            None,
            "close must drop the pane's locked size"
        );
    }

    #[test]
    fn remove_active_workspace_falls_back() {
        let mut g = WorkspaceGraph::new();
        let a = g.create_workspace();
        let b = g.create_workspace();
        assert_eq!(g.active(), Some(a));
        let removed = g.remove_workspace(a);
        assert!(removed.is_some());
        assert_eq!(
            g.active(),
            Some(b),
            "removing active falls back to a remaining workspace"
        );
        // removing the last one clears active.
        g.remove_workspace(b);
        assert_eq!(g.active(), None);
        assert!(g.is_empty());
    }

    #[test]
    fn dock_center_swaps_two_panes() {
        let mut g = WorkspaceGraph::new();
        let wid = g.create_workspace();
        let root = g.leaves(wid).unwrap()[0];
        let new = g.split(wid, root, SplitDirection::Horizontal).unwrap();
        // Center dock = swap; both leaves still present, no error.
        g.dock(wid, root, new, DockRegion::Center).unwrap();
        let leaves = g.leaves(wid).unwrap();
        assert_eq!(leaves.len(), 2);
        assert!(leaves.contains(&root) && leaves.contains(&new));
    }
}
