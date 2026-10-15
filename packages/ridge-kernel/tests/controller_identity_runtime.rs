#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_core::{DeviceIdentity, remote::{HostKind, HostRecord, HostStatus}};
use ridge_kernel::client;
use serde_json::json;
use std::fs;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_import_preserves_browser_public_key_host_identity_and_restart() {
    let root = std::env::temp_dir().join(format!("ridge-controller-import-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let mut second = attach(&owned.endpoint).await;
    let host = rpc(&mut first, 1, "get_device_identity_pub", json!({})).await["result"].clone();
    let host_bytes = fs::read(root.join("device_identity.key")).unwrap();
    let path = root.join("cloud-controller-identity.key");
    let premature = rpc(&mut first, 2, "connect_host", json!({"kind":"rdg","addr":"fixture","cloud":{
        "userToken":"owned-fixture","username":"owner","deviceName":"fixture","baseDomain":"localhost:1","plaintext":true,"code":"123456"
    }})).await;
    assert!(premature["error"]["message"].as_str().unwrap().contains("migration required"));
    assert!(!path.exists()); assert!(!root.join("remote-hosts.json").exists());
    for args in [json!({}), json!({"legacySeed":[1,2]}), json!({"legacySeed":vec![0u16;33]}), json!({"legacySeed":vec![256u16;32]})] {
        assert!(rpc(&mut first, 3, "initialize_cloud_controller_identity", args).await["error"].is_object());
        assert!(!path.exists());
    }
    let seed = [9u8;32]; let expected = DeviceIdentity::from_seed(&seed).public_bytes();
    let denied = client::request_json(&owned.endpoint, "POST", "/v1/domain/commands", Some(&json!({
        "method":"initialize_cloud_controller_identity","args":{"legacySeed":seed},"native":false,"roots":[root],"connectionId":uuid::Uuid::new_v4()
    }))).unwrap();
    assert_eq!(denied["ok"], false); assert!(!path.exists());
    let (a,b) = tokio::join!(rpc(&mut first,4,"initialize_cloud_controller_identity",json!({"legacySeed":seed})),
        rpc(&mut second,4,"initialize_cloud_controller_identity",json!({"legacySeed":seed})));
    assert_eq!(a["result"], json!(expected)); assert_eq!(b["result"], json!(expected));
    let encrypted = fs::read(&path).unwrap();
    assert!(rpc(&mut second,5,"initialize_cloud_controller_identity",json!({"legacySeed":vec![10u8;32]})).await["error"].is_object());
    assert!(fs::read(&path).unwrap() == encrypted);
    assert_eq!(rpc(&mut first,6,"get_device_identity_pub",json!({})).await["result"],host);
    assert!(fs::read(root.join("device_identity.key")).unwrap() == host_bytes);
    drop(first); drop(second); drop(owned);
    let restarted = start(&root).await; let mut view = attach(&restarted.endpoint).await;
    assert_eq!(rpc(&mut view,1,"initialize_cloud_controller_identity",json!({"legacySeed":null})).await["result"],json!(expected));
    assert_eq!(DeviceIdentity::load_existing_at(&path).unwrap().unwrap().public_bytes(), expected);
    assert!(fs::read(&path).unwrap() == encrypted);
    assert!(fs::read(root.join("device_identity.key")).unwrap() == host_bytes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prior_runtime_identity_and_damaged_controller_files_cannot_be_rotated() {
    let root = std::env::temp_dir().join(format!("ridge-controller-prior-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let host = DeviceIdentity::load_or_create_at(&root.join("device_identity.key")).unwrap();
    let host_bytes = fs::read(root.join("device_identity.key")).unwrap();
    let record = HostRecord {id:"cloud:prior".into(),kind:HostKind::Rdg,label:"Prior Runtime Cloud".into(),addr:"prior".into(),
        status:HostStatus::Disconnected,detail:String::new(),sessions:Vec::new()};
    fs::write(root.join("remote-hosts.json"),serde_json::to_vec(&std::collections::HashMap::from([(record.id.clone(),record)])).unwrap()).unwrap();
    let owned = start(&root).await; let mut view = attach(&owned.endpoint).await;
    let path = root.join("cloud-controller-identity.key");
    assert!(rpc(&mut view,1,"initialize_cloud_controller_identity",json!({"legacySeed":vec![9u8;32]})).await["error"].is_object());
    assert!(!path.exists());
    assert_eq!(rpc(&mut view,2,"initialize_cloud_controller_identity",json!({"legacySeed":null})).await["result"],json!(host.public_bytes()));
    assert!(fs::read(root.join("device_identity.key")).unwrap() == host_bytes);
    drop(view); drop(owned);
    fs::write(&path,b"interrupted encrypted publication").unwrap();
    let restarted = start(&root).await; let mut view = attach(&restarted.endpoint).await;
    assert!(rpc(&mut view,3,"initialize_cloud_controller_identity",json!({"legacySeed":null})).await["error"].is_object());
    assert_eq!(fs::read(path).unwrap(),b"interrupted encrypted publication");
    assert!(fs::read(root.join("device_identity.key")).unwrap() == host_bytes);
}
