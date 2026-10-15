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


/// 把 PNG 字节落盘成 `clip-<uuid>.png`，返回绝对路径字符串。两个 command 共用。
fn save_png_bytes_to_temp(png: &[u8]) -> Result<String, String> {
    use base64::Engine;
    crate::commands::runtime::request("save_clipboard_image_to_temp",serde_json::json!({"pngBase64":base64::engine::general_purpose::STANDARD.encode(png)}),true,crate::commands::runtime::window_connection("clipboard-read"))
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
        // 1) 位图（截图 / 图片软件「复制图片」，CF_DIB）：编码成 PNG 落盘，返回临时路径。
        if let Ok(image) = app.clipboard().read_image() {
            let png = encode_rgba_to_png(image.rgba(), image.width(), image.height())?;
            return Ok(Some(save_png_bytes_to_temp(&png)?));
        }
        // 2) 文件列表（资源管理器「复制」图片文件，CF_HDROP）：直接用原文件路径——文件已存在、
        //    无需落盘，由 CLI 识别路径为图片。剪贴板既无位图也无文件时返回 None，前端 fallback 文本。
        if let Some(path) = first_clipboard_image_file() {
            return Ok(Some(path));
        }
        Ok(None)
    })
    .await
    .map_err(|_| "clipboard read task panicked".to_string())?
}

/// Windows：读剪贴板文件列表（CF_HDROP，即资源管理器里「复制」图片文件的格式），
/// 返回第一个存在的图片文件绝对路径。剪贴板没有文件列表时 get_clipboard 返回 Err → None。
#[cfg(windows)]
fn first_clipboard_image_file() -> Option<String> {
    let files: Vec<String> = clipboard_win::get_clipboard(clipboard_win::formats::FileList).ok()?;
    ridge_core::clipboard::pick_image_file(files)
}

/// 非 Windows 暂不支持「复制文件」式粘贴（mac/Linux 文件列表格式各异，用户主路径是位图）。
#[cfg(not(windows))]
fn first_clipboard_image_file() -> Option<String> {
    None
}
