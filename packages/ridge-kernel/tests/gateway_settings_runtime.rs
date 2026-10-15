//! A shared durable Runtime gateway preference survives detach and service restart.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use serde_json::json;
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gateway_setting_is_shared_and_rejects_invalid_or_remote_mutation() {
    let root = std::env::temp_dir().join(format!("ridge-gateway-settings-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("runtime-settings.json"), br#"{"unknownPreference":"retained"}"#).unwrap();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let mut second = attach(&owned.endpoint).await;
    assert_eq!(rpc(&mut first, 1, "get_hitl_enabled", json!({})).await["result"], false);
    let enabled = rpc(&mut first, 2, "initialize_hitl_settings", json!({"enabled":true})).await;
    assert_eq!(enabled["result"], true, "{enabled}");
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = second.next().await {
                let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                if event["name"] == "teammate://hitl-settings-changed" { break event; }
            }
        }
    }).await.expect("the second presentation must receive the committed preference notification");
    assert!(event["payload"].is_null(), "notification must not carry authority or preference data");
    assert_eq!(rpc(&mut second, 1, "get_hitl_enabled", json!({})).await["result"], true);
    drop(first);
    assert_eq!(rpc(&mut second, 2, "get_hitl_enabled", json!({})).await["result"], true);
    for args in [json!({}), json!({"enabled":"false"}), json!({"enabled":null})] {
        let rejected = rpc(&mut second, 3, "set_hitl_enabled", args).await;
        assert!(rejected["error"].is_object(), "{rejected}");
        assert_eq!(rpc(&mut second, 4, "get_hitl_enabled", json!({})).await["result"], true);
    }
    let remote = client::request_json(&owned.endpoint, "POST", "/v1/domain/commands", Some(&json!({
        "method":"set_hitl_enabled","args":{"enabled":false},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote["ok"], false, "{remote}");
    assert_eq!(rpc(&mut second, 5, "get_hitl_enabled", json!({})).await["result"], true);
    let imported = rpc(&mut second, 6, "initialize_hitl_settings", json!({"enabled":false})).await;
    assert_eq!(imported["result"], true, "new window must retain the existing shared preference");
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("runtime-settings.json")).unwrap()).unwrap();
    assert_eq!(stored["hitlEnabled"], true);
    assert_eq!(stored["unknownPreference"], "retained");
    assert!(std::fs::read_dir(&root).unwrap().flatten().any(|entry|
        entry.file_name().to_string_lossy().starts_with("runtime-settings.archive-")
        && std::fs::read(entry.path()).ok().as_deref() == Some(br#"{"unknownPreference":"retained"}"#)));
    drop(second);
    drop(owned);
    let restarted = start(&root).await;
    let mut presentation = attach(&restarted.endpoint).await;
    assert_eq!(rpc(&mut presentation, 1, "get_hitl_enabled", json!({})).await["result"], true);
    assert!(rpc(&mut presentation, 2, "set_hitl_enabled", json!({"enabled":false})).await["error"].is_null());
    assert_eq!(rpc(&mut presentation, 3, "get_hitl_enabled", json!({})).await["result"], false);
    let corrupt = br#"{"hitlEnabled":"false"}"#;
    std::fs::write(root.join("runtime-settings.json"), corrupt).unwrap();
    assert!(rpc(&mut presentation, 4, "get_hitl_enabled", json!({})).await["error"].is_object());
    assert!(rpc(&mut presentation, 5, "set_hitl_enabled", json!({"enabled":true})).await["error"].is_object());
    assert_eq!(std::fs::read(root.join("runtime-settings.json")).unwrap(), corrupt);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_first_imports_converge_on_one_committed_preference() {
    let root = std::env::temp_dir().join(format!("ridge-gateway-import-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let mut second = attach(&owned.endpoint).await;
    let (a, b) = tokio::join!(
        rpc(&mut first, 1, "initialize_hitl_settings", json!({"enabled":true})),
        rpc(&mut second, 1, "initialize_hitl_settings", json!({"enabled":false})),
    );
    assert!(a["result"].is_boolean(), "{a}");
    assert_eq!(a["result"], b["result"], "only the first committed import may choose the preference");
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("runtime-settings.json")).unwrap()).unwrap();
    assert_eq!(stored["hitlEnabled"], a["result"]);
}
