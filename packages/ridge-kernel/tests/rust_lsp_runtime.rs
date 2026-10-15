#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use serde_json::{json, Value};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rust_analyzer_is_runtime_owned_and_retains_unsaved_document_after_detach() {
    let root = std::env::temp_dir().join(format!("ridge-rust-lsp-{}", Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"owned_lsp_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    let file = root.join("src/lib.rs");
    let disk = "pub fn original() -> u32 { 1 }\n";
    let unsaved = "pub fn answer() -> u32 { 42 }\npub fn caller() -> u32 { answer() }\n";
    std::fs::write(&file, disk).unwrap();
    let uri = url::Url::from_file_path(&file).unwrap().to_string();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let opened = rpc(&mut first, 1, "lsp_did_open", json!({"workspaceRoot":root,"uri":uri,"languageId":"rust","text":unsaved})).await;
    assert!(opened["error"].is_null(), "installed rust-analyzer must initialize: {opened}");
    drop(first);
    let mut second = attach(&owned.endpoint).await;
    let args = json!({"workspaceRoot":root,"uri":uri,"line":1,"character":26});
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let hover = rpc(&mut second, 1, "lsp_hover", args.clone()).await;
        if hover["result"]["contents"].to_string().contains("answer") { break; }
        assert!((hover["error"].is_null() || content_modified(&hover)) && std::time::Instant::now() < deadline, "Rust project did not become ready: {hover}");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let (definition, rows) = loop {
        let definition = rpc(&mut second, 2, "lsp_definition", args.clone()).await;
        assert!(definition["error"].is_null() || content_modified(&definition), "{definition}");
        if let Some(rows) = definition["result"].as_array().filter(|rows| !rows.is_empty()) {
            break (definition.clone(), rows.clone());
        }
        assert!(std::time::Instant::now() < deadline, "Rust navigation did not become ready: {definition}");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    assert!(rows.iter().any(|row| {
        let path = row["targetUri"].as_str().or(row["uri"].as_str())
            .and_then(|uri| url::Url::parse(uri).ok())
            .and_then(|uri| uri.to_file_path().ok())
            .and_then(|path| path.canonicalize().ok());
        path == file.canonicalize().ok()
            && (row["targetRange"]["start"]["line"] == 0 || row["range"]["start"]["line"] == 0)
    }), "definition must refer to the retained document: {definition}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), disk, "queries must use the retained unsaved buffer without writing source");
    #[cfg(windows)]
    let children = owned_children(owned.endpoint.pid);
    drop(second);
    drop(owned);
    #[cfg(windows)]
    for process in children {
        assert!(ridge_core::process_identity::validate(&process).is_err(), "explicit Runtime shutdown must terminate owned LSP process {process:?}");
    }
}

#[cfg(windows)]
fn owned_children(kernel: u32) -> Vec<ridge_core::process_identity::ProcessInstance> {
    let mut queue = vec![kernel];
    let mut children = Vec::new();
    while let Some(parent) = queue.pop() {
        let command = format!("ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -Filter 'ParentProcessId = {parent}' | Select-Object -ExpandProperty ProcessId)");
        let output = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &command]).output().unwrap();
        assert!(output.status.success());
        let ids: Vec<u32> = serde_json::from_slice(&output.stdout).unwrap();
        for pid in ids {
            if let Ok(process) = ridge_core::process_identity::instance(pid) {
                assert!(ridge_core::process_identity::descendant(&ridge_core::process_identity::instance(kernel).unwrap(), &process).unwrap());
                queue.push(pid);
                children.push(process);
            }
        }
    }
    assert!(!children.is_empty(), "real language server must remain owned by the live Runtime");
    children
}

fn content_modified(reply: &Value) -> bool {
    reply["error"]["message"].as_str()
        .and_then(|message| message.split_once("LSP server rejected request: ").map(|(_, error)| error))
        .and_then(|error| serde_json::from_str::<Value>(error).ok())
        .is_some_and(|error| error["code"] == -32801)
}
