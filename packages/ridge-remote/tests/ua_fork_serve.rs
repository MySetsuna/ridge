//! 端到端钉：**真起一个 HTTP server、真发 HTTP 请求**，验证 UA 分流发对了页。
//!
//! 为什么不是单测：iter-62 之前的 bug 恰恰不在某个谓词里，而在「谓词 → 取哪份
//! 字节」这条装配链上（旧 `ui_dir` 曾在桌面产物缺失时回落移动目录，于是磁盘上的
//! 手机 index 冒充了桌面壳）。只断言 `wants_desktop_ui` 为真是抓不住的——必须
//! 沿着 socket 把真正回给浏览器的那份 HTML 拿回来看。
//!
//! 场景刻意模拟**单文件 `rdg`**：磁盘无 `remote-dist`，桌面 SPA 只能来自内嵌产物。
//!
//! 需要 `embed-ui`（内嵌产物）才有意义，故整文件 cfg 门控。

#![cfg(feature = "embed-ui")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use axum::extract::FromRef;
use ridge_remote::serve::{serve_router, ServeState, UaServeConfig};

/// 让 `serve_router::<ServeState>()` 能直接以 `ServeState` 作主 State。
struct Harness {
    port: u16,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

fn state() -> ServeState {
    ServeState {
        cfg: UaServeConfig {
            // 指向一个不存在的统一根：两套 UI 都只能走内嵌。
            remote_dir: PathBuf::from("__no_such_remote_dist__"),
        },
        tls_enabled: false,
        enabled: Arc::new(AtomicBool::new(true)),
    }
}

async fn start() -> Harness {
    let st = state();
    let app = serve_router::<ServeState>().with_state(ServeState::from_ref(&st));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let shutdown = async {
            let _ = rx.await;
        };
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await;
    });
    Harness {
        port,
        _shutdown: tx,
    }
}

/// 手写一次 HTTP/1.1 GET（不引第三方 client），返回 (状态行, 响应体)。
///
/// **必须**配 `flavor = "multi_thread"`：这是阻塞式 std I/O，若与 server task 同处
/// 单线程 runtime，读会把 executor 占死、server 永远得不到调度（实测四条测试全部挂住）。
fn get(port: u16, path: &str, ua: &str) -> (String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nUser-Agent: {ua}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let status = head.lines().next().unwrap_or("").to_string();
    (status, body.to_string())
}

const DESKTOP_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36";
const IPHONE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Safari/605";

/// 主钉：同一个端口，桌面 UA 拿到桌面 SPA 壳，手机 UA 拿到手机 SPA 壳。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_browser_gets_desktop_spa_and_phone_gets_mobile_spa() {
    let h = start().await;

    let (status, desktop) = get(h.port, "/", DESKTOP_UA);
    assert!(status.contains("200"), "desktop GET / → {status}");
    assert!(
        desktop.contains("_app/immutable/entry/"),
        "电脑浏览器必须拿到 SvelteKit 桌面壳，实得：{}",
        &desktop[..desktop.len().min(500)]
    );
    assert!(
        !desktop.contains("src=\"/assets/"),
        "电脑浏览器拿到了手机 SPA（正是要修的串台）"
    );

    let (status, mobile) = get(h.port, "/", IPHONE_UA);
    assert!(status.contains("200"), "phone GET / → {status}");
    assert!(
        mobile.contains("/assets/"),
        "手机必须拿到轻量移动壳，实得：{}",
        &mobile[..mobile.len().min(500)]
    );
    assert_ne!(desktop, mobile, "两端拿到的必须是不同的壳");
}

/// `?ui=` 显式覆盖照旧生效（边缘浏览器 / 排障用）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_ui_override_wins_over_ua() {
    let h = start().await;
    let (_, forced_mobile) = get(h.port, "/?ui=mobile", DESKTOP_UA);
    assert!(forced_mobile.contains("/assets/"));
    let (_, forced_desktop) = get(h.port, "/?ui=desktop", IPHONE_UA);
    assert!(forced_desktop.contains("_app/immutable/entry/"));
}

/// 壳能开还不够：桌面 SPA 的 `_app/*` 资产也必须从**桌面**内嵌包取到，
/// 否则页面白屏（旧实现的 SPA fallback 只会回落手机内嵌包）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_app_assets_resolve_from_the_embedded_desktop_bundle() {
    let h = start().await;
    let (status, body) = get(h.port, "/_app/version.json", DESKTOP_UA);
    assert!(status.contains("200"), "_app/version.json → {status}");
    assert!(
        body.contains("version"),
        "桌面包的 _app 资产没取到，页面会白屏：{body}"
    );
}

/// `?ui=` 覆盖后**后续资产请求**也必须取得到——这是 iter-62 手机端 e2e 实测抓到的
/// 白屏真因：覆盖参数只在页面那一次请求上，浏览器随后拉 `/assets/index-*.js` /
/// `/_app/immutable/*` 时不带它，于是又被 UA 判回另一套产物，整页 404。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn assets_resolve_across_ui_kinds_because_the_override_is_not_on_asset_requests() {
    let h = start().await;

    // 桌面 UA 打开手机页后拉手机资产：URL 上没有 `?ui=mobile`。
    let (_, shell) = get(h.port, "/?ui=mobile", DESKTOP_UA);
    let mobile_js = shell
        .split("src=\"")
        .find_map(|s| s.split('"').next().filter(|p| p.starts_with("/assets/")))
        .expect("手机壳里应有 /assets/ 入口脚本");
    let (status, body) = get(h.port, mobile_js, DESKTOP_UA);
    assert!(
        status.contains("200"),
        "{mobile_js} → {status}（页面会白屏）"
    );
    assert!(!body.is_empty(), "{mobile_js} 空响应");

    // 反向：手机 UA 打开桌面页后拉 `_app` 资产，同样不带覆盖参数。
    let (status, body) = get(h.port, "/_app/version.json", IPHONE_UA);
    assert!(status.contains("200"), "_app/version.json → {status}");
    assert!(body.contains("version"), "桌面资产跨形态没取到：{body}");
}

/// 跨形态回退**只对具体资产**——壳绝不跨，否则又回到「电脑浏览器被发手机页」。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_never_falls_back_across_ui_kinds() {
    let h = start().await;
    let (_, desktop) = get(h.port, "/index.html", DESKTOP_UA);
    assert!(
        !desktop.contains("src=\"/assets/"),
        "index.html 跨形态串台了：{}",
        &desktop[..desktop.len().min(400)]
    );
}

/// 资产路径的穿越守卫：`/assets/*` 通配段此前被直接 join 进产物目录。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn asset_path_traversal_is_rejected() {
    let h = start().await;
    for p in [
        "/assets/../../../../Windows/win.ini",
        "/assets/..%2f..%2fetc%2fpasswd",
    ] {
        let (status, _) = get(h.port, p, DESKTOP_UA);
        assert!(status.contains("404"), "{p} → {status}");
    }
}

/// 未知客户端路由回落到**对应形态**的壳，而不是另一端的壳。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_client_route_falls_back_to_its_own_shell() {
    let h = start().await;
    let (_, desktop) = get(h.port, "/some/spa/route", DESKTOP_UA);
    assert!(desktop.contains("_app/immutable/entry/"));
    let (_, mobile) = get(h.port, "/some/spa/route", IPHONE_UA);
    assert!(mobile.contains("/assets/"));
}
