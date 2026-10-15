#[path="common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach,rpc,start};
use ridge_kernel::client;
use ridge_core::RemoteTotp;
use serde_json::json;
use uuid::Uuid;
use base64::Engine;

#[test]
fn damaged_remote_blacklist_refuses_boot_without_replacement() {
    let root=std::env::temp_dir().join(format!("ridge-blacklist-damage-{}",Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path=root.join("remote-blacklist.json");
    std::fs::write(&path,b"damaged legacy blacklist").unwrap();
    let result=std::process::Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR",&root).env("RIDGE_KERNEL_FS_ROOT",&root)
        .env("RIDGE_WORKSPACE_SAVE_DIR",&root).env("RIDGE_AUTH_FILE",root.join("auth.json"))
        .output().unwrap();
    assert!(!result.status.success());assert!(!root.join("kernel.json").exists());
    assert_eq!(std::fs::read(&path).unwrap(),b"damaged legacy blacklist");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn shells_and_host_share_legacy_seed_reset_identity_and_grants() {
    let root=std::env::temp_dir().join(format!("ridge-auth-owner-{}",Uuid::new_v4()));
    let seeds=root.join("totp");
    let legacy=RemoteTotp::load_or_create_checked_in(&seeds,"default").unwrap();
    let seed_path=std::fs::read_dir(&seeds).unwrap().next().unwrap().unwrap().path();
    let raw=std::fs::read(&seed_path).unwrap();
    let owned=start(&root).await;
    let mut first=attach(&owned.endpoint).await;
    let mut second=attach(&owned.endpoint).await;
    let host=ridge_kernel::auth_client::remote_auth(owned.endpoint.clone());
    let initial=rpc(&mut first,1,"remote_totp_info",json!({"machineName":"owned test"})).await;
    assert_eq!(initial["result"]["uri"],legacy.otpauth_uri("owned test"));
    assert!(host.verify(&legacy.current_code()));
    let transcript=b"owned channel binding";
    assert!(host.verify_bind_tag(transcript,&legacy.current_bind_tag(transcript)));
    assert!(!host.verify_bind_tag(b"another channel",&legacy.current_bind_tag(transcript)));
    assert_eq!(std::fs::read(&seed_path).unwrap(),raw,"reads do not rewrite legacy seed");
    let key=base64::engine::general_purpose::STANDARD.encode([7u8;32]);
    let args=json!({"ctrlPubB64":key});
    assert_eq!(rpc(&mut first,2,"totp_trust_check",args.clone()).await["result"],false);
    assert!(rpc(&mut first,3,"totp_trust_record",args.clone()).await["error"].is_null());
    assert!(host.trust_check(&[7;32]));
    assert_eq!(rpc(&mut second,1,"totp_trust_check",args.clone()).await["result"],true);
    assert!(rpc(&mut second,2,"remote_set_totp_identity",json!({"username":"another account"})).await["error"].is_null());
    assert!(!host.trust_check(&[7;32]));
    assert_eq!(host.current_identity(),"another account");
    assert!(rpc(&mut first,4,"remote_set_totp_identity",json!({"username":null})).await["error"].is_null());
    assert!(host.trust_check(&[7;32]));
    assert!(rpc(&mut first,5,"remote_reset_totp",json!({})).await["error"].is_null());
    let reset=rpc(&mut second,3,"remote_totp_info",json!({"machineName":"owned test"})).await["result"].clone();
    assert_ne!(reset["uri"],initial["result"]["uri"]);
    assert!(host.verify(reset["code"].as_str().unwrap()));
    assert_eq!(host.otpauth_uri("owned test"),reset["uri"].as_str().unwrap());
    let reset_raw=std::fs::read(&seed_path).unwrap();
    assert_ne!(reset_raw,raw);
    for method in ["remote_totp_info","verify_remote_totp","verify_remote_totp_bind","remote_reset_totp","remote_set_totp_identity","totp_trust_check","totp_trust_record","totp_trust_revoke_all"] {
        let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
            "method":method,"args":args,"native":false,"roots":[],"connectionId":Uuid::new_v4()
        }))).unwrap();
        assert_eq!(denied["ok"],false,"remote must not access {method}");
    }
    assert_eq!(std::fs::read(&seed_path).unwrap(),reset_raw);
    for bad in [json!({"ctrlPubB64":"bad"}),json!({"ctrlPubB64":"AA=="})] {
        assert!(rpc(&mut second,4,"totp_trust_record",bad).await["error"].is_object());
    }
    drop(first);drop(second);drop(owned);
    assert!(!host.verify(reset["code"].as_str().unwrap()),"no authentication fallback when Runtime stops");
    assert!(host.call("remote_reset_totp",json!({})).is_err());
    assert_eq!(std::fs::read(&seed_path).unwrap(),reset_raw);
    let restarted=start(&root).await;
    let mut presentation=attach(&restarted.endpoint).await;
    assert_eq!(rpc(&mut presentation,1,"remote_totp_info",json!({"machineName":"owned test"})).await["result"]["uri"],reset["uri"]);
    assert_eq!(rpc(&mut presentation,2,"totp_trust_check",args.clone()).await["result"],true);
    assert!(rpc(&mut presentation,3,"totp_trust_revoke_all",json!({})).await["error"].is_null());
    assert_eq!(rpc(&mut presentation,4,"totp_trust_check",args).await["result"],false);
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn damaged_seed_refuses_boot_and_failed_identity_switch_preserves_active_seed() {
    let root=std::env::temp_dir().join(format!("ridge-auth-damage-{}",Uuid::new_v4()));
    let seeds=root.join("totp");
    RemoteTotp::load_or_create_checked_in(&seeds,"default").unwrap();
    let default=std::fs::read_dir(&seeds).unwrap().next().unwrap().unwrap().path();
    RemoteTotp::load_or_create_checked_in(&seeds,"damaged account").unwrap();
    let other=std::fs::read_dir(&seeds).unwrap().map(|e|e.unwrap().path()).find(|p|p!=&default).unwrap();
    std::fs::write(&other,b"damaged encrypted seed").unwrap();
    let owned=start(&root).await;
    let mut presentation=attach(&owned.endpoint).await;
    let before=rpc(&mut presentation,1,"remote_totp_info",json!({})).await["result"].clone();
    assert!(rpc(&mut presentation,2,"remote_set_totp_identity",json!({"username":"damaged account"})).await["error"].is_object());
    let after=rpc(&mut presentation,3,"remote_totp_info",json!({})).await["result"].clone();
    assert_eq!(before["uri"],after["uri"]);assert_eq!(after["identity"],"default");
    assert_eq!(std::fs::read(&other).unwrap(),b"damaged encrypted seed");
    std::fs::write(&default,b"damaged default seed").unwrap();
    assert!(rpc(&mut presentation,4,"remote_reset_totp",json!({})).await["error"].is_object());
    assert_eq!(std::fs::read(&default).unwrap(),b"damaged default seed");
    drop(presentation);drop(owned);
    let result=std::process::Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR",&root).env("RIDGE_KERNEL_FS_ROOT",&root)
        .env("RIDGE_WORKSPACE_SAVE_DIR",&root).env("RIDGE_AUTH_FILE",root.join("auth.json"))
        .output().unwrap();
    assert!(!result.status.success());
    assert!(!root.join("kernel.json").exists());
    assert_eq!(std::fs::read(&default).unwrap(),b"damaged default seed");
}
