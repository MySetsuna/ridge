//! WebGPU per-pane backend — Round 3 §4.3 Phase B (single Surface).
//!
//! ## Status
//!
//! All panes share one process-wide [`super::gpu_context::GpuContext`]
//! that owns `wgpu::Instance` / `Device` / `Queue` / `cell_pipeline` /
//! `GlyphAtlas` / `atlas_texture` / `GlyphRasterizer` / `sampler`, AND
//! one process-wide [`super::surface_host::SurfaceHost`] that owns the
//! single `wgpu::Surface` bound to the global host canvas in
//! `+page.svelte`.
//!
//! Each `WebGpuPaneBackend` instance keeps only what is genuinely
//! per-pane: a 16-byte `frame_uniform`, a vertex `instance_buffer`, a
//! `bind_group` referencing the shared atlas view via the per-pane
//! uniform, a `pending_instances` accumulator, a per-frame
//! `frame_pinned` bitmap that guards the in-frame atlas eviction race,
//! and a `viewport: ScissorRect` describing where on the host canvas
//! this pane lives in device pixels.
//!
//! ## Per-frame protocol (Phase B)
//!
//! 1. JS RAF tick calls `SurfaceHostHandle::beginFrame(theme_bg)` once.
//! 2. For each dirty pane, the renderer drives `begin_frame` /
//!    `draw_row` / overlays / `end_frame` against THIS struct.
//! 3. `end_frame` here uploads its uniform + instance buffer, then
//!    queues pane draw handles for the host's shared frame pass; the host
//!    records viewport, scissor, bind group, vertex buffer, and draw calls
//!    after all panes have uploaded their frame data.
//! 4. JS calls `SurfaceHostHandle::endFrame()` after all panes; one
//!    `queue.submit` + one `present` for the entire window.
//!
//! ## Atlas-generation cross-pane invalidation
//!
//! When pane A grows the atlas (font enlarged, DPR jumped) it calls
//! `ctx.rebuild_atlas()`, which bumps `ctx.atlas_generation`. Pane B's
//! existing `bind_group` still references the *old* `atlas_view` until
//! its next `begin_frame` notices that `atlas_generation_seen` is
//! behind and rebuilds — without that check, B would sample stale slots
//! and render misaligned glyphs.
//!
//! ## Adapter-miss policy
//!
//! `new(host)` returns `Err` when shared browser GPU setup fails.
//! `RenderHandle::newWithWebgpuFirst` propagates the
//! explicit initialization error so the host can surface the failure.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use std::cell::RefCell;
use std::rc::Rc;

use super::glyph_atlas::{glyph_quad_geometry_for_cell, GlyphEntry, GlyphKey};
use super::gpu_context::{GpuContext, ATLAS_SUPERSAMPLE};
use super::surface_host::{ScissorRect, SurfaceHost};
use crate::render::backend::{
    physical_row_boundary, CursorDraw, FrameMetrics, RenderBackend, RowDraw, ScrollCopyResult,
    Theme,
};
use crate::render::renderer::{
    history_overlay_geometry, history_overlay_surface, history_text, history_text_width,
};
use crate::term::attr_table::AttrTable;
use crate::term::cell::{scan_line_path, RenderPath};
use crate::term::grid::ScrollOp;

thread_local! {
    /// §present-fast (2026-06-22): process-wide opt-in flag gating
    /// `requires_full_frame`. Default `false` keeps the always-true
    /// correctness behaviour (full re-encode + LoadOp::Clear every tick),
    /// required where the WebView2 swap chain drops prior pixels under
    /// LoadOp::Load (dev Edge WebView2 148). Set to `true` from JS
    /// (`setPresentFast`, gated on `localStorage.RIDGE_PRESENT_FAST`) on a
    /// release WebView2 verified to preserve swap-chain pixels — flips the
    /// renderer back to the dirty-row fast path, killing the per-frame full
    /// Clear behind IME-composition / selection flicker AND the per-frame
    /// glyph re-admission that maxes out the switch-workspace atlas-eviction
    /// churn (transient garble). Fully reversible: unset → always-full path.
    static PRESENT_FAST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// §present-fast: set the process-wide present-fast flag. See `PRESENT_FAST`
/// and `WebGpuPaneBackend::requires_full_frame`.
pub fn set_present_fast(on: bool) {
    PRESENT_FAST.with(|c| c.set(on));
}

#[inline]
fn present_fast() -> bool {
    PRESENT_FAST.with(|c| c.get())
}

/// High bit tag for grapheme-cluster glyph IDs so they cannot collide
/// with any Unicode codepoint (max 0x10FFFF).
const CLUSTER_TAG: u32 = 0x8000_0000;

/// CellInstance `is_color` sentinel for solid overlay rectangles. Character
/// cells never use this mode: box drawing, block elements, and symbols all
/// go through the system-font rasterizer and shared glyph atlas. 0 = mono
/// atlas glyph, 1 = color emoji, 2 = solid overlay.
const INSTANCE_MODE_SOLID: u32 = 2;

/// Convert an `[u8; 4]` byte color into the f32 form CellInstance
/// fields use. Vertex stage shaders can multiply linearly without
/// re-normalizing.
fn rgba_u8_to_f32(rgba: [u8; 4]) -> [f32; 4] {
    [
        rgba[0] as f32 / 255.0,
        rgba[1] as f32 / 255.0,
        rgba[2] as f32 / 255.0,
        rgba[3] as f32 / 255.0,
    ]
}

fn push_background_instance(
    instances: &mut Vec<CellInstance>,
    start_col: usize,
    end_col: usize,
    cell_w: f32,
    pixel_y: f32,
    row_h: f32,
    fg: [u8; 4],
    bg: [u8; 4],
) {
    let pixel_x = (start_col as f32 * cell_w + 0.5).floor();
    let pixel_x_right = (end_col as f32 * cell_w + 0.5).floor();
    instances.push(CellInstance {
        cell_xy: [pixel_x, pixel_y],
        cell_size: [pixel_x_right - pixel_x, row_h],
        atlas_uv: [0.0, 0.0, 0.0, 0.0],
        atlas_layer: 0,
        fg_rgba: rgba_u8_to_f32(fg),
        bg_rgba: rgba_u8_to_f32(bg),
        is_color: 0,
    });
}

fn flush_background_run(
    instances: &mut Vec<CellInstance>,
    run: &mut Option<(usize, usize, [u8; 4], [u8; 4])>,
    cell_w: f32,
    pixel_y: f32,
    row_h: f32,
) {
    let Some((start_col, end_col, fg, bg)) = run.take() else {
        return;
    };
    push_background_instance(
        instances, start_col, end_col, cell_w, pixel_y, row_h, fg, bg,
    );
}

/// Initial per-frame cell instance buffer capacity. Realistic terminal
/// sessions have a few thousand cells; 1024 covers small panes and the
/// buffer grows on demand for larger ones.
const INITIAL_INSTANCE_CAPACITY: u32 = 1024;

/// CPU-side instance struct matching the WGSL `InstanceIn` layout.
/// `#[repr(C)]` makes the field order load-bearing — must mirror the
/// `attributes: &[VertexAttribute { offset, ... }]` array passed to
/// `RenderPipelineDescriptor::vertex.buffers` (defined in
/// `gpu_context.rs::new`).
///
/// Pod + Zeroable allow `bytemuck::cast_slice(&[CellInstance])` to
/// return `&[u8]` without unsafe transmutes. Layout: 7 fields,
/// all f32 / u32 / [f32; N] arrays, 4-byte aligned, 72 bytes total — no
/// implicit padding so `Pod` is sound.
///
/// §B.3 (2026-05-08) — `is_color` was added so the fragment shader can
/// branch on per-glyph color/mono classification carried from the
/// rasterizer's pixel-scan, instead of inferring it per-fragment from
/// `glyph.rgb < 0.99`. The per-fragment heuristic was unreliable
/// because Linear-filter sampling at AA fringe pixels averages a
/// painted (1,1,1) texel with a transparent (0,0,0,0) neighbour,
/// producing fractional rgb that the heuristic misclassified as
/// "color emoji" — the shader then used the gray rgb instead of
/// tinting with `fg_rgba`, producing the user-visible "白色毛边" /
/// halo on monochrome glyphs against contrasting backgrounds.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CellInstance {
    cell_xy: [f32; 2],   // 0..8
    cell_size: [f32; 2], // 8..16
    atlas_uv: [f32; 4],  // 16..32
    atlas_layer: u32,    // 32..36
    fg_rgba: [f32; 4],   // 36..52
    bg_rgba: [f32; 4],   // 52..68
    is_color: u32, // 68..72  — 0 = mono atlas glyph, 1 = color emoji bitmap, 2 = solid overlay (cell.wgsl skips atlas sampling)
}

/// Re-exported so `gpu_context.rs` can wire the shared `cell_pipeline`'s
/// vertex layout against the same struct stride. Changing `CellInstance`
/// offsets here without updating the matching `VertexAttribute` array in
/// `gpu_context.rs::new` would silently corrupt every drawn cell.
pub(super) const CELL_INSTANCE_STRIDE: u64 = std::mem::size_of::<CellInstance>() as u64;

/// WebGPU per-pane backend — Phase B form. The heavy GPU resources live
/// on a shared [`GpuContext`], the swap-chain surface lives on a shared
/// [`SurfaceHost`]; this struct keeps just per-pane scratch buffers + a
/// scissor rect describing where on the host canvas this pane lives.
pub struct WebGpuPaneBackend {
    /// Shared GPU stack (instance / device / queue / pipeline / atlas /
    /// rasterizer / sampler). All `borrow` / `borrow_mut` calls in this
    /// file are short-lived and **never nested** — see `draw_row` for
    /// the lookup-then-admit pattern that splits hits and misses into
    /// separate borrows.
    ctx: Rc<RefCell<GpuContext>>,
    /// Shared swap-chain host. `end_frame` calls
    /// `host.queue_pane(viewport, ...)` so all panes
    /// composite into one render pass per frame on the global host
    /// canvas. The host takes its `ctx.borrow()` only when recording
    /// that shared pass.
    host: Rc<RefCell<SurfaceHost>>,
    /// Last `ctx.atlas_generation` this pane built `bind_group` against.
    /// When `begin_frame` sees a higher value it rebuilds the bind
    /// group so the next `draw_row` samples the new `atlas_view`.
    atlas_generation_seen: u64,
    /// Pane's rectangle on the host canvas in **device pixels**.
    /// `resize_surface` records the new value; `end_frame` passes it to
    /// `host.queue_pane` which sets viewport + scissor on the shared
    /// pass. Empty rects (`w == 0 || h == 0`) skip drawing entirely
    /// (parked-by-clip — pane dragged to zero width or off-canvas).
    viewport: ScissorRect,
    /// 16-byte uniform buffer holding `FrameUniform { viewport, _pad }`.
    /// Per-pane because the vertex shader's NDC conversion divides
    /// `cell_xy` by this `viewport` (= pane-local device-pixel size).
    /// `queue_pane` then maps the resulting NDC into the pane's rect
    /// on the host canvas via `pass.set_viewport(scissor.x, scissor.y,
    /// scissor.w, scissor.h, 0, 1)`.
    frame_uniform: wgpu::Buffer,
    /// Per-cell instance buffer. Initial capacity =
    /// `INITIAL_INSTANCE_CAPACITY`; doubles on overflow inside `end_frame`.
    instance_buffer: Rc<wgpu::Buffer>,
    instance_capacity: u32,
    /// Bind group instance against `ctx.cell_bind_group_layout`. Holds
    /// references to `frame_uniform` (per-pane) + `ctx.atlas_view` +
    /// `ctx.sampler` (shared). Rebuilt when `ctx.atlas_generation`
    /// advances (atlas reallocated) — see `begin_frame`.
    bind_group: Rc<wgpu::BindGroup>,
    /// Per-frame CellInstance accumulator. `begin_frame` clears it,
    /// `draw_row` / `draw_cursor` / `draw_*_overlay` push, `end_frame`
    /// uploads via `queue.write_buffer` and forwards to host.
    pending_instances: Vec<CellInstance>,
    /// Same-frame glyph admission cache. The shared atlas remains the source
    /// of truth; this only avoids repeating RefCell/HashMap lookups for a
    /// glyph repeated across rows in one pane frame.
    glyph_frame_cache: std::collections::HashMap<GlyphKey, Option<GlyphEntry>>,
    /// Visible kernel rows redrawn in this pane frame. Used only to restore
    /// wallpaper pixels beneath transparent default cells before cell draws.
    damaged_rows: Vec<u32>,
    /// Per-layer pin flag, reset to all-`false` every `begin_frame`.
    /// A layer is pinned the moment any cell in this frame's
    /// `pending_instances` references it, so `ctx.rasterize_and_admit`
    /// can skip pinned layers during LRU eviction. Length tracks
    /// `ctx.atlas_layers` (re-checked defensively in `begin_frame`).
    frame_pinned: Vec<bool>,
    metrics: FrameMetrics,
    theme: Theme,
    /// Font identity copied once at `begin_frame`; row rendering must not
    /// borrow/hash the shared context for every dirty row.
    font_family_hash: u64,
    font_size_q: u16,
    /// Set when the renderer must re-encode every visible row on the
    /// next frame. Drives `requires_full_frame()` (consumed by
    /// `Renderer::tick` to mark all rows dirty so the row-hash diff
    /// doesn't skip them). Reset to false at the bottom of `end_frame`
    /// after the host pass records the draw. The host's
    /// `LoadOp::Clear` vs `Load` decision is now governed by
    /// `SurfaceHost::needs_initial_clear` (frame-level, cross-pane),
    /// independent from this per-pane re-encode flag.
    /// Set true on construct, on `resize_surface` dim change, on
    /// `invalidate_atlas`, on cross-pane atlas-generation rebuild, and
    /// via `on_full_invalidate` when the renderer detects scroll /
    /// selection / snapshot-growth.
    needs_initial_clear: bool,
}

impl Drop for WebGpuPaneBackend {
    fn drop(&mut self) {
        // Drop bind_group, frame_uniform, and instance_buffer explicitly
        // (if wgpu needs it) or just let them drop naturally.
        // In wgpu-rs, buffers/bindgroups drop automatically on scope exit.
    }
}

fn glyph_id_for(cluster_text: Option<&str>, character: char) -> u32 {
    match cluster_text {
        Some(text) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            CLUSTER_TAG | (hasher.finish() as u32 & !CLUSTER_TAG)
        }
        None => character as u32,
    }
}

impl WebGpuPaneBackend {
    /// Acquire (or reuse) the shared `GpuContext` + `SurfaceHost`, then
    /// allocate this pane's per-pane buffers + bind group. Async
    /// because the first call performs the browser GPU adapter /
    /// device bootstrap; subsequent calls return the cached `Rc`
    /// immediately.
    ///
    /// Per-workspace SurfaceHost passed in by JS. Caller obtains the
    /// reference from a `SurfaceHostHandle` constructed for the
    /// pane's workspace tab — no thread-local lookup, multiple
    /// SurfaceHost instances coexist, one per workspace canvas.
    pub async fn new(host: Rc<RefCell<SurfaceHost>>) -> Result<Self, String> {
        let ctx = host.borrow().gpu_context();
        let (frame_uniform, instance_buffer, bind_group, atlas_generation_seen, frame_pinned) = {
            let ctx_b = ctx.borrow();

            let frame_uniform = ctx_b.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ridge-frame-uniform"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let instance_buffer = ctx_b.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ridge-instance-buffer"),
                size: (INITIAL_INSTANCE_CAPACITY as u64) * CELL_INSTANCE_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let bind_group = ctx_b.build_bind_group(&frame_uniform);
            let atlas_generation_seen = ctx_b.atlas_generation;
            let frame_pinned = vec![false; ctx_b.atlas_layers as usize];

            (
                frame_uniform,
                Rc::new(instance_buffer),
                Rc::new(bind_group),
                atlas_generation_seen,
                frame_pinned,
            )
        }; // ctx_b drops here — borrow released before constructing Self.

        Ok(Self {
            ctx,
            host,
            atlas_generation_seen,
            viewport: ScissorRect::ZERO,
            frame_uniform,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            bind_group,
            pending_instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize),
            glyph_frame_cache: std::collections::HashMap::new(),
            damaged_rows: Vec::new(),
            frame_pinned,
            metrics: FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
                tui_mode: false,
            },
            theme: Theme::default_dark(),
            font_family_hash: 0,
            font_size_q: 0,
            // First frame must re-encode every row — viewport rect just
            // assigned by JS is fresh and the pane has never drawn.
            needs_initial_clear: true,
        })
    }

    /// Set the CSS font family + pixel size used for glyph rasterization.
    /// Forwards to the shared `GpuContext` — every pane sees the new
    /// font on the next frame because `ctx.set_font_config` invalidates
    /// the atlas (bumps `atlas_generation`), and per-pane `begin_frame`
    /// detects the bump and rebuilds its bind group.
    ///
    /// Idempotent on no-op (same family + size).
    pub fn set_font_config(&mut self, font_family: String, font_size_px: f32) {
        self.ctx
            .borrow_mut()
            .set_font_config(font_family, font_size_px);
    }

    pub fn backend_name(&self) -> &'static str {
        self.ctx.borrow().backend_name
    }

    /// Update the pane's `(x, y)` position on the host canvas, in device
    /// pixels. Called by JS (`manager.ts::_recomputeViewport`) when the
    /// splitter drag moves a pane's container without changing its
    /// dimensions. Does not flag `needs_initial_clear` — the pane's own
    /// pixels are unchanged on a positional shift; JS calls
    /// `surfaceHost.invalidate()` after layout settle so the host's
    /// next frame `LoadOp::Clear`s the old area.
    pub fn set_viewport_offset(&mut self, x: u32, y: u32) {
        self.viewport.x = x;
        self.viewport.y = y;
    }
}

fn cursor_glyph_id(cursor: &CursorDraw) -> u32 {
    match &cursor.cluster_text {
        Some(text) if !text.is_empty() => {
            use std::hash::Hasher;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            hasher.write(text.as_bytes());
            let raw = hasher.finish() as u32;
            CLUSTER_TAG | (raw & !CLUSTER_TAG)
        }
        _ => cursor.ch as u32,
    }
}

fn cursor_geometry(
    style: crate::render::backend::CursorStyle,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    thickness: f32,
) -> (f32, f32, f32, f32) {
    match style {
        crate::render::backend::CursorStyle::Block => (x, y, width, height),
        crate::render::backend::CursorStyle::Bar => (x, y, thickness, height),
        crate::render::backend::CursorStyle::Underline => {
            (x, y + height - thickness, width, thickness)
        }
    }
}

impl WebGpuPaneBackend {
    fn row_pixel_bounds(&self, row: usize) -> (f32, f32) {
        let fallback_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
        let fallback_top = row as f32 * fallback_h;
        let top = physical_row_boundary(row, self.metrics.cell_h, self.metrics.dpr)
            .map(|value| value as f32)
            .unwrap_or(fallback_top);
        let bottom = row
            .checked_add(1)
            .and_then(|next| physical_row_boundary(next, self.metrics.cell_h, self.metrics.dpr))
            .map(|value| value as f32)
            .unwrap_or(fallback_top + fallback_h);
        (top, bottom)
    }

    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        // Use the same Host-font Swash metrics as the WebGPU glyph atlas so
        // pane fitting and painted advances cannot drift.
        self.ctx
            .borrow_mut()
            .rasterizer
            .measure(font_family, font_size_px)
    }

    fn requires_full_frame(&self) -> bool {
        // The compositor-owned frame store preserves unchanged rows. Only
        // structural/renderer invalidation needs a full grid encode; ordinary
        // frames repair and redraw the rows selected by Renderer hashes.
        let _present_fast = present_fast();
        self.needs_initial_clear
    }

    fn supports_scroll_copy(&self) -> bool {
        !self.viewport.is_empty() && self.host.borrow().is_frame_open()
    }

    fn scroll_rows(&mut self, scroll: ScrollOp, metrics: FrameMetrics) -> ScrollCopyResult {
        self.host
            .borrow_mut()
            .scroll_pane(self.viewport, scroll, metrics.cell_h, metrics.dpr)
    }

    fn on_full_invalidate(&mut self) {
        // Renderer signalled a renderer-side full-redraw condition
        // (first frame, scroll offset change, selection toggle,
        // snapshot growth). Switch the next frame back to `LoadOp::Clear`
        // so the new row→content mapping doesn't paint over stale
        // background pixels left from the previous mapping.
        self.needs_initial_clear = true;
    }

    fn resize_surface(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), String> {
        // Phase B: pane no longer owns its own surface. We record the
        // pane's WIDTH × HEIGHT here (in device pixels) and let JS
        // separately drive the (x, y) host-canvas offset through
        // `set_viewport_offset` whenever the splitter / window layout
        // moves the container. The host's own surface.configure runs
        // via `SurfaceHost::resize`, called from
        // `manager.ts::resizeHost()` on the host-parent ResizeObserver.
        let backing_w = ((width_css as f32) * dpr).round().max(1.0) as u32;
        let backing_h = ((height_css as f32) * dpr).round().max(1.0) as u32;
        if self.viewport.w != backing_w || self.viewport.h != backing_h {
            self.viewport.w = backing_w;
            self.viewport.h = backing_h;
            // Resize re-flows the row→content mapping; the renderer's
            // tick logic relies on `requires_full_frame()` returning
            // true here so every visible row is re-encoded against the
            // new dimensions on the next frame. The host pane backend
            // also asks the host to clear (via JS
            // `surfaceHost.invalidate()` after a settled fit) so the
            // pane's old pixels don't bleed past its new scissor.
            self.needs_initial_clear = true;
        }
        Ok(())
    }

    fn invalidate_atlas(&mut self) {
        // Drop every cached glyph + reset next-free-layer + bump
        // generation. Per-pane bind groups will rebuild on their next
        // `begin_frame` via the generation-mismatch check. This is the
        // "atlas rebuild from scratch" path used after font changes
        // (handled inside `set_font_config`) or explicit resets.
        self.ctx.borrow_mut().invalidate_atlas();
        // Atlas rebuilding doesn't touch the swap-chain texture, but
        // the next frame is functionally a full repaint (every glyph
        // re-rasterizes) — keep `LoadOp::Clear` for that one frame so
        // stale pixels from the prior atlas can't show through any
        // sub-pixel anti-alias gaps.
        self.needs_initial_clear = true;
    }

    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme) {
        self.metrics = metrics;
        self.theme = theme.clone();
        self.pending_instances.clear();
        self.glyph_frame_cache.clear();
        self.damaged_rows.clear();

        // Compute slot dims from current metrics BEFORE taking ctx
        // borrow — `slot_dims_for` is a static helper, no ctx access.
        let (need_w, need_h) =
            GpuContext::slot_dims_for(self.metrics.cell_w, self.metrics.cell_h, self.metrics.dpr);

        let mut ctx = self.ctx.borrow_mut();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&ctx.font_family, &mut hasher);
        self.font_family_hash = std::hash::Hasher::finish(&hasher);
        self.font_size_q = (ctx.font_size_px * 100.0).round() as u16;

        // 1) Atlas slot growth — only ever grows. Shrinking on small
        //    metric jiggles would thrash the Swash slot allocation and
        //    re-rasterize every glyph.
        if need_w > ctx.slot_w || need_h > ctx.slot_h {
            ctx.slot_w = need_w.max(ctx.slot_w);
            ctx.slot_h = need_h.max(ctx.slot_h);
            // Best-effort rebuild. On failure we keep the old (now
            // undersized) atlas — wide glyphs continue to clip but
            // the renderer doesn't crash. `rebuild_atlas` itself
            // bumps `atlas_generation`.
            let _ = ctx.rebuild_atlas();
        }

        // 2) Bind-group invalidation — another pane may have rebuilt
        //    or invalidated the atlas since our last frame, leaving our
        //    `bind_group` referencing the old `atlas_view`. Rebuild
        //    against the new view before `draw_row` touches anything.
        if ctx.atlas_generation != self.atlas_generation_seen {
            self.bind_group = Rc::new(ctx.build_bind_group(&self.frame_uniform));
            self.atlas_generation_seen = ctx.atlas_generation;
            // Cross-pane safety: another pane just reallocated the
            // shared atlas. Our prior cached pixels reference glyph UVs
            // that no longer exist — seed bg this frame instead of
            // `LoadOp::Load`-ing over visually-correct-but-now-stale
            // pixels.
            self.needs_initial_clear = true;
        }

        // 3) Reset frame_pinned — defensive sync with atlas_layers, then
        //    blanket false. `rebuild_atlas` doesn't change layer count
        //    so the length sync only fires if some future code path
        //    grows it; cost-free on the common path.
        let needed_len = ctx.atlas_layers as usize;
        if self.frame_pinned.len() != needed_len {
            self.frame_pinned = vec![false; needed_len];
        } else {
            for p in &mut self.frame_pinned {
                *p = false;
            }
        }
    }

    fn clear(&mut self) {
        // Draw a full-viewport opaque background quad so the pane
        // controls its own clear colour independently of the shared
        // SurfaceHost's LoadOp::Clear.  When `tui_mode` is active the
        // quad uses `theme.tui_bg` instead of `theme.bg`, preventing
        // the theme accent background from polluting TUI apps.
        //
        // §wallpaper-fix: 壁纸激活且非 TUI 时，跳过这块不透明 seed。
        // `SurfaceHost::begin_frame` 本帧已在所有 pane 之下铺满整屏壁纸
        // quad；这里再压一块不透明 theme.bg 全视口 quad 会把 cell 区域的壁纸
        // 整个盖掉，只剩 scissor 之外的 pane padding 透出壁纸 —— 正是「底部
        // 一条壁纸、其余纯色」的现象。普通 shell 模式下默认背景单元已被
        // `resolve_cell_colors` 解析为透明 [0,0,0,0]，字形与显式着色单元仍
        // 正常叠加在壁纸之上。TUI 模式保留不透明 tui_bg seed：全屏 TUI 应用
        // 独占画面、需要纯色背景，不应透出壁纸。
        if !self.metrics.tui_mode && self.ctx.borrow().has_wallpaper() {
            return;
        }
        let bg_color = if self.metrics.tui_mode {
            self.theme.tui_bg
        } else {
            self.theme.bg
        };
        self.pending_instances.push(CellInstance {
            cell_xy: [0.0, 0.0],
            cell_size: [self.viewport.w as f32, self.viewport.h as f32],
            atlas_uv: [0.0, 0.0, 0.0, 0.0],
            atlas_layer: 0,
            fg_rgba: rgba_u8_to_f32(bg_color),
            bg_rgba: rgba_u8_to_f32(bg_color),
            is_color: 0,
        });
    }

    fn draw_row_backgrounds(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let row_idx = row.row_index;
        if self.damaged_rows.last().copied() != Some(row_idx as u32) {
            self.damaged_rows.push(row_idx as u32);
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let (pixel_y, pixel_y_bot) = self.row_pixel_bounds(row_idx);
        let row_h_int = pixel_y_bot - pixel_y;
        let tui_mode = self.metrics.tui_mode;
        let theme = &self.theme;

        // Consume tracking: columns consumed by a preceding wide cell's
        // grid allocation have their bg covered — we skip them to
        // prevent an independent bg from visually cutting the emoji.
        let render_path = scan_line_path(row.cells, row.clusters);
        let mut consume_until: usize = 0;
        let mut background_run: Option<(usize, usize, [u8; 4], [u8; 4])> = None;

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                flush_background_run(
                    &mut self.pending_instances,
                    &mut background_run,
                    cell_w,
                    pixel_y,
                    row_h_int,
                );
                continue;
            }

            if col < consume_until {
                flush_background_run(
                    &mut self.pending_instances,
                    &mut background_run,
                    cell_w,
                    pixel_y,
                    row_h_int,
                );
                continue;
            }

            let (_attrs, fg, bg) =
                crate::render::backend::resolve_cell_colors(cell, attrs_table, &theme, tui_mode);

            let cell_span = cell.width.max(1) as usize;

            // Normal shell default backgrounds are transparent: the damaged
            // row was already repaired by SurfaceHost, so emitting one quad
            // per cell only burns CPU and instance bandwidth.
            if tui_mode || bg[3] != 0 {
                if cell_span == 1 {
                    let extends = matches!(
                        background_run.as_ref(),
                        Some((_, end_col, run_fg, run_bg))
                            if *end_col == col && *run_fg == fg && *run_bg == bg
                    );
                    if extends {
                        if let Some((_, end_col, _, _)) = background_run.as_mut() {
                            *end_col = col + 1;
                        }
                    } else {
                        flush_background_run(
                            &mut self.pending_instances,
                            &mut background_run,
                            cell_w,
                            pixel_y,
                            row_h_int,
                        );
                        background_run = Some((col, col + 1, fg, bg));
                    }
                } else {
                    flush_background_run(
                        &mut self.pending_instances,
                        &mut background_run,
                        cell_w,
                        pixel_y,
                        row_h_int,
                    );
                    push_background_instance(
                        &mut self.pending_instances,
                        col,
                        col + cell_span,
                        cell_w,
                        pixel_y,
                        row_h_int,
                        fg,
                        bg,
                    );
                }
            } else {
                flush_background_run(
                    &mut self.pending_instances,
                    &mut background_run,
                    cell_w,
                    pixel_y,
                    row_h_int,
                );
            }

            if render_path == RenderPath::Slow && cell_span > 1 {
                consume_until = col + cell_span;
            }
        }

        flush_background_run(
            &mut self.pending_instances,
            &mut background_run,
            cell_w,
            pixel_y,
            row_h_int,
        );
    }

    fn admit_glyph(
        &mut self,
        key: GlyphKey,
        glyph_text: &str,
        style_flags: u8,
    ) -> Option<GlyphEntry> {
        let entry = {
            let mut ctx = self.ctx.borrow_mut();
            match ctx.atlas.lookup(&key) {
                Some(entry) => Some(entry),
                None => ctx
                    .rasterize_and_admit(
                        key,
                        glyph_text,
                        self.metrics.dpr,
                        style_flags,
                        &self.frame_pinned,
                    )
                    .ok(),
            }
        }?;
        self.mark_glyph_used(entry);
        Some(entry)
    }

    /// Pin and mark an atlas layer once per pane frame. The local pin bitmap
    /// already resets at `begin_frame`, so repeated cells using one glyph can
    /// avoid borrowing the shared context just to rewrite `frame_written`.
    fn mark_glyph_used(&mut self, entry: GlyphEntry) {
        let layer = entry.layer as usize;
        if layer < self.frame_pinned.len() {
            if self.frame_pinned[layer] {
                return;
            }
            self.frame_pinned[layer] = true;
        }
        let mut ctx = self.ctx.borrow_mut();
        if layer < ctx.frame_written.len() {
            ctx.frame_written[layer] = true;
        }
    }

    fn admit_cached_glyph(
        &mut self,
        key: GlyphKey,
        glyph_text: &str,
        style_flags: u8,
    ) -> Option<GlyphEntry> {
        if let Some(entry) = self.glyph_frame_cache.get(&key).copied() {
            if let Some(entry) = entry {
                self.mark_glyph_used(entry);
            }
            return entry;
        }
        let entry = self.admit_glyph(key, glyph_text, style_flags);
        self.glyph_frame_cache.insert(key, entry);
        entry
    }

    fn should_skip_cell(cell: &crate::term::cell::Cell) -> bool {
        cell.width == 0 || (cell.ch == ' ' && cell.attr == crate::term::attr_table::AttrId::DEFAULT)
    }

    fn cluster_text_for<'a>(
        row: &'a RowDraw<'a>,
        render_path: RenderPath,
        col: usize,
    ) -> Option<&'a str> {
        if render_path == RenderPath::Fast || row.clusters.is_empty() {
            return None;
        }
        let target = col.min(u16::MAX as usize) as u16;
        row.clusters
            .binary_search_by_key(&target, |cluster| cluster.col)
            .ok()
            .map(|index| row.clusters[index].text.as_ref())
    }

    fn glyph_style_flags(flags: crate::term::attrs::Flags) -> u8 {
        let mut style = 0;
        if flags.contains(crate::term::attrs::Flags::BOLD) {
            style |= GlyphKey::STYLE_BOLD;
        }
        if flags.contains(crate::term::attrs::Flags::ITALIC) {
            style |= GlyphKey::STYLE_ITALIC;
        }
        style
    }

    fn append_atlas_glyph(
        instances: &mut Vec<CellInstance>,
        entry: Option<GlyphEntry>,
        pixel_x: f32,
        pixel_y: f32,
        allocation_w: f32,
        allocation_h: f32,
        emoji_em_px: f32,
        fg: [u8; 4],
    ) {
        if let Some(entry) = entry {
            let (cell_xy, cell_size, atlas_uv) = glyph_quad_geometry_for_cell(
                pixel_x,
                pixel_y,
                &entry,
                allocation_w,
                allocation_h,
                emoji_em_px,
            );
            instances.push(CellInstance {
                cell_xy,
                cell_size,
                atlas_uv,
                atlas_layer: entry.layer as u32,
                fg_rgba: rgba_u8_to_f32(fg),
                bg_rgba: [0.0, 0.0, 0.0, 0.0],
                is_color: u32::from(entry.is_color),
            });
        }
    }

    fn draw_row_texts(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let row_idx = row.row_index;
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let (pixel_y, pixel_y_bot) = self.row_pixel_bounds(row_idx);
        let cell_h = pixel_y_bot - pixel_y;
        let emoji_em_px = self.emoji_em_px();
        let tui_mode = self.metrics.tui_mode;
        let (font_family_hash, font_size_q) = self.font_key();

        let render_path = scan_line_path(row.cells, row.clusters);

        for (col, cell) in row.cells.iter().enumerate() {
            if Self::should_skip_cell(cell) {
                continue;
            }

            let attrs = attrs_table.get(cell.attr);
            let (_attrs, fg, _bg) = {
                let theme = &self.theme;
                crate::render::backend::resolve_cell_colors(cell, attrs_table, theme, tui_mode)
            };

            // Pixel-aligned positions — floor(pos + 0.5) prevents sub-pixel
            // seams between adjacent cells that would show as hairline gaps.
            let pixel_x = (col as f32 * cell_w + 0.5).floor();
            let cell_span = usize::from(cell.width.max(1));
            let pixel_x_right = ((col + cell_span) as f32 * cell_w + 0.5).floor();

            // ── Fast-path skip: for pure ASCII lines, no cluster lookup
            // is needed. This avoids the linear scan through `row.clusters`
            // and the per-cell char encoding for the common case of code
            // and log output, keeping the tight loop minimal.
            let cluster_text = Self::cluster_text_for(row, render_path, col);
            let mut ch_buf = [0u8; 4];
            let glyph_text: &str = match cluster_text {
                Some(text) => text,
                None => cell.ch.encode_utf8(&mut ch_buf),
            };

            let style_flags = Self::glyph_style_flags(attrs.flags);
            let glyph_id = glyph_id_for(cluster_text, cell.ch);
            let key = GlyphKey::new(
                font_family_hash,
                font_size_q,
                glyph_id,
                style_flags,
                self.metrics.dpr * ATLAS_SUPERSAMPLE as f32,
            );
            let entry = self.admit_cached_glyph(key, glyph_text, style_flags);
            Self::append_atlas_glyph(
                &mut self.pending_instances,
                entry,
                pixel_x,
                pixel_y,
                (pixel_x_right - pixel_x).max(1.0),
                cell_h,
                emoji_em_px,
                fg,
            );
        }
    }

    fn draw_cursor(&mut self, cursor: &CursorDraw, _attrs_table: &AttrTable) {
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let effective_col = cursor.col as f64;
        let pixel_x = (effective_col as f32 * cell_w + 0.5).floor();
        let cursor_span = cursor.width.max(1) as usize;

        let effective_span = self.cursor_span(cursor, cursor_span, cell_w);

        let effective_span_f = effective_span as f64;
        let pixel_x_right = ((effective_col + effective_span_f) as f32 * cell_w + 0.5).floor();
        let span_w = pixel_x_right - pixel_x;

        let (pixel_y, pixel_y_bot) = self.row_pixel_bounds(cursor.row);
        let cell_h_int = pixel_y_bot - pixel_y;
        let bar_thickness = (2.0 * self.metrics.dpr).round().max(1.0);

        let (block_x, block_y, block_w, block_h) = cursor_geometry(
            cursor.style,
            pixel_x,
            pixel_y,
            span_w,
            cell_h_int,
            bar_thickness,
        );
        let cursor_color = rgba_u8_to_f32(self.theme.cursor_color);
        self.pending_instances.push(CellInstance {
            cell_xy: [block_x, block_y],
            cell_size: [block_w, block_h],
            atlas_uv: [0.0, 0.0, 0.0, 0.0],
            atlas_layer: 0,
            fg_rgba: cursor_color,
            bg_rgba: cursor_color,
            is_color: 0,
        });

        self.draw_cursor_glyph(
            cursor,
            effective_col,
            cell_w,
            span_w,
            pixel_y,
            cell_h_int,
            cursor_color,
        );
    }
}

impl WebGpuPaneBackend {
    fn cursor_span(&mut self, cursor: &CursorDraw, requested: usize, cell_w: f32) -> usize {
        if requested < 2 {
            return requested;
        }
        let (font_family_hash, font_size_q) = self.font_key();
        let key = GlyphKey::new(
            font_family_hash,
            font_size_q,
            cursor_glyph_id(cursor),
            0,
            self.metrics.dpr * ATLAS_SUPERSAMPLE as f32,
        );
        self.ctx
            .borrow_mut()
            .atlas
            .lookup(&key)
            .map(|entry| {
                if entry.is_color {
                    requested
                } else {
                    ((entry.px_w as f32).max(1.0) / cell_w).ceil() as usize
                }
            })
            .unwrap_or(requested)
    }

    fn draw_cursor_glyph(
        &mut self,
        cursor: &CursorDraw,
        effective_col: f64,
        cell_w: f32,
        span_w: f32,
        pixel_y: f32,
        cell_h: f32,
        cursor_color: [f32; 4],
    ) {
        use crate::render::backend::CursorStyle;

        if !matches!(cursor.style, CursorStyle::Block) || cursor.ch == ' ' {
            return;
        }
        let (font_family_hash, font_size_q) = self.font_key();
        let glyph_id = cursor_glyph_id(cursor);
        let key = GlyphKey::new(
            font_family_hash,
            font_size_q,
            glyph_id,
            0,
            self.metrics.dpr * ATLAS_SUPERSAMPLE as f32,
        );
        let entry = self.lookup_cursor_glyph(&key);
        let Some(entry) = entry else {
            return;
        };
        if (entry.layer as usize) < self.frame_pinned.len() {
            self.frame_pinned[entry.layer as usize] = true;
        }
        let gx = (effective_col as f32 * cell_w + 0.5).floor();
        let (cell_xy, cell_size, atlas_uv) = glyph_quad_geometry_for_cell(
            gx,
            pixel_y,
            &entry,
            span_w.max(1.0),
            cell_h,
            self.emoji_em_px(),
        );
        self.pending_instances.push(CellInstance {
            cell_xy,
            cell_size,
            atlas_uv,
            atlas_layer: entry.layer as u32,
            fg_rgba: rgba_u8_to_f32(self.theme.cursor_text_color),
            bg_rgba: cursor_color,
            is_color: if entry.is_color { 1 } else { 0 },
        });
    }

    fn font_key(&self) -> (u64, u16) {
        (self.font_family_hash, self.font_size_q)
    }

    fn emoji_em_px(&self) -> f32 {
        ((self.font_size_q as f32 / 100.0) * self.metrics.dpr)
            .round()
            .max(1.0)
    }

    fn lookup_cursor_glyph(&mut self, key: &GlyphKey) -> Option<GlyphEntry> {
        let mut ctx = self.ctx.borrow_mut();
        let entry = ctx.atlas.lookup(key);
        if let Some(glyph) = entry {
            if (glyph.layer as usize) < ctx.frame_written.len() {
                ctx.frame_written[glyph.layer as usize] = true;
            }
            Some(glyph)
        } else {
            None
        }
    }
}

impl WebGpuPaneBackend {
    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]) {
        if rects.is_empty() {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let sel_color = rgba_u8_to_f32(self.theme.selection_bg);
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let pixel_x = (col_start as f32 * cell_w + 0.5).floor();
            let pixel_x_right = (col_end as f32 * cell_w + 0.5).floor();
            let width = pixel_x_right - pixel_x;
            let (pixel_y, pixel_y_bot) = self.row_pixel_bounds(row);
            let height = pixel_y_bot - pixel_y;
            self.pending_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [width, height],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: sel_color,
                bg_rgba: sel_color,
                // Selection is a solid translucent rectangle, not an atlas glyph.
                is_color: INSTANCE_MODE_SOLID,
            });
        }
    }

    fn draw_preedit_overlay(
        &mut self,
        text: &str,
        row: usize,
        col: usize,
        theme: &crate::render::backend::Theme,
    ) {
        if text.is_empty() {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let (pixel_y, pixel_y_bot) = self.row_pixel_bounds(row);
        let cell_h = pixel_y_bot - pixel_y;
        // CJK chars from candidate previews can be wide; ASCII pinyin is
        // narrow. Cheap heuristic: codepoint < 0x80 → 1 cell, otherwise
        // 2 cells. Correct for the IME-preedit use case (pinyin + Chinese
        // candidates); doesn't try to handle every CJK / emoji edge.
        let char_widths: Vec<(char, u8)> = text
            .chars()
            .map(|c| (c, if (c as u32) < 0x80 { 1u8 } else { 2u8 }))
            .collect();
        let total_cells: usize = char_widths.iter().map(|(_, w)| *w as usize).sum();
        if total_cells == 0 {
            return;
        }
        let pixel_x_start = (col as f32 * cell_w + 0.5).floor();
        let pixel_x_end = ((col + total_cells) as f32 * cell_w + 0.5).floor();
        let total_width = pixel_x_end - pixel_x_start;

        // 1) Opaque background quad to cover the cells we're overlaying.
        //    Uses theme.bg so the preedit looks like fresh blank cells
        //    even though the underlying kernel cells are unchanged.
        let bg_color = rgba_u8_to_f32(theme.bg);
        self.pending_instances.push(CellInstance {
            cell_xy: [pixel_x_start, pixel_y],
            cell_size: [total_width, cell_h],
            atlas_uv: [0.0, 0.0, 0.0, 0.0],
            atlas_layer: 0,
            fg_rgba: bg_color,
            bg_rgba: bg_color,
            is_color: 0,
        });

        // 2) Glyphs. Reuse the standard atlas / rasterize path.
        let fg_color = rgba_u8_to_f32(theme.fg);
        let (font_family_hash, font_size_q) = {
            let ctx = self.ctx.borrow();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&ctx.font_family, &mut h);
            (
                std::hash::Hasher::finish(&h),
                (ctx.font_size_px * 100.0).round() as u16,
            )
        };
        self.draw_preedit_glyphs(
            &char_widths,
            col,
            cell_w,
            pixel_y,
            cell_h,
            fg_color,
            font_family_hash,
            font_size_q,
        );

        // 3) Underline — IME preedit convention. 1 device-px tall, bottom
        //    of the cell row.
        let underline_thickness = (1.0 * self.metrics.dpr).round().max(1.0);
        let underline_y = pixel_y + cell_h - underline_thickness;
        self.pending_instances.push(CellInstance {
            cell_xy: [pixel_x_start, underline_y],
            cell_size: [total_width, underline_thickness],
            atlas_uv: [0.0, 0.0, 0.0, 0.0],
            atlas_layer: 0,
            fg_rgba: fg_color,
            bg_rgba: fg_color,
            is_color: 0,
        });
    }
}

impl WebGpuPaneBackend {
    fn draw_preedit_glyphs(
        &mut self,
        char_widths: &[(char, u8)],
        col: usize,
        cell_w: f32,
        pixel_y: f32,
        cell_h: f32,
        fg_color: [f32; 4],
        font_family_hash: u64,
        font_size_q: u16,
    ) {
        let mut cell_offset = 0usize;
        for (ch, width) in char_widths {
            let key = GlyphKey::new(
                font_family_hash,
                font_size_q,
                *ch as u32,
                0,
                self.metrics.dpr * ATLAS_SUPERSAMPLE as f32,
            );
            let glyph = ch.to_string();
            let entry = {
                let mut ctx = self.ctx.borrow_mut();
                match ctx.atlas.lookup(&key) {
                    Some(entry) => {
                        if (entry.layer as usize) < ctx.frame_written.len() {
                            ctx.frame_written[entry.layer as usize] = true;
                        }
                        Some(entry)
                    }
                    None => ctx
                        .rasterize_and_admit(key, &glyph, self.metrics.dpr, 0, &self.frame_pinned)
                        .ok(),
                }
            };
            if let Some(entry) = entry {
                if (entry.layer as usize) < self.frame_pinned.len() {
                    self.frame_pinned[entry.layer as usize] = true;
                }
                let start_col = col + cell_offset;
                let end_col = start_col + usize::from((*width).max(1));
                let pixel_x = (start_col as f32 * cell_w + 0.5).floor();
                let pixel_x_right = (end_col as f32 * cell_w + 0.5).floor();
                let (cell_xy, cell_size, atlas_uv) = glyph_quad_geometry_for_cell(
                    pixel_x,
                    pixel_y,
                    &entry,
                    (pixel_x_right - pixel_x).max(1.0),
                    cell_h,
                    self.emoji_em_px(),
                );
                self.pending_instances.push(CellInstance {
                    cell_xy,
                    cell_size,
                    atlas_uv,
                    atlas_layer: entry.layer as u32,
                    fg_rgba: fg_color,
                    bg_rgba: [0.0, 0.0, 0.0, 0.0],
                    is_color: if entry.is_color { 1 } else { 0 },
                });
            }
            cell_offset += *width as usize;
        }
    }
}

impl WebGpuPaneBackend {
    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        if rects.is_empty() {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let thickness = (2.0 * self.metrics.dpr).round().max(1.0);
        let link_color = rgba_u8_to_f32(self.theme.hyperlink_color);
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let pixel_x = (col_start as f32 * cell_w + 0.5).floor();
            let pixel_x_right = (col_end as f32 * cell_w + 0.5).floor();
            let width = pixel_x_right - pixel_x;
            let (_, pixel_y_bot) = self.row_pixel_bounds(row);
            let pixel_y = pixel_y_bot - thickness;
            self.pending_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [width, thickness],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: link_color,
                bg_rgba: link_color,
                is_color: 0,
            });
        }
    }

    fn draw_history_overlay(
        &mut self,
        overlay: &crate::render::renderer::HistoryOverlay,
        theme: &crate::render::backend::Theme,
    ) {
        // §1.34 — wasm-side shell-history popup. Mirror of preedit:
        // one cell row per item; panel width = widest item (capped);
        // selected row inverts bg/fg; 1-device-px border.
        // §1.35 — added cell padding (H_PAD_CELLS / V_PAD_CELLS) for
        // visual breathing room around content and selection highlight.
        let requested_visible = overlay.items.len().min(overlay.max_visible_rows);
        if requested_visible == 0 {
            return;
        }
        let cell_w = (self.metrics.cell_w * self.metrics.dpr).max(1.0);
        let cell_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);

        let normalised: Vec<String> = overlay
            .items
            .iter()
            .take(requested_visible)
            .map(|s| history_text(s))
            .collect();
        let row_widths_cells: Vec<usize> =
            normalised.iter().map(|s| history_text_width(s)).collect();
        let widest_cells = row_widths_cells.iter().copied().max().unwrap_or(0);
        let Some(geometry) =
            history_overlay_geometry(overlay, widest_cells, requested_visible, cell_w, cell_h)
        else {
            return;
        };
        let visible_count = geometry.visible_count;
        let panel_cells_w = geometry.content_cols;
        let needs_scrollbar = overlay.total_items > visible_count && geometry.scrollbar_w > 0.0;
        let sb_w = geometry.scrollbar_w;
        let panel_w = geometry.panel_w;
        let panel_h = geometry.panel_h;
        let panel_x = geometry.panel_x;
        let panel_y_top = geometry.panel_y;
        let pad_w = geometry.pad_w;
        let pad_h = geometry.pad_h;

        let inner_x = panel_x + pad_w;
        let inner_y = panel_y_top + pad_h;

        let bg = rgba_u8_to_f32(history_overlay_surface(theme.bg, theme.fg));
        let fg = rgba_u8_to_f32(theme.fg);

        // 1) Panel background.
        self.pending_instances.push(CellInstance {
            cell_xy: [panel_x, panel_y_top],
            cell_size: [panel_w, panel_h],
            atlas_uv: [0.0, 0.0, 0.0, 0.0],
            atlas_layer: 0,
            fg_rgba: bg,
            bg_rgba: bg,
            is_color: 0,
        });

        // 2) Selected-row highlight (inverse).
        if overlay.selected_index >= 0 && (overlay.selected_index as usize) < visible_count {
            let sel_y = inner_y + (overlay.selected_index as f32) * cell_h;
            self.pending_instances.push(CellInstance {
                cell_xy: [inner_x, sel_y],
                cell_size: [panel_w - 2.0 * pad_w, cell_h],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: fg,
                bg_rgba: fg,
                is_color: 0,
            });
        }

        // 3) Glyphs.
        let (font_family_hash, font_size_q) = {
            let ctx = self.ctx.borrow();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&ctx.font_family, &mut h);
            (
                std::hash::Hasher::finish(&h),
                (ctx.font_size_px * 100.0).round() as u16,
            )
        };
        self.draw_history_glyphs(
            &normalised,
            visible_count,
            overlay.selected_index as isize,
            inner_x,
            inner_y,
            panel_cells_w,
            cell_w,
            cell_h,
            bg,
            fg,
            font_family_hash,
            font_size_q,
        );

        // 4) 1-device-px border.
        let bw = 1.0_f32;
        for (x, y, w, h) in [
            (panel_x, panel_y_top, panel_w, bw),
            (panel_x, panel_y_top + panel_h - bw, panel_w, bw),
            (panel_x, panel_y_top, bw, panel_h),
            (panel_x + panel_w - bw, panel_y_top, bw, panel_h),
        ] {
            self.pending_instances.push(CellInstance {
                cell_xy: [x, y],
                cell_size: [w, h],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: fg,
                bg_rgba: fg,
                is_color: 0,
            });
        }

        // 5) §history-scroll — scrollbar track + thumb (Warp-style position
        // indicator). Opaque colors mixed from bg→fg so no alpha-blend
        // dependency. Drawn in the reserved right strip (see `sb_w`/`sb_gap`).
        if needs_scrollbar && overlay.total_items > 0 {
            let mix = |t: f32| {
                [
                    bg[0] * (1.0 - t) + fg[0] * t,
                    bg[1] * (1.0 - t) + fg[1] * t,
                    bg[2] * (1.0 - t) + fg[2] * t,
                    1.0,
                ]
            };
            let sb_x = panel_x + panel_w - sb_w - bw;
            let track_y = panel_y_top + bw;
            let track_h = (panel_h - 2.0 * bw).max(1.0);
            let total = overlay.total_items as f32;
            let frac_start = (overlay.first_visible as f32 / total).clamp(0.0, 1.0);
            let frac_len = (visible_count as f32 / total).clamp(0.0, 1.0);
            let min_thumb = (track_h * 0.10).clamp(10.0, track_h);
            let thumb_h = (frac_len * track_h).max(min_thumb).min(track_h);
            let mut thumb_y = track_y + frac_start * track_h;
            if thumb_y + thumb_h > track_y + track_h {
                thumb_y = track_y + track_h - thumb_h;
            }
            for (y, h, t) in [(track_y, track_h, 0.18_f32), (thumb_y, thumb_h, 0.55_f32)] {
                self.pending_instances.push(CellInstance {
                    cell_xy: [sb_x, y],
                    cell_size: [sb_w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    atlas_layer: 0,
                    fg_rgba: mix(t),
                    bg_rgba: mix(t),
                    is_color: 0,
                });
            }
        }
    }
}

impl WebGpuPaneBackend {
    fn draw_history_glyphs(
        &mut self,
        rows: &[String],
        visible_count: usize,
        selected_index: isize,
        inner_x: f32,
        inner_y: f32,
        panel_cells_w: usize,
        cell_w: f32,
        cell_h: f32,
        bg: [f32; 4],
        fg: [f32; 4],
        font_family_hash: u64,
        font_size_q: u16,
    ) {
        for (row_index, text) in rows.iter().take(visible_count).enumerate() {
            let row_y = inner_y + row_index as f32 * cell_h;
            let glyph_color = if selected_index >= 0 && row_index == selected_index as usize {
                bg
            } else {
                fg
            };
            self.draw_history_row(
                text,
                row_y,
                inner_x,
                panel_cells_w,
                cell_w,
                cell_h,
                glyph_color,
                font_family_hash,
                font_size_q,
            );
        }
    }

    fn draw_history_row(
        &mut self,
        text: &str,
        row_y: f32,
        inner_x: f32,
        panel_cells_w: usize,
        cell_w: f32,
        cell_h: f32,
        glyph_color: [f32; 4],
        font_family_hash: u64,
        font_size_q: u16,
    ) {
        let mut cell_offset = 0usize;
        for ch in text.chars() {
            let ch_width = if (ch as u32) < 0x80 { 1 } else { 2 };
            if cell_offset + ch_width > panel_cells_w {
                break;
            }
            self.draw_history_char(
                ch,
                row_y,
                inner_x + cell_offset as f32 * cell_w,
                ch_width as f32 * cell_w,
                cell_h,
                glyph_color,
                font_family_hash,
                font_size_q,
            );
            cell_offset += ch_width;
        }
    }

    fn draw_history_char(
        &mut self,
        ch: char,
        row_y: f32,
        pixel_x: f32,
        allocation_w: f32,
        allocation_h: f32,
        glyph_color: [f32; 4],
        font_family_hash: u64,
        font_size_q: u16,
    ) {
        let key = GlyphKey::new(
            font_family_hash,
            font_size_q,
            ch as u32,
            0,
            self.metrics.dpr * ATLAS_SUPERSAMPLE as f32,
        );
        let glyph = ch.to_string();
        let entry = {
            let mut ctx = self.ctx.borrow_mut();
            match ctx.atlas.lookup(&key) {
                Some(entry) => {
                    if (entry.layer as usize) < ctx.frame_written.len() {
                        ctx.frame_written[entry.layer as usize] = true;
                    }
                    Some(entry)
                }
                None => ctx
                    .rasterize_and_admit(key, &glyph, self.metrics.dpr, 0, &self.frame_pinned)
                    .ok(),
            }
        };
        let Some(entry) = entry else {
            return;
        };
        if (entry.layer as usize) < self.frame_pinned.len() {
            self.frame_pinned[entry.layer as usize] = true;
        }
        let (cell_xy, cell_size, atlas_uv) = glyph_quad_geometry_for_cell(
            pixel_x,
            row_y,
            &entry,
            allocation_w,
            allocation_h,
            self.emoji_em_px(),
        );
        self.pending_instances.push(CellInstance {
            cell_xy,
            cell_size,
            atlas_uv,
            atlas_layer: entry.layer as u32,
            fg_rgba: glyph_color,
            bg_rgba: [0.0, 0.0, 0.0, 0.0],
            is_color: if entry.is_color { 1 } else { 0 },
        });
    }
}

impl WebGpuPaneBackend {
    fn background_damage_rects(&mut self) -> Vec<ScissorRect> {
        if self.metrics.tui_mode || self.damaged_rows.is_empty() {
            return Vec::new();
        }
        self.damaged_rows.sort_unstable();
        self.damaged_rows.dedup();
        let viewport = self.viewport;
        let rect_for_rows = |start: u32, end: u32| {
            let fallback_h = (self.metrics.cell_h * self.metrics.dpr).round().max(1.0);
            let top = physical_row_boundary(start as usize, self.metrics.cell_h, self.metrics.dpr)
                .unwrap_or_else(|| (start as f32 * fallback_h).round().max(0.0) as u32);
            let bottom = physical_row_boundary(end as usize, self.metrics.cell_h, self.metrics.dpr)
                .unwrap_or_else(|| (end as f32 * fallback_h).round().max(0.0) as u32);
            let clipped_top = top.min(viewport.h);
            let clipped_bottom = bottom.min(viewport.h);
            ScissorRect {
                x: viewport.x,
                y: viewport.y.saturating_add(clipped_top),
                w: viewport.w,
                h: clipped_bottom.saturating_sub(clipped_top),
            }
        };
        let mut rects = Vec::new();
        let mut start = self.damaged_rows[0];
        let mut end = start.saturating_add(1);
        for row in self.damaged_rows.iter().copied().skip(1) {
            if row == end {
                end = end.saturating_add(1);
            } else {
                rects.push(rect_for_rows(start, end));
                start = row;
                end = row.saturating_add(1);
            }
        }
        rects.push(rect_for_rows(start, end));
        rects.retain(|rect| !rect.is_empty());
        rects
    }

    fn end_frame(&mut self) {
        // Phase B per-frame protocol. Steps:
        //   1. Upload frame uniform (pane-local viewport size in pixels).
        //   2. Grow instance buffer if the frame exceeded current capacity.
        //   3. Upload pending CellInstance bytes.
        //   4. Queue the pane's draw handles for the host's shared pass;
        //      the host records viewport, scissor, bind group, vertex
        //      buffer, and draw calls after all panes have uploaded data.
        //
        // No `surface.get_current_texture` / `queue.submit` /
        // `frame.present` here in Phase B — those happen once per frame
        // in `SurfaceHost::end_frame`, called by JS after iterating
        // every dirty pane.

        let n_cells = self.pending_instances.len() as u32;

        // The vertex shader divides `cell_xy` by `frame.viewport` to
        // produce NDC. With single-canvas + scissor, `cell_xy` is
        // pane-local device-pixel coords, so the uniform must hold the
        // pane's own viewport size — `host.queue_pane` then maps that
        // NDC into the pane's rect on the host canvas via
        // `pass.set_viewport(scissor)`.
        let viewport_uniform: [f32; 4] = [self.viewport.w as f32, self.viewport.h as f32, 0.0, 0.0];

        // Step 2: grow the instance buffer outside any ctx borrow so
        // `&mut self.instance_buffer` doesn't conflict with a live
        // `ctx.borrow()`.
        if n_cells > self.instance_capacity {
            let new_capacity = n_cells.next_power_of_two().max(self.instance_capacity * 2);
            let new_buffer = self
                .ctx
                .borrow()
                .device
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ridge-instance-buffer-grown"),
                    size: (new_capacity as u64) * CELL_INSTANCE_STRIDE,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            self.instance_buffer = Rc::new(new_buffer);
            self.instance_capacity = new_capacity;
            // bind_group references frame_uniform + atlas_view +
            // sampler — instance buffer is bound per-frame via
            // `set_vertex_buffer` below, so no rebuild needed here.
        }

        // Step 1 + 3: write uniform + instance bytes via the shared
        // queue. Borrow scoped tight so the `host.borrow_mut()` call
        // below doesn't risk nested borrows on either Rc.
        {
            let ctx = self.ctx.borrow();
            ctx.queue.write_buffer(
                &self.frame_uniform,
                0,
                bytemuck::bytes_of(&viewport_uniform),
            );
            if n_cells > 0 {
                let instance_bytes: &[u8] = bytemuck::cast_slice(&self.pending_instances);
                ctx.queue
                    .write_buffer(&self.instance_buffer, 0, instance_bytes);
            }
        }

        let background_damage = self.background_damage_rects();
        if !background_damage.is_empty() {
            self.host
                .borrow_mut()
                .repair_background_damage(&background_damage);
        }

        // Empty viewport (parked-by-clip) or no draws → skip the host
        // queue entirely. `host.queue_pane` itself short-circuits on
        // empty rect, but bailing here also avoids the `ctx.borrow()`
        // round-trip + closure capture.
        if self.viewport.is_empty() || n_cells == 0 {
            // Even with nothing to draw, we may still need to consume
            // the seed-clear flag — but the host owns the seed-clear
            // decision in Phase B (one Clear per frame, regardless of
            // which pane goes first), so just clear the per-pane flag
            // and bail.
            self.needs_initial_clear = false;
            return;
        }

        // Step 4: hand off the pane's buffer handles to the host. The shared
        // cell pipeline stays owned by GpuContext and is bound once per pane
        // draw inside the host's single frame pass.
        let viewport = self.viewport;
        self.host.borrow_mut().queue_pane(
            viewport,
            &self.bind_group,
            &self.instance_buffer,
            n_cells,
        );

        // Seed-equivalent flag consumed — `requires_full_frame` returns
        // false next tick so the row-hash diff in Renderer::tick can
        // skip non-dirty rows.
        self.needs_initial_clear = false;
        // This pane's draw is now recorded in the unsubmitted host encoder.
        // Mark every cited glyph layer committed so a sibling cannot overwrite
        // it later in the same frame. Repeated marks are intentionally cheaper
        // than building and scanning a per-pane replay cache.
        {
            let mut ctx = self.ctx.borrow_mut();
            for instance in &self.pending_instances {
                if instance.atlas_layer >= super::gpu_context::ATLAS_RESERVED_LAYERS {
                    ctx.mark_committed(instance.atlas_layer as u16);
                }
            }
        }
    }
}

impl RenderBackend for WebGpuPaneBackend {
    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        WebGpuPaneBackend::measure_font(self, font_family, font_size_px)
    }

    fn requires_full_frame(&self) -> bool {
        WebGpuPaneBackend::requires_full_frame(self)
    }

    fn resize_surface(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), String> {
        WebGpuPaneBackend::resize_surface(self, width_css, height_css, dpr)
    }

    fn invalidate_atlas(&mut self) {
        WebGpuPaneBackend::invalidate_atlas(self)
    }

    fn on_full_invalidate(&mut self) {
        WebGpuPaneBackend::on_full_invalidate(self)
    }

    fn supports_scroll_copy(&self) -> bool {
        WebGpuPaneBackend::supports_scroll_copy(self)
    }

    fn scroll_rows(&mut self, scroll: ScrollOp, metrics: FrameMetrics) -> ScrollCopyResult {
        WebGpuPaneBackend::scroll_rows(self, scroll, metrics)
    }

    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme) {
        WebGpuPaneBackend::begin_frame(self, metrics, theme)
    }

    fn clear(&mut self) {
        WebGpuPaneBackend::clear(self)
    }

    fn draw_row_backgrounds(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        WebGpuPaneBackend::draw_row_backgrounds(self, row, attrs_table)
    }

    fn draw_row_texts(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        WebGpuPaneBackend::draw_row_texts(self, row, attrs_table)
    }

    fn draw_cursor(&mut self, cursor: &CursorDraw, attrs_table: &AttrTable) {
        WebGpuPaneBackend::draw_cursor(self, cursor, attrs_table)
    }

    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]) {
        WebGpuPaneBackend::draw_selection_overlay(self, rects)
    }

    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        WebGpuPaneBackend::draw_hyperlink_underlines(self, rects)
    }

    fn draw_preedit_overlay(&mut self, text: &str, row: usize, col: usize, theme: &Theme) {
        WebGpuPaneBackend::draw_preedit_overlay(self, text, row, col, theme)
    }

    fn draw_history_overlay(
        &mut self,
        overlay: &crate::render::renderer::HistoryOverlay,
        theme: &Theme,
    ) {
        WebGpuPaneBackend::draw_history_overlay(self, overlay, theme)
    }

    fn end_frame(&mut self) {
        WebGpuPaneBackend::end_frame(self)
    }
}
