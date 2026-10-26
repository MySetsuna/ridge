//! Actual writer process, portable legacy fields and deferred saves share one Runtime owner.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;
fn file_op(endpoint: &KernelEndpoint, args: Value) -> Value {
    client::request_json(endpoint, "POST", "/v1/domain/workspace-files", Some(&args)).unwrap()
}
#[test]
#[ignore = "separate presentation process used by the owner test"]
fn enqueue_from_exiting_presentation() {
    let root = std::path::PathBuf::from(std::env::var_os("RIDGE_FILE_FIXTURE_ROOT").unwrap());
    let endpoint: KernelEndpoint =
        serde_json::from_slice(&fs::read(root.join("kernel.json")).unwrap()).unwrap();
    let workspace = std::env::var("RIDGE_FILE_FIXTURE_WORKSPACE").unwrap();
    let response = file_op(
        &endpoint,
        json!({"operation":"schedule-save","workspaceId":workspace,"name":"stale presentation name","path":root.join("legacy.ridge")}),
    );
    assert_eq!(response["ok"], true);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_payloads_survive_shared_saves_and_deferred_writer_outlives_its_presentation() {
    let root = std::env::temp_dir().join(format!("ridge-file-owner-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join(".git")).unwrap();
    let owned = start(&root).await;
    let workspace = client::request_json(&owned.endpoint, "POST", "/v1/domain/workspaces", None)
        .unwrap()["workspace_id"]
        .clone();
    let pane = Uuid::new_v4();
    let topology = json!({"pane_tree":{"root":{"Leaf":pane},"panes":{pane.to_string():{"id":pane,"mode":"Terminal","cwd":root}}}});
    client::request_json(
        &owned.endpoint,
        "PUT",
        &format!(
            "/v1/domain/workspaces/{}/topology",
            workspace.as_str().unwrap()
        ),
        Some(&topology),
    )
    .unwrap();
    let path = root.join("legacy.ridge");
    let payload = json!({"version":1,"name":"legacy","saved_at":"2000-01-01","pane_tree":{"root":{"Leaf":pane},"legacyTree":"keep","panes":{pane.to_string():{"id":pane,"mode":"Terminal","cwd":root,"shell_kind":"old-shell","legacyPane":"keep"}}},
        "git_repos":["legacy-repo"],"index_path":"legacy-index","pane_titles":{pane.to_string():"Legacy label"},"legacyDocument":{"keep":true}});
    let original = serde_json::to_vec_pretty(&payload).unwrap();
    fs::write(&path, &original).unwrap();
    let saved = file_op(
        &owned.endpoint,
        json!({"operation":"save","workspaceId":workspace,"name":"Saved by Runtime","path":path}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let preserved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(preserved["legacyDocument"], payload["legacyDocument"]);
    assert_eq!(preserved["pane_tree"]["legacyTree"], "keep");
    assert_eq!(
        preserved["pane_tree"]["panes"][pane.to_string()]["legacyPane"],
        "keep"
    );
    assert!(preserved["pane_tree"]["panes"][pane.to_string()]["shell_kind"].is_null());
    assert_eq!(preserved["index_path"], "legacy-index");
    assert_eq!(preserved["pane_titles"][pane.to_string()], "Legacy label");
    assert!(preserved["git_repos"]
        .as_array()
        .unwrap()
        .contains(&json!(root)));
    assert!(preserved["git_repos"]
        .as_array()
        .unwrap()
        .contains(&json!("legacy-repo")));
    assert_eq!(
        fs::read(saved["value"]["backupPath"].as_str().unwrap()).unwrap(),
        original
    );
    let mut socket = attach(&owned.endpoint).await;
    assert_eq!(
        rpc(&mut socket, 1, "get_last_opened_workspace_path", json!({})).await["result"],
        json!(path)
    );
    assert_eq!(
        rpc(&mut socket, 2, "list_recent_workspaces", json!({})).await["result"],
        json!([path])
    );
    let damaged = root.join("damaged.ridge");
    fs::write(&damaged, b"invalid retained payload").unwrap();
    let directory = root.join("directory.ridge");
    fs::create_dir(&directory).unwrap();
    let listed = rpc(&mut socket, 3, "list_saved_workspace_files", json!({})).await;
    let entries = listed["result"].as_array().unwrap();
    assert!(entries.iter().any(|entry| entry["name"] == "damaged"), "{listed}");
    assert!(!entries.iter().any(|entry| entry["path"] == json!(directory)));
    assert_eq!(fs::read(&damaged).unwrap(), b"invalid retained payload");
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "enqueue_from_exiting_presentation", "--ignored"])
        .env("RIDGE_FILE_FIXTURE_ROOT", &root)
        .env("RIDGE_FILE_FIXTURE_WORKSPACE", workspace.as_str().unwrap())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let second = root.join("new-target.ridge");
    let redirected = file_op(
        &owned.endpoint,
        json!({"operation":"save","workspaceId":workspace,"name":"Latest name","path":second}),
    );
    assert_eq!(redirected["ok"], true);
    let old_bytes = fs::read(&path).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let archived = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("new-target.ridge.archive-")
            });
        if archived {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Runtime did not finish the detached presentation's autosave"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // An archive precedes file replacement and graph commit. Read through the
    // writer's graph lock before taking a committed baseline or injecting faults.
    let info = file_op(&owned.endpoint, json!({"operation":"info","workspaceId":workspace}));
    assert_eq!(info["ok"], true, "{info}");
    assert_eq!(info["value"]["file_path"], json!(second));
    assert_eq!(info["value"]["name"], "Latest name");
    assert_eq!(
        fs::read(&path).unwrap(),
        old_bytes,
        "deferred save must not reuse a stale target"
    );
    let latest: Value = serde_json::from_slice(&fs::read(&second).unwrap()).unwrap();
    assert_eq!(latest["name"], "Latest name");
    assert_eq!(
        file_op(&owned.endpoint, json!({"operation":"last"}))["value"],
        json!(second)
    );
    let committed_bytes = fs::read(&second).unwrap();
    let graph = root.join("workspace-graph.json");
    let held = root.join("held-graph.json");
    fs::rename(&graph, &held).unwrap();
    fs::create_dir(&graph).unwrap();
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"schedule-save","workspaceId":workspace,"name":"stale","path":path})
        )["ok"],
        true
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if file_op(
            &owned.endpoint,
            json!({"operation":"info","workspaceId":workspace}),
        )["ok"]
            == false
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "autosave failures must become observable"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        fs::read(&second).unwrap(),
        committed_bytes,
        "failed autosave must restore previous content"
    );
    fs::remove_dir(&graph).unwrap();
    fs::rename(&held, &graph).unwrap();
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"save","workspaceId":workspace,"name":"Recovered","path":second})
        )["ok"],
        true
    );
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"info","workspaceId":workspace})
        )["ok"],
        true,
        "a successful retry clears prior failure"
    );
    let damaged = root.join("damaged.ridge");
    fs::write(&damaged, b"{recoverable invalid content").unwrap();
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"save","workspaceId":workspace,"name":"Cannot erase","path":damaged})
        )["ok"],
        false
    );
    assert_eq!(fs::read(&damaged).unwrap(), b"{recoverable invalid content");
    let outside = std::env::temp_dir().join(format!("outside-{}.ridge", Uuid::new_v4()));
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"save","workspaceId":workspace,"name":"Outside","path":outside})
        )["ok"],
        false
    );
    assert!(!outside.exists());
    socket.close(None).await.unwrap();
    assert!(client::health_ok(&owned.endpoint));
    let current_bytes = fs::read(&second).unwrap();
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"schedule-save","workspaceId":workspace,"name":"stale","path":second})
        )["ok"],
        true
    );
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"delete-file","workspaceId":workspace})
        )["ok"],
        true
    );
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(
        !second.exists(),
        "queued autosave cannot recreate a detached file"
    );
    let archived = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("new-target.ridge.archive-")
                && fs::read(entry.path()).unwrap() == current_bytes
        });
    assert!(archived);
    assert!(file_op(&owned.endpoint, json!({"operation":"last"}))["value"].is_null());
    let unassociated = root.join("unassociated.ridge");
    fs::write(&unassociated, &current_bytes).unwrap();
    let deleted = file_op(
        &owned.endpoint,
        json!({"operation":"delete-saved","path":unassociated}),
    );
    assert_eq!(deleted["ok"], true, "{deleted}");
    assert!(!unassociated.exists());
    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let protected = nested.join("protected.ridge");
    fs::write(&protected, &current_bytes).unwrap();
    assert_eq!(
        file_op(
            &owned.endpoint,
            json!({"operation":"delete-saved","path":protected})
        )["ok"],
        false
    );
    assert!(protected.exists());
    let fresh = client::request_json(
        &owned.endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"Unsaved survives detach"})),
    )
    .unwrap()["workspace_id"]
        .clone();
    let fresh_id = fresh.as_str().unwrap();
    let saved = client::request_json(
        &owned.endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"Saved workspace survives restart"})),
    )
    .unwrap()["workspace_id"]
        .clone();
    let saved_path = root.join("saved-for-restore.ridge");
    let saved_result = file_op(
        &owned.endpoint,
        json!({"operation":"save","workspaceId":saved,"name":"Saved workspace","path":saved_path}),
    );
    assert_eq!(saved_result["ok"], true, "{saved_result}");
    let exported = file_op(&owned.endpoint, json!({"operation":"save-restore"}));
    assert_eq!(exported["ok"], true, "{exported}");
    let session = root
        .join("session-workspaces")
        .join(format!("session-{fresh_id}.ridge"));
    assert!(
        !session.exists(),
        "temporary workspaces must not be exported as user-saved files"
    );
    assert_eq!(exported["value"], json!([saved_path]));
    assert!(
        file_op(&owned.endpoint, json!({"operation":"restore"}))["value"]
            .as_array()
            .unwrap()
            .contains(&json!(saved_path))
    );
    assert_eq!(
        file_op(&owned.endpoint, json!({"operation":"open","path":saved_path}))["value"]
            ["workspaceId"],
        saved,
        "restoration reuses the live authority rather than creating another workspace"
    );
    assert!(
        file_op(
            &owned.endpoint,
            json!({"operation":"info","workspaceId":fresh})
        )["value"]["file_path"]
            .is_null(),
        "private restore exports must not mark an unsaved workspace as user-saved"
    );
    let snapshot = file_op(
        &owned.endpoint,
        json!({"operation":"snapshot","workspaceId":fresh}),
    );
    assert_eq!(snapshot["value"]["name"], "Unsaved survives detach");
    drop(owned);
    let restarted = start(&root).await;
    let restored = file_op(&restarted.endpoint, json!({"operation":"open","path":saved_path}));
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(
        restored["value"]["workspaceId"], saved,
        "persistent workspace identities survive service restart"
    );
    let persisted = file_op(
        &restarted.endpoint,
        json!({"operation":"snapshot","workspaceId":fresh}),
    );
    assert_eq!(
        persisted["value"]["pane_tree"],
        snapshot["value"]["pane_tree"]
    );
    assert!(
        client::list_domain_ptys(&restarted.endpoint)
            .unwrap()
            .is_empty(),
        "restoring metadata must not launch replacement PTYs"
    );
    let mut client = attach(&restarted.endpoint).await;
    let directory_save = rpc(&mut client, 10, "save_workspace_to_file",
        json!({"workspaceId":fresh,"name":"Directory save","path":root})).await;
    let directory_path = root.join("Directory save.ridge");
    assert_eq!(directory_save["result"], json!(directory_path), "{directory_save}");
    assert!(directory_path.is_file());
    let stem = root.join("new.directory");
    let stem_save = rpc(&mut client, 11, "save_workspace_to_file",
        json!({"workspaceId":fresh,"name":"Stem save","path":stem})).await;
    let expected = stem.join("Stem save.ridge");
    assert_eq!(stem_save["result"], json!(expected), "{stem_save}");
    assert!(expected.is_file(), "new directories must match the save dialog preview");
    assert!(!stem.with_extension("ridge").exists(), "must not save to a guessed filename");

    let explicit = root.join("explicit.RIDGE");
    let explicit_save = rpc(&mut client, 13, "save_workspace_to_file",
        json!({"workspaceId":fresh,"name":"Explicit save","path":explicit})).await;
    assert_eq!(explicit_save["result"], json!(explicit), "{explicit_save}");
    assert!(explicit.is_file());
    let listed = rpc(&mut client, 15, "list_saved_workspace_files", json!({})).await;
    assert!(listed["result"].as_array().unwrap().iter().any(|entry| entry["path"] == json!(explicit)), "{listed}");
    let discovery = root.join("uppercase-discovery");
    fs::create_dir_all(&discovery).unwrap();
    let discovered = discovery.join("saved.RIDGE");
    fs::copy(&explicit, &discovered).unwrap();
    assert_eq!(ridge_core::workspace::file::first_in_directory(&discovery, &[]).unwrap(), json!(discovered));

    let suffix_dir = format!("{}/directory.ridge/", root.display());
    let suffix_save = rpc(&mut client, 14, "save_workspace_to_file",
        json!({"workspaceId":fresh,"name":"Trailing separator","path":suffix_dir})).await;
    let suffix_path = std::path::Path::new(&suffix_dir).join("Trailing separator.ridge");
    assert_eq!(suffix_save["result"], json!(suffix_path), "{suffix_save}");
    assert!(suffix_path.is_file());

    let browsed = rpc(&mut client, 12, "browse_directory", json!({"path":root})).await;
    let browsed_path = browsed["result"]["path"].as_str().expect("shared directory response");
    assert_eq!(fs::canonicalize(browsed_path).unwrap(), fs::canonicalize(&root).unwrap());

}
