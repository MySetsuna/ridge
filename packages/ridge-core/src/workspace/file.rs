//! Portable `.ridge` v1 payload shared by service and presentation adapters.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
pub const RIDGE_FILE_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileAssociation {
    pub path: String,
    #[serde(default)]
    pub git_repos: Vec<String>,
    #[serde(default)]
    pub index_path: Option<String>,
    #[serde(default)]
    pub pane_titles: HashMap<String, String>,
}

pub fn read(path: &Path) -> Result<RidgeFile, String> {
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("ridge"))
    {
        return Err("workspace file must use .ridge extension".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let payload: RidgeFile =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid .ridge file: {error}"))?;
    validate(&payload)?;
    Ok(payload)
}

pub fn validate(payload: &RidgeFile) -> Result<super::pane_tree::PaneTree, String> {
    if payload.version != RIDGE_FILE_VERSION {
        return Err(format!("unsupported .ridge version {}", payload.version));
    }
    let tree: super::pane_tree::PaneTree =
        serde_json::from_value(payload.pane_tree.clone()).map_err(|error| error.to_string())?;
    fn walk(
        node: &super::pane_tree::PaneNode,
        tree: &super::pane_tree::PaneTree,
        seen: &mut std::collections::HashSet<uuid::Uuid>,
    ) -> Result<(), String> {
        use super::pane_tree::PaneNode;
        match node {
            PaneNode::Leaf(id) => {
                if !seen.insert(*id) || tree.panes.get(id).is_none_or(|pane| pane.id != *id) {
                    return Err("invalid or duplicate workspace pane identity".into());
                }
            }
            PaneNode::Split {
                children, ratios, ..
            } => {
                if children.len() < 2
                    || ratios.len() != children.len()
                    || ratios.iter().any(|r| !r.is_finite() || *r < 0.0)
                    || ratios.iter().sum::<f32>() <= 0.0
                {
                    return Err("invalid workspace split geometry".into());
                }
                for child in children {
                    walk(child, tree, seen)?;
                }
            }
        }
        Ok(())
    }
    let mut leaves=std::collections::HashSet::new();
    walk(&tree.root, &tree, &mut leaves)?;
    for (pane,binding) in &payload.foreign_panes {
        if !leaves.contains(pane)||binding.workspace_id.is_nil()||binding.session_id.is_nil()||binding.host_id.trim().is_empty()||binding.host_id.len()>4096||binding.host_label.len()>4096||binding.title.len()>65536||binding.cwd.as_ref().is_some_and(|cwd|cwd.len()>65536){return Err("invalid foreign workspace reference".into());}
    }
    Ok(tree)
}

pub fn write(path: &Path, payload: &RidgeFile) -> Result<(), String> {
    validate(payload)?;
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("ridge"))
    {
        return Err("workspace file must use .ridge extension".into());
    }
    let bytes = serde_json::to_vec_pretty(payload).map_err(|error| error.to_string())?;
    write_atomic(path, &bytes).map_err(|error| error.to_string())
}

/// Unique same-directory temporary files avoid concurrent writers sharing a
/// `.tmp` name. Original content remains intact until a complete flushed rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&temporary)?;
        output.write_all(bytes)?;
        output.sync_all()?;
        drop(output);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RidgeFile {
    #[serde(default,skip_serializing_if="HashMap::is_empty")]
    pub foreign_panes:HashMap<uuid::Uuid,super::graph::ForeignPaneBinding>,
    pub version: u32,
    pub name: String,
    pub saved_at: String,
    pub pane_tree: serde_json::Value,
    #[serde(default)]
    pub git_repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_path: Option<String>,
    /// Per-pane teammate display names keyed by pane UUID string. Written by
    /// Claude Code's `new-window -n` / `split-window -n` and surfaced in the
    /// Ridge pane header. Runtime state (busy / idle, agent_id) is deliberately
    /// NOT persisted — it's session-scoped and would lie across restarts.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pane_titles: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_reference_must_name_a_live_layout_leaf(){
        let (_,mut file)=fixture();
        let binding=super::super::graph::ForeignPaneBinding{host_id:"peer".into(),host_label:"Peer".into(),kind:crate::remote::HostKind::Remote,workspace_id:uuid::Uuid::new_v4(),session_id:uuid::Uuid::new_v4(),title:"terminal".into(),cwd:None};
        let pane=validate(&file).unwrap().get_all_leaves()[0];
        file.foreign_panes.insert(pane,binding.clone());assert!(validate(&file).is_ok());
        file.foreign_panes.insert(uuid::Uuid::new_v4(),binding);
        assert_eq!(validate(&file).unwrap_err(),"invalid foreign workspace reference");
    }
    fn fixture() -> (std::path::PathBuf, RidgeFile) {
        let root = std::env::temp_dir().join(format!("ridge-file-codec-{}", uuid::Uuid::new_v4()));
        let value = RidgeFile {
            foreign_panes:HashMap::new(),
            version: 1,
            name: "保存的工作区".into(),
            saved_at: "2026-10-11T00:00:00Z".into(),
            pane_tree: serde_json::to_value(super::super::pane_tree::PaneTree::default()).unwrap(),
            git_repos: vec!["repo".into()],
            index_path: Some("index".into()),
            pane_titles: HashMap::new(),
        };
        (root, value)
    }
    #[test]
    fn legacy_unknown_fields_are_read_without_rewriting_the_source() {
        let (root, payload) = fixture();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("legacy.ridge");
        let mut value = serde_json::to_value(payload).unwrap();
        value["serialized_panes"] = serde_json::json!({"historical":"record"});
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let decoded = read(&path).unwrap();
        assert_eq!(decoded.name, "保存的工作区");
        assert_eq!(decoded.git_repos, vec!["repo"]);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn invalid_save_keeps_the_existing_document_and_read_creates_nothing() {
        let (root, mut payload) = fixture();
        let path = root.join("saved.ridge");
        assert!(read(&path).is_err());
        assert!(!root.exists());
        write(&path, &payload).unwrap();
        let original = std::fs::read(&path).unwrap();
        payload.version = 2;
        assert!(write(&path, &payload).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        payload.version = 1;
        payload.pane_tree = serde_json::json!({});
        assert!(write(&path, &payload).is_err());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

/// Stable, non-recursive discovery; actual files remain under caller/operator roots.
pub fn first_in_directory(root:&Path,scopes:&[&crate::sandbox::RootScope])->crate::CoreResult<serde_json::Value> {
    if scopes.iter().any(|scope|!scope.is_allowed(&root.to_string_lossy())) {return Err(crate::CoreError::OutsideSandbox);}
    let entries=match std::fs::read_dir(root) {
        Ok(entries)=>entries,
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return Ok(serde_json::Value::Null),
        Err(error)=>return Err(crate::CoreError::internal(error)),
    };
    let mut files=Vec::new();
    for entry in entries {
        let entry=entry.map_err(crate::CoreError::internal)?;
        let path=entry.path();
        if path.extension().and_then(|value|value.to_str())!=Some("ridge") {continue;}
        if scopes.iter().any(|scope|!scope.is_allowed(&path.to_string_lossy())) {return Err(crate::CoreError::OutsideSandbox);}
        if entry.file_type().map_err(crate::CoreError::internal)?.is_file() {files.push(path);}
    }
    files.sort();
    Ok(serde_json::json!(files.into_iter().next().map(|path|path.to_string_lossy().into_owned())))
}
