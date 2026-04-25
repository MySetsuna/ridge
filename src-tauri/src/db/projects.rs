use chrono::Utc;
use rusqlite::{Connection, Result as SqliteResult};
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
}

impl ProjectStore {
    pub fn new(db_path: &PathBuf) -> SqliteResult<Self> {
        let conn = Connection::open(db_path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
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
        Ok(())
    }

    pub fn open_project(&self, path: &str) -> SqliteResult<Project> {
        let conn = self.conn.lock().unwrap();
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
        let mut stmt = conn.prepare(
            "SELECT id, path, created_at, updated_at FROM projects WHERE path = ?1",
        )?;
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

    pub fn get_recent_projects(&self, limit: usize) -> SqliteResult<Vec<Project>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, created_at, updated_at FROM projects ORDER BY updated_at DESC LIMIT ?1",
        )?;

        let projects = stmt
            .query_map([limit], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(projects)
    }

    pub fn remove_project(&self, project_id: i64) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM projects WHERE id = ?1", [project_id])?;
        Ok(())
    }

    #[allow(dead_code)] // legacy ProjectSidebar persistence; kept for schema compatibility
    pub fn add_recent_file(&self, project_id: i64, path: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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