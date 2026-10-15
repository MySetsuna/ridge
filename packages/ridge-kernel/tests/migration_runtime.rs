//! Partial startup failure must retain recoverable legacy data and one Runtime writer.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::json;
use std::{fs, path::Path, process::{Command, ExitStatus, Stdio}, time::{Duration, Instant}};

fn launch(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ridge-kernel"));
    command.env("RIDGE_KERNEL_DATA_DIR", root).env("RIDGE_KERNEL_FS_ROOT", root)
        .env("RIDGE_WORKSPACE_SAVE_DIR", root).env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::null()).stdout(Stdio::null());
    command
}

fn finish(command: &mut Command, log: &Path) -> ExitStatus {
    let mut child = command.stderr(fs::File::create(log).unwrap()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() { return status; }
        if Instant::now() >= deadline {
            let _ = child.kill(); let _ = child.wait();
            panic!("owned startup did not finish; log: {}", log.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_migration_failure_preserves_legacy_data_and_retry_has_one_writer() {
    use ridge_core::{teammate::{model::Teammate, topology::TopologyGraph}, workspace::graph::WorkspaceGraph};
    let root = std::env::temp_dir().join(format!("ridge-partial-migration-{}", uuid::Uuid::new_v4()));
    let project = root.join("legacy-project"); fs::create_dir_all(&project).unwrap();
    let database = root.join("projects.db");
    let legacy = rusqlite::Connection::open(&database).unwrap();
    legacy.execute_batch("CREATE TABLE projects(id INTEGER PRIMARY KEY,path TEXT UNIQUE,created_at TEXT,updated_at TEXT); CREATE TABLE recent_files(id INTEGER PRIMARY KEY,project_id INTEGER,path TEXT,opened_at TEXT); INSERT INTO recent_files VALUES(72,71,'kept.txt','2001-01-01');").unwrap();
    legacy.execute("INSERT INTO projects VALUES(71,?1,'2001-01-01','2001-01-01')", [project.to_string_lossy()]).unwrap();
    drop(legacy);
    let mut graph = WorkspaceGraph::new(); let workspace = graph.create_workspace();
    let mut old_graph = serde_json::to_value(&graph).unwrap();
    old_graph.as_object_mut().unwrap().remove("order");
    old_graph.as_object_mut().unwrap().remove("next_display_seq");
    let original_graph = serde_json::to_vec_pretty(&old_graph).unwrap();
    let graph_path = root.join("workspace-graph.json");
    fs::write(&graph_path, b"{interrupted").unwrap();
    let mut roster = TopologyGraph::new(); roster.add_teammate(Teammate::new("kept", "Legacy member", 71));
    let roster_bytes = serde_json::to_vec_pretty(&roster).unwrap();
    fs::write(root.join("agent-roster.json"), &roster_bytes).unwrap();

    #[cfg(windows)]
    {
        let original = fs::read(&database).unwrap();
        let permissions = fs::metadata(&database).unwrap().permissions();
        let mut denied = permissions.clone(); denied.set_readonly(true);
        fs::set_permissions(&database, denied).unwrap();
        let result = std::panic::catch_unwind(|| finish(&mut launch(&root), &root.join("readonly-projects.log")));
        fs::set_permissions(&database, permissions).unwrap();
        assert!(!result.unwrap().success());
        assert_eq!(fs::read(&database).unwrap(), original);
        assert!(!root.join("kernel.json").exists());
    }
    let previous_archives = fs::read_dir(&root).unwrap().filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("projects.archive-")).count();

    let failure_log = root.join("startup-failure.log");
    assert!(!finish(&mut launch(&root), &failure_log).success());
    assert!(fs::read_to_string(&failure_log).unwrap().contains("workspace-graph.json"));
    assert!(!root.join("kernel.json").exists(), "failed startup published a discoverable Runtime");
    assert_eq!(fs::read(&graph_path).unwrap(), b"{interrupted");
    assert_eq!(fs::read(root.join("agent-roster.json")).unwrap(), roster_bytes);
    let archives: Vec<_> = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("projects.archive-")).collect();
    assert_eq!(archives.len(), previous_archives + 1, "the earlier database step must publish its recovery archive");
    let archived = rusqlite::Connection::open_with_flags(&archives[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(archived.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(archived.query_row("SELECT path FROM recent_files WHERE id=72 AND project_id=71", [], |row| row.get::<_, String>(0)).unwrap(), "kept.txt");
    drop(archived);
    // Repair only the deliberately damaged fixture file; retry the real startup path.
    fs::write(&graph_path, &original_graph).unwrap();
    #[cfg(windows)]
    {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let lock = OpenOptions::new().read(true).share_mode(0).open(root.join("agent-roster.json")).unwrap();
        let log = root.join("locked-roster.log");
        let status = finish(&mut launch(&root), &log);
        drop(lock);
        assert!(!status.success());
        assert!(fs::read_to_string(log).unwrap().contains("agent-roster.json"));
        assert_eq!(fs::read(root.join("agent-roster.json")).unwrap(), roster_bytes);
        assert!(!root.join("kernel.json").exists());
    }
    let owned = start(&root).await;
    let mut view = attach(&owned.endpoint).await;
    let projects = rpc(&mut view, 1, "get_recent_projects", json!({})).await;
    assert_eq!(projects["result"][0]["id"], 71);
    let renamed = rpc(&mut view, 2, "rename_workspace", json!({"workspaceId":workspace,"name":"Recovered"})).await;
    assert!(renamed["error"].is_null(), "{renamed}");
    let graph_archives: Vec<_> = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("workspace-graph.json.archive-")).collect();
    assert_eq!(graph_archives.len(), 1);
    assert_eq!(fs::read(&graph_archives[0]).unwrap(), original_graph);
    let before: Vec<_> = ["projects.db", "agent-roster.json", "workspace-graph.json"].into_iter()
        .map(|name| (name, fs::read(root.join(name)).unwrap())).collect();
    assert!(finish(&mut launch(&root), &root.join("second-start.log")).success());
    let endpoint: KernelEndpoint = serde_json::from_slice(&fs::read(root.join("kernel.json")).unwrap()).unwrap();
    assert_eq!(endpoint.pid, owned.endpoint.pid);
    assert_eq!(endpoint.started_at_unix, owned.endpoint.started_at_unix);
    assert!(client::health_ok(&owned.endpoint));
    for (name, bytes) in before { assert!(fs::read(root.join(name)).unwrap() == bytes, "second startup changed {name}"); }
    assert_eq!(fs::read_dir(&root).unwrap().filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("projects.archive-")).count(), archives.len());
    view.close(None).await.unwrap();
    drop(owned);
    let reopened = start(&root).await;
    let mut view = attach(&reopened.endpoint).await;
    assert_eq!(rpc(&mut view, 3, "get_recent_projects", json!({})).await["result"][0]["id"], 71);
    view.close(None).await.unwrap();
}
