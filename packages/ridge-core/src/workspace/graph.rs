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
//! Kernel 持有并持久化此图谱；统一 Host 向 desktop / CLI / Remote 投影同一实体。
//! CHG-070 正迁出旧桌面 `AppState`，展示层镜像不成为另一份工作区权威。
//!
//! ## 不发事件
//!
//! 图谱方法是**纯**的（mutate + 返回结构结果），**不**自己 emit。调用方（host）在
//! **释放锁之后**经 [`crate::ctx::EventSink`] 广播结构变更——保留"先解锁再发"的安全序，
//! 并让 host 保留 respawn-vs-suppress 决策（详定稿 §1.5 B）。

use crate::error::CoreError;
use crate::workspace::pane_tree::{DockRegion, PaneNode, PaneTree, SplitDirection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 单个 workspace 的图谱数据：布局树 + 名称 + 每 pane 锁定渲染尺寸（共享属性）。
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct ForeignPaneBinding {
    pub host_id:String,
    pub host_label:String,
    pub kind:crate::remote::HostKind,
    pub workspace_id:Uuid,
    pub session_id:Uuid,
    pub title:String,
    pub cwd:Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    #[serde(default,skip_serializing_if="HashMap::is_empty")]
    pub foreign_panes:HashMap<Uuid,ForeignPaneBinding>,
    #[serde(default)]
    pub display_seq: u64,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub file: Option<super::file::FileAssociation>,
    pub pane_tree: PaneTree,
    pub name: Option<String>,
    /// 每 pane 的**锁定渲染尺寸** `(cols, rows)`（D11 共享属性，last-write-wins，
    /// 随 attach 快照下发）。与桌面 split-target 选择用的"实测尺寸"是两回事——后者
    /// 是 GUI 旁表，不进图谱。
    pub locked_sizes: HashMap<Uuid, (u16, u16)>,
}

/// 一份权威的 workspaces 集合 + 当前活动 workspace。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceGraph {
    workspaces: HashMap<Uuid, WorkspaceMeta>,
    active: Option<Uuid>,
    #[serde(default)]
    order: Vec<Uuid>,
    #[serde(default)]
    next_display_seq: u64,
}

impl WorkspaceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// 新建一个空 workspace（单 leaf 树）；若是首个则置为 active。返回新 workspace id。
    pub fn create_workspace(&mut self) -> Uuid {
        self.normalize_metadata();
        let wid = Uuid::new_v4();
        self.workspaces.insert(wid, WorkspaceMeta { display_seq: self.next_display_seq, ..Default::default() });
        self.next_display_seq = self.next_display_seq.saturating_add(1);
        self.order.push(wid);
        if self.active.is_none() {
            self.active = Some(wid);
        }
        wid
    }

    /// 以既有 meta 插入一个 workspace（host 从 `.ridge` 还原时用）；首个则置 active。
    pub fn insert_workspace(&mut self, wid: Uuid, mut meta: WorkspaceMeta) {
        self.normalize_metadata();
        if meta.display_seq == 0 {
            meta.display_seq = self.workspaces.get(&wid).map(|meta|meta.display_seq).unwrap_or(self.next_display_seq);
        }
        self.next_display_seq = self.next_display_seq.max(meta.display_seq.saturating_add(1));
        if !self.order.contains(&wid) { self.order.push(wid); }
        self.workspaces.insert(wid, meta);
        if self.active.is_none() {
            self.active = Some(wid);
        }
    }

    /// 删除 workspace 并返回其 meta。若删的是 active，则回退到任意剩余 workspace
    /// （悬空回退的图谱侧，host 侧 ViewState 回退见 Wave C）。
    pub fn remove_workspace(&mut self, wid: Uuid) -> Option<WorkspaceMeta> {
        self.normalize_metadata();
        let removed = self.workspaces.remove(&wid);
        self.order.retain(|id| *id != wid);
        if removed.is_some() && self.active == Some(wid) {
            self.active = self.order.first().copied();
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
        let positions: HashMap<_,_> = self.order.iter().enumerate().map(|(index,id)|(id,index)).collect();
        let mut ids: Vec<_> = self.workspaces.keys().collect();
        ids.sort_by_key(|id|(positions.get(id).copied().unwrap_or(usize::MAX), **id));
        ids.into_iter()
    }

    fn normalize_metadata(&mut self) {
        self.order = self.workspace_ids().copied().collect();
        self.next_display_seq = self.next_display_seq.max(self.workspaces.values().map(|meta|meta.display_seq).max().unwrap_or(0).saturating_add(1)).max(1);
        for id in &self.order {
            let meta = self.workspaces.get_mut(id).expect("ordered workspace exists");
            if meta.display_seq == 0 { meta.display_seq = self.next_display_seq; self.next_display_seq = self.next_display_seq.saturating_add(1); }
        }
    }

    pub fn reorder(&mut self, from: usize, to: usize) -> Result<(), CoreError> {
        if from >= self.len() || to >= self.len() { return Err(CoreError::InvalidArgs("workspace index out of range".into())); }
        self.normalize_metadata();
        let id = self.order.remove(from);
        self.order.insert(to, id);
        Ok(())
    }

    pub fn rename(&mut self, id: Uuid, name: String) -> Result<(), CoreError> {
        self.meta_mut(id)?.name = Some(name);
        Ok(())
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
        meta.foreign_panes.remove(&pane);
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

    /// Move one leaf without recreating its identity. Returns whether the source workspace closed.
    pub fn dock_between(&mut self, from: Uuid, to: Uuid, source: Uuid, target: Uuid, region: DockRegion) -> Result<bool, CoreError> {
        if from == to { self.dock(from, source, target, region)?; return Ok(false); }
        if !self.leaves(from)?.contains(&source) { return Err(CoreError::PaneNotFound(source)); }
        if !self.leaves(to)?.contains(&target) { return Err(CoreError::PaneNotFound(target)); }
        if self.meta(to)?.pane_tree.panes.contains_key(&source) { return Err(CoreError::InvalidArgs("duplicate pane identity".into())); }
        let closed = self.leaves(from)?.len() == 1;
        let mut next = self.clone();
        let src = next.meta_mut(from)?;
        if !src.pane_tree.detach_external_leaf(source) { return Err(CoreError::PaneNotFound(source)); }
        let pane = src.pane_tree.panes.remove(&source).ok_or(CoreError::PaneNotFound(source))?;
        let size = src.locked_sizes.remove(&source);
        let foreign = src.foreign_panes.remove(&source);
        let dst = next.meta_mut(to)?;
        dst.pane_tree.panes.insert(source, pane);
        if let Some(size) = size { dst.locked_sizes.insert(source, size); }
        if let Some(foreign) = foreign { dst.foreign_panes.insert(source, foreign); }
        dst.pane_tree.attach_external_leaf(source, target, region)?;
        if closed {
            next.remove_workspace(from);
            next.set_active(to)?;
        }
        *self = next;
        Ok(closed)
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
    fn workspace_order_and_labels_survive_reorder_close_and_roundtrip() {
        let mut graph = WorkspaceGraph::new();
        let first = graph.create_workspace(); let second = graph.create_workspace(); let third = graph.create_workspace();
        let labels: Vec<_> = graph.workspace_ids().map(|id|graph.workspace(*id).unwrap().display_seq).collect();
        assert_eq!(labels, vec![1,2,3]);
        graph.rename(second,"second".into()).unwrap(); graph.reorder(2,0).unwrap();
        assert_eq!(graph.workspace_ids().copied().collect::<Vec<_>>(),vec![third,first,second]);
        let bytes = serde_json::to_vec(&graph).unwrap();
        let mut restored: WorkspaceGraph = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.workspace_ids().copied().collect::<Vec<_>>(),vec![third,first,second]);
        assert_eq!(restored.workspace(second).unwrap().name.as_deref(),Some("second"));
        assert_eq!(restored.workspace(third).unwrap().display_seq,3);
        restored.remove_workspace(third);
        let fourth = restored.create_workspace();
        assert_eq!(restored.workspace(fourth).unwrap().display_seq,4);
    }

    #[test]
    fn legacy_graph_read_is_pure_and_failed_reorder_changes_nothing() {
        let mut graph = WorkspaceGraph::new();graph.create_workspace();graph.create_workspace();
        let mut old = serde_json::to_value(&graph).unwrap();
        old.as_object_mut().unwrap().remove("order");old.as_object_mut().unwrap().remove("next_display_seq");
        for meta in old["workspaces"].as_object_mut().unwrap().values_mut() {meta.as_object_mut().unwrap().remove("display_seq");}
        let mut restored: WorkspaceGraph = serde_json::from_value(old).unwrap();
        let before=serde_json::to_value(&restored).unwrap();let ids=restored.workspace_ids().copied().collect::<Vec<_>>();
        assert_eq!(ids,restored.workspace_ids().copied().collect::<Vec<_>>());
        assert!(restored.reorder(0,2).is_err());
        assert_eq!(before,serde_json::to_value(&restored).unwrap());
        restored.reorder(1,0).unwrap();
        assert_eq!(restored.workspace_ids().copied().collect::<Vec<_>>(),vec![ids[1],ids[0]]);
    }

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

    #[test]
    fn dock_between_preserves_metadata_and_commits_only_valid_moves() {
        for region in [DockRegion::Left, DockRegion::Right, DockRegion::Top, DockRegion::Bottom, DockRegion::Center] {
            let mut graph = WorkspaceGraph::new();
            let from = graph.create_workspace();
            let to = graph.create_workspace();
            let third = graph.create_workspace();
            let source = graph.leaves(from).unwrap()[0];
            let remaining = graph.split(from, source, SplitDirection::Horizontal).unwrap();
            let target = graph.leaves(to).unwrap()[0];
            graph.set_locked_size(from, source, 103, 37).unwrap();
            let binding = ForeignPaneBinding { host_id: "retained-host".into(), host_label: "peer".into(), kind: crate::remote::HostKind::Remote,
                workspace_id: Uuid::new_v4(), session_id: Uuid::new_v4(), title: "existing".into(), cwd: None };
            graph.workspace_mut(from).unwrap().foreign_panes.insert(source, binding.clone());
            let pane = serde_json::to_value(&graph.workspace(from).unwrap().pane_tree.panes[&source]).unwrap();
            let before = serde_json::to_value(&graph).unwrap();
            assert!(graph.dock_between(from, to, source, Uuid::new_v4(), region.clone()).is_err());
            assert_eq!(serde_json::to_value(&graph).unwrap(), before);
            assert!(!graph.dock_between(from, to, source, target, region.clone()).unwrap());
            assert_eq!(graph.leaves(from).unwrap(), vec![remaining]);
            assert_eq!(graph.locked_size(to, source), Some((103, 37)));
            assert_eq!(graph.workspace(to).unwrap().foreign_panes[&source], binding);
            assert_eq!(serde_json::to_value(&graph.workspace(to).unwrap().pane_tree.panes[&source]).unwrap(), pane);
            graph.set_active(third).unwrap();
            assert!(graph.dock_between(from, to, remaining, target, region).unwrap());
            assert!(graph.workspace(from).is_none());
            assert_eq!(graph.active(), Some(to));
            assert_eq!(graph.leaves(to).unwrap().len(), 3);
        }
    }

    #[test]
    fn graph_round_trips_for_kernel_persistence() {
        let mut graph = WorkspaceGraph::new();
        let workspace = graph.create_workspace();
        let pane = graph.leaves(workspace).unwrap()[0];
        graph.set_locked_size(workspace, pane, 120, 40).unwrap();
        let restored: WorkspaceGraph =
            serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
        assert_eq!(restored.active(), Some(workspace));
        assert_eq!(restored.locked_size(workspace, pane), Some((120, 40)));
    }
}
