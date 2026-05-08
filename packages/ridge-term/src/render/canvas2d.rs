//! Canvas2D rendering backend.
//!
//! This is the **correctness oracle**, not the fast path. WebGPU (round 3)
//! will be measured against this — same input → same pixels (within AA
//! tolerance). Don't optimize this aggressively; cleverness here would
//! make the oracle harder to trust.
//!
//! ## DPR + coordinate system
//! On a 2× display:
//!   - `canvas.width = css_w * 2`, `canvas.style.width = css_w + 'px'`
//!   - `ctx.scale(dpr, dpr)` lets us keep drawing in CSS-pixel coordinates
//! After scale, every (x, y) we pass to fillRect/fillText is in CSS pixels
//! and the GPU rasterizes at backing-store resolution. Best of both.
//!
//! ## Glyph baseline
//! Canvas2D `fillText` defaults to `textBaseline = 'alphabetic'`, which
//! draws from the baseline. For terminal cells we want top-aligned text
//! so descenders stay inside the cell. We set `textBaseline = 'top'`
//! once per begin_frame.
//!
//! ## Cell boundary alignment
//! `fillRect(col * cell_w, row * cell_h, cell_w, cell_h)` produces sub-
//! pixel boundaries when cell_w isn't an integer. Browser snaps to the
//! nearest pixel which can leave 1-px gaps between adjacent cells of
//! the same bg. We `Math.round` (well, `.round()`) all rect coordinates
//! to integers in CSS pixels, accepting that the rightmost column may
//! be 1px narrower than the others.

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::render::backend::{
    resolve_cell_colors, CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme,
};
use crate::term::attr_table::AttrTable;
use crate::term::attrs::Flags;
use crate::term::wcwidth::is_color_emoji_codepoint;

pub struct Canvas2dBackend {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    /// Saved per begin_frame so draw_row / draw_cursor can read them.
    metrics: FrameMetrics,
    /// `Theme` is cloned each frame because it holds 256 colors (~1KB).
    /// Cheap enough; avoids lifetime gymnastics in the trait.
    theme: Theme,
    /// CSS font string, rebuilt when font_family/size changes.
    font_css: String,
    /// CSS dimensions of the canvas surface in pixels.
    css_w: u32,
    css_h: u32,
}

impl Canvas2dBackend {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let ctx_obj = canvas
            .get_context("2d")
            .map_err(|e| format!("getContext('2d') failed: {:?}", e))?
            .ok_or_else(|| "getContext('2d') returned null".to_string())?;
        let ctx: CanvasRenderingContext2d = ctx_obj
            .dyn_into()
            .map_err(|_| "context is not Canvas2D".to_string())?;

        Ok(Self {
            canvas,
            ctx,
            metrics: FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
            },
            theme: Theme::default_dark(),
            font_css: String::from("15px monospace"),
            css_w: 0,
            css_h: 0,
        })
    }

    /// Set the font CSS used for `fillText`. Must include size, e.g.
    /// `15px "JetBrains Mono", monospace`. Call before measure_font.
    pub fn set_font(&mut self, font_css: String) {
        self.font_css = font_css;
    }

    fn rgba_to_css(c: [u8; 4]) -> String {
        if c[3] == 0xff {
            format!("rgb({},{},{})", c[0], c[1], c[2])
        } else {
            format!(
                "rgba({},{},{},{:.3})",
                c[0],
                c[1],
                c[2],
                c[3] as f32 / 255.0
            )
        }
    }
}

impl RenderBackend for Canvas2dBackend {
    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        // Build a temporary font string — don't mutate self.font_css here,
        // measure_font is meant to be a query.
        let font = format!("{}px {}", font_size_px, font_family);
        // Save & restore ctx state so we don't pollute the live font.
        self.ctx.save();
        self.ctx.set_font(&font);
        self.ctx.set_text_baseline("top");
        let metrics = self
            .ctx
            .measure_text("M")
            .map_err(|e| format!("measureText failed: {:?}", e))?;
        self.ctx.restore();

        // Width: use 'M' width. Most fixed-width fonts give a stable value.
        let w = metrics.width() as f32;

        // Height: prefer fontBoundingBoxAscent + Descent if available;
        // fall back to 1.2 * font_size (typical line-height estimate).
        let ascent = metrics.font_bounding_box_ascent() as f32;
        let descent = metrics.font_bounding_box_descent() as f32;
        let h = if ascent > 0.0 && descent > 0.0 {
            ascent + descent
        } else {
            font_size_px * 1.2
        };

        // Round to integer CSS pixels — sub-pixel cell sizes cause the
        // boundary-alignment problem documented in the module header.
        Ok((w.round().max(1.0), h.round().max(1.0)))
    }

    fn resize_surface(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), String> {
        self.css_w = width_css;
        self.css_h = height_css;
        let backing_w = (width_css as f32 * dpr).round() as u32;
        let backing_h = (height_css as f32 * dpr).round() as u32;
        self.canvas.set_width(backing_w.max(1));
        self.canvas.set_height(backing_h.max(1));
        // CSS size: lock to 100% of the container's content-box so the
        // canvas tracks subsequent container resizes automatically.
        //
        // Earlier code wrote `style.width = "{N}px"` here; that froze
        // the canvas at the first fit's pixel size. After freeze,
        // `getBoundingClientRect` always returned the frozen rect, so
        // `manager.ts::fitPane` (which reads canvas dims to compute
        // rows/cols) decided `sizeChanged === false` and bailed —
        // PTY and wasm kernel were never resized again. Container
        // would visually grow / shrink while the terminal content
        // stayed at the first-mount cols/rows. (TASKS §1.9, 2026-05-03.)
        //
        // The HTML width/height attributes (set via `set_width` /
        // `set_height` above) control the device-pixel backing buffer
        // and are independent of CSS — DPR changes update the buffer
        // without touching CSS layout.
        let style = self.canvas.style();
        style
            .set_property("width", "100%")
            .map_err(|e| format!("style.width: {:?}", e))?;
        style
            .set_property("height", "100%")
            .map_err(|e| format!("style.height: {:?}", e))?;
        // Reset transform — setting width/height clears the transform
        // matrix automatically, but we'll re-apply scale in begin_frame.
        Ok(())
    }

    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme) {
        self.metrics = metrics;
        self.theme = theme.clone();

        // Apply DPR scale + font + baseline at the start of every frame.
        // Setting canvas.width earlier resets the matrix to identity, so
        // we must re-scale here — this is correct, not paranoid.
        let _ = self
            .ctx
            .set_transform(metrics.dpr as f64, 0.0, 0.0, metrics.dpr as f64, 0.0, 0.0);
        self.ctx.set_font(&self.font_css);
        self.ctx.set_text_baseline("top");
    }

    fn clear(&mut self) {
        // §A.4 (2026-05-08) — hard-clear before painting bg so any pixel
        // residue left by partial-alpha overlays (selection tint, IME helper
        // shadow), or by a previous frame's draw that wrote outside cell
        // bounds (cursor extension at sub-pixel rounding), cannot survive a
        // full-redraw frame. Only fires on the full-redraw path
        // (`backend.rs::draw_frame` calls `clear` iff `full_redraw == true`),
        // so partial dirty-row updates remain on the optimised path. CSS-px
        // units match the subsequent `fill_rect` (set_transform applies dpr
        // scale uniformly to both calls).
        self.ctx
            .clear_rect(0.0, 0.0, self.css_w as f64, self.css_h as f64);
        self.ctx
            .set_fill_style_str(&Self::rgba_to_css(self.theme.bg));
        self.ctx
            .fill_rect(0.0, 0.0, self.css_w as f64, self.css_h as f64);
    }

    fn draw_row(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;
        let y_top = (row.row_index as f64 * cell_h).round();

        // Pass 1: backgrounds. Skip cells whose bg matches theme bg
        // (they were already painted by `clear()`, or by the previous
        // frame's content that didn't change). For partial draws (no
        // clear), we MUST paint bg to overwrite the previous frame.
        // Conservative: always paint, accept the perf hit. Canvas2D
        // is the oracle, not the fast path.
        for (col, cell) in row.cells.iter().enumerate() {
            // Wide-cell continuation halves carry the same bg as the
            // first half — paint them too so the bg spans both cells.
            let (_attrs, _fg, bg) = resolve_cell_colors(cell, attrs_table, &self.theme);
            self.ctx.set_fill_style_str(&Self::rgba_to_css(bg));
            let x = (col as f64 * cell_w).round();
            self.ctx.fill_rect(x, y_top, cell_w.ceil(), cell_h.ceil());
        }

        // Pass 2: glyphs. Skip width-0 (continuation halves of wide
        // cells — the wide cell's own draw already covers both halves)
        // and blanks (space at default attrs).
        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            if cell.ch == ' ' && cell.attr == crate::term::attr_table::AttrId::DEFAULT {
                continue;
            }
            let (attrs, fg, _bg) = resolve_cell_colors(cell, attrs_table, &self.theme);
            self.ctx.set_fill_style_str(&Self::rgba_to_css(fg));

            // Bold: rebuild the font string with `bold` weight prefix.
            // Italic: same with `italic` style. We only do this when needed
            // — most cells stay on the base font.
            if attrs.flags.contains(Flags::BOLD) || attrs.flags.contains(Flags::ITALIC) {
                let mut font = String::new();
                if attrs.flags.contains(Flags::ITALIC) {
                    font.push_str("italic ");
                }
                if attrs.flags.contains(Flags::BOLD) {
                    font.push_str("bold ");
                }
                font.push_str(&self.font_css);
                self.ctx.set_font(&font);
            } else {
                self.ctx.set_font(&self.font_css);
            }

            let x = (col as f64 * cell_w).round();
            // §4.7: if a multi-codepoint grapheme cluster was registered
            // at this column, paint the full cluster string via
            // `fill_text` (browsers handle ZWJ + variation selectors
            // natively when the font stack includes color-emoji fonts).
            // Otherwise fall back to the single codepoint stored in
            // `cell.ch`. Linear scan over `row.clusters` is cheap because
            // most rows have 0 clusters and emoji-heavy rows still have
            // <10.
            let cluster = if !row.clusters.is_empty() {
                let target = col.min(u16::MAX as usize) as u16;
                row.clusters.iter().find(|c| c.col == target)
            } else {
                None
            };
            // Wide-cell color emoji stretch: emoji fonts target ~1em
            // advance, narrower than 2 latin cells, so a bare `fillText`
            // leaves a gap on the right. Detect by codepoint range
            // (Canvas2D doesn't rasterize so we use a Unicode-block
            // heuristic — see `is_color_emoji_codepoint`). On match,
            // measure the natural advance and scale horizontally to
            // fill the 2-cell box. Cap the scale at 1.5× so degenerate
            // measurements can't grossly distort the glyph.
            let leading_cp: u32 = match cluster {
                Some(c) => c.text.chars().next().map(|ch| ch as u32).unwrap_or(0),
                None => cell.ch as u32,
            };
            let stretch_emoji = cell.width >= 2 && is_color_emoji_codepoint(leading_cp);
            let glyph_str: &str;
            let mut buf = [0u8; 4];
            match cluster {
                Some(cspan) => glyph_str = &cspan.text,
                None => glyph_str = cell.ch.encode_utf8(&mut buf),
            }
            if stretch_emoji {
                let target_w = cell_w * (cell.width.max(1) as f64);
                let natural_w = self
                    .ctx
                    .measure_text(glyph_str)
                    .ok()
                    .map(|m| m.width())
                    .unwrap_or(target_w);
                let scale_x = if natural_w > 1.0 {
                    (target_w / natural_w).clamp(1.0, 1.5)
                } else {
                    1.0
                };
                self.ctx.save();
                let _ = self.ctx.translate(x, y_top);
                let _ = self.ctx.scale(scale_x, 1.0);
                let _ = self.ctx.fill_text(glyph_str, 0.0, 0.0);
                self.ctx.restore();
            } else {
                let _ = self.ctx.fill_text(glyph_str, x, y_top);
            }

            // Underline / strikethrough as separate strokes after the glyph.
            if attrs.flags.contains(Flags::UNDERLINE) {
                let y = y_top + cell_h - 2.0;
                self.ctx.fill_rect(x, y, cell_w, 1.0);
            }
            if attrs.flags.contains(Flags::STRIKETHROUGH) {
                let y = y_top + cell_h * 0.5;
                self.ctx.fill_rect(x, y, cell_w, 1.0);
            }
        }
    }

    fn draw_cursor(&mut self, cursor: &CursorDraw, attrs_table: &AttrTable) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;
        let x = (cursor.col as f64 * cell_w).round();
        let y = (cursor.row as f64 * cell_h).round();

        // Cursor color (theme override) — paint the shape first.
        self.ctx
            .set_fill_style_str(&Self::rgba_to_css(self.theme.cursor_color));
        match cursor.style {
            CursorStyle::Block => {
                let w = if cursor.width == 2 {
                    cell_w * 2.0
                } else {
                    cell_w
                };
                self.ctx.fill_rect(x, y, w.ceil(), cell_h.ceil());
                // Repaint the glyph in cursor_text_color on top, so the
                // character under the cursor stays legible.
                if cursor.ch != ' ' && cursor.ch != '\0' {
                    self.ctx
                        .set_fill_style_str(&Self::rgba_to_css(self.theme.cursor_text_color));
                    self.ctx.set_font(&self.font_css);
                    let mut buf = [0u8; 4];
                    let s = cursor.ch.encode_utf8(&mut buf);
                    let _ = self.ctx.fill_text(s, x, y);
                }
                // Silence unused on attrs_table for this style.
                let _ = attrs_table;
            }
            CursorStyle::Bar => {
                self.ctx.fill_rect(x, y, 2.0, cell_h.ceil());
                let _ = attrs_table;
            }
            CursorStyle::Underline => {
                self.ctx.fill_rect(x, y + cell_h - 2.0, cell_w.ceil(), 2.0);
                let _ = attrs_table;
            }
        }
    }

    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;
        self.ctx
            .set_fill_style_str(&Self::rgba_to_css(self.theme.selection_bg));
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let x = (col_start as f64 * cell_w).round();
            let y = (row as f64 * cell_h).round();
            let w = ((col_end - col_start) as f64 * cell_w).ceil();
            self.ctx.fill_rect(x, y, w, cell_h.ceil());
        }
    }

    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;
        self.ctx
            .set_fill_style_str(&Self::rgba_to_css(self.theme.hyperlink_color));
        // 1px tall underline at the bottom of the cell row. The DPR
        // transform applied in begin_frame means 1.0 here = 1 CSS px,
        // not 1 device px — visible on hi-DPI without manual scaling.
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let x = (col_start as f64 * cell_w).round();
            let y = (row as f64 * cell_h + cell_h - 1.0).round();
            let w = ((col_end - col_start) as f64 * cell_w).ceil();
            self.ctx.fill_rect(x, y, w, 1.0);
        }
    }

    fn end_frame(&mut self) {
        // Canvas2D draws are immediate — nothing to commit.
    }
}
