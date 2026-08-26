//! Selected-font shaping and rasterization for the WebGPU glyph atlas.
//!
//! The host loads the user's installed font files before WebGPU starts and
//! passes their SFNT bytes through `installFontData`. cosmic-text/Swash then
//! keep atlas misses synchronous without creating a browser 2D context.

#![cfg(feature = "webgpu")]

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cosmic_text::fontdb;
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
    Wrap,
};

use super::glyph_atlas::GlyphKey;
use crate::term::wcwidth::is_emoji_presentation;

const MAX_FONT_FACES: usize = 32;
const MAX_FONT_BYTES: usize = 96 * 1024 * 1024;
const MAX_SINGLE_FONT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct FontRegistry {
    faces: Vec<Arc<Vec<u8>>>,
    hashes: Vec<u64>,
    total_bytes: usize,
}

thread_local! {
    static FONT_REGISTRY: RefCell<FontRegistry> = RefCell::new(FontRegistry::default());
}

/// Validate and retain one SFNT/TTC payload. Returns true only for new data.
pub fn register_font_data(data: Vec<u8>) -> Result<bool, String> {
    if data.is_empty() || data.len() > MAX_SINGLE_FONT_BYTES {
        return Err(format!(
            "FONT_DATA_INVALID: font payload size {} is outside 1..={MAX_SINGLE_FONT_BYTES}",
            data.len()
        ));
    }
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();
    FONT_REGISTRY.with(|cell| {
        let mut registry = cell.borrow_mut();
        if registry.hashes.contains(&hash) {
            return Ok(false);
        }
        if registry.faces.len() >= MAX_FONT_FACES
            || registry.total_bytes.saturating_add(data.len()) > MAX_FONT_BYTES
        {
            return Err("FONT_DATA_LIMIT: terminal font registry is full".to_string());
        }
        let data = Arc::new(data);
        let mut probe = fontdb::Database::new();
        if probe
            .load_font_source(fontdb::Source::Binary(data.clone()))
            .is_empty()
        {
            return Err("FONT_DATA_INVALID: payload contains no supported font face".to_string());
        }
        registry.total_bytes += data.len();
        registry.hashes.push(hash);
        registry.faces.push(data);
        Ok(true)
    })
}

fn registered_fonts() -> Vec<Arc<Vec<u8>>> {
    FONT_REGISTRY.with(|cell| cell.borrow().faces.clone())
}

#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    /// Tightly packed premultiplied RGBA8 pixels for the painted bitmap.
    pub rgba: Vec<u8>,
    pub width: u16,
    pub height: u16,
    /// Horizontal advance in CSS pixels.
    pub advance: f32,
    /// Device-pixel offset from cell top to the packed bitmap top.
    pub ascent_offset: f32,
    /// Device-pixel offset from the logical cell origin to the bitmap left.
    pub left_offset: f32,
    pub is_color: bool,
    pub is_box_drawing: bool,
}

struct PendingImage {
    x: i32,
    y: i32,
    content: SwashContent,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[derive(Clone, PartialEq)]
struct BoxConnectorReferenceKey {
    font_stack: String,
    font_size_bits: u32,
    dpr_bits: u32,
    style_flags: u8,
}

struct BoxConnectorReferences {
    key: BoxConnectorReferenceKey,
    horizontal: RasterizedGlyph,
    vertical: RasterizedGlyph,
}

pub struct GlyphRasterizer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    loaded_sources: usize,
    resolved_stack: String,
    resolved_family: Option<String>,
    resolved_emoji_family: Option<String>,
    box_connector_references: Option<BoxConnectorReferences>,
    slot_w: u16,
    slot_h: u16,
}

impl GlyphRasterizer {
    pub fn new(slot_w: u16, slot_h: u16) -> Result<Self, String> {
        let mut rasterizer = Self {
            font_system: FontSystem::new_with_locale_and_db(
                "en-US".to_string(),
                fontdb::Database::new(),
            ),
            swash_cache: SwashCache::new(),
            loaded_sources: 0,
            resolved_stack: String::new(),
            resolved_family: None,
            resolved_emoji_family: None,
            box_connector_references: None,
            slot_w: slot_w.max(1),
            slot_h: slot_h.max(1),
        };
        rasterizer.sync_registered_fonts()?;
        Ok(rasterizer)
    }

    /// Pull newly registered host fonts into this live rasterizer.
    pub fn sync_registered_fonts(&mut self) -> Result<bool, String> {
        let fonts = registered_fonts();
        if fonts.is_empty() {
            return Err(
                "FONT_DATA_MISSING: install selected system fonts before WebGPU initialization"
                    .to_string(),
            );
        }
        let mut changed = false;
        for data in fonts.iter().skip(self.loaded_sources) {
            if self
                .font_system
                .db_mut()
                .load_font_source(fontdb::Source::Binary(data.clone()))
                .is_empty()
            {
                return Err("FONT_DATA_INVALID: registered font could not be loaded".to_string());
            }
            changed = true;
        }
        self.loaded_sources = fonts.len();
        if changed {
            self.swash_cache = SwashCache::new();
            self.resolved_stack.clear();
            self.resolved_family = None;
            self.resolved_emoji_family = None;
            self.box_connector_references = None;
        }
        Ok(changed)
    }

    pub fn rasterize(
        &mut self,
        font_family: &str,
        font_size_px: f32,
        dpr: f32,
        style_flags: u8,
        glyph_text: &str,
    ) -> Result<RasterizedGlyph, String> {
        let dpr = valid_dpr(dpr);
        let device_size = (font_size_px.max(1.0) * dpr).max(1.0);
        let line_height = (device_size * 1.2).max(1.0);
        let wants_color_emoji = is_emoji_presentation(glyph_text);
        let box_char = box_drawing_char(glyph_text);
        let is_box_drawing = box_char.is_some();
        let family = if wants_color_emoji {
            self.resolve_emoji_family(font_family)?
        } else {
            self.resolve_family(font_family)?
        };
        let attrs = attrs_for(&family, if wants_color_emoji { 0 } else { style_flags });
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(device_size, line_height),
        );
        buffer.set_size(Some(self.slot_w as f32), Some(self.slot_h as f32));
        buffer.set_wrap(Wrap::None);
        buffer.set_text(glyph_text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut advance_dev = 0.0_f32;
        let mut glyphs = Vec::new();
        for run in buffer.layout_runs() {
            advance_dev = advance_dev.max(run.line_w);
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                glyphs.push((physical.x, physical.y, physical.cache_key));
            }
        }

        let mut images = Vec::with_capacity(glyphs.len());
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let mut is_color = false;
        for (origin_x, origin_y, key) in glyphs {
            let Some(image) = self
                .swash_cache
                .get_image_uncached(&mut self.font_system, key)
            else {
                continue;
            };
            let x = origin_x + image.placement.left;
            let y = origin_y - image.placement.top;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x.saturating_add(image.placement.width as i32));
            max_y = max_y.max(y.saturating_add(image.placement.height as i32));
            is_color |= image.content == SwashContent::Color;
            images.push(PendingImage {
                x,
                y,
                content: image.content,
                width: image.placement.width,
                height: image.placement.height,
                data: image.data,
            });
        }

        let advance = advance_dev / dpr;
        if images.is_empty() {
            if wants_color_emoji {
                return Err(format!(
                    "FONT_COLOR_EMOJI_MISSING: Host face {family} cannot render {glyph_text:?}"
                ));
            }
            let width = advance_dev.ceil().clamp(1.0, self.slot_w as f32) as u16;
            return Ok(RasterizedGlyph {
                rgba: vec![0; width as usize * 4],
                width,
                height: 1,
                advance,
                ascent_offset: 0.0,
                left_offset: 0.0,
                is_color: false,
                is_box_drawing,
            });
        }
        if wants_color_emoji && !is_color {
            return Err(format!(
                "FONT_COLOR_EMOJI_MISSING: Host face {family} returned a monochrome glyph for {glyph_text:?}"
            ));
        }

        // Retain native side bearings and overhang. Terminal width controls
        // layout only; the quad may paint outside its reserved cells.
        let crop_left = min_x.min(0);
        let mut crop_top = min_y;
        let crop_right = max_x.max(advance_dev.ceil() as i32);
        let width = crop_right
            .saturating_sub(crop_left)
            .clamp(1, self.slot_w as i32) as u16;
        let mut height = max_y.saturating_sub(crop_top).clamp(1, self.slot_h as i32) as u16;
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for image in &images {
            composite_image(
                &mut rgba,
                width as usize,
                height as usize,
                image,
                crop_left,
                crop_top,
            );
        }
        stabilize_box_connector_edges(
            &mut rgba,
            width as usize,
            &mut height,
            &mut crop_top,
            crop_left,
            advance_dev,
            line_height,
            self.slot_h as usize,
            glyph_text,
        );

        let mut result = RasterizedGlyph {
            rgba,
            width,
            height,
            advance,
            ascent_offset: crop_top as f32,
            left_offset: crop_left as f32,
            is_color,
            is_box_drawing,
        };
        if box_char.is_some_and(is_rounded_box_corner) {
            let (horizontal, vertical) =
                self.box_connector_references(font_family, font_size_px, dpr, style_flags)?;
            align_rounded_corner_connectors(
                &mut result,
                box_char.unwrap(),
                dpr,
                &horizontal,
                &vertical,
            );
        }
        Ok(result)
    }

    pub fn slot_dimensions(&self) -> (u16, u16) {
        (self.slot_w, self.slot_h)
    }

    pub fn measure(&mut self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        let family = self.resolve_family(font_family)?;
        let metrics = Metrics::new(font_size_px.max(1.0), (font_size_px * 1.2).max(1.0));
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(None, Some(metrics.line_height));
        buffer.set_wrap(Wrap::None);
        buffer.set_text(
            "M",
            &Attrs::new().family(Family::Name(&family)),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        let run = buffer
            .layout_runs()
            .next()
            .ok_or_else(|| format!("FONT_DATA_MISSING: no face matched {family}"))?;
        Ok((
            run.line_w.round().max(1.0),
            run.line_height.round().max(1.0),
        ))
    }

    fn box_connector_references(
        &mut self,
        font_stack: &str,
        font_size_px: f32,
        dpr: f32,
        style_flags: u8,
    ) -> Result<(RasterizedGlyph, RasterizedGlyph), String> {
        let key = BoxConnectorReferenceKey {
            font_stack: font_stack.to_string(),
            font_size_bits: font_size_px.max(1.0).to_bits(),
            dpr_bits: dpr.to_bits(),
            style_flags,
        };
        if let Some(cached) = self
            .box_connector_references
            .as_ref()
            .filter(|cached| cached.key == key)
        {
            return Ok((cached.horizontal.clone(), cached.vertical.clone()));
        }

        // Straight references do not enter this rounded-corner branch.
        let horizontal = self.rasterize(font_stack, font_size_px, dpr, style_flags, "─")?;
        let vertical = self.rasterize(font_stack, font_size_px, dpr, style_flags, "│")?;
        self.box_connector_references = Some(BoxConnectorReferences {
            key,
            horizontal: horizontal.clone(),
            vertical: vertical.clone(),
        });
        Ok((horizontal, vertical))
    }

    fn resolve_family(&mut self, stack: &str) -> Result<String, String> {
        self.prepare_stack(stack);
        if let Some(family) = self.resolved_family.as_ref() {
            return Ok(family.clone());
        }
        let family = first_available_family(stack, |candidate| self.query_family(candidate))
            .ok_or_else(|| format!("FONT_DATA_MISSING: no loaded face matched {stack}"))?;
        self.resolved_family = Some(family.clone());
        Ok(family)
    }

    fn resolve_emoji_family(&mut self, stack: &str) -> Result<String, String> {
        self.prepare_stack(stack);
        if let Some(family) = self.resolved_emoji_family.as_ref() {
            return Ok(family.clone());
        }
        let family = emoji_family_candidates(stack)
            .find_map(|candidate| self.query_family(candidate))
            .ok_or_else(|| {
                format!("FONT_COLOR_EMOJI_MISSING: no Host color emoji face matched {stack}")
            })?;
        self.resolved_emoji_family = Some(family.clone());
        Ok(family)
    }

    fn prepare_stack(&mut self, stack: &str) {
        if self.resolved_stack == stack {
            return;
        }
        self.resolved_stack.clear();
        self.resolved_stack.push_str(stack);
        self.resolved_family = None;
        self.resolved_emoji_family = None;
    }

    fn query_family(&self, candidate: &str) -> Option<String> {
        let query_family = match candidate.to_ascii_lowercase().as_str() {
            "monospace" | "ui-monospace" => Family::Monospace,
            "serif" => Family::Serif,
            "sans-serif" | "system-ui" => Family::SansSerif,
            _ => Family::Name(candidate),
        };
        let query = fontdb::Query {
            families: std::slice::from_ref(&query_family),
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let id = self.font_system.db().query(&query)?;
        self.font_system
            .db()
            .face(id)
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone())
            .or_else(|| Some(candidate.to_string()))
    }
}

fn valid_dpr(dpr: f32) -> f32 {
    if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    }
}

fn first_available_family(
    stack: &str,
    mut resolve: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    stack
        .split(',')
        .map(|part| part.trim().trim_matches(['\'', '"']).trim())
        .filter(|part| !part.is_empty())
        .find_map(|part| resolve(part))
}

fn emoji_family_candidates(stack: &str) -> impl Iterator<Item = &str> {
    stack
        .split(',')
        .map(|part| part.trim().trim_matches(['\'', '"']).trim())
        .filter(|part| {
            let family = part.to_ascii_lowercase();
            family == "segoe ui emoji" || family.contains("color emoji")
        })
}

fn box_drawing_char(glyph_text: &str) -> Option<char> {
    let mut chars = glyph_text.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && ('\u{2500}'..='\u{257f}').contains(&ch)).then_some(ch)
}

fn is_rounded_box_corner(ch: char) -> bool {
    matches!(ch, '╭' | '╮' | '╯' | '╰')
}

const CONNECT_LEFT: u8 = 1;
const CONNECT_RIGHT: u8 = 2;
const CONNECT_TOP: u8 = 4;
const CONNECT_BOTTOM: u8 = 8;

fn glyph_logical_x_bounds(glyph: &RasterizedGlyph, dpr: f32) -> Option<(usize, usize)> {
    let width = glyph.width as usize;
    if width == 0 {
        return None;
    }
    let left = ((-glyph.left_offset.round() as i32).max(0) as usize).min(width - 1);
    let right = (left + (glyph.advance * dpr).ceil().max(1.0) as usize - 1).min(width - 1);
    Some((left, right))
}

fn align_rounded_corner_connectors(
    target: &mut RasterizedGlyph,
    ch: char,
    dpr: f32,
    horizontal: &RasterizedGlyph,
    vertical: &RasterizedGlyph,
) {
    let sides = box_connector_sides(ch);
    let width = target.width as usize;
    let height = target.height as usize;
    let Some((logical_left, logical_right)) = glyph_logical_x_bounds(target, dpr) else {
        return;
    };
    if height == 0 || target.rgba.len() < width * height * 4 {
        return;
    }

    if sides & (CONNECT_LEFT | CONNECT_RIGHT) != 0 {
        let Some((reference_left, reference_right)) = glyph_logical_x_bounds(horizontal, dpr)
        else {
            return;
        };
        let reference_height = horizontal.height as usize;
        let profile = horizontal_connector_pixels(
            &horizontal.rgba,
            horizontal.width as usize,
            reference_height,
            reference_left,
            reference_right,
            reference_height.saturating_sub(1) / 2,
            sides & CONNECT_RIGHT != 0,
        )
        .map(|(_, pixels)| pixels)
        .unwrap_or_default();
        if !profile.is_empty() {
            let target_x = if sides & CONNECT_LEFT != 0 {
                logical_left
            } else {
                logical_right
            };
            for y in 0..height {
                target.rgba[(y * width + target_x) * 4..(y * width + target_x + 1) * 4].fill(0);
            }
            let reference_top = horizontal.ascent_offset.round() as i32;
            let target_top = target.ascent_offset.round() as i32;
            for (reference_y, pixel) in profile {
                let target_y = reference_y as i32 + reference_top - target_top;
                if (0..height as i32).contains(&target_y) {
                    let offset = (target_y as usize * width + target_x) * 4;
                    target.rgba[offset..offset + 4].copy_from_slice(&pixel);
                }
            }
        }
    }

    if sides & (CONNECT_TOP | CONNECT_BOTTOM) != 0 {
        let Some((reference_left, reference_right)) = glyph_logical_x_bounds(vertical, dpr) else {
            return;
        };
        let profile = vertical_connector_pixels(
            &vertical.rgba,
            vertical.width as usize,
            vertical.height as usize,
            (reference_left + reference_right) / 2,
            sides & CONNECT_BOTTOM != 0,
        );
        if !profile.is_empty() {
            let target_y = if sides & CONNECT_TOP != 0 {
                0
            } else {
                height - 1
            };
            for x in logical_left..=logical_right {
                target.rgba[(target_y * width + x) * 4..(target_y * width + x + 1) * 4].fill(0);
            }
            let reference_left_offset = vertical.left_offset.round() as i32;
            let target_left_offset = target.left_offset.round() as i32;
            for (reference_x, pixel) in profile {
                let target_x = reference_x as i32 + reference_left_offset - target_left_offset;
                if (logical_left as i32..=logical_right as i32).contains(&target_x) {
                    let offset = (target_y * width + target_x as usize) * 4;
                    target.rgba[offset..offset + 4].copy_from_slice(&pixel);
                }
            }
        }
    }
}

fn box_connector_sides(ch: char) -> u8 {
    match ch {
        '\u{2500}' | '\u{2501}' | '\u{2504}' | '\u{2505}' | '\u{2508}' | '\u{2509}'
        | '\u{254c}' | '\u{254d}' | '\u{2550}' | '\u{257c}' | '\u{257e}' => {
            CONNECT_LEFT | CONNECT_RIGHT
        }
        '\u{2502}' | '\u{2503}' | '\u{2506}' | '\u{2507}' | '\u{250a}' | '\u{250b}'
        | '\u{254e}' | '\u{254f}' | '\u{2551}' | '\u{257d}' | '\u{257f}' => {
            CONNECT_TOP | CONNECT_BOTTOM
        }
        '\u{250c}'..='\u{250f}' | '\u{2552}'..='\u{2557}' | '\u{256d}' => {
            CONNECT_RIGHT | CONNECT_BOTTOM
        }
        '\u{2510}'..='\u{2513}' | '\u{2558}'..='\u{255d}' | '\u{256e}' => {
            CONNECT_LEFT | CONNECT_BOTTOM
        }
        '\u{2514}'..='\u{2517}' | '\u{2570}' => CONNECT_RIGHT | CONNECT_TOP,
        '\u{2518}'..='\u{251b}' | '\u{256f}' => CONNECT_LEFT | CONNECT_TOP,
        '\u{251c}'..='\u{2523}' | '\u{255e}'..='\u{2560}' => {
            CONNECT_TOP | CONNECT_BOTTOM | CONNECT_RIGHT
        }
        '\u{2524}'..='\u{252b}' | '\u{2561}'..='\u{2563}' => {
            CONNECT_TOP | CONNECT_BOTTOM | CONNECT_LEFT
        }
        '\u{252c}'..='\u{2533}' | '\u{2564}'..='\u{2566}' => {
            CONNECT_LEFT | CONNECT_RIGHT | CONNECT_BOTTOM
        }
        '\u{2534}'..='\u{253b}' | '\u{2567}'..='\u{2569}' => {
            CONNECT_LEFT | CONNECT_RIGHT | CONNECT_TOP
        }
        '\u{253c}'..='\u{254b}' | '\u{256a}'..='\u{256c}' => {
            CONNECT_LEFT | CONNECT_RIGHT | CONNECT_TOP | CONNECT_BOTTOM
        }
        '\u{2574}' | '\u{2578}' => CONNECT_LEFT,
        '\u{2575}' | '\u{2579}' => CONNECT_TOP,
        '\u{2576}' | '\u{257a}' => CONNECT_RIGHT,
        '\u{2577}' | '\u{257b}' => CONNECT_BOTTOM,
        _ => 0,
    }
}

fn attrs_for(family: &str, style_flags: u8) -> Attrs<'_> {
    let weight = if style_flags & GlyphKey::STYLE_BOLD != 0 {
        Weight::BOLD
    } else {
        Weight::NORMAL
    };
    let style = if style_flags & GlyphKey::STYLE_ITALIC != 0 {
        Style::Italic
    } else {
        Style::Normal
    };
    Attrs::new()
        .family(Family::Name(family))
        .weight(weight)
        .style(style)
}

fn composite_image(
    dst: &mut [u8],
    width: usize,
    height: usize,
    image: &PendingImage,
    crop_left: i32,
    crop_top: i32,
) {
    for src_y in 0..image.height as usize {
        for src_x in 0..image.width as usize {
            let x = image.x - crop_left + src_x as i32;
            let y = image.y - crop_top + src_y as i32;
            if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
                continue;
            }
            let src = match image.content {
                SwashContent::Mask => {
                    let alpha = image.data[src_y * image.width as usize + src_x];
                    [alpha, alpha, alpha, alpha]
                }
                SwashContent::Color => {
                    let offset = (src_y * image.width as usize + src_x) * 4;
                    let alpha = image.data[offset + 3];
                    [
                        premultiply(image.data[offset], alpha),
                        premultiply(image.data[offset + 1], alpha),
                        premultiply(image.data[offset + 2], alpha),
                        alpha,
                    ]
                }
                SwashContent::SubpixelMask => continue,
            };
            let offset = (y as usize * width + x as usize) * 4;
            blend_premult(&mut dst[offset..offset + 4], src);
        }
    }
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((channel as u32 * alpha as u32 + 127) / 255) as u8
}

fn blend_premult(dst: &mut [u8], src: [u8; 4]) {
    let inv = 255_u32 - src[3] as u32;
    for channel in 0..3 {
        dst[channel] =
            (src[channel] as u32 + (dst[channel] as u32 * inv + 127) / 255).min(255) as u8;
    }
    dst[3] = (src[3] as u32 + (dst[3] as u32 * inv + 127) / 255).min(255) as u8;
}

/// Keep Swash's outline, but make its already-painted box connectors meet the
/// neighboring cell. Curves and all non-connector anti-aliasing remain intact.
#[allow(clippy::too_many_arguments)]
fn stabilize_box_connector_edges(
    rgba: &mut Vec<u8>,
    width: usize,
    height: &mut u16,
    ascent_offset: &mut i32,
    left_offset: i32,
    advance_dev: f32,
    line_height: f32,
    slot_h: usize,
    glyph_text: &str,
) {
    let Some(ch) = box_drawing_char(glyph_text) else {
        return;
    };
    let sides = box_connector_sides(ch);
    if width == 0 || sides == 0 {
        return;
    }

    let glyph_height = *height as usize;
    let logical_left = ((-left_offset).max(0) as usize).min(width - 1);
    let logical_right = (logical_left + advance_dev.ceil().max(1.0) as usize - 1).min(width - 1);

    let row_bytes = width * 4;
    let center_x = (logical_left + logical_right) / 2;
    let top_extension = if sides & CONNECT_TOP != 0 {
        (*ascent_offset).max(0) as usize
    } else {
        0
    }
    .min(slot_h.saturating_sub(glyph_height));
    let top_connector = vertical_connector_pixels(rgba, width, glyph_height, center_x, false);
    let bottom_connector = vertical_connector_pixels(rgba, width, glyph_height, center_x, true);
    let shared_vertical_connector =
        if sides & (CONNECT_TOP | CONNECT_BOTTOM) == (CONNECT_TOP | CONNECT_BOTTOM) {
            if connector_profile_strength(&bottom_connector)
                > connector_profile_strength(&top_connector)
            {
                &bottom_connector
            } else {
                &top_connector
            }
        } else if sides & CONNECT_TOP != 0 {
            &top_connector
        } else {
            &bottom_connector
        };
    let mut glyph_height = glyph_height;
    if top_extension != 0 && !shared_vertical_connector.is_empty() {
        rgba.resize((glyph_height + top_extension) * row_bytes, 0);
        rgba.copy_within(0..glyph_height * row_bytes, top_extension * row_bytes);
        rgba[..top_extension * row_bytes].fill(0);
        for (x, pixel) in shared_vertical_connector {
            for y in 0..top_extension {
                rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(pixel);
            }
        }
        *ascent_offset -= top_extension as i32;
        *height += top_extension as u16;
        glyph_height += top_extension;
    }

    let target_height = line_height.round().max(1.0) as i32;
    let bottom_extension = if sides & CONNECT_BOTTOM != 0 {
        target_height
            .saturating_sub(*ascent_offset + *height as i32)
            .max(0) as usize
    } else {
        0
    }
    .min(slot_h.saturating_sub(glyph_height));
    if bottom_extension != 0 && !shared_vertical_connector.is_empty() {
        rgba.resize((glyph_height + bottom_extension) * row_bytes, 0);
        for (x, pixel) in shared_vertical_connector {
            for y in glyph_height..glyph_height + bottom_extension {
                rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(pixel);
            }
        }
        *height += bottom_extension as u16;
        glyph_height += bottom_extension;
    }

    let center_y = ((line_height * 0.5).round() as i32 - *ascent_offset)
        .clamp(0, glyph_height.saturating_sub(1) as i32) as usize;
    if sides & CONNECT_LEFT != 0 {
        extend_horizontal_connector(
            rgba,
            width,
            glyph_height,
            logical_left,
            logical_right,
            center_y,
            false,
        );
    }
    if sides & CONNECT_RIGHT != 0 {
        extend_horizontal_connector(
            rgba,
            width,
            glyph_height,
            logical_left,
            logical_right,
            center_y,
            true,
        );
    }
}

fn connector_profile_strength(profile: &[(usize, [u8; 4])]) -> (u8, u32) {
    profile.iter().fold((0, 0), |(peak, total), (_, pixel)| {
        (peak.max(pixel[3]), total + pixel[3] as u32)
    })
}

fn vertical_connector_pixels(
    rgba: &[u8],
    width: usize,
    height: usize,
    center_x: usize,
    from_bottom: bool,
) -> Vec<(usize, [u8; 4])> {
    for min_alpha in [192, 128, 1] {
        for step in 0..height {
            let y = if from_bottom { height - step - 1 } else { step };
            if let Some(anchor) = (0..width)
                .filter(|&x| rgba[(y * width + x) * 4 + 3] >= min_alpha)
                .min_by_key(|&x| x.abs_diff(center_x))
            {
                let mut start = anchor;
                while start > 0 && rgba[(y * width + start - 1) * 4 + 3] != 0 {
                    start -= 1;
                }
                let mut end = anchor + 1;
                while end < width && rgba[(y * width + end) * 4 + 3] != 0 {
                    end += 1;
                }
                return (start..end)
                    .map(|x| {
                        let offset = (y * width + x) * 4;
                        (x, rgba[offset..offset + 4].try_into().unwrap())
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

fn extend_horizontal_connector(
    rgba: &mut [u8],
    width: usize,
    height: usize,
    logical_left: usize,
    logical_right: usize,
    center_y: usize,
    from_right: bool,
) {
    let Some((source_x, pixels)) = horizontal_connector_pixels(
        rgba,
        width,
        height,
        logical_left,
        logical_right,
        center_y,
        from_right,
    ) else {
        return;
    };
    let (start, end) = if from_right {
        (source_x, logical_right)
    } else {
        (logical_left, source_x)
    };
    for x in start..=end {
        for (y, pixel) in &pixels {
            let offset = (y * width + x) * 4;
            rgba[offset..offset + 4].copy_from_slice(pixel);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn horizontal_connector_pixels(
    rgba: &[u8],
    width: usize,
    height: usize,
    logical_left: usize,
    logical_right: usize,
    center_y: usize,
    from_right: bool,
) -> Option<(usize, Vec<(usize, [u8; 4])>)> {
    for min_alpha in [192, 128, 1] {
        for step in 0..=logical_right.saturating_sub(logical_left) {
            let x = if from_right {
                logical_right - step
            } else {
                logical_left + step
            };
            if let Some(anchor) = (0..height)
                .filter(|&y| rgba[(y * width + x) * 4 + 3] >= min_alpha)
                .min_by_key(|&y| y.abs_diff(center_y))
            {
                let mut start = anchor;
                while start > 0 && rgba[((start - 1) * width + x) * 4 + 3] != 0 {
                    start -= 1;
                }
                let mut end = anchor + 1;
                while end < height && rgba[(end * width + x) * 4 + 3] != 0 {
                    end += 1;
                }
                let pixels = (start..end)
                    .map(|y| {
                        let offset = (y * width + x) * 4;
                        (y, rgba[offset..offset + 4].try_into().unwrap())
                    })
                    .collect();
                return Some((x, pixels));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_stack_uses_first_available_family() {
        assert_eq!(
            first_available_family("'JetBrains Mono', ui-monospace, Consolas", |family| {
                (family == "ui-monospace").then(|| "Consolas".to_string())
            })
            .as_deref(),
            Some("Consolas")
        );
    }

    #[test]
    fn emoji_candidates_exclude_text_and_symbol_faces() {
        assert_eq!(
            emoji_family_candidates(
                "Consolas,'Segoe UI Symbol',emoji,'Segoe UI Emoji','Noto Color Emoji'"
            )
            .collect::<Vec<_>>(),
            vec!["Segoe UI Emoji", "Noto Color Emoji"]
        );
    }

    #[test]
    fn premultiplied_blend_preserves_coverage() {
        let mut dst = [0, 0, 0, 0];
        blend_premult(&mut dst, [64, 32, 16, 128]);
        assert_eq!(dst, [64, 32, 16, 128]);
        blend_premult(&mut dst, [0, 64, 0, 128]);
        assert_eq!(dst, [32, 80, 8, 192]);
    }

    #[test]
    fn horizontal_box_glyph_copies_native_coverage_to_both_edges() {
        let mut rgba = vec![0; 7 * 3 * 4];
        for x in 2..=4 {
            rgba[(7 + x) * 4..(7 + x + 1) * 4].copy_from_slice(&[48, 48, 48, 48]);
            rgba[(14 + x) * 4..(14 + x + 1) * 4].copy_from_slice(&[160, 160, 160, 160]);
        }
        rgba[7 * 4..8 * 4].copy_from_slice(&[12, 12, 12, 12]);
        rgba[13 * 4..14 * 4].copy_from_slice(&[20, 20, 20, 20]);
        rgba[14 * 4..15 * 4].copy_from_slice(&[24, 24, 24, 24]);
        rgba[20 * 4..21 * 4].copy_from_slice(&[28, 28, 28, 28]);
        let mut height = 3;
        let mut top = 0;
        stabilize_box_connector_edges(&mut rgba, 7, &mut height, &mut top, 0, 7.0, 3.0, 6, "─");
        for x in 0..7 {
            assert_eq!(&rgba[(7 + x) * 4..(7 + x + 1) * 4], &[48; 4]);
            assert_eq!(&rgba[(14 + x) * 4..(14 + x + 1) * 4], &[160; 4]);
        }

        let mut ordinary = rgba.clone();
        ordinary[3] = 96;
        stabilize_box_connector_edges(&mut ordinary, 7, &mut height, &mut top, 0, 7.0, 3.0, 6, "A");
        assert_eq!(ordinary[3], 96);
    }

    #[test]
    fn rounded_corner_extends_only_its_font_connector() {
        let mut rgba = vec![0; 5 * 4 * 4];
        rgba[(5 + 1) * 4..(5 + 2) * 4].copy_from_slice(&[32, 32, 32, 32]);
        for x in 2..=3 {
            rgba[(2 * 5 + x) * 4..(2 * 5 + x + 1) * 4].copy_from_slice(&[128, 128, 128, 128]);
        }
        rgba[(2 * 5 + 4) * 4..(2 * 5 + 5) * 4].copy_from_slice(&[16, 16, 16, 16]);
        rgba[(3 * 5 + 2) * 4..(3 * 5 + 3) * 4].copy_from_slice(&[160, 160, 160, 160]);
        let curve_pixel = rgba[(5 + 1) * 4..(5 + 2) * 4].to_vec();
        let mut height = 4;
        let mut top = 0;
        stabilize_box_connector_edges(&mut rgba, 5, &mut height, &mut top, 0, 5.0, 6.0, 8, "╭");
        assert_eq!(top, 0);
        assert_eq!(height, 6);
        assert_eq!(&rgba[(5 + 1) * 4..(5 + 2) * 4], curve_pixel);
        assert_eq!(&rgba[(2 * 5 + 4) * 4..(2 * 5 + 5) * 4], &[128; 4]);
        assert_eq!(&rgba[(4 * 5 + 2) * 4..(4 * 5 + 3) * 4], &[160; 4]);
        assert_eq!(&rgba[(5 * 5 + 2) * 4..(5 * 5 + 3) * 4], &[160; 4]);
        assert_eq!(&rgba[(2 * 5) * 4..(2 * 5 + 1) * 4], &[0; 4]);
    }

    #[test]
    fn vertical_connector_reuses_one_profile_at_both_cell_edges() {
        let mut rgba = vec![0; 3 * 2 * 4];
        rgba[(1 * 4)..(2 * 4)].copy_from_slice(&[200; 4]);
        rgba[(3 + 1) * 4..(3 + 2) * 4].copy_from_slice(&[240; 4]);
        let mut height = 2;
        let mut top = 1;

        stabilize_box_connector_edges(&mut rgba, 3, &mut height, &mut top, 0, 3.0, 5.0, 6, "│");

        assert_eq!(top, 0);
        assert_eq!(height, 5);
        assert_eq!(&rgba[(1 * 4)..(1 * 4 + 4)], &[240; 4]);
        assert_eq!(&rgba[(3 * 3 + 1) * 4..(3 * 3 + 2) * 4], &[240; 4]);
        assert_eq!(&rgba[(4 * 3 + 1) * 4..(4 * 3 + 2) * 4], &[240; 4]);
    }

    #[test]
    fn rounded_corner_replaces_only_declared_edges_with_reference_profiles() {
        let mut target = RasterizedGlyph {
            rgba: vec![0; 5 * 5 * 4],
            width: 5,
            height: 5,
            advance: 5.0,
            ascent_offset: 0.0,
            left_offset: 0.0,
            is_color: false,
            is_box_drawing: true,
        };
        target.rgba[(1 * 5 + 1) * 4..(1 * 5 + 2) * 4].copy_from_slice(&[33; 4]);
        target.rgba[(1 * 5 + 4) * 4..(1 * 5 + 5) * 4].copy_from_slice(&[44; 4]);
        target.rgba[(2 * 5) * 4..(2 * 5 + 1) * 4].copy_from_slice(&[12; 4]);
        target.rgba[(3 * 5) * 4..(3 * 5 + 1) * 4].copy_from_slice(&[18; 4]);
        target.rgba[(4 * 5 + 2) * 4..(4 * 5 + 3) * 4].copy_from_slice(&[24; 4]);

        let horizontal = RasterizedGlyph {
            rgba: vec![210; 5 * 4],
            width: 5,
            height: 1,
            advance: 5.0,
            ascent_offset: 2.0,
            left_offset: 0.0,
            is_color: false,
            is_box_drawing: true,
        };
        let mut vertical = RasterizedGlyph {
            rgba: vec![0; 5 * 5 * 4],
            width: 5,
            height: 5,
            advance: 5.0,
            ascent_offset: 0.0,
            left_offset: 0.0,
            is_color: false,
            is_box_drawing: true,
        };
        for y in 0..5 {
            vertical.rgba[(y * 5 + 2) * 4..(y * 5 + 3) * 4].copy_from_slice(&[80; 4]);
            vertical.rgba[(y * 5 + 3) * 4..(y * 5 + 4) * 4].copy_from_slice(&[190; 4]);
        }

        align_rounded_corner_connectors(&mut target, '╮', 1.0, &horizontal, &vertical);

        assert_eq!(&target.rgba[(1 * 5 + 1) * 4..(1 * 5 + 2) * 4], &[33; 4]);
        assert_eq!(&target.rgba[(1 * 5 + 4) * 4..(1 * 5 + 5) * 4], &[44; 4]);
        for y in 0..5 {
            let expected = if y == 2 { [210; 4] } else { [0; 4] };
            assert_eq!(&target.rgba[(y * 5) * 4..(y * 5 + 1) * 4], &expected);
        }
        assert_eq!(&target.rgba[(4 * 5 + 2) * 4..(4 * 5 + 3) * 4], &[80; 4]);
        assert_eq!(&target.rgba[(4 * 5 + 3) * 4..(4 * 5 + 4) * 4], &[190; 4]);
    }

    #[test]
    fn connector_topology_covers_common_box_forms() {
        assert_eq!(box_connector_sides('─'), CONNECT_LEFT | CONNECT_RIGHT);
        assert_eq!(box_connector_sides('│'), CONNECT_TOP | CONNECT_BOTTOM);
        assert_eq!(box_connector_sides('╭'), CONNECT_RIGHT | CONNECT_BOTTOM);
        assert_eq!(box_connector_sides('┼'), 15);
        assert_eq!(box_connector_sides('╱'), 0);
    }

    #[test]
    fn color_glyph_composite_retains_non_gray_rgba() {
        let image = PendingImage {
            x: 0,
            y: 0,
            content: SwashContent::Color,
            width: 1,
            height: 1,
            data: vec![255, 32, 0, 128],
        };
        let mut rgba = vec![0; 4];
        composite_image(&mut rgba, 1, 1, &image, 0, 0);
        assert_eq!(rgba, vec![128, 16, 0, 128]);
        assert_ne!(rgba[0], rgba[1]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn consolas_rounded_corner_edges_match_straight_profiles() {
        let path = std::path::Path::new(r"C:\Windows\Fonts\consola.ttf");
        if !path.exists() {
            return;
        }
        register_font_data(std::fs::read(path).unwrap()).unwrap();
        let mut rasterizer = GlyphRasterizer::new(256, 256).unwrap();
        let corner = rasterizer
            .rasterize("'Consolas'", 19.0, 1.0, 0, "╮")
            .unwrap();
        let horizontal = rasterizer
            .rasterize("'Consolas'", 19.0, 1.0, 0, "─")
            .unwrap();
        let vertical = rasterizer
            .rasterize("'Consolas'", 19.0, 1.0, 0, "│")
            .unwrap();

        let column_profile = |glyph: &RasterizedGlyph, x: usize| {
            (0..glyph.height as usize)
                .filter_map(|y| {
                    let offset = (y * glyph.width as usize + x) * 4;
                    let pixel: [u8; 4] = glyph.rgba[offset..offset + 4].try_into().unwrap();
                    (pixel[3] != 0)
                        .then_some((y as i32 + glyph.ascent_offset.round() as i32, pixel))
                })
                .collect::<Vec<_>>()
        };
        let row_profile = |glyph: &RasterizedGlyph, y: usize| {
            (0..glyph.width as usize)
                .filter_map(|x| {
                    let offset = (y * glyph.width as usize + x) * 4;
                    let pixel: [u8; 4] = glyph.rgba[offset..offset + 4].try_into().unwrap();
                    (pixel[3] != 0).then_some((x as i32 + glyph.left_offset.round() as i32, pixel))
                })
                .collect::<Vec<_>>()
        };
        let (corner_left, _) = glyph_logical_x_bounds(&corner, 1.0).unwrap();
        let (_, horizontal_right) = glyph_logical_x_bounds(&horizontal, 1.0).unwrap();

        assert_eq!(
            column_profile(&corner, corner_left),
            column_profile(&horizontal, horizontal_right)
        );
        assert_eq!(
            row_profile(&corner, corner.height as usize - 1),
            row_profile(&vertical, vertical.height as usize - 1)
        );
        assert_eq!(
            rasterizer
                .box_connector_references
                .as_ref()
                .unwrap()
                .key
                .font_size_bits,
            19.0_f32.to_bits()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn consolas_box_drawing_uses_monochrome_swash_bitmaps() {
        let path = std::path::Path::new(r"C:\Windows\Fonts\consola.ttf");
        if !path.exists() {
            return;
        }
        register_font_data(std::fs::read(path).unwrap()).unwrap();
        let mut rasterizer = GlyphRasterizer::new(256, 256).unwrap();

        for (text, sides) in [
            ("─", CONNECT_LEFT | CONNECT_RIGHT),
            ("│", CONNECT_TOP | CONNECT_BOTTOM),
            ("╭", CONNECT_RIGHT | CONNECT_BOTTOM),
        ] {
            let glyph = rasterizer
                .rasterize("'Consolas'", 15.0, 1.0, 0, text)
                .unwrap();
            assert!(
                !glyph.is_color,
                "{text} must remain a monochrome font bitmap"
            );
            assert!(glyph.is_box_drawing);

            let width = glyph.width as usize;
            let height = glyph.height as usize;
            let logical_left = ((-glyph.left_offset.round() as i32).max(0) as usize).min(width - 1);
            let logical_right =
                (logical_left + glyph.advance.ceil().max(1.0) as usize - 1).min(width - 1);
            let alpha_at = |x: usize, y: usize| glyph.rgba[(y * width + x) * 4 + 3];
            if sides & CONNECT_LEFT != 0 {
                assert!((0..height).any(|y| alpha_at(logical_left, y) != 0));
            }
            if sides & CONNECT_RIGHT != 0 {
                assert!((0..height).any(|y| alpha_at(logical_right, y) != 0));
            }
            if sides & CONNECT_TOP != 0 {
                assert!((logical_left..=logical_right).any(|x| alpha_at(x, 0) != 0));
            }
            if sides & CONNECT_BOTTOM != 0 {
                assert!((logical_left..=logical_right).any(|x| alpha_at(x, height - 1) != 0));
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn segoe_ui_emoji_rasterizes_robot_as_color() {
        let path = std::path::Path::new(r"C:\Windows\Fonts\seguiemj.ttf");
        if !path.exists() {
            return;
        }
        register_font_data(std::fs::read(path).unwrap()).unwrap();
        let mut rasterizer = GlyphRasterizer::new(256, 256).unwrap();
        let glyph = rasterizer
            .rasterize("'Segoe UI Emoji'", 16.0, 2.0, 0, "\u{1f916}")
            .unwrap();

        assert!(glyph.is_color);
        assert!(glyph
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[3] != 0 && (pixel[0] != pixel[1] || pixel[1] != pixel[2])));
    }
}
