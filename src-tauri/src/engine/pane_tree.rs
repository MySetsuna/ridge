// src-tauri/src/engine/pane_tree.rs
use std::collections::HashMap;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use crate::types::PaneMode;
use crate::utils::error::AppError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
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
    // 可扩展：pty_handle、editor_state 等
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
        panes.insert(root_id, Pane { id: root_id, mode: PaneMode::Terminal });

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
            });
            Ok(new_pane_id)
        } else {
            Err(AppError::PaneNotFound(target_id))
        }
    }

    /// Resize（递归找到包含该 pane 的 Split，调整 ratios）
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

    /// Close（删除 Leaf，并把兄弟节点提升或调整 ratio）
    pub fn close(&mut self, pane_id: Uuid) -> Result<(), AppError> {
        fn recurse(node: &mut PaneNode, pane_id: Uuid) -> bool {
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
                    if matches!(children[i], PaneNode::Split { .. }) && recurse(&mut children[i], pane_id)
                    {
                        return true;
                    }
                    i += 1;
                }
            }
            false
        }

        if recurse(&mut self.root, pane_id) {
            self.panes.remove(&pane_id);
            Ok(())
        } else {
            Err(AppError::PaneNotFound(pane_id))
        }
    }

    /// 获取当前布局（供前端递归渲染 SplitContainer 使用）
    pub fn get_layout(&self) -> PaneNode {
        self.root.clone()
    }

    /// 查找某个 Pane 的完整路径（Fiber return 指针模拟，用于调试/快捷键）
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