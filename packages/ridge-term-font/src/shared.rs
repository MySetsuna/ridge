pub(crate) fn valid_dpr(dpr: f32) -> f32 {
    if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    }
}

pub(crate) fn box_drawing_char(glyph_text: &str) -> Option<char> {
    let mut chars = glyph_text.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && ('\u{2500}'..='\u{257f}').contains(&ch)).then_some(ch)
}

pub(crate) fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((channel as u32 * alpha as u32 + 127) / 255) as u8
}

pub(crate) fn is_grid_block_element(glyph_text: &str) -> bool {
    let mut chars = glyph_text.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    matches!(
        ch,
        '\u{2580}'..='\u{2590}' | '\u{2594}' | '\u{2595}' | '\u{2596}'..='\u{259f}'
    )
}

pub(crate) fn normalize_grid_block_coverage(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        if pixel[3] != 0 {
            pixel[0] = pixel[3];
            pixel[1] = pixel[3];
            pixel[2] = pixel[3];
            pixel[3] = 255;
        }
    }
}
