//! Runtime-owned legacy project history and client-local project selection.
use crate::project_store::{Project, ProjectStore};
use ridge_core::{CoreError, CoreResult, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Project> for ProjectInfo {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            name: Path::new(&project.path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.path.clone()),
            path: project.path,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}
pub struct Projects {
    store: ProjectStore,
    selections: Mutex<HashMap<String, (PathBuf, Instant)>>,
}
impl Projects {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        Ok(Self {
            store: ProjectStore::new(&path)
                .map_err(|error| format!("open project history {}: {error}", path.display()))?,
            selections: Default::default(),
        })
    }
    pub(crate) fn dispatch(
        &self,
        method: &str,
        args: &Value,
        ctx: &Ctx,
        operator: &ridge_core::sandbox::RootScope,
    ) -> Option<CoreResult<Value>> {
        if !matches!(
            method,
            "open_project" | "get_recent_projects" | "remove_project" | "get_current_project"
        ) {
            return None;
        }
        Some((|| {
            let permitted = |path: &str| {
                operator.is_allowed(path) && ctx.capabilities().root_scope().is_allowed(path)
            };
            let connection = ctx.connection().as_ref().ok_or_else(|| {
                CoreError::InvalidArgs("project selection requires a client identity".into())
            })?;
            let mut selections = self
                .selections
                .lock()
                .map_err(|_| CoreError::internal("project selections unavailable"))?;
            selections.retain(|_, (_, updated)| updated.elapsed() < Duration::from_secs(24 * 3600));
            match method {
                "open_project" => {
                    let path = args["path"]
                        .as_str()
                        .ok_or_else(|| CoreError::InvalidArgs("missing project path".into()))?;
                    if !Path::new(path).is_absolute()
                        || ridge_core::dispatch::path_has_traversal(path)
                    {
                        return Err(CoreError::InvalidArgs(
                            "project path must be absolute without traversal".into(),
                        ));
                    }
                    if !permitted(path) {
                        return Err(CoreError::OutsideSandbox);
                    }
                    if !selections.contains_key(connection) && selections.len() >= 1024 {
                        return Err(CoreError::InvalidArgs(
                            "project client capacity reached".into(),
                        ));
                    }
                    let project = self
                        .store
                        .open_project(path)
                        .map_err(|error| CoreError::io(error.to_string()))?;
                    selections.insert(connection.clone(), (PathBuf::from(path), Instant::now()));
                    serde_json::to_value(ProjectInfo::from(project)).map_err(CoreError::internal)
                }
                "get_current_project" => Ok(selections
                    .get_mut(connection)
                    .and_then(|(path, updated)| {
                        let text = path.to_string_lossy();
                        if !permitted(&text) {
                            return None;
                        }
                        *updated = Instant::now();
                        Some(json!(text))
                    })
                    .unwrap_or(Value::Null)),
                "get_recent_projects" => {
                    let projects = self
                        .store
                        .get_recent_projects_where(10, permitted)
                        .map_err(|error| CoreError::io(error.to_string()))?;
                    serde_json::to_value(
                        projects
                            .into_iter()
                            .map(ProjectInfo::from)
                            .collect::<Vec<_>>(),
                    )
                    .map_err(CoreError::internal)
                }
                _ => {
                    let id = args["projectId"]
                        .as_i64()
                        .filter(|id| *id > 0)
                        .ok_or_else(|| CoreError::InvalidArgs("invalid projectId".into()))?;
                    if let Some(project) = self
                        .store
                        .project(id)
                        .map_err(|error| CoreError::io(error.to_string()))?
                    {
                        if !permitted(&project.path) {
                            return Err(CoreError::OutsideSandbox);
                        }
                    }
                    self.store
                        .remove_project(id)
                        .map_err(|error| CoreError::io(error.to_string()))?;
                    Ok(Value::Null)
                }
            }
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_sqlite_ids_recent_files_and_scoped_selections_survive_owner_migration() {
        let root =
            std::env::temp_dir().join(format!("ridge-projects-migration-{}", uuid::Uuid::new_v4()));
        let a = root.join("a");
        let b = root.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let path = root.join("projects.db");
        let old = rusqlite::Connection::open(&path).unwrap();
        old.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT,path TEXT NOT NULL UNIQUE,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE recent_files (id INTEGER PRIMARY KEY AUTOINCREMENT,project_id INTEGER NOT NULL,path TEXT NOT NULL,opened_at TEXT NOT NULL);").unwrap();
        old.execute(
            "INSERT INTO projects VALUES (37,?1,'2000-01-01','2000-01-01')",
            [a.to_string_lossy()],
        )
        .unwrap();
        old.execute(
            "INSERT INTO recent_files VALUES (41,37,'old.txt','2000-01-01')",
            [],
        )
        .unwrap();
        drop(old);
        let service = Projects::open(path.clone()).unwrap();
        let scope = ridge_core::sandbox::RootScope::from_roots([&root]);
        let first = crate::host_core::runtime_ctx(&[root.clone()], true).with_connection("first");
        let second = crate::host_core::runtime_ctx(&[root.clone()], true).with_connection("second");
        let call = |method, args, ctx: &Ctx| service.dispatch(method, &args, ctx, &scope).unwrap();
        assert_eq!(
            call("get_current_project", json!({}), &first).unwrap(),
            Value::Null
        );
        assert_eq!(
            call("open_project", json!({"path":a}), &first).unwrap()["id"],
            37
        );
        call("open_project", json!({"path":b}), &second).unwrap();
        assert_eq!(
            call("get_current_project", json!({}), &first).unwrap(),
            json!(a)
        );
        assert_eq!(
            call("get_current_project", json!({}), &second).unwrap(),
            json!(b)
        );
        let limited = crate::host_core::runtime_ctx(&[b.clone()], true).with_connection("first");
        assert_eq!(
            call("get_current_project", json!({}), &limited).unwrap(),
            Value::Null
        );
        assert!(call("remove_project", json!({"projectId":37}), &limited).is_err());
        let visible = call("get_recent_projects", json!({}), &limited).unwrap();
        assert_eq!(visible.as_array().unwrap().len(), 1);
        assert_eq!(visible[0]["path"], json!(b));
        drop(service);
        let migrated = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            migrated
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            migrated
                .query_row("SELECT path FROM recent_files WHERE id=41", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "old.txt"
        );
        let archive = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("projects.archive-")
            })
            .unwrap();
        let backup = rusqlite::Connection::open(archive.path()).unwrap();
        assert_eq!(
            backup
                .query_row("SELECT count(*) FROM projects", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            backup
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
