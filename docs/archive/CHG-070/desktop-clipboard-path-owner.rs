pub fn resolve_pasted_image_path(text: String) -> Option<String> {
    ridge_core::clipboard::resolve_pasted_image_path(&text)
}