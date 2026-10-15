pub fn set_app_handle(handle: AppHandle) {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(endpoint) = ridge_kernel::client::running_endpoint() {
                let connection = crate::commands::runtime::window_connection("lsp-events");
                let url = format!("ws://127.0.0.1:{}/v1/domain/events?connectionId={connection}&native=true", endpoint.port);
                if let Ok(mut request) = url.into_client_request() {
                    if let Ok(token) = endpoint.token.parse() {
                        request.headers_mut().insert("x-ridge-kernel-token", token);
                        if let Ok((mut socket, _)) = tokio_tungstenite::connect_async(request).await {
                            while let Some(Ok(message)) = socket.next().await {
                                if let Ok(text) = message.to_text() {
                                    if let Ok(event) = serde_json::from_str::<Value>(text) {
                                        if event["name"] == "lsp://diagnostics" {
                                            let _ = handle.emit("lsp://diagnostics", event["payload"].clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}
