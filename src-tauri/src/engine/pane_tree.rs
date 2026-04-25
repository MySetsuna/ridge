// src-tauri/src/engine/pane_tree.rs
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;
use serde::{Deserialize, Serialize};
use crate::types::PaneMode;
use crate::utils::error::AppError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// 拖拽停靠：左右为水平分栏，上下为垂直分栏；中心为两窗格互换位置（PTY 随 id 不变）。
#[derive(Clone, Debug)]
pub enum DockRegion {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PaneNode {
    Leaf(Uuid),
    Split {
        direction: SplitDirection,
        children: Vec<PaneNode>,
        ratios: Vec<f32>,          // 每个 child 占父节点的百分比（总和=100）
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pane {
    pub id: Uuid,
    pub mode: PaneMode,
    /// Working directory reported via OSC 7 by the PTY shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// 用于持久化的终端类型（pwsh/cmd/bash/git-bash/wsl/zsh 等），
    /// 由 `create_pane` 在首次 spawn 时写入；重建 .wind 工作区时按此重启同类 shell。
    /// 未显式指定时保留 None，表示使用平台默认（Windows=powershell, Unix=zsh）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_kind: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaneTree {
    pub root: PaneNode,
    pub panes: HashMap<Uuid, Pane>,   // 所有 Leaf 的元数据（Fiber 风格的 alternate 存储）
}

impl PaneTree {
    /// 创建初始根节点（Fiber 根 Fiber）
    pub fn new() -> Self {
        let root_id = Uuid::new_v4();
        let mut panes = HashMap::new();
        panes.insert(root_id, Pane { id: root_id, mode: PaneMode::Terminal, cwd: None, shell_kind: None });

        Self {
            root: PaneNode::Leaf(root_id),
            panes,
        }
    }

    /// ==================== React Fiber 风格的核心算法 ====================

    /// Phase 1: Find + Reconcile（递归查找目标 Leaf 并生成新的 Wip 节点）
    fn reconcile_split(
        node: &PaneNode,
        target_id: Uuid,
        direction: SplitDirection,
        new_pane_id: Uuid,
    ) -> Option<PaneNode> {
        match node {
            PaneNode::Leaf(id) if *id == target_id => {
                // 找到目标 Leaf → 替换为 Split Fiber
                Some(PaneNode::Split {
                    direction,
                    children: vec![
                        PaneNode::Leaf(*id),           // 旧 Leaf 保留
                        PaneNode::Leaf(new_pane_id),   // 新 Leaf
                    ],
                    ratios: vec![50.0, 50.0],
                })
            }
            PaneNode::Split { direction: d, children, ratios } => {
                // 递归遍历 children（Fiber 的 child/sibling 遍历）
                let mut new_children = Vec::with_capacity(children.len());
                let mut found = false;

                for child in children {
                    if let Some(new_child) =
                        Self::reconcile_split(child, target_id, direction.clone(), new_pane_id)
                    {
                        new_children.push(new_child);
                        found = true;
                    } else {
                        new_children.push(child.clone());
                    }
                }

                if found {
                    Some(PaneNode::Split {
                        direction: d.clone(),
                        children: new_children,
                        ratios: ratios.clone(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Phase 2: Commit（原子替换 root）
    pub fn split(&mut self, target_id: Uuid, direction: SplitDirection) -> Result<Uuid, AppError> {
        let new_pane_id = Uuid::new_v4();

        // 创建新的 Wip 树
        if let Some(new_root) = Self::reconcile_split(&self.root, target_id, direction, new_pane_id) {
            self.root = new_root;
            self.panes.insert(new_pane_id, Pane {
                id: new_pane_id,
                mode: PaneMode::Terminal,
                cwd: None,
                shell_kind: None,
            });
            Ok(new_pane_id)
        } else {
            Err(AppError::PaneNotFound(target_id))
        }
    }

    /// Resize（递归找到包含该 pane 的 Split，调整 ratios）
    #[allow(dead_code)] // public API; callers do ratio updates via set_split_ratios_at_path
    pub fn resize(&mut self, pane_id: Uuid, new_ratio: f32) -> Result<(), AppError> {
        fn recurse(node: &mut PaneNode, pane_id: Uuid, new_ratio: f32) -> bool {
            if let PaneNode::Split { children, ratios, .. } = node {
                for (i, child) in children.iter_mut().enumerate() {
                    if let PaneNode::Leaf(id) = &*child {
                        if *id == pane_id {
                            // 找到父 Split，调整当前 child 的 ratio
                            if i < ratios.len() {
                                ratios[i] = new_ratio.clamp(10.0, 90.0);
                                // 重新归一化其他 ratios
                                let sum: f32 = ratios.iter().sum();
                                if sum > 0.0 {
                                    for r in ratios.iter_mut() {
                                        *r = (*r / sum) * 100.0;
                                    }
                                }
                                return true;
                            }
                        }
                    } else if recurse(child, pane_id, new_ratio) {
                        return true;
                    }
                }
            }
            false
        }

        if recurse(&mut self.root, pane_id, new_ratio) {
            Ok(())
        } else {
            Err(AppError::PaneNotFound(pane_id))
        }
    }

    /// 从根起依次取 `Split` 的第 `path[i]` 个子节点，定位到目标 `Split`，写入子占比（与前端 `splitPath` 一致）。
    pub fn set_split_ratios_at_path(
        &mut self,
        path: &[usize],
        ratios: Vec<f32>,
    ) -> Result<(), AppError> {
        let next = self.validate_and_normalize_ratio_update(path, &ratios)?;
        let node = Self::mut_split_at_path(&mut self.root, path)?;
        let PaneNode::Split { ratios: rs, .. } = node else {
            return Err(AppError::PtyError("target is not a split".into()));
        };
        *rs = next;
        Ok(())
    }

    /// 原子批量更新多个 split 的 ratios：先全量校验，后统一写入，避免部分成功。
    pub fn set_split_ratios_batch(
        &mut self,
        updates: &[(Vec<usize>, Vec<f32>)],
    ) -> Result<(), AppError> {
        let mut normalized_updates: Vec<(Vec<usize>, Vec<f32>)> = Vec::with_capacity(updates.len());
        for (path, ratios) in updates {
            let normalized = self.validate_and_normalize_ratio_update(path, ratios)?;
            normalized_updates.push((path.clone(), normalized));
        }
        for (path, ratios) in normalized_updates {
            let node = Self::mut_split_at_path(&mut self.root, &path)?;
            let PaneNode::Split { ratios: rs, .. } = node else {
                return Err(AppError::PtyError("target is not a split".into()));
            };
            *rs = ratios;
        }
        Ok(())
    }

    fn validate_and_normalize_ratio_update(
        &self,
        path: &[usize],
        ratios: &[f32],
    ) -> Result<Vec<f32>, AppError> {
        let node = Self::split_at_path(&self.root, path)?;
        let PaneNode::Split { children, .. } = node else {
            return Err(AppError::PtyError("target is not a split".into()));
        };
        if children.len() != ratios.len() {
            return Err(AppError::PtyError(format!(
                "ratios len {} != children {}",
                ratios.len(),
                children.len()
            )));
        }
        let sum: f32 = ratios.iter().sum();
        if sum <= 1e-6 {
            return Err(AppError::PtyError("ratios sum is zero".into()));
        }
        Ok(ratios.iter().map(|r| (*r / sum) * 100.0).collect())
    }

    fn split_at_path<'a>(node: &'a PaneNode, path: &[usize]) -> Result<&'a PaneNode, AppError> {
        let mut cur = node;
        for &idx in path {
            match cur {
                PaneNode::Split { children, .. } => {
                    cur = children.get(idx).ok_or_else(|| {
                        AppError::PtyError(format!("split path out of range at child index {idx}"))
                    })?;
                }
                _ => {
                    return Err(AppError::PtyError(
                        "split path crosses a non-split node".into(),
                    ));
                }
            }
        }
        match cur {
            PaneNode::Split { .. } => Ok(cur),
            _ => Err(AppError::PtyError(
                "split path does not end on a split".into(),
            )),
        }
    }

    fn mut_split_at_path<'a>(
        node: &'a mut PaneNode,
        path: &[usize],
    ) -> Result<&'a mut PaneNode, AppError> {
        let mut cur = node;
        for &idx in path {
            match cur {
                PaneNode::Split { children, .. } => {
                    cur = children.get_mut(idx).ok_or_else(|| {
                        AppError::PtyError(format!("split path out of range at child index {idx}"))
                    })?;
                }
                _ => {
                    return Err(AppError::PtyError(
                        "split path crosses a non-split node".into(),
                    ));
                }
            }
        }
        match cur {
            PaneNode::Split { .. } => Ok(cur),
            _ => Err(AppError::PtyError(
                "split path does not end on a split".into(),
            )),
        }
    }

    /// 从树中摘掉指定 Leaf（不删 `panes` 元数据、不关 PTY）。
    fn remove_leaf_from_structure(node: &mut PaneNode, pane_id: Uuid) -> bool {
        if let PaneNode::Split { children, ratios, .. } = node {
            let mut i = 0;
            while i < children.len() {
                let hit = matches!(
                    &children[i],
                    PaneNode::Leaf(id) if *id == pane_id
                );
                if hit {
                    children.remove(i);
                    ratios.remove(i);
                    if children.len() == 1 {
                        let only_child = children.remove(0);
                        *node = only_child;
                    } else if !ratios.is_empty() {
                        let sum: f32 = ratios.iter().sum();
                        if sum > 0.0 {
                            for r in ratios.iter_mut() {
                                *r = (*r / sum) * 100.0;
                            }
                        }
                    }
                    return true;
                }
                if matches!(children[i], PaneNode::Split { .. })
                    && Self::remove_leaf_from_structure(&mut children[i], pane_id)
                {
                    return true;
                }
                i += 1;
            }
        }
        false
    }

    /// Close（删除 Leaf，并把兄弟节点提升或调整 ratio）
    pub fn close(&mut self, pane_id: Uuid) -> Result<(), AppError> {
        if Self::remove_leaf_from_structure(&mut self.root, pane_id) {
            self.panes.remove(&pane_id);
            Ok(())
        } else {
            Err(AppError::PaneNotFound(pane_id))
        }
    }

    /// 仅从布局树移除窗格 id，保留 `panes` 与 PTY（用于拖拽重组）。
    pub fn detach_leaf(&mut self, pane_id: Uuid) -> Result<(), AppError> {
        let leaves = self.get_all_leaves();
        if leaves.len() <= 1 {
            return Err(AppError::PtyError("Cannot detach the only pane".into()));
        }
        if !leaves.contains(&pane_id) {
            return Err(AppError::PaneNotFound(pane_id));
        }
        if Self::remove_leaf_from_structure(&mut self.root, pane_id) {
            Ok(())
        } else {
            Err(AppError::PaneNotFound(pane_id))
        }
    }

    fn replace_leaf_with_subtree(
        node: &mut PaneNode,
        leaf_id: Uuid,
        replacement: PaneNode,
    ) -> bool {
        match node {
            PaneNode::Leaf(id) if *id == leaf_id => {
                *node = replacement;
                true
            }
            PaneNode::Split { children, .. } => {
                for child in children.iter_mut() {
                    if Self::replace_leaf_with_subtree(child, leaf_id, replacement.clone()) {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn swap_two_leaf_ids_in_tree(node: &mut PaneNode, a: Uuid, b: Uuid) {
        match node {
            PaneNode::Leaf(id) => {
                if *id == a {
                    *id = b;
                } else if *id == b {
                    *id = a;
                }
            }
            PaneNode::Split { children, .. } => {
                for child in children.iter_mut() {
                    Self::swap_two_leaf_ids_in_tree(child, a, b);
                }
            }
        }
    }

    /// 互换两窗格在树中的位置（仅交换 id，PTY 绑定不变）。
    pub fn swap_leaves(&mut self, a: Uuid, b: Uuid) -> Result<(), AppError> {
        if a == b {
            return Ok(());
        }
        let leaves = self.get_all_leaves();
        if !leaves.contains(&a) {
            return Err(AppError::PaneNotFound(a));
        }
        if !leaves.contains(&b) {
            return Err(AppError::PaneNotFound(b));
        }
        Self::swap_two_leaf_ids_in_tree(&mut self.root, a, b);
        Ok(())
    }

    /// VS Code 式停靠：边为分栏，中心为两格互换。
    pub fn dock_pane(
        &mut self,
        source: Uuid,
        target: Uuid,
        region: DockRegion,
    ) -> Result<(), AppError> {
        if source == target {
            return Ok(());
        }
        self.panes
            .get(&source)
            .ok_or(AppError::PaneNotFound(source))?;
        self.panes
            .get(&target)
            .ok_or(AppError::PaneNotFound(target))?;

        match region {
            DockRegion::Center => self.swap_leaves(source, target),
            DockRegion::Left | DockRegion::Right | DockRegion::Top | DockRegion::Bottom => {
                let leaves = self.get_all_leaves();
                if !leaves.contains(&source) {
                    return Err(AppError::PaneNotFound(source));
                }
                if !leaves.contains(&target) {
                    return Err(AppError::PaneNotFound(target));
                }
                self.detach_leaf(source)?;
                let new_subtree = match region {
                    DockRegion::Left => PaneNode::Split {
                        direction: SplitDirection::Horizontal,
                        children: vec![PaneNode::Leaf(source), PaneNode::Leaf(target)],
                        ratios: vec![50.0, 50.0],
                    },
                    DockRegion::Right => PaneNode::Split {
                        direction: SplitDirection::Horizontal,
                        children: vec![PaneNode::Leaf(target), PaneNode::Leaf(source)],
                        ratios: vec![50.0, 50.0],
                    },
                    DockRegion::Top => PaneNode::Split {
                        direction: SplitDirection::Vertical,
                        children: vec![PaneNode::Leaf(source), PaneNode::Leaf(target)],
                        ratios: vec![50.0, 50.0],
                    },
                    DockRegion::Bottom => PaneNode::Split {
                        direction: SplitDirection::Vertical,
                        children: vec![PaneNode::Leaf(target), PaneNode::Leaf(source)],
                        ratios: vec![50.0, 50.0],
                    },
                    DockRegion::Center => unreachable!(),
                };
                if !Self::replace_leaf_with_subtree(&mut self.root, target, new_subtree) {
                    return Err(AppError::PtyError(
                        "dock attach failed: target leaf missing after detach".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    /// 获取当前布局（供前端递归渲染 SplitContainer 使用）
    #[allow(dead_code)] // exposed API; today the layout flows through commands/pane.rs::get_pane_layout instead
    pub fn get_layout(&self) -> PaneNode {
        self.root.clone()
    }

    /// 查找某个 Pane 的完整路径（Fiber return 指针模拟，用于调试/快捷键）
    #[allow(dead_code)] // path-style helpers planned for future keyboard-driven pane jumps
    pub fn find_path(&self, pane_id: Uuid) -> Option<Vec<Uuid>> {
        fn recurse(node: &PaneNode, pane_id: Uuid, path: &mut Vec<Uuid>) -> bool {
            match node {
                PaneNode::Leaf(id) if *id == pane_id => true,
                PaneNode::Split { children, .. } => {
                    for child in children {
                        path.push(pane_id); // 记录父节点
                        if recurse(child, pane_id, path) {
                            return true;
                        }
                        path.pop();
                    }
                    false
                }
                _ => false,
            }
        }
        let mut path = Vec::new();
        if recurse(&self.root, pane_id, &mut path) {
            Some(path)
        } else {
            None
        }
    }

    /// 获取所有 Leaf（用于前端批量创建 xterm 实例）
    pub fn get_all_leaves(&self) -> Vec<Uuid> {
        let mut leaves = Vec::new();
        fn recurse(node: &PaneNode, leaves: &mut Vec<Uuid>) {
            match node {
                PaneNode::Leaf(id) => leaves.push(*id),
                PaneNode::Split { children, .. } => {
                    for child in children {
                        recurse(child, leaves);
                    }
                }
            }
        }
        recurse(&self.root, &mut leaves);
        leaves
    }
}
#[cfg(test)]
mod tests {
use super::*;

#[test]
fn pane_serde_roundtrip_without_cwd() {
    let pane = Pane { id: Uuid::new_v4(), mode: PaneMode::Terminal, cwd: None, shell_kind: None };
    let json = serde_json::to_string(&pane).unwrap();
    let deserialized: Pane = serde_json::from_str(&json).unwrap();
    assert!(deserialized.cwd.is_none());
    assert_eq!(deserialized.id, pane.id);
}

#[test]
fn pane_serde_roundtrip_with_unix_cwd() {
    let pane = Pane {
        id: Uuid::new_v4(),
        mode: PaneMode::Terminal,
        cwd: Some(PathBuf::from("/home/user/projects")),
        shell_kind: None,
    };
    let json = serde_json::to_string(&pane).unwrap();
    assert!(json.contains("/home/user/projects"));
    let deserialized: Pane = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.cwd.as_ref().unwrap().to_str(), Some("/home/user/projects"));
}

#[test]
fn pane_serde_roundtrip_with_windows_cwd() {
    let pane = Pane {
        id: Uuid::new_v4(),
        mode: PaneMode::Terminal,
        cwd: Some(PathBuf::from("C:/Users/Alice/code")),
        shell_kind: None,
    };
    let json = serde_json::to_string(&pane).unwrap();
    let deserialized: Pane = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.cwd.as_ref().unwrap().to_str(), Some("C:/Users/Alice/code"));
}

#[test]
fn pane_deserializes_cwd_field_explicitly() {
    let json = r#"{"id":"00000000-0000-0000-0000-000000000001","mode":"Terminal","cwd":"/tmp/test"}"#;
    let deserialized: Pane = serde_json::from_str(json).unwrap();
    assert_eq!(deserialized.cwd.as_ref().unwrap().to_str(), Some("/tmp/test"));
}

#[test]
fn pane_deserializes_missing_cwd_as_none() {
    let json = r#"{"id":"00000000-0000-0000-0000-000000000001","mode":"Terminal"}"#;
    let deserialized: Pane = serde_json::from_str(json).unwrap();
    assert!(deserialized.cwd.is_none());
}

#[test]
fn pane_serializes_cwd_none_omitted_by_skip_serializing() {
    let pane = Pane { id: Uuid::new_v4(), mode: PaneMode::Terminal, cwd: None, shell_kind: None };
    let json = serde_json::to_string(&pane).unwrap();
    assert!(!json.contains("cwd"));
}

#[test]
fn pane_tree_new_has_no_cwd() {
    let tree = PaneTree::new();
    for pane in tree.panes.values() {
        assert!(pane.cwd.is_none());
    }
}

#[test]
fn pane_tree_split_preserves_cwd_none_on_new_pane() {
    let mut tree = PaneTree::new();
    let root_id = tree.get_all_leaves()[0];
    let new_id = tree.split(root_id, SplitDirection::Horizontal).unwrap();
    assert!(tree.panes.get(&new_id).unwrap().cwd.is_none());
}
}
