//! Product Git commands exercise real repositories through the authenticated common boundary.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{start, attach, rpc};
use futures::{SinkExt, StreamExt};
use ridge_kernel::client;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(repo).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().into()
}
async fn call(socket: &mut Socket, id: &mut u64, repo: &Path, method: &str, mut args: Value) -> Value {
    *id += 1; args["repoRoot"] = json!(repo);
    let response = rpc(socket, *id, method, args).await;
    assert!(response["error"].is_null(), "{method}: {response}");
    response["result"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_control_commands_share_execution_and_preserve_admission() {
    let root = std::env::temp_dir().join(format!("ridge-git-commands-{}", Uuid::new_v4()));
    let repo = root.join("repo"); fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--initial-branch=main"]);
    git(&repo, &["config", "user.name", "Owned fixture"]);
    git(&repo, &["config", "user.email", "fixture@localhost"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("file.txt"), "one\n").unwrap();
    git(&repo, &["add", "file.txt"]); git(&repo, &["commit", "-m", "one"]);
    let first = git(&repo, &["rev-parse", "HEAD"]);
    let owned = start(&root).await; let mut socket = attach(&owned.endpoint).await; let mut id = 0;
    socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"type":"data-request","method":"get_scm_status","repoRoot":repo,"_reqId":700}).to_string())).await.unwrap();
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let tokio_tungstenite::tungstenite::Message::Text(text) = socket.next().await.unwrap().unwrap() {
                let response: Value = serde_json::from_str(&text).unwrap();
                if response["_reqId"] == 700 { break response; }
            }
        }
    }).await.unwrap();
    assert_eq!(status["type"], "data-result"); assert_eq!(status["_result"]["current_branch"], "main", "{status}");
    fs::write(repo.join("file.txt"), "two\n").unwrap();
    assert!(call(&mut socket, &mut id, &repo, "git_stage", json!({"paths":["file.txt"]})).await.is_null());
    call(&mut socket, &mut id, &repo, "git_unstage", json!({"paths":["file.txt"]})).await;
    assert!(git(&repo, &["diff", "--cached"]).is_empty());
    call(&mut socket, &mut id, &repo, "git_stage", json!({"paths":["file.txt"]})).await;
    call(&mut socket, &mut id, &repo, "git_commit", json!({"message":"two"})).await;
    let second = git(&repo, &["rev-parse", "HEAD"]); assert_ne!(first, second);
    let files = call(&mut socket, &mut id, &repo, "git_get_commit_files", json!({"hash":second})).await;
    assert_eq!(files[0]["path"], "file.txt");
    let compared = call(&mut socket, &mut id, &repo, "git_compare_commits", json!({"from":first,"to":second})).await;
    assert_eq!(compared[0]["path"], "file.txt");
    for (method, args) in [
        ("git_get_file_versions_at_commit", json!({"path":"file.txt","hash":second})),
        ("git_get_file_versions_between", json!({"path":"file.txt","from":first,"to":second})),
    ] {
        let versions = call(&mut socket, &mut id, &repo, method, args).await;
        assert_eq!(versions["original"], "one\n"); assert_eq!(versions["modified"], "two\n");
    }
    fs::write(repo.join("file.txt"), "discard\n").unwrap();
    call(&mut socket, &mut id, &repo, "git_discard", json!({"paths":["file.txt"]})).await;
    assert_eq!(fs::read_to_string(repo.join("file.txt")).unwrap(), "two\n");
    fs::write(repo.join("untracked.txt"), "owned").unwrap();
    call(&mut socket, &mut id, &repo, "git_clean_untracked", json!({"paths":["untracked.txt"]})).await;
    assert!(!repo.join("untracked.txt").exists());
    call(&mut socket, &mut id, &repo, "git_checkout", json!({"branch":"feature","create":true,"base":"HEAD"})).await;
    fs::write(repo.join("feature.txt"), "feature\n").unwrap();
    call(&mut socket, &mut id, &repo, "git_stage", json!({"paths":["feature.txt"]})).await;
    call(&mut socket, &mut id, &repo, "git_commit", json!({"message":"feature"})).await;
    call(&mut socket, &mut id, &repo, "git_checkout", json!({"branch":"main"})).await;
    assert!(call(&mut socket, &mut id, &repo, "git_merge_branch", json!({"branch":"feature"})).await.is_string());
    assert!(repo.join("feature.txt").exists());
    call(&mut socket, &mut id, &repo, "git_rename_branch", json!({"oldName":"feature","newName":"renamed"})).await;
    call(&mut socket, &mut id, &repo, "git_delete_branch", json!({"branch":"renamed","force":false})).await;
    call(&mut socket, &mut id, &repo, "git_rebase", json!({"onto":"main"})).await;
    for method in ["git_stash_apply", "git_stash_pop", "git_stash_drop", "git_stash_branch"] {
        if method != "git_stash_pop" {
            fs::write(repo.join("file.txt"), "stash\n").unwrap();
            call(&mut socket, &mut id, &repo, "git_stash_push", json!({"message":"owned stash","includeUntracked":true})).await;
            assert_eq!(fs::read_to_string(repo.join("file.txt")).unwrap(), "two\n");
        }
        let args = if method == "git_stash_branch" { json!({"reference":"stash@{0}","branch":"from-stash"}) } else { json!({"reference":"stash@{0}"}) };
        call(&mut socket, &mut id, &repo, method, args).await;
        if method != "git_stash_drop" {
            assert_eq!(fs::read_to_string(repo.join("file.txt")).unwrap(), "stash\n");
            call(&mut socket, &mut id, &repo, "git_discard", json!({"paths":["file.txt"]})).await;
        }
    }
    call(&mut socket, &mut id, &repo, "git_checkout", json!({"branch":"main"})).await;
    call(&mut socket, &mut id, &repo, "git_checkout", json!({"branch":"pick","create":true})).await;
    fs::write(repo.join("picked.txt"), "picked\n").unwrap();
    call(&mut socket, &mut id, &repo, "git_stage", json!({"paths":["picked.txt"]})).await;
    call(&mut socket, &mut id, &repo, "git_commit", json!({"message":"pick"})).await;
    let pick = git(&repo, &["rev-parse", "HEAD"]);
    call(&mut socket, &mut id, &repo, "git_checkout", json!({"branch":"main"})).await;
    call(&mut socket, &mut id, &repo, "git_cherry_pick", json!({"hash":pick})).await;
    assert!(repo.join("picked.txt").exists());
    call(&mut socket, &mut id, &repo, "git_revert", json!({"hash":"HEAD"})).await;
    assert!(!repo.join("picked.txt").exists());
    call(&mut socket, &mut id, &repo, "git_reset", json!({"hash":"HEAD","mode":"mixed"})).await;
    let bare = root.join("origin.git"); fs::create_dir(&bare).unwrap(); git(&bare, &["init", "--bare", "--initial-branch=main"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    call(&mut socket, &mut id, &repo, "git_push", json!({"setUpstream":true})).await;
    for method in ["git_fetch", "git_pull", "git_sync"] { call(&mut socket, &mut id, &repo, method, json!({})).await; }
    call(&mut socket, &mut id, &repo, "git_push_branch", json!({"branch":"main"})).await;
    call(&mut socket, &mut id, &repo, "git_create_tag", json!({"name":"owned-tag","hash":"HEAD"})).await;
    call(&mut socket, &mut id, &repo, "git_push_tag", json!({"name":"owned-tag"})).await;
    assert!(!git(&bare, &["tag", "--list", "owned-tag"]).is_empty());
    call(&mut socket, &mut id, &repo, "git_delete_tag", json!({"name":"owned-tag"})).await;
    for method in ["git_cherry_pick_abort", "git_revert_abort"] {
        id += 1; let response = rpc(&mut socket, id, method, json!({"repoRoot":repo})).await;
        assert!(response["error"].is_object()); assert_ne!(response["error"]["code"], -32601);
    }
    let command = |method: &str, args: Value, native: bool, roots: Value| client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":method,"args":args,"native":native,"roots":roots,"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(command("git_stage", json!({"repoRoot":repo,"paths":["file.txt"]}), false, json!([repo]))["ok"], true);
    assert_eq!(command("git_delete_branch", json!({"repoRoot":repo,"branch":"pick"}), false, json!([repo]))["error"]["data"]["kind"], "capability_denied");
    let sub = repo.join("sub"); fs::create_dir(&sub).unwrap();
    let head = git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(command("git_checkout", json!({"repoRoot":sub,"branch":"pick"}), true, json!([sub]))["error"]["data"]["kind"], "outside_sandbox");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(command("git_stage", json!({"repo_root":repo,"paths":[]}), true, json!([]))["error"]["data"]["kind"], "invalid_args");
    assert_eq!(command("git_discard", json!({"repoRoot":repo,"paths":["../auth.json"]}), true, json!([]))["error"]["data"]["kind"], "path_traversal");
    assert_eq!(command("git_checkout", json!({"repoRoot":root,"branch":"main"}), true, json!([]))["ok"], false);
    drop(socket); drop(owned);
    assert!(root.is_absolute() && root.starts_with(std::env::temp_dir())); fs::remove_dir_all(root).unwrap();
}
