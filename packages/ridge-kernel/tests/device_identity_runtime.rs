#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_core::device_identity::{DeviceIdentity, verify};
use ridge_kernel::client;
use serde_json::json;
use uuid::Uuid;
use ridge_core::device_identity::DeviceBindingSigner;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_binding_signer_retains_legacy_key_across_clients_and_restart() {
    let root = std::env::temp_dir().join(format!("ridge-device-key-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let key = root.join("device_identity.key");
    let legacy = DeviceIdentity::load_or_create_at(&key).unwrap();
    let original = std::fs::read(&key).unwrap();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let mut second = attach(&owned.endpoint).await;
    let public: Vec<u8> = serde_json::from_value(rpc(&mut first, 1, "get_device_identity_pub", json!({})).await["result"].clone()).unwrap();
    assert_eq!(public, legacy.public_bytes());
    let context = vec![0u8, 1, 2, 255];
    let signature: Vec<u8> = serde_json::from_value(rpc(&mut second, 1, "sign_device_identity", json!({"context":context})).await["result"].clone()).unwrap();
    let public: [u8;32] = public.try_into().unwrap();
    let signature: [u8;64] = signature.try_into().unwrap();
    let mut message = b"ridge-id-bind-v1".to_vec(); message.extend_from_slice(&context);
    assert!(verify(&public, &message, &signature));
    assert!(!verify(&public, &context, &signature), "signer must retain its fixed binding domain");
    let cli = ridge_kernel::identity_client::DeviceBindingClient::connect(owned.endpoint.clone()).unwrap();
    assert_eq!(cli.public_bytes(), public);
    assert!(verify(&public, &message, &cli.sign_binding(&context).unwrap()));
    for args in [json!({}), json!({"context":[256]}), json!({"context":"private"}), json!({"context":vec![0u8;65537]})] {
        assert!(rpc(&mut second, 2, "sign_device_identity", args).await["error"].is_object());
    }
    for method in ["get_device_identity_pub", "sign_device_identity"] {
        let denied = client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
            "method":method,"args":{"context":context},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
        }))).unwrap();
        assert_eq!(denied["ok"], false);
    }
    drop(first);
    assert_eq!(rpc(&mut second, 3, "get_device_identity_pub", json!({})).await["result"], json!(public.to_vec()));
    assert_eq!(std::fs::read(&key).unwrap(), original);
    drop(second); drop(owned);
    assert!(cli.sign_binding(&context).is_err(), "service unavailability must not fall back to a shell-held private key");
    let restarted = start(&root).await;
    let mut presentation = attach(&restarted.endpoint).await;
    assert_eq!(rpc(&mut presentation, 1, "get_device_identity_pub", json!({})).await["result"], json!(public.to_vec()));
    assert_eq!(std::fs::read(&key).unwrap(), original, "owner migration must not rotate or rewrite the existing key");
}

#[test]
fn corrupt_device_key_refuses_service_start_without_replacement() {
    let root = std::env::temp_dir().join(format!("ridge-damaged-device-key-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let damaged = b"corrupt encrypted device key";
    std::fs::write(root.join("device_identity.key"), damaged).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR", &root).env("RIDGE_KERNEL_FS_ROOT", &root)
        .env("RIDGE_WORKSPACE_SAVE_DIR", &root).env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .output().unwrap();
    assert!(!result.status.success());
    assert_eq!(std::fs::read(root.join("device_identity.key")).unwrap(), damaged);
    assert!(!root.join("kernel.json").exists());
}
