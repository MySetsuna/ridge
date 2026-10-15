use futures::{SinkExt, StreamExt};
use ridge_kernel::{client, local_client::ClientGrant, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
pub struct Owned {
    pub endpoint: KernelEndpoint,
    process: Child,
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = client::shutdown_endpoint(&self.endpoint, Duration::from_secs(3));
        let _ = self.process.wait();
    }
}
pub async fn start(root: &Path) -> Owned {
    start_with_env(root,&[]).await
}
pub async fn start_with_env(root: &Path,env:&[(&str,std::path::PathBuf)]) -> Owned {
    let mut command=Command::new(env!("CARGO_BIN_EXE_ridge-kernel"));
    for (key,value) in env {command.env(key,value);}
    let mut process = command.env("RIDGE_KERNEL_DATA_DIR", root)
        .env("RIDGE_KERNEL_FS_ROOT", root)
        .env("RIDGE_WORKSPACE_SAVE_DIR", root)
        .env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(process.try_wait().unwrap().is_none());
        if let Ok(bytes) = fs::read(root.join("kernel.json")) {
            if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                if endpoint.pid == process.id() && client::health_ok(&endpoint) {
                    return Owned { endpoint, process };
                }
            }
        }
        assert!(Instant::now() < deadline, "Runtime startup deadline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
pub async fn attach(endpoint: &KernelEndpoint) -> Socket {
    attach_grant(&grant(endpoint)).await
}
pub fn grant(endpoint: &KernelEndpoint) -> ClientGrant {
    let device = Uuid::new_v4().to_string();
    let origin = "http://127.0.0.1:5173";
    serde_json::from_value(
        client::request_json(
            endpoint,
            "POST",
            "/v1/local/clients",
            Some(&json!({"deviceId":device,"origin":origin})),
        )
        .unwrap(),
    )
    .unwrap()
}
pub async fn attach_grant(grant: &ClientGrant) -> Socket {
    let mut request = format!("{}?deviceId={}", grant.ws_url, grant.device_id)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", grant.origin.parse().unwrap());
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("ridge-client, auth-{}", grant.token)
            .parse()
            .unwrap(),
    );
    tokio_tungstenite::connect_async(request).await.unwrap().0
}
pub async fn rpc(socket: &mut Socket, id: u64, method: &str, args: Value) -> Value {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":args}).to_string(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                let response: Value = serde_json::from_str(&text).unwrap();
                if response["id"] == id {
                    return response;
                }
            }
        }
    })
    .await
    .expect("business RPC deadline")
}
