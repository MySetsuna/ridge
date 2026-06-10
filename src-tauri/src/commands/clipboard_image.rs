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

/// 是否带常见图片扩展名（不含 svg —— CLI 多把 svg 当文本/矢量而非图片附件）。
fn is_image_ext(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// 从一组文件路径里挑出第一个真实存在的图片文件（纯逻辑，便于单测）。
fn pick_image_file(files: Vec<String>) -> Option<String> {
    files
        .into_iter()
        .find(|f| is_image_ext(f) && std::path::Path::new(f).is_file())
}

/// Windows：读剪贴板文件列表（CF_HDROP，即资源管理器里「复制」图片文件的格式），
/// 返回第一个存在的图片文件绝对路径。剪贴板没有文件列表时 get_clipboard 返回 Err → None。
#[cfg(windows)]
fn first_clipboard_image_file() -> Option<String> {
    let files: Vec<String> =
        clipboard_win::get_clipboard(clipboard_win::formats::FileList).ok()?;
    pick_image_file(files)
}

/// 非 Windows 暂不支持「复制文件」式粘贴（mac/Linux 文件列表格式各异，用户主路径是位图）。
#[cfg(not(windows))]
fn first_clipboard_image_file() -> Option<String> {
    None
}

/// 把「复制为路径 / Copy as path」得到的文本（可能带引号）解析成一个真实存在的图片文件
/// 绝对路径。仅当文本是单一、带图片扩展名、且文件存在的路径时返回 Some（前端据此粘**裸**
/// 路径，CLI 才会识别为图片）；否则 None → 走普通文本粘贴，绝不误伤普通文本。
#[tauri::command]
pub fn resolve_pasted_image_path(text: String) -> Option<String> {
    let trimmed = text.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
        return None;
    }
    if is_image_ext(trimmed) && std::path::Path::new(trimmed).is_file() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_strips_quotes_for_existing_image() {
        // 「复制为路径 / Copy as path」：剪贴板是带引号的图片路径文本。
        let dir = std::env::temp_dir().join("wind-clip-rtest");
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("shot.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let p = img.to_string_lossy().to_string();

        assert_eq!(resolve_pasted_image_path(format!("\"{p}\"")), Some(p.clone()));
        assert_eq!(resolve_pasted_image_path(p.clone()), Some(p.clone()));
        assert_eq!(resolve_pasted_image_path(format!("  {p}  ")), Some(p.clone()));

        let _ = std::fs::remove_file(&img);
    }

    #[test]
    fn resolve_rejects_non_image_and_missing() {
        // 普通文本不误伤
        assert_eq!(resolve_pasted_image_path("hello world".into()), None);
        // 图片扩展名但文件不存在
        assert_eq!(resolve_pasted_image_path("C:/nope/x.png".into()), None);
        // 多行（粘贴的整段文本）不当成路径
        assert_eq!(resolve_pasted_image_path("a.png\nb".into()), None);
        // 真实存在但非图片扩展名
        let dir = std::env::temp_dir().join("wind-clip-rtest2");
        std::fs::create_dir_all(&dir).unwrap();
        let txt = dir.join("note.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert_eq!(
            resolve_pasted_image_path(txt.to_string_lossy().to_string()),
            None
        );
        let _ = std::fs::remove_file(&txt);
    }

    #[test]
    fn pick_image_file_finds_existing_image() {
        // 「复制图片文件」：CF_HDROP 读到的文件列表里挑出图片文件。
        let dir = std::env::temp_dir().join("wind-clip-pick");
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pic.png");
        std::fs::write(&img, b"x").unwrap();
        let p = img.to_string_lossy().to_string();

        let files = vec!["C:/whatever/readme.txt".to_string(), p.clone()];
        assert_eq!(pick_image_file(files), Some(p.clone()));
        assert_eq!(pick_image_file(vec!["a.txt".into(), "b.doc".into()]), None);

        let _ = std::fs::remove_file(&img);
    }
}
