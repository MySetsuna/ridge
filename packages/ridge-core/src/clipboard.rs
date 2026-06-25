//! 剪贴板「图片文件路径」判定的纯逻辑——与具体宿主（Tauri 桌面 / headless）无关。
//!
//! 真正读系统剪贴板（CF_HDROP 文件列表 / CF_DIB 位图）的平台代码留在各宿主侧
//! （桌面 `src-tauri` 用 `clipboard-win` + `arboard`）；这里只承载**可单测的纯
//! 字符串/路径判定**。把它放在不链 Tauri 的 ridge-core，是因为宿主 crate 的测试
//! 二进制链接了 webview2，裸 `cargo test` 启动即崩（DLL 加载期 0xc0000139），
//! 纯逻辑若留在宿主里就被一并困住跑不了——抽到这里即可正常 `cargo test`。

use std::path::Path;

/// 是否带常见图片扩展名（不含 svg —— CLI 多把 svg 当文本/矢量而非图片附件）。
pub fn is_image_ext(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// 从一组文件路径里挑出第一个真实存在的图片文件（宿主从 CF_HDROP 读到的文件列表）。
pub fn pick_image_file(files: Vec<String>) -> Option<String> {
    files
        .into_iter()
        .find(|f| is_image_ext(f) && Path::new(f).is_file())
}

/// 规整宿主从 CF_HDROP 读到 / 准备写入的通用文件列表：去首尾空白、丢空串、保序去重。
/// 用于「系统资源管理器 ↔ 文件树」双向文件剪贴板互通的纯逻辑部分。
pub fn sanitize_file_list(files: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    files
        .into_iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .filter(|f| seen.insert(f.clone()))
        .collect()
}

/// 把「复制为路径 / Copy as path」得到的文本（可能带引号）解析成一个真实存在的图片
/// 文件绝对路径。仅当文本是单一、带图片扩展名、且文件存在的路径时返回 `Some`（宿主据此
/// 粘**裸**路径，CLI 才会识别为图片）；否则 `None` → 走普通文本粘贴，绝不误伤普通文本。
pub fn resolve_pasted_image_path(text: &str) -> Option<String> {
    let trimmed = text.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
        return None;
    }
    if is_image_ext(trimmed) && Path::new(trimmed).is_file() {
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
        let dir = std::env::temp_dir().join("ridge-core-clip-rtest");
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("shot.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let p = img.to_string_lossy().to_string();

        assert_eq!(resolve_pasted_image_path(&format!("\"{p}\"")), Some(p.clone()));
        assert_eq!(resolve_pasted_image_path(&p), Some(p.clone()));
        assert_eq!(resolve_pasted_image_path(&format!("  {p}  ")), Some(p.clone()));

        let _ = std::fs::remove_file(&img);
    }

    #[test]
    fn resolve_rejects_non_image_and_missing() {
        // 普通文本不误伤
        assert_eq!(resolve_pasted_image_path("hello world"), None);
        // 图片扩展名但文件不存在
        assert_eq!(resolve_pasted_image_path("C:/nope/x.png"), None);
        // 多行（粘贴的整段文本）不当成路径
        assert_eq!(resolve_pasted_image_path("a.png\nb"), None);
        // 真实存在但非图片扩展名
        let dir = std::env::temp_dir().join("ridge-core-clip-rtest2");
        std::fs::create_dir_all(&dir).unwrap();
        let txt = dir.join("note.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert_eq!(resolve_pasted_image_path(&txt.to_string_lossy()), None);
        let _ = std::fs::remove_file(&txt);
    }

    #[test]
    fn sanitize_file_list_trims_drops_empty_and_dedupes() {
        let input = vec![
            "  C:/a.txt  ".to_string(),
            "".to_string(),
            "   ".to_string(),
            "C:/a.txt".to_string(), // 去重（与第一条 trim 后相同）
            "C:/b.txt".to_string(),
        ];
        assert_eq!(
            sanitize_file_list(input),
            vec!["C:/a.txt".to_string(), "C:/b.txt".to_string()]
        );
        assert!(sanitize_file_list(vec![]).is_empty());
        assert!(sanitize_file_list(vec!["  ".to_string()]).is_empty());
    }

    #[test]
    fn pick_image_file_finds_existing_image() {
        // 「复制图片文件」：CF_HDROP 读到的文件列表里挑出图片文件。
        let dir = std::env::temp_dir().join("ridge-core-clip-pick");
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
