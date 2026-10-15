//! Real scoped context RPC; reject escapes while preserving existing large-file behavior.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{start,attach,rpc};
use ridge_kernel::client;
use serde_json::json;
use std::fs;
use uuid::Uuid;
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn context_scan_uses_portable_service_and_rejects_escape_before_read() {
    let root=std::env::temp_dir().join(format!("ridge-context-runtime-{}",Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("AGENTS.md"),"portable rule
").unwrap();
    fs::write(root.join("CLAUDE.md"),"legacy convention
").unwrap();
    fs::write(root.join("unrelated.md"),"not context").unwrap();
    let owned=start(&root).await;
    let mut socket=attach(&owned.endpoint).await;
    let risk = rpc(&mut socket, 7, "classify_command_risk", json!({"command":"rm -rf /"})).await;
    assert_eq!(risk["result"]["level"], "Dangerous", "{risk}");
    let invalid_risk = rpc(&mut socket, 8, "classify_command_risk", json!({"command":42})).await;
    assert_eq!(invalid_risk["error"]["data"]["kind"], "invalid_args", "{invalid_risk}");
    let remote_risk = client::request_json(&owned.endpoint, "POST", "/v1/domain/commands", Some(&json!({
        "method":"classify_command_risk","args":{"command":"pwd"},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote_risk["error"]["data"]["kind"], "capability_denied", "{remote_risk}");
    let scanned=rpc(&mut socket,1,"scan_workspace_context_files",json!({"workspaceRoot":root})).await;
    assert!(scanned["error"].is_null(),"{scanned}");
    let files=scanned["result"]["files"].as_array().unwrap();
    assert_eq!(files.len(),2);
    assert_eq!(files[0]["bytes"],"portable rule
".len());
    assert!(scanned["result"]["promptBlock"].as_str().unwrap().contains("portable rule"));
    fs::write(root.join("b.ridge"),b"unparsed b").unwrap();
    fs::write(root.join("a.ridge"),b"unparsed a").unwrap();
    fs::create_dir(root.join("0.ridge")).unwrap();
    let discovered=rpc(&mut socket,5,"get_workspace_file_in_directory",json!({"path":root})).await;
    assert_eq!(discovered["result"],json!(root.join("a.ridge")),"{discovered}");
    assert_eq!(fs::read(root.join("a.ridge")).unwrap(),b"unparsed a");
    let remote_file=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"get_workspace_file_in_directory","args":{"path":root},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote_file["value"],json!(root.join("a.ridge")),"{remote_file}");
    let remote=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"scan_workspace_context_files","args":{"workspaceRoot":root},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote["ok"],false,"existing Remote capability policy must remain enforced");
    let outside=std::env::temp_dir().join(format!("ridge-context-outside-{}",Uuid::new_v4()));
    fs::create_dir_all(&outside).unwrap();
    let secret=outside.join("AGENTS.md");
    fs::write(&secret,"outside content must not appear").unwrap();
    let directory_escape=rpc(&mut socket,6,"get_workspace_file_in_directory",json!({"path":outside})).await;
    assert_eq!(directory_escape["error"]["data"]["kind"],"outside_sandbox","{directory_escape}");
    let denied=rpc(&mut socket,2,"scan_workspace_context_files",json!({"workspaceRoot":outside})).await;
    assert_eq!(denied["error"]["data"]["kind"],"outside_sandbox","{denied}");
    let target=root.join("AGENTS.md");
    fs::rename(&target,root.join("AGENTS.original")).unwrap();
    #[cfg(windows)] std::os::windows::fs::symlink_file(&secret,&target).unwrap();
    #[cfg(unix)] std::os::unix::fs::symlink(&secret,&target).unwrap();
    let escaped=rpc(&mut socket,3,"scan_workspace_context_files",json!({"workspaceRoot":root})).await;
    assert_eq!(escaped["error"]["data"]["kind"],"outside_sandbox","{escaped}");
    assert!(!escaped.to_string().contains("outside content must not appear"));
    fs::remove_file(&target).unwrap();
    fs::rename(root.join("AGENTS.original"),&target).unwrap();
    fs::write(root.join("CLAUDE.md"),vec![b'x';1024*1024+1]).unwrap();
    let oversized=rpc(&mut socket,4,"scan_workspace_context_files",json!({"workspaceRoot":root})).await;
    assert!(oversized["error"].is_null(),"context migration must retain large-file behavior");
    assert!(oversized["result"]["files"].as_array().unwrap().iter().any(|file|file["name"]=="CLAUDE.md" && file["bytes"]==1024*1024+1));
    assert_eq!(fs::metadata(root.join("CLAUDE.md")).unwrap().len(),1024*1024+1);
}
