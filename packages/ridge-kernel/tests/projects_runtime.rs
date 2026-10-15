//! Actual presentation sessions share one project DB while their selections remain isolated.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, attach_grant, grant, rpc, start};
use ridge_kernel::client;
use serde_json::json;
use std::fs;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_history_migrates_and_survives_client_detach_runtime_restart_and_permission_denial()
{
    let root =
        std::env::temp_dir().join(format!("ridge-project-history-runtime-{}", Uuid::new_v4()));
    let first_path = root.join("first");
    let second_path = root.join("second");
    fs::create_dir_all(&first_path).unwrap();
    fs::create_dir_all(&second_path).unwrap();
    let path = root.join("projects.db");
    let legacy = rusqlite::Connection::open(&path).unwrap();
    legacy.execute_batch("CREATE TABLE projects (id INTEGER PRIMARY KEY AUTOINCREMENT,path TEXT NOT NULL UNIQUE,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE recent_files(id INTEGER PRIMARY KEY AUTOINCREMENT,project_id INTEGER NOT NULL,path TEXT NOT NULL,opened_at TEXT NOT NULL);").unwrap();
    legacy
        .execute(
            "INSERT INTO projects VALUES (71,?1,'2001-01-01','2001-01-01')",
            [first_path.to_string_lossy()],
        )
        .unwrap();
    legacy
        .execute_batch("INSERT INTO recent_files VALUES (72,71,'kept.txt','2001-01-01')")
        .unwrap();
    drop(legacy);
    let owned = start(&root).await;
    let first_grant = grant(&owned.endpoint);
    let mut a = attach_grant(&first_grant).await;
    let mut b = attach(&owned.endpoint).await;
    let (first, second) = tokio::join!(
        rpc(&mut a, 1, "open_project", json!({"path":first_path})),
        rpc(&mut b, 1, "open_project", json!({"path":second_path}))
    );
    assert!(
        first["error"].is_null() && second["error"].is_null(),
        "{first} {second}"
    );
    assert_eq!(first["result"]["id"], 71);
    assert_eq!(
        rpc(&mut a, 2, "get_current_project", json!({})).await["result"],
        json!(first_path)
    );
    assert_eq!(
        rpc(&mut b, 2, "get_current_project", json!({})).await["result"],
        json!(second_path)
    );
    let history = rpc(&mut a, 3, "get_recent_projects", json!({})).await;
    assert_eq!(history["result"].as_array().unwrap().len(), 2);
    let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"remove_project","args":{"projectId":71},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(denied["ok"], false);
    let invalid = rpc(
        &mut a,
        4,
        "open_project",
        json!({"path":std::env::temp_dir()}),
    )
    .await;
    assert!(
        !invalid["error"].is_null(),
        "operator roots are still authoritative for a local grant"
    );
    assert_eq!(
        rpc(&mut a, 5, "get_current_project", json!({})).await["result"],
        json!(first_path)
    );
    a.close(None).await.unwrap();
    let mut reconnected = attach_grant(&first_grant).await;
    assert_eq!(
        rpc(&mut reconnected, 10, "get_current_project", json!({})).await["result"],
        json!(first_path)
    );
    reconnected.close(None).await.unwrap();
    assert_eq!(
        rpc(&mut b, 6, "get_current_project", json!({})).await["result"],
        json!(second_path)
    );
    b.close(None).await.unwrap();
    drop(owned);
    let restarted = start(&root).await;
    let mut restored = attach(&restarted.endpoint).await;
    let history = rpc(&mut restored, 7, "get_recent_projects", json!({})).await;
    assert_eq!(history["result"].as_array().unwrap().len(), 2);
    assert!(history["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|project| project["id"] == 71 && project["path"] == json!(first_path)));
    assert!(
        rpc(&mut restored, 8, "remove_project", json!({"projectId":71})).await["error"].is_null()
    );
    assert_eq!(
        rpc(&mut restored, 9, "get_recent_projects", json!({})).await["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let persisted = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        persisted
            .query_row("SELECT path FROM recent_files WHERE id=72", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "kept.txt"
    );
    let archives: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("projects.archive-")
        })
        .collect();
    assert!(archives.len() >= 2);
    assert!(archives
        .iter()
        .any(|file| rusqlite::Connection::open(file.path())
            .unwrap()
            .query_row("SELECT id FROM projects WHERE id=71", [], |row| row
                .get::<_, i64>(0))
            .is_ok()));
    restored.close(None).await.unwrap();
}
