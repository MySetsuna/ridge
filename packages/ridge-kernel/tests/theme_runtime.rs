//! Theme state belongs to one Runtime across concurrent presentations and CLI/Remote requests.
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::fs;
use uuid::Uuid;

#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};

fn command(endpoint: &KernelEndpoint, method: &str, args: Value, native: bool) -> Value {
    client::request_json(
        endpoint,
        "POST",
        "/v1/domain/commands",
        Some(&json!({
            "method":method,"args":args,"native":native,"roots":[],"connectionId":Uuid::new_v4()
        })),
    )
    .unwrap()
}
fn entry(label: &str) -> Value {
    json!({"id":"","label":label,"type":"dark","loader":{"primary":"#fff","secondary":"#000"},"colors":{"bg":"#010203"}})
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn themes_share_authority_preserve_old_data_and_survive_presentation_and_runtime_exit() {
    let root = std::env::temp_dir().join(format!("ridge-theme-runtime-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut legacy = entry("Legacy desktop theme");
    legacy["id"] = json!("custom-legacy");
    let legacy_bytes = serde_json::to_vec(
        &json!({"version":1,"themes":[legacy],"legacyUnknown":"retained in archive"}),
    )
    .unwrap();
    fs::write(root.join("user-themes.json"), &legacy_bytes).unwrap();
    fs::write(root.join("active-theme.txt"), "custom-legacy").unwrap();
    let owned = start(&root).await;
    let endpoint = &owned.endpoint;
    assert_eq!(
        command(endpoint, "get_active_theme_entry", json!({}), false)["value"]["id"],
        "custom-legacy"
    );
    assert_eq!(
        fs::read(root.join("user-themes.json")).unwrap(),
        legacy_bytes,
        "reading legacy data must not rewrite it"
    );
    let mut first = attach(endpoint).await;
    let mut second = attach(endpoint).await;
    let (a, b) = tokio::join!(
        rpc(
            &mut first,
            1,
            "save_user_theme",
            json!({"entry":entry("First")})
        ),
        rpc(
            &mut second,
            1,
            "save_user_theme",
            json!({"entry":entry("Second")})
        )
    );
    assert!(a["error"].is_null() && b["error"].is_null(), "{a} {b}");
    let first_id = a["result"]["id"].as_str().unwrap();
    let second_id = b["result"]["id"].as_str().unwrap();
    assert_ne!(first_id, second_id);
    let stored: Value =
        serde_json::from_slice(&fs::read(root.join("user-themes.json")).unwrap()).unwrap();
    assert_eq!(stored["themes"].as_array().unwrap().len(), 3);
    assert!(stored["themes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|theme| theme["id"] == "custom-legacy"));
    let image = root.join("wallpaper.png");
    fs::write(&image, [1, 2, 3]).unwrap();
    let bg = rpc(
        &mut first,
        2,
        "save_theme_bg_image_from_path",
        json!({"path":image}),
    )
    .await;
    assert!(bg["error"].is_null(), "{bg}");
    let mut edit = a["result"].clone();
    edit["bgImage"] = bg["result"].clone();
    assert!(rpc(&mut first, 3, "save_user_theme", json!({"entry":edit})).await["error"].is_null());
    assert!(rpc(
        &mut first,
        4,
        "set_active_theme",
        json!({"themeId":first_id})
    )
    .await["error"]
        .is_null());
    let active = command(endpoint, "get_active_theme_entry", json!({}), false);
    assert_eq!(active["ok"], true);
    assert_eq!(
        active["value"]["id"], first_id,
        "Remote observes the same active theme"
    );
    let original = fs::read(root.join("user-themes.json")).unwrap();
    for (method, args) in [
        ("save_user_theme", json!({"entry":entry("Forbidden")})),
        ("delete_user_theme", json!({"id":first_id})),
        ("save_theme_bg_image", json!({"bytes":[9],"ext":"png"})),
        ("save_theme_bg_image_from_path", json!({"path":image})),
    ] {
        assert_eq!(
            command(endpoint, method, args, false)["ok"],
            false,
            "Remote cannot gain local theme write privileges"
        );
    }
    assert_eq!(fs::read(root.join("user-themes.json")).unwrap(), original);
    edit["bgImage"] = json!("../auth.json");
    assert!(!rpc(&mut first, 5, "save_user_theme", json!({"entry":edit})).await["error"].is_null());
    assert_eq!(fs::read(root.join("user-themes.json")).unwrap(), original);
    fs::write(root.join("user-themes.json"), b"{interrupted external edit").unwrap();
    assert!(!rpc(
        &mut first,
        6,
        "save_user_theme",
        json!({"entry":entry("Cannot erase")})
    )
    .await["error"]
        .is_null());
    assert_eq!(
        fs::read(root.join("user-themes.json")).unwrap(),
        b"{interrupted external edit"
    );
    fs::write(root.join("user-themes.json"), &original).unwrap();
    first.close(None).await.unwrap();
    assert_eq!(
        rpc(&mut second, 7, "get_active_theme_entry", json!({})).await["result"]["id"],
        first_id
    );
    second.close(None).await.unwrap();
    assert!(client::health_ok(endpoint));
    let first_id = first_id.to_owned();
    let second_id = second_id.to_owned();
    let asset = bg["result"].as_str().unwrap().to_owned();
    drop(owned);
    let restarted = start(&root).await;
    assert_eq!(
        command(
            &restarted.endpoint,
            "get_active_theme_entry",
            json!({}),
            false
        )["value"]["id"],
        first_id
    );
    assert_eq!(
        command(
            &restarted.endpoint,
            "delete_user_theme",
            json!({"id":second_id}),
            true
        )["ok"],
        true
    );
    assert_eq!(
        command(
            &restarted.endpoint,
            "delete_user_theme",
            json!({"id":first_id}),
            true
        )["ok"],
        true
    );
    assert_eq!(
        fs::read(root.join("theme-assets").join(asset)).unwrap(),
        vec![1, 2, 3]
    );
    let archives: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|file| {
            file.file_name()
                .to_string_lossy()
                .starts_with("user-themes.archive-")
        })
        .collect();
    assert!(archives.len() >= 4);
    assert!(archives
        .iter()
        .any(|archive| fs::read(archive.path()).unwrap() == legacy_bytes));
    for archive in archives {
        serde_json::from_slice::<Value>(&fs::read(archive.path()).unwrap()).unwrap();
    }
}
