//! 通用「文件列表」剪贴板互通（Windows CF_HDROP）：
//! - 从系统资源管理器「复制」的文件 → 文件树内 Ctrl+V 粘贴（读 CF_HDROP）。
//! - 文件树内 Ctrl+C 复制的文件 → 系统资源管理器「粘贴」出真实文件（写 CF_HDROP）。
//!
//! 仅 Windows 实现（mac/Linux 文件列表剪贴板格式各异，且本项目主路径在 Windows）。
//! 非 Windows 平台读返回空、写返回 false（由前端 fallback 用 clipboard-manager 写纯文本）。
//!
//! 纯字符串/路径判定（去空、去重）下沉到 `ridge_core::clipboard`（不链 Tauri、可单测）。

/// 读系统剪贴板里的文件列表（CF_HDROP）。无文件列表 / 非 Windows 时返回空 Vec。
///
/// `get_clipboard` 底层操作系统剪贴板，放进 `spawn_blocking` 避免阻塞异步运行时。
#[tauri::command]
pub async fn read_clipboard_file_paths() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(read_clipboard_file_paths_blocking)
        .await
        .map_err(|_| "clipboard read task panicked".to_string())
}

#[cfg(windows)]
fn read_clipboard_file_paths_blocking() -> Vec<String> {
    let files: Vec<String> =
        clipboard_win::get_clipboard(clipboard_win::formats::FileList).unwrap_or_default();
    ridge_core::clipboard::sanitize_file_list(files)
}

#[cfg(not(windows))]
fn read_clipboard_file_paths_blocking() -> Vec<String> {
    Vec::new()
}

/// 把一组文件路径写进系统剪贴板。Windows 下**同一会话内**同时写：
/// - CF_HDROP（资源管理器「粘贴」得到真实文件副本）；
/// - CF_UNICODETEXT（终端 / 编辑器粘贴得到换行分隔的路径文本）。
///
/// 返回是否已写入文本：Windows = true，前端据此跳过自己的 `writeText`；
/// 非 Windows = false，前端 fallback 用 clipboard-manager 写纯文本。
#[tauri::command]
pub fn write_clipboard_file_paths(paths: Vec<String>) -> Result<bool, String> {
    let clean = ridge_core::clipboard::sanitize_file_list(paths);
    if clean.is_empty() {
        return Ok(false);
    }
    write_clipboard_file_paths_impl(&clean)
}

#[cfg(windows)]
fn write_clipboard_file_paths_impl(paths: &[String]) -> Result<bool, String> {
    use clipboard_win::formats::{FileList, Unicode};
    use clipboard_win::{Clipboard, Setter};

    // 文本镜像用换行分隔，沿用既有「复制路径」格式。
    let text = paths.join("\n");

    let _clip =
        Clipboard::new_attempts(10).map_err(|e| format!("open clipboard failed: {e}"))?;
    // 顺序不可换：先写文本（`set_string` 内部 DoClear——清空剪贴板取得所有权并置
    // CF_UNICODETEXT），再写文件列表（`set_file_list` 内部 NoClear——仅追加 CF_HDROP，
    // 不会清掉上一步的文本）。若反过来，写文本时的 DoClear 会把 CF_HDROP 清掉。
    Unicode
        .write_clipboard(&text)
        .map_err(|e| format!("set clipboard text failed: {e}"))?;
    FileList
        .write_clipboard(paths)
        .map_err(|e| format!("set clipboard files failed: {e}"))?;
    Ok(true)
}

#[cfg(not(windows))]
fn write_clipboard_file_paths_impl(_paths: &[String]) -> Result<bool, String> {
    Ok(false)
}
