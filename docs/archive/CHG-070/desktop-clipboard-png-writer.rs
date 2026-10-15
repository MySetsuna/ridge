fn save_png_bytes_to_temp(png: &[u8]) -> Result<String, String> {
    let dir = temp_image_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create temp dir failed: {e}"))?;
    let path = dir.join(format!("clip-{}.png", Uuid::new_v4()));
    std::fs::write(&path, png).map_err(|e| format!("write png failed: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

