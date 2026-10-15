#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use serde_json::{json, Value};
use uuid::Uuid;
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_typescript_server_keeps_open_document_after_presentation_detach() {
    let root = std::env::temp_dir().join(format!("ridge-lsp-owner-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let modules = root.join("node_modules");
    std::fs::create_dir_all(&modules).unwrap();
    let compiler = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node_modules/typescript").canonicalize().expect("repository TypeScript compiler must be installed");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&compiler, modules.join("typescript")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&compiler, modules.join("typescript")).unwrap();
    let file = root.join("owned.ts");
    let text = "export function greet(name: string): string { return name; }\nconst output = greet('hello');\n";
    std::fs::write(&file, text).unwrap();
    let uri = url::Url::from_file_path(&file).unwrap().to_string();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let opened = rpc(&mut first, 1, "lsp_did_open", json!({"workspaceRoot":root,"uri":uri,"languageId":"typescript","text":text})).await;
    assert!(opened["error"].is_null(), "real installed TypeScript LSP must initialize: {opened}");
    drop(first);
    let mut second = attach(&owned.endpoint).await;
    let args = json!({"workspaceRoot":root,"uri":uri,"line":1,"character":16});
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let hover = rpc(&mut second, 1, "lsp_hover", args.clone()).await;
        if hover["result"]["contents"].to_string().contains("greet") { break; }
        assert!(hover["error"].is_null() && std::time::Instant::now() < deadline, "TypeScript project did not become ready: {hover}");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let definition = rpc(&mut second, 2, "lsp_definition", args.clone()).await;
    let locations: Vec<Value> = serde_json::from_value(definition["result"].clone()).unwrap();
    assert!(!locations.is_empty(), "{definition}");
    assert!(locations.iter().any(|location| {
        location["uri"].as_str().or(location["targetUri"].as_str())
            .and_then(|uri| url::Url::parse(uri).ok())
            .and_then(|uri| uri.to_file_path().ok())
            .and_then(|path| path.canonicalize().ok()) == file.canonicalize().ok()
    }), "definition must target the original document: {definition}");
    assert!(rpc(&mut second, 3, "lsp_references", args.clone()).await["result"].as_array().unwrap().len() >= 2);
    let remote = client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"lsp_hover","args":args,"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote["ok"], false);
    let outside = root.with_extension("outside.ts");
    std::fs::write(&outside, "const private = 1;").unwrap();
    let outside_uri = url::Url::from_file_path(outside).unwrap().to_string();
    assert!(rpc(&mut second, 4, "lsp_did_open", json!({"workspaceRoot":root,"uri":outside_uri,"languageId":"typescript","text":"private"})).await["error"].is_object());
    assert!(rpc(&mut second, 5, "lsp_hover", json!({"workspaceRoot":root,"uri":uri,"line":-1,"character":16})).await["error"].is_object());
    assert!(rpc(&mut second, 6, "lsp_did_change", json!({"workspaceRoot":root,"uri":uri,"version":2,"text":text.replace("greet", "welcome")})).await["error"].is_null());
    assert!(rpc(&mut second, 7, "lsp_hover", args).await["result"]["contents"].to_string().contains("welcome"));
    let mut observer = attach(&owned.endpoint).await;
    let invalid = format!("{}\nconst invalid: string = 42;\n", text.replace("greet", "welcome"));
    assert!(rpc(&mut second, 8, "lsp_did_change", json!({"workspaceRoot":root,"uri":uri,"version":3,"text":invalid})).await["error"].is_null());
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while let Some(Ok(Message::Text(text))) = observer.next().await {
            let event: Value = serde_json::from_str(&text).unwrap();
            if event["name"] == "lsp://diagnostics" && event["payload"]["diagnostics"].as_array().is_some_and(|rows| rows.iter().any(|row| row["code"] == 2322)) { return; }
        }
        panic!("diagnostic stream closed before the real compiler result");
    }).await.expect("Runtime must deliver real TypeScript diagnostics to another Native presentation");
    assert_eq!(std::fs::read_to_string(file).unwrap(), text, "document synchronization must not write source files");
}
