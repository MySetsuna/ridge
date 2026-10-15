use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub path: String,
    pub created_at: String,
    pub updated_at: String,
}

// Schema row companion to `RecentFileInfo` — see project.rs for context.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentFile {
    pub id: i64,
    pub project_id: i64,
    pub path: String,
    pub opened_at: String,
}

pub struct ProjectStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl ProjectStore {
    pub fn new(db_path: &PathBuf) -> SqliteResult<Self> {
        let existed = db_path.exists();
        let conn = Connection::open(db_path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let store = Self {
            conn: Mutex::new(conn),
            path: db_path.clone(),
        };
        if version == 0 && existed {
            store.backup()?;
        }
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> SqliteResult<()> {
        let mut connection = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let conn = connection.transaction()?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS recent_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                path TEXT NOT NULL,
                opened_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_recent_files_project ON recent_files(project_id)",
            [],
        )?;
        conn.prepare("SELECT id, path, created_at, updated_at FROM projects LIMIT 0")?;
        conn.prepare("SELECT id, project_id, path, opened_at FROM recent_files LIMIT 0")?;
        conn.pragma_update(None, "user_version", 1)?;
        conn.commit()
    }

    fn backup(&self) -> SqliteResult<()> {
        let connection = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let nonce = uuid::Uuid::new_v4();
        let incomplete = self
            .path
            .with_extension(format!("backup-{nonce}.incomplete"));
        connection.backup(rusqlite::DatabaseName::Main, &incomplete, None)?;
        std::fs::rename(
            &incomplete,
            self.path.with_extension(format!("archive-{nonce}.db")),
        )
        .map_err(|error| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
                Some(format!("publish project archive: {error}")),
            )
        })
    }

    pub fn open_project(&self, path: &str) -> SqliteResult<Project> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // Try to update existing project first
        let rows_affected = conn.execute(
            "UPDATE projects SET updated_at = ?1 WHERE path = ?2",
            rusqlite::params![now, path],
        )?;

        if rows_affected == 0 {
            // Insert new project
            conn.execute(
                "INSERT INTO projects (path, created_at, updated_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![path, now, now],
            )?;
        }

        // Fetch the project
        let mut stmt =
            conn.prepare("SELECT id, path, created_at, updated_at FROM projects WHERE path = ?1")?;
        let project = stmt.query_row([&path], |row| {
            Ok(Project {
                id: row.get(0)?,
                path: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;

        Ok(project)
    }

    pub fn get_recent_projects_where(
        &self,
        limit: usize,
        include: impl Fn(&str) -> bool,
    ) -> SqliteResult<Vec<Project>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut stmt = conn.prepare(
            "SELECT id, path, created_at, updated_at FROM projects ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                path: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;
        let mut projects = Vec::new();
        for row in rows {
            let project = row?;
            if include(&project.path) {
                projects.push(project);
            }
            if projects.len() >= limit {
                break;
            }
        }
        Ok(projects)
    }

    pub fn remove_project(&self, project_id: i64) -> SqliteResult<()> {
        self.backup()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        conn.execute("DELETE FROM projects WHERE id = ?1", [project_id])?;
        Ok(())
    }

    pub fn project(&self, id: i64) -> SqliteResult<Option<Project>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        conn.query_row(
            "SELECT id, path, created_at, updated_at FROM projects WHERE id = ?1",
            [id],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .optional()
    }

    #[allow(dead_code)] // legacy ProjectSidebar persistence; kept for schema compatibility
    pub fn add_recent_file(&self, project_id: i64, path: &str) -> SqliteResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // Remove old entry if exists
        conn.execute(
            "DELETE FROM recent_files WHERE project_id = ?1 AND path = ?2",
            rusqlite::params![project_id, path],
        )?;

        // Add new entry
        conn.execute(
            "INSERT INTO recent_files (project_id, path, opened_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![project_id, path, now],
        )?;

        // Keep only last 20 files per project
        conn.execute(
            "DELETE FROM recent_files WHERE project_id = ?1 AND id NOT IN (
                SELECT id FROM recent_files WHERE project_id = ?1 ORDER BY opened_at DESC LIMIT 20
            )",
            [project_id],
        )?;

        Ok(())
    }

    #[allow(dead_code)] // legacy ProjectSidebar persistence; kept for schema compatibility
    pub fn get_recent_files(&self, project_id: i64, limit: usize) -> SqliteResult<Vec<RecentFile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, path, opened_at FROM recent_files
             WHERE project_id = ?1 ORDER BY opened_at DESC LIMIT ?2",
        )?;

        let files = stmt
            .query_map(rusqlite::params![project_id, limit], |row| {
                Ok(RecentFile {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    path: row.get(2)?,
                    opened_at: row.get(3)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_crash_fixture() {
        let Some(path) = std::env::var_os("RIDGE_PROJECT_MIGRATION_CRASH_FIXTURE") else {
            return;
        };
        let connection = Connection::open(path).unwrap();
        connection.execute_batch("BEGIN IMMEDIATE; CREATE TABLE recent_files (id INTEGER PRIMARY KEY, project_id INTEGER, path TEXT, opened_at TEXT); UPDATE projects SET path='uncommitted' WHERE id=71; PRAGMA user_version=1;").unwrap();
        // Exit without dropping the transaction: exercise SQLite's on-disk crash recovery.
        std::process::exit(73);
    }

    #[test]
    fn interrupted_schema_transaction_recovers_legacy_rows_before_retry() {
        let root =
            std::env::temp_dir().join(format!("ridge-project-crash-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("projects.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE, created_at TEXT, updated_at TEXT); INSERT INTO projects VALUES (71,'legacy','before','before');").unwrap();
        drop(legacy);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "project_store::tests::migration_crash_fixture",
                "--nocapture",
            ])
            .env("RIDGE_PROJECT_MIGRATION_CRASH_FIXTURE", &path)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let recovered = Connection::open(&path).unwrap();
        assert_eq!(
            recovered
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            recovered
                .query_row("SELECT path FROM projects WHERE id=71", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "legacy"
        );
        assert_eq!(
            recovered
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='recent_files'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(recovered);
        let migrated = ProjectStore::new(&path).unwrap();
        assert_eq!(migrated.project(71).unwrap().unwrap().path, "legacy");
    }

    #[test]
    fn legacy_archive_restores_ids_history_and_accepts_new_writes() {
        let root =
            std::env::temp_dir().join(format!("ridge-project-restore-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("projects.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE, created_at TEXT, updated_at TEXT); CREATE TABLE recent_files (id INTEGER PRIMARY KEY, project_id INTEGER, path TEXT, opened_at TEXT); INSERT INTO projects VALUES (71,'legacy','before','before'); INSERT INTO recent_files VALUES (72,71,'kept.txt','before');").unwrap();
        drop(legacy);
        let store = ProjectStore::new(&path).unwrap();
        store.open_project("after-migration").unwrap();
        drop(store);
        let archive = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("projects.archive-")
            })
            .unwrap()
            .path();
        let archived = Connection::open(&archive).unwrap();
        assert_eq!(
            archived
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(archived);
        // Restore into an isolated destination; keep both the live store and archive intact.
        let restored_path = root.join("restored.db");
        std::fs::copy(&archive, &restored_path).unwrap();
        let restored = ProjectStore::new(&restored_path).unwrap();
        assert_eq!(restored.project(71).unwrap().unwrap().path, "legacy");
        let files = restored.get_recent_files(71, 20).unwrap();
        assert_eq!((files[0].id, files[0].path.as_str()), (72, "kept.txt"));
        assert_eq!(
            restored
                .get_recent_projects_where(20, |_| true)
                .unwrap()
                .len(),
            1
        );
        restored.open_project("after-restore").unwrap();
        drop(restored);
        let reopened = ProjectStore::new(&restored_path).unwrap();
        assert_eq!(
            reopened
                .get_recent_projects_where(20, |_| true)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn conflicting_writer_blocks_migration_without_changes_then_retry_succeeds() {
        let root =
            std::env::temp_dir().join(format!("ridge-project-conflict-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("projects.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE, created_at TEXT, updated_at TEXT); INSERT INTO projects VALUES (71,'legacy','before','before'); BEGIN IMMEDIATE;").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(ProjectStore::new(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            legacy
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        legacy.execute_batch("ROLLBACK").unwrap();
        drop(legacy);
        let migrated = ProjectStore::new(&path).unwrap();
        assert_eq!(migrated.project(71).unwrap().unwrap().path, "legacy");
    }

    #[test]
    fn failed_archive_blocks_project_removal_without_losing_the_row() {
        let root =
            std::env::temp_dir().join(format!("ridge-project-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut store = ProjectStore::new(&root.join("projects.db")).unwrap();
        let project = store.open_project("legacy-path").unwrap();
        let blocker = root.join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        store.path = blocker.join("projects.db");
        assert!(store.remove_project(project.id).is_err());
        assert_eq!(
            store.project(project.id).unwrap().unwrap().path,
            "legacy-path"
        );
    }
    #[test]
    fn corrupt_or_incompatible_legacy_schema_is_preserved_and_migration_rolls_back() {
        let root =
            std::env::temp_dir().join(format!("ridge-project-schema-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let damaged = root.join("damaged.db");
        std::fs::write(&damaged, b"not sqlite but recoverable").unwrap();
        assert!(ProjectStore::new(&damaged).is_err());
        assert_eq!(
            std::fs::read(&damaged).unwrap(),
            b"not sqlite but recoverable"
        );
        let incompatible = root.join("incompatible.db");
        let connection = Connection::open(&incompatible).unwrap();
        connection.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY, legacy TEXT); INSERT INTO projects VALUES (17,'keep');").unwrap();
        drop(connection);
        assert!(ProjectStore::new(&incompatible).is_err());
        let restored = Connection::open(&incompatible).unwrap();
        assert_eq!(
            restored
                .query_row("SELECT legacy FROM projects WHERE id=17", [], |row| row
                    .get::<_, String>(
                    0
                ))
                .unwrap(),
            "keep"
        );
        assert_eq!(
            restored
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name='recent_files'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}
