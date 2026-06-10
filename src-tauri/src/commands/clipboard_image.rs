//! 剪贴板图片 → 临时 PNG 文件，让终端里运行的 TUI（如 Claude Code）以「粘贴文件路径」的
//! 方式接收图片。
//!
//! 背景：Windows 上 Claude Code 自读系统剪贴板图片是坏的（上游 open bug），且没有任何终端
//! 协议能把二进制图片喂给前台程序；业界（WezTerm 等）的事实标准做法是——终端把剪贴板图片
//! 落盘成临时文件，再把文件路径作为 bracketed-paste 文本粘进去，由 CLI 识别路径为图片附件。
//!
//! 两条读图路径，都落盘到「服务器端」（运行 PTY/CLI 的这一端）的临时目录：
//! - 桌面：`read_clipboard_image_to_temp` 在后端直接读本机系统剪贴板（= 用户剪贴板）。
//! - 远程 Web：浏览器端读客户端剪贴板图片，base64 编码后经 `save_clipboard_image_to_temp`
//!   落到服务器端（远程下 `invoke` 经 WS 路由到 host，本机 `read_image` 读到的是 host 的
//!   剪贴板而非远程用户的，故远程必须前端读图）。

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use uuid::Uuid;

/// 临时图片落盘目录：`<系统临时目录>/wind-clipboard/`。
fn temp_image_dir() -> PathBuf {
    std::env::temp_dir().join("wind-clipboard")
}

/// 把 PNG 字节落盘成 `clip-<uuid>.png`，返回绝对路径字符串。两个 command 共用。
fn save_png_bytes_to_temp(png: &[u8]) -> Result<String, String> {
    let dir = temp_image_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create temp dir failed: {e}"))?;
    let path = dir.join(format!("clip-{}.png", Uuid::new_v4()));
    std::fs::write(&path, png).map_err(|e| format!("write png failed: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// 把 RGBA8 像素编码为 PNG 字节（arboard 读到的剪贴板图片即 RGBA8）。
fn encode_rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png header failed: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("png encode failed: {e}"))?;
    }
    Ok(out)
}

/// 桌面：读本机系统剪贴板里的图片，编码为 PNG 落盘，返回路径。
/// 剪贴板没有图片（或读取失败）时返回 `Ok(None)`，让前端 fallback 到文本粘贴。
///
/// 注意：`read_image()` 底层是 arboard，其文档明确「不要在主线程调用，Linux 上可能死锁」，
/// 故整段放进 `spawn_blocking`。
#[tauri::command]
pub async fn read_clipboard_image_to_temp(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
        let image = match app.clipboard().read_image() {
            Ok(img) => img,
            // 剪贴板里没有位图（只有文本/为空）时 arboard 返回 Err —— 视作「无图」。
            Err(_) => return Ok(None),
        };
        let png = encode_rgba_to_png(image.rgba(), image.width(), image.height())?;
        Ok(Some(save_png_bytes_to_temp(&png)?))
    })
    .await
    .map_err(|_| "clipboard read task panicked".to_string())?
}

/// 远程/通用：前端传来已编码好的 PNG（base64），落盘到服务器端临时目录，返回路径。
/// 容忍 `data:image/png;base64,...` 这种 data URL 前缀。
#[tauri::command]
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

/// 启动期清理：删掉临时目录里超过 `max_age` 的旧图片。不做单文件即时删除，避免与 CLI
/// 异步读图竞态（CLI 可能在粘贴后才去读文件）。目录不存在时静默返回。
pub fn cleanup_old_temp_images(max_age: Duration) {
    let dir = temp_image_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if now
            .duration_since(modified)
            .map(|age| age > max_age)
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
