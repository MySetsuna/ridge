use serde::Serialize;

/// 与前端 GitGraph 约定一致；后续可换为 git2 拓扑数据。
#[derive(Clone, Debug, Serialize)]
pub struct CommitNode {
    pub lane: usize,
    pub parents: Vec<usize>,
    pub message: String,
}

#[tauri::command]
pub fn get_git_graph(_repo_path: String) -> Result<Vec<CommitNode>, String> {
    Ok(vec![])
}
