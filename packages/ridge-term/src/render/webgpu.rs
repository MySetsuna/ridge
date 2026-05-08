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
//!    invokes `host.record_pane(viewport, &pipeline, |pass| draw)` —
//!    the host opens the render pass on its shared encoder, sets
//!    viewport + scissor to clip the pane's draw to its rect on the
//!    host canvas, and lets the closure record the actual draw call.
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
//! `new` returns `Err` when [`SurfaceHost::get`](super::surface_host::SurfaceHost::get)
//! returns `None` (host never initialised — usually a JS bug or adapter
//! miss at boot) or `GpuContext::get_or_init` fails. `RenderHandle
//! ::newWithWebgpuFirst` falls back to `Canvas2dBackend` so the pane
//! never crashes; the error string is the only signal.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use std::cell::RefCell;
use std::rc::Rc;

use super::glyph_atlas::{GlyphEntry, GlyphKey};
use super::gpu_context::GpuContext;
use super::surface_host::{ScissorRect, SurfaceHost};
use crate::render::backend::{CursorDraw, FrameMetrics, RenderBackend, RowDraw, Theme};
use crate::term::attr_table::AttrTable;

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
/// return `&[u8]` without unsafe transmutes. Layout: 6 fields,
/// all f32 / u32 / [f32; N] arrays, 4-byte aligned, 68 bytes total — no
/// implicit padding so `Pod` is sound.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CellInstance {
    cell_xy: [f32; 2],   // 0..8
    cell_size: [f32; 2], // 8..16
    atlas_uv: [f32; 4],  // 16..32
    atlas_layer: u32,    // 32..36
    fg_rgba: [f32; 4],   // 36..52
    bg_rgba: [f32; 4],   // 52..68
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
    /// `host.record_pane(viewport, &pipeline, |pass| draw)` so all panes
    /// composite into one render pass per frame on the global host
    /// canvas. Never `borrow_mut`'d while `ctx` is borrowed (host's
    /// `record_pane` itself takes a fresh `ctx.borrow()` inside).
    host: Rc<RefCell<SurfaceHost>>,
    /// Last `ctx.atlas_generation` this pane built `bind_group` against.
    /// When `begin_frame` sees a higher value it rebuilds the bind
    /// group so the next `draw_row` samples the new `atlas_view`.
    atlas_generation_seen: u64,
    /// Pane's rectangle on the host canvas in **device pixels**.
    /// `resize_surface` records the new value; `end_frame` passes it to
    /// `host.record_pane` which sets viewport + scissor on the shared
    /// pass. Empty rects (`w == 0 || h == 0`) skip drawing entirely
    /// (parked-by-clip — pane dragged to zero width or off-canvas).
    viewport: ScissorRect,
    /// 16-byte uniform buffer holding `FrameUniform { viewport, _pad }`.
    /// Per-pane because the vertex shader's NDC conversion divides
    /// `cell_xy` by this `viewport` (= pane-local device-pixel size).
    /// `record_pane` then maps the resulting NDC into the pane's rect
    /// on the host canvas via `pass.set_viewport(scissor.x, scissor.y,
    /// scissor.w, scissor.h, 0, 1)`.
    frame_uniform: wgpu::Buffer,
    /// Per-cell instance buffer. Initial capacity =
    /// `INITIAL_INSTANCE_CAPACITY`; doubles on overflow inside `end_frame`.
    instance_buffer: wgpu::Buffer,
    instance_capacity: u32,
    /// Bind group instance against `ctx.cell_bind_group_layout`. Holds
    /// references to `frame_uniform` (per-pane) + `ctx.atlas_view` +
    /// `ctx.sampler` (shared). Rebuilt when `ctx.atlas_generation`
    /// advances (atlas reallocated) — see `begin_frame`.
    bind_group: wgpu::BindGroup,
    /// Per-frame CellInstance accumulator. `begin_frame` clears it,
    /// `draw_row` / `draw_cursor` / `draw_*_overlay` push, `end_frame`
    /// uploads via `queue.write_buffer` and forwards to host.
    pending_instances: Vec<CellInstance>,
    /// Per-layer pin flag, reset to all-`false` every `begin_frame`.
    /// A layer is pinned the moment any cell in this frame's
    /// `pending_instances` references it, so `ctx.rasterize_and_admit`
    /// can skip pinned layers during LRU eviction. Length tracks
    /// `ctx.atlas_layers` (re-checked defensively in `begin_frame`).
    frame_pinned: Vec<bool>,
    metrics: FrameMetrics,
    theme: Theme,
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

impl WebGpuPaneBackend {
    /// Acquire (or reuse) the shared `GpuContext` + `SurfaceHost`, then
    /// allocate this pane's per-pane buffers + bind group. Async
    /// because the first call performs the full WebGPU adapter /
    /// device bootstrap; subsequent calls return the cached `Rc`
    /// immediately.
    ///
    /// Returns `Err("SurfaceHost not initialized")` when JS hasn't
    /// called `SurfaceHostHandle::init(canvas)` yet — the caller in
    /// `lib.rs::RenderHandle::new_with_webgpu_first` falls back to
    /// `Canvas2dBackend` so the pane never crashes.
    pub async fn new() -> Result<Self, String> {
        let host = SurfaceHost::get().ok_or_else(|| {
            "WebGpuPaneBackend: SurfaceHost not initialized — call \
             SurfaceHostHandle.init(canvas) before constructing pane backends"
                .to_string()
        })?;
        let ctx = GpuContext::get_or_init().await?;
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
                instance_buffer,
                bind_group,
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
            frame_pinned,
            metrics: FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
            },
            theme: Theme::default_dark(),
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

impl RenderBackend for WebGpuPaneBackend {
    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        // Delegate to the shared rasterizer's OffscreenCanvas-backed
        // measure path. Bit-for-bit identical to Canvas2dBackend so
        // fitPane stays backend-agnostic.
        self.ctx
            .borrow()
            .rasterizer
            .measure(font_family, font_size_px)
    }

    fn requires_full_frame(&self) -> bool {
        // §4.3 Phase B: always true. The host's multi-buffered swap
        // chain (`desired_maximum_frame_latency: 2`) makes
        // `LoadOp::Load` cross-frame semantics unreliable — the
        // texture acquired this frame may hold frame N-2's content,
        // not frame N-1's. To guarantee every visible pixel is freshly
        // drawn each present, every visible row of every host-mode
        // pane re-encodes every tick. Combined with
        // `SurfaceHost::begin_frame` re-asserting its own
        // `needs_initial_clear` so the first pane's pass starts with
        // `LoadOp::Clear`, this gives a deterministic full repaint per
        // frame regardless of which pane is "dirty".
        //
        // The cost is ~80 cells × 24 rows × pane-count of cell
        // instance encoding per frame — well under 1 ms even on a
        // dozen panes. The dirty-row optimisation re-emerges at the
        // FRAME level: when no pane has new content, JS skips the
        // entire `beginFrame` / `endFrame` round trip.
        true
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

        // Compute slot dims from current metrics BEFORE taking ctx
        // borrow — `slot_dims_for` is a static helper, no ctx access.
        let (need_w, need_h) =
            GpuContext::slot_dims_for(self.metrics.cell_w, self.metrics.cell_h, self.metrics.dpr);

        let mut ctx = self.ctx.borrow_mut();

        // 1) Atlas slot growth — only ever grows. Shrinking on small
        //    metric jiggles would thrash the rasterizer's OffscreenCanvas
        //    allocation and re-rasterize every glyph.
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
            self.bind_group = ctx.build_bind_group(&self.frame_uniform);
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
        // Records intent only — actual GPU work happens in `end_frame`
        // so a single RenderPass can include both the LoadOp::Clear AND
        // the cell instance draw. We always clear with theme.bg.
    }

    fn draw_row(&mut self, row: &RowDraw<'_>, attrs_table: &AttrTable) {
        let row_idx = row.row_index;
        let cell_w = self.metrics.cell_w * self.metrics.dpr;
        let cell_h = self.metrics.cell_h * self.metrics.dpr;
        let theme = self.theme.clone();
        // Integer-align row top + bottom so adjacent rows share an exact
        // pixel boundary with no fractional gap or overlap.
        let row_top = ((row_idx as f32) * cell_h).floor();
        let row_bot = (((row_idx + 1) as f32) * cell_h).floor();
        let row_h_int = (row_bot - row_top).max(1.0);

        // Pre-compute font-key state with a short borrow — released
        // before the per-cell loop so the miss path's `borrow_mut` can
        // run without panicking.
        let (font_family_hash, font_size_q) = {
            let ctx = self.ctx.borrow();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&ctx.font_family, &mut h);
            let font_family_hash = std::hash::Hasher::finish(&h);
            let font_size_q = (ctx.font_size_px * 100.0).round() as u16;
            (font_family_hash, font_size_q)
        };
        let dpr = self.metrics.dpr;

        for (col, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                continue;
            }
            let attrs = attrs_table.get(cell.attr);
            let (_attrs, fg, bg) =
                crate::render::backend::resolve_cell_colors(cell, attrs_table, &theme);

            let cell_span = cell.width.max(1) as usize;
            let pixel_x = ((col as f32) * cell_w).floor();
            let pixel_x_right = (((col + cell_span) as f32) * cell_w).floor();
            let cell_w_px = (pixel_x_right - pixel_x).max(1.0);
            let pixel_y = row_top;

            // Style flags pack BOLD + ITALIC bits per GlyphKey docstring.
            let mut style_flags: u8 = 0;
            if attrs.flags.contains(crate::term::attrs::Flags::BOLD) {
                style_flags |= GlyphKey::STYLE_BOLD;
            }
            if attrs.flags.contains(crate::term::attrs::Flags::ITALIC) {
                style_flags |= GlyphKey::STYLE_ITALIC;
            }

            // §4.7 (2026-05-07): if the row sidecar registered a multi-
            // codepoint grapheme cluster at this column, atlas-key it by
            // a cluster hash with the high bit set so it can't collide
            // with any Unicode codepoint (max 0x10FFFF, well below the
            // tag bit). The rasterizer receives the full cluster string
            // so the browser paints ZWJ / RIS / VS clusters as a single
            // visual unit. Non-cluster cells take the existing codepoint
            // path — zero overhead for ASCII / CJK output.
            const CLUSTER_TAG: u32 = 0x8000_0000;
            let cluster_text: Option<&str> = if !row.clusters.is_empty() {
                let target = col.min(u16::MAX as usize) as u16;
                row.clusters
                    .iter()
                    .find(|c| c.col == target)
                    .map(|c| c.text.as_ref())
            } else {
                None
            };
            let glyph_id: u32 = match cluster_text {
                Some(text) => {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    std::hash::Hash::hash(text, &mut h);
                    let raw = std::hash::Hasher::finish(&h) as u32;
                    CLUSTER_TAG | (raw & !CLUSTER_TAG)
                }
                None => cell.ch as u32,
            };
            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id,
                style_flags,
            };

            // ── Single `borrow_mut` for both lookup and (on miss)
            //    admit. `GlyphAtlas::lookup` requires `&mut self` because
            //    it bumps the LRU on hit; `rasterize_and_admit` is also
            //    `&mut self`. Sequential mutations inside one borrow are
            //    safe; we just have to make sure the borrow ends BEFORE
            //    the post-loop frame_pinned write so a later iteration's
            //    borrow_mut doesn't nest.
            //    §4.7: pass cluster string when present, otherwise the
            //    single codepoint as a one-char string slice.
            let mut ch_buf = [0u8; 4];
            let glyph_text: &str = match cluster_text {
                Some(text) => text,
                None => cell.ch.encode_utf8(&mut ch_buf),
            };
            let entry: Option<GlyphEntry> = {
                let mut ctx = self.ctx.borrow_mut();
                match ctx.atlas.lookup(&key) {
                    Some(e) => Some(e),
                    None => ctx
                        .rasterize_and_admit(key, glyph_text, dpr, style_flags, &self.frame_pinned)
                        .ok(),
                }
            };

            // ── Pin the layer (hit OR fresh insert) so a subsequent
            //    miss in this same frame can't evict + overwrite it
            //    before `end_frame` submits. Critical: must run AFTER
            //    the admit above so the pin guards the just-uploaded
            //    layer too.
            if let Some(e) = entry {
                if (e.layer as usize) < self.frame_pinned.len() {
                    self.frame_pinned[e.layer as usize] = true;
                }
            }

            let (atlas_uv, atlas_layer) = match entry {
                Some(e) => (e.uv, e.layer as u32),
                None => ([0.0, 0.0, 0.0, 0.0], 0),
            };

            if cell_span >= 2 {
                // Wide cell (CJK / fullwidth): split into a background
                // instance covering the full 2-cell quad + a glyph
                // instance sized to the glyph's actual advance. Without
                // this split the shader linearly stretches a 1 em CJK
                // glyph across a ~1.2 em (2 latin advances) quad — the
                // visible "中文只有左半边" symptom from before §4.5.
                self.pending_instances.push(CellInstance {
                    cell_xy: [pixel_x, pixel_y],
                    cell_size: [cell_w_px, row_h_int],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    atlas_layer: 0,
                    fg_rgba: rgba_u8_to_f32(fg),
                    bg_rgba: rgba_u8_to_f32(bg),
                });
                if let Some(e) = entry {
                    // Color emoji glyphs (COLR / CPAL / sbix / SVG) have
                    // a natural advance ≈ 1em — narrower than the 2 latin
                    // cells the renderer reserved. Stretch their quad to
                    // the full cell pair so the emoji fills the space.
                    // CJK glyphs (e.is_color = false) keep the natural
                    // advance to avoid horizontal distortion of the
                    // character.
                    let glyph_w_px = if e.is_color {
                        cell_w_px
                    } else {
                        (e.px_w as f32).min(cell_w_px).max(1.0)
                    };
                    self.pending_instances.push(CellInstance {
                        cell_xy: [pixel_x, pixel_y],
                        cell_size: [glyph_w_px, row_h_int],
                        atlas_uv: e.uv,
                        atlas_layer: e.layer as u32,
                        fg_rgba: rgba_u8_to_f32(fg),
                        bg_rgba: [0.0, 0.0, 0.0, 0.0],
                    });
                }
            } else {
                // Narrow cell: bg + glyph collapse into a single
                // instance. The shader's `mix(bg, fg, coverage)` paints
                // the glyph over the cell bg in one pass.
                self.pending_instances.push(CellInstance {
                    cell_xy: [pixel_x, pixel_y],
                    cell_size: [cell_w_px, row_h_int],
                    atlas_uv,
                    atlas_layer,
                    fg_rgba: rgba_u8_to_f32(fg),
                    bg_rgba: rgba_u8_to_f32(bg),
                });
            }
        }
    }

    fn draw_cursor(&mut self, cursor: &CursorDraw, _attrs_table: &AttrTable) {
        // Cursor reuses the cell pipeline — geometrically just another
        // colored quad, drawn OVER the row instances pushed earlier.
        use crate::render::backend::CursorStyle;

        let cell_w = self.metrics.cell_w * self.metrics.dpr;
        let cell_h = self.metrics.cell_h * self.metrics.dpr;
        let pixel_x = ((cursor.col as f32) * cell_w).floor();
        let cursor_span = cursor.width.max(1) as usize;
        let pixel_x_right = (((cursor.col + cursor_span) as f32) * cell_w).floor();
        let cell_w_px = (pixel_x_right - pixel_x).max(1.0);
        let pixel_y = ((cursor.row as f32) * cell_h).floor();
        let pixel_y_bot = (((cursor.row + 1) as f32) * cell_h).floor();
        let cell_h_int = (pixel_y_bot - pixel_y).max(1.0);
        let bar_thickness = (2.0 * self.metrics.dpr).floor().max(1.0);

        // 1) Cursor block (colored rectangle at the appropriate
        //    style-specific size).
        let (block_x, block_y, block_w, block_h) = match cursor.style {
            CursorStyle::Block => (pixel_x, pixel_y, cell_w_px, cell_h_int),
            CursorStyle::Bar => (pixel_x, pixel_y, bar_thickness, cell_h_int),
            CursorStyle::Underline => (
                pixel_x,
                pixel_y + cell_h_int - bar_thickness,
                cell_w_px,
                bar_thickness,
            ),
        };
        let cursor_color = rgba_u8_to_f32(self.theme.cursor_color);
        self.pending_instances.push(CellInstance {
            cell_xy: [block_x, block_y],
            cell_size: [block_w, block_h],
            atlas_uv: [0.0, 0.0, 0.0, 0.0],
            atlas_layer: 0,
            fg_rgba: cursor_color,
            bg_rgba: cursor_color,
        });

        // 2) Inverted glyph (only meaningful for Block). Atlas-hit-only
        //    — we don't rasterize-on-miss here to keep per-frame work
        //    bounded. If the glyph isn't cached yet, the next draw_row
        //    tick will populate it; cursor renders as a solid block this
        //    frame, then the next frame inverts on top.
        if matches!(cursor.style, CursorStyle::Block) && cursor.ch != ' ' {
            let (font_family_hash, font_size_q) = {
                let ctx = self.ctx.borrow();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&ctx.font_family, &mut h);
                let font_family_hash = std::hash::Hasher::finish(&h);
                let font_size_q = (ctx.font_size_px * 100.0).round() as u16;
                (font_family_hash, font_size_q)
            };
            let key = GlyphKey {
                font_family_hash,
                font_size_q,
                glyph_id: cursor.ch as u32,
                style_flags: 0,
            };
            let entry: Option<GlyphEntry> = {
                let mut ctx = self.ctx.borrow_mut();
                ctx.atlas.lookup(&key)
            };
            if let Some(entry) = entry {
                let cursor_text_color = rgba_u8_to_f32(self.theme.cursor_text_color);
                self.pending_instances.push(CellInstance {
                    cell_xy: [pixel_x, pixel_y],
                    cell_size: [cell_w_px, cell_h_int],
                    atlas_uv: entry.uv,
                    atlas_layer: entry.layer as u32,
                    fg_rgba: cursor_text_color,
                    bg_rgba: cursor_color,
                });
            }
        }
    }

    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]) {
        if rects.is_empty() {
            return;
        }
        let cell_w = self.metrics.cell_w * self.metrics.dpr;
        let cell_h = self.metrics.cell_h * self.metrics.dpr;
        let sel_color = rgba_u8_to_f32(self.theme.selection_bg);
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let pixel_x = ((col_start as f32) * cell_w).floor();
            let pixel_x_right = ((col_end as f32) * cell_w).floor();
            let width = (pixel_x_right - pixel_x).max(1.0);
            let pixel_y = ((row as f32) * cell_h).floor();
            let pixel_y_bot = (((row + 1) as f32) * cell_h).floor();
            let height = (pixel_y_bot - pixel_y).max(1.0);
            self.pending_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [width, height],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: sel_color,
                bg_rgba: sel_color,
            });
        }
    }

    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        if rects.is_empty() {
            return;
        }
        let cell_w = self.metrics.cell_w * self.metrics.dpr;
        let cell_h = self.metrics.cell_h * self.metrics.dpr;
        let thickness = (2.0 * self.metrics.dpr).floor().max(1.0);
        let link_color = rgba_u8_to_f32(self.theme.hyperlink_color);
        for &(row, col_start, col_end) in rects {
            if col_end <= col_start {
                continue;
            }
            let pixel_x = ((col_start as f32) * cell_w).floor();
            let pixel_x_right = ((col_end as f32) * cell_w).floor();
            let width = (pixel_x_right - pixel_x).max(1.0);
            let pixel_y_bot = (((row + 1) as f32) * cell_h).floor();
            let pixel_y = pixel_y_bot - thickness;
            self.pending_instances.push(CellInstance {
                cell_xy: [pixel_x, pixel_y],
                cell_size: [width, thickness],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                atlas_layer: 0,
                fg_rgba: link_color,
                bg_rgba: link_color,
            });
        }
    }

    fn end_frame(&mut self) {
        // Phase B per-frame protocol. Steps:
        //   1. Upload frame uniform (pane-local viewport size in pixels).
        //   2. Grow instance buffer if the frame exceeded current capacity.
        //   3. Upload pending CellInstance bytes.
        //   4. Forward to `host.record_pane(viewport, &cell_pipeline,
        //      |pass| draw)` — host opens RenderPass on its shared
        //      encoder, sets viewport + scissor to clip the pane's draw
        //      to its rect on the host canvas, and lets the closure
        //      record `set_bind_group` / `set_vertex_buffer` / `draw`.
        //
        // No `surface.get_current_texture` / `queue.submit` /
        // `frame.present` here in Phase B — those happen once per frame
        // in `SurfaceHost::end_frame`, called by JS after iterating
        // every dirty pane.

        let n_cells = self.pending_instances.len() as u32;

        // The vertex shader divides `cell_xy` by `frame.viewport` to
        // produce NDC. With single-canvas + scissor, `cell_xy` is
        // pane-local device-pixel coords, so the uniform must hold the
        // pane's own viewport size — `host.record_pane` then maps that
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
            self.instance_buffer = new_buffer;
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

        // Empty viewport (parked-by-clip) or no draws → skip the host
        // record entirely. `host.record_pane` itself short-circuits on
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

        // Step 4: hand off to host. `&ctx.cell_pipeline` is borrowed
        // through the `Ref<GpuContext>` guard for the entire
        // `record_pane` call; the closure additionally captures
        // `&self.bind_group` and `&self.instance_buffer` (lifetimes
        // bounded by `&mut self`).
        let viewport = self.viewport;
        let bind_group = &self.bind_group;
        let instance_buffer = &self.instance_buffer;
        let ctx = self.ctx.borrow();
        self.host
            .borrow_mut()
            .record_pane(viewport, &ctx.cell_pipeline, |pass| {
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, instance_buffer.slice(..));
                pass.draw(0..4, 0..n_cells);
            });

        // Seed-equivalent flag consumed — `requires_full_frame` returns
        // false next tick so the row-hash diff in Renderer::tick can
        // skip non-dirty rows.
        self.needs_initial_clear = false;
    }
}
