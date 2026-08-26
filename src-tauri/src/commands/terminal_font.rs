use ridge_core::terminal_font::{self, TerminalFontChunk, TerminalFontResponse};

#[tauri::command]
pub fn read_terminal_font_face_chunk(
    content_hash: String,
    offset: usize,
    length: usize,
) -> Result<TerminalFontChunk, String> {
    terminal_font::read_terminal_font_face_chunk(content_hash, offset, length)
}

#[tauri::command]
pub async fn load_terminal_font_faces(
    families: Vec<String>,
    known_hashes: Option<Vec<String>>,
) -> Result<TerminalFontResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        terminal_font::load_terminal_font_faces(families, known_hashes)
    })
    .await
    .map_err(|error| format!("FONT_DATA_IO: font resolver task failed: {error}"))?
}
