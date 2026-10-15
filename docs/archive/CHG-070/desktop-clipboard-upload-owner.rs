pub async fn save_clipboard_image_to_temp(png_base64: String) -> Result<String, String> {
    use base64::Engine;
    // 若是 data URL（`data:...;base64,XXXX`），只取逗号后的 base64 主体。
    let b64 = png_base64
        .split_once(',')
        .map(|(_, b)| b)
        .unwrap_or(png_base64.as_str());
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| format!("invalid base64: {e}"))?;
    tokio::task::spawn_blocking(move || save_png_bytes_to_temp(&bytes))
        .await
        .map_err(|_| "save image task panicked".to_string())?
}