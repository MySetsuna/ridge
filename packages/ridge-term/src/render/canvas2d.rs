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
use wasm_bindgen::JsValue;
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, OffscreenCanvas,
    OffscreenCanvasRenderingContext2d, TextMetrics,
};

use crate::render::backend::{
    resolve_cell_colors, CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme,
};
use crate::render::procedural_box;
use crate::term::attr_table::AttrTable;
use crate::term::attrs::Flags;
// §B.8 (2026-05-08) — `is_color_emoji_codepoint` and
// `is_visual_wide_codepoint` were the codepoint-list driven gates of
// the pre-§B.8 stretch path; now the canvas's own `fill_text_with_max_width`
// + `cell_w * 2.0` cap handles the overflow at runtime, codepoint-
// agnostic. Imports removed; the helpers themselves are still in
// `wcwidth.rs` for completeness / external consumers.

/// §p4.9 (2026-05-22) — abstraction over the 2D drawing context so the
/// same `Canvas2dBackend` body works on both `CanvasRenderingContext2d`
/// (DOM canvas, main thread) and `OffscreenCanvasRenderingContext2d`
/// (worker thread, OffscreenCanvas). Each method is a one-liner that
/// delegates to the underlying inherent web-sys method; the only
/// non-trivial part is `measure_text`, whose return type is the same
/// `web_sys::TextMetrics` for both contexts.
pub trait Canvas2dCtxLike {
    fn save(&self);
    fn restore(&self);
    fn set_font(&self, font: &str);
    fn set_text_baseline(&self, value: &str);
    fn measure_text(&self, text: &str) -> Result<TextMetrics, JsValue>;
    fn set_transform(
        &self,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> Result<(), JsValue>;
    fn clear_rect(&self, x: f64, y: f64, w: f64, h: f64);
    fn fill_rect(&self, x: f64, y: f64, w: f64, h: f64);
    fn fill_text(&self, text: &str, x: f64, y: f64) -> Result<(), JsValue>;
    fn set_fill_style_str(&self, value: &str);
}

impl Canvas2dCtxLike for CanvasRenderingContext2d {
    fn save(&self) {
        CanvasRenderingContext2d::save(self);
    }
    fn restore(&self) {
        CanvasRenderingContext2d::restore(self);
    }
    fn set_font(&self, font: &str) {
        CanvasRenderingContext2d::set_font(self, font);
    }
    fn set_text_baseline(&self, value: &str) {
        CanvasRenderingContext2d::set_text_baseline(self, value);
    }
    fn measure_text(&self, text: &str) -> Result<TextMetrics, JsValue> {
        CanvasRenderingContext2d::measure_text(self, text)
    }
    fn set_transform(
        &self,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> Result<(), JsValue> {
        CanvasRenderingContext2d::set_transform(self, a, b, c, d, e, f)
    }
    fn clear_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        CanvasRenderingContext2d::clear_rect(self, x, y, w, h);
    }
    fn fill_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        CanvasRenderingContext2d::fill_rect(self, x, y, w, h);
    }
    fn fill_text(&self, text: &str, x: f64, y: f64) -> Result<(), JsValue> {
        CanvasRenderingContext2d::fill_text(self, text, x, y)
    }
    fn set_fill_style_str(&self, value: &str) {
        CanvasRenderingContext2d::set_fill_style_str(self, value);
    }
}

impl Canvas2dCtxLike for OffscreenCanvasRenderingContext2d {
    fn save(&self) {
        OffscreenCanvasRenderingContext2d::save(self);
    }
    fn restore(&self) {
        OffscreenCanvasRenderingContext2d::restore(self);
    }
    fn set_font(&self, font: &str) {
        OffscreenCanvasRenderingContext2d::set_font(self, font);
    }
    fn set_text_baseline(&self, value: &str) {
        OffscreenCanvasRenderingContext2d::set_text_baseline(self, value);
    }
    fn measure_text(&self, text: &str) -> Result<TextMetrics, JsValue> {
        OffscreenCanvasRenderingContext2d::measure_text(self, text)
    }
    fn set_transform(
        &self,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    ) -> Result<(), JsValue> {
        OffscreenCanvasRenderingContext2d::set_transform(self, a, b, c, d, e, f)
    }
    fn clear_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        OffscreenCanvasRenderingContext2d::clear_rect(self, x, y, w, h);
    }
    fn fill_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        OffscreenCanvasRenderingContext2d::fill_rect(self, x, y, w, h);
    }
    fn fill_text(&self, text: &str, x: f64, y: f64) -> Result<(), JsValue> {
        OffscreenCanvasRenderingContext2d::fill_text(self, text, x, y)
    }
    fn set_fill_style_str(&self, value: &str) {
        OffscreenCanvasRenderingContext2d::set_fill_style_str(self, value);
    }
}

/// §p4.9 (2026-05-22) — abstraction over the canvas surface. Both
/// `HtmlCanvasElement` and `OffscreenCanvas` accept `set_width` /
/// `set_height` to size the backing buffer, but only `HtmlCanvasElement`
/// has a `.style` CSS property — for the worker path, the host owns
/// the layout (it transferred a pre-sized canvas) so `lock_css_size`
/// is a documented no-op.
pub trait Canvas2dSurfaceLike {
    fn set_width(&self, value: u32);
    fn set_height(&self, value: u32);
    /// On the main thread, lock the canvas CSS size to `width: 100%`
    /// / `height: 100%` so it tracks subsequent container resizes
    /// (see TASKS §1.9 — earlier code froze it at first-fit pixel
    /// size, blocking later fitPane calls). On the worker thread,
    /// the host owns layout, so this is a no-op.
    fn lock_css_size_to_100_percent(&self) -> Result<(), String>;
}

impl Canvas2dSurfaceLike for HtmlCanvasElement {
    fn set_width(&self, value: u32) {
        HtmlCanvasElement::set_width(self, value);
    }
    fn set_height(&self, value: u32) {
        HtmlCanvasElement::set_height(self, value);
    }
    fn lock_css_size_to_100_percent(&self) -> Result<(), String> {
        let style = self.style();
        style
            .set_property("width", "100%")
            .map_err(|e| format!("style.width: {:?}", e))?;
        style
            .set_property("height", "100%")
            .map_err(|e| format!("style.height: {:?}", e))?;
        Ok(())
    }
}

impl Canvas2dSurfaceLike for OffscreenCanvas {
    fn set_width(&self, value: u32) {
        OffscreenCanvas::set_width(self, value);
    }
    fn set_height(&self, value: u32) {
        OffscreenCanvas::set_height(self, value);
    }
    fn lock_css_size_to_100_percent(&self) -> Result<(), String> {
        // OffscreenCanvas has no DOM presence — host owns layout.
        Ok(())
    }
}

pub struct Canvas2dBackend {
    canvas: Box<dyn Canvas2dSurfaceLike>,
    ctx: Box<dyn Canvas2dCtxLike>,
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
    /// Main-thread constructor — `HtmlCanvasElement` from the DOM.
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let ctx_obj = canvas
            .get_context("2d")
            .map_err(|e| format!("getContext('2d') failed: {:?}", e))?
            .ok_or_else(|| "getContext('2d') returned null".to_string())?;
        let ctx: CanvasRenderingContext2d = ctx_obj
            .dyn_into()
            .map_err(|_| "context is not Canvas2D".to_string())?;
        Ok(Self::from_handles(Box::new(canvas), Box::new(ctx)))
    }

    /// §p4.9 (2026-05-22) — worker-thread constructor.
    ///
    /// `OffscreenCanvas` is the only canvas type a DedicatedWorker can
    /// own (you can't ship a `HtmlCanvasElement` cross-realm; you call
    /// `canvas.transferControlToOffscreen()` on the main thread, then
    /// postMessage the resulting `OffscreenCanvas` via `transferList`).
    /// The 2D context surface this gives us back is fully symmetric
    /// with `CanvasRenderingContext2d` for everything this backend uses
    /// — see `Canvas2dCtxLike`.
    pub fn new_from_offscreen(canvas: OffscreenCanvas) -> Result<Self, String> {
        let ctx_obj = canvas
            .get_context("2d")
            .map_err(|e| format!("getContext('2d') failed: {:?}", e))?
            .ok_or_else(|| "getContext('2d') returned null".to_string())?;
        let ctx: OffscreenCanvasRenderingContext2d = ctx_obj
            .dyn_into()
            .map_err(|_| "context is not OffscreenCanvas2D".to_string())?;
        Ok(Self::from_handles(Box::new(canvas), Box::new(ctx)))
    }

    /// Shared post-context init. Both constructors funnel here so the
    /// default metrics / theme / font_css live in one place.
    fn from_handles(
        canvas: Box<dyn Canvas2dSurfaceLike>,
        ctx: Box<dyn Canvas2dCtxLike>,
    ) -> Self {
        Self {
            canvas,
            ctx,
            metrics: FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
                tui_mode: false,
            },
            theme: Theme::default_dark(),
            font_css: String::from("15px monospace"),
            css_w: 0,
            css_h: 0,
        }
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
        self.canvas.lock_css_size_to_100_percent()?;
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
        
        let clear_bg = if self.metrics.tui_mode { self.theme.tui_bg } else { [0, 0, 0, 0] };
        if clear_bg[3] != 0 {
            self.ctx
                .set_fill_style_str(&Self::rgba_to_css(clear_bg));
            self.ctx
                .fill_rect(0.0, 0.0, self.css_w as f64, self.css_h as f64);
        }
    }

    fn draw_row_backgrounds(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;

        let y_top = (row.row_index as f64 * cell_h).round();
        let y_bottom = ((row.row_index + 1) as f64 * cell_h).round();
        let h = y_bottom - y_top;

        let mut extra_cells: f64 = 0.0;

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }

            let (_attrs, _fg, bg) = resolve_cell_colors(cell, attrs_table, &self.theme, self.metrics.tui_mode);

            let mut buf = [0u8; 4];
            let glyph_str: &str = match row.clusters.iter().find(|c| c.col == col.min(u16::MAX as usize) as u16) {
                Some(cspan) => &cspan.text,
                None => cell.ch.encode_utf8(&mut buf),
            };

            let effective_span: f64 = if cell.width >= 2 {
                match self.ctx.measure_text(glyph_str) {
                    Ok(m) => (m.width().max(1.0) / cell_w).ceil(),
                    Err(_) => cell.width as f64,
                }
            } else {
                cell.width as f64
            };

            let effective_col = col as f64 + extra_cells;

            let x_left = (effective_col * cell_w).round();
            let x_right = ((effective_col + effective_span) * cell_w).round();
            let w = x_right - x_left;

            if w <= 0.0 {
                continue;
            }

            if bg[3] == 0 {
                self.ctx.clear_rect(x_left, y_top, w, h);
            } else {
                self.ctx.set_fill_style_str(&Self::rgba_to_css(bg));
                self.ctx.fill_rect(x_left, y_top, w, h);
            }

            extra_cells += effective_span - cell.width as f64;
        }
    }

    fn draw_row_texts(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;

        let y_top = (row.row_index as f64 * cell_h).round();
        let y_bottom = ((row.row_index + 1) as f64 * cell_h).round();
        let h = y_bottom - y_top;

        let mut extra_cells: f64 = 0.0;

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            if cell.ch == ' ' && cell.attr == crate::term::attr_table::AttrId::DEFAULT {
                continue;
            }
            let (attrs, fg, _bg) = resolve_cell_colors(cell, attrs_table, &self.theme, self.metrics.tui_mode);
            self.ctx.set_fill_style_str(&Self::rgba_to_css(fg));

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

            let cluster = if !row.clusters.is_empty() {
                let target = col.min(u16::MAX as usize) as u16;
                row.clusters.iter().find(|c| c.col == target)
            } else {
                None
            };
            let glyph_str: &str;
            let mut buf = [0u8; 4];
            match cluster {
                Some(cspan) => glyph_str = &cspan.text,
                None => glyph_str = cell.ch.encode_utf8(&mut buf),
            }

            let effective_col = col as f64 + extra_cells;
            let grid_x_left = (effective_col * cell_w).round();
            let grid_span_w = ((effective_col + cell.width as f64) * cell_w).round() - grid_x_left;

            let effective_span: f64 = if cell.width >= 2 {
                match self.ctx.measure_text(glyph_str) {
                    Ok(m) => (m.width().max(1.0) / cell_w).ceil(),
                    Err(_) => cell.width as f64,
                }
            } else {
                cell.width as f64
            };

            let first_char = glyph_str.chars().next();
            let mut drawn_procedurally = false;

            if let Some(ch) = first_char {
                if let Some(rects) = procedural_box(ch, grid_x_left as f32, y_top as f32, grid_span_w as f32, h as f32) {
                    for r in rects {
                        self.ctx.fill_rect(r.x as f64, r.y as f64, r.w as f64, r.h as f64);
                    }
                    drawn_procedurally = true;
                }
            }

            if !drawn_procedurally {
                // §B.9 — render at natural browser size (no aspect-fit).
                // Wide / emoji glyphs expand beyond their grid span and
                // push subsequent cells right via extra_cells.
                let _ = self.ctx.fill_text(glyph_str, grid_x_left, y_top);
            }

            let effective_w = ((effective_col + effective_span) * cell_w).round() - grid_x_left;

            if attrs.flags.contains(Flags::UNDERLINE) {
                let y = y_top + h - 2.0;
                self.ctx.fill_rect(grid_x_left, y, effective_w, 1.0);
            }
            if attrs.flags.contains(Flags::STRIKETHROUGH) {
                let y = y_top + h * 0.5;
                self.ctx.fill_rect(grid_x_left, y, effective_w, 1.0);
            }

            extra_cells += effective_span - cell.width as f64;
        }
    }

    fn draw_cursor(&mut self, cursor: &CursorDraw, _attrs_table: &AttrTable) {
        let cell_w = self.metrics.cell_w as f64;
        let cell_h = self.metrics.cell_h as f64;
        let effective_col = cursor.col as f64 + cursor.extra_cells;
        let x = (effective_col * cell_w + 0.5).floor();
        let y = (cursor.row as f64 * cell_h + 0.5).floor();
        let y_bottom = ((cursor.row + 1) as f64 * cell_h + 0.5).floor();
        let cell_h_int = y_bottom - y;
        let cursor_span = cursor.width.max(1) as usize;

        // §B.9 — measure the glyph's natural advance to compute the
        // effective cursor block width (no aspect-fit; natural size).
        let text = match &cursor.cluster_text {
            Some(t) if !t.is_empty() => t.clone(),
            _ => cursor.ch.encode_utf8(&mut [0u8; 4]).to_string(),
        };
        let effective_span = if cursor_span >= 2 {
            match self.ctx.measure_text(&text) {
                Ok(m) => (m.width().max(1.0) / cell_w).ceil() as usize,
                Err(_) => cursor_span,
            }
        } else {
            cursor_span
        };
        let x_right = ((effective_col + effective_span as f64) * cell_w + 0.5).floor();
        let span_w = x_right - x;

        self.ctx
            .set_fill_style_str(&Self::rgba_to_css(self.theme.cursor_color));
        match cursor.style {
            CursorStyle::Block => {
                self.ctx.fill_rect(x, y, span_w, cell_h_int);
                if cursor.ch != ' ' && cursor.ch != '\0' {
                    self.ctx
                        .set_fill_style_str(&Self::rgba_to_css(self.theme.cursor_text_color));
                    self.ctx.set_font(&self.font_css);
                    // §B.9 — render at natural size, no aspect-fit.
                    let _ = self.ctx.fill_text(&text, x, y);
                }
            }
            CursorStyle::Bar => {
                self.ctx.fill_rect(x, y, 2.0, cell_h_int);
            }
            CursorStyle::Underline => {
                self.ctx.fill_rect(x, y + cell_h_int - 2.0, span_w, 2.0);
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
            let x_left = (col_start as f64 * cell_w).round();
            let x_right = (col_end as f64 * cell_w).round();
            let w = x_right - x_left;
            let y_top = (row as f64 * cell_h).round();
            let y_bottom = ((row + 1) as f64 * cell_h).round();
            let h = y_bottom - y_top;
            self.ctx.fill_rect(x_left, y_top, w, h);
        }
    }

    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        let cell_w = self.metrics.cell_w as f64;
        self.ctx
            .set_fill_style_str(&Self::rgba_to_css(self.theme.hyperlink_color));
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let x_left = (col_start as f64 * cell_w).round();
            let x_right = (col_end as f64 * cell_w).round();
            let w = x_right - x_left;
            let y_bottom = ((row + 1) as f64 * self.metrics.cell_h as f64).round();
            let y = y_bottom - 1.0;
            self.ctx.fill_rect(x_left, y, w, 1.0);
        }
    }

    fn end_frame(&mut self) {
        // Canvas2D draws are immediate — nothing to commit.
    }
}
